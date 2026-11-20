//! Evaluation of 2D plots (`xyPlot`, `polarPlot`) with the engine's `evaluate_series`.
//!
//! [`evaluate_plot`] takes a plot model (see [`crate::regions::Plot2d`]) and the environment of
//! the worksheet at the plot's position, and returns the points of every trace. Rules:
//!
//! * The parameter of a trace is its first variable that is a range variable, or else its first
//!   variable that is not defined (x in `y(x)`). A subscripted name with a literal subscript is a
//!   variable of its own (`x_1`). The x equation is searched first, then the y equation. A trace
//!   with no variable is one point.
//! * `f_k` on an axis, with `f` a user function and `x_k` the parameter, is `f(x_k)`: Mathcad's
//!   reading of a plotted function of a subscripted range (sample 17: `c_1` is `c(x_1)`).
//! * The x axis domain is the stored one: the `startValue` and `endValue` of its `xyDomain`, evaluated
//!   in the worksheet (sample 6: displayed -5.1 to 3.4, sampled -5 to 10/3). Where those are absent
//!   or do not evaluate, it is the displayed range.
//! * `num-of-points` samples the parameter at one fewer evenly spaced values than it states, across
//!   the x axis domain (the polar angle domain for a polar plot): Prime stores 499 values for 500,
//!   in every stored trace of the samples. A trace without it takes the values of a range
//!   variable defined in the worksheet, if any, and otherwise the x axis domain.
//! * Each value is evaluated on its own, so a value where an expression is not finite (for
//!   example `1/x` at 0) is a gap. A complex value is a gap too (Prime stores NaN there, for
//!   example `sqrt(x)` for x < 0). A gap ends the current polyline, and each gap is counted.
//! * Any other error (an undefined name, a work limit, an unsupported expression) ends the trace
//!   and is recorded on it. The other traces are still evaluated.
//! * The number of points per trace is capped at [`MAX_PLOT_POINTS`]. Longer ranges are sampled
//!   evenly to that many values, and a note says so.
//! * `TraceData::samples` keeps the series as Prime stores it (before the polar conversion), so
//!   it can be compared with `<Trace2dResult>` (see `compare`).
//! * Each trace has its own work budget. Nothing here panics on odd input.
//!
//! Known limits: a parametric trace whose parameter has no range (and no `num-of-points`) is
//! sampled on the x axis domain, which is wrong when the domain is not the parameter's range.

use mathcraft_core::eval::NOT_REAL_PLOT_VALUE;
use mathcraft_core::eval::{
    evaluate, evaluate_series, Budget, Env, EvalError, Matrix, Scalar, Value,
};
use mathcraft_core::expr::{Expr, Id, IdKind};
use mathcraft_core::units::Dimension;

use crate::regions::{Axis2d, Equation, Plot2d, PlotTrace, TraceStyle};

/// Largest number of values one trace evaluates.
pub const MAX_PLOT_POINTS: usize = 10_000;
/// Values of a trace with no `num-of-points`, when no range variable is defined.
pub const DEFAULT_POINTS: usize = 500;

/// Name given to the parameter of a trace with no variable. No expression uses it.
const NO_PARAMETER: &str = "\u{0}plot";

/// The stored series of a trace: its points in sampling order (see
/// [`Trace2d::points`]), or the reason it cannot be read.
pub type StoredSeries = Result<Vec<Option<(f64, f64)>>, String>;

/// The evaluated plot: its ranges, and the points of each trace.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotData {
    /// Polar plot: the traces are Cartesian points, and both ranges are symmetric.
    pub polar: bool,
    /// `(min, max)` of the horizontal axis, in data units (SI base units, as the engine gives).
    pub x_range: (f64, f64),
    /// `(min, max)` of the vertical axis.
    pub y_range: (f64, f64),
    /// Logarithmic horizontal axis. Only when the axis asks for it and its range is positive.
    pub log_x: bool,
    /// Logarithmic vertical axis, under the same rule.
    pub log_y: bool,
    /// Whether either axis draws grid lines.
    pub grid: bool,
    /// Whether the plot has a legend element.
    pub legend: bool,
    /// The plot title, when the plot has one (see `Plot2d::title`).
    pub title: Option<String>,
    /// How the horizontal axis is drawn (crossing, tick marks, labels, caption).
    pub x_look: crate::regions::AxisLook,
    /// How the vertical axis is drawn.
    pub y_look: crate::regions::AxisLook,
    /// One entry per trace, in order.
    pub traces: Vec<TraceData>,
    /// The marker lines of the plot (see `Plot2d::markers`).
    pub markers: Vec<crate::regions::PlotMarker>,
    /// Problems that did not stop the evaluation (capped counts, fallback ranges).
    pub notes: Vec<String>,
}

impl PlotData {
    /// Total number of points over all traces, in every segment.
    pub fn point_count(&self) -> usize {
        self.traces
            .iter()
            .flat_map(|t| t.segments.iter())
            .map(Vec::len)
            .sum()
    }
}

/// The evaluated points of one trace.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceData {
    pub style: TraceStyle,
    /// The legend name of the trace, when it has one (see `PlotTrace::name`).
    pub name: Option<String>,
    /// Polylines. A gap (a value that is not finite) starts a new one. Points are `(x, y)`.
    pub segments: Vec<Vec<(f64, f64)>>,
    /// Number of values that were not finite and were dropped as gaps.
    pub gaps: usize,
    /// Why the trace is incomplete, when it is.
    pub error: Option<TraceError>,
    /// One entry per sampled parameter value, in order, as Prime stores a trace: `(x, y)` before
    /// any polar conversion (for a polar plot `(angle, radius)`), and `None` for a gap. Empty
    /// when the trace stopped on an error.
    pub samples: Vec<Option<(f64, f64)>>,
    /// Prime's stored series for this trace (see `Trace2d::points`), attached by the caller (see
    /// `evaluate::attach_stored_traces`). `None` when the trace has no stored data; `Err` when the
    /// stored data cannot be read. Gaps are `None`, so the field holds no NaN and compares equal.
    pub stored: Option<StoredSeries>,
}

/// An error that ended a trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceError {
    /// The engine's message, or the conversion reason.
    pub message: String,
    /// The region uses something the evaluator does not handle (not a failure of the engine).
    pub unsupported: bool,
}

/// Evaluates a 2D plot in the environment `env` (the worksheet's definitions at the plot).
pub fn evaluate_plot(plot: &Plot2d, env: &Env) -> PlotData {
    let mut notes = Vec::new();
    let domain_x = domain_of(&plot.x_axis, plot.polar, env, &mut notes, "x");
    let traces: Vec<TraceData> = plot
        .traces
        .iter()
        .map(|trace| trace_data(trace, env, domain_x, plot.polar, &mut notes))
        .collect();
    let x_range;
    let y_range;
    if plot.polar {
        let radius = radial_bound(&plot.y_axis, &traces);
        x_range = (-radius, radius);
        y_range = (-radius, radius);
    } else {
        x_range = range_of(&plot.x_axis, &traces, |p| p.0);
        y_range = range_of(&plot.y_axis, &traces, |p| p.1);
    }
    PlotData {
        polar: plot.polar,
        x_range,
        y_range,
        log_x: !plot.polar && plot.x_axis.log && x_range.0 > 0.0,
        log_y: !plot.polar && plot.y_axis.log && y_range.0 > 0.0,
        grid: plot.x_axis.grid_lines || plot.y_axis.grid_lines,
        legend: plot.legend,
        title: plot.title.clone(),
        x_look: plot.x_axis.look.clone(),
        y_look: plot.y_axis.look.clone(),
        traces,
        markers: plot.markers.clone(),
        notes,
    }
}

/// The sampling domain of an axis, as `(start, end)`: the domain Prime stores (`startValue` and
/// `endValue`, evaluated in the worksheet), else the displayed range. A polar angle stated in
/// degrees (end above 2 pi) is converted to radians, the unit the engine's angles use. A missing or
/// reversed range falls back to `(0, 1)` with a note.
fn domain_of(
    axis: &Axis2d,
    angle: bool,
    env: &Env,
    notes: &mut Vec<String>,
    name: &str,
) -> (f64, f64) {
    let stored = valid_range(
        domain_value(axis.domain_start.as_ref(), env).or(axis.start),
        domain_value(axis.domain_end.as_ref(), env).or(axis.end),
    );
    match stored.or_else(|| valid_range(axis.start, axis.end)) {
        Some((start, end))
            if angle && end.abs().max(start.abs()) > std::f64::consts::TAU * 1.0001 =>
        {
            let to_rad = std::f64::consts::PI / 180.0;
            (start * to_rad, end * to_rad)
        }
        Some(range) => range,
        None => {
            notes.push(format!("{name} axis has no usable range; using 0 to 1"));
            (0.0, 1.0)
        }
    }
}

/// The real value of one domain bound (see [`domain_of`]). `None` when the bound is absent, a
/// placeholder, or does not evaluate to a finite real number.
fn domain_value(bound: Option<&Equation>, env: &Env) -> Option<f64> {
    let Equation::Expr(expr) = bound? else {
        return None;
    };
    match evaluate(expr, env).ok()? {
        Value::Scalar(s) if s.value.im == 0.0 && s.value.re.is_finite() => Some(s.value.re),
        _ => None,
    }
}

/// `(start, end)` when both are finite and `start < end`.
pub(crate) fn valid_range(start: Option<f64>, end: Option<f64>) -> Option<(f64, f64)> {
    let (start, end) = (start?, end?);
    (start.is_finite() && end.is_finite() && start < end).then_some((start, end))
}

/// The displayed range of a Cartesian axis. Without a usable range, the extent of the points.
fn range_of(axis: &Axis2d, traces: &[TraceData], coord: impl Fn(&(f64, f64)) -> f64) -> (f64, f64) {
    if let Some(range) = valid_range(axis.start, axis.end) {
        return range;
    }
    let values = traces
        .iter()
        .flat_map(|t| t.segments.iter())
        .flatten()
        .map(coord);
    let (min, max) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    if min < max {
        (min, max)
    } else {
        // Nothing to draw: Prime shows an empty plot over -10 to 10.
        (-10.0, 10.0)
    }
}

/// Half-width of a polar plot: the largest radius of the radial axis, else of the points.
fn radial_bound(axis: &Axis2d, traces: &[TraceData]) -> f64 {
    let from_axis = valid_range(axis.start, axis.end)
        .map(|(a, b)| a.abs().max(b.abs()))
        .filter(|r| *r > 0.0);
    let from_points = traces
        .iter()
        .flat_map(|t| t.segments.iter())
        .flatten()
        .map(|(x, y)| x.hypot(*y))
        .fold(0.0_f64, f64::max);
    let radius = from_axis.unwrap_or(from_points);
    if radius.is_finite() && radius > 0.0 {
        radius
    } else {
        1.0
    }
}

/// The points of one trace.
fn trace_data(
    trace: &PlotTrace,
    env: &Env,
    domain: (f64, f64),
    polar: bool,
    notes: &mut Vec<String>,
) -> TraceData {
    let mut out = TraceData {
        style: trace.style.clone(),
        name: trace.name.clone(),
        segments: Vec::new(),
        gaps: 0,
        error: None,
        samples: Vec::new(),
        stored: None,
    };
    let (x, y) = match (&trace.x, &trace.y) {
        (Equation::Unsupported(reason), _) | (_, Equation::Unsupported(reason)) => {
            out.error = Some(TraceError {
                message: reason.clone(),
                unsupported: true,
            });
            return out;
        }
        (Equation::Expr(x), Equation::Expr(y)) => (x, y),
        // An empty equation is a trace with nothing to draw.
        _ => return out,
    };
    let parameter = plot_variable(x, env).or_else(|| plot_variable(y, env));
    let y = &function_at_parameter(y, parameter.as_deref(), env);
    let values = sample_values(trace, parameter.as_deref(), env, domain, notes);
    let name = parameter.as_deref().unwrap_or(NO_PARAMETER);
    let budget = Budget::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    for value in &values {
        let a = point(env, x, name, value, &budget);
        let b = point(env, y, name, value, &budget);
        match (a, b) {
            (Ok(Some(a)), Ok(Some(b))) => {
                out.samples.push(Some((a, b)));
                current.push(if polar {
                    (b * a.cos(), b * a.sin())
                } else {
                    (a, b)
                });
            }
            (Ok(None), _) | (_, Ok(None)) => {
                out.gaps += 1;
                out.samples.push(None);
                flush(&mut current, &mut out.segments);
            }
            (Err(e), _) | (_, Err(e)) => {
                out.error = Some(TraceError {
                    unsupported: matches!(e, EvalError::Unsupported(_)),
                    message: e.to_string(),
                });
                out.samples.clear();
                break;
            }
        }
    }
    flush(&mut current, &mut out.segments);
    out
}

fn flush(current: &mut Vec<(f64, f64)>, segments: &mut Vec<Vec<(f64, f64)>>) {
    if !current.is_empty() {
        segments.push(std::mem::take(current));
    }
}

/// The parameter values a trace is evaluated at.
fn sample_values(
    trace: &PlotTrace,
    parameter: Option<&str>,
    env: &Env,
    domain: (f64, f64),
    notes: &mut Vec<String>,
) -> Vec<Scalar> {
    let Some(name) = parameter else {
        return vec![Scalar::real(0.0, Dimension::dimensionless())];
    };
    if trace.num_of_points.is_none() {
        if let Some(range) = env.range_variable(name) {
            return capped(range, notes, name);
        }
    }
    let count = match trace.num_of_points {
        Some(0) | None => DEFAULT_POINTS,
        // Prime stores one value fewer than `num-of-points` (500 gives 499 values, checked on
        // every stored trace of the samples), spread evenly over the domain.
        Some(n) => (n as usize).saturating_sub(1),
    };
    let count = if count > MAX_PLOT_POINTS {
        notes.push(format!(
            "trace asks for {count} points; capped at {MAX_PLOT_POINTS}"
        ));
        MAX_PLOT_POINTS
    } else {
        count.max(2)
    };
    let (start, end) = domain;
    let step = (end - start) / (count - 1) as f64;
    (0..count)
        .map(|i| {
            let v = if i + 1 == count {
                end
            } else {
                start + step * i as f64
            };
            Scalar::real(v, Dimension::dimensionless())
        })
        .collect()
}

/// The values of a range variable, evenly thinned to [`MAX_PLOT_POINTS`] when it is longer.
fn capped(range: &Matrix, notes: &mut Vec<String>, name: &str) -> Vec<Scalar> {
    let data = range.data();
    if data.len() <= MAX_PLOT_POINTS {
        return data.to_vec();
    }
    notes.push(format!(
        "range `{name}` has {} values; thinned to {MAX_PLOT_POINTS}",
        data.len()
    ));
    (0..MAX_PLOT_POINTS)
        .map(|i| data[i * (data.len() - 1) / (MAX_PLOT_POINTS - 1)])
        .collect()
}

/// The value of `expr` at one parameter value. `Ok(None)` when the value is not finite (a gap).
fn point(
    env: &Env,
    expr: &Expr,
    name: &str,
    value: &Scalar,
    budget: &Budget,
) -> Result<Option<f64>, EvalError> {
    let values = Matrix::new(1, 1, vec![*value])?;
    match evaluate_series(env, expr, name, &values, budget) {
        Ok(pairs) => Ok(pairs.first().map(|&(_, y)| y).filter(|y| y.is_finite())),
        Err(EvalError::NotFinite | EvalError::DivisionByZero | EvalError::Overflow) => Ok(None),
        // A complex value is a gap, as NaN is in Prime's stored traces (for example `sqrt` of a
        // negative number on part of the range).
        Err(EvalError::Unsupported(message)) if message == NOT_REAL_PLOT_VALUE => Ok(None),
        Err(e) => Err(e),
    }
}

/// The parameter of an expression: its first variable that is a range variable, or else its first
/// variable that is not defined. A user function is not a parameter.
fn plot_variable(expr: &Expr, env: &Env) -> Option<String> {
    let mut names = Vec::new();
    collect_variables(expr, env, &mut names);
    names
        .iter()
        .find(|n| env.is_range(n))
        .or_else(|| names.iter().find(|n| env.variable(n).is_none()))
        .cloned()
}

/// The storage key of a variable use: its name, or `name_sub` for a literal subscript (`x_1` is
/// the variable `x_1`, as the evaluator keys it). `None` for a subscript that is not a number.
fn variable_key(id: &Id) -> Option<String> {
    match id.subscript.as_deref() {
        None => Some(id.name.clone()),
        Some(sub @ Expr::Number(_)) => Some(format!("{}_{sub}", id.name)),
        Some(_) => None,
    }
}

/// Mathcad plots `f_k` as `f(x_k)` when `f` is a user function and `x_k` is the trace's parameter
/// (sheets with `x_1 := -4 .. 4` and `c_1` on the y axis, where `c(x)` is defined). Returns the
/// expression with that call; any other expression is returned unchanged.
fn function_at_parameter(expr: &Expr, parameter: Option<&str>, env: &Env) -> Expr {
    if let (Expr::Id(id), Some(parameter)) = (expr, parameter) {
        if let (Some(sub @ Expr::Number(_)), Some(key)) =
            (id.subscript.as_deref(), variable_key(id))
        {
            let is_function_use = env.function(&id.name).is_some()
                && env.variable(&key).is_none()
                && !env.is_range(&key);
            if is_function_use && parameter.ends_with(&format!("_{sub}")) {
                return Expr::call(&id.name, vec![Expr::var(parameter)]);
            }
        }
    }
    expr.clone()
}

pub(crate) fn collect_variables(expr: &Expr, env: &Env, out: &mut Vec<String>) {
    let collect = |e: &Expr, out: &mut Vec<String>| collect_variables(e, env, out);
    match expr {
        Expr::Id(id) => {
            let is_function_use = id.subscript.is_some() && env.function(&id.name).is_some();
            if matches!(id.kind, IdKind::Variable) && !is_function_use {
                out.extend(variable_key(id));
            }
        }
        Expr::Unary { operand, .. } => collect(operand, out),
        Expr::Binary { lhs, rhs, .. } => {
            collect(lhs, out);
            collect(rhs, out);
        }
        Expr::Call { args, .. } => args.iter().for_each(|a| collect(a, out)),
        Expr::Range { from, step, to } => {
            collect(from, out);
            if let Some(step) = step {
                collect(step, out);
            }
            collect(to, out);
        }
        Expr::Matrix { elements, .. } => elements.iter().for_each(|e| collect(e, out)),
        Expr::Index { base, .. } => collect(base, out),
        Expr::NthRoot { degree, radicand } => {
            if let Some(degree) = degree {
                collect(degree, out);
            }
            collect(radicand, out);
        }
        Expr::Parens(inner) => collect(inner, out),
        Expr::Scale { value, .. } => collect(value, out),
        Expr::Eval { expr, .. } => collect(expr, out),
        _ => {}
    }
}

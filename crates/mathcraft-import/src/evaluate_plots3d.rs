//! Evaluation of 3D plots (`plot3D`) with the engine's `evaluate_series` and `evaluate_surface`.
//!
//! [`evaluate_plot3d`] takes a 3D plot model (see [`crate::regions::Plot3d`]) and the worksheet's
//! environment at the plot, and returns the coordinates of every trace as a grid (a curve is a
//! grid of one column). Rules, as far as the samples show them:
//!
//! * Trace `i` uses plot equation `i`. Its free variables (names that are range variables, or else
//!   undefined) are taken in order of first appearance. Zero variables: a matrix surface, `z =
//!   M[r, c]`, with `M` the value of the expression (a matrix). One variable: a curve over it. Two
//!   variables: a surface, with the first variable along the columns and the second along the rows
//!   (sample 13: `f(x, y)` over ranges `x` and `y` gives `Z[r][c] = f(x_c, y_r)`).
//! * A variable with a range variable of the same name takes the range's values (the stored data
//!   uses them, whatever `num-of-points` says). Otherwise it is sampled evenly over its axis domain
//!   (`xAxis` for the first variable, `yAxis` for the second) with `num-of-points - 1` values.
//! * A curve is `(v, 0, f(v))` when its plot equation has one slot (sample 13: `x` gives x, 0, x).
//!   Its second slot, when it is an expression, is the y coordinate.
//! * Parametric (inferred, not in any sample): a plot equation with three slots `x(u, v)`,
//!   `y(u, v)`, `z(u, v)` where slots 2 and 3 are expressions. Over one variable it is a space
//!   curve.
//! * A matrix surface: `X` runs along the columns and `Y` along the rows, evenly over the x and y
//!   axis domains, and `Z` is the matrix. The matrix is either an expression with no free variable
//!   that evaluates to a matrix, or a bare user function `A` with two parameters that are range
//!   variables (sample 14: `A(i, j)` over the ranges `i` and `j`, a 41 x 41 matrix).
//! * A value that is not finite or is complex is a gap: its coordinates are `NaN` and it is
//!   counted. Gaps are never dropped silently. Any other error ends the trace and is recorded.
//! * Grids are capped: each side has at most [`MAX_SURFACE_SIDE`] values (longer ranges are thinned
//!   evenly, with a note). Every evaluation is charged to the trace's own work budget, and the
//!   engine refuses products above its own limit.
//! * Nothing here panics on odd input.
//!
//! Limits: a surface with a grid side of one value has no spacing for its axis; the axis domain
//! is then its start. Matrix surfaces use the axis domains as stated, which may differ from the
//! data range Prime stores (sample 14: the stored X runs from -10 to 45 for an axis stated as -15
//! to 45, see the golden report).

use mathcraft_core::eval::{
    evaluate, evaluate_series, evaluate_surface, Budget, Env, EvalError, Matrix, Scalar, Value,
    NOT_REAL_PLOT_VALUE,
};
use mathcraft_core::expr::Expr;
use mathcraft_core::units::Dimension;

use crate::evaluate_plots::{
    collect_variables, valid_range, TraceError, DEFAULT_POINTS, MAX_PLOT_POINTS,
};
use crate::regions::{Axis3d, Equation, Plot3d, Trace3d, TraceStyle};
use crate::results::Trace3dStored;

/// Most values one side of a surface grid, or one curve, may have. A longer range is thinned.
pub const MAX_SURFACE_SIDE: usize = 200;
/// Values per side of a surface whose trace has no `num-of-points` and no range variable.
pub const DEFAULT_SURFACE_SIDE: usize = 40;

/// The evaluated 3D plot: its ranges and its traces.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot3dData {
    /// `(min, max)` of the x, y and z axes, in SI base units: the axis as stated, else the data.
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    pub z_range: (f64, f64),
    /// Whether any axis draws grid lines.
    pub grid: bool,
    /// Whether the plot has a legend element.
    pub legend: bool,
    /// One entry per trace, in order.
    pub traces: Vec<Trace3dData>,
    /// Problems that did not stop the evaluation (thinned grids, fallback domains).
    pub notes: Vec<String>,
}

/// The evaluated coordinates of one trace. A curve has `cols == 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace3dData {
    pub style: TraceStyle,
    /// `true` for a surface (a grid of two variables or a matrix), `false` for a curve.
    pub surface: bool,
    pub rows: usize,
    pub cols: usize,
    /// Coordinates, row-major with `rows * cols` values each. `NaN` is a gap.
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// Number of points that were gaps (a value that was not finite or was complex).
    pub gaps: usize,
    /// Why the trace is incomplete, when it is. The coordinates are empty then.
    pub error: Option<TraceError>,
    /// Prime's stored data for this trace (see `results::trace3d_of`), attached by the caller.
    /// `None` when the trace has no stored data; `Err` when it cannot be read.
    pub stored: Option<Result<Trace3dStored, String>>,
}

/// Evaluates a 3D plot in the environment `env` (the worksheet's definitions at the plot).
pub fn evaluate_plot3d(plot: &Plot3d, env: &Env) -> Plot3dData {
    let mut notes = Vec::new();
    let domain_x = axis_domain(&plot.x_axis, "x", &mut notes);
    let domain_y = axis_domain(&plot.y_axis, "y", &mut notes);
    let traces: Vec<Trace3dData> = plot
        .traces
        .iter()
        .map(|trace| trace_data(trace, env, [domain_x, domain_y], &mut notes))
        .collect();
    let all = |pick: fn(&Trace3dData) -> &Vec<f64>| -> Vec<f64> {
        traces
            .iter()
            .flat_map(|t| pick(t).iter().copied())
            .filter(|v| v.is_finite())
            .collect()
    };
    let x_values = all(|t| &t.x);
    let y_values = all(|t| &t.y);
    let z_values = all(|t| &t.z);
    Plot3dData {
        x_range: range_or_extent(&plot.x_axis, &x_values),
        y_range: range_or_extent(&plot.y_axis, &y_values),
        z_range: range_or_extent(&plot.z_axis, &z_values),
        grid: plot.x_axis.grid_lines || plot.y_axis.grid_lines || plot.z_axis.grid_lines,
        legend: plot.legend,
        traces,
        notes,
    }
}

/// The sampling domain of a horizontal axis. A missing or reversed range falls back to `(0, 1)`
/// with a note.
fn axis_domain(axis: &Axis3d, name: &str, notes: &mut Vec<String>) -> (f64, f64) {
    match valid_range(axis.start, axis.end) {
        Some(range) => range,
        None => {
            notes.push(format!("{name} axis has no usable range; using 0 to 1"));
            (0.0, 1.0)
        }
    }
}

/// The axis as stated when it has a usable range, else the extent of the values (or `(0, 1)`).
fn range_or_extent(axis: &Axis3d, values: &[f64]) -> (f64, f64) {
    if let Some(range) = valid_range(axis.start, axis.end) {
        return range;
    }
    let (min, max) = values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    if min < max {
        (min, max)
    } else {
        (0.0, 1.0)
    }
}

/// The coordinates of one trace.
fn trace_data(
    trace: &Trace3d,
    env: &Env,
    domains: [(f64, f64); 2],
    notes: &mut Vec<String>,
) -> Trace3dData {
    let out = Trace3dData {
        style: trace.style.clone(),
        surface: false,
        rows: 0,
        cols: 0,
        x: Vec::new(),
        y: Vec::new(),
        z: Vec::new(),
        gaps: 0,
        error: None,
        stored: None,
    };
    let slots = &trace.equation;
    let slot = |i: usize| slots.get(i).unwrap_or(&Equation::Empty);
    if let Equation::Unsupported(reason) = slot(0) {
        return failed(out, reason.clone(), true);
    }
    let height = match slot(0) {
        Equation::Expr(expr) => expr.clone(),
        // An empty first slot is a trace with nothing to draw.
        _ => return out,
    };
    // Three expression slots make a parametric plot: `x`, `y` and `z` in slots 0, 1 and 2.
    let exprs: Vec<Expr> = match (slot(1), slot(2)) {
        (Equation::Expr(y), Equation::Expr(z)) => vec![height, y.clone(), z.clone()],
        _ => vec![height],
    };

    let budget = Budget::new();
    // A bare user function `A` with two parameters that are range variables is a matrix (sample
    // 14: `A(i, j)` over the ranges `i` and `j`). It is drawn as a matrix surface.
    if let (Expr::Id(id), 1) = (&exprs[0], exprs.len()) {
        if id.subscript.is_none() && env.variable(&id.name).is_none() {
            if let Some(function) = env.function(&id.name) {
                let result = match function.params.as_slice() {
                    [p, q] if env.is_range(p) && env.is_range(q) => {
                        let call = Expr::call(&id.name, vec![Expr::var(p), Expr::var(q)]);
                        surface(trace, env, &[call], p, q, domains, true, &budget, notes)
                    }
                    _ => Err(TraceFailure::unsupported(format!(
                        "`{}` is not a matrix over two range variables",
                        id.name
                    ))),
                };
                return finish(out, result);
            }
        }
    }
    let mut vars = Vec::new();
    for expr in &exprs {
        collect_variables(expr, env, &mut vars);
    }
    let vars = free_variables(vars, env);
    let result = match vars.as_slice() {
        [] => matrix_surface(&exprs[0], env, domains, notes),
        [u] => curve(trace, env, &exprs, slot(1), u, domains[0], &budget, notes),
        [u, v] => surface(trace, env, &exprs, u, v, domains, false, &budget, notes),
        _ => Err(TraceFailure::unsupported(
            "a 3D plot takes at most two variables".to_string(),
        )),
    };
    finish(out, result)
}

/// Stores a computed grid in the trace, or the failure that ended it.
fn finish(mut out: Trace3dData, result: Result<Grid, TraceFailure>) -> Trace3dData {
    match result {
        Ok(grid) => {
            out.surface = grid.surface;
            out.rows = grid.rows;
            out.cols = grid.cols;
            out.gaps = (0..grid.z.len())
                .filter(|&i| grid.x[i].is_nan() || grid.y[i].is_nan() || grid.z[i].is_nan())
                .count();
            out.x = grid.x;
            out.y = grid.y;
            out.z = grid.z;
            out
        }
        Err(failure) => failed(out, failure.message, failure.unsupported),
    }
}

/// Marks a trace as failed, keeping no coordinates.
fn failed(mut out: Trace3dData, message: String, unsupported: bool) -> Trace3dData {
    out.error = Some(TraceError {
        message,
        unsupported,
    });
    out
}

/// The free variables of a plot equation: range variables first (in order of appearance), then
/// the undefined names (in order). A defined variable (a matrix or a constant) is not one.
fn free_variables(names: Vec<String>, env: &Env) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in &names {
        if env.is_range(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    for name in &names {
        if env.variable(name).is_none() && !env.is_range(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

/// A computed grid: `rows x cols` coordinates.
struct Grid {
    surface: bool,
    rows: usize,
    cols: usize,
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
}

/// A failure that ends one trace.
struct TraceFailure {
    message: String,
    unsupported: bool,
}

impl TraceFailure {
    fn unsupported(message: String) -> Self {
        Self {
            message,
            unsupported: true,
        }
    }
}

fn engine(error: EvalError) -> TraceFailure {
    TraceFailure {
        message: error.to_string(),
        unsupported: matches!(error, EvalError::Unsupported(_)),
    }
}

/// A matrix surface: the value of the first expression is a matrix `Z`, drawn with `X` along the
/// columns and `Y` along the rows over the axis domains.
fn matrix_surface(
    expr: &Expr,
    env: &Env,
    domains: [(f64, f64); 2],
    notes: &mut Vec<String>,
) -> Result<Grid, TraceFailure> {
    let value = evaluate(expr, env).map_err(engine)?;
    let Value::Matrix(matrix) = value else {
        return Err(TraceFailure::unsupported(
            "a 3D plot needs a matrix or a function of x and y".to_string(),
        ));
    };
    let (rows, cols) = matrix.shape();
    if rows < 2 || cols < 2 {
        return Err(TraceFailure::unsupported(
            "a matrix surface needs at least 2 rows and 2 columns".to_string(),
        ));
    }
    let row_idx = thin_indices(rows, MAX_SURFACE_SIDE, notes, "matrix rows");
    let col_idx = thin_indices(cols, MAX_SURFACE_SIDE, notes, "matrix columns");
    let (nr, nc) = (row_idx.len(), col_idx.len());
    let xs = linspace(domains[0], nc);
    let ys = linspace(domains[1], nr);
    let mut grid = Grid {
        surface: true,
        rows: nr,
        cols: nc,
        x: Vec::with_capacity(nr * nc),
        y: Vec::with_capacity(nr * nc),
        z: Vec::with_capacity(nr * nc),
    };
    for (ri, &r) in row_idx.iter().enumerate() {
        for (ci, &c) in col_idx.iter().enumerate() {
            grid.x.push(xs[ci]);
            grid.y.push(ys[ri]);
            grid.z.push(scalar_value(matrix.get(r, c).map_err(engine)?));
        }
    }
    Ok(grid)
}

/// The real part of a matrix element, or `NaN` for a complex one.
fn scalar_value(s: &Scalar) -> f64 {
    if s.value.im != 0.0 {
        f64::NAN
    } else {
        s.value.re
    }
}

/// A curve over one variable `u`: `(u, slot 1 or 0, slot 0)`, or the parametric `(x, y, z)` of
/// the three expressions over `u`.
#[allow(clippy::too_many_arguments)]
fn curve(
    trace: &Trace3d,
    env: &Env,
    exprs: &[Expr],
    second: &Equation,
    u: &str,
    domain: (f64, f64),
    budget: &Budget,
    notes: &mut Vec<String>,
) -> Result<Grid, TraceFailure> {
    let values = sample_axis(
        trace,
        env,
        u,
        domain,
        DEFAULT_POINTS,
        MAX_PLOT_POINTS,
        notes,
    );
    let n = values.len();
    let line = |expr: &Expr| line_values(env, expr, u, &values, budget).map_err(engine);
    let (x, y, z) = if exprs.len() == 3 {
        (line(&exprs[0])?, line(&exprs[1])?, line(&exprs[2])?)
    } else {
        let y = match second {
            Equation::Expr(e) => line(e)?,
            _ => vec![0.0; n],
        };
        (values.clone(), y, line(&exprs[0])?)
    };
    Ok(Grid {
        surface: false,
        rows: n,
        cols: 1,
        x,
        y,
        z,
    })
}

/// A surface over `u` (columns) and `v` (rows). Height mode: `Z = expr(u, v)`, `X` and `Y` the grid
/// coordinates. Parametric mode: the three expressions give the coordinates.
#[allow(clippy::too_many_arguments)]
fn surface(
    trace: &Trace3d,
    env: &Env,
    exprs: &[Expr],
    u: &str,
    v: &str,
    domains: [(f64, f64); 2],
    matrix_style: bool,
    budget: &Budget,
    notes: &mut Vec<String>,
) -> Result<Grid, TraceFailure> {
    let us = sample_axis(
        trace,
        env,
        u,
        domains[0],
        DEFAULT_SURFACE_SIDE,
        MAX_SURFACE_SIDE,
        notes,
    );
    let vs = sample_axis(
        trace,
        env,
        v,
        domains[1],
        DEFAULT_SURFACE_SIDE,
        MAX_SURFACE_SIDE,
        notes,
    );
    let (nr, nc) = (vs.len(), us.len());
    let grid_of = |expr: &Expr| grid_values(env, expr, (v, &vs), (u, &us), budget).map_err(engine);
    // X runs along the columns and Y along the rows. For an expression they are the variables'
    // values. For a matrix (`matrix_style`) they are evenly spaced over the axis domains, as the
    // matrix surface of sample 14 stores them.
    let (x, y) = if matrix_style {
        let xs = linspace(domains[0], nc);
        let ys = linspace(domains[1], nr);
        (
            ys.iter()
                .flat_map(|_| xs.iter().copied())
                .collect::<Vec<f64>>(),
            ys.iter()
                .flat_map(|&yy| std::iter::repeat_n(yy, nc))
                .collect::<Vec<f64>>(),
        )
    } else {
        (
            vs.iter()
                .flat_map(|_| us.iter().copied())
                .collect::<Vec<f64>>(),
            vs.iter()
                .flat_map(|&vv| std::iter::repeat_n(vv, nc))
                .collect::<Vec<f64>>(),
        )
    };
    let (x, y, z) = if exprs.len() == 3 {
        (
            grid_of(&exprs[0])?,
            grid_of(&exprs[1])?,
            grid_of(&exprs[2])?,
        )
    } else {
        (x, y, grid_of(&exprs[0])?)
    };
    Ok(Grid {
        surface: true,
        rows: nr,
        cols: nc,
        x,
        y,
        z,
    })
}

/// The values of a variable: its range variable (thinned to `cap`), else `count - 1` evenly spaced
/// values over the domain (`num-of-points`, or `default` when the trace has none).
fn sample_axis(
    trace: &Trace3d,
    env: &Env,
    name: &str,
    domain: (f64, f64),
    default: usize,
    cap: usize,
    notes: &mut Vec<String>,
) -> Vec<f64> {
    if let Some(range) = env.range_variable(name) {
        let data: Vec<f64> = range.data().iter().map(scalar_value).collect();
        let idx = thin_indices(data.len(), cap, notes, name);
        return idx.iter().map(|&i| data[i]).collect();
    }
    let count = match trace.num_of_points {
        Some(0) | None => default,
        // As in the 2D plots: one value fewer than `num-of-points`.
        Some(n) => (n as usize).saturating_sub(1),
    }
    .max(2);
    let count = if count > cap {
        notes.push(format!("`{name}` asks for {count} values; capped at {cap}"));
        cap
    } else {
        count
    };
    linspace(domain, count)
}

/// `count` evenly spaced values from `start` to `end`, the last exactly `end`. One value gives
/// `start`.
fn linspace((start, end): (f64, f64), count: usize) -> Vec<f64> {
    match count {
        0 => Vec::new(),
        1 => vec![start],
        n => (0..n)
            .map(|i| {
                if i + 1 == n {
                    end
                } else {
                    start + (end - start) * i as f64 / (n - 1) as f64
                }
            })
            .collect(),
    }
}

/// Indices of `len` values, thinned evenly to at most `cap`. Notes the thinning.
fn thin_indices(len: usize, cap: usize, notes: &mut Vec<String>, name: &str) -> Vec<usize> {
    if len <= cap {
        return (0..len).collect();
    }
    notes.push(format!("`{name}` has {len} values; thinned to {cap}"));
    (0..cap).map(|i| i * (len - 1) / (cap - 1)).collect()
}

/// Whether an engine error is a value that cannot be drawn (a gap), not a failure.
fn is_gap(error: &EvalError) -> bool {
    match error {
        EvalError::NotFinite | EvalError::DivisionByZero | EvalError::Overflow => true,
        EvalError::Unsupported(message) => message == NOT_REAL_PLOT_VALUE,
        _ => false,
    }
}

/// `expr` at each value of a curve's variable, as real numbers (`NaN` for a gap). The whole run is
/// tried first, then each value alone when one of them is a gap.
fn line_values(
    env: &Env,
    expr: &Expr,
    name: &str,
    values: &[f64],
    budget: &Budget,
) -> Result<Vec<f64>, EvalError> {
    let column = |vs: &[f64]| {
        Matrix::new(
            vs.len(),
            1,
            vs.iter()
                .map(|&v| Scalar::real(v, Dimension::dimensionless()))
                .collect(),
        )
    };
    match evaluate_series(env, expr, name, &column(values)?, budget) {
        Ok(pairs) => Ok(pairs.into_iter().map(|(_, y)| y).collect()),
        Err(e) if !is_gap(&e) => Err(e),
        Err(_) => values
            .iter()
            .map(
                |&v| match evaluate_series(env, expr, name, &column(&[v])?, budget) {
                    Ok(pairs) => Ok(pairs.first().map_or(f64::NAN, |p| p.1)),
                    Err(e) if is_gap(&e) => Ok(f64::NAN),
                    Err(e) => Err(e),
                },
            )
            .collect(),
    }
}

/// `expr` on the grid of two variables, as row-major values (`NaN` for a gap). Rows follow
/// `rows`, columns follow `cols` (see `evaluate_surface`). The whole grid is tried first, then
/// each row, then each cell.
fn grid_values(
    env: &Env,
    expr: &Expr,
    rows: (&str, &[f64]),
    cols: (&str, &[f64]),
    budget: &Budget,
) -> Result<Vec<f64>, EvalError> {
    let column = |vs: &[f64]| {
        Matrix::new(
            vs.len(),
            1,
            vs.iter()
                .map(|&v| Scalar::real(v, Dimension::dimensionless()))
                .collect(),
        )
    };
    let row_m = column(rows.1)?;
    let col_m = column(cols.1)?;
    match evaluate_surface(env, expr, (rows.0, &row_m), (cols.0, &col_m), budget) {
        Ok(grid) => Ok(grid.into_iter().flatten().collect()),
        Err(e) if !is_gap(&e) => Err(e),
        Err(_) => {
            let mut out = Vec::with_capacity(rows.1.len() * cols.1.len());
            for &r in rows.1 {
                let one_row = column(&[r])?;
                match evaluate_surface(env, expr, (rows.0, &one_row), (cols.0, &col_m), budget) {
                    Ok(grid) => out.extend(grid.into_iter().flatten()),
                    Err(e) if !is_gap(&e) => return Err(e),
                    Err(_) => {
                        for &c in cols.1 {
                            let one = column(&[c])?;
                            match evaluate_surface(
                                env,
                                expr,
                                (rows.0, &one_row),
                                (cols.0, &one),
                                budget,
                            ) {
                                Ok(cell) => out.push(cell[0][0]),
                                Err(e) if is_gap(&e) => out.push(f64::NAN),
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
            }
            Ok(out)
        }
    }
}

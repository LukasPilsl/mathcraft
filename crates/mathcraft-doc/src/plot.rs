//! New and edited 2D plots: the plot model of an empty x-y or polar plot, and the text form the
//! plot dialog edits ([`PlotSpec`]). No egui types here.
//!
//! What Prime does (help `about_traces_and_axis_expressions.md`, screenshots 0053 and 0226): an
//! empty plot has one trace with a placeholder on each axis, and the axes show -10 to 10. Each
//! further expression on an axis adds a trace; the axis expressions are paired in order, and the
//! last expression of the shorter axis serves the rest. A new trace takes the next colour of
//! Prime's palette (dark blue, black, red, yellow, orange, green, blue; the order the samples
//! store), a thin solid line, no symbol.

use mathcraft_core::expr::Expr;
use mathcraft_import::regions::{
    Axis2d, Equation, Plot2d, PlotInfo, PlotMarker, PlotTrace, RegionKind, RegionModel, TraceStyle,
};

use crate::context::{parse_math_in, EditContext};
use crate::locale::{decimal_comma, message};

/// Size of a new plot region, in page points. Not from a sample (Prime's own default is not
/// stored anywhere): about the size of the 4-trace plot of `3_rata_3_Kurve_durch_3_Punkte`.
pub const PLOT_DEFAULT_SIZE: (f64, f64) = (345.0, 330.0);

/// Prime's trace colours in order, as `[r, g, b, a]` (the samples' `color` attributes).
pub const PALETTE: [[u8; 4]; 7] = [
    [0x00, 0x00, 0x8B, 0xFF],
    [0x00, 0x00, 0x00, 0xFF],
    [0xFF, 0x00, 0x00, 0xFF],
    [0xFF, 0xFF, 0x00, 0xFF],
    [0xFF, 0xA5, 0x00, 0xFF],
    [0x00, 0x80, 0x00, 0xFF],
    [0x00, 0x00, 0xFF, 0xFF],
];

/// The default range of the parameter of a plot: what Prime shows for an empty plot.
const DEFAULT_DOMAIN: (f64, f64) = (-10.0, 10.0);

/// The plot dialog's text form. Expressions are lists separated by commas (semicolons when the
/// decimal comma is on), one entry per trace. A limit that is empty means automatic.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlotSpec {
    /// The expressions of the x axis (the angle for a polar plot).
    pub x: String,
    /// The expressions of the y axis (the radius for a polar plot).
    pub y: String,
    /// Displayed start of the x axis.
    pub x_min: String,
    /// Displayed end of the x axis.
    pub x_max: String,
    /// Displayed start of the y axis.
    pub y_min: String,
    /// Displayed end of the y axis.
    pub y_max: String,
    /// Grid lines on both axes.
    pub grid: bool,
    /// The plot title.
    pub title: String,
    /// True for a polar plot (not editable: it only says which labels to show).
    pub polar: bool,
}

/// An empty plot: one trace, placeholders on both axes.
pub fn new_plot(polar: bool) -> PlotInfo {
    let spec = PlotSpec {
        polar,
        ..PlotSpec::default()
    };
    // An empty spec always builds.
    build(&spec, &EditContext::default()).expect("an empty plot spec builds")
}

/// The region kind of a new plot, plus the page size it is shown at.
pub fn new_plot_kind(polar: bool) -> (RegionKind, (f64, f64)) {
    (RegionKind::Plot(new_plot(polar)), PLOT_DEFAULT_SIZE)
}

/// The spec of an existing 2D plot, or `None` for a plot without a 2D model.
pub fn spec_of(region: &RegionModel) -> Option<PlotSpec> {
    let RegionKind::Plot(info) = &region.kind else {
        return None;
    };
    let plot = info.plot.as_deref()?;
    Some(PlotSpec {
        x: equations_text(&plot.x_axis),
        y: equations_text(&plot.y_axis),
        x_min: number_text(plot.x_axis.start),
        x_max: number_text(plot.x_axis.end),
        y_min: number_text(plot.y_axis.start),
        y_max: number_text(plot.y_axis.end),
        grid: plot.x_axis.grid_lines || plot.y_axis.grid_lines,
        title: plot.title.clone().unwrap_or_default(),
        polar: plot.polar,
    })
}

/// The separator between the expressions of an axis.
fn separator() -> char {
    if decimal_comma() {
        ';'
    } else {
        ','
    }
}

fn equations_text(axis: &Axis2d) -> String {
    let parts: Vec<String> = axis
        .equations
        .iter()
        .map(|e| match e {
            Equation::Expr(expr) => expr.to_string(),
            Equation::Empty | Equation::Unsupported(_) => String::new(),
        })
        .collect();
    if parts.iter().all(String::is_empty) {
        return String::new();
    }
    parts.join(&format!("{} ", separator()))
}

fn number_text(value: Option<f64>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{v}"),
        _ => String::new(),
    }
}

/// Splits `text` at the separators outside brackets and quotes. A semicolon always separates;
/// a comma separates unless the decimal comma is on.
pub fn split_expressions(text: &str) -> Vec<String> {
    let comma = !decimal_comma();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            '(' | '[' | '{' if !quoted => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' | '}' if !quoted => {
                depth -= 1;
                current.push(c);
            }
            ';' if !quoted && depth <= 0 => parts.push(std::mem::take(&mut current)),
            ',' if comma && !quoted && depth <= 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts.iter().map(|p| p.trim().to_owned()).collect()
}

fn parse_equations(text: &str, ctx: &EditContext) -> Result<Vec<Equation>, String> {
    if text.trim().is_empty() {
        return Ok(vec![Equation::Empty]);
    }
    split_expressions(text)
        .into_iter()
        .map(|part| {
            if part.is_empty() {
                return Ok(Equation::Empty);
            }
            parse_math_in(&part, ctx)
                .map(Equation::Expr)
                .map_err(|e| message("edit.cannot_read", &[&format!("{part}: {e}")]))
        })
        .collect()
}

fn parse_limit(text: &str, what: &str) -> Result<Option<f64>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let normal = text.replace(',', ".");
    normal
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(Some)
        .ok_or_else(|| message("edit.plot_bad_limit", &[what, text]))
}

fn bound(value: f64) -> Equation {
    // The same tree a typed number gives, so the written plot reads back equal.
    match parse_math_in(&format!("{value}"), &EditContext::default()) {
        Ok(expr) => Equation::Expr(expr),
        Err(_) => Equation::Expr(Expr::Number(mathcraft_core::expr::Number::Real {
            value,
            text: format!("{value}"),
        })),
    }
}

fn style_for(index: usize) -> TraceStyle {
    TraceStyle {
        color: Some(PALETTE[index % PALETTE.len()]),
        line_weight: Some(1.0),
        line_style: Some("Solid".to_owned()),
        symbol: Some("none".to_owned()),
        mode: Some("lines".to_owned()),
        automatic: false,
    }
}

/// Builds the plot model of `spec`. The traces are paired as Prime pairs them: trace `i` takes
/// expression `i` of each axis, and the last expression of the shorter axis serves the rest.
///
/// # Errors
/// A message for an expression that does not parse, or a limit that is not a number.
pub fn build(spec: &PlotSpec, ctx: &EditContext) -> Result<PlotInfo, String> {
    let xs = parse_equations(&spec.x, ctx)?;
    let ys = parse_equations(&spec.y, ctx)?;
    let x_start = parse_limit(&spec.x_min, &message("edit.plot_x_min", &[]))?;
    let x_end = parse_limit(&spec.x_max, &message("edit.plot_x_max", &[]))?;
    let y_start = parse_limit(&spec.y_min, &message("edit.plot_y_min", &[]))?;
    let y_end = parse_limit(&spec.y_max, &message("edit.plot_y_max", &[]))?;
    for (a, b, what) in [
        (x_start, x_end, message("edit.plot_x_min", &[])),
        (y_start, y_end, message("edit.plot_y_min", &[])),
    ] {
        if let (Some(a), Some(b)) = (a, b) {
            if a >= b {
                return Err(message(
                    "edit.plot_bad_limit",
                    &[&what, &format!("{a} >= {b}")],
                ));
            }
        }
    }
    let count = xs.len().max(ys.len()).max(1);
    let at = |list: &[Equation], i: usize| {
        list.get(i)
            .or_else(|| list.last())
            .cloned()
            .unwrap_or(Equation::Empty)
    };
    let traces: Vec<PlotTrace> = (0..count)
        .map(|i| PlotTrace {
            result_ref: None,
            num_of_points: None,
            style: style_for(i),
            name: None,
            x: at(&xs, i),
            y: at(&ys, i),
        })
        .collect();
    // An angle runs over a turn by default, a Cartesian parameter over -10 to 10.
    let (d_start, d_end) = if spec.polar {
        (0.0, 360.0)
    } else {
        DEFAULT_DOMAIN
    };
    let x_axis = Axis2d {
        start: x_start,
        end: x_end,
        domain_start: Some(x_start.map_or_else(|| bound(d_start), bound)),
        domain_end: Some(x_end.map_or_else(|| bound(d_end), bound)),
        log: false,
        grid_lines: spec.grid,
        grid_frequency: Some(11),
        equations: xs,
        look: Default::default(),
    };
    let y_axis = Axis2d {
        start: y_start,
        end: y_end,
        domain_start: None,
        domain_end: None,
        log: false,
        grid_lines: spec.grid,
        grid_frequency: Some(11),
        equations: ys,
        look: Default::default(),
    };
    let title = Some(spec.title.trim().to_owned()).filter(|t| !t.is_empty());
    let x_range = x_start.zip(x_end).filter(|_| !spec.polar);
    let y_range = y_start.zip(y_end).filter(|_| !spec.polar);
    Ok(PlotInfo {
        kind: if spec.polar { "polarPlot" } else { "xyPlot" }.to_owned(),
        traces: Vec::new(),
        x_range,
        y_range,
        plot: Some(Box::new(Plot2d {
            polar: spec.polar,
            legend: true,
            title,
            x_axis,
            y_axis,
            traces,
            markers: Vec::new(),
        })),
        plot3d: None,
    })
}

/// A change to the selected plot, from the Diagramme tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotOp {
    /// A new trace with a placeholder on each axis.
    AddTrace,
    /// Remove the last trace (a plot keeps at least one).
    RemoveTrace,
    /// Turn an x-y plot into a polar one (`true`) or the reverse.
    SetPolar(bool),
    /// A vertical (`true`) or horizontal marker at the end of its axis.
    AddMarker(bool),
    /// Remove the last marker.
    DeleteMarker,
    /// Logarithmic scale of the x axis (`false`) or the y axis (`true`).
    ToggleLog(bool),
    /// Grid lines on both axes.
    ToggleGrid,
    /// A style change of the last trace.
    Style(StyleChange),
}

/// One property of a trace style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleChange {
    /// The symbol name (`none`, `circle`, ...); see [`SYMBOLS`].
    Symbol(&'static str),
    /// The line style (`Solid`, `Dash`, ...); see [`LINE_STYLES`].
    Line(&'static str),
    /// An index into [`PALETTE`].
    Color(usize),
    /// A line thickness in points; see [`WEIGHTS`].
    Weight(u8),
}

/// The symbols of the Symbol menu (`none` is the only name seen in a sample).
pub const SYMBOLS: [&str; 6] = ["none", "circle", "square", "diamond", "triangle", "cross"];
/// The line styles of the Linienstil menu (`Solid` is the only name seen in a sample).
pub const LINE_STYLES: [&str; 3] = ["Solid", "Dash", "Dot"];
/// The line thicknesses of the Spurdicke menu (the samples use 1 and 6).
pub const WEIGHTS: [u8; 4] = [1, 2, 4, 6];

/// True when `plot` has a marker to delete.
pub fn has_marker(plot: &Plot2d) -> bool {
    !plot.markers.is_empty()
}

/// Applies `op` to `plot`. Returns false when nothing changed (the last trace cannot be removed,
/// there is no marker to delete).
pub fn apply(plot: &mut Plot2d, op: PlotOp) -> bool {
    match op {
        PlotOp::AddTrace => {
            let i = plot.traces.len();
            plot.x_axis.equations.resize(i, Equation::Empty);
            plot.y_axis.equations.resize(i, Equation::Empty);
            plot.x_axis.equations.push(Equation::Empty);
            plot.y_axis.equations.push(Equation::Empty);
            plot.traces.push(PlotTrace {
                result_ref: None,
                num_of_points: None,
                style: style_for(i),
                name: None,
                x: Equation::Empty,
                y: Equation::Empty,
            });
            true
        }
        PlotOp::RemoveTrace => {
            if plot.traces.len() <= 1 {
                return false;
            }
            plot.traces.pop();
            let n = plot.traces.len();
            plot.x_axis.equations.truncate(n);
            plot.y_axis.equations.truncate(n);
            true
        }
        PlotOp::SetPolar(polar) => {
            if plot.polar == polar {
                return false;
            }
            plot.polar = polar;
            let (d_start, d_end) = if polar { (0.0, 360.0) } else { DEFAULT_DOMAIN };
            plot.x_axis.start = None;
            plot.x_axis.end = None;
            plot.y_axis.start = None;
            plot.y_axis.end = None;
            plot.x_axis.log = false;
            plot.y_axis.log = false;
            plot.x_axis.domain_start = Some(bound(d_start));
            plot.x_axis.domain_end = Some(bound(d_end));
            plot.y_axis.domain_start = None;
            plot.y_axis.domain_end = None;
            plot.markers.clear();
            true
        }
        PlotOp::AddMarker(vertical) => {
            let axis = if vertical { &plot.x_axis } else { &plot.y_axis };
            let value = axis.end.unwrap_or(DEFAULT_DOMAIN.1);
            plot.markers.push(PlotMarker { vertical, value });
            true
        }
        PlotOp::DeleteMarker => plot.markers.pop().is_some(),
        PlotOp::ToggleLog(y) => {
            if plot.polar {
                return false;
            }
            let axis = if y {
                &mut plot.y_axis
            } else {
                &mut plot.x_axis
            };
            axis.log = !axis.log;
            true
        }
        PlotOp::ToggleGrid => {
            let on = !(plot.x_axis.grid_lines || plot.y_axis.grid_lines);
            plot.x_axis.grid_lines = on;
            plot.y_axis.grid_lines = on;
            true
        }
        PlotOp::Style(change) => {
            let Some(trace) = plot.traces.last_mut() else {
                return false;
            };
            apply_style(&mut trace.style, &change);
            true
        }
    }
}

/// Applies one style change to a trace style.
pub fn apply_style(style: &mut TraceStyle, change: &StyleChange) {
    match *change {
        StyleChange::Symbol(name) => style.symbol = Some(name.to_owned()),
        StyleChange::Line(name) => style.line_style = Some(name.to_owned()),
        StyleChange::Color(i) => style.color = PALETTE.get(i).copied(),
        StyleChange::Weight(w) => style.line_weight = Some(f64::from(w)),
    }
}

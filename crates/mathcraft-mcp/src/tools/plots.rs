//! Plot tools: insert_plot and set_plot_options. A plot is x-y or polar, built from one text
//! expression per trace, with axis limits, a title, grid lines, logarithmic axes, markers and trace
//! styles. The Doc API has no trace names, legend or axis labels yet, and no 3D plots: those
//! arguments are not in the schemas, so sending them is an `invalid_params` (unknown argument).

use mathcraft_doc::locale::decimal_comma;
use mathcraft_doc::plot::{
    self, PlotOp, PlotSpec, StyleChange, LINE_STYLES, PALETTE, SYMBOLS, WEIGHTS,
};
use mathcraft_doc::{Doc, Placement};
use mathcraft_import::regions::{Axis2d, Equation, RegionKind, TraceStyle};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::write::{
    doc_err, error_keys, feedback, invalid, merge, number, number_schema, placement,
};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

/// Most series (traces) in one plot.
pub const MAX_SERIES: usize = 32;
/// Longest expression of one series or axis, in bytes.
pub const MAX_EXPR_BYTES: usize = 4096;
/// Longest plot title, in characters.
const MAX_TITLE_CHARS: usize = 200;
/// Largest magnitude of an axis limit, in data units.
const MAX_AXIS: f64 = 1.0e12;
/// Largest plot width or height, in page points.
const MAX_SIZE: f64 = 1.0e6;

const LABEL_INSERT: &str = "AI: insert_plot";
const LABEL_OPTIONS: &str = "AI: set_plot_options";

/// A plot edit, checked and owned: `insert_plot` and `set_plot_options` run standalone or as ops
/// of `batch`, through the same parser and [`apply_plot`].
pub(crate) enum PlotWrite {
    Insert(InsertPlan),
    Options(OptionChanges),
}

/// A new plot: its spec, placement and styling.
pub(crate) struct InsertPlan {
    spec: PlotSpec,
    at: Placement,
    styles: Vec<(usize, Vec<StyleChange>)>,
    /// Logarithmic x, logarithmic y.
    log: (bool, bool),
    size: (Option<f64>, Option<f64>),
}

/// The requested changes of set_plot_options, checked.
pub(crate) struct OptionChanges {
    region: u32,
    polar: Option<bool>,
    series: Option<Vec<Series>>,
    x_range: Option<Option<(f64, f64)>>,
    y_range: Option<Option<(f64, f64)>>,
    title: Option<String>,
    grid: Option<bool>,
    /// Logarithmic x, logarithmic y.
    log: [Option<bool>; 2],
    traces: Vec<(usize, Vec<StyleChange>)>,
    /// Some(true) for a vertical marker, Some(false) for a horizontal one.
    marker: Option<bool>,
    delete_marker: bool,
}

/// The unit of `x` in `insert_plot`: a short name such as `m` or `m/s`.
fn unit_arg(args: &Args<'_>) -> Result<Option<String>, ToolError> {
    let Some(unit) = args.opt_str("x_unit")? else {
        return Ok(None);
    };
    let ok = unit.len() <= 16
        && unit.starts_with(|c: char| c.is_ascii_alphabetic())
        && unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '^'));
    if !ok {
        return Err(invalid(
            "`x_unit` is a unit name such as `m` or `m/s` (letters, digits, _ / ^)",
        ));
    }
    Ok(Some(unit.to_owned()))
}

/// Writes every `x` of `expr` as `(x*unit)`. `x` counts only as a whole name, so `exp` and `x1`
/// are left alone; `2x` reads as `2*(x*unit)`.
pub(crate) fn with_x_unit(expr: &str, unit: &str) -> String {
    let chars: Vec<char> = expr.chars().collect();
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    let mut out = String::with_capacity(expr.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        let before = i.checked_sub(1).map(|j| chars[j]);
        let after = chars.get(i + 1).copied();
        let whole =
            c == 'x' && before.is_none_or(|b| !is_name(b)) && after.is_none_or(|a| !is_name(a));
        if whole {
            if before.is_some_and(|b| b.is_ascii_digit()) {
                out.push('*');
            }
            out.push_str(&format!("(x*{unit})"));
        } else {
            out.push(c);
        }
    }
    out
}

pub fn register(t: &mut ToolTable) {
    let mut props = vec![
        schema::document_id(),
        (
            "kind",
            json!({"type": "string", "enum": ["xy", "polar"], "description": "x-y (default) or polar; for polar, x is the angle and y the radius"}),
        ),
        ("series", series_schema()),
        (
            "x_range",
            range_schema(
                "Displayed x axis `{min, max}` (min below max); null or absent for automatic",
            ),
        ),
        (
            "y_range",
            range_schema(
                "Displayed y axis `{min, max}` (min below max); null or absent for automatic",
            ),
        ),
        (
            "title",
            schema::string("Plot title; at most 200 characters"),
        ),
        ("grid", schema::boolean("Grid lines on both axes")),
        (
            "log_x",
            schema::boolean("Logarithmic x axis (not for polar)"),
        ),
        (
            "log_y",
            schema::boolean("Logarithmic y axis (not for polar)"),
        ),
        (
            "width",
            number_schema("Width in page points (default about 345)"),
        ),
        (
            "height",
            number_schema("Height in page points (default about 330)"),
        ),
        (
            "x_unit",
            schema::string("Unit of x, such as `m`. The plot variable x is unitless, so with a unit every expression is written with x*unit (the curve is in the unit; the axis numbers stay plain)."),
        ),
    ];
    props.extend(position_props());
    t.add(Tool {
        name: "insert_plot",
        description: "Inserts an x-y or polar plot. Each series is `expr` (y against x, x defaults to `x`; for polar, the radius against the angle x) or `x_expr` with `y_expr` (parametric). Up to 32 series, 4 KiB per expression. x is unitless: to use units write `x*m` in the expression, or pass `x_unit: \"m\"` and the server writes it for you (`sin(x)` becomes `sin((x*m))`). Example: {\"series\":[{\"expr\":\"x^2*g\"}],\"x_unit\":\"m\"}. Returns region_id, the plot description and any new errors. Trace names, legend and axis labels are not supported yet. Allowed in `batch`.".into(),
        input_schema: schema::object(&props, &["document_id", "series"]),
        annotations: Annotations {
            title: "Insert plot",
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        handler: insert_plot,
    });
    t.add(Tool {
        name: "set_plot_options",
        description: "Changes a plot: `series` replaces every expression (the trace styles of kept indices stay), `x_range`/`y_range` set the displayed limits (null for automatic), `title`, `grid`, `log_x`/`log_y`, `polar` (switching resets the limits and markers), `traces` sets styles per trace index, `add_marker` adds a vertical or horizontal marker at the end of its axis (its value cannot be set yet) and `delete_marker` removes the last one. Each call is one undo step (labelled \"AI: set_plot_options\"). Allowed in `batch`. Returns the plot description and any new errors.".into(),
        input_schema: schema::object(&options_props(), &["document_id", "region_id"]),
        annotations: Annotations {
            title: "Set plot options",
            read_only: false,
            destructive: true,
            idempotent: true,
        },
        handler: set_plot_options,
    });
}

fn options_props() -> Vec<(&'static str, Value)> {
    vec![
        schema::document_id(),
        ("region_id", schema::integer("Plot to change")),
        ("series", series_schema()),
        (
            "x_range",
            range_schema("Displayed x axis `{min, max}`, or null for automatic"),
        ),
        (
            "y_range",
            range_schema("Displayed y axis `{min, max}`, or null for automatic"),
        ),
        ("title", schema::string("Plot title; empty clears it")),
        ("grid", schema::boolean("Grid lines on both axes")),
        ("log_x", schema::boolean("Logarithmic x axis")),
        ("log_y", schema::boolean("Logarithmic y axis")),
        ("polar", schema::boolean("true for polar, false for x-y")),
        (
            "traces",
            json!({
                "type": "array",
                "maxItems": MAX_SERIES,
                "items": {
                    "type": "object",
                    "properties": {
                        "index": {"type": "integer", "minimum": 0, "description": "Trace index, from 0"},
                        "style": style_schema(),
                    },
                    "required": ["index", "style"],
                    "additionalProperties": false,
                },
                "description": "Style changes of existing traces",
            }),
        ),
        (
            "add_marker",
            json!({"type": "string", "enum": ["vertical", "horizontal"], "description": "Add a marker at the end of the axis"}),
        ),
        ("delete_marker", schema::boolean("Remove the last marker")),
    ]
}

/// Names the style choices the Doc API has. Symbols, line styles and weights are the menus of
/// Prime's plot dialog; colours are indices into its palette.
fn style_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "symbol": {"type": "string", "enum": SYMBOLS},
            "line": {"type": "string", "enum": LINE_STYLES},
            "color": {
                "type": "integer", "minimum": 0, "maximum": PALETTE.len() - 1,
                "description": "Palette index: 0 dark blue, 1 black, 2 red, 3 yellow, 4 orange, 5 green, 6 blue",
            },
            "weight": {"type": "integer", "enum": WEIGHTS, "description": "Line thickness in points"},
        },
        "additionalProperties": false,
    })
}

fn series_schema() -> Value {
    json!({
        "type": "array",
        "minItems": 1,
        "maxItems": MAX_SERIES,
        "description": "Traces: each has `expr`, or `x_expr` and `y_expr`",
        "items": {
            "type": "object",
            "properties": {
                "expr": {"type": "string", "description": "y as a function of x (one expression)"},
                "x_expr": {"type": "string", "description": "x of a parametric trace (one expression)"},
                "y_expr": {"type": "string", "description": "y of a parametric trace (one expression)"},
                "style": style_schema(),
            },
            "additionalProperties": false,
        },
    })
}

fn range_schema(description: &str) -> Value {
    json!({
        "type": ["object", "null"],
        "description": description,
        "properties": {"min": {"type": "number"}, "max": {"type": "number"}},
        "required": ["min", "max"],
        "additionalProperties": false,
    })
}

fn position_props() -> [(&'static str, Value); 3] {
    [
        ("x", number_schema("Left edge in page points (needs y)")),
        ("y", number_schema("Top edge in page points (needs x)")),
        (
            "after_region",
            schema::integer("Place directly below this region"),
        ),
    ]
}

/// Checks a value against a fixed list and returns the list's own spelling.
pub(crate) fn pick(
    options: &[&'static str],
    value: &str,
    what: &str,
) -> Result<&'static str, ToolError> {
    options
        .iter()
        .copied()
        .find(|o| *o == value)
        .ok_or_else(|| {
            invalid(format!(
                "unknown {what} `{value}`; use one of {}",
                options.join(", ")
            ))
        })
}

/// A boolean argument that may be absent or null (then None).
pub(crate) fn flag(args: &Args<'_>, key: &str) -> Result<Option<bool>, ToolError> {
    match args.0.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => args.opt_bool(key).map(Some),
    }
}

/// One expression of a series, checked: non-empty, bounded, no control characters, and exactly one
/// expression (a top-level comma or semicolon would make the Doc API read two traces).
fn expression(text: &str, what: &str) -> Result<String, ToolError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(invalid(format!("{what} is empty")));
    }
    if t.len() > MAX_EXPR_BYTES {
        return Err(ToolError::LimitReached(format!(
            "{what} is {} bytes, over the limit of {MAX_EXPR_BYTES}",
            t.len()
        )));
    }
    if t.chars().any(char::is_control) {
        return Err(invalid(format!("{what} has a control character")));
    }
    if plot::split_expressions(t).len() != 1 {
        return Err(invalid(format!(
            "{what} must be one expression (no top-level comma or semicolon); use one series per expression"
        )));
    }
    Ok(t.to_owned())
}

/// Joins the expressions of one axis the way the plot text form reads them.
fn join(items: &[&str]) -> String {
    items.join(if decimal_comma() { "; " } else { ", " })
}

/// One trace as the caller gave it.
struct Series {
    x: String,
    y: String,
    /// Given as `x_expr` and `y_expr` (x is a function of the parameter, not the plain axis).
    parametric: bool,
    style: Option<TraceStyleArg>,
}

/// The style choices of one trace; None where the caller said nothing.
#[derive(Clone, Copy, Default)]
struct TraceStyleArg {
    symbol: Option<&'static str>,
    line: Option<&'static str>,
    color: Option<usize>,
    weight: Option<u8>,
}

fn style_of(v: &Value, what: &str) -> Result<TraceStyleArg, ToolError> {
    let map = v
        .as_object()
        .ok_or_else(|| invalid(format!("{what} style must be an object")))?;
    let a = Args(map);
    a.only(&["symbol", "line", "color", "weight"])?;
    let symbol = a
        .opt_str("symbol")?
        .map(|s| pick(&SYMBOLS, s, "symbol"))
        .transpose()?;
    let line = a
        .opt_str("line")?
        .map(|s| pick(&LINE_STYLES, s, "line style"))
        .transpose()?;
    let color = match a.opt_u64("color")? {
        None => None,
        Some(c) => Some(
            usize::try_from(c)
                .ok()
                .filter(|c| *c < PALETTE.len())
                .ok_or_else(|| {
                    invalid(format!(
                        "{what} colour must be a palette index 0 to {}",
                        PALETTE.len() - 1
                    ))
                })?,
        ),
    };
    let weight = match a.opt_u64("weight")? {
        None => None,
        Some(w) => Some(
            WEIGHTS
                .iter()
                .copied()
                .find(|x| u64::from(*x) == w)
                .ok_or_else(|| invalid(format!("{what} weight must be one of {WEIGHTS:?}")))?,
        ),
    };
    if symbol.is_none() && line.is_none() && color.is_none() && weight.is_none() {
        return Err(invalid(format!("{what} style sets nothing")));
    }
    Ok(TraceStyleArg {
        symbol,
        line,
        color,
        weight,
    })
}

/// The Doc API style changes of one trace style.
fn changes(st: &TraceStyleArg) -> Vec<StyleChange> {
    let mut out = Vec::new();
    if let Some(s) = st.symbol {
        out.push(StyleChange::Symbol(s));
    }
    if let Some(l) = st.line {
        out.push(StyleChange::Line(l));
    }
    if let Some(c) = st.color {
        out.push(StyleChange::Color(c));
    }
    if let Some(w) = st.weight {
        out.push(StyleChange::Weight(w));
    }
    out
}

fn parse_series(v: &Value, i: usize) -> Result<Series, ToolError> {
    let map = v
        .as_object()
        .ok_or_else(|| invalid(format!("series {i} must be an object")))?;
    let a = Args(map);
    a.only(&["expr", "x_expr", "y_expr", "style"])?;
    let expr = a.opt_str("expr")?;
    let x_expr = a.opt_str("x_expr")?;
    let y_expr = a.opt_str("y_expr")?;
    let (x, y, parametric) = match (expr, x_expr, y_expr) {
        (Some(e), None, None) => ("x", e, false),
        (None, Some(x), Some(y)) => (x, y, true),
        _ => {
            return Err(invalid(format!(
                "series {i}: give `expr`, or `x_expr` and `y_expr`"
            )))
        }
    };
    let style = match a.0.get("style") {
        None | Some(Value::Null) => None,
        Some(s) => Some(style_of(s, &format!("series {i}"))?),
    };
    Ok(Series {
        x: expression(x, &format!("series {i} x expression"))?,
        y: expression(y, &format!("series {i} expression"))?,
        parametric,
        style,
    })
}

fn series_list(args: &Args<'_>) -> Result<Option<Vec<Series>>, ToolError> {
    let Some(v) = args.0.get("series").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let items = v
        .as_array()
        .ok_or_else(|| invalid("`series` must be an array"))?;
    if items.is_empty() {
        return Err(invalid("`series` is empty; give at least one trace"));
    }
    if items.len() > MAX_SERIES {
        return Err(ToolError::LimitReached(format!(
            "{} series, over the limit of {MAX_SERIES}",
            items.len()
        )));
    }
    items
        .iter()
        .enumerate()
        .map(|(i, s)| parse_series(s, i))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// `{min, max}`: finite, bounded, min below max. Null or absent gives None (automatic).
fn limits(args: &Args<'_>, key: &str) -> Result<Option<(f64, f64)>, ToolError> {
    let Some(v) = args.0.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let map = v.as_object().ok_or_else(|| {
        invalid(format!(
            "`{key}` must be an object with min and max, or null"
        ))
    })?;
    let a = Args(map);
    a.only(&["min", "max"])?;
    let (Some(lo), Some(hi)) = (number(&a, "min")?, number(&a, "max")?) else {
        return Err(invalid(format!("`{key}` needs min and max")));
    };
    if lo.abs() > MAX_AXIS || hi.abs() > MAX_AXIS {
        return Err(invalid(format!("`{key}` is above {MAX_AXIS} in magnitude")));
    }
    if lo >= hi {
        return Err(invalid(format!("`{key}`: min must be below max")));
    }
    Ok(Some((lo, hi)))
}

/// The limit texts of the plot dialog (empty means automatic).
fn bounds(r: Option<(f64, f64)>) -> (String, String) {
    match r {
        Some((a, b)) => (format!("{a}"), format!("{b}")),
        None => (String::new(), String::new()),
    }
}

fn title_arg(args: &Args<'_>) -> Result<Option<String>, ToolError> {
    let Some(t) = args.opt_str("title")? else {
        return Ok(None);
    };
    let t = t.trim();
    if t.chars().count() > MAX_TITLE_CHARS {
        return Err(ToolError::LimitReached(format!(
            "`title` is over {MAX_TITLE_CHARS} characters"
        )));
    }
    if t.chars().any(char::is_control) {
        return Err(invalid("`title` has a control character"));
    }
    Ok(Some(t.to_owned()))
}

/// Optional width and height in page points, each above 0 and bounded.
fn size_arg(args: &Args<'_>) -> Result<(Option<f64>, Option<f64>), ToolError> {
    let w = number(args, "width")?;
    let h = number(args, "height")?;
    for (key, v) in [("width", w), ("height", h)] {
        if let Some(v) = v {
            if !(v > 0.0 && v <= MAX_SIZE) {
                return Err(invalid(format!(
                    "`{key}` must be above 0 and at most {MAX_SIZE}"
                )));
            }
        }
    }
    Ok((w, h))
}

/// The `traces` argument of set_plot_options: style changes per trace index.
fn trace_styles(args: &Args<'_>) -> Result<Vec<(usize, Vec<StyleChange>)>, ToolError> {
    let Some(v) = args.0.get("traces").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let items = v
        .as_array()
        .ok_or_else(|| invalid("`traces` must be an array"))?;
    if items.len() > MAX_SERIES {
        return Err(ToolError::LimitReached(format!(
            "{} trace styles, over the limit of {MAX_SERIES}",
            items.len()
        )));
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let map = item
                .as_object()
                .ok_or_else(|| invalid(format!("traces[{i}] must be an object")))?;
            let a = Args(map);
            a.only(&["index", "style"])?;
            let index = usize::try_from(a.u64("index")?)
                .map_err(|_| invalid(format!("traces[{i}] index is too large")))?;
            let style =
                a.0.get("style")
                    .ok_or_else(|| invalid(format!("traces[{i}] needs `style`")))?;
            let st = style_of(style, &format!("traces[{i}]"))?;
            Ok((index, changes(&st)))
        })
        .collect()
}

/// Parses `insert_plot` (standalone, or as a batch op when `standalone` is false).
fn parse_insert(args: &Args<'_>, standalone: bool) -> Result<InsertPlan, ToolError> {
    only_with(
        args,
        standalone,
        &[
            "kind",
            "series",
            "x_range",
            "y_range",
            "title",
            "grid",
            "log_x",
            "log_y",
            "x",
            "y",
            "after_region",
            "width",
            "height",
            "x_unit",
        ],
    )?;
    let polar = match args.opt_str("kind")?.unwrap_or("xy") {
        "xy" => false,
        "polar" => true,
        other => return Err(invalid(format!("unknown kind `{other}`; use xy or polar"))),
    };
    let mut series = series_list(args)?.ok_or_else(|| invalid("missing argument `series`"))?;
    if let Some(unit) = unit_arg(args)? {
        for s in &mut series {
            // A plain trace keeps `x` as the axis; a parametric x is a function of the parameter.
            if s.parametric {
                s.x = with_x_unit(&s.x, &unit);
            }
            s.y = with_x_unit(&s.y, &unit);
        }
    }
    let x_range = limits(args, "x_range")?;
    let y_range = limits(args, "y_range")?;
    let title = title_arg(args)?.unwrap_or_default();
    let grid = flag(args, "grid")?.unwrap_or(false);
    let log_x = flag(args, "log_x")?.unwrap_or(false);
    let log_y = flag(args, "log_y")?.unwrap_or(false);
    if polar && (log_x || log_y) {
        return Err(invalid("a polar plot has no logarithmic axis"));
    }
    let at = placement(args)?;
    let size = size_arg(args)?;
    let (x_min, x_max) = bounds(x_range);
    let (y_min, y_max) = bounds(y_range);
    let xs: Vec<&str> = series.iter().map(|s| s.x.as_str()).collect();
    let ys: Vec<&str> = series.iter().map(|s| s.y.as_str()).collect();
    let spec = PlotSpec {
        x: join(&xs),
        y: join(&ys),
        x_min,
        x_max,
        y_min,
        y_max,
        grid,
        title,
        polar,
    };
    let styles: Vec<(usize, Vec<StyleChange>)> = series
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.style.map(|st| (i, changes(&st))))
        .collect();
    Ok(InsertPlan {
        spec,
        at,
        styles,
        log: (log_x, log_y),
        size,
    })
}

/// Parses `set_plot_options` (standalone, or as a batch op when `standalone` is false).
fn parse_options(args: &Args<'_>, standalone: bool) -> Result<OptionChanges, ToolError> {
    only_with(
        args,
        standalone,
        &[
            "region_id",
            "series",
            "x_range",
            "y_range",
            "title",
            "grid",
            "log_x",
            "log_y",
            "polar",
            "traces",
            "add_marker",
            "delete_marker",
        ],
    )?;
    let region = args.region_id("region_id")?;
    let series = series_list(args)?;
    let x_range = args
        .0
        .contains_key("x_range")
        .then(|| limits(args, "x_range"))
        .transpose()?;
    let y_range = args
        .0
        .contains_key("y_range")
        .then(|| limits(args, "y_range"))
        .transpose()?;
    let title = title_arg(args)?;
    let grid = flag(args, "grid")?;
    let log = [flag(args, "log_x")?, flag(args, "log_y")?];
    let polar = flag(args, "polar")?;
    let traces = trace_styles(args)?;
    let marker = match args.opt_str("add_marker")? {
        None => None,
        Some(name) => Some(pick(&["vertical", "horizontal"], name, "marker")? == "vertical"),
    };
    let delete_marker = flag(args, "delete_marker")?.unwrap_or(false);
    let nothing = series.is_none()
        && x_range.is_none()
        && y_range.is_none()
        && title.is_none()
        && grid.is_none()
        && log == [None, None]
        && polar.is_none()
        && traces.is_empty()
        && marker.is_none()
        && !delete_marker;
    if nothing {
        return Err(invalid("nothing to change: give at least one plot option"));
    }
    Ok(OptionChanges {
        region,
        polar,
        series,
        x_range,
        y_range,
        title,
        grid,
        log,
        traces,
        marker,
        delete_marker,
    })
}

/// Parses one plot op of `batch` (`insert_plot` or `set_plot_options`), or a standalone call.
pub(crate) fn parse_plot(
    name: &str,
    args: &Args<'_>,
    standalone: bool,
) -> Result<PlotWrite, ToolError> {
    match name {
        "insert_plot" => Ok(PlotWrite::Insert(parse_insert(args, standalone)?)),
        "set_plot_options" => Ok(PlotWrite::Options(parse_options(args, standalone)?)),
        other => Err(invalid(format!("`{other}` is not a plot op"))),
    }
}

/// The only-keys check: the head key (`document_id`, or `op` in a batch) plus the given keys.
fn only_with(args: &Args<'_>, standalone: bool, keys: &[&str]) -> Result<(), ToolError> {
    let head = if standalone { "document_id" } else { "op" };
    let all: Vec<&str> = std::iter::once(head).chain(keys.iter().copied()).collect();
    args.only(&all)
}

/// Applies one checked plot edit to the document and returns the region it made or changed. The
/// caller groups the steps and rolls them back on error.
pub(crate) fn apply_plot(doc: &mut Doc, w: &PlotWrite) -> Result<u32, ToolError> {
    match w {
        PlotWrite::Insert(plan) => {
            build_plot(doc, &plan.spec, plan.at, &plan.styles, plan.log, plan.size)
        }
        PlotWrite::Options(c) => {
            let current = plot_state(doc, c.region)?;
            if c.polar.unwrap_or(current.polar) && c.log.contains(&Some(true)) {
                return Err(invalid("a polar plot has no logarithmic axis"));
            }
            apply_options(doc, c.region, c)?;
            Ok(c.region)
        }
    }
}

fn insert_plot(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    let plan = PlotWrite::Insert(parse_insert(args, true)?);
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let before = error_keys(doc);
        let start = doc.steps_executed();
        match apply_plot(doc, &plan) {
            Ok(id) => {
                let taken = doc.steps_executed() - start;
                if taken > 0 {
                    doc.collapse_steps(taken, LABEL_INSERT);
                }
                let fb = feedback(doc, &before, &[id], &[]);
                Ok(merge(
                    json!({"ok": true, "region_id": id, "plot": plot_view(doc, id)}),
                    fb,
                ))
            }
            Err(e) => {
                let taken = doc.steps_executed() - start;
                doc.rollback_steps(taken).map_err(|r| {
                    ToolError::Internal(format!("insert failed and could not be undone: {r}"))
                })?;
                Err(e)
            }
        }
    })?;
    Ok(ToolOutput::json(v))
}

fn build_plot(
    doc: &mut Doc,
    spec: &PlotSpec,
    at: Placement,
    styles: &[(usize, Vec<StyleChange>)],
    log: (bool, bool),
    size: (Option<f64>, Option<f64>),
) -> Result<u32, ToolError> {
    let id = doc.insert_plot(spec, at).map_err(doc_err)?;
    if log.0 {
        doc.plot_op(id, PlotOp::ToggleLog(false)).map_err(doc_err)?;
    }
    if log.1 {
        doc.plot_op(id, PlotOp::ToggleLog(true)).map_err(doc_err)?;
    }
    apply_styles(doc, id, styles)?;
    if size.0.is_some() || size.1.is_some() {
        let (w0, h0) = doc
            .region(id)
            .and_then(|r| r.size)
            .unwrap_or(plot::PLOT_DEFAULT_SIZE);
        doc.resize_region(id, (size.0.unwrap_or(w0), size.1.unwrap_or(h0)))
            .map_err(doc_err)?;
    }
    Ok(id)
}

fn apply_styles(
    doc: &mut Doc,
    id: u32,
    styles: &[(usize, Vec<StyleChange>)],
) -> Result<(), ToolError> {
    for (trace, list) in styles {
        for change in list {
            doc.plot_trace_style(id, *trace, *change).map_err(doc_err)?;
        }
    }
    Ok(())
}

/// What the current plot is, as far as set_plot_options needs it.
struct PlotState {
    polar: bool,
    /// Logarithmic x axis, logarithmic y axis.
    log: [bool; 2],
    /// The text form of the plot (limits, title, grid, expressions).
    spec: PlotSpec,
    /// Some expression of the plot does not read back as text.
    unsupported: bool,
    traces: usize,
}

fn not_a_plot(id: u32) -> ToolError {
    invalid(format!("region {id} is not an x-y or polar plot"))
}

fn plot_state(doc: &Doc, id: u32) -> Result<PlotState, ToolError> {
    let region = doc.region(id).ok_or_else(|| crate::error::no_region(id))?;
    let RegionKind::Plot(info) = &region.kind else {
        return Err(not_a_plot(id));
    };
    let (Some(model), Some(spec)) = (info.plot.as_deref(), plot::spec_of(region)) else {
        return Err(not_a_plot(id));
    };
    let unsupported = model
        .x_axis
        .equations
        .iter()
        .chain(model.y_axis.equations.iter())
        .any(|e| matches!(e, Equation::Unsupported(_)));
    Ok(PlotState {
        polar: model.polar,
        log: [model.x_axis.log, model.y_axis.log],
        spec,
        unsupported,
        traces: model.traces.len(),
    })
}

fn set_plot_options(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    let changes = PlotWrite::Options(parse_options(args, true)?);
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let before = error_keys(doc);
        let start = doc.steps_executed();
        match apply_plot(doc, &changes) {
            Ok(id) => {
                let taken = doc.steps_executed() - start;
                if taken > 0 {
                    doc.collapse_steps(taken, LABEL_OPTIONS);
                }
                let fb = feedback(doc, &before, &[id], &[]);
                Ok(merge(
                    json!({"ok": true, "region_id": id, "plot": plot_view(doc, id)}),
                    fb,
                ))
            }
            Err(e) => {
                let taken = doc.steps_executed() - start;
                doc.rollback_steps(taken).map_err(|r| {
                    ToolError::Internal(format!("plot change failed and could not be undone: {r}"))
                })?;
                Err(e)
            }
        }
    })?;
    Ok(ToolOutput::json(v))
}

fn apply_options(doc: &mut Doc, id: u32, c: &OptionChanges) -> Result<(), ToolError> {
    // Polar first: switching resets the limits and markers.
    if let Some(p) = c.polar {
        if plot_state(doc, id)?.polar != p {
            doc.plot_op(id, PlotOp::SetPolar(p)).map_err(doc_err)?;
        }
    }
    if c.series.is_some()
        || c.x_range.is_some()
        || c.y_range.is_some()
        || c.title.is_some()
        || c.grid.is_some()
    {
        let state = plot_state(doc, id)?;
        let mut spec = state.spec;
        match &c.series {
            Some(series) => {
                let xs: Vec<&str> = series.iter().map(|s| s.x.as_str()).collect();
                let ys: Vec<&str> = series.iter().map(|s| s.y.as_str()).collect();
                spec.x = join(&xs);
                spec.y = join(&ys);
            }
            None if state.unsupported => {
                return Err(invalid(
                    "the plot has an expression this server cannot edit; pass `series` to replace the expressions",
                ));
            }
            None => {}
        }
        if let Some(r) = c.x_range {
            (spec.x_min, spec.x_max) = bounds(r);
        }
        if let Some(r) = c.y_range {
            (spec.y_min, spec.y_max) = bounds(r);
        }
        if let Some(t) = &c.title {
            spec.title.clone_from(t);
        }
        if let Some(g) = c.grid {
            spec.grid = g;
        }
        doc.set_plot_spec(id, &spec).map_err(doc_err)?;
    }
    for (axis, want) in c.log.iter().enumerate() {
        if let Some(want) = want {
            if plot_state(doc, id)?.log[axis] != *want {
                doc.plot_op(id, PlotOp::ToggleLog(axis == 1))
                    .map_err(doc_err)?;
            }
        }
    }
    if let Some(vertical) = c.marker {
        doc.plot_op(id, PlotOp::AddMarker(vertical))
            .map_err(doc_err)?;
    }
    if c.delete_marker && !doc.plot_op(id, PlotOp::DeleteMarker).map_err(doc_err)? {
        return Err(ToolError::NotFound(
            "the plot has no marker to delete".into(),
        ));
    }
    let traces = plot_state(doc, id)?.traces;
    for (index, list) in &c.traces {
        if *index >= traces {
            return Err(invalid(format!(
                "trace {index} does not exist; the plot has {traces} trace(s)"
            )));
        }
        for change in list {
            doc.plot_trace_style(id, *index, *change).map_err(doc_err)?;
        }
    }
    Ok(())
}

/// The plot as the read tools describe it: axes, traces with their expressions and styles, and
/// markers. Null for a region that is not a plot.
fn plot_view(doc: &Doc, id: u32) -> Value {
    let Some(region) = doc.region(id) else {
        return Value::Null;
    };
    let RegionKind::Plot(info) = &region.kind else {
        return Value::Null;
    };
    let Some(m) = info.plot.as_deref() else {
        return json!({"kind": info.kind});
    };
    json!({
        "kind": if m.polar { "polar" } else { "xy" },
        "title": m.title,
        "grid": m.x_axis.grid_lines || m.y_axis.grid_lines,
        "x_axis": axis_view(&m.x_axis),
        "y_axis": axis_view(&m.y_axis),
        "traces": m.traces.iter().enumerate().map(|(i, t)| json!({
            "index": i,
            "x": equation_view(&t.x),
            "y": equation_view(&t.y),
            "style": style_view(&t.style),
        })).collect::<Vec<_>>(),
        "markers": m.markers.iter().map(|k| json!({"vertical": k.vertical, "value": k.value})).collect::<Vec<_>>(),
    })
}

fn axis_view(a: &Axis2d) -> Value {
    json!({"start": a.start, "end": a.end, "log": a.log, "grid": a.grid_lines})
}

fn equation_view(e: &Equation) -> Value {
    match e {
        Equation::Expr(x) => json!(x.to_string()),
        Equation::Empty => Value::Null,
        Equation::Unsupported(reason) => json!({"unsupported": reason}),
    }
}

fn style_view(s: &TraceStyle) -> Value {
    json!({
        "color_rgba": s.color,
        "line_weight": s.line_weight,
        "line_style": s.line_style,
        "symbol": s.symbol,
    })
}

//! Plot regions (`<plot>`): the 2D model read from the plot XML (`xyPlot`, `polarPlot`).
//!
//! What the stored XML says, as far as it is known (checked on the 23 plots of the samples):
//!
//! * The plot type element holds `<traces>` and `<axes>`. Trace `i` plots the `i`-th
//!   `<plotEquation>` of the x axis (`xAxis`, or `angularAxis` for a polar plot) against the `i`-th
//!   `<plotEquation>` of the y axis (`yAxis`, or `radialAxis`). The counts are equal in every
//!   sample.
//! * A trace with `num-of-points` samples its parameter at one fewer value than that, across the x axis
//!   domain (the polar angle domain for a polar plot). A trace without it takes the values of its
//!   range variable (see `evaluate_plots`).
//! * An equation whose `<math>` is only a placeholder is an empty trace, not an error.
//! * `scale-type` on a domain is `linear` in every sample. `log` is read as logarithmic. The
//!   value Prime writes for a log axis is not in any sample, so this is inferred.
//! * `<title/>` and `<legend/>` are empty in every sample. Titles and legend entries are not read.
//! * `<axisLabel/>` on each axis is empty in every sample; its text is read as the axis caption
//!   (the element name is the only evidence). `axisLine position` is `origin` in every 2D sample;
//!   `positionticmark` and `legendWidth` (layout hints of Prime's own labels) are not read.
//! * A 3D plot (`plot3D`) is read as a region with no 2D model (`Plot2d` is `None`).

use mathcraft_core::expr::Expr;
use roxmltree::Node;

use super::plot3d::read_plot3d;
use super::PlotInfo;
use crate::expr::convert_math;

/// A 2D plot: its traces and its two axes. For a polar plot the axes are the angular and the
/// radial axis, stored in `x_axis` and `y_axis`.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot2d {
    /// `polarPlot` rather than `xyPlot`.
    pub polar: bool,
    /// Whether the plot has a `<legend>` element.
    pub legend: bool,
    /// The plot title: the text of `<title>`, when it is not empty. Not seen in any sample (the
    /// samples have `<title/>`), so the element name is the one the other plot children use.
    pub title: Option<String>,
    /// The x axis (`xAxis`, or `angularAxis` when polar).
    pub x_axis: Axis2d,
    /// The y axis (`yAxis`, or `radialAxis` when polar).
    pub y_axis: Axis2d,
    /// The traces in document order. Trace `i` uses equation `i` of each axis.
    pub traces: Vec<PlotTrace>,
    /// Vertical and horizontal markers. Display only: no sample has one, so the element is not
    /// known and they are neither read nor written (`UNVERIFIED_FORMATS.md`).
    pub markers: Vec<PlotMarker>,
}

/// A marker line on a 2D plot (Prime's Add vertical or horizontal marker).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotMarker {
    /// A vertical line at an x value (`true`), or a horizontal one at a y value.
    pub vertical: bool,
    /// The value on its axis, in data units.
    pub value: f64,
}

/// One axis of a 2D plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Axis2d {
    /// `start` attribute: the axis's first value as Prime displays it.
    pub start: Option<f64>,
    /// `end` attribute: the axis's last value as Prime displays it.
    pub end: Option<f64>,
    /// `startValue` of the domain: the first value Prime samples the axis over. With auto-scale the
    /// displayed `start` is padded past it (sample 6: displayed -5.1, sampled from -5). `None` when
    /// the element is empty or a placeholder.
    pub domain_start: Option<Equation>,
    /// `endValue` of the domain, read like `domain_start`.
    pub domain_end: Option<Equation>,
    /// Logarithmic scale (`scale-type` of the domain contains `log`).
    pub log: bool,
    /// Whether the axis has `<gridLines>`.
    pub grid_lines: bool,
    /// `gridFrequency`: the number of grid divisions Prime shows (inferred, not confirmed).
    pub grid_frequency: Option<u32>,
    /// The `<plotEquation>` of each trace, in order.
    pub equations: Vec<Equation>,
    /// How the axis is drawn: where it crosses, which of tick marks and labels show, its caption.
    pub look: AxisLook,
}

/// The display settings of one axis, read from `<axisLine>`, `<axisGrid>` and `<axisLabel>`.
/// Reader only: the writer keeps the original XML of a stored plot (see `UNVERIFIED_FORMATS.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct AxisLook {
    /// `axisLine position="origin"` (every 2D sample): the axis crosses the other one at zero,
    /// kept inside the range. False for any other stated position: the axis sits at the frame.
    /// Absent is read as the origin, Prime's default (screenshot 0053).
    pub at_origin: bool,
    /// `tickMarks display`: false only when it says `false`.
    pub tick_marks: bool,
    /// `gridLabels display`: false only when it says `false`.
    pub grid_labels: bool,
    /// Text of `<axisLabel>`, when not empty. Empty in every sample.
    pub label: Option<String>,
}

impl Default for AxisLook {
    fn default() -> Self {
        Self {
            at_origin: true,
            tick_marks: true,
            grid_labels: true,
            label: None,
        }
    }
}

/// One trace of a 2D plot.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotTrace {
    /// `resultRef` of the trace's data in `result.xml`.
    pub result_ref: Option<u32>,
    /// `num-of-points` as written: the trace stores one value fewer (see `evaluate_plots`). `None`
    /// for a trace without it.
    pub num_of_points: Option<u32>,
    /// The trace's style.
    pub style: TraceStyle,
    /// The trace's legend name: the `name` attribute of `<trace>`, when not empty. Not seen in any
    /// sample (the samples' legends are empty), so the attribute name is inferred.
    pub name: Option<String>,
    /// The equation on the x axis (`x` for trace `i` is equation `i` of `xAxis`).
    pub x: Equation,
    /// The equation on the y axis.
    pub y: Equation,
}

/// The style of one trace, as written in `<traceStyle>`.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceStyle {
    /// `color` as `[r, g, b, a]`. Stored as `#AARRGGBB`. `None` when absent or malformed.
    pub color: Option<[u8; 4]>,
    /// `line-weight` in points.
    pub line_weight: Option<f64>,
    /// `line-style`, for example `Solid`.
    pub line_style: Option<String>,
    /// `symbol`, for example `none`.
    pub symbol: Option<String>,
    /// The text of `<traceStyle>`, for example `lines`.
    pub mode: Option<String>,
    /// `automaticStyle="true"`: Prime picks the colour itself, and the stored `color` is only a
    /// placeholder (every 3D trace of the samples is `#FF000000` with this flag). Seen only on
    /// 3D traces in the samples; 2D traces never carry it.
    pub automatic: bool,
}

/// One plot equation.
#[derive(Debug, Clone, PartialEq)]
pub enum Equation {
    /// A converted expression, for example `x` or `y(x)`.
    Expr(Expr),
    /// A placeholder: the trace has no expression on this axis.
    Empty,
    /// The equation could not be converted. The text is the reason.
    Unsupported(String),
}

/// Reads a `<plot>` element into a [`PlotInfo`]. The 2D model is read for `xyPlot` and
/// `polarPlot`. Other plot types have `plot: None`.
pub(crate) fn plot_info(node: Node<'_, '_>) -> PlotInfo {
    let children: Vec<Node<'_, '_>> = node.children().filter(|c| c.is_element()).collect();
    let type_node = children
        .iter()
        .find(|c| c.tag_name().name().ends_with("Plot"))
        .or(children.first())
        .copied();
    let kind = type_node.map_or_else(String::new, |c| c.tag_name().name().to_owned());
    let traces = node
        .descendants()
        .filter(|n| n.is_element() && matches!(n.tag_name().name(), "trace" | "trace3D"))
        .filter_map(|n| n.attribute("resultRef")?.trim().parse().ok())
        .collect();
    let (x_name, y_name) = if kind == "polarPlot" {
        ("angularAxis", "radialAxis")
    } else {
        ("xAxis", "yAxis")
    };
    let plot = match type_node {
        Some(t) if kind == "xyPlot" || kind == "polarPlot" => Some(Box::new(read_plot2d(
            t,
            kind == "polarPlot",
            x_name,
            y_name,
        ))),
        _ => None,
    };
    let plot3d = match type_node {
        Some(t) if kind == "plot3D" => Some(Box::new(read_plot3d(t))),
        _ => None,
    };
    PlotInfo {
        kind,
        traces,
        x_range: axis_range(node, "xAxis"),
        y_range: axis_range(node, "yAxis"),
        plot,
        plot3d,
    }
}

/// The 2D model of a plot type element (`xyPlot` or `polarPlot`).
fn read_plot2d(node: Node<'_, '_>, polar: bool, x_name: &str, y_name: &str) -> Plot2d {
    let x_axis = find_child(node, "axes")
        .and_then(|axes| find_child(axes, x_name))
        .map_or_else(empty_axis, axis2d);
    let y_axis = find_child(node, "axes")
        .and_then(|axes| find_child(axes, y_name))
        .map_or_else(empty_axis, axis2d);
    let traces: Vec<PlotTrace> = find_child(node, "traces")
        .into_iter()
        .flat_map(|t| children_named(t, "trace"))
        .enumerate()
        .map(|(i, trace)| PlotTrace {
            result_ref: trace
                .attribute("resultRef")
                .and_then(|v| v.trim().parse().ok()),
            num_of_points: trace
                .attribute("num-of-points")
                .and_then(|v| v.trim().parse().ok()),
            style: trace_style(trace),
            name: trace
                .attribute("name")
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned),
            x: equation_at(&x_axis, i),
            y: equation_at(&y_axis, i),
        })
        .collect();
    Plot2d {
        polar,
        legend: find_child(node, "legend").is_some(),
        title: element_text(node, "title"),
        x_axis,
        y_axis,
        traces,
        markers: Vec::new(),
    }
}

/// The text of the child `name` of `node`, trimmed; `None` when there is no such child or its
/// text is empty.
fn element_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    let child = find_child(node, name)?;
    let text: String = child
        .descendants()
        .filter(|d| d.is_text())
        .filter_map(|d| d.text())
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// One bound of a domain (`startValue` or `endValue`): its math, or `None` when the element is
/// empty. A placeholder is `Empty`.
fn domain_bound(domain: Node<'_, '_>, tag: &str) -> Option<Equation> {
    let math = find_child(domain, tag)?
        .children()
        .find(|c| c.is_element())?;
    Some(match convert_math(math) {
        Ok(Expr::Placeholder) => Equation::Empty,
        Ok(expr) => Equation::Expr(expr),
        Err(err) => Equation::Unsupported(format!("cannot convert plot domain: {err}")),
    })
}

fn empty_axis() -> Axis2d {
    Axis2d {
        start: None,
        end: None,
        domain_start: None,
        domain_end: None,
        log: false,
        grid_lines: false,
        grid_frequency: None,
        equations: Vec::new(),
        look: AxisLook::default(),
    }
}

fn axis2d(node: Node<'_, '_>) -> Axis2d {
    let grid = find_child(node, "axisGrid");
    let domain = node
        .children()
        .find(|c| c.is_element() && c.tag_name().name().ends_with("Domain"));
    let equations = find_child(node, "plotEquations")
        .map(|list| {
            children_named(list, "plotEquation")
                .into_iter()
                .map(equation)
                .collect()
        })
        .unwrap_or_default();
    Axis2d {
        start: finite_attr(node, "start"),
        end: finite_attr(node, "end"),
        domain_start: domain.and_then(|d| domain_bound(d, "startValue")),
        domain_end: domain.and_then(|d| domain_bound(d, "endValue")),
        log: domain
            .and_then(|d| d.attribute("scale-type"))
            .is_some_and(|s| s.to_ascii_lowercase().contains("log")),
        grid_lines: grid.is_some_and(|g| find_child(g, "gridLines").is_some()),
        grid_frequency: grid
            .and_then(|g| find_child(g, "gridFrequency"))
            .and_then(|f| f.text())
            .and_then(|t| t.trim().parse().ok()),
        equations,
        look: axis_look(node, grid),
    }
}

/// `display="false"` on the element `name` of `parent`; everything else shows.
fn display_flag(parent: Option<Node<'_, '_>>, name: &str) -> bool {
    !parent
        .and_then(|p| find_child(p, name))
        .and_then(|n| n.attribute("display"))
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("false"))
}

fn axis_look(node: Node<'_, '_>, grid: Option<Node<'_, '_>>) -> AxisLook {
    let position = find_child(node, "axisLine").and_then(|l| l.attribute("position"));
    AxisLook {
        at_origin: position.is_none_or(|p| p.trim().eq_ignore_ascii_case("origin")),
        tick_marks: display_flag(grid, "tickMarks"),
        grid_labels: display_flag(grid, "gridLabels"),
        label: element_text(node, "axisLabel"),
    }
}

/// The equation of trace `index`. An axis can list fewer equations than there are traces: sample
/// 8 lists `SP[0]` and `x` on its x axis for six traces, and the traces after the first all use
/// `x`. So a trace past the end of the list takes the last equation (inferred from that sample).
/// An axis with no equations at all gives `Unsupported`, so the mismatch is visible.
fn equation_at(axis: &Axis2d, index: usize) -> Equation {
    axis.equations
        .get(index)
        .or_else(|| axis.equations.last())
        .cloned()
        .unwrap_or_else(|| {
            Equation::Unsupported(format!("no plot equation for trace {}", index + 1))
        })
}

/// One `<plotEquation>`: its first `<math>` is the expression.
fn equation(node: Node<'_, '_>) -> Equation {
    match find_child(node, "math") {
        Some(math) => equation_of_math(math),
        None => Equation::Empty,
    }
}

/// The equation of one `<math>` element: a placeholder is `Empty`.
pub(super) fn equation_of_math(math: Node<'_, '_>) -> Equation {
    match convert_math(math) {
        Ok(Expr::Placeholder) => Equation::Empty,
        Ok(expr) => Equation::Expr(expr),
        Err(err) => Equation::Unsupported(format!("cannot convert plot equation: {err}")),
    }
}

pub(super) fn trace_style(trace: Node<'_, '_>) -> TraceStyle {
    let style = find_child(trace, "traceStyle");
    let attr = |name: &str| style.and_then(|s| s.attribute(name)).map(str::to_owned);
    TraceStyle {
        color: attr("color").as_deref().and_then(parse_color),
        line_weight: style.and_then(|s| finite_attr(s, "line-weight")),
        line_style: attr("line-style"),
        symbol: attr("symbol"),
        mode: style
            .and_then(|s| s.text())
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty()),
        automatic: attr("automaticStyle").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")),
    }
}

/// `#AARRGGBB` (or `#RRGGBB`, opaque) as `[r, g, b, a]`.
fn parse_color(text: &str) -> Option<[u8; 4]> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        8 => Some([
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
            (value >> 24) as u8,
        ]),
        6 => Some([(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]),
        _ => None,
    }
}

/// `(start, end)` of an axis of the plot, when the plot states both.
fn axis_range(plot: Node<'_, '_>, axis: &str) -> Option<(f64, f64)> {
    let el = plot
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == axis)?;
    Some((finite_attr(el, "start")?, finite_attr(el, "end")?))
}

pub(super) fn finite_attr(node: Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

pub(super) fn find_child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

pub(super) fn children_named<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Vec<Node<'a, 'i>> {
    node.children()
        .filter(|c| c.is_element() && c.tag_name().name() == name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_argb_and_rgb() {
        assert_eq!(parse_color("#FF00008B"), Some([0, 0, 0x8B, 0xFF]));
        assert_eq!(parse_color("#8000FF00"), Some([0, 0xFF, 0, 0x80]));
        assert_eq!(parse_color("#00FF00"), Some([0, 0xFF, 0, 255]));
        assert_eq!(parse_color("FF00008B"), None);
        assert_eq!(parse_color("#FFF"), None);
        assert_eq!(parse_color("#zz0000"), None);
        assert_eq!(parse_color("#é0000"), None);
    }

    #[test]
    fn the_domain_bounds_are_read_from_xy_domain() {
        // Shaped as sample 6's x axis: the displayed range is padded, the domain is not.
        let xml = r#"<xyPlot xmlns="http://schemas.mathsoft.com/worksheet50" xmlns:ml="http://schemas.mathsoft.com/math50"><axes>
            <xAxis start="-5.1" end="3.4"><xyDomain scale-type="linear" auto-scale="true">
              <startValue resultRef="13"><ml:real>-5</ml:real></startValue>
              <endValue resultRef="14"><ml:apply><ml:div /><ml:real>10</ml:real><ml:real>3</ml:real></ml:apply></endValue>
            </xyDomain></xAxis>
            <yAxis start="-1" end="6"><xyDomain scale-type="linear"><startValue /><endValue>
              <ml:placeholder /></endValue></xyDomain></yAxis>
          </axes></xyPlot>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let plot = read_plot2d(doc.root_element(), false, "xAxis", "yAxis");
        assert_eq!(plot.x_axis.start, Some(-5.1));
        assert!(matches!(plot.x_axis.domain_start, Some(Equation::Expr(_))));
        assert!(matches!(
            plot.x_axis.domain_end,
            Some(Equation::Expr(Expr::Binary { .. }))
        ));
        assert_eq!(plot.y_axis.domain_start, None);
        assert_eq!(plot.y_axis.domain_end, Some(Equation::Empty));
    }

    #[test]
    fn title_trace_names_and_automatic_style_are_read() {
        let xml = r##"<xyPlot xmlns="http://schemas.mathsoft.com/worksheet50"><title>  Speed  </title>
            <traces><trace num-of-points="5" name="v(t)"><traceStyle color="#FF00008B" automaticStyle="true">lines</traceStyle></trace>
            <trace num-of-points="5" name="  "><traceStyle color="#FFFF0000" automaticStyle="false"/></trace></traces>
            <axes><xAxis start="0" end="1"><xyDomain/></xAxis><yAxis start="0" end="1"><xyDomain/></yAxis></axes></xyPlot>"##;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let plot = read_plot2d(doc.root_element(), false, "xAxis", "yAxis");
        assert_eq!(plot.title.as_deref(), Some("Speed"));
        assert_eq!(plot.traces[0].name.as_deref(), Some("v(t)"));
        assert_eq!(plot.traces[1].name, None);
        assert!(plot.traces[0].style.automatic);
        assert!(!plot.traces[1].style.automatic);
    }
}

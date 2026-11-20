//! Headless 2D plot drawing: axis scales, tick positions, the mapping from data to points inside a
//! rectangle, and the traces of an evaluated plot (`mathcraft_import::evaluate_plots::PlotData`).
//! No egui types here; `canvas.rs` draws what this module decides.
//!
//! Geometry is in points, with `y` growing downwards on the screen (the top of the rectangle is the
//! maximum of the vertical axis).

use mathcraft_import::evaluate_plots::PlotData;

use crate::i18n::trf;

/// Tick count aimed at when an axis has no stated grid frequency.
pub const TARGET_TICKS: usize = 6;
/// Most ticks one axis may have, however the range is set.
pub const MAX_TICKS: usize = 60;
/// Colours of traces without a stated colour, in order: the colours of Prime's help figures
/// (dark teal, blue, green, purple, then orange and grey).
pub const PALETTE: [[u8; 4]; 6] = [
    [0x1F, 0x4E, 0x5A, 0xFF],
    [0x3A, 0x6F, 0xD8, 0xFF],
    [0x6A, 0xA8, 0x4F, 0xFF],
    [0x75, 0x50, 0x7B, 0xFF],
    [0xD9, 0x7B, 0x1E, 0xFF],
    [0x80, 0x80, 0x80, 0xFF],
];
/// Plot area fill (a pale, slightly green off-white, as in Prime).
pub const BACKGROUND: [u8; 3] = [0xF8, 0xFA, 0xF7];
/// Thin outer frame of the plot region.
pub const FRAME: [u8; 3] = [0xB4, 0xB8, 0xB4];
/// Left and bottom axes, ticks and arrowheads.
pub const AXIS: [u8; 3] = [0x26, 0x26, 0x26];
/// Major and minor grid lines.
pub const GRID_MAJOR: [u8; 3] = [0xD6, 0xDA, 0xD6];
pub const GRID_MINOR: [u8; 3] = [0xE8, 0xEB, 0xE8];

/// The three corners of an axis arrowhead with its tip at `tip`, pointing along the unit vector
/// `dir`, `len` long and `len * 0.7` wide. Screen coordinates.
pub fn arrow_head(tip: [f32; 2], dir: [f32; 2], len: f32) -> [[f32; 2]; 3] {
    let back = [tip[0] - dir[0] * len, tip[1] - dir[1] * len];
    let w = len * 0.35;
    let n = [-dir[1] * w, dir[0] * w];
    [
        tip,
        [back[0] + n[0], back[1] + n[1]],
        [back[0] - n[0], back[1] - n[1]],
    ]
}

/// Minor ticks of a logarithmic axis: `2..=9` times each decade, inside `[min, max]`.
pub fn log_minor_ticks(min: f64, max: f64) -> Vec<f64> {
    if !(min.is_finite() && max.is_finite() && min > 0.0 && min < max) {
        return Vec::new();
    }
    let first = min.log10().floor() as i32;
    let last = max.log10().ceil() as i32;
    if (last - first) as usize > MAX_TICKS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for k in first..last {
        for m in 2..=9 {
            let v = f64::from(m) * 10f64.powi(k);
            if v > min && v < max {
                out.push(v);
            }
        }
    }
    out
}

/// One axis of a drawn plot: its range, scale and tick positions.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisView {
    pub min: f64,
    pub max: f64,
    pub log: bool,
    /// Data values of the grid lines and tick labels, inside `[min, max]`.
    pub ticks: Vec<f64>,
    /// Decimal places for the tick labels.
    pub decimals: usize,
    /// The axis crosses the other one at zero (kept inside the range) instead of sitting at the
    /// frame (`axisLine position`).
    pub at_origin: bool,
    /// Draw tick marks (`tickMarks display`).
    pub tick_marks: bool,
    /// Draw tick labels (`gridLabels display`).
    pub labels: bool,
    /// The axis caption (`axisLabel`).
    pub caption: Option<String>,
}

/// The value on `other` at which an axis is drawn: zero when the axis goes through the origin and
/// zero is in the range, else the nearest end of the range. A logarithmic axis has no zero, so it
/// sits at its minimum.
pub fn axis_crossing(at_origin: bool, other: &AxisView) -> f64 {
    if !at_origin || other.log {
        return other.min;
    }
    0.0_f64.clamp(other.min, other.max)
}

/// One trace, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceView {
    /// `[r, g, b, a]`.
    pub color: [u8; 4],
    /// The legend name, when the trace has one.
    pub name: Option<String>,
    /// Stroke width in points.
    pub width: f32,
    pub dashed: bool,
    /// Dotted rather than dashed (`line-style` names a dot).
    pub dotted: bool,
    /// The marker drawn at each sample, when the trace has a symbol (see [`symbol_shape`]).
    pub symbol: Option<Symbol>,
    /// Points only (no lines), from the trace's mode.
    pub points_only: bool,
    /// Polylines of `[x, y]` in data units. A gap starts a new one.
    pub segments: Vec<Vec<[f64; 2]>>,
}

/// Everything the canvas draws for one plot.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotView {
    pub x: AxisView,
    pub y: AxisView,
    pub grid: bool,
    /// A polar plot: drawn with circular grid lines.
    pub polar: bool,
    /// The plot's legend flag (a `<legend>` element): the legend is drawn only when it is set.
    pub legend: bool,
    /// The plot title, drawn above the frame when present.
    pub title: Option<String>,
    pub traces: Vec<TraceView>,
    /// Marker lines: `(vertical, value)` in data units.
    pub markers: Vec<(bool, f64)>,
    /// Traces that could not be drawn, as a short text, when any.
    pub note: Option<String>,
}

/// Builds the drawing of an evaluated plot.
pub fn view_of(data: &PlotData) -> PlotView {
    let mut x = with_look(axis_view(data.x_range, data.log_x), &data.x_look);
    let mut y = with_look(axis_view(data.y_range, data.log_y), &data.y_look);
    if data.polar {
        // A polar plot has its own centre and rings: its axes stay at the frame.
        x.at_origin = false;
        y.at_origin = false;
    }
    let traces: Vec<TraceView> = data
        .traces
        .iter()
        .enumerate()
        .map(|(i, trace)| {
            let style = &trace.style;
            TraceView {
                color: style.color.unwrap_or(PALETTE[i % PALETTE.len()]),
                name: trace.name.clone(),
                width: style
                    .line_weight
                    .map_or(1.0, |w| (w as f32).clamp(0.5, 4.0)),
                dashed: style
                    .line_style
                    .as_deref()
                    .is_some_and(|s| s.to_ascii_lowercase().contains("dash")),
                dotted: style
                    .line_style
                    .as_deref()
                    .is_some_and(|s| s.to_ascii_lowercase().contains("dot")),
                symbol: style.symbol.as_deref().and_then(symbol_shape),
                points_only: style
                    .mode
                    .as_deref()
                    .is_some_and(|m| m.eq_ignore_ascii_case("points")),
                segments: trace
                    .segments
                    .iter()
                    .map(|seg| seg.iter().map(|&(a, b)| [a, b]).collect())
                    .collect(),
            }
        })
        .collect();
    let failed = data.traces.iter().filter(|t| t.error.is_some()).count();
    let note = (failed > 0).then(|| {
        trf(
            "plot.traces_not_drawn",
            &[&failed.to_string(), &data.traces.len().to_string()],
        )
    });
    PlotView {
        x,
        y,
        grid: data.grid,
        polar: data.polar,
        legend: data.legend,
        title: data.title.clone(),
        traces,
        markers: data.markers.iter().map(|m| (m.vertical, m.value)).collect(),
        note,
    }
}

/// The shape of a trace symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    Circle,
    Square,
    Diamond,
    Triangle,
    Cross,
    Star,
}

/// The shape for a `symbol` attribute. `none` (the only value the samples show) gives `None`. The
/// other names are guesses (`UNVERIFIED_FORMATS.md`).
pub fn symbol_shape(name: &str) -> Option<Symbol> {
    match name.to_ascii_lowercase().as_str() {
        "circle" => Some(Symbol::Circle),
        "square" => Some(Symbol::Square),
        "diamond" => Some(Symbol::Diamond),
        "triangle" => Some(Symbol::Triangle),
        "cross" | "x" => Some(Symbol::Cross),
        "star" => Some(Symbol::Star),
        _ => None,
    }
}

/// The legend of a plot: one entry per trace that has a name, in trace order. Empty when the plot
/// has no legend flag, or when no trace has a name; then no legend is drawn.
pub fn legend_entries(view: &PlotView) -> Vec<(&str, [u8; 4])> {
    if !view.legend {
        return Vec::new();
    }
    view.traces
        .iter()
        .filter_map(|t| t.name.as_deref().map(|name| (name, t.color)))
        .collect()
}

fn with_look(mut axis: AxisView, look: &mathcraft_import::regions::AxisLook) -> AxisView {
    axis.at_origin = look.at_origin;
    axis.tick_marks = look.tick_marks;
    axis.labels = look.grid_labels;
    axis.caption = look.label.clone();
    axis
}

/// What one legend row shows: the trace's name and a sample of its line and marker.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendItem {
    pub name: String,
    pub color: [u8; 4],
    pub width: f32,
    pub dashed: bool,
    pub dotted: bool,
    pub symbol: Option<Symbol>,
    /// Points only: the sample is the marker alone (a dot when the trace has no symbol).
    pub points_only: bool,
}

/// The legend rows of a plot: the named traces, in trace order, each with its drawing style.
/// Empty under the same rule as [`legend_entries`].
pub fn legend_items(view: &PlotView) -> Vec<LegendItem> {
    if !view.legend {
        return Vec::new();
    }
    view.traces
        .iter()
        .filter_map(|t| {
            Some(LegendItem {
                name: t.name.clone()?,
                color: t.color,
                width: t.width,
                dashed: t.dashed,
                dotted: t.dotted,
                symbol: t.symbol,
                points_only: t.points_only,
            })
        })
        .collect()
}

/// Where the legend box and its rows go, in the units of its inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendLayout {
    /// `[left, top, right, bottom]` of the box.
    pub rect: [f32; 4],
    /// Left and right end of every swatch.
    pub swatch_x: [f32; 2],
    /// Left edge of the names.
    pub text_x: f32,
    /// Vertical centre of each row.
    pub row_y: Vec<f32>,
}

/// The legend box in the top right corner of `plot` (`[left, top, right, bottom]`), `pad` inside
/// the frame, sized to the widest name. `text_widths` has one entry per row.
pub fn legend_layout(
    plot: [f32; 4],
    text_widths: &[f32],
    row: f32,
    pad: f32,
    swatch: f32,
    gap: f32,
) -> LegendLayout {
    let widest = text_widths.iter().copied().fold(0.0_f32, f32::max);
    let width = pad * 2.0 + swatch + gap + widest;
    let height = pad * 2.0 + row * text_widths.len() as f32;
    let right = plot[2] - pad;
    let left = right - width;
    let top = plot[1] + pad;
    LegendLayout {
        rect: [left, top, right, top + height],
        swatch_x: [left + pad, left + pad + swatch],
        text_x: left + pad + swatch + gap,
        row_y: (0..text_widths.len())
            .map(|i| top + pad + row * (i as f32 + 0.5))
            .collect(),
    }
}

/// The outline of a diamond or triangle marker of radius `r` around the origin (screen
/// coordinates, y down), as offsets from the marker centre. Other symbols are strokes or
/// circles, so they have no outline.
pub fn symbol_outline(symbol: Symbol, r: f32) -> Vec<[f32; 2]> {
    match symbol {
        Symbol::Diamond => vec![[0.0, -r], [r, 0.0], [0.0, r], [-r, 0.0]],
        Symbol::Triangle => vec![[0.0, -r], [r, r], [-r, r]],
        _ => Vec::new(),
    }
}

/// The axis drawing for a `(min, max)` range. A logarithmic axis gets its decades as ticks.
fn axis_view((min, max): (f64, f64), log: bool) -> AxisView {
    let ticks = if log {
        log_ticks(min, max)
    } else {
        nice_ticks(min, max, TARGET_TICKS)
    };
    let step = match ticks.as_slice() {
        [a, b, ..] => (b - a).abs(),
        _ => (max - min).abs(),
    };
    AxisView {
        min,
        max,
        log,
        decimals: decimals_for(step),
        ticks,
        at_origin: true,
        tick_marks: true,
        labels: true,
        caption: None,
    }
}

/// Round tick values for a range: multiples of 1, 2 or 5 times a power of ten, about `target` of
/// them. Empty when the range is not finite and increasing. Never more than [`MAX_TICKS`].
pub fn nice_ticks(min: f64, max: f64, target: usize) -> Vec<f64> {
    if !(min.is_finite() && max.is_finite() && min < max) {
        return Vec::new();
    }
    let raw = (max - min) / target.max(1) as f64;
    let magnitude = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * magnitude)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * magnitude);
    if !(step.is_finite() && step > 0.0) {
        return Vec::new();
    }
    let first = (min / step).ceil();
    let last = (max / step).floor();
    if !(first.is_finite() && last.is_finite()) || last < first {
        return Vec::new();
    }
    let count = (last - first) as usize + 1;
    if count > MAX_TICKS {
        return Vec::new();
    }
    (0..count)
        .map(|i| (first + i as f64) * step)
        .filter(|v| *v >= min && *v <= max)
        .collect()
}

/// Decades `10^k` inside a positive range, or the two ends when there is no decade inside.
pub fn log_ticks(min: f64, max: f64) -> Vec<f64> {
    if !(min.is_finite() && max.is_finite() && min > 0.0 && min < max) {
        return Vec::new();
    }
    let first = min.log10().ceil();
    let last = max.log10().floor();
    if !(first.is_finite() && last.is_finite()) || last < first {
        return vec![min, max];
    }
    let count = (last - first) as usize + 1;
    if count > MAX_TICKS {
        return Vec::new();
    }
    (0..count)
        .map(|i| 10f64.powf(first + i as f64))
        .filter(|v| *v >= min && *v <= max)
        .collect()
}

/// Decimal places that show a tick step: none for whole steps, more for small ones.
pub fn decimals_for(step: f64) -> usize {
    if !(step.is_finite() && step > 0.0) {
        return 0;
    }
    let places = (-step.log10().floor()).max(0.0);
    (places as usize).min(9)
}

/// The tick label of `value`, with `decimals` places and no trailing zeros. Large or tiny values
/// use exponent form.
pub fn format_tick(value: f64, decimals: usize) -> String {
    if !value.is_finite() {
        return String::new();
    }
    // Negative zero prints as "0", not "-0".
    let value = if value == 0.0 { 0.0 } else { value };
    if value != 0.0 && (value.abs() >= 1.0e6 || value.abs() < 1.0e-4) {
        return format!("{value:.2e}");
    }
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

/// The tick label as Prime prints it: every label of an axis has the same number of decimals
/// (`0.0`, `0.5`, `1.0`), so the labels line up. Tiny or huge values use exponent form.
pub fn format_axis_tick(value: f64, decimals: usize) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let value = if value == 0.0 { 0.0 } else { value };
    if value != 0.0 && (value.abs() >= 1.0e6 || value.abs() < 1.0e-6) {
        return format!("{value:.2e}");
    }
    format!("{value:.decimals$}")
}

/// The tick label of `value` on an axis: equal decimals on a linear axis; on a logarithmic axis
/// the decades are printed without trailing zeros (`10000`, `100`, `0.01`).
pub fn axis_label(axis: &AxisView, value: f64) -> String {
    if axis.log {
        format_tick(value, 9)
    } else {
        format_axis_tick(value, axis.decimals)
    }
}

/// The label of a polar plot spoke at `degrees` (`0°`, `30°`, ...).
pub fn polar_angle_label(degrees: i32) -> String {
    format!("{degrees}\u{b0}")
}

/// Space around the axes frame, in points at 100 % zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// Tick mark length, gap between tick mark and label, and outer padding, in points at 100 %.
pub const TICK_LEN: f32 = 4.0;
pub const LABEL_GAP: f32 = 3.0;
pub const OUTER_PAD: f32 = 4.0;

/// The margins that keep every label off the frame: the left margin holds the widest y label,
/// the right margin half of the last x label, the bottom margin the label height. All inputs
/// and the result are in the same unit (points at the current zoom); `scale` multiplies the
/// fixed paddings.
pub fn plot_margins(
    widest_y_label: f32,
    last_x_label: f32,
    label_height: f32,
    title_height: Option<f32>,
    scale: f32,
) -> Margins {
    let pad = OUTER_PAD * scale;
    let tick = (TICK_LEN + LABEL_GAP) * scale;
    Margins {
        left: pad + widest_y_label + tick,
        right: pad.max(last_x_label / 2.0 + pad / 2.0),
        top: pad + label_height / 2.0 + title_height.map_or(0.0, |h| h + 4.0 * scale),
        bottom: pad + tick + label_height,
    }
}

/// A coordinate axis: its range and whether it is logarithmic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub min: f64,
    pub max: f64,
    pub log: bool,
}

impl Scale {
    /// The position of `value` along the axis as a fraction (0 at `min`, 1 at `max`). `None` for a
    /// value that is not finite, or not positive on a logarithmic axis.
    pub fn fraction(&self, value: f64) -> Option<f64> {
        if !value.is_finite() {
            return None;
        }
        let (lo, hi, v) = if self.log {
            if self.min <= 0.0 || value <= 0.0 {
                return None;
            }
            (self.min.log10(), self.max.log10(), value.log10())
        } else {
            (self.min, self.max, value)
        };
        let span = hi - lo;
        if !(span.is_finite() && span > 0.0) {
            return None;
        }
        Some((v - lo) / span)
    }
}

/// The screen position of data point `p` inside `rect` (`[left, top, width, height]`), with the
/// vertical axis growing upwards. `None` when the point cannot be placed.
pub fn screen_point(x: Scale, y: Scale, rect: [f64; 4], p: [f64; 2]) -> Option<[f64; 2]> {
    let fx = x.fraction(p[0])?;
    let fy = y.fraction(p[1])?;
    let [left, top, width, height] = rect;
    Some([left + fx * width, top + (1.0 - fy) * height])
}

/// The scale of an axis view.
pub fn scale_of(axis: &AxisView) -> Scale {
    Scale {
        min: axis.min,
        max: axis.max,
        log: axis.log,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_with_names(names: &[Option<&str>]) -> PlotView {
        PlotView {
            x: axis_view((0.0, 1.0), false),
            y: axis_view((0.0, 1.0), false),
            grid: false,
            polar: false,
            title: None,
            legend: true,
            traces: names
                .iter()
                .map(|name| TraceView {
                    color: [0, 0, 0, 255],
                    name: name.map(str::to_owned),
                    width: 1.0,
                    dashed: false,
                    dotted: false,
                    symbol: None,
                    points_only: false,
                    segments: Vec::new(),
                })
                .collect(),
            markers: Vec::new(),
            note: None,
        }
    }

    #[test]
    fn the_legend_lists_only_the_named_traces_and_is_empty_without_names() {
        let none = view_with_names(&[None, None]);
        assert!(legend_entries(&none).is_empty());
        let some = view_with_names(&[Some("v(t)"), None, Some("a(t)")]);
        let entries: Vec<&str> = legend_entries(&some).iter().map(|(n, _)| *n).collect();
        assert_eq!(entries, vec!["v(t)", "a(t)"]);
    }

    #[test]
    fn no_legend_is_drawn_unless_the_plot_has_the_legend_flag() {
        let mut flagged_off = view_with_names(&[Some("v(t)")]);
        flagged_off.legend = false;
        assert!(legend_entries(&flagged_off).is_empty());
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn axes_cross_at_zero_inside_the_range_and_at_the_nearest_end_outside() {
        let mut other = axis_view((-2.0, 6.0), false);
        assert_eq!(axis_crossing(true, &other), 0.0);
        assert_eq!(axis_crossing(false, &other), -2.0);
        other = axis_view((1.0, 6.0), false);
        assert_eq!(axis_crossing(true, &other), 1.0);
        other = axis_view((-6.0, -1.0), false);
        assert_eq!(axis_crossing(true, &other), -1.0);
        other = axis_view((0.1, 100.0), true);
        assert_eq!(axis_crossing(true, &other), 0.1);
    }

    #[test]
    fn legend_rows_carry_the_trace_style() {
        let mut v = view_with_names(&[Some("a"), None, Some("b")]);
        v.traces[2].dashed = true;
        v.traces[2].symbol = Some(Symbol::Square);
        let items = legend_items(&v);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].name, "b");
        assert!(items[1].dashed);
        assert_eq!(items[1].symbol, Some(Symbol::Square));
        v.legend = false;
        assert!(legend_items(&v).is_empty());
    }

    #[test]
    fn legend_box_sits_top_right_and_fits_the_widest_name() {
        let l = legend_layout(
            [10.0, 20.0, 210.0, 120.0],
            &[30.0, 50.0],
            12.0,
            4.0,
            16.0,
            4.0,
        );
        // width 4+16+4+50+4 = 78, right edge 4 inside the frame
        assert_eq!(l.rect, [128.0, 24.0, 206.0, 24.0 + 8.0 + 24.0]);
        assert_eq!(l.swatch_x, [132.0, 148.0]);
        assert_eq!(l.text_x, 152.0);
        assert_eq!(l.row_y, vec![34.0, 46.0]);
    }

    #[test]
    fn diamond_and_triangle_outlines_are_centred_with_the_radius() {
        let d = symbol_outline(Symbol::Diamond, 3.0);
        assert_eq!(d.len(), 4);
        assert!(d
            .iter()
            .all(|p| (p[0].abs() + p[1].abs() - 3.0).abs() < 1e-6));
        let t = symbol_outline(Symbol::Triangle, 3.0);
        assert_eq!(t, vec![[0.0, -3.0], [3.0, 3.0], [-3.0, 3.0]]);
        assert!(symbol_outline(Symbol::Circle, 3.0).is_empty());
    }

    #[test]
    fn a_missing_look_shows_ticks_and_labels() {
        let v = axis_view((0.0, 1.0), false);
        assert!(v.tick_marks && v.labels && v.at_origin && v.caption.is_none());
    }

    #[test]
    fn log_axis_labels_have_no_trailing_zeros() {
        let log = axis_view((1.0, 10000.0), true);
        assert_eq!(axis_label(&log, 10000.0), "10000");
        assert_eq!(axis_label(&log, 100.0), "100");
        assert_eq!(axis_label(&log, 0.01), "0.01");
        let lin = axis_view((0.0, 1.0), false);
        assert_eq!(axis_label(&lin, 0.5), "0.5");
        assert_eq!(polar_angle_label(30), "30\u{b0}");
    }

    #[test]
    fn ticks_of_a_simple_range_are_round() {
        let unit = nice_ticks(0.0, 1.0, 5);
        assert_eq!(unit.len(), 6, "{unit:?}");
        for (v, want) in unit.iter().zip([0.0, 0.2, 0.4, 0.6, 0.8, 1.0]) {
            assert!(close(*v, want), "{v} != {want}");
        }
        let t = nice_ticks(-3.0, 8.0, 6);
        assert!(t.iter().all(|v| *v >= -3.0 && *v <= 8.0), "{t:?}");
        assert!(t.iter().any(|v| v.abs() < 1e-12), "zero is a tick: {t:?}");
        let step = t[1] - t[0];
        for w in t.windows(2) {
            assert!(close(w[1] - w[0], step), "equal steps: {t:?}");
        }
    }

    #[test]
    fn ticks_follow_the_range_scale() {
        let tiny = nice_ticks(0.0, 1.0e-6, 5);
        assert!(!tiny.is_empty() && tiny.iter().all(|v| *v >= 0.0 && *v <= 1.0e-6));
        let huge = nice_ticks(0.0, 1.0e10, 5);
        assert!(huge.len() >= 2 && huge.iter().all(|v| *v >= 0.0 && *v <= 1.0e10));
        let shifted = nice_ticks(1000.0, 1010.0, 5);
        assert!(
            shifted.iter().all(|v| *v >= 1000.0 && *v <= 1010.0),
            "{shifted:?}"
        );
    }

    #[test]
    fn degenerate_ranges_have_no_ticks() {
        assert!(nice_ticks(1.0, 1.0, 5).is_empty());
        assert!(nice_ticks(2.0, 1.0, 5).is_empty());
        assert!(nice_ticks(f64::NAN, 1.0, 5).is_empty());
        assert!(nice_ticks(0.0, f64::INFINITY, 5).is_empty());
        assert!(nice_ticks(0.0, 1.0, 0).len() <= MAX_TICKS);
    }

    #[test]
    fn ticks_are_capped() {
        // A range that would need thousands of ticks at this target gives none, not a flood.
        assert!(nice_ticks(0.0, 1.0e6, 1_000_000).len() <= MAX_TICKS);
        assert!(nice_ticks(0.0, 1.0e300, 3).len() <= MAX_TICKS);
        assert!(nice_ticks(-1.0e308, 1.0e308, 4).len() <= MAX_TICKS);
    }

    #[test]
    fn log_ticks_are_decades() {
        assert_eq!(log_ticks(0.1, 100.0), vec![0.1, 1.0, 10.0, 100.0]);
        assert_eq!(
            log_ticks(2.0, 5.0),
            vec![2.0, 5.0],
            "no decade inside: the ends"
        );
        assert!(log_ticks(0.0, 10.0).is_empty(), "zero has no logarithm");
        assert!(log_ticks(-1.0, 10.0).is_empty());
        assert!(log_ticks(1.0, 1.0e300).len() <= MAX_TICKS);
    }

    #[test]
    fn tick_labels_are_short() {
        assert_eq!(format_tick(2.5, 1), "2.5");
        assert_eq!(format_tick(3.0, 1), "3");
        assert_eq!(format_tick(-0.0, 0), "0");
        assert_eq!(format_tick(1.0e7, 0), "1.00e7");
        assert_eq!(format_tick(f64::NAN, 2), "");
        assert_eq!(decimals_for(0.2), 1);
        assert_eq!(decimals_for(5.0), 0);
        assert_eq!(decimals_for(0.0), 0);
    }

    #[test]
    fn axis_labels_have_equal_decimals() {
        assert_eq!(format_axis_tick(0.5, 1), "0.5");
        assert_eq!(format_axis_tick(1.0, 1), "1.0");
        assert_eq!(format_axis_tick(-0.0002, 4), "-0.0002");
        assert_eq!(format_axis_tick(0.0, 4), "0.0000");
        assert_eq!(format_axis_tick(2.0e6, 0), "2.00e6");
        assert_eq!(format_axis_tick(300000.0, 0), "300000");
    }

    #[test]
    fn small_negative_and_crossing_ticks() {
        let t = nice_ticks(-0.0006, 0.0002, 6);
        assert!(t.iter().any(|v| v.abs() < 1e-15), "{t:?}");
        let d = decimals_for(t[1] - t[0]);
        assert_eq!(d, 4, "{t:?}");
        let labels: Vec<String> = t.iter().map(|v| format_axis_tick(*v, d)).collect();
        assert!(labels.contains(&"-0.0002".to_string()), "{labels:?}");
        let big = nice_ticks(0.0, 250000.0, 6);
        assert!(big.len() >= 3 && big.iter().all(|v| *v <= 250000.0));
    }

    #[test]
    fn margins_reserve_label_room_at_any_zoom() {
        for zoom in [0.5_f32, 1.0, 2.0, 3.0] {
            let w = 40.0 * zoom;
            let m = plot_margins(w, 10.0 * zoom, 12.0 * zoom, None, zoom);
            assert!(
                m.left >= w + TICK_LEN * zoom,
                "left {} for label {w}",
                m.left
            );
            assert!(m.bottom >= 12.0 * zoom + TICK_LEN * zoom);
            assert!(m.right >= 5.0 * zoom, "half the last x label fits");
            assert!(m.top >= 6.0 * zoom, "half the top y label fits");
        }
        let titled = plot_margins(10.0, 10.0, 12.0, Some(16.0), 1.0);
        let plain = plot_margins(10.0, 10.0, 12.0, None, 1.0);
        assert!(titled.top > plain.top + 16.0);
    }

    #[test]
    fn linear_mapping_puts_the_ends_on_the_rectangle_corners() {
        let x = Scale {
            min: -2.0,
            max: 6.0,
            log: false,
        };
        let y = Scale {
            min: 0.0,
            max: 4.0,
            log: false,
        };
        let rect = [10.0, 20.0, 80.0, 40.0];
        assert_eq!(
            screen_point(x, y, rect, [-2.0, 0.0]),
            Some([10.0, 60.0]),
            "bottom left"
        );
        assert_eq!(
            screen_point(x, y, rect, [6.0, 4.0]),
            Some([90.0, 20.0]),
            "top right"
        );
        assert_eq!(
            screen_point(x, y, rect, [2.0, 2.0]),
            Some([50.0, 40.0]),
            "centre"
        );
    }

    #[test]
    fn logarithmic_mapping_is_by_decade() {
        let x = Scale {
            min: 1.0,
            max: 100.0,
            log: true,
        };
        assert!(close(x.fraction(10.0).unwrap(), 0.5));
        assert!(close(x.fraction(1.0).unwrap(), 0.0));
        assert!(x.fraction(0.0).is_none(), "zero is off a log axis");
        assert!(x.fraction(-5.0).is_none());
        let bad = Scale {
            min: 0.0,
            max: 100.0,
            log: true,
        };
        assert!(
            bad.fraction(5.0).is_none(),
            "a log axis needs a positive minimum"
        );
    }

    #[test]
    fn unplaceable_points_are_none() {
        let s = Scale {
            min: 0.0,
            max: 1.0,
            log: false,
        };
        assert!(s.fraction(f64::NAN).is_none());
        assert!(s.fraction(f64::INFINITY).is_none());
        let flat = Scale {
            min: 1.0,
            max: 1.0,
            log: false,
        };
        assert!(flat.fraction(1.0).is_none(), "a zero span places nothing");
        assert!(screen_point(s, flat, [0.0, 0.0, 1.0, 1.0], [0.5, 1.0]).is_none());
    }
}

#[cfg(test)]
mod style_tests {
    use super::*;

    #[test]
    fn default_palette_starts_with_teal_and_has_distinct_colours() {
        assert_eq!(PALETTE[0], [0x1F, 0x4E, 0x5A, 0xFF]);
        let distinct: std::collections::HashSet<_> = PALETTE.iter().collect();
        assert_eq!(distinct.len(), PALETTE.len());
    }

    #[test]
    fn arrow_head_points_along_the_axis() {
        let [tip, a, b] = arrow_head([10.0, 0.0], [1.0, 0.0], 6.0);
        assert_eq!(tip, [10.0, 0.0]);
        assert!((a[0] - 4.0).abs() < 1e-6 && (b[0] - 4.0).abs() < 1e-6);
        assert!((a[1] + b[1]).abs() < 1e-6 && a[1].abs() > 1.0);
    }

    #[test]
    fn log_minor_ticks_fill_the_decades() {
        let t = log_minor_ticks(1.0, 100.0);
        assert_eq!(t.len(), 16);
        assert!(t.contains(&2.0) && t.contains(&90.0) && !t.contains(&10.0));
        assert!(log_minor_ticks(0.0, 10.0).is_empty());
        let part = log_minor_ticks(3.0, 30.0);
        assert!(part.iter().all(|v| *v > 3.0 && *v < 30.0));
    }
}

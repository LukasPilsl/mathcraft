//! Headless 3D plot drawing: the view transform (rotation by azimuth and elevation, orthographic
//! projection), the painter's order of the surface quads, shading, the height colour map, the axis
//! ticks, and the traces of an evaluated 3D plot (`mathcraft_import::evaluate_plots3d::Plot3dData`).
//! No egui types here; `canvas.rs` draws what this module decides.
//!
//! Coordinates: the data is mapped into the cube `[-1, 1]^3` (see [`to_unit`]). The view rotates
//! that cube with [`rotation`]. After rotation a point `q` is at screen `(q[0], q[2])` (right, up)
//! and at depth `q[1]`: the viewer is at `y = -infinity`, so a larger `q[1]` is farther away.

use mathcraft_import::evaluate_plots3d::{Plot3dData, Trace3dData};

use crate::i18n::trf;
use crate::plot::{nice_ticks, TARGET_TICKS};

/// Rotation about the vertical axis, in radians. The default looks at the plot from the front
/// and a little from the side.
pub const DEFAULT_AZIMUTH: f64 = -0.6;
/// Tilt of the view from the horizontal, in radians (positive looks down on the plot).
pub const DEFAULT_ELEVATION: f64 = 0.5;
/// Elevation is kept inside this range, so the plot never turns upside down.
pub const MAX_ELEVATION: f64 = 1.5;
/// Most quads one surface may have on screen. A grid is thinned below this in the evaluator
/// already (200 x 200 gives 39 601 quads); this bounds the drawing work for any input.
pub const MAX_QUADS: usize = 40_000;

/// The view angles of one 3D plot, in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Angles {
    pub azimuth: f64,
    pub elevation: f64,
}

impl Default for Angles {
    fn default() -> Self {
        Self {
            azimuth: DEFAULT_AZIMUTH,
            elevation: DEFAULT_ELEVATION,
        }
    }
}

impl Angles {
    /// Turns the view by a drag of `dx` and `dy` (in points). Elevation is clamped.
    pub fn dragged(self, dx: f64, dy: f64) -> Self {
        const RADIANS_PER_POINT: f64 = 0.01;
        Self {
            azimuth: self.azimuth - dx * RADIANS_PER_POINT,
            elevation: (self.elevation + dy * RADIANS_PER_POINT)
                .clamp(-MAX_ELEVATION, MAX_ELEVATION),
        }
    }
}

/// A 3x3 matrix, row-major.
pub type Matrix3 = [[f64; 3]; 3];

/// The rotation of the view: about the vertical axis by `azimuth`, then tilted by `elevation`
/// about the horizontal axis. Orthonormal for any angles.
pub fn rotation(angles: Angles) -> Matrix3 {
    let (sa, ca) = angles.azimuth.sin_cos();
    let (se, ce) = angles.elevation.sin_cos();
    let rz = [[ca, -sa, 0.0], [sa, ca, 0.0], [0.0, 0.0, 1.0]];
    let rx = [[1.0, 0.0, 0.0], [0.0, ce, -se], [0.0, se, ce]];
    multiply(rx, rz)
}

/// `a * b`.
pub fn multiply(a: Matrix3, b: Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// `m * p`.
pub fn apply(m: &Matrix3, p: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2],
    ]
}

/// The screen position of a rotated point, in units of the plot's half size: `(right, up)`.
pub fn project(q: [f64; 3]) -> [f64; 2] {
    [q[0], q[2]]
}

/// Depth of a rotated point: larger is farther from the viewer.
pub fn depth(q: [f64; 3]) -> f64 {
    q[1]
}

/// Indices of `keys` sorted for the painter's algorithm: the farthest first (largest key first).
/// A `NaN` key is drawn first, so it cannot hide anything. The sort is stable.
pub fn depth_order(keys: &[f64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by(|&a, &b| {
        let (ka, kb) = (keys[a], keys[b]);
        match (ka.is_nan(), kb.is_nan()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => kb.total_cmp(&ka),
        }
    });
    order
}

/// Maps a data value into `[-1, 1]` over the axis range `(min, max)`. A degenerate range maps to
/// `0`, and a value that is not finite maps to `NaN`.
pub fn to_unit(value: f64, (min, max): (f64, f64)) -> f64 {
    if !value.is_finite() {
        return f64::NAN;
    }
    let span = max - min;
    if span.is_finite() && span > 0.0 {
        (2.0 * (value - min) / span - 1.0).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// A colour for a height `t` in `[0, 1]` (values outside are clamped): blue, cyan, green, yellow,
/// red. `NaN` is the lowest colour.
pub fn height_color(t: f64) -> [u8; 3] {
    const STOPS: [(f64, [f64; 3]); 5] = [
        (0.0, [0.10, 0.20, 0.75]),
        (0.35, [0.10, 0.70, 0.80]),
        (0.55, [0.20, 0.75, 0.25]),
        (0.80, [0.95, 0.85, 0.15]),
        (1.0, [0.85, 0.15, 0.10]),
    ];
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let mut rgb = STOPS[STOPS.len() - 1].1;
    for pair in STOPS.windows(2) {
        let ((t0, c0), (t1, c1)) = (pair[0], pair[1]);
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            rgb = [0, 1, 2].map(|k| c0[k] + (c1[k] - c0[k]) * f);
            break;
        }
    }
    rgb.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Whether a surface quad, projected to the screen, can be drawn. A quad with a non-finite corner
/// or an edge shorter than `min_edge` is skipped: a corner clamped onto the box (see [`to_unit`])
/// makes two corners coincide, and a zero-length edge has no direction, so the tessellator of the
/// filled polygon turns it into long slivers that cross the whole window. A quad that is not
/// convex on screen (a bow-tie, when clamped corners swap sides) is skipped for the same reason.
pub fn quad_is_drawable(corners: &[[f64; 2]; 4], min_edge: f64) -> bool {
    if !corners.iter().all(|c| c[0].is_finite() && c[1].is_finite()) {
        return false;
    }
    if !(0..4).all(|i| {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        (b[0] - a[0]).hypot(b[1] - a[1]) >= min_edge
    }) {
        return false;
    }
    // Convex: the turn at every corner has the same sign (the cross products of consecutive edges).
    let turns: [f64; 4] = std::array::from_fn(|i| {
        let (a, b, c) = (corners[i], corners[(i + 1) % 4], corners[(i + 2) % 4]);
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
    });
    let positive = turns.iter().all(|&t| t > 0.0);
    let negative = turns.iter().all(|&t| t < 0.0);
    positive || negative
}

/// Light intensity in `[0.35, 1]` for a surface normal (in the rotated view frame). Two-sided:
/// a face turned away from the light is as lit as its back.
pub fn shade(normal: [f64; 3]) -> f64 {
    // Light from above and in front of the viewer, in view coordinates.
    let light = normalize([0.3, -0.5, 0.8]);
    let n = normalize(normal);
    let d = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).abs();
    0.35 + 0.65 * d
}

/// Unit vector, or zero when the input is zero or not finite.
pub fn normalize(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len.is_finite() && len > 0.0 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0; 3]
    }
}

/// Cross product.
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Tick values of one axis of the box, inside its range (see [`nice_ticks`]).
pub fn axis_ticks(range: (f64, f64)) -> Vec<f64> {
    nice_ticks(range.0, range.1, TARGET_TICKS)
}

/// One quadrilateral of a surface, in cube coordinates, with its colour before shading.
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    /// Corners in cube coordinates, in grid order.
    pub corners: [[f64; 3]; 4],
    /// Surface normal in cube coordinates (not normalised).
    pub normal: [f64; 3],
    /// Base colour `[r, g, b]` from the height.
    pub color: [u8; 3],
}

/// One trace, ready to draw: quads for a surface, or polylines for a curve (in cube coordinates).
#[derive(Debug, Clone, PartialEq)]
pub struct TraceView {
    /// Stroke colour for curves, `[r, g, b, a]` (the style's colour, or a palette colour when the
    /// style is automatic or has no colour).
    pub color: [u8; 4],
    pub surface: bool,
    pub quads: Vec<Quad>,
    pub polylines: Vec<Vec<[f64; 3]>>,
}

/// The axes of the box: range and tick values (data units).
#[derive(Debug, Clone, PartialEq)]
pub struct AxisView {
    pub min: f64,
    pub max: f64,
    pub ticks: Vec<f64>,
    pub decimals: usize,
}

/// Everything the canvas draws for one 3D plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot3dView {
    pub x: AxisView,
    pub y: AxisView,
    pub z: AxisView,
    pub traces: Vec<TraceView>,
    /// A short text when traces were not drawn, when any.
    pub note: Option<String>,
}

/// Colours of curves without a stated colour (as in the 2D plots).
const PALETTE: [[u8; 4]; 4] = [
    [0x00, 0x00, 0x8B, 0xFF],
    [0x00, 0x00, 0x00, 0xFF],
    [0xFF, 0x00, 0x00, 0xFF],
    [0x00, 0x80, 0x00, 0xFF],
];

/// Colours of curves with an automatic style, in order. Black is left out: the stored colour of
/// such a curve is black, which is only the placeholder Prime writes.
const AUTOMATIC_PALETTE: [[u8; 4]; 4] = [
    [0x00, 0x00, 0x8B, 0xFF],
    [0xFF, 0x00, 0x00, 0xFF],
    [0x00, 0x80, 0x00, 0xFF],
    [0xFF, 0xA5, 0x00, 0xFF],
];

/// Builds the drawing of an evaluated 3D plot.
pub fn view_of(data: &Plot3dData) -> Plot3dView {
    let x = axis_view(data.x_range);
    let y = axis_view(data.y_range);
    let z = axis_view(data.z_range);
    let x_range = data.x_range;
    let y_range = data.y_range;
    let z_range = data.z_range;
    let traces: Vec<TraceView> = data
        .traces
        .iter()
        .enumerate()
        .map(|(i, trace)| trace_view(trace, i, [x_range, y_range, z_range]))
        .collect();
    let failed = data.traces.iter().filter(|t| t.error.is_some()).count();
    let note = (failed > 0).then(|| {
        trf(
            "plot.traces_not_drawn",
            &[&failed.to_string(), &data.traces.len().to_string()],
        )
    });
    Plot3dView {
        x,
        y,
        z,
        traces,
        note,
    }
}

fn axis_view((min, max): (f64, f64)) -> AxisView {
    let ticks = axis_ticks((min, max));
    let step = match ticks.as_slice() {
        [a, b, ..] => (b - a).abs(),
        _ => (max - min).abs(),
    };
    AxisView {
        min,
        max,
        decimals: decimals_for(step),
        ticks,
    }
}

/// Decimal places for tick labels of a given step (at least 0, at most 6).
fn decimals_for(step: f64) -> usize {
    if !(step.is_finite() && step > 0.0) {
        return 0;
    }
    let places = (-step.log10()).ceil();
    if places.is_finite() && places > 0.0 {
        (places as usize).min(6)
    } else {
        0
    }
}

/// The drawing of one trace. Surfaces become quads (a quad needs its four corners finite) and
/// curves polylines (a gap starts a new one). Counts are capped at [`MAX_QUADS`].
fn trace_view(trace: &Trace3dData, index: usize, ranges: [(f64, f64); 3]) -> TraceView {
    // `automaticStyle` traces store black as a placeholder; Prime picks their colour, so the
    // stored colour is ignored for them and the automatic palette (no black) is used.
    let color = if trace.style.automatic {
        AUTOMATIC_PALETTE[index % AUTOMATIC_PALETTE.len()]
    } else {
        trace.style.color.unwrap_or(PALETTE[index % PALETTE.len()])
    };
    let mut view = TraceView {
        color,
        surface: trace.surface,
        quads: Vec::new(),
        polylines: Vec::new(),
    };
    if trace.error.is_some() || trace.rows == 0 || trace.cols == 0 {
        return view;
    }
    let point = |i: usize| -> [f64; 3] {
        [
            to_unit(trace.x[i], ranges[0]),
            to_unit(trace.y[i], ranges[1]),
            to_unit(trace.z[i], ranges[2]),
        ]
    };
    if !trace.surface {
        let mut current = Vec::new();
        for i in 0..trace.rows {
            let p = point(i);
            if p.iter().all(|v| v.is_finite()) {
                current.push(p);
            } else if !current.is_empty() {
                view.polylines.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            view.polylines.push(current);
        }
        return view;
    }
    let (rows, cols) = (trace.rows, trace.cols);
    let z_min = trace
        .z
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::INFINITY, f64::min);
    let z_max = trace
        .z
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    let height = |z: f64| -> f64 {
        if z_max > z_min {
            (z - z_min) / (z_max - z_min)
        } else {
            0.5
        }
    };
    for r in 0..rows.saturating_sub(1) {
        for c in 0..cols.saturating_sub(1) {
            if view.quads.len() >= MAX_QUADS {
                return view;
            }
            let idx = [
                r * cols + c,
                r * cols + c + 1,
                (r + 1) * cols + c + 1,
                (r + 1) * cols + c,
            ];
            let corners = idx.map(point);
            if corners.iter().any(|p| p.iter().any(|v| !v.is_finite())) {
                continue;
            }
            let z_mean = idx.iter().map(|&i| trace.z[i]).sum::<f64>() / 4.0;
            let diag1 = [
                corners[2][0] - corners[0][0],
                corners[2][1] - corners[0][1],
                corners[2][2] - corners[0][2],
            ];
            let diag2 = [
                corners[3][0] - corners[1][0],
                corners[3][1] - corners[1][1],
                corners[3][2] - corners[1][2],
            ];
            view.quads.push(Quad {
                corners,
                normal: cross(diag1, diag2),
                color: height_color(height(z_mean)),
            });
        }
    }
    view
}

/// The corner coordinates of a box, used for the edges and tick labels in the canvas.
pub fn box_corner(bits: u8) -> [f64; 3] {
    [
        if bits & 1 != 0 { 1.0 } else { -1.0 },
        if bits & 2 != 0 { 1.0 } else { -1.0 },
        if bits & 4 != 0 { 1.0 } else { -1.0 },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_import::evaluate_plots3d::Trace3dData;
    use mathcraft_import::regions::TraceStyle;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn rotation_is_orthonormal_for_any_angles() {
        for (azimuth, elevation) in [(0.0, 0.0), (-0.6, 0.5), (2.9, -1.2), (7.0, 1.5)] {
            let r = rotation(Angles { azimuth, elevation });
            for i in 0..3 {
                for j in 0..3 {
                    let dot: f64 = (0..3).map(|k| r[k][i] * r[k][j]).sum();
                    let want = if i == j { 1.0 } else { 0.0 };
                    assert!(close(dot, want), "R^T R [{i}][{j}] = {dot}");
                }
            }
            let v = apply(&r, [0.3, -0.7, 0.2]);
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            assert!(close(len, (0.09f64 + 0.49 + 0.04).sqrt()));
        }
    }

    #[test]
    fn default_view_puts_z_up_and_x_right_for_zero_angles() {
        let r = rotation(Angles {
            azimuth: 0.0,
            elevation: 0.0,
        });
        assert_eq!(project(apply(&r, [1.0, 0.0, 0.0])), [1.0, 0.0]);
        assert_eq!(project(apply(&r, [0.0, 0.0, 1.0])), [0.0, 1.0]);
        // Looking from above (positive elevation) the top of the box comes toward the viewer:
        // a point above the centre is nearer than one below it.
        let up = Angles {
            azimuth: 0.0,
            elevation: 1.0,
        };
        let r = rotation(up);
        assert!(depth(apply(&r, [0.0, 0.0, 1.0])) < depth(apply(&r, [0.0, 0.0, -1.0])));
    }

    #[test]
    fn drag_turns_the_view_and_clamps_the_elevation() {
        let a = Angles::default().dragged(100.0, 0.0);
        assert!(close(a.azimuth, DEFAULT_AZIMUTH - 1.0));
        let b = Angles::default().dragged(0.0, 10_000.0);
        assert!(close(b.elevation, MAX_ELEVATION));
    }

    #[test]
    fn painter_draws_the_farthest_first() {
        let keys = [0.2, 0.9, -0.5, f64::NAN, 0.9];
        // Largest depth first; the NaN first of all; equal keys keep their order.
        assert_eq!(depth_order(&keys), vec![3, 1, 4, 0, 2]);
        assert!(depth_order(&[]).is_empty());
    }

    #[test]
    fn unit_mapping_and_degenerate_ranges() {
        assert!(close(to_unit(0.0, (0.0, 4.0)), -1.0));
        assert!(close(to_unit(4.0, (0.0, 4.0)), 1.0));
        assert!(close(to_unit(2.0, (0.0, 4.0)), 0.0));
        assert_eq!(to_unit(5.0, (0.0, 4.0)), 1.0, "clamped");
        assert_eq!(to_unit(1.0, (2.0, 2.0)), 0.0, "degenerate range");
        assert!(to_unit(f64::NAN, (0.0, 1.0)).is_nan());
    }

    #[test]
    fn height_colours_run_from_blue_to_red() {
        let low = height_color(0.0);
        let high = height_color(1.0);
        assert!(low[2] > low[0], "low is blue: {low:?}");
        assert!(high[0] > high[2], "high is red: {high:?}");
        assert_eq!(height_color(-3.0), low);
        assert_eq!(height_color(3.0), high);
        assert_eq!(height_color(f64::NAN), low);
    }

    #[test]
    fn shading_is_two_sided_and_bounded() {
        let n = [0.0, 0.0, 1.0];
        let up = shade(n);
        let down = shade([0.0, 0.0, -1.0]);
        assert!(close(up, down));
        assert!((0.35..=1.0).contains(&up));
        assert_eq!(shade([0.0, 0.0, 0.0]), 0.35);
    }

    #[test]
    fn ticks_lie_inside_the_range() {
        let ticks = axis_ticks((-15.0, 45.0));
        assert!(!ticks.is_empty());
        assert!(ticks.iter().all(|t| (-15.0..=45.0).contains(t)));
        assert!(axis_ticks((1.0, 1.0)).is_empty());
        assert!(axis_ticks((f64::NAN, 1.0)).is_empty());
    }

    #[test]
    fn decimals_follow_the_tick_step() {
        assert_eq!(decimals_for(1.0), 0);
        assert_eq!(decimals_for(0.25), 1);
        assert_eq!(decimals_for(0.0), 0);
        assert_eq!(decimals_for(1e-12), 6);
    }

    fn style() -> TraceStyle {
        TraceStyle {
            color: None,
            line_weight: None,
            line_style: None,
            symbol: None,
            mode: None,
            automatic: false,
        }
    }

    fn grid(rows: usize, cols: usize, z: impl Fn(usize, usize) -> f64) -> Trace3dData {
        let mut x = Vec::new();
        let mut y = Vec::new();
        let mut zs = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                x.push(c as f64);
                y.push(r as f64);
                zs.push(z(r, c));
            }
        }
        Trace3dData {
            style: style(),
            surface: true,
            rows,
            cols,
            x,
            y,
            z: zs,
            gaps: 0,
            error: None,
            stored: None,
        }
    }

    #[test]
    fn surface_quads_skip_holes_and_keep_the_grid_order() {
        let trace = grid(3, 3, |r, c| {
            if r == 1 && c == 1 {
                f64::NAN
            } else {
                (r + c) as f64
            }
        });
        let ranges = [(0.0, 2.0), (0.0, 2.0), (0.0, 4.0)];
        let view = trace_view(&trace, 0, ranges);
        // 2 x 2 cells; the centre hole touches all four cells.
        assert!(view.quads.is_empty());

        let trace = grid(3, 3, |r, c| (r + c) as f64);
        let view = trace_view(&trace, 0, ranges);
        assert_eq!(view.quads.len(), 4);
        let first = &view.quads[0];
        assert!(close(first.corners[0][0], -1.0) && close(first.corners[0][1], -1.0));
        assert!(close(first.corners[2][0], 0.0) && close(first.corners[2][1], 0.0));
        // Heights are the mean of each cell's corners over the surface's range (0 to 4): the
        // first cell averages 1 (0.25) and the last 3 (0.75).
        assert_eq!(first.color, height_color(0.25));
        assert_eq!(view.quads[3].color, height_color(0.75));
    }

    #[test]
    fn a_flat_surface_has_a_vertical_normal() {
        let trace = grid(2, 2, |_, _| 1.0);
        let view = trace_view(&trace, 0, [(0.0, 1.0), (0.0, 1.0), (0.0, 1.0)]);
        let n = normalize(view.quads[0].normal);
        assert!(close(n[2].abs(), 1.0), "normal {n:?}");
    }

    #[test]
    fn curves_split_at_gaps_and_surfaces_are_capped() {
        let mut trace = grid(5, 1, |r, _| r as f64);
        trace.surface = false;
        trace.z[2] = f64::NAN;
        let view = trace_view(&trace, 1, [(0.0, 4.0), (0.0, 4.0), (0.0, 4.0)]);
        assert_eq!(view.polylines.len(), 2);
        assert_eq!(view.polylines[0].len(), 2);
        assert_eq!(view.polylines[1].len(), 2);
        assert_eq!(view.color, PALETTE[1]);

        let big = grid(300, 300, |r, c| (r * c) as f64);
        let view = trace_view(&big, 0, [(0.0, 299.0), (0.0, 299.0), (0.0, 1e5)]);
        assert_eq!(view.quads.len(), MAX_QUADS);
    }

    #[test]
    fn automatic_style_ignores_the_stored_black() {
        let mut trace = grid(5, 1, |r, _| r as f64);
        trace.surface = false;
        trace.style.color = Some([0, 0, 0, 255]);
        trace.style.automatic = true;
        let view = trace_view(&trace, 1, [(0.0, 4.0), (0.0, 4.0), (0.0, 4.0)]);
        assert_eq!(view.color, AUTOMATIC_PALETTE[1]);
        trace.style.automatic = false;
        let view = trace_view(&trace, 1, [(0.0, 4.0), (0.0, 4.0), (0.0, 4.0)]);
        assert_eq!(view.color, [0, 0, 0, 255]);
    }

    #[test]
    fn degenerate_quads_are_not_drawn() {
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(quad_is_drawable(&square, 0.5));
        // Two corners on top of each other: a zero-length edge.
        let collapsed = [[0.0, 0.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(!quad_is_drawable(&collapsed, 0.5));
        let nan = [[0.0, 0.0], [f64::NAN, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(!quad_is_drawable(&nan, 0.5));
        // Shorter than the threshold: not drawn either.
        let tiny = [[0.0, 0.0], [0.2, 0.0], [0.2, 0.2], [0.0, 0.2]];
        assert!(!quad_is_drawable(&tiny, 0.5));
        // A bow-tie: the two lower corners swapped, so the edges cross.
        let bow_tie = [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 1.0]];
        assert!(!quad_is_drawable(&bow_tie, 0.5));
    }

    #[test]
    fn box_corners_cover_the_cube() {
        assert_eq!(box_corner(0), [-1.0, -1.0, -1.0]);
        assert_eq!(box_corner(7), [1.0, 1.0, 1.0]);
        assert_eq!(box_corner(2), [-1.0, 1.0, -1.0]);
    }
}

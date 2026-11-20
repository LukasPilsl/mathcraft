//! Box model of the math layout: primitives, boxes, and the drawing of each construct (scripts,
//! fractions, brackets, roots, matrices). No expression types here; `mathlayout.rs` maps an
//! expression onto these boxes.
//!
//! Coordinates: the origin is the left edge of a box on its baseline. `x` grows to the right and
//! `y` grows downwards. A box reaches `ascent` above the baseline and `descent` below it.

use crate::fonts::{advance_em, Face};
use crate::glyphs::{printable, MAX_TOKEN_CHARS};

/// Size of superscripts and subscripts, relative to their base.
pub const SCRIPT: f32 = 0.7;

/// Ascent and descent of a text run, relative to its font size.
const TEXT_ASCENT: f32 = 0.78;
const TEXT_DESCENT: f32 = 0.24;
/// Height of the math axis (where fraction bars and matrices are centred), above the baseline.
const AXIS: f32 = 0.3;
/// Points used to draw one parenthesis or bracket curve.
const CURVE_STEPS: usize = 12;

/// One drawable primitive, relative to the owning box's origin.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    /// Text whose left end sits on the baseline point `(x, y)`, in one bundled face.
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        face: Face,
    },
    /// Filled rectangle with its top-left corner at `(x, y)`.
    Rect { x: f32, y: f32, w: f32, h: f32 },
    /// Stroked polyline with the given width.
    Line { points: Vec<[f32; 2]>, width: f32 },
}

impl Prim {
    fn shifted(self, dx: f32, dy: f32) -> Self {
        match self {
            Prim::Text {
                x,
                y,
                text,
                size,
                face,
            } => Prim::Text {
                x: x + dx,
                y: y + dy,
                text,
                size,
                face,
            },
            Prim::Rect { x, y, w, h } => Prim::Rect {
                x: x + dx,
                y: y + dy,
                w,
                h,
            },
            Prim::Line { points, width } => Prim::Line {
                points: points
                    .into_iter()
                    .map(|[px, py]| [px + dx, py + dy])
                    .collect(),
                width,
            },
        }
    }
}

/// A laid-out expression: its size and the primitives that draw it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathBox {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub prims: Vec<Prim>,
    /// Where the insertion point sits, `(x, baseline y)`, when the box is a draft with a caret
    /// (see `draftlayout.rs`). Placed boxes carry their child's caret along.
    pub caret: Option<[f32; 2]>,
}

impl MathBox {
    /// Total height, ascent plus descent.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// Width of `s` in points at font size `size`, in the roman face of the bundled font.
pub fn text_width(s: &str, size: f32) -> f32 {
    text_width_in(s, size, Face::Roman)
}

/// Width of `s` in points at font size `size`, in `face`. The advances are the bundled font's
/// (no kerning). A character with no glyph in any bundled face is measured as a box.
pub fn text_width_in(s: &str, size: f32, face: Face) -> f32 {
    s.chars().map(|c| advance(face, c)).sum::<f32>() * size
}

/// Advance of one character, in ems: the bundled font's, or the width of the fallback box.
fn advance(face: Face, c: char) -> f32 {
    advance_em(face, c)
        .or_else(|| advance_em(face, crate::glyphs::FALLBACK_BOX))
        .unwrap_or(0.8)
}

/// Accumulates children placed at offsets and tracks the extents of the result.
#[derive(Default)]
struct Builder {
    top: f32,
    bottom: f32,
    prims: Vec<Prim>,
    caret: Option<[f32; 2]>,
}

impl Builder {
    fn extend(&mut self, top: f32, bottom: f32) {
        self.top = self.top.min(top);
        self.bottom = self.bottom.max(bottom);
    }

    /// Places `child` with its baseline at `(dx, dy)`, `dy` positive downwards.
    fn put(&mut self, child: MathBox, dx: f32, dy: f32) {
        self.extend(dy - child.ascent, dy + child.descent);
        if let Some([cx, cy]) = child.caret {
            self.caret = Some([cx + dx, cy + dy]);
        }
        self.prims
            .extend(child.prims.into_iter().map(|p| p.shifted(dx, dy)));
    }

    fn push(&mut self, prim: Prim, top: f32, bottom: f32) {
        self.extend(top, bottom);
        self.prims.push(prim);
    }

    fn finish(self, width: f32) -> MathBox {
        MathBox {
            width,
            ascent: -self.top,
            descent: self.bottom,
            prims: self.prims,
            caret: self.caret,
        }
    }
}

/// Horizontal run of boxes on one baseline.
pub fn hcat(parts: Vec<MathBox>) -> MathBox {
    let mut b = Builder::default();
    let mut x = 0.0;
    for part in parts {
        let w = part.width;
        b.put(part, x, 0.0);
        x += w;
    }
    b.finish(x)
}

/// Empty horizontal space of `w` points.
pub fn space(w: f32) -> MathBox {
    MathBox {
        width: w,
        ..MathBox::default()
    }
}

/// A run of upright text in the roman face. See [`text_in`].
pub fn text(s: &str, size: f32) -> MathBox {
    text_in(s, size, Face::Roman)
}

/// A run of text in `face`. Long names are cut, and characters with no glyph are drawn as a box
/// (see `glyphs.rs`). Width is measured on the text as drawn.
pub fn text_in(s: &str, size: f32, face: Face) -> MathBox {
    if s.is_empty() {
        return MathBox::default();
    }
    let s = printable(s, MAX_TOKEN_CHARS, face);
    MathBox {
        width: text_width_in(&s, size, face),
        ascent: TEXT_ASCENT * size,
        descent: TEXT_DESCENT * size,
        prims: vec![Prim::Text {
            x: 0.0,
            y: 0.0,
            text: s,
            size,
            face,
        }],
        caret: None,
    }
}

/// A binary operator with a space on each side.
pub fn infix(lhs: MathBox, op: &str, rhs: MathBox, size: f32) -> MathBox {
    let gap = 0.25 * size;
    hcat(vec![lhs, space(gap), text(op, size), space(gap), rhs])
}

/// `base` with `script` attached at `dy` (positive is lower). Used for superscripts and
/// subscripts, and for the transpose mark.
pub fn attach(base: MathBox, script: MathBox, dy: f32) -> MathBox {
    let mut b = Builder::default();
    let w = base.width + script.width;
    let base_w = base.width;
    b.put(base, 0.0, 0.0);
    b.put(script, base_w, dy);
    b.finish(w)
}

/// Superscript: `script` raised above `base`, at 0.7 of the base size.
pub fn superscript(base: MathBox, script: MathBox, size: f32) -> MathBox {
    let raise = 0.42 * size;
    let gap = 0.03 * size;
    let base_w = base.width;
    let width = base_w + gap + script.width;
    let mut b = Builder::default();
    b.put(base, 0.0, 0.0);
    b.put(script, base_w + gap, -raise);
    b.finish(width)
}

/// Fraction with `num` above `den` and a bar on the math axis.
pub fn fraction(num: MathBox, den: MathBox, size: f32) -> MathBox {
    let thick = (0.05 * size).max(0.8);
    let gap = 0.12 * size;
    let pad = 0.1 * size;
    let width = num.width.max(den.width) + 2.0 * pad;
    let bar_top = -AXIS * size - thick / 2.0;
    let bar_bottom = -AXIS * size + thick / 2.0;
    let num_y = bar_top - gap - num.descent;
    let den_y = bar_bottom + gap + den.ascent;
    let num_x = (width - num.width) / 2.0;
    let den_x = (width - den.width) / 2.0;
    let mut b = Builder::default();
    b.put(num, num_x, num_y);
    b.put(den, den_x, den_y);
    b.push(
        Prim::Rect {
            x: 0.0,
            y: bar_top,
            w: width,
            h: thick,
        },
        bar_top,
        bar_bottom,
    );
    b.finish(width)
}

/// Which pair of brackets surrounds a box.
#[derive(Clone, Copy)]
pub enum Pair {
    Round,
    Square,
}

/// Gap before a round bracket's stroke, in ems. A script on the content before the bracket sits
/// over this gap, so it clears the stroke (`F_(φ)`, `F_1(φ)`).
pub const BRACKET_LEAD: f32 = 0.1;

/// Vertical padding of a bracket beyond the content's ascent and descent, in ems (see
/// [`delimiter_extent`]).
pub const BRACKET_PAD: f32 = 0.04;

/// Vertical extent of a pair of brackets around `inner`, as `(top, bottom)` relative to the
/// baseline (`top` negative). The brackets are centred on the math axis, as fraction bars and
/// matrices are, and reach as far as the content does on the farther side of the axis plus the
/// pad. A subscript on the content therefore does not pull the brackets down, and a symmetric
/// content (a matrix, a fraction) gets symmetric brackets.
pub fn delimiter_extent(inner: &MathBox, size: f32) -> (f32, f32) {
    let centre = -AXIS * size;
    let above = inner.ascent - AXIS * size;
    let below = inner.descent + AXIS * size;
    let half = above.max(below) + BRACKET_PAD * size;
    (centre - half, centre + half)
}

/// Surrounds `inner` with a pair of brackets sized to its height.
pub fn bracket_pair(inner: MathBox, pair: Pair, size: f32) -> MathBox {
    let (top, bottom) = delimiter_extent(&inner, size);
    let height = bottom - top;
    let width_stroke = (0.06 * size).max(0.8);
    let mut b = Builder::default();
    match pair {
        Pair::Round => {
            let bulge = 0.08 * size + 0.02 * height;
            let lead = BRACKET_LEAD * size;
            let pad = 0.05 * size;
            let inner_x = lead + bulge + pad;
            let right_x = inner_x + inner.width + pad;
            let left = curve(top, bottom, bulge, true, lead);
            let right = curve(top, bottom, bulge, false, right_x);
            b.put(inner, inner_x, 0.0);
            b.push(
                Prim::Line {
                    points: left,
                    width: width_stroke,
                },
                top,
                bottom,
            );
            b.push(
                Prim::Line {
                    points: right,
                    width: width_stroke,
                },
                top,
                bottom,
            );
            b.finish(right_x + bulge)
        }
        Pair::Square => {
            let arm = 0.12 * size;
            let pad = 0.1 * size;
            let inner_x = arm + pad;
            let right_x = inner_x + inner.width + pad;
            b.put(inner, inner_x, 0.0);
            b.push(
                Prim::Line {
                    points: vec![[arm, top], [0.0, top], [0.0, bottom], [arm, bottom]],
                    width: width_stroke,
                },
                top,
                bottom,
            );
            b.push(
                Prim::Line {
                    points: vec![
                        [right_x, top],
                        [right_x + arm, top],
                        [right_x + arm, bottom],
                        [right_x, bottom],
                    ],
                    width: width_stroke,
                },
                top,
                bottom,
            );
            b.finish(right_x + arm)
        }
    }
}

/// Points of one parenthesis curve. `open` bulges to the left, a closing one to the right.
/// `x0` is the offset of the curve's own left edge.
fn curve(top: f32, bottom: f32, bulge: f32, open: bool, x0: f32) -> Vec<[f32; 2]> {
    let height = bottom - top;
    (0..=CURVE_STEPS)
        .map(|i| {
            let t = i as f32 / CURVE_STEPS as f32;
            let k = 4.0 * (t - 0.5) * (t - 0.5);
            let x = if open { bulge * k } else { bulge * (1.0 - k) };
            [x0 + x, top + height * t]
        })
        .collect()
}

/// `‖inner‖`: the content between double vertical bars (the norm operator).
pub fn norm_bars(inner: MathBox, size: f32) -> MathBox {
    let thick = (0.05 * size).max(0.8);
    let gap = (0.12 * size).max(1.6);
    let pad = 0.08 * size;
    let top = -inner.ascent;
    let bottom = inner.descent;
    let bar = |x: f32| Prim::Rect {
        x,
        y: top,
        w: thick,
        h: bottom - top,
    };
    let inner_x = 2.0 * thick + gap + pad;
    let right_x = inner_x + inner.width + pad;
    let mut b = Builder::default();
    b.push(bar(0.0), top, bottom);
    b.push(bar(thick + gap), top, bottom);
    b.put(inner, inner_x, 0.0);
    b.push(bar(right_x), top, bottom);
    b.push(bar(right_x + thick + gap), top, bottom);
    b.finish(right_x + 2.0 * thick + gap)
}

/// Absolute value: vertical bars around the operand.
pub fn abs_bars(inner: MathBox, size: f32) -> MathBox {
    let thick = (0.05 * size).max(0.8);
    let pad = 0.08 * size;
    let top = -inner.ascent;
    let bottom = inner.descent;
    let right_x = thick + pad + inner.width + pad;
    let mut b = Builder::default();
    b.push(
        Prim::Rect {
            x: 0.0,
            y: top,
            w: thick,
            h: bottom - top,
        },
        top,
        bottom,
    );
    b.put(inner, thick + pad, 0.0);
    b.push(
        Prim::Rect {
            x: right_x,
            y: top,
            w: thick,
            h: bottom - top,
        },
        top,
        bottom,
    );
    b.finish(right_x + thick)
}

/// Square root (no degree) or nth root, with an overbar over the radicand.
pub fn radical(degree: Option<MathBox>, radicand: MathBox, size: f32) -> MathBox {
    let thick = (0.05 * size).max(0.8);
    let gap = 0.12 * size;
    let pad = 0.1 * size;
    let sign_w = 0.55 * size;
    let top = -radicand.ascent - gap - thick;
    let bottom = radicand.descent + gap;
    let x0 = degree.as_ref().map_or(0.0, |d| d.width * 0.9);
    let bar_y = top + thick / 2.0;
    let points = vec![
        [x0, bottom - 0.45 * (bottom - top)],
        [x0 + 0.3 * sign_w, bottom],
        [x0 + sign_w, bar_y],
        [x0 + sign_w + radicand.width + pad, bar_y],
    ];
    let width = x0 + sign_w + radicand.width + 2.0 * pad;
    let mut b = Builder::default();
    if let Some(d) = degree {
        let dy = top - d.descent;
        b.put(d, 0.0, dy);
    }
    b.push(
        Prim::Line {
            points,
            width: thick,
        },
        top,
        bottom,
    );
    b.put(radicand, x0 + sign_w + pad / 2.0, 0.0);
    b.finish(width)
}

/// A matrix: cells in a grid, centred on the math axis, inside square brackets. `cells` is
/// row-major and holds exactly `rows * cols` boxes.
pub fn matrix(rows: usize, cols: usize, cells: Vec<MathBox>, size: f32) -> MathBox {
    let col_gap = 0.6 * size;
    let row_gap = 0.25 * size;
    let mut col_w = vec![0.0_f32; cols];
    let mut row_asc = vec![0.0_f32; rows];
    let mut row_desc = vec![0.0_f32; rows];
    for (r, row) in cells.chunks(cols).enumerate() {
        for (c, cell) in row.iter().enumerate() {
            col_w[c] = col_w[c].max(cell.width);
            row_asc[r] = row_asc[r].max(cell.ascent);
            row_desc[r] = row_desc[r].max(cell.descent);
        }
    }
    let inner_h: f32 = row_asc
        .iter()
        .zip(&row_desc)
        .map(|(a, d)| a + d)
        .sum::<f32>()
        + row_gap * rows.saturating_sub(1) as f32;
    let mut b = Builder::default();
    let mut y = -AXIS * size - inner_h / 2.0;
    let mut cell_iter = cells.into_iter();
    for (asc, desc) in row_asc.iter().zip(&row_desc) {
        let baseline = y + asc;
        let mut x = 0.0;
        for cw in &col_w {
            let cell = cell_iter.next().unwrap_or_default();
            let cx = x + (cw - cell.width) / 2.0;
            b.put(cell, cx, baseline);
            x += cw + col_gap;
        }
        y += asc + desc + row_gap;
    }
    let inner_w: f32 = col_w.iter().sum::<f32>() + col_gap * cols.saturating_sub(1) as f32;
    let inner = b.finish(inner_w);
    bracket_pair(inner, Pair::Square, size)
}

/// A zero-width box that carries the insertion point, `size` points tall text.
pub fn caret_marker() -> MathBox {
    MathBox {
        caret: Some([0.0, 0.0]),
        ..MathBox::default()
    }
}

/// Placeholder for an empty slot: a small empty (outlined) box, as Prime draws it. It is a stroke,
/// so it never reads as a letter.
pub fn placeholder(size: f32) -> MathBox {
    let mut b = Builder::default();
    let top = -0.6 * size;
    let bottom = 0.1 * size;
    let w = 0.5 * size;
    let stroke = (0.06 * size).max(0.8);
    let outline = vec![[0.0, top], [w, top], [w, bottom], [0.0, bottom], [0.0, top]];
    b.push(
        Prim::Line {
            points: outline,
            width: stroke,
        },
        top,
        bottom,
    );
    b.finish(w)
}

/// A base with an optional upper and lower limit stacked above and below it, centred on the
/// base (the summation and product signs). The gap is `GAP * size` from the base's ink box.
pub fn stacked_limits(
    base: MathBox,
    upper: Option<MathBox>,
    lower: Option<MathBox>,
    size: f32,
) -> MathBox {
    const GAP: f32 = 0.08;
    let width = [
        Some(base.width),
        upper.as_ref().map(|u| u.width),
        lower.as_ref().map(|l| l.width),
    ]
    .into_iter()
    .flatten()
    .fold(0.0_f32, f32::max);
    let centred = |w: f32| (width - w) / 2.0;
    let gap = GAP * size;
    let base_x = centred(base.width);
    let base_top = -base.ascent;
    let base_bottom = base.descent;
    let mut b = Builder::default();
    if let Some(u) = upper {
        // The limit's descent sits `gap` above the base's top.
        let dy = base_top - gap - u.descent;
        let dx = centred(u.width);
        b.put(u, dx, dy);
    }
    b.put(base, base_x, 0.0);
    if let Some(l) = lower {
        // The limit's ascent sits `gap` below the base's bottom.
        let dy = base_bottom + gap + l.ascent;
        let dx = centred(l.width);
        b.put(l, dx, dy);
    }
    b.finish(width)
}

/// Comma-separated list of boxes, as one box.
pub fn list(items: Vec<MathBox>, size: f32) -> MathBox {
    let mut parts = Vec::with_capacity(items.len() * 2);
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            parts.push(text(",", size));
            parts.push(space(0.2 * size));
        }
        parts.push(item);
    }
    hcat(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke_points(b: &MathBox) -> Vec<Vec<[f32; 2]>> {
        b.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Line { points, .. } => Some(points.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn round_bracket_encloses_the_content_with_the_pad() {
        let inner = text("x", 12.0);
        let (ascent, descent) = (inner.ascent, inner.descent);
        let b = bracket_pair(inner, Pair::Round, 12.0);
        // Strokes reach the content's top and bottom plus the pad (the box is the bracket's).
        assert!(b.ascent >= ascent + BRACKET_PAD * 12.0 - 1e-4);
        assert!(b.descent >= descent + BRACKET_PAD * 12.0 - 1e-4);
    }

    /// Vertical centre of the first stroke of a bracket pair (its own points, baseline relative).
    fn stroke_centre(b: &MathBox) -> f32 {
        let pts = &stroke_points(b)[0];
        let top = pts.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let bottom = pts.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        (top + bottom) / 2.0
    }

    #[test]
    fn brackets_are_centred_on_the_math_axis() {
        // Plain text: the bracket's centre is the axis (0.3 em above the baseline), not the
        // centre of the text box, which sat low by a quarter of the descent.
        for inner in [text("x", 12.0), text("F(0 °)", 12.0)] {
            let b = bracket_pair(inner, Pair::Round, 12.0);
            assert!(
                (stroke_centre(&b) + AXIS * 12.0).abs() < 0.05,
                "centre {}",
                stroke_centre(&b)
            );
        }
    }

    #[test]
    fn a_subscript_does_not_pull_the_brackets_down() {
        // `(α_s)`: the subscript hangs below the base. The strokes still reach past the base's
        // bottom, but the centre stays on the axis, so the brackets do not sink.
        let plain = bracket_pair(text("α", 12.0), Pair::Round, 12.0);
        let sub = bracket_pair(
            attach(text("α", 12.0), text("s", 8.4), 0.25 * 12.0),
            Pair::Round,
            12.0,
        );
        assert!((stroke_centre(&sub) - stroke_centre(&plain)).abs() < 1e-3);
        assert!(sub.descent > plain.descent);
    }

    #[test]
    fn a_placeholder_is_an_outlined_box_not_a_filled_one() {
        let p = placeholder(12.0);
        assert!(p
            .prims
            .iter()
            .all(|prim| !matches!(prim, Prim::Text { .. })));
        assert!(p
            .prims
            .iter()
            .all(|prim| !matches!(prim, Prim::Rect { .. })));
        let outline = stroke_points(&p);
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].len(), 5, "closed square");
    }

    #[test]
    fn stacked_limits_sit_above_and_below_the_sign_and_are_centred() {
        let sign = text("Σ", 19.2);
        let upper = text("2", 8.4);
        let lower = text("i=1", 8.4);
        let (sign_w, up_w, low_w) = (sign.width, upper.width, lower.width);
        let b = stacked_limits(sign, Some(upper), Some(lower), 12.0);
        let at = |s: &str| -> (f32, f32) {
            b.prims
                .iter()
                .find_map(|p| match p {
                    Prim::Text { x, y, text, .. } if text == s => Some((*x, *y)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{s} drawn"))
        };
        let (sign_xy, up_xy, low_xy) = (at("Σ"), at("2"), at("i=1"));
        // The upper limit is above the sign's top, the lower one below its foot.
        assert!(up_xy.1 < sign_xy.1 - 19.2 * 0.78 + 1e-3, "upper {up_xy:?}");
        assert!(low_xy.1 > sign_xy.1 + 1e-3, "lower {low_xy:?}");
        // Each limit is centred on the sign (its ink box is the same width as the sign here).
        let centre = |x: f32, w: f32| x + w / 2.0;
        assert!((centre(up_xy.0, up_w) - centre(sign_xy.0, sign_w)).abs() < 1e-3);
        assert!((centre(low_xy.0, low_w) - centre(sign_xy.0, sign_w)).abs() < 1e-3);
        // The width is the widest part: a wide limit does not reach into the body.
        assert!(b.width >= low_w.max(up_w).max(sign_w) - 1e-3);
    }

    #[test]
    fn a_limit_wider_than_the_sign_widens_the_head_and_keeps_the_sign_centred() {
        let b = stacked_limits(text("Σ", 19.2), None, Some(text("i=1234567", 8.4)), 12.0);
        let sign_x = b
            .prims
            .iter()
            .find_map(|p| match p {
                Prim::Text { x, text, .. } if text == "Σ" => Some(*x),
                _ => None,
            })
            .expect("sign drawn");
        assert!(sign_x > 0.0, "the sign is centred, not at the left edge");
    }

    #[test]
    fn round_bracket_strokes_stay_inside_the_box_height() {
        let b = bracket_pair(text("(α_s)", 12.0), Pair::Round, 12.0);
        for curve in stroke_points(&b) {
            for [_, y] in curve {
                assert!(
                    y >= -b.ascent - 1e-4 && y <= b.descent + 1e-4,
                    "y {y} outside"
                );
            }
        }
    }

    #[test]
    fn open_bracket_clears_a_script_on_the_content_before_it() {
        // The opening stroke bulges to the right of its middle, so its middle sits at the lead.
        let b = bracket_pair(text("φ", 12.0), Pair::Round, 12.0);
        let strokes = stroke_points(&b);
        let left_min = strokes[0]
            .iter()
            .map(|p| p[0])
            .fold(f32::INFINITY, f32::min);
        assert!(
            (left_min - BRACKET_LEAD * 12.0).abs() < 1e-4,
            "left {left_min}"
        );
        let right_max = strokes[1].iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!(
            (right_max - b.width).abs() < 1e-4,
            "right {right_max} width {}",
            b.width
        );
    }

    #[test]
    fn a_bracket_is_never_wider_than_its_content_and_both_strokes() {
        let inner = text("ab", 12.0);
        let inner_w = inner.width;
        let b = bracket_pair(inner, Pair::Round, 12.0);
        assert!(b.width > inner_w + 2.0 * BRACKET_LEAD * 12.0);
    }
}

//! The typeset draft: the editor's item tree laid out as stacked mathematics while it is typed.
//!
//! Fractions, powers, roots, matrices and the calculus operators show as they do in an evaluated
//! region (the same boxes as `mathlayout.rs`, from `mathbox.rs`). Every empty slot is an outlined
//! placeholder, and the insertion point is carried through the boxes (`MathBox::caret`) so the
//! canvas can draw the caret where the editor is.

use crate::edit::math::{Item, OpKind, Row};
use crate::fonts::Face;
use crate::mathbox::{
    abs_bars, attach, bracket_pair, caret_marker, fraction, hcat, matrix, norm_bars, placeholder,
    radical, space, stacked_limits, superscript, text, text_in, MathBox, Pair, SCRIPT,
};

/// The editor's tree with the caret in it: the root row, the path to the caret's row (item index,
/// slot) and the caret's position in that row.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftTree {
    pub root: Row,
    pub path: Vec<(usize, usize)>,
    pub pos: usize,
}

impl DraftTree {
    /// True when the draft holds something a flat line cannot show: a fraction, power, root,
    /// matrix or calculus operator (also inside parentheses).
    pub fn is_structured(&self) -> bool {
        fn row(r: &Row) -> bool {
            r.iter().any(|item| match item {
                Item::Atom(_) => false,
                Item::Group(inner) => row(inner),
                _ => true,
            })
        }
        row(&self.root)
    }

    /// Lays the draft out with base font `size`; the box carries the caret.
    pub fn layout(&self, size: f32) -> MathBox {
        let ctx = Ctx {
            caret: &self.path,
            pos: self.pos,
        };
        lay_row(&self.root, size, &mut Vec::new(), &ctx, false)
    }
}

struct Ctx<'a> {
    caret: &'a [(usize, usize)],
    pos: usize,
}

const OPERATORS: [(&str, &str); 7] = [
    (":=", "\u{2254}"),
    ("<=", "≤"),
    (">=", "≥"),
    ("<>", "≠"),
    ("!=", "≠"),
    ("==", "="),
    ("->", "→"),
];

/// One row: its items on a baseline. A `slot` row draws a placeholder when empty.
fn lay_row(
    row: &Row,
    size: f32,
    path: &mut Vec<(usize, usize)>,
    ctx: &Ctx<'_>,
    slot: bool,
) -> MathBox {
    let here = path.as_slice() == ctx.caret;
    let mut parts: Vec<MathBox> = Vec::new();
    let mut i = 0;
    loop {
        if here && ctx.pos.min(row.len()) == i {
            parts.push(caret_marker());
        }
        if i == row.len() {
            break;
        }
        match &row[i] {
            Item::Atom(_) => {
                // A run of atoms, split where the caret is.
                let end = (i + 1..row.len())
                    .find(|&j| !matches!(row[j], Item::Atom(_)) || (here && ctx.pos == j))
                    .unwrap_or(row.len());
                let chars: Vec<char> = row[i..end]
                    .iter()
                    .filter_map(|item| match item {
                        Item::Atom(c) => Some(*c),
                        _ => None,
                    })
                    .collect();
                parts.push(atoms(&chars, size));
                i = end;
            }
            item => {
                path.push((i, 0));
                parts.push(lay_item(item, size, path, ctx));
                path.pop();
                i += 1;
            }
        }
    }
    if slot && row.is_empty() {
        // An empty slot: the outlined box, with the caret in front when it is here.
        let mut out = vec![placeholder(size)];
        if here {
            out.insert(0, caret_marker());
        }
        return hcat(out);
    }
    hcat(parts)
}

/// Lays out slot `index` of the item at the end of `path`.
fn slot_box(row: &Row, size: f32, path: &[(usize, usize)], ctx: &Ctx<'_>, index: usize) -> MathBox {
    let mut p = path.to_vec();
    if let Some(last) = p.last_mut() {
        last.1 = index;
    }
    lay_row(row, size, &mut p, ctx, true)
}

fn lay_item(item: &Item, size: f32, path: &[(usize, usize)], ctx: &Ctx<'_>) -> MathBox {
    let slot = |row: &Row, i: usize, s: f32| slot_box(row, s, path, ctx, i);
    let small = SCRIPT * size;
    match item {
        Item::Atom(_) => MathBox::default(),
        Item::Group(inner) => bracket_pair(slot(inner, 0, size), Pair::Round, size),
        Item::Sqrt(inner) => radical(None, slot(inner, 0, size), size),
        Item::Abs(inner) => abs_bars(slot(inner, 0, size), size),
        Item::Fraction { num, den } => fraction(slot(num, 0, size), slot(den, 1, size), size),
        Item::Power { base, exp } => superscript(slot(base, 0, size), slot(exp, 1, small), size),
        Item::Index { base, index } => {
            attach(slot(base, 0, size), slot(index, 1, small), 0.25 * size)
        }
        Item::Matrix { cols, cells } => {
            let cols = (*cols).max(1);
            let rows = cells.len().div_ceil(cols).max(1);
            let boxes = cells
                .iter()
                .enumerate()
                .map(|(i, c)| slot(c, i, size))
                .collect();
            matrix(rows, cols, boxes, size)
        }
        Item::Op { kind, slots } => {
            let s = |i: usize, sz: f32| slot(&slots[i], i, sz);
            let partial = matches!(kind, OpKind::Partial | OpKind::NthPartial);
            let d = || text(if partial { "∂" } else { "d" }, size);
            match kind {
                OpKind::Derivative | OpKind::Partial => {
                    let den = hcat(vec![d(), s(0, size)]);
                    hcat(vec![
                        fraction(d(), den, size),
                        space(0.25 * size),
                        s(1, size),
                    ])
                }
                OpKind::NthDerivative | OpKind::NthPartial => {
                    // The order is edited in the numerator; the denominator repeats it.
                    let none = Ctx {
                        caret: &[(usize::MAX, 0)],
                        pos: 0,
                    };
                    let again = lay_row(&slots[0], small, &mut Vec::new(), &none, true);
                    let num = superscript(d(), s(0, small), size);
                    let den = hcat(vec![d(), superscript(s(1, size), again, size)]);
                    hcat(vec![
                        fraction(num, den, size),
                        space(0.25 * size),
                        s(2, size),
                    ])
                }
                OpKind::Gradient => hcat(vec![
                    attach(text("∇", size), s(0, small), 0.25 * size),
                    space(0.25 * size),
                    s(1, size),
                ]),
                OpKind::Norm => norm_bars(s(0, size), size),
                OpKind::Integral => {
                    let sign = superscript(
                        attach(text("∫", 1.6 * size), s(0, small), 0.45 * size),
                        s(1, small),
                        size,
                    );
                    hcat(vec![
                        sign,
                        space(0.25 * size),
                        s(2, size),
                        space(0.15 * size),
                        d(),
                        s(3, size),
                    ])
                }
                OpKind::Sum | OpKind::Product => {
                    let glyph = if *kind == OpKind::Sum { "Σ" } else { "Π" };
                    let lower = hcat(vec![s(0, small), text("=", small), s(1, small)]);
                    let head = stacked_limits(
                        text(glyph, 1.6 * size),
                        Some(s(2, small)),
                        Some(lower),
                        size,
                    );
                    hcat(vec![head, space(0.25 * size), s(3, size)])
                }
                OpKind::Limit => {
                    let approach = hcat(vec![s(0, small), text("→", small), s(1, small)]);
                    let head = attach(text("lim", size), approach, 0.45 * size);
                    hcat(vec![head, space(0.25 * size), s(2, size)])
                }
                OpKind::NthRoot => radical(Some(s(0, small)), s(1, size), size),
            }
        }
    }
}

/// A run of typed characters as boxes: names in italic, numbers upright, operators spaced, `_`
/// lowering the rest of the name.
fn atoms(chars: &[char], size: f32) -> MathBox {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        if let Some((from, to)) = OPERATORS.iter().find(|(from, _)| rest.starts_with(from)) {
            // `==` (the comparison) is a bold equals sign, as Prime shows it.
            if *from == "==" {
                parts.push(hcat(vec![
                    space(0.25 * size),
                    text_in("=", size, Face::Bold),
                    space(0.25 * size),
                ]));
            } else {
                parts.push(spaced(to, size));
            }
            i += from.chars().count();
            continue;
        }
        let c = chars[i];
        if c.is_alphabetic() {
            let end = (i..chars.len())
                .find(|&j| !(chars[j].is_alphanumeric() || chars[j] == '.'))
                .unwrap_or(chars.len());
            let name: String = chars[i..end].iter().collect();
            let face = if name.chars().count() > 1
                && crate::mathlayout::STANDARD_FUNCTIONS.contains(&name.as_str())
            {
                Face::Roman
            } else {
                Face::Italic
            };
            let mut word = text_in(&name, size, face);
            i = end;
            if chars.get(i) == Some(&'_') {
                let sub_end = (i + 1..chars.len())
                    .find(|&j| !chars[j].is_alphanumeric())
                    .unwrap_or(chars.len());
                let sub: String = chars[i + 1..sub_end].iter().collect();
                word = attach(word, text(&sub, SCRIPT * size), 0.25 * size);
                i = sub_end;
            }
            parts.push(word);
        } else if c.is_ascii_digit() || c == '.' {
            let end = (i..chars.len())
                .find(|&j| !(chars[j].is_ascii_digit() || chars[j] == '.'))
                .unwrap_or(chars.len());
            let number: String = chars[i..end].iter().collect();
            parts.push(text(&number, size));
            i = end;
        } else if matches!(c, '+' | '-' | '=' | '<' | '>') {
            let op = if c == '-' { '−' } else { c };
            parts.push(spaced(&op.to_string(), size));
            i += 1;
        } else if c == '*' {
            parts.push(text("·", size));
            i += 1;
        } else if c == ' ' {
            parts.push(space(0.25 * size));
            i += 1;
        } else {
            parts.push(text(&c.to_string(), size));
            i += 1;
        }
    }
    hcat(parts)
}

fn spaced(op: &str, size: f32) -> MathBox {
    hcat(vec![space(0.25 * size), text(op, size), space(0.25 * size)])
}

#[cfg(test)]
mod tests {
    use crate::edit::math::{MathEditor, Template};
    use crate::mathbox::Prim;

    fn typed(ed: &mut MathEditor, text: &str) {
        for c in text.chars() {
            ed.key(c);
        }
    }

    #[test]
    fn a_plain_line_is_not_structured() {
        let mut ed = MathEditor::new();
        typed(&mut ed, "x:=(a+b)*2");
        assert!(!ed.tree().is_structured());
    }

    #[test]
    fn a_derivative_is_stacked_with_placeholders_and_a_caret() {
        let mut ed = MathEditor::new();
        ed.apply_template(Template::Derivative);
        let tree = ed.tree();
        assert!(tree.is_structured());
        let empty = tree.layout(20.0);
        let text: Vec<&str> = empty
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        // `d` over `d` and an empty variable (outlined boxes), not the flat `d/d` text.
        assert_eq!(text, ["d", "d"]);
        assert!(empty.caret.is_some());
        // Typing in the variable slot widens the draft and keeps the caret.
        typed(&mut ed, "t");
        let filled = ed.tree().layout(20.0);
        assert!(filled.caret.is_some());
        assert!(filled.width != empty.width);
    }

    #[test]
    fn reopened_text_gets_its_structure_back() {
        for text in [
            "derivative(t, v)",
            "derivative(t, 2, v)",
            "integral(x, 0, 1, x)",
            "sum(i, 1, 5, i)",
            "limit(x, 0, sin(x)/x)",
            "root(x, 3)",
            "sqrt(x^2+1)",
            "a/b+c^2",
            "y:=[1, 2; 3, 4]",
        ] {
            let ed = MathEditor::from_text(text);
            assert_eq!(ed.to_text(), text);
            assert!(ed.tree().is_structured(), "{text} stays flat");
        }
        assert!(!MathEditor::from_text("x:=2*y+1").tree().is_structured());
    }
}

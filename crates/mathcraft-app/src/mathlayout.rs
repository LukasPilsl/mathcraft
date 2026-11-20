//! Pure 2D math layout: an [`Expr`] becomes a [`MathBox`] of positioned primitives, in points.
//!
//! No egui types live here, so the layout is unit-testable. The painter in `canvas.rs` draws
//! the primitives. The box model and the drawing of each construct are in `mathbox.rs`.

use mathcraft_core::expr::{Accumulation, BinaryOp, Expr, Id, IdKind, Number, Prec, UnaryOp};

use crate::fonts::Face;
use crate::mathbox::{
    abs_bars, attach, bracket_pair, fraction, hcat, infix, list, matrix, norm_bars, placeholder,
    radical, space, stacked_limits, superscript, text, text_in, Pair, SCRIPT,
};
pub use crate::mathbox::{text_width, MathBox, Prim};

/// Deepest expression level laid out. Deeper input becomes a `[too deep]` text.
pub const MAX_DEPTH: usize = 120;
/// Largest matrix (in cells) drawn as a grid. Larger ones become a text placeholder.
const MAX_MATRIX_CELLS: usize = 10_000;

/// Lays out `expr` with a base font size of `size` points.
pub fn layout(expr: &Expr, size: f32) -> MathBox {
    lay(expr, size, 0)
}

/// Standard function names. A worksheet may label one of them as a variable (or not label it at
/// all), so these stay upright whatever the label says.
pub(crate) const STANDARD_FUNCTIONS: [&str; 24] = [
    "sin", "cos", "tan", "cot", "sec", "csc", "asin", "acos", "atan", "acot", "sinh", "cosh",
    "tanh", "asinh", "acosh", "atanh", "ln", "log", "exp", "sqrt", "abs", "min", "max", "floor",
];

/// Keywords of symbolic evaluation (`-> solve, x`, `-> factor`). They are command words, not
/// variables, so they are set upright; their arguments (`x`) keep the variable face.
const SYMBOLIC_KEYWORDS: [&str; 22] = [
    "solve",
    "factor",
    "expand",
    "simplify",
    "assume",
    "substitute",
    "collect",
    "coeffs",
    "series",
    "float",
    "rewrite",
    "parfrac",
    "fourier",
    "laplace",
    "invlaplace",
    "ztrans",
    "invztrans",
    "diff",
    "integrate",
    "ALL",
    "real",
    "complex",
];

/// One item of a symbolic command: a keyword is upright, anything else lays out as usual.
fn command_item(expr: &Expr, size: f32, depth: usize) -> MathBox {
    match expr {
        Expr::Id(id) if SYMBOLIC_KEYWORDS.contains(&id.name.as_str()) => {
            ident_in(id, Face::Roman, size, depth)
        }
        _ => lay(expr, size, depth),
    }
}

/// Identifier: a name, with its literal subscript lowered when it has one. Variables are set in
/// italic, as Prime does; function names, units and constants stay upright.
fn ident(id: &Id, size: f32, depth: usize) -> MathBox {
    // The word `inf` (an infinite bound written without the sign) is drawn as the sign.
    if id.name == "inf" && id.subscript.is_none() && id.kind == IdKind::Variable {
        return text_in("∞", size, Face::Roman);
    }
    let face = match id.kind {
        IdKind::Variable if !STANDARD_FUNCTIONS.contains(&id.name.as_str()) => Face::Italic,
        IdKind::Variable | IdKind::Function | IdKind::Unit | IdKind::Constant => Face::Roman,
    };
    ident_in(id, face, size, depth)
}

/// Identifier set in `face`, with its literal subscript lowered when it has one.
fn ident_in(id: &Id, face: Face, size: f32, depth: usize) -> MathBox {
    let base = text_in(&id.name, size, face);
    match &id.subscript {
        None => base,
        Some(sub) => {
            let sub_box = lay(sub, SCRIPT * size, depth);
            attach(base, sub_box, 0.25 * size)
        }
    }
}

/// Most columns a matrix result draws. A wider result keeps the first columns and ends in a column
/// of ellipses, so a 1 x 400 result stays a readable width (the full text is in the tooltip).
pub const MAX_RESULT_COLUMNS: usize = 20;

/// A matrix result: its cells (row-major text, `rows * cols` of them) in a grid inside brackets,
/// and the unit after the grid when there is one. See [`MAX_RESULT_COLUMNS`].
pub fn matrix_result(rows: usize, cols: usize, cells: &[String], unit: &str, size: f32) -> MathBox {
    let wide = cols > MAX_RESULT_COLUMNS;
    let shown = cols.min(MAX_RESULT_COLUMNS);
    let boxes = (0..rows)
        .flat_map(|r| (0..shown).map(move |c| (r, c)))
        .map(|(r, c)| {
            if wide && c + 1 == shown {
                return text("…", size);
            }
            match cells.get(r * cols + c) {
                Some(s) => text(&s.replace('-', "−"), size),
                None => MathBox::default(),
            }
        })
        .collect();
    let grid = matrix(rows, shown, boxes, size);
    if unit.is_empty() {
        grid
    } else {
        hcat(vec![grid, space(0.25 * size), text(unit, size)])
    }
}

/// Sign of a rational's numerator text, split off so the magnitude can be laid out as a
/// fraction. Returns the sign (`"−"` or empty) and the magnitude.
fn split_sign(s: &str) -> (&str, &str) {
    match s.strip_prefix('-') {
        Some(rest) => ("−", rest),
        None => ("", s),
    }
}

fn binary(op: BinaryOp, lhs: MathBox, rhs: MathBox, size: f32) -> MathBox {
    match op {
        BinaryOp::Add => infix(lhs, "+", rhs, size),
        BinaryOp::Sub => infix(lhs, "−", rhs, size),
        BinaryOp::Mul => infix(lhs, "·", rhs, size),
        BinaryOp::Div => fraction(lhs, rhs, size),
        BinaryOp::Cross => infix(lhs, "×", rhs, size),
        BinaryOp::Pow => superscript(lhs, rhs, size),
        BinaryOp::Eq => infix(lhs, "=", rhs, size),
        BinaryOp::Ne => infix(lhs, "≠", rhs, size),
        BinaryOp::Lt => infix(lhs, "<", rhs, size),
        BinaryOp::Gt => infix(lhs, ">", rhs, size),
        BinaryOp::Le => infix(lhs, "≤", rhs, size),
        BinaryOp::Ge => infix(lhs, "≥", rhs, size),
        BinaryOp::And => infix(lhs, "and", rhs, size),
        BinaryOp::Or => infix(lhs, "or", rhs, size),
        BinaryOp::Xor => infix(lhs, "xor", rhs, size),
    }
}

fn unary(op: UnaryOp, operand: MathBox, size: f32) -> MathBox {
    match op {
        UnaryOp::Neg => hcat(vec![text("−", size), operand]),
        UnaryOp::Not => hcat(vec![text("not", size), space(0.25 * size), operand]),
        UnaryOp::Abs => abs_bars(operand, size),
        UnaryOp::Norm => norm_bars(operand, size),
        UnaryOp::Factorial => hcat(vec![operand, text("!", size)]),
        UnaryOp::Percent => hcat(vec![operand, text("%", size)]),
        UnaryOp::Transpose => {
            let mark = text("T", SCRIPT * size);
            attach(operand, mark, -0.42 * size)
        }
    }
}

/// Lays out `expr` as an operand that needs precedence `min` (strictly above it when `strict`),
/// in round brackets when its outermost form binds looser.
fn lay_min(expr: &Expr, min: Prec, strict: bool, size: f32, depth: usize) -> MathBox {
    let p = expr.prec();
    let loose = if strict { p <= min } else { p < min };
    let boxed = lay(expr, size, depth);
    if loose {
        bracket_pair(boxed, Pair::Round, size)
    } else {
        boxed
    }
}

fn lay(expr: &Expr, size: f32, depth: usize) -> MathBox {
    if depth > MAX_DEPTH {
        return text("[too deep]", size);
    }
    let d = depth + 1;
    match expr {
        Expr::Number(Number::Real { text: literal, .. }) => text(&literal.replace('-', "−"), size),
        Expr::Number(Number::Rational(r)) => {
            let numer = r.numer().to_string();
            let denom = r.denom().to_string();
            let (sign, magnitude) = split_sign(&numer);
            if denom == "1" {
                text(&format!("{sign}{magnitude}"), size)
            } else {
                hcat(vec![
                    text(sign, size),
                    fraction(text(magnitude, size), text(&denom, size), size),
                ])
            }
        }
        Expr::Id(id) => ident(id, size, d),
        Expr::Unary { op, operand } => {
            // Postfix operands must be atoms: `(x+1)!`, `(-2)%`, `(v^T)!`.
            let min = match op {
                UnaryOp::Neg => Some(Prec::POW),
                UnaryOp::Not => Some(Prec::NOT),
                UnaryOp::Abs | UnaryOp::Norm => None,
                UnaryOp::Factorial | UnaryOp::Percent | UnaryOp::Transpose => Some(Prec::ATOM),
            };
            let inner = match min {
                Some(m) => lay_min(operand, m, false, size, d),
                None => lay(operand, size, d),
            };
            unary(*op, inner, size)
        }
        Expr::Binary {
            op: BinaryOp::Pow,
            lhs,
            rhs,
        } => superscript(
            lay_min(lhs, Prec::ATOM, false, size, d),
            lay(rhs, SCRIPT * size, d),
            size,
        ),
        // The fraction bar groups both operands.
        Expr::Binary {
            op: BinaryOp::Div,
            lhs,
            rhs,
        } => fraction(lay(lhs, size, d), lay(rhs, size, d), size),
        Expr::Binary { op, lhs, rhs } => {
            let p = op.prec();
            let comparison = p == Prec::CMP;
            let arithmetic = matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul);
            // Left-associative: an equal-precedence right operand needs brackets, and so does a
            // negative one (`a−(−b)`); comparisons chain in neither direction.
            let left = lay_min(lhs, p, comparison, size, d);
            let right = if arithmetic && rhs.prec() == Prec::NEG {
                lay_min(rhs, Prec::ATOM, false, size, d)
            } else {
                lay_min(rhs, p, true, size, d)
            };
            binary(*op, left, right, size)
        }
        Expr::Call { func, args } => {
            // A call is always a function name, upright whatever its label says.
            let name = ident_in(func, Face::Roman, size, d);
            let inner = list(args.iter().map(|a| lay(a, size, d)).collect(), size);
            hcat(vec![name, bracket_pair(inner, Pair::Round, size)])
        }
        Expr::Range { from, step, to } => {
            let mut parts = vec![lay(from, size, d)];
            if let Some(step) = step {
                parts.push(text(", ", size));
                parts.push(lay(step, size, d));
            }
            parts.push(text(" .. ", size));
            parts.push(lay(to, size, d));
            hcat(parts)
        }
        Expr::Matrix {
            rows,
            cols,
            elements,
        } => {
            if *rows == 0 || *cols == 0 {
                return text("[ ]", size);
            }
            if rows.saturating_mul(*cols) > MAX_MATRIX_CELLS {
                return text("[matrix]", size);
            }
            let cells = (0..*rows * *cols)
                .map(|i| match elements.get(i) {
                    Some(e) => lay(e, size, d),
                    None => MathBox::default(),
                })
                .collect();
            matrix(*rows, *cols, cells, size)
        }
        Expr::Index { base, indices } => {
            let inner = list(indices.iter().map(|i| lay(i, size, d)).collect(), size);
            let postfix = matches!(
                &**base,
                Expr::Unary {
                    op: UnaryOp::Transpose | UnaryOp::Factorial | UnaryOp::Percent,
                    ..
                }
            );
            let base = if postfix {
                bracket_pair(lay(base, size, d), Pair::Round, size)
            } else {
                lay_min(base, Prec::POSTFIX, false, size, d)
            };
            hcat(vec![base, text("[", size), inner, text("]", size)])
        }
        Expr::NthRoot { degree, radicand } => {
            let degree = degree.as_ref().map(|deg| lay(deg, 0.6 * size, d));
            radical(degree, lay(radicand, size, d), size)
        }
        Expr::Parens(inner) => bracket_pair(lay(inner, size, d), Pair::Round, size),
        Expr::Define { target, expr } => {
            infix(lay(target, size, d), ":=", lay(expr, size, d), size)
        }
        Expr::GlobalDefine { target, expr } => {
            infix(lay(target, size, d), "≡", lay(expr, size, d), size)
        }
        Expr::Eval {
            expr,
            unit_override,
        } => {
            let mut parts = vec![lay(expr, size, d)];
            if let Some(unit) = unit_override {
                parts.push(space(0.25 * size));
                parts.push(text("→", size));
                parts.push(space(0.25 * size));
                parts.push(lay(unit, size, d));
            }
            // A symbolic evaluation has no trailing `=`, as in the source text.
            if !matches!(expr.as_ref(), Expr::SymEval { .. }) {
                parts.push(space(0.25 * size));
                parts.push(text("=", size));
            }
            hcat(parts)
        }
        Expr::SymEval { expr, command, .. } => {
            let mut parts = vec![lay(expr, size, d), space(0.25 * size), text("→", size)];
            if let Some(command) = command {
                parts.push(space(0.25 * size));
                let items = command.0.iter().map(|c| command_item(c, size, d)).collect();
                parts.push(list(items, size));
            }
            hcat(parts)
        }
        Expr::Scale { value, unit } => hcat(vec![
            lay_min(value, Prec::ATOM, false, size, d),
            space(0.25 * size),
            lay(unit, size, d),
        ]),
        Expr::Placeholder => placeholder(size),
        Expr::Str(s) => text(&format!("\"{s}\""), size),
        // A flat marker until the layout draws programming regions.
        Expr::Program(_) => text("[program]", size),
        Expr::Accumulate {
            op,
            var,
            from,
            to,
            body,
        } => {
            let glyph = match op {
                Accumulation::Sum => "Σ",
                Accumulation::Product => "Π",
            };
            let small = 0.7 * size;
            let lower = hcat(vec![
                ident(var, small, d + 1),
                text("=", small),
                lay(from, small, d + 1),
            ]);
            let upper = lay(to, small, d + 1);
            let head = stacked_limits(text(glyph, 1.6 * size), Some(upper), Some(lower), size);
            big_operator(head, lay(body, size, d + 1), size)
        }
        Expr::Fold { op, operand } => {
            let glyph = match op {
                Accumulation::Sum => "Σ",
                Accumulation::Product => "Π",
            };
            let head = text(glyph, 1.6 * size);
            big_operator(head, lay(operand, size, d + 1), size)
        }
        Expr::Integral {
            var,
            lower,
            upper,
            body,
        } => {
            let small = 0.7 * size;
            let scripts = (lay(lower, small, d + 1), lay(upper, small, d + 1));
            // The integral's limits sit beside the sign, as Prime sets them.
            let (lower, upper) = scripts;
            let sign = superscript(
                attach(text("∫", 1.6 * size), lower, 0.45 * size),
                upper,
                size,
            );
            hcat(vec![
                big_operator(sign, lay(body, size, d + 1), size),
                space(0.15 * size),
                text("d", size),
                ident(var, size, d + 1),
            ])
        }
        Expr::Derivative {
            var,
            order,
            body,
            partial,
        } => {
            // d^n/dx^n (∂^n/∂x^n for a partial derivative) as a fraction, the function beside it.
            let glyph = if *partial { "∂" } else { "d" };
            let (num, den) = match order {
                Some(n) => {
                    let n = lay(n, 0.7 * size, d + 1);
                    (
                        superscript(text(glyph, size), n.clone(), size),
                        hcat(vec![
                            text(glyph, size),
                            superscript(ident(var, size, d + 1), n, size),
                        ]),
                    )
                }
                None => (
                    text(glyph, size),
                    hcat(vec![text(glyph, size), ident(var, size, d + 1)]),
                ),
            };
            hcat(vec![
                fraction(num, den, size),
                space(0.25 * size),
                lay(body, size, d + 1),
            ])
        }
        Expr::Gradient { vars, body } => {
            // `∇` with the variables as a subscript, the function beside it.
            let small = 0.7 * size;
            let mut names = Vec::new();
            for (i, v) in vars.iter().enumerate() {
                if i > 0 {
                    names.push(text(",", small));
                }
                names.push(ident(v, small, d + 1));
            }
            let nabla = text("∇", size);
            let head = if names.is_empty() {
                nabla
            } else {
                attach(nabla, hcat(names), 0.25 * size)
            };
            hcat(vec![head, space(0.25 * size), lay(body, size, d + 1)])
        }
        Expr::Limit { var, target, body } => {
            // `lim` with `x→target` beneath it, the body beside it.
            let small = 0.7 * size;
            let approach = hcat(vec![
                ident(var, small, d + 1),
                text("→", small),
                lay(target, small, d + 1),
            ]);
            let head = attach(text("lim", size), approach, 0.45 * size);
            hcat(vec![head, space(0.25 * size), lay(body, size, d + 1)])
        }
    }
}

/// A big operator (its sign with any limits, `head`), then its operand.
fn big_operator(head: MathBox, operand: MathBox, size: f32) -> MathBox {
    hcat(vec![head, space(0.25 * size), operand])
}

#[cfg(test)]
#[path = "mathlayout_tests.rs"]
mod tests;

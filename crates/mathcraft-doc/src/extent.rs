//! A conservative estimate of the height a region takes on the page, for placing the next region
//! below it. The doc crate has no GUI, so this mirrors the shape of the app's layout: a line is
//! 24 pt, a matrix is one line per row, a fraction stacks its two parts, big operators and
//! integrals are taller, and an evaluation also shows its result (a matrix result is as tall as
//! the matrix). It errs on the tall side: a gap is harmless, an overlap is not.

use mathcraft_core::expr::{BinaryOp, Expr, Id, Number, UnaryOp};
use mathcraft_import::regions::{RegionKind, RegionModel};

/// Height of one line of math, in page points.
const LINE: f64 = 24.0;
/// Padding a matrix adds above and below its rows.
const MATRIX_PAD: f64 = 8.0;
/// Rows of a result shown before it scrolls (the default matrix display is 12 x 12).
const MAX_RESULT_ROWS: f64 = 12.0;
/// Height of a sum, product, integral or limit with its bounds.
const BIG_OPERATOR: f64 = 60.0;
const MAX_DEPTH: usize = 24;

/// The height region `r` takes, in page points: its stated size when it has one, else an estimate
/// from its content. `all` is the worksheet's regions, used to size the result of a name.
#[must_use]
pub fn region_height(r: &RegionModel, all: &[RegionModel]) -> f64 {
    let result = match &r.kind {
        RegionKind::Math(Expr::Eval { expr, .. }) => rows_height(rows(expr, all, 0)),
        _ => 0.0,
    };
    base_height(r, result)
}

/// An upper bound of [`region_height`] for `r` that needs no other region. An evaluation's result
/// shows at most [`MAX_RESULT_ROWS`] rows, so the bound is the height with that many.
#[must_use]
pub fn region_height_bound(r: &RegionModel) -> f64 {
    let result = match &r.kind {
        RegionKind::Math(Expr::Eval { .. }) => rows_height(MAX_RESULT_ROWS),
        _ => 0.0,
    };
    base_height(r, result)
}

/// The height of `r` with its evaluation result (if any) of height `result`.
fn base_height(r: &RegionModel, result: f64) -> f64 {
    let stated = r.size.map(|s| s.1);
    match &r.kind {
        RegionKind::Math(e) => height(e, 0).max(result).max(stated.unwrap_or(0.0)),
        _ => stated.unwrap_or(LINE),
    }
}

/// The lowest bottom edge (`y` plus [`region_height`]) of the positioned regions, or `None` when
/// none has a position. The same value as taking [`region_height`] of every region, but a region
/// is measured only while its bound ([`region_height_bound`]) can still reach the lowest edge
/// found, so a worksheet of evaluations is not searched once per region.
#[must_use]
pub fn lowest_bottom(regions: &[RegionModel]) -> Option<f64> {
    // The bound of each positioned region's bottom edge (`None` when it has no position).
    let upper: Vec<Option<f64>> = regions
        .iter()
        .map(|r| r.pos.map(|(_, y)| y + region_height_bound(r)))
        .collect();
    // The region with the highest bound is measured first: its exact bottom is the best so far,
    // and every region whose bound is not above it is ruled out without a measurement.
    let (_, first) = upper
        .iter()
        .enumerate()
        .filter_map(|(i, u)| u.map(|u| (u, i)))
        .max_by(|a, b| a.0.total_cmp(&b.0))?;
    let bottom = |i: usize| -> f64 {
        let y = regions[i].pos.map_or(0.0, |(_, y)| y);
        y + region_height(&regions[i], regions)
    };
    let mut best = bottom(first);
    for (i, u) in upper.iter().enumerate() {
        if u.is_some_and(|u| u > best) {
            best = best.max(bottom(i));
        }
    }
    Some(best)
}

fn rows_height(rows: f64) -> f64 {
    if rows <= 1.0 {
        LINE
    } else {
        rows * LINE + MATRIX_PAD
    }
}

/// Height of the drawn expression, results excluded.
fn height(e: &Expr, depth: usize) -> f64 {
    if depth > MAX_DEPTH {
        return LINE;
    }
    let d = depth + 1;
    match e {
        Expr::Binary {
            op: BinaryOp::Div,
            lhs,
            rhs,
        } => height(lhs, d) + height(rhs, d) + 6.0,
        Expr::Binary { lhs, rhs, .. } => height(lhs, d).max(height(rhs, d)) + 0.0,
        Expr::Unary { op, operand } => {
            let h = height(operand, d);
            if matches!(op, UnaryOp::Abs) {
                h
            } else {
                h.max(LINE)
            }
        }
        Expr::Parens(i) => height(i, d),
        Expr::Call { args, .. } => args.iter().map(|a| height(a, d)).fold(LINE, f64::max),
        Expr::Matrix {
            rows,
            cols,
            elements,
        } => {
            let cell = elements
                .iter()
                .take(rows.saturating_mul(*cols).min(1024))
                .map(|x| height(x, d))
                .fold(LINE, f64::max);
            (*rows as f64) * cell + MATRIX_PAD
        }
        Expr::Accumulate { body, .. } => BIG_OPERATOR.max(height(body, d) + 36.0),
        Expr::Fold { operand, .. } => BIG_OPERATOR.max(height(operand, d) + 36.0),
        Expr::Integral { body, .. } => BIG_OPERATOR.max(height(body, d) + 36.0),
        Expr::Limit { body, .. } | Expr::Derivative { body, .. } => {
            (height(body, d) + 12.0).max(40.0)
        }
        Expr::NthRoot { radicand, .. } => height(radicand, d) + 6.0,
        Expr::Index { base, .. } => height(base, d),
        Expr::Define { expr, .. } | Expr::GlobalDefine { expr, .. } => height(expr, d),
        Expr::Eval { expr, .. } => height(expr, d),
        Expr::SymEval { expr, .. } => height(expr, d).max(LINE * 2.0),
        Expr::Scale { value, unit } => height(value, d).max(height(unit, d)),
        Expr::Program(stmts) => (stmts.len().max(1) as f64) * LINE,
        _ => LINE,
    }
}

/// How many rows the value of `e` shows (1 for a scalar), capped at the matrix display.
fn rows(e: &Expr, all: &[RegionModel], depth: usize) -> f64 {
    if depth > 8 {
        return 1.0;
    }
    let d = depth + 1;
    let n = match e {
        Expr::Matrix { rows, .. } => *rows as f64,
        Expr::Id(id) => defined_rows(id, all, d),
        Expr::Parens(i) => rows(i, all, d),
        Expr::Binary { lhs, rhs, .. } => rows(lhs, all, d).max(rows(rhs, all, d)),
        Expr::Unary {
            op: UnaryOp::Transpose,
            operand,
        } => cols(operand, all, d),
        Expr::Unary { operand, .. } => rows(operand, all, d),
        Expr::Scale { value, .. } => rows(value, all, d),
        Expr::Range { from, step, to } => range_len(from, step.as_deref(), to),
        Expr::Call { args, .. } => args
            .iter()
            .map(|a| rows(a, all, d))
            .fold(1.0, f64::max)
            .max(1.0),
        Expr::Index { .. } => 1.0,
        _ => 1.0,
    };
    n.clamp(1.0, MAX_RESULT_ROWS)
}

fn cols(e: &Expr, all: &[RegionModel], depth: usize) -> f64 {
    match e {
        Expr::Matrix { cols, .. } => *cols as f64,
        Expr::Id(id) => match definition(id, all) {
            Some(rhs) if depth < 8 => cols(rhs, all, depth + 1),
            _ => 1.0,
        },
        _ => 1.0,
    }
}

fn definition<'a>(id: &Id, all: &'a [RegionModel]) -> Option<&'a Expr> {
    all.iter().rev().find_map(|r| match &r.kind {
        RegionKind::Math(Expr::Define { target, expr } | Expr::GlobalDefine { target, expr }) => {
            match &**target {
                Expr::Id(t) if t.name == id.name && t.subscript == id.subscript => Some(&**expr),
                _ => None,
            }
        }
        _ => None,
    })
}

fn defined_rows(id: &Id, all: &[RegionModel], depth: usize) -> f64 {
    match definition(id, all) {
        Some(rhs) => rows(rhs, all, depth),
        None => 1.0,
    }
}

fn number(e: &Expr) -> Option<f64> {
    match e {
        Expr::Number(Number::Real { text, .. }) => text.parse().ok(),
        Expr::Number(Number::Rational(r)) => r
            .numer()
            .to_string()
            .parse::<f64>()
            .ok()
            .and_then(|n| r.denom().to_string().parse::<f64>().ok().map(|d| n / d)),
        _ => None,
    }
}

/// Length of a range with literal ends; the display cap when the ends are not literals.
fn range_len(from: &Expr, step: Option<&Expr>, to: &Expr) -> f64 {
    let (Some(a), Some(b)) = (number(from), number(to)) else {
        return MAX_RESULT_ROWS;
    };
    let s = match step {
        Some(s) => number(s).map(|n| n - a),
        None => Some(if b >= a { 1.0 } else { -1.0 }),
    };
    match s {
        Some(s) if s != 0.0 && ((b - a) / s).is_finite() => ((b - a) / s).floor().max(0.0) + 1.0,
        _ => MAX_RESULT_ROWS,
    }
}

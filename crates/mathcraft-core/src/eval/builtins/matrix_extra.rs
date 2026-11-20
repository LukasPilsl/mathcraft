//! Lookup functions, ranking, the Kronecker product and log-spaced vectors.
//!
//! Help: "Functions > Vector and Matrix > Other Array Functions > Lookup Functions", "Ranking Vectors",
//! "Kronecker Product", "Logarithmically Spaced Points". Lookups count rows and columns from
//! `ORIGIN`, so they are context builtins.
//!
//! Comparison modifiers, as read from the help examples ("Example: Effect of TOL on Lookup
//! Functions") with `t` the value of `TOL` (SI magnitudes; `TOL` is compared to the difference of
//! the SI values):
//! - `eq`: `|a - z| <= t`; `not`: its negation.
//! - `lt`: `a < z - t`; `gt`: `a > z + t`; `geq`: `a >= z - t`; `leq`: `a <= z + t`.
//! - `near`: `|a - z| <= d + t`, where `d` is the smallest `|a - z|` over the searched elements.
//! - `range`: `z` is a two-element column `[lo; hi]` and the match is `lo - t <= a <= hi + t`.
//!
//! Not built: the string comparison operands and the user comparison function `f`. Matrices hold
//! only numbers here, so a string `z` or string element is refused, and so is a complex element or
//! operand (the comparisons are on real values). A lookup with no match is an error (the help does
//! not say what Prime returns; this is a choice, see the report).
//!
//! `Rank` gives 1-based sorted positions with ties averaged; `order` gives the indices that sort the
//! vector (ties in original order, counted from `ORIGIN`). `match` returns a nested array `[rows;
//! cols]` of the matching indices, column by column, counted from `ORIGIN`.

use super::matrix::expect_matrix;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::indexing::zero_based;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use crate::units::Dimension;

pub fn register(r: &mut Registry) {
    r.register_ctx("lookup", Arity::Range(3, 4), lookup);
    r.register_ctx("hlookup", Arity::Range(3, 4), hlookup);
    r.register_ctx("vlookup", Arity::Range(3, 4), vlookup);
    r.register_ctx("vhlookup", Arity::Range(3, 4), vhlookup);
    r.register_ctx("match", Arity::Range(2, 3), match_indices);
    r.register("Rank", Arity::Exact(1), rank);
    r.register_ctx("order", Arity::Exact(1), order);
    r.register("kronecker", Arity::Exact(2), kronecker);
    r.register("logpts", Arity::Exact(3), logpts);
    r.register("logspace", Arity::Exact(3), logspace);
}

// ---- comparisons ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    Eq,
    Near,
    Gt,
    Lt,
    Geq,
    Leq,
    Not,
    Range,
}

/// The modifier argument (`"eq"` when absent).
fn rule_of(v: Option<&Value>) -> Result<Rule, EvalError> {
    let Some(v) = v else {
        return Ok(Rule::Eq);
    };
    let Value::Str(s) = v else {
        return Err(EvalError::Unsupported(
            "a lookup modifier must be a string (the user comparison function is not built)"
                .to_string(),
        ));
    };
    Ok(match s.as_str() {
        "eq" => Rule::Eq,
        "near" => Rule::Near,
        "gt" => Rule::Gt,
        "lt" => Rule::Lt,
        "geq" => Rule::Geq,
        "leq" => Rule::Leq,
        "not" => Rule::Not,
        "range" => Rule::Range,
        other => {
            return Err(EvalError::Unsupported(format!(
                "unknown lookup modifier \"{other}\""
            )))
        }
    })
}

/// The searched value `z` with its rule. `range` keeps `[lo, hi]` in `bounds`.
#[derive(Debug, Clone, Copy)]
struct Probe {
    z: f64,
    dim: Dimension,
    rule: Rule,
    tol: f64,
    bounds: (f64, f64),
}

/// The real SI value of a scalar operand, refusing complex and non-finite values.
fn real_operand(s: &Scalar) -> Result<f64, EvalError> {
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(
            "lookup comparisons take real values".to_string(),
        ));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

fn tol_of(ctx: &BuiltinCtx) -> Result<f64, EvalError> {
    let tol = ctx.tol()?;
    if tol.is_finite() && tol >= 0.0 {
        Ok(tol)
    } else {
        Err(EvalError::Unsupported(
            "TOL must be a finite, non-negative number".to_string(),
        ))
    }
}

/// Builds the probe from the operand `z` (a scalar, or a `[lo; hi]` column for `range`).
fn probe_of(z: &Value, rule: Rule, tol: f64) -> Result<Probe, EvalError> {
    if rule == Rule::Range {
        let Value::Matrix(m) = z else {
            return Err(EvalError::Unsupported(
                "the range modifier needs a two-element column [lo; hi]".to_string(),
            ));
        };
        if m.rows() != 2 || m.cols() != 1 {
            return Err(EvalError::Unsupported(
                "the range modifier needs a two-element column [lo; hi]".to_string(),
            ));
        }
        let lo = real_operand(m.get(0, 0)?)?;
        let hi = real_operand(m.get(1, 0)?)?;
        return Ok(Probe {
            z: lo,
            dim: m.get(0, 0)?.dim,
            rule,
            tol,
            bounds: (lo, hi),
        });
    }
    let Value::Scalar(s) = z else {
        return Err(EvalError::Unsupported(
            "a lookup value z must be a scalar (string and array operands are not built)"
                .to_string(),
        ));
    };
    let x = real_operand(s)?;
    Ok(Probe {
        z: x,
        dim: s.dim,
        rule,
        tol,
        bounds: (x, x),
    })
}

/// Whether the element `s` satisfies the probe. `near_limit` is the `near` bound (see the module
/// docs); other rules ignore it.
fn accepts(p: &Probe, s: &Scalar, near_limit: f64) -> bool {
    if s.dim != p.dim || s.value.im != 0.0 || !s.value.re.is_finite() {
        return false;
    }
    let a = s.value.re;
    let t = p.tol;
    // `eq` and `not` compare relative to the magnitude of the values: a difference counts as zero
    // when it is within TOL of the larger magnitude. An absolute TOL made lookup(1e-6, {1e-5, ...})
    // match 1e-5, because SI values of any size were compared to 0.001. The help does not define
    // `eq` with TOL; the one-sided rules and `range` keep the absolute TOL that the help example
    // "Effect of TOL on Lookup Functions" shows (at TOL 0.7 only the value 2.6 drops out of `lt 3`).
    // `near` is the nearest distance plus TOL, also from that example.
    match p.rule {
        Rule::Eq => (a - p.z).abs() <= relative_slack(t, a, p.z),
        Rule::Not => (a - p.z).abs() > relative_slack(t, a, p.z),
        Rule::Near => (a - p.z).abs() <= near_limit,
        Rule::Lt => a < p.z - t,
        Rule::Gt => a > p.z + t,
        Rule::Geq => a >= p.z - t,
        Rule::Leq => a <= p.z + t,
        Rule::Range => a >= p.bounds.0 - t && a <= p.bounds.1 + t,
    }
}

/// The slack TOL allows between `a` and `z` for `eq`: `tol` times the larger of their magnitudes.
fn relative_slack(tol: f64, a: f64, z: f64) -> f64 {
    tol * a.abs().max(z.abs())
}

/// The `near` bound over the searched cells: the smallest distance plus `TOL`.
fn near_limit(p: &Probe, cells: &[(usize, usize, &Scalar)]) -> f64 {
    if p.rule != Rule::Near {
        return 0.0;
    }
    let nearest = cells
        .iter()
        .filter(|(_, _, s)| s.dim == p.dim && s.value.im == 0.0 && s.value.re.is_finite())
        .map(|(_, _, s)| (s.value.re - p.z).abs())
        .fold(f64::INFINITY, f64::min);
    if nearest.is_finite() {
        nearest + p.tol
    } else {
        f64::NEG_INFINITY
    }
}

/// Indices into `cells` of the elements that satisfy the probe, in the order given.
fn hits(p: &Probe, cells: &[(usize, usize, &Scalar)]) -> Vec<usize> {
    let limit = near_limit(p, cells);
    cells
        .iter()
        .enumerate()
        .filter(|(_, (_, _, s))| accepts(p, s, limit))
        .map(|(k, _)| k)
        .collect()
}

// ---- helpers --------------------------------------------------------------------------------

fn arg<'a>(args: &'a [Value], k: usize, name: &str) -> Result<&'a Value, EvalError> {
    args.get(k)
        .ok_or_else(|| EvalError::Unsupported(format!("{name} is missing an argument")))
}

/// A dimensionless integer argument (an index or a count), without ORIGIN.
fn integer_arg(v: &Value, name: &str) -> Result<i64, EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!("{name} needs an integer")));
    };
    ops::require_dimensionless(s.dim)?;
    integer_of(s).ok_or_else(|| EvalError::Unsupported(format!("{name} needs an integer")))
}

fn origin_index(zero: usize, origin: i64) -> Result<i64, EvalError> {
    (zero as i64).checked_add(origin).ok_or(EvalError::Overflow)
}

fn none_found(name: &str) -> EvalError {
    EvalError::Unsupported(format!("{name}: no element matches z"))
}

/// The cells of `m` in column-major order (the sweep order of the lookups), with row and column
/// positions counted from zero.
fn column_cells(m: &Matrix) -> Vec<(usize, usize, &Scalar)> {
    let mut out = Vec::with_capacity(m.rows() * m.cols());
    for j in 0..m.cols() {
        for i in 0..m.rows() {
            if let Ok(s) = m.get(i, j) {
                out.push((i, j, s));
            }
        }
    }
    out
}

/// A column vector of the given scalars (each keeps its own dimension).
fn column_of(items: Vec<Scalar>) -> Result<Value, EvalError> {
    let n = items.len();
    Ok(Value::Matrix(Matrix::new(n, 1, items)?))
}

// ---- lookups --------------------------------------------------------------------------------

/// `lookup(z, A, B, [modifier])`: the elements of `B` at the positions where `A` matches `z`,
/// column by column.
fn lookup(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let a = expect_matrix(arg(args, 1, "lookup")?, "lookup")?;
    let b = expect_matrix(arg(args, 2, "lookup")?, "lookup")?;
    if a.shape() != b.shape() {
        return Err(EvalError::ShapeMismatch {
            left: a.shape(),
            right: b.shape(),
        });
    }
    let probe = probe_of(arg(args, 0, "lookup")?, rule_of(args.get(3))?, tol_of(ctx)?)?;
    let cells = column_cells(a);
    let found = hits(&probe, &cells);
    if found.is_empty() {
        return Err(none_found("lookup"));
    }
    let items = found
        .into_iter()
        .map(|k| b.get(cells[k].0, cells[k].1).cloned())
        .collect::<Result<Vec<_>, _>>()?;
    column_of(items)
}

/// `hlookup(z, A, r, [modifier])`: row `r` of `A` at the columns whose first row matches `z`.
fn hlookup(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let a = expect_matrix(arg(args, 1, "hlookup")?, "hlookup")?;
    let r = zero_based(
        integer_arg(arg(args, 2, "hlookup")?, "hlookup")?,
        ctx.origin()?,
    )?;
    if r >= a.rows() {
        return Err(EvalError::IndexOutOfRange {
            index: r,
            bound: a.rows(),
        });
    }
    let probe = probe_of(
        arg(args, 0, "hlookup")?,
        rule_of(args.get(3))?,
        tol_of(ctx)?,
    )?;
    let first_row: Vec<(usize, usize, &Scalar)> = (0..a.cols())
        .map(|j| Ok((0, j, a.get(0, j)?)))
        .collect::<Result<_, EvalError>>()?;
    let found = hits(&probe, &first_row);
    if found.is_empty() {
        return Err(none_found("hlookup"));
    }
    let items = found
        .into_iter()
        .map(|k| a.get(r, first_row[k].1).cloned())
        .collect::<Result<Vec<_>, _>>()?;
    column_of(items)
}

/// `vlookup(z, A, c, [modifier])`: column `c` of `A` at the rows whose first column matches `z`.
fn vlookup(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let a = expect_matrix(arg(args, 1, "vlookup")?, "vlookup")?;
    let c = zero_based(
        integer_arg(arg(args, 2, "vlookup")?, "vlookup")?,
        ctx.origin()?,
    )?;
    if c >= a.cols() {
        return Err(EvalError::IndexOutOfRange {
            index: c,
            bound: a.cols(),
        });
    }
    let probe = probe_of(
        arg(args, 0, "vlookup")?,
        rule_of(args.get(3))?,
        tol_of(ctx)?,
    )?;
    let first_col: Vec<(usize, usize, &Scalar)> = (0..a.rows())
        .map(|i| Ok((i, 0, a.get(i, 0)?)))
        .collect::<Result<_, EvalError>>()?;
    let found = hits(&probe, &first_col);
    if found.is_empty() {
        return Err(none_found("vlookup"));
    }
    let items = found
        .into_iter()
        .map(|k| a.get(first_col[k].0, c).cloned())
        .collect::<Result<Vec<_>, _>>()?;
    column_of(items)
}

/// `vhlookup(z1, z2, A, [modifier])`: the elements at the crossings of the rows whose first column
/// matches `z1` and the columns whose first row matches `z2`, column by column.
fn vhlookup(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let a = expect_matrix(arg(args, 2, "vhlookup")?, "vhlookup")?;
    let rule = rule_of(args.get(3))?;
    let tol = tol_of(ctx)?;
    let p_row = probe_of(arg(args, 0, "vhlookup")?, rule, tol)?;
    let p_col = probe_of(arg(args, 1, "vhlookup")?, rule, tol)?;
    let first_col: Vec<(usize, usize, &Scalar)> = (0..a.rows())
        .map(|i| Ok((i, 0, a.get(i, 0)?)))
        .collect::<Result<_, EvalError>>()?;
    let first_row: Vec<(usize, usize, &Scalar)> = (0..a.cols())
        .map(|j| Ok((0, j, a.get(0, j)?)))
        .collect::<Result<_, EvalError>>()?;
    let rows = hits(&p_row, &first_col);
    let cols = hits(&p_col, &first_row);
    let mut items = Vec::with_capacity(rows.len() * cols.len());
    for &c in &cols {
        for &r in &rows {
            items.push(a.get(first_col[r].0, first_row[c].1).cloned()?);
        }
    }
    if items.is_empty() {
        return Err(none_found("vhlookup"));
    }
    column_of(items)
}

/// `match(z, A, [modifier])`: the `[rows; cols]` positions of the matches, counted from `ORIGIN`,
/// as a nested array of two column vectors.
fn match_indices(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let a = expect_matrix(arg(args, 1, "match")?, "match")?;
    let probe = probe_of(arg(args, 0, "match")?, rule_of(args.get(2))?, tol_of(ctx)?)?;
    let origin = ctx.origin()?;
    let cells = column_cells(a);
    let found = hits(&probe, &cells);
    if found.is_empty() {
        return Err(none_found("match"));
    }
    let mut row_items = Vec::with_capacity(found.len());
    let mut col_items = Vec::with_capacity(found.len());
    for k in found {
        let (i, j, _) = cells[k];
        row_items.push(Scalar::real(
            origin_index(i, origin)? as f64,
            Dimension::dimensionless(),
        ));
        col_items.push(Scalar::real(
            origin_index(j, origin)? as f64,
            Dimension::dimensionless(),
        ));
    }
    Ok(Value::List(vec![
        column_of(row_items)?,
        column_of(col_items)?,
    ]))
}

// ---- ranking --------------------------------------------------------------------------------

/// The real keys of a vector (row or column), refusing complex and non-finite elements.
fn vector_keys(v: &Value, name: &str) -> Result<(usize, usize, Vec<f64>), EvalError> {
    let m = expect_matrix(v, name)?;
    if m.rows() != 1 && m.cols() != 1 {
        return Err(EvalError::Unsupported(format!("{name} needs a vector")));
    }
    let mut keys = Vec::with_capacity(m.rows() * m.cols());
    for s in m.data() {
        if s.value.im != 0.0 {
            return Err(EvalError::Unsupported(format!("{name} needs real values")));
        }
        if !s.value.re.is_finite() {
            return Err(EvalError::NotFinite);
        }
        keys.push(s.value.re);
    }
    Ok((m.rows(), m.cols(), keys))
}

/// Indices that sort `keys` ascending; ties keep their original order.
fn sorted_order(keys: &[f64]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..keys.len()).collect();
    idx.sort_by(|&a, &b| keys[a].total_cmp(&keys[b]).then(a.cmp(&b)));
    idx
}

/// `Rank(v)`: for each element, its 1-based sorted position, with tied elements given the average
/// of the positions they share.
fn rank(args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, keys) = vector_keys(arg(args, 0, "Rank")?, "Rank")?;
    let order = sorted_order(&keys);
    let mut out = vec![0.0; keys.len()];
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && keys[order[end]] == keys[order[start]] {
            end += 1;
        }
        // Positions start..end (0-based) share the average 1-based position.
        let average = (start + 1 + end) as f64 / 2.0;
        for &k in &order[start..end] {
            out[k] = average;
        }
        start = end;
    }
    let data = out
        .into_iter()
        .map(|x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// `order(v)`: the indices that sort `v` ascending, counted from `ORIGIN`.
fn order(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, keys) = vector_keys(arg(args, 0, "order")?, "order")?;
    let origin = ctx.origin()?;
    let data = sorted_order(&keys)
        .into_iter()
        .map(|k| {
            origin_index(k, origin).map(|i| Scalar::real(i as f64, Dimension::dimensionless()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

// ---- products and sequences -----------------------------------------------------------------

/// `kronecker(M, N)`: the block matrix whose `(i, j)` block is `M(i, j) N`. Element dimensions
/// multiply, as in an ordinary product.
fn kronecker(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args, 0, "kronecker")?, "kronecker")?;
    let n = expect_matrix(arg(args, 1, "kronecker")?, "kronecker")?;
    let rows = m.rows().checked_mul(n.rows()).ok_or(EvalError::Overflow)?;
    let cols = m.cols().checked_mul(n.cols()).ok_or(EvalError::Overflow)?;
    let count = rows.checked_mul(cols).ok_or(EvalError::Overflow)?;
    if count > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(count));
    }
    let mut data = Vec::with_capacity(count);
    for r in 0..rows {
        for c in 0..cols {
            let a = m.get(r / n.rows(), c / n.cols())?;
            let b = n.get(r % n.rows(), c % n.cols())?;
            let product = ops::mul(&Value::Scalar(*a), &Value::Scalar(*b))?;
            match product {
                Value::Scalar(s) => data.push(s),
                _ => {
                    return Err(EvalError::Unsupported(
                        "kronecker product failed".to_string(),
                    ))
                }
            }
        }
    }
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// The positive integer `v` as a count, bounded by `MAX_ELEMENTS`.
fn count_arg(v: &Value, name: &str) -> Result<usize, EvalError> {
    let n = integer_arg(v, name)?;
    if n < 1 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a positive integer"
        )));
    }
    usize::try_from(n).map_err(|_| EvalError::Overflow)
}

/// `logpts(minexp, dec, dnpts)`: `dec` decades from `10^minexp`, each with `dnpts` points that are
/// evenly spaced on the linear scale of the decade, starting at its first value and not reaching
/// the next decade (`logpts(-1, 1, 9)` is `0.1, 0.2, ..., 0.9`). The points of decade `d`, `k`
/// from 0, are `10^(minexp + d) (1 + 9 k / dnpts)`.
fn logpts(args: &[Value]) -> Result<Value, EvalError> {
    let Value::Scalar(start) = arg(args, 0, "logpts")? else {
        return Err(EvalError::Unsupported(
            "logpts needs a scalar exponent".to_string(),
        ));
    };
    ops::require_dimensionless(start.dim)?;
    let minexp = real_operand(start)?;
    let dec = count_arg(arg(args, 1, "logpts")?, "logpts")?;
    let per = count_arg(arg(args, 2, "logpts")?, "logpts")?;
    let total = dec.checked_mul(per).ok_or(EvalError::Overflow)?;
    if total > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(total));
    }
    let mut data = Vec::with_capacity(total);
    for d in 0..dec {
        let base = 10f64.powf(minexp + d as f64);
        for k in 0..per {
            let x = base * (1.0 + 9.0 * k as f64 / per as f64);
            // A point that underflows to zero (logpts(-400, 2, 3)) or overflows is not a point
            // the worksheet can use; refuse it rather than return a zero.
            if !x.is_finite() || x == 0.0 {
                return Err(EvalError::Unsupported(format!(
                    "logpts: the point at decade exponent {} is outside the range of real numbers",
                    minexp + d as f64
                )));
            }
            data.push(Scalar::real(x, Dimension::dimensionless()));
        }
    }
    Ok(Value::Matrix(Matrix::new(total, 1, data)?))
}

/// `logspace(min, max, npts)`: `npts` (at least 2) geometrically spaced points from `min` to `max`,
/// both included. `min` and `max` must be positive and share a dimension.
fn logspace(args: &[Value]) -> Result<Value, EvalError> {
    let (Value::Scalar(lo), Value::Scalar(hi)) =
        (arg(args, 0, "logspace")?, arg(args, 1, "logspace")?)
    else {
        return Err(EvalError::Unsupported(
            "logspace needs scalar bounds".to_string(),
        ));
    };
    if lo.dim != hi.dim {
        return Err(EvalError::DimensionMismatch {
            left: lo.dim,
            right: hi.dim,
        });
    }
    let (a, b) = (real_operand(lo)?, real_operand(hi)?);
    if a <= 0.0 || b <= 0.0 {
        return Err(EvalError::Unsupported(
            "logspace needs positive bounds".to_string(),
        ));
    }
    let n = count_arg(arg(args, 2, "logspace")?, "logspace")?;
    if n < 2 {
        return Err(EvalError::Unsupported(
            "logspace needs at least 2 points".to_string(),
        ));
    }
    if n > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(n));
    }
    let ratio = (b / a).powf(1.0 / (n - 1) as f64);
    let mut data = Vec::with_capacity(n);
    for k in 0..n {
        let x = if k == 0 {
            a
        } else if k == n - 1 {
            b
        } else {
            a * ratio.powi(k as i32)
        };
        if !x.is_finite() {
            return Err(EvalError::NotFinite);
        }
        data.push(Scalar::real(x, lo.dim));
    }
    Ok(Value::Matrix(Matrix::new(n, 1, data)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> BuiltinCtx<'static> {
        BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3))
    }

    fn dl(x: f64) -> Scalar {
        Scalar::real(x, Dimension::dimensionless())
    }

    fn col(xs: &[f64]) -> Value {
        Value::Matrix(Matrix::new(xs.len(), 1, xs.iter().map(|&x| dl(x)).collect()).unwrap())
    }

    /// A matrix from rows.
    fn mat(rows: &[&[f64]]) -> Value {
        let r = rows.len();
        let c = rows[0].len();
        let data = rows
            .iter()
            .flat_map(|row| row.iter().map(|&x| dl(x)))
            .collect();
        Value::Matrix(Matrix::new(r, c, data).unwrap())
    }

    fn values(v: &Value) -> Vec<f64> {
        match v {
            Value::Matrix(m) => m.data().iter().map(|s| s.value.re).collect(),
            other => panic!("expected a matrix, got {other:?}"),
        }
    }

    fn s(text: &str) -> Value {
        Value::Str(text.to_string())
    }

    /// The help example matrix A of "Effect of TOL on Lookup Functions" without its header row
    /// (strings are not matrix elements here): columns time, degrees, volts.
    fn tol_example() -> Value {
        mat(&[
            &[1.0, 96.5, 2.6],
            &[2.0, 97.1, 3.1],
            &[2.0, 97.5, 3.3],
            &[4.0, 97.1, 3.5],
            &[5.0, 98.3, 3.8],
            &[6.0, 98.5, 3.9],
        ])
    }

    #[test]
    fn lookup_exact_match_returns_the_paired_element() {
        let x = mat(&[&[1.0, 96.5], &[50.0, 98.0], &[56.0, 99.0]]);
        let y = mat(&[&[10.0, 71.0], &[20.0, 85.0], &[30.0, 63.0]]);
        // 50 sits at row 1 of column 0 only, so the paired element is y(1, 0).
        let got = lookup(&ctx(), &[dl_value(50.0), x, y]).unwrap();
        assert_eq!(values(&got), vec![20.0]);
    }

    fn dl_value(x: f64) -> Value {
        Value::Scalar(dl(x))
    }

    #[test]
    fn lookup_gt_and_range_follow_the_help_examples() {
        let x = col(&[1.0, 2.0, 3.0, 4.0, 50.0, 56.0]);
        let y = col(&[10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);
        // z3 := [50; 70] with "range": elements 50 and 56.
        let got = lookup(
            &ctx(),
            &[col(&[50.0, 70.0]), x.clone(), y.clone(), s("range")],
        )
        .unwrap();
        assert_eq!(values(&got), vec![50.0, 60.0]);
        // "lt" with z = 4: 1, 2, 3 (4 itself is not less than 4).
        let got = lookup(&ctx(), &[dl_value(4.0), x, y, s("lt")]).unwrap();
        assert_eq!(values(&got), vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn match_lt_follows_the_tol_example() {
        let a = tol_example();
        let got = match_indices(&ctx(), &[dl_value(3.0), a.clone(), s("lt")]).unwrap();
        // Column-wise, counted over the data rows (the help's header row is not a matrix here):
        // column 0 rows 0, 1, 2 (values 1, 2, 2), then column 2 row 0 (2.6).
        match got {
            Value::List(parts) => {
                assert_eq!(values(&parts[0]), vec![0.0, 1.0, 2.0, 0.0]);
                assert_eq!(values(&parts[1]), vec![0.0, 0.0, 0.0, 2.0]);
            }
            other => panic!("{other:?}"),
        }
        // TOL 0.7: 2.6 is no longer less than 3 - 0.7.
        let tight = BuiltinCtx::new(Ok(0), Ok(0.7), Ok(0.7));
        let got = match_indices(&tight, &[dl_value(3.0), a, s("lt")]).unwrap();
        match got {
            Value::List(parts) => assert_eq!(values(&parts[0]), vec![0.0, 1.0, 2.0]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn match_near_takes_the_nearest_plus_tol() {
        let a = tol_example();
        // Column 1 (degrees) near 97.3: nearest 97.1 (distance 0.2); rows 2, 3, 4 (1-based 1..3).
        let got = match_indices(&ctx(), &[dl_value(97.3), a.clone(), s("near")]).unwrap();
        match got {
            Value::List(parts) => {
                assert_eq!(values(&parts[0]), vec![1.0, 2.0, 3.0]);
                assert_eq!(values(&parts[1]), vec![1.0, 1.0, 1.0]);
            }
            other => panic!("{other:?}"),
        }
        // TOL 0.7 admits 96.5 (distance 0.8 <= 0.2 + 0.7).
        let wide = BuiltinCtx::new(Ok(0), Ok(0.7), Ok(0.7));
        let got = match_indices(&wide, &[dl_value(97.3), a, s("near")]).unwrap();
        match got {
            Value::List(parts) => assert_eq!(values(&parts[0]).len(), 4),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn vlookup_near_returns_the_rows_within_tol() {
        // The help: vlookup(1.5, A, 2, "near") = [2.6; 3.1; 3.3] with TOL 0.001.
        let a = tol_example();
        let got = vlookup(
            &ctx(),
            &[dl_value(1.5), a.clone(), dl_value(2.0), s("near")],
        )
        .unwrap();
        assert_eq!(values(&got), vec![2.6, 3.1, 3.3]);
        let wide = BuiltinCtx::new(Ok(0), Ok(2.5), Ok(2.5));
        let got = vlookup(&wide, &[dl_value(1.5), a, dl_value(2.0), s("near")]).unwrap();
        assert_eq!(values(&got), vec![2.6, 3.1, 3.3, 3.5]);
    }

    #[test]
    fn vlookup_eq_and_hlookup_pick_rows_and_columns() {
        let a = tol_example();
        // Row 3 (value 4) holds 97.1 in column 1 and 3.5 in column 2.
        let got = vlookup(&ctx(), &[dl_value(4.0), a.clone(), dl_value(1.0)]).unwrap();
        assert_eq!(values(&got), vec![97.1]);
        let got = vlookup(&ctx(), &[dl_value(4.0), a.clone(), dl_value(2.0)]).unwrap();
        assert_eq!(values(&got), vec![3.5]);
        let got =
            hlookup(&ctx(), &[dl_value(97.5), a.clone(), dl_value(2.0), s("eq")]).unwrap_err();
        // Row 0 holds 1, 96.5, 2.6, so 97.5 is found nowhere in the first row.
        assert!(matches!(got, EvalError::Unsupported(_)));
        let first = mat(&[&[3.0, 5.0, 7.0], &[1.0, 2.0, 3.0]]);
        let got = hlookup(&ctx(), &[dl_value(5.0), first, dl_value(1.0), s("eq")]).unwrap();
        assert_eq!(values(&got), vec![2.0]);
    }

    #[test]
    fn vhlookup_returns_the_crossings_column_by_column() {
        // Row 0 holds 1, 10, 30 (30 is in column 2); column 0 holds 1, 2, 2 (rows 1 and 2).
        let b = mat(&[&[1.0, 10.0, 30.0], &[2.0, 20.0, 200.0], &[2.0, 30.0, 300.0]]);
        let got = vhlookup(&ctx(), &[dl_value(2.0), dl_value(30.0), b]).unwrap();
        // Column 2 crossed with rows 1 and 2: values 200, 300.
        assert_eq!(values(&got), vec![200.0, 300.0]);
    }

    #[test]
    fn origin_shifts_match_indices_and_selects_rows() {
        let shifted = BuiltinCtx::new(Ok(1), Ok(1e-3), Ok(1e-3));
        let a = mat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let got = match_indices(&shifted, &[dl_value(4.0), a.clone()]).unwrap();
        match got {
            Value::List(parts) => {
                assert_eq!(values(&parts[0]), vec![2.0]);
                assert_eq!(values(&parts[1]), vec![2.0]);
            }
            other => panic!("{other:?}"),
        }
        // hlookup row index 2 with ORIGIN 1 is the second row.
        let got = hlookup(&shifted, &[dl_value(2.0), a, dl_value(2.0)]).unwrap();
        assert_eq!(values(&got), vec![4.0]);
    }

    #[test]
    fn lookups_refuse_bad_input() {
        let a = tol_example();
        assert!(matches!(
            match_indices(&ctx(), &[dl_value(1.0), a.clone(), s("bogus")]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            match_indices(&ctx(), &[dl_value(f64::NAN), a.clone()]),
            Err(EvalError::NotFinite)
        ));
        assert!(matches!(
            vlookup(&ctx(), &[dl_value(1.0), a.clone(), dl_value(9.0)]),
            Err(EvalError::IndexOutOfRange { .. })
        ));
        assert!(matches!(
            lookup(&ctx(), &[dl_value(1.0), a.clone(), col(&[1.0])]),
            Err(EvalError::ShapeMismatch { .. })
        ));
        assert!(matches!(
            match_indices(&ctx(), &[dl_value(99.0), a]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn rank_averages_ties_and_order_is_stable() {
        let v = col(&[-2.0, 3.0, -1.0, 1.0]);
        assert_eq!(values(&rank(&[v]).unwrap()), vec![1.0, 4.0, 2.0, 3.0]);
        let v = col(&[-2.0, 3.0, -1.0, 3.0]);
        assert_eq!(
            values(&rank(std::slice::from_ref(&v)).unwrap()),
            vec![1.0, 3.5, 2.0, 3.5]
        );
        assert_eq!(
            values(&order(&ctx(), &[v]).unwrap()),
            vec![0.0, 2.0, 1.0, 3.0]
        );
    }

    #[test]
    fn rank_refuses_nan_and_matrices() {
        assert_eq!(rank(&[col(&[1.0, f64::NAN])]), Err(EvalError::NotFinite));
        assert!(rank(&[mat(&[&[1.0, 2.0], &[3.0, 4.0]])]).is_err());
    }

    #[test]
    fn kronecker_of_a_two_by_two_with_the_identity() {
        let m = mat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let i2 = mat(&[&[1.0, 0.0], &[0.0, 1.0]]);
        let got = kronecker(&[m, i2]).unwrap();
        let Value::Matrix(k) = got else { panic!() };
        assert_eq!(k.shape(), (4, 4));
        assert_eq!(
            values(&Value::Matrix(k)),
            vec![
                1.0, 0.0, 2.0, 0.0, //
                0.0, 1.0, 0.0, 2.0, //
                3.0, 0.0, 4.0, 0.0, //
                0.0, 3.0, 0.0, 4.0,
            ]
        );
    }

    #[test]
    fn logpts_matches_the_help_sequence() {
        let got = logpts(&[dl_value(-1.0), dl_value(1.0), dl_value(9.0)]).unwrap();
        let v = values(&got);
        let expected: Vec<f64> = (1..=9).map(|k| k as f64 / 10.0).collect();
        assert_eq!(v.len(), 9);
        for (a, b) in v.iter().zip(&expected) {
            assert!((a - b).abs() < 1e-15, "{a} vs {b}");
        }
    }

    #[test]
    fn logspace_includes_both_ends() {
        let got = logspace(&[dl_value(1.0), dl_value(1000.0), dl_value(4.0)]).unwrap();
        let v = values(&got);
        let expected = [1.0, 10.0, 100.0, 1000.0];
        for (a, b) in v.iter().zip(expected) {
            assert!((a - b).abs() <= 1e-12 * b, "{a} vs {b}");
        }
        assert_eq!(*v.last().unwrap(), 1000.0);
    }

    #[test]
    fn logspace_and_logpts_refuse_bad_arguments() {
        assert!(logspace(&[dl_value(0.0), dl_value(1.0), dl_value(3.0)]).is_err());
        assert!(logspace(&[dl_value(1.0), dl_value(2.0), dl_value(1.0)]).is_err());
        assert!(logpts(&[dl_value(0.0), dl_value(0.0), dl_value(3.0)]).is_err());
        assert!(logpts(&[dl_value(0.0), dl_value(1e9), dl_value(1e9)]).is_err());
    }
}

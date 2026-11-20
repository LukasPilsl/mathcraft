//! Indexing: `M[i]` and `M[i, j]`, counted from `ORIGIN`.
//!
//! The recursive part (evaluating the base and the indices) is kept in small functions, so deep
//! index nesting stays within the native stack bound (see `expr_eval::MAX_NODE_DEPTH`).

use super::env::{integer_of, origin_of};
use super::error::EvalError;
use super::expr_eval::{range_in_expression, sub, Ctx};
use super::ops;
use super::value::{Matrix, Scalar, Value};
use crate::expr::Expr;

/// `M[i]` or `M[i, j]`: the element at the given indices, counted from `ORIGIN`.
///
/// The recursive part is kept to small functions: this frame holds only what the base and the
/// index evaluation need, so deep index nesting stays within the native stack bound.
pub(super) fn index(base: &Expr, indices: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let m = match sub(base, ctx)? {
        Value::Matrix(m) => m,
        other => return index_other(other, indices, ctx),
    };
    let positions = index_positions(indices, ctx)?;
    select(&m, &positions, ctx)
}

/// Indexing a base that is not a matrix: a nested array, or an error. Kept out of [`index`] so the
/// recursive frame of a matrix index is the same size as before (see `expr_eval::MAX_NODE_DEPTH`).
#[inline(never)]
fn index_other(other: Value, indices: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    match other {
        Value::List(items) => {
            let positions = index_positions(indices, ctx)?;
            select_list(&items, &positions, ctx)
        }
        other => Err(not_indexable(&other)),
    }
}

#[inline(never)]
#[cold]
fn not_indexable(v: &Value) -> EvalError {
    let what = match v {
        Value::Str(_) => "cannot index text",
        _ => "cannot index a scalar",
    };
    EvalError::Unsupported(what.to_string())
}

/// `list[i]`: the element at `i`, counted from `ORIGIN`. A nested array takes exactly one index.
#[inline(never)]
pub(super) fn select_list(
    items: &[Value],
    positions: &[i64],
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let [i] = positions else {
        return Err(EvalError::Unsupported(
            "a nested array takes one index".to_string(),
        ));
    };
    let origin = origin_in(ctx)?;
    let k = zero_based(*i, origin)?;
    items.get(k).cloned().ok_or(EvalError::IndexOutOfRange {
        index: k,
        bound: items.len(),
    })
}

fn index_positions(indices: &[Expr], ctx: Ctx<'_>) -> Result<Vec<i64>, EvalError> {
    let mut positions = Vec::with_capacity(indices.len());
    for e in indices {
        positions.push(index_value(sub(e, ctx)?)?);
    }
    Ok(positions)
}

/// Picks the element once every operand is evaluated (so not on the recursion path).
#[inline(never)]
pub(super) fn select(m: &Matrix, positions: &[i64], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let origin = origin_in(ctx)?;
    let element = match positions {
        [i] => vector_element(m, zero_based(*i, origin)?)?,
        [i, j] => *m.get(zero_based(*i, origin)?, zero_based(*j, origin)?)?,
        _ => {
            return Err(EvalError::Unsupported(
                "indexing takes one or two indices".to_string(),
            ))
        }
    };
    Ok(Value::Scalar(element))
}

/// One element of a vector, by its zero-based position in row-major order.
fn vector_element(m: &Matrix, k: usize) -> Result<Scalar, EvalError> {
    if m.rows() != 1 && m.cols() != 1 {
        return Err(EvalError::Unsupported(
            "a single index needs a vector; give two indices for a matrix".to_string(),
        ));
    }
    let len = m.rows() * m.cols();
    if k >= len {
        return Err(EvalError::IndexOutOfRange {
            index: k,
            bound: len,
        });
    }
    Ok(m.data()[k])
}

/// An index value: a dimensionless real integer.
fn index_value(v: Value) -> Result<i64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            integer_of(&s)
                .ok_or_else(|| EvalError::Unsupported("index must be an integer".to_string()))
        }
        _ => Err(EvalError::Unsupported("index must be a scalar".to_string())),
    }
}

/// `ORIGIN` as seen by the current code: the captured one inside a function body.
/// A range named `ORIGIN` (live, or captured by a function body) is refused like any other read.
pub(super) fn origin_in(ctx: Ctx<'_>) -> Result<i64, EvalError> {
    match ctx.frame {
        Some(fr) => {
            if fr.func.closure.range_variables.contains("ORIGIN") {
                return Err(range_in_expression());
            }
            origin_of(fr.func.closure.variables.get("ORIGIN").map(|v| &**v))
        }
        None => ctx.env.origin(),
    }
}

/// Converts an index counted from `origin` to a zero-based position.
pub(super) fn zero_based(index: i64, origin: i64) -> Result<usize, EvalError> {
    let k = index
        .checked_sub(origin)
        .ok_or_else(|| EvalError::Unsupported("index out of the integer range".to_string()))?;
    usize::try_from(k)
        .map_err(|_| EvalError::Unsupported(format!("index {index} is below ORIGIN {origin}")))
}

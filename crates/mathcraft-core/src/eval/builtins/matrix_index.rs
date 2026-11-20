//! Functions that count from `ORIGIN`: `last`, `submatrix`, `csort` and `rsort` (help: "Number of
//! Elements in a Vector", "Joining and Subdividing Arrays", "Sorting Arrays", "Example: Effect of
//! ORIGIN on non-Lookup Functions"). They are context builtins: `ORIGIN` comes from [`BuiltinCtx`].
//!
//! Units: `last` gives a dimensionless index. `submatrix` and the sorts keep each element's own
//! dimension, since they only move elements. A sort orders by the real parts of its key line, which
//! must share one dimension (as `sort` requires).
//!
//! Sorts are ascending by the real part of the key. Tied keys come out in heapsort order (see
//! `heapsort_order`), which is what Mathcad's help describes; that order is not stable.
//!
//! Errors: an index outside the array is `IndexOutOfRange`, which reports the zero-based position.

use super::matrix::{expect_matrix, heapsort_order, uniform_dim};
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::indexing::zero_based;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("last", Arity::Exact(1), last);
    r.register_ctx("submatrix", Arity::Exact(5), submatrix);
    r.register_ctx("csort", Arity::Exact(2), csort);
    r.register_ctx("rsort", Arity::Exact(2), rsort);
}

/// The argument at position `k`, or an `Unsupported` error naming `name`.
fn arg<'a>(args: &'a [Value], k: usize, name: &str) -> Result<&'a Value, EvalError> {
    args.get(k)
        .ok_or_else(|| EvalError::Unsupported(format!("{name} is missing an argument")))
}

/// An integer argument: a dimensionless real integer, given as an index or a count.
fn integer_arg(v: &Value, name: &str) -> Result<i64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            integer_of(s).ok_or_else(|| EvalError::Unsupported(format!("{name} needs an integer")))
        }
        _ => Err(EvalError::Unsupported(format!("{name} needs an integer"))),
    }
}

/// `last(v)`: the index of the last element of column vector `v`, counted from ORIGIN (so
/// `rows - 1 + ORIGIN`).
fn last(env: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args, 0, "last")?, "last")?;
    if m.cols() != 1 {
        return Err(EvalError::Unsupported(
            "last needs a column vector".to_string(),
        ));
    }
    let last_index = (m.rows() as i64 - 1)
        .checked_add(env.origin()?)
        .ok_or(EvalError::Overflow)?;
    Ok(Value::Scalar(Scalar::real(
        last_index as f64,
        Dimension::dimensionless(),
    )))
}

/// `submatrix(A, ir, jr, ic, jc)`: rows `ir` through `jr` and columns `ic` through `jc`, both
/// ends included, counted from ORIGIN. `ir <= jr` and `ic <= jc` are required.
fn submatrix(env: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args, 0, "submatrix")?, "submatrix")?;
    let origin = env.origin()?;
    let mut bounds = [0usize; 4];
    for (k, slot) in bounds.iter_mut().enumerate() {
        let index = integer_arg(arg(args, k + 1, "submatrix")?, "submatrix")?;
        *slot = zero_based(index, origin)?;
    }
    let [r0, r1, c0, c1] = bounds;
    if r1 < r0 || c1 < c0 {
        return Err(EvalError::Unsupported(
            "submatrix needs ir <= jr and ic <= jc".to_string(),
        ));
    }
    if r1 >= m.rows() {
        return Err(EvalError::IndexOutOfRange {
            index: r1,
            bound: m.rows(),
        });
    }
    if c1 >= m.cols() {
        return Err(EvalError::IndexOutOfRange {
            index: c1,
            bound: m.cols(),
        });
    }
    let mut data = Vec::with_capacity((r1 - r0 + 1) * (c1 - c0 + 1));
    for i in r0..=r1 {
        for j in c0..=c1 {
            data.push(*m.get(i, j)?);
        }
    }
    Ok(Value::Matrix(Matrix::new(r1 - r0 + 1, c1 - c0 + 1, data)?))
}

/// `csort(A, n)`: the rows of `A` rearranged until column `n` (counted from ORIGIN) ascends.
fn csort(env: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args, 0, "csort")?, "csort")?;
    let key = zero_based(integer_arg(arg(args, 1, "csort")?, "csort")?, env.origin()?)?;
    Ok(Value::Matrix(sort_rows_by_column(m, key)?))
}

/// `rsort(A, n)`: the columns of `A` rearranged until row `n` (counted from ORIGIN) ascends. This
/// is `csort` on the transpose.
fn rsort(env: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args, 0, "rsort")?, "rsort")?;
    let key = zero_based(integer_arg(arg(args, 1, "rsort")?, "rsort")?, env.origin()?)?;
    let sorted = sort_rows_by_column(&transposed(m)?, key)?;
    Ok(Value::Matrix(transposed(&sorted)?))
}

/// The rows of `m`, reordered so that column `key` (zero-based) ascends by real part.
fn sort_rows_by_column(m: &Matrix, key: usize) -> Result<Matrix, EvalError> {
    if key >= m.cols() {
        return Err(EvalError::IndexOutOfRange {
            index: key,
            bound: m.cols(),
        });
    }
    let rows: Vec<&[Scalar]> = m.data().chunks(m.cols()).collect();
    let key_scalars: Vec<Scalar> = rows.iter().map(|r| r[key]).collect();
    uniform_dim(&key_scalars)?;
    let keys: Vec<f64> = key_scalars.iter().map(|s| s.value.re).collect();
    let data = heapsort_order(&keys)
        .iter()
        .flat_map(|&i| rows[i].iter().copied())
        .collect();
    Matrix::new(m.rows(), m.cols(), data)
}

/// The transpose of `m`: element `(i, j)` moves to `(j, i)`. Elements keep their dimensions.
fn transposed(m: &Matrix) -> Result<Matrix, EvalError> {
    let (rows, cols) = m.shape();
    let mut data = Vec::with_capacity(rows * cols);
    for j in 0..cols {
        for i in 0..rows {
            data.push(m.data()[i * cols + j]);
        }
    }
    Matrix::new(cols, rows, data)
}

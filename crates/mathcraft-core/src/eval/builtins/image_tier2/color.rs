//! Packed colour matrices: `extract`, `replace` and `subcolor` (help: "Working with Packed
//! Matrices"). A packed image of width `w` pixels has `3w` columns: the red plane in columns
//! `0..w`, green in `w..2w`, blue in `2w..3w`.
//!
//! - `extract(M, n)`: plane `n` (1 red, 2 green, 3 blue) as a `rows x w` matrix. Without `n`, the three
//!   planes as a nested list (the help: "all three components are returned in a nested matrix").
//! - `replace(M, N, n)`: `M` with plane `n` replaced by `N`, which must be `rows x w`.
//! - `subcolor(M, ir, jr, ic, jc)`: the submatrix of the packed matrix from row `ir` to row `jr` and
//!   column `ic` to column `jc`, both ends included; the indices count from `ORIGIN` (the help allows
//!   zero only as the chosen origin).
//!
//! Elements are copied as they are (units included); the planes are not converted.

use crate::eval::builtins::matrix::expect_matrix;
use crate::eval::builtins::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Matrix, Value};

use super::util::{charge, index_arg, int_arg};

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("extract", Arity::Range(1, 2), extract);
    r.register_ctx("replace", Arity::Exact(3), replace);
    r.register_ctx("subcolor", Arity::Exact(5), subcolor);
}

/// The plane number `n` (1 to 3) of a packed matrix with `w` pixels per plane, as a column offset.
fn plane_number(v: &Value, name: &str) -> Result<usize, EvalError> {
    let n = int_arg(v, name)?;
    if !(1..=3).contains(&n) {
        return Err(EvalError::Unsupported(format!(
            "{name} needs the colour number 1, 2 or 3"
        )));
    }
    Ok(n as usize - 1)
}

/// The packed matrix's width per plane: its columns divided by three.
fn plane_width(m: &Matrix, name: &str) -> Result<usize, EvalError> {
    let cols = m.cols();
    if !cols.is_multiple_of(3) {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a packed colour matrix, with a column count divisible by 3"
        )));
    }
    Ok(cols / 3)
}

/// The columns `from..from + w` of `m`, as a new matrix.
fn plane_of(m: &Matrix, from: usize, w: usize) -> Result<Value, EvalError> {
    let rows = m.rows();
    let mut data = Vec::with_capacity(rows * w);
    for i in 0..rows {
        for j in 0..w {
            data.push(*m.get(i, from + j)?);
        }
    }
    Ok(Value::Matrix(Matrix::new(rows, w, data)?))
}

/// `extract(M)` or `extract(M, n)`.
fn extract(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(&args[0], "extract")?;
    let w = plane_width(m, "extract")?;
    charge(ctx, m.data().len())?;
    match args.get(1) {
        Some(v) => {
            let k = plane_number(v, "extract")?;
            plane_of(m, k * w, w)
        }
        None => Ok(Value::List(
            (0..3)
                .map(|k| plane_of(m, k * w, w))
                .collect::<Result<Vec<_>, _>>()?,
        )),
    }
}

/// `replace(M, N, n)`.
fn replace(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(&args[0], "replace")?;
    let n = expect_matrix(&args[1], "replace")?;
    let w = plane_width(m, "replace")?;
    let k = plane_number(&args[2], "replace")?;
    if n.rows() != m.rows() || n.cols() != w {
        return Err(EvalError::ShapeMismatch {
            left: (m.rows(), w),
            right: (n.rows(), n.cols()),
        });
    }
    charge(ctx, m.data().len())?;
    let mut out = m.clone();
    for i in 0..m.rows() {
        for j in 0..w {
            out.set(i, k * w + j, *n.get(i, j)?)?;
        }
    }
    Ok(Value::Matrix(out))
}

/// `subcolor(M, ir, jr, ic, jc)`: rows `ir..=jr` and columns `ic..=jc`, counted from `ORIGIN`.
fn subcolor(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(&args[0], "subcolor")?;
    let origin = ctx.origin()?;
    let ir = index_arg(&args[1], "subcolor", origin)?;
    let jr = index_arg(&args[2], "subcolor", origin)?;
    let ic = index_arg(&args[3], "subcolor", origin)?;
    let jc = index_arg(&args[4], "subcolor", origin)?;
    if ir > jr || ic > jc {
        return Err(EvalError::Unsupported(
            "subcolor needs ir <= jr and ic <= jc".to_string(),
        ));
    }
    if jr >= m.rows() {
        return Err(EvalError::IndexOutOfRange {
            index: jr,
            bound: m.rows(),
        });
    }
    if jc >= m.cols() {
        return Err(EvalError::IndexOutOfRange {
            index: jc,
            bound: m.cols(),
        });
    }
    let (rows, cols) = (jr - ir + 1, jc - ic + 1);
    charge(ctx, rows * cols)?;
    let mut data = Vec::with_capacity(rows * cols);
    for i in ir..=jr {
        for j in ic..=jc {
            data.push(*m.get(i, j)?);
        }
    }
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

//! Vector and matrix functions: shape queries, identity and diagonal arrays, joining, reversing
//! and sorting, trace, determinant and cross product. Norms, condition numbers, reduced row
//! echelon form, rank and linear solves live in `matrix_linalg`; singular values in `matrix_svd`.
//!
//! Help: "Functions > Vector and Matrix" (INDEX.md). Units: joining, reversing and sorting keep each
//! element's dimension; `det` gives `D^n`; `cross` gives the product of the two dimensions.
//!
//! `last`, `submatrix`, `csort` and `rsort` count from `ORIGIN` and live in `matrix_index`.
//!
//! Eigenvalues, eigenvectors and `geninv` live in `matrix_decomp`.
//!
//! Stubs: `matrix` takes a user function. The factorizations `Cholesky`, `LU`, `QR` and `svd` are
//! in `matrix_decomp2` (nested results are `Value::List`).

use num_complex::Complex64;

use super::{Arity, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use crate::units::Dimension;

/// Registers every function of this file and of `matrix_linalg`.
pub fn register(r: &mut Registry) {
    let one = Arity::Exact(1);
    r.register("rows", one, rows);
    r.register("cols", one, cols);
    r.register("length", one, length);
    r.register("identity", one, identity);
    r.register("diag", one, diag);
    r.register("augment", Arity::AtLeast(1), augment);
    r.register("stack", Arity::AtLeast(1), stack);
    r.register("reverse", one, reverse);
    r.register("sort", one, sort);
    r.register("tr", one, tr);
    r.register("det", one, det);
    r.register("cross", Arity::Exact(2), cross);
    super::matrix_linalg::register(r);
    super::matrix_index::register(r);
    super::matrix_decomp::register(r);
    super::matrix_decomp2::register(r);
    r.register("matrix", Arity::Exact(3), stub_matrix);
    r.register("vec", Arity::Range(1, 3), stub_vec);
}

/// `vec(t)` with a range variable is evaluated before the arguments (see `range_iter::range_vector`),
/// so this only runs for the other forms: `vec(x, y)` and `vec(x, y, z)` (start, step, end), and
/// `vec(x)` with a value that is not a range. Not built yet, so refused rather than guessed.
fn stub_vec(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "vec takes a range variable (vec(t)); vec(x, y) and vec(x, y, z) are not built yet"
            .to_string(),
    ))
}

fn stub_matrix(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "matrix: not implemented yet (needs a user function argument)".to_string(),
    ))
}

// ---- shared helpers (also used by matrix_linalg) ---------------------------------------------

/// The first argument of a unary builtin.
fn arg(args: &[Value]) -> Result<&Value, EvalError> {
    args.first()
        .ok_or_else(|| EvalError::Unsupported("missing argument".to_string()))
}

/// The matrix (or column vector) in `v`. A scalar is not a matrix for these functions.
pub(super) fn expect_matrix<'a>(v: &'a Value, name: &str) -> Result<&'a Matrix, EvalError> {
    match v {
        Value::Matrix(m) => Ok(m),
        _ => Err(EvalError::Unsupported(format!(
            "{name} needs a vector or matrix"
        ))),
    }
}

/// The square matrix in `v`; `NotSquare` otherwise.
pub(super) fn expect_square<'a>(v: &'a Value, name: &str) -> Result<&'a Matrix, EvalError> {
    let m = expect_matrix(v, name)?;
    if m.rows() == m.cols() {
        Ok(m)
    } else {
        Err(EvalError::NotSquare {
            rows: m.rows(),
            cols: m.cols(),
        })
    }
}

/// The common dimension of all elements. The first element whose dimension differs gives
/// `DimensionMismatch`.
pub(super) fn uniform_dim(data: &[Scalar]) -> Result<Dimension, EvalError> {
    let first = data
        .first()
        .ok_or_else(|| EvalError::Unsupported("empty matrix".to_string()))?;
    for s in data {
        if s.dim != first.dim {
            return Err(EvalError::DimensionMismatch {
                left: first.dim,
                right: s.dim,
            });
        }
    }
    Ok(first.dim)
}

/// A scalar with dimension `dim` from a complex result, refusing non-finite parts.
pub(super) fn finite_scalar(z: Complex64, dim: Dimension) -> Result<Scalar, EvalError> {
    if z.re.is_finite() && z.im.is_finite() {
        Ok(Scalar::new(z, dim))
    } else {
        Err(EvalError::NotFinite)
    }
}

/// The element count of a `rows x cols` result, refused above `MAX_ELEMENTS` before anything is
/// allocated.
fn element_count(rows: usize, cols: usize) -> Result<usize, EvalError> {
    let count = rows.checked_mul(cols).ok_or(EvalError::Overflow)?;
    if count > MAX_ELEMENTS {
        Err(EvalError::TooLarge(count))
    } else {
        Ok(count)
    }
}

fn dimensionless(x: f64) -> Value {
    Value::Scalar(Scalar::real(x, Dimension::dimensionless()))
}

/// A positive integer size argument (dimensionless, integral, at least 1).
fn size_of(v: &Value, name: &str) -> Result<usize, EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a scalar size"
        )));
    };
    ops::require_dimensionless(s.dim)?;
    match integer_of(s) {
        Some(n) if n >= 1 => usize::try_from(n).map_err(|_| EvalError::Overflow),
        _ => Err(EvalError::Unsupported(format!(
            "{name} needs a positive integer"
        ))),
    }
}

fn scalar_of(v: Value) -> Result<Scalar, EvalError> {
    match v {
        Value::Scalar(s) => Ok(s),
        _ => Err(EvalError::Unsupported(
            "expected a scalar result".to_string(),
        )),
    }
}

// ---- shape queries --------------------------------------------------------------------------

/// `rows(A)`: the row count, or 0 when `A` is not a vector or matrix.
fn rows(args: &[Value]) -> Result<Value, EvalError> {
    let n = match arg(args)? {
        Value::Matrix(m) => m.rows(),
        _ => 0,
    };
    Ok(dimensionless(n as f64))
}

/// `cols(A)`: the column count, or 0 when `A` is not a vector or matrix.
fn cols(args: &[Value]) -> Result<Value, EvalError> {
    let n = match arg(args)? {
        Value::Matrix(m) => m.cols(),
        _ => 0,
    };
    Ok(dimensionless(n as f64))
}

/// `length(v)`: the number of elements of a column vector.
fn length(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args)?, "length")?;
    if m.cols() != 1 {
        return Err(EvalError::Unsupported(
            "length needs a column vector".to_string(),
        ));
    }
    Ok(dimensionless(m.rows() as f64))
}

// ---- creating arrays ------------------------------------------------------------------------

/// `identity(n)`: the `n x n` identity matrix.
fn identity(args: &[Value]) -> Result<Value, EvalError> {
    let n = size_of(arg(args)?, "identity")?;
    let count = element_count(n, n)?;
    let mut data = Vec::with_capacity(count);
    for i in 0..n {
        for j in 0..n {
            let v = if i == j { 1.0 } else { 0.0 };
            data.push(Scalar::real(v, Dimension::dimensionless()));
        }
    }
    Ok(Value::Matrix(Matrix::new(n, n, data)?))
}

/// `diag(v)` builds the square matrix with `v` on its diagonal; `diag(M)` gives the diagonal of a
/// square matrix as a column vector. Off-diagonal zeros take the dimension of `v`.
fn diag(args: &[Value]) -> Result<Value, EvalError> {
    match arg(args)? {
        Value::Matrix(m) if m.cols() == 1 => diag_of_vector(m),
        v => {
            let m = expect_square(v, "diag")?;
            let n = m.rows();
            let data: Vec<Scalar> = m.data().iter().step_by(n + 1).copied().collect();
            Ok(Value::Matrix(Matrix::new(n, 1, data)?))
        }
    }
}

fn diag_of_vector(v: &Matrix) -> Result<Value, EvalError> {
    let n = v.rows();
    let count = element_count(n, n)?;
    let dim = uniform_dim(v.data())?;
    let zero = Scalar::new(Complex64::new(0.0, 0.0), dim);
    let mut data = Vec::with_capacity(count);
    for (i, e) in v.data().iter().enumerate() {
        for j in 0..n {
            data.push(if i == j { *e } else { zero });
        }
    }
    Ok(Value::Matrix(Matrix::new(n, n, data)?))
}

/// Each argument as a block: a matrix as it is, a scalar as a 1 x 1 matrix.
fn as_block(v: &Value, name: &str) -> Result<Matrix, EvalError> {
    match v {
        Value::Matrix(m) => Ok(m.clone()),
        Value::Scalar(s) => Matrix::new(1, 1, vec![*s]),
        Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => Err(
            EvalError::Unsupported(format!("{name} needs vectors, matrices or numbers")),
        ),
    }
}

/// `augment(A, B, ...)`: blocks side by side. All blocks need the same row count.
fn augment(args: &[Value]) -> Result<Value, EvalError> {
    let blocks = args
        .iter()
        .map(|v| as_block(v, "augment"))
        .collect::<Result<Vec<_>, _>>()?;
    let first = blocks
        .first()
        .ok_or_else(|| EvalError::Unsupported("augment of nothing".to_string()))?;
    let rows = first.rows();
    for b in &blocks {
        if b.rows() != rows {
            return Err(EvalError::ShapeMismatch {
                left: first.shape(),
                right: b.shape(),
            });
        }
    }
    let total_cols: usize = blocks.iter().map(Matrix::cols).sum();
    let count = element_count(rows, total_cols)?;
    let mut data = Vec::with_capacity(count);
    let row_chunks: Vec<Vec<&[Scalar]>> = blocks
        .iter()
        .map(|b| b.data().chunks(b.cols()).collect())
        .collect();
    for i in 0..rows {
        for chunks in &row_chunks {
            if let Some(row) = chunks.get(i) {
                data.extend_from_slice(row);
            }
        }
    }
    Ok(Value::Matrix(Matrix::new(rows, total_cols, data)?))
}

/// `stack(A, B, ...)`: blocks top to bottom. All blocks need the same column count.
fn stack(args: &[Value]) -> Result<Value, EvalError> {
    let blocks = args
        .iter()
        .map(|v| as_block(v, "stack"))
        .collect::<Result<Vec<_>, _>>()?;
    let first = blocks
        .first()
        .ok_or_else(|| EvalError::Unsupported("stack of nothing".to_string()))?;
    let cols = first.cols();
    for b in &blocks {
        if b.cols() != cols {
            return Err(EvalError::ShapeMismatch {
                left: first.shape(),
                right: b.shape(),
            });
        }
    }
    let total_rows: usize = blocks.iter().map(Matrix::rows).sum();
    let count = element_count(total_rows, cols)?;
    let mut data = Vec::with_capacity(count);
    for b in &blocks {
        data.extend_from_slice(b.data());
    }
    Ok(Value::Matrix(Matrix::new(total_rows, cols, data)?))
}

// ---- reversing and sorting ------------------------------------------------------------------

/// `reverse(A)`: a vector's elements in reverse order, or a matrix's rows in reverse order.
fn reverse(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args)?, "reverse")?;
    let data: Vec<Scalar> = m
        .data()
        .chunks(m.cols())
        .rev()
        .flat_map(|row| row.iter().copied())
        .collect();
    Ok(Value::Matrix(Matrix::new(m.rows(), m.cols(), data)?))
}

/// `sort(v)`: a column vector in ascending order. Elements are ordered by their real parts (the
/// help says imaginary parts are ignored); each element keeps its value. Tied real parts come out
/// in heapsort order (see [`heapsort_order`]). Elements of mixed dimensions cannot be ordered.
fn sort(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args)?, "sort")?;
    if m.cols() != 1 {
        return Err(EvalError::Unsupported(
            "sort needs a column vector".to_string(),
        ));
    }
    uniform_dim(m.data())?;
    let keys: Vec<f64> = m.data().iter().map(|s| s.value.re).collect();
    let data = heapsort_order(&keys)
        .into_iter()
        .map(|i| m.data()[i])
        .collect();
    Ok(Value::Matrix(Matrix::new(m.rows(), 1, data)?))
}

/// The order that sorts `keys` ascending, as indices into `keys`: the heapsort of Numerical Recipes
/// (`hpsort`, C 2nd ed. section 8.3) run on the indices. Mathcad's sort functions use this
/// heapsort (help: "Sorting Arrays"), which is not stable, so tied keys come out in the order it
/// leaves them. Ties compare equal, so the sifts use strict `<`, as in the source.
pub(super) fn heapsort_order(keys: &[f64]) -> Vec<usize> {
    let n = keys.len();
    if n < 2 {
        return (0..n).collect();
    }
    // One-based, as in the source: `ra[0]` is unused and `ra[k]` holds an index into `keys`.
    let mut ra: Vec<usize> = std::iter::once(0).chain(0..n).collect();
    let less = |a: usize, b: usize| keys[a] < keys[b];
    let mut l = (n >> 1) + 1;
    let mut ir = n;
    loop {
        let rra;
        if l > 1 {
            l -= 1;
            rra = ra[l];
        } else {
            rra = ra[ir];
            ra[ir] = ra[1];
            ir -= 1;
            if ir == 1 {
                ra[1] = rra;
                break;
            }
        }
        // Sift `rra` down from `l` in the heap of size `ir`.
        let mut i = l;
        let mut j = l << 1;
        while j <= ir {
            if j < ir && less(ra[j], ra[j + 1]) {
                j += 1;
            }
            if less(rra, ra[j]) {
                ra[i] = ra[j];
                i = j;
                j <<= 1;
            } else {
                j = ir + 1;
            }
        }
        ra[i] = rra;
    }
    ra.split_off(1)
}

// ---- trace, determinant, cross product ------------------------------------------------------

/// `tr(M)`: the sum of the diagonal of a square matrix. Diagonal elements must share a dimension.
fn tr(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(arg(args)?, "tr")?;
    let mut acc: Option<Value> = None;
    for e in m.data().iter().step_by(m.rows() + 1) {
        let term = Value::Scalar(*e);
        acc = Some(match acc {
            None => term,
            Some(sum) => ops::add(&sum, &term)?,
        });
    }
    acc.ok_or_else(|| EvalError::Unsupported("trace of an empty matrix".to_string()))
}

/// `det(M)`: the determinant of a square matrix, `D^n` for a matrix of dimension `D`.
fn det(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(arg(args)?, "det")?;
    Ok(Value::Scalar(ops::det(m)?))
}

fn times(a: &Scalar, b: &Scalar) -> Result<Scalar, EvalError> {
    scalar_of(ops::mul(&Value::Scalar(*a), &Value::Scalar(*b))?)
}

fn minus(a: &Scalar, b: &Scalar) -> Result<Scalar, EvalError> {
    scalar_of(ops::sub(&Value::Scalar(*a), &Value::Scalar(*b))?)
}

/// `cross(u, v)`: the cross product of two three-element column vectors. Elements multiply, so
/// the result's dimension is the product of the dimensions of `u` and `v`.
fn cross(args: &[Value]) -> Result<Value, EvalError> {
    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
        return Err(EvalError::Unsupported(
            "cross needs two vectors".to_string(),
        ));
    };
    let u = expect_matrix(a, "cross")?;
    let v = expect_matrix(b, "cross")?;
    for w in [u, v] {
        if w.shape() != (3, 1) {
            return Err(EvalError::ShapeMismatch {
                left: w.shape(),
                right: (3, 1),
            });
        }
    }
    let (&[ux, uy, uz], &[vx, vy, vz]) = (u.data(), v.data()) else {
        return Err(EvalError::Unsupported(
            "cross needs two 3-element vectors".to_string(),
        ));
    };
    let c0 = minus(&times(&uy, &vz)?, &times(&uz, &vy)?)?;
    let c1 = minus(&times(&uz, &vx)?, &times(&ux, &vz)?)?;
    let c2 = minus(&times(&ux, &vy)?, &times(&uy, &vx)?)?;
    Ok(Value::Matrix(Matrix::new(3, 1, vec![c0, c1, c2])?))
}

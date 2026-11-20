//! Dimension-checked arithmetic on [`Value`]s.
//!
//! Scalars combine with the unit rules of [`crate::units::Dimension`]. Matrices are element-wise
//! for `+ - *(scalar) /(scalar)`, the matrix product for matrix `*` matrix, a dot product for two
//! column vectors, and every output element is dimension-checked on its own. Nothing here
//! panics: all arithmetic is checked. Scalar kernels live in `kernels`; determinant and inverse
//! live in `linalg`.

use num_complex::Complex64;

use super::error::EvalError;
use super::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use super::work;
use crate::units::Dimension;

mod kernels;
mod linalg;

pub use linalg::{det, inverse};

use kernels::{checked, compare_scalars, s_add, s_div, s_mul, s_pow, s_sub};

#[cfg(test)]
mod tests;

/// Comparison operators. Results are dimensionless scalars, 1 for true and 0 for false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

/// `Ok(())` when `dim` is dimensionless; the error carries the offending dimension.
pub fn require_dimensionless(dim: Dimension) -> Result<(), EvalError> {
    if dim.is_dimensionless() {
        Ok(())
    } else {
        Err(EvalError::DimensionNotDimensionless { dim })
    }
}

/// Applies `f` to every scalar of a value: the scalar itself, or each matrix element.
/// Text is refused. This is the helper that element-wise functions such as `sin` build on.
pub fn map_scalar(
    v: &Value,
    mut f: impl FnMut(&Scalar) -> Result<Scalar, EvalError>,
) -> Result<Value, EvalError> {
    match v {
        Value::Scalar(s) => Ok(Value::Scalar(f(s)?)),
        Value::Matrix(m) => Ok(Value::Matrix(map_matrix(m, &mut f)?)),
        Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => Err(
            EvalError::Unsupported("expected a number or a matrix".to_string()),
        ),
    }
}

fn map_matrix(
    m: &Matrix,
    f: &mut impl FnMut(&Scalar) -> Result<Scalar, EvalError>,
) -> Result<Matrix, EvalError> {
    let data = m.data().iter().map(f).collect::<Result<Vec<_>, _>>()?;
    Matrix::new(m.rows(), m.cols(), data)
}

/// `x!`: the factorial of a dimensionless real. A nonnegative integer up to 170 is the exact
/// product; any other real uses the gamma extension, `Gamma(x + 1)` (help: factorial operator).
/// Negative integers are a pole and arguments above 170 overflow, both errors, never a hang.
pub fn factorial(v: &Value) -> Result<Value, EvalError> {
    map_scalar(v, |s| {
        require_dimensionless(s.dim)?;
        if s.value.im != 0.0 {
            return Err(EvalError::Unsupported(
                "the factorial needs a real argument".to_string(),
            ));
        }
        let x = s.value.re;
        if !x.is_finite() {
            return Err(EvalError::NotFinite);
        }
        let result = if x >= 0.0 && x.fract() == 0.0 {
            if x > 170.0 {
                return Err(EvalError::Overflow);
            }
            (2..=x as u32).fold(1.0_f64, |acc, k| acc * f64::from(k))
        } else {
            super::builtins::special::special_gamma::gamma(x + 1.0)?
        };
        Ok(Scalar::new(checked(Complex64::new(result, 0.0))?, s.dim))
    })
}

/// `‖x‖`: the magnitude of a scalar, or the square root of the sum of the squared magnitudes of
/// every element of a vector or matrix. The result has the common dimension of the elements.
pub fn norm(v: &Value) -> Result<Value, EvalError> {
    match v {
        Value::Scalar(s) => Ok(Value::Scalar(magnitude_scalar(s)?)),
        Value::Matrix(m) => {
            let dim = linalg::uniform_dim(m, "‖x‖")?;
            let norm = m
                .data()
                .iter()
                .fold(0.0_f64, |acc, s| acc.hypot(s.value.norm()));
            Ok(Value::Scalar(Scalar::new(
                checked(Complex64::new(norm, 0.0))?,
                dim,
            )))
        }
        Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => Err(
            EvalError::Unsupported("expected a number or a matrix".to_string()),
        ),
    }
}

/// `u × v`: the cross product of two three-element column vectors. The elements of the result
/// have the dimension of the product of the operands' dimensions (m and N give N*m).
pub fn cross(a: &Value, b: &Value) -> Result<Value, EvalError> {
    let (Value::Matrix(x), Value::Matrix(y)) = (a, b) else {
        return Err(EvalError::Unsupported(
            "the cross product needs two three-element column vectors".to_string(),
        ));
    };
    for m in [x, y] {
        if m.cols() != 1 || m.rows() != 3 {
            return Err(EvalError::ShapeMismatch {
                left: x.shape(),
                right: y.shape(),
            });
        }
    }
    let (p, q) = (x.data(), y.data());
    let term = |i: usize, j: usize| -> Result<Scalar, EvalError> {
        s_sub(&s_mul(&p[i], &q[j])?, &s_mul(&p[j], &q[i])?)
    };
    Ok(Value::Matrix(Matrix::new(
        3,
        1,
        vec![term(1, 2)?, term(2, 0)?, term(0, 1)?],
    )?))
}

/// Sum; both operands must have the same dimension (element-wise for matrices, with a scalar
/// broadcast over the matrix).
pub fn add(a: &Value, b: &Value) -> Result<Value, EvalError> {
    elementwise(a, b, "+", s_add)
}

/// Difference; same rules as [`add`].
pub fn sub(a: &Value, b: &Value) -> Result<Value, EvalError> {
    elementwise(a, b, "-", s_sub)
}

/// Product. Scalar times scalar, scalar times matrix (element-wise), matrix times matrix (the
/// matrix product), and two column vectors of one length (the dot product, see [`dot`]).
pub fn mul(a: &Value, b: &Value) -> Result<Value, EvalError> {
    match (a, b) {
        (Value::Matrix(x), Value::Matrix(y)) if is_dot_pair(x, y) => Ok(Value::Scalar(dot(x, y)?)),
        (Value::Matrix(x), Value::Matrix(y)) => Ok(Value::Matrix(matmul(x, y)?)),
        _ => elementwise(a, b, "*", s_mul),
    }
}

/// Quotient. Scalars and matrices divided by a scalar, a scalar divided by a matrix, and two
/// matrices of one shape are element-wise. A matrix `x` (m x n) divided by a square `y` (n x n)
/// is `x * inverse(y)`; `y` singular is `Singular`.
pub fn div(a: &Value, b: &Value) -> Result<Value, EvalError> {
    match (a, b) {
        (Value::Matrix(x), Value::Matrix(y)) if x.shape() != y.shape() => {
            if y.rows() != y.cols() || x.cols() != y.rows() {
                return Err(EvalError::ShapeMismatch {
                    left: x.shape(),
                    right: y.shape(),
                });
            }
            Ok(Value::Matrix(matmul(x, &inverse(y)?)?))
        }
        _ => elementwise(a, b, "/", s_div),
    }
}

/// Negation of a scalar or of each matrix element.
pub fn neg(v: &Value) -> Result<Value, EvalError> {
    // `0.0 - x` (not `-x`) so a zero part becomes +0.0, not -0.0.
    map_scalar(v, |s| {
        let z = Complex64::new(0.0 - s.value.re, 0.0 - s.value.im);
        Ok(Scalar::new(checked(z)?, s.dim))
    })
}

/// Power. Scalar to a scalar power; scalar to a matrix power, and a column vector to a scalar
/// power, both element-wise; square matrix to an integer power (negative powers use the
/// inverse). The exponent must be dimensionless.
pub fn pow(base: &Value, exp: &Value) -> Result<Value, EvalError> {
    match (base, exp) {
        (Value::Scalar(b), Value::Scalar(e)) => Ok(Value::Scalar(s_pow(b, e)?)),
        (Value::Scalar(_), Value::Matrix(_)) => elementwise(base, exp, "^", s_pow),
        (Value::Matrix(m), Value::Scalar(_)) if m.cols() == 1 && m.rows() > 1 => {
            elementwise(base, exp, "^", s_pow)
        }
        (Value::Matrix(m), Value::Scalar(e)) => Ok(Value::Matrix(matrix_pow(m, e)?)),
        _ => Err(EvalError::Unsupported(
            "power needs a scalar or matrix base and a scalar or matrix exponent".to_string(),
        )),
    }
}

/// Transpose of a matrix. A scalar is its own transpose.
pub fn transpose(v: &Value) -> Result<Value, EvalError> {
    match v {
        Value::Scalar(_) => Ok(v.clone()),
        Value::Matrix(m) => {
            let mut data = Vec::with_capacity(m.data().len());
            for i in 0..m.cols() {
                for j in 0..m.rows() {
                    let e = *m.get(j, i)?;
                    data.push(Scalar::new(checked(e.value)?, e.dim));
                }
            }
            Ok(Value::Matrix(Matrix::new(m.cols(), m.rows(), data)?))
        }
        Value::Str(_) => Err(EvalError::Unsupported("transpose of text".to_string())),
        Value::List(_) => Err(EvalError::Unsupported(
            "transpose of a nested array".to_string(),
        )),
        Value::Symbolic(_) => Err(EvalError::Unsupported(
            "transpose of a symbolic result".to_string(),
        )),
        Value::Func(_) => Err(EvalError::Unsupported(
            "transpose of a function".to_string(),
        )),
    }
}

/// Complex conjugate, element-wise for matrices.
pub fn conjugate(v: &Value) -> Result<Value, EvalError> {
    map_scalar(v, |s| {
        let z = Complex64::new(s.value.re, 0.0 - s.value.im);
        Ok(Scalar::new(checked(z)?, s.dim))
    })
}

/// Absolute value `|x|`, following the Mathcad Prime help page "Absolute Value/Determinant
/// Operator" (`determinant_operator.md`). A scalar or complex number gives its magnitude, with its
/// dimension. A column vector gives the square root of the sum of the squared magnitudes of its
/// elements (the Euclidean norm), with the shared element dimension. A square matrix gives its
/// determinant, with the dimension `D^n` (see [`linalg::det`]). A 1x1 matrix is a column vector,
/// so it gives its magnitude. Other shapes, such as row vectors and non-square matrices, are
/// refused, and so are matrices with mixed element dimensions. For element-wise magnitudes use
/// [`map_abs`].
pub fn abs(v: &Value) -> Result<Value, EvalError> {
    match v {
        Value::Scalar(s) => Ok(Value::Scalar(magnitude_scalar(s)?)),
        Value::Matrix(m) if m.cols() != 1 && m.rows() == m.cols() => {
            Ok(Value::Scalar(linalg::det(m)?))
        }
        Value::Matrix(m) => {
            if m.cols() != 1 {
                return Err(EvalError::Unsupported(
                    "|x| of a matrix needs a column vector or a square matrix".to_string(),
                ));
            }
            let dim = linalg::uniform_dim(m, "|x|")?;
            // Folding with hypot avoids overflow of the squares for large elements.
            let norm = m
                .data()
                .iter()
                .fold(0.0_f64, |acc, s| acc.hypot(s.value.norm()));
            Ok(Value::Scalar(Scalar::new(
                checked(Complex64::new(norm, 0.0))?,
                dim,
            )))
        }
        Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => Err(
            EvalError::Unsupported("expected a number or a matrix".to_string()),
        ),
    }
}

/// Same as [`abs`].
pub fn magnitude(v: &Value) -> Result<Value, EvalError> {
    abs(v)
}

/// Magnitude of each element: the scalar's own magnitude, or element-wise for a matrix (the
/// vectorize operator applied to `|x|`).
pub fn map_abs(v: &Value) -> Result<Value, EvalError> {
    map_scalar(v, magnitude_scalar)
}

/// Comparison, giving 1 or 0 as dimensionless values. Two numbers compare directly and must
/// share a dimension. A scalar against a matrix, and two matrices of one shape, compare
/// element-wise. Texts compare for `=` and `<>`. Ordering of complex numbers is refused.
pub fn compare(op: CmpOp, a: &Value, b: &Value) -> Result<Value, EvalError> {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => {
            let truth = match op {
                CmpOp::Eq => x == y,
                CmpOp::Ne => x != y,
                _ => return Err(EvalError::Unsupported("ordering of text".to_string())),
            };
            Ok(Value::Scalar(flag(truth)))
        }
        (Value::Str(_), _) | (_, Value::Str(_)) | (Value::List(_), _) | (_, Value::List(_)) => Err(
            EvalError::Unsupported("comparison needs two scalars".to_string()),
        ),
        _ => elementwise(a, b, "compare", |x, y| Ok(flag(compare_scalars(op, x, y)?))),
    }
}

// ---- helpers --------------------------------------------------------------------------------

fn magnitude_scalar(s: &Scalar) -> Result<Scalar, EvalError> {
    Ok(Scalar::new(
        checked(Complex64::new(s.value.norm(), 0.0))?,
        s.dim,
    ))
}

/// 1 or 0 as a dimensionless scalar.
fn flag(truth: bool) -> Scalar {
    Scalar::real(if truth { 1.0 } else { 0.0 }, Dimension::dimensionless())
}

/// Two column vectors of one length, longer than one element. A single element pair keeps the
/// ordinary product path.
fn is_dot_pair(x: &Matrix, y: &Matrix) -> bool {
    x.cols() == 1 && y.cols() == 1 && x.rows() == y.rows() && x.rows() > 1
}

/// Dot product `sum a_i * conj(b_i)`. Every term must share one dimension (checked by `s_add`).
fn dot(x: &Matrix, y: &Matrix) -> Result<Scalar, EvalError> {
    let mut acc: Option<Scalar> = None;
    for (p, q) in x.data().iter().zip(y.data()) {
        let prod = s_mul(p, &Scalar::new(q.value.conj(), q.dim))?;
        acc = Some(match acc {
            None => prod,
            Some(sum) => s_add(&sum, &prod)?,
        });
    }
    acc.ok_or_else(|| EvalError::Unsupported("empty product".to_string()))
}

// ---- element-wise and matrix plumbing ------------------------------------------------------

/// Applies a scalar kernel to scalar pairs, broadcasts a scalar over a matrix, or zips two
/// matrices of equal shape.
fn elementwise(
    a: &Value,
    b: &Value,
    op: &str,
    f: impl Fn(&Scalar, &Scalar) -> Result<Scalar, EvalError>,
) -> Result<Value, EvalError> {
    match (a, b) {
        (Value::Scalar(x), Value::Scalar(y)) => Ok(Value::Scalar(f(x, y)?)),
        (Value::Matrix(m), Value::Scalar(y)) => Ok(Value::Matrix(map_matrix(m, &mut |x| f(x, y))?)),
        (Value::Scalar(x), Value::Matrix(m)) => Ok(Value::Matrix(map_matrix(m, &mut |y| f(x, y))?)),
        (Value::Matrix(x), Value::Matrix(y)) => {
            if x.shape() != y.shape() {
                return Err(EvalError::ShapeMismatch {
                    left: x.shape(),
                    right: y.shape(),
                });
            }
            let data = x
                .data()
                .iter()
                .zip(y.data())
                .map(|(p, q)| f(p, q))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Matrix(Matrix::new(x.rows(), x.cols(), data)?))
        }
        _ => Err(EvalError::Unsupported(format!(
            "`{op}` needs numbers or matrices"
        ))),
    }
}

/// Matrix product. Each output element is a sum of products; the sum's terms must share one
/// dimension (checked by `s_add`), so every element carries its own dimension.
fn matmul(a: &Matrix, b: &Matrix) -> Result<Matrix, EvalError> {
    if a.cols() != b.rows() {
        return Err(EvalError::ShapeMismatch {
            left: a.shape(),
            right: b.shape(),
        });
    }
    let count = a.rows().saturating_mul(b.cols());
    if count > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(count));
    }
    work::check_mults(a.rows().saturating_mul(a.cols()).saturating_mul(b.cols()) as u64)?;
    let mut data = Vec::with_capacity(count);
    for i in 0..a.rows() {
        for j in 0..b.cols() {
            let mut acc: Option<Scalar> = None;
            for k in 0..a.cols() {
                let prod = s_mul(a.get(i, k)?, b.get(k, j)?)?;
                acc = Some(match acc {
                    None => prod,
                    Some(sum) => s_add(&sum, &prod)?,
                });
            }
            data.push(acc.ok_or_else(|| EvalError::Unsupported("empty product".to_string()))?);
        }
    }
    Matrix::new(a.rows(), b.cols(), data)
}

/// `M^n` for an integer `n` by repeated squaring. `M^0` is the dimensionless identity.
fn matrix_pow(m: &Matrix, exp: &Scalar) -> Result<Matrix, EvalError> {
    require_dimensionless(exp.dim)?;
    let n = match rational_integer(exp.value) {
        Some(n) => n,
        None => {
            return Err(EvalError::Unsupported(
                "matrix power needs an integer exponent".to_string(),
            ))
        }
    };
    if m.rows() != m.cols() {
        return Err(EvalError::NotSquare {
            rows: m.rows(),
            cols: m.cols(),
        });
    }
    let mut k = n.unsigned_abs();
    let mut base = if n < 0 { inverse(m)? } else { m.clone() };
    let mut result: Option<Matrix> = None;
    while k > 0 {
        if k & 1 == 1 {
            result = Some(match result {
                None => base.clone(),
                Some(r) => matmul(&r, &base)?,
            });
        }
        k >>= 1;
        if k > 0 {
            base = matmul(&base, &base)?;
        }
    }
    match result {
        Some(r) => Ok(r),
        None => identity(m.rows()),
    }
}

/// The integer `n` when `z` is a real number that [`kernels::rational_from_f64`] recovers as a
/// whole number.
fn rational_integer(z: Complex64) -> Option<i32> {
    if z.im != 0.0 {
        return None;
    }
    let r = kernels::rational_from_f64(z.re)?;
    if *r.denom() == 1 {
        Some(*r.numer())
    } else {
        None
    }
}

fn identity(n: usize) -> Result<Matrix, EvalError> {
    let mut data = Vec::with_capacity(n.saturating_mul(n));
    for i in 0..n {
        for j in 0..n {
            let v = if i == j { 1.0 } else { 0.0 };
            data.push(Scalar::real(v, Dimension::dimensionless()));
        }
    }
    Matrix::new(n, n, data)
}

pub(super) fn square_size(m: &Matrix) -> Result<usize, EvalError> {
    if m.rows() == m.cols() {
        Ok(m.rows())
    } else {
        Err(EvalError::NotSquare {
            rows: m.rows(),
            cols: m.cols(),
        })
    }
}

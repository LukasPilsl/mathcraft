//! Determinant and inverse by LU decomposition with partial pivoting.
//!
//! Both need every element to share one dimension, so the result's dimension is a power of it.

use num_complex::Complex64;

use crate::eval::error::{from_units, EvalError};
use crate::eval::ops::kernels::{cdiv, checked};
use crate::eval::ops::square_size;
use crate::eval::value::{Matrix, Scalar};
use crate::eval::work::{self, Dense};
use crate::units::{Dimension, Exp};

/// Determinant of a square matrix by LU decomposition with partial pivoting.
///
/// All elements must share one dimension `D`; the result has dimension `D^n`.
pub fn det(m: &Matrix) -> Result<Scalar, EvalError> {
    let n = square_size(m)?;
    work::check_units(work::dense_units(Dense::Elimination, n, n, 0))?;
    let d = uniform_dim(m, "det")?;
    let lu = lu_decompose(m);
    let value = if lu.singular {
        Complex64::new(0.0, 0.0)
    } else {
        (0..n).fold(Complex64::new(lu.sign, 0.0), |acc, k| acc * lu.a[k * n + k])
    };
    Ok(Scalar::new(checked(value)?, dim_power(d, n)?))
}

/// Inverse of a square matrix. All elements must share one dimension `D`; the inverse has
/// elements of dimension `D^-1`. A singular matrix is `Singular`, and so is one whose reciprocal
/// condition number (1-norm) is below `1e-15`: a pivot can be non-zero yet tiny relative to the
/// matrix, and its inverse would then be garbage.
pub fn inverse(m: &Matrix) -> Result<Matrix, EvalError> {
    let n = square_size(m)?;
    work::check_units(work::dense_units(Dense::Inverse, n, n, 0))?;
    let d = uniform_dim(m, "inverse")?;
    let lu = lu_decompose(m);
    if lu.singular {
        return Err(EvalError::Singular);
    }
    let mut out = vec![Complex64::new(0.0, 0.0); n * n];
    for j in 0..n {
        // Forward substitution with L (unit diagonal) on P e_j.
        let mut y = vec![Complex64::new(0.0, 0.0); n];
        for i in 0..n {
            let mut s = if lu.perm[i] == j {
                Complex64::new(1.0, 0.0)
            } else {
                Complex64::new(0.0, 0.0)
            };
            for (k, yk) in y.iter().enumerate().take(i) {
                s -= lu.a[i * n + k] * yk;
            }
            y[i] = s;
        }
        // Back substitution with U.
        let mut x = vec![Complex64::new(0.0, 0.0); n];
        for i in (0..n).rev() {
            let mut s = y[i];
            for (k, xk) in x.iter().enumerate().skip(i + 1) {
                s -= lu.a[i * n + k] * xk;
            }
            x[i] = cdiv(s, lu.a[i * n + i]);
        }
        for (i, xi) in x.into_iter().enumerate() {
            out[i * n + j] = xi;
        }
    }
    let entries: Vec<Complex64> = m.data().iter().map(|s| s.value).collect();
    if norm1(&entries, n) * norm1(&out, n) > 1e15 {
        return Err(EvalError::Singular);
    }
    let inv_dim = d.inverse().map_err(from_units)?;
    let data = out
        .into_iter()
        .map(|v| Ok(Scalar::new(checked(v)?, inv_dim)))
        .collect::<Result<Vec<_>, EvalError>>()?;
    Matrix::new(n, n, data)
}

/// The 1-norm (largest absolute column sum) of an `n x n` row-major array.
fn norm1(values: &[Complex64], n: usize) -> f64 {
    (0..n)
        .map(|j| (0..n).map(|i| values[i * n + j].norm()).sum::<f64>())
        .fold(0.0, f64::max)
}

/// The common dimension of all elements, or `Unsupported` when they differ.
pub(super) fn uniform_dim(m: &Matrix, what: &str) -> Result<Dimension, EvalError> {
    let first = m
        .data()
        .first()
        .ok_or_else(|| EvalError::Unsupported("empty matrix".to_string()))?
        .dim;
    if m.data().iter().all(|s| s.dim == first) {
        Ok(first)
    } else {
        Err(EvalError::Unsupported(format!(
            "{what} of a matrix with mixed element dimensions"
        )))
    }
}

/// `d^n` for an integer count `n`, with checked exponent arithmetic.
fn dim_power(d: Dimension, n: usize) -> Result<Dimension, EvalError> {
    let n = i32::try_from(n).map_err(|_| EvalError::Overflow)?;
    d.pow(Exp::from_integer(n)).map_err(from_units)
}

/// LU factors of a square matrix: `a` holds L (below the diagonal, unit diagonal implied) and U
/// (on and above it); row `k` of the factored matrix is row `perm[k]` of the input.
struct Lu {
    a: Vec<Complex64>,
    perm: Vec<usize>,
    sign: f64,
    singular: bool,
}

/// Doolittle LU with partial pivoting (largest modulus in the column). A column with no
/// non-zero pivot marks the matrix singular and is skipped.
fn lu_decompose(m: &Matrix) -> Lu {
    let n = m.rows();
    let mut a: Vec<Complex64> = m.data().iter().map(|s| s.value).collect();
    let mut perm: Vec<usize> = (0..n).collect();
    let mut sign = 1.0;
    let mut singular = false;
    for k in 0..n {
        let mut pivot_row = k;
        let mut best = a[k * n + k].norm();
        for i in (k + 1)..n {
            let v = a[i * n + k].norm();
            if v > best {
                best = v;
                pivot_row = i;
            }
        }
        if best == 0.0 {
            singular = true;
            continue;
        }
        if pivot_row != k {
            for j in 0..n {
                a.swap(k * n + j, pivot_row * n + j);
            }
            perm.swap(k, pivot_row);
            sign = -sign;
        }
        let pivot = a[k * n + k];
        for i in (k + 1)..n {
            let factor = cdiv(a[i * n + k], pivot);
            a[i * n + k] = factor;
            for j in (k + 1)..n {
                let upper = a[k * n + j];
                a[i * n + j] -= factor * upper;
            }
        }
    }
    Lu {
        a,
        perm,
        sign,
        singular,
    }
}

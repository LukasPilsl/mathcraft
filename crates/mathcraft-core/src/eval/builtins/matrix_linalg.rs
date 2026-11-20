//! Matrix norms, condition numbers, reduced row echelon form, rank and linear solves (help: "Norm
//! of a Square Matrix", "Norm of a Vector", "Condition Number of a Matrix", "Rank and Linear
//! Systems Properties of Matrices", "Solving Linear Systems"). Singular values are in `matrix_svd`.
//!
//! Units: norms keep the common dimension of the elements; condition numbers are dimensionless;
//! `lsolve` gives the dimension of `b` divided by that of `A`. `rref` needs dimensionless elements
//! (its row operations do not keep a unit per column); `rank` is dimensionless.
//!
//! Tolerances: `rank` and `rref` treat a pivot whose magnitude is at most `TOL` as zero. `TOL` is
//! an absolute value, as the worksheet's TOL is, so the result does not depend on the scale of the
//! matrix: rank(diag(1e-4, 1)) = 1 and rank(1e-6 * A) = 0 for A with entries of order 1. `lsolve`
//! refuses a pivot at most `SINGULAR_RATIO` times the largest element (relative).
//!
//! Linear solves: a square `A` uses LU with partial pivoting. A tall `A` (more rows than columns)
//! gives the least-squares solution by modified Gram-Schmidt QR, the solution the help gives as
//! `geninv(A) b`. A wide `A` is refused: the help defines no rule for it. Each column of a matrix
//! `b` is solved separately, as the help gives the columns of `geninv(M) v`.

use num_complex::Complex64;

use super::matrix::{expect_matrix, expect_square, finite_scalar, uniform_dim};
use super::matrix_svd::singular_values;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::{from_units, EvalError};
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// `lsolve` refuses a pivot at most this times the largest element of `A`.
const SINGULAR_RATIO: f64 = 1e-12;

pub(super) fn register(r: &mut Registry) {
    let one = Arity::Exact(1);
    r.register("norm", one, norm);
    r.register("norm1", one, norm1);
    r.register("norm2", one, norm2);
    r.register("norme", one, norme);
    r.register("normi", one, normi);
    r.register("cond1", one, cond1);
    r.register("cond2", one, cond2);
    r.register("conde", one, conde);
    r.register("condi", one, condi);
    r.register("svds", one, svds);
    r.register_ctx("rref", one, rref);
    r.register_ctx("rank", one, rank);
    r.register("lsolve", Arity::Exact(2), lsolve);
}

fn arg(args: &[Value]) -> Result<&Value, EvalError> {
    args.first()
        .ok_or_else(|| EvalError::Unsupported("missing argument".to_string()))
}

// ---- norms ----------------------------------------------------------------------------------

/// Largest absolute column sum (`norm1`).
fn column_sum_norm(m: &Matrix) -> Result<Scalar, EvalError> {
    let dim = uniform_dim(m.data())?;
    let mut sums = vec![0.0_f64; m.cols()];
    for row in m.data().chunks(m.cols()) {
        for (sum, e) in sums.iter_mut().zip(row) {
            *sum += e.value.norm();
        }
    }
    let best = sums.into_iter().fold(0.0, f64::max);
    finite_scalar(Complex64::new(best, 0.0), dim)
}

/// Largest absolute row sum (`normi`).
fn row_sum_norm(m: &Matrix) -> Result<Scalar, EvalError> {
    let dim = uniform_dim(m.data())?;
    let best = m
        .data()
        .chunks(m.cols())
        .map(|row| row.iter().map(|e| e.value.norm()).sum::<f64>())
        .fold(0.0, f64::max);
    finite_scalar(Complex64::new(best, 0.0), dim)
}

/// Square root of the sum of absolute squares (`norme`, and `norm` of a vector). Folding with
/// `hypot` avoids overflow of the squares.
fn euclidean_norm(m: &Matrix) -> Result<Scalar, EvalError> {
    let dim = uniform_dim(m.data())?;
    let total = m
        .data()
        .iter()
        .fold(0.0_f64, |acc, e| acc.hypot(e.value.norm()));
    finite_scalar(Complex64::new(total, 0.0), dim)
}

fn norm1(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(arg(args)?, "norm1")?;
    Ok(Value::Scalar(column_sum_norm(m)?))
}

fn normi(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(arg(args)?, "normi")?;
    Ok(Value::Scalar(row_sum_norm(m)?))
}

fn norme(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(arg(args)?, "norme")?;
    Ok(Value::Scalar(euclidean_norm(m)?))
}

/// `norm(v)`: the Euclidean norm of a column vector.
fn norm(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args)?, "norm")?;
    if m.cols() != 1 {
        return Err(EvalError::Unsupported(
            "norm needs a column vector".to_string(),
        ));
    }
    Ok(Value::Scalar(euclidean_norm(m)?))
}

/// Largest singular value (`norm2`).
fn norm2(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(arg(args)?, "norm2")?;
    let dim = uniform_dim(m.data())?;
    let sv = singular_values(m)?;
    let top = sv.first().copied().unwrap_or(0.0);
    Ok(Value::Scalar(finite_scalar(Complex64::new(top, 0.0), dim)?))
}

/// `svds(M)`: the singular values of any real matrix, largest first, as a column vector.
fn svds(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args)?, "svds")?;
    let dim = uniform_dim(m.data())?;
    let sv = singular_values(m)?;
    let data = sv
        .into_iter()
        .map(|s| finite_scalar(Complex64::new(s, 0.0), dim))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(Matrix::new(data.len(), 1, data)?))
}

// ---- condition numbers ----------------------------------------------------------------------

/// `norm(M) * norm(M^-1)`. The product of `D` and `D^-1` is dimensionless. A singular matrix
/// has no inverse and gives `Singular`.
fn condition_with(
    v: &Value,
    name: &str,
    norm_of: fn(&Matrix) -> Result<Scalar, EvalError>,
) -> Result<Value, EvalError> {
    let m = expect_square(v, name)?;
    let inv = ops::inverse(m)?;
    let a = norm_of(m)?;
    let b = norm_of(&inv)?;
    ops::mul(&Value::Scalar(a), &Value::Scalar(b))
}

fn cond1(args: &[Value]) -> Result<Value, EvalError> {
    condition_with(arg(args)?, "cond1", column_sum_norm)
}

fn condi(args: &[Value]) -> Result<Value, EvalError> {
    condition_with(arg(args)?, "condi", row_sum_norm)
}

fn conde(args: &[Value]) -> Result<Value, EvalError> {
    condition_with(arg(args)?, "conde", euclidean_norm)
}

/// Ratio of the largest to the smallest singular value. A zero smallest value is `Singular`.
fn cond2(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(arg(args)?, "cond2")?;
    uniform_dim(m.data())?;
    let sv = singular_values(m)?;
    let top = sv.first().copied().unwrap_or(0.0);
    let bottom = sv.last().copied().unwrap_or(0.0);
    if bottom == 0.0 {
        return Err(EvalError::Singular);
    }
    let ratio = finite_scalar(
        Complex64::new(top / bottom, 0.0),
        Dimension::dimensionless(),
    )?;
    Ok(Value::Scalar(ratio))
}

// ---- rref, rank -----------------------------------------------------------------------------

/// The pivot threshold of `rank` and `rref`: the worksheet's `TOL`. A pivot with magnitude at most
/// `TOL` is zero.
///
/// Decision: a negative or non-finite `TOL` is an error, not a clamp. A negative threshold would
/// count every pivot, and a NaN compares false, so either would silently change the answer. Zero
/// is allowed: then only exact zero pivots are zero.
fn pivot_tolerance(env: &BuiltinCtx) -> Result<f64, EvalError> {
    let tol = env.tol()?;
    if tol.is_finite() && tol >= 0.0 {
        Ok(tol)
    } else {
        Err(EvalError::Unsupported(
            "TOL must be a finite non-negative number".to_string(),
        ))
    }
}

/// Gauss-Jordan elimination with partial pivoting on a copy of `m`. Returns the reduced entries
/// (row-major) and the rank. A column whose best pivot has magnitude at most `tol` is skipped, and
/// its entries below the pivot row are set to zero.
fn gauss_jordan(m: &Matrix, tol: f64) -> Result<(Vec<Complex64>, usize), EvalError> {
    let (rows, cols) = m.shape();
    let zero = Complex64::new(0.0, 0.0);
    let one = Complex64::new(1.0, 0.0);
    let mut a: Vec<Complex64> = m.data().iter().map(|s| s.value).collect();
    let mut pivot_row = 0;
    for col in 0..cols {
        if pivot_row == rows {
            break;
        }
        let (best_row, best_mag) = (pivot_row..rows)
            .map(|i| (i, a[i * cols + col].norm()))
            .fold(
                (pivot_row, -1.0),
                |best, cand| {
                    if cand.1 > best.1 {
                        cand
                    } else {
                        best
                    }
                },
            );
        if best_mag <= tol {
            for i in pivot_row..rows {
                a[i * cols + col] = zero;
            }
            continue;
        }
        if best_row != pivot_row {
            for j in 0..cols {
                a.swap(pivot_row * cols + j, best_row * cols + j);
            }
        }
        let p = a[pivot_row * cols + col];
        for j in col..cols {
            a[pivot_row * cols + j] /= p;
        }
        a[pivot_row * cols + col] = one;
        for i in 0..rows {
            if i == pivot_row {
                continue;
            }
            let f = a[i * cols + col];
            if f == zero {
                continue;
            }
            for j in col..cols {
                let t = a[pivot_row * cols + j];
                a[i * cols + j] -= f * t;
            }
            a[i * cols + col] = zero;
        }
        pivot_row += 1;
    }
    Ok((a, pivot_row))
}

/// `rref(A)`: the reduced row echelon form. Elements must be dimensionless.
fn rref(env: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args)?, "rref")?;
    ops::require_dimensionless(uniform_dim(m.data())?)?;
    let (reduced, _) = gauss_jordan(m, pivot_tolerance(env)?)?;
    let data = reduced
        .into_iter()
        .map(|z| finite_scalar(z, Dimension::dimensionless()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(Matrix::new(m.rows(), m.cols(), data)?))
}

/// `rank(A)`: the number of pivots of the reduction, a dimensionless integer.
fn rank(env: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(arg(args)?, "rank")?;
    uniform_dim(m.data())?;
    let (_, r) = gauss_jordan(m, pivot_tolerance(env)?)?;
    Ok(Value::Scalar(Scalar::real(
        r as f64,
        Dimension::dimensionless(),
    )))
}

// ---- linear solve ---------------------------------------------------------------------------

/// `lsolve(A, B)`: the solution `X` of `A X = B`. `A` is square, or has more rows than columns
/// (least squares). `B` has the rows of `A` and any number of columns. A singular `A` gives
/// `Singular`.
fn lsolve(args: &[Value]) -> Result<Value, EvalError> {
    let (Some(av), Some(bv)) = (args.first(), args.get(1)) else {
        return Err(EvalError::Unsupported(
            "lsolve needs a matrix and a vector".to_string(),
        ));
    };
    let a = expect_matrix(av, "lsolve")?;
    let b = expect_matrix(bv, "lsolve")?;
    if b.rows() != a.rows() {
        return Err(EvalError::ShapeMismatch {
            left: a.shape(),
            right: b.shape(),
        });
    }
    let (m, n) = a.shape();
    if m < n {
        return Err(EvalError::Unsupported(
            "lsolve of a wide matrix (fewer rows than columns): the help gives no rule for it"
                .to_string(),
        ));
    }
    let p = b.cols();
    let a_dim = uniform_dim(a.data())?;
    let b_dim = uniform_dim(b.data())?;
    let x_dim = b_dim.checked_div(a_dim).map_err(from_units)?;
    let x = if m == n {
        solve_square(a, b)?
    } else {
        least_squares(a, b)?
    };
    let data = x
        .into_iter()
        .map(|z| finite_scalar(z, x_dim))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(Matrix::new(n, p, data)?))
}

/// Largest element magnitude, the scale of the relative pivot test.
fn max_norm(m: &Matrix) -> f64 {
    m.data().iter().map(|s| s.value.norm()).fold(0.0, f64::max)
}

/// `sum conj(u_k) v_k`.
fn dot(u: &[Complex64], v: &[Complex64]) -> Complex64 {
    u.iter().zip(v).map(|(a, b)| a.conj() * b).sum()
}

/// Solves `A X = B` for square `A`: one LU factorization with partial pivoting, then one pair of
/// triangular substitutions per column of `B`. Returns `X` row-major (n x p).
fn solve_square(a: &Matrix, b: &Matrix) -> Result<Vec<Complex64>, EvalError> {
    let n = a.rows();
    let p = b.cols();
    let scale = max_norm(a);
    let zero = Complex64::new(0.0, 0.0);
    let mut lu: Vec<Complex64> = a.data().iter().map(|s| s.value).collect();
    // perm[k] is the row of B that row k of the factorization came from.
    let mut perm: Vec<usize> = (0..n).collect();
    for k in 0..n {
        let (piv, mag) = (k..n)
            .map(|i| (i, lu[i * n + k].norm()))
            .fold(
                (k, -1.0),
                |best, cand| if cand.1 > best.1 { cand } else { best },
            );
        if mag <= SINGULAR_RATIO * scale {
            return Err(EvalError::Singular);
        }
        if piv != k {
            for j in 0..n {
                lu.swap(k * n + j, piv * n + j);
            }
            perm.swap(k, piv);
        }
        for i in (k + 1)..n {
            let f = lu[i * n + k] / lu[k * n + k];
            lu[i * n + k] = f;
            for j in (k + 1)..n {
                let t = lu[k * n + j];
                lu[i * n + j] -= f * t;
            }
        }
    }
    let mut x = vec![zero; n * p];
    for j in 0..p {
        let rhs: Vec<Complex64> = perm.iter().map(|&r| b.data()[r * p + j].value).collect();
        let col = substitute(&lu, n, rhs);
        for (i, z) in col.into_iter().enumerate() {
            x[i * p + j] = z;
        }
    }
    Ok(x)
}

/// Forward then back substitution with the packed LU factors of `solve_square`.
fn substitute(lu: &[Complex64], n: usize, mut x: Vec<Complex64>) -> Vec<Complex64> {
    for i in 0..n {
        for j in 0..i {
            let t = lu[i * n + j] * x[j];
            x[i] -= t;
        }
    }
    for i in (0..n).rev() {
        for j in (i + 1)..n {
            let t = lu[i * n + j] * x[j];
            x[i] -= t;
        }
        x[i] /= lu[i * n + i];
    }
    x
}

/// Least-squares solution `X` of `A X = B` for `A` with more rows than columns, by modified
/// Gram-Schmidt QR (`A = Q R`, `X = R^-1 Q^H B`). Returns `X` row-major (n x p). A column that
/// is dependent on the earlier ones gives `Singular`.
pub(super) fn least_squares(a: &Matrix, b: &Matrix) -> Result<Vec<Complex64>, EvalError> {
    let (m, n) = a.shape();
    let p = b.cols();
    // A is divided by its largest modulus `s`, so the norms below stay in range (1e-300 and 1e300
    // entries are not squared to zero or infinity). (A / s) X_s = B gives X = X_s / s for A X = B.
    // An all-zero A has no full column rank.
    let s = max_norm(a);
    if !s.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if s == 0.0 {
        return Err(EvalError::Singular);
    }
    let zero = Complex64::new(0.0, 0.0);
    let mut cols: Vec<Vec<Complex64>> = (0..n)
        .map(|j| (0..m).map(|i| a.data()[i * n + j].value / s).collect())
        .collect();
    let mut q: Vec<Vec<Complex64>> = Vec::with_capacity(n);
    let mut r = vec![zero; n * n];
    for j in 0..n {
        let mut v = std::mem::take(&mut cols[j]);
        // Two projection passes against the earlier columns: the second removes the rounding
        // that the first leaves behind.
        for _ in 0..2 {
            for i in 0..j {
                let c = dot(&q[i], &v);
                r[i * n + j] += c;
                for (vt, qt) in v.iter_mut().zip(q[i].iter()) {
                    *vt -= c * *qt;
                }
            }
        }
        // The largest entry of A / s is 1, so the threshold is relative to it.
        let norm = v.iter().map(|z| z.norm_sqr()).sum::<f64>().sqrt();
        if norm <= SINGULAR_RATIO {
            return Err(EvalError::Singular);
        }
        r[j * n + j] = Complex64::new(norm, 0.0);
        for z in &mut v {
            *z /= norm;
        }
        q.push(v);
    }
    let mut x = vec![zero; n * p];
    for j in 0..p {
        let rhs: Vec<Complex64> = (0..m).map(|i| b.data()[i * p + j].value).collect();
        let mut y: Vec<Complex64> = q.iter().map(|qi| dot(qi, &rhs)).collect();
        for i in (0..n).rev() {
            for k in (i + 1)..n {
                let t = r[i * n + k] * y[k];
                y[i] -= t;
            }
            y[i] /= r[i * n + i];
        }
        for (i, z) in y.into_iter().enumerate() {
            x[i * p + j] = z / s;
        }
    }
    Ok(x)
}

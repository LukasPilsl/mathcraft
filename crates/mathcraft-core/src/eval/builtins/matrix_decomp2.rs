//! Matrix factorizations (help: "Matrix Factorization", `*_decomposition.md`, `singular_value_decomposition.md`).
//!
//! - `Cholesky(M, 0)` and `Cholesky(M, 0, 1)`: one matrix, lower and upper, no pivoting.
//! - `Cholesky(M)` and `Cholesky(M, 1, u)`: the nested pair `(P, factor)`, complete pivoting.
//! - `LU(M)`: the nested triple `(P, L, U)` with `P M = L U`, partial pivoting.
//! - `QR(M, p)`: `(P, Q, R)` with `M P = Q R` when `p` is non-zero (default), or `(Q, R)` with
//!   `M = Q R` when `p` is zero. Column pivoting by the largest remaining column norm.
//! - `svd(A)` (real, `rows >= cols`): `(s, U, VH)` with `A = U diag(s) VH`; the kernel is in
//!   `matrix_svd`.
//!
//! Shapes (LAPACK's full forms, as in the help images): `P` is `m x m` for `LU` and `n x n` for
//! `QR`; `L` is `m x min`, `U` is `min x n`; `Q` is `m x m` and `R` is `m x n`; `U` of `svd` is
//! `m x n` and `VH` is `n x n`. `LU` and `QR` accept any `m x n`.
//!
//! Scaling: the entries are divided by the largest modulus before a factorization and the factor
//! is multiplied back, so `1e300 * A` and `1e-300 * A` do not overflow or underflow in the
//! products. Pivot choices do not depend on the scale.
//!
//! Errors (the help gives no codes; these are the choices): a non-square `M` is `NotSquare` where
//! the help requires a square matrix; a matrix that is not Hermitian is `Unsupported` ("not
//! symmetric"); a matrix that is not positive definite (indefinite or singular) is `Unsupported`
//! ("not positive definite"); non-finite entries are `NotFinite`. Elements of the Cholesky factors
//! must be dimensionless. The loops of `LU`, `QR` and `Cholesky` are bounded by the matrix sizes
//! (no iteration cap is needed); `svd` has a sweep cap in `matrix_svd`. The work budget charges
//! each call by its class in `work_table`.

use std::ops::Range;

use num_complex::Complex64;

use super::matrix::{expect_matrix, expect_square, finite_scalar, uniform_dim};
use super::{Arity, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Largest allowed difference `|M_ij - conj(M_ji)|` after scaling by the largest modulus.
const HERMITIAN_TOL: f64 = 1e-12;

pub(super) fn register(r: &mut Registry) {
    r.register("Cholesky", Arity::Range(1, 3), cholesky);
    r.register("LU", Arity::Exact(1), lu);
    r.register("QR", Arity::Range(1, 2), qr);
    r.register("svd", Arity::Exact(1), svd);
}

fn first_arg<'a>(args: &'a [Value], name: &str) -> Result<&'a Value, EvalError> {
    args.first()
        .ok_or_else(|| EvalError::Unsupported(format!("{name} needs a matrix")))
}

/// `Cholesky(M, p, u)`: `p` is the pivoting flag (default 1), `u` the upper/lower flag (default 0).
/// Without pivoting the result is the factor matrix; with pivoting it is `(P, factor)`.
fn cholesky(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(first_arg(args, "Cholesky")?, "Cholesky")?;
    let pivot = flag(args.get(1), true, "Cholesky")?;
    let upper = flag(args.get(2), false, "Cholesky")?;
    let n = m.rows();
    if pivot {
        let (perm, lower) = pivoted_lower_factor(m)?;
        let p = column_permutation(&perm)?;
        let factor = factor_matrix(&lower, n, upper)?;
        return Ok(Value::List(vec![Value::Matrix(p), Value::Matrix(factor)]));
    }
    let lower = lower_factor(m)?;
    Ok(Value::Matrix(factor_matrix(&lower, n, upper)?))
}

/// The lower factor `L`, or the upper factor `U = L^H`, as a matrix of dimensionless scalars.
fn factor_matrix(lower: &[Complex64], n: usize, upper: bool) -> Result<Matrix, EvalError> {
    let mut data = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            let z = if upper {
                // U = L^H: U_ij = conj(L_ji).
                lower[j * n + i].conj()
            } else {
                lower[i * n + j]
            };
            data.push(finite_scalar(z, Dimension::dimensionless())?);
        }
    }
    Matrix::new(n, n, data)
}

/// An optional 0/1 option: `default` when absent, false for zero, true for any other integer (the
/// help: a non-zero value enables the option). Anything that is not an integer is refused.
fn flag(arg: Option<&Value>, default: bool, name: &str) -> Result<bool, EvalError> {
    let Some(v) = arg else {
        return Ok(default);
    };
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!(
            "{name} options must be integers"
        )));
    };
    ops::require_dimensionless(s.dim)?;
    match integer_of(s) {
        Some(0) => Ok(false),
        Some(_) => Ok(true),
        None => Err(EvalError::Unsupported(format!(
            "{name} options must be integers"
        ))),
    }
}

/// The dimensionless real matrix `P` with `P[perm[j]][j] = 1`, so `P^T M P` permutes the rows and
/// columns of `M` by `perm`.
fn column_permutation(perm: &[usize]) -> Result<Matrix, EvalError> {
    let n = perm.len();
    let mut data = vec![Scalar::real(0.0, Dimension::dimensionless()); n * n];
    for (j, &p) in perm.iter().enumerate() {
        data[p * n + j] = Scalar::real(1.0, Dimension::dimensionless());
    }
    Matrix::new(n, n, data)
}

fn not_positive_definite() -> EvalError {
    EvalError::Unsupported("Cholesky: M is not positive definite".to_string())
}

/// The entries divided by the largest modulus (1 when all are zero), and that modulus. Non-finite
/// entries are refused. Division by a real keeps the parts in range.
fn scaled_entries(data: &[Scalar]) -> Result<(f64, Vec<Complex64>), EvalError> {
    if data
        .iter()
        .any(|s| !(s.value.re.is_finite() && s.value.im.is_finite()))
    {
        return Err(EvalError::NotFinite);
    }
    let big = data.iter().map(|s| s.value.norm()).fold(0.0_f64, f64::max);
    let scale = if big == 0.0 { 1.0 } else { big };
    Ok((scale, data.iter().map(|s| s.value / scale).collect()))
}

/// Diagonal equilibration of a Hermitian dimensionless matrix `M` for Cholesky: `s_i = sqrt(M_ii)`
/// and `B_ij = M_ij / (s_i s_j)`, so `B` has a unit diagonal and `M = S B S` with `S = diag(s)`.
/// Each entry is divided in two steps, so no product of diagonal entries is formed. Returns
/// `(d, B)` with `d_i = M_ii`. Checks: finite entries; real positive diagonal (else not positive
/// definite); Hermitian pattern within [`HERMITIAN_TOL`] on `B`.
fn equilibrated(m: &Matrix) -> Result<(Vec<f64>, Vec<Complex64>), EvalError> {
    ops::require_dimensionless(uniform_dim(m.data())?)?;
    if m.data()
        .iter()
        .any(|s| !(s.value.re.is_finite() && s.value.im.is_finite()))
    {
        return Err(EvalError::NotFinite);
    }
    let n = m.rows();
    let entry = |i: usize, j: usize| m.data()[i * n + j].value;
    let mut d = Vec::with_capacity(n);
    for i in 0..n {
        let x = entry(i, i);
        if x.im.abs() > HERMITIAN_TOL * x.re.abs() {
            return Err(not_symmetric());
        }
        if x.re.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Err(not_positive_definite());
        }
        d.push(x.re);
    }
    let s: Vec<f64> = d.iter().map(|x| x.sqrt()).collect();
    let mut b = vec![Complex64::new(0.0, 0.0); n * n];
    for i in 0..n {
        for j in 0..n {
            let z = (entry(i, j) / s[i]) / s[j];
            if !z.norm().is_finite() {
                // Only a matrix that is not positive definite has an entry this large.
                return Err(not_positive_definite());
            }
            b[i * n + j] = z;
        }
        b[i * n + i] = Complex64::new(1.0, 0.0);
    }
    for i in 0..n {
        for j in 0..i {
            if (b[i * n + j] - b[j * n + i].conj()).norm() > HERMITIAN_TOL {
                return Err(not_symmetric());
            }
        }
    }
    Ok((d, b))
}

fn not_symmetric() -> EvalError {
    EvalError::Unsupported("Cholesky: M is not symmetric (Hermitian)".to_string())
}

/// The lower factor `L` (row-major, `n x n`, zeros above the diagonal) with `L L^H = M`. The factor
/// of the equilibrated matrix is scaled back by row: `L = S L_B`.
fn lower_factor(m: &Matrix) -> Result<Vec<Complex64>, EvalError> {
    let n = m.rows();
    let zero = Complex64::new(0.0, 0.0);
    let (d, a) = equilibrated(m)?;
    // Cholesky-Crout on the equilibrated matrix, whose diagonal is 1 and entries are at most 1.
    let mut l = vec![zero; n * n];
    for j in 0..n {
        let mut dj = 1.0_f64;
        for k in 0..j {
            dj -= l[j * n + k].norm_sqr();
        }
        // A pivot not clearly above rounding means the matrix is not positive definite. NaN fails
        // the comparison and is refused too.
        if dj.is_nan() || dj <= n as f64 * f64::EPSILON {
            return Err(not_positive_definite());
        }
        let ljj = dj.sqrt();
        l[j * n + j] = Complex64::new(ljj, 0.0);
        for i in (j + 1)..n {
            let mut x = a[i * n + j];
            for k in 0..j {
                x -= l[i * n + k] * l[j * n + k].conj();
            }
            l[i * n + j] = x / ljj;
        }
    }
    Ok(scale_rows(&l, n, &d))
}

/// Row `i` of the `n x n` row-major `l` multiplied by `sqrt(d[i])`.
fn scale_rows(l: &[Complex64], n: usize, d: &[f64]) -> Vec<Complex64> {
    l.iter()
        .enumerate()
        .map(|(idx, z)| z * d[idx / n].sqrt())
        .collect()
}

/// Complete pivoting (LAPACK's `pstrf` rule): at each step the largest remaining diagonal entry of
/// the Schur complement moves to the front (first one on ties). Returns `perm`, where `perm[j]` is
/// the original index at position `j`, and the lower factor of the permuted matrix, so that
/// `P^T M P = L L^H` with `P[perm[j]][j] = 1`.
///
/// The work is on the equilibrated matrix, whose diagonal is 1. The Schur diagonal of `M` at `p`
/// is `d_p` times the one of `B`, so the pivot is the largest `d_p * remaining_B(p)`: the same
/// choice at any scale, and the rule of the unscaled matrix.
fn pivoted_lower_factor(m: &Matrix) -> Result<(Vec<usize>, Vec<Complex64>), EvalError> {
    let n = m.rows();
    let zero = Complex64::new(0.0, 0.0);
    let (mut d, mut a) = equilibrated(m)?;
    let mut l = vec![zero; n * n];
    let mut perm: Vec<usize> = (0..n).collect();
    for j in 0..n {
        // Remaining diagonal of the equilibrated Schur complement at `p`: 1 - sum_k |L_pk|^2.
        let remaining = |p: usize| 1.0 - (0..j).map(|k| l[p * n + k].norm_sqr()).sum::<f64>();
        let key = |p: usize| d[p] * remaining(p);
        let p = (j..n).fold(j, |best, q| if key(q) > key(best) { q } else { best });
        let r = remaining(p);
        if r.is_nan() || r <= n as f64 * f64::EPSILON {
            return Err(not_positive_definite());
        }
        if p != j {
            // Symmetric swap of positions j and p: rows, then columns, of the equilibrated matrix;
            // the computed part of L (columns below j) and the scales follow the rows.
            for k in 0..n {
                a.swap(j * n + k, p * n + k);
            }
            for k in 0..n {
                a.swap(k * n + j, k * n + p);
            }
            for k in 0..j {
                l.swap(j * n + k, p * n + k);
            }
            perm.swap(j, p);
            d.swap(j, p);
        }
        let ljj = r.sqrt();
        l[j * n + j] = Complex64::new(ljj, 0.0);
        for i in (j + 1)..n {
            let mut x = a[i * n + j];
            for k in 0..j {
                x -= l[i * n + k] * l[j * n + k].conj();
            }
            l[i * n + j] = x / ljj;
        }
    }
    Ok((perm, scale_rows(&l, n, &d)))
}

// ---- LU -------------------------------------------------------------------------------------

/// `LU(M)`: `(P, L, U)` with `P M = L U`. Partial pivoting: the largest modulus in the column
/// (first on ties) is the pivot. `P` is `m x m`, `L` is `m x k`, `U` is `k x n` with `k = min(m,n)`.
/// `U` carries the dimension of `M`; `P` and `L` are dimensionless. A singular `M` still factors
/// (its zero pivots stay zero).
fn lu(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(first_arg(args, "LU")?, "LU")?;
    let (rows, cols) = m.shape();
    uniform_dim(m.data())?;
    let dim = m.data()[0].dim;
    let (scale, mut a) = scaled_entries(m.data())?;
    let k = rows.min(cols);
    let mut perm: Vec<usize> = (0..rows).collect();
    for j in 0..k {
        let p = (j..rows).fold(j, |best, i| {
            if a[i * cols + j].norm() > a[best * cols + j].norm() {
                i
            } else {
                best
            }
        });
        if p != j {
            for c in 0..cols {
                a.swap(j * cols + c, p * cols + c);
            }
            perm.swap(j, p);
        }
        let pivot = a[j * cols + j];
        if pivot == Complex64::new(0.0, 0.0) {
            // Every entry below a zero pivot is zero (the pivot has the largest modulus).
            continue;
        }
        for i in (j + 1)..rows {
            a[i * cols + j] /= pivot;
        }
        for i in (j + 1)..rows {
            let l = a[i * cols + j];
            for c in (j + 1)..cols {
                let upper = a[j * cols + c];
                a[i * cols + c] -= l * upper;
            }
        }
    }
    let one = Complex64::new(1.0, 0.0);
    let zero = Complex64::new(0.0, 0.0);
    // Row i of P M is row perm[i] of M, so P[i][perm[i]] = 1.
    let mut p_data = Vec::with_capacity(rows * rows);
    for &src in &perm {
        for c in 0..rows {
            let z = if c == src { one } else { zero };
            p_data.push(finite_scalar(z, Dimension::dimensionless())?);
        }
    }
    let mut l_data = Vec::with_capacity(rows * k);
    for i in 0..rows {
        for j in 0..k {
            let z = if i == j {
                one
            } else if i > j {
                a[i * cols + j]
            } else {
                zero
            };
            l_data.push(finite_scalar(z, Dimension::dimensionless())?);
        }
    }
    let mut u_data = Vec::with_capacity(k * cols);
    for i in 0..k {
        for c in 0..cols {
            let z = if c >= i {
                a[i * cols + c] * scale
            } else {
                zero
            };
            u_data.push(finite_scalar(z, dim)?);
        }
    }
    Ok(Value::List(vec![
        Value::Matrix(Matrix::new(rows, rows, p_data)?),
        Value::Matrix(Matrix::new(rows, k, l_data)?),
        Value::Matrix(Matrix::new(k, cols, u_data)?),
    ]))
}

// ---- QR -------------------------------------------------------------------------------------

/// A Householder reflector `H = I - 2 v v^H / (v^H v)` acting on rows `j..` (its length is the
/// number of rows it touches): `start` is `j`, `vhv` is `v^H v`.
struct Reflector {
    start: usize,
    v: Vec<Complex64>,
    vhv: f64,
}

/// Applies `H` from the left to the columns in `cols` of a row-major matrix with `ncols` columns.
fn reflect(data: &mut [Complex64], ncols: usize, h: &Reflector, cols: Range<usize>) {
    for c in cols {
        let s: Complex64 =
            h.v.iter()
                .enumerate()
                .map(|(i, vi)| vi.conj() * data[(h.start + i) * ncols + c])
                .sum();
        let f = s * (2.0 / h.vhv);
        for (i, vi) in h.v.iter().enumerate() {
            data[(h.start + i) * ncols + c] -= *vi * f;
        }
    }
}

fn column_norm_sq(a: &[Complex64], cols: usize, rows: usize, c: usize, from: usize) -> f64 {
    (from..rows).map(|i| a[i * cols + c].norm_sqr()).sum()
}

/// `QR(M, p)`: `(P, Q, R)` with `M P = Q R` (pivoting on, the default) or `(Q, R)` with `M = Q R`.
/// Householder reflections; `Q` is `m x m` unitary, `R` is `m x n` upper trapezoidal, `P` is
/// `n x n`. Pivoting picks the column with the largest remaining norm (first on ties), as LAPACK's
/// `geqp3`. `R` carries the dimension of `M`; `P` and `Q` are dimensionless.
fn qr(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(first_arg(args, "QR")?, "QR")?;
    let pivot = flag(args.get(1), true, "QR")?;
    let (rows, cols) = m.shape();
    uniform_dim(m.data())?;
    let dim = m.data()[0].dim;
    let (scale, mut a) = scaled_entries(m.data())?;
    let k = rows.min(cols);
    let mut piv: Vec<usize> = (0..cols).collect();
    let mut reflectors = Vec::with_capacity(k);
    for j in 0..k {
        if pivot {
            let c = (j..cols).fold(j, |best, c| {
                if column_norm_sq(&a, cols, rows, c, j) > column_norm_sq(&a, cols, rows, best, j) {
                    c
                } else {
                    best
                }
            });
            if c != j {
                for i in 0..rows {
                    a.swap(i * cols + j, i * cols + c);
                }
                piv.swap(j, c);
            }
        }
        let x: Vec<Complex64> = (j..rows).map(|i| a[i * cols + j]).collect();
        let norm = x.iter().map(|z| z.norm_sqr()).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        // The phase of the leading entry keeps `v` away from cancellation: H x = -phase |x| e_1.
        let x0 = x[0];
        let phase = if x0.norm() == 0.0 {
            Complex64::new(1.0, 0.0)
        } else {
            x0 / x0.norm()
        };
        let mut v = x;
        v[0] += phase * norm;
        let vhv: f64 = v.iter().map(|z| z.norm_sqr()).sum();
        let h = Reflector { start: j, v, vhv };
        reflect(&mut a, cols, &h, j..cols);
        a[j * cols + j] = -phase * norm;
        for i in (j + 1)..rows {
            a[i * cols + j] = Complex64::new(0.0, 0.0);
        }
        reflectors.push(h);
    }
    // Q = H_1 H_2 ... H_k: start from the identity and apply H_k first.
    let mut q = vec![Complex64::new(0.0, 0.0); rows * rows];
    for i in 0..rows {
        q[i * rows + i] = Complex64::new(1.0, 0.0);
    }
    for h in reflectors.iter().rev() {
        reflect(&mut q, rows, h, 0..rows);
    }
    let zero = Complex64::new(0.0, 0.0);
    let mut q_data = Vec::with_capacity(rows * rows);
    for z in q {
        q_data.push(finite_scalar(z, Dimension::dimensionless())?);
    }
    let mut r_data = Vec::with_capacity(rows * cols);
    for i in 0..rows {
        for c in 0..cols {
            let z = if c >= i {
                a[i * cols + c] * scale
            } else {
                zero
            };
            r_data.push(finite_scalar(z, dim)?);
        }
    }
    let q_matrix = Value::Matrix(Matrix::new(rows, rows, q_data)?);
    let r_matrix = Value::Matrix(Matrix::new(rows, cols, r_data)?);
    if pivot {
        let p = column_permutation(&piv)?;
        Ok(Value::List(vec![Value::Matrix(p), q_matrix, r_matrix]))
    } else {
        Ok(Value::List(vec![q_matrix, r_matrix]))
    }
}

// ---- svd ------------------------------------------------------------------------------------

/// `svd(A)`: `(s, U, VH)` with `A = U diag(s) VH` for a real `A` with `rows >= cols` (the help's
/// requirement). `s` is the `n x 1` column of singular values, descending, with the dimension of
/// `A`; `U` is `m x n` and `VH` is `n x n`, both dimensionless. The signs follow the convention of
/// `matrix_svd` (the first non-zero entry of each column of `U` is negative), not LAPACK's.
fn svd(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(first_arg(args, "svd")?, "svd")?;
    let (rows, cols) = m.shape();
    if rows < cols {
        return Err(EvalError::Unsupported(
            "svd needs rows >= columns (help: m >= n)".to_string(),
        ));
    }
    uniform_dim(m.data())?;
    let dim = m.data()[0].dim;
    let parts = super::matrix_svd::full_svd(m)?;
    let mut s_data = Vec::with_capacity(cols);
    for &sv in &parts.s {
        s_data.push(finite_scalar(Complex64::new(sv, 0.0), dim)?);
    }
    let mut u_data = Vec::with_capacity(rows * cols);
    for i in 0..rows {
        for col in &parts.u {
            u_data.push(finite_scalar(
                Complex64::new(col[i], 0.0),
                Dimension::dimensionless(),
            )?);
        }
    }
    // VH = V^T: row k of VH is column k of V.
    let mut vh_data = Vec::with_capacity(cols * cols);
    for col in &parts.v {
        for &x in col {
            vh_data.push(finite_scalar(
                Complex64::new(x, 0.0),
                Dimension::dimensionless(),
            )?);
        }
    }
    Ok(Value::List(vec![
        Value::Matrix(Matrix::new(cols, 1, s_data)?),
        Value::Matrix(Matrix::new(rows, cols, u_data)?),
        Value::Matrix(Matrix::new(cols, cols, vh_data)?),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_matrix(n: usize, vals: &[f64]) -> Matrix {
        let data = vals
            .iter()
            .map(|x| Scalar::real(*x, Dimension::dimensionless()))
            .collect();
        Matrix::new(n, n, data).expect("valid test matrix")
    }

    fn rect(rows: usize, cols: usize, vals: &[f64]) -> Matrix {
        let data = vals
            .iter()
            .map(|x| Scalar::real(*x, Dimension::dimensionless()))
            .collect();
        Matrix::new(rows, cols, data).expect("valid test matrix")
    }

    fn complex_matrix(n: usize, vals: &[(f64, f64)]) -> Matrix {
        let data = vals
            .iter()
            .map(|(re, im)| Scalar::new(Complex64::new(*re, *im), Dimension::dimensionless()))
            .collect();
        Matrix::new(n, n, data).expect("valid test matrix")
    }

    fn lower_of(m: &Matrix) -> Vec<Complex64> {
        lower_factor(m).expect("factor")
    }

    fn chol(m: &Matrix, args: &[f64]) -> Result<Vec<Complex64>, EvalError> {
        let mut values = vec![Value::Matrix(m.clone())];
        values.extend(
            args.iter()
                .map(|x| Value::Scalar(Scalar::real(*x, Dimension::dimensionless()))),
        );
        match cholesky(&values)? {
            Value::Matrix(out) => Ok(out.data().iter().map(|s| s.value).collect()),
            other => panic!("expected a matrix, got {other:?}"),
        }
    }

    /// `(P, factor)` of a pivoted Cholesky, as complex data.
    fn pivoted(m: &Matrix, upper: bool) -> (Vec<Complex64>, Vec<Complex64>) {
        let values = vec![
            Value::Matrix(m.clone()),
            Value::Scalar(Scalar::real(1.0, Dimension::dimensionless())),
            Value::Scalar(Scalar::real(
                f64::from(u8::from(upper)),
                Dimension::dimensionless(),
            )),
        ];
        match cholesky(&values).expect("pivoted") {
            Value::List(items) => match (&items[0], &items[1]) {
                (Value::Matrix(p), Value::Matrix(f)) => (
                    p.data().iter().map(|s| s.value).collect(),
                    f.data().iter().map(|s| s.value).collect(),
                ),
                other => panic!("bad pair {other:?}"),
            },
            other => panic!("expected a pair, got {other:?}"),
        }
    }

    /// `L L^H` for a row-major `n x n` factor.
    fn llh(l: &[Complex64], n: usize) -> Vec<Complex64> {
        let mut out = vec![Complex64::new(0.0, 0.0); n * n];
        for i in 0..n {
            for j in 0..n {
                out[i * n + j] = (0..n).map(|k| l[i * n + k] * l[j * n + k].conj()).sum();
            }
        }
        out
    }

    /// Row-major product of `(r x k)` and `(k x c)` data.
    fn mul(a: &[Complex64], r: usize, k: usize, b: &[Complex64], c: usize) -> Vec<Complex64> {
        let mut out = vec![Complex64::new(0.0, 0.0); r * c];
        for i in 0..r {
            for j in 0..c {
                out[i * c + j] = (0..k).map(|t| a[i * k + t] * b[t * c + j]).sum();
            }
        }
        out
    }

    fn values(m: &Matrix) -> Vec<Complex64> {
        m.data().iter().map(|s| s.value).collect()
    }

    fn assert_close(got: &[Complex64], want: &[Complex64], tol: f64) {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).norm() <= tol, "got {g}, want {w}");
        }
    }

    fn triple(v: Value) -> Vec<Matrix> {
        match v {
            Value::List(items) => items
                .into_iter()
                .map(|x| match x {
                    Value::Matrix(m) => m,
                    other => panic!("expected a matrix, got {other:?}"),
                })
                .collect(),
            other => panic!("expected a list, got {other:?}"),
        }
    }

    fn spd_4() -> Matrix {
        // A^T A + I for a 4 x 4 A, so it is symmetric positive definite.
        let a = [
            1.0, 2.0, -1.0, 0.5, 0.3, -2.0, 1.5, 2.0, 2.0, 0.1, 1.0, -1.0, 0.0, 1.0, 2.5, 3.0,
        ];
        let mut m = vec![0.0; 16];
        for i in 0..4 {
            for j in 0..4 {
                m[i * 4 + j] = (0..4).map(|k| a[k * 4 + i] * a[k * 4 + j]).sum::<f64>();
                if i == j {
                    m[i * 4 + j] += 1.0;
                }
            }
        }
        real_matrix(4, &m)
    }

    /// The help's M1 for LU and QR (dimensions 4 x 3).
    fn help_m1() -> Matrix {
        rect(
            4,
            3,
            &[
                3.0, 1.0, 2.0, 7.0, -1.0, 2.0, -2.0, 8.0, 2.0, 3.0, -1.0, 9.0,
            ],
        )
    }

    #[test]
    fn help_example_lower_factor_matches_the_printed_values() {
        // Help image example_matrix_factorization_ch_real14: M = (5 2 3; 2 7 -4; 3 -4 9).
        let m = real_matrix(3, &[5.0, 2.0, 3.0, 2.0, 7.0, -4.0, 3.0, -4.0, 9.0]);
        let l = chol(&m, &[0.0]).expect("factor");
        let want = [2.236, 0.0, 0.0, 0.894, 2.49, 0.0, 1.342, -2.088, 1.685];
        for (g, w) in l.iter().zip(want) {
            assert!((g.re - w).abs() < 5e-4, "got {}, want {w}", g.re);
            assert_eq!(g.im, 0.0);
        }
    }

    #[test]
    fn help_example_upper_factor_is_the_transpose_of_the_lower() {
        // Help image example_matrix_factorization_ch_real27: U = L^T with the same values.
        let m = real_matrix(3, &[5.0, 2.0, 3.0, 2.0, 7.0, -4.0, 3.0, -4.0, 9.0]);
        let u = chol(&m, &[0.0, 1.0]).expect("factor");
        let want = [2.236, 0.894, 1.342, 0.0, 2.49, -2.088, 0.0, 0.0, 1.685];
        for (g, w) in u.iter().zip(want) {
            assert!((g.re - w).abs() < 5e-4, "got {}, want {w}", g.re);
        }
    }

    #[test]
    fn help_example_pivoted_cholesky_matches_the_printed_values() {
        // Help image example_matrix_factorization_ch_real7: P = (0 0 1; 0 1 0; 1 0 0) and
        // L = (3 0 0; -1.333 2.285 0; 1 1.459 1.368).
        let m = real_matrix(3, &[5.0, 2.0, 3.0, 2.0, 7.0, -4.0, 3.0, -4.0, 9.0]);
        let (p, l) = pivoted(&m, false);
        let want_p = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0];
        assert_close(&p, &want_p.map(|x| Complex64::new(x, 0.0)), 0.0);
        let want_l = [3.0, 0.0, 0.0, -1.333, 2.285, 0.0, 1.0, 1.459, 1.368];
        for (g, w) in l.iter().zip(want_l) {
            assert!((g.re - w).abs() < 5e-4, "got {}, want {w}", g.re);
        }
    }

    #[test]
    fn pivoted_cholesky_reproduces_the_permuted_matrix() {
        let m = spd_4();
        let target = values(&m);
        let (p, l) = pivoted(&m, false);
        // P^T M P = L L^T.
        let pt: Vec<Complex64> = (0..16).map(|idx| p[(idx % 4) * 4 + idx / 4]).collect();
        let permuted = mul(&mul(&pt, 4, 4, &target, 4), 4, 4, &p, 4);
        assert_close(&llh(&l, 4), &permuted, 1e-12);
        // Upper form: P^T M P = U^T U.
        let (_, u) = pivoted(&m, true);
        let utu: Vec<Complex64> = (0..16)
            .map(|idx| {
                let (i, j) = (idx / 4, idx % 4);
                (0..4).map(|k| u[k * 4 + i].conj() * u[k * 4 + j]).sum()
            })
            .collect();
        assert_close(&utu, &permuted, 1e-12);
    }

    #[test]
    fn lower_and_upper_factors_reproduce_the_matrix() {
        let m = spd_4();
        let target = values(&m);
        let l = chol(&m, &[0.0]).expect("lower");
        assert_close(&llh(&l, 4), &target, 1e-12);
        let u = chol(&m, &[0.0, 1.0]).expect("upper");
        // U^H U = M, so M_ij = sum_k conj(U_ki) U_kj.
        let uhu: Vec<Complex64> = (0..16)
            .map(|idx| {
                let (i, j) = (idx / 4, idx % 4);
                (0..4).map(|k| u[k * 4 + i].conj() * u[k * 4 + j]).sum()
            })
            .collect();
        assert_close(&uhu, &target, 1e-12);
    }

    #[test]
    fn complex_hermitian_factor_matches_the_hand_result() {
        // M = (2, 1+i; 1-i, 2): L11 = sqrt 2, L21 = (1-i)/sqrt 2, L22 = 1.
        let m = complex_matrix(2, &[(2.0, 0.0), (1.0, 1.0), (1.0, -1.0), (2.0, 0.0)]);
        let l = lower_of(&m);
        let s2 = 2.0_f64.sqrt();
        let want = [
            Complex64::new(s2, 0.0),
            Complex64::new(0.0, 0.0),
            Complex64::new(1.0 / s2, -1.0 / s2),
            Complex64::new(1.0, 0.0),
        ];
        assert_close(&l, &want, 1e-12);
    }

    #[test]
    fn huge_and_tiny_scales_give_the_scaled_factor() {
        let base = real_matrix(3, &[5.0, 2.0, 3.0, 2.0, 7.0, -4.0, 3.0, -4.0, 9.0]);
        let reference = chol(&base, &[0.0]).expect("base");
        for (k, root) in [(300, 150.0), (-300, -150.0)] {
            let scaled = real_matrix(
                3,
                &base
                    .data()
                    .iter()
                    .map(|s| s.value.re * 10f64.powi(k))
                    .collect::<Vec<_>>(),
            );
            let l = chol(&scaled, &[0.0]).expect("scaled");
            for (g, r) in l.iter().zip(&reference) {
                let want = r.re * 10f64.powf(root);
                assert!(
                    (g.re - want).abs() <= 1e-12 * want.abs().max(1e-300),
                    "scale 1e{k}: got {}, want {want}",
                    g.re
                );
            }
        }
    }

    #[test]
    fn indefinite_and_singular_matrices_are_not_positive_definite() {
        let indefinite = real_matrix(2, &[1.0, 2.0, 2.0, 1.0]);
        let singular = real_matrix(2, &[1.0, 1.0, 1.0, 1.0]);
        let zero = real_matrix(2, &[0.0; 4]);
        for m in [indefinite, singular, zero] {
            assert!(matches!(
                chol(&m, &[0.0]),
                Err(EvalError::Unsupported(msg)) if msg.contains("not positive definite")
            ));
            assert!(matches!(
                cholesky(&[Value::Matrix(m)]),
                Err(EvalError::Unsupported(msg)) if msg.contains("not positive definite")
            ));
        }
    }

    #[test]
    fn asymmetric_matrix_is_refused() {
        let m = real_matrix(2, &[2.0, 1.0, 0.0, 2.0]);
        assert!(matches!(
            chol(&m, &[0.0]),
            Err(EvalError::Unsupported(msg)) if msg.contains("not symmetric")
        ));
    }

    #[test]
    fn non_square_nan_and_infinite_inputs_are_refused() {
        let rect = Matrix::new(1, 2, vec![Scalar::real(1.0, Dimension::dimensionless()); 2])
            .expect("valid");
        assert!(matches!(
            cholesky(&[Value::Matrix(rect)]),
            Err(EvalError::NotSquare { rows: 1, cols: 2 })
        ));
        let nan = real_matrix(1, &[f64::NAN]);
        assert_eq!(chol(&nan, &[0.0]), Err(EvalError::NotFinite));
        let inf = real_matrix(1, &[f64::INFINITY]);
        assert_eq!(chol(&inf, &[0.0]), Err(EvalError::NotFinite));
    }

    #[test]
    fn non_integer_option_is_refused() {
        let m = real_matrix(1, &[4.0]);
        assert!(matches!(chol(&m, &[0.5]), Err(EvalError::Unsupported(_))));
    }

    #[test]
    fn lu_of_the_help_matrix_satisfies_p_m_equals_l_u() {
        let m = help_m1();
        let parts = triple(lu(&[Value::Matrix(m.clone())]).expect("lu"));
        let (p, l, u) = (&parts[0], &parts[1], &parts[2]);
        assert_eq!(p.shape(), (4, 4));
        assert_eq!(l.shape(), (4, 3));
        assert_eq!(u.shape(), (3, 3));
        let pm = mul(&values(p), 4, 4, &values(&m), 3);
        assert_close(&pm, &mul(&values(l), 4, 3, &values(u), 3), 1e-12);
        // Partial pivoting: every multiplier has modulus at most 1.
        assert!(values(l).iter().all(|z| z.norm() <= 1.0 + 1e-12));
        // The first pivot is the largest entry of column 0 (7, row 1 of M).
        assert_eq!(values(p)[1], Complex64::new(1.0, 0.0));
    }

    #[test]
    fn lu_of_a_wide_and_a_complex_matrix_reconstructs_it() {
        let wide = rect(2, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 7.0]);
        let parts = triple(lu(&[Value::Matrix(wide.clone())]).expect("wide"));
        assert_eq!(parts[1].shape(), (2, 2));
        assert_eq!(parts[2].shape(), (2, 3));
        assert_close(
            &mul(&values(&parts[0]), 2, 2, &values(&wide), 3),
            &mul(&values(&parts[1]), 2, 2, &values(&parts[2]), 3),
            1e-12,
        );
        let c = complex_matrix(2, &[(1.0, 2.0), (3.0, -1.0), (0.5, 0.0), (2.0, 2.0)]);
        let parts = triple(lu(&[Value::Matrix(c.clone())]).expect("complex"));
        assert_close(
            &mul(&values(&parts[0]), 2, 2, &values(&c), 2),
            &mul(&values(&parts[1]), 2, 2, &values(&parts[2]), 2),
            1e-12,
        );
    }

    #[test]
    fn lu_of_a_singular_matrix_keeps_a_zero_pivot() {
        let m = real_matrix(2, &[1.0, 2.0, 2.0, 4.0]);
        let parts = triple(lu(&[Value::Matrix(m.clone())]).expect("lu"));
        assert_eq!(values(&parts[2])[3], Complex64::new(0.0, 0.0));
        assert_close(
            &mul(&values(&parts[0]), 2, 2, &values(&m), 2),
            &mul(&values(&parts[1]), 2, 2, &values(&parts[2]), 2),
            1e-12,
        );
    }

    #[test]
    fn lu_scales_with_huge_and_tiny_inputs() {
        let base = help_m1();
        let reference = triple(lu(&[Value::Matrix(base.clone())]).expect("base"));
        for k in [300, -300] {
            let scaled = rect(
                4,
                3,
                &base
                    .data()
                    .iter()
                    .map(|s| s.value.re * 10f64.powi(k))
                    .collect::<Vec<_>>(),
            );
            let parts = triple(lu(&[Value::Matrix(scaled)]).expect("scaled"));
            for (g, r) in values(&parts[2]).iter().zip(values(&reference[2])) {
                let want = r * 10f64.powi(k);
                assert!((g - want).norm() <= 1e-12 * want.norm().max(1e-300));
            }
        }
    }

    #[test]
    fn lu_refuses_non_finite_entries() {
        let m = real_matrix(2, &[1.0, f64::NAN, 0.0, 1.0]);
        assert_eq!(lu(&[Value::Matrix(m)]), Err(EvalError::NotFinite));
    }

    #[test]
    fn qr_with_pivoting_matches_the_help_pivot_and_reconstructs() {
        let m = help_m1();
        let parts = triple(qr(&[Value::Matrix(m.clone())]).expect("qr"));
        assert_eq!(parts.len(), 3);
        let (p, q, r) = (&parts[0], &parts[1], &parts[2]);
        // The help's P for this M1: the largest column (column 2) comes first.
        let want_p = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0];
        assert_close(&values(p), &want_p.map(|x| Complex64::new(x, 0.0)), 0.0);
        assert_eq!(q.shape(), (4, 4));
        assert_eq!(r.shape(), (4, 3));
        assert_close(
            &mul(&values(&m), 4, 3, &values(p), 3),
            &mul(&values(q), 4, 4, &values(r), 3),
            1e-12,
        );
        // Q is unitary and R is upper trapezoidal.
        let qhq = mul(&adjoint(&values(q), 4), 4, 4, &values(q), 4);
        for i in 0..4 {
            for j in 0..4 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((qhq[i * 4 + j] - Complex64::new(want, 0.0)).norm() < 1e-12);
            }
        }
        for i in 0..4 {
            for c in 0..i.min(3) {
                assert_eq!(values(r)[i * 3 + c], Complex64::new(0.0, 0.0));
            }
        }
    }

    fn adjoint(a: &[Complex64], n: usize) -> Vec<Complex64> {
        (0..n * n)
            .map(|idx| a[(idx % n) * n + idx / n].conj())
            .collect()
    }

    #[test]
    fn qr_without_pivoting_has_two_nested_results_and_reconstructs() {
        let m = help_m1();
        let parts = triple(
            qr(&[
                Value::Matrix(m.clone()),
                Value::Scalar(Scalar::real(0.0, Dimension::dimensionless())),
            ])
            .expect("qr"),
        );
        assert_eq!(parts.len(), 2);
        assert_close(
            &values(&m),
            &mul(&values(&parts[0]), 4, 4, &values(&parts[1]), 3),
            1e-12,
        );
    }

    #[test]
    fn qr_of_a_wide_complex_matrix_reconstructs_it() {
        let wide = Matrix::new(
            2,
            3,
            [
                (1.0, 2.0),
                (0.0, -1.0),
                (3.0, 0.5),
                (2.0, 0.0),
                (-1.0, 1.0),
                (0.0, 0.0),
            ]
            .iter()
            .map(|(re, im)| Scalar::new(Complex64::new(*re, *im), Dimension::dimensionless()))
            .collect(),
        )
        .expect("valid");
        let parts = triple(qr(&[Value::Matrix(wide.clone())]).expect("qr"));
        assert_close(
            &mul(&values(&wide), 2, 3, &values(&parts[0]), 3),
            &mul(&values(&parts[1]), 2, 2, &values(&parts[2]), 3),
            1e-12,
        );
    }

    #[test]
    fn qr_scales_and_refuses_non_finite_entries() {
        let base = help_m1();
        let scaled = rect(
            4,
            3,
            &base
                .data()
                .iter()
                .map(|s| s.value.re * 1e300)
                .collect::<Vec<_>>(),
        );
        let parts = triple(qr(&[Value::Matrix(scaled.clone())]).expect("scaled"));
        assert_close(
            &mul(&values(&scaled), 4, 3, &values(&parts[0]), 3),
            &mul(&values(&parts[1]), 4, 4, &values(&parts[2]), 3),
            1e290,
        );
        let nan = real_matrix(1, &[f64::NAN]);
        assert_eq!(qr(&[Value::Matrix(nan)]), Err(EvalError::NotFinite));
    }

    #[test]
    fn svd_refuses_wide_matrices_and_non_finite_entries() {
        let wide = rect(2, 3, &[1.0; 6]);
        assert!(matches!(
            svd(&[Value::Matrix(wide)]),
            Err(EvalError::Unsupported(msg)) if msg.contains("rows >= columns")
        ));
        let nan = real_matrix(1, &[f64::NAN]);
        assert_eq!(svd(&[Value::Matrix(nan)]), Err(EvalError::NotFinite));
    }

    #[test]
    fn svd_of_the_help_matrix_reconstructs_it() {
        // The help's M (image example_matrix_factorization_svd0): (1 2 -1; 2.3 4 4; -2 5.1 1; 0 0.8 6).
        // Its singular values are the image's 8.657433, 5.174279, 3.035734.
        let m = rect(
            4,
            3,
            &[1.0, 2.0, -1.0, 2.3, 4.0, 4.0, -2.0, 5.1, 1.0, 0.0, 0.8, 6.0],
        );
        let parts = triple(svd(&[Value::Matrix(m.clone())]).expect("svd"));
        let (s, u, vh) = (&parts[0], &parts[1], &parts[2]);
        assert_eq!(s.shape(), (3, 1));
        for (g, w) in values(s).iter().zip([
            8.657_433_488_599_099,
            5.174_278_883_842_083,
            3.035_734_412_413_042,
        ]) {
            assert!((g.re - w).abs() < 1e-9, "got {}, want {w}", g.re);
        }
        let mut usv = vec![Complex64::new(0.0, 0.0); 12];
        for i in 0..4 {
            for j in 0..3 {
                usv[i * 3 + j] = (0..3)
                    .map(|k| values(u)[i * 3 + k] * values(s)[k] * values(vh)[k * 3 + j])
                    .sum();
            }
        }
        assert_close(&usv, &values(&m), 1e-12);
    }

    #[test]
    fn cholesky_of_a_badly_scaled_diagonal_is_positive_definite() {
        // The diagonal spans 1e-600 to 1e600: a global scale by the largest entry underflows the
        // small pivot. Equilibration keeps both: L = diag(1e-150, 1e150).
        let m = real_matrix(2, &[1e-300, 0.0, 0.0, 1e300]);
        let l = chol(&m, &[0.0]).expect("lower");
        assert!((l[0].re - 1e-150).abs() <= 1e-12 * 1e-150, "{}", l[0].re);
        assert!((l[3].re - 1e150).abs() <= 1e-12 * 1e150, "{}", l[3].re);
        assert_eq!(l[1].re, 0.0);
        // Pivoting takes the large diagonal first: P swaps the two positions, L = diag(1e150, 1e-150).
        let (p, pl) = pivoted(&m, false);
        assert_close(
            &p,
            &[0.0, 1.0, 1.0, 0.0].map(|x| Complex64::new(x, 0.0)),
            0.0,
        );
        assert!((pl[0].re - 1e150).abs() <= 1e-12 * 1e150, "{}", pl[0].re);
        assert!((pl[3].re - 1e-150).abs() <= 1e-12 * 1e-150, "{}", pl[3].re);
    }

    #[test]
    fn cholesky_with_mixed_scales_reproduces_the_matrix() {
        // M = S B S with S = diag(1e-150, 1, 1e150) and B the (1 0.5 0; 0.5 1 0.25; 0 0.25 1).
        let sc = [1e-150, 1.0, 1e150];
        let b = [1.0, 0.5, 0.0, 0.5, 1.0, 0.25, 0.0, 0.25, 1.0];
        let mut vals = vec![0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                vals[i * 3 + j] = sc[i] * b[i * 3 + j] * sc[j];
            }
        }
        let m = real_matrix(3, &vals);
        let l = chol(&m, &[0.0]).expect("lower");
        for (g, w) in llh(&l, 3).iter().zip(&vals) {
            assert!(
                (g.re - w).abs() <= 1e-12 * w.abs().max(1e-300),
                "got {}, want {w}",
                g.re
            );
        }
        // The largest diagonal (1e300, original index 2) is the first pivot.
        let (p, pl) = pivoted(&m, false);
        let pivoted_llh = llh(&pl, 3);
        assert!((pivoted_llh[0].re - 1e300).abs() <= 1e-12 * 1e300);
        let anti = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0];
        assert_close(&p, &anti.map(|x| Complex64::new(x, 0.0)), 0.0);
    }

    #[test]
    fn help_complex_hermitian_example_factors_and_reconstructs() {
        // Help image example_matrix_factorization_ch_cmplx0: M = (5, 2+3i, 5i; 2-3i, 7, 1+7i;
        // -5i, 1-7i, 12). M[0][2] is 5i and M[2][0] is -5i (Hermitian).
        let m = complex_matrix(
            3,
            &[
                (5.0, 0.0),
                (2.0, 3.0),
                (0.0, 5.0),
                (2.0, -3.0),
                (7.0, 0.0),
                (1.0, 7.0),
                (0.0, -5.0),
                (1.0, -7.0),
                (12.0, 0.0),
            ],
        );
        let target = values(&m);
        let l = chol(&m, &[0.0]).expect("lower");
        assert_close(&llh(&l, 3), &target, 1e-12);
        let upper = chol(&m, &[0.0, 1.0]).expect("upper");
        let uhu: Vec<Complex64> = (0..9)
            .map(|idx| {
                let (r, c) = (idx / 3, idx % 3);
                (0..3)
                    .map(|k| upper[k * 3 + r].conj() * upper[k * 3 + c])
                    .sum()
            })
            .collect();
        assert_close(&uhu, &target, 1e-12);
        let (p, pl) = pivoted(&m, false);
        let pt: Vec<Complex64> = (0..9).map(|idx| p[(idx % 3) * 3 + idx / 3]).collect();
        let permuted = mul(&mul(&pt, 3, 3, &target, 3), 3, 3, &p, 3);
        assert_close(&llh(&pl, 3), &permuted, 1e-12);
    }
}

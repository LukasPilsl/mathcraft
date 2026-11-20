//! Eigenvalues, eigenvectors and generalized eigenproblems of square matrices, and the generalized
//! (pseudo) inverse `geninv`. Help: "Eigenvectors and Eigenvalues", "Example: Eigenvectors and
//! Eigenvalues", "Example: Finding the Left and Right Eigenvectors", "Example: Using genvals and
//! genvecs Functions", "Rank and Linear Systems Properties of Matrices".
//!
//! Method (complex arithmetic for every input, so real and complex matrices share one path):
//! - Eigenvalues: Householder reduction to upper Hessenberg form, then single-shift QR iteration with
//!   Wilkinson shifts and deflation. The iterations are capped at `MAX_QR_ITERATIONS * n` in total;
//!   a matrix that does not settle by then is an `Unsupported` error, never a guess.
//! - Scale: every solver first divides its matrix by the largest modulus of its entries (and the
//!   eigenvalue `z` by the same factor), so inputs near `1e300` or `1e-300` behave like unit ones.
//!   Eigenvalues are multiplied back; eigenvectors are scale-free. An all-zero matrix is its own case.
//! - Eigenvectors: inverse iteration on `A - (z + d) I`, `d = SHIFT_RATIO * max|A_ij|`, with
//!   `SOLVES` solves from one LU factorization. A zero pivot is replaced by a tiny floor, so an exact
//!   eigenvalue does not divide by zero. A zero matrix gives unit vectors.
//! - Left eigenvectors `v^H M = z v^H` are right eigenvectors of `M^H` for `conj(z)`. Generalized:
//!   `M x = z N x` is `B x = z x` with `B = N^-1 M`; a singular `N` is `Singular`. The left vectors
//!   `y^H M = z y^H N` use `B' = (N^H)^-1 M^H` with `conj(z)`.
//!
//! Conventions: eigenvalues sorted by real part, largest first, then by imaginary part, largest
//! first. In every eigenvector the largest-modulus component (the first on a tie) is real and
//! positive. `eigenvec` and `eigenvecs` give unit 2-norm vectors. `genvecs` instead scales each
//! column so its largest-modulus component is 1, as the help's `genvecs(M, N, "L")` example shows
//! (every column's largest entry is 1). The help's signs cannot be reproduced for that example,
//! so here the largest component is positive where the help's may be negative. A real matrix with
//! a real eigenvalue gives a real eigenvector. `eigenvec` and `eigenvecs` return the same vectors
//! here; the help says they may differ, since both are valid.
//!
//! Units: eigenvalues carry the dimension of `M` (of `M / N` for `genvals`); eigenvectors are
//! dimensionless. The `z` of `eigenvec` must have the dimension of `M`.
//!
//! `geninv(A)` (at least as many rows as columns) is the least-squares left inverse: the least-squares
//! solution of `A X = I`. A rank-deficient `A` is `Singular`; the help's `TOL`-based pseudo inverse is
//! not built.

use num_complex::Complex64;

use super::matrix::{expect_matrix, expect_square, finite_scalar, uniform_dim};
use super::matrix_linalg::least_squares;
use super::{Arity, Registry};
use crate::eval::error::{from_units, EvalError};
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

type C = Complex64;

/// QR iterations per eigenvalue of the matrix, on average: the total cap is `MAX_QR_ITERATIONS * n`.
/// A per-eigenvalue cap refused a 40 x 40 all-ones matrix, whose zero eigenvalues need more steps.
const MAX_QR_ITERATIONS: usize = 30;
/// Relative size of the diagonal shift of inverse iteration.
const SHIFT_RATIO: f64 = 1e-9;
/// Solves per eigenvector in inverse iteration (from one LU factorization).
const SOLVES: usize = 3;
/// A pivot below this fraction of the largest element counts as singular.
const SINGULAR_RATIO: f64 = 1e-12;
/// An eigenvalue of a real matrix with an imaginary part below this fraction of the largest element
/// is taken to be real.
const REAL_SNAP_RATIO: f64 = 1e-12;
/// `eigenvec(M, z)` refuses `z` when the relative residual `max_i |(M v - z v)_i| / max|M_ij|` of
/// the unit vector `v` is above this.
const EIGENPAIR_TOLERANCE: f64 = 1e-8;

pub(super) fn register(r: &mut Registry) {
    r.register("eigenvals", Arity::Exact(1), eigenvals);
    r.register("eigenvec", Arity::Exact(2), eigenvec);
    r.register("eigenvecs", Arity::Range(1, 2), eigenvecs);
    r.register("genvals", Arity::Exact(2), genvals);
    r.register("genvecs", Arity::Range(2, 3), genvecs);
    r.register("geninv", Arity::Exact(1), geninv);
}

// ---- builtins -------------------------------------------------------------------------------

/// `eigenvals(M)`: the eigenvalues of a square matrix, as a column vector, sorted (see the module
/// documentation).
fn eigenvals(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(first(args)?, "eigenvals")?;
    let dim = uniform_dim(m.data())?;
    let a = finite_entries(m)?;
    let vals = eigenvalues(&a, m.rows(), is_real(m))?;
    complex_matrix(vals.len(), 1, vals, dim)
}

/// `eigenvec(M, z)`: one unit eigenvector of `M` for the eigenvalue `z`, as a column vector.
fn eigenvec(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(first(args)?, "eigenvec")?;
    let dim = uniform_dim(m.data())?;
    let z = match arg_at(args, 1)? {
        Value::Scalar(s) => *s,
        _ => {
            return Err(EvalError::Unsupported(
                "eigenvec needs a scalar eigenvalue".to_string(),
            ))
        }
    };
    if z.dim != dim {
        return Err(EvalError::DimensionMismatch {
            left: dim,
            right: z.dim,
        });
    }
    let a = finite_entries(m)?;
    let shift = finite_complex(z.value)?;
    let real = is_real(m) && shift.im == 0.0;
    let v = inverse_iteration(&a, m.rows(), shift, 0, real)?;
    check_eigenpair(&a, m.rows(), shift, &v)?;
    complex_matrix(v.len(), 1, v, Dimension::dimensionless())
}

/// `eigenvecs(M, ["L"])`: the unit eigenvectors of `M`, one per column, in the order of `eigenvals`.
/// `"R"` (the default) gives right eigenvectors, `"L"` left ones.
fn eigenvecs(args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_square(first(args)?, "eigenvecs")?;
    let left = side(args.get(1))?;
    let n = m.rows();
    let a = finite_entries(m)?;
    let real = is_real(m);
    let vals = eigenvalues(&a, n, real)?;
    let mat = if left { conj_transpose(&a, n) } else { a };
    let mut data = vec![C::new(0.0, 0.0); n * n];
    for (j, lam) in vals.iter().enumerate() {
        let shift = if left { lam.conj() } else { *lam };
        let v = inverse_iteration(&mat, n, shift, j, real && lam.im == 0.0)?;
        for (i, x) in v.into_iter().enumerate() {
            data[i * n + j] = x;
        }
    }
    complex_matrix(n, n, data, Dimension::dimensionless())
}

/// `genvals(M, N)`: the eigenvalues of `M x = v N x`, sorted as `eigenvals` sorts them.
fn genvals(args: &[Value]) -> Result<Value, EvalError> {
    let (m, nn) = pencil(args, "genvals")?;
    let dim = uniform_dim(m.data())?
        .checked_div(uniform_dim(nn.data())?)
        .map_err(from_units)?;
    let b = pencil_matrix(m, nn, false)?;
    let vals = eigenvalues(&b, m.rows(), is_real(m) && is_real(nn))?;
    complex_matrix(vals.len(), 1, vals, dim)
}

/// `genvecs(M, N, ["L"])`: the generalized eigenvectors, one per column, in the order of `genvals`.
/// `"R"` (the default) gives `M x = v N x`, `"L"` gives `y^H M = v y^H N`. Each column is scaled so
/// its largest entry is 1 (not unit norm), as the help's example shows; see the module documentation.
fn genvecs(args: &[Value]) -> Result<Value, EvalError> {
    let (m, nn) = pencil(args, "genvecs")?;
    let left = side(args.get(2))?;
    let n = m.rows();
    let real = is_real(m) && is_real(nn);
    let right = pencil_matrix(m, nn, false)?;
    let vals = eigenvalues(&right, n, real)?;
    let mat = if left {
        pencil_matrix(m, nn, true)?
    } else {
        right
    };
    let mut data = vec![C::new(0.0, 0.0); n * n];
    for (j, lam) in vals.iter().enumerate() {
        let shift = if left { lam.conj() } else { *lam };
        let mut v = inverse_iteration(&mat, n, shift, j, real && lam.im == 0.0)?;
        scale_to_largest(&mut v);
        for (i, x) in v.into_iter().enumerate() {
            data[i * n + j] = x;
        }
    }
    complex_matrix(n, n, data, Dimension::dimensionless())
}

/// `geninv(A)`: the least-squares left inverse of `A` (rows at least columns), a `cols x rows` matrix.
fn geninv(args: &[Value]) -> Result<Value, EvalError> {
    let a = expect_matrix(first(args)?, "geninv")?;
    let (m, n) = a.shape();
    if m < n {
        return Err(EvalError::Unsupported(
            "geninv needs at least as many rows as columns".to_string(),
        ));
    }
    let dim = Dimension::dimensionless()
        .checked_div(uniform_dim(a.data())?)
        .map_err(from_units)?;
    let identity: Vec<Scalar> = (0..m * m)
        .map(|k| {
            let v = if k / m == k % m { 1.0 } else { 0.0 };
            Scalar::real(v, Dimension::dimensionless())
        })
        .collect();
    let rhs = Matrix::new(m, m, identity)?;
    let x = least_squares(a, &rhs)?;
    complex_matrix(n, m, x, dim)
}

// ---- argument helpers -----------------------------------------------------------------------

fn first(args: &[Value]) -> Result<&Value, EvalError> {
    arg_at(args, 0)
}

fn arg_at(args: &[Value], i: usize) -> Result<&Value, EvalError> {
    args.get(i)
        .ok_or_else(|| EvalError::Unsupported("missing argument".to_string()))
}

/// The optional `"L"` / `"R"` argument: true for left. Absent means right.
fn side(v: Option<&Value>) -> Result<bool, EvalError> {
    match v {
        None => Ok(false),
        Some(Value::Str(s)) if s == "R" => Ok(false),
        Some(Value::Str(s)) if s == "L" => Ok(true),
        Some(_) => Err(EvalError::Unsupported(
            "the eigenvector side must be \"L\" or \"R\"".to_string(),
        )),
    }
}

/// The two square matrices of a generalized problem, of one shape.
fn pencil<'a>(args: &'a [Value], name: &str) -> Result<(&'a Matrix, &'a Matrix), EvalError> {
    let m = expect_square(first(args)?, name)?;
    let nn = expect_square(arg_at(args, 1)?, name)?;
    if nn.shape() != m.shape() {
        return Err(EvalError::ShapeMismatch {
            left: m.shape(),
            right: nn.shape(),
        });
    }
    Ok((m, nn))
}

fn is_real(m: &Matrix) -> bool {
    m.data().iter().all(|s| s.value.im == 0.0)
}

/// Row-major complex copy of a matrix, refusing non-finite entries.
fn finite_entries(m: &Matrix) -> Result<Vec<C>, EvalError> {
    m.data().iter().map(|s| finite_complex(s.value)).collect()
}

fn finite_complex(z: C) -> Result<C, EvalError> {
    if z.re.is_finite() && z.im.is_finite() {
        Ok(z)
    } else {
        Err(EvalError::NotFinite)
    }
}

/// A `rows x cols` matrix value of complex entries with dimension `dim`.
fn complex_matrix(
    rows: usize,
    cols: usize,
    data: Vec<C>,
    dim: Dimension,
) -> Result<Value, EvalError> {
    let scalars = data
        .into_iter()
        .map(|z| finite_scalar(z, dim))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(Matrix::new(rows, cols, scalars)?))
}

fn conj_transpose(a: &[C], n: usize) -> Vec<C> {
    let mut out = vec![C::new(0.0, 0.0); n * n];
    for i in 0..n {
        for j in 0..n {
            out[i * n + j] = a[j * n + i].conj();
        }
    }
    out
}

fn max_modulus(a: &[C]) -> f64 {
    a.iter().map(|z| z.norm()).fold(0.0, f64::max)
}

/// Refuses `z` unless it is an eigenvalue of `a` (row-major, `n x n`, finite) for the unit vector
/// `v`: the residual `max_i |(A v - z v)_i| / max|A_ij|` must be at most `EIGENPAIR_TOLERANCE`. The
/// residual is taken on `A / scale` and `z / scale`, so it cannot overflow. A zero matrix has only
/// the eigenvalue 0.
fn check_eigenpair(a: &[C], n: usize, z: C, v: &[C]) -> Result<(), EvalError> {
    let not_eigenvalue = || {
        EvalError::Unsupported(
            "the value is not an eigenvalue of the matrix, so it has no eigenvector".to_string(),
        )
    };
    let scale = max_modulus(a);
    if scale == 0.0 {
        return if z == C::new(0.0, 0.0) {
            Ok(())
        } else {
            Err(not_eigenvalue())
        };
    }
    let scaled: Vec<C> = a.iter().map(|x| x / scale).collect();
    let zs = z / scale;
    let mut worst = 0.0_f64;
    for i in 0..n {
        let av: C = (0..n).map(|j| scaled[i * n + j] * v[j]).sum();
        let r = (av - zs * v[i]).norm();
        if r.is_nan() {
            // `f64::max` would drop a NaN, so it is refused here.
            return Err(not_eigenvalue());
        }
        worst = worst.max(r);
    }
    if worst <= EIGENPAIR_TOLERANCE {
        Ok(())
    } else {
        Err(not_eigenvalue())
    }
}

// ---- eigenvalues ----------------------------------------------------------------------------

/// Eigenvalues of the `n x n` matrix `a` (row-major, finite), sorted: by real part descending, then
/// imaginary part descending. For a real input, an imaginary part negligible against the largest
/// element is set to zero.
fn eigenvalues(a: &[C], n: usize, real: bool) -> Result<Vec<C>, EvalError> {
    // Work on A / scale, whose largest entry is 1; the eigenvalues are multiplied back at the end.
    let scale = unit_scale(a)?;
    let mut h: Vec<C> = a.iter().map(|z| z / scale).collect();
    to_hessenberg(&mut h, n);
    let mut vals = hessenberg_eigenvalues(&mut h, n)?;
    if real {
        for z in &mut vals {
            if z.im.abs() <= REAL_SNAP_RATIO {
                z.im = 0.0;
            }
        }
    }
    // Real parts equal to within rounding (a conjugate pair) are ties, broken by the imaginary part.
    // The key is a rounded real part, so the order is total (a comparator that is not can panic).
    // The keys are in scaled units, where the largest element is 1.
    let tol = 1e-9;
    vals.sort_by(|p, q| {
        let (kp, kq) = ((p.re / tol).round(), (q.re / tol).round());
        kq.total_cmp(&kp).then(q.im.total_cmp(&p.im))
    });
    for z in &mut vals {
        *z *= scale;
    }
    Ok(vals)
}

/// The factor a solver divides its matrix by: the largest modulus of the entries. An all-zero matrix
/// gives 1 (nothing to scale). A largest modulus that overflows is `NotFinite`.
fn unit_scale(a: &[C]) -> Result<f64, EvalError> {
    let s = max_modulus(a);
    if !s.is_finite() {
        Err(EvalError::NotFinite)
    } else if s > 0.0 {
        Ok(s)
    } else {
        Ok(1.0)
    }
}

/// Reduces `a` (n x n, row-major) to upper Hessenberg form in place, by Householder reflections
/// `P = I - 2 v v^H / (v^H v)`, applied from both sides. A similarity: the eigenvalues are kept.
fn to_hessenberg(a: &mut [C], n: usize) {
    for k in 0..n.saturating_sub(2) {
        let len = n - k - 1;
        let x: Vec<C> = (0..len).map(|i| a[(k + 1 + i) * n + k]).collect();
        let tail: f64 = x[1..].iter().map(|z| z.norm_sqr()).sum();
        if tail == 0.0 {
            continue;
        }
        let norm = (x[0].norm_sqr() + tail).sqrt();
        let phase = if x[0].norm() == 0.0 {
            C::new(1.0, 0.0)
        } else {
            x[0] / x[0].norm()
        };
        let mut v = x;
        v[0] += phase * norm;
        let vn: f64 = v.iter().map(|z| z.norm_sqr()).sum();
        if vn == 0.0 {
            continue;
        }
        // Rows k+1.. of every column: P a.
        for j in 0..n {
            let w: C = (0..len).map(|i| v[i].conj() * a[(k + 1 + i) * n + j]).sum();
            let f = w * (2.0 / vn);
            for (i, vi) in v.iter().enumerate() {
                a[(k + 1 + i) * n + j] -= *vi * f;
            }
        }
        // Columns k+1.. of every row: a P.
        for i in 0..n {
            let w: C = (0..len).map(|j| a[i * n + k + 1 + j] * v[j]).sum();
            let f = w * (2.0 / vn);
            for (j, vj) in v.iter().enumerate() {
                a[i * n + k + 1 + j] -= f * vj.conj();
            }
        }
        // The reflection maps the column to alpha e1; set it exactly.
        a[(k + 1) * n + k] = -phase * norm;
        for i in (k + 2)..n {
            a[i * n + k] = C::new(0.0, 0.0);
        }
    }
}

/// Eigenvalues of the upper Hessenberg matrix `h` (n x n, consumed) by single-shift QR with
/// deflation. The Schur vectors are not formed: the eigenvectors come from inverse iteration.
fn hessenberg_eigenvalues(h: &mut [C], n: usize) -> Result<Vec<C>, EvalError> {
    let scale = max_modulus(h);
    let budget = MAX_QR_ITERATIONS * n;
    let mut hi = n - 1;
    // `total` is capped; `stall` counts the steps since the last deflation (for the exceptional shift).
    let mut total = 0usize;
    let mut stall = 0usize;
    while hi > 0 {
        // The unreduced block is lo..=hi: the subdiagonal above lo is negligible.
        let mut lo = hi;
        while lo > 0 {
            // A pair of diagonal entries that are both tiny (a cluster of zero eigenvalues settles near
            // 1e-185, not 0) would make the relative test below unreachable: the matrix scale is used.
            let mut s = h[(lo - 1) * n + lo - 1].norm() + h[lo * n + lo].norm();
            if s < f64::EPSILON * scale {
                s = scale;
            }
            if h[lo * n + lo - 1].norm() <= f64::EPSILON * s {
                h[lo * n + lo - 1] = C::new(0.0, 0.0);
                break;
            }
            lo -= 1;
        }
        if lo == hi {
            hi -= 1;
            stall = 0;
            continue;
        }
        total += 1;
        stall += 1;
        if total > budget {
            return Err(EvalError::Unsupported(format!(
                "eigenvalues did not converge in {budget} QR iterations"
            )));
        }
        let mu = if stall.is_multiple_of(10) {
            // Exceptional shift, to break a stall.
            h[hi * n + hi] + C::new(0.75 * h[hi * n + hi - 1].norm(), 0.0)
        } else {
            wilkinson_shift(h, n, hi)
        };
        qr_step(h, n, lo, hi, mu);
    }
    Ok((0..n).map(|i| h[i * n + i]).collect())
}

/// The eigenvalue of the trailing 2 x 2 block (rows and columns `hi-1`, `hi`) closer to `h[hi][hi]`.
fn wilkinson_shift(h: &[C], n: usize, hi: usize) -> C {
    let a = h[(hi - 1) * n + hi - 1];
    let b = h[(hi - 1) * n + hi];
    let c = h[hi * n + hi - 1];
    let d = h[hi * n + hi];
    let half = (a - d) * 0.5;
    let root = (half * half + b * c).sqrt();
    let mid = d + half;
    let (e1, e2) = (mid + root, mid - root);
    if (e1 - d).norm() <= (e2 - d).norm() {
        e1
    } else {
        e2
    }
}

/// Rotation `G = [c s; -conj(s) c]` (c real, unitary) with `G [x; y] = [r; 0]`.
fn givens(x: C, y: C) -> (f64, C) {
    let ax = x.norm();
    let r = ax.hypot(y.norm());
    if r == 0.0 {
        return (1.0, C::new(0.0, 0.0));
    }
    let c = ax / r;
    let s = if ax == 0.0 {
        y.conj() / r
    } else {
        (x / ax) * y.conj() / r
    };
    (c, s)
}

/// One explicit shifted QR step on the block `lo..=hi`: `H - mu I = Q R`, `H <- R Q + mu I`. The
/// rows above and the columns right of the block are updated too, so the matrix stays similar to
/// the input.
fn qr_step(h: &mut [C], n: usize, lo: usize, hi: usize, mu: C) {
    for i in lo..=hi {
        h[i * n + i] -= mu;
    }
    let mut rotations = Vec::with_capacity(hi - lo);
    for k in lo..hi {
        let (c, s) = givens(h[k * n + k], h[(k + 1) * n + k]);
        for j in k..n {
            let p = h[k * n + j];
            let q = h[(k + 1) * n + j];
            h[k * n + j] = p * c + s * q;
            h[(k + 1) * n + j] = q * c - s.conj() * p;
        }
        rotations.push((k, c, s));
    }
    for (k, c, s) in rotations {
        for i in 0..=hi {
            let p = h[i * n + k];
            let q = h[i * n + k + 1];
            h[i * n + k] = p * c + q * s.conj();
            h[i * n + k + 1] = q * c - p * s;
        }
    }
    for i in lo..=hi {
        h[i * n + i] += mu;
    }
}

// ---- inverse iteration and LU ---------------------------------------------------------------

/// LU factors with partial pivoting: `P A = L U`, `perm[k]` the row of `A` in row `k` of `P A`.
struct Lu {
    n: usize,
    lu: Vec<C>,
    perm: Vec<usize>,
}

/// Factors `a` (n x n, consumed). A pivot of modulus below `floor` is replaced by `floor`; the
/// returned flag says whether that happened.
fn lu_factor(mut a: Vec<C>, n: usize, floor: f64) -> (Lu, bool) {
    let mut perm: Vec<usize> = (0..n).collect();
    let mut replaced = false;
    for k in 0..n {
        let mut piv = k;
        let mut best = a[k * n + k].norm();
        for i in (k + 1)..n {
            let m = a[i * n + k].norm();
            if m > best {
                best = m;
                piv = i;
            }
        }
        if piv != k {
            for j in 0..n {
                a.swap(k * n + j, piv * n + j);
            }
            perm.swap(k, piv);
        }
        if a[k * n + k].norm() < floor {
            a[k * n + k] = C::new(floor, 0.0);
            replaced = true;
        }
        let pivot = a[k * n + k];
        for i in (k + 1)..n {
            let f = a[i * n + k] / pivot;
            a[i * n + k] = f;
            for j in (k + 1)..n {
                let t = a[k * n + j];
                a[i * n + j] -= f * t;
            }
        }
    }
    (Lu { n, lu: a, perm }, replaced)
}

/// Entry magnitude above which `lu_solve_bounded` rescales its solution. Far below the overflow
/// limit, so the next step cannot overflow.
const GROWTH_LIMIT: f64 = 1e100;

/// Solves `A x = b` with the factors of `lu_factor`, exactly (no rescaling).
fn lu_solve(f: &Lu, b: &[C]) -> Vec<C> {
    lu_solve_bounded(f, b, f64::INFINITY)
}

/// Solves `A x = b` with the factors of `lu_factor`. Back substitution can grow the solution
/// without bound (a nearly singular or defective `A`), so when an entry exceeds `limit` the whole
/// vector is divided by its largest modulus. The result is then the solution times a positive
/// factor: only its direction is meaningful, which is all inverse iteration uses.
fn lu_solve_bounded(f: &Lu, b: &[C], limit: f64) -> Vec<C> {
    let n = f.n;
    let mut x: Vec<C> = f.perm.iter().map(|&r| b[r]).collect();
    for i in 0..n {
        for j in 0..i {
            let t = f.lu[i * n + j] * x[j];
            x[i] -= t;
        }
    }
    for i in (0..n).rev() {
        for j in (i + 1)..n {
            let t = f.lu[i * n + j] * x[j];
            x[i] -= t;
        }
        x[i] /= f.lu[i * n + i];
        if x[i].norm() > limit {
            let big = max_modulus(&x);
            for z in x.iter_mut() {
                *z /= big;
            }
        }
    }
    x
}

/// A deterministic start vector with entries in [0.5, 1.5), distinct for distinct `index`.
fn start_vector(n: usize, index: usize) -> Vec<C> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64 ^ (index as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let u = (state >> 11) as f64 / (1u64 << 53) as f64;
            C::new(0.5 + u, 0.0)
        })
        .collect()
}

/// Scales `v` to unit 2-norm. Scaling by the largest modulus first keeps the squares finite.
fn normalize(v: &mut [C]) -> Result<(), EvalError> {
    let big = max_modulus(v);
    if big == 0.0 || !big.is_finite() {
        return Err(EvalError::NotFinite);
    }
    for z in v.iter_mut() {
        *z /= big;
    }
    let norm = v.iter().map(|z| z.norm_sqr()).sum::<f64>().sqrt();
    for z in v.iter_mut() {
        *z /= norm;
    }
    Ok(())
}

/// The convention of the help examples: the largest-modulus component (the first on a tie) is real
/// and positive. For a real eigenvector of a real matrix the imaginary parts are dropped.
fn fix_phase(v: &mut [C], real: bool) {
    let mut best = 0;
    let mut best_mod = -1.0;
    for (i, z) in v.iter().enumerate() {
        let m = z.norm();
        if m > best_mod {
            best_mod = m;
            best = i;
        }
    }
    if best_mod <= 0.0 {
        return;
    }
    let rot = (v[best] / best_mod).conj();
    for z in v.iter_mut() {
        *z *= rot;
    }
    v[best] = C::new(best_mod, 0.0);
    if real {
        for z in v.iter_mut() {
            z.im = 0.0;
        }
    }
}

/// Scales `v` so its largest-modulus component has modulus 1 (`genvecs`). Called after `fix_phase`,
/// so that component is real and positive and becomes exactly 1.
fn scale_to_largest(v: &mut [C]) {
    let big = max_modulus(v);
    if big > 0.0 {
        for z in v.iter_mut() {
            *z /= big;
        }
    }
}

/// Unit eigenvector of the `n x n` matrix `a` (row-major, finite) for eigenvalue `z`, by inverse
/// iteration. `index` picks the start vector. A zero matrix gives the unit vector `e_index`.
/// The matrix and `z` are divided by the scale of `a` first; the eigenvector does not depend on it.
fn inverse_iteration(
    a: &[C],
    n: usize,
    z: C,
    index: usize,
    real: bool,
) -> Result<Vec<C>, EvalError> {
    if max_modulus(a) == 0.0 {
        let mut e = vec![C::new(0.0, 0.0); n];
        e[index % n] = C::new(1.0, 0.0);
        return Ok(e);
    }
    let scale = unit_scale(a)?;
    let scaled: Vec<C> = a.iter().map(|x| x / scale).collect();
    let z = z / scale;
    let shift = z + C::new(SHIFT_RATIO, 0.0);
    let mut shifted = scaled;
    for i in 0..n {
        shifted[i * n + i] -= shift;
    }
    let (lu, _) = lu_factor(shifted, n, f64::EPSILON);
    let mut x = start_vector(n, index);
    for _ in 0..SOLVES {
        let mut y = lu_solve_bounded(&lu, &x, GROWTH_LIMIT);
        normalize(&mut y)?;
        x = y;
    }
    fix_phase(&mut x, real);
    Ok(x)
}

/// `B = N^-1 M` (or `B = (N^H)^-1 M^H` for the left problem), for `N` nonsingular. Columns are solved
/// one at a time against one factorization.
fn pencil_matrix(m: &Matrix, nn: &Matrix, left: bool) -> Result<Vec<C>, EvalError> {
    let n = m.rows();
    let mut a = finite_entries(m)?;
    let mut d = finite_entries(nn)?;
    if left {
        a = conj_transpose(&a, n);
        d = conj_transpose(&d, n);
    }
    // Both matrices are divided by their largest modulus, so the factorization sees entries of size
    // one. B = (d / sd)^-1 (a / sa) * (sa / sd): the last factor restores the magnitude.
    let sd = max_modulus(&d);
    if !sd.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if sd == 0.0 {
        return Err(EvalError::Singular);
    }
    let sa = unit_scale(&a)?;
    let d: Vec<C> = d.iter().map(|z| z / sd).collect();
    let (lu, replaced) = lu_factor(d, n, SINGULAR_RATIO);
    if replaced {
        return Err(EvalError::Singular);
    }
    let mut b = vec![C::new(0.0, 0.0); n * n];
    for j in 0..n {
        let col: Vec<C> = (0..n).map(|i| a[i * n + j] / sa).collect();
        let x = lu_solve(&lu, &col);
        for (i, z) in x.into_iter().enumerate() {
            b[i * n + j] = z * sa / sd;
        }
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Dimension;

    fn real_matrix(rows: usize, cols: usize, vals: &[f64]) -> Matrix {
        let data = vals
            .iter()
            .map(|x| Scalar::real(*x, Dimension::dimensionless()))
            .collect();
        Matrix::new(rows, cols, data).expect("valid test matrix")
    }

    fn complex_entries(rows: usize, cols: usize, vals: &[(f64, f64)]) -> Matrix {
        let data = vals
            .iter()
            .map(|(re, im)| Scalar::new(C::new(*re, *im), Dimension::dimensionless()))
            .collect();
        Matrix::new(rows, cols, data).expect("valid test matrix")
    }

    /// The entries of a matrix value, row-major.
    fn entries(v: Value) -> Vec<C> {
        match v {
            Value::Matrix(m) => m.data().iter().map(|s| s.value).collect(),
            other => panic!("expected a matrix, got {other:?}"),
        }
    }

    fn assert_close(got: &[C], want: &[(f64, f64)], tol: f64) {
        assert_eq!(got.len(), want.len(), "length: {got:?}");
        for (g, (re, im)) in got.iter().zip(want) {
            assert!(
                (g.re - re).abs() <= tol && (g.im - im).abs() <= tol,
                "got {g}, want {re}{im:+}i (tol {tol}); all: {got:?}"
            );
        }
    }

    /// `|M v - z v|` over the largest entry, for a column of an eigenvector matrix.
    fn residual(a: &[C], n: usize, z: C, v: &[C]) -> f64 {
        let mut worst: f64 = 0.0;
        for i in 0..n {
            let mv: C = (0..n).map(|j| a[i * n + j] * v[j]).sum();
            worst = worst.max((mv - z * v[i]).norm());
        }
        worst / max_modulus(a).max(1.0)
    }

    #[test]
    fn eigenvalues_of_a_real_2x2_match_the_help() {
        // Help: "Example: Finding the Left and Right Eigenvectors", A = [3 2; 3 -2].
        let a = real_matrix(2, 2, &[3.0, 2.0, 3.0, -2.0]);
        let got = entries(eigenvals(&[Value::Matrix(a)]).unwrap());
        assert_close(&got, &[(4.0, 0.0), (-3.0, 0.0)], 1e-12);
    }

    #[test]
    fn eigenvalues_of_the_complex_example_match_the_help() {
        // Help: "Example: Eigenvectors and Eigenvalues", A = [4i -2 6; 3 0 10; 2 5 -1].
        let a = complex_entries(
            3,
            3,
            &[
                (0.0, 4.0),
                (-2.0, 0.0),
                (6.0, 0.0),
                (3.0, 0.0),
                (0.0, 0.0),
                (10.0, 0.0),
                (2.0, 0.0),
                (5.0, 0.0),
                (-1.0, 0.0),
            ],
        );
        let got = entries(eigenvals(&[Value::Matrix(a)]).unwrap());
        assert_close(
            &got,
            &[(7.237, 0.329), (-0.636, 3.665), (-7.6, 0.006)],
            2e-3,
        );
    }

    #[test]
    fn eigenvalues_of_the_6x6_help_matrix_match() {
        // Help: "Example: Using genvals and genvecs Functions", M and N.
        let m = real_matrix(
            6,
            6,
            &[
                7.5, 12.0, 5.5, 5.5, 8.5, 4.0, //
                5.5, 8.5, 4.0, 7.5, 12.0, 5.0, //
                7.5, 12.0, 5.5, 7.5, 12.0, 5.5, //
                5.5, 8.5, 4.0, 4.5, 7.5, 4.0, //
                5.5, 9.0, 4.5, 5.0, 8.0, 4.0, //
                5.5, 8.5, 3.5, 7.0, 11.0, 4.5,
            ],
        );
        let got = entries(eigenvals(&[Value::Matrix(m)]).unwrap());
        assert_close(
            &got,
            &[
                (40.274, 0.0),
                (0.298, 0.148),
                (0.298, -0.148),
                (-0.159, 0.0),
                (-0.343, 0.0),
                (-1.868, 0.0),
            ],
            2e-3,
        );
    }

    #[test]
    fn genvals_of_the_help_pair_match() {
        let m = real_matrix(
            6,
            6,
            &[
                7.5, 12.0, 5.5, 5.5, 8.5, 4.0, //
                5.5, 8.5, 4.0, 7.5, 12.0, 5.0, //
                7.5, 12.0, 5.5, 7.5, 12.0, 5.5, //
                5.5, 8.5, 4.0, 4.5, 7.5, 4.0, //
                5.5, 9.0, 4.5, 5.0, 8.0, 4.0, //
                5.5, 8.5, 3.5, 7.0, 11.0, 4.5,
            ],
        );
        let nn = real_matrix(
            6,
            6,
            &[
                9.0, 13.0, 6.5, 6.5, 9.5, 5.0, //
                5.5, 9.0, 4.0, 7.5, 10.0, 5.0, //
                7.5, 12.0, 6.0, 7.5, 12.0, 5.5, //
                5.5, 8.5, 6.0, 5.0, 7.5, 4.0, //
                9.5, 3.0, 4.5, 3.0, 2.5, 4.0, //
                3.0, 4.5, 3.5, 7.0, 11.0, 5.0,
            ],
        );
        let got = entries(genvals(&[Value::Matrix(m), Value::Matrix(nn)]).unwrap());
        assert_close(
            &got,
            &[
                (285.568, 0.0),
                (1.166, 0.0),
                (0.859, 0.0),
                (0.029, 0.0),
                (-0.096, 0.0),
                (-0.232, 0.0),
            ],
            2e-3,
        );
    }

    #[test]
    fn right_eigenvectors_match_the_help_example() {
        // Help: R := eigenvecs(A, "R") = [0.894 -0.316; 0.447 0.949].
        let a = real_matrix(2, 2, &[3.0, 2.0, 3.0, -2.0]);
        let got = entries(eigenvecs(&[Value::Matrix(a)]).unwrap());
        assert_close(
            &got,
            &[(0.894, 0.0), (-0.316, 0.0), (0.447, 0.0), (0.949, 0.0)],
            1e-3,
        );
    }

    #[test]
    fn left_eigenvectors_match_the_help_example() {
        // Help: L := eigenvecs(A, "L") = [0.949 -0.447; 0.316 0.894].
        let a = real_matrix(2, 2, &[3.0, 2.0, 3.0, -2.0]);
        let got = entries(eigenvecs(&[Value::Matrix(a), Value::Str("L".to_string())]).unwrap());
        assert_close(
            &got,
            &[(0.949, 0.0), (-0.447, 0.0), (0.316, 0.0), (0.894, 0.0)],
            1e-3,
        );
    }

    #[test]
    fn complex_eigenvectors_match_the_help_example() {
        // Help: v := eigenvecs(A) for the complex 3 x 3 example (columns in the order of eigenvals).
        let a = complex_entries(
            3,
            3,
            &[
                (0.0, 4.0),
                (-2.0, 0.0),
                (6.0, 0.0),
                (3.0, 0.0),
                (0.0, 0.0),
                (10.0, 0.0),
                (2.0, 0.0),
                (5.0, 0.0),
                (-1.0, 0.0),
            ],
        );
        let got = entries(eigenvecs(&[Value::Matrix(a)]).unwrap());
        // Row-major 3 x 3: v = [0.176+0.089i, 0.913, -0.496+0.236i; 0.819, -0.305-0.153i,
        // -0.582-0.094i; 0.54, -0.199-0.102i, 0.592].
        assert_close(
            &got,
            &[
                (0.176, 0.089),
                (0.913, 0.0),
                (-0.496, 0.236),
                (0.819, 0.0),
                (-0.305, -0.153),
                (-0.582, -0.094),
                (0.54, 0.0),
                (-0.199, -0.102),
                (0.592, 0.0),
            ],
            2e-3,
        );
    }

    #[test]
    fn eigenvectors_satisfy_the_defining_relation_for_a_general_matrix() {
        let vals = [
            0.3, -1.2, 2.5, 0.7, //
            4.1, 0.2, -0.6, 1.9, //
            -2.2, 0.8, 1.1, -0.4, //
            0.5, 3.3, -1.7, 2.0,
        ];
        let a: Vec<C> = vals.iter().map(|x| C::new(*x, 0.0)).collect();
        let m = real_matrix(4, 4, &vals);
        let w = entries(eigenvecs(&[Value::Matrix(m.clone())]).unwrap());
        let lam = eigenvalues(&a, 4, true).unwrap();
        for (j, z) in lam.iter().enumerate() {
            let v: Vec<C> = (0..4).map(|i| w[i * 4 + j]).collect();
            assert!(residual(&a, 4, *z, &v) < 1e-9, "column {j}");
            let norm: f64 = v.iter().map(|x| x.norm_sqr()).sum();
            assert!((norm - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn symmetric_matrix_has_real_eigenvalues_sorted_descending() {
        let m = real_matrix(3, 3, &[2.0, -1.0, 0.0, -1.0, 2.0, -1.0, 0.0, -1.0, 2.0]);
        let got = entries(eigenvals(&[Value::Matrix(m)]).unwrap());
        let want = 2.0 + 2.0_f64.sqrt();
        assert_close(
            &got,
            &[(want, 0.0), (2.0, 0.0), (2.0 - 2.0_f64.sqrt(), 0.0)],
            1e-12,
        );
        assert!(got.iter().all(|z| z.im == 0.0));
    }

    #[test]
    fn rotation_matrix_has_a_conjugate_pair() {
        let m = real_matrix(2, 2, &[0.0, -1.0, 1.0, 0.0]);
        let got = entries(eigenvals(&[Value::Matrix(m)]).unwrap());
        assert_close(&got, &[(0.0, 1.0), (0.0, -1.0)], 1e-12);
    }

    #[test]
    fn eigenvec_for_a_given_eigenvalue_matches_the_column() {
        let m = real_matrix(2, 2, &[3.0, 2.0, 3.0, -2.0]);
        let z = Scalar::real(4.0, Dimension::dimensionless());
        let got = entries(eigenvec(&[Value::Matrix(m), Value::Scalar(z)]).unwrap());
        assert_close(&got, &[(0.894, 0.0), (0.447, 0.0)], 1e-3);
    }

    #[test]
    fn genvecs_satisfy_the_pencil_relation() {
        let m = real_matrix(2, 2, &[3.0, 2.0, 3.0, -2.0]);
        let nn = real_matrix(2, 2, &[2.0, 0.0, 1.0, 1.0]);
        let vals =
            entries(genvals(&[Value::Matrix(m.clone()), Value::Matrix(nn.clone())]).unwrap());
        let w = entries(genvecs(&[Value::Matrix(m.clone()), Value::Matrix(nn.clone())]).unwrap());
        let a: Vec<C> = m.data().iter().map(|s| s.value).collect();
        let b: Vec<C> = nn.data().iter().map(|s| s.value).collect();
        for j in 0..2 {
            let x: Vec<C> = (0..2).map(|i| w[i * 2 + j]).collect();
            for i in 0..2 {
                let ax: C = (0..2).map(|k| a[i * 2 + k] * x[k]).sum();
                let bx: C = (0..2).map(|k| b[i * 2 + k] * x[k]).sum();
                assert!((ax - vals[j] * bx).norm() < 1e-9, "column {j} row {i}");
            }
        }
        // Left: y^H M = v y^H N, i.e. M^H y = conj(v) N^H y.
        let wl = entries(
            genvecs(&[
                Value::Matrix(m.clone()),
                Value::Matrix(nn.clone()),
                Value::Str("L".to_string()),
            ])
            .unwrap(),
        );
        for j in 0..2 {
            let y: Vec<C> = (0..2).map(|i| wl[i * 2 + j]).collect();
            for k in 0..2 {
                let lhs: C = (0..2).map(|i| y[i].conj() * a[i * 2 + k]).sum();
                let rhs: C = (0..2).map(|i| y[i].conj() * b[i * 2 + k]).sum::<C>() * vals[j];
                assert!((lhs - rhs).norm() < 1e-9, "left column {j} entry {k}");
            }
        }
    }

    #[test]
    fn geninv_of_a_tall_matrix_is_its_left_inverse() {
        // A = [1 0; 0 1; 1 1]: geninv(A) = (A^T A)^-1 A^T = 1/3 [2 -1 1; -1 2 1].
        let a = real_matrix(3, 2, &[1.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        let got = entries(geninv(&[Value::Matrix(a)]).unwrap());
        assert_close(
            &got,
            &[
                (2.0 / 3.0, 0.0),
                (-1.0 / 3.0, 0.0),
                (1.0 / 3.0, 0.0),
                (-1.0 / 3.0, 0.0),
                (2.0 / 3.0, 0.0),
                (1.0 / 3.0, 0.0),
            ],
            1e-12,
        );
    }

    #[test]
    fn geninv_of_a_square_matrix_is_the_inverse() {
        let a = real_matrix(2, 2, &[4.0, 7.0, 2.0, 6.0]);
        let got = entries(geninv(&[Value::Matrix(a)]).unwrap());
        assert_close(
            &got,
            &[(0.6, 0.0), (-0.7, 0.0), (-0.2, 0.0), (0.4, 0.0)],
            1e-12,
        );
    }

    #[test]
    fn non_square_input_is_refused() {
        let m = real_matrix(2, 3, &[1.0; 6]);
        assert_eq!(
            eigenvals(&[Value::Matrix(m.clone())]),
            Err(EvalError::NotSquare { rows: 2, cols: 3 })
        );
        assert_eq!(
            genvals(&[Value::Matrix(m.clone()), Value::Matrix(m)]),
            Err(EvalError::NotSquare { rows: 2, cols: 3 })
        );
    }

    #[test]
    fn wide_geninv_is_unsupported() {
        let a = real_matrix(2, 3, &[1.0; 6]);
        assert!(matches!(
            geninv(&[Value::Matrix(a)]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn singular_geninv_and_singular_pencil_are_refused() {
        let a = real_matrix(3, 2, &[1.0, 2.0, 2.0, 4.0, 3.0, 6.0]);
        assert_eq!(geninv(&[Value::Matrix(a)]), Err(EvalError::Singular));
        let m = real_matrix(2, 2, &[1.0, 0.0, 0.0, 1.0]);
        let zero = real_matrix(2, 2, &[0.0; 4]);
        assert_eq!(
            genvals(&[Value::Matrix(m), Value::Matrix(zero)]),
            Err(EvalError::Singular)
        );
    }

    #[test]
    fn large_eigen_problems_are_over_the_work_budget() {
        use crate::eval::work::{check_units, dense_units, Dense};
        assert!(check_units(dense_units(Dense::Eigenvalues, 1000, 1000, 0)).is_err());
        assert!(check_units(dense_units(Dense::Eigenvectors, 1000, 1000, 0)).is_err());
        assert!(check_units(dense_units(Dense::Eigenvalues, 40, 40, 0)).is_ok());
        assert!(check_units(dense_units(Dense::Eigenvectors, 40, 40, 0)).is_ok());
    }

    #[test]
    fn non_finite_entries_are_refused_without_panic() {
        let m = real_matrix(2, 2, &[1.0, f64::NAN, 0.0, 1.0]);
        assert_eq!(
            eigenvals(&[Value::Matrix(m.clone())]),
            Err(EvalError::NotFinite)
        );
        assert_eq!(eigenvecs(&[Value::Matrix(m)]), Err(EvalError::NotFinite));
        let inf = real_matrix(1, 1, &[f64::INFINITY]);
        assert_eq!(eigenvals(&[Value::Matrix(inf)]), Err(EvalError::NotFinite));
    }

    #[test]
    fn zero_and_scalar_sized_matrices_do_not_panic() {
        let zero = real_matrix(3, 3, &[0.0; 9]);
        let got = entries(eigenvals(&[Value::Matrix(zero.clone())]).unwrap());
        assert!(got.iter().all(|z| *z == C::new(0.0, 0.0)));
        let vecs = entries(eigenvecs(&[Value::Matrix(zero)]).unwrap());
        assert_eq!(vecs.len(), 9);
        let one = real_matrix(1, 1, &[5.0]);
        let got = entries(eigenvals(&[Value::Matrix(one.clone())]).unwrap());
        assert_close(&got, &[(5.0, 0.0)], 1e-15);
        let v = entries(eigenvecs(&[Value::Matrix(one)]).unwrap());
        assert_close(&v, &[(1.0, 0.0)], 1e-15);
    }

    #[test]
    fn a_defective_matrix_terminates_with_finite_output() {
        // A Jordan block: one eigenvalue twice. Inverse iteration gives one direction, finite.
        let m = real_matrix(2, 2, &[1.0, 1.0, 0.0, 1.0]);
        let got = entries(eigenvals(&[Value::Matrix(m.clone())]).unwrap());
        assert_close(&got, &[(1.0, 0.0), (1.0, 0.0)], 1e-6);
        assert!(entries(eigenvecs(&[Value::Matrix(m)]).unwrap())
            .iter()
            .all(|z| z.re.is_finite() && z.im.is_finite()));
    }

    /// The `[3 2; 3 -2]` example at scale `s`: its eigenvalues are `4 s` and `-3 s`.
    fn scaled_example(s: f64) -> Matrix {
        real_matrix(2, 2, &[3.0 * s, 2.0 * s, 3.0 * s, -2.0 * s])
    }

    #[test]
    fn extreme_scales_give_the_unit_scale_answers() {
        for s in [1e300, 1e-300] {
            let got = entries(eigenvals(&[Value::Matrix(scaled_example(s))]).unwrap());
            assert_close(&got, &[(4.0 * s, 0.0), (-3.0 * s, 0.0)], 1e-12 * s);
            let v = entries(eigenvecs(&[Value::Matrix(scaled_example(s))]).unwrap());
            assert_close(
                &v,
                &[(0.894, 0.0), (-0.316, 0.0), (0.447, 0.0), (0.949, 0.0)],
                1e-3,
            );
            let z = Scalar::real(4.0 * s, Dimension::dimensionless());
            let one =
                entries(eigenvec(&[Value::Matrix(scaled_example(s)), Value::Scalar(z)]).unwrap());
            assert_close(&one, &[(0.894, 0.0), (0.447, 0.0)], 1e-3);
        }
    }

    #[test]
    fn extreme_scale_eigenvectors_have_small_residuals() {
        // A diagonal-scaled matrix with a wide spread of entries, at 1e300 and 1e-300.
        for s in [1e300, 1e-300] {
            let vals = [
                3.0 * s,
                0.5 * s,
                0.0,
                0.5 * s,
                -2.0 * s,
                0.0,
                1.0 * s,
                0.25 * s,
                4.0 * s,
            ];
            let a: Vec<C> = vals.iter().map(|x| C::new(*x, 0.0)).collect();
            let m = real_matrix(3, 3, &vals);
            let lam = eigenvalues(&a, 3, true).unwrap();
            let w = entries(eigenvecs(&[Value::Matrix(m)]).unwrap());
            for (j, z) in lam.iter().enumerate() {
                let v: Vec<C> = (0..3).map(|i| w[i * 3 + j]).collect();
                assert!(residual(&a, 3, *z, &v) < 1e-9, "scale {s}, column {j}");
            }
        }
    }

    #[test]
    fn eigenvec_refuses_a_value_that_is_not_an_eigenvalue() {
        // 1 is not an eigenvalue of [3 2; 3 -2] (they are 4 and -3): Unsupported, not a vector.
        let m = real_matrix(2, 2, &[3.0, 2.0, 3.0, -2.0]);
        let one = Scalar::real(1.0, Dimension::dimensionless());
        assert!(matches!(
            eigenvec(&[Value::Matrix(m), Value::Scalar(one)]),
            Err(EvalError::Unsupported(_))
        ));
        // Zero matrix: 0 is its eigenvalue, nothing else is.
        let zero = real_matrix(2, 2, &[0.0; 4]);
        let nonzero = Scalar::real(1e-3, Dimension::dimensionless());
        assert!(matches!(
            eigenvec(&[Value::Matrix(zero.clone()), Value::Scalar(nonzero)]),
            Err(EvalError::Unsupported(_))
        ));
        let zero_z = Scalar::real(0.0, Dimension::dimensionless());
        assert!(eigenvec(&[Value::Matrix(zero), Value::Scalar(zero_z)]).is_ok());
    }

    #[test]
    fn geninv_of_a_tiny_or_huge_scalar_is_its_reciprocal() {
        for (a, want) in [(1e-300, 1e300), (1e300, 1e-300)] {
            let m = real_matrix(1, 1, &[a]);
            let got = entries(geninv(&[Value::Matrix(m)]).unwrap());
            assert_eq!(got.len(), 1);
            assert!(
                ((got[0].re - want) / want).abs() < 1e-12 && got[0].im == 0.0,
                "geninv({a}) = {got:?}, want {want}"
            );
        }
        // An all-zero matrix has no inverse.
        let zero = real_matrix(2, 2, &[0.0; 4]);
        assert_eq!(geninv(&[Value::Matrix(zero)]), Err(EvalError::Singular));
    }

    #[test]
    fn defective_nilpotent_eigenvectors_stay_finite() {
        // Strictly upper triangular ones: nilpotent, so every eigenvalue is 0 and the shifted solves
        // grow without bound. The eigenvectors must still be finite unit vectors.
        let n = 35;
        let vals: Vec<f64> = (0..n * n)
            .map(|k| if k % n > k / n { 1.0 } else { 0.0 })
            .collect();
        let m = real_matrix(n, n, &vals);
        let w = entries(eigenvecs(&[Value::Matrix(m)]).unwrap());
        assert_eq!(w.len(), n * n);
        for j in 0..n {
            let norm: f64 = (0..n).map(|i| w[i * n + j].norm_sqr()).sum();
            assert!(w.iter().all(|z| z.re.is_finite() && z.im.is_finite()));
            assert!((norm - 1.0).abs() < 1e-9, "column {j}: norm^2 {norm}");
        }
    }

    #[test]
    fn all_ones_40_by_40_eigenvalues_converge() {
        // One eigenvalue 40 and 39 zeros. A cap of 30 steps per eigenvalue refused this matrix.
        let n = 40;
        let m = real_matrix(n, n, &vec![1.0; n * n]);
        let got = entries(eigenvals(&[Value::Matrix(m)]).unwrap());
        assert_eq!(got.len(), n);
        assert_close(&got[..1], &[(40.0, 0.0)], 1e-9);
        for z in &got[1..] {
            assert!(z.norm() < 1e-8, "{z}");
        }
    }

    /// The 6 x 6 pair of the help's "Example: Using genvals and genvecs Functions".
    fn help_pair() -> (Matrix, Matrix) {
        let m = real_matrix(
            6,
            6,
            &[
                7.5, 12.0, 5.5, 5.5, 8.5, 4.0, //
                5.5, 8.5, 4.0, 7.5, 12.0, 5.0, //
                7.5, 12.0, 5.5, 7.5, 12.0, 5.5, //
                5.5, 8.5, 4.0, 4.5, 7.5, 4.0, //
                5.5, 9.0, 4.5, 5.0, 8.0, 4.0, //
                5.5, 8.5, 3.5, 7.0, 11.0, 4.5,
            ],
        );
        let nn = real_matrix(
            6,
            6,
            &[
                9.0, 13.0, 6.5, 6.5, 9.5, 5.0, //
                5.5, 9.0, 4.0, 7.5, 10.0, 5.0, //
                7.5, 12.0, 6.0, 7.5, 12.0, 5.5, //
                5.5, 8.5, 6.0, 5.0, 7.5, 4.0, //
                9.5, 3.0, 4.5, 3.0, 2.5, 4.0, //
                3.0, 4.5, 3.5, 7.0, 11.0, 5.0,
            ],
        );
        (m, nn)
    }

    #[test]
    fn genvecs_columns_have_largest_entry_one_as_in_the_help() {
        // Help image example_using_genvals_and_genvecs5.png: genvecs(M, N, "L") has every column's
        // largest |entry| equal to 1. The help's signs cannot be reproduced (the largest entry is
        // positive here), so the magnitudes are compared, row-major, to the printed values.
        let (m, nn) = help_pair();
        let left = entries(
            genvecs(&[
                Value::Matrix(m.clone()),
                Value::Matrix(nn.clone()),
                Value::Str("L".to_string()),
            ])
            .unwrap(),
        );
        let help: [f64; 36] = [
            1.0, 0.533, -1.0, -0.189, -0.442, 0.364, //
            -0.295, -0.164, 0.107, -0.144, -0.706, -0.319, //
            -0.854, -1.0, -0.688, 1.0, 1.0, -1.0, //
            -0.249, 0.183, 0.084, -0.261, -0.679, 0.663, //
            -0.127, -0.067, 0.119, -0.375, 0.435, -0.224, //
            0.534, -2.559e-3, 0.225, -0.331, 0.178, 0.83,
        ];
        for j in 0..6 {
            let big = (0..6).map(|i| left[i * 6 + j].norm()).fold(0.0, f64::max);
            assert!((big - 1.0).abs() < 1e-12, "column {j}: largest {big}");
            for i in 0..6 {
                let (got, want) = (left[i * 6 + j].norm(), help[i * 6 + j].abs());
                assert!(
                    (got - want).abs() < 2e-3,
                    "row {i} column {j}: {got} vs {want}"
                );
            }
        }
        // Right vectors too: every column's largest entry is 1.
        let right = entries(genvecs(&[Value::Matrix(m), Value::Matrix(nn)]).unwrap());
        for j in 0..6 {
            let big = (0..6).map(|i| right[i * 6 + j].norm()).fold(0.0, f64::max);
            assert!((big - 1.0).abs() < 1e-12, "right column {j}: largest {big}");
        }
    }
}

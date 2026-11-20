//! Singular values by one-sided Jacobi rotations (help: "Singular Value Decomposition", `svds`
//! and `svd`). The rotations orthogonalize the columns of `M`, or the rows of a wide `M`, which has
//! the same singular values. `full_svd` also accumulates the rotations into `V` and returns the
//! left vectors `U`, for the `svd` builtin (`m x n`, `m >= n`, real entries).
//!
//! Scaling: the entries are divided by the largest absolute entry before the rotations, and the
//! singular values are multiplied back after them. So `1e200 * A` and `1e-300 * A` give the
//! singular values of `A` scaled the same way, without the squares of the entries overflowing or
//! underflowing. Entries below about 1e-16 times the largest round to zero in that division, so
//! with mixed huge and tiny entries only the values at the largest scale are resolved: singular
//! values below about 1e-16 times the largest may come out as 0. `U` and `V` do not depend on the
//! scale.
//!
//! Convergence: at most `MAX_SWEEPS` sweeps. A matrix not settled by then is an error, never a
//! guessed result. Non-finite entries are refused.
//!
//! Signs (`full_svd` only): LAPACK's singular vectors are not reproduced. The convention here is
//! that the first entry of each column of `U` with modulus above `1e-12` is negative; `V`'s column
//! is flipped with it so `U diag(s) V^T` is unchanged. For the help's M (`svd` example,
//! `M = (1 2 -1; 2.3 4 4; -2 5.1 1; 0 0.8 6)`) this reproduces the image's first two `U` columns
//! and `VH` rows. The image's third `U` column and `VH` row have the opposite sign. No rule of the
//! form "the first, last, largest or summed entry of each column is negative" gives the image's
//! three signs at once, so the image's signs are not reproduced and the test pins this convention.
//!
//! Help example values: the singular values of that M are (8.657433, 5.174279, 3.035734), as the
//! image prints them.
//!
//! Zero singular values: their `U` columns are not determined by the rotations, so they are
//! completed to an orthonormal set (the unit vector with the largest residual, Gram-Schmidt twice).

use crate::eval::error::EvalError;
use crate::eval::value::Matrix;

/// Sweeps of one-sided Jacobi before giving up. Each sweep visits every column pair once.
const MAX_SWEEPS: usize = 60;

/// Singular values of a real matrix, largest first.
pub(super) fn singular_values(m: &Matrix) -> Result<Vec<f64>, EvalError> {
    jacobi_singular_values(m, MAX_SWEEPS)
}

/// The thin SVD of a real `m x n` matrix with `m >= n`, columns as vectors: `s` descending (with
/// the matrix's scale), `u[k]` the `k`-th left vector (length `m`), `v[k]` the `k`-th right vector
/// (length `n`), so that `A = sum_k s_k u_k v_k^T`.
pub(super) struct Svd {
    pub s: Vec<f64>,
    pub u: Vec<Vec<f64>>,
    pub v: Vec<Vec<f64>>,
}

/// See the module documentation. `m` must have at least as many rows as columns (the caller
/// checks it); complex entries are refused.
pub(super) fn full_svd(m: &Matrix) -> Result<Svd, EvalError> {
    if m.data().iter().any(|s| s.value.im != 0.0) {
        return Err(EvalError::Unsupported(
            "svd of a complex matrix: not implemented yet (help: real elements)".to_string(),
        ));
    }
    if m.data().iter().any(|s| !s.value.re.is_finite()) {
        return Err(EvalError::NotFinite);
    }
    let (rows, cols) = m.shape();
    let big = m
        .data()
        .iter()
        .map(|s| s.value.re.abs())
        .fold(0.0, f64::max);
    let scale = if big == 0.0 { 1.0 } else { big };
    let mut us: Vec<Vec<f64>> = (0..cols)
        .map(|j| {
            m.data()
                .iter()
                .skip(j)
                .step_by(cols)
                .map(|s| s.value.re / scale)
                .collect()
        })
        .collect();
    let mut vs: Vec<Vec<f64>> = (0..cols)
        .map(|j| (0..cols).map(|i| f64::from(u8::from(i == j))).collect())
        .collect();
    let mut settled = false;
    for _ in 0..MAX_SWEEPS {
        if !jacobi_sweep(&mut us, Some(&mut vs)) {
            settled = true;
            break;
        }
    }
    if !settled {
        return Err(EvalError::Unsupported(format!(
            "singular values did not converge in {MAX_SWEEPS} sweeps"
        )));
    }
    let norms: Vec<f64> = us.iter().map(|c| norm(c)).collect();
    let sigma_max = norms.iter().copied().fold(0.0, f64::max);
    // Below this (relative to the largest) a column is numerically zero: its direction is noise.
    let zero_tol = cols as f64 * f64::EPSILON * sigma_max;
    let mut order: Vec<usize> = (0..cols).collect();
    order.sort_by(|&a, &b| norms[b].total_cmp(&norms[a]));

    let mut s = Vec::with_capacity(cols);
    let mut u: Vec<Option<Vec<f64>>> = Vec::with_capacity(cols);
    let mut v = Vec::with_capacity(cols);
    for &k in &order {
        let sv = norms[k];
        if sv > 0.0 && sv > zero_tol {
            u.push(Some(us[k].iter().map(|x| x / sv).collect()));
            s.push(sv * scale);
        } else {
            u.push(None);
            s.push(0.0);
        }
        v.push(vs[k].clone());
    }
    // Complete the undetermined columns of U, in order, against every column already set.
    for slot in 0..cols {
        if u[slot].is_some() {
            continue;
        }
        let accepted: Vec<&Vec<f64>> = u.iter().flatten().collect();
        let fresh = complete(&accepted, rows)?;
        u[slot] = Some(fresh);
    }
    let mut u: Vec<Vec<f64>> = u.into_iter().flatten().collect();
    for (uk, vk) in u.iter_mut().zip(v.iter_mut()) {
        let lead = uk.iter().find(|x| x.abs() > 1e-12).copied().unwrap_or(0.0);
        if lead > 0.0 {
            uk.iter_mut().for_each(|x| *x = -*x);
            vk.iter_mut().for_each(|x| *x = -*x);
        }
    }
    Ok(Svd { s, u, v })
}

/// The unit vector orthogonal to `accepted` with the largest residual (so the choice is
/// deterministic and well conditioned). Two Gram-Schmidt passes against `accepted`.
fn complete(accepted: &[&Vec<f64>], rows: usize) -> Result<Vec<f64>, EvalError> {
    let mut best: Option<(f64, Vec<f64>)> = None;
    for i in 0..rows {
        let mut w = vec![0.0; rows];
        w[i] = 1.0;
        for _ in 0..2 {
            for a in accepted {
                let d: f64 = a.iter().zip(&w).map(|(x, y)| x * y).sum();
                for (wj, aj) in w.iter_mut().zip(a.iter()) {
                    *wj -= d * aj;
                }
            }
        }
        let r = norm(&w);
        let better = match &best {
            None => true,
            Some((b, _)) => r > *b,
        };
        if better {
            best = Some((r, w));
        }
    }
    match best {
        Some((r, w)) if r > 1e-8 => Ok(w.into_iter().map(|x| x / r).collect()),
        _ => Err(EvalError::Unsupported(
            "svd: could not complete the singular vectors".to_string(),
        )),
    }
}

fn norm(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum::<f64>().sqrt()
}

fn jacobi_singular_values(m: &Matrix, max_sweeps: usize) -> Result<Vec<f64>, EvalError> {
    if m.data().iter().any(|s| s.value.im != 0.0) {
        return Err(EvalError::Unsupported(
            "singular values of a complex matrix: not implemented yet".to_string(),
        ));
    }
    if m.data().iter().any(|s| !s.value.re.is_finite()) {
        return Err(EvalError::NotFinite);
    }
    let (rows, cols) = m.shape();
    let k = rows.min(cols);
    let big = m
        .data()
        .iter()
        .map(|s| s.value.re.abs())
        .fold(0.0, f64::max);
    if big == 0.0 {
        return Ok(vec![0.0; k]);
    }
    // Columns of the scaled matrix (rows of a wide one), every entry in [-1, 1].
    let mut vecs: Vec<Vec<f64>> = if rows >= cols {
        (0..cols)
            .map(|j| {
                m.data()
                    .iter()
                    .skip(j)
                    .step_by(cols)
                    .map(|s| s.value.re / big)
                    .collect()
            })
            .collect()
    } else {
        m.data()
            .chunks(cols)
            .map(|row| row.iter().map(|s| s.value.re / big).collect())
            .collect()
    };
    let mut settled = false;
    for _ in 0..max_sweeps {
        if !jacobi_sweep(&mut vecs, None) {
            settled = true;
            break;
        }
    }
    if !settled {
        return Err(EvalError::Unsupported(format!(
            "singular values did not converge in {max_sweeps} sweeps"
        )));
    }
    let mut sv: Vec<f64> = vecs.iter().map(|c| norm(c) * big).collect();
    sv.sort_by(|a, b| b.total_cmp(a));
    Ok(sv)
}

/// One sweep over all column pairs. Returns whether any pair was rotated. When `vs` is given, the
/// same rotations are applied to it (it accumulates `V`).
fn jacobi_sweep(vecs: &mut [Vec<f64>], mut vs: Option<&mut Vec<Vec<f64>>>) -> bool {
    let k = vecs.len();
    let mut rotated = false;
    for p in 0..k {
        for q in (p + 1)..k {
            let (left, right) = vecs.split_at_mut(q);
            let (Some(up), Some(uq)) = (left.get_mut(p), right.first_mut()) else {
                continue;
            };
            let alpha: f64 = up.iter().map(|x| x * x).sum();
            let beta: f64 = uq.iter().map(|x| x * x).sum();
            let gamma: f64 = up.iter().zip(uq.iter()).map(|(x, y)| x * y).sum();
            let Some((c, s)) = rotation(alpha, beta, gamma) else {
                continue;
            };
            rotate(up, uq, c, s);
            if let Some(vs) = vs.as_deref_mut() {
                let (vl, vr) = vs.split_at_mut(q);
                if let (Some(vp), Some(vq)) = (vl.get_mut(p), vr.first_mut()) {
                    rotate(vp, vq, c, s);
                }
            }
            rotated = true;
        }
    }
    rotated
}

/// The Jacobi rotation `(c, s)` that orthogonalizes two columns with squared norms `alpha`, `beta`
/// and inner product `gamma`. `None` when they already are orthogonal (to rounding).
fn rotation(alpha: f64, beta: f64, gamma: f64) -> Option<(f64, f64)> {
    // A column whose squared norm is zero or underflowed is below 1e-154 in every entry, while the
    // largest scaled entry is 1, so the column is negligible: no rotation. The cosine test takes
    // the square roots of the norms, so `alpha * beta` never underflows.
    if alpha == 0.0
        || beta == 0.0
        || gamma == 0.0
        || gamma.abs() <= f64::EPSILON * alpha.sqrt() * beta.sqrt()
    {
        return None;
    }
    let zeta = (beta - alpha) / (2.0 * gamma);
    // `hypot` keeps zeta^2 from overflowing when gamma is tiny.
    let t = zeta.signum() / (zeta.abs() + zeta.hypot(1.0));
    let c = 1.0 / (1.0 + t * t).sqrt();
    Some((c, c * t))
}

fn rotate(up: &mut [f64], uq: &mut [f64], c: f64, s: f64) {
    for (x, y) in up.iter_mut().zip(uq.iter_mut()) {
        let (a, b) = (*x, *y);
        *x = c * a - s * b;
        *y = s * a + c * b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::value::Scalar;
    use crate::units::Dimension;

    fn real_matrix(rows: usize, cols: usize, vals: &[f64]) -> Matrix {
        let data = vals
            .iter()
            .map(|x| Scalar::real(*x, Dimension::dimensionless()))
            .collect();
        Matrix::new(rows, cols, data).expect("valid test matrix")
    }

    #[test]
    fn a_zero_sweep_budget_is_reported_as_not_converged() {
        // [1 2; 3 4] needs at least one rotation, so no sweep settles it.
        let m = real_matrix(2, 2, &[1.0, 2.0, 3.0, 4.0]);
        assert!(matches!(
            jacobi_singular_values(&m, 0),
            Err(EvalError::Unsupported(msg)) if msg.contains("did not converge")
        ));
        assert!(jacobi_singular_values(&m, MAX_SWEEPS).is_ok());
    }

    #[test]
    fn non_finite_entries_are_refused_before_any_rotation() {
        let m = real_matrix(1, 2, &[f64::INFINITY, 1.0]);
        assert_eq!(singular_values(&m), Err(EvalError::NotFinite));
        let n = real_matrix(1, 1, &[f64::NAN]);
        assert_eq!(singular_values(&n), Err(EvalError::NotFinite));
        assert!(matches!(full_svd(&n), Err(EvalError::NotFinite)));
    }

    #[test]
    fn zero_matrix_has_zero_singular_values() {
        let m = real_matrix(2, 3, &[0.0; 6]);
        assert_eq!(singular_values(&m), Ok(vec![0.0, 0.0]));
    }

    fn reconstruct(svd: &Svd, rows: usize, cols: usize) -> Vec<f64> {
        let mut out = vec![0.0; rows * cols];
        for k in 0..cols {
            for i in 0..rows {
                for j in 0..cols {
                    out[i * cols + j] += svd.s[k] * svd.u[k][i] * svd.v[k][j];
                }
            }
        }
        out
    }

    fn assert_orthonormal(vs: &[Vec<f64>]) {
        for (a, x) in vs.iter().enumerate() {
            for (b, y) in vs.iter().enumerate() {
                let d: f64 = x.iter().zip(y).map(|(p, q)| p * q).sum();
                let want = if a == b { 1.0 } else { 0.0 };
                assert!((d - want).abs() < 1e-12, "({a},{b}): {d}");
            }
        }
    }

    /// The help's M of the SVD example, row-major: (1 2 -1; 2.3 4 4; -2 5.1 1; 0 0.8 6), as the
    /// image example_matrix_factorization_svd0 shows it.
    fn help_m() -> Vec<f64> {
        vec![1.0, 2.0, -1.0, 2.3, 4.0, 4.0, -2.0, 5.1, 1.0, 0.0, 0.8, 6.0]
    }

    #[test]
    fn full_svd_of_the_help_matrix_reconstructs_it() {
        // Singular values as the image prints them: 8.657433, 5.174279, 3.035734.
        let vals = help_m();
        let m = real_matrix(4, 3, &vals);
        let svd = full_svd(&m).expect("svd");
        for (g, w) in svd.s.iter().zip([
            8.657_433_488_599_099,
            5.174_278_883_842_083,
            3.035_734_412_413_042,
        ]) {
            assert!((g - w).abs() < 1e-9, "got {g}, want {w}");
        }
        assert!(svd.s.windows(2).all(|w| w[0] >= w[1]));
        let back = reconstruct(&svd, 4, 3);
        for (g, w) in back.iter().zip(vals) {
            assert!((g - w).abs() < 1e-12, "got {g}, want {w}");
        }
        assert_orthonormal(&svd.u);
        assert_orthonormal(&svd.v);
    }

    #[test]
    fn help_svd_vectors_match_the_image_up_to_the_third_column_sign() {
        // The image's U (columns) and VH (rows), printed to 3 decimals.
        let image_u: [[f64; 4]; 3] = [
            [-0.071, -0.671, -0.448, -0.586],
            [-0.377, -0.021, -0.7, 0.605],
            [0.443, 0.619, -0.552, -0.34],
        ];
        let image_vh: [[f64; 3]; 3] = [
            [-0.083, -0.645, -0.76],
            [0.188, -0.759, 0.623],
            [0.979, 0.092, -0.184],
        ];
        let svd = full_svd(&real_matrix(4, 3, &help_m())).expect("svd");
        // The first and second pairs match the image; the third pair is flipped (module note).
        let want_signs = [1.0, 1.0, -1.0];
        for (k, image_col) in image_u.iter().enumerate() {
            for (i, want) in image_col.iter().enumerate() {
                assert!(
                    (svd.u[k][i] - want_signs[k] * want).abs() < 1e-3,
                    "U col {k} row {i}: {} vs {want}",
                    svd.u[k][i]
                );
            }
        }
        for (k, image_row) in image_vh.iter().enumerate() {
            for (j, want) in image_row.iter().enumerate() {
                assert!(
                    (svd.v[k][j] - want_signs[k] * want).abs() < 1e-3,
                    "VH row {k} col {j}: {} vs {want}",
                    svd.v[k][j]
                );
            }
        }
    }

    #[test]
    fn sign_convention_makes_the_first_entry_of_each_u_column_negative() {
        let svd = full_svd(&real_matrix(4, 3, &help_m())).expect("svd");
        for uk in &svd.u {
            let lead = uk.iter().find(|x| x.abs() > 1e-12).copied().unwrap_or(0.0);
            assert!(lead < 0.0, "{uk:?}");
        }
    }

    #[test]
    fn corner_matrices_with_extreme_entries_converge_to_the_large_value() {
        // Before the fix, alpha * beta underflowed to 0, the threshold was 0 and zeta overflowed:
        // every sweep was a no-op and [1e200 1; 1 1e-200] was refused. The second value is 0 (or,
        // for the third case, 1 against 1e200), so it is checked against 1e-12 of the largest.
        let cases: [([f64; 4], f64); 5] = [
            ([1e200, 1.0, 1.0, 1e-200], 1e200),
            ([1e-200, 1.0, 1.0, 1e200], 1e200),
            ([1e200, 1.0, 1e-200, 1.0], 1e200),
            ([1e300, 1.0, 1.0, 1e-300], 1e300),
            ([1e-300, 1.0, 1.0, 1e300], 1e300),
        ];
        for (vals, big) in cases {
            let m = real_matrix(2, 2, &vals);
            let s = singular_values(&m).expect("converges");
            assert!((s[0] - big).abs() <= 1e-12 * big, "{vals:?}: {s:?}");
            assert!(s[1] <= 1e-12 * big, "{vals:?}: {s:?}");
            let svd = full_svd(&m).expect("svd");
            assert!((svd.s[0] - big).abs() <= 1e-12 * big, "{vals:?}");
            assert_orthonormal(&svd.u);
            assert_orthonormal(&svd.v);
        }
    }

    #[test]
    fn rank_deficient_matrix_has_zero_values_and_an_orthonormal_u() {
        // Rank 1: every column is a multiple of (1, 2, 3).
        let m = real_matrix(3, 2, &[1.0, 2.0, 2.0, 4.0, 3.0, 6.0]);
        let svd = full_svd(&m).expect("svd");
        assert!(svd.s[1].abs() < 1e-12, "{:?}", svd.s);
        assert_orthonormal(&svd.u);
        let back = reconstruct(&svd, 3, 2);
        for (g, w) in back.iter().zip([1.0, 2.0, 2.0, 4.0, 3.0, 6.0]) {
            assert!((g - w).abs() < 1e-12);
        }
    }

    #[test]
    fn zero_matrix_gives_orthonormal_u_and_v() {
        let m = real_matrix(3, 2, &[0.0; 6]);
        let svd = full_svd(&m).expect("svd");
        assert_eq!(svd.s, vec![0.0, 0.0]);
        assert_orthonormal(&svd.u);
        assert_orthonormal(&svd.v);
    }

    #[test]
    fn huge_and_tiny_scales_scale_the_values_only() {
        let base = [1.0, 2.3, -2.0, 0.0, 2.0, 4.0, 5.1, 0.8, -1.0, 4.0, 1.0, 6.0];
        let reference = full_svd(&real_matrix(4, 3, &base)).expect("base");
        for k in [300, -300] {
            let scaled: Vec<f64> = base.iter().map(|x| x * 10f64.powi(k)).collect();
            let svd = full_svd(&real_matrix(4, 3, &scaled)).expect("scaled");
            for (g, r) in svd.s.iter().zip(&reference.s) {
                let want = r * 10f64.powi(k);
                assert!(
                    (g - want).abs() <= 1e-12 * want.abs(),
                    "scale {k}: {g} vs {want}"
                );
            }
            for (gu, ru) in svd.u.iter().zip(&reference.u) {
                for (g, r) in gu.iter().zip(ru) {
                    assert!((g - r).abs() < 1e-12);
                }
            }
        }
    }
}

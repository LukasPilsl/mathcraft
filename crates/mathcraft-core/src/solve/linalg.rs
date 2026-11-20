//! Dense helpers on row-major `f64` slices: LU solve with partial pivoting, the Gram products
//! used by the damped Gauss-Newton step, and norms.
//!
//! The eval module's LU works on unit-carrying values and is private, so the solver keeps its
//! own small f64 version here. Sizes are bounded by the caller (`n <= 10_000`, `n * m` capped).

/// Solves `a x = b` by LU with partial pivoting. `a` is `n x n` row-major and is overwritten;
/// `b` is overwritten with the solution. Returns `false` when the matrix is numerically singular
/// (a pivot not above `n * eps` times the largest entry) or the solution is not finite.
pub(crate) fn lu_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> bool {
    let scale = a.iter().fold(0.0_f64, |s, v| s.max(v.abs()));
    if !scale.is_finite() || scale == 0.0 {
        return false;
    }
    let tiny = scale * f64::EPSILON * n as f64;
    for k in 0..n {
        let mut pivot_row = k;
        let mut best = a[k * n + k].abs();
        for i in (k + 1)..n {
            let v = a[i * n + k].abs();
            if v > best {
                best = v;
                pivot_row = i;
            }
        }
        if best.is_nan() || best <= tiny {
            return false;
        }
        if pivot_row != k {
            for j in 0..n {
                a.swap(k * n + j, pivot_row * n + j);
            }
            b.swap(k, pivot_row);
        }
        let pivot = a[k * n + k];
        for i in (k + 1)..n {
            let factor = a[i * n + k] / pivot;
            a[i * n + k] = factor;
            for j in (k + 1)..n {
                let upper = a[k * n + j];
                a[i * n + j] -= factor * upper;
            }
            b[i] -= factor * b[k];
        }
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in (i + 1)..n {
            s -= a[i * n + j] * b[j];
        }
        b[i] = s / a[i * n + i];
    }
    b.iter().all(|v| v.is_finite())
}

/// `J^T J` (`cols x cols`) of a `rows x cols` row-major matrix.
pub(crate) fn gram_cols(j: &[f64], cols: usize) -> Vec<f64> {
    let mut out = vec![0.0; cols * cols];
    for row in j.chunks_exact(cols) {
        for (a, &ra) in row.iter().enumerate() {
            if ra == 0.0 {
                continue;
            }
            for (b, &rb) in row.iter().enumerate() {
                out[a * cols + b] += ra * rb;
            }
        }
    }
    out
}

/// `J J^T` (`rows x rows`) of a `rows x cols` row-major matrix.
pub(crate) fn gram_rows(j: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    let mut out = vec![0.0; rows * rows];
    for a in 0..rows {
        let ra = &j[a * cols..(a + 1) * cols];
        for b in a..rows {
            let rb = &j[b * cols..(b + 1) * cols];
            let v: f64 = ra.iter().zip(rb).map(|(x, y)| x * y).sum();
            out[a * rows + b] = v;
            out[b * rows + a] = v;
        }
    }
    out
}

/// `J^T v` for a `rows x cols` row-major matrix `J` and a vector `v` of length `rows`.
pub(crate) fn mat_t_vec(j: &[f64], cols: usize, v: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; cols];
    for (row, &vr) in j.chunks_exact(cols).zip(v) {
        for (o, &jv) in out.iter_mut().zip(row) {
            *o += vr * jv;
        }
    }
    out
}

/// Squared Euclidean norm.
pub(crate) fn norm2_sq(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum()
}

/// Euclidean norm.
pub(crate) fn norm2(v: &[f64]) -> f64 {
    norm2_sq(v).sqrt()
}

/// Largest absolute entry (0 for an empty slice).
pub(crate) fn norm_inf(v: &[f64]) -> f64 {
    v.iter().fold(0.0_f64, |s, x| s.max(x.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lu_solves_3x3_with_pivoting() {
        // Needs a row swap: the first pivot is zero.
        let mut a = vec![0.0, 1.0, 2.0, 1.0, 0.0, 3.0, 4.0, -3.0, 8.0];
        // Solution (1, 2, 3): rows give 8, 10 and 22.
        let mut b = vec![8.0, 10.0, 22.0];
        assert!(lu_solve(&mut a, 3, &mut b));
        for (got, want) in b.iter().zip([1.0, 2.0, 3.0]) {
            assert!((got - want).abs() < 1e-12, "{got} vs {want}");
        }
    }

    #[test]
    fn lu_reports_singular() {
        let mut a = vec![1.0, 2.0, 2.0, 4.0];
        let mut b = vec![1.0, 2.0];
        assert!(!lu_solve(&mut a, 2, &mut b));
    }

    #[test]
    fn gram_and_transpose_products() {
        // J is 2 x 2: [[1, 2], [3, 4]].
        let j = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(gram_cols(&j, 2), vec![10.0, 14.0, 14.0, 20.0]);
        assert_eq!(gram_rows(&j, 2, 2), vec![5.0, 11.0, 11.0, 25.0]);
        assert_eq!(mat_t_vec(&j, 2, &[1.0, 1.0]), vec![4.0, 6.0]);
    }
}

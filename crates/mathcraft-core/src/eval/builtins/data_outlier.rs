//! Outlier detection and removal (Mathcad Prime help: "Outlier Detection and Removal").
//!
//! The test statistic of a point is `|v_i - mean| / stdev`, with the sample standard deviation
//! (`n - 1`), as the help's examples use.
//!
//! - `Grubbs(v, a)`: every point whose statistic exceeds the Grubbs critical value for
//!   `alpha = 1 - a`, as rows `[index, statistic, crit - statistic]`, the index counted from
//!   `ORIGIN`. The critical value is `(N-1)/sqrt(N) * sqrt(t^2 / (N - 2 + t^2))` with
//!   `t = qt(alpha / (2N), N - 2)` (the help's definition). The help's values for N = 195 are
//!   reproduced in the tests: 3.4247 at alpha 0.1, 3.8145 at 0.02.
//! - `GrubbsClassic(v, a)`: the single point with the largest statistic, as one row
//!   `[index, statistic, crit - statistic]`. The help's example reads `0.183` for alpha 0.02.
//! - `ThreeSigma(v)`: the points whose statistic exceeds 3, as rows `[index, statistic]`. When no
//!   point does, the single point with the largest statistic is returned, as the help says.
//! - `trim(v, vindex)`: removes the rows (or elements) listed in `vindex`, counted from `ORIGIN`.
//!
//! Not built: the matrix form of the detectors (the help's nested pairs of indices); they refuse a
//! matrix. `Grubbs` refuses when no point exceeds the critical value, since the help gives no
//! result for that case. The help's example data (195 heat-flow values) is not in the help text,
//! so the example's outlier rows are not reproduced; the critical values and the row rules are.

use super::data_util::{grid, real_of, real_scalar, reals, vector};
use super::distributions_cont::t_quantile;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    r.register_ctx("Grubbs", Arity::Exact(2), grubbs);
    r.register_ctx("GrubbsClassic", Arity::Exact(2), grubbs_classic);
    r.register_ctx("ThreeSigma", Arity::Exact(1), three_sigma);
    r.register_ctx("trim", Arity::Exact(2), trim);
}

/// The data of a detector: a column vector of real values.
fn data_of(v: &Value, name: &str) -> Result<Vec<f64>, EvalError> {
    let xs = vector(v, name)?;
    reals(&xs, name)
}

/// The significance argument `a` of the Grubbs tests, as `alpha = 1 - a`. `a` must lie strictly
/// between 0 and 1.
fn alpha_of(v: &Value, name: &str) -> Result<f64, EvalError> {
    let (a, dim) = real_scalar(v, name)?;
    ops::require_dimensionless(dim)?;
    if !(a > 0.0 && a < 1.0) {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a probability strictly between 0 and 1"
        )));
    }
    Ok(1.0 - a)
}

/// Mean and sample standard deviation. Fewer than three points, or a zero spread, are refused.
fn spread(v: &[f64], name: &str) -> Result<(f64, f64), EvalError> {
    let n = v.len();
    if n < 3 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs at least three points"
        )));
    }
    let mean = v.iter().sum::<f64>() / n as f64;
    let ss: f64 = v.iter().map(|x| (x - mean) * (x - mean)).sum();
    let sd = (ss / (n as f64 - 1.0)).sqrt();
    if !(sd.is_finite() && sd > 0.0) {
        return Err(EvalError::Unsupported(format!(
            "{name} needs data with a non-zero spread"
        )));
    }
    Ok((mean, sd))
}

/// The test statistic `|v_i - mean| / stdev` of every point.
fn statistics(v: &[f64], name: &str) -> Result<Vec<f64>, EvalError> {
    let (mean, sd) = spread(v, name)?;
    Ok(v.iter().map(|x| (x - mean).abs() / sd).collect())
}

/// The position of the largest statistic (the first one on a tie).
fn largest(stats: &[f64]) -> usize {
    let mut best = 0;
    for (i, &s) in stats.iter().enumerate() {
        if s > stats[best] {
            best = i;
        }
    }
    best
}

/// The Grubbs critical value for level `alpha` and `n` points (the help's definition).
fn grubbs_critical(alpha: f64, n: usize) -> Result<f64, EvalError> {
    let nf = n as f64;
    let t = t_quantile(alpha / (2.0 * nf), nf - 2.0)?;
    let t2 = t * t;
    Ok((nf - 1.0) / nf.sqrt() * (t2 / (nf - 2.0 + t2)).sqrt())
}

/// The points above the Grubbs critical value, as `(zero-based index, statistic, crit - statistic)`.
fn grubbs_hits(v: &[f64], alpha: f64) -> Result<Vec<(usize, f64, f64)>, EvalError> {
    let stats = statistics(v, "Grubbs")?;
    let crit = grubbs_critical(alpha, v.len())?;
    let mut hits = Vec::new();
    for (i, &s) in stats.iter().enumerate() {
        if s > crit {
            hits.push((i, s, crit - s));
        }
    }
    if hits.is_empty() {
        return Err(EvalError::Unsupported(
            "Grubbs: no point exceeds the critical value at this level".to_string(),
        ));
    }
    Ok(hits)
}

/// The point with the largest statistic, as `(zero-based index, statistic, crit - statistic)`.
fn grubbs_classic_hit(v: &[f64], alpha: f64) -> Result<(usize, f64, f64), EvalError> {
    let stats = statistics(v, "GrubbsClassic")?;
    let crit = grubbs_critical(alpha, v.len())?;
    let i = largest(&stats);
    Ok((i, stats[i], crit - stats[i]))
}

/// The points with statistic above 3, as `(zero-based index, statistic)`; the largest point when none.
fn three_sigma_hits(v: &[f64]) -> Result<Vec<(usize, f64)>, EvalError> {
    let stats = statistics(v, "ThreeSigma")?;
    let mut hits = Vec::new();
    for (i, &s) in stats.iter().enumerate() {
        if s > 3.0 {
            hits.push((i, s));
        }
    }
    if hits.is_empty() {
        let i = largest(&stats);
        hits.push((i, stats[i]));
    }
    Ok(hits)
}

/// Zero-based row numbers for `indices` counted from `origin`, refusing any outside `0..rows`.
fn zero_based(indices: &[Scalar], origin: i64, rows: usize) -> Result<Vec<usize>, EvalError> {
    let mut out = Vec::with_capacity(indices.len());
    for s in indices {
        let zero = real_of(s, "trim")? - origin as f64;
        if zero.fract() != 0.0 || zero < 0.0 || zero >= rows as f64 {
            return Err(EvalError::Unsupported(
                "trim: an index is outside the rows of v".to_string(),
            ));
        }
        out.push(zero as usize);
    }
    Ok(out)
}

/// The rows of a `rows x cols` row-major block that are not listed in `removed`.
fn kept_rows(
    rows: usize,
    cols: usize,
    data: &[Scalar],
    removed: &[usize],
) -> Result<Value, EvalError> {
    let mut gone = vec![false; rows];
    for &r in removed {
        gone[r] = true;
    }
    let mut out: Vec<Scalar> = Vec::with_capacity(data.len());
    let mut kept = 0;
    for (r, &drop) in gone.iter().enumerate() {
        if !drop {
            out.extend_from_slice(&data[r * cols..(r + 1) * cols]);
            kept += 1;
        }
    }
    if kept == 0 {
        return Err(EvalError::Unsupported(
            "trim would remove every row".to_string(),
        ));
    }
    Ok(Value::Matrix(Matrix::new(kept, cols, out)?))
}

/// A matrix of dimensionless rows, each of `cols` values.
fn table(rows: &[Vec<f64>], cols: usize) -> Result<Value, EvalError> {
    let data: Vec<Scalar> = rows
        .iter()
        .flat_map(|r| {
            r.iter()
                .map(|&x| Scalar::real(x, Dimension::dimensionless()))
        })
        .collect();
    Ok(Value::Matrix(Matrix::new(rows.len(), cols, data)?))
}

/// The index shown to the user: the zero-based position plus `ORIGIN`.
fn shown_index(zero: usize, origin: i64) -> Result<f64, EvalError> {
    let i = (zero as i64)
        .checked_add(origin)
        .ok_or(EvalError::Overflow)?;
    Ok(i as f64)
}

/// `Grubbs(v, a)`.
fn grubbs(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let v = data_of(&args[0], "Grubbs")?;
    let alpha = alpha_of(&args[1], "Grubbs")?;
    let origin = ctx.origin()?;
    let mut rows = Vec::new();
    for (i, s, d) in grubbs_hits(&v, alpha)? {
        rows.push(vec![shown_index(i, origin)?, s, d]);
    }
    table(&rows, 3)
}

/// `GrubbsClassic(v, a)`.
fn grubbs_classic(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let v = data_of(&args[0], "GrubbsClassic")?;
    let alpha = alpha_of(&args[1], "GrubbsClassic")?;
    let origin = ctx.origin()?;
    let (i, s, d) = grubbs_classic_hit(&v, alpha)?;
    table(&[vec![shown_index(i, origin)?, s, d]], 3)
}

/// `ThreeSigma(v)`.
fn three_sigma(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let v = data_of(&args[0], "ThreeSigma")?;
    let origin = ctx.origin()?;
    let mut rows = Vec::new();
    for (i, s) in three_sigma_hits(&v)? {
        rows.push(vec![shown_index(i, origin)?, s]);
    }
    table(&rows, 2)
}

/// `trim(v, vindex)`: the rows of `v` (a vector or a matrix) not listed in `vindex`.
fn trim(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let origin = ctx.origin()?;
    let (rows, cols, data) = grid(&args[0], "trim")?;
    let indices = vector(&args[1], "trim")?;
    let removed = zero_based(&indices, origin, rows)?;
    kept_rows(rows, cols, &data, &removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 19 zeros and one 10 (N = 20): mean 0.5, sample variance 5. The 10 has statistic 9.5 / sqrt(5).
    fn spiked() -> Vec<f64> {
        let mut v = vec![0.0; 19];
        v.push(10.0);
        v
    }

    #[test]
    fn grubbs_critical_values_match_the_help() {
        // The help's example: N = 195, crit(alpha) = 3.4247 at 0.1, 3.8145 at 0.02 (its text
        // quotes 3.815), and the two values read from its GrubbsClassic and Grubbs rows.
        for (alpha, want) in [(0.1, 3.4247), (0.15, 3.3191), (0.2, 3.2422), (0.02, 3.8145)] {
            let got = grubbs_critical(alpha, 195).expect("crit");
            assert!((got - want).abs() < 1e-3, "alpha {alpha}: {got} vs {want}");
        }
    }

    #[test]
    fn grubbs_flags_the_point_above_the_critical_value() {
        let v = spiked();
        let hits = grubbs_hits(&v, 0.1).expect("hits");
        assert_eq!(hits.len(), 1);
        let (i, s, d) = hits[0];
        assert_eq!(i, 19);
        assert!((s - 9.5 / 5.0_f64.sqrt()).abs() < 1e-12);
        let crit = grubbs_critical(0.1, 20).expect("crit");
        assert!((d - (crit - s)).abs() < 1e-12);
        assert!(s > crit);
    }

    #[test]
    fn grubbs_refuses_when_nothing_is_above_the_critical_value() {
        let v: Vec<f64> = (0..20).map(|k| k as f64).collect();
        assert!(grubbs_hits(&v, 0.1).is_err());
    }

    #[test]
    fn grubbs_classic_takes_the_largest_point() {
        let v = spiked();
        let (i, s, d) = grubbs_classic_hit(&v, 0.1).expect("classic");
        assert_eq!(i, 19);
        assert!((s - 9.5 / 5.0_f64.sqrt()).abs() < 1e-12);
        let crit = grubbs_critical(0.1, 20).expect("crit");
        assert!((d - (crit - s)).abs() < 1e-12);
    }

    #[test]
    fn three_sigma_returns_the_outliers_or_the_closest_point() {
        let hits = three_sigma_hits(&spiked()).expect("three sigma");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 19);
        assert!((hits[0].1 - 9.5 / 5.0_f64.sqrt()).abs() < 1e-12);

        // Nine zeros and a 10: statistic 9 / sqrt(10) = 2.846, below 3, so the largest point is returned.
        let mut v = vec![0.0; 9];
        v.push(10.0);
        let hits = three_sigma_hits(&v).expect("closest");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 9);
        assert!((hits[0].1 - 9.0 / 10.0_f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn detectors_refuse_degenerate_data() {
        assert!(three_sigma_hits(&[1.0, 2.0]).is_err());
        assert!(three_sigma_hits(&[4.0, 4.0, 4.0]).is_err());
    }

    #[test]
    fn zero_based_counts_from_origin_and_refuses_outside_the_rows() {
        let idx = [
            Scalar::real(2.0, Dimension::dimensionless()),
            Scalar::real(1.0, Dimension::dimensionless()),
        ];
        assert_eq!(zero_based(&idx, 1, 5).expect("origin 1"), vec![1, 0]);
        // With ORIGIN 1, index 0 is below the first row.
        let low = [Scalar::real(0.0, Dimension::dimensionless())];
        assert!(zero_based(&low, 1, 5).is_err());
        let high = [Scalar::real(5.0, Dimension::dimensionless())];
        assert!(zero_based(&high, 0, 5).is_err());
    }

    #[test]
    fn trim_removes_the_listed_rows_and_keeps_the_rest() {
        let data: Vec<Scalar> = (0..8)
            .map(|k| Scalar::real(k as f64, Dimension::dimensionless()))
            .collect();
        // Four rows of two columns; remove rows 1 and 3.
        let v = kept_rows(4, 2, &data, &[1, 3]).expect("trim");
        let Value::Matrix(m) = v else {
            panic!("trim returns a matrix");
        };
        assert_eq!((m.rows(), m.cols()), (2, 2));
        let got: Vec<f64> = m.data().iter().map(|s| s.value.re).collect();
        assert_eq!(got, vec![0.0, 1.0, 4.0, 5.0]);
        assert!(kept_rows(2, 2, &data[..4], &[0, 1]).is_err());
    }
}

//! Histograms (Mathcad Prime help: "Histogram", "Example: Histograms").
//!
//! - `hist(intvls, data)`: the count of data values in each bin, as a column.
//! - `histogram(intvls, data)`: a matrix with two columns, the bin midpoints (dimension of `data`)
//!   and the counts (the same as `hist`).
//!
//! `intvls` is either an ascending vector of at least two edges (the dimension must match `data`),
//! or a positive integer `P` for `P` bins of equal length spanning `min(data)` to `max(data)`. Bins
//! are left-closed, `[e_i, e_{i+1})`, and the last bin also includes its right edge. Values outside
//! the edges are not counted. The help's example confirms these rules (`hist(int1, V1) = [1; 0; 6; 2; 0]`).
//! `data` must be a vector. For `P` bins the data must span a range (`max > min`).

use num_complex::Complex64;

use super::data_util::{column, count, one_dim, reals, vector};
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    r.register("hist", Arity::Exact(2), hist);
    r.register("histogram", Arity::Exact(2), histogram);
}

/// Bin midpoints, counts, and the dimension of the midpoints.
struct Bins {
    mids: Vec<f64>,
    counts: Vec<f64>,
    dim: Dimension,
}

fn bins(args: &[Value], name: &str) -> Result<Bins, EvalError> {
    let data = vector(&args[1], name)?;
    let dim = one_dim(&data)?;
    let x = reals(&data, name)?;
    let edges: Vec<f64> = match &args[0] {
        Value::Scalar(s) if s.dim.is_dimensionless() => {
            let p = count(&args[0], name)?;
            let lo = x.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if hi <= lo {
                return Err(EvalError::Unsupported(format!(
                    "{name}: the data must span a range for {p} equal bins"
                )));
            }
            let mut e: Vec<f64> = (0..=p)
                .map(|i| lo + (hi - lo) * i as f64 / p as f64)
                .collect();
            e[0] = lo;
            e[p] = hi;
            e
        }
        _ => {
            let edge_values = vector(&args[0], name)?;
            let edim = one_dim(&edge_values)?;
            if edim != dim {
                return Err(EvalError::DimensionMismatch {
                    left: edim,
                    right: dim,
                });
            }
            let e = reals(&edge_values, name)?;
            if e.len() < 2 || e.windows(2).any(|w| w[1] <= w[0]) {
                return Err(EvalError::Unsupported(format!(
                    "{name}: the intervals must be ascending, with at least two edges"
                )));
            }
            e
        }
    };
    let nb = edges.len() - 1;
    let (first, last) = (edges[0], edges[nb]);
    let mut counts = vec![0.0; nb];
    for &v in &x {
        if v < first || v > last {
            continue;
        }
        // The number of edges at or below `v` is at least 1 (v >= first); the last edge, when
        // `v` equals it, falls into the last bin.
        let at_or_below = edges.partition_point(|&e| e <= v);
        let i = (at_or_below - 1).min(nb - 1);
        counts[i] += 1.0;
    }
    let mids = (0..nb).map(|i| 0.5 * (edges[i] + edges[i + 1])).collect();
    Ok(Bins { mids, counts, dim })
}

/// `hist(intvls, data)`.
fn hist(args: &[Value]) -> Result<Value, EvalError> {
    let b = bins(args, "hist")?;
    let zs: Vec<Complex64> = b.counts.iter().map(|&c| Complex64::new(c, 0.0)).collect();
    column(&zs, Dimension::dimensionless())
}

/// `histogram(intvls, data)`: the midpoints and the counts as two columns.
fn histogram(args: &[Value]) -> Result<Value, EvalError> {
    let b = bins(args, "histogram")?;
    let nb = b.counts.len();
    // Row-major: each bin is (midpoint, count). The counts are dimensionless.
    let mut data = Vec::with_capacity(2 * nb);
    for (mid, count) in b.mids.iter().zip(&b.counts) {
        data.push(Scalar::real(*mid, b.dim));
        data.push(Scalar::real(*count, Dimension::dimensionless()));
    }
    Ok(Value::Matrix(Matrix::new(nb, 2, data)?))
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;
    use super::*;

    #[test]
    fn edges_count_left_closed_bins_and_include_the_last_edge() {
        let data = col(&[1.0, 2.0, 2.0, 3.0, 4.0, 5.0]);
        let edges = col(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let h = hist(&[edges, data]).expect("hist");
        assert_all_close(&values(&h), &[1.0, 2.0, 1.0, 2.0], 0.0);
    }

    #[test]
    fn values_outside_the_edges_are_not_counted() {
        let h = hist(&[col(&[0.0, 1.0, 2.0]), col(&[-1.0, 0.5, 3.0])]).expect("hist");
        assert_all_close(&values(&h), &[1.0, 0.0], 0.0);
    }

    #[test]
    fn integer_bins_span_the_data_range() {
        let data = col(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let h = hist(&[num(2.0), data.clone()]).expect("hist");
        assert_all_close(&values(&h), &[3.0, 4.0], 0.0);
        let m = histogram(&[num(2.0), data]).expect("histogram");
        let m = values(&m);
        assert_all_close(&m, &[1.5, 3.0, 4.5, 4.0], 1e-12);
    }

    #[test]
    fn histogram_midpoints_carry_the_data_dimension() {
        let s = crate::units::Dimension::base(1);
        let data = Value::Matrix(
            crate::eval::value::Matrix::new(
                3,
                1,
                vec![
                    crate::eval::value::Scalar::real(1.0, s),
                    crate::eval::value::Scalar::real(2.0, s),
                    crate::eval::value::Scalar::real(3.0, s),
                ],
            )
            .expect("data"),
        );
        let edges = Value::Matrix(
            crate::eval::value::Matrix::new(
                2,
                1,
                vec![
                    crate::eval::value::Scalar::real(0.0, s),
                    crate::eval::value::Scalar::real(4.0, s),
                ],
            )
            .expect("edges"),
        );
        let m = match histogram(&[edges, data]).expect("histogram") {
            Value::Matrix(m) => m,
            other => panic!("{other:?}"),
        };
        assert_eq!(m.data()[0].dim, s);
        assert_eq!(m.data()[1].dim, Dimension::dimensionless());
        assert_all_close(&values(&Value::Matrix(m.clone())), &[2.0, 3.0], 1e-12);
    }

    #[test]
    fn bad_intervals_are_refused() {
        assert!(matches!(
            hist(&[col(&[1.0, 1.0, 2.0]), col(&[1.0])]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            hist(&[num(3.0), col(&[2.0, 2.0])]),
            Err(EvalError::Unsupported(_))
        ));
    }
}

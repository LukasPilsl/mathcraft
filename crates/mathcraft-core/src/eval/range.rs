//! Range operators `from .. to` and `from, second .. to` (help: range_operators.md).
//!
//! The result is a column vector (n x 1) of the values `from + k * step` for `k = 0, 1, ...`,
//! where `step` is `second - from` (1 when no second value is given, or -1 when `to` is below
//! `from`: `1..-1` is 1, 0, -1). The last value is the
//! last one not past `to`: `0, 0.5 .. pi` stops at 3. A tolerance of [`END_TOLERANCE`] steps
//! keeps an end that lies on a step (`0 .. 0.3` with step 0.1) inside the range despite rounding.
//!
//! Rules (help and checked here): endpoints are real; `from`, `second` and `to` share a dimension,
//! and every element carries it; without a second value the endpoints must be dimensionless;
//! a zero step and an empty range (an explicit step that moves away from `to`) are errors. The element count
//! is computed and capped at [`MAX_RANGE_ELEMENTS`] before anything is allocated, so `0 .. 1e12`
//! and `0, 1e-300 .. 1` return `TooLarge` instead of hanging.

use super::error::EvalError;
use super::expr_eval::{sub, Ctx};
use super::value::{Matrix, Scalar, Value};
use super::work::{Budget, ELEMENT_UNITS};
use crate::expr::Expr;
use crate::units::Dimension;

/// Largest number of elements a range may have. Each element is a `Scalar` (32 bytes), so the
/// cap bounds a range at about 32 MB.
pub const MAX_RANGE_ELEMENTS: usize = 1_000_000;

/// Relative tolerance on the count: a last value within this many steps of `to` is included.
pub const END_TOLERANCE: f64 = 1e-9;

/// Evaluates a range node. The endpoints are evaluated in order: `from`, then `step` (the second
/// value), then `to`.
pub(super) fn range(
    from: &Expr,
    step: Option<&Expr>,
    to: &Expr,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let a = real_scalar(sub(from, ctx)?)?;
    let second = match step {
        Some(e) => Some(real_scalar(sub(e, ctx)?)?),
        None => None,
    };
    let c = real_scalar(sub(to, ctx)?)?;
    let matrix = build(a, second, c, ctx.budget)?;
    Ok(Value::Matrix(matrix))
}

/// The column vector for endpoints `a`, optional second value `second`, and end `c`.
fn build(
    a: Scalar,
    second: Option<Scalar>,
    c: Scalar,
    budget: &Budget,
) -> Result<Matrix, EvalError> {
    if a.dim != c.dim {
        return Err(EvalError::Unsupported(
            "range endpoints must have the same units".to_string(),
        ));
    }
    let step = match second {
        Some(b) => {
            if b.dim != a.dim {
                return Err(EvalError::Unsupported(
                    "range step must have the same units as its endpoints".to_string(),
                ));
            }
            b.value.re - a.value.re
        }
        None => {
            if a.dim != Dimension::dimensionless() {
                return Err(EvalError::Unsupported(
                    "a range with units must give its second value".to_string(),
                ));
            }
            // Without a second value the step is 1, or -1 when the end is below the start:
            // `1..-1` is 1, 0, -1.
            if c.value.re < a.value.re {
                -1.0
            } else {
                1.0
            }
        }
    };
    let (start, end) = (a.value.re, c.value.re);
    if step == 0.0 {
        return Err(EvalError::Unsupported("range step is zero".to_string()));
    }
    let steps = (end - start) / step;
    if !steps.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if steps < -END_TOLERANCE {
        return Err(EvalError::Unsupported(
            "range is empty: its end is on the wrong side of its start".to_string(),
        ));
    }
    let last = (steps + END_TOLERANCE).floor();
    // Count the elements as a float first so a huge range never allocates.
    if last >= MAX_RANGE_ELEMENTS as f64 {
        return Err(EvalError::TooLarge(MAX_RANGE_ELEMENTS.saturating_add(1)));
    }
    let count = last as usize + 1;
    // Materialising the column is O(count) work, charged before it is built (one `ELEMENT_UNITS`
    // per element, the same rate as a matrix copy).
    budget.charge((count as u64).saturating_mul(ELEMENT_UNITS))?;
    let data: Vec<Scalar> = (0..count)
        .map(|k| Scalar::real(start + k as f64 * step, a.dim))
        .collect();
    Matrix::new(count, 1, data)
}

/// A range endpoint: a real scalar. Vectors and complex values are refused.
fn real_scalar(v: Value) -> Result<Scalar, EvalError> {
    match v {
        Value::Scalar(s) if s.value.im == 0.0 => Ok(s),
        Value::Scalar(_) => Err(EvalError::Unsupported(
            "range endpoints must be real".to_string(),
        )),
        _ => Err(EvalError::Unsupported(
            "range endpoints must be scalars".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The builder with a full budget of its own.
    fn build(a: Scalar, second: Option<Scalar>, c: Scalar) -> Result<Matrix, EvalError> {
        super::build(a, second, c, &Budget::new())
    }

    fn s(x: f64) -> Scalar {
        Scalar::real(x, Dimension::dimensionless())
    }

    fn column(m: &Matrix) -> Vec<f64> {
        m.data().iter().map(|e| e.value.re).collect()
    }

    #[test]
    fn default_step_is_one() {
        let m = build(s(0.0), None, s(3.0)).unwrap();
        assert_eq!(m.shape(), (4, 1));
        assert_eq!(column(&m), vec![0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn second_value_sets_the_step() {
        // Help: `j := 2, 2.1 .. 2.3` is [2; 2.1; 2.2; 2.3].
        let m = build(s(2.0), Some(s(2.1)), s(2.3)).unwrap();
        let v = column(&m);
        assert_eq!(v.len(), 4);
        assert!((v[1] - 2.1).abs() < 1e-12 && (v[3] - 2.3).abs() < 1e-12);
    }

    #[test]
    fn stops_short_of_end_not_on_a_step() {
        // Help: `0, 0.5 .. pi` is [0; 0.5; 1; 1.5; 2; 2.5; 3] (seven values, last 3).
        let m = build(s(0.0), Some(s(0.5)), s(std::f64::consts::PI)).unwrap();
        assert_eq!(m.shape(), (7, 1));
        assert_eq!(column(&m)[6], 3.0);
    }

    #[test]
    fn negative_step_and_negative_start() {
        assert_eq!(
            column(&build(s(-1.0), None, s(2.0)).unwrap()),
            vec![-1.0, 0.0, 1.0, 2.0]
        );
        assert_eq!(
            column(&build(s(2.0), Some(s(1.0)), s(0.0)).unwrap()),
            vec![2.0, 1.0, 0.0]
        );
    }

    #[test]
    fn end_on_a_step_is_included_despite_rounding() {
        // 0.1 * 3 is not exactly 0.3 in binary; the end must still be included.
        let m = build(s(0.0), Some(s(0.1)), s(0.3)).unwrap();
        assert_eq!(m.shape(), (4, 1));
    }

    #[test]
    fn units_are_carried_and_must_agree() {
        let len = Dimension::base(0);
        let a = Scalar::real(1.0, len);
        let c = Scalar::real(5.0, len);
        let b = Scalar::real(2.0, len);
        let v = build(a, Some(b), c).unwrap();
        assert_eq!(v.data()[0].dim, len);
        assert!(build(a, None, c).is_err(), "units without a second value");
        assert!(build(a, Some(s(2.0)), c).is_err(), "step without units");
    }

    #[test]
    fn zero_step_and_empty_range_are_errors() {
        assert!(build(s(0.0), Some(s(0.0)), s(1.0)).is_err());
        assert!(build(s(0.0), Some(s(-1.0)), s(3.0)).is_err());
    }

    #[test]
    fn default_step_counts_down_when_the_end_is_below_the_start() {
        assert_eq!(
            column(&build(s(1.0), None, s(-1.0)).unwrap()),
            vec![1.0, 0.0, -1.0]
        );
        assert_eq!(
            column(&build(s(3.0), None, s(0.0)).unwrap()),
            vec![3.0, 2.0, 1.0, 0.0]
        );
    }

    #[test]
    fn huge_ranges_are_refused_without_allocating() {
        assert_eq!(
            build(s(0.0), None, s(1e12)).unwrap_err(),
            EvalError::TooLarge(MAX_RANGE_ELEMENTS + 1)
        );
        assert_eq!(
            build(s(0.0), Some(s(1e-300)), s(1.0)).unwrap_err(),
            EvalError::TooLarge(MAX_RANGE_ELEMENTS + 1)
        );
    }

    #[test]
    fn non_finite_endpoint_is_refused() {
        assert_eq!(
            build(s(0.0), None, s(f64::INFINITY)).unwrap_err(),
            EvalError::NotFinite
        );
    }
}

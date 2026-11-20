//! Hypergeometric functions by their power series: `fhyper(a, b, c, x)` (Gauss 2F1) and
//! `mhyper(a, b, x)` (confluent 1F1, help's M(a, b, x)).
//!
//! - `fhyper` needs `-1 < x < 1` (help). The series is geometric there.
//! - `mhyper` is entire in `x`; large `|x|` is refused by the precision rule below.
//! - Refused, never a wrong value: `c` (for `fhyper`) or `b` (for `mhyper`) a zero or negative
//!   integer (the series has a zero denominator); a series that does not converge within
//!   `MAX_TERMS`; a sum that loses more than 8 digits to cancellation (largest term above
//!   `MAX_LOSS` times the sum, or a zero sum of nonzero terms).
//! - A terminating series (`a` or `b` a zero or negative integer) stops at its last term.
//! - Each term is charged `TERM_UNITS` to the statement's budget.

use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

use super::{Arity, BuiltinCtx, Registry};

/// Largest number of series terms; a series still going after this is refused.
const MAX_TERMS: usize = 200_000;
/// Units charged per series term (a few multiply-adds).
const TERM_UNITS: u64 = 10;
/// Largest ratio of the largest term to the sum that is accepted (8 digits lost at most).
const MAX_LOSS: f64 = 1e8;

pub fn register(r: &mut Registry) {
    r.register_ctx("fhyper", Arity::Exact(4), fhyper_fn);
    r.register_ctx("mhyper", Arity::Exact(3), mhyper_fn);
}

/// A dimensionless real scalar argument. Complex and non-scalar values are refused.
fn real_arg(v: &Value) -> Result<f64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            if s.value.im != 0.0 {
                return Err(EvalError::Unsupported(
                    "this function takes real arguments".to_string(),
                ));
            }
            if !s.value.re.is_finite() {
                return Err(EvalError::NotFinite);
            }
            Ok(s.value.re)
        }
        _ => Err(EvalError::Unsupported(
            "expected a scalar argument".to_string(),
        )),
    }
}

/// Zero or a negative integer: the parameter of a hypergeometric denominator.
fn nonpositive_integer(v: f64) -> bool {
    v <= 0.0 && v.fract() == 0.0
}

/// Sum of `1 + t_1 + t_2 + ...` with `t_{k+1} = t_k * step(k)`, the power series of a
/// hypergeometric function. Refuses a series that does not converge, and one whose sum has lost
/// more than 8 digits to cancellation.
fn series(ctx: &BuiltinCtx<'_>, step: impl Fn(f64) -> f64) -> Result<f64, EvalError> {
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    let mut largest = 1.0_f64;
    let mut converged = false;
    for k in 0..MAX_TERMS {
        ctx.charge(TERM_UNITS)?;
        let ratio = step(k as f64);
        term *= ratio;
        if !term.is_finite() {
            return Err(EvalError::Overflow);
        }
        sum += term;
        largest = largest.max(term.abs());
        // Exhausted (a terminating series, or x = 0), or below rounding and shrinking.
        if term == 0.0 || (term.abs() <= 1e-17 * sum.abs() && ratio.abs() < 1.0) {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(EvalError::Unsupported(
            "the hypergeometric series did not converge within 200000 terms".to_string(),
        ));
    }
    if sum == 0.0 || largest > MAX_LOSS * sum.abs() {
        return Err(EvalError::Unsupported(
            "cancellation in the hypergeometric series loses too many digits for a reliable value"
                .to_string(),
        ));
    }
    Ok(sum)
}

fn scalar_result(r: f64) -> Result<Value, EvalError> {
    if !r.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::real(r, Dimension::dimensionless())))
}

/// `fhyper(a, b, c, x)` = 2F1(a, b; c; x) for -1 < x < 1.
fn fhyper_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let [a, b, c, x] = args else {
        return Err(EvalError::Unsupported(
            "fhyper takes four arguments".to_string(),
        ));
    };
    let (a, b, c, x) = (real_arg(a)?, real_arg(b)?, real_arg(c)?, real_arg(x)?);
    if !(x > -1.0 && x < 1.0) {
        return Err(EvalError::Unsupported(
            "fhyper needs -1 < x < 1".to_string(),
        ));
    }
    if nonpositive_integer(c) {
        return Err(EvalError::Unsupported(
            "fhyper is undefined when c is zero or a negative integer".to_string(),
        ));
    }
    let r = series(ctx, |k| (a + k) * (b + k) / ((c + k) * (k + 1.0)) * x)?;
    scalar_result(r)
}

/// `mhyper(a, b, x)` = 1F1(a; b; x) = M(a, b, x).
fn mhyper_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let [a, b, x] = args else {
        return Err(EvalError::Unsupported(
            "mhyper takes three arguments".to_string(),
        ));
    };
    let (a, b, x) = (real_arg(a)?, real_arg(b)?, real_arg(x)?);
    if nonpositive_integer(b) {
        return Err(EvalError::Unsupported(
            "mhyper is undefined when b is zero or a negative integer".to_string(),
        ));
    }
    let r = series(ctx, |k| (a + k) / ((b + k) * (k + 1.0)) * x)?;
    scalar_result(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> BuiltinCtx<'static> {
        BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3))
    }

    fn sc(x: f64) -> Value {
        Value::Scalar(Scalar::real(x, Dimension::dimensionless()))
    }

    fn real_of(v: Result<Value, EvalError>) -> f64 {
        match v {
            Ok(Value::Scalar(s)) => s.value.re,
            other => panic!("expected a scalar, got {other:?}"),
        }
    }

    fn f(a: f64, b: f64, c: f64, x: f64) -> Result<Value, EvalError> {
        fhyper_fn(&ctx(), &[sc(a), sc(b), sc(c), sc(x)])
    }

    fn m(a: f64, b: f64, x: f64) -> Result<Value, EvalError> {
        mhyper_fn(&ctx(), &[sc(a), sc(b), sc(x)])
    }

    #[test]
    fn ln_one_plus_x_is_x_times_fhyper_1_1_2_minus_x() {
        // ln(1 + x) = x * 2F1(1, 1; 2; -x), help identity.
        let x = 0.5;
        let got = real_of(f(1.0, 1.0, 2.0, -x)) * x;
        assert!((got - (1.0 + x).ln()).abs() < 1e-14, "got {got}");
    }

    #[test]
    fn asin_is_x_times_fhyper_half_half_three_halves_x_squared() {
        // asin(x) = x * 2F1(1/2, 1/2; 3/2; x^2), help identity.
        for x in [0.1, 0.6, -0.9] {
            let got = real_of(f(0.5, 0.5, 1.5, x * x)) * x;
            assert!((got - x.asin()).abs() < 1e-13, "x {x}: got {got}");
        }
    }

    #[test]
    fn exp_is_m_times_mhyper_1_1_x() {
        // exp(x) = M(1, 1, x), help identity. Negative x has cancellation, still within 8 digits.
        for x in [1.5, -3.0, 0.0, 10.0] {
            let got = real_of(m(1.0, 1.0, x));
            assert!(
                (got - x.exp()).abs() <= 1e-13 * x.exp().max(1.0),
                "x {x}: got {got}"
            );
        }
    }

    #[test]
    fn exp_times_sinh_is_x_times_mhyper_1_2_2x() {
        // exp(x) sinh(x) = x * M(1, 2, 2x), help identity.
        let x = 0.7;
        let got = real_of(m(1.0, 2.0, 2.0 * x)) * x;
        let want = x.exp() * x.sinh();
        assert!((got - want).abs() < 1e-14, "got {got} want {want}");
    }

    #[test]
    fn fhyper_refuses_x_outside_the_unit_interval() {
        assert!(matches!(
            f(1.0, 1.0, 2.0, 1.0),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            f(1.0, 1.0, 2.0, -1.5),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn nonpositive_integer_denominators_are_refused() {
        assert!(matches!(
            f(1.0, 1.0, 0.0, 0.2),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            f(1.0, 1.0, -2.0, 0.2),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(m(1.0, 0.0, 0.2), Err(EvalError::Unsupported(_))));
    }

    #[test]
    fn terminating_series_stops_at_its_last_term() {
        // 2F1(-2, 1; 1; x) = 1 - 2x + x^2 = (1 - x)^2.
        let x = 0.3;
        let got = real_of(f(-2.0, 1.0, 1.0, x));
        assert!((got - (1.0 - x) * (1.0 - x)).abs() < 1e-15, "got {got}");
    }

    #[test]
    fn heavy_cancellation_is_refused_not_guessed() {
        // M(1, 1, -60) = e^-60 is far below the largest term e^60 / sqrt(...): digits are lost.
        assert!(matches!(m(1.0, 1.0, -60.0), Err(EvalError::Unsupported(_))));
    }
}

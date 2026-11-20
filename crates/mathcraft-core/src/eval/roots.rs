//! `sqrt(x)` and the nth-root operator `nthRoot(x, n)`.
//!
//! Help (`square_root_and_nth_root_operator`): the nth-root operator returns a real root whenever
//! one exists, so for a negative real `x` and odd `n` it is `-((-x)^(1/n))` (`nthRoot(-8, 3) =
//! -2`). For even `n` it is the principal root. The principal branch of `x^(1/n)` is a different
//! thing: `ops::pow` gives it, so `(-8)^(1/3) = 1 + 1.732i`.

use num_complex::Complex64;

use super::env::integer_of;
use super::error::EvalError;
use super::expr_eval::{sub, Ctx};
use super::ops;
use super::value::{Scalar, Value};
use crate::expr::Expr;
use crate::units::Dimension;

/// `sqrt(x)` (no degree or a placeholder degree) and `nthRoot(x, n)`.
///
/// Help, `square_root_and_nth_root_operator`: the nth-root operator returns a real root whenever
/// one exists, so for a negative real `x` and odd `n` it is `-((-x)^(1/n))` (`nthRoot(-8, 3) =
/// -2`). For even `n` it is the principal root. The principal branch of `x^(1/n)` is a different
/// thing and is what `ops::pow` gives: `(-8)^(1/3) = 1 + 1.732i`.
pub(super) fn nth_root(
    degree: Option<&Expr>,
    radicand: &Expr,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let x = sub(radicand, ctx)?;
    let k = match degree {
        Some(d) if !matches!(d, Expr::Placeholder) => degree_value(sub(d, ctx)?)?,
        _ => 2,
    };
    let exponent = Value::Scalar(Scalar::real(1.0 / k as f64, Dimension::dimensionless()));
    if k % 2 == 1 {
        ops::map_scalar(&x, |s| odd_root(s, &exponent))
    } else {
        ops::pow(&x, &exponent)
    }
}

/// The degree of a root: a positive dimensionless integer. Zero is a division by zero, as the
/// reciprocal of the degree.
fn degree_value(v: Value) -> Result<i64, EvalError> {
    let n = match v {
        Value::Scalar(s) => s,
        _ => {
            return Err(EvalError::Unsupported(
                "root degree must be a scalar".to_string(),
            ))
        }
    };
    ops::require_dimensionless(n.dim)?;
    let k = integer_of(&n)
        .ok_or_else(|| EvalError::Unsupported("root degree must be an integer".to_string()))?;
    match k {
        0 => Err(EvalError::DivisionByZero),
        k if k < 0 => Err(EvalError::Unsupported(
            "root degree must be a positive integer".to_string(),
        )),
        k => Ok(k),
    }
}

/// One element of an odd-degree nth root: a negative real has the real root `-((-x)^e)`, any
/// other value takes the principal power `x^e`.
fn odd_root(s: &Scalar, exponent: &Value) -> Result<Scalar, EvalError> {
    let z = s.value;
    if z.im == 0.0 && z.re < 0.0 {
        let magnitude = Scalar::new(Complex64::new(-z.re, 0.0), s.dim);
        let root = power_scalar(&magnitude, exponent)?;
        return Ok(Scalar::new(-root.value, root.dim));
    }
    power_scalar(s, exponent)
}

fn power_scalar(s: &Scalar, exponent: &Value) -> Result<Scalar, EvalError> {
    match ops::pow(&Value::Scalar(*s), exponent)? {
        Value::Scalar(r) => Ok(r),
        _ => Err(EvalError::Unsupported(
            "root of a scalar is not a scalar".to_string(),
        )),
    }
}

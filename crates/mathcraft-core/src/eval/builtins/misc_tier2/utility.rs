//! Expression-type and sign functions (help: "Expression Type Functions", "Sign of Number Functions").
//!
//! Built: `IsArray`, `IsScalar`, `IsString` (1 or 0 as a dimensionless real), `csgn` (the half-plane
//! of a complex number, elementwise) and `signum(z, [x])` (z/|z|, or x where z = 0; scalars and
//! matrices of one shape, a scalar broadcasts).
//!
//! `IsFunction` is a function-argument form (`func_forms`): it needs the function table.
//!
//! Not built: `hasVariables`/`getVariables` (they need the unevaluated expression, not a value),
//! `IsRange` (a range is never a value in this engine),
//! `SIUnitsOf` (its result is a unit-scaled dimension, which the value model does not return yet).

use super::super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;
use num_complex::Complex64;

pub(super) fn register(r: &mut Registry) {
    r.register("IsArray", Arity::Exact(1), is_array);
    r.register("IsScalar", Arity::Exact(1), is_scalar);
    r.register("IsString", Arity::Exact(1), is_string);
    r.register("SIUnitsOf", Arity::Exact(1), si_units_of);
    r.register("csgn", Arity::Exact(1), csgn);
    r.register("signum", Arity::Range(1, 2), signum);
}

fn flag(b: bool) -> Value {
    Value::Scalar(Scalar::real(
        if b { 1.0 } else { 0.0 },
        Dimension::dimensionless(),
    ))
}

/// `IsArray(x)`: 1 for a vector or matrix (and for a nested list), 0 otherwise.
fn is_array(args: &[Value]) -> Result<Value, EvalError> {
    Ok(flag(matches!(args[0], Value::Matrix(_) | Value::List(_))))
}

/// `IsScalar(x)`: 1 for a real or complex scalar, 0 otherwise.
fn is_scalar(args: &[Value]) -> Result<Value, EvalError> {
    Ok(flag(matches!(args[0], Value::Scalar(_))))
}

/// `IsString(x)`: 1 for a string, 0 otherwise.
fn is_string(args: &[Value]) -> Result<Value, EvalError> {
    Ok(flag(matches!(args[0], Value::Str(_))))
}

/// `SIUnitsOf(x)`: 1 carrying the dimension of the real scalar `x` in the SI base units (help
/// `example_siunitsof`: `SIUnitsOf(E) = 1 s^4 A^2 / (kg m^3)`, the value of `E` is ignored). A
/// dimensionless `x` gives 1. Scalars are stored in SI, so the result is `1` with `x`'s dimension.
fn si_units_of(args: &[Value]) -> Result<Value, EvalError> {
    match &args[0] {
        Value::Scalar(s) if s.value.im == 0.0 => Ok(Value::Scalar(Scalar::real(1.0, s.dim))),
        _ => Err(EvalError::Unsupported(
            "SIUnitsOf takes a real scalar".to_string(),
        )),
    }
}

/// The half-plane of `z`: 0 for zero, 1 when `Re z > 0` or (`Re z = 0` and `Im z > 0`), else -1.
fn csgn(args: &[Value]) -> Result<Value, EvalError> {
    ops::map_scalar(&args[0], |s| {
        let z = s.value;
        let v = if z.re == 0.0 && z.im == 0.0 {
            0.0
        } else if z.re > 0.0 || (z.re == 0.0 && z.im > 0.0) {
            1.0
        } else {
            -1.0
        };
        Ok(Scalar::real(v, Dimension::dimensionless()))
    })
}

/// `signum(z, [x])`: `z/|z|` where `z ≠ 0`, and `x` (1 when omitted) where `z = 0`.
fn signum(args: &[Value]) -> Result<Value, EvalError> {
    let x = match args.get(1) {
        Some(v) => v.clone(),
        None => Value::Scalar(Scalar::real(1.0, Dimension::dimensionless())),
    };
    // The shape is that of the non-scalar operand; a scalar operand broadcasts.
    let (rows, cols, zs, xs) = match (&args[0], &x) {
        (Value::Scalar(z), Value::Scalar(xs)) => {
            return Ok(Value::Scalar(sign_one(z, xs)));
        }
        (Value::Matrix(z), Value::Scalar(xs)) => {
            let zs = z.data().iter().map(|z| sign_one(z, xs)).collect::<Vec<_>>();
            return Ok(Value::Matrix(Matrix::new(z.rows(), z.cols(), zs)?));
        }
        (Value::Scalar(z), Value::Matrix(xm)) => {
            let out = xm
                .data()
                .iter()
                .map(|xs| sign_one(z, xs))
                .collect::<Vec<_>>();
            return Ok(Value::Matrix(Matrix::new(xm.rows(), xm.cols(), out)?));
        }
        (Value::Matrix(z), Value::Matrix(xm)) => {
            if z.shape() != xm.shape() {
                return Err(EvalError::ShapeMismatch {
                    left: z.shape(),
                    right: xm.shape(),
                });
            }
            (z.rows(), z.cols(), z.data(), xm.data())
        }
        _ => {
            return Err(EvalError::Unsupported(
                "signum takes numbers and arrays of numbers".to_string(),
            ))
        }
    };
    let out = zs
        .iter()
        .zip(xs)
        .map(|(z, xv)| sign_one(z, xv))
        .collect::<Vec<_>>();
    Ok(Value::Matrix(Matrix::new(rows, cols, out)?))
}

fn sign_one(z: &Scalar, x: &Scalar) -> Scalar {
    let n = z.value.norm();
    if n == 0.0 {
        return *x;
    }
    Scalar::new(
        Complex64::new(z.value.re / n, z.value.im / n),
        Dimension::dimensionless(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn si_units_of_keeps_the_dimension_and_drops_the_value() {
        let dim = Dimension::from_ints([-3, -1, 4, 2, 0, 0, 0]);
        let x = Value::Scalar(Scalar::real(8.854e-12, dim));
        match si_units_of(&[x]) {
            Ok(Value::Scalar(r)) => {
                assert_eq!(r.value.re, 1.0);
                assert_eq!(r.dim, dim);
            }
            other => panic!("{other:?}"),
        }
        let plain = Value::Scalar(s(5.0, 0.0));
        assert!(matches!(si_units_of(&[plain]), Ok(Value::Scalar(r)) if r.value.re == 1.0));
        assert!(si_units_of(&[Value::Str("a".into())]).is_err());
    }

    use super::*;

    fn s(re: f64, im: f64) -> Scalar {
        Scalar::new(Complex64::new(re, im), Dimension::dimensionless())
    }

    fn scalar_of(v: Result<Value, EvalError>) -> (f64, f64) {
        match v {
            Ok(Value::Scalar(s)) => (s.value.re, s.value.im),
            other => panic!("expected a scalar, got {other:?}"),
        }
    }

    #[test]
    fn csgn_follows_the_help_table() {
        let c = |re, im| scalar_of(csgn(&[Value::Scalar(s(re, im))]));
        assert_eq!(c(0.0, 0.0), (0.0, 0.0));
        assert_eq!(c(2.0, -5.0), (1.0, 0.0));
        assert_eq!(c(0.0, 3.0), (1.0, 0.0));
        assert_eq!(c(0.0, -3.0), (-1.0, 0.0));
        assert_eq!(c(-2.0, 0.0), (-1.0, 0.0));
    }

    #[test]
    fn signum_is_z_over_abs_z_and_x_at_zero() {
        let got = scalar_of(signum(&[Value::Scalar(s(3.0, 4.0))]));
        assert!((got.0 - 0.6).abs() < 1e-15 && (got.1 - 0.8).abs() < 1e-15);
        assert_eq!(scalar_of(signum(&[Value::Scalar(s(0.0, 0.0))])), (1.0, 0.0));
        let with_x = signum(&[Value::Scalar(s(0.0, 0.0)), Value::Scalar(s(7.0, 0.0))]);
        assert_eq!(scalar_of(with_x), (7.0, 0.0));
    }

    #[test]
    fn signum_is_elementwise_on_vectors() {
        let z = Value::Matrix(Matrix::new(1, 2, vec![s(-2.0, 0.0), s(0.0, 0.0)]).unwrap());
        match signum(&[z]) {
            Ok(Value::Matrix(m)) => {
                assert_eq!(m.get(0, 0).unwrap().value.re, -1.0);
                assert_eq!(m.get(0, 1).unwrap().value.re, 1.0);
            }
            other => panic!("expected a vector, got {other:?}"),
        }
    }

    #[test]
    fn type_tests_return_one_or_zero() {
        let m = Value::Matrix(Matrix::new(1, 1, vec![s(1.0, 0.0)]).unwrap());
        assert_eq!(scalar_of(is_array(std::slice::from_ref(&m))), (1.0, 0.0));
        assert_eq!(
            scalar_of(is_array(&[Value::Scalar(s(1.0, 0.0))])),
            (0.0, 0.0)
        );
        assert_eq!(
            scalar_of(is_scalar(&[Value::Scalar(s(1.0, 2.0))])),
            (1.0, 0.0)
        );
        assert_eq!(
            scalar_of(is_string(&[Value::Str("x".to_string())])),
            (1.0, 0.0)
        );
        assert_eq!(scalar_of(is_string(&[m])), (0.0, 0.0));
    }
}

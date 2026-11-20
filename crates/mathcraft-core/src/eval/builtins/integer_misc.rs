//! Integer functions and the conditional `if`: `gcd`, `lcm`, `mod`, `permut`, `combin`.
//!
//! Help: `gcd_lcm_and_mod` (gcd and lcm take real dimensionless integers, or arrays of them; `mod`
//! returns the remainder with the sign of `x`), `combinations_and_permutations` (`n` and `k` are
//! integers with `n >= k >= 0`), and `conditional_branching` (`if(cond, x, y)`).
//!
//! `if` is registered only so that its name and arity are known: the evaluator takes it as a special
//! form (see `expr_eval`), because only the branch the condition picks is evaluated.

use super::{Arity, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

/// Largest result `permut` and `combin` return (the largest finite double, rounded down).
const MAX_RESULT: f64 = 1e308;

/// Registers the integer functions and the name of `if`.
pub fn register(r: &mut Registry) {
    r.register("gcd", Arity::AtLeast(1), gcd);
    r.register("lcm", Arity::AtLeast(1), lcm);
    r.register("mod", Arity::Exact(2), modulo);
    r.register("permut", Arity::Exact(2), permut);
    r.register("combin", Arity::Exact(2), combin);
    r.register("if", Arity::Exact(3), if_is_a_special_form);
}

/// `if` is evaluated by the evaluator (see `expr_eval::call_builtin`), never through the registry.
fn if_is_a_special_form(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "if is evaluated by the evaluator".to_string(),
    ))
}

fn dimensionless_value(v: i64) -> Value {
    Value::Scalar(Scalar::real(v as f64, Dimension::dimensionless()))
}

/// Every integer of the arguments: each scalar, and each element of each matrix. Each must be real,
/// dimensionless and an integer (within the range `integer_of` accepts).
fn integers(args: &[Value], name: &str) -> Result<Vec<i64>, EvalError> {
    let mut out = Vec::new();
    for arg in args {
        let scalars: &[Scalar] = match arg {
            Value::Scalar(s) => std::slice::from_ref(s),
            Value::Matrix(m) => m.data(),
            _ => {
                return Err(EvalError::Unsupported(format!(
                    "{name} takes numbers and arrays of numbers"
                )))
            }
        };
        for s in scalars {
            ops::require_dimensionless(s.dim)?;
            let k = integer_of(s).ok_or_else(|| {
                EvalError::Unsupported(format!("{name} needs integers (of magnitude at most 9e15)"))
            })?;
            out.push(k);
        }
    }
    Ok(out)
}

/// Euclid's algorithm on magnitudes; `gcd(0, 0) = 0`.
fn gcd_pair(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// `gcd(A, B, C, ...)`: the largest integer dividing every value. The result is not negative.
fn gcd(args: &[Value]) -> Result<Value, EvalError> {
    let values = integers(args, "gcd")?;
    let g = values
        .iter()
        .fold(0u64, |acc, v| gcd_pair(acc, v.unsigned_abs()));
    Ok(dimensionless_value(g as i64))
}

/// `lcm(A, B, C, ...)`: the smallest positive integer that every value divides. A zero among the
/// values gives zero. A result beyond `i64` is an overflow.
fn lcm(args: &[Value]) -> Result<Value, EvalError> {
    let values = integers(args, "lcm")?;
    let mut acc: u64 = 1;
    for v in &values {
        let m = v.unsigned_abs();
        if acc == 0 || m == 0 {
            acc = 0;
            continue;
        }
        let step = acc / gcd_pair(acc, m);
        acc = step.checked_mul(m).ok_or(EvalError::Overflow)?;
        if acc > i64::MAX as u64 {
            return Err(EvalError::Overflow);
        }
    }
    Ok(dimensionless_value(acc as i64))
}

/// `mod(x, y)`: the remainder of `x` divided by `y`, `x - y·trunc(x/y)`, so it has the sign of `x`
/// (`mod(-7, 3) = -1`, `mod(7, -3) = 1`). Both are real scalars of one dimension, and `y ≠ 0`.
fn modulo(args: &[Value]) -> Result<Value, EvalError> {
    let [Value::Scalar(x), Value::Scalar(y)] = args else {
        return Err(EvalError::Unsupported("mod takes two scalars".to_string()));
    };
    if x.value.im != 0.0 || y.value.im != 0.0 {
        return Err(EvalError::Unsupported("mod needs real scalars".to_string()));
    }
    if x.dim != y.dim {
        return Err(EvalError::DimensionMismatch {
            left: x.dim,
            right: y.dim,
        });
    }
    if y.value.re == 0.0 {
        return Err(EvalError::DivisionByZero);
    }
    let (a, b) = (x.value.re, y.value.re);
    let r = a - b * (a / b).trunc();
    if !r.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::real(r, x.dim)))
}

/// The integers `n` and `k` of `permut(n, k)` and `combin(n, k)`, with `n >= k >= 0`.
fn subset_counts(args: &[Value], name: &str) -> Result<(u64, u64), EvalError> {
    let values = integers(args, name)?;
    let [n, k] = values.as_slice() else {
        return Err(EvalError::Unsupported(format!("{name} takes two integers")));
    };
    if *k < 0 || *n < *k {
        return Err(EvalError::Unsupported(format!(
            "{name} needs integers n >= k >= 0"
        )));
    }
    Ok((*n as u64, *k as u64))
}

/// `permut(n, k) = n! / (n - k)!`, the product of the `k` factors `n, n-1, ..`. The loop stops at the
/// first result above [`MAX_RESULT`] (the factors are at least 2 until the last one, so that is at
/// most about 1100 steps).
fn permut(args: &[Value]) -> Result<Value, EvalError> {
    let (n, k) = subset_counts(args, "permut")?;
    let mut result = 1.0f64;
    for i in 0..k {
        result *= (n - i) as f64;
        if result > MAX_RESULT {
            return Err(EvalError::Overflow);
        }
    }
    Ok(Value::Scalar(Scalar::real(
        result,
        Dimension::dimensionless(),
    )))
}

/// `combin(n, k) = n! / (k! (n - k)!)`, built multiplicatively on the smaller of `k` and `n - k`.
/// The loop stops at the first partial product above [`MAX_RESULT`]: each step multiplies by at least
/// 2 once the count passes 1, so that is at most about 1100 steps.
fn combin(args: &[Value]) -> Result<Value, EvalError> {
    let (n, k) = subset_counts(args, "combin")?;
    let k = k.min(n - k);
    let mut result = 1.0f64;
    for i in 1..=k {
        result = result * (n - k + i) as f64 / i as f64;
        if result > MAX_RESULT {
            return Err(EvalError::Overflow);
        }
    }
    Ok(Value::Scalar(Scalar::real(
        result,
        Dimension::dimensionless(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(v: f64) -> Value {
        Value::Scalar(Scalar::real(v, Dimension::dimensionless()))
    }

    fn value_of(v: Result<Value, EvalError>) -> f64 {
        match v {
            Ok(Value::Scalar(s)) => s.value.re,
            other => panic!("expected a scalar, got {other:?}"),
        }
    }

    #[test]
    fn gcd_and_lcm_of_the_help_style_values() {
        assert_eq!(value_of(gcd(&[int(945.0), int(90.0)])), 45.0);
        assert_eq!(value_of(gcd(&[int(-12.0), int(18.0)])), 6.0);
        assert_eq!(value_of(gcd(&[int(0.0), int(0.0)])), 0.0);
        assert_eq!(value_of(gcd(&[int(4.0), int(6.0), int(10.0)])), 2.0);
        assert_eq!(value_of(lcm(&[int(4.0), int(6.0)])), 12.0);
        assert_eq!(value_of(lcm(&[int(4.0), int(0.0)])), 0.0);
    }

    #[test]
    fn gcd_refuses_fractions_and_lcm_refuses_overflow() {
        assert!(matches!(
            gcd(&[int(2.5), int(4.0)]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            lcm(&[int(4e15), int(3e15 + 1.0)]),
            Err(EvalError::Overflow)
        ));
    }

    #[test]
    fn mod_takes_the_sign_of_x() {
        assert_eq!(value_of(modulo(&[int(7.0), int(3.0)])), 1.0);
        assert_eq!(value_of(modulo(&[int(-7.0), int(3.0)])), -1.0);
        assert_eq!(value_of(modulo(&[int(7.0), int(-3.0)])), 1.0);
        assert_eq!(value_of(modulo(&[int(90.0), int(945.0)])), 90.0);
        assert!(matches!(
            modulo(&[int(1.0), int(0.0)]),
            Err(EvalError::DivisionByZero)
        ));
    }

    #[test]
    fn permut_and_combin_of_the_help_values() {
        assert_eq!(value_of(permut(&[int(5.0), int(2.0)])), 20.0);
        assert_eq!(value_of(combin(&[int(5.0), int(2.0)])), 10.0);
        assert_eq!(value_of(combin(&[int(6.0), int(0.0)])), 1.0);
        assert_eq!(value_of(combin(&[int(52.0), int(5.0)])), 2598960.0);
    }

    #[test]
    fn permut_and_combin_refuse_results_above_the_double_range() {
        assert!(matches!(
            permut(&[int(1000.0), int(500.0)]),
            Err(EvalError::Overflow)
        ));
        assert!(matches!(
            combin(&[int(2000.0), int(1000.0)]),
            Err(EvalError::Overflow)
        ));
        assert!(matches!(
            combin(&[int(3.0), int(5.0)]),
            Err(EvalError::Unsupported(_))
        ));
    }
}

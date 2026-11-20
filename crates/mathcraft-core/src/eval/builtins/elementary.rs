//! Elementary functions: trigonometric, hyperbolic, exponential and logarithmic, square root,
//! absolute value, complex parts, rounding, sign, and min and max.
//!
//! Unit rules (Mathcad help, "Trigonometric, Log, and Exponential" and "Truncation and Rounding"):
//! - trig, inverse trig, hyperbolic, `exp`, `ln`, `ln0` and `log` take dimensionless arguments;
//!   an angle such as `30 °` is already dimensionless (its SI value is radians).
//! - `sqrt` halves every exponent of the dimension, exactly (`sqrt(m^2) = m`).
//! - `abs`, `re`, `im` keep the dimension; `arg` and `sign` return dimensionless values.
//! - lowercase `floor`, `ceil`, `trunc`, `round` need a dimensionless argument. The uppercase
//!   two-argument `Floor(z, y)`, `Ceil(z, y)`, `Trunc(z, y)`, `Round(z, y)` take `z` and `y` of one
//!   dimension, `y` real and non-zero, and keep the dimension of `z`.
//! - `min` and `max` need one dimension for all their values.
//! - Element-wise over matrices, except `min` and `max` with one matrix argument, which reduce it.
//!
//! Unsupported, each `EvalError::Unsupported`: `min` and `max` of complex numbers (the help
//! orders reals only), and `min` or `max` mixing a matrix with other arguments. Not implemented:
//! `csgn` and `sign` of a complex number.

use num_complex::Complex64;

use super::{Arity, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

/// Largest magnitude of a trigonometric argument; the help says Mathcad errors beyond 10^12.
const TRIG_LIMIT: f64 = 1e12;

/// Registers every function of this tier.
pub fn register(r: &mut Registry) {
    let one = Arity::Exact(1);
    r.register("sin", one, sin);
    r.register("cos", one, cos);
    r.register("tan", one, tan);
    r.register("asin", one, asin);
    r.register("acos", one, acos);
    r.register("atan", one, atan);
    r.register("sinh", one, sinh);
    r.register("cosh", one, cosh);
    r.register("tanh", one, tanh);
    r.register("exp", one, exp);
    r.register("ln", one, ln);
    r.register("log", Arity::Range(1, 2), log);
    r.register("sqrt", one, sqrt);
    r.register("abs", one, abs);
    r.register("re", one, re);
    r.register("im", one, im);
    // Mathcad's help writes the complex parts capitalized; both spellings are the same function.
    r.register("Re", one, re);
    r.register("Im", one, im);
    r.register("arg", one, arg);
    r.register("ln0", one, ln0);
    r.register("floor", one, floor);
    r.register("ceil", one, ceil);
    r.register("trunc", one, trunc);
    r.register("round", Arity::Range(1, 2), round);
    r.register("Floor", Arity::Exact(2), floor_to_multiple);
    r.register("Ceil", Arity::Exact(2), ceil_to_multiple);
    r.register("Trunc", Arity::Exact(2), trunc_to_multiple);
    r.register("Round", Arity::Exact(2), round_to_multiple);
    r.register("sign", one, sign);
    r.register("min", Arity::AtLeast(1), min);
    r.register("max", Arity::AtLeast(1), max);
}

/// The one argument of a unary builtin.
fn single(args: &[Value]) -> Result<&Value, EvalError> {
    args.first()
        .ok_or_else(|| EvalError::Unsupported("missing argument".to_string()))
}

fn dimensionless() -> Dimension {
    Dimension::dimensionless()
}

/// Rejects non-finite complex results, the same rule the scalar kernels apply.
fn finite(z: Complex64) -> Result<Complex64, EvalError> {
    if z.re.is_finite() && z.im.is_finite() {
        Ok(z)
    } else {
        Err(EvalError::NotFinite)
    }
}

/// Applies `f` to each element of a dimensionless value, giving dimensionless results.
fn transcendental_map(
    args: &[Value],
    f: fn(Complex64) -> Result<Complex64, EvalError>,
) -> Result<Value, EvalError> {
    ops::map_scalar(single(args)?, |s| {
        ops::require_dimensionless(s.dim)?;
        Ok(Scalar::new(finite(f(s.value)?)?, dimensionless()))
    })
}

/// As [`transcendental_map`], with the trigonometric argument limit.
fn trig_map(
    args: &[Value],
    f: fn(Complex64) -> Result<Complex64, EvalError>,
) -> Result<Value, EvalError> {
    ops::map_scalar(single(args)?, |s| {
        ops::require_dimensionless(s.dim)?;
        if s.value.re.abs() > TRIG_LIMIT || s.value.im.abs() > TRIG_LIMIT {
            return Err(EvalError::Unsupported(
                "trigonometric argument above 1e12".to_string(),
            ));
        }
        Ok(Scalar::new(finite(f(s.value)?)?, dimensionless()))
    })
}

/// Natural logarithm of a non-zero complex number (principal branch).
fn ln_z(z: Complex64) -> Result<Complex64, EvalError> {
    if z.re == 0.0 && z.im == 0.0 {
        return Err(EvalError::NotFinite);
    }
    Ok(z.ln())
}

/// `ln0(z)`: as `ln`, but `-1e307` at zero (help: `ln0` returns -1x10^307 at z = 0).
fn ln0_z(z: Complex64) -> Result<Complex64, EvalError> {
    if z.re == 0.0 && z.im == 0.0 {
        return Ok(Complex64::new(-1e307, 0.0));
    }
    Ok(z.ln())
}

/// Base-10 logarithm; real positive arguments use `log10` directly so `log(100) = 2` exactly.
fn log10_z(z: Complex64) -> Result<Complex64, EvalError> {
    if z.im == 0.0 && z.re > 0.0 {
        return Ok(Complex64::new(z.re.log10(), 0.0));
    }
    Ok(ln_z(z)? / std::f64::consts::LN_10)
}

fn ln0(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, ln0_z)
}

fn sin(args: &[Value]) -> Result<Value, EvalError> {
    trig_map(args, |z| Ok(z.sin()))
}

fn cos(args: &[Value]) -> Result<Value, EvalError> {
    trig_map(args, |z| Ok(z.cos()))
}

fn tan(args: &[Value]) -> Result<Value, EvalError> {
    trig_map(args, |z| Ok(complex_tan(z)))
}

/// `tan` of a complex number. A real argument uses the real `tan`, which stays finite at the
/// double nearest `pi/2` (`1.633123935319537e16`); the complex formula divides by zero there.
pub(crate) fn complex_tan(z: Complex64) -> Complex64 {
    if z.im == 0.0 {
        Complex64::new(z.re.tan(), 0.0)
    } else {
        z.tan()
    }
}

/// Open question (shareholder, check in live Prime): for a real argument above 1 this returns the
/// complex principal value; pinned by `asin_acos_branch_cut_open_question`. Do not change it yet.
fn asin(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, |z| Ok(z.asin()))
}

/// See `asin`: same open branch-cut question for real arguments above 1.
fn acos(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, |z| Ok(z.acos()))
}

fn atan(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, |z| Ok(z.atan()))
}

fn sinh(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, |z| Ok(z.sinh()))
}

fn cosh(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, |z| Ok(z.cosh()))
}

fn tanh(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, |z| Ok(z.tanh()))
}

fn exp(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, |z| Ok(z.exp()))
}

fn ln(args: &[Value]) -> Result<Value, EvalError> {
    transcendental_map(args, ln_z)
}

/// `log(z)` is base 10; `log(z, b)` is base `b`, a positive real dimensionless scalar other than 1.
fn log(args: &[Value]) -> Result<Value, EvalError> {
    let Some(base_arg) = args.get(1) else {
        return transcendental_map(args, log10_z);
    };
    let value = single(args)?;
    let base = match base_arg {
        Value::Scalar(b) => *b,
        _ => {
            return Err(EvalError::Unsupported(
                "log base must be a scalar".to_string(),
            ))
        }
    };
    ops::require_dimensionless(base.dim)?;
    let b = base.value;
    if b.im != 0.0 || b.re <= 0.0 || b.re == 1.0 {
        return Err(EvalError::Unsupported(
            "log base must be a positive real number other than 1".to_string(),
        ));
    }
    let ln_base = b.re.ln();
    ops::map_scalar(value, |s| {
        ops::require_dimensionless(s.dim)?;
        let quotient = ln_z(s.value)? / ln_base;
        Ok(Scalar::new(finite(quotient)?, dimensionless()))
    })
}

/// `sqrt(x)` is `x^(1/2)` through the power kernel, so dimensions halve exactly.
fn sqrt(args: &[Value]) -> Result<Value, EvalError> {
    let half = Value::Scalar(Scalar::real(0.5, dimensionless()));
    ops::map_scalar(single(args)?, |s| {
        scalar_of(ops::pow(&Value::Scalar(*s), &half)?)
    })
}

fn scalar_of(v: Value) -> Result<Scalar, EvalError> {
    match v {
        Value::Scalar(s) => Ok(s),
        _ => Err(EvalError::Unsupported(
            "expected a scalar result".to_string(),
        )),
    }
}

/// `abs(x)` is the operator `|x|`: magnitude of a scalar, Euclidean norm of a column vector,
/// determinant of a square matrix (see `ops::abs`).
fn abs(args: &[Value]) -> Result<Value, EvalError> {
    ops::abs(single(args)?)
}

fn re(args: &[Value]) -> Result<Value, EvalError> {
    ops::map_scalar(single(args)?, |s| {
        Ok(Scalar::new(Complex64::new(s.value.re, 0.0), s.dim))
    })
}

fn im(args: &[Value]) -> Result<Value, EvalError> {
    ops::map_scalar(single(args)?, |s| {
        Ok(Scalar::new(Complex64::new(s.value.im, 0.0), s.dim))
    })
}

fn arg(args: &[Value]) -> Result<Value, EvalError> {
    ops::map_scalar(single(args)?, |s| {
        Ok(Scalar::real(s.value.arg(), dimensionless()))
    })
}

/// Applies a real rounding rule to the real and imaginary parts separately.
fn round_parts(value: &Value, rule: impl Fn(f64) -> f64) -> Result<Value, EvalError> {
    ops::map_scalar(value, |s| {
        ops::require_dimensionless(s.dim)?;
        let z = Complex64::new(rule(s.value.re), rule(s.value.im));
        Ok(Scalar::new(finite(z)?, dimensionless()))
    })
}

fn floor(args: &[Value]) -> Result<Value, EvalError> {
    round_parts(single(args)?, f64::floor)
}

fn ceil(args: &[Value]) -> Result<Value, EvalError> {
    round_parts(single(args)?, f64::ceil)
}

/// `trunc(z)`: the integer part, dropping the fraction (`trunc(-2.6) = -2`).
fn trunc(args: &[Value]) -> Result<Value, EvalError> {
    round_parts(single(args)?, f64::trunc)
}

fn floor_to_multiple(args: &[Value]) -> Result<Value, EvalError> {
    multiple_of(args, f64::floor)
}

fn ceil_to_multiple(args: &[Value]) -> Result<Value, EvalError> {
    multiple_of(args, f64::ceil)
}

fn trunc_to_multiple(args: &[Value]) -> Result<Value, EvalError> {
    multiple_of(args, f64::trunc)
}

fn round_to_multiple(args: &[Value]) -> Result<Value, EvalError> {
    multiple_of(args, f64::round)
}

/// `Rule(z, y) = rule(z / y) * y` for real `y` (Mathcad help: `Round(z, y) = round(z/y) * y`).
/// `z` may be a matrix; `z` and `y` must have one dimension, which the result keeps. Complex `z`
/// is rounded part by part.
fn multiple_of(args: &[Value], rule: fn(f64) -> f64) -> Result<Value, EvalError> {
    let (z, y) = match args {
        [z, Value::Scalar(y)] => (z, *y),
        _ => {
            return Err(EvalError::Unsupported(
                "this rounding takes a value and a scalar step".to_string(),
            ))
        }
    };
    if y.value.im != 0.0 {
        return Err(EvalError::Unsupported(
            "the step of a rounding must be real".to_string(),
        ));
    }
    if y.value.re == 0.0 {
        return Err(EvalError::DivisionByZero);
    }
    let step = y.value.re;
    ops::map_scalar(z, |s| {
        if s.dim != y.dim {
            return Err(EvalError::DimensionMismatch {
                left: s.dim,
                right: y.dim,
            });
        }
        let re = rule(s.value.re / step) * step;
        let im = rule(s.value.im / step) * step;
        Ok(Scalar::new(finite(Complex64::new(re, im))?, s.dim))
    })
}

/// `round(z)` rounds to the nearest integer, halves away from zero. `round(z, n)` rounds to `n`
/// decimal places; a negative `n` rounds to places left of the decimal point.
fn round(args: &[Value]) -> Result<Value, EvalError> {
    let places = match args.get(1) {
        None => 0,
        Some(Value::Scalar(n)) => {
            ops::require_dimensionless(n.dim)?;
            integer_of(n).ok_or_else(|| {
                EvalError::Unsupported("round places must be an integer".to_string())
            })?
        }
        Some(_) => {
            return Err(EvalError::Unsupported(
                "round places must be a scalar".to_string(),
            ))
        }
    };
    let scale = 10f64.powi(i32::try_from(places).map_err(|_| EvalError::Overflow)?);
    round_parts(single(args)?, move |x| (x * scale).round() / scale)
}

/// `sign(x)` for real `x`: 1, 0 or -1. Complex arguments are refused (use `csgn`, not built yet).
fn sign(args: &[Value]) -> Result<Value, EvalError> {
    ops::map_scalar(single(args)?, |s| {
        if s.value.im != 0.0 {
            return Err(EvalError::Unsupported(
                "sign of a complex number".to_string(),
            ));
        }
        let x = s.value.re;
        let v = if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        };
        Ok(Scalar::real(v, dimensionless()))
    })
}

fn min(args: &[Value]) -> Result<Value, EvalError> {
    extremum(args, |candidate, best| candidate < best)
}

fn max(args: &[Value]) -> Result<Value, EvalError> {
    extremum(args, |candidate, best| candidate > best)
}

/// Picks the value that `better` prefers. One argument may be a matrix (all its elements are
/// candidates); several arguments must be scalars. All candidates share one dimension.
fn extremum(args: &[Value], better: fn(f64, f64) -> bool) -> Result<Value, EvalError> {
    let candidates: Vec<Scalar> = match args {
        [Value::Scalar(s)] => vec![*s],
        [Value::Matrix(m)] => m.data().to_vec(),
        _ => args
            .iter()
            .map(|v| match v {
                Value::Scalar(s) => Ok(*s),
                _ => Err(EvalError::Unsupported(
                    "min and max of several arguments take scalars".to_string(),
                )),
            })
            .collect::<Result<_, _>>()?,
    };
    let (first, rest) = candidates
        .split_first()
        .ok_or_else(|| EvalError::Unsupported("min and max of nothing".to_string()))?;
    let dim = first.dim;
    let mut best = *first;
    for c in rest {
        if c.dim != dim {
            return Err(EvalError::DimensionMismatch {
                left: dim,
                right: c.dim,
            });
        }
        if c.value.im != 0.0 || best.value.im != 0.0 {
            return Err(EvalError::Unsupported(
                "ordering of complex numbers".to_string(),
            ));
        }
        if better(c.value.re, best.value.re) {
            best = *c;
        }
    }
    Ok(Value::Scalar(Scalar::new(best.value, dim)))
}

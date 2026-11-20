//! Probability distribution functions with closed forms or finite sums: normal, `cnorm`, uniform,
//! exponential, binomial and Poisson. Their arguments are unitless (every distribution page says
//! so). Densities and cumulative probabilities are 0 or 1 outside the support, as the help says.
//! The helpers here are shared with the other distribution tiers (`distributions_cont`,
//! `distributions_disc`, `random`). The other distributions are registered there.

use super::math;
use crate::eval::builtins::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value, MAX_ELEMENTS};
use crate::units::Dimension;

/// Registers the distribution functions and their stubs.
pub(super) fn register(r: &mut Registry) {
    r.register("dnorm", Arity::Exact(3), dnorm);
    r.register("pnorm", Arity::Exact(3), pnorm);
    r.register("qnorm", Arity::Exact(3), qnorm);
    r.register("cnorm", Arity::Exact(1), cnorm);
    r.register("dunif", Arity::Exact(3), dunif);
    r.register("punif", Arity::Exact(3), punif);
    r.register("qunif", Arity::Exact(3), qunif);
    r.register("dexp", Arity::Exact(2), dexp);
    r.register("pexp", Arity::Exact(2), pexp);
    r.register("qexp", Arity::Exact(2), qexp);
    r.register("dbinom", Arity::Exact(3), dbinom);
    r.register("pbinom", Arity::Exact(3), pbinom);
    r.register("qbinom", Arity::Exact(3), qbinom);
    r.register("dpois", Arity::Exact(2), dpois);
    r.register("ppois", Arity::Exact(2), ppois);
    r.register("qpois", Arity::Exact(2), qpois);
}

pub(crate) fn unitless() -> Dimension {
    Dimension::dimensionless()
}

pub(crate) fn finite(x: f64) -> Result<f64, EvalError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(EvalError::NotFinite)
    }
}

/// A real, finite, unitless number.
pub(crate) fn real_unitless(s: &Scalar) -> Result<f64, EvalError> {
    ops::require_dimensionless(s.dim)?;
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(
            "distribution arguments must be real".to_string(),
        ));
    }
    finite(s.value.re)
}

/// A scalar parameter (not a matrix), real and unitless.
pub(crate) fn param(v: &Value) -> Result<f64, EvalError> {
    match v {
        Value::Scalar(s) => real_unitless(s),
        _ => Err(EvalError::Unsupported(
            "distribution parameters must be scalars".to_string(),
        )),
    }
}

/// A count argument: an integer, unitless, real.
pub(crate) fn integer_of(s: &Scalar) -> Result<i64, EvalError> {
    let x = real_unitless(s)?;
    if x.fract() != 0.0 || x.abs() > 9.0e15 {
        return Err(EvalError::Unsupported("expected an integer".to_string()));
    }
    Ok(x as i64)
}

/// Applies `f` to each element of the variable argument (a scalar or a matrix); the results are
/// unitless and keep the argument's shape.
pub(crate) fn map_real(
    v: &Value,
    f: impl Fn(f64) -> Result<f64, EvalError>,
) -> Result<Value, EvalError> {
    ops::map_scalar(v, |s| {
        let x = real_unitless(s)?;
        Ok(Scalar::real(finite(f(x)?)?, unitless()))
    })
}

/// As [`map_real`], for the integer argument of a discrete distribution.
pub(crate) fn map_integer(
    v: &Value,
    f: impl Fn(i64) -> Result<f64, EvalError>,
) -> Result<Value, EvalError> {
    ops::map_scalar(v, |s| {
        let k = integer_of(s)?;
        Ok(Scalar::real(finite(f(k)?)?, unitless()))
    })
}

pub(crate) fn three(args: &[Value]) -> Result<(&Value, &Value, &Value), EvalError> {
    match args {
        [a, b, c] => Ok((a, b, c)),
        _ => Err(EvalError::Unsupported(
            "expected three arguments".to_string(),
        )),
    }
}

pub(crate) fn two(args: &[Value]) -> Result<(&Value, &Value), EvalError> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(EvalError::Unsupported("expected two arguments".to_string())),
    }
}

pub(crate) fn probability(p: f64) -> Result<f64, EvalError> {
    if (0.0..=1.0).contains(&p) {
        Ok(p)
    } else {
        Err(EvalError::Unsupported(
            "probability must be between 0 and 1".to_string(),
        ))
    }
}

pub(crate) fn positive(x: f64, what: &str) -> Result<f64, EvalError> {
    if x > 0.0 {
        Ok(x)
    } else {
        Err(EvalError::Unsupported(format!("{what} must be positive")))
    }
}

/// Largest count the discrete sums will loop over. Beyond it the call is refused; `what` names the
/// count in the message.
pub(crate) fn cap_count(k: i64, what: &str) -> Result<u64, EvalError> {
    let limit = MAX_ELEMENTS as u64;
    let k = u64::try_from(k).unwrap_or(0);
    if k > limit {
        Err(EvalError::Unsupported(format!(
            "{what} above the supported limit of {limit:.0e}"
        )))
    } else {
        Ok(k)
    }
}

// ---- normal ----

fn dnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (x, mu, sigma) = three(args)?;
    let (mu, sigma) = (param(mu)?, positive(param(sigma)?, "sigma")?);
    map_real(x, |x| {
        let z = (x - mu) / sigma;
        Ok(math::dnorm_std(z) / sigma)
    })
}

fn pnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (x, mu, sigma) = three(args)?;
    let (mu, sigma) = (param(mu)?, positive(param(sigma)?, "sigma")?);
    map_real(x, |x| Ok(math::pnorm_std((x - mu) / sigma)))
}

fn qnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (p, mu, sigma) = three(args)?;
    let (mu, sigma) = (param(mu)?, positive(param(sigma)?, "sigma")?);
    map_real(p, |p| {
        let z = math::qnorm_std(p).ok_or_else(|| {
            EvalError::Unsupported("qnorm needs 0 < p < 1 (the limits are infinite)".to_string())
        })?;
        Ok(mu + sigma * z)
    })
}

fn cnorm(args: &[Value]) -> Result<Value, EvalError> {
    let [x] = args else {
        return Err(EvalError::Unsupported(
            "cnorm takes one argument".to_string(),
        ));
    };
    map_real(x, |x| Ok(math::pnorm_std(x)))
}

// ---- uniform ----

fn uniform_bounds(a: &Value, b: &Value) -> Result<(f64, f64), EvalError> {
    let (a, b) = (param(a)?, param(b)?);
    if a < b {
        Ok((a, b))
    } else {
        Err(EvalError::Unsupported("uniform needs a < b".to_string()))
    }
}

fn dunif(args: &[Value]) -> Result<Value, EvalError> {
    let (x, a, b) = three(args)?;
    let (a, b) = uniform_bounds(a, b)?;
    map_real(x, |x| {
        Ok(if x >= a && x <= b { 1.0 / (b - a) } else { 0.0 })
    })
}

fn punif(args: &[Value]) -> Result<Value, EvalError> {
    let (x, a, b) = three(args)?;
    let (a, b) = uniform_bounds(a, b)?;
    map_real(x, |x| {
        Ok(if x < a {
            0.0
        } else if x > b {
            1.0
        } else {
            (x - a) / (b - a)
        })
    })
}

fn qunif(args: &[Value]) -> Result<Value, EvalError> {
    let (p, a, b) = three(args)?;
    let (a, b) = uniform_bounds(a, b)?;
    map_real(p, |p| Ok(a + probability(p)? * (b - a)))
}

// ---- exponential ----

fn dexp(args: &[Value]) -> Result<Value, EvalError> {
    let (x, r) = two(args)?;
    let r = positive(param(r)?, "rate")?;
    map_real(x, |x| Ok(if x < 0.0 { 0.0 } else { r * (-r * x).exp() }))
}

fn pexp(args: &[Value]) -> Result<Value, EvalError> {
    let (x, r) = two(args)?;
    let r = positive(param(r)?, "rate")?;
    map_real(x, |x| Ok(if x < 0.0 { 0.0 } else { -(-r * x).exp_m1() }))
}

fn qexp(args: &[Value]) -> Result<Value, EvalError> {
    let (p, r) = two(args)?;
    let r = positive(param(r)?, "rate")?;
    map_real(p, |p| Ok(-(-probability(p)?).ln_1p() / r))
}

// ---- binomial and Poisson ----

/// The count `n` of a binomial (an integer, not negative, within the loop cap).
fn trials(v: &Value) -> Result<u64, EvalError> {
    match v {
        Value::Scalar(s) => {
            let n = integer_of(s)?;
            if n < 0 {
                return Err(EvalError::Unsupported("n must not be negative".to_string()));
            }
            cap_count(n, "n")
        }
        _ => Err(EvalError::Unsupported(
            "distribution parameters must be scalars".to_string(),
        )),
    }
}

fn dbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (k, n, q) = three(args)?;
    let n = trials(n)?;
    let q = probability(param(q)?)?;
    map_integer(k, |k| {
        if k < 0 || k as u64 > n {
            return Ok(0.0);
        }
        Ok(math::dbinom(k as u64, n, q))
    })
}

fn pbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (k, n, q) = three(args)?;
    let n = trials(n)?;
    let q = probability(param(q)?)?;
    map_integer(k, |k| {
        if k < 0 {
            return Ok(0.0);
        }
        if k as u64 >= n {
            return Ok(1.0);
        }
        Ok(math::pbinom(k as u64, n, q))
    })
}

fn qbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (p, n, q) = three(args)?;
    let n = trials(n)?;
    let q = probability(param(q)?)?;
    map_real(p, |p| {
        let p = probability(p)?;
        Ok(math::qbinom(p, n, q) as f64)
    })
}

/// The rate `lambda` of a Poisson distribution: positive and at most `POISSON_LAMBDA_MAX`.
fn lambda_of(v: &Value) -> Result<f64, EvalError> {
    let lambda = positive(param(v)?, "lambda")?;
    if lambda > math::POISSON_LAMBDA_MAX {
        return Err(EvalError::Unsupported(format!(
            "poisson lambda above {:e} is not supported",
            math::POISSON_LAMBDA_MAX
        )));
    }
    Ok(lambda)
}

fn dpois(args: &[Value]) -> Result<Value, EvalError> {
    let (k, lambda) = two(args)?;
    let lambda = lambda_of(lambda)?;
    map_integer(k, |k| {
        if k < 0 {
            return Ok(0.0);
        }
        Ok(math::dpois(cap_count(k, "k")?, lambda))
    })
}

fn ppois(args: &[Value]) -> Result<Value, EvalError> {
    let (k, lambda) = two(args)?;
    let lambda = lambda_of(lambda)?;
    map_integer(k, |k| {
        if k < 0 {
            return Ok(0.0);
        }
        Ok(math::ppois(cap_count(k, "k")?, lambda))
    })
}

fn qpois(args: &[Value]) -> Result<Value, EvalError> {
    let (p, lambda) = two(args)?;
    let lambda = lambda_of(lambda)?;
    map_real(p, |p| {
        let p = probability(p)?;
        if p >= 1.0 {
            return Err(EvalError::Unsupported(
                "qpois needs p < 1 (the limit is infinite)".to_string(),
            ));
        }
        Ok(math::qpois(p, lambda) as f64)
    })
}

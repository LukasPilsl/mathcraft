//! Continuous distributions built on the incomplete gamma and beta functions: gamma, beta, Student
//! t, chi-squared and F, each with density (`d`), cumulative probability (`p`) and inverse (`q`);
//! and the closed-form Cauchy, Weibull, logistic and log normal families.
//!
//! Densities are evaluated in log form, so the gamma and beta functions never overflow for large
//! shape parameters. Cumulative probabilities use [`special_gamma::gamma_p`] and
//! [`special_gamma::ibeta`]. Inverses bracket the root and then bisect the monotone cumulative
//! function (see [`invert_cdf`]); every loop has a fixed bound, so a hostile argument ends with a
//! refusal, never a hang. Relative accuracy of the inverses is about 1e-14 wherever the cumulative
//! function is accurate. Quantiles below `f64::MIN_POSITIVE` are reported at that floor, and the
//! t quantile refuses when its tail probability is too small for the beta inversion (see [`qt`]).
//! Log densities of large shapes use the Stirling forms in [`special_gamma`], so `d` up to `1e300`
//! and shapes up to `1e8` keep their relative accuracy.
//!
//! Arguments are unitless (see the distribution help pages). Densities and cumulative
//! probabilities are 0 or 1 outside the support, as the help says.

use std::f64::consts::PI;

use super::special::special_gamma as gam;
use super::statistics::{dist, math};
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::Value;

use dist::{finite, map_real, param, positive, probability, three, two};

/// Bracketing steps for an open-ended search (each step doubles the bracket).
const BRACKET_STEPS: usize = 1100;
/// Bisection steps. Each halves the bracket, or the log bracket, so this covers the double range.
const BISECTION_STEPS: usize = 400;

/// Registers the continuous distribution functions.
pub(super) fn register(r: &mut Registry) {
    r.register("dgamma", Arity::Exact(2), dgamma);
    r.register("pgamma", Arity::Exact(2), pgamma);
    r.register("qgamma", Arity::Exact(2), qgamma);
    r.register("dbeta", Arity::Exact(3), dbeta);
    r.register("pbeta", Arity::Exact(3), pbeta);
    r.register("qbeta", Arity::Exact(3), qbeta);
    r.register("dt", Arity::Exact(2), dt);
    r.register("pt", Arity::Exact(2), pt);
    r.register("qt", Arity::Exact(2), qt);
    r.register("dchisq", Arity::Exact(2), dchisq);
    r.register("pchisq", Arity::Exact(2), pchisq);
    r.register("qchisq", Arity::Exact(2), qchisq);
    r.register("dF", Arity::Exact(3), df);
    r.register("pF", Arity::Exact(3), pf);
    r.register("qF", Arity::Exact(3), qf);
    r.register("dcauchy", Arity::Exact(3), dcauchy);
    r.register("pcauchy", Arity::Exact(3), pcauchy);
    r.register("qcauchy", Arity::Exact(3), qcauchy);
    r.register("dweibull", Arity::Exact(2), dweibull);
    r.register("pweibull", Arity::Exact(2), pweibull);
    r.register("qweibull", Arity::Exact(2), qweibull);
    r.register("dlogis", Arity::Exact(3), dlogis);
    r.register("plogis", Arity::Exact(3), plogis);
    r.register("qlogis", Arity::Exact(3), qlogis);
    r.register("dlnorm", Arity::Exact(3), dlnorm);
    r.register("plnorm", Arity::Exact(3), plnorm);
    r.register("qlnorm", Arity::Exact(3), qlnorm);
}

fn unsupported(msg: impl Into<String>) -> EvalError {
    EvalError::Unsupported(msg.into())
}

/// `exp(ln)`, refused when the density is infinite (a singularity at the boundary of the support).
fn exp_checked(ln: f64) -> Result<f64, EvalError> {
    let v = ln.exp();
    if v.is_finite() {
        Ok(v)
    } else {
        Err(EvalError::NotFinite)
    }
}

/// `e * ln(x)`, with the term zero when `e` is zero (so `0 * ln 0` is not NaN).
fn xlogy(e: f64, x: f64) -> f64 {
    if e == 0.0 {
        0.0
    } else {
        e * x.ln()
    }
}

/// A probability from an incomplete function: rounding can push it a few ulps outside `[0, 1]`
/// (and a non-finite value is a refusal, never a number).
fn unit_probability(v: f64) -> Result<f64, EvalError> {
    if v.is_finite() {
        Ok(v.clamp(0.0, 1.0))
    } else {
        Err(EvalError::NotFinite)
    }
}

/// The gamma density with shape `s` (unit scale) at `x`; zero for `x < 0`.
fn gamma_density(x: f64, s: f64) -> Result<f64, EvalError> {
    if x < 0.0 {
        return Ok(0.0);
    }
    exp_checked(gam::gamma_ln_density(x, s)?)
}

/// The `x` in `[0, upper]` (or `[0, inf)` when `upper` is `None`) with `cdf(x) = p`.
///
/// `cdf` is non-decreasing with `cdf(0) = 0`. The bracket is found by doubling (open support) or
/// is `[0, upper]`. Then the interval is bisected: geometrically once its ratio exceeds 4, so tiny
/// quantiles keep their relative accuracy, and arithmetically otherwise. The loop stops when the
/// bracket is within a few ulps of its right end, or after [`BISECTION_STEPS`].
fn invert_cdf(
    p: f64,
    upper: Option<f64>,
    cdf: impl Fn(f64) -> Result<f64, EvalError>,
) -> Result<f64, EvalError> {
    if p == 0.0 {
        return Ok(0.0);
    }
    if p == 1.0 {
        return match upper {
            Some(u) => Ok(u),
            None => Err(EvalError::NotFinite),
        };
    }
    let mut hi = upper.unwrap_or(1.0);
    if upper.is_none() {
        let mut steps = 0;
        while cdf(hi)? < p {
            hi *= 2.0;
            steps += 1;
            if steps > BRACKET_STEPS || !hi.is_finite() {
                return Err(unsupported("quantile search did not reach the probability"));
            }
        }
    }
    let floor = f64::MIN_POSITIVE;
    if cdf(floor)? >= p {
        return Ok(floor);
    }
    let mut lo = floor;
    for _ in 0..BISECTION_STEPS {
        if hi - lo <= 4.0 * f64::EPSILON * hi {
            break;
        }
        // sqrt(lo) sqrt(hi), not sqrt(lo hi): the product underflows once hi is near 1e-154.
        let mid = if hi / lo > 4.0 {
            lo.sqrt() * hi.sqrt()
        } else {
            0.5 * (lo + hi)
        };
        if cdf(mid)? < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(0.5 * (lo + hi))
}

/// Quantile of the beta distribution `Beta(a, b)` on `[0, 1]`.
fn beta_quantile(p: f64, a: f64, b: f64) -> Result<f64, EvalError> {
    invert_cdf(p, Some(1.0), |x| gam::ibeta(x, a, b))
}

// ---- gamma ----

/// The shape `s` of a gamma distribution: positive.
fn shape(v: &Value) -> Result<f64, EvalError> {
    positive(param(v)?, "shape s")
}

fn dgamma(args: &[Value]) -> Result<Value, EvalError> {
    let (x, s) = two(args)?;
    let s = shape(s)?;
    map_real(x, |x| gamma_density(x, s))
}

fn pgamma(args: &[Value]) -> Result<Value, EvalError> {
    let (x, s) = two(args)?;
    let s = shape(s)?;
    map_real(x, |x| {
        if x <= 0.0 {
            Ok(0.0)
        } else {
            unit_probability(gam::gamma_p(s, x)?)
        }
    })
}

fn qgamma(args: &[Value]) -> Result<Value, EvalError> {
    let (p, s) = two(args)?;
    let s = shape(s)?;
    map_real(p, |p| {
        invert_cdf(probability(p)?, None, |x| gam::gamma_p(s, x))
    })
}

// ---- beta ----

/// The shape parameters `a` and `b` of a beta distribution: positive.
fn beta_shapes(a: &Value, b: &Value) -> Result<(f64, f64), EvalError> {
    Ok((
        positive(param(a)?, "shape s1")?,
        positive(param(b)?, "shape s2")?,
    ))
}

fn dbeta(args: &[Value]) -> Result<Value, EvalError> {
    let (x, a, b) = three(args)?;
    let (a, b) = beta_shapes(a, b)?;
    map_real(x, |x| {
        if !(0.0..=1.0).contains(&x) {
            return Ok(0.0);
        }
        exp_checked(gam::beta_ln_density(x, a, b)?)
    })
}

fn pbeta(args: &[Value]) -> Result<Value, EvalError> {
    let (x, a, b) = three(args)?;
    let (a, b) = beta_shapes(a, b)?;
    map_real(x, |x| {
        if x <= 0.0 {
            Ok(0.0)
        } else if x >= 1.0 {
            Ok(1.0)
        } else {
            unit_probability(gam::ibeta(x, a, b)?)
        }
    })
}

fn qbeta(args: &[Value]) -> Result<Value, EvalError> {
    let (p, a, b) = three(args)?;
    let (a, b) = beta_shapes(a, b)?;
    map_real(p, |p| beta_quantile(probability(p)?, a, b))
}

// ---- Student t ----

/// The degrees of freedom `d` of a t or chi-squared distribution: positive.
fn degrees(v: &Value) -> Result<f64, EvalError> {
    positive(param(v)?, "degrees of freedom d")
}

fn dt(args: &[Value]) -> Result<Value, EvalError> {
    let (x, d) = two(args)?;
    let d = degrees(d)?;
    map_real(x, |x| {
        // lnGamma((d+1)/2) - lnGamma(d/2) is a shift of 1/2 at d/2, so it is taken without
        // cancellation for huge d.
        let ln = gam::ln_gamma_shift(d / 2.0, 0.5)?
            - 0.5 * (PI * d).ln()
            - (d + 1.0) / 2.0 * (x * x / d).ln_1p();
        exp_checked(ln)
    })
}

/// `P(T <= x)` for Student's t with `d` degrees of freedom. With `w = x^2 / (d + x^2)` the
/// probability `P(|T| <= |x|)` is `I_w(1/2, d/2)`, so `P(T <= x) = 1/2 +- I_w(1/2, d/2) / 2`
/// (plus for `x >= 0`). `w` is formed as `1 / (1 + d / x^2)`, which stays accurate for huge `d`.
fn t_cdf(x: f64, d: f64) -> Result<f64, EvalError> {
    let w = if x == 0.0 {
        0.0
    } else {
        1.0 / (1.0 + d / (x * x))
    };
    let within = gam::ibeta(w, 0.5, d / 2.0)?;
    unit_probability(if x < 0.0 {
        0.5 - 0.5 * within
    } else {
        0.5 + 0.5 * within
    })
}

fn pt(args: &[Value]) -> Result<Value, EvalError> {
    let (x, d) = two(args)?;
    let d = degrees(d)?;
    map_real(x, |x| t_cdf(x, d))
}

fn qt(args: &[Value]) -> Result<Value, EvalError> {
    let (p, d) = two(args)?;
    let d = degrees(d)?;
    map_real(p, |p| t_quantile(probability(p)?, d))
}

/// The Student t quantile for a probability `p` in (0, 1) and `d` > 0 degrees of freedom. Shared
/// with the outlier tests (`data_outlier`), which need the same quantile.
pub(super) fn t_quantile(p: f64, d: f64) -> Result<f64, EvalError> {
    if p == 0.5 {
        return Ok(0.0);
    }
    // The quantile solves I_z(d/2, 1/2) = 2 min(p, 1-p) for z = d/(d+x^2). Using 1 - p (exact
    // for p >= 1/2) keeps the tail accurate on both sides.
    let (tail, sign) = if p < 0.5 {
        (2.0 * p, -1.0)
    } else {
        (2.0 * (1.0 - p), 1.0)
    };
    let z = beta_quantile(tail, d / 2.0, 0.5)?;
    if z <= 0.0 {
        return Err(EvalError::NotFinite);
    }
    // A tail below about 1e-154 gives z below the floor: the answer is not representable
    // through z, so refuse rather than return the floor's quantile.
    if z <= f64::MIN_POSITIVE {
        return Err(unsupported(
            "t quantile: probability too small for the inversion",
        ));
    }
    Ok(sign * (d * (1.0 - z) / z).sqrt())
}

// ---- chi-squared ----

fn dchisq(args: &[Value]) -> Result<Value, EvalError> {
    let (x, d) = two(args)?;
    let d = degrees(d)?;
    map_real(x, |x| Ok(0.5 * gamma_density(x / 2.0, d / 2.0)?))
}

fn pchisq(args: &[Value]) -> Result<Value, EvalError> {
    let (x, d) = two(args)?;
    let d = degrees(d)?;
    map_real(x, |x| {
        if x <= 0.0 {
            Ok(0.0)
        } else {
            unit_probability(gam::gamma_p(d / 2.0, x / 2.0)?)
        }
    })
}

fn qchisq(args: &[Value]) -> Result<Value, EvalError> {
    let (p, d) = two(args)?;
    let d = degrees(d)?;
    map_real(p, |p| {
        Ok(2.0 * invert_cdf(probability(p)?, None, |x| gam::gamma_p(d / 2.0, x))?)
    })
}

// ---- F ----

/// The degrees of freedom `d1` and `d2` of an F distribution: positive.
fn f_degrees(d1: &Value, d2: &Value) -> Result<(f64, f64), EvalError> {
    Ok((
        positive(param(d1)?, "degrees of freedom d1")?,
        positive(param(d2)?, "degrees of freedom d2")?,
    ))
}

fn df(args: &[Value]) -> Result<Value, EvalError> {
    let (x, d1, d2) = three(args)?;
    let (d1, d2) = f_degrees(d1, d2)?;
    map_real(x, |x| {
        if x < 0.0 {
            return Ok(0.0);
        }
        // f_F(x) = f_beta(z; d1/2, d2/2) dz/dx with z = d1 x / (d1 x + d2), so the F density reuses
        // the stable beta density. dz/dx = (d1/d2) / (1 + d1 x / d2)^2.
        let z = 1.0 / (1.0 + d2 / (d1 * x));
        let ln = gam::beta_ln_density(z, d1 / 2.0, d2 / 2.0)? + (d1 / d2).ln()
            - 2.0 * (d1 * x / d2).ln_1p();
        exp_checked(ln)
    })
}

fn pf(args: &[Value]) -> Result<Value, EvalError> {
    let (x, d1, d2) = three(args)?;
    let (d1, d2) = f_degrees(d1, d2)?;
    map_real(x, |x| {
        if x <= 0.0 {
            return Ok(0.0);
        }
        // z = d1 x / (d1 x + d2), written so that an overflowing d1 x gives z = 1 rather than NaN.
        let z = 1.0 / (1.0 + d2 / (d1 * x));
        gam::ibeta(z, d1 / 2.0, d2 / 2.0)
    })
}

fn qf(args: &[Value]) -> Result<Value, EvalError> {
    let (p, d1, d2) = three(args)?;
    let (d1, d2) = f_degrees(d1, d2)?;
    map_real(p, |p| {
        let p = probability(p)?;
        // With t = I^-1(p; d1/2, d2/2) the quantile is (d2/d1) t/(1-t). For p > 1/2 the value
        // 1 - t is computed as the beta quantile of 1 - p with the shapes swapped, which keeps
        // the small quantity accurate on both sides.
        let ratio = d2 / d1;
        if p <= 0.5 {
            let t = beta_quantile(p, d1 / 2.0, d2 / 2.0)?;
            Ok(ratio * t / (1.0 - t))
        } else {
            let s = beta_quantile(1.0 - p, d2 / 2.0, d1 / 2.0)?;
            Ok(ratio * (1.0 - s) / s)
        }
    })
}

// ---- Cauchy ----

/// Location `l` (any real) and scale `s` (positive) of the Cauchy and logistic families.
fn location_scale(l: &Value, s: &Value, what: &str) -> Result<(f64, f64), EvalError> {
    Ok((param(l)?, positive(param(s)?, what)?))
}

fn dcauchy(args: &[Value]) -> Result<Value, EvalError> {
    let (x, l, s) = three(args)?;
    let (l, s) = location_scale(l, s, "scale s")?;
    map_real(x, |x| {
        let z = (x - l) / s;
        Ok(1.0 / (PI * s * (1.0 + z * z)))
    })
}

fn pcauchy(args: &[Value]) -> Result<Value, EvalError> {
    let (x, l, s) = three(args)?;
    let (l, s) = location_scale(l, s, "scale s")?;
    map_real(x, |x| Ok(0.5 + ((x - l) / s).atan() / PI))
}

fn qcauchy(args: &[Value]) -> Result<Value, EvalError> {
    let (p, l, s) = three(args)?;
    let (l, s) = location_scale(l, s, "scale s")?;
    map_real(p, |p| {
        let p = probability(p)?;
        // tan(pi (p - 1/2)) = -cot(pi p) for p < 1/2, written with the small argument so that the
        // tails keep their relative accuracy.
        Ok(if p <= 0.5 {
            l - s / (PI * p).tan()
        } else {
            l + s / (PI * (1.0 - p)).tan()
        })
    })
}

// ---- Weibull ----

/// The shape `s` of a Weibull distribution: positive.
fn weibull_shape(v: &Value) -> Result<f64, EvalError> {
    positive(param(v)?, "shape s")
}

fn dweibull(args: &[Value]) -> Result<Value, EvalError> {
    let (x, s) = two(args)?;
    let s = weibull_shape(s)?;
    map_real(x, |x| {
        if x < 0.0 {
            return Ok(0.0);
        }
        exp_checked(s.ln() + xlogy(s - 1.0, x) - x.powf(s))
    })
}

fn pweibull(args: &[Value]) -> Result<Value, EvalError> {
    let (x, s) = two(args)?;
    let s = weibull_shape(s)?;
    map_real(x, |x| {
        if x <= 0.0 {
            Ok(0.0)
        } else {
            Ok(-(-x.powf(s)).exp_m1())
        }
    })
}

fn qweibull(args: &[Value]) -> Result<Value, EvalError> {
    let (p, s) = two(args)?;
    let s = weibull_shape(s)?;
    map_real(p, |p| {
        let p = probability(p)?;
        Ok((-(-p).ln_1p()).powf(1.0 / s))
    })
}

// ---- logistic ----

fn dlogis(args: &[Value]) -> Result<Value, EvalError> {
    let (x, l, s) = three(args)?;
    let (l, s) = location_scale(l, s, "scale s")?;
    map_real(x, |x| {
        // exp(-|z|) / (s (1 + exp(-|z|))^2) is the density, symmetric in z and stable for large |z|.
        let e = (-((x - l) / s).abs()).exp();
        let d = 1.0 + e;
        Ok(e / (s * d * d))
    })
}

fn plogis(args: &[Value]) -> Result<Value, EvalError> {
    let (x, l, s) = three(args)?;
    let (l, s) = location_scale(l, s, "scale s")?;
    map_real(x, |x| {
        let z = (x - l) / s;
        Ok(if z >= 0.0 {
            1.0 / (1.0 + (-z).exp())
        } else {
            let e = z.exp();
            e / (1.0 + e)
        })
    })
}

fn qlogis(args: &[Value]) -> Result<Value, EvalError> {
    let (p, l, s) = three(args)?;
    let (l, s) = location_scale(l, s, "scale s")?;
    map_real(p, |p| {
        let p = probability(p)?;
        // ln(p / (1 - p)); p = 0 or 1 gives an infinite value, refused by `map_real`.
        Ok(l + s * (p.ln() - (-p).ln_1p()))
    })
}

// ---- log normal ----

/// The log mean `mu` (any real) and the log standard deviation `sigma` (positive).
fn lognormal_params(mu: &Value, sigma: &Value) -> Result<(f64, f64), EvalError> {
    Ok((param(mu)?, positive(param(sigma)?, "sigma")?))
}

fn dlnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (x, mu, sigma) = three(args)?;
    let (mu, sigma) = lognormal_params(mu, sigma)?;
    map_real(x, |x| {
        if x <= 0.0 {
            return Ok(0.0);
        }
        let z = (x.ln() - mu) / sigma;
        Ok(math::dnorm_std(z) / (sigma * x))
    })
}

fn plnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (x, mu, sigma) = three(args)?;
    let (mu, sigma) = lognormal_params(mu, sigma)?;
    map_real(x, |x| {
        if x <= 0.0 {
            Ok(0.0)
        } else {
            Ok(math::pnorm_std((x.ln() - mu) / sigma))
        }
    })
}

fn qlnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (p, mu, sigma) = three(args)?;
    let (mu, sigma) = lognormal_params(mu, sigma)?;
    map_real(p, |p| {
        let p = probability(p)?;
        // Like qgamma(0), the zero quantile of the log normal is 0 (its z is -inf, exp(-inf) = 0).
        if p == 0.0 {
            return Ok(0.0);
        }
        // p = 1 gives z = +inf, and the non-finite quantile is refused.
        let z = math::qnorm_std(p).unwrap_or(f64::INFINITY);
        Ok(finite(mu + sigma * z)?.exp())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_cdf_recovers_a_known_root() {
        // cdf(x) = x^2 on [0, 1]: the root of x^2 = 0.25 is 0.5.
        let x = invert_cdf(0.25, Some(1.0), |x| Ok(x * x)).unwrap();
        assert!((x - 0.5).abs() < 1e-15, "{x}");
        // Open support: cdf(x) = 1 - exp(-x), the median is ln 2.
        let m = invert_cdf(0.5, None, |x| Ok(-(-x).exp_m1())).unwrap();
        assert!((m - std::f64::consts::LN_2).abs() < 1e-14, "{m}");
    }

    #[test]
    fn invert_cdf_handles_the_limits_without_looping() {
        assert_eq!(invert_cdf(0.0, None, Ok).unwrap(), 0.0);
        assert!(invert_cdf(1.0, None, |x| Ok(x.min(1.0))).is_err());
        // A cdf that never reaches p is refused after the bracket steps.
        assert!(invert_cdf(0.9, None, |_| Ok(0.5)).is_err());
    }
}

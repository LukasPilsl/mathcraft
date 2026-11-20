//! Discrete distributions: geometric, hypergeometric and negative binomial, each with density
//! (`d`), cumulative probability (`p`) and inverse (`q`).
//!
//! Geometric and hypergeometric inverses are closed forms or finite sums. The negative binomial
//! cumulative probability is the regularised incomplete beta function `I_q(n, k + 1)`, and its
//! inverse is an integer search on it (exponential bracketing, then bisection on the integers), so
//! the number of evaluations is about `2 log2(k)`.
//!
//! Arguments are unitless. Integer arguments must be integers (the help says so); a density is 0
//! outside the support, as the help says.

use super::special::special_gamma as gam;
use super::statistics::dist::{integer_of, map_integer, map_real, param, probability, three, two};
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Value, MAX_ELEMENTS};

/// The largest population or draw count of a hypergeometric distribution. Above it the sums over
/// the support are not charged accurately (the work charge clamps its parameter at one million).
pub(crate) const HYPERGEOMETRIC_MAX: u64 = 1_000_000;

/// Registers the discrete distribution functions.
pub(super) fn register(r: &mut Registry) {
    r.register("dgeom", Arity::Exact(2), dgeom);
    r.register("pgeom", Arity::Exact(2), pgeom);
    r.register("qgeom", Arity::Exact(2), qgeom);
    r.register("dhypergeom", Arity::Exact(4), dhypergeom);
    r.register("phypergeom", Arity::Exact(4), phypergeom);
    r.register("qhypergeom", Arity::Exact(4), qhypergeom);
    r.register("dnbinom", Arity::Exact(3), dnbinom);
    r.register("pnbinom", Arity::Exact(3), pnbinom);
    r.register("qnbinom", Arity::Exact(3), qnbinom);
}

fn unsupported(msg: impl Into<String>) -> EvalError {
    EvalError::Unsupported(msg.into())
}

/// A scalar integer parameter, not negative, within `max`.
pub(crate) fn count_param(v: &Value, what: &str, max: u64) -> Result<u64, EvalError> {
    match v {
        Value::Scalar(s) => {
            let k = integer_of(s)?;
            if k < 0 {
                return Err(unsupported(format!("{what} must not be negative")));
            }
            if k as u64 > max {
                return Err(unsupported(format!(
                    "{what} above the supported limit of {max}"
                )));
            }
            Ok(k as u64)
        }
        _ => Err(unsupported("distribution parameters must be scalars")),
    }
}

/// `ln C(n, k)` for `0 <= k <= n`, through the log gamma function.
fn ln_choose(n: f64, k: f64) -> Result<f64, EvalError> {
    Ok(gam::ln_gamma(n + 1.0)? - gam::ln_gamma(k + 1.0)? - gam::ln_gamma(n - k + 1.0)?)
}

/// `k * ln_1p(-q)` with the term zero when `k` is zero (so `q = 1, k = 0` is not NaN).
fn k_ln_1mq(k: f64, q: f64) -> f64 {
    if k == 0.0 {
        0.0
    } else {
        k * (-q).ln_1p()
    }
}

// ---- geometric ----

/// The geometric distribution with success probability `q`: `P(X = k) = q (1 - q)^k` for `k >= 0`.
fn geom_pmf(k: i64, q: f64) -> f64 {
    if k < 0 {
        return 0.0;
    }
    if k == 0 {
        return q;
    }
    q * k_ln_1mq(k as f64, q).exp()
}

/// The smallest integer `k >= 0` with `P(X <= k) >= p`, for `0 <= p`. `p = 0` gives 0; `q = 1`
/// gives 0 for every `p`; `p = 1` is infinite and comes out as a non-finite value.
pub(crate) fn geom_quantile(p: f64, q: f64) -> Result<f64, EvalError> {
    if p == 0.0 || q == 1.0 {
        return Ok(0.0);
    }
    if q == 0.0 {
        return Err(unsupported(
            "qgeom needs q > 0 for p > 0 (the quantile is infinite)",
        ));
    }
    // The ratio is exactly an integer m when p is a power of (1 - q); the shrink keeps rounding
    // from turning m into m + 1 (the smallest k is then m - 1).
    let r = (-p).ln_1p() / (-q).ln_1p() * (1.0 - 4.0 * f64::EPSILON);
    Ok((r.ceil() - 1.0).max(0.0))
}

fn dgeom(args: &[Value]) -> Result<Value, EvalError> {
    let (k, q) = two(args)?;
    let q = probability(param(q)?)?;
    map_integer(k, |k| Ok(geom_pmf(k, q)))
}

fn pgeom(args: &[Value]) -> Result<Value, EvalError> {
    let (k, q) = two(args)?;
    let q = probability(param(q)?)?;
    map_integer(k, |k| {
        if k < 0 {
            return Ok(0.0);
        }
        // P(X <= k) = 1 - (1 - q)^(k + 1).
        Ok(-((k as f64 + 1.0) * (-q).ln_1p()).exp_m1())
    })
}

fn qgeom(args: &[Value]) -> Result<Value, EvalError> {
    let (p, q) = two(args)?;
    let q = probability(param(q)?)?;
    map_real(p, |p| geom_quantile(probability(p)?, q))
}

// ---- hypergeometric ----

/// The hypergeometric population `a` (successes), `b` (failures) and `n` (draws), each an integer
/// within [`HYPERGEOMETRIC_MAX`], with `n <= a + b`.
#[derive(Clone, Copy)]
pub(crate) struct Hyper {
    pub(crate) a: u64,
    pub(crate) b: u64,
    pub(crate) n: u64,
}

impl Hyper {
    pub(crate) fn new(a: u64, b: u64, n: u64) -> Result<Self, EvalError> {
        if n > a + b {
            return Err(unsupported("hypergeometric needs n <= a + b"));
        }
        Ok(Self { a, b, n })
    }

    /// The support `[lo, hi]` of the number of successes drawn.
    fn support(self) -> (u64, u64) {
        (self.n.saturating_sub(self.b), self.n.min(self.a))
    }

    /// `P(X = m)`, zero outside the support.
    fn pmf(self, m: u64) -> Result<f64, EvalError> {
        let (lo, hi) = self.support();
        if m < lo || m > hi {
            return Ok(0.0);
        }
        let (m, a, b, n) = (m as f64, self.a as f64, self.b as f64, self.n as f64);
        Ok((ln_choose(a, m)? + ln_choose(b, n - m)? - ln_choose(a + b, n)?).exp())
    }

    /// `P(X <= m)`, summed from the bottom of the support.
    fn cdf(self, m: i64) -> Result<f64, EvalError> {
        let (lo, hi) = self.support();
        if m < lo as i64 {
            return Ok(0.0);
        }
        if m as u64 >= hi {
            return Ok(1.0);
        }
        let mut sum = 0.0;
        for j in lo..=m as u64 {
            sum += self.pmf(j)?;
        }
        Ok(sum.min(1.0))
    }
}

/// Hypergeometric population arguments `a, b, n`, each an integer within the limit.
pub(crate) fn hyper_args(a: &Value, b: &Value, n: &Value) -> Result<Hyper, EvalError> {
    let a = count_param(a, "a", HYPERGEOMETRIC_MAX)?;
    let b = count_param(b, "b", HYPERGEOMETRIC_MAX)?;
    let n = count_param(n, "n", HYPERGEOMETRIC_MAX)?;
    Hyper::new(a, b, n)
}

/// Smallest `m` with `P(X <= m) >= p` for the hypergeometric distribution `h`.
pub(crate) fn hypergeom_quantile(p: f64, h: Hyper) -> Result<f64, EvalError> {
    let (lo, hi) = h.support();
    let mut sum = 0.0;
    for m in lo..=hi {
        sum += h.pmf(m)?;
        if sum >= p {
            return Ok(m as f64);
        }
    }
    // Rounding can leave the total a hair under p; the top of the support is the answer then.
    Ok(hi as f64)
}

fn dhypergeom(args: &[Value]) -> Result<Value, EvalError> {
    let [m, a, b, n] = args else {
        return Err(unsupported("dhypergeom takes four arguments"));
    };
    let h = hyper_args(a, b, n)?;
    map_integer(m, |m| {
        if m < 0 {
            return Ok(0.0);
        }
        h.pmf(m as u64)
    })
}

fn phypergeom(args: &[Value]) -> Result<Value, EvalError> {
    let [m, a, b, n] = args else {
        return Err(unsupported("phypergeom takes four arguments"));
    };
    let h = hyper_args(a, b, n)?;
    map_integer(m, |m| h.cdf(m))
}

fn qhypergeom(args: &[Value]) -> Result<Value, EvalError> {
    let [p, a, b, n] = args else {
        return Err(unsupported("qhypergeom takes four arguments"));
    };
    let h = hyper_args(a, b, n)?;
    map_real(p, |p| hypergeom_quantile(probability(p)?, h))
}

// ---- negative binomial ----

/// The size `n` of a negative binomial: a positive integer, as the help says.
fn nbinom_size(v: &Value) -> Result<f64, EvalError> {
    let n = count_param(v, "n", u64::MAX)?;
    if n == 0 {
        return Err(unsupported("n must be positive"));
    }
    Ok(n as f64)
}

/// The success probability `q` of a negative binomial: `0 < q <= 1`.
fn nbinom_prob(v: &Value) -> Result<f64, EvalError> {
    let q = param(v)?;
    if q > 0.0 && q <= 1.0 {
        Ok(q)
    } else {
        Err(unsupported("negative binomial needs 0 < q <= 1"))
    }
}

/// `P(X <= k)` for the negative binomial with size `n`: `I_q(n, k + 1)`.
fn nbinom_cdf(k: u64, n: f64, q: f64) -> Result<f64, EvalError> {
    gam::ibeta(q, n, k as f64 + 1.0)
}

fn dnbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (k, n, q) = three(args)?;
    let n = nbinom_size(n)?;
    let q = nbinom_prob(q)?;
    map_integer(k, |k| {
        if k < 0 {
            return Ok(0.0);
        }
        let k = k as f64;
        let ln = gam::ln_gamma(n + k)? - gam::ln_gamma(n)? - gam::ln_gamma(k + 1.0)?
            + n * q.ln()
            + k_ln_1mq(k, q);
        Ok(ln.exp())
    })
}

fn pnbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (k, n, q) = three(args)?;
    let n = nbinom_size(n)?;
    let q = nbinom_prob(q)?;
    map_integer(k, |k| {
        if k < 0 {
            return Ok(0.0);
        }
        nbinom_cdf(k as u64, n, q)
    })
}

fn qnbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (p, n, q) = three(args)?;
    let n = nbinom_size(n)?;
    let q = nbinom_prob(q)?;
    map_real(p, |p| {
        let p = probability(p)?;
        if p == 0.0 {
            return Ok(0.0);
        }
        if nbinom_cdf(0, n, q)? >= p {
            return Ok(0.0);
        }
        // Exponential bracketing, then bisection on the integers: the answer lies in (lo, hi].
        let mut hi: u64 = 1;
        while nbinom_cdf(hi, n, q)? < p {
            hi = hi
                .checked_mul(2)
                .filter(|&h| h <= MAX_ELEMENTS as u64)
                .ok_or_else(|| {
                    unsupported("negative binomial quantile above the supported limit")
                })?;
        }
        let mut lo = hi / 2;
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if nbinom_cdf(mid, n, q)? >= p {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Ok(hi as f64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geom_quantile_is_the_smallest_k() {
        // P(X <= k) = 1 - 0.5^(k+1): the median is 0 (P(X<=0) = 0.5).
        assert_eq!(geom_quantile(0.5, 0.5).unwrap(), 0.0);
        assert_eq!(geom_quantile(0.5 + 1e-12, 0.5).unwrap(), 1.0);
        assert_eq!(geom_quantile(0.0, 0.3).unwrap(), 0.0);
        assert_eq!(geom_quantile(0.4, 1.0).unwrap(), 0.0);
    }

    #[test]
    fn hypergeom_support_and_total() {
        let h = Hyper::new(5, 7, 4).unwrap();
        let total: f64 = (0..=4).map(|m| h.pmf(m).unwrap()).sum();
        assert!((total - 1.0).abs() < 1e-14, "{total}");
        assert_eq!(h.pmf(5).unwrap(), 0.0);
        assert!(Hyper::new(2, 2, 5).is_err());
    }
}

//! Random numbers: `rnd` and the `r` generators of the distributions (`runif`, `rnorm`, ...).
//!
//! The generator is xoshiro256** (Blackman and Vigna, 2018; the algorithm is public domain), seeded
//! through SplitMix64. It is implemented here, so the crate has no random-number dependency. Each
//! thread has one stream, which starts from a fixed seed, so a worksheet gives the same numbers
//! every time it is evaluated in a fresh process. The `Seed` function is not built yet; [`reseed`]
//! is its hook.
//!
//! Each generator returns a column vector of `m` draws (`m` an integer, `1 <= m <= MAX_DRAWS`),
//! except `rnd`, which returns one number in `[0, x)`. Draws are finite; a non-finite draw (only
//! possible for a degenerate parameter) is refused. Gamma draws use Marsaglia and Tsang's method
//! with a bounded rejection loop. Discrete draws invert the cumulative distribution with the same
//! quantile functions as the `q` builtins, so they are exact in distribution.

use std::cell::RefCell;
use std::f64::consts::PI;

use super::distributions_disc::{count_param, geom_quantile, hyper_args, hypergeom_quantile};
use super::statistics::dist::{finite, param, positive, probability, three, two, unitless};
use super::statistics::math;
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Matrix, Scalar, Value};

/// The seed of every thread's stream until it is reseeded.
const DEFAULT_SEED: u64 = 0x4D41_5448_4352_4146;
/// Largest number of draws one call returns.
pub(crate) const MAX_DRAWS: u64 = 1_000_000;
/// Largest number of trials a binomial draw accepts (inversion is charged per trial).
const BINOMIAL_MAX_TRIALS: u64 = 1_000_000;
/// Rejection attempts of the gamma sampler before the call is refused. The acceptance rate is
/// above 0.9 for every shape, so this bound is never reached in practice.
const GAMMA_ATTEMPTS: usize = 1000;

/// xoshiro256** with a SplitMix64 seeding sequence.
#[derive(Debug, Clone)]
pub(crate) struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// A generator for `seed`. Every seed, including 0, gives a valid (non-zero) state.
    pub(crate) fn seeded(seed: u64) -> Self {
        let mut x = seed;
        let mut s = [0u64; 4];
        for slot in &mut s {
            *slot = splitmix64(&mut x);
        }
        if s == [0; 4] {
            s[0] = 1;
        }
        Self { s }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform on the open interval `(0, 1)`: 53 random bits, offset by half a unit, so `ln` and
    /// the quantile inversions never see 0 or 1.
    pub(crate) fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) * (0.5f64).powi(53)
    }

    /// A standard normal draw (Box-Muller; the second value of the pair is discarded).
    pub(crate) fn normal(&mut self) -> f64 {
        let u1 = self.unit();
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
    }

    /// A gamma draw with shape `a > 0` and unit scale (Marsaglia and Tsang, 2000). Shapes below 1
    /// use the boost `G(a) = G(a + 1) U^(1/a)`.
    pub(crate) fn gamma(&mut self, a: f64) -> Result<f64, EvalError> {
        if a < 1.0 {
            let g = self.gamma(a + 1.0)?;
            return Ok(g * self.unit().powf(1.0 / a));
        }
        let d = a - 1.0 / 3.0;
        let c = 1.0 / (9.0 * d).sqrt();
        for _ in 0..GAMMA_ATTEMPTS {
            let x = self.normal();
            let v = (1.0 + c * x).powi(3);
            if v <= 0.0 {
                continue;
            }
            let u = self.unit();
            if u < 1.0 - 0.0331 * x * x * x * x || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
                return Ok(d * v);
            }
        }
        Err(unsupported(
            "gamma random numbers: rejection did not terminate",
        ))
    }
}

impl Rng {
    /// `ln` of a gamma draw with shape `a > 0`. Small shapes are drawn in log space
    /// (`ln G(a) = ln G(a + 1) + ln U / a`), so the value never underflows to `ln 0`.
    pub(crate) fn log_gamma(&mut self, a: f64) -> Result<f64, EvalError> {
        if a < 1.0 {
            let g = self.gamma(a + 1.0)?;
            return Ok(g.ln() + self.unit().ln() / a);
        }
        Ok(self.gamma(a)?.ln())
    }
}

fn splitmix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

thread_local! {
    static STREAM: RefCell<Rng> = RefCell::new(Rng::seeded(DEFAULT_SEED));
}

fn with_stream<T>(f: impl FnOnce(&mut Rng) -> T) -> T {
    STREAM.with(|s| f(&mut s.borrow_mut()))
}

/// Restarts this thread's stream at `seed`: the hook for the `Seed` function, and for tests.
#[cfg(test)]
pub(crate) fn reseed(seed: u64) {
    STREAM.with(|s| *s.borrow_mut() = Rng::seeded(seed));
}

/// Registers `rnd` and the generators.
pub(super) fn register(r: &mut Registry) {
    r.register("rnd", Arity::Exact(1), rnd);
    r.register("runif", Arity::Exact(3), runif);
    r.register("rnorm", Arity::Exact(3), rnorm);
    r.register("rexp", Arity::Exact(2), rexp);
    r.register("rbinom", Arity::Exact(3), rbinom);
    r.register("rpois", Arity::Exact(2), rpois);
    r.register("rF", Arity::Exact(3), rf);
    r.register("rbeta", Arity::Exact(3), rbeta);
    r.register("rcauchy", Arity::Exact(3), rcauchy);
    r.register("rchisq", Arity::Exact(2), rchisq);
    r.register("rgamma", Arity::Exact(2), rgamma);
    r.register("rgeom", Arity::Exact(2), rgeom);
    r.register("rhypergeom", Arity::Exact(4), rhypergeom);
    r.register("rlnorm", Arity::Exact(3), rlnorm);
    r.register("rlogis", Arity::Exact(3), rlogis);
    r.register("rnbinom", Arity::Exact(3), rnbinom);
    r.register("rt", Arity::Exact(2), rt);
    r.register("rweibull", Arity::Exact(2), rweibull);
}

fn unsupported(msg: impl Into<String>) -> EvalError {
    EvalError::Unsupported(msg.into())
}

/// The number of draws `m`: an integer, positive and within [`MAX_DRAWS`].
fn draw_count(v: &Value) -> Result<usize, EvalError> {
    match v {
        Value::Scalar(s) => {
            let m = super::statistics::dist::integer_of(s)?;
            if m < 1 {
                Err(unsupported("the number of draws m must be positive"))
            } else if m as u64 > MAX_DRAWS {
                Err(unsupported(format!(
                    "m above the supported limit of {MAX_DRAWS}"
                )))
            } else {
                Ok(m as usize)
            }
        }
        _ => Err(unsupported("the number of draws must be a scalar")),
    }
}

/// A column vector of `m` draws, each made by `draw` on the thread's stream.
fn draws(
    m: &Value,
    mut draw: impl FnMut(&mut Rng) -> Result<f64, EvalError>,
) -> Result<Value, EvalError> {
    let count = draw_count(m)?;
    with_stream(|rng| {
        let mut data = Vec::with_capacity(count);
        for _ in 0..count {
            let x = finite(draw(rng)?)?;
            data.push(Scalar::real(x, unitless()));
        }
        Ok(Value::Matrix(Matrix::new(count, 1, data)?))
    })
}

/// A real parameter that must be positive.
fn positive_param(v: &Value, what: &str) -> Result<f64, EvalError> {
    positive(param(v)?, what)
}

/// The `n` of a binomial or a negative binomial: a non-negative integer within the limit.
fn trial_count(v: &Value) -> Result<u64, EvalError> {
    count_param(v, "n", BINOMIAL_MAX_TRIALS)
}

fn rnd(args: &[Value]) -> Result<Value, EvalError> {
    let [x] = args else {
        return Err(unsupported("rnd takes one argument"));
    };
    let x = positive_param(x, "x")?;
    let u = with_stream(|r| r.unit());
    Ok(Value::Scalar(Scalar::real(finite(u * x)?, unitless())))
}

fn runif(args: &[Value]) -> Result<Value, EvalError> {
    let (m, a, b) = three(args)?;
    let (a, b) = (param(a)?, param(b)?);
    if a >= b {
        return Err(unsupported("uniform needs a < b"));
    }
    draws(m, |r| Ok(a + (b - a) * r.unit()))
}

fn rnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (m, mu, sigma) = three(args)?;
    let (mu, sigma) = (param(mu)?, positive_param(sigma, "sigma")?);
    draws(m, |r| Ok(mu + sigma * r.normal()))
}

fn rexp(args: &[Value]) -> Result<Value, EvalError> {
    let (m, rate) = two(args)?;
    let rate = positive_param(rate, "rate")?;
    draws(m, |r| Ok(-r.unit().ln() / rate))
}

fn rbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (m, n, q) = three(args)?;
    let n = trial_count(n)?;
    let q = probability(param(q)?)?;
    draws(m, |r| Ok(math::qbinom(r.unit(), n, q) as f64))
}

fn rpois(args: &[Value]) -> Result<Value, EvalError> {
    let (m, lambda) = two(args)?;
    let lambda = positive_param(lambda, "lambda")?;
    if lambda > math::POISSON_LAMBDA_MAX {
        return Err(unsupported(format!(
            "poisson lambda above {:e} is not supported",
            math::POISSON_LAMBDA_MAX
        )));
    }
    draws(m, |r| Ok(math::qpois(r.unit(), lambda) as f64))
}

fn rf(args: &[Value]) -> Result<Value, EvalError> {
    let (m, d1, d2) = three(args)?;
    let (d1, d2) = (
        positive_param(d1, "degrees of freedom d1")?,
        positive_param(d2, "degrees of freedom d2")?,
    );
    draws(m, |r| {
        let x = 2.0 * r.gamma(d1 / 2.0)? / d1;
        let y = 2.0 * r.gamma(d2 / 2.0)? / d2;
        Ok(x / y)
    })
}

fn rbeta(args: &[Value]) -> Result<Value, EvalError> {
    let (m, a, b) = three(args)?;
    let (a, b) = (
        positive_param(a, "shape s1")?,
        positive_param(b, "shape s2")?,
    );
    draws(m, |r| {
        let x = r.gamma(a)?;
        let y = r.gamma(b)?;
        // Both draws underflow to 0 for small shapes (a = b = 0.001 gives 0 about half the time).
        // Then the ratio is taken in log space: X / (X + Y) = 1 / (1 + exp(ln Y - ln X)).
        if x > 0.0 && y > 0.0 && (x + y).is_finite() {
            return Ok(x / (x + y));
        }
        let ln_x = r.log_gamma(a)?;
        let ln_y = r.log_gamma(b)?;
        Ok(1.0 / (1.0 + (ln_y - ln_x).exp()))
    })
}

fn rcauchy(args: &[Value]) -> Result<Value, EvalError> {
    let (m, l, s) = three(args)?;
    let (l, s) = (param(l)?, positive_param(s, "scale s")?);
    draws(m, |r| Ok(l + s * (PI * (r.unit() - 0.5)).tan()))
}

fn rchisq(args: &[Value]) -> Result<Value, EvalError> {
    let (m, d) = two(args)?;
    let d = positive_param(d, "degrees of freedom d")?;
    draws(m, |r| Ok(2.0 * r.gamma(d / 2.0)?))
}

fn rgamma(args: &[Value]) -> Result<Value, EvalError> {
    let (m, s) = two(args)?;
    let s = positive_param(s, "shape s")?;
    draws(m, |r| r.gamma(s))
}

fn rgeom(args: &[Value]) -> Result<Value, EvalError> {
    let (m, q) = two(args)?;
    let q = probability(param(q)?)?;
    draws(m, |r| geom_quantile(r.unit(), q))
}

fn rhypergeom(args: &[Value]) -> Result<Value, EvalError> {
    let [m, a, b, n] = args else {
        return Err(unsupported("rhypergeom takes four arguments"));
    };
    let h = hyper_args(a, b, n)?;
    draws(m, |r| hypergeom_quantile(r.unit(), h))
}

fn rlnorm(args: &[Value]) -> Result<Value, EvalError> {
    let (m, mu, sigma) = three(args)?;
    let (mu, sigma) = (param(mu)?, positive_param(sigma, "sigma")?);
    draws(m, |r| Ok((mu + sigma * r.normal()).exp()))
}

fn rlogis(args: &[Value]) -> Result<Value, EvalError> {
    let (m, l, s) = three(args)?;
    let (l, s) = (param(l)?, positive_param(s, "scale s")?);
    draws(m, |r| {
        let u = r.unit();
        Ok(l + s * (u.ln() - (-u).ln_1p()))
    })
}

fn rnbinom(args: &[Value]) -> Result<Value, EvalError> {
    let (m, n, p) = three(args)?;
    let n = count_param(n, "n", u64::MAX)?;
    if n == 0 {
        return Err(unsupported("n must be positive"));
    }
    let n = n as f64;
    let p = param(p)?;
    if !(p > 0.0 && p <= 1.0) {
        return Err(unsupported("negative binomial needs 0 < p <= 1"));
    }
    // A gamma-Poisson mixture: lambda ~ Gamma(n, (1 - p) / p) and X ~ Poisson(lambda).
    let scale = (1.0 - p) / p;
    draws(m, |r| {
        let lambda = r.gamma(n)? * scale;
        if lambda <= 0.0 {
            return Ok(0.0);
        }
        if lambda > math::POISSON_LAMBDA_MAX {
            return Err(unsupported(format!(
                "negative binomial mean above {:e} is not supported",
                math::POISSON_LAMBDA_MAX
            )));
        }
        Ok(math::qpois(r.unit(), lambda) as f64)
    })
}

fn rt(args: &[Value]) -> Result<Value, EvalError> {
    let (m, d) = two(args)?;
    let d = positive_param(d, "degrees of freedom d")?;
    draws(m, |r| {
        let z = r.normal();
        let chi = 2.0 * r.gamma(d / 2.0)?;
        Ok(z / (chi / d).sqrt())
    })
}

fn rweibull(args: &[Value]) -> Result<Value, EvalError> {
    let (m, s) = two(args)?;
    let s = positive_param(s, "shape s")?;
    draws(m, |r| Ok((-r.unit().ln()).powf(1.0 / s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mean and sample variance of `n` draws of `f` on a fresh generator with `seed`.
    fn moments(seed: u64, n: usize, mut f: impl FnMut(&mut Rng) -> f64) -> (f64, f64) {
        let mut rng = Rng::seeded(seed);
        let xs: Vec<f64> = (0..n).map(|_| f(&mut rng)).collect();
        let mean = xs.iter().sum::<f64>() / n as f64;
        let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n as f64 - 1.0);
        (mean, var)
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::seeded(42);
        let mut b = Rng::seeded(42);
        let mut c = Rng::seeded(43);
        let xs: Vec<u64> = (0..16).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..16).map(|_| b.next_u64()).collect();
        let zs: Vec<u64> = (0..16).map(|_| c.next_u64()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs, zs);
        // The zero seed is valid too.
        assert_ne!(Rng::seeded(0).next_u64(), 0);
    }

    #[test]
    fn unit_is_open_and_roughly_uniform() {
        let (mean, var) = moments(7, 200_000, |r| r.unit());
        assert!((mean - 0.5).abs() < 0.003, "mean {mean}");
        assert!((var - 1.0 / 12.0).abs() < 0.002, "var {var}");
        let mut rng = Rng::seeded(9);
        for _ in 0..10_000 {
            let u = rng.unit();
            assert!(u > 0.0 && u < 1.0);
        }
    }

    #[test]
    fn normal_moments() {
        let (mean, var) = moments(11, 200_000, |r| r.normal());
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((var - 1.0).abs() < 0.02, "var {var}");
    }

    #[test]
    fn gamma_moments_for_several_shapes() {
        // Mean s, variance s (unit scale). Shapes below one use the boost.
        for s in [0.4, 1.0, 2.5, 9.0] {
            let (mean, var) = moments(13, 200_000, |r| r.gamma(s).unwrap());
            assert!(
                (mean - s).abs() < 0.03 * s.max(1.0),
                "shape {s}: mean {mean}"
            );
            assert!((var - s).abs() < 0.06 * s.max(1.0), "shape {s}: var {var}");
        }
    }

    #[test]
    fn rnd_stream_is_seeded_per_thread() {
        reseed(5);
        let a = with_stream(|r| r.unit());
        reseed(5);
        let b = with_stream(|r| r.unit());
        assert_eq!(a, b);
    }

    #[test]
    fn draw_count_is_checked() {
        use crate::eval::value::Scalar as S;
        let v = |x: f64| Value::Scalar(S::real(x, unitless()));
        assert!(draw_count(&v(0.0)).is_err());
        assert!(draw_count(&v(2.5)).is_err());
        assert!(draw_count(&v(1e9)).is_err());
        assert_eq!(draw_count(&v(3.0)).unwrap(), 3);
    }
}

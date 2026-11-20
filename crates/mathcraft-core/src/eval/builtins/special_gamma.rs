//! Gamma family, error function, digamma and incomplete beta: pure `f64` kernels.
//!
//! Real arguments only. The builtins in `special.rs` refuse complex arguments with
//! `EvalError::Unsupported`. Accuracy, checked against CPython's `math` module in
//! `tests/builtins_special.rs`:
//! - `gamma`, `ln_gamma`: about 1e-14 relative on 0.5..=171 (Lanczos, g = 7, 9 terms).
//! - `erf`, `erfc`: about 1e-14 relative (positive-term series below 2, Laplace continued fraction
//!   for `erfc` from 2 on, so `erfc(10) = 2.088487583762545e-45` keeps its relative accuracy).
//! - `gamma_p`, `gamma_q`, `upper_incomplete_gamma`, `ibeta`: series or continued fractions with
//!   a fixed iteration cap; they return `Unsupported` when the cap is reached.
//!
//! Domain rules: the gamma function is undefined at zero and the negative integers, and the
//! builtins report that as `Unsupported`. Arguments whose result overflows `f64` return
//! `Overflow`. Results that underflow to zero are returned as zero.

use std::f64::consts::PI;

use crate::eval::error::EvalError;

/// Smallest magnitude used in continued-fraction divisions (Numerical Recipes' FPMIN).
const FPMIN: f64 = 1e-300;
/// Relative tolerance of every iterative kernel in this file.
const EPS: f64 = 2.220_446_049_250_313e-16;
/// Iteration cap for every series, continued fraction and reflection loop.
const MAX_ITER: usize = 100_000;
/// Above this, `gamma(z)` is beyond `f64` (Gamma(171.62) is the largest finite value).
const GAMMA_LIMIT: f64 = 172.0;
/// `erfc` switches from `1 - erf` to its continued fraction at this magnitude.
const ERFC_CF_MIN: f64 = 2.0;

/// Stirling's series is used from this argument up; below it the argument is shifted up by a
/// product, which is exact to a few ulps for the sizes used here.
const STIRLING_MIN: f64 = 20.0;

/// Coefficients `B_2k / (2k (2k-1))` for k = 1..8 of the Stirling tail
/// `lnGamma(x) = (x - 1/2) ln x - x + ln(2 pi)/2 + sum_k c_k x^(1 - 2k)`.
const STIRLING_COEFF: [f64; 8] = [
    1.0 / 12.0,
    -1.0 / 360.0,
    1.0 / 1260.0,
    -1.0 / 1680.0,
    1.0 / 1188.0,
    -691.0 / 360_360.0,
    1.0 / 156.0,
    -3617.0 / 122_400.0,
];

/// The Stirling tail `sum_k c_k x^(1 - 2k)` for `x >= STIRLING_MIN`.
fn stirling_tail(x: f64) -> f64 {
    let u = 1.0 / (x * x);
    let mut acc = 0.0;
    for &c in STIRLING_COEFF.iter().rev() {
        acc = acc * u + c;
    }
    acc / x
}

/// `Gamma(y)` for `y >= STIRLING_MIN`, split so that no intermediate power overflows.
fn gamma_stirling(y: f64) -> f64 {
    let h = y.powf((y - 0.5) / 2.0);
    (2.0 * PI).sqrt() * h * (h * (-y).exp()) * stirling_tail(y).exp()
}

/// `Gamma(z)` for real `z`. Errors: `Unsupported` at zero and the negative integers, `Overflow`
/// above about 171.6, `NotFinite` for non-finite input.
pub(crate) fn gamma(z: f64) -> Result<f64, EvalError> {
    if !z.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if z <= 0.0 && z.fract() == 0.0 {
        return Err(EvalError::Unsupported(
            "gamma is undefined at 0 and the negative integers".to_string(),
        ));
    }
    if z >= GAMMA_LIMIT {
        return Err(EvalError::Overflow);
    }
    if z < 0.0 {
        // Reflection: Gamma(z) = pi / (sin(pi z) Gamma(1 - z)). A huge 1 - z underflows to 0.
        let big = 1.0 - z;
        if big >= GAMMA_LIMIT {
            return Ok(0.0);
        }
        return Ok(PI / ((PI * z).sin() * gamma(big)?));
    }
    // Gamma(z) = Gamma(z + n) / (z (z+1) ... (z+n-1)) with z + n >= STIRLING_MIN.
    let n = if z < STIRLING_MIN {
        (STIRLING_MIN - z).ceil()
    } else {
        0.0
    };
    let mut product = 1.0;
    let mut j = 0.0;
    while j < n {
        product *= z + j;
        j += 1.0;
    }
    let result = gamma_stirling(z + n) / product;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(EvalError::Overflow)
    }
}

/// `ln(Gamma(x))` for real `x > 0`. Negative arguments give a complex logarithm (or a pole), so
/// they are refused. Near the zeros at `x = 1` and `x = 2` the result is absolute to about 1e-14.
pub(crate) fn ln_gamma(x: f64) -> Result<f64, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if x <= 0.0 {
        return Err(EvalError::Unsupported(
            "lnGamma is refused at zero and negative arguments".to_string(),
        ));
    }
    let n = if x < STIRLING_MIN {
        (STIRLING_MIN - x).ceil()
    } else {
        0.0
    };
    let mut log_product = 0.0;
    let mut j = 0.0;
    while j < n {
        log_product += (x + j).ln();
        j += 1.0;
    }
    let y = x + n;
    let stirling = (y - 0.5) * y.ln() - y + 0.5 * (2.0 * PI).ln() + stirling_tail(y);
    Ok(stirling - log_product)
}

/// `ln(Gamma(b + a)) - ln(Gamma(b))` for `a >= 0` and `b > 0`. For `b >= STIRLING_MIN` it uses the
/// Stirling form `a ln b + (b + a - 1/2) ln(1 + a/b) - a + (tail(b + a) - tail(b))`, so the two large
/// log gammas never cancel (the absolute error is about `eps * a ln b`, not `eps * b ln b`).
pub(crate) fn ln_gamma_shift(b: f64, a: f64) -> Result<f64, EvalError> {
    if !a.is_finite() || !b.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if a == 0.0 {
        return Ok(0.0);
    }
    if b < STIRLING_MIN {
        return Ok(ln_gamma(b + a)? - ln_gamma(b)?);
    }
    let s = b + a;
    Ok(a * b.ln() + (s - 0.5) * (a / b).ln_1p() - a + stirling_tail(s) - stirling_tail(b))
}

/// `ln B(a, b) = ln Gamma(a) + ln Gamma(b) - ln Gamma(a + b)` for `a, b > 0`, through
/// [`ln_gamma_shift`] so that a large shape does not cancel against the sum.
pub(crate) fn ln_beta(a: f64, b: f64) -> Result<f64, EvalError> {
    let (small, big) = if a <= b { (a, b) } else { (b, a) };
    Ok(ln_gamma(small)? - ln_gamma_shift(big, small)?)
}

/// `e * t`, with the term zero when `e` is zero (so `0 * (-inf)` is not NaN).
fn term(e: f64, t: f64) -> f64 {
    if e == 0.0 {
        0.0
    } else {
        e * t
    }
}

/// Log of the gamma density with unit scale and shape `s > 0` at `x >= 0`.
///
/// For `s >= STIRLING_MIN` the density is written around its mode: with `delta = (x - s) / s`,
/// `ln f = (s - 1) ln(1 + delta) + (s - x) - ln(2 pi s) / 2 - tail(s)`, so the terms of size `s`
/// cancel analytically instead of through two log gammas of size `s ln s`.
pub(crate) fn gamma_ln_density(x: f64, s: f64) -> Result<f64, EvalError> {
    if s >= STIRLING_MIN {
        let delta = (x - s) / s;
        Ok(term(s - 1.0, delta.ln_1p()) + (s - x) - 0.5 * (2.0 * PI * s).ln() - stirling_tail(s))
    } else {
        Ok(term(s - 1.0, x.ln()) - x - ln_gamma(s)?)
    }
}

/// Log of the beta density `x^(a-1) (1-x)^(b-1) / B(a, b)` at `0 < x < 1`.
///
/// For `a, b >= STIRLING_MIN` it is written around the mean `m = a / (a + b)` with
/// `u = (x - m) / m` and `v = (m - x) / (1 - m)`:
/// `ln f = (a - 1) ln(1 + u) + (b - 1) ln(1 + v) + ln(N / (m (1 - m))) / 2 - ln(2 pi) / 2 - (tail(a) + tail(b) - tail(N))`
/// with `N = a + b`. Otherwise `ln B` comes from [`ln_beta`] and `ln(1 - x)` from `ln_1p`.
pub(crate) fn beta_ln_density(x: f64, a: f64, b: f64) -> Result<f64, EvalError> {
    if a >= STIRLING_MIN && b >= STIRLING_MIN {
        let n = a + b;
        let m = a / n;
        let u = (x - m) / m;
        let v = (m - x) / (1.0 - m);
        Ok(
            term(a - 1.0, u.ln_1p()) + term(b - 1.0, v.ln_1p()) + 0.5 * (n / (m * (1.0 - m))).ln()
                - 0.5 * (2.0 * PI).ln()
                - (stirling_tail(a) + stirling_tail(b) - stirling_tail(n)),
        )
    } else {
        Ok(term(a - 1.0, x.ln()) + term(b - 1.0, (-x).ln_1p()) - ln_beta(a, b)?)
    }
}

/// Digamma `psi(x) = d/dx ln Gamma(x)` for real `x`. Poles at zero and the negative integers
/// are `Unsupported`.
pub(crate) fn digamma(x: f64) -> Result<f64, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if x <= 0.0 && x.fract() == 0.0 {
        return Err(EvalError::Unsupported(
            "Psi is undefined at 0 and the negative integers".to_string(),
        ));
    }
    if x < 0.0 {
        // psi(x) = psi(1 - x) - pi cot(pi x)
        return Ok(digamma(1.0 - x)? - PI / (PI * x).tan());
    }
    // Shift to y >= 12, then the asymptotic series through B10 (truncation error below 1e-15).
    let mut acc = 0.0;
    let mut y = x;
    while y < 12.0 {
        acc -= 1.0 / y;
        y += 1.0;
    }
    let inv = 1.0 / (y * y);
    let series = inv
        * (1.0 / 12.0
            - inv
                * (1.0 / 120.0 - inv * (1.0 / 252.0 - inv * (1.0 / 240.0 - inv * (1.0 / 132.0)))));
    Ok(acc + y.ln() - 0.5 / y - series)
}

/// Series for the regularised lower incomplete gamma: `P(a, x) = exp(a ln x - x - lnGamma(a)) S`.
fn gamma_series_sum(a: f64, x: f64) -> Result<f64, EvalError> {
    let mut ap = a;
    let mut del = 1.0 / a;
    let mut sum = del;
    for _ in 0..MAX_ITER {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * EPS {
            return Ok(sum);
        }
    }
    Err(EvalError::Unsupported(
        "incomplete gamma series did not converge".to_string(),
    ))
}

/// Continued fraction (modified Lentz) for `Q(a, x)`, returning `h` with
/// `Gamma(a, x) = exp(a ln x - x) h`.
fn gamma_cf_h(a: f64, x: f64) -> Result<f64, EvalError> {
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / FPMIN;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..MAX_ITER {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = b + an / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            return Ok(h);
        }
    }
    Err(EvalError::Unsupported(
        "incomplete gamma continued fraction did not converge".to_string(),
    ))
}

/// Checks the shape parameters of the regularised and unregularised incomplete gamma.
fn check_gamma_args(a: f64, x: f64) -> Result<(), EvalError> {
    if !a.is_finite() || !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if a <= 0.0 || x < 0.0 {
        return Err(EvalError::Unsupported(
            "incomplete gamma needs a > 0 and x >= 0".to_string(),
        ));
    }
    Ok(())
}

/// Regularised lower incomplete gamma `P(a, x) = gamma(a, x) / Gamma(a)`, for `a > 0`, `x >= 0`.
pub(crate) fn gamma_p(a: f64, x: f64) -> Result<f64, EvalError> {
    check_gamma_args(a, x)?;
    if x == 0.0 {
        return Ok(0.0);
    }
    if x < a + 1.0 {
        let pref = (a * x.ln() - x - ln_gamma(a)?).exp();
        Ok(pref * gamma_series_sum(a, x)?)
    } else {
        Ok(1.0 - gamma_q_cf(a, x)?)
    }
}

/// Regularised upper incomplete gamma `Q(a, x) = 1 - P(a, x)`, for `a > 0`, `x >= 0`.
/// Kept for the statistics distributions (chi-square and gamma tails), which are not built yet,
/// so it has no caller in this build.
#[allow(dead_code)]
pub(crate) fn gamma_q(a: f64, x: f64) -> Result<f64, EvalError> {
    check_gamma_args(a, x)?;
    if x == 0.0 {
        return Ok(1.0);
    }
    if x < a + 1.0 {
        let pref = (a * x.ln() - x - ln_gamma(a)?).exp();
        Ok(1.0 - pref * gamma_series_sum(a, x)?)
    } else {
        gamma_q_cf(a, x)
    }
}

fn gamma_q_cf(a: f64, x: f64) -> Result<f64, EvalError> {
    let h = gamma_cf_h(a, x)?;
    Ok((a * x.ln() - x - ln_gamma(a)?).exp() * h)
}

/// Unregularised upper incomplete gamma `Gamma(a, x) = integral from x to infinity of
/// t^(a-1) e^(-t) dt`, for `a > 0`, `x >= 0`. `Gamma(a, 0) = Gamma(a)`. Errors with `Overflow`
/// when the result exceeds `f64`.
pub(crate) fn upper_incomplete_gamma(a: f64, x: f64) -> Result<f64, EvalError> {
    check_gamma_args(a, x)?;
    if x == 0.0 {
        return gamma(a);
    }
    if x >= a + 1.0 {
        // Continued-fraction branch: no Gamma(a) factor, so large a does not overflow here.
        let value = (a * x.ln() - x).exp() * gamma_cf_h(a, x)?;
        return if value.is_finite() {
            Ok(value)
        } else {
            Err(EvalError::Overflow)
        };
    }
    let p = gamma_p(a, x)?;
    let value = gamma(a)? * (1.0 - p);
    if value.is_finite() {
        Ok(value)
    } else {
        Err(EvalError::Overflow)
    }
}

/// Continued fraction for the incomplete beta function (modified Lentz).
fn beta_cf(a: f64, b: f64, x: f64) -> Result<f64, EvalError> {
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..MAX_ITER {
        let mf = m as f64;
        let m2 = 2.0 * mf;
        let aa = mf * (b - mf) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + mf) * (qab + mf) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            return Ok(h);
        }
    }
    Err(EvalError::Unsupported(
        "incomplete beta continued fraction did not converge".to_string(),
    ))
}

/// Incomplete beta as the help defines `ibeta(upper, p, q)`:
/// `Gamma(p+q)/(Gamma(p) Gamma(q)) * integral from 0 to upper of t^(p-1) (1-t)^(q-1) dt`.
/// Needs `0 <= upper <= 1` and `p, q > 0`; `ibeta(1, p, q) = 1`.
pub(crate) fn ibeta(upper: f64, p: f64, q: f64) -> Result<f64, EvalError> {
    if !upper.is_finite() || !p.is_finite() || !q.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if !(0.0..=1.0).contains(&upper) || p <= 0.0 || q <= 0.0 {
        return Err(EvalError::Unsupported(
            "ibeta needs 0 <= a <= 1 and positive x and y".to_string(),
        ));
    }
    if upper == 0.0 {
        return Ok(0.0);
    }
    if upper == 1.0 {
        return Ok(1.0);
    }
    // ln(1 - upper) through ln_1p: for a huge q, q ln(1 - upper) is O(1) and 1 - upper rounds to 1.
    let ln_front = p * upper.ln() + q * (-upper).ln_1p() - ln_beta(p, q)?;
    let front = ln_front.exp();
    if upper < (p + 1.0) / (p + q + 2.0) {
        Ok(front * beta_cf(p, q, upper)? / p)
    } else {
        Ok(1.0 - front * beta_cf(q, p, 1.0 - upper)? / q)
    }
}

/// Error function `erf(x) = 2/sqrt(pi) integral from 0 to x of e^(-t^2) dt`.
///
/// Below 2 it uses the series `erf(x) = 2x/sqrt(pi) e^(-x^2) sum (2x^2)^n / (2n+1)!!`, whose
/// terms are all positive. From 2 on it is `1 - erfc(x)`, where `erfc` is small and the
/// relative error of `1 - erfc` is harmless.
pub(crate) fn erf(x: f64) -> f64 {
    if x.abs() >= ERFC_CF_MIN {
        return x.signum() * (1.0 - erfc_pos(x.abs()));
    }
    let x2 = x * x;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..MAX_ITER {
        term *= 2.0 * x2 / (2 * n + 1) as f64;
        sum += term;
        if term < sum * EPS {
            break;
        }
    }
    2.0 * x / PI.sqrt() * (-x2).exp() * sum
}

/// Complementary error function `erfc(x) = 1 - erf(x)`, with full relative accuracy in the tail.
pub(crate) fn erfc(x: f64) -> f64 {
    if x >= ERFC_CF_MIN {
        erfc_pos(x)
    } else if x <= -ERFC_CF_MIN {
        2.0 - erfc_pos(-x)
    } else {
        1.0 - erf(x)
    }
}

/// `erfc(x)` for `x >= 2` by the Laplace continued fraction
/// `erfc(x) = e^(-x^2)/sqrt(pi) / (x + (1/2)/(x + 1/(x + (3/2)/(x + ...))))`.
fn erfc_pos(x: f64) -> f64 {
    if x > 27.3 {
        return 0.0;
    }
    // Modified Lentz on f = b0 + a1/(b1 + a2/(b2 + ...)) with b_k = x and a_k = k/2.
    let mut f = x;
    let mut c = x;
    let mut d = 0.0;
    for k in 1..MAX_ITER {
        let ak = k as f64 / 2.0;
        d = x + ak * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = x + ak / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = c * d;
        f *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    (-x * x).exp() / (PI.sqrt() * f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_at_integers_and_half_integers() {
        assert!((gamma(5.0).unwrap() - 24.0).abs() < 1e-12);
        assert!((gamma(0.5).unwrap() - PI.sqrt()).abs() < 1e-15);
        // Gamma(3/2) = sqrt(pi) / 2
        assert!((gamma(1.5).unwrap() - PI.sqrt() / 2.0).abs() < 1e-15);
    }

    #[test]
    fn gamma_reflection_for_negative_arguments() {
        // Gamma(-0.5) = -2 sqrt(pi)
        assert!((gamma(-0.5).unwrap() + 2.0 * PI.sqrt()).abs() < 1e-14);
    }

    #[test]
    fn gamma_poles_and_overflow_are_errors() {
        assert!(matches!(gamma(0.0), Err(EvalError::Unsupported(_))));
        assert!(matches!(gamma(-3.0), Err(EvalError::Unsupported(_))));
        assert_eq!(gamma(200.0), Err(EvalError::Overflow));
    }

    #[test]
    fn erf_known_values() {
        assert!((erf(0.5) - 0.520_499_877_813_046_5).abs() < 1e-15);
        assert!((erfc(10.0) - 2.088_487_583_762_545e-45).abs() / 2.088e-45 < 1e-13);
    }

    #[test]
    fn ibeta_limits() {
        assert_eq!(ibeta(1.0, 2.0, 3.0).unwrap(), 1.0);
        assert_eq!(ibeta(0.0, 2.0, 3.0).unwrap(), 0.0);
    }
}

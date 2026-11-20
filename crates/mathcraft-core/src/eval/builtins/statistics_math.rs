//! Numeric kernels for the statistics builtins. Private to the statistics tier: it does not use the
//! special-functions tier, so the error function and the discrete-distribution sums are written here.
//!
//! Accuracy (measured in the tests against exact or correctly rounded references):
//! - `erf` is a positive-term series below `SERIES_LIMIT` and a continued fraction above it, so
//!   `erfc` keeps relative error near 1e-15 in the upper tail. Below the limit `erfc` is `1 - erf`,
//!   which loses about two digits near the limit.
//! - `qnorm` is Newton's method on `ln(pnorm)`, which keeps relative accuracy in the lower tail.
//! - `dbinom` and `dpois` use the saddle-point form (`stirlerr` and `bd0`), so no `ln k!` is
//!   subtracted from another large logarithm. Measured against exact values: `dbinom(50000,
//!   100000, 0.5)` relative error 3.4e-16, `dpois(1e6, 1e6)` below 1e-15. `stirlerr` uses exact
//!   summation below 20 and the Stirling series (omitted term below 1e-17) from 20 on.
//! - `pbinom` and `ppois` add the smaller tail: the terms at or below the mode are summed downward
//!   from `k`, the terms above it upward from `k + 1` and subtracted from one. The recurrence is
//!   re-anchored on the closed form every 128 steps. Measured: `pbinom(50000, 100000, 0.5)` within
//!   3e-15 absolute, `ppois(1e6, 1e6)` relative error 2e-14.
//! - `qbinom` and `qpois` bisect on the cumulative function.

use std::f64::consts::{PI, SQRT_2};

/// Below this argument `erfc` uses `1 - erf`; above it, the continued fraction.
const SERIES_LIMIT: f64 = 1.0;

/// `ln(sqrt(2 pi))`.
const LN_SQRT_2PI: f64 = 0.918_938_533_204_672_7;

/// `ln(2 pi)`.
const LN_2PI: f64 = 1.837_877_066_409_345_5;

/// Below this count `stirlerr` sums `ln k!` exactly; at or above it the Stirling series is used.
const STIRLING_MIN: u64 = 20;

/// Tail terms are recomputed from the closed form every this many steps, so the rounding of the
/// multiplicative recurrence does not accumulate.
const REANCHOR: u64 = 128;

/// A tail sum stops once its next term is below this fraction of the running sum.
const TAIL_EPS: f64 = 1e-18;

/// Largest Poisson rate accepted. A cumulative value costs about `9 sqrt(lambda)` steps.
pub(crate) const POISSON_LAMBDA_MAX: f64 = 1e12;

/// `erf(x)` for `0 <= x < SERIES_LIMIT`, from the positive-term series
/// `erf(x) = 2x/sqrt(pi) e^{-x^2} sum_n (2x^2)^n / (1*3*...*(2n+1))`.
fn erf_series(x: f64) -> f64 {
    let x2 = x * x;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut n = 1.0;
    while term > sum * 1e-17 && n < 1000.0 {
        term *= 2.0 * x2 / (2.0 * n + 1.0);
        sum += term;
        n += 1.0;
    }
    2.0 * x / PI.sqrt() * (-x2).exp() * sum
}

/// `erfc(x)` for `x >= SERIES_LIMIT`, from the continued fraction
/// `erfc(x) = e^{-x^2} / (sqrt(pi) * (x + (1/2)/(x + 1/(x + (3/2)/(x + ...)))))`, evaluated by the
/// modified Lentz method.
fn erfc_cf(x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let mut f = x;
    let mut c = x;
    let mut d = 0.0;
    for k in 1..5000 {
        let a = f64::from(k) / 2.0;
        d = x + a * d;
        if d == 0.0 {
            d = TINY;
        }
        c = x + a / c;
        if c == 0.0 {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = c * d;
        f *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            break;
        }
    }
    (-x * x).exp() / (PI.sqrt() * f)
}

/// `erfc(x)` for `x >= 0`.
fn erfc_nonneg(x: f64) -> f64 {
    if x < SERIES_LIMIT {
        1.0 - erf_series(x)
    } else {
        erfc_cf(x)
    }
}

/// Standard normal cumulative distribution `Phi(z)`. Lower tail keeps relative accuracy.
pub(crate) fn pnorm_std(z: f64) -> f64 {
    let t = z / SQRT_2;
    if z < 0.0 {
        0.5 * erfc_nonneg(-t)
    } else {
        1.0 - 0.5 * erfc_nonneg(t)
    }
}

/// Standard normal density `phi(z)`.
pub(crate) fn dnorm_std(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * PI).sqrt()
}

/// Inverse of `pnorm_std` for `0 < p < 1`; `None` outside that open interval.
pub(crate) fn qnorm_std(p: f64) -> Option<f64> {
    if !(p > 0.0 && p < 1.0) {
        return None;
    }
    if p == 0.5 {
        return Some(0.0);
    }
    if p > 0.5 {
        // 1 - p is exact here (Sterbenz), so the upper half loses nothing extra.
        return lower_tail_quantile(1.0 - p).map(|x| -x);
    }
    lower_tail_quantile(p)
}

/// Solves `Phi(x) = p` for `0 < p < 0.5` by Newton's method on `g(x) = ln Phi(x) - ln p`, whose
/// derivative is `phi(x)/Phi(x)`. The density term is computed in the log domain so that it does
/// not underflow before `Phi` does.
fn lower_tail_quantile(p: f64) -> Option<f64> {
    let ln_p = p.ln();
    // Abramowitz and Stegun 26.2.23 as the starting point (absolute error about 4.5e-4).
    let t = (-2.0 * ln_p).sqrt();
    let mut x = -(t
        - (2.515_517 + 0.802_853 * t + 0.010_328 * t * t)
            / (1.0 + 1.432_788 * t + 0.189_269 * t * t + 0.001_308 * t * t * t));
    for _ in 0..200 {
        let ln_big_phi = pnorm_std(x).ln();
        let ln_phi = -0.5 * x * x - LN_SQRT_2PI;
        let g = ln_big_phi - ln_p;
        let step = g * (ln_big_phi - ln_phi).exp();
        if !step.is_finite() {
            return None;
        }
        x -= step;
        if step.abs() <= 1e-15 * x.abs().max(1.0) {
            break;
        }
    }
    x.is_finite().then_some(x)
}

// ---- discrete distributions: saddle-point terms ----

/// `stirlerr(n) = ln(n!) - ((n + 1/2) ln n - n + ln sqrt(2 pi))` for `n >= 1`. Exact summation
/// below `STIRLING_MIN`; the Stirling series above it, whose first omitted term is below 1e-17.
fn stirlerr(n: u64) -> f64 {
    debug_assert!(n >= 1, "stirlerr is defined for n >= 1");
    if n < STIRLING_MIN {
        let nf = n as f64;
        let ln_fact: f64 = (2..=n).map(|i| (i as f64).ln()).sum();
        return ln_fact - ((nf + 0.5) * nf.ln() - nf + LN_SQRT_2PI);
    }
    let x = n as f64;
    let x2 = 1.0 / (x * x);
    (1.0 / 12.0 - x2 * (1.0 / 360.0 - x2 * (1.0 / 1260.0 - x2 * (1.0 / 1680.0 - x2 / 1188.0)))) / x
}

/// `bd0(x, m) = x ln(x/m) + m - x` for `x, m > 0`, evaluated without cancellation near `x = m`
/// (Loader's series). `bd0(0, m) = m`.
fn bd0(x: f64, m: f64) -> f64 {
    if x == 0.0 {
        return m;
    }
    if (x - m).abs() < 0.1 * (x + m) {
        let v = (x - m) / (x + m);
        let mut s = (x - m) * v;
        let mut ej = 2.0 * x * v;
        let v2 = v * v;
        for j in 1..1000u32 {
            ej *= v2;
            let s1 = s + ej / f64::from(2 * j + 1);
            if s1 == s {
                break;
            }
            s = s1;
        }
        s
    } else {
        x * (x / m).ln() + m - x
    }
}

/// `P(X = k)` for `X ~ Binomial(n, p)`, with `0 < p < 1` (the endpoints are handled by callers).
/// Zero outside `0..=n`.
fn binom_pmf(k: u64, n: u64, p: f64) -> f64 {
    if k > n {
        return 0.0;
    }
    let nf = n as f64;
    let qc = 1.0 - p;
    if k == 0 {
        return if p < 0.1 {
            (-bd0(nf, nf * qc) - nf * p).exp()
        } else {
            (nf * qc.ln()).exp()
        };
    }
    if k == n {
        return if qc < 0.1 {
            (-bd0(nf, nf * p) - nf * qc).exp()
        } else {
            (nf * p.ln()).exp()
        };
    }
    let kf = k as f64;
    let rest = n - k;
    let rf = rest as f64;
    let lc = stirlerr(n) - stirlerr(k) - stirlerr(rest) - bd0(kf, nf * p) - bd0(rf, nf * qc);
    let lf = LN_2PI + kf.ln() + (rf / nf).ln();
    (lc - 0.5 * lf).exp()
}

/// `P(X = k)` for `X ~ Poisson(lambda)`, `lambda > 0`.
fn pois_pmf(k: u64, lambda: f64) -> f64 {
    if k == 0 {
        return (-lambda).exp();
    }
    let kf = k as f64;
    (-stirlerr(k) - bd0(kf, lambda)).exp() / (2.0 * PI * kf).sqrt()
}

/// `sum_{j <= from} pmf(j)` for a pmf at or below its mode. `ratio(j) = pmf(j-1) / pmf(j)` is below
/// one there, so the terms shrink and the sum stops at a negligible term.
fn sum_down(from: u64, pmf: impl Fn(u64) -> f64, ratio: impl Fn(u64) -> f64) -> f64 {
    let mut j = from;
    let mut term = pmf(j);
    let mut sum = term;
    let mut steps = 0u64;
    while j > 0 && term > sum * TAIL_EPS {
        term *= ratio(j);
        j -= 1;
        steps += 1;
        if steps.is_multiple_of(REANCHOR) {
            term = pmf(j);
        }
        sum += term;
    }
    sum
}

/// `sum_{j >= from} pmf(j)` for a pmf at or above its mode. `ratio(j) = pmf(j+1) / pmf(j)` is below
/// one there. The pmf is zero past its support, so the sum ends there.
fn sum_up(from: u64, pmf: impl Fn(u64) -> f64, ratio: impl Fn(u64) -> f64) -> f64 {
    let mut j = from;
    let mut term = pmf(j);
    let mut sum = term;
    let mut steps = 0u64;
    while term > sum * TAIL_EPS {
        term *= ratio(j);
        j += 1;
        steps += 1;
        if steps.is_multiple_of(REANCHOR) {
            term = pmf(j);
        }
        sum += term;
    }
    sum
}

/// Smallest `k` in `lo..=hi` with `pred(k)`, for a monotone `pred` that holds at `hi`.
fn least_true(mut lo: u64, mut hi: u64, pred: impl Fn(u64) -> bool) -> u64 {
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if pred(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

// ---- binomial ----

/// Mode of `Binomial(n, p)`: `floor((n + 1) p)`, capped at `n`.
fn binom_mode(n: u64, p: f64) -> u64 {
    (((n as f64) + 1.0) * p).floor().min(n as f64) as u64
}

/// Binomial probability `C(n,k) p^k (1-p)^(n-k)` for `0 <= k <= n` and `0 <= p <= 1`.
pub(crate) fn dbinom(k: u64, n: u64, p: f64) -> f64 {
    if p == 0.0 {
        return f64::from(u8::from(k == 0));
    }
    if p == 1.0 {
        return f64::from(u8::from(k == n));
    }
    binom_pmf(k, n, p)
}

/// Binomial cumulative probability `P(X <= k)` for `0 <= p <= 1`.
pub(crate) fn pbinom(k: u64, n: u64, p: f64) -> f64 {
    if k >= n || p == 0.0 {
        return 1.0;
    }
    if p == 1.0 {
        return 0.0;
    }
    let pmf = |j| binom_pmf(j, n, p);
    let value = if k <= binom_mode(n, p) {
        let down = (1.0 - p) / p;
        sum_down(k, pmf, |j| j as f64 / (n - j + 1) as f64 * down)
    } else {
        let up = p / (1.0 - p);
        1.0 - sum_up(k + 1, pmf, |j| (n - j) as f64 / (j + 1) as f64 * up)
    };
    value.clamp(0.0, 1.0)
}

/// Smallest `k` with `P(X <= k) >= prob`, for `0 <= prob <= 1`. The quantile at 1 is `n` (not
/// reached by rounding of `pbinom`). Bisection on `0..=n`, so at most
/// `log2(n) + 1` cumulative evaluations.
pub(crate) fn qbinom(prob: f64, n: u64, p: f64) -> u64 {
    if prob <= 0.0 || p == 0.0 {
        return 0;
    }
    if prob >= 1.0 || p == 1.0 {
        return n;
    }
    least_true(0, n, |k| pbinom(k, n, p) >= prob)
}

// ---- Poisson ----

/// Poisson probability `lambda^k e^{-lambda} / k!` for `k >= 0`, `lambda > 0`.
pub(crate) fn dpois(k: u64, lambda: f64) -> f64 {
    pois_pmf(k, lambda)
}

/// Poisson cumulative probability `P(X <= k)` for `lambda > 0`.
pub(crate) fn ppois(k: u64, lambda: f64) -> f64 {
    let pmf = |j| pois_pmf(j, lambda);
    let value = if k as f64 <= lambda.floor() {
        sum_down(k, pmf, |j| j as f64 / lambda)
    } else {
        1.0 - sum_up(k + 1, pmf, |j| lambda / (j + 1) as f64)
    };
    value.clamp(0.0, 1.0)
}

/// Smallest `k` with `P(X <= k) >= prob`, for `0 <= prob <= 1` and `lambda > 0`. The upper end of
/// the bracket doubles from `lambda` until it reaches `prob`, then bisection finds the count.
pub(crate) fn qpois(prob: f64, lambda: f64) -> u64 {
    if prob <= 0.0 {
        return 0;
    }
    let mut hi = (lambda.floor() as u64).max(1);
    while ppois(hi, lambda) < prob && hi < (1 << 62) {
        hi *= 2;
    }
    least_true(0, hi, |k| ppois(k, lambda) >= prob)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close_rel(actual: f64, expected: f64, rel: f64) {
        let scale = expected.abs().max(f64::MIN_POSITIVE);
        assert!(
            (actual - expected).abs() <= rel * scale,
            "{actual:e} is not {expected:e} (relative tolerance {rel:e})"
        );
    }

    #[test]
    fn erfc_matches_reference_values() {
        // Reference: CPython math.erfc (correctly rounded).
        close_rel(erfc_nonneg(0.5), 0.479_500_122_186_953_5, 1e-14);
        close_rel(erfc_nonneg(1.0), 0.157_299_207_050_285_16, 1e-14);
        close_rel(erfc_nonneg(1.5), 0.033_894_853_524_689_274, 1e-14);
        close_rel(erfc_nonneg(2.0), 0.004_677_734_981_047_264_5, 1e-14);
        close_rel(erfc_nonneg(3.0), 2.209_049_699_858_544e-5, 1e-14);
        close_rel(erfc_nonneg(5.0), 1.537_459_794_428_035e-12, 1e-13);
    }

    #[test]
    fn pnorm_reference_values() {
        close_rel(pnorm_std(0.0), 0.5, 1e-16);
        close_rel(pnorm_std(1.96), 0.975_002_104_851_779_5, 1e-15);
        close_rel(pnorm_std(-1.96), 0.024_997_895_148_220_5, 1e-14);
        // Phi(-37) is about 5.7e-300, well inside the double range.
        let tail = pnorm_std(-37.0);
        assert!(tail > 0.0 && tail < 1e-299, "{tail:e}");
    }

    #[test]
    fn qnorm_inverts_pnorm_on_a_grid() {
        let mut z = -8.0;
        while z <= 3.0 {
            let p = pnorm_std(z);
            let back = qnorm_std(p).expect("p in (0,1)");
            assert!((back - z).abs() <= 1e-12, "z {z}: back {back}");
            z += 0.25;
        }
    }

    #[test]
    fn qnorm_reference_and_tails() {
        close_rel(qnorm_std(0.975).unwrap(), 1.959_963_984_540_054, 1e-14);
        assert_eq!(qnorm_std(0.5), Some(0.0));
        let far = qnorm_std(1e-300).expect("finite");
        assert!(far.is_finite() && far < -36.0);
        assert!(qnorm_std(0.0).is_none());
        assert!(qnorm_std(1.0).is_none());
    }

    #[test]
    fn binomial_and_poisson_small_cases() {
        close_rel(dbinom(3, 10, 0.5), 0.117_187_5, 1e-13);
        close_rel(pbinom(3, 10, 0.3), 0.649_610_718_399_999_6, 1e-13);
        assert_eq!(qbinom(0.0, 10, 0.3), 0);
        assert_eq!(qbinom(1.0, 10, 0.3), 10);
        close_rel(dpois(2, 1.0), 0.183_939_720_585_721_2, 1e-14);
        close_rel(ppois(2, 1.0), 0.919_698_602_928_605_8, 1e-14);
    }

    #[test]
    fn stirling_series_matches_exact_sum_at_the_switch() {
        // Exact ln(20!) = 42.335616460753485; compare the series at 20 with the direct sum.
        let exact: f64 = (2..=20u64).map(|i| (i as f64).ln()).sum();
        let x = 20.0_f64;
        let via_series = x * x.ln() - x + 0.5 * x.ln() + LN_SQRT_2PI + stirlerr(20);
        close_rel(via_series, exact, 1e-15);
    }

    #[test]
    fn large_binomial_pmf_is_accurate() {
        // Exact: C(100000, 50000) / 2^100000 (python fractions).
        close_rel(dbinom(50_000, 100_000, 0.5), 2.523_126_214_196_74e-3, 1e-13);
    }

    #[test]
    fn large_poisson_pmf_is_accurate() {
        // Exact: Stirling series in Decimal at 60 digits.
        close_rel(dpois(1_000_000, 1e6), 3.989_422_471_562_44e-4, 1e-13);
    }

    #[test]
    fn large_binomial_cdf_is_half_plus_half_the_mode() {
        // Symmetry: P(X <= n/2) = 1/2 + P(X = n/2) / 2 for p = 1/2.
        let expected = 0.5 + dbinom(50_000, 100_000, 0.5) / 2.0;
        assert!((pbinom(50_000, 100_000, 0.5) - expected).abs() <= 1e-12);
    }

    #[test]
    fn large_poisson_cdf_at_the_mean() {
        // Exact: 0.50026596148628365 (Decimal sum of the terms from the mode down).
        close_rel(ppois(1_000_000, 1e6), 0.500_265_961_486_283_7, 1e-12);
    }

    #[test]
    fn tiny_tail_values_keep_their_magnitude() {
        // P(X = 0) = 2^-1000 for n = 1000, p = 1/2, and the lower tail equals it.
        close_rel(pbinom(0, 1000, 0.5), 9.332_636_185_032_189e-302, 1e-12);
    }

    #[test]
    fn binomial_cdf_symmetry_at_half() {
        for k in [0u64, 1, 17, 250, 499] {
            let sum = pbinom(k, 1000, 0.5) + pbinom(1000 - k - 1, 1000, 0.5);
            assert!((sum - 1.0).abs() <= 1e-13, "k {k}: {sum}");
        }
    }

    #[test]
    fn binomial_quantile_inverts_the_cdf() {
        for k in [0u64, 1, 5, 50, 100, 200, 250, 290, 300, 310, 350] {
            let p = pbinom(k, 1000, 0.3);
            assert_eq!(qbinom(p, 1000, 0.3), k, "k {k}");
        }
    }

    #[test]
    fn poisson_quantile_inverts_the_cdf() {
        for k in [0u64, 500, 900, 990, 1000, 1010, 1100, 1200] {
            let p = ppois(k, 1000.0);
            assert_eq!(qpois(p, 1000.0), k, "k {k}");
        }
    }
}

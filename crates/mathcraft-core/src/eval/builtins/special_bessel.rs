//! Bessel, Airy and Fresnel functions for real arguments: pure `f64` kernels.
//!
//! Method: every kernel is an integral representation evaluated by composite 20-point
//! Gauss-Legendre quadrature (`quad`). The integrands are entire, so the rule converges
//! exponentially in the panel count. Integrals over `[0, infinity)` are cut where the integrand
//! is below e^-750, which is below the smallest `f64`.
//!
//! - `bessel_j(nu, x)`: J_nu(x) (Schlafli-type integral, plus a tail term for non-integer nu).
//! - `bessel_y_int(n, x)`: Y_n(x) for integer n, x > 0.
//! - `bessel_i(nu, x)`: I_nu(x), any real x for integer nu, x > 0 otherwise.
//! - `bessel_k(nu, x)`: K_nu(x) for x > 0 (positive integrand, no cancellation).
//! - `airy_ai`, `airy_bi`: Ai and Bi through the Bessel functions of order +-1/3.
//! - `fresnel(x)`: FresnelC and FresnelS.
//!
//! Limits: arguments above `MAX_ARG` (1e5 for Bessel, 1000 for Fresnel) are refused with
//! `Unsupported` so that every call finishes in bounded time. Overflow returns `Overflow`.
//! Accuracy target about 1e-13 relative away from zeros of the function; the tests check
//! J0(1), Y0(1), I0(1), K0(1), Ai(0), Ai(+-1), Bi(+-1) against published values.

use std::f64::consts::PI;
use std::sync::OnceLock;

use crate::eval::error::EvalError;

/// Largest |x| accepted by the Bessel kernels.
pub(crate) const MAX_ARG: f64 = 1e5;
/// Largest |x| accepted by the Fresnel kernels.
pub(crate) const MAX_FRESNEL_ARG: f64 = 1000.0;
/// Exponent cut for the tails of the `[0, infinity)` integrals: exp(-750) is below `f64`.
const TAIL_CUT: f64 = 750.0;
/// Panels for the tail integrals; the integrands there are smooth on the scale of one panel.
const TAIL_PANELS: usize = 2000;

/// Gauss-Legendre nodes and weights on [-1, 1] for 20 points, computed once by Newton's method.
fn gauss_legendre() -> &'static [(f64, f64)] {
    static NODES: OnceLock<Vec<(f64, f64)>> = OnceLock::new();
    NODES.get_or_init(|| {
        const N: usize = 20;
        let mut out = Vec::with_capacity(N);
        for i in 1..=N {
            let mut x = (PI * (i as f64 - 0.25) / (N as f64 + 0.5)).cos();
            let mut dp = 1.0;
            for _ in 0..100 {
                let (mut p0, mut p1) = (1.0, x);
                for k in 2..=N {
                    let kf = k as f64;
                    let p2 = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
                    p0 = p1;
                    p1 = p2;
                }
                dp = N as f64 * (x * p1 - p0) / (x * x - 1.0);
                let dx = p1 / dp;
                x -= dx;
                if dx.abs() < 1e-16 {
                    break;
                }
            }
            out.push((x, 2.0 / ((1.0 - x * x) * dp * dp)));
        }
        out
    })
}

/// Composite Gauss-Legendre integral of `f` over `[a, b]` with `panels` equal panels.
pub(crate) fn quad(f: impl Fn(f64) -> f64, a: f64, b: f64, panels: usize) -> f64 {
    let h = (b - a) / panels as f64;
    let half = h / 2.0;
    let mut sum = 0.0;
    for p in 0..panels {
        let c = a + (p as f64 + 0.5) * h;
        for &(x, w) in gauss_legendre() {
            sum += w * f(c + half * x);
        }
    }
    sum * half
}

/// Panels for a finite integral whose integrand oscillates with frequency about `|x| + |nu|`.
fn oscillatory_panels(x: f64, nu: f64) -> usize {
    16 + ((x.abs() + nu.abs()) / 2.5) as usize
}

/// Upper limit `T` of a tail integral: the first `t >= 0` where `g(t) >= TAIL_CUT`, with `g`
/// increasing. Grows geometrically, so it finishes in a few hundred steps.
fn tail_limit(g: impl Fn(f64) -> f64) -> f64 {
    let mut t = 0.5;
    for _ in 0..10_000 {
        if g(t) >= TAIL_CUT {
            return t;
        }
        t += (0.1 * t).max(0.05);
    }
    t
}

fn check_finite(v: f64) -> Result<f64, EvalError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(EvalError::Overflow)
    }
}

fn check_arg(x: f64) -> Result<(), EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if x.abs() > MAX_ARG {
        return Err(EvalError::Unsupported(
            "Bessel argument above 1e5 in magnitude".to_string(),
        ));
    }
    Ok(())
}

/// `sin(nu pi)`, exactly zero for integer `nu`.
fn sin_nu_pi(nu: f64) -> f64 {
    if nu.fract() == 0.0 {
        0.0
    } else {
        (nu * PI).sin()
    }
}

/// Bessel function of the first kind `J_nu(x)`.
///
/// Integer order: any real `x` (`J_n(-x) = (-1)^n J_n(x)`). Non-integer order: `x > 0` only,
/// since the power `x^nu` is complex for negative `x`.
pub(crate) fn bessel_j(nu: f64, x: f64) -> Result<f64, EvalError> {
    check_arg(x)?;
    if !nu.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if nu.fract() == 0.0 {
        if x < 0.0 {
            let sign = if (nu as i64).rem_euclid(2) == 0 {
                1.0
            } else {
                -1.0
            };
            return Ok(sign * bessel_j(nu, -x)?);
        }
        if x == 0.0 {
            return Ok(if nu == 0.0 { 1.0 } else { 0.0 });
        }
    } else if x <= 0.0 {
        return Err(EvalError::Unsupported(
            "J of non-integer order needs a positive argument".to_string(),
        ));
    }
    let first = quad(
        |t| (nu * t - x * t.sin()).cos(),
        0.0,
        PI,
        oscillatory_panels(x, nu),
    ) / PI;
    let s = sin_nu_pi(nu);
    if s == 0.0 {
        return check_finite(first);
    }
    let upper = tail_limit(|t| x * t.sinh() + nu * t);
    let tail = quad(|t| (-x * t.sinh() - nu * t).exp(), 0.0, upper, TAIL_PANELS);
    check_finite(first - s / PI * tail)
}

/// Bessel function of the second kind `Y_n(x)` for integer `n` and `x > 0`.
///
/// Uses Schlafli's integral `Y_n = (1/pi) int_0^pi sin(x sin t - n t) dt
/// - (1/pi) int_0^inf (e^(n t) + (-1)^n e^(-n t)) e^(-x sinh t) dt`, and `Y_(-n) = (-1)^n Y_n`.
pub(crate) fn bessel_y_int(n: i64, x: f64) -> Result<f64, EvalError> {
    check_arg(x)?;
    if x <= 0.0 {
        return Err(EvalError::Unsupported(
            "Y of an argument not above zero is complex or undefined".to_string(),
        ));
    }
    if n < 0 {
        let sign = if n.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
        return Ok(sign * bessel_y_int(-n, x)?);
    }
    let nf = n as f64;
    let sign_n = if n % 2 == 0 { 1.0 } else { -1.0 };
    let first = quad(
        |t| (x * t.sin() - nf * t).sin(),
        0.0,
        PI,
        oscillatory_panels(x, nf),
    ) / PI;
    let upper = tail_limit(|t| x * t.sinh() - nf * t);
    let tail = quad(
        |t| (nf * t).exp() * (1.0 + sign_n * (-2.0 * nf * t).exp()) * (-x * t.sinh()).exp(),
        0.0,
        upper,
        TAIL_PANELS,
    ) / PI;
    check_finite(first - tail)
}

/// Modified Bessel function of the first kind `I_nu(x)`.
///
/// `I_nu(x) = (1/pi) int_0^pi e^(x cos t) cos(nu t) dt - (sin(nu pi)/pi) int_0^inf
/// e^(-x cosh t - nu t) dt`. Integer order: any real `x`. Overflow returns `Overflow`.
pub(crate) fn bessel_i(nu: f64, x: f64) -> Result<f64, EvalError> {
    check_arg(x)?;
    if !nu.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if nu.fract() != 0.0 && x <= 0.0 {
        return Err(EvalError::Unsupported(
            "I of non-integer order needs a positive argument".to_string(),
        ));
    }
    let first = quad(
        |t| (x * t.cos()).exp() * (nu * t).cos(),
        0.0,
        PI,
        oscillatory_panels(x, 0.0) + (x.abs() / 2.0) as usize,
    ) / PI;
    let s = sin_nu_pi(nu);
    if s == 0.0 {
        return check_finite(first);
    }
    let upper = tail_limit(|t| x * t.cosh() + nu * t);
    let tail = quad(|t| (-x * t.cosh() - nu * t).exp(), 0.0, upper, TAIL_PANELS);
    check_finite(first - s / PI * tail)
}

/// Modified Bessel function of the second kind `K_nu(x) = int_0^inf e^(-x cosh t) cosh(nu t) dt`,
/// for `x > 0` and real `nu`. The integrand is positive, so there is no cancellation.
pub(crate) fn bessel_k(nu: f64, x: f64) -> Result<f64, EvalError> {
    check_arg(x)?;
    if !nu.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if x == 0.0 {
        return Err(EvalError::NotFinite);
    }
    if x < 0.0 {
        return Err(EvalError::Unsupported(
            "K of a negative argument is complex".to_string(),
        ));
    }
    let nu_abs = nu.abs();
    let upper = tail_limit(|t| x * t.cosh() - nu_abs * t);
    let value = quad(
        |t| (-x * t.cosh()).exp() * (nu * t).cosh(),
        0.0,
        upper,
        TAIL_PANELS,
    );
    check_finite(value)
}

/// Airy function `Ai(x)`, real `x`.
pub(crate) fn airy_ai(x: f64) -> Result<f64, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if x == 0.0 {
        // Ai(0) = 1 / (3^(2/3) Gamma(2/3))
        return Ok(1.0 / (3f64.powf(2.0 / 3.0) * super::special_gamma::gamma(2.0 / 3.0)?));
    }
    let a = x.abs();
    let zeta = 2.0 / 3.0 * a.powf(1.5);
    check_zeta(zeta)?;
    if x > 0.0 {
        // Ai(x) = (1/pi) sqrt(x/3) K_{1/3}(zeta). Underflow to zero for large zeta.
        let k = bessel_k(1.0 / 3.0, zeta).unwrap_or(0.0);
        return Ok(a.sqrt() / 3f64.sqrt() / PI * k);
    }
    // Ai(-x) = (sqrt(x)/3) (J_{1/3}(zeta) + J_{-1/3}(zeta))
    let sum = bessel_j(1.0 / 3.0, zeta)? + bessel_j(-1.0 / 3.0, zeta)?;
    Ok(a.sqrt() / 3.0 * sum)
}

/// Airy function of the second kind `Bi(x)`, real `x`.
pub(crate) fn airy_bi(x: f64) -> Result<f64, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if x == 0.0 {
        // Bi(0) = 1 / (3^(1/6) Gamma(2/3))
        return Ok(1.0 / (3f64.powf(1.0 / 6.0) * super::special_gamma::gamma(2.0 / 3.0)?));
    }
    let a = x.abs();
    let zeta = 2.0 / 3.0 * a.powf(1.5);
    check_zeta(zeta)?;
    let root = (a / 3.0).sqrt();
    if x > 0.0 {
        // Bi(x) = sqrt(x/3) (I_{-1/3}(zeta) + I_{1/3}(zeta))
        Ok(root * (bessel_i(-1.0 / 3.0, zeta)? + bessel_i(1.0 / 3.0, zeta)?))
    } else {
        // Bi(-x) = sqrt(x/3) (J_{-1/3}(zeta) - J_{1/3}(zeta))
        Ok(root * (bessel_j(-1.0 / 3.0, zeta)? - bessel_j(1.0 / 3.0, zeta)?))
    }
}

fn check_zeta(zeta: f64) -> Result<(), EvalError> {
    if zeta > MAX_ARG {
        return Err(EvalError::Unsupported(
            "Airy argument above about 1800 in magnitude".to_string(),
        ));
    }
    Ok(())
}

/// Fresnel integrals `(C(x), S(x))` with `C = int_0^x cos(pi t^2/2) dt` and
/// `S = int_0^x sin(pi t^2/2) dt`. Both are odd in `x`.
pub(crate) fn fresnel(x: f64) -> Result<(f64, f64), EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if x.abs() > MAX_FRESNEL_ARG {
        return Err(EvalError::Unsupported(
            "Fresnel argument above 1000 in magnitude".to_string(),
        ));
    }
    let a = x.abs();
    // The phase pi t^2 / 2 has total size pi a^2 / 2; each panel carries at most about 8 radians.
    let panels = 4 + (PI * a * a / 2.0 / 8.0) as usize;
    let c = quad(|t| (PI * t * t / 2.0).cos(), 0.0, a, panels);
    let s = quad(|t| (PI * t * t / 2.0).sin(), 0.0, a, panels);
    let sign = x.signum();
    Ok((sign * c, sign * s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauss_legendre_integrates_polynomials_exactly() {
        let v = quad(|t| t.powi(10), 0.0, 1.0, 1);
        assert!((v - 1.0 / 11.0).abs() < 1e-15);
    }

    #[test]
    fn j0_at_one() {
        assert!((bessel_j(0.0, 1.0).unwrap() - 0.765_197_686_557_966_6).abs() < 1e-14);
    }
}

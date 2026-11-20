//! Orthogonal polynomials, Lambert W, complete and incomplete elliptic integrals, and the
//! Kronecker delta and Heaviside step: pure `f64` kernels.
//!
//! - Polynomials use three-term recurrences; degree `n` is capped at `MAX_DEGREE`.
//! - `lambert_w(branch, x)`: real branches 0 and -1 only (help: real for `-1/e <= x`).
//! - `elliptic_k`, `elliptic_e`: complete integrals with the AGM; `m < 1` (real `m`).
//! - `elliptic_f`, `elliptic_e_incomplete`: incomplete integrals by quadrature, `m < 1`, `0 <= z <= 1`.

use std::f64::consts::{E, PI};

use super::special_bessel::quad;
use crate::eval::error::EvalError;

/// Largest polynomial degree accepted; each recurrence step is O(1), so this bounds the loop.
pub(crate) const MAX_DEGREE: i64 = 1_000_000;

/// Which orthogonal polynomial a recurrence computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Poly {
    Hermite,
    Laguerre,
    Legendre,
    ChebyshevT,
    ChebyshevU,
}

/// Degree check shared by the polynomial builtins.
pub(crate) fn check_degree(n: f64) -> Result<usize, EvalError> {
    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n > MAX_DEGREE as f64 {
        return Err(EvalError::Unsupported(
            "polynomial degree must be a nonnegative integer up to 1e6".to_string(),
        ));
    }
    Ok(n as usize)
}

/// `P_n(x)` for the classical families. Errors with `Overflow` if the value leaves `f64`.
pub(crate) fn polynomial(kind: Poly, n: usize, x: f64) -> Result<f64, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    // Starting values p_0 and p_1 of each family.
    let (p0, p1) = match kind {
        Poly::Hermite | Poly::ChebyshevU => (1.0, 2.0 * x),
        Poly::ChebyshevT | Poly::Legendre => (1.0, x),
        Poly::Laguerre => (1.0, 1.0 - x),
    };
    if n == 0 {
        return Ok(p0);
    }
    let (mut prev, mut cur) = (p0, p1);
    for k in 1..n {
        let kf = k as f64;
        let next = match kind {
            Poly::Hermite => 2.0 * x * cur - 2.0 * kf * prev,
            Poly::ChebyshevT | Poly::ChebyshevU => 2.0 * x * cur - prev,
            Poly::Laguerre => ((2.0 * kf + 1.0 - x) * cur - kf * prev) / (kf + 1.0),
            Poly::Legendre => ((2.0 * kf + 1.0) * x * cur - kf * prev) / (kf + 1.0),
        };
        prev = cur;
        cur = next;
    }
    if cur.is_finite() {
        Ok(cur)
    } else {
        Err(EvalError::Overflow)
    }
}

/// Jacobi polynomial `P_n^(a,b)(x)` with `a, b > -1`, by the three-term recurrence (A&S 22.7.1).
pub(crate) fn jacobi(n: usize, a: f64, b: f64, x: f64) -> Result<f64, EvalError> {
    if !(a > -1.0 && b > -1.0 && a.is_finite() && b.is_finite() && x.is_finite()) {
        return Err(EvalError::Unsupported(
            "Jacobi parameters a and b must be greater than -1".to_string(),
        ));
    }
    if n == 0 {
        return Ok(1.0);
    }
    let mut prev = 1.0;
    let mut cur = ((a - b) + (a + b + 2.0) * x) / 2.0;
    for k in 2..=n {
        let kf = k as f64;
        let s = a + b;
        let c1 = 2.0 * kf * (kf + s) * (2.0 * kf + s - 2.0);
        let c2 = (2.0 * kf + s - 1.0) * ((2.0 * kf + s) * (2.0 * kf + s - 2.0) * x + a * a - b * b);
        let c3 = 2.0 * (kf + a - 1.0) * (kf + b - 1.0) * (2.0 * kf + s);
        let next = (c2 * cur - c3 * prev) / c1;
        prev = cur;
        cur = next;
    }
    if cur.is_finite() {
        Ok(cur)
    } else {
        Err(EvalError::Overflow)
    }
}

/// Lambert W function, real branches: `branch` 0 (principal, `x >= -1/e`) or -1
/// (`-1/e <= x < 0`). Halley iteration from a branch-point or asymptotic start.
pub(crate) fn lambert_w(branch: i64, x: f64) -> Result<f64, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    let edge = -1.0 / E;
    if x < edge {
        return Err(EvalError::Unsupported(
            "LambertW has no real value below -1/e".to_string(),
        ));
    }
    match branch {
        0 => {
            if x == edge {
                return Ok(-1.0);
            }
            if x == 0.0 {
                return Ok(0.0);
            }
            let start = if x < -0.3 {
                let p = (2.0 * (E * x + 1.0)).max(0.0).sqrt();
                -1.0 + p
            } else if x < 3.0 {
                (1.0 + x).ln()
            } else {
                let l = x.ln();
                l - l.ln()
            };
            Ok(halley(start, x))
        }
        -1 => {
            if x >= 0.0 {
                return Err(EvalError::Unsupported(
                    "LambertW branch -1 is real only for -1/e <= x < 0".to_string(),
                ));
            }
            if x == edge {
                return Ok(-1.0);
            }
            let start = if x > -0.3 {
                let l = (-x).ln();
                l - (-l).ln()
            } else {
                -1.0 - (2.0 * (E * x + 1.0)).max(0.0).sqrt()
            };
            Ok(halley(start, x))
        }
        _ => Err(EvalError::Unsupported(
            "LambertW is implemented for the real branches 0 and -1 only".to_string(),
        )),
    }
}

/// Halley iteration for `w e^w = x`. Stops when the step is below rounding; capped at 100 steps.
fn halley(start: f64, x: f64) -> f64 {
    let mut w = start;
    for _ in 0..100 {
        let ew = w.exp();
        let f = w * ew - x;
        let w1 = w + 1.0;
        if w1 == 0.0 {
            break;
        }
        let dw = f / (ew * w1 - (w + 2.0) * f / (2.0 * w1));
        w -= dw;
        if dw.abs() <= 4.0 * f64::EPSILON * (1.0 + w.abs()) {
            break;
        }
    }
    w
}

/// Complete elliptic integrals `(K(m), E(m))` for real `m < 1`, by the arithmetic-geometric mean.
/// `E = K (1 - sum_{n>=0} 2^(n-1) c_n^2)` with `c_0^2 = m` and `c_{n+1} = (a_n - b_n)/2`.
pub(crate) fn elliptic_ke(m: f64) -> Result<(f64, f64), EvalError> {
    if !m.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if m >= 1.0 {
        return Err(EvalError::Unsupported(
            "complete elliptic integrals are implemented for m < 1 only".to_string(),
        ));
    }
    let mut a = 1.0;
    let mut b = (1.0 - m).sqrt();
    let mut sum = 0.5 * m;
    let mut pow = 1.0;
    for _ in 0..100 {
        let c = (a - b) / 2.0;
        // Stop once c is at rounding level: below it the terms are noise, and `pow` doubles.
        if c.abs() <= 4.0 * f64::EPSILON * a {
            break;
        }
        sum += pow * c * c;
        pow *= 2.0;
        let (an, bn) = ((a + b) / 2.0, (a * b).sqrt());
        a = an;
        b = bn;
    }
    let k = PI / (2.0 * a);
    Ok((k, k * (1.0 - sum)))
}

/// Incomplete elliptic integrals `F(z, m)` and `E(z, m)` for `0 <= z <= 1`, `m < 1`, written in
/// the angle `t = asin z`: `F = int_0^t dt / sqrt(1 - m sin^2 t)`, `E = int_0^t sqrt(...) dt`.
pub(crate) fn elliptic_incomplete(z: f64, m: f64) -> Result<(f64, f64), EvalError> {
    if !z.is_finite() || !m.is_finite() {
        return Err(EvalError::NotFinite);
    }
    if !(0.0..=1.0).contains(&z) || m >= 1.0 {
        return Err(EvalError::Unsupported(
            "incomplete elliptic integrals need 0 <= z <= 1 and m < 1".to_string(),
        ));
    }
    let t = z.asin();
    let panels = 8 + (m.abs() * 16.0) as usize;
    let f = quad(|s| 1.0 / (1.0 - m * s.sin().powi(2)).sqrt(), 0.0, t, panels);
    let e = quad(|s| (1.0 - m * s.sin().powi(2)).sqrt(), 0.0, t, panels);
    Ok((f, e))
}

/// Kronecker delta: 1 when `m == n`, otherwise 0. Arguments are integers.
pub(crate) fn kronecker(m: f64, n: f64) -> f64 {
    if m == n {
        1.0
    } else {
        0.0
    }
}

/// Heaviside step: 0 below zero, 1 above zero, 0.5 at zero.
pub(crate) fn heaviside(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        0.0
    } else {
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legendre_p2_matches_formula() {
        let x = 0.3;
        let got = polynomial(Poly::Legendre, 2, x).unwrap();
        assert!((got - (3.0 * x * x - 1.0) / 2.0).abs() < 1e-15);
    }

    #[test]
    fn lambert_w_branch_zero_at_one() {
        // W(1) = 0.5671432904097838 (omega constant)
        assert!((lambert_w(0, 1.0).unwrap() - 0.567_143_290_409_783_8).abs() < 1e-15);
    }

    #[test]
    fn elliptic_k_at_zero_is_half_pi() {
        let (k, e) = elliptic_ke(0.0).unwrap();
        assert!((k - PI / 2.0).abs() < 1e-15);
        assert!((e - PI / 2.0).abs() < 1e-15);
    }
}

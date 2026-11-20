//! Scalar kernels: checked arithmetic on one [`Scalar`] at a time, with the unit rules and the
//! complex division that avoids underflow. The element-wise and matrix plumbing lives in `ops`.

use num_complex::Complex64;

use crate::eval::error::{from_units, EvalError};
use crate::eval::ops::{require_dimensionless, CmpOp};
use crate::eval::value::Scalar;
use crate::units::{Dimension, Exp};

/// `Ok(v)` when both parts are finite; otherwise `NotFinite`.
pub(super) fn checked(v: Complex64) -> Result<Complex64, EvalError> {
    if v.re.is_finite() && v.im.is_finite() {
        Ok(v)
    } else {
        Err(EvalError::NotFinite)
    }
}

fn is_zero(v: Complex64) -> bool {
    v.re == 0.0 && v.im == 0.0
}

/// Quotient `a / b` of complex numbers without underflow. `Complex64`'s own `/` divides by
/// `|b|^2`, which underflows to zero for tiny divisors (`1e-200 / 1e-200` would be infinite).
/// A real divisor divides the parts directly; otherwise Smith's algorithm scales by the larger
/// part of the divisor. The divisor must be non-zero.
pub(super) fn cdiv(a: Complex64, b: Complex64) -> Complex64 {
    if b.im == 0.0 {
        return Complex64::new(a.re / b.re, a.im / b.re);
    }
    if b.re.abs() >= b.im.abs() {
        let r = b.im / b.re;
        let d = b.re + b.im * r;
        Complex64::new((a.re + a.im * r) / d, (a.im - a.re * r) / d)
    } else {
        let r = b.re / b.im;
        let d = b.im + b.re * r;
        Complex64::new((a.re * r + a.im) / d, (a.im * r - a.re) / d)
    }
}

fn same_dim(a: &Scalar, b: &Scalar) -> Result<(), EvalError> {
    if a.dim == b.dim {
        Ok(())
    } else {
        Err(EvalError::DimensionMismatch {
            left: a.dim,
            right: b.dim,
        })
    }
}

pub(super) fn s_add(a: &Scalar, b: &Scalar) -> Result<Scalar, EvalError> {
    same_dim(a, b)?;
    Ok(Scalar::new(checked(a.value + b.value)?, a.dim))
}

pub(super) fn s_sub(a: &Scalar, b: &Scalar) -> Result<Scalar, EvalError> {
    same_dim(a, b)?;
    Ok(Scalar::new(checked(a.value - b.value)?, a.dim))
}

pub(super) fn s_mul(a: &Scalar, b: &Scalar) -> Result<Scalar, EvalError> {
    let dim = a.dim.checked_mul(b.dim).map_err(from_units)?;
    Ok(Scalar::new(checked(a.value * b.value)?, dim))
}

pub(super) fn s_div(a: &Scalar, b: &Scalar) -> Result<Scalar, EvalError> {
    if is_zero(b.value) {
        return Err(EvalError::DivisionByZero);
    }
    let dim = a.dim.checked_div(b.dim).map_err(from_units)?;
    Ok(Scalar::new(checked(cdiv(a.value, b.value))?, dim))
}

pub(super) fn compare_scalars(op: CmpOp, a: &Scalar, b: &Scalar) -> Result<bool, EvalError> {
    same_dim(a, b)?;
    let (x, y) = (checked(a.value)?, checked(b.value)?);
    match op {
        CmpOp::Eq => Ok(x == y),
        CmpOp::Ne => Ok(x != y),
        _ if x.im != 0.0 || y.im != 0.0 => Err(EvalError::Unsupported(
            "ordering of complex numbers".to_string(),
        )),
        CmpOp::Lt => Ok(x.re < y.re),
        CmpOp::Gt => Ok(x.re > y.re),
        CmpOp::Le => Ok(x.re <= y.re),
        CmpOp::Ge => Ok(x.re >= y.re),
    }
}

/// Power of scalars. The exponent must be dimensionless. A rational real exponent `p/q` keeps
/// dimensions exact (`sqrt(m^2) = m`) for non-negative real bases. Integer exponents are exact
/// for any base. Exponent `1/2` uses the complex square root, and a negative real base with an
/// exponent `p/2` is exactly imaginary or real, so both are exact (`sqrt(-4) = 2i`, computed
/// with no rounding noise). Every other case, including a negative real base with a fractional
/// exponent of denominator other than 2, is the principal branch `exp(p ln base)`, so
/// `(-1)^(1/3) = 0.5 + 0.866i`. The real odd root `nthRoot(-8, 3) = -2` is a different operator
/// and lives in `expr_eval`.
pub(super) fn s_pow(base: &Scalar, exp: &Scalar) -> Result<Scalar, EvalError> {
    require_dimensionless(exp.dim)?;
    let p = exp.value;
    if is_zero(p) {
        return Ok(Scalar::real(1.0, Dimension::dimensionless()));
    }
    let base_zero = is_zero(base.value);
    if base_zero && p.im == 0.0 && p.re < 0.0 {
        return Err(EvalError::DivisionByZero);
    }
    let rational = if p.im == 0.0 {
        rational_from_f64(p.re)
    } else {
        None
    };
    let (value, dim) = match rational {
        Some(r) if *r.denom() == 1 => {
            let n = *r.numer();
            (base.value.powi(n), base.dim.pow(r).map_err(from_units)?)
        }
        Some(r) if is_half(r) => (
            sqrt_complex(base.value),
            base.dim.pow(r).map_err(from_units)?,
        ),
        Some(r) if *r.denom() == 2 && base.value.im == 0.0 && base.value.re < 0.0 => {
            // (-x)^(p/2) = x^(p/2) e^(i pi p/2) on the principal branch. p is odd here, so the
            // phase is i (p = 1 mod 4) or -i (p = 3 mod 4).
            let mag = crate::units::Quantity::from_si(-base.value.re, base.dim)
                .pow(r)
                .map_err(from_units)?;
            let sign = if r.numer().rem_euclid(4) == 1 {
                1.0
            } else {
                -1.0
            };
            (Complex64::new(0.0, sign * mag.value_si), mag.dim)
        }
        Some(r) if base.value.im == 0.0 && base.value.re >= 0.0 => {
            let q = crate::units::Quantity::from_si(base.value.re, base.dim)
                .pow(r)
                .map_err(from_units)?;
            (Complex64::new(q.value_si, 0.0), q.dim)
        }
        Some(r) => (base.value.powc(p), base.dim.pow(r).map_err(from_units)?),
        None => {
            if !base.dim.is_dimensionless() {
                return Err(EvalError::Unsupported(
                    "non-rational exponent on a dimensioned base".to_string(),
                ));
            }
            (base.value.powc(p), Dimension::dimensionless())
        }
    };
    Ok(Scalar::new(checked(value)?, dim))
}

/// Exponent exactly one half.
fn is_half(r: Exp) -> bool {
    *r.numer() == 1 && *r.denom() == 2
}

/// The principal square root. Real arguments are exact: `sqrt(x)` for `x >= 0` and
/// `i sqrt(-x)` for `x < 0`, with no rounding from a complex formula.
pub(super) fn sqrt_complex(z: Complex64) -> Complex64 {
    if z.im == 0.0 {
        if z.re >= 0.0 {
            Complex64::new(z.re.sqrt(), 0.0)
        } else {
            Complex64::new(0.0, (-z.re).sqrt())
        }
    } else {
        z.sqrt()
    }
}

/// The rational `p/q` closest to `x` (continued fractions, `q <= 1000`), if one matches to
/// 1e-12 relative. `None` for irrational-looking values.
pub(super) fn rational_from_f64(x: f64) -> Option<Exp> {
    const MAX_DEN: i128 = 1000;
    if !x.is_finite() || x.abs() > 1e9 {
        return None;
    }
    if x == x.trunc() {
        return Some(Exp::from_integer(x as i32));
    }
    let (mut h_prev, mut h) = (0_i128, 1_i128);
    let (mut k_prev, mut k) = (1_i128, 0_i128);
    let mut rest = x;
    for _ in 0..64 {
        let a = rest.floor();
        let a_int = a as i128;
        let h_next = a_int.checked_mul(h)?.checked_add(h_prev)?;
        let k_next = a_int.checked_mul(k)?.checked_add(k_prev)?;
        if k_next > MAX_DEN {
            break;
        }
        h_prev = h;
        h = h_next;
        k_prev = k;
        k = k_next;
        if (h as f64 / k as f64 - x).abs() <= 1e-12 * x.abs().max(1.0) {
            return Some(Exp::new(i32::try_from(h).ok()?, i32::try_from(k).ok()?));
        }
        let frac = rest - a;
        if frac == 0.0 {
            break;
        }
        rest = 1.0 / frac;
        if !rest.is_finite() || rest.abs() > 1e18 {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_recovers_simple_fractions() {
        assert_eq!(rational_from_f64(0.5), Some(Exp::new(1, 2)));
        assert_eq!(rational_from_f64(1.0 / 3.0), Some(Exp::new(1, 3)));
        assert_eq!(rational_from_f64(-2.0), Some(Exp::from_integer(-2)));
        assert_eq!(rational_from_f64(0.0), Some(Exp::from_integer(0)));
        assert_eq!(rational_from_f64(2.0 / 3.0), Some(Exp::new(2, 3)));
    }

    #[test]
    fn rational_rejects_irrational_and_non_finite() {
        assert_eq!(rational_from_f64(std::f64::consts::SQRT_2), None);
        assert_eq!(rational_from_f64(f64::NAN), None);
        assert_eq!(rational_from_f64(f64::INFINITY), None);
        assert_eq!(rational_from_f64(1e12), None);
    }
}

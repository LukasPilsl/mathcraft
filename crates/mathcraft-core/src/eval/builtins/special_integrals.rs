//! Polar angle and the integrals of special functions, for real arguments:
//! `atan2`, the sine and cosine integrals `Si`, `Ci`, `Shi`, `Chi`, the exponential integral `Ei`
//! and the logarithmic integrals `li` and `Li`.
//!
//! Help: `polar_angles` (`atan2(x, y)` is the angle of the point (x, y), in (-pi, pi], undefined at
//! the origin), `integral_sin_cos` (the definitions of `Si`, `Ci`, `Shi`, `Chi` as integrals),
//! `exponential_integrals` (`Ei(x)` is the Cauchy principal value, the real part of `-Ei(1, -x)`),
//! and `Logarithmic_integrals` (`li(x) = Ei(ln x)` for x > 0, `x != 1`; `Li(x)` is the integral from
//! 2 to x of 1/ln t).
//!
//! Method: each integral is a closed-form integrand integrated over [0, x] by 20-point
//! Gauss-Legendre on panels of width at most 1. The integrands are entire, so this is accurate to
//! double precision. `Ei` uses its power series for x > 0 (all terms positive, no cancellation) and
//! `-E1(-x)` for x < 0 (series for small arguments, continued fraction for large ones).
//!
//! Limits (returned as errors, never as wrong numbers): arguments must be real and dimensionless;
//! `Ci` and `li` of a negative argument are complex and refused; `Shi` and `Chi` need `|x| <= 700`
//! (the integrand overflows beyond); `Si`, `Ci` need `|x| <= 10000` (the panel count is capped).

use std::f64::consts::PI;

use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

/// Euler's constant.
const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;
/// Gauss-Legendre points per panel.
const NODES: usize = 20;
/// Panel width in the integration variable.
const PANEL: f64 = 1.0;
/// Largest |x| for the integrals of sinh and cosh (their integrands overflow beyond this).
const HYPERBOLIC_LIMIT: f64 = 700.0;
/// Largest |x| for the sine and cosine integrals (bounds the panel count).
const TRIG_LIMIT: f64 = 10_000.0;
/// Termination threshold of the series and continued fraction.
const EPS: f64 = 1e-17;

pub(super) fn register(r: &mut Registry) {
    let one = Arity::Exact(1);
    r.register_ctx("atan2", Arity::Exact(2), atan2_fn);
    r.register_ctx("Si", one, si_fn);
    r.register_ctx("Ci", one, ci_fn);
    r.register_ctx("Shi", one, shi_fn);
    r.register_ctx("Chi", one, chi_fn);
    r.register_ctx("Ei", one, ei_fn);
    r.register_ctx("li", one, li_fn);
    r.register_ctx("Li", one, big_li_fn);
}

// ----- argument handling ----------------------------------------------------------------------

/// A dimensionless real scalar. Complex and dimensioned values are refused.
fn real_of(s: &Scalar) -> Result<f64, EvalError> {
    ops::require_dimensionless(s.dim)?;
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(
            "this function takes real arguments".to_string(),
        ));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// Applies a real kernel to a scalar or matrix argument, element by element.
fn map_real(
    ctx: &BuiltinCtx<'_>,
    v: &Value,
    f: impl Fn(&BuiltinCtx<'_>, f64) -> Result<f64, EvalError>,
) -> Result<Value, EvalError> {
    ops::map_scalar(v, |s| {
        let x = real_of(s)?;
        let r = f(ctx, x)?;
        if !r.is_finite() {
            return Err(EvalError::NotFinite);
        }
        Ok(Scalar::real(r, Dimension::dimensionless()))
    })
}

// ----- numerical integration ------------------------------------------------------------------

/// Gauss-Legendre nodes and weights on [-1, 1] (`n` points), by Newton iteration on the Legendre
/// polynomial (the recurrence of Numerical Recipes `gauleg`).
fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut nodes = Vec::with_capacity(n);
    let mut weights = Vec::with_capacity(n);
    for i in 0..n {
        let mut z = (PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p1, mut p2) = (1.0f64, 0.0f64);
            for j in 1..=n {
                let p3 = p2;
                p2 = p1;
                let jf = j as f64;
                p1 = ((2.0 * jf - 1.0) * z * p2 - (jf - 1.0) * p3) / jf;
            }
            dp = n as f64 * (z * p1 - p2) / (z * z - 1.0);
            let z_old = z;
            z = z_old - p1 / dp;
            if (z - z_old).abs() < 1e-15 {
                break;
            }
        }
        nodes.push(z);
        weights.push(2.0 / ((1.0 - z * z) * dp * dp));
    }
    (nodes, weights)
}

/// The integral of `f` over [0, x] (x may be negative), by 20-point Gauss-Legendre on panels of
/// width at most [`PANEL`]. Charges one unit per integrand evaluation to the budget.
fn integrate(ctx: &BuiltinCtx<'_>, x: f64, f: impl Fn(f64) -> f64) -> Result<f64, EvalError> {
    if x == 0.0 {
        return Ok(0.0);
    }
    let sign = x.signum();
    let len = x.abs();
    let panels = (len / PANEL).ceil().max(1.0) as usize;
    ctx.charge((panels * NODES) as u64)?;
    let (nodes, weights) = gauss_legendre(NODES);
    let h = len / panels as f64;
    let mut total = 0.0;
    for p in 0..panels {
        let a = p as f64 * h;
        let mid = a + 0.5 * h;
        for (t, w) in nodes.iter().zip(&weights) {
            total += w * f(mid + 0.5 * h * t);
        }
    }
    Ok(sign * total * 0.5 * h)
}

/// The sinc-type integrand `sin t / t` (t is never 0 inside the rule).
fn sin_over(t: f64) -> f64 {
    t.sin() / t
}

/// `(cos t - 1) / t`, written without cancellation as `-2 sin^2(t/2) / t`.
fn cos_minus_one_over(t: f64) -> f64 {
    let s = (0.5 * t).sin();
    -2.0 * s * s / t
}

/// `sinh t / t`.
fn sinh_over(t: f64) -> f64 {
    t.sinh() / t
}

/// `(cosh t - 1) / t`, written as `2 sinh^2(t/2) / t`.
fn cosh_minus_one_over(t: f64) -> f64 {
    let s = (0.5 * t).sinh();
    2.0 * s * s / t
}

// ----- Si, Ci, Shi, Chi -----------------------------------------------------------------------

/// `Si(x) = integral from 0 to x of sin t / t`. Odd in x.
fn si(ctx: &BuiltinCtx<'_>, x: f64) -> Result<f64, EvalError> {
    if x.abs() > TRIG_LIMIT {
        return Err(EvalError::Unsupported(
            "Si needs |x| at most 10000".to_string(),
        ));
    }
    integrate(ctx, x, sin_over)
}

/// `Ci(x) = gamma + ln x + integral from 0 to x of (cos t - 1) / t`, for x > 0. Negative x is
/// complex and refused.
fn ci(ctx: &BuiltinCtx<'_>, x: f64) -> Result<f64, EvalError> {
    if x <= 0.0 {
        return Err(EvalError::Unsupported(
            "Ci is complex for x <= 0; only x > 0 is supported".to_string(),
        ));
    }
    if x > TRIG_LIMIT {
        return Err(EvalError::Unsupported(
            "Ci needs x at most 10000".to_string(),
        ));
    }
    Ok(EULER_GAMMA + x.ln() + integrate(ctx, x, cos_minus_one_over)?)
}

/// `Shi(x) = integral from 0 to x of sinh t / t`. Odd in x.
fn shi(ctx: &BuiltinCtx<'_>, x: f64) -> Result<f64, EvalError> {
    if x.abs() > HYPERBOLIC_LIMIT {
        return Err(EvalError::Unsupported(
            "Shi needs |x| at most 700".to_string(),
        ));
    }
    integrate(ctx, x, sinh_over)
}

/// `Chi(x) = gamma + ln |x| + integral from 0 to x of (cosh t - 1) / t`. Even in x.
fn chi(ctx: &BuiltinCtx<'_>, x: f64) -> Result<f64, EvalError> {
    if x == 0.0 {
        return Err(EvalError::NotFinite);
    }
    if x.abs() > HYPERBOLIC_LIMIT {
        return Err(EvalError::Unsupported(
            "Chi needs |x| at most 700".to_string(),
        ));
    }
    let a = x.abs();
    Ok(EULER_GAMMA + a.ln() + integrate(ctx, a, cosh_minus_one_over)?)
}

fn si_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    map_real(ctx, one(args)?, si)
}

fn ci_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    map_real(ctx, one(args)?, ci)
}

fn shi_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    map_real(ctx, one(args)?, shi)
}

fn chi_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    map_real(ctx, one(args)?, chi)
}

fn one(args: &[Value]) -> Result<&Value, EvalError> {
    args.first()
        .ok_or_else(|| EvalError::Unsupported("missing argument".to_string()))
}

// ----- exponential and logarithmic integrals -------------------------------------------------

/// `E1(y)` for y > 0: the series for y <= 1, otherwise the continued fraction (modified Lentz).
fn e1(ctx: &BuiltinCtx<'_>, y: f64) -> Result<f64, EvalError> {
    if y <= 1.0 {
        // E1(y) = -gamma - ln y - sum_{k>=1} (-y)^k / (k k!)
        let mut term = 1.0f64;
        let mut sum = 0.0f64;
        for k in 1..=200u32 {
            term *= -y / k as f64;
            let add = term / k as f64;
            sum += add;
            if add.abs() <= EPS * sum.abs().max(1e-300) {
                break;
            }
        }
        ctx.charge(200)?;
        return Ok(-EULER_GAMMA - y.ln() - sum);
    }
    // E1(y) = exp(-y) / (y + 1/(1 + 1/(y + 2/(1 + 2/(y + ...)))))
    const FPMIN: f64 = 1e-300;
    let mut b = y + 1.0;
    let mut c = 1.0 / FPMIN;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..=1000u32 {
        let an = -((i * i) as f64);
        b += 2.0;
        d = 1.0 / (an * d + b);
        c = b + an / c;
        let del = c * d;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    ctx.charge(1000)?;
    Ok(h * (-y).exp())
}

/// `Ei(x)`, the principal value: `gamma + ln x + sum_{k>=1} x^k / (k k!)` for x > 0, and `-E1(-x)`
/// for x < 0. `Ei(0)` is minus infinity and is refused.
fn ei(ctx: &BuiltinCtx<'_>, x: f64) -> Result<f64, EvalError> {
    if x == 0.0 {
        return Err(EvalError::NotFinite);
    }
    if x < 0.0 {
        return Ok(-e1(ctx, -x)?);
    }
    if x > HYPERBOLIC_LIMIT {
        return Err(EvalError::Unsupported("Ei needs x at most 700".to_string()));
    }
    // All terms are positive for x > 0, so the sum has no cancellation.
    let mut term = 1.0f64;
    let mut sum = 0.0f64;
    for k in 1..=20_000u32 {
        term *= x / k as f64;
        let add = term / k as f64;
        sum += add;
        if add <= EPS * sum {
            break;
        }
    }
    ctx.charge(20_000)?;
    Ok(EULER_GAMMA + x.ln() + sum)
}

fn ei_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    map_real(ctx, one(args)?, ei)
}

/// `li(x) = Ei(ln x)` for x > 0, x != 1. Negative and zero x are refused (complex or infinite).
fn li(ctx: &BuiltinCtx<'_>, x: f64) -> Result<f64, EvalError> {
    if x <= 0.0 {
        return Err(EvalError::Unsupported("li needs x > 0".to_string()));
    }
    if x == 1.0 {
        return Err(EvalError::NotFinite);
    }
    ei(ctx, x.ln())
}

/// `Li(x) = li(x) - li(2)`, the offset logarithmic integral.
fn big_li(ctx: &BuiltinCtx<'_>, x: f64) -> Result<f64, EvalError> {
    if x <= 0.0 {
        return Err(EvalError::Unsupported("Li needs x > 0".to_string()));
    }
    if x == 1.0 {
        return Err(EvalError::NotFinite);
    }
    Ok(ei(ctx, x.ln())? - ei(ctx, 2f64.ln())?)
}

fn li_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    map_real(ctx, one(args)?, li)
}

fn big_li_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    map_real(ctx, one(args)?, big_li)
}

// ----- atan2 -----------------------------------------------------------------------------------

/// `atan2(x, y)`: the angle from the x-axis to the point (x, y), in (-pi, pi] (the help excludes
/// -pi). It is `arg(x + y i)`. Undefined at the origin.
fn atan2_fn(_ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let [Value::Scalar(x), Value::Scalar(y)] = args else {
        return Err(EvalError::Unsupported(
            "atan2 takes two scalars".to_string(),
        ));
    };
    let (x, y) = (real_of(x)?, real_of(y)?);
    if x == 0.0 && y == 0.0 {
        return Err(EvalError::Unsupported(
            "atan2 is undefined at (0, 0)".to_string(),
        ));
    }
    let mut angle = y.atan2(x);
    if angle == -PI {
        angle = PI;
    }
    Ok(Value::Scalar(Scalar::real(
        angle,
        Dimension::dimensionless(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> BuiltinCtx<'static> {
        BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3))
    }

    fn close(got: f64, want: f64, rel: f64) {
        let err = (got - want).abs();
        assert!(
            err <= rel * want.abs().max(1.0),
            "got {got:e}, want {want:e}"
        );
    }

    #[test]
    fn gauss_legendre_integrates_polynomials_exactly() {
        let (x, w) = gauss_legendre(NODES);
        let s: f64 = x.iter().zip(&w).map(|(x, w)| w * x.powi(10)).sum();
        close(s, 2.0 / 11.0, 1e-14);
    }

    #[test]
    fn sine_and_cosine_integrals_match_tables() {
        let c = ctx();
        close(si(&c, 1.0).unwrap(), 0.946_083_070_367_183, 1e-13);
        close(si(&c, 10.0).unwrap(), 1.658_347_594_218_874, 1e-12);
        close(si(&c, -1.0).unwrap(), -0.946_083_070_367_183, 1e-13);
        close(ci(&c, 1.0).unwrap(), 0.337_403_922_900_968, 1e-12);
        close(ci(&c, 10.0).unwrap(), -0.045_456_433_004_455, 1e-11);
        close(shi(&c, 1.0).unwrap(), 1.057_250_875_375_67, 1e-13);
        close(chi(&c, 1.0).unwrap(), 0.837_866_940_980_208, 1e-13);
        close(chi(&c, -1.0).unwrap(), 0.837_866_940_980_208, 1e-13);
    }

    #[test]
    fn exponential_integrals_match_tables() {
        let c = ctx();
        close(ei(&c, 1.0).unwrap(), 1.895_117_816_355_937, 1e-13);
        close(ei(&c, 2.0).unwrap(), 4.954_234_356_001_89, 1e-13);
        close(ei(&c, -1.0).unwrap(), -0.219_383_934_395_520, 1e-13);
        close(ei(&c, -2.0).unwrap(), -0.048_900_510_708_061, 1e-12);
        close(ei(&c, 10.0).unwrap(), 2_492.228_976_241_884, 1e-12);
    }

    #[test]
    fn li_and_offset_li_follow_their_definitions() {
        let c = ctx();
        close(li(&c, 10.0).unwrap(), ei(&c, 10f64.ln()).unwrap(), 1e-14);
        close(big_li(&c, 2.0).unwrap(), 0.0, 1e-14);
        assert!(matches!(li(&c, 1.0), Err(EvalError::NotFinite)));
        assert!(matches!(li(&c, -1.0), Err(EvalError::Unsupported(_))));
    }

    #[test]
    fn atan2_takes_x_first_and_maps_minus_pi_to_pi() {
        let c = ctx();
        let f = |x: f64, y: f64| {
            let args = [
                Value::Scalar(Scalar::real(x, Dimension::dimensionless())),
                Value::Scalar(Scalar::real(y, Dimension::dimensionless())),
            ];
            match atan2_fn(&c, &args) {
                Ok(Value::Scalar(s)) => s.value.re,
                other => panic!("{other:?}"),
            }
        };
        close(f(1.0, 1.0), PI / 4.0, 1e-15);
        close(f(-1.0, 0.0), PI, 1e-15);
        close(f(0.0, -1.0), -PI / 2.0, 1e-15);
    }

    #[test]
    fn hostile_arguments_are_refused_not_panicked() {
        let c = ctx();
        assert!(ci(&c, 0.0).is_err());
        assert!(ci(&c, -3.0).is_err());
        assert!(si(&c, 1e9).is_err());
        assert!(shi(&c, 1e6).is_err());
        assert!(matches!(ei(&c, 0.0), Err(EvalError::NotFinite)));
        assert!(ei(&c, 1e300).is_err());
        assert!(e1(&c, 1e-300).is_ok());
    }
}

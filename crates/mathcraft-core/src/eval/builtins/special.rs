//! Special functions and the remaining trigonometric and hyperbolic names: the builtins that
//! wrap the pure kernels in `special_gamma.rs`, `special_bessel.rs` and `special_misc.rs`.
//!
//! Unit rules (Mathcad help, "Special Functions" and "Trigonometric, Log, and Exponential"):
//! every argument is dimensionless, every result is dimensionless. Complex arguments are refused
//! with `Unsupported` for the real-only kernels; the trig and hyperbolic names accept complex
//! values. Argument order follows the help: the order `m` comes first in `Jn(m, z)`, and the
//! Gamma incomplete form is `Gamma(a, x)`.
//!
//! Registered but not implemented (each returns `Unsupported` with the reason):
//! `DAi`, `DBi` (Airy derivatives), `ellipticPi`, `js`, `ys` (spherical Bessel: half-integer
//! orders need the real-order `Y`), `bei`, `ber` (Kelvin functions). Scaled `sc` variants and
//! complex arguments of the Bessel family are not implemented.
//!
//! Domain errors that the `EvalError` enum has no variant for (gamma at a pole, `ln` of a
//! non-positive `lnGamma` argument, Lambert W below -1/e) come back as `Unsupported`, with a
//! message that names the reason.

use std::f64::consts::FRAC_PI_2;

use num_complex::Complex64;

use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

#[path = "special_bessel.rs"]
pub(crate) mod special_bessel;
#[path = "special_gamma.rs"]
pub(crate) mod special_gamma;
#[path = "special_misc.rs"]
pub(crate) mod special_misc;

use special_bessel as bessel;
use special_gamma as gamma;
use special_misc as misc;

/// Registers every function of this tier.
pub fn register(r: &mut Registry) {
    let one = Arity::Exact(1);
    let two = Arity::Exact(2);
    r.register("Γ", Arity::Range(1, 2), gamma_fn);
    r.register("lnΓ", one, ln_gamma_fn);
    r.register("Psi", one, psi_fn);
    r.register("ibeta", Arity::Exact(3), ibeta_fn);
    r.register("erf", one, erf_fn);
    r.register("erfc", one, erfc_fn);
    r.register("J0", one, j0);
    r.register("J1", one, j1);
    r.register("Jn", two, jn);
    r.register("Y0", one, y0);
    r.register("Y1", one, y1);
    r.register("Yn", two, yn);
    r.register("I0", one, i0);
    r.register("I1", one, i1);
    r.register("In", two, in_fn);
    r.register("K0", one, k0);
    r.register("K1", one, k1);
    r.register("Kn", two, kn);
    r.register("H1", two, hankel1);
    r.register("H2", two, hankel2);
    r.register("Ai", one, ai);
    r.register("Bi", one, bi);
    r.register("DAi", one, not_implemented_dai);
    r.register("DBi", one, not_implemented_dbi);
    r.register("FresnelC", one, fresnel_c);
    r.register("FresnelS", one, fresnel_s);
    r.register("LambertW", Arity::Range(1, 2), lambert_w_fn);
    r.register("δ", two, kronecker_fn);
    r.register("Φ", one, heaviside_fn);
    r.register("Her", two, her);
    r.register("Lag", two, lag);
    r.register("Leg", two, leg);
    r.register("Tcheb", two, tcheb);
    r.register("Ucheb", two, ucheb);
    r.register("Jac", Arity::Exact(4), jac);
    r.register("ellipticK", one, elliptic_k);
    r.register("ellipticE", Arity::Range(1, 2), elliptic_e);
    r.register("ellipticF", two, elliptic_f);
    r.register("ellipticPi", Arity::Range(2, 3), not_implemented_pi);
    r.register("js", two, not_implemented_js);
    r.register("ys", two, not_implemented_ys);
    r.register("bei", two, not_implemented_bei);
    r.register("ber", two, not_implemented_ber);
    register_trig(r);
    super::special_integrals::register(r);
}

/// The trigonometric and hyperbolic names that `elementary` does not register.
fn register_trig(r: &mut Registry) {
    let one = Arity::Exact(1);
    r.register("csc", one, csc);
    r.register("sec", one, sec);
    r.register("cot", one, cot);
    r.register("csch", one, csch);
    r.register("sech", one, sech);
    r.register("coth", one, coth);
    r.register("asec", one, asec);
    r.register("acsc", one, acsc);
    r.register("acot", one, acot);
    r.register("asinh", one, asinh);
    r.register("acosh", one, acosh);
    r.register("atanh", one, atanh);
    r.register("acsch", one, acsch);
    r.register("asech", one, asech);
    r.register("acoth", one, acoth);
    r.register("sinc", one, sinc);
}

// ----- argument handling -------------------------------------------------------------------

/// A dimensionless real scalar argument. Complex values are refused.
fn real_arg(v: &Value) -> Result<f64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            if s.value.im != 0.0 {
                return Err(EvalError::Unsupported(
                    "this function takes real arguments".to_string(),
                ));
            }
            Ok(s.value.re)
        }
        _ => Err(EvalError::Unsupported(
            "expected a scalar argument".to_string(),
        )),
    }
}

/// A real argument that must be an integer no larger than one million in magnitude.
fn int_arg(v: &Value) -> Result<i64, EvalError> {
    let x = real_arg(v)?;
    if !x.is_finite() || x.fract() != 0.0 || x.abs() > 1e6 {
        return Err(EvalError::Unsupported(
            "this order must be an integer".to_string(),
        ));
    }
    Ok(x as i64)
}

fn dimensionless_real(x: f64) -> Result<Value, EvalError> {
    if x.is_finite() {
        Ok(Value::Scalar(Scalar::real(x, Dimension::dimensionless())))
    } else {
        Err(EvalError::NotFinite)
    }
}

/// Applies a real kernel element-wise to a scalar or matrix argument.
fn map_real(v: &Value, f: impl Fn(f64) -> Result<f64, EvalError>) -> Result<Value, EvalError> {
    ops::map_scalar(v, |s| {
        ops::require_dimensionless(s.dim)?;
        if s.value.im != 0.0 {
            return Err(EvalError::Unsupported(
                "this function takes real arguments".to_string(),
            ));
        }
        let r = f(s.value.re)?;
        if !r.is_finite() {
            return Err(EvalError::NotFinite);
        }
        Ok(Scalar::real(r, Dimension::dimensionless()))
    })
}

/// A scalar parameter `p` (order, degree, shape) with a value argument `z` mapped element-wise.
fn with_param(
    p: &Value,
    z: &Value,
    f: impl Fn(f64, f64) -> Result<f64, EvalError>,
) -> Result<Value, EvalError> {
    let pv = real_arg(p)?;
    map_real(z, |x| f(pv, x))
}

/// Applies a complex kernel element-wise, for the trigonometric and hyperbolic names. A real
/// argument whose result has only rounding noise in the imaginary part (such as `acsc(2)`) gives
/// a real result.
fn map_complex(v: &Value, f: impl Fn(Complex64) -> Complex64) -> Result<Value, EvalError> {
    ops::map_scalar(v, |s| {
        ops::require_dimensionless(s.dim)?;
        let mut z = f(s.value);
        if s.value.im == 0.0 && z.im.abs() <= 1e-14 * (1.0 + z.re.abs()) {
            z.im = 0.0;
        }
        if !(z.re.is_finite() && z.im.is_finite()) {
            return Err(EvalError::NotFinite);
        }
        Ok(Scalar::new(z, Dimension::dimensionless()))
    })
}

fn one_arg(args: &[Value]) -> Result<&Value, EvalError> {
    args.first()
        .ok_or_else(|| EvalError::Unsupported("missing argument".to_string()))
}

// ----- gamma family, error function, beta ---------------------------------------------------

fn gamma_fn(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [z] => map_real(z, gamma::gamma),
        [a, x] => with_param(a, x, gamma::upper_incomplete_gamma),
        _ => Err(EvalError::Unsupported(
            "Gamma takes 1 or 2 arguments".to_string(),
        )),
    }
}

fn ln_gamma_fn(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, gamma::ln_gamma)
}

fn psi_fn(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, gamma::digamma)
}

fn ibeta_fn(args: &[Value]) -> Result<Value, EvalError> {
    let (a, x, y) = match args {
        [a, x, y] => (real_arg(a)?, real_arg(x)?, real_arg(y)?),
        _ => {
            return Err(EvalError::Unsupported(
                "ibeta takes three arguments".to_string(),
            ))
        }
    };
    dimensionless_real(gamma::ibeta(a, x, y)?)
}

fn erf_fn(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| Ok(gamma::erf(x)))
}

fn erfc_fn(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| Ok(gamma::erfc(x)))
}

// ----- Bessel, Airy, Fresnel ------------------------------------------------------------------

fn j0(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_j(0.0, x))
}

fn j1(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_j(1.0, x))
}

fn jn(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [m, z] => with_param(m, z, bessel::bessel_j),
        _ => Err(EvalError::Unsupported("Jn takes two arguments".to_string())),
    }
}

fn y0(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_y_int(0, x))
}

fn y1(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_y_int(1, x))
}

fn yn(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [m, z] => {
            let n = int_arg(m)?;
            map_real(z, |x| bessel::bessel_y_int(n, x))
        }
        _ => Err(EvalError::Unsupported("Yn takes two arguments".to_string())),
    }
}

fn i0(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_i(0.0, x))
}

fn i1(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_i(1.0, x))
}

fn in_fn(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [m, z] => with_param(m, z, bessel::bessel_i),
        _ => Err(EvalError::Unsupported("In takes two arguments".to_string())),
    }
}

fn k0(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_k(0.0, x))
}

fn k1(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::bessel_k(1.0, x))
}

fn kn(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [m, z] => with_param(m, z, bessel::bessel_k),
        _ => Err(EvalError::Unsupported("Kn takes two arguments".to_string())),
    }
}

/// Hankel function `H1_m(z) = J_m(z) + i Y_m(z)`, integer `m`, `z > 0`.
fn hankel1(args: &[Value]) -> Result<Value, EvalError> {
    hankel(args, 1.0)
}

/// Hankel function `H2_m(z) = J_m(z) - i Y_m(z)`, integer `m`, `z > 0`.
fn hankel2(args: &[Value]) -> Result<Value, EvalError> {
    hankel(args, -1.0)
}

fn hankel(args: &[Value], sign: f64) -> Result<Value, EvalError> {
    let (m, z) = match args {
        [m, z] => (int_arg(m)?, real_arg(z)?),
        _ => {
            return Err(EvalError::Unsupported(
                "Hankel takes two arguments".to_string(),
            ))
        }
    };
    let j = bessel::bessel_j(m as f64, z)?;
    let y = bessel::bessel_y_int(m, z)?;
    Ok(Value::Scalar(Scalar::new(
        Complex64::new(j, sign * y),
        Dimension::dimensionless(),
    )))
}

fn ai(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, bessel::airy_ai)
}

fn bi(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, bessel::airy_bi)
}

fn fresnel_c(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::fresnel(x).map(|p| p.0))
}

fn fresnel_s(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| bessel::fresnel(x).map(|p| p.1))
}

fn not_implemented_dai(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "DAi (Airy derivative) is not implemented yet".to_string(),
    ))
}

fn not_implemented_dbi(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "DBi (Airy derivative) is not implemented yet".to_string(),
    ))
}

fn not_implemented_pi(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "ellipticPi is not implemented yet".to_string(),
    ))
}

fn not_implemented_js(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "js (spherical Bessel) is not implemented yet".to_string(),
    ))
}

fn not_implemented_ys(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "ys (spherical Bessel) is not implemented yet".to_string(),
    ))
}

fn not_implemented_bei(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "bei (Kelvin function) is not implemented yet".to_string(),
    ))
}

fn not_implemented_ber(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "ber (Kelvin function) is not implemented yet".to_string(),
    ))
}

// ----- Lambert W, step and delta ---------------------------------------------------------------

fn lambert_w_fn(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [x] => map_real(x, |x| misc::lambert_w(0, x)),
        [n, x] => {
            let branch = int_arg(n)?;
            map_real(x, |x| misc::lambert_w(branch, x))
        }
        _ => Err(EvalError::Unsupported(
            "LambertW takes one or two arguments".to_string(),
        )),
    }
}

fn kronecker_fn(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [m, n] => dimensionless_real(misc::kronecker(int_arg(m)? as f64, int_arg(n)? as f64)),
        _ => Err(EvalError::Unsupported(
            "delta takes two arguments".to_string(),
        )),
    }
}

fn heaviside_fn(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |x| Ok(misc::heaviside(x)))
}

// ----- orthogonal polynomials ------------------------------------------------------------------

fn poly_call(args: &[Value], kind: misc::Poly) -> Result<Value, EvalError> {
    match args {
        [n, x] => {
            let degree = misc::check_degree(real_arg(n)?)?;
            map_real(x, |x| misc::polynomial(kind, degree, x))
        }
        _ => Err(EvalError::Unsupported(
            "polynomials take two arguments".to_string(),
        )),
    }
}

fn her(args: &[Value]) -> Result<Value, EvalError> {
    poly_call(args, misc::Poly::Hermite)
}

fn lag(args: &[Value]) -> Result<Value, EvalError> {
    poly_call(args, misc::Poly::Laguerre)
}

fn leg(args: &[Value]) -> Result<Value, EvalError> {
    poly_call(args, misc::Poly::Legendre)
}

fn tcheb(args: &[Value]) -> Result<Value, EvalError> {
    poly_call(args, misc::Poly::ChebyshevT)
}

fn ucheb(args: &[Value]) -> Result<Value, EvalError> {
    poly_call(args, misc::Poly::ChebyshevU)
}

fn jac(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [n, a, b, x] => {
            let degree = misc::check_degree(real_arg(n)?)?;
            let (a, b) = (real_arg(a)?, real_arg(b)?);
            map_real(x, |x| misc::jacobi(degree, a, b, x))
        }
        _ => Err(EvalError::Unsupported(
            "Jac takes four arguments".to_string(),
        )),
    }
}

// ----- elliptic integrals ----------------------------------------------------------------------

fn elliptic_k(args: &[Value]) -> Result<Value, EvalError> {
    map_real(one_arg(args)?, |m| misc::elliptic_ke(m).map(|p| p.0))
}

fn elliptic_e(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [m] => map_real(m, |m| misc::elliptic_ke(m).map(|p| p.1)),
        [z, m] => with_param(z, m, |z, m| misc::elliptic_incomplete(z, m).map(|p| p.1)),
        _ => Err(EvalError::Unsupported(
            "ellipticE takes one or two arguments".to_string(),
        )),
    }
}

fn elliptic_f(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [z, m] => with_param(z, m, |z, m| misc::elliptic_incomplete(z, m).map(|p| p.0)),
        _ => Err(EvalError::Unsupported(
            "ellipticF takes two arguments".to_string(),
        )),
    }
}

// ----- trigonometric and hyperbolic names ------------------------------------------------------

fn csc(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.sin().inv())
}

fn sec(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.cos().inv())
}

fn cot(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| super::elementary::complex_tan(z).inv())
}

fn csch(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.sinh().inv())
}

fn sech(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.cosh().inv())
}

fn coth(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.tanh().inv())
}

fn asec(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.inv().acos())
}

fn acsc(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.inv().asin())
}

/// `acot(z) = pi/2 - atan(z)`, which lies in (0, pi) for real `z` as the help specifies.
fn acot(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| {
        Complex64::new(FRAC_PI_2, 0.0) - z.atan()
    })
}

fn asinh(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.asinh())
}

fn acosh(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.acosh())
}

fn atanh(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.atanh())
}

fn acsch(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.inv().asinh())
}

fn asech(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.inv().acosh())
}

fn acoth(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| z.inv().atanh())
}

/// `sinc(z) = sin(z)/z`, with the value 1 at zero.
fn sinc(args: &[Value]) -> Result<Value, EvalError> {
    map_complex(one_arg(args)?, |z| {
        if z == Complex64::new(0.0, 0.0) {
            Complex64::new(1.0, 0.0)
        } else {
            z.sin() / z
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_name_has_an_arity() {
        let mut r = Registry::new();
        register(&mut r);
        assert!(r.get("Jn").is_some());
        assert!(r.get("erf").is_some());
        assert!(r.get("csc").is_some());
    }
}

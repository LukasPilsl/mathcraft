//! Coordinate mappings, `angle`, `fact2` and the real-argument forms of the symbolic special
//! functions `beta`, `Zeta`, `dilog` and `polylog`.
//!
//! Help: `coordinate_system_mapping_functions` (the six mappings return a column vector, angles in
//! radians; the one-vector form takes a 2 or 3 element vector), `example_converting_*` (spherical:
//! `x = r sin(phi) cos(theta)`, `y = r sin(phi) sin(theta)`, `z = r cos(phi)`; `xy2pol((2;3)) =
//! (3.606; 0.983)`), `polar_angles` (`angle` is in [0, 2 pi)), `other_symbolic_functions`
//! (`fact2`, n >= -1), `beta_hgeom_zeta` and `logarithmic_functions` (definitions only).
//!
//! The mappings use the angle range (-pi, pi] for theta (`atan2`) and [0, pi] for phi. All
//! arguments are real and dimensionless, except that `angle` takes two real scalars of the same
//! dimension and returns a dimensionless angle. Anything outside the real domain (`beta` with a
//! non-positive argument, `polylog` with `|x| > 0.9` for `n >= 3` or `x >= 1` for `n <= 0`,
//! `dilog` of a negative number, `Zeta(1)`) is refused, never approximated.
//!
//! Methods: `Zeta` is Euler-Maclaurin (20 terms, 8 Bernoulli corrections) for `s >= 0`, and the
//! functional equation for `s < 0`. `Li2` uses Landen and reflection to bring the argument into
//! `[0, 0.5]`, then its series. `polylog(n, x)` for `n >= 3` is the series, at most `MAX_TERMS`,
//! refused when its largest term dwarfs the sum. For integer `n <= 0` it is the exact Eulerian
//! identity in integer arithmetic (the series cancels badly there).

use std::f64::consts::PI;

use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};

use super::special::special_gamma::{gamma, ln_beta};
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Largest series length of `polylog`.
const MAX_TERMS: u64 = 20_000;
/// Largest `m` for which `polylog(-m, x)` is summed exactly, and the most bits its exact sum may
/// hold (order times the bit length of the denominator of `1 - x`).
const MAX_EXACT_ORDER: u64 = 1000;
const MAX_EXACT_BITS: u64 = 200_000;

pub fn register(r: &mut Registry) {
    r.register_ctx("angle", Arity::Exact(2), angle_fn);
    r.register_ctx("xy2pol", Arity::Range(1, 2), xy2pol_fn);
    r.register_ctx("pol2xy", Arity::Range(1, 2), pol2xy_fn);
    r.register_ctx("xyz2sph", Arity::Range(1, 3), xyz2sph_fn);
    r.register_ctx("sph2xyz", Arity::Range(1, 3), sph2xyz_fn);
    r.register_ctx("xyz2cyl", Arity::Range(1, 3), xyz2cyl_fn);
    r.register_ctx("cyl2xyz", Arity::Range(1, 3), cyl2xyz_fn);
    r.register_ctx("fact2", Arity::Exact(1), fact2_fn);
    r.register_ctx("beta", Arity::Exact(2), beta_fn);
    r.register_ctx("Zeta", Arity::Exact(1), zeta_fn);
    r.register_ctx("dilog", Arity::Exact(1), dilog_fn);
    r.register_ctx("polylog", Arity::Exact(2), polylog_fn);
}

fn unsupported(msg: &str) -> EvalError {
    EvalError::Unsupported(msg.to_string())
}

fn real_of(s: &Scalar) -> Result<f64, EvalError> {
    ops::require_dimensionless(s.dim)?;
    real_si(s)
}

/// The stored SI value of a real scalar, whatever its dimension.
fn real_si(s: &Scalar) -> Result<f64, EvalError> {
    if s.value.im != 0.0 {
        return Err(unsupported("this function takes real arguments"));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

fn scalar_arg(v: &Value) -> Result<f64, EvalError> {
    match v {
        Value::Scalar(s) => real_of(s),
        _ => Err(unsupported("this function takes real scalars")),
    }
}

fn out(x: f64) -> Result<Value, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::real(x, Dimension::dimensionless())))
}

/// The `n` coordinates of a mapping call: `n` scalars, or one vector of `n` elements.
fn coords(args: &[Value], n: usize) -> Result<Vec<f64>, EvalError> {
    if args.len() == n {
        return args.iter().map(scalar_arg).collect();
    }
    if let [Value::Matrix(m)] = args {
        if m.rows() * m.cols() == n && (m.rows() == 1 || m.cols() == 1) {
            return m.data().iter().map(real_of).collect();
        }
    }
    Err(unsupported(
        "pass the coordinates as separate scalars or as one vector of the right length",
    ))
}

fn column(v: &[f64]) -> Result<Value, EvalError> {
    if v.iter().any(|x| !x.is_finite()) {
        return Err(EvalError::NotFinite);
    }
    let data = v
        .iter()
        .map(|&x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(v.len(), 1, data)?))
}

/// `angle(x, y)`: the polar angle of (x, y) in [0, 2 pi). `x` and `y` are real scalars of the same
/// dimension, read as their stored SI values; the angle is dimensionless.
fn angle_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let [Value::Scalar(xs), Value::Scalar(ys)] = args else {
        return Err(unsupported("angle takes two real scalars"));
    };
    if xs.dim != ys.dim {
        return Err(EvalError::DimensionMismatch {
            left: xs.dim,
            right: ys.dim,
        });
    }
    let (x, y) = (real_si(xs)?, real_si(ys)?);
    if x == 0.0 && y == 0.0 {
        return Err(unsupported("angle is undefined at (0, 0)"));
    }
    let mut a = y.atan2(x);
    if a < 0.0 {
        a += 2.0 * PI;
    }
    if a >= 2.0 * PI {
        a = 0.0;
    }
    out(a)
}

fn xy2pol_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let v = coords(args, 2)?;
    column(&[v[0].hypot(v[1]), v[1].atan2(v[0])])
}

fn pol2xy_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let v = coords(args, 2)?;
    column(&[v[0] * v[1].cos(), v[0] * v[1].sin()])
}

fn xyz2sph_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let v = coords(args, 3)?;
    let r = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if r == 0.0 {
        return Err(unsupported("the origin has no spherical angles"));
    }
    column(&[r, v[1].atan2(v[0]), (v[2] / r).clamp(-1.0, 1.0).acos()])
}

fn sph2xyz_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let v = coords(args, 3)?;
    let (r, t, p) = (v[0], v[1], v[2]);
    column(&[r * p.sin() * t.cos(), r * p.sin() * t.sin(), r * p.cos()])
}

fn xyz2cyl_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let v = coords(args, 3)?;
    column(&[v[0].hypot(v[1]), v[1].atan2(v[0]), v[2]])
}

fn cyl2xyz_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let v = coords(args, 3)?;
    column(&[v[0] * v[1].cos(), v[0] * v[1].sin(), v[2]])
}

/// `fact2(n)`: the double factorial, `n >= -1` an integer. `fact2(-1) = fact2(0) = 1`.
fn fact2_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let n = scalar_arg(&args[0])?;
    if n.fract() != 0.0 || n < -1.0 {
        return Err(unsupported("fact2 needs an integer n >= -1"));
    }
    let mut p = 1.0f64;
    let mut k = n;
    // The product leaves f64 near n = 300, so the loop is short whatever n is.
    while k > 1.0 {
        p *= k;
        if !p.is_finite() {
            return Err(EvalError::Overflow);
        }
        k -= 2.0;
    }
    out(p)
}

/// `beta(x, y) = Gamma(x) Gamma(y) / Gamma(x + y)` for `x, y > 0`.
fn beta_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (x, y) = (scalar_arg(&args[0])?, scalar_arg(&args[1])?);
    if x <= 0.0 || y <= 0.0 {
        return Err(unsupported("beta is built for positive real arguments"));
    }
    let v = ln_beta(x, y)?.exp();
    if !v.is_finite() {
        return Err(EvalError::Overflow);
    }
    out(v)
}

/// Riemann zeta of a real `s`, `s != 1`, `|s| <= 100`.
pub(crate) fn zeta(s: f64) -> Result<f64, EvalError> {
    if s == 1.0 {
        return Err(unsupported("Zeta has a pole at 1"));
    }
    if s.abs() > 100.0 {
        return Err(unsupported("Zeta is built for |s| <= 100"));
    }
    if s < 0.0 {
        if s.fract() == 0.0 && (s as i64) % 2 == 0 {
            return Ok(0.0);
        }
        // zeta(s) = 2^s pi^(s-1) sin(pi s / 2) Gamma(1 - s) zeta(1 - s)
        let z = zeta(1.0 - s)?;
        let v = 2f64.powf(s) * PI.powf(s - 1.0) * (PI * s / 2.0).sin() * gamma(1.0 - s)? * z;
        return if v.is_finite() {
            Ok(v)
        } else {
            Err(EvalError::Overflow)
        };
    }
    const N: usize = 20;
    // B_{2j} / (2j)! for j = 1..=8.
    const COEF: [f64; 8] = [
        1.0 / 12.0,
        -1.0 / 720.0,
        1.0 / 30_240.0,
        -1.0 / 1_209_600.0,
        1.0 / 47_900_160.0,
        -691.0 / 1_307_674_368_000.0,
        1.0 / 74_724_249_600.0,
        -3617.0 / 10_670_622_842_880_000.0,
    ];
    let n = N as f64;
    let mut sum = 0.0;
    for k in 1..N {
        sum += (k as f64).powf(-s);
    }
    sum += n.powf(1.0 - s) / (s - 1.0) + 0.5 * n.powf(-s);
    // rising = s (s+1) ... (s + 2j - 2)
    let mut rising = s;
    for (j, c) in COEF.iter().enumerate() {
        let two_j = 2.0 * (j as f64 + 1.0);
        sum += c * rising * n.powf(-s - two_j + 1.0);
        rising *= (s + two_j - 1.0) * (s + two_j);
    }
    Ok(sum)
}

fn zeta_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    out(zeta(scalar_arg(&args[0])?)?)
}

/// `Li2(z)` for real `z <= 1`.
fn li2(z: f64) -> Result<f64, EvalError> {
    fn series(w: f64) -> f64 {
        let (mut term, mut sum) = (1.0, 0.0);
        for k in 1..=80 {
            term *= w;
            sum += term / (k * k) as f64;
        }
        sum
    }
    if z > 1.0 {
        return Err(unsupported("the dilogarithm is complex beyond 1"));
    }
    let pi2_6 = PI * PI / 6.0;
    Ok(if z == 1.0 {
        pi2_6
    } else if z < -1.0 {
        -pi2_6 - 0.5 * (-z).ln().powi(2) - li2(1.0 / z)?
    } else if z < 0.0 {
        -series(z / (z - 1.0)) - 0.5 * (1.0 - z).ln().powi(2)
    } else if z <= 0.5 {
        series(z)
    } else {
        pi2_6 - z.ln() * (1.0 - z).ln() - series(1.0 - z)
    })
}

/// `dilog(x) = Li2(1 - x)`, real for `x >= 0`.
fn dilog_fn(_c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let x = scalar_arg(&args[0])?;
    out(li2(1.0 - x)?)
}

/// `polylog(n, x)`: orders 1 and 2 in closed form for `x <= 1`; other integer orders by the
/// series for `|x| <= 0.9`, and `Zeta(n)` at `x = 1`.
fn polylog_fn(c: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (n, x) = (scalar_arg(&args[0])?, scalar_arg(&args[1])?);
    if n.fract() != 0.0 || n.abs() > 1000.0 {
        return Err(unsupported("polylog needs an integer order up to 1000"));
    }
    if n == 1.0 {
        if x >= 1.0 {
            return Err(unsupported("polylog(1, x) needs x < 1"));
        }
        return out(-(1.0 - x).ln());
    }
    if n == 2.0 {
        return out(li2(x)?);
    }
    if n <= 0.0 {
        if x >= 1.0 {
            return Err(unsupported("polylog(n, x) for n <= 0 needs x < 1"));
        }
        let m = (-n) as u64;
        c.charge((m + 1) * (m + 1))?;
        return out(polylog_nonpositive(m, x)?);
    }
    if x == 1.0 && n > 2.0 {
        return out(zeta(n)?);
    }
    if x.abs() > 0.9 {
        return Err(unsupported("polylog is built for |x| <= 0.9 at this order"));
    }
    c.charge(MAX_TERMS)?;
    let (mut p, mut sum, mut max_term) = (1.0f64, 0.0f64, 0.0f64);
    for k in 1..=MAX_TERMS {
        p *= x;
        let t = p * (k as f64).powf(-n);
        sum += t;
        max_term = max_term.max(t.abs());
        if t.abs() < 1e-18 * sum.abs().max(1e-300) {
            if max_term > 1e4 * sum.abs() {
                return Err(unsupported("polylog loses precision for these arguments"));
            }
            return out(sum);
        }
        if !sum.is_finite() {
            return Err(EvalError::Overflow);
        }
    }
    Err(unsupported("polylog series did not converge"))
}

/// `Li_{-m}(x)` for an integer `m >= 0` and a real `x < 1`, from the exact identity
/// `Li_{-m}(x) = sum_{k=0}^{m} k! S(m+1, k+1) y^(k+1)` with `y = x/(1-x)` and `S` the Stirling
/// numbers of the second kind. The terms cancel to many digits for negative `x`, so the sum is
/// formed in integers over the common denominator `(1-x)^(m+1)` and rounded once at the end.
fn polylog_nonpositive(m: u64, x: f64) -> Result<f64, EvalError> {
    if m > MAX_EXACT_ORDER {
        return Err(unsupported("polylog of this order is not supported"));
    }
    let r = BigRational::from_float(x).ok_or(EvalError::NotFinite)?;
    // y = p / d with d = q - p > 0 (because x < 1), from x = p / q.
    let p = r.numer().clone();
    let d = r.denom() - &p;
    let m = m as usize;
    if (m as u64 + 1) * d.bits() > MAX_EXACT_BITS {
        return Err(unsupported(
            "polylog of this order at this argument is too large to evaluate exactly",
        ));
    }
    // The integer type is the rational's own (taken from `BigRational`, so no second bigint crate).
    let zero = BigRational::zero().numer().clone();
    let one = BigRational::one().numer().clone();
    // Stirling numbers S(n, k), row by row up to n = m + 1.
    let mut row = vec![one.clone()];
    for n in 1..=m + 1 {
        let mut next = vec![zero.clone(); n + 1];
        for k in 1..=n {
            let same = if k < row.len() {
                &row[k] * (k as u64)
            } else {
                zero.clone()
            };
            next[k] = same + &row[k - 1];
        }
        row = next;
    }
    let mut pow_p = vec![one.clone()];
    let mut pow_d = vec![one.clone()];
    for j in 1..=m + 1 {
        let (np, nd) = (&pow_p[j - 1] * &p, &pow_d[j - 1] * &d);
        pow_p.push(np);
        pow_d.push(nd);
    }
    // Li * d^(m+1) = sum_k k! S(m+1, k+1) p^(k+1) d^(m-k).
    let mut num = zero.clone();
    let mut fact = one.clone();
    for k in 0..=m {
        if k > 0 {
            fact = &fact * (k as u64);
        }
        let coef = &fact * &row[k + 1];
        num += coef * &pow_p[k + 1] * &pow_d[m - k];
    }
    BigRational::new(num, pow_d[m + 1].clone())
        .to_f64()
        .ok_or(EvalError::NotFinite)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> BuiltinCtx<'static> {
        BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3))
    }
    fn sc(x: f64) -> Value {
        Value::Scalar(Scalar::real(x, Dimension::dimensionless()))
    }
    fn vecv(v: &[f64]) -> Value {
        column(v).unwrap()
    }
    fn num(v: Result<Value, EvalError>) -> f64 {
        match v {
            Ok(Value::Scalar(s)) => s.value.re,
            o => panic!("{o:?}"),
        }
    }
    fn col(v: Result<Value, EvalError>) -> Vec<f64> {
        match v {
            Ok(Value::Matrix(m)) => {
                assert_eq!(m.cols(), 1);
                m.data().iter().map(|s| s.value.re).collect()
            }
            o => panic!("{o:?}"),
        }
    }
    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol * b.abs().max(1.0), "{a} vs {b}");
    }

    #[test]
    fn polar_examples_from_help() {
        let p = col(xy2pol_fn(&ctx(), &[vecv(&[2.0, 3.0])]));
        close(p[0], 3.606, 1e-3);
        close(p[1], 0.983, 1e-3);
        let b = col(pol2xy_fn(&ctx(), &[vecv(&p)]));
        close(b[0], 2.0, 1e-12);
        close(b[1], 3.0, 1e-12);
        let p = col(xy2pol_fn(&ctx(), &[sc(2.0), sc(10.0)]));
        close(p[0], 10.198, 1e-3);
        close(p[1], 1.373, 1e-3);
        let q = col(pol2xy_fn(&ctx(), &[sc(2.236), sc(1.107)]));
        close(q[0], 1.0, 1e-3);
        close(q[1], 2.0, 1e-3);
    }

    #[test]
    fn spherical_examples_from_help() {
        let first = col(sph2xyz_fn(&ctx(), &[sc(2.0), sc(-PI), sc(0.0)]));
        close(first[0], 0.0, 1e-12);
        close(first[2], 2.0, 1e-12);
        let row1 = col(sph2xyz_fn(&ctx(), &[vecv(&[2.1, -2.742, 0.2])]));
        close(row1[0], -0.384, 2e-3);
        close(row1[1], -0.162, 2e-3);
        close(row1[2], 2.058, 2e-3);
        let row10 = col(sph2xyz_fn(&ctx(), &[sc(3.0), sc(0.858), sc(2.0)]));
        close(row10[0], 1.783, 2e-3);
        close(row10[1], 2.064, 2e-3);
        close(row10[2], -1.248, 2e-3);
        let back = col(xyz2sph_fn(&ctx(), &[vecv(&row10)]));
        close(back[0], 3.0, 1e-12);
        close(back[1], 0.858, 1e-12);
        close(back[2], 2.0, 1e-12);
        let cyl = col(xyz2cyl_fn(&ctx(), &[sc(1.0), sc(1.0), sc(5.0)]));
        close(cyl[0], 2f64.sqrt(), 1e-12);
        close(cyl[1], PI / 4.0, 1e-12);
        let xyz = col(cyl2xyz_fn(&ctx(), &[vecv(&cyl)]));
        close(xyz[0], 1.0, 1e-12);
        close(xyz[2], 5.0, 1e-12);
        assert!(xyz2sph_fn(&ctx(), &[sc(0.0), sc(0.0), sc(0.0)]).is_err());
        assert!(xy2pol_fn(&ctx(), &[vecv(&[1.0, 2.0, 3.0])]).is_err());
    }

    #[test]
    fn angle_range() {
        close(
            num(angle_fn(&ctx(), &[sc(1.0), sc(-1.0)])),
            7.0 * PI / 4.0,
            1e-14,
        );
        close(num(angle_fn(&ctx(), &[sc(-1.0), sc(0.0)])), PI, 1e-14);
        assert!(angle_fn(&ctx(), &[sc(0.0), sc(0.0)]).is_err());
    }

    #[test]
    fn angle_takes_equal_dimensions_and_returns_a_dimensionless_angle() {
        let len = |v: f64| Value::Scalar(Scalar::real(v, Dimension::base(0)));
        let time = Value::Scalar(Scalar::real(1.0, Dimension::base(2)));
        let a = angle_fn(&ctx(), &[len(1.0), len(1.0)]).unwrap();
        match &a {
            Value::Scalar(s) => {
                assert!(s.dim.is_dimensionless());
                close(s.value.re, PI / 4.0, 1e-14);
            }
            o => panic!("{o:?}"),
        }
        close(num(angle_fn(&ctx(), &[len(-2.0), len(0.0)])), PI, 1e-14);
        close(
            num(angle_fn(&ctx(), &[len(0.0), len(-3.0)])),
            1.5 * PI,
            1e-14,
        );
        assert!(matches!(
            angle_fn(&ctx(), &[len(1.0), time]),
            Err(EvalError::DimensionMismatch { .. })
        ));
        assert!(angle_fn(&ctx(), &[len(0.0), len(0.0)]).is_err());
    }

    #[test]
    fn polylog_negative_orders_are_exact() {
        // References: exact sum of k^m x^k at the f64 argument (two exact methods agree).
        close(
            num(polylog_fn(&ctx(), &[sc(-10.0), sc(-0.9)])),
            -8.837_504_267_469_951,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-20.0), sc(-0.9)])),
            1.129_738_226_691_580_4e8,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-5.0), sc(-0.89)])),
            -0.242_844_808_735_064,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-2.0), sc(-0.9)])),
            -0.013_121_446_274_967_193,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-1.0), sc(-0.9)])),
            -0.249_307_479_224_376_7,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-3.0), sc(-0.5)])),
            2.0 / 27.0,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-4.0), sc(-0.5)])),
            1.0 / 8.1,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-7.0), sc(0.3)])),
            1_141.555_947_898_288,
            1e-13,
        );
        close(
            num(polylog_fn(&ctx(), &[sc(-30.0), sc(-0.5)])),
            -4.269_041_878_000_547e16,
            1e-13,
        );
        // Li_0(x) = x / (1 - x)
        close(num(polylog_fn(&ctx(), &[sc(0.0), sc(0.5)])), 1.0, 1e-14);
        assert_eq!(num(polylog_fn(&ctx(), &[sc(-3.0), sc(0.0)])), 0.0);
        assert!(polylog_fn(&ctx(), &[sc(-2.0), sc(1.0)]).is_err());
        assert!(polylog_fn(&ctx(), &[sc(-2.0), sc(2.0)]).is_err());
    }

    #[test]
    fn fact2_values() {
        assert_eq!(num(fact2_fn(&ctx(), &[sc(7.0)])), 105.0);
        assert_eq!(num(fact2_fn(&ctx(), &[sc(8.0)])), 384.0);
        assert_eq!(num(fact2_fn(&ctx(), &[sc(-1.0)])), 1.0);
        assert!(fact2_fn(&ctx(), &[sc(-2.0)]).is_err());
        assert!(fact2_fn(&ctx(), &[sc(1.0e9)]).is_err());
    }

    #[test]
    fn beta_zeta_values() {
        close(num(beta_fn(&ctx(), &[sc(2.0), sc(3.0)])), 1.0 / 12.0, 1e-13);
        assert!(beta_fn(&ctx(), &[sc(0.0), sc(3.0)]).is_err());
        close(zeta(2.0).unwrap(), PI * PI / 6.0, 1e-14);
        close(zeta(3.0).unwrap(), 1.202_056_903_159_594_2, 1e-14);
        close(zeta(0.5).unwrap(), -1.460_354_508_809_586_8, 1e-13);
        close(zeta(0.0).unwrap(), -0.5, 1e-13);
        close(zeta(-1.0).unwrap(), -1.0 / 12.0, 1e-12);
        close(zeta(-3.0).unwrap(), 1.0 / 120.0, 1e-12);
        assert_eq!(zeta(-2.0).unwrap(), 0.0);
        assert!(zeta(1.0).is_err());
    }

    #[test]
    fn dilog_polylog_values() {
        close(li2(0.5).unwrap(), 0.582_240_526_465_012_5, 1e-14);
        close(li2(-1.0).unwrap(), -PI * PI / 12.0, 1e-14);
        close(li2(0.9).unwrap(), 1.299_714_723_004_958_8, 1e-13);
        close(li2(-3.0).unwrap(), -1.939_375_420_766_708, 1e-12);
        assert!(li2(2.0).is_err());
        close(
            num(dilog_fn(&ctx(), &[sc(0.5)])),
            0.582_240_526_465_012_5,
            1e-14,
        );
        close(num(dilog_fn(&ctx(), &[sc(1.0)])), 0.0, 1e-14);
        let p = |n, x| num(polylog_fn(&ctx(), &[sc(n), sc(x)]));
        close(p(1.0, 0.5), 2f64.ln(), 1e-14);
        close(p(2.0, 0.5), 0.582_240_526_465_012_5, 1e-14);
        // Li3(0.5) = 0.5372131936080402
        close(p(3.0, 0.5), 0.537_213_193_608_040_2, 1e-14);
        // Li0(x) = x / (1 - x)
        close(p(0.0, 0.5), 1.0, 1e-14);
        close(p(3.0, 1.0), 1.202_056_903_159_594_2, 1e-14);
        assert!(polylog_fn(&ctx(), &[sc(3.0), sc(0.95)]).is_err());
        assert!(polylog_fn(&ctx(), &[sc(2.5), sc(0.5)]).is_err());
    }
}

//! Scalar root finding and polynomial roots (help: finding_roots).
//!
//! - `root(f(x), x, [a, b])`: the root of `f` in the interval, by Brent's method (a bracketing
//!   method: the ends must have opposite signs). `f` is an expression of the bound variable `x`
//!   (see `special_forms`). A function with no sign change in the interval is refused, and so is a
//!   sign change that is not a root (a pole or a jump): the value at the answer must be small
//!   compared with the function's values at the ends. The worksheet's `TOL` is not used: the
//!   answer is computed to machine precision, which is at least as close as `TOL` allows.
//! - `root(f(x), x)`, the guess form (help: finding_roots): the root of `f` near the current value
//!   of `x`, by the secant method. The parser reads a two-argument `root` as this form only when
//!   the second argument is a plain name that the first one uses (`root(x^2 - 2, x)`); otherwise
//!   it is the nth root (`root(27, 3)`, `root(27, n)`). Each evaluation is charged
//!   ([`SAMPLE_UNITS`]) and the iteration is capped at [`MAX_GUESS_ITERATIONS`]. A result is
//!   returned only when the steps have settled and the residual is small against the function's
//!   starting values. A run-away secant (a flat start) falls back to a bracket search from the guess
//!   with Brent's method; a flat secant, or a pole with no bracket, is refused as not converged.
//! - `polyroots(v)`: the roots of the polynomial whose coefficients are `v`, lowest power first
//!   (2 to 99 coefficients). Durand-Kerner iteration on the monic polynomial, then three Newton
//!   steps per root. The roots are returned as a column sorted by real part, then imaginary part.
//!   A double root is returned as a pair at its midpoint. A repeated root of higher multiplicity,
//!   or two roots within 1e-5 of each other, is refused as not converged rather than returned as
//!   noisy values (Mathcad returns such a cluster; this does not).

use num_complex::Complex64;

use super::builtins::{Arity, Registry};
use super::closure::uses_variable;
use super::error::EvalError;
use super::expr_eval::Ctx;
use super::special_forms::{bound_name, eval_with, value_of, SAMPLE_UNITS};
use super::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use crate::expr::Expr;
use crate::units::Dimension;

/// Iterations Brent's method may take. Each takes one function evaluation, so the budget bounds
/// it too.
const MAX_ROOT_ITERATIONS: usize = 200;

/// A root's residual must be at most this fraction of the function's larger end value. Rejects a
/// pole or a jump, where the sign change has no zero.
const ROOT_RESIDUAL_RELATIVE: f64 = 1e-9;

/// Secant steps the guess form may take, the relative step at which it stops, and the residual
/// (relative to the function's starting values) it must reach.
const MAX_GUESS_ITERATIONS: usize = 100;
const GUESS_STEP_TOL: f64 = 1e-12;
const GUESS_RESIDUAL_RELATIVE: f64 = 1e-10;
/// A secant step longer than this many times the current scale is a run-away, and the guess form
/// falls back to a bracket search: the first doubling step of that search is `BRACKET_FIRST_STEP`
/// relative, and it takes at most `BRACKET_STEPS` doublings.
const GUESS_RUNAWAY: f64 = 10.0;
const BRACKET_FIRST_STEP: f64 = 1e-3;
const BRACKET_STEPS: usize = 64;

/// Absolute floor of the root's x tolerance, for roots at zero.
const ROOT_X_FLOOR: f64 = 1e-15;

/// Iterations Durand-Kerner may take, and the relative size of its last step. Roundoff keeps the
/// iterates moving at about 1e-15 relative, so the step tolerance sits above that.
const MAX_POLY_ITERATIONS: usize = 2000;
const POLY_STEP_TOL: f64 = 1e-12;

/// Iterations without a new smallest step after which the steps count as stalled at the rounding
/// floor, and the relative step at which a stalled iteration is accepted (see `durand_kerner`).
const POLY_STALL_ITERATIONS: usize = 50;
const POLY_FLOOR_TOL: f64 = 1e-3;

/// Newton steps that polish each root after the iteration (each kept only if it lowers `|p|`).
const POLY_POLISH_STEPS: usize = 3;

/// Two roots closer than this, relative to the larger of them (or to 1), are a cluster. The spec
/// figure was 1e-5, but a quadruple root's iterates spread about 1e-4 (eps^(1/4)), so that window
/// misses them; 1e-3 catches every multiplicity up to about five, and still refuses any cluster
/// the 1e-5 window would.
const POLY_CLUSTER_TOL: f64 = 1e-3;

/// Relative size of `p(m)` (against the sum of `|c_k| |m|^k`) at which a pair's midpoint `m` counts
/// as a double root.
const POLY_DOUBLE_ROOT_TOL: f64 = 1e-14;

/// Largest polynomial (in coefficients) `polyroots` accepts, as the help states.
const MAX_POLY_COEFFICIENTS: usize = 99;

pub(super) fn register(r: &mut Registry) {
    r.register("polyroots", Arity::Exact(1), polyroots);
}

/// `root(f(x), x)` (the guess form) or `root(f(x), x, [a, b])` (the bracketed form).
pub(super) fn root_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    match args {
        [body, var] => root_with_guess(body, var, ctx),
        [body, var, interval] => root_in_interval(body, var, interval, ctx),
        _ => Err(EvalError::Unsupported(
            "root takes two or three arguments".to_string(),
        )),
    }
}

/// `root(f(x), x)`: the root of `f` near the current value of `x` (the guess), by the secant
/// method. The result has the guess's units.
fn root_with_guess(body: &Expr, var: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let key = bound_name(var)?;
    if !uses_variable(body, &key) {
        return Err(EvalError::Unsupported(format!(
            "root(f(x), x): f must depend on {key}"
        )));
    }
    let guess = match value_of(var, ctx)? {
        Value::Scalar(s) => s,
        _ => {
            return Err(EvalError::Unsupported(
                "the guess for root must be a scalar".to_string(),
            ))
        }
    };
    let dim = guess.dim;
    let f = |z: Complex64| -> Result<Complex64, EvalError> {
        ctx.budget.charge(SAMPLE_UNITS)?;
        let bound = [(key.clone(), Value::Scalar(Scalar::new(z, dim)))];
        match eval_with(body, ctx, &bound)? {
            Value::Scalar(s) if s.value.re.is_finite() && s.value.im.is_finite() => Ok(s.value),
            Value::Scalar(_) => Err(EvalError::NotFinite),
            _ => Err(EvalError::Unsupported(
                "root needs a scalar function".to_string(),
            )),
        }
    };
    let z = secant(f, guess.value)?;
    Ok(Value::Scalar(Scalar::new(z, dim)))
}

/// Secant iteration from `guess` (and a nearby second point). Converged when a step is at most
/// [`GUESS_STEP_TOL`] relative and the residual is at most [`GUESS_RESIDUAL_RELATIVE`] of the
/// function's starting values; anything else is refused as not converged.
fn secant<F>(mut f: F, guess: Complex64) -> Result<Complex64, EvalError>
where
    F: FnMut(Complex64) -> Result<Complex64, EvalError>,
{
    let refused = || EvalError::NoConvergence("root(f(x), x) from the guess".to_string());
    let mut x0 = guess;
    let mut f0 = f(x0)?;
    if f0.norm() == 0.0 {
        return Ok(x0);
    }
    let mut x1 = guess + Complex64::new(1e-3 * (1.0 + guess.norm()), 0.0);
    let mut f1 = f(x1)?;
    let scale = f0.norm().max(f1.norm());
    for _ in 0..MAX_GUESS_ITERATIONS {
        if f1.norm() == 0.0 {
            return Ok(x1);
        }
        if f1 == f0 {
            // A flat secant: no step can be taken from here.
            return Err(refused());
        }
        let x2 = x1 - f1 * (x1 - x0) / (f1 - f0);
        if !x2.re.is_finite() || !x2.im.is_finite() {
            return Err(refused());
        }
        let step = (x2 - x1).norm();
        if step > GUESS_RUNAWAY * (1.0 + x1.norm()) {
            // A flat start (`x^3 - 1` at 0) sends the secant far away; a real root is still found by
            // bracketing from the guess.
            return bracket_from_guess(&mut f, guess);
        }
        x0 = x1;
        f0 = f1;
        x1 = x2;
        f1 = f(x1)?;
        if step <= GUESS_STEP_TOL * (1.0 + x1.norm()) {
            return if f1.norm() <= GUESS_RESIDUAL_RELATIVE * scale {
                Ok(x1)
            } else {
                Err(refused())
            };
        }
    }
    Err(refused())
}

/// The guess form's bracket search, used when the secant runs away: the real points `guess ± d`
/// with `d` doubling from [`BRACKET_FIRST_STEP`] (relative), until `f` changes sign against the
/// guess; then Brent's method on that bracket. Points where `f` is complex or not finite are
/// skipped. A guess that is complex, or no sign change within the steps, is refused.
fn bracket_from_guess<F>(f: &mut F, guess: Complex64) -> Result<Complex64, EvalError>
where
    F: FnMut(Complex64) -> Result<Complex64, EvalError>,
{
    let refused = || EvalError::NoConvergence("root(f(x), x) from the guess".to_string());
    if guess.im != 0.0 {
        return Err(refused());
    }
    let x0 = guess.re;
    let Some(f0) = real_value(f, x0)? else {
        return Err(refused());
    };
    if f0 == 0.0 {
        return Ok(guess);
    }
    let mut step = BRACKET_FIRST_STEP * (1.0 + x0.abs());
    for _ in 0..BRACKET_STEPS {
        for x in [x0 + step, x0 - step] {
            let Some(fx) = real_value(f, x)? else {
                continue;
            };
            if fx == 0.0 {
                return Ok(Complex64::new(x, 0.0));
            }
            if (fx > 0.0) != (f0 > 0.0) {
                let (lo, hi) = (x0.min(x), x0.max(x));
                let r = brent(
                    |t: f64| real_value(f, t)?.ok_or(EvalError::NotFinite),
                    lo,
                    hi,
                )?;
                return Ok(Complex64::new(r, 0.0));
            }
        }
        step *= 2.0;
    }
    Err(refused())
}

/// `f` at the real point `x`: its value when it is real and finite, `None` when it is complex or
/// not finite (outside the function's domain).
fn real_value<F>(f: &mut F, x: f64) -> Result<Option<f64>, EvalError>
where
    F: FnMut(Complex64) -> Result<Complex64, EvalError>,
{
    match f(Complex64::new(x, 0.0)) {
        Ok(v) if v.im == 0.0 => Ok(Some(v.re)),
        Ok(_) | Err(EvalError::NotFinite) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `root(f(x), x, [a, b])`: the root of `f` between `a` and `b`, in the units of `a`.
fn root_in_interval(
    body: &Expr,
    var: &Expr,
    interval: &Expr,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let key = bound_name(var)?;
    let (lo, hi, dim) = bracket(value_of(interval, ctx)?)?;
    // The help requires a < b; a reversed interval is refused rather than silently swapped.
    if lo >= hi {
        return Err(EvalError::Unsupported(
            "root: the interval [a, b] needs a < b".to_string(),
        ));
    }
    let f = |x: f64| -> Result<f64, EvalError> {
        ctx.budget.charge(SAMPLE_UNITS)?;
        let bound = [(key.clone(), Value::Scalar(Scalar::real(x, dim)))];
        match eval_with(body, ctx, &bound)? {
            Value::Scalar(s) if s.value.im == 0.0 && s.value.re.is_finite() => Ok(s.value.re),
            Value::Scalar(s) if s.value.im != 0.0 => Err(EvalError::Unsupported(
                "root of a complex value: the function must be real on the interval".to_string(),
            )),
            Value::Scalar(_) => Err(EvalError::NotFinite),
            _ => Err(EvalError::Unsupported(
                "root needs a scalar function".to_string(),
            )),
        }
    };
    let x = brent(f, lo, hi)?;
    Ok(Value::Scalar(Scalar::real(x, dim)))
}

/// The two ends of the bracket as real numbers, and their shared dimension.
fn bracket(v: Value) -> Result<(f64, f64, Dimension), EvalError> {
    let Value::Matrix(m) = v else {
        return Err(EvalError::Unsupported(
            "the interval of root must be a vector [a, b]".to_string(),
        ));
    };
    let [a, b] = m.data() else {
        return Err(EvalError::Unsupported(
            "the interval of root must have two ends [a, b]".to_string(),
        ));
    };
    if a.dim != b.dim {
        return Err(EvalError::DimensionMismatch {
            left: a.dim,
            right: b.dim,
        });
    }
    if a.value.im != 0.0 || b.value.im != 0.0 {
        return Err(EvalError::Unsupported(
            "the interval of root must be real".to_string(),
        ));
    }
    if !a.value.re.is_finite() || !b.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok((a.value.re, b.value.re, a.dim))
}

/// Brent's method on `[lo, hi]` (Numerical Recipes `zbrent`). Accepts only a sign change whose
/// residual is small (see [`ROOT_RESIDUAL_RELATIVE`]).
fn brent<F>(mut f: F, lo: f64, hi: f64) -> Result<f64, EvalError>
where
    F: FnMut(f64) -> Result<f64, EvalError>,
{
    let fa0 = f(lo)?;
    let fb0 = f(hi)?;
    if fa0 == 0.0 {
        return Ok(lo);
    }
    if fb0 == 0.0 {
        return Ok(hi);
    }
    if (fa0 > 0.0) == (fb0 > 0.0) {
        return Err(EvalError::NoSignChange(
            "root: f(a) and f(b) have the same sign, so the interval brackets no root".to_string(),
        ));
    }
    let (mut a, mut b, mut fa, mut fb) = (lo, hi, fa0, fb0);
    let (mut c, mut fc) = (a, fa);
    let mut d = b - a;
    let mut e = d;
    for _ in 0..MAX_ROOT_ITERATIONS {
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * ROOT_X_FLOOR;
        let xm = 0.5 * (c - b);
        if xm.abs() <= tol1 || fb == 0.0 {
            return accept(b, fb, fa0, fb0);
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            // Inverse quadratic or secant interpolation, when it stays inside the bracket.
            let s = fb / fa;
            let (mut p, mut q);
            if a == c {
                p = 2.0 * xm * s;
                q = 1.0 - s;
            } else {
                let qa = fa / fc;
                let r = fb / fc;
                p = s * (2.0 * xm * qa * (qa - r) - (b - a) * (r - 1.0));
                q = (qa - 1.0) * (r - 1.0) * (s - 1.0);
            }
            if p > 0.0 {
                q = -q;
            }
            p = p.abs();
            let min1 = 3.0 * xm * q - (tol1 * q).abs();
            let min2 = (e * q).abs();
            if 2.0 * p < min1.min(min2) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        b += if d.abs() > tol1 { d } else { tol1.copysign(xm) };
        fb = match f(b) {
            // The iteration reached a point where the function is not finite: inside a sign
            // change, that is a pole, the same refusal as a pole with a finite value.
            Err(EvalError::NotFinite) => return Err(pole_or_jump()),
            other => other?,
        };
    }
    Err(EvalError::NoConvergence("root".to_string()))
}

/// The converged point, refused when its function value is not small against the ends (a pole or
/// a jump in the interval).
fn accept(x: f64, fx: f64, fa0: f64, fb0: f64) -> Result<f64, EvalError> {
    if fx.abs() <= ROOT_RESIDUAL_RELATIVE * fa0.abs().max(fb0.abs()) {
        Ok(x)
    } else {
        Err(pole_or_jump())
    }
}

/// The refusal of a sign change with no root in it: a pole or a jump.
#[cold]
fn pole_or_jump() -> EvalError {
    EvalError::NoSignChange("root: the sign change is a pole or a jump, not a root".to_string())
}

/// `polyroots(v)`: all roots of the polynomial with coefficients `v` (constant term first).
fn polyroots(args: &[Value]) -> Result<Value, EvalError> {
    let Value::Matrix(m) = &args[0] else {
        return Err(EvalError::Unsupported(
            "polyroots takes a vector of coefficients".to_string(),
        ));
    };
    if m.data().len() > MAX_POLY_COEFFICIENTS {
        return Err(EvalError::TooLarge(m.data().len()));
    }
    let mut coeffs: Vec<Complex64> = Vec::with_capacity(m.data().len());
    for s in m.data() {
        if !s.dim.is_dimensionless() {
            return Err(EvalError::DimensionNotDimensionless { dim: s.dim });
        }
        coeffs.push(s.value);
    }
    // The leading (highest) zeros do not change the roots; they are dropped.
    while coeffs
        .last()
        .is_some_and(|c| *c == Complex64::new(0.0, 0.0))
    {
        coeffs.pop();
    }
    if coeffs.len() < 2 {
        return Err(EvalError::Unsupported(
            "polyroots needs a polynomial of degree at least 1".to_string(),
        ));
    }
    if coeffs
        .iter()
        .any(|c| !c.re.is_finite() || !c.im.is_finite())
    {
        return Err(EvalError::NotFinite);
    }
    let mut roots = durand_kerner(&coeffs)?;
    roots.sort_by(|x, y| x.re.total_cmp(&y.re).then(x.im.total_cmp(&y.im)));
    let data: Vec<Scalar> = roots
        .into_iter()
        .map(|z| Scalar::new(z, Dimension::dimensionless()))
        .collect();
    if data.len() > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(data.len()));
    }
    Ok(Value::Matrix(Matrix::new(data.len(), 1, data)?))
}

/// The roots of `sum coeffs[k] z^k` (`coeffs` has a nonzero last element and at least two entries).
fn durand_kerner(coeffs: &[Complex64]) -> Result<Vec<Complex64>, EvalError> {
    let n = coeffs.len() - 1;
    let lead = coeffs[n];
    // Monic coefficients: z^n + sum_{k<n} a[k] z^k.
    let a: Vec<Complex64> = coeffs[..n].iter().map(|c| c / lead).collect();
    // Horner on the monic polynomial: start from the leading 1, multiply by z, add a[k].
    let eval_p = |z: Complex64| -> Complex64 {
        let mut acc = Complex64::new(1.0, 0.0);
        for ak in a.iter().rev() {
            acc = acc * z + ak;
        }
        acc
    };
    // Starting points: powers of (0.4 + 0.9i), the usual non-symmetric choice.
    let seed = Complex64::new(0.4, 0.9);
    let mut z: Vec<Complex64> = (0..n).map(|k| seed.powi(k as i32)).collect();
    // The smallest step seen, and the iterations since it last fell: at the rounding floor the
    // steps stop falling and hover there.
    let mut best_step = f64::INFINITY;
    let mut stalled = 0usize;
    for _ in 0..MAX_POLY_ITERATIONS {
        let mut largest_step: f64 = 0.0;
        let mut largest_root: f64 = 0.0;
        for i in 0..n {
            let mut denom = Complex64::new(1.0, 0.0);
            for j in 0..n {
                if j != i {
                    denom *= z[i] - z[j];
                }
            }
            if denom.norm() == 0.0 {
                // Two estimates coincide: nudge one (the iteration cannot divide by zero).
                z[i] += Complex64::new(1e-10, 1e-10);
                largest_step = f64::INFINITY;
                continue;
            }
            let step = eval_p(z[i]) / denom;
            z[i] -= step;
            largest_step = largest_step.max(step.norm());
            largest_root = largest_root.max(z[i].norm());
        }
        if !largest_step.is_finite() {
            return Err(EvalError::NotFinite);
        }
        let scale = 1.0 + largest_root;
        if largest_step <= POLY_STEP_TOL * scale {
            return finish(z, &a, coeffs);
        }
        if largest_step < best_step {
            best_step = largest_step;
            stalled = 0;
        } else {
            stalled += 1;
        }
        if stalled >= POLY_STALL_ITERATIONS {
            // The steps hover at the rounding floor of this polynomial. That is converged when the
            // floor is small; the Newton polish then finishes the job.
            if best_step <= POLY_FLOOR_TOL * scale {
                return finish(z, &a, coeffs);
            }
            break;
        }
    }
    Err(EvalError::NoConvergence("polyroots".to_string()))
}

/// The converged iterates, polished and checked for clusters.
fn finish(
    mut z: Vec<Complex64>,
    a: &[Complex64],
    coeffs: &[Complex64],
) -> Result<Vec<Complex64>, EvalError> {
    polish(&mut z, a);
    resolve_clusters(&mut z, coeffs)?;
    Ok(z)
}

/// The monic polynomial `z^n + sum a[k] z^k` (`a` has `n` entries) and its derivative at `z`.
fn monic_value(a: &[Complex64], z: Complex64) -> (Complex64, Complex64) {
    let mut p = Complex64::new(1.0, 0.0);
    let mut dp = Complex64::new(0.0, 0.0);
    for ak in a.iter().rev() {
        dp = dp * z + p;
        p = p * z + ak;
    }
    (p, dp)
}

/// Newton steps on each root of the monic polynomial. A step is kept only when it lowers `|p|`, so
/// a root that is already at roundoff, or a multiple root where `p'` vanishes, is left alone.
fn polish(z: &mut [Complex64], a: &[Complex64]) {
    for zi in z.iter_mut() {
        for _ in 0..POLY_POLISH_STEPS {
            let (p, dp) = monic_value(a, *zi);
            if dp.norm() == 0.0 {
                break;
            }
            let next = *zi - p / dp;
            if !next.re.is_finite() || !next.im.is_finite() {
                break;
            }
            if monic_value(a, next).0.norm() > p.norm() {
                break;
            }
            *zi = next;
        }
    }
}

/// Refuses repeated roots that the iteration cannot separate. A cluster of roots within
/// [`POLY_CLUSTER_TOL`] of each other is refused as not converged (Mathcad returns the cluster as
/// noisy values; this refuses rather than return them), except a pair that is a double root: its
/// midpoint `m` has `p(m)` zero to working precision, and then both roots are `m`.
fn resolve_clusters(z: &mut [Complex64], coeffs: &[Complex64]) -> Result<(), EvalError> {
    let n = z.len();
    let close = |u: Complex64, v: Complex64| {
        (u - v).norm() <= POLY_CLUSTER_TOL * u.norm().max(v.norm()).max(1.0)
    };
    let refused = || EvalError::NoConvergence("polyroots: a repeated root".to_string());
    let mut pairs = Vec::new();
    for i in 0..n {
        let near_i: Vec<usize> = (0..n).filter(|&j| j != i && close(z[i], z[j])).collect();
        match near_i.as_slice() {
            [] => {}
            [j] => {
                let j = *j;
                let near_j: Vec<usize> = (0..n).filter(|&k| k != j && close(z[j], z[k])).collect();
                if near_j != [i] {
                    return Err(refused());
                }
                if j > i {
                    pairs.push((i, j));
                }
            }
            _ => return Err(refused()),
        }
    }
    for (i, j) in pairs {
        let m = (z[i] + z[j]) * 0.5;
        if !is_double_root(m, coeffs) {
            return Err(refused());
        }
        z[i] = m;
        z[j] = m;
    }
    Ok(())
}

/// Whether `p(m)` vanishes to working precision at `m`: `|p(m)|` is at most
/// [`POLY_DOUBLE_ROOT_TOL`] times the sum of `|c_k| |m|^k`.
fn is_double_root(m: Complex64, coeffs: &[Complex64]) -> bool {
    let mut value = Complex64::new(0.0, 0.0);
    let mut scale = 0.0;
    let mag = m.norm();
    for c in coeffs.iter().rev() {
        value = value * m + c;
        scale = scale * mag + c.norm();
    }
    value.norm() <= POLY_DOUBLE_ROOT_TOL * scale
}

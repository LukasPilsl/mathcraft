//! Numeric calculus operators (help: summation_operator, product_operator, integral_operator,
//! derivative_and_prime_operators, numerical_integration_methods).
//!
//! - Summation and product over a range variable: terms `i = m, m+1, ...` while `i <= n`, with
//!   real limits allowed. An empty range gives 0 or 1. The vector form folds all elements.
//! - Definite integral: adaptive Gauss-Kronrod (G7K15), bisecting the panel with the largest
//!   error estimate, at most [`MAX_PANELS`] panels. Infinite limits use the standard
//!   substitutions; the endpoints are never evaluated. A limit at infinity is a half line, and a
//!   whole-line integral is the sum of its two halves, each of which must converge on its own
//!   (so `sin` over the real line is refused, not taken as 0). The result is accepted only when
//!   its error estimate meets the tolerance; otherwise it is an error ("did not converge").
//! - Derivative of order `n` (0 to 5, help): Richardson extrapolation of symmetric divided
//!   differences over halving steps, taking the estimate with the smallest error. It is refused
//!   when no estimate meets the tolerance of its order (see [`derivative_tol`]). A first
//!   derivative at a corner (`abs` at 0) is refused too (see [`check_corner`]).
//!
//! The bound variable is a local that shadows the worksheet. Units: an integral has the unit of
//! integrand times variable; an nth derivative has the unit of the function over the variable to
//! the nth power. Every evaluation is charged to the statement's work budget.

use num_complex::Complex64;
use num_rational::Ratio;

use super::error::EvalError;
use super::expr_eval::{eval, sub, var_key, Ctx};
use super::lookup::lookup;
use super::value::{Matrix, Scalar, Value};
use super::work::ELEMENT_UNITS;
use crate::expr::{Accumulation, Expr, Id, UnaryOp};
use crate::units::Dimension;

/// Terms one summation or product may run. Larger ranges are refused before any term runs.
pub const MAX_TERMS: usize = 1_000_000;

/// Panels one adaptive integral may hold. Each panel costs 15 integrand evaluations.
pub const MAX_PANELS: usize = 500;

/// Highest derivative order (help: numeric evaluation takes n from 0 to 5).
pub const MAX_DERIVATIVE_ORDER: i32 = 5;

/// Work units charged per summation term, on top of the work of the term itself.
const TERM_UNITS: u64 = 16;

/// Work units charged per integrand sample, on top of the work of the expression.
const SAMPLE_UNITS: u64 = 10;

/// Work units charged per derivative sample. A derivative sample is four times as dear as an
/// integrand sample: a nested derivative multiplies the samples of every level, so the budget must
/// stop a deep nest in seconds, not minutes (see the nested-derivative test).
const DERIVATIVE_SAMPLE_UNITS: u64 = 4 * SAMPLE_UNITS;

/// Richardson levels of the derivative table (steps `h0 / 2^k`, k < this).
const DERIVATIVE_LEVELS: usize = 10;

/// Relative accuracy an integral aims for. Its absolute floor is `ABS_FLOOR` times the integral of
/// the absolute value of the integrand, so a tiny-valued integrand is not accepted at a fixed
/// absolute error.
///
/// TOL. Mathcad's default TOL is 0.001, a much looser target. This one is 1e-10 on purpose: the
/// integrator's own error must stay far below the digits a worksheet shows, so a golden
/// comparison at 1e-9 tests the formula, not the quadrature. A result that Mathcad gives with
/// only its 0.001 accuracy may therefore differ from ours in the digits Mathcad does not show.
const INTEGRAL_REL_TOL: f64 = 1e-10;
const ABS_FLOOR: f64 = 1e-12;

/// Relative accuracy of a first-order derivative. A derivative of order `n` is allowed a decade
/// more per order (see [`derivative_tol`]), since Mathcad loses about a digit per order.
const DERIVATIVE_TOL: f64 = 1e-9;

/// Relative gap between the one-sided difference quotients that marks a corner (see
/// [`check_corner`]).
const CORNER_REL_TOL: f64 = 1e-6;

/// The accepted relative error of a derivative of order `n` (1 to [`MAX_DERIVATIVE_ORDER`]):
/// `DERIVATIVE_TOL` for the first, then a decade looser per order. A value whose error estimate is
/// above this is never returned.
pub fn derivative_tol(n: i32) -> f64 {
    DERIVATIVE_TOL * 10f64.powi(n - 1)
}

/// Sum or product over `var` from `from` to `to` (see [`Expr::Accumulate`]).
pub(super) fn accumulate(
    op: Accumulation,
    var: &Id,
    from: &Expr,
    to: &Expr,
    body: &Expr,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let lo = real_dimensionless(scalar_of(sub(from, ctx)?)?)?;
    let hi = real_dimensionless(scalar_of(sub(to, ctx)?)?)?;
    // Terms are `lo + k` for `k = 0, 1, ...` while `<= hi`. The slack that absorbs rounding in
    // `hi - lo` is relative to the limits, so `0 .. 0.9999999999` is one term, not two.
    let span = hi - lo;
    let count = if hi < lo {
        0.0
    } else {
        (span + 1e-12 * lo.abs().max(hi.abs()).max(1.0)).floor() + 1.0
    };
    if count > MAX_TERMS as f64 {
        return Err(EvalError::TooManyTerms {
            terms: count,
            limit: MAX_TERMS,
        });
    }
    let mut binding = Binding::new(var, ctx)?;
    let mut acc: Option<Scalar> = None;
    for k in 0..count as usize {
        ctx.budget.charge(TERM_UNITS)?;
        let term = binding.scalar(body, ctx, lo + k as f64, dimensionless())?;
        acc = Some(combine(op, acc, term)?);
    }
    Ok(Value::Scalar(acc.unwrap_or_else(|| identity(op))))
}

/// Sum or product of every element of a vector (the vector form, see [`Expr::Fold`]).
pub(super) fn fold(op: Accumulation, operand: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let value = sub(operand, ctx)?;
    let elements: Vec<Scalar> = match value {
        Value::Scalar(s) => vec![s],
        Value::Matrix(m) => m.data().to_vec(),
        _ => {
            return Err(EvalError::Unsupported(
                "sum and product take a vector or a matrix".to_string(),
            ))
        }
    };
    ctx.budget.charge(ELEMENT_UNITS * elements.len() as u64)?;
    let mut acc: Option<Scalar> = None;
    for e in elements {
        acc = Some(combine(op, acc, e)?);
    }
    Ok(Value::Scalar(acc.unwrap_or_else(|| identity(op))))
}

/// Definite integral of `body` in `var` from `lower` to `upper` (see [`Expr::Integral`]).
pub(super) fn integral(
    var: &Id,
    lower: &Expr,
    upper: &Expr,
    body: &Expr,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let lo = bound(lower, ctx)?;
    let hi = bound(upper, ctx)?;
    // The variable's unit is the unit of the finite limits; two infinite limits give none.
    let var_dim = match (lo, hi) {
        (Bound::Finite(a), Bound::Finite(b)) => {
            if a.dim != b.dim {
                return Err(EvalError::DimensionMismatch {
                    left: a.dim,
                    right: b.dim,
                });
            }
            a.dim
        }
        (Bound::Finite(a), _) | (_, Bound::Finite(a)) => a.dim,
        _ => dimensionless(),
    };
    let mut binding = Binding::new(var, ctx)?;
    let mut body_dim: Option<Dimension> = None;
    let mut sample = |x: f64| -> Result<Complex64, EvalError> {
        ctx.budget.charge(SAMPLE_UNITS)?;
        let s = binding.scalar(body, ctx, x, var_dim)?;
        match body_dim {
            None => body_dim = Some(s.dim),
            Some(d) if d != s.dim => {
                return Err(EvalError::DimensionMismatch {
                    left: d,
                    right: s.dim,
                })
            }
            Some(_) => {}
        }
        Ok(s.value)
    };
    let total = match (lo, hi) {
        (Bound::Finite(a), Bound::Finite(b)) => adaptive(&mut sample, a.value.re, b.value.re)?,
        // Infinite limits: the upper limit +inf is `a + t/(1-t)`; the lower limit -inf is
        // `b - t/(1-t)`; both are on [0, 1) with Jacobian `1/(1-t)^2`.
        (Bound::Finite(a), Bound::Infinite(s)) => {
            let a = a.value.re;
            if s > 0.0 {
                upper_half(&mut sample, a)?
            } else {
                -lower_half(&mut sample, a)?
            }
        }
        (Bound::Infinite(s), Bound::Finite(b)) => {
            let b = b.value.re;
            if s < 0.0 {
                lower_half(&mut sample, b)?
            } else {
                -upper_half(&mut sample, b)?
            }
        }
        (Bound::Infinite(s), Bound::Infinite(t)) => {
            if s == t {
                Complex64::new(0.0, 0.0)
            } else if s < 0.0 {
                whole_line(&mut sample)?
            } else {
                -whole_line(&mut sample)?
            }
        }
    };
    let dim = match body_dim {
        Some(d) => d.checked_mul(var_dim).map_err(EvalError::Units)?,
        // The body was never sampled: an empty interval, so the value is zero.
        None => dimensionless(),
    };
    if !total.re.is_finite() || !total.im.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::new(total, dim)))
}

/// `d^n/dx^n body` at the current value of `var` (see [`Expr::Derivative`]).
pub(super) fn derivative(
    var: &Id,
    order: Option<&Expr>,
    body: &Expr,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let n = match order {
        None => 1,
        Some(e) => order_of(sub(e, ctx)?)?,
    };
    let point = match lookup(var, ctx)? {
        Value::Scalar(s) => s,
        _ => {
            return Err(EvalError::Unsupported(
                "the derivative point must be a scalar".to_string(),
            ))
        }
    };
    if point.value.im != 0.0 {
        return Err(EvalError::Unsupported(
            "a derivative at a complex point".to_string(),
        ));
    }
    let x0 = point.value.re;
    let mut binding = Binding::new(var, ctx)?;
    let mut body_dim: Option<Dimension> = None;
    let mut f = |x: f64| -> Result<Complex64, EvalError> {
        ctx.budget.charge(DERIVATIVE_SAMPLE_UNITS)?;
        let s = binding.scalar(body, ctx, x, point.dim)?;
        match body_dim {
            None => body_dim = Some(s.dim),
            Some(d) if d != s.dim => {
                return Err(EvalError::DimensionMismatch {
                    left: d,
                    right: s.dim,
                })
            }
            Some(_) => {}
        }
        Ok(s.value)
    };
    let value = match n {
        0 => f(x0)?,
        1 => {
            check_corner(&mut f, x0)?;
            richardson(&mut f, x0, n)?
        }
        _ => richardson(&mut f, x0, n)?,
    };
    let dim = match body_dim {
        Some(d) => {
            let power = Ratio::from_integer(n);
            let unit = point.dim.pow(power).map_err(EvalError::Units)?;
            d.checked_div(unit).map_err(EvalError::Units)?
        }
        None => dimensionless(),
    };
    if !value.re.is_finite() || !value.im.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::new(value, dim)))
}

/// The variables of a gradient: the listed ones, or the plain-name arguments of a call body
/// (`∇ f(a, b, c)`). A body that is not such a call needs the list.
pub(super) fn gradient_variables(vars: &[Id], body: &Expr) -> Result<Vec<Id>, EvalError> {
    if !vars.is_empty() {
        return Ok(vars.to_vec());
    }
    let Expr::Call { args, .. } = body else {
        return Err(EvalError::Unsupported(
            "the gradient needs its variables".to_string(),
        ));
    };
    let ids: Option<Vec<Id>> = args
        .iter()
        .map(|a| match a {
            Expr::Id(id) if id.subscript.is_none() => Some(id.clone()),
            _ => None,
        })
        .collect();
    match ids {
        Some(ids) if !ids.is_empty() => Ok(ids),
        _ => Err(EvalError::Unsupported(
            "the gradient needs its variables".to_string(),
        )),
    }
}

/// `∇ f`: the column vector of the partial derivatives of `body` with respect to each variable, at
/// the variables' current values. Each component has the dimension of `body` over its variable.
pub(super) fn gradient(vars: &[Id], body: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let vars = gradient_variables(vars, body)?;
    let mut out = Vec::with_capacity(vars.len());
    for v in &vars {
        match derivative(v, None, body, ctx)? {
            Value::Scalar(s) => out.push(s),
            _ => {
                return Err(EvalError::Unsupported(
                    "the gradient needs a scalar function".to_string(),
                ))
            }
        }
    }
    Ok(Value::Matrix(Matrix::new(out.len(), 1, out)?))
}

// ----- shared helpers ---------------------------------------------------------------------------

/// The bound variable as a local: a copy of the caller's locals with one slot for the variable,
/// which each evaluation overwrites.
struct Binding {
    locals: Vec<(String, Value)>,
    slot: usize,
}

impl Binding {
    fn new(var: &Id, ctx: Ctx<'_>) -> Result<Self, EvalError> {
        let mut locals = ctx.locals.to_vec();
        locals.push((
            var_key(var)?,
            Value::Scalar(Scalar::real(0.0, dimensionless())),
        ));
        let slot = locals.len() - 1;
        Ok(Self { locals, slot })
    }

    /// Evaluates `body` with the variable set to `x` (SI value) of dimension `dim`.
    fn scalar(
        &mut self,
        body: &Expr,
        ctx: Ctx<'_>,
        x: f64,
        dim: Dimension,
    ) -> Result<Scalar, EvalError> {
        self.locals[self.slot].1 = Value::Scalar(Scalar::real(x, dim));
        let inner = Ctx {
            locals: &self.locals,
            nodes: ctx.nodes + 1,
            ..ctx
        };
        match eval(body, inner)? {
            Value::Scalar(s) => Ok(s),
            _ => Err(EvalError::Unsupported(
                "the summand or integrand must be a scalar".to_string(),
            )),
        }
    }
}

fn dimensionless() -> Dimension {
    Dimension::dimensionless()
}

fn identity(op: Accumulation) -> Scalar {
    let v = match op {
        Accumulation::Sum => 0.0,
        Accumulation::Product => 1.0,
    };
    Scalar::real(v, dimensionless())
}

/// Adds or multiplies one term into the running total, refusing mixed units for a sum and a
/// non-finite result.
fn combine(op: Accumulation, acc: Option<Scalar>, term: Scalar) -> Result<Scalar, EvalError> {
    let Some(a) = acc else {
        return Ok(term);
    };
    let s = match op {
        Accumulation::Sum => {
            if a.dim != term.dim {
                return Err(EvalError::DimensionMismatch {
                    left: a.dim,
                    right: term.dim,
                });
            }
            Scalar::new(a.value + term.value, a.dim)
        }
        Accumulation::Product => Scalar::new(
            a.value * term.value,
            a.dim.checked_mul(term.dim).map_err(EvalError::Units)?,
        ),
    };
    if !s.value.re.is_finite() || !s.value.im.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s)
}

/// The scalar of a value that must be one (a limit or a term).
fn scalar_of(v: Value) -> Result<Scalar, EvalError> {
    match v {
        Value::Scalar(s) => Ok(s),
        _ => Err(EvalError::Unsupported(
            "a range limit must be a scalar".to_string(),
        )),
    }
}

/// A real, dimensionless value (a range limit or a derivative order).
fn real_dimensionless(s: Scalar) -> Result<f64, EvalError> {
    if !s.dim.is_dimensionless() {
        return Err(EvalError::DimensionNotDimensionless { dim: s.dim });
    }
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported("a complex limit".to_string()));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// A derivative order: an integer from 0 to [`MAX_DERIVATIVE_ORDER`].
fn order_of(v: Value) -> Result<i32, EvalError> {
    let s = match v {
        Value::Scalar(s) => s,
        _ => return Err(order_error()),
    };
    let x = real_dimensionless(s).map_err(|_| order_error())?;
    if x.fract() != 0.0 || !(0.0..=MAX_DERIVATIVE_ORDER as f64).contains(&x) {
        return Err(order_error());
    }
    Ok(x as i32)
}

fn order_error() -> EvalError {
    EvalError::Unsupported(format!(
        "the derivative order must be an integer from 0 to {MAX_DERIVATIVE_ORDER}"
    ))
}

/// One limit of an integral: a finite value, or an infinity with its sign.
#[derive(Clone, Copy)]
enum Bound {
    Finite(Scalar),
    Infinite(f64),
}

/// `inf` / `∞` (sign +1) and `-∞` (sign -1) are infinite limits; anything else is evaluated.
fn bound(e: &Expr, ctx: Ctx<'_>) -> Result<Bound, EvalError> {
    if let Some(sign) = infinity_sign(e) {
        return Ok(Bound::Infinite(sign));
    }
    Ok(Bound::Finite(scalar_bound(sub(e, ctx)?)?))
}

fn scalar_bound(v: Value) -> Result<Scalar, EvalError> {
    match v {
        Value::Scalar(s) if s.value.im == 0.0 && s.value.re.is_finite() => Ok(s),
        Value::Scalar(s) if s.value.im != 0.0 => Err(EvalError::Unsupported(
            "a complex limit of an integral".to_string(),
        )),
        Value::Scalar(_) => Err(EvalError::NotFinite),
        _ => Err(EvalError::Unsupported(
            "an integral limit must be a scalar".to_string(),
        )),
    }
}

/// The spellings of infinity: `∞` (Ctrl+Shift+Z in Prime, help: integral_operator) and `inf`,
/// which Prime's results show for it (help: sum_operator, definite_integrals_with_complex_limits).
fn is_infinity(name: &str) -> bool {
    name == "∞" || name == "inf"
}

fn infinity_sign(e: &Expr) -> Option<f64> {
    match e {
        Expr::Id(id) if is_infinity(&id.name) => Some(1.0),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => match operand.as_ref() {
            Expr::Id(id) if is_infinity(&id.name) => Some(-1.0),
            _ => None,
        },
        Expr::Parens(inner) => infinity_sign(inner),
        _ => None,
    }
}

// ----- adaptive Gauss-Kronrod ---------------------------------------------------------------------

/// Kronrod nodes on [0, 1] of the 15-point rule, in QUADPACK's order (the last is the centre).
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_47,
    0.0,
];

/// Kronrod weights of [`XGK`].
const WGK: [f64; 8] = [
    0.022_935_322_010_529_225,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_18,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_83,
];

/// Gauss weights: nonzero at the odd positions of [`XGK`] (the 7-point Gauss nodes) and the centre.
const WG: [f64; 8] = [
    0.0,
    0.129_484_966_168_869_7,
    0.0,
    0.279_705_391_489_276_7,
    0.0,
    0.381_830_050_505_118_9,
    0.0,
    0.417_959_183_673_469_4,
];

/// One Gauss-Kronrod panel over `[a, b]`: its integral, its error estimate and the integral of
/// `|f|` (scale for the tolerance).
#[derive(Clone, Copy)]
struct Panel {
    a: f64,
    b: f64,
    value: Complex64,
    err: f64,
    abs: f64,
}

fn panel<F>(f: &mut F, a: f64, b: f64) -> Result<Panel, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    let centre = 0.5 * (a + b);
    let half = 0.5 * (b - a);
    let fc = f(centre)?;
    let mut resk = fc * WGK[7];
    let mut resg = fc * WG[7];
    let mut resabs = fc.norm() * WGK[7];
    for j in 0..7 {
        let x = half * XGK[j];
        let f1 = f(centre - x)?;
        let f2 = f(centre + x)?;
        let sum = f1 + f2;
        resk += sum * WGK[j];
        resg += sum * WG[j];
        resabs += (f1.norm() + f2.norm()) * WGK[j];
    }
    Ok(Panel {
        a,
        b,
        value: resk * half,
        err: ((resk - resg) * half).norm(),
        abs: resabs * half.abs(),
    })
}

/// Adaptive integral of `f` over `[a, b]` (finite).
fn adaptive<F>(f: &mut F, a: f64, b: f64) -> Result<Complex64, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    if a == b {
        return Ok(Complex64::new(0.0, 0.0));
    }
    let mut panels = vec![panel(f, a, b)?];
    loop {
        let total: Complex64 = panels.iter().map(|p| p.value).sum();
        let err: f64 = panels.iter().map(|p| p.err).sum();
        let abs: f64 = panels.iter().map(|p| p.abs).sum();
        let tol = (INTEGRAL_REL_TOL * total.norm()).max(ABS_FLOOR * abs);
        if err <= tol {
            return Ok(total);
        }
        if panels.len() >= MAX_PANELS {
            return Err(EvalError::NoConvergence("integral".to_string()));
        }
        let worst = panels
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.err.total_cmp(&y.1.err))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let p = panels.swap_remove(worst);
        let mid = 0.5 * (p.a + p.b);
        panels.push(panel(f, p.a, mid)?);
        panels.push(panel(f, mid, p.b)?);
    }
}

/// Adaptive integral over `[t0, t1]` after the substitution `x = map(t)`, which returns the point
/// and the Jacobian weight `dx/dt`.
fn adaptive_mapped<F, M>(f: &mut F, t0: f64, t1: f64, map: M) -> Result<Complex64, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
    M: Fn(f64) -> (f64, f64),
{
    let mut g = |t: f64| -> Result<Complex64, EvalError> {
        let (x, w) = map(t);
        match f(x) {
            Ok(v) => Ok(v * w),
            // Past TAIL_ARG the integrand cannot be evaluated (`sin` refuses its argument), so the
            // tail does not converge as far as the integral is concerned.
            Err(EvalError::Unsupported(_) | EvalError::NotFinite) if x.abs() > TAIL_ARG => {
                Err(EvalError::NoConvergence("integral".to_string()))
            }
            Err(e) => Err(e),
        }
    };
    adaptive(&mut g, t0, t1)
}

/// Argument beyond which an infinite tail is treated as not converging when the integrand fails
/// there (see [`adaptive_mapped`]).
const TAIL_ARG: f64 = 1e12;

/// The integral over `[a, +inf)`.
fn upper_half<F>(f: &mut F, a: f64) -> Result<Complex64, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    adaptive_mapped(f, 0.0, 1.0, |t| {
        let d = 1.0 - t;
        (a + t / d, 1.0 / (d * d))
    })
}

/// The integral over `(-inf, b]`.
fn lower_half<F>(f: &mut F, b: f64) -> Result<Complex64, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    adaptive_mapped(f, 0.0, 1.0, |t| {
        let d = 1.0 - t;
        (b - t / d, 1.0 / (d * d))
    })
}

/// The integral over the whole real line, as the sum of its two halves `(-inf, 0]` and `[0, inf)`.
/// Each half must converge on its own: a pair that would cancel (`sin` over R) is an error, not 0.
fn whole_line<F>(f: &mut F) -> Result<Complex64, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    let lower = lower_half(f, 0.0)?;
    let upper = upper_half(f, 0.0)?;
    Ok(lower + upper)
}

// ----- derivatives ----------------------------------------------------------------------------

/// The nth derivative at `x0` by Richardson extrapolation of the symmetric divided difference
/// `D(h) = sum_j (-1)^j C(n, j) f(x0 + (n/2 - j) h) / h^n`, whose error is a series in `h^2`.
/// The estimate is accepted only within [`derivative_tol`]`(n)`.
fn richardson<F>(f: &mut F, x0: f64, n: i32) -> Result<Complex64, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    let scale = x0.abs().max(1.0);
    let h0 = 0.1 * scale;
    let tol = derivative_tol(n);
    let mut fmag: f64 = 0.0;
    let mut best: Option<(Complex64, f64)> = None;
    let mut prev: Vec<Complex64> = Vec::new();
    for k in 0..DERIVATIVE_LEVELS {
        let h = h0 / (1u64 << k) as f64;
        let d = divided_difference(f, x0, h, n, &mut fmag)?;
        let mut row = Vec::with_capacity(k + 1);
        row.push(d);
        for m in 1..=k {
            let factor = 4f64.powi(m as i32);
            let next = (row[m - 1] * factor - prev[m - 1]) / (factor - 1.0);
            row.push(next);
        }
        if k > 0 {
            let err = (row[k] - row[k - 1]).norm();
            if best.is_none_or(|(_, e)| err < e) {
                best = Some((row[k], err));
            }
            let target = tol * row[k].norm().max(fmag / scale);
            if err <= target {
                return Ok(row[k]);
            }
        }
        prev = row;
    }
    match best {
        Some((value, err)) if err <= tol * value.norm().max(fmag / scale) => Ok(value),
        _ => Err(EvalError::NoConvergence("derivative".to_string())),
    }
}

/// Refuses a first derivative at a corner, where the function has no derivative (`abs` at 0, a
/// jump). At the smallest table step `h` and at `h / 1000`, the one-sided difference quotients are
/// compared. At a smooth point their gap is about `f'' h`, which shrinks with the step; at a corner
/// the gap does not shrink. The gap must exceed [`CORNER_REL_TOL`] of the quotients' scale and
/// must not have shrunk by half, or the point is refused.
fn check_corner<F>(f: &mut F, x0: f64) -> Result<(), EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    let scale = x0.abs().max(1.0);
    let h_min = 0.1 * scale / (1u64 << (DERIVATIVE_LEVELS - 1)) as f64;
    let f0 = f(x0)?;
    let mut fmag = f0.norm();
    let mut gaps = [0.0f64; 2];
    let mut sizes = [0.0f64; 2];
    for (i, h) in [h_min, h_min / 1000.0].into_iter().enumerate() {
        let plus = f(x0 + h)?;
        let minus = f(x0 - h)?;
        fmag = fmag.max(plus.norm()).max(minus.norm());
        let right = (plus - f0) / h;
        let left = (f0 - minus) / h;
        gaps[i] = (right - left).norm();
        sizes[i] = right.norm().max(left.norm());
    }
    let size = sizes[1].max(fmag / scale);
    if gaps[1] > CORNER_REL_TOL * size && gaps[1] > 0.5 * gaps[0] {
        return Err(EvalError::NoConvergence("derivative".to_string()));
    }
    Ok(())
}

/// `D(h)` for the nth derivative, with `fmag` tracking the largest sampled magnitude.
fn divided_difference<F>(
    f: &mut F,
    x0: f64,
    h: f64,
    n: i32,
    fmag: &mut f64,
) -> Result<Complex64, EvalError>
where
    F: FnMut(f64) -> Result<Complex64, EvalError>,
{
    let mut acc = Complex64::new(0.0, 0.0);
    let mut coeff = 1.0f64; // C(n, j) with sign (-1)^j
    for j in 0..=n {
        if j > 0 {
            coeff = -coeff * f64::from(n - j + 1) / f64::from(j);
        }
        let offset = (f64::from(n) / 2.0 - f64::from(j)) * h;
        let v = f(x0 + offset)?;
        *fmag = fmag.max(v.norm());
        acc += v * coeff;
    }
    Ok(acc / h.powi(n))
}

#[cfg(test)]
#[path = "calculus_tests.rs"]
mod tests;

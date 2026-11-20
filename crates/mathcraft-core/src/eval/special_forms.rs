//! Function-argument builtins: builtins whose arguments are a function or an expression of a bound
//! variable, not values. Examples: `root(f(x), x, [a, b])`, `genfit(vx, vy, vg, F)`,
//! `linfit(vx, vy, F)`, `rkfixed(init, x1, x2, n, D)`.
//!
//! They are registered like any builtin (name and arity, so the call is arity-checked and the name
//! is known), but the call is dispatched here, in `expr_eval::call_builtin`, before any argument is
//! evaluated. Each form then decides how to evaluate its arguments:
//! - a function argument is either a bare name of a user function ([`named_function`]), called with
//!   values through `expr_eval::call_user_with`, or an expression whose variable is bound for each
//!   evaluation ([`eval_with`]), as the calculus operators bind theirs (see `calculus`);
//! - every evaluation of a function argument is charged to the statement's budget
//!   ([`SAMPLE_UNITS`]), on top of the work of its body, so a solver's iterations are bounded.
//!
//! A form reads its arguments in the context it was called from, so a function argument that
//! itself calls a form (a root inside `f`, called by a root) nests through the same path, bounded
//! by the node depth limit.

use super::builtins::{Arity, Registry};
use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{eval, resolve_user, sub, system_real, var_key, Ctx};
use super::range_iter::range_vector;
use super::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use crate::expr::Expr;
use crate::units::Dimension;

/// A form: its unevaluated argument expressions and the context of the call.
pub(super) type SpecialForm = fn(&[Expr], Ctx<'_>) -> Result<Value, EvalError>;

/// Work units charged per evaluation of a function argument, on top of the work of its body. The
/// same figure as one integrand sample of `calculus`.
pub(super) const SAMPLE_UNITS: u64 = 10;

/// Expression levels a form call counts against `expr_eval::MAX_NODE_DEPTH`. A form's native
/// frames (the root finder, the solver's buffers, the closure that evaluates the argument) are
/// several times the size of an ordinary node's, so each nested form costs this many levels. With
/// it, the node limit allows about nine nested forms (a root inside a root's function, ...) on a
/// 2 MiB thread, where a count of one per form would overflow the stack first.
pub(super) const FORM_NODE_COST: usize = 40;

/// Largest parameter count of a fitted model or an ODE system the forms accept.
pub(super) const MAX_UNKNOWNS: usize = 99;

/// Every form, by name, with its arity. The form functions live in their families' modules.
const FORMS: &[(&str, Arity, SpecialForm)] = &[
    ("root", Arity::Range(2, 3), super::root::root_form),
    (
        "genfit",
        Arity::Exact(4),
        super::builtins::fit_nonlin::genfit_form,
    ),
    (
        "linfit",
        Arity::Exact(3),
        super::builtins::fit_linear::linfit_form,
    ),
    ("rkfixed", Arity::Exact(5), super::ode::rkfixed_form),
    ("Rkadapt", Arity::Exact(5), super::ode::rkadapt_form),
    ("Stiffr", Arity::Exact(6), super::ode_stiff::stiffr_form),
    ("Stiffb", Arity::Exact(6), super::ode_stiff::stiffb_form),
    (
        "Bulstoer",
        Arity::Exact(5),
        super::ode_implicit::bulstoer_form,
    ),
    ("Adams", Arity::Range(5, 6), super::ode_implicit::adams_form),
    ("BDF", Arity::Range(5, 7), super::ode_implicit::bdf_form),
    ("Radau", Arity::Range(5, 8), super::ode_implicit::radau_form),
    (
        "AdamsBDF",
        Arity::Range(5, 7),
        super::ode_implicit::adamsbdf_form,
    ),
    ("sbval", Arity::Exact(6), super::ode_bvp::sbval_form),
    (
        "statespace",
        Arity::Range(5, 7),
        super::ode_state::statespace_form,
    ),
    ("numol", Arity::Exact(9), super::pde_mol::numol_form),
    ("bvalfit", Arity::Exact(9), super::ode_bvp::bvalfit_form),
    (
        "maximize",
        Arity::AtLeast(2),
        super::optimize_form::maximize_form,
    ),
    (
        "minimize",
        Arity::AtLeast(2),
        super::optimize_form::minimize_form,
    ),
    (
        "IsFunction",
        Arity::Exact(1),
        super::func_forms::is_function_form,
    ),
    ("funmap", Arity::Exact(2), super::func_forms::funmap_form),
    ("funconv", Arity::Exact(2), super::func_forms::funconv_form),
    (
        "funcdeconv",
        Arity::Exact(3),
        super::func_forms::funcdeconv_form,
    ),
];

/// Registers the names of every form. The registry entry carries the arity; the body is the form
/// itself, reached through [`lookup`] before the registry entry is ever called.
pub(super) fn register(r: &mut Registry) {
    for (name, arity, _) in FORMS {
        r.register(name, *arity, not_a_plain_call);
    }
}

fn not_a_plain_call(_: &[Value]) -> Result<Value, EvalError> {
    Err(EvalError::Unsupported(
        "a function-argument builtin reached a plain call".to_string(),
    ))
}

/// The form of a builtin name, when the name is one.
pub(super) fn lookup_form(name: &str) -> Option<SpecialForm> {
    FORMS
        .iter()
        .find(|(known, _, _)| *known == name)
        .map(|(_, _, form)| *form)
}

/// A user function named by a bare name argument (`F` in `genfit(vx, vy, vg, F)`), resolved as a
/// call would resolve it (inside a body, through the captured closure). `None` for any other
/// expression, which the caller then reads as a value or an expression.
pub(super) fn named_function<'a>(
    e: &Expr,
    ctx: Ctx<'a>,
) -> Result<Option<(String, &'a UserFunction)>, EvalError> {
    let Expr::Id(id) = e else {
        return Ok(None);
    };
    let key = var_key(id)?;
    Ok(resolve_user(&key, ctx)?.map(|f| (key, f)))
}

/// The storage key of a bound variable argument (`x` in `root(f(x), x, ...)`).
pub(super) fn bound_name(e: &Expr) -> Result<String, EvalError> {
    match e {
        Expr::Id(id) => var_key(id),
        _ => Err(EvalError::Unsupported(
            "the variable argument must be a name".to_string(),
        )),
    }
}

/// Evaluates `body` one level deeper with `bindings` as extra locals (a bound variable of an
/// expression argument). The body keeps the caller's frame, so names it captured still resolve.
pub(super) fn eval_with(
    body: &Expr,
    ctx: Ctx<'_>,
    bindings: &[(String, Value)],
) -> Result<Value, EvalError> {
    let mut locals = ctx.locals.to_vec();
    locals.extend_from_slice(bindings);
    eval(
        body,
        Ctx {
            locals: &locals,
            nodes: ctx.nodes + 1,
            ..ctx
        },
    )
}

/// Evaluates an argument expression in the caller's context.
pub(super) fn value_of(e: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    sub(e, ctx)
}

/// A data argument as a vector. A bare range variable (`vx := 0..4`) at the top level is its whole
/// range as a column, as `vec(vx)` gives it (see `range_iter::range_vector`); anything else is
/// evaluated as [`value_of`] does.
pub(super) fn vector_of(e: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    if ctx.frame.is_none() {
        if let Some(whole) = range_vector("vec", std::slice::from_ref(e), ctx.env, ctx.budget) {
            return whole;
        }
    }
    value_of(e, ctx)
}

/// `value` as a dimensionless real number.
pub(super) fn real_of(value: &Value, what: &str) -> Result<f64, EvalError> {
    match value {
        Value::Scalar(s) => real_scalar_value(s, what),
        _ => Err(EvalError::Unsupported(format!("{what} must be a number"))),
    }
}

/// The real, dimensionless numbers of a value: a scalar, or the elements of a vector (row or
/// column) in order. Units are refused: the solvers work on plain numbers.
pub(super) fn reals_of(value: &Value, what: &str) -> Result<Vec<f64>, EvalError> {
    match value {
        Value::Scalar(s) => Ok(vec![real_scalar_value(s, what)?]),
        Value::Matrix(m) => m
            .data()
            .iter()
            .map(|s| real_scalar_value(s, what))
            .collect(),
        _ => Err(EvalError::Unsupported(format!(
            "{what} must be a number or a vector"
        ))),
    }
}

fn real_scalar_value(s: &Scalar, what: &str) -> Result<f64, EvalError> {
    if !s.dim.is_dimensionless() {
        return Err(EvalError::DimensionNotDimensionless { dim: s.dim });
    }
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(format!("{what} must be real")));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// A dimensionless real scalar value.
pub(super) fn real_value(x: f64) -> Value {
    Value::Scalar(Scalar::real(x, Dimension::dimensionless()))
}

/// A column vector of dimensionless reals.
pub(super) fn column(values: &[f64]) -> Result<Value, EvalError> {
    if values.is_empty() {
        return Err(EvalError::Unsupported("an empty vector".to_string()));
    }
    let data = values
        .iter()
        .map(|v| Scalar::real(*v, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(values.len(), 1, data)?))
}

/// A matrix of `rows x cols` dimensionless reals, row-major. Refused above the size limit before
/// any allocation.
pub(super) fn matrix(rows: usize, cols: usize, values: &[f64]) -> Result<Value, EvalError> {
    let count = rows.checked_mul(cols).ok_or(EvalError::Overflow)?;
    if count > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(count));
    }
    let data = values
        .iter()
        .map(|v| Scalar::real(*v, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// The system variable `TOL` read as a real number, in the caller's scope (see `expr_eval`).
pub(super) fn tolerance(ctx: Ctx<'_>) -> Result<f64, EvalError> {
    system_real(ctx, "TOL")
}

//! `maximize(f, x, ...)` and `minimize(f, x, ...)` outside a solve block (help "Optimization
//! Functions"): the unconstrained optimum of the function `f` from the guesses `x, ...`. Outside a
//! block the help says constraints are ignored, so this form takes none. Inside a block the same
//! functions are solver members (`solve_block::solver_call`), with the block's constraints.
//!
//! `f` is the name of a user function of the guesses' count of arguments, called with values, as
//! `genfit`'s model is. Each evaluation is charged [`SAMPLE_UNITS`] on top of the body's work, and
//! each BFGS iteration is charged the dense elimination it implies (see `solve_block_rows`).

use std::cell::RefCell;
use std::rc::Rc;

use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{call_user_with, Ctx};
use super::solve_block::{MAX_OPT_ITERATIONS, MAX_SOLVE_EVALUATIONS};
use super::special_forms::{named_function, value_of, SAMPLE_UNITS};
use super::value::{Matrix, Scalar, Value};
use super::work::{dense_units, Dense};
use crate::expr::Expr;
use crate::solve::{optimize, IterationInfo, Limits, Problem, SolveError};
use crate::units::Dimension;

/// `maximize(f, x, ...)`: the local maximum of `f` nearest the guesses.
pub(super) fn maximize_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    optimize_form("maximize", args, ctx, -1.0)
}

/// `minimize(f, x, ...)`: the local minimum of `f` nearest the guesses.
pub(super) fn minimize_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    optimize_form("minimize", args, ctx, 1.0)
}

fn optimize_form(form: &str, args: &[Expr], ctx: Ctx<'_>, sign: f64) -> Result<Value, EvalError> {
    let Some((objective, guesses)) = args.split_first() else {
        return Err(EvalError::Unsupported(format!(
            "{form} takes the objective and its guesses"
        )));
    };
    let Some((name, func)) = named_function(objective, ctx)? else {
        return Err(EvalError::Unsupported(format!(
            "{form}: the objective must be the name of a user function; a function passed in as a \
             parameter of another function (or any other expression) is not accepted"
        )));
    };
    let mut guess = Vec::with_capacity(guesses.len());
    let mut dims = Vec::with_capacity(guesses.len());
    for e in guesses {
        match value_of(e, ctx)? {
            Value::Scalar(s) if s.value.im == 0.0 && s.value.re.is_finite() => {
                guess.push(s.value.re);
                dims.push(s.dim);
            }
            _ => {
                return Err(EvalError::Unsupported(format!(
                    "{form} takes real scalar guess values"
                )))
            }
        }
    }
    let failure: Rc<RefCell<Option<EvalError>>> = Rc::new(RefCell::new(None));
    let n = guess.len();
    let objective_at = |x: &[f64]| -> Result<f64, SolveError> {
        match objective_value(&name, func, &dims, sign, x, ctx) {
            Ok(v) => Ok(v),
            // A trial point outside the objective's domain is a failed step: the search backs off.
            Err(EvalError::NotFinite) => Err(SolveError::NotFinite),
            Err(e) => {
                failure.borrow_mut().get_or_insert(e);
                Err(SolveError::Unsupported(
                    "objective evaluation failed".to_string(),
                ))
            }
        }
    };
    let hook = |info: &IterationInfo| -> Result<(), SolveError> {
        ctx.budget
            .charge(dense_units(Dense::Elimination, info.m + info.k, info.n, 0))
            .map_err(|e| {
                failure.borrow_mut().get_or_insert(e);
                SolveError::BudgetExceeded
            })
    };
    let problem = Problem::new(guess)
        .objective(objective_at)
        .on_iteration(hook)
        .limits(Limits {
            max_iterations: MAX_OPT_ITERATIONS,
            max_evaluations: Some(MAX_SOLVE_EVALUATIONS),
        });
    let solved = optimize(&problem);
    if let Some(e) = failure.borrow_mut().take() {
        return Err(e);
    }
    let solution = solved.map_err(|e| EvalError::Solve(e.to_string()))?;
    debug_assert_eq!(solution.x.len(), n);
    if n == 1 {
        return Ok(Value::Scalar(Scalar::real(solution.x[0], dims[0])));
    }
    let data = solution
        .x
        .iter()
        .zip(&dims)
        .map(|(x, d)| Scalar::real(*x, *d))
        .collect();
    Ok(Value::Matrix(Matrix::new(n, 1, data)?))
}

/// One objective evaluation of `f` at the unknowns `x`, as a real finite value signed by `sign`.
fn objective_value(
    name: &str,
    func: &UserFunction,
    dims: &[Dimension],
    sign: f64,
    x: &[f64],
    ctx: Ctx<'_>,
) -> Result<f64, EvalError> {
    ctx.budget.charge(SAMPLE_UNITS)?;
    let values: Vec<Value> = x
        .iter()
        .zip(dims)
        .map(|(v, d)| Value::Scalar(Scalar::real(*v, *d)))
        .collect();
    match call_user_with(name, func, values, &[], ctx)? {
        // A complex value is a point outside the objective's domain, reported as not finite.
        Value::Scalar(s) if s.value.im != 0.0 => Err(EvalError::NotFinite),
        Value::Scalar(s) if s.value.re.is_finite() => Ok(sign * s.value.re),
        Value::Scalar(_) => Err(EvalError::NotFinite),
        _ => Err(EvalError::Unsupported(
            "the objective of maximize or minimize must be a scalar".to_string(),
        )),
    }
}

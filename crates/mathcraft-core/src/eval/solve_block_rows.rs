//! The constraint closures of a solve block and the solver's per-iteration charge. Each evaluation writes
//! the unknowns into the block's scratch worksheet and evaluates the constraints, all charged to the
//! block's budget (see `work` and `solve_constraints::evaluation_units`).

use std::cell::RefCell;
use std::rc::Rc;

use super::env::Env;
use super::error::EvalError;
use super::expr_eval::evaluate_budget;
use super::solve_constraints::{evaluation_units, Constraint};
use super::value::{Scalar, Value};
use super::work::{dense_units, Budget, Dense};
use crate::expr::Expr;
use crate::solve::{IterationInfo, SolveError};
use crate::units::Dimension;

/// Builds the constraint closure for one group (equalities or inequalities). It writes the
/// unknowns into the shared scratch worksheet, evaluates each constraint, and keeps the first
/// evaluation error in `failure`, which the solve then reports in place of the solver's message.
pub(super) fn row_fn(
    work: &Rc<RefCell<Env>>,
    names: &[String],
    dims: &[Dimension],
    group: Vec<Constraint>,
    budget: &Rc<Budget>,
    failure: &Rc<RefCell<Option<EvalError>>>,
) -> (
    usize,
    impl Fn(&[f64]) -> Result<Vec<f64>, crate::solve::SolveError>,
) {
    let work = Rc::clone(work);
    let budget = Rc::clone(budget);
    let failure = Rc::clone(failure);
    let names = names.to_vec();
    let dims = dims.to_vec();
    let rows = group.len();
    let f = move |x: &[f64]| match evaluate_group(&work, &names, &dims, &group, &budget, x) {
        Ok(out) => Ok(out),
        // A trial point outside the constraint's domain is a failed step, not an error: the
        // solver backs off. The first failure of any other kind is kept in `failure`.
        Err(EvalError::NotFinite) => Err(SolveError::NotFinite),
        Err(e) => {
            if failure.borrow().is_none() {
                *failure.borrow_mut() = Some(nested_as_unsupported(e));
            }
            Err(SolveError::Unsupported(
                "constraint evaluation failed".to_string(),
            ))
        }
    };
    (rows, f)
}

/// One evaluation of a constraint group at unknowns `x`: the unknowns are written into the scratch
/// worksheet (charged per unknown), then each constraint is evaluated (charged per constraint).
fn evaluate_group(
    work: &RefCell<Env>,
    names: &[String],
    dims: &[Dimension],
    group: &[Constraint],
    budget: &Budget,
    x: &[f64],
) -> Result<Vec<f64>, EvalError> {
    budget.charge(evaluation_units(names.len()))?;
    let mut env = work.borrow_mut();
    for ((name, dim), v) in names.iter().zip(dims).zip(x) {
        env.set_variable(name.clone(), Value::Scalar(Scalar::real(*v, *dim)));
    }
    group.iter().map(|c| c.residual(&env, budget)).collect()
}

/// Builds the objective closure of `maximize` (`sign` -1) or `minimize` (`sign` 1). Each call writes
/// the unknowns into the scratch worksheet and evaluates `call`, the objective applied to the
/// unknowns, charged like a constraint. The value is negated for `maximize`, so the optimiser always
/// minimises. Errors are kept in `failure` as for [`row_fn`].
pub(super) fn objective_fn(
    work: &Rc<RefCell<Env>>,
    names: &[String],
    dims: &[Dimension],
    call: Expr,
    budget: &Rc<Budget>,
    failure: &Rc<RefCell<Option<EvalError>>>,
    sign: f64,
) -> impl Fn(&[f64]) -> Result<f64, crate::solve::SolveError> {
    let work = Rc::clone(work);
    let budget = Rc::clone(budget);
    let failure = Rc::clone(failure);
    let names = names.to_vec();
    let dims = dims.to_vec();
    move |x: &[f64]| match objective_value(&work, &names, &dims, &call, &budget, x, sign) {
        Ok(v) => Ok(v),
        // A trial point where the objective is complex or not finite (`ln` of a negative) is a
        // failed step: the line search backs off.
        Err(EvalError::NotFinite) => Err(SolveError::NotFinite),
        Err(e) => {
            if failure.borrow().is_none() {
                *failure.borrow_mut() = Some(nested_as_unsupported(e));
            }
            Err(SolveError::Unsupported(
                "objective evaluation failed".to_string(),
            ))
        }
    }
}

/// One objective evaluation at unknowns `x`: a real, finite scalar, signed by `sign`.
fn objective_value(
    work: &RefCell<Env>,
    names: &[String],
    dims: &[Dimension],
    call: &Expr,
    budget: &Budget,
    x: &[f64],
    sign: f64,
) -> Result<f64, EvalError> {
    budget.charge(evaluation_units(names.len()))?;
    let mut env = work.borrow_mut();
    for ((name, dim), v) in names.iter().zip(dims).zip(x) {
        env.set_variable(name.clone(), Value::Scalar(Scalar::real(*v, *dim)));
    }
    match evaluate_budget(call, &env, budget)? {
        // A complex value is a point outside the objective's domain, reported as not finite.
        Value::Scalar(s) if s.value.im != 0.0 => Err(EvalError::NotFinite),
        Value::Scalar(s) if s.value.re.is_finite() => Ok(sign * s.value.re),
        Value::Scalar(_) => Err(EvalError::NotFinite),
        _ => Err(EvalError::Unsupported(
            "the objective of maximize or minimize must be a scalar".to_string(),
        )),
    }
}

/// The solver's own cost, charged once per outer iteration: the dense elimination of the Jacobian
/// and the normal equations (`(m + k) x n`). The closure evaluations of the Jacobian are charged by
/// [`evaluate_group`]. An exhausted budget is recorded in `failure` (the solve reports it in place of
/// the solver's message) and aborts the solve.
pub(super) fn iteration_charge(
    budget: &Rc<Budget>,
    failure: &Rc<RefCell<Option<EvalError>>>,
) -> impl Fn(&IterationInfo) -> Result<(), SolveError> {
    let budget = Rc::clone(budget);
    let failure = Rc::clone(failure);
    move |info| {
        let units = dense_units(Dense::Elimination, info.m + info.k, info.n, 0);
        budget.charge(units).map_err(|e| {
            failure.borrow_mut().get_or_insert(e);
            SolveError::BudgetExceeded
        })
    }
}

/// A solver call inside a constraint (`find` in `x^2 == find(x)`) is a nested solve block, which
/// is not supported. The builtin lookup reports it as a use outside a block, so it is renamed here.
fn nested_as_unsupported(e: EvalError) -> EvalError {
    match e {
        EvalError::OutsideSolveBlock(_) => {
            EvalError::Unsupported("nested solve blocks are not supported".to_string())
        }
        other => other,
    }
}

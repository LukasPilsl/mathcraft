//! Boundary value problems by shooting (help: deriving_initial_conditions_for_an_ode):
//! `sbval(v1, x1, x2, D, load1, scorei)` and `bvalfit(v1, v2, x1, x2, xf, D, load1, load2, scoreb)`.
//!
//! - `load1(x, v)` returns the full initial vector at `x1`: the unknown entries are the entries of
//!   `v` (the guesses are the unknowns), the known ones constants. `load2` is the same at `x2`.
//! - `scorei(x2, y)` returns the mismatch of the trajectory at `x2` from the specified condition
//!   (zero at the solution), one entry per unknown. `scoreb(xf, y)` returns the vector compared at
//!   the matching point `xf`: the residual is `scoreb(xf, Y_1) - scoreb(xf, Y_2)`, where `Y_1`
//!   shoots from `x1` and `Y_2` from `x2`.
//! - The unknowns are found by Newton's method with a finite-difference Jacobian and backtracking.
//!   A trial whose shot overflows or does not converge is halved like a worse one. Each shot is an
//!   adaptive Dormand-Prince integration at a fixed tolerance of [`SHOOT_TOL`] (`TOL` is not used).
//!   A Newton iteration that cannot find a smaller step, or does not converge, is an error.
//! - The result is the full initial vector at `x1` (the solved one), ready for an ODE solver.
//!   Deviation: the help says it returns "those initial values left unspecified at x1"; the full
//!   vector is returned because the solvers need all of it.

use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{call_user_uncounted, Ctx};
use super::ode::{self, Derivative, Problem, Stepper, DERIVATIVE_UNITS};
use super::ode_stiff::solve_dense;
use super::special_forms::{
    column, named_function, real_of, real_value, reals_of, value_of, MAX_UNKNOWNS,
};
use super::value::Value;
use crate::expr::Expr;

/// Tolerance of each shot (the local error target of the Dormand-Prince integration).
const SHOOT_TOL: f64 = 1e-10;

/// Accepted steps one shot may take.
const SHOOT_STEPS: usize = 100_000;

/// Newton iterations and backtracking halvings allowed.
const NEWTON_MAX: usize = 40;
const BACKTRACK_MAX: usize = 30;

/// `sbval(v1, x1, x2, D, load1, scorei)`.
pub(super) fn sbval_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [v1, x1, x2, d, load1, scorei] = args else {
        return Err(EvalError::Unsupported(
            "sbval takes six arguments: v1, x1, x2, D, load1, scorei".to_string(),
        ));
    };
    let guess = reals_of(&value_of(v1, ctx)?, "the guesses v1")?;
    check_unknowns(guess.len())?;
    let x1 = real_of(&value_of(x1, ctx)?, "x1")?;
    let x2 = real_of(&value_of(x2, ctx)?, "x2")?;
    let deriv = named(d, ctx, 2, "D")?;
    let load = named(load1, ctx, 2, "load1")?;
    let score = named(scorei, ctx, 2, "scorei")?;
    let stepper = deriv.shot(ctx);

    let shoot = |u: &[f64]| -> Result<Vec<f64>, EvalError> {
        let init = load.call(x1, u, ctx)?;
        let end = integrate(&stepper, x1, x2, &init)?;
        let residual = score.call(x2, &end, ctx)?;
        if residual.len() != guess.len() {
            return Err(EvalError::Unsupported(format!(
                "scorei returns {} values for {} unknowns",
                residual.len(),
                guess.len()
            )));
        }
        Ok(residual)
    };
    let u = newton(&guess, shoot, "sbval")?;
    let init = load.call(x1, &u, ctx)?;
    column(&init).map_err(|_| EvalError::Unsupported("load1 returns an empty vector".to_string()))
}

/// `bvalfit(v1, v2, x1, x2, xf, D, load1, load2, scoreb)`.
pub(super) fn bvalfit_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [v1, v2, x1, x2, xf, d, load1, load2, scoreb] = args else {
        return Err(EvalError::Unsupported(
            "bvalfit takes nine arguments: v1, v2, x1, x2, xf, D, load1, load2, scoreb".to_string(),
        ));
    };
    let g1 = reals_of(&value_of(v1, ctx)?, "the guesses v1")?;
    let g2 = reals_of(&value_of(v2, ctx)?, "the guesses v2")?;
    let total = g1.len() + g2.len();
    check_unknowns(total)?;
    let x1 = real_of(&value_of(x1, ctx)?, "x1")?;
    let x2 = real_of(&value_of(x2, ctx)?, "x2")?;
    let xf = real_of(&value_of(xf, ctx)?, "xf")?;
    let deriv = named(d, ctx, 2, "D")?;
    let load_a = named(load1, ctx, 2, "load1")?;
    let load_b = named(load2, ctx, 2, "load2")?;
    let score = named(scoreb, ctx, 2, "scoreb")?;
    let stepper = deriv.shot(ctx);
    let split = g1.len();

    let residual = |u: &[f64]| -> Result<Vec<f64>, EvalError> {
        let init_a = load_a.call(x1, &u[..split], ctx)?;
        let init_b = load_b.call(x2, &u[split..], ctx)?;
        let end_a = integrate(&stepper, x1, xf, &init_a)?;
        let end_b = integrate(&stepper, x2, xf, &init_b)?;
        let sa = score.call(xf, &end_a, ctx)?;
        let sb = score.call(xf, &end_b, ctx)?;
        if sa.len() != total || sb.len() != total {
            return Err(EvalError::Unsupported(format!(
                "scoreb returns {} values for {total} unknowns",
                sa.len()
            )));
        }
        Ok(sa.iter().zip(&sb).map(|(a, b)| a - b).collect())
    };
    let guess: Vec<f64> = g1.iter().chain(&g2).copied().collect();
    let u = newton(&guess, residual, "bvalfit")?;
    let init = load_a.call(x1, &u[..split], ctx)?;
    column(&init).map_err(|_| EvalError::Unsupported("load1 returns an empty vector".to_string()))
}

fn check_unknowns(count: usize) -> Result<(), EvalError> {
    if count == 0 || count > MAX_UNKNOWNS {
        return Err(EvalError::Unsupported(format!(
            "a boundary value problem takes between 1 and {MAX_UNKNOWNS} unknowns"
        )));
    }
    Ok(())
}

/// A named user function with the number of parameters the form requires.
struct Named<'c> {
    name: String,
    func: &'c UserFunction,
}

fn named<'c>(e: &Expr, ctx: Ctx<'c>, arity: usize, what: &str) -> Result<Named<'c>, EvalError> {
    match named_function(e, ctx)? {
        Some((name, func)) if func.params.len() == arity => Ok(Named { name, func }),
        Some((name, func)) => Err(EvalError::ArgCount {
            name,
            expected: arity.to_string(),
            found: func.params.len(),
        }),
        None => Err(EvalError::Unsupported(format!(
            "{what} must be the name of a function"
        ))),
    }
}

impl<'c> Named<'c> {
    /// Calls `f(x, v)` with the components of the vector `v` bound as `param_k`, charged as one
    /// derivative evaluation.
    fn call(&self, x: f64, v: &[f64], ctx: Ctx<'c>) -> Result<Vec<f64>, EvalError> {
        ctx.budget.charge(DERIVATIVE_UNITS)?;
        let extra: Vec<(String, Value)> = v
            .iter()
            .enumerate()
            .map(|(k, val)| (format!("{}_{k}", self.func.params[1]), real_value(*val)))
            .collect();
        let out = call_user_uncounted(
            &self.name,
            self.func,
            vec![real_value(x), column(v)?],
            &extra,
            ctx,
        )?;
        reals_of(&out, &self.name)
    }

    /// The ODE right-hand side `D` (this name) as a stepper for the shooting integrations.
    fn shot(&self, ctx: Ctx<'c>) -> Shot<'c> {
        Shot {
            problem: Problem {
                name: self.name.clone(),
                func: self.func,
                param: self.func.params[1].clone(),
                init: Vec::new(),
                x1: 0.0,
                x2: 0.0,
                intvls: 1,
            },
            ctx,
        }
    }
}

/// The right-hand side `D` as a [`Stepper`] for the shooting integrations.
struct Shot<'c> {
    problem: Problem<'c>,
    ctx: Ctx<'c>,
}

impl Stepper for Shot<'_> {
    fn eval(&self, x: f64, y: &[f64]) -> Result<Vec<f64>, EvalError> {
        Derivative {
            problem: &self.problem,
            ctx: self.ctx,
        }
        .eval(x, y)
    }
}

/// Integrates `D` from `x1` to `x2` and returns the state at `x2`.
fn integrate(stepper: &Shot<'_>, x1: f64, x2: f64, init: &[f64]) -> Result<Vec<f64>, EvalError> {
    let rows = ode::adaptive(stepper, x1, x2, 1, init, SHOOT_TOL, SHOOT_STEPS)?;
    let last = rows
        .last()
        .ok_or_else(|| EvalError::NoConvergence("shooting".to_string()))?;
    Ok(last[1..].to_vec())
}

/// Newton's method on `residual(u) = 0` from `start`, with a finite-difference Jacobian and
/// backtracking. Not converged is an error.
fn newton<F>(start: &[f64], mut residual: F, name: &str) -> Result<Vec<f64>, EvalError>
where
    F: FnMut(&[f64]) -> Result<Vec<f64>, EvalError>,
{
    let no_convergence = || EvalError::NoConvergence(name.to_string());
    let mut u = start.to_vec();
    let mut r = residual(&u)?;
    for _ in 0..NEWTON_MAX {
        let scale = 1.0 + max_abs(&u);
        if max_abs(&r) <= 1e-9 * scale {
            return Ok(u);
        }
        let m = u.len();
        let mut jac = vec![vec![0.0; m]; m];
        for j in 0..m {
            let mut up = u.clone();
            up[j] += 1e-7 * (1.0 + u[j].abs());
            let dh = up[j] - u[j];
            let rp = residual(&up)?;
            for (row, (a, b)) in jac.iter_mut().zip(rp.iter().zip(&r)) {
                row[j] = (a - b) / dh;
            }
        }
        let rhs: Vec<f64> = r.iter().map(|v| -v).collect();
        let step = solve_dense(jac, rhs).ok_or_else(no_convergence)?;
        let current = max_abs(&r);
        let mut t = 1.0;
        let mut accepted = false;
        for _ in 0..BACKTRACK_MAX {
            let trial: Vec<f64> = u.iter().zip(&step).map(|(a, d)| a + t * d).collect();
            // A trial whose shot overflows (or does not converge) is a too-long step: halve it.
            let rt = match residual(&trial) {
                Ok(rt) => rt,
                Err(EvalError::NotFinite | EvalError::NoConvergence(_)) => {
                    t *= 0.5;
                    continue;
                }
                Err(e) => return Err(e),
            };
            if max_abs(&rt) < current {
                u = trial;
                r = rt;
                accepted = true;
                break;
            }
            t *= 0.5;
        }
        if !accepted {
            return Err(no_convergence());
        }
    }
    if max_abs(&r) <= 1e-9 * (1.0 + max_abs(&u)) {
        return Ok(u);
    }
    Err(no_convergence())
}

fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |a, x| a.max(x.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newton_solves_a_small_nonlinear_system() {
        // x^2 + y = 3, x + y^2 = 5 has the solutions (1, 2) and about (-0.773, 2.403): the start
        // point picks the root.
        let r = |u: &[f64]| -> Result<Vec<f64>, EvalError> {
            Ok(vec![u[0] * u[0] + u[1] - 3.0, u[0] + u[1] * u[1] - 5.0])
        };
        let root = newton(&[1.2, 1.8], r, "test").expect("converges");
        assert!(
            (root[0] - 1.0).abs() < 1e-8 && (root[1] - 2.0).abs() < 1e-8,
            "{root:?}"
        );
    }
}

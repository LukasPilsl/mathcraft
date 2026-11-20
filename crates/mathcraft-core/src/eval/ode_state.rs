//! The state-space solver (help: "State Space ODE Solver"): `statespace(init, t1, t2, intvls, A, [B, u])`
//! solves `x'(t) = A(t) x(t) + B(t) u(t)` for the state vector `x`, with `x(t1) = init`.
//!
//! `A` is the name of a user function of `t` that returns the `n x n` state matrix; `B` (optional)
//! a function returning an `n x k` matrix or vector, and `u` (required with `B`) a function
//! returning the `k` inputs as a vector or a scalar. Entries are plain numbers (the solver refuses
//! units).
//!
//! The result is `(intvls+1) x (n+1)`: column 0 holds the `intvls + 1` evenly spaced points from
//! `t1` to `t2`, columns 1..n the states at those points. The integration is the adaptive
//! Dormand-Prince solver of `ode` at the fixed tolerance [`INTEGRATION_TOL`] (the help names no
//! tolerance for statespace), so each output point is exact to that tolerance and no interpolation
//! is needed. Every evaluation of `A`, `B` and `u` is charged to the budget, and the solve takes at
//! most [`MAX_STEPS`] accepted steps (a stiff system is refused rather than run to the end).

use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{call_user_with, Ctx};
use super::ode::{self, Stepper, DERIVATIVE_UNITS};
use super::special_forms::{
    matrix, named_function, real_of, real_value, reals_of, value_of, MAX_UNKNOWNS,
};
use super::value::{Value, MAX_ELEMENTS};
use crate::expr::Expr;

/// Tolerance of the internal integrator (relative, as `ode::error_norm` scales it).
const INTEGRATION_TOL: f64 = 1e-9;
/// Accepted steps one statespace solve may take.
const MAX_STEPS: usize = 200_000;

/// A user function of `t` that the solver evaluates at each derivative evaluation.
struct Named<'a> {
    name: String,
    func: &'a UserFunction,
}

/// The right-hand side `A(t) x + B(t) u(t)`.
struct StateSpace<'a> {
    a: Named<'a>,
    /// The input matrix and the input function, both or neither.
    input: Option<(Named<'a>, Named<'a>)>,
    n: usize,
    ctx: Ctx<'a>,
}

impl StateSpace<'_> {
    /// The values of `func` at `t`, as a row-major list of numbers with its shape.
    fn sample(&self, f: &Named<'_>, t: f64) -> Result<(usize, usize, Vec<f64>), EvalError> {
        self.ctx.budget.charge(DERIVATIVE_UNITS)?;
        let out = call_user_with(&f.name, f.func, vec![real_value(t)], &[], self.ctx)?;
        let shape = match &out {
            Value::Matrix(m) => (m.rows(), m.cols()),
            Value::Scalar(_) => (1, 1),
            _ => {
                return Err(EvalError::Unsupported(format!(
                    "{} must return a number or a matrix",
                    f.name
                )))
            }
        };
        Ok((shape.0, shape.1, reals_of(&out, &f.name)?))
    }
}

impl Stepper for StateSpace<'_> {
    fn eval(&self, t: f64, x: &[f64]) -> Result<Vec<f64>, EvalError> {
        let n = self.n;
        let (ar, ac, a) = self.sample(&self.a, t)?;
        if (ar, ac) != (n, n) {
            return Err(EvalError::Unsupported(format!(
                "A(t) must be {n} x {n} for {n} states, found {ar} x {ac}"
            )));
        }
        let mut dx: Vec<f64> = (0..n)
            .map(|i| (0..n).map(|j| a[i * n + j] * x[j]).sum())
            .collect();
        if let Some((bf, uf)) = &self.input {
            let (br, bc, b) = self.sample(bf, t)?;
            if br != n {
                return Err(EvalError::Unsupported(format!(
                    "B(t) must have {n} rows, one for each state"
                )));
            }
            let (ur, uc, u) = self.sample(uf, t)?;
            if ur * uc != bc {
                return Err(EvalError::Unsupported(
                    "u(t) must have one entry for each column of B(t)".into(),
                ));
            }
            for (i, d) in dx.iter_mut().enumerate() {
                *d += (0..bc).map(|j| b[i * bc + j] * u[j]).sum::<f64>();
            }
        }
        if dx.iter().any(|v| !v.is_finite()) {
            return Err(EvalError::NotFinite);
        }
        Ok(dx)
    }
}

/// `statespace(init, t1, t2, intvls, A, [B, u])`.
pub(super) fn statespace_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let (init, t1, t2, intvls, a, input_args) = match args {
        [init, t1, t2, n, a] => (init, t1, t2, n, a, None),
        [init, t1, t2, n, a, b, u] => (init, t1, t2, n, a, Some((b, u))),
        _ => {
            return Err(EvalError::Unsupported(
                "statespace takes four or six arguments: init, t1, t2, intvls, A, and optionally B and u"
                    .to_string(),
            ))
        }
    };
    let init = reals_of(&value_of(init, ctx)?, "the initial conditions")?;
    if init.is_empty() || init.len() > MAX_UNKNOWNS {
        return Err(EvalError::Unsupported(format!(
            "statespace takes between 1 and {MAX_UNKNOWNS} states"
        )));
    }
    let t1 = real_of(&value_of(t1, ctx)?, "the start of the interval")?;
    let t2 = real_of(&value_of(t2, ctx)?, "the end of the interval")?;
    let count = real_of(&value_of(intvls, ctx)?, "the interval count")?;
    let n = init.len();
    if count.fract() != 0.0 || !(1.0..=MAX_ELEMENTS as f64).contains(&count) {
        return Err(EvalError::Unsupported(
            "the interval count must be a positive integer".to_string(),
        ));
    }
    let intvls = count as usize;
    if (intvls + 1).saturating_mul(n + 1) > MAX_ELEMENTS {
        return Err(EvalError::TooLarge((intvls + 1) * (n + 1)));
    }
    let a = function_arg(a, "A", ctx)?;
    let input = match input_args {
        None => None,
        Some((b, u)) => Some((function_arg(b, "B", ctx)?, function_arg(u, "u", ctx)?)),
    };
    let system = StateSpace { a, input, n, ctx };
    let rows =
        ode::adaptive(&system, t1, t2, intvls, &init, INTEGRATION_TOL, MAX_STEPS).map_err(|e| {
            match e {
                EvalError::NoConvergence(_) => EvalError::NoConvergence("statespace".to_string()),
                other => other,
            }
        })?;
    let data: Vec<f64> = rows.iter().flatten().copied().collect();
    matrix(rows.len(), n + 1, &data)
}

/// A function argument of the solver: the name of a user function of one variable.
fn function_arg<'a>(e: &Expr, what: &str, ctx: Ctx<'a>) -> Result<Named<'a>, EvalError> {
    match named_function(e, ctx)? {
        Some((name, func)) if func.params.len() == 1 => Ok(Named { name, func }),
        Some((name, func)) => Err(EvalError::ArgCount {
            name,
            expected: "1".to_string(),
            found: func.params.len(),
        }),
        None => Err(EvalError::Unsupported(format!(
            "statespace: {what} must be the name of a function of t, such as {what}(t)"
        ))),
    }
}

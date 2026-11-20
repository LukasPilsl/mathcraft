//! Single-line initial value solvers (help: ode_solvers): `rkfixed(init, x1, x2, intvls, D)` and
//! `Rkadapt(init, x1, x2, intvls, D)`. Each returns an `(intvls+1) x (n+1)` matrix: column 0 holds
//! the `intvls+1` evenly spaced points from `x1` to `x2`, and columns 1..n hold the solution
//! `y_0 .. y_{n-1}` at those points. `init` is a vector (or a scalar for one equation) of the
//! values at `x1`. `D` is a user function `D(x, y)` returning the `n` derivatives; inside its body
//! the components of `y` are named `y_0, y_1, ...` (bound as locals, see `special_forms`).
//!
//! - `rkfixed`: classical fourth-order Runge-Kutta with [`SUBSTEPS`] fixed steps per output
//!   interval, that is one RK4 step per interval as Mathcad takes (the help's damped oscillator with
//!   40 intervals on [0, 6] gives a squared error sum of 0.54, which one step per interval
//!   reproduces). `TOL` is not used.
//! - `Rkadapt`: Dormand-Prince 5(4) with error control. Each step's error, scaled by `TOL` (the
//!   worksheet's tolerance, see the help), must be at most 1 in the RMS norm, or the step is
//!   retried smaller. Steps land exactly on the output points, so no interpolation is needed.
//!
//! Every derivative evaluation is charged to the work budget ([`DERIVATIVE_UNITS`]); the evaluations
//! are not counted as user calls, so the call budget does not cap a solve. Nesting depth is still
//! checked. A solve that needs more than [`MAX_STEPS`] accepted steps is refused as not converged.

use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{call_user_uncounted, Ctx};
use super::special_forms::{
    column, matrix, named_function, real_of, real_value, reals_of, tolerance, value_of,
    MAX_UNKNOWNS,
};
use super::value::{Value, MAX_ELEMENTS};
use crate::expr::Expr;

/// Fixed RK4 substeps per output interval for `rkfixed`. Mathcad takes one step per interval (the
/// help's damped oscillator example reproduces its error sum of 0.54 only with one).
const SUBSTEPS: usize = 1;

/// Work units charged per derivative evaluation (and per Jacobian evaluation, see `ode_implicit`).
/// The budget then caps a solve at about 7.5e5 evaluations. Measured in a release build, the worst
/// refusals take under 2 s (Radau and Bulstoer on Van der Pol with mu = 1000 over [0, 3000] with 1e6
/// and 10 intervals; a 1e6-interval rkfixed or Radau on a decay takes under 1 s). The charge was 100
/// before, which let those refusals take 4 to 10 s. A call is not charged here, so the call count
/// does not cap the solve (see `expr_eval::call_user_uncounted`).
pub(super) const DERIVATIVE_UNITS: u64 = 400;

/// Accepted steps one `Rkadapt` solve may take, over all output intervals.
const MAX_STEPS: usize = 100_000;

/// Largest factor a step size grows or shrinks by in one adaptive step.
const MAX_GROWTH: f64 = 5.0;
const MIN_SHRINK: f64 = 0.2;
const SAFETY: f64 = 0.9;

/// Dormand-Prince 5(4) coefficients (nodes `c`, stage weights `a`, 5th-order weights `b`, and the
/// error weights `e` = b - b*, with b* the embedded 4th-order weights).
const C: [f64; 7] = [0.0, 0.2, 0.3, 0.8, 8.0 / 9.0, 1.0, 1.0];
const A21: f64 = 0.2;
const A31: f64 = 3.0 / 40.0;
const A32: f64 = 9.0 / 40.0;
const A41: f64 = 44.0 / 45.0;
const A42: f64 = -56.0 / 15.0;
const A43: f64 = 32.0 / 9.0;
const A51: f64 = 19372.0 / 6561.0;
const A52: f64 = -25360.0 / 2187.0;
const A53: f64 = 64448.0 / 6561.0;
const A54: f64 = -212.0 / 729.0;
const A61: f64 = 9017.0 / 3168.0;
const A62: f64 = -355.0 / 33.0;
const A63: f64 = 46732.0 / 5247.0;
const A64: f64 = 49.0 / 176.0;
const A65: f64 = -5103.0 / 18656.0;
const B1: f64 = 35.0 / 384.0;
const B3: f64 = 500.0 / 1113.0;
const B4: f64 = 125.0 / 192.0;
const B5: f64 = -2187.0 / 6784.0;
const B6: f64 = 11.0 / 84.0;
const E1: f64 = 71.0 / 57600.0;
const E3: f64 = -71.0 / 16695.0;
const E4: f64 = 71.0 / 1920.0;
const E5: f64 = -17253.0 / 339200.0;
const E6: f64 = 22.0 / 525.0;
const E7: f64 = -1.0 / 40.0;

/// The arguments of a solver: the initial values, the interval, the output count and the
/// derivative function (with its name, for calls).
pub(super) struct Problem<'a> {
    pub(super) name: String,
    pub(super) func: &'a UserFunction,
    pub(super) param: String,
    pub(super) init: Vec<f64>,
    pub(super) x1: f64,
    pub(super) x2: f64,
    pub(super) intvls: usize,
}

/// `rkfixed(init, x1, x2, intvls, D)`.
pub(super) fn rkfixed_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [init, x1, x2, intvls, d] = args else {
        return Err(EvalError::Unsupported(
            "a solver takes five arguments".to_string(),
        ));
    };
    let problem = problem(init, x1, x2, intvls, d, ctx)?;
    let rows = solve_fixed(&problem, ctx)?;
    table(&problem, &rows)
}

/// `Rkadapt(init, x1, x2, intvls, D)`.
pub(super) fn rkadapt_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [init, x1, x2, intvls, d] = args else {
        return Err(EvalError::Unsupported(
            "a solver takes five arguments".to_string(),
        ));
    };
    let problem = problem(init, x1, x2, intvls, d, ctx)?;
    let tol = tolerance(ctx)?;
    if !(tol > 0.0 && tol.is_finite()) {
        return Err(EvalError::Unsupported(
            "TOL must be a positive number for Rkadapt".to_string(),
        ));
    }
    let rows = solve_adaptive(&problem, tol, ctx)?;
    table(&problem, &rows)
}

/// Reads the five arguments of a solver (the initial values, the interval, the output count and
/// the derivative function).
pub(super) fn problem<'a>(
    init: &Expr,
    x1: &Expr,
    x2: &Expr,
    intvls: &Expr,
    d: &Expr,
    ctx: Ctx<'a>,
) -> Result<Problem<'a>, EvalError> {
    let init = reals_of(&value_of(init, ctx)?, "the initial values")?;
    if init.is_empty() || init.len() > MAX_UNKNOWNS {
        return Err(EvalError::Unsupported(format!(
            "a solver takes between 1 and {MAX_UNKNOWNS} unknowns"
        )));
    }
    let x1 = real_of(&value_of(x1, ctx)?, "the start of the interval")?;
    let x2 = real_of(&value_of(x2, ctx)?, "the end of the interval")?;
    let count = real_of(&value_of(intvls, ctx)?, "the interval count")?;
    if count.fract() != 0.0 || !(1.0..=MAX_ELEMENTS as f64).contains(&count) {
        return Err(EvalError::Unsupported(
            "the interval count must be a positive integer".to_string(),
        ));
    }
    let Some((name, func)) = named_function(d, ctx)? else {
        return Err(EvalError::Unsupported(
            "the derivative must be the name of a function D(x, y)".to_string(),
        ));
    };
    if func.params.len() != 2 {
        return Err(EvalError::ArgCount {
            name,
            expected: "2".to_string(),
            found: func.params.len(),
        });
    }
    let points = count as usize + 1;
    if points.saturating_mul(init.len() + 1) > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(points * (init.len() + 1)));
    }
    Ok(Problem {
        param: func.params[1].clone(),
        name,
        func,
        init,
        x1,
        x2,
        intvls: count as usize,
    })
}

/// One derivative evaluation: `D(x, y)` with the components of `y` bound as `y_0, y_1, ...`.
pub(super) struct Derivative<'p, 'c> {
    pub(super) problem: &'p Problem<'c>,
    pub(super) ctx: Ctx<'c>,
}

impl Derivative<'_, '_> {
    pub(super) fn eval(&self, x: f64, y: &[f64]) -> Result<Vec<f64>, EvalError> {
        self.ctx.budget.charge(DERIVATIVE_UNITS)?;
        let p = self.problem;
        let state = column(y)?;
        let extra: Vec<(String, Value)> = y
            .iter()
            .enumerate()
            .map(|(k, v)| (format!("{}_{k}", p.param), real_value(*v)))
            .collect();
        let out = call_user_uncounted(
            &p.name,
            p.func,
            vec![real_value(x), state],
            &extra,
            self.ctx,
        )?;
        let d = reals_of(&out, "the derivative D")?;
        if d.len() != y.len() {
            return Err(EvalError::Unsupported(format!(
                "D returns {} values for {} unknowns",
                d.len(),
                y.len()
            )));
        }
        Ok(d)
    }
}

/// The right-hand side `f(x, y)` of an initial value problem, as the steppers see it. Implemented
/// by the user-function solvers (`D`) and by the solve-block ODE system (`ode_block`).
pub(super) trait Stepper {
    fn eval(&self, x: f64, y: &[f64]) -> Result<Vec<f64>, EvalError>;
}

impl Stepper for Derivative<'_, '_> {
    fn eval(&self, x: f64, y: &[f64]) -> Result<Vec<f64>, EvalError> {
        Derivative::eval(self, x, y)
    }
}

/// The output rows `[x, y_0, ..., y_{n-1}]` for `rkfixed`.
fn solve_fixed(p: &Problem<'_>, ctx: Ctx<'_>) -> Result<Vec<Vec<f64>>, EvalError> {
    let d = Derivative { problem: p, ctx };
    let h_out = (p.x2 - p.x1) / p.intvls as f64;
    let h = h_out / SUBSTEPS as f64;
    let mut x = p.x1;
    let mut y = p.init.clone();
    let mut rows = vec![row(p.x1, &y)];
    for k in 0..p.intvls {
        for _ in 0..SUBSTEPS {
            y = rk4_step(&d, x, &y, h)?;
            x += h;
        }
        // The output point is the exact grid point, not the accumulated substep sum.
        x = p.x1 + (k + 1) as f64 * h_out;
        rows.push(row(x, &y));
    }
    Ok(rows)
}

/// One classical RK4 step of size `h` from `(x, y)`.
pub(super) fn rk4_step(
    d: &Derivative<'_, '_>,
    x: f64,
    y: &[f64],
    h: f64,
) -> Result<Vec<f64>, EvalError> {
    let n = y.len();
    let k1 = d.eval(x, y)?;
    let y2: Vec<f64> = (0..n).map(|i| y[i] + 0.5 * h * k1[i]).collect();
    let k2 = d.eval(x + 0.5 * h, &y2)?;
    let y3: Vec<f64> = (0..n).map(|i| y[i] + 0.5 * h * k2[i]).collect();
    let k3 = d.eval(x + 0.5 * h, &y3)?;
    let y4: Vec<f64> = (0..n).map(|i| y[i] + h * k3[i]).collect();
    let k4 = d.eval(x + h, &y4)?;
    Ok((0..n)
        .map(|i| y[i] + h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
        .collect())
}

/// The output rows for `Rkadapt`: adaptive steps from `x1` that land on each output point.
fn solve_adaptive(p: &Problem<'_>, tol: f64, ctx: Ctx<'_>) -> Result<Vec<Vec<f64>>, EvalError> {
    let d = Derivative { problem: p, ctx };
    adaptive(&d, p.x1, p.x2, p.intvls, &p.init, tol, MAX_STEPS).map_err(|e| match e {
        EvalError::NoConvergence(_) => EvalError::NoConvergence("Rkadapt".to_string()),
        other => other,
    })
}

/// Adaptive Dormand-Prince integration of `d` from `x1` to `x2`: the output rows at `intvls + 1`
/// evenly spaced points, each step landing exactly on the points. `tol` scales the error (see
/// [`error_norm`]); `max_steps` accepted steps are allowed, else the solve is refused as not
/// converged (the caller names the solver in its message).
pub(super) fn adaptive(
    d: &dyn Stepper,
    x1: f64,
    x2: f64,
    intvls: usize,
    init: &[f64],
    tol: f64,
    max_steps: usize,
) -> Result<Vec<Vec<f64>>, EvalError> {
    let dir = if x2 < x1 { -1.0 } else { 1.0 };
    let h_out = (x2 - x1) / intvls as f64;
    let mut x = x1;
    let mut y = init.to_vec();
    let mut h = dir * (h_out.abs() / 10.0).max(f64::MIN_POSITIVE);
    let mut steps = 0usize;
    let mut rows = vec![row(x1, &y)];
    for k in 0..intvls {
        let target = x1 + (k + 1) as f64 * h_out;
        while (target - x) * dir > 0.0 {
            steps += 1;
            if steps > max_steps {
                return Err(EvalError::NoConvergence("adaptive".to_string()));
            }
            // Land exactly on the output point when the next step would pass it.
            let remaining = target - x;
            let try_h = if h.abs() >= remaining.abs() {
                remaining
            } else {
                h
            };
            let (y_new, err) = dopri_step(d, x, &y, try_h)?;
            let err_norm = error_norm(&y, &y_new, &err, tol);
            let factor = if err_norm == 0.0 {
                MAX_GROWTH
            } else {
                (SAFETY * err_norm.powf(-0.2)).clamp(MIN_SHRINK, MAX_GROWTH)
            };
            if err_norm <= 1.0 {
                let landed = try_h == remaining;
                x = if landed { target } else { x + try_h };
                y = y_new;
                // A step cut short to land on an output point does not shrink the next step.
                let proposed = try_h.abs() * factor;
                h = dir
                    * if landed {
                        h.abs().max(proposed)
                    } else {
                        proposed
                    };
            } else {
                h = try_h * factor.min(1.0);
                if h.abs() < f64::EPSILON * x.abs().max(1.0) {
                    return Err(EvalError::NoConvergence("adaptive".to_string()));
                }
            }
        }
        x = target;
        rows.push(row(x, &y));
    }
    Ok(rows)
}

/// One Dormand-Prince step of size `h`: the fifth-order result and the embedded error estimate.
fn dopri_step(
    d: &dyn Stepper,
    x: f64,
    y: &[f64],
    h: f64,
) -> Result<(Vec<f64>, Vec<f64>), EvalError> {
    let n = y.len();
    let stage = |weights: &[(f64, &Vec<f64>)]| -> Vec<f64> {
        (0..n)
            .map(|i| y[i] + h * weights.iter().map(|(w, k)| w * k[i]).sum::<f64>())
            .collect()
    };
    let k1 = d.eval(x, y)?;
    let k2 = d.eval(x + C[1] * h, &stage(&[(A21, &k1)]))?;
    let k3 = d.eval(x + C[2] * h, &stage(&[(A31, &k1), (A32, &k2)]))?;
    let k4 = d.eval(x + C[3] * h, &stage(&[(A41, &k1), (A42, &k2), (A43, &k3)]))?;
    let k5 = d.eval(
        x + C[4] * h,
        &stage(&[(A51, &k1), (A52, &k2), (A53, &k3), (A54, &k4)]),
    )?;
    let k6 = d.eval(
        x + h,
        &stage(&[(A61, &k1), (A62, &k2), (A63, &k3), (A64, &k4), (A65, &k5)]),
    )?;
    let y_new: Vec<f64> = (0..n)
        .map(|i| y[i] + h * (B1 * k1[i] + B3 * k3[i] + B4 * k4[i] + B5 * k5[i] + B6 * k6[i]))
        .collect();
    let k7 = d.eval(x + h, &y_new)?;
    let err: Vec<f64> = (0..n)
        .map(|i| h * (E1 * k1[i] + E3 * k3[i] + E4 * k4[i] + E5 * k5[i] + E6 * k6[i] + E7 * k7[i]))
        .collect();
    Ok((y_new, err))
}

/// RMS norm of the error, each component scaled by `tol` and by its size (mixed tolerance).
pub(super) fn error_norm(y: &[f64], y_new: &[f64], err: &[f64], tol: f64) -> f64 {
    let n = err.len() as f64;
    let sum: f64 = (0..err.len())
        .map(|i| {
            let scale = tol * (1.0 + y[i].abs().max(y_new[i].abs()));
            (err[i] / scale).powi(2)
        })
        .sum();
    let norm = (sum / n).sqrt();
    if norm.is_finite() {
        norm
    } else {
        f64::INFINITY
    }
}

/// `[x, y_0, ..., y_{n-1}]`.
pub(super) fn row(x: f64, y: &[f64]) -> Vec<f64> {
    let mut r = Vec::with_capacity(y.len() + 1);
    r.push(x);
    r.extend_from_slice(y);
    r
}

/// The solution matrix: one row per output point.
pub(super) fn table(p: &Problem<'_>, rows: &[Vec<f64>]) -> Result<Value, EvalError> {
    let cols = p.init.len() + 1;
    let data: Vec<f64> = rows.iter().flatten().copied().collect();
    matrix(rows.len(), cols, &data)
}

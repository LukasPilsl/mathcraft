//! Stiff single-line solver `Stiffr(init, x1, x2, intvls, D, AJ)` (help: ode_solvers, "Stiffr uses
//! Rosenbrock method"). `AJ(x, y)` returns the augmented Jacobian: an `n x (n+1)` matrix whose first
//! column is the partial derivatives of `D` with respect to `x` and whose other columns are the
//! Jacobian `dD/dy`. Output layout and `TOL` are as in `ode.rs`.
//!
//! The method is the second-order, L-stable Rosenbrock method ROS2 (Verwer et al., gamma =
//! 1 + 1/sqrt(2)). One step from `(x, y)` of size `h` solves two linear systems with
//! `I - gamma h J`, where `J` is the Jacobian at `(x, y)`:
//!
//! ```text
//! (I - gamma h J) k1 = h f(x, y) + gamma h^2 f_x(x, y)
//! (I - gamma h J) k2 = h f(x + h, y + k1) - 2 k1
//! y_next = y + 3/2 k1 + 1/2 k2
//! ```
//!
//! The local error is estimated by step doubling: one step of `h` is compared with two steps of
//! `h/2`, and the difference over 3 (= 2^2 - 1) is the error of the finer result. Each component is
//! scaled by `TOL * (0.01 + |y_i|)` (see [`stiff_norm`]): a floor of 0.01 instead of the 1 of
//! `Rkadapt`, so that a component near 1e-5 does not get an absolute tolerance of `TOL`, which made
//! Robertson diverge at the default TOL 0.001. A trial is accepted when that error is at most 1, and
//! the finer result is kept. A trial whose stage overflows is rejected with the smallest shrink.
//!
//! `TOL` is the tolerance of each step (the local error), not a bound on the global error. The local
//! errors accumulate over the steps, so the global error is not bounded by `TOL` and grows with the
//! number of steps. Measured: for `y' = -y` at `t = 1` with `TOL = 1e-6` the error is about 20 TOL; for
//! the harmonic oscillator on [0, 10] with 50 intervals the max error is 3.1e-4 at `TOL = 1e-6` and
//! 1.4e-5 at `TOL = 1e-8` (about `TOL^(2/3)`). See the tests `stiffr_global_error_accumulates_over_steps`
//! and `stiffr_oscillator_global_error_is_measured`.
//! Every `D` and `AJ` evaluation is charged to the work budget; a solve that needs more than
//! [`MAX_TRIALS`] trial steps is refused as not converged.

use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{call_user_uncounted, Ctx};
use super::ode::{self, Derivative, Problem, DERIVATIVE_UNITS};
use super::special_forms::{column, named_function, real_value, reals_of, tolerance};
use super::value::Value;
use crate::expr::Expr;

/// The ROS2 stage coefficient `gamma = 1 + 1/sqrt(2)` (L-stable).
const GAMMA: f64 = 1.707_106_781_186_547_5;

/// Trial steps (accepted or rejected) one `Stiffr` solve may take.
const MAX_TRIALS: usize = 100_000;

/// Absolute floor of the error scale (see [`stiff_norm`]).
const STIFF_FLOOR: f64 = 1e-2;

/// Largest factor a step size grows or shrinks by in one trial (order-2 error: exponent 1/3).
const MAX_GROWTH: f64 = 5.0;
const MIN_SHRINK: f64 = 0.2;
const SAFETY: f64 = 0.9;

/// `Stiffr(init, x1, x2, intvls, D, AJ)`.
pub(super) fn stiffr_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    stiff_form("Stiffr", args, ctx)
}

/// `Stiffb(init, x1, x2, intvls, D, AJ)`. The help's Stiffb and Stiffr example plots show identical
/// solutions, and the help names no other step rule for Stiffb, so it runs the same integrator.
pub(super) fn stiffb_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    stiff_form("Stiffb", args, ctx)
}

fn stiff_form(name: &str, args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [init, x1, x2, intvls, d, aj] = args else {
        return Err(EvalError::Unsupported(format!(
            "{name} takes six arguments: init, x1, x2, intvls, D, AJ"
        )));
    };
    let problem = ode::problem(init, x1, x2, intvls, d, ctx)?;
    let jac = augmented(aj, ctx)?;
    let tol = tolerance(ctx)?;
    if !(tol > 0.0 && tol.is_finite()) {
        return Err(EvalError::Unsupported(format!(
            "TOL must be a positive number for {name}"
        )));
    }
    let rows = solve(&problem, &jac, tol, name)?;
    ode::table(&problem, &rows)
}

/// The augmented Jacobian function `AJ(x, y)`, called with the components of `y` bound as
/// `y_0, y_1, ...` (the same convention as `D`).
struct Augmented<'c> {
    name: String,
    func: &'c UserFunction,
    param: String,
    ctx: Ctx<'c>,
}

/// Resolves `AJ`, checking that it takes `(x, y)`.
fn augmented<'c>(e: &Expr, ctx: Ctx<'c>) -> Result<Augmented<'c>, EvalError> {
    let Some((name, func)) = named_function(e, ctx)? else {
        return Err(EvalError::Unsupported(
            "the augmented Jacobian must be the name of a function AJ(x, y)".to_string(),
        ));
    };
    if func.params.len() != 2 {
        return Err(EvalError::ArgCount {
            name,
            expected: "2".to_string(),
            found: func.params.len(),
        });
    }
    Ok(Augmented {
        param: func.params[1].clone(),
        name,
        func,
        ctx,
    })
}

/// The first column `f_x` and the `n x n` block `J` of `AJ` at one point.
struct Jacobian {
    fx: Vec<f64>,
    j: Vec<Vec<f64>>,
}

impl Augmented<'_> {
    fn eval(&self, x: f64, y: &[f64]) -> Result<Jacobian, EvalError> {
        self.ctx.budget.charge(DERIVATIVE_UNITS)?;
        let n = y.len();
        let extra: Vec<(String, Value)> = y
            .iter()
            .enumerate()
            .map(|(k, v)| (format!("{}_{k}", self.param), real_value(*v)))
            .collect();
        let out = call_user_uncounted(
            &self.name,
            self.func,
            vec![real_value(x), column(y)?],
            &extra,
            self.ctx,
        )?;
        let shape_ok = matches!(&out, Value::Matrix(m) if m.shape() == (n, n + 1));
        if !shape_ok {
            return Err(EvalError::Unsupported(format!(
                "AJ must return a {n} x {} matrix",
                n + 1
            )));
        }
        let flat = reals_of(&out, "the augmented Jacobian AJ")?;
        let rows: Vec<&[f64]> = flat.chunks(n + 1).collect();
        Ok(Jacobian {
            fx: rows.iter().map(|r| r[0]).collect(),
            j: rows.iter().map(|r| r[1..].to_vec()).collect(),
        })
    }
}

/// One ROS2 step of size `h` from `(x, y)`. `None` when a stage overflows (a non-finite value or a
/// singular system): the trial is then rejected by the caller.
fn ros2_step(
    d: &Derivative<'_, '_>,
    aj: &Augmented<'_>,
    x: f64,
    y: &[f64],
    h: f64,
) -> Result<Option<Vec<f64>>, EvalError> {
    let n = y.len();
    let Some(jac) = not_finite_as_none(aj.eval(x, y))? else {
        return Ok(None);
    };
    let Some(f0) = not_finite_as_none(d.eval(x, y))? else {
        return Ok(None);
    };
    // The system matrix I - gamma h J, shared by both stages.
    let mut a = vec![vec![0.0; n]; n];
    for (i, row) in a.iter_mut().enumerate() {
        for (k, entry) in row.iter_mut().enumerate() {
            let delta = if i == k { 1.0 } else { 0.0 };
            *entry = delta - GAMMA * h * jac.j[i][k];
        }
    }
    let rhs1: Vec<f64> = (0..n)
        .map(|i| h * f0[i] + GAMMA * h * h * jac.fx[i])
        .collect();
    let Some(k1) = solve_dense(a.clone(), rhs1) else {
        return Ok(None);
    };
    let y1: Vec<f64> = (0..n).map(|i| y[i] + k1[i]).collect();
    let Some(f1) = not_finite_as_none(d.eval(x + h, &y1))? else {
        return Ok(None);
    };
    let rhs2: Vec<f64> = (0..n).map(|i| h * f1[i] - 2.0 * k1[i]).collect();
    let Some(k2) = solve_dense(a, rhs2) else {
        return Ok(None);
    };
    Ok(Some(
        (0..n).map(|i| y[i] + 1.5 * k1[i] + 0.5 * k2[i]).collect(),
    ))
}

/// A stage result, with an overflow (`NotFinite`) as `None` and every other error passed on.
fn not_finite_as_none<T>(r: Result<T, EvalError>) -> Result<Option<T>, EvalError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(EvalError::NotFinite) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The coarse result (one step of `h`) and the finer one (two steps of `h/2`).
type TrialPair = (Vec<f64>, Vec<f64>);

/// One trial of `h` against two of `h/2`: the coarse result and the finer one, or `None` when a
/// stage overflows.
fn trial_pair(
    d: &Derivative<'_, '_>,
    aj: &Augmented<'_>,
    x: f64,
    y: &[f64],
    h: f64,
) -> Result<Option<TrialPair>, EvalError> {
    let Some(big) = ros2_step(d, aj, x, y, h)? else {
        return Ok(None);
    };
    let half = h / 2.0;
    let Some(mid) = ros2_step(d, aj, x, y, half)? else {
        return Ok(None);
    };
    let Some(fine) = ros2_step(d, aj, x + half, &mid, half)? else {
        return Ok(None);
    };
    Ok(Some((big, fine)))
}

/// Solves `a x = b` by Gaussian elimination with partial pivoting. `None` for a singular system.
pub(super) fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|&p, &q| a[p][col].abs().total_cmp(&a[q][col].abs()))
            .unwrap_or(col);
        let p = a[pivot][col];
        if p == 0.0 || !p.is_finite() {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let factor = a[row][col] / a[col][col];
            if factor != 0.0 {
                let (upper, lower) = a.split_at_mut(row);
                for (dst, src) in lower[0].iter_mut().zip(&upper[col]).skip(col) {
                    *dst -= factor * src;
                }
                b[row] -= factor * b[col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let tail: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    Some(x)
}

/// RMS norm of the error with each component scaled by `tol * (STIFF_FLOOR + |y|)`. The floor is
/// 0.01 rather than the 1 of [`ode::error_norm`]: with 1, a component near 1e-5 (Robertson's y1) gets
/// an absolute tolerance of `tol`, the steps grow, and the solution diverges at the default TOL 0.001.
fn stiff_norm(y: &[f64], y_new: &[f64], err: &[f64], tol: f64) -> f64 {
    let n = err.len() as f64;
    let sum: f64 = (0..err.len())
        .map(|i| {
            let scale = tol * (STIFF_FLOOR + y[i].abs().max(y_new[i].abs()));
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

/// The output rows for `Stiffr`: adaptive trial steps from `x1` that land on each output point.
fn solve(
    p: &Problem<'_>,
    aj: &Augmented<'_>,
    tol: f64,
    name: &str,
) -> Result<Vec<Vec<f64>>, EvalError> {
    let d = Derivative {
        problem: p,
        ctx: aj.ctx,
    };
    let dir = if p.x2 < p.x1 { -1.0 } else { 1.0 };
    let h_out = (p.x2 - p.x1) / p.intvls as f64;
    let mut x = p.x1;
    let mut y = p.init.clone();
    let mut h = dir * (h_out.abs() / 10.0).max(f64::MIN_POSITIVE);
    let mut trials = 0usize;
    let mut rows = vec![ode::row(p.x1, &y)];
    for k in 0..p.intvls {
        let target = p.x1 + (k + 1) as f64 * h_out;
        while (target - x) * dir > 0.0 {
            trials += 1;
            if trials > MAX_TRIALS {
                return Err(EvalError::NoConvergence(name.to_string()));
            }
            // Land exactly on the output point when the next step would pass it.
            let remaining = target - x;
            let try_h = if h.abs() >= remaining.abs() {
                remaining
            } else {
                h
            };
            // One step of try_h against two of half the size; the finer result is the candidate.
            // An overflowing trial is rejected with the smallest shrink, as the implicit solvers
            // reject an overflowing step; only a step too small to make progress is an error.
            let Some((big, fine)) = trial_pair(&d, aj, x, &y, try_h)? else {
                h = try_h * MIN_SHRINK;
                if h.abs() < f64::EPSILON * x.abs().max(1.0) {
                    return Err(EvalError::NoConvergence(name.to_string()));
                }
                continue;
            };
            let err: Vec<f64> = (0..y.len()).map(|i| (fine[i] - big[i]) / 3.0).collect();
            let err_norm = stiff_norm(&y, &fine, &err, tol);
            let factor = if err_norm == 0.0 {
                MAX_GROWTH
            } else {
                (SAFETY * err_norm.powf(-1.0 / 3.0)).clamp(MIN_SHRINK, MAX_GROWTH)
            };
            if err_norm <= 1.0 {
                let landed = try_h == remaining;
                x = if landed { target } else { x + try_h };
                y = fine;
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
                    return Err(EvalError::NoConvergence(name.to_string()));
                }
            }
        }
        x = target;
        rows.push(ode::row(x, &y));
    }
    Ok(rows)
}

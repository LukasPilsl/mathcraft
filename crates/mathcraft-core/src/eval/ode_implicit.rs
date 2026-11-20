//! Further single-line ODE solvers (help: ode_solvers), with the signatures of `rkfixed` and
//! `Stiffr` (see `ode.rs`, `ode_stiff.rs`). Each returns the `(intvls+1) x (n+1)` solution matrix.
//!
//! - `Bulstoer(init, x1, x2, intvls, D)`: Bulirsch-Stoer. Each macro-step is a modified midpoint
//!   sequence (2, 4, ..., 16 substeps, Gragg smoothing) extrapolated to zero step size with Neville's
//!   recurrence in h^2. It is accepted when the extrapolation error, scaled by `TOL`, is at most 1,
//!   else halved. Like `rkfixed`, it uses `TOL`.
//! - `Radau(init, x1, x2, intvls, D, [J], [M], [tol])`: Radau IIA, three stages, order 5, stiffly
//!   accurate. Adaptive trial steps with step doubling, the stages solved by simplified Newton with
//!   the Jacobian at the start of the step. `tol` (scalar or one per unknown) is the step's error
//!   target and defaults to [`DEFAULT_TOL`]; the help says `TOL` does not apply.
//! - `Adams(init, x1, x2, intvls, D, [tol])`: Adams-Bashforth-Moulton PECE of order 4 with an RK4
//!   start, on uniform steps.
//! - `BDF(init, x1, x2, intvls, D, [J], [tol])`: backward Euler, then BDF2, on uniform steps, each
//!   step solved by Newton with the Jacobian at the start of the step.
//! - `AdamsBDF(init, x1, x2, intvls, D, [J], [tol])`: Adams, and BDF when Adams needs more than
//!   [`ADAMS_STEPS`] steps. Deviation: Mathcad's stiffness test is not replicated; the switch is
//!   made on the step count.
//!
//! The two multistep solvers (`Adams`, `BDF`, `AdamsBDF`) use uniform steps: the number of steps per
//! output interval doubles until two successive solutions agree to `tol` (scaled as in `Rkadapt`).
//! The returned solution is the finer one. `M` other than the identity is refused (not implemented).
//! `Stiffb` is not implemented.
//!
//! Limits: the multistep solvers take uniform steps, so a stiff problem can reach [`MAX_STEPS`] (or
//! [`ADAMS_STEPS`]) first and is refused: BDF refuses Van der Pol with mu = 1000 at tol 1e-4 (measured),
//! and tight tolerances on stiff problems are refused the same way. Robertson is stiff: use Radau or
//! Stiffr there. `Bulstoer` is for non-stiff problems: on y' = -1000 y it is about 16 times TOL off
//! (measured). Radau's tol is the per-step error target: on the help's oscillator at tol 1e-7 the max
//! error is about 10 tol, since the errors accumulate over the steps.
//!
//! Every derivative and Jacobian evaluation, and every linear solve, is charged to the work budget.
//! A solve that needs more than [`MAX_STEPS`] steps (or [`ADAMS_STEPS`] for Adams) is refused as
//! not converged.

use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{call_user_uncounted, Ctx};
use super::ode::{self, Derivative, Problem, DERIVATIVE_UNITS};
use super::ode_stiff::solve_dense;
use super::special_forms::{column, named_function, real_value, reals_of, tolerance, value_of};
use super::value::Value;
use crate::expr::Expr;

/// The tolerance of the solvers that take `tol` when none is given: 10^-5, as the help states for
/// Adams, AdamsBDF, BDF and Radau. (`Bulstoer`, `Rkadapt` and `Stiffr` take the worksheet `TOL`,
/// whose built-in default is 0.001 as the help's built-in variables table states.)
const DEFAULT_TOL: f64 = 1e-5;

/// Steps one solve may take in total (over all doublings, or trial steps for the adaptive solvers).
const MAX_STEPS: usize = 100_000;

/// Steps `AdamsBDF` lets Adams take before it switches to BDF.
const ADAMS_STEPS: usize = 20_000;

/// Newton iterations allowed per Radau stage solve and per BDF step.
const RADAU_NEWTON: usize = 10;
const BDF_NEWTON: usize = 20;

/// Relative change at which a Newton iteration counts as converged.
const NEWTON_TOL: f64 = 1e-10;

const SAFETY: f64 = 0.9;
const MAX_GROWTH: f64 = 5.0;
const MIN_SHRINK: f64 = 0.2;

/// Modified midpoint sub-step counts of the Bulirsch-Stoer sequence.
const BS_SEQUENCE: [usize; 8] = [2, 4, 6, 8, 10, 12, 14, 16];

/// The solver families of this file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Adams,
    Bdf,
    Radau,
    AdamsBdf,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Method::Adams => "Adams",
            Method::Bdf => "BDF",
            Method::Radau => "Radau",
            Method::AdamsBdf => "AdamsBDF",
        }
    }

    /// Whether the solver takes the optional Jacobian J.
    fn takes_j(self) -> bool {
        self != Method::Adams
    }
}

/// `Bulstoer(init, x1, x2, intvls, D)`.
pub(super) fn bulstoer_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [init, x1, x2, intvls, d] = args else {
        return Err(EvalError::Unsupported(
            "Bulstoer takes five arguments: init, x1, x2, intvls, D".to_string(),
        ));
    };
    let problem = ode::problem(init, x1, x2, intvls, d, ctx)?;
    let tol = tolerance(ctx)?;
    if !(tol > 0.0 && tol.is_finite()) {
        return Err(EvalError::Unsupported(
            "TOL must be a positive number for Bulstoer".to_string(),
        ));
    }
    let sys = Sys {
        d: Derivative {
            problem: &problem,
            ctx,
        },
        jac: None,
    };
    let tols = vec![tol; problem.init.len()];
    let rows = march("Bulstoer", &problem, |x, y, h| {
        bulstoer_trial(&sys, x, y, h, &tols)
    })?;
    ode::table(&problem, &rows)
}

/// `Adams(init, x1, x2, intvls, D, [tol])`.
pub(super) fn adams_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    implicit_form(args, ctx, Method::Adams)
}

/// `BDF(init, x1, x2, intvls, D, [J], [tol])`.
pub(super) fn bdf_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    implicit_form(args, ctx, Method::Bdf)
}

/// `Radau(init, x1, x2, intvls, D, [J], [M], [tol])`.
pub(super) fn radau_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    implicit_form(args, ctx, Method::Radau)
}

/// `AdamsBDF(init, x1, x2, intvls, D, [J], [tol])`.
pub(super) fn adamsbdf_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    implicit_form(args, ctx, Method::AdamsBdf)
}

/// The shared front end: the problem, the optional arguments, then the chosen method.
fn implicit_form(args: &[Expr], ctx: Ctx<'_>, method: Method) -> Result<Value, EvalError> {
    let name = method.name();
    if args.len() < 5 {
        return Err(EvalError::Unsupported(format!(
            "{name} takes at least five arguments: init, x1, x2, intvls, D"
        )));
    }
    let problem = ode::problem(&args[0], &args[1], &args[2], &args[3], &args[4], ctx)?;
    let n = problem.init.len();
    let Options { jac, tol } = options(&args[5..], n, ctx, name, method)?;
    let sys = Sys {
        d: Derivative {
            problem: &problem,
            ctx,
        },
        jac,
    };
    let rows = match method {
        Method::Adams => doubling(name, problem.intvls, &tol, ADAMS_STEPS, |m| {
            adams_solve(&sys, &problem, m)
        })?,
        Method::Bdf => doubling(name, problem.intvls, &tol, MAX_STEPS, |m| {
            bdf_solve(&sys, &problem, m)
        })?,
        Method::AdamsBdf => {
            match doubling(name, problem.intvls, &tol, ADAMS_STEPS, |m| {
                adams_solve(&sys, &problem, m)
            }) {
                Err(EvalError::NoConvergence(_)) => {
                    doubling(name, problem.intvls, &tol, MAX_STEPS, |m| {
                        bdf_solve(&sys, &problem, m)
                    })?
                }
                other => other?,
            }
        }
        Method::Radau => march(name, &problem, |x, y, h| radau_trial(&sys, x, y, h, &tol))?,
    };
    ode::table(&problem, &rows)
}

/// The optional arguments after `D`: the Jacobian `J` (a function name), the mass matrix `M` (the
/// identity only) and `tol` (a scalar, or one value per unknown).
struct Options<'c> {
    jac: Option<UserJac<'c>>,
    tol: Vec<f64>,
}

fn options<'c>(
    extras: &[Expr],
    n: usize,
    ctx: Ctx<'c>,
    name: &str,
    method: Method,
) -> Result<Options<'c>, EvalError> {
    let mut jac = None;
    let mut tol = None;
    for e in extras {
        if let Some((fname, func)) = named_function(e, ctx)? {
            if !method.takes_j() {
                return Err(EvalError::Unsupported(format!("{name} takes no J")));
            }
            if func.params.len() != 2 {
                return Err(EvalError::ArgCount {
                    name: fname,
                    expected: "2".to_string(),
                    found: func.params.len(),
                });
            }
            if jac.is_some() {
                return Err(EvalError::Unsupported(format!("{name}: J is given twice")));
            }
            jac = Some(UserJac {
                name: fname,
                func,
                ctx,
            });
            continue;
        }
        let value = value_of(e, ctx)?;
        if n > 1 {
            if let Value::Matrix(m) = &value {
                if m.shape() == (n, n) {
                    if method != Method::Radau {
                        return Err(EvalError::Unsupported(format!("{name} takes no M")));
                    }
                    let identity = reals_of(&value, "the mass matrix M")?
                        .iter()
                        .enumerate()
                        .all(|(k, &v)| v == if k % (n + 1) == 0 { 1.0 } else { 0.0 });
                    if !identity {
                        return Err(EvalError::Unsupported(
                            "a mass matrix M other than the identity is not implemented"
                                .to_string(),
                        ));
                    }
                    continue;
                }
            }
        }
        let values = reals_of(&value, "tol")?;
        let t = if values.len() == 1 {
            vec![values[0]; n]
        } else if values.len() == n {
            values
        } else {
            return Err(EvalError::Unsupported(
                "tol must be a scalar or one value per unknown".to_string(),
            ));
        };
        if tol.replace(t).is_some() {
            return Err(EvalError::Unsupported(format!(
                "{name}: tol is given twice"
            )));
        }
    }
    let tol = tol.unwrap_or_else(|| vec![DEFAULT_TOL; n]);
    if tol.iter().any(|t| !(*t > 0.0 && t.is_finite())) {
        return Err(EvalError::Unsupported(
            "tol must be a positive number".to_string(),
        ));
    }
    Ok(Options { jac, tol })
}

/// A user Jacobian `J(x, y)`, an `n x n` matrix.
struct UserJac<'c> {
    name: String,
    func: &'c UserFunction,
    ctx: Ctx<'c>,
}

impl UserJac<'_> {
    fn eval(&self, x: f64, y: &[f64]) -> Result<Vec<Vec<f64>>, EvalError> {
        self.ctx.budget.charge(DERIVATIVE_UNITS)?;
        let n = y.len();
        let out = call_user_uncounted(
            &self.name,
            self.func,
            vec![real_value(x), column(y)?],
            &bindings(&self.func.params[1], y),
            self.ctx,
        )?;
        if !matches!(&out, Value::Matrix(m) if m.shape() == (n, n)) {
            return Err(EvalError::Unsupported(format!(
                "J must return a {n} x {n} matrix"
            )));
        }
        let flat = reals_of(&out, "the Jacobian J")?;
        Ok(flat.chunks(n).map(<[f64]>::to_vec).collect())
    }
}

/// The components of a vector parameter bound as `name_0, name_1, ...`.
fn bindings(param: &str, y: &[f64]) -> Vec<(String, Value)> {
    y.iter()
        .enumerate()
        .map(|(k, v)| (format!("{param}_{k}"), real_value(*v)))
        .collect()
}

/// The right-hand side and its Jacobian at a point.
struct Sys<'p, 'c> {
    d: Derivative<'p, 'c>,
    jac: Option<UserJac<'c>>,
}

impl Sys<'_, '_> {
    fn f(&self, x: f64, y: &[f64]) -> Result<Vec<f64>, EvalError> {
        self.d.eval(x, y)
    }

    /// The Jacobian at `(x, y)`: the user's `J`, else a forward difference of `D` (given `f0`).
    fn jacobian(&self, x: f64, y: &[f64], f0: &[f64]) -> Result<Vec<Vec<f64>>, EvalError> {
        if let Some(j) = &self.jac {
            return j.eval(x, y);
        }
        let n = y.len();
        let mut jm = vec![vec![0.0; n]; n];
        for j in 0..n {
            let mut yp = y.to_vec();
            yp[j] += f64::EPSILON.sqrt() * y[j].abs().max(1.0);
            let delta = yp[j] - y[j];
            let fp = self.f(x, &yp)?;
            for (row, (fpi, f0i)) in jm.iter_mut().zip(fp.iter().zip(f0)) {
                row[j] = (fpi - f0i) / delta;
            }
        }
        Ok(jm)
    }

    /// Charges one dense linear solve of dimension `dim` to the budget.
    fn charge_solve(&self, dim: usize) -> Result<(), EvalError> {
        let units = (dim as u64).saturating_pow(3) / 3 + 1;
        self.d.ctx.budget.charge(units)
    }
}

/// Largest absolute value of a slice (0 for an empty one).
fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |a, x| a.max(x.abs()))
}

/// Scaled RMS norm of a local error estimate: each component against `tol` and its size.
fn scaled_rms(y: &[f64], y_new: &[f64], err: &[f64], tol: &[f64]) -> f64 {
    let sum: f64 = (0..err.len())
        .map(|i| {
            let scale = tol[i] * (1.0 + y[i].abs().max(y_new[i].abs()));
            (err[i] / scale).powi(2)
        })
        .sum();
    let norm = (sum / err.len() as f64).sqrt();
    if norm.is_finite() {
        norm
    } else {
        f64::INFINITY
    }
}

/// The largest scaled difference between two solution matrices (the points, not the x column).
fn scaled_gap(a: &[Vec<f64>], b: &[Vec<f64>], tol: &[f64]) -> f64 {
    let mut worst = 0.0f64;
    for (ra, rb) in a.iter().zip(b) {
        for i in 1..ra.len() {
            let diff = (ra[i] - rb[i]).abs();
            if !diff.is_finite() {
                return f64::INFINITY;
            }
            worst = worst.max(diff / (tol[i - 1] * (1.0 + rb[i].abs())));
        }
    }
    worst
}

/// The uniform-step driver of the multistep solvers. `solve(m)` integrates with `m` steps per
/// output interval (`None` when a step's Newton iteration fails); `m` doubles until two successive
/// solutions agree to `tol`, and the finer one is returned.
fn doubling<F>(
    name: &str,
    intvls: usize,
    tol: &[f64],
    max_steps: usize,
    mut solve: F,
) -> Result<Vec<Vec<f64>>, EvalError>
where
    F: FnMut(usize) -> Result<Option<Vec<Vec<f64>>>, EvalError>,
{
    let mut m = 1usize;
    let mut prev: Option<Vec<Vec<f64>>> = None;
    loop {
        if intvls.saturating_mul(m) > max_steps {
            return Err(EvalError::NoConvergence(name.to_string()));
        }
        // A solve whose state overflows is not converged at this step size: refine.
        let attempt = match solve(m) {
            Err(EvalError::NotFinite) => None,
            other => other?,
        };
        match attempt {
            None => prev = None,
            Some(rows) => {
                if let Some(p) = &prev {
                    if scaled_gap(p, &rows, tol) <= 1.0 {
                        return Ok(rows);
                    }
                }
                prev = Some(rows);
            }
        }
        m *= 2;
    }
}

/// Adams-Bashforth-Moulton PECE, order 4, `m` steps per output interval. `None` if a value is not
/// finite.
fn adams_solve(
    sys: &Sys<'_, '_>,
    p: &Problem<'_>,
    m: usize,
) -> Result<Option<Vec<Vec<f64>>>, EvalError> {
    let steps = p.intvls * m;
    let h = (p.x2 - p.x1) / steps as f64;
    let h_out = (p.x2 - p.x1) / p.intvls as f64;
    let n = p.init.len();
    let mut y = p.init.clone();
    let mut rows = vec![ode::row(p.x1, &y)];
    // The last four derivatives, oldest first.
    let mut fs: Vec<Vec<f64>> = vec![sys.f(p.x1, &y)?];
    for k in 0..steps {
        let next = if k < 3 {
            ode::rk4_step(&sys.d, p.x1 + k as f64 * h, &y, h)?
        } else {
            let pred: Vec<f64> = (0..n)
                .map(|i| {
                    y[i] + h / 24.0
                        * (55.0 * fs[3][i] - 59.0 * fs[2][i] + 37.0 * fs[1][i] - 9.0 * fs[0][i])
                })
                .collect();
            let fp = sys.f(p.x1 + (k + 1) as f64 * h, &pred)?;
            (0..n)
                .map(|i| {
                    y[i] + h / 24.0 * (9.0 * fp[i] + 19.0 * fs[3][i] - 5.0 * fs[2][i] + fs[1][i])
                })
                .collect()
        };
        if next.iter().any(|v| !v.is_finite()) {
            return Ok(None);
        }
        y = next;
        fs.push(sys.f(p.x1 + (k + 1) as f64 * h, &y)?);
        if fs.len() > 4 {
            fs.remove(0);
        }
        if (k + 1) % m == 0 {
            rows.push(ode::row(p.x1 + ((k + 1) / m) as f64 * h_out, &y));
        }
    }
    Ok(Some(rows))
}

/// Backward Euler, then BDF2, `m` uniform steps per output interval. `None` when a Newton iteration
/// does not converge.
fn bdf_solve(
    sys: &Sys<'_, '_>,
    p: &Problem<'_>,
    m: usize,
) -> Result<Option<Vec<Vec<f64>>>, EvalError> {
    let steps = p.intvls * m;
    let h = (p.x2 - p.x1) / steps as f64;
    let h_out = (p.x2 - p.x1) / p.intvls as f64;
    let n = p.init.len();
    let mut y = p.init.clone();
    let mut y_old: Option<Vec<f64>> = None;
    let mut rows = vec![ode::row(p.x1, &y)];
    for k in 0..steps {
        let x = p.x1 + k as f64 * h;
        // The step solves Y - base - c h D(x + h, Y) = 0.
        let (base, c): (Vec<f64>, f64) = match &y_old {
            None => (y.clone(), 1.0),
            Some(yo) => (
                (0..n)
                    .map(|i| 4.0 / 3.0 * y[i] - 1.0 / 3.0 * yo[i])
                    .collect(),
                2.0 / 3.0,
            ),
        };
        let f0 = sys.f(x, &y)?;
        let j0 = sys.jacobian(x, &y, &f0)?;
        let mat: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| f64::from(u8::from(i == j)) - c * h * j0[i][j])
                    .collect()
            })
            .collect();
        let mut yy: Vec<f64> = (0..n).map(|i| y[i] + h * f0[i]).collect();
        let mut converged = false;
        for _ in 0..BDF_NEWTON {
            sys.charge_solve(n)?;
            let fy = sys.f(x + h, &yy)?;
            let rhs: Vec<f64> = (0..n).map(|i| base[i] + c * h * fy[i] - yy[i]).collect();
            let Some(delta) = solve_dense(mat.clone(), rhs) else {
                return Ok(None);
            };
            let change = max_abs(&delta);
            for (v, d) in yy.iter_mut().zip(&delta) {
                *v += d;
            }
            if !change.is_finite() {
                return Ok(None);
            }
            if change <= NEWTON_TOL * (1.0 + max_abs(&yy)) {
                converged = true;
                break;
            }
        }
        if !converged {
            return Ok(None);
        }
        y_old = Some(std::mem::replace(&mut y, yy));
        if (k + 1) % m == 0 {
            rows.push(ode::row(p.x1 + ((k + 1) / m) as f64 * h_out, &y));
        }
    }
    Ok(Some(rows))
}

/// Result of one trial step of an adaptive solver: accepted with the new state, or rejected. The
/// factor scales the step size for the next trial.
enum Trial {
    Accept { y: Vec<f64>, factor: f64 },
    Reject { factor: f64 },
}

/// The adaptive driver: trial steps from `x1` that land on each output point, as `Stiffr` does.
fn march<S>(name: &str, p: &Problem<'_>, mut step: S) -> Result<Vec<Vec<f64>>, EvalError>
where
    S: FnMut(f64, &[f64], f64) -> Result<Trial, EvalError>,
{
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
            if trials > MAX_STEPS {
                return Err(EvalError::NoConvergence(name.to_string()));
            }
            // Land exactly on the output point when the next step would pass it.
            let remaining = target - x;
            let try_h = if h.abs() >= remaining.abs() {
                remaining
            } else {
                h
            };
            // A state the right-hand side cannot take (overflow) rejects the step, as a failed
            // Newton iteration does; any other error stops the solve.
            let trial = match step(x, &y, try_h) {
                Err(EvalError::NotFinite) => Trial::Reject { factor: 0.5 },
                other => other?,
            };
            match trial {
                Trial::Accept { y: next, factor } => {
                    let landed = try_h == remaining;
                    x = if landed { target } else { x + try_h };
                    y = next;
                    let proposed = try_h.abs() * factor;
                    h = dir
                        * if landed {
                            h.abs().max(proposed)
                        } else {
                            proposed
                        };
                }
                Trial::Reject { factor } => {
                    h = try_h * factor.min(1.0);
                    if h.abs() < f64::EPSILON * x.abs().max(1.0) {
                        return Err(EvalError::NoConvergence(name.to_string()));
                    }
                }
            }
        }
        x = target;
        rows.push(ode::row(x, &y));
    }
    Ok(rows)
}

/// The modified midpoint method: `steps` substeps of `big_h` from `(x, y)` with Gragg's smoothing.
fn midpoint(
    sys: &Sys<'_, '_>,
    x: f64,
    y: &[f64],
    big_h: f64,
    steps: usize,
) -> Result<Vec<f64>, EvalError> {
    let h = big_h / steps as f64;
    let n = y.len();
    let mut z0 = y.to_vec();
    let f0 = sys.f(x, &z0)?;
    let mut z1: Vec<f64> = (0..n).map(|i| z0[i] + h * f0[i]).collect();
    for m in 1..steps {
        let fm = sys.f(x + m as f64 * h, &z1)?;
        let z2: Vec<f64> = (0..n).map(|i| z0[i] + 2.0 * h * fm[i]).collect();
        z0 = z1;
        z1 = z2;
    }
    // z0 is z_{steps-1} and z1 is z_steps.
    let f_end = sys.f(x + big_h, &z1)?;
    Ok((0..n)
        .map(|i| 0.5 * (z1[i] + z0[i] + h * f_end[i]))
        .collect())
}

/// Neville's recurrence at zero for the points `(xs[i], ys[i])`. Returns the extrapolated value
/// over all points and over all but the last one (for the error estimate).
fn extrapolate(xs: &[f64], ys: &[Vec<f64>]) -> (Vec<f64>, Vec<f64>) {
    let k = xs.len();
    let mut tab: Vec<Vec<f64>> = ys.to_vec();
    let mut prev = ys[0].clone();
    for m in 1..k {
        let next: Vec<Vec<f64>> = (0..k - m)
            .map(|i| {
                let den = xs[i] - xs[i + m];
                (0..tab[i].len())
                    .map(|c| (-xs[i + m] * tab[i][c] + xs[i] * tab[i + 1][c]) / den)
                    .collect()
            })
            .collect();
        if m + 2 == k {
            prev = next[0].clone();
        }
        tab = next;
    }
    (tab[0].clone(), prev)
}

/// One Bulirsch-Stoer macro-step of size `h`.
fn bulstoer_trial(
    sys: &Sys<'_, '_>,
    x: f64,
    y: &[f64],
    h: f64,
    tol: &[f64],
) -> Result<Trial, EvalError> {
    let mut xs: Vec<f64> = Vec::new();
    let mut ests: Vec<Vec<f64>> = Vec::new();
    for &steps in &BS_SEQUENCE {
        ests.push(midpoint(sys, x, y, h, steps)?);
        xs.push((h / steps as f64).powi(2));
        if xs.len() < 3 {
            continue;
        }
        let (est, prev) = extrapolate(&xs, &ests);
        let diff: Vec<f64> = est.iter().zip(&prev).map(|(a, b)| a - b).collect();
        let err = scaled_rms(y, &est, &diff, tol);
        if err <= 1.0 {
            let factor = if err == 0.0 {
                MAX_GROWTH
            } else {
                (SAFETY * err.powf(-1.0 / 3.0)).clamp(MIN_SHRINK, MAX_GROWTH)
            };
            return Ok(Trial::Accept { y: est, factor });
        }
    }
    Ok(Trial::Reject { factor: 0.5 })
}

/// The three Radau IIA nodes and the Butcher matrix.
fn radau_tableau() -> ([f64; 3], [[f64; 3]; 3]) {
    let s6 = 6f64.sqrt();
    let c = [(4.0 - s6) / 10.0, (4.0 + s6) / 10.0, 1.0];
    let a = [
        [
            (88.0 - 7.0 * s6) / 360.0,
            (296.0 - 169.0 * s6) / 1800.0,
            (-2.0 + 3.0 * s6) / 225.0,
        ],
        [
            (296.0 + 169.0 * s6) / 1800.0,
            (88.0 + 7.0 * s6) / 360.0,
            (-2.0 - 3.0 * s6) / 225.0,
        ],
        [(16.0 - s6) / 36.0, (16.0 + s6) / 36.0, 1.0 / 9.0],
    ];
    (c, a)
}

/// One Radau IIA step of size `h` from `(x, y)`: the stage derivatives K are the unknowns of
/// `K_i = D(x + c_i h, y + h sum_j a_ij K_j)`. `None` when the Newton iteration does not converge.
fn radau_step(
    sys: &Sys<'_, '_>,
    x: f64,
    y: &[f64],
    h: f64,
    tableau: &([f64; 3], [[f64; 3]; 3]),
) -> Result<Option<Vec<f64>>, EvalError> {
    let (c, a) = tableau;
    let n = y.len();
    let dim = 3 * n;
    let f0 = sys.f(x, y)?;
    let j0 = sys.jacobian(x, y, &f0)?;
    // The Newton matrix I - h (A x J0) at the start of the step (simplified Newton).
    let mut mat = vec![vec![0.0; dim]; dim];
    for (bi, row_block) in a.iter().enumerate() {
        for (bj, &aij) in row_block.iter().enumerate() {
            for (r, j_row) in j0.iter().enumerate() {
                for (cc, &jv) in j_row.iter().enumerate() {
                    let delta = f64::from(u8::from(bi == bj && r == cc));
                    mat[bi * n + r][bj * n + cc] = delta - h * aij * jv;
                }
            }
        }
    }
    let mut k: Vec<Vec<f64>> = vec![f0; 3];
    for _ in 0..RADAU_NEWTON {
        sys.charge_solve(dim)?;
        let mut rhs = vec![0.0; dim];
        for (i, &ci) in c.iter().enumerate() {
            let yi: Vec<f64> = (0..n)
                .map(|r| y[r] + h * (0..3).map(|j| a[i][j] * k[j][r]).sum::<f64>())
                .collect();
            let gi = sys.f(x + ci * h, &yi)?;
            for r in 0..n {
                rhs[i * n + r] = gi[r] - k[i][r];
            }
        }
        let Some(delta) = solve_dense(mat.clone(), rhs) else {
            return Ok(None);
        };
        let change = max_abs(&delta);
        if !change.is_finite() {
            return Ok(None);
        }
        for (i, ki) in k.iter_mut().enumerate() {
            for (r, v) in ki.iter_mut().enumerate() {
                *v += delta[i * n + r];
            }
        }
        let size = k.iter().map(|ki| max_abs(ki)).fold(0.0f64, f64::max);
        if change <= NEWTON_TOL * (1.0 + size) {
            let y_new: Vec<f64> = (0..n)
                .map(|r| y[r] + h * (0..3).map(|j| a[2][j] * k[j][r]).sum::<f64>())
                .collect();
            return Ok(Some(y_new));
        }
    }
    Ok(None)
}

/// One adaptive Radau trial: one step of `h` against two of `h/2`. The error of the finer result is
/// their difference over `2^5 - 1` (the method's order is 5, the step-doubling factor 2^5 - 1 = 31).
fn radau_trial(
    sys: &Sys<'_, '_>,
    x: f64,
    y: &[f64],
    h: f64,
    tol: &[f64],
) -> Result<Trial, EvalError> {
    let tableau = radau_tableau();
    let Some(big) = radau_step(sys, x, y, h, &tableau)? else {
        return Ok(Trial::Reject { factor: 0.5 });
    };
    let half = h / 2.0;
    let Some(mid) = radau_step(sys, x, y, half, &tableau)? else {
        return Ok(Trial::Reject { factor: 0.5 });
    };
    let Some(fine) = radau_step(sys, x + half, &mid, half, &tableau)? else {
        return Ok(Trial::Reject { factor: 0.5 });
    };
    let err: Vec<f64> = fine.iter().zip(&big).map(|(f, b)| (f - b) / 31.0).collect();
    let e = scaled_rms(y, &fine, &err, tol);
    if !e.is_finite() {
        return Ok(Trial::Reject { factor: MIN_SHRINK });
    }
    let factor = if e == 0.0 {
        MAX_GROWTH
    } else {
        (SAFETY * e.powf(-1.0 / 6.0)).clamp(MIN_SHRINK, MAX_GROWTH)
    };
    if e <= 1.0 {
        Ok(Trial::Accept { y: fine, factor })
    } else {
        Ok(Trial::Reject { factor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neville_extrapolation_is_exact_for_polynomials() {
        // q(t) = 1 + 2t - 3t^2 sampled at t = h^2 for h = 0.5, 0.25, 0.125: q(0) = 1 exactly.
        let q = |t: f64| 1.0 + 2.0 * t - 3.0 * t * t;
        let xs = [0.25, 0.0625, 0.015625];
        let ys: Vec<Vec<f64>> = xs.iter().map(|&t| vec![q(t)]).collect();
        let (est, prev) = extrapolate(&xs, &ys);
        assert!((est[0] - 1.0).abs() < 1e-12, "{est:?}");
        assert_eq!(prev.len(), 1);
    }

    #[test]
    fn radau_nodes_end_at_one_and_the_last_row_is_the_weights() {
        let (c, a) = radau_tableau();
        assert!((c[2] - 1.0).abs() < 1e-15);
        // Stiffly accurate: the weights (last row) sum to 1.
        assert!((a[2].iter().sum::<f64>() - 1.0).abs() < 1e-14);
    }
}

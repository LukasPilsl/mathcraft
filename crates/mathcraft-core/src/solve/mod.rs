//! Numeric core for Mathcad solve blocks: `find` (solve equalities, with inequalities and bounds)
//! and `minerr` (least-squares residual minimisation).
//!
//! Pure numerics: no worksheet, units or parsing. The caller builds a [`Problem`] from closures
//! and scales the unknowns to SI base units.
//!
//! # Semantics (Prime help, "TOL and CTOL" and "System of Equations Functions")
//! - CTOL (default 0.001) is an absolute tolerance in constraint units. An equality `f = 0` is
//!   met when `|f| <= CTOL`; an inequality `g >= 0` is met when `g >= -CTOL`. Help example:
//!   `x < 0.693` with CTOL = 0.001 accepts `x` up to 0.694.
//! - TOL: the help says solve-block tolerances are internal (solver accuracy 10^-12) and are not
//!   set in solve blocks. The default here is 1e-12. The effective value is `min(TOL, CTOL)`, as
//!   the help says a smaller CTOL becomes the accuracy.
//! - Exact `find` convergence test, all at the returned point:
//!   1. every equality has `|f_i| <= CTOL` and every inequality has `g_j >= -CTOL` (absolute,
//!      in the user's units; never scaled);
//!   2. the last accepted step satisfies `|dx_j| <= TOL * (|x_j| + 1e-30)` for each unknown `j`
//!      (relative to each unknown, so an unknown near 1e-9 is held to 1e-21).
//!
//!   A stalled Newton line search also counts as converged when (1) holds and one of: (2) holds
//!   for the full Newton step; the scaled residual is at the round-off floor,
//!   `max_i |f_i| / scale_i <= TOL` (scales below); or the full Newton step is at most
//!   `1e-9` relative per unknown. The floor case is needed for ill-conditioned systems
//!   (Hilbert-8: its last Newton step is above TOL on x ~ 1 and no line search can lower the
//!   residual). The
//!   `1e-9` case covers large systems whose round-off keeps the residual above the floor: there
//!   the line search has stalled at a feasible point, so the Newton correction is the distance to
//!   the nearby root, and the accuracy is about `1e-9 * |x_j|` times the conditioning of J. Both
//!   are accepted only when the line search cannot reduce the residual, so a root that the search
//!   could still approach is not stopped early.
//!   A stalled Levenberg-Marquardt run converges only at a stationary point: the scaled
//!   stationarity ratio `|J^T r|_inf / (max|J_ik| |r|_2) <= 1e-8`, on the unweighted gradient
//!   (see `lm::stationarity`). Stationarity is tested without row scales, because LM minimises the
//!   unweighted sum of squares, so a row weighting would move the point it converges to.
//!   A small residual alone is never enough for LM, so `atan(x) = 1.6` or `1e-15 (x - 3)` at a
//!   wrong point is not reported as converged.
//!   A converged result is polished by up to two Gauss-Newton steps, kept only
//!   if the residual norm does not grow and the point stays feasible.
//!   `converged` means the constraints hold at `x`. For ill-conditioned systems the error in `x`
//!   can be large (Hilbert-10: residual 3e-16, error 5e-4), and that is conditioning, not a bug.
//! - Scaling: each equation has a scale fixed at the guess,
//!   `max(|r_i(guess)|, ||J_i||_1 * (||x||_inf + 1e-300), 1e-300)`, with no absolute floor of 1
//!   (an inequality row uses at least CTOL). The Newton line search compares residual norms with
//!   each equation divided by its scale. Newton steps themselves are unchanged (they are invariant
//!   to row scaling). Levenberg-Marquardt minimises the plain sum of squares, because weighting
//!   rows would change the least-squares minimiser; its damping floor is relative to the largest
//!   diagonal entry, so `1e-15 (x - 3)` is solved like `x - 3`. Jacobian stencils are relative to
//!   each unknown. A badly scaled equation such as `1e-12 * f` accepts residuals up to CTOL in its
//!   own units only through its scale, and accuracy comes from test 2.
//! - `minerr` returns the best point of a Levenberg-Marquardt run. `residual` is ERR = `|r|_2`
//!   over the equalities and the inequality violations `min(0, g + CTOL)`, not its square. The
//!   help says LM sums and squares the residuals internally but does not define ERR, so the
//!   2-norm is kept.
//!
//! # Method and limits
//! - Square equality systems: Newton with central-difference Jacobian and backtracking. On a
//!   singular Jacobian or a stall, Levenberg-Marquardt runs from the same point.
//! - Other systems and anything with inequalities: Levenberg-Marquardt on `[f; min(0, g + CTOL)]`.
//!   This finds some feasible point, not an optimum.
//! - Inequality starts: if no ordinary start gives a feasible point, starts are moved onto the
//!   feasible side of each inequality violated at the guess (the guess is moved along the
//!   constraint gradient until the linearised constraint equals `max(|g|, 1)`). For `x^2 = 4`
//!   with `x >= 0` from -1 this start is 1, which gives 2. An active-set variant (adding the
//!   violated inequality as an equality) is not used: for that example `x = 0` contradicts
//!   `x^2 = 4`. Limits: the seed uses the linearised constraint, so a nonlinear or thin or
//!   disconnected feasible set can be missed. The result is then `NoConvergence`, never a point
//!   that violates a constraint.
//! - Singular ties: when the Jacobian is singular at a point equidistant from two roots (the
//!   circle and line sample: the gradients of `x^2 + y^2` and `x + y` are parallel on `x = y`),
//!   Newton stops and LM picks a side from round-off. This build returns `(-0.414, 2.414)` for
//!   guess `(1, 1)` and `(2.414, -0.414)` for `(1, 0.99)`. The help's first example shows
//!   `(2.414, -0.414)` for `(1, 1)`, so this is a known mismatch, not a documented tie rule.
//! - Underdetermined systems (`m < n`): damped least-norm steps, so the solution is the one
//!   nearest the guess. Overdetermined systems (`m > n`) must be consistent, or `Inconsistent`.
//! - Bounds are enforced by projecting every trial point (not a full box-constrained method).
//! - Restarts: if the first run fails, up to four deterministic offsets of the guess are tried,
//!   then the inequality seeds. Each start gets its own iteration allowance (Newton and LM from
//!   the same start share it); the evaluation budget is global. Results are reproducible for the
//!   same inputs.
//! - Budgets: with `Limits::max_evaluations = None` (automatic) each start may run
//!   `max(25, WORK_CAP / (n^2 * (m + k)))` iterations (`WORK_CAP` = 7e9, see `auto_iterations`;
//!   the floor of 25 keeps large well-posed systems such as tri-diagonal n = 1000, which needs
//!   about 7, and dense least squares n = 1000, about 12). The evaluations are `8 * n * iterations`
//!   clamped to `MIN_EVALUATIONS..=MAX_EVALUATIONS` (50 000 000) and are shared by all starts.
//!   A Jacobian costs `4n` closure calls. Measured: about 0.08 s per iteration at n = 1000 in
//!   release. A dense no-root system at n = 1000 uses the full allowance for each start: 25
//!   iterations, each about 0.35 s, so about 9 s per start and about 44 s for the five starts.
//!   A harder large system that needs more iterations returns `NoConvergence`. Callers that solve
//!   big systems, such as the Session integration, should pass explicit `Limits`.
//! - Complex solutions are not supported.

mod linalg;
mod lm;
mod newton;
mod optimize;
mod problem;
mod scale;

#[cfg(test)]
mod tests;

pub use optimize::{optimize, Optimum, UNBOUNDED_X};
pub use problem::{
    auto_evaluations, auto_iterations, ConstraintFn, IterationHook, IterationInfo, Limits, Problem,
    Solution, SolveError, DEFAULT_CTOL, DEFAULT_TOL, MAX_CELLS, MAX_DIM, MAX_EVALUATIONS,
    MIN_EVALUATIONS, WORK_CAP,
};

use linalg::{lu_solve, norm2, norm2_sq};
use problem::{Evaluator, Halt, Kind};

/// Outer polish steps after a converged solve.
const POLISH_STEPS: usize = 2;
/// Most feasible inequality seeds tried after the ordinary restarts.
const MAX_SEEDS: usize = 8;

/// Solves the equalities and inequalities of `problem` from its guess.
pub fn find(problem: &Problem<'_>) -> Result<Solution, SolveError> {
    let (n, m, k, tol) = validate(problem)?;
    let mut ev = Evaluator::new(problem, n, m, k, tol);
    let guess = ev.clamp(problem.guess.clone());
    match find_run(&mut ev, &guess) {
        Ok(sol) => Ok(sol),
        Err(Halt::Budget) => Err(SolveError::NoConvergence {
            iterations: ev.iterations,
            residual: ev.best().map_or(f64::NAN, |(norm, _)| *norm),
        }),
        Err(Halt::Error(e)) => Err(e),
    }
}

/// Least-squares minimisation of the residual from the guess (Levenberg-Marquardt).
///
/// Returns the best point found even when it is not a zero: `converged` then says whether the
/// run stopped at a stationary point or on the step tolerance. Only input errors and NaN/inf
/// values are returned as `Err`; a spent budget returns the best point with `converged = false`.
pub fn minerr(problem: &Problem<'_>) -> Result<Solution, SolveError> {
    let (n, m, k, tol) = validate(problem)?;
    let mut ev = Evaluator::new(problem, n, m, k, tol);
    let guess = ev.clamp(problem.guess.clone());
    match minerr_run(&mut ev, &guess) {
        Ok(sol) => Ok(sol),
        Err(Halt::Budget) => {
            let (norm, x) = ev
                .best()
                .cloned()
                .unwrap_or_else(|| (f64::NAN, guess.clone()));
            Ok(Solution {
                x,
                converged: false,
                residual: norm,
                iterations: ev.iterations,
                evaluations: ev.evals,
            })
        }
        Err(Halt::Error(e)) => Err(e),
    }
}

fn find_run(ev: &mut Evaluator<'_, '_>, guess: &[f64]) -> Result<Solution, Halt> {
    ev.prepare(guess)?;
    let square = ev.m == ev.n && ev.k == 0;
    let mut starts = restarts(ev, guess);
    if ev.k > 0 {
        starts.extend(feasible_seeds(ev, guess)?);
    }
    let mut last_r = Vec::new();
    for start in starts {
        ev.begin_run();
        let mut attempt = None;
        if square {
            let a = newton::run(ev, start.clone())?;
            if a.converged && ev.feasible(&a.r) {
                attempt = Some(a);
            }
        }
        let a = match attempt {
            Some(a) => a,
            None => lm::run(ev, start)?,
        };
        if a.converged && ev.feasible(&a.r) {
            let (px, pr) = polish(ev, a.x.clone())?;
            let (x, r) = if ev.feasible(&pr) {
                (px, pr)
            } else {
                (a.x, a.r)
            };
            return Ok(Solution {
                x,
                converged: true,
                residual: norm2(&r),
                iterations: ev.iterations,
                evaluations: ev.evals,
            });
        }
        last_r = a.r;
    }
    let residual = norm2(&last_r);
    if ev.m > ev.n {
        Err(Halt::Error(SolveError::Inconsistent { residual }))
    } else {
        Err(Halt::Error(SolveError::NoConvergence {
            iterations: ev.iterations,
            residual,
        }))
    }
}

fn minerr_run(ev: &mut Evaluator<'_, '_>, guess: &[f64]) -> Result<Solution, Halt> {
    ev.prepare(guess)?;
    ev.begin_run();
    let a = lm::run(ev, guess.to_vec())?;
    let (x, r) = polish(ev, a.x)?;
    Ok(Solution {
        x,
        converged: a.converged,
        residual: norm2(&r),
        iterations: ev.iterations,
        evaluations: ev.evals,
    })
}

/// The guess, then four deterministic offsets of it (scaled by `max(|g|, 1)`), clipped to bounds.
fn restarts(ev: &Evaluator<'_, '_>, guess: &[f64]) -> Vec<Vec<f64>> {
    let mut starts = vec![ev.clamp(guess.to_vec())];
    for s in [0.1, -0.1, 1.0, -1.0] {
        let shifted: Vec<f64> = guess.iter().map(|g| g + s * g.abs().max(1.0)).collect();
        starts.push(ev.clamp(shifted));
    }
    starts
}

/// Starts on the feasible side of each inequality violated at the guess. The guess moves along
/// the constraint gradient `grad` until the linearised constraint equals `max(|g|, 1)`, which is
/// the projection of the guess onto the linearised feasible region with a unit margin.
fn feasible_seeds(ev: &mut Evaluator<'_, '_>, guess: &[f64]) -> Result<Vec<Vec<f64>>, Halt> {
    let r = ev.eval(Kind::Aug, guess)?;
    let n = guess.len();
    let mut out = Vec::new();
    let j = ev.jacobian(Kind::Aug, guess, r.len())?;
    for i in ev.m..r.len() {
        if r[i] >= 0.0 || out.len() == MAX_SEEDS {
            continue;
        }
        // r_i = min(0, g + CTOL) and r_i < 0 only when g < -CTOL, so g = r_i - CTOL.
        let g = r[i] - ev.ctol;
        let grad = &j[i * n..(i + 1) * n];
        let nn = norm2_sq(grad);
        if !(nn > 0.0 && nn.is_finite()) {
            continue;
        }
        let margin = (-g).max(1.0);
        let t = (margin - g) / nn;
        let seed: Vec<f64> = guess.iter().zip(grad).map(|(x, d)| x + t * d).collect();
        out.push(ev.clamp(seed));
    }
    Ok(out)
}

/// Up to `POLISH_STEPS` exact-Jacobian Gauss-Newton steps. A step is kept only if the residual
/// norm does not grow. Returns the point and its residual.
fn polish(ev: &mut Evaluator<'_, '_>, mut x: Vec<f64>) -> Result<(Vec<f64>, Vec<f64>), Halt> {
    let mut r = ev.eval(Kind::Aug, &x)?;
    for _ in 0..POLISH_STEPS {
        let rows = r.len();
        let n = x.len();
        if rows == 0 || r.iter().all(|v| *v == 0.0) {
            break;
        }
        let j = ev.jacobian(Kind::Aug, &x, rows)?;
        let Some(dx) = exact_direction(&j, rows, n, &r) else {
            break;
        };
        let trial: Vec<f64> = x.iter().zip(&dx).map(|(a, d)| a + d).collect();
        let trial = ev.clamp(trial);
        let rt = ev.eval(Kind::Aug, &trial)?;
        if norm2(&rt) > norm2(&r) {
            break;
        }
        x = trial;
        r = rt;
    }
    Ok((x, r))
}

/// Gauss-Newton step with no damping: direct LU when square, normal equations otherwise.
fn exact_direction(j: &[f64], rows: usize, cols: usize, r: &[f64]) -> Option<Vec<f64>> {
    if rows == cols {
        let mut a = j.to_vec();
        let mut rhs: Vec<f64> = r.iter().map(|v| -v).collect();
        lu_solve(&mut a, cols, &mut rhs).then_some(rhs)
    } else {
        lm::direction(j, rows, cols, r, 0.0)
    }
}

/// Checks sizes, tolerances and bounds. Returns `(n, m, k, effective tolerance)`.
fn validate(p: &Problem<'_>) -> Result<(usize, usize, usize, f64), SolveError> {
    let n = p.guess.len();
    let m = p.eq.as_ref().map_or(0, |(m, _)| *m);
    let k = p.ineq.as_ref().map_or(0, |(k, _)| *k);
    if n == 0 {
        return Err(SolveError::InvalidProblem("no unknowns".into()));
    }
    if n > MAX_DIM || m > MAX_DIM || k > MAX_DIM || n.saturating_mul(m + k) > MAX_CELLS {
        return Err(SolveError::TooLarge { n, m: m + k });
    }
    if m + k == 0 {
        return Err(SolveError::Unsupported(
            "no constraints: nothing to solve".into(),
        ));
    }
    if p.guess.iter().any(|v| !v.is_finite()) {
        return Err(SolveError::InvalidProblem("guess must be finite".into()));
    }
    if !(p.tol.is_finite() && p.tol > 0.0) {
        return Err(SolveError::InvalidProblem("TOL must be positive".into()));
    }
    if !(p.ctol.is_finite() && p.ctol > 0.0) {
        return Err(SolveError::InvalidProblem("CTOL must be positive".into()));
    }
    if p.limits.max_iterations == 0 || p.limits.max_evaluations == Some(0) {
        return Err(SolveError::InvalidProblem(
            "budgets must be positive".into(),
        ));
    }
    if let (Some(lo), Some(hi)) = (&p.lower, &p.upper) {
        if lo.len() != n || hi.len() != n {
            return Err(SolveError::InvalidProblem(
                "bounds must have length n".into(),
            ));
        }
        if lo
            .iter()
            .zip(hi)
            .any(|(a, b)| a.is_nan() || b.is_nan() || a > b)
        {
            return Err(SolveError::InvalidProblem(
                "lower bound above upper bound".into(),
            ));
        }
    }
    // Mathcad: a CTOL below the default becomes the calculation accuracy.
    Ok((n, m, k, p.tol.min(p.ctol)))
}

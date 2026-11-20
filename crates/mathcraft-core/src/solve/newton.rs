//! Newton iteration for square equality systems (`m == n`, no inequalities).
//!
//! Each step solves `J dx = -f` with a central-difference Jacobian, then backtracks (halving the
//! step) until the scaled residual norm decreases (Armijo-style test with factor 1e-4). The
//! scaled residual divides each equation by its scale at the guess (see `Evaluator::prepare`).
//! A singular Jacobian or a stalled line search returns `converged = false`, and the caller falls
//! back to Levenberg-Marquardt. A stalled line search counts as converged only when the residual
//! is within CTOL and one of three holds: the full Newton step passes the TOL step test, the
//! scaled residual is at the round-off floor (`Evaluator::at_floor`), or the full Newton step is
//! at most `STALL_STEP` relative per unknown (see the comment at the stall test).

use super::linalg::{lu_solve, norm2, norm_inf};
use super::problem::{Attempt, Evaluator, Halt, Kind};

/// Smallest backtracking factor tried before the step is declared stalled.
const MIN_ALPHA: f64 = 1.0 / 1024.0;
/// Relative per-unknown Newton step accepted at a stalled line search (see `run`).
const STALL_STEP: f64 = 1e-9;

/// Runs Newton from `x0`. The result is converged only when the residual is within CTOL and the
/// last accepted step passes `Evaluator::step_small` (see the crate docs for the exact test).
pub(crate) fn run(ev: &mut Evaluator<'_, '_>, x0: Vec<f64>) -> Result<Attempt, Halt> {
    let n = x0.len();
    let mut x = ev.clamp(x0);
    let mut f = ev.eval(Kind::Eq, &x)?;
    let mut converged = false;
    while ev.iterate()? {
        if norm_inf(&f) == 0.0 {
            converged = true;
            break;
        }
        let mut jac = ev.jacobian(Kind::Eq, &x, n)?;
        let mut dx: Vec<f64> = f.iter().map(|v| -v).collect();
        if !lu_solve(&mut jac, n, &mut dx) {
            // Singular Jacobian: hand over to Levenberg-Marquardt.
            break;
        }
        let f_norm = norm2(&ev.scaled(&f));
        let mut alpha = 1.0;
        let mut accepted = None;
        while alpha >= MIN_ALPHA {
            let trial: Vec<f64> = x.iter().zip(&dx).map(|(a, d)| a + alpha * d).collect();
            let trial = ev.clamp(trial);
            let ft = ev.eval(Kind::Eq, &trial)?;
            if norm2(&ev.scaled(&ft)) <= (1.0 - 1e-4 * alpha) * f_norm {
                accepted = Some((trial, ft));
                break;
            }
            alpha *= 0.5;
        }
        let Some((trial, ft)) = accepted else {
            // No decrease possible. Converged when the residual is feasible and one of:
            // (a) the full Newton step passes the TOL step test;
            // (b) the scaled residual is at the round-off floor (inf-norm <= TOL), which covers
            //     ill-conditioned systems such as Hilbert-8, whose last step is above TOL but
            //     cannot lower |f|;
            // (c) the full Newton step is at most STALL_STEP (1e-9) relative per unknown:
            //     |dx_j| <= 1e-9 (|x_j| + 1e-30). This only applies here, after the line search
            //     has stalled at a feasible point, so no further decrease is available along the
            //     Newton direction. Newton's correction is then the distance to the nearby root
            //     up to the conditioning of J, so the returned x is accurate to about
            //     1e-9 * (|x_j| + 1e-30) times the condition number of J at the root. Measured
            //     need: a tri-diagonal cubic at n = 1000 stalls with |dx| = 4e-12 at round-off
            //     ~4e-10 above the floor, which (a) and (b) both reject.
            converged = ev.feasible(&f)
                && (ev.step_small(&dx, &x)
                    || ev.at_floor(&f)
                    || ev.step_within(&dx, &x, STALL_STEP));
            break;
        };
        let step: Vec<f64> = trial.iter().zip(&x).map(|(a, b)| a - b).collect();
        x = trial;
        f = ft;
        if ev.step_small(&step, &x) && ev.feasible(&f) {
            converged = true;
            break;
        }
    }
    Ok(Attempt { x, r: f, converged })
}

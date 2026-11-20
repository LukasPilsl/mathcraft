//! Levenberg-Marquardt on the augmented residual `r = [f(x); min(0, g(x) + CTOL)]`.
//!
//! With no inequalities `r` is just the equalities, so this is plain nonlinear least squares. An
//! inequality contributes only when violated, so `r = 0` exactly when every constraint holds
//! (a feasibility problem). Steps are Marquardt-damped Gauss-Newton: `(J^T J + lambda D) dx = -J^T r`
//! when `rows >= cols`, and the dual form `dx = J^T (J J^T + lambda D)^-1 (-r)` otherwise (the
//! least-norm step for underdetermined systems). `D` is the diagonal of the Gram matrix.
//! The cost is the plain sum of squares, as minerr's least-squares objective requires: rows are
//! not rescaled here, because weighting them would change the minimiser.

use super::linalg::{gram_cols, gram_rows, lu_solve, mat_t_vec, norm2_sq, norm_inf};
use super::problem::{Attempt, Evaluator, Halt, Kind};

const INITIAL_LAMBDA: f64 = 1e-3;
const MIN_LAMBDA: f64 = 1e-12;
const GROW: f64 = 4.0;
/// Trial steps per iteration before the run is declared stuck.
const MAX_TRIES: usize = 30;
/// A stall counts as stationary when the stationarity ratio (`stationarity`) is at most this. Relative to the natural size of the gradient, not to its start value: a start
/// value test is fooled when the gradient decays toward a flat tail (`atan(x) = 1.6` at x = 1e13).
const GRAD_TOL: f64 = 1e-8;

/// One damped step for a residual `r` (length `rows`) and Jacobian `j` (`rows x cols`).
/// `None` when the damped system is numerically singular.
pub(crate) fn direction(
    j: &[f64],
    rows: usize,
    cols: usize,
    r: &[f64],
    lambda: f64,
) -> Option<Vec<f64>> {
    if rows >= cols {
        let mut a = gram_cols(j, cols);
        damp(&mut a, cols, lambda);
        let mut rhs = mat_t_vec(j, cols, r);
        rhs.iter_mut().for_each(|v| *v = -*v);
        lu_solve(&mut a, cols, &mut rhs).then_some(rhs)
    } else {
        let mut a = gram_rows(j, rows, cols);
        damp(&mut a, rows, lambda);
        let mut y: Vec<f64> = r.iter().map(|v| -v).collect();
        if !lu_solve(&mut a, rows, &mut y) {
            return None;
        }
        Some(mat_t_vec(j, cols, &y))
    }
}

/// Adds `lambda * max(a_ii, 1e-12 * max_k a_kk)` to each diagonal entry of the `n x n` matrix
/// `a`. The floor is relative to the largest diagonal, so the damping does not depend on the
/// units of the equations: an equation of size 1e-15 is damped like one of size 1.
fn damp(a: &mut [f64], n: usize, lambda: f64) {
    let max_diag = (0..n).fold(0.0_f64, |s, i| s.max(a[i * n + i]));
    for i in 0..n {
        let d = a[i * n + i];
        a[i * n + i] += lambda * d.max(1e-12 * max_diag);
    }
}

/// Stationarity ratio of the unweighted least-squares cost `|r|^2 / 2`, relative to its natural
/// size: `|J^T r|_inf / (max |J_ik| * |r|_2)`. It is 0 at a stationary point, is unchanged when
/// every equation is multiplied by the same constant, and is 1 for one equation with a nonzero
/// slope (so a flat tail such as `atan(x) = 1.6` at large x is not stationary). The gradient is
/// unweighted because LM minimises the plain sum of squares; row scales would move the minimiser
/// of the reported least-squares point. A stall is stationary when this is at most `GRAD_TOL`
/// (1e-8): round-off in the ratio at scale 1e6 is about 1e-10, so a tighter bound misses
/// convergence there. Returns 0 when `J` is zero.
pub(crate) fn stationarity(j: &[f64], rows: usize, cols: usize, r: &[f64]) -> f64 {
    let mut g = vec![0.0; cols];
    let mut jmax = 0.0_f64;
    for i in 0..rows {
        for (k, jik) in j[i * cols..(i + 1) * cols].iter().enumerate() {
            jmax = jmax.max(jik.abs());
            g[k] += jik * r[i];
        }
    }
    let denom = (jmax * norm2_sq(r).sqrt()).max(f64::MIN_POSITIVE);
    norm_inf(&g) / denom
}

/// Runs Levenberg-Marquardt from `x0`. `converged` is set when the last accepted step is small
/// (taken with lambda <= 1, so Gauss-Newton-like), or when the search stalls at a stationary
/// point (`stationarity <= GRAD_TOL`). A stall with a large gradient is not convergence,
/// whatever the residual. Callers that need a zero residual also check `Evaluator::feasible`.
pub(crate) fn run(ev: &mut Evaluator<'_, '_>, x0: Vec<f64>) -> Result<Attempt, Halt> {
    let n = x0.len();
    let mut x = ev.clamp(x0);
    let mut r = ev.eval(Kind::Aug, &x)?;
    let mut cost = norm2_sq(&r);
    let mut lambda = INITIAL_LAMBDA;
    let mut converged = false;
    while ev.iterate()? {
        if cost == 0.0 {
            converged = true;
            break;
        }
        let rows = r.len();
        let j = ev.jacobian(Kind::Aug, &x, rows)?;
        let stationary = stationarity(&j, rows, n, &r) <= GRAD_TOL;

        let mut accepted = None;
        for _ in 0..MAX_TRIES {
            let Some(dx) = direction(&j, rows, n, &r, lambda) else {
                lambda *= GROW;
                continue;
            };
            let trial: Vec<f64> = x.iter().zip(&dx).map(|(a, d)| a + d).collect();
            let trial = ev.clamp(trial);
            let rt = ev.eval(Kind::Aug, &trial)?;
            let ct = norm2_sq(&rt);
            if ct < cost {
                accepted = Some((trial, rt, ct));
                break;
            }
            lambda *= GROW;
        }
        let Some((trial, rt, ct)) = accepted else {
            // No decrease found: converged only at a stationary point.
            converged = stationary;
            break;
        };
        let step: Vec<f64> = trial.iter().zip(&x).map(|(a, b)| a - b).collect();
        let small = ev.step_small(&step, &trial);
        let used = lambda;
        x = trial;
        r = rt;
        cost = ct;
        lambda = (lambda / GROW).max(MIN_LAMBDA);
        if small && used <= 1.0 {
            converged = true;
            break;
        }
    }
    Ok(Attempt { x, r, converged })
}

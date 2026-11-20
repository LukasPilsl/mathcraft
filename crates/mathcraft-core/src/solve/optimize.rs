//! Minimisation (and maximisation, by the caller negating the objective): help "Optimization
//! Functions", `maximize` and `minimize`.
//!
//! Method: a local quasi-Newton search. Unconstrained problems run BFGS on the objective with an
//! Armijo backtracking line search and central-difference gradients (step `FD_REL * max(|x_j|, 1)`).
//! Constraints use the augmented Lagrangian method: an equality `h = 0` has multiplier `lam` and the
//! term `lam h + rho/2 h^2`; an inequality `g >= 0` has multiplier `mu >= 0` and the term
//! `(max(0, mu - rho g)^2 - mu^2) / (2 rho)` (Rockafellar's form). Each bound `lo <= x_j <= hi` is one
//! or two inequalities. Each outer round minimises the augmented Lagrangian with BFGS; the
//! multipliers then move and the penalty `rho` grows until the point is feasible within
//! `min(CTOL, FEAS_CAP)`.
//!
//! Help: the result is the stationary point the search reaches from the guess (the local extremum
//! nearest the guess), not necessarily the global one. A result is returned only when every inner
//! search converged and the point is feasible. Otherwise:
//! - `Infeasible`: no point satisfies the constraints (the outer rounds end infeasible);
//! - `Unbounded`: an unknown runs past [`UNBOUNDED_X`];
//! - `NoConvergence`: an iteration or evaluation cap, or a stalled search away from a stationary
//!   point (never a plausible number);
//! - `BudgetExceeded` (from the caller's iteration hook), or a closure's own error.
//!
//! The objective and the constraints are evaluated through the closures of [`Problem`], so every
//! evaluation is charged by the caller. The iteration hook is called once per BFGS iteration.

use std::cell::Cell;

use super::linalg::{norm2, norm_inf};
use super::problem::{IterationInfo, Problem, SolveError, MAX_DIM};

/// Relative step of the central-difference gradient (balances truncation and round-off, about
/// 1e-10 absolute gradient error for unit-scale functions).
const FD_REL: f64 = 1e-5;
/// Inner search stops when `|grad|_inf * max(1, |x|_inf) <= GRAD_TOL * max(|L|, |L_0|)`, where `L_0`
/// is the merit at the start of the search (so a tiny objective is measured against its own scale).
const GRAD_TOL: f64 = 1e-9;
/// A search that cannot decrease the merit even after a reset is accepted as converged when its
/// gradient is at most this (the round-off floor of the finite-difference gradient).
const STALL_GRAD_TOL: f64 = 1e-6;
/// Progress test at the round-off floor: a step at most `PROGRESS_STEP_REL` (relative) that lowers
/// the merit by at most `PROGRESS_F_REL` (relative to its scale), `PROGRESS_STEPS` times in a row,
/// ends the search. The gradient must then be within `STALL_GRAD_TOL`, else the search fails.
const PROGRESS_STEP_REL: f64 = 1e-10;
const PROGRESS_F_REL: f64 = 1e-14;
const PROGRESS_STEPS: usize = 5;
/// Armijo sufficient-decrease constant and the most halvings of one line search.
const ARMIJO: f64 = 1e-4;
const MAX_LINE_HALVINGS: usize = 50;
/// BFGS iterations allowed to one inner search, and outer augmented-Lagrangian rounds.
const MAX_INNER: usize = 500;
const MAX_OUTER: usize = 60;
/// Initial penalty, its growth when the violation does not fall by a factor 4, and its cap.
const RHO0: f64 = 10.0;
const RHO_GROWTH: f64 = 10.0;
const RHO_MAX: f64 = 1e12;
/// Largest feasibility violation accepted, whatever CTOL is. The objective's error is of the order
/// of this violation times the multipliers, so it is kept far below the 4-digit display precision.
const FEAS_CAP: f64 = 1e-10;
/// Smallest feasibility tolerance used, whatever a tiny CTOL says (the round-off of the violation).
const FEAS_MIN: f64 = 1e-13;
/// Curvature check of an unconstrained stationary point: the Hessian's smallest eigenvalue may be
/// negative by `HESS_REL * max|λ| + HESS_NOISE * |f| / xs^2` (the finite-difference noise), where
/// `xs = max(1, |x|_inf)`. Restarts pushed along the lowest-curvature direction by
/// `CURVATURE_STEP * 2^retry * xs`, at most `MAX_CURVATURE_RETRIES` times.
const HESS_REL: f64 = 1e-6;
const HESS_NOISE: f64 = 1e-5;
const CURVATURE_STEP: f64 = 0.1;
const MAX_CURVATURE_RETRIES: usize = 3;
/// Step of the finite-difference Hessian (relative to `max(1, |x_j|)`).
const HESS_FD_REL: f64 = 1e-4;
/// An unknown beyond this magnitude means the objective is unbounded in that direction.
pub const UNBOUNDED_X: f64 = 1e12;

/// The result of an optimisation.
#[derive(Debug, Clone, PartialEq)]
pub struct Optimum {
    /// The unknowns at the optimum (feasible within the tolerance).
    pub x: Vec<f64>,
    /// The objective at `x`, as the problem's closure gives it (not negated).
    pub value: f64,
    /// BFGS iterations used over the whole solve.
    pub iterations: usize,
    /// Objective and constraint evaluations (one per point evaluated).
    pub evaluations: usize,
}

/// Minimises the problem's objective over its constraints and bounds, from its guess. Constraints
/// and bounds follow the rules of [`Problem`]: equalities, inequalities `g >= 0`, and bounds.
pub fn optimize(p: &Problem<'_>) -> Result<Optimum, SolveError> {
    let (n, m, k_user) = validate(p)?;
    let bounds = bound_rows(p, n);
    let k = k_user + bounds.len();
    let core = Core {
        p,
        n,
        m,
        k,
        bounds,
        evals: Cell::new(0),
        iters: Cell::new(0),
        max_iters: p.limits.max_iterations,
        max_evals: p.limits.max_evaluations.unwrap_or(usize::MAX),
    };
    let feas = p.ctol.clamp(FEAS_MIN, FEAS_CAP);
    let mut al = Al {
        lam: vec![0.0; m],
        mu: vec![0.0; k],
        rho: RHO0,
    };
    let mut x = p.guess.clone();
    // The scale of the merit's values, from its magnitude at the guess. Fixed for the whole solve:
    // a later round's merit can be near zero, and its own magnitude must not shrink the scale.
    let floor = core.merit(&x, &al)?.abs().max(f64::MIN_POSITIVE);
    if m + k == 0 {
        // A stationary point is a minimum only if the Hessian is positive semi-definite. A search
        // that stops at a maximum or a saddle (`x^4 - x^2` from 0) restarts from a point pushed
        // along the lowest-curvature direction. Constrained problems are not curvature-checked.
        let mut start = x;
        for retry in 0..=MAX_CURVATURE_RETRIES {
            let (xs, converged) = core.search(start, &al, floor)?;
            if !converged {
                return Err(SolveError::NoConvergence {
                    iterations: core.iters.get(),
                    residual: f64::NAN,
                });
            }
            let Some(dir) = core.lowest_curvature(&xs, &al)? else {
                return core.finish(xs);
            };
            let step = CURVATURE_STEP * 2f64.powi(retry as i32) * norm_inf(&xs).max(1.0);
            start = xs.iter().zip(&dir).map(|(a, d)| a + step * d).collect();
        }
        return Err(SolveError::NoConvergence {
            iterations: core.iters.get(),
            residual: f64::NAN,
        });
    }
    let mut prev = f64::INFINITY;
    let mut viol = f64::INFINITY;
    for _ in 0..MAX_OUTER {
        let (xn, converged) = core.search(x, &al, floor)?;
        x = xn;
        if !converged {
            return Err(SolveError::NoConvergence {
                iterations: core.iters.get(),
                residual: viol,
            });
        }
        let (_, h, g) = core.parts(&x)?;
        viol = violation(&h, &g);
        if viol <= feas {
            return core.finish(x);
        }
        for (lam, hv) in al.lam.iter_mut().zip(&h) {
            *lam += al.rho * hv;
        }
        for (mu, gv) in al.mu.iter_mut().zip(&g) {
            *mu = (*mu - al.rho * gv).max(0.0);
        }
        if viol > 0.25 * prev {
            if al.rho >= RHO_MAX {
                break;
            }
            al.rho = (al.rho * RHO_GROWTH).min(RHO_MAX);
        }
        prev = viol;
    }
    Err(SolveError::Infeasible { violation: viol })
}

/// Multipliers and penalty of the augmented Lagrangian. With no constraints it is the objective.
struct Al {
    lam: Vec<f64>,
    mu: Vec<f64>,
    rho: f64,
}

impl Al {
    /// The augmented Lagrangian at objective `f`, equalities `h` and inequalities `g` (`g >= 0`).
    fn value(&self, f: f64, h: &[f64], g: &[f64]) -> f64 {
        let mut l = f;
        for (i, hv) in h.iter().enumerate() {
            l += self.lam[i] * hv + 0.5 * self.rho * hv * hv;
        }
        for (j, gv) in g.iter().enumerate() {
            let mu = self.mu[j];
            let t = (mu - self.rho * gv).max(0.0);
            l += (t * t - mu * mu) / (2.0 * self.rho);
        }
        l
    }
}

/// One optimisation's data: the problem, sizes, the bound rows and the counters.
struct Core<'p, 'a> {
    p: &'p Problem<'a>,
    n: usize,
    /// Equalities.
    m: usize,
    /// Inequalities, the user's then the bounds'.
    k: usize,
    /// Bound rows: row `g = sign * x[j] + c`.
    bounds: Vec<(usize, f64, f64)>,
    evals: Cell<usize>,
    iters: Cell<usize>,
    max_iters: usize,
    max_evals: usize,
}

impl Core<'_, '_> {
    /// Objective, equalities and inequalities (user's, then bounds) at `x`. One evaluation.
    fn parts(&self, x: &[f64]) -> Result<(f64, Vec<f64>, Vec<f64>), SolveError> {
        let e = self.evals.get() + 1;
        if e > self.max_evals {
            return Err(SolveError::NoConvergence {
                iterations: self.iters.get(),
                residual: f64::NAN,
            });
        }
        self.evals.set(e);
        let objective = self
            .p
            .objective
            .as_ref()
            .ok_or_else(|| SolveError::InvalidProblem("no objective".into()))?;
        let f = objective(x)?;
        if !f.is_finite() {
            return Err(SolveError::NotFinite);
        }
        let h = match &self.p.eq {
            Some((m, eq)) => checked(eq(x)?, *m)?,
            None => Vec::new(),
        };
        let mut g = match &self.p.ineq {
            Some((k, ineq)) => checked(ineq(x)?, *k)?,
            None => Vec::new(),
        };
        g.extend(self.bounds.iter().map(|(j, s, c)| s * x[*j] + c));
        Ok((f, h, g))
    }

    /// The augmented Lagrangian at `x`.
    fn merit(&self, x: &[f64], al: &Al) -> Result<f64, SolveError> {
        let (f, h, g) = self.parts(x)?;
        Ok(al.value(f, &h, &g))
    }

    /// Central-difference gradient of the augmented Lagrangian (2n evaluations).
    fn gradient(&self, x: &[f64], al: &Al) -> Result<Vec<f64>, SolveError> {
        let mut grad = vec![0.0; self.n];
        let mut xp = x.to_vec();
        for j in 0..self.n {
            let h = FD_REL * x[j].abs().max(1.0);
            let (hi, lo) = (x[j] + h, x[j] - h);
            xp[j] = hi;
            let fp = self.merit(&xp, al)?;
            xp[j] = lo;
            let fm = self.merit(&xp, al)?;
            xp[j] = x[j];
            grad[j] = (fp - fm) / (hi - lo);
        }
        Ok(grad)
    }

    /// For an unconstrained stationary point `x`: `None` when the finite-difference Hessian of the
    /// objective is positive semi-definite (within the noise), else the unit eigenvector of its
    /// lowest eigenvalue, the direction of least curvature to restart along.
    fn lowest_curvature(&self, x: &[f64], al: &Al) -> Result<Option<Vec<f64>>, SolveError> {
        let n = self.n;
        let mut h = vec![0.0; n * n];
        let mut xp = x.to_vec();
        for j in 0..n {
            let step = HESS_FD_REL * x[j].abs().max(1.0);
            xp[j] = x[j] + step;
            let gp = self.gradient(&xp, al)?;
            xp[j] = x[j] - step;
            let gm = self.gradient(&xp, al)?;
            xp[j] = x[j];
            for i in 0..n {
                h[i * n + j] = (gp[i] - gm[i]) / (2.0 * step);
            }
        }
        for i in 0..n {
            for j in i + 1..n {
                let v = 0.5 * (h[i * n + j] + h[j * n + i]);
                h[i * n + j] = v;
                h[j * n + i] = v;
            }
        }
        let (vals, vecs) = jacobi_eigen(h, n);
        let lmax = vals.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
        let (k, lmin) = vals
            .iter()
            .enumerate()
            .fold(
                (0, f64::INFINITY),
                |(bk, bv), (k, &v)| {
                    if v < bv {
                        (k, v)
                    } else {
                        (bk, bv)
                    }
                },
            );
        let fx = self.merit(x, al)?;
        let xs = norm_inf(x).max(1.0);
        let tol = HESS_REL * lmax + HESS_NOISE * fx.abs() / (xs * xs);
        if lmin >= -tol || lmin.is_nan() {
            return Ok(None);
        }
        Ok(Some((0..n).map(|i| vecs[i * n + k]).collect()))
    }

    /// Counts one BFGS iteration and calls the caller's hook (its work budget).
    fn tick(&self) -> Result<(), SolveError> {
        let it = self.iters.get() + 1;
        self.iters.set(it);
        if it > self.max_iters {
            return Err(SolveError::NoConvergence {
                iterations: it - 1,
                residual: f64::NAN,
            });
        }
        if let Some(hook) = &self.p.on_iteration {
            hook(&IterationInfo {
                n: self.n,
                m: self.m,
                k: self.k,
                iteration: it,
            })?;
        }
        Ok(())
    }

    /// BFGS minimisation of the augmented Lagrangian from `start`. Returns the point and whether
    /// the search converged to a stationary point. `floor` is the merit's scale at the guess (see
    /// `optimize`): the search's scale is at least that, so a tiny objective does not count as
    /// converged at the guess.
    fn search(&self, start: Vec<f64>, al: &Al, floor: f64) -> Result<(Vec<f64>, bool), SolveError> {
        let n = self.n;
        let mut x = start;
        let mut fx = self.merit(&x, al)?;
        let mut g = self.gradient(&x, al)?;
        let mut hinv = identity(n);
        let mut fresh = true;
        let mut floor_steps = 0usize;
        for _ in 0..MAX_INNER {
            self.tick()?;
            if x.iter().any(|v| v.abs() > UNBOUNDED_X) {
                return Err(SolveError::Unbounded);
            }
            // The gradient is measured per unit of x (`xs`), so a runaway point is not accepted as
            // stationary just because its value is huge.
            let scale = fx.abs().max(floor);
            let xs = norm_inf(&x).max(1.0);
            let gnorm = norm_inf(&g);
            if gnorm * xs <= GRAD_TOL * scale {
                return Ok((x, true));
            }
            let mut d = mat_vec(&hinv, n, &g);
            for v in &mut d {
                *v = -*v;
            }
            let mut slope = dot(&g, &d);
            if slope.is_nan() || slope >= 0.0 {
                hinv = identity(n);
                fresh = true;
                d = g.iter().map(|v| -v).collect();
                slope = -dot(&g, &g);
            }
            let mut t = 1.0;
            let mut accepted = None;
            for _ in 0..MAX_LINE_HALVINGS {
                let xt: Vec<f64> = x.iter().zip(&d).map(|(a, b)| a + t * b).collect();
                // A trial point where the objective is not finite is a failed step, not an error.
                let ft = match self.merit(&xt, al) {
                    Ok(v) => v,
                    Err(SolveError::NotFinite) => f64::INFINITY,
                    Err(e) => return Err(e),
                };
                if ft <= fx + ARMIJO * t * slope {
                    accepted = Some((xt, ft));
                    break;
                }
                t *= 0.5;
            }
            let Some((xt, ft)) = accepted else {
                if !fresh {
                    hinv = identity(n);
                    fresh = true;
                    continue;
                }
                return Ok((x, gnorm <= STALL_GRAD_TOL * scale));
            };
            let gt = self.gradient(&xt, al)?;
            let s: Vec<f64> = xt.iter().zip(&x).map(|(a, b)| a - b).collect();
            let y: Vec<f64> = gt.iter().zip(&g).map(|(a, b)| a - b).collect();
            let sy = dot(&s, &y);
            if sy > 0.0 && sy > 1e-12 * norm2(&s) * norm2(&y) {
                bfgs_update(&mut hinv, n, &s, &y, sy);
                fresh = false;
            }
            // Negligible progress for several steps in a row: the search is at the round-off floor
            // of the merit (a curved valley with a penalty), not moving to a better point.
            let tiny_step = norm_inf(&s) <= PROGRESS_STEP_REL * (1.0 + norm_inf(&xt));
            if tiny_step && (fx - ft).abs() <= PROGRESS_F_REL * scale {
                floor_steps += 1;
                if floor_steps >= PROGRESS_STEPS {
                    return Ok((xt.clone(), norm_inf(&gt) <= STALL_GRAD_TOL * scale));
                }
            } else {
                floor_steps = 0;
            }
            x = xt;
            fx = ft;
            g = gt;
        }
        Ok((x, false))
    }

    /// The result at `x`: its objective value and the counts.
    fn finish(&self, x: Vec<f64>) -> Result<Optimum, SolveError> {
        let (value, _, _) = self.parts(&x)?;
        Ok(Optimum {
            x,
            value,
            iterations: self.iters.get(),
            evaluations: self.evals.get(),
        })
    }
}

/// Checks sizes, the guess, the objective, the tolerance and the bounds. Returns `(n, m, k)` for
/// the user's equalities and inequalities (bounds are counted by [`bound_rows`]).
fn validate(p: &Problem<'_>) -> Result<(usize, usize, usize), SolveError> {
    let n = p.guess.len();
    let m = p.eq.as_ref().map_or(0, |(m, _)| *m);
    let k = p.ineq.as_ref().map_or(0, |(k, _)| *k);
    if n == 0 {
        return Err(SolveError::InvalidProblem("no unknowns".into()));
    }
    if n > MAX_DIM || m > MAX_DIM || k > MAX_DIM {
        return Err(SolveError::TooLarge { n, m: m + k });
    }
    if p.objective.is_none() {
        return Err(SolveError::InvalidProblem("no objective".into()));
    }
    if p.guess.iter().any(|v| !v.is_finite()) {
        return Err(SolveError::InvalidProblem("guess must be finite".into()));
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
    Ok((n, m, k))
}

/// The finite bounds as inequality rows `sign * x[j] + c >= 0`: `x_j - lo` and `hi - x_j`.
fn bound_rows(p: &Problem<'_>, n: usize) -> Vec<(usize, f64, f64)> {
    let (Some(lo), Some(hi)) = (&p.lower, &p.upper) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for j in 0..n {
        if lo[j].is_finite() {
            rows.push((j, 1.0, -lo[j]));
        }
        if hi[j].is_finite() {
            rows.push((j, -1.0, hi[j]));
        }
    }
    rows
}

/// A constraint closure's values, checked for length and finiteness.
fn checked(v: Vec<f64>, expected: usize) -> Result<Vec<f64>, SolveError> {
    if v.len() != expected {
        return Err(SolveError::DimensionMismatch {
            expected,
            got: v.len(),
        });
    }
    if v.iter().any(|x| !x.is_finite()) {
        return Err(SolveError::NotFinite);
    }
    Ok(v)
}

/// The largest violation: `|h|` for equalities, `max(0, -g)` for inequalities.
fn violation(h: &[f64], g: &[f64]) -> f64 {
    let eq = h.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
    g.iter().fold(eq, |a, v| a.max(-v))
}

/// Eigenvalues and eigenvectors of a symmetric `n x n` matrix (row-major) by cyclic Jacobi
/// rotations. The eigenvector `k` is column `k` of the returned row-major matrix.
fn jacobi_eigen(mut a: Vec<f64>, n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = identity(n);
    for _ in 0..100 {
        let total: f64 = a.iter().map(|x| x * x).sum();
        let off: f64 = (0..n)
            .flat_map(|p| (p + 1..n).map(move |q| (p, q)))
            .map(|(p, q)| a[p * n + q] * a[p * n + q])
            .sum();
        if off <= 1e-30 * total {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq == 0.0 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k * n + p], a[k * n + q]);
                    a[k * n + p] = c * akp - s * akq;
                    a[k * n + q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p * n + k], a[q * n + k]);
                    a[p * n + k] = c * apk - s * aqk;
                    a[q * n + k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
}

fn identity(n: usize) -> Vec<f64> {
    let mut m = vec![0.0; n * n];
    for i in 0..n {
        m[i * n + i] = 1.0;
    }
    m
}

fn mat_vec(m: &[f64], n: usize, v: &[f64]) -> Vec<f64> {
    (0..n)
        .map(|i| {
            m[i * n..(i + 1) * n]
                .iter()
                .zip(v)
                .map(|(a, b)| a * b)
                .sum()
        })
        .collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The BFGS update of the inverse Hessian `h` with the step `s` and gradient change `y`:
/// `H + (1 + rho y'Hy) rho s s' - rho (Hy s' + s (Hy)')`, `rho = 1 / (s'y)`.
fn bfgs_update(h: &mut [f64], n: usize, s: &[f64], y: &[f64], sy: f64) {
    let rho = 1.0 / sy;
    let hy = mat_vec(h, n, y);
    let yhy = dot(y, &hy);
    for i in 0..n {
        for j in 0..n {
            h[i * n + j] +=
                (1.0 + rho * yhy) * rho * s[i] * s[j] - rho * (hy[i] * s[j] + s[i] * hy[j]);
        }
    }
}

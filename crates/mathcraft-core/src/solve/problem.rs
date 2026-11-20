//! Problem definition for the numeric solve core: unknowns, constraints, tolerances, limits, and
//! the evaluator that counts closure calls, checks their values and tracks the best point seen.

use thiserror::Error;

use super::linalg::{norm2, norm_inf};

/// A constraint function: unknowns in, constraint values out.
pub type ConstraintFn<'a> = Box<dyn Fn(&[f64]) -> Result<Vec<f64>, SolveError> + 'a>;

/// The objective of an optimisation (`optimize`): unknowns in, one real value out (minimised).
pub type ObjectiveFn<'a> = Box<dyn Fn(&[f64]) -> Result<f64, SolveError> + 'a>;

/// Called once per outer iteration (Newton or Levenberg-Marquardt step) with the problem's size. An
/// error aborts the solve and is returned by `find` or `minerr`. Used to charge the solver's own
/// work (the linear algebra and the constraint-closure cost) to a caller's budget.
pub type IterationHook<'a> = Box<dyn Fn(&IterationInfo) -> Result<(), SolveError> + 'a>;

/// What an [`IterationHook`] is told about the solve at the start of an outer iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IterationInfo {
    /// Unknowns.
    pub n: usize,
    /// Equalities.
    pub m: usize,
    /// Inequalities.
    pub k: usize,
    /// Outer iterations so far over the whole solve, this one included (1 for the first).
    pub iteration: usize,
}

/// Largest number of unknowns, and largest number of equalities or inequalities, accepted.
pub const MAX_DIM: usize = 10_000;
/// Largest `n * (m + k)` accepted, which bounds the Jacobian to about 32 MB.
pub const MAX_CELLS: usize = 4_000_000;
/// Default convergence tolerance TOL (Prime help: solver accuracy 10^-12, not set in solve blocks).
pub const DEFAULT_TOL: f64 = 1e-12;
/// Default constraint tolerance CTOL (Prime help: 0.001).
pub const DEFAULT_CTOL: f64 = 1e-3;
/// Base relative step for the Jacobian stencil (the wider stencil uses twice this).
pub(crate) const FD_STEP: f64 = 1e-3;
/// Absolute floor in the per-unknown step test, so that a root at exactly zero is reachable.
pub(crate) const ABS_FLOOR: f64 = 1e-30;
/// Smallest automatic evaluation budget.
pub const MIN_EVALUATIONS: usize = 100_000;
/// Absolute cap on the automatic evaluation budget (Jacobians cost `4n` calls each).
pub const MAX_EVALUATIONS: usize = 50_000_000;
/// Dense-work cap for automatic iteration budgets, in units of `n^2 * rows` (one iteration costs
/// about that many multiply-adds). Measured: n = 1000 square takes about 0.08 s per Newton or
/// Levenberg-Marquardt iteration in release, and n = 2000 with one row about 20 ms per iteration.
pub const WORK_CAP: f64 = 7.0e9;
/// Fewest automatic iterations per start, whatever `n` is. Large well-posed systems (tri-diagonal
/// n = 1000 needs 4 Newton steps, dense least squares n = 1000 needs about 12) must not be cut
/// by the work cap, which is only a guard against minutes-long dense runs.
pub const MIN_AUTO_ITERATIONS: usize = 25;

/// Iteration and evaluation budgets for one solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum outer iterations (Newton or Levenberg-Marquardt steps).
    pub max_iterations: usize,
    /// Maximum constraint-function calls (each call of an equality or inequality closure counts).
    /// `None` is automatic: the iteration count is first cut to `WORK_CAP / (n^2 * rows)` (see
    /// `auto_iterations`), then the evaluations are `8 * n * iterations`, clamped to
    /// `MIN_EVALUATIONS..=MAX_EVALUATIONS`, so a large problem gets enough Jacobians to finish.
    /// With an explicit `Some(_)` the iteration count is used as given.
    pub max_evaluations: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_iterations: 200,
            max_evaluations: None,
        }
    }
}

/// The automatic iteration budget for `n` unknowns and `rows` constraints per start:
/// `max_iterations`, cut to `WORK_CAP / (n^2 * rows)` but never below `MIN_AUTO_ITERATIONS`.
pub fn auto_iterations(n: usize, rows: usize, max_iterations: usize) -> usize {
    let cost = (n as f64).powi(2) * rows.max(1) as f64;
    let cap = (WORK_CAP / cost).floor().max(MIN_AUTO_ITERATIONS as f64) as usize;
    max_iterations.min(cap)
}

/// The automatic evaluation budget for `n` unknowns and `max_iterations` iterations.
pub fn auto_evaluations(n: usize, max_iterations: usize) -> usize {
    n.saturating_mul(8)
        .saturating_mul(max_iterations)
        .clamp(MIN_EVALUATIONS, MAX_EVALUATIONS)
}

/// Errors from building or solving a problem. Closures may return any of these.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SolveError {
    /// The problem definition is inconsistent (bad sizes, tolerances or bounds).
    #[error("invalid problem: {0}")]
    InvalidProblem(String),
    /// A constraint closure returned a vector of the wrong length.
    #[error("constraint returned {got} values, expected {expected}")]
    DimensionMismatch { expected: usize, got: usize },
    /// Too many unknowns or constraints for the solver's allocation caps.
    #[error("problem too large: {n} unknowns, {m} constraints")]
    TooLarge { n: usize, m: usize },
    /// A constraint value was NaN or infinite; the solve is aborted.
    #[error("constraint value is NaN or infinite")]
    NotFinite,
    /// The iteration or evaluation budget ran out, or no feasible point was reached.
    #[error("no convergence after {iterations} iterations (best residual {residual:e})")]
    NoConvergence { iterations: usize, residual: f64 },
    /// More equalities than unknowns, and the least-squares point does not satisfy them.
    #[error("overdetermined system is inconsistent (residual {residual:e})")]
    Inconsistent { residual: f64 },
    /// The problem is outside what this core handles.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// An optimisation found no point that satisfies the constraints within the tolerance.
    #[error("no point satisfies the constraints (violation {violation:e})")]
    Infeasible { violation: f64 },
    /// An optimisation's unknowns run off to infinity: the objective has no minimum there.
    #[error("the objective is unbounded")]
    Unbounded,
    /// The caller's [`IterationHook`] refused a further iteration (its work budget is spent).
    #[error("work budget exceeded")]
    BudgetExceeded,
}

/// A solve problem: `n` unknowns, `m` equalities `f_i(x) = 0`, `k` inequalities `g_j(x) >= 0`,
/// optional bounds, the guess, and the tolerances.
pub struct Problem<'a> {
    pub(crate) guess: Vec<f64>,
    pub(crate) eq: Option<(usize, ConstraintFn<'a>)>,
    pub(crate) ineq: Option<(usize, ConstraintFn<'a>)>,
    pub(crate) lower: Option<Vec<f64>>,
    pub(crate) upper: Option<Vec<f64>>,
    pub(crate) tol: f64,
    pub(crate) ctol: f64,
    pub(crate) limits: Limits,
    pub(crate) on_iteration: Option<IterationHook<'a>>,
    pub(crate) objective: Option<ObjectiveFn<'a>>,
}

impl<'a> Problem<'a> {
    /// A problem with the given guess (its length is `n`) and no constraints yet.
    pub fn new(guess: Vec<f64>) -> Self {
        Problem {
            guess,
            eq: None,
            ineq: None,
            lower: None,
            upper: None,
            tol: DEFAULT_TOL,
            ctol: DEFAULT_CTOL,
            limits: Limits::default(),
            on_iteration: None,
            objective: None,
        }
    }

    /// Sets the objective of an optimisation (see `optimize`). `find` and `minerr` ignore it.
    pub fn objective<F>(mut self, f: F) -> Self
    where
        F: Fn(&[f64]) -> Result<f64, SolveError> + 'a,
    {
        self.objective = Some(Box::new(f));
        self
    }

    /// Sets a hook called at the start of every outer iteration (see [`IterationHook`]).
    pub fn on_iteration<H>(mut self, hook: H) -> Self
    where
        H: Fn(&IterationInfo) -> Result<(), SolveError> + 'a,
    {
        self.on_iteration = Some(Box::new(hook));
        self
    }

    /// Sets `m` equality constraints; `f` maps the `n` unknowns to exactly `m` values.
    pub fn equations<F>(mut self, m: usize, f: F) -> Self
    where
        F: Fn(&[f64]) -> Result<Vec<f64>, SolveError> + 'a,
    {
        self.eq = Some((m, Box::new(f)));
        self
    }

    /// Sets `k` inequality constraints `g_j(x) >= 0`; `g` maps the unknowns to exactly `k` values.
    pub fn inequalities<G>(mut self, k: usize, g: G) -> Self
    where
        G: Fn(&[f64]) -> Result<Vec<f64>, SolveError> + 'a,
    {
        self.ineq = Some((k, Box::new(g)));
        self
    }

    /// Sets elementwise bounds `lower <= x <= upper` (infinite entries allowed).
    pub fn bounds(mut self, lower: Vec<f64>, upper: Vec<f64>) -> Self {
        self.lower = Some(lower);
        self.upper = Some(upper);
        self
    }

    /// Sets the convergence tolerance TOL (relative step size).
    pub fn tol(mut self, tol: f64) -> Self {
        self.tol = tol;
        self
    }

    /// Sets the constraint tolerance CTOL (absolute, in constraint units).
    pub fn ctol(mut self, ctol: f64) -> Self {
        self.ctol = ctol;
        self
    }

    /// Sets the iteration and evaluation budgets.
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }
}

/// The result of a solve.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    /// The unknowns.
    pub x: Vec<f64>,
    /// `find`: all constraints met within CTOL and the last step below TOL.
    /// `minerr`: the iteration reached a stationary point or the step tolerance.
    pub converged: bool,
    /// Euclidean norm of the constraint residual vector (Mathcad's ERR for `minerr`).
    pub residual: f64,
    /// Outer iterations used.
    pub iterations: usize,
    /// Constraint-closure calls used.
    pub evaluations: usize,
}

/// Internal early exit: the budget ran out, or a real error.
#[derive(Debug)]
pub(crate) enum Halt {
    Budget,
    Error(SolveError),
}

impl From<SolveError> for Halt {
    fn from(e: SolveError) -> Self {
        Halt::Error(e)
    }
}

/// Which residual an engine works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The equalities only (length `m`).
    Eq,
    /// Equalities, then `min(0, g_j + CTOL)` for each inequality (length `m + k`).
    Aug,
}

/// Output of one engine run.
#[derive(Debug, Clone)]
pub(crate) struct Attempt {
    pub x: Vec<f64>,
    pub r: Vec<f64>,
    /// The engine's own stopping test passed (small step, or vanishing gradient).
    pub converged: bool,
}

/// Evaluates the constraints for the engines, enforcing budgets and finiteness.
pub(crate) struct Evaluator<'p, 'a> {
    problem: &'p Problem<'a>,
    pub n: usize,
    pub m: usize,
    pub k: usize,
    pub evals: usize,
    /// Outer iterations over the whole solve (reported).
    pub iterations: usize,
    /// Outer iterations of the current start (the per-start allowance is `max_iterations`).
    run_iterations: usize,
    best: Option<(f64, Vec<f64>)>,
    tol: f64,
    pub ctol: f64,
    max_iterations: usize,
    max_evaluations: usize,
    /// Per-row scale, fixed at the guess (see `prepare`); residual and Jacobian rows divide by it.
    pub(super) scale: Vec<f64>,
}

impl<'p, 'a> Evaluator<'p, 'a> {
    pub fn new(problem: &'p Problem<'a>, n: usize, m: usize, k: usize, tol: f64) -> Self {
        let max_iterations = match problem.limits.max_evaluations {
            None => auto_iterations(n, m + k, problem.limits.max_iterations),
            Some(_) => problem.limits.max_iterations,
        };
        Evaluator {
            problem,
            n,
            m,
            k,
            evals: 0,
            iterations: 0,
            run_iterations: 0,
            best: None,
            tol,
            ctol: problem.ctol,
            max_iterations,
            max_evaluations: problem
                .limits
                .max_evaluations
                .unwrap_or_else(|| auto_evaluations(n, max_iterations)),
            scale: vec![1.0; m + k],
        }
    }

    /// Per-unknown relative step test: every `|step_j| <= tol * (|x_j| + ABS_FLOOR)`, where `x`
    /// is the point the step arrives at.
    pub fn step_within(&self, step: &[f64], x: &[f64], tol: f64) -> bool {
        step.iter()
            .zip(x)
            .all(|(d, xj)| d.abs() <= tol * (xj.abs() + ABS_FLOOR))
    }

    /// The solve's convergence step test (TOL, see `step_within`).
    pub fn step_small(&self, step: &[f64], x: &[f64]) -> bool {
        self.step_within(step, x, self.tol)
    }

    /// True when the equation-scaled residual `r` is at the numerical floor: its inf-norm is at
    /// most TOL. Used only for a stalled line search (see `newton::run`), never on its own.
    pub fn at_floor(&self, r: &[f64]) -> bool {
        norm_inf(&self.scaled(r)) <= self.tol
    }

    /// Starts a new start point: its iteration allowance is `max_iterations` again. The
    /// evaluation budget is not reset.
    pub fn begin_run(&mut self) {
        self.run_iterations = 0;
    }

    /// Starts one outer iteration: `Ok(false)` when this start's iteration allowance is spent. The
    /// problem's [`IterationHook`], if any, runs first and may abort the solve.
    pub fn iterate(&mut self) -> Result<bool, Halt> {
        if self.run_iterations >= self.max_iterations {
            return Ok(false);
        }
        self.run_iterations += 1;
        self.iterations += 1;
        if let Some(hook) = &self.problem.on_iteration {
            hook(&IterationInfo {
                n: self.n,
                m: self.m,
                k: self.k,
                iteration: self.iterations,
            })?;
        }
        Ok(true)
    }

    /// The point with the smallest residual norm seen so far.
    pub fn best(&self) -> Option<&(f64, Vec<f64>)> {
        self.best.as_ref()
    }

    /// Projects `x` onto the bounds.
    pub fn clamp(&self, mut x: Vec<f64>) -> Vec<f64> {
        for (j, v) in x.iter_mut().enumerate() {
            *v = self.clamp_at(j, *v);
        }
        x
    }

    fn clamp_at(&self, j: usize, v: f64) -> f64 {
        let mut v = v;
        if let Some(lo) = &self.problem.lower {
            v = v.max(lo[j]);
        }
        if let Some(hi) = &self.problem.upper {
            v = v.min(hi[j]);
        }
        v
    }

    /// True when every equality is within CTOL and every inequality is satisfied within CTOL.
    /// `r` is a residual of kind `Aug` (or `Eq` when there are no inequalities).
    pub fn feasible(&self, r: &[f64]) -> bool {
        r.iter().enumerate().all(|(i, v)| {
            if i < self.m {
                v.abs() <= self.ctol
            } else {
                *v == 0.0
            }
        })
    }

    fn charge(&mut self) -> Result<(), Halt> {
        if self.evals >= self.max_evaluations {
            return Err(Halt::Budget);
        }
        self.evals += 1;
        Ok(())
    }

    fn checked(v: Vec<f64>, expected: usize) -> Result<Vec<f64>, Halt> {
        if v.len() != expected {
            return Err(Halt::Error(SolveError::DimensionMismatch {
                expected,
                got: v.len(),
            }));
        }
        if v.iter().any(|x| !x.is_finite()) {
            return Err(Halt::Error(SolveError::NotFinite));
        }
        Ok(v)
    }

    fn call_eq(&mut self, x: &[f64]) -> Result<Vec<f64>, Halt> {
        let Some((m, f)) = self.problem.eq.as_ref() else {
            return Ok(Vec::new());
        };
        self.charge()?;
        let v = f(x)?;
        Self::checked(v, *m)
    }

    fn call_ineq(&mut self, x: &[f64]) -> Result<Vec<f64>, Halt> {
        let Some((k, g)) = self.problem.ineq.as_ref() else {
            return Ok(Vec::new());
        };
        self.charge()?;
        let v = g(x)?;
        Self::checked(v, *k)
    }

    /// Residual of the given kind at `x`. Counts one call per constraint closure used.
    pub fn eval(&mut self, kind: Kind, x: &[f64]) -> Result<Vec<f64>, Halt> {
        let mut r = self.call_eq(x)?;
        if kind == Kind::Aug {
            let g = self.call_ineq(x)?;
            let ctol = self.ctol;
            r.extend(g.into_iter().map(|v| (v + ctol).min(0.0)));
        }
        let norm = norm2(&r);
        if self.best.as_ref().is_none_or(|(b, _)| norm < *b) {
            self.best = Some((norm, x.to_vec()));
        }
        Ok(r)
    }

    /// Jacobian of the residual of `kind` at `x`, row-major `rows x n`, by Richardson-extrapolated
    /// central differences: `(4 D(h) - D(2h)) / 3` with `h = FD_STEP * |x_j|` (or `FD_STEP` when
    /// `x_j = 0`), so the stencil scales with each unknown. This has O(h^4) truncation error and
    /// keeps round-off near 1e-13, which the polish steps need for inconsistent least-squares
    /// problems. A step clipped by a bound falls back to one central difference with its width.
    pub fn jacobian(&mut self, kind: Kind, x: &[f64], rows: usize) -> Result<Vec<f64>, Halt> {
        let n = x.len();
        let mut jac = vec![0.0; rows * n];
        for j in 0..n {
            let base = if x[j] == 0.0 { 1.0 } else { x[j].abs() };
            let h = FD_STEP * base;
            let (d1, clipped) = self.column(kind, x, j, h)?;
            let col = if clipped {
                d1
            } else {
                let (d2, _) = self.column(kind, x, j, 2.0 * h)?;
                d1.iter()
                    .zip(&d2)
                    .map(|(a, b)| (4.0 * a - b) / 3.0)
                    .collect()
            };
            for (i, v) in col.into_iter().enumerate().take(rows) {
                jac[i * n + j] = v;
            }
        }
        Ok(jac)
    }

    /// One central-difference column for variable `j` with step `h`. Returns the column and
    /// whether a bound clipped either end of the stencil.
    fn column(
        &mut self,
        kind: Kind,
        x: &[f64],
        j: usize,
        h: f64,
    ) -> Result<(Vec<f64>, bool), Halt> {
        let mut xp = x.to_vec();
        xp[j] = self.clamp_at(j, x[j] + h);
        let mut xm = x.to_vec();
        xm[j] = self.clamp_at(j, x[j] - h);
        let clipped = xp[j] != x[j] + h || xm[j] != x[j] - h;
        let denom = xp[j] - xm[j];
        if denom == 0.0 {
            return Ok((Vec::new(), true));
        }
        let fp = self.eval(kind, &xp)?;
        let fm = self.eval(kind, &xm)?;
        let col = fp.iter().zip(&fm).map(|(p, q)| (p - q) / denom).collect();
        Ok((col, clipped))
    }
}

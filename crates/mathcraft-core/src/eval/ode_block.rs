//! `odesolve(vf, b, [intvls])` inside a solve block (help: "ODE Function", "Solving ODEs").
//!
//! The block gives the ODEs as equations whose derivatives are primed calls (`x''(t)`, `x'(t)`) or
//! `d/dt x(t)`, and the initial conditions as equations at one point (`x(0) = -1`, `x'(0) = 1`).
//! `X := odesolve(x(t), T)` and `[X; Y] := odesolve([x(t); y(t)], T)` bind the solution as
//! functions of `t` on the interval from the initial point to `T`.
//!
//! How it works:
//! - Each ODE is linear in the highest derivatives of the unknowns (the help's rule). Its first-order
//!   form is found numerically: the residuals are evaluated with the highest derivatives set to 0
//!   and to each unit vector, which gives the linear system `J u = -r0` for them. For a linear
//!   dependence this is exact.
//! - The integration is the adaptive Dormand-Prince solver of `ode` ([`ode::adaptive`]) with the
//!   fixed [`INTEGRATION_TOL`]: the help says odesolve is not affected by TOL. It lands on the
//!   `intvls + 1` output points, and the solution between them is a cubic spline (`cspline` and
//!   `interp`), so the returned function is a plain closure over its table.
//! - Every residual and derivative evaluation is charged to the block's work budget, and the solve
//!   takes at most [`MAX_STEPS`] accepted steps.
//!
//! Refused with a message: boundary value problems (conditions at two points; sbval is not
//! implemented), algebraic constraints, inequalities, an end point equal to the initial point, and
//! a stiff problem: the explicit steps either run past [`MAX_STEPS`] or make the solution diverge
//! (see [`DIVERGENCE_LIMIT`]), and both are refused. No stiff method (Stiffr-style) is wired in yet.
//! An ODE that is not linear in its highest derivative (`x''^2 == 4`) is refused, both by a probe
//! at the start and by the residual of the solved derivatives. The solution function refuses points
//! outside the solved range: the help limits it to that range, so it is not extrapolated.

use std::cell::RefCell;

use super::closure::{capture, uses_variable};
use super::env::{Env, UserFunction};
use super::error::EvalError;
use super::expr_eval::{evaluate_budget, number_value, var_key};
use super::ode::{self, Stepper, DERIVATIVE_UNITS};
use super::solve_constraints::Constraint;
use super::value::{Matrix, Scalar, Value};
use super::work::Budget;
use crate::expr::{BinaryOp, Expr, Id};
use crate::units::Dimension;

/// Output intervals when the call gives none (help: 1000).
const DEFAULT_INTVLS: usize = 1000;
/// Largest number of output intervals accepted (the output table is held in memory).
const MAX_INTVLS: usize = 100_000;
/// Relative tolerance of the internal integrator. The help says odesolve is not affected by TOL.
const INTEGRATION_TOL: f64 = 1e-10;
/// Accepted steps one odesolve may take before it is refused as probably stiff.
const MAX_STEPS: usize = 200_000;
/// Largest state magnitude an odesolve accepts before it is refused as diverging.
const DIVERGENCE_LIMIT: f64 = 1e12;

/// Scratch names of the solution table, from which each solution function is built.
const GRID: &str = "__odesolve_grid";
const VALUES: &str = "__odesolve_values";
const SPLINE: &str = "__odesolve_spline";
const LOW: &str = "__odesolve_low";
const HIGH: &str = "__odesolve_high";
/// Relative residual tolerance of the linear solve for the highest derivatives (see `eval`) and of
/// the affinity probe (see `check_affine`).
const LINEAR_TOL: f64 = 1e-8;

/// True for a solve-block member that is an `odesolve` call (through parentheses or `=`).
pub(super) fn is_odesolve(expr: &Expr) -> bool {
    match strip(expr) {
        Expr::Call { func, .. } => func.name.eq_ignore_ascii_case("odesolve"),
        _ => false,
    }
}

/// Solves the `odesolve` member `call` of a solve block against the block's constraints, in the
/// scratch worksheet `env`. `names` are the target names, one per unknown function in the order of
/// `vf`. Returns the solution functions, to be bound under those names.
pub(super) fn solve_odesolve(
    env: &Env,
    call: &Expr,
    names: &[String],
    constraints: &[Constraint],
    budget: &Budget,
) -> Result<Vec<(String, UserFunction)>, EvalError> {
    let Expr::Call { args, .. } = strip(call) else {
        return Err(unsupported("odesolve is a function call"));
    };
    let (vf, end, intvls) =
        match args.as_slice() {
            [vf, end] => (vf, end, None),
            [vf, end, n] => (vf, end, Some(n)),
            _ => return Err(unsupported(
                "odesolve takes the functions, the end point and an optional number of intervals",
            )),
        };
    if names.is_empty() {
        return Err(unsupported(
            "odesolve: assign the solution to a function name or a vector of function names",
        ));
    }
    let (unknowns, t) = unknown_functions(vf)?;
    if names.len() != unknowns.len() {
        return Err(unsupported(format!(
            "odesolve gives {} functions for {} names",
            unknowns.len(),
            names.len()
        )));
    }
    if constraints.is_empty() {
        return Err(unsupported("solve block has no constraints"));
    }
    let end = scalar_of(evaluate_budget(end, env, budget)?, "the end point")?;
    let count = match intvls {
        None => DEFAULT_INTVLS,
        Some(n) => interval_count(&evaluate_budget(n, env, budget)?)?,
    };

    // The ODEs contain the variable of integration; the other equations are initial conditions.
    let mut odes = Vec::new();
    let mut conditions = Vec::new();
    for c in constraints {
        if !c.is_equality() {
            return Err(unsupported(
                "an inequality cannot be a constraint of an odesolve block",
            ));
        }
        let (lhs, rhs) = c.sides();
        if uses_variable(lhs, &t) || uses_variable(rhs, &t) {
            odes.push(c);
        } else {
            conditions.push(c);
        }
    }

    // Rewrite each ODE with the state names `__odesolve_<f>_<k>` (k-th derivative of f), and find
    // the order of each unknown from the highest derivative its ODEs use.
    let m = unknowns.len();
    let mut orders = vec![0usize; m];
    let mut equations = Vec::with_capacity(odes.len());
    for c in &odes {
        let (lhs, rhs) = c.sides();
        let mut node = |n: &Expr| ode_node(n, &unknowns, &t, &mut orders);
        let lhs = rewrite(lhs, &mut node)?;
        let rhs = rewrite(rhs, &mut node)?;
        equations.push(Constraint::equation(lhs, rhs));
    }
    if equations.len() != m {
        return Err(unsupported(format!(
            "odesolve needs one ODE per unknown function: {} equations for {m} functions",
            equations.len()
        )));
    }
    for (i, order) in orders.iter().enumerate() {
        if *order == 0 {
            return Err(unsupported(format!(
                "`{}` has no derivative in the ODEs (algebraic constraints are not supported)",
                unknowns[i]
            )));
        }
    }

    // The initial conditions: one per derivative of order below each unknown's order, all at one
    // point. Conditions at two points are boundary value problems, which are refused.
    let mut start: Option<Scalar> = None;
    let mut slots: Vec<Vec<Option<Scalar>>> = orders.iter().map(|&n| vec![None; n]).collect();
    for c in &conditions {
        let (i, k, point, value) = condition(c, &unknowns, env, budget)?;
        if k >= orders[i] {
            return Err(unsupported(format!(
                "the condition on the derivative of order {k} of `{}` is not needed by its ODE",
                unknowns[i]
            )));
        }
        match start {
            None => start = Some(point),
            Some(p) if p.value.re != point.value.re => {
                return Err(unsupported(
                    "boundary value problems (conditions at two points) are not implemented yet; \
                     give all the initial conditions at one point",
                ))
            }
            Some(_) => {}
        }
        if slots[i][k].is_some() {
            return Err(unsupported(format!(
                "two conditions on the derivative of order {k} of `{}`",
                unknowns[i]
            )));
        }
        slots[i][k] = Some(value);
    }
    let start =
        start.ok_or_else(|| unsupported("odesolve needs initial conditions at one point"))?;
    for (i, row) in slots.iter().enumerate() {
        if let Some(k) = row.iter().position(Option::is_none) {
            return Err(unsupported(format!(
                "odesolve needs an initial condition on the derivative of order {k} of `{}`",
                unknowns[i]
            )));
        }
    }

    // Dimensions: the variable's dimension is the initial point's; each derivative divides by it,
    // and the highest one (not given by a condition) is the last of them.
    let t_dim = start.dim;
    if end.dim != t_dim {
        return Err(EvalError::DimensionMismatch {
            left: t_dim,
            right: end.dim,
        });
    }
    if end.value.re == start.value.re {
        return Err(unsupported(
            "odesolve: the end point must differ from the initial point",
        ));
    }
    let mut dims: Vec<Vec<Dimension>> = Vec::with_capacity(m);
    for row in &slots {
        let mut d: Vec<Dimension> = row.iter().flatten().map(|s| s.dim).collect();
        let last = *d.last().unwrap_or(&t_dim);
        d.push(last.checked_div(t_dim).map_err(EvalError::Units)?);
        dims.push(d);
    }

    let mut y0 = Vec::new();
    for row in &slots {
        y0.extend(row.iter().map(|s| s.map_or(0.0, |s| s.value.re)));
    }
    let system = System {
        eqs: equations,
        t: t.clone(),
        t_dim,
        unknowns: unknowns.clone(),
        orders: orders.clone(),
        dims: dims.clone(),
        work: RefCell::new(env.clone()),
        budget,
    };
    system.check_affine(start.value.re, &y0)?;
    let rows = ode::adaptive(
        &system,
        start.value.re,
        end.value.re,
        count,
        &y0,
        INTEGRATION_TOL,
        MAX_STEPS,
    )
    .map_err(|e| match e {
        EvalError::NoConvergence(_) => EvalError::Unsupported(format!(
            "odesolve: more than {MAX_STEPS} steps; the problem may be stiff, and odesolve has no \
             stiff method yet"
        )),
        other => other,
    })?;

    let grid: Vec<f64> = rows.iter().map(|r| r[0]).collect();
    let mut out = Vec::with_capacity(m);
    let mut offset = 0;
    for i in 0..m {
        let values: Vec<f64> = rows.iter().map(|r| r[1 + offset]).collect();
        offset += orders[i];
        let function = solution_function(
            &names[i], env, &t, t_dim, &grid, &values, dims[i][0], budget,
        )?;
        out.push((names[i].clone(), function));
    }
    Ok(out)
}

/// The function of `t` through the solution table: a cubic spline through the output points,
/// evaluated by `interp`. Its closure holds the table, so it does not depend on the worksheet.
#[allow(clippy::too_many_arguments)]
fn solution_function(
    name: &str,
    env: &Env,
    t: &str,
    t_dim: Dimension,
    grid: &[f64],
    values: &[f64],
    dim: Dimension,
    budget: &Budget,
) -> Result<UserFunction, EvalError> {
    // The spline needs increasing points: a backward solve is stored in increasing order.
    let (grid, values): (Vec<f64>, Vec<f64>) = if grid.first() > grid.last() {
        (
            grid.iter().rev().copied().collect(),
            values.iter().rev().copied().collect(),
        )
    } else {
        (grid.to_vec(), values.to_vec())
    };
    let mut scratch = env.clone();
    scratch.set_variable(GRID.to_string(), column(&grid, t_dim)?);
    scratch.set_variable(VALUES.to_string(), column(&values, dim)?);
    let spline = evaluate_budget(
        &Expr::call("cspline", vec![Expr::var(GRID), Expr::var(VALUES)]),
        &scratch,
        budget,
    )?;
    scratch.set_variable(SPLINE.to_string(), spline);
    // The help: the solution is limited to the solved range, so the spline is not extrapolated.
    // Outside `[LOW, HIGH]` the function refuses with `error`.
    let (lo, hi) = (
        grid.first().copied().unwrap_or(0.0),
        grid.last().copied().unwrap_or(0.0),
    );
    scratch.set_variable(LOW.to_string(), scalar(lo.min(hi), t_dim));
    scratch.set_variable(HIGH.to_string(), scalar(lo.max(hi), t_dim));
    let params = vec![t.to_string()];
    let inside = Expr::binary(
        BinaryOp::And,
        Expr::binary(BinaryOp::Ge, Expr::var(t), Expr::var(LOW)),
        Expr::binary(BinaryOp::Le, Expr::var(t), Expr::var(HIGH)),
    );
    let interpolated = Expr::call(
        "interp",
        vec![
            Expr::var(SPLINE),
            Expr::var(GRID),
            Expr::var(VALUES),
            Expr::var(t),
        ],
    );
    let refused = Expr::call(
        "error",
        vec![Expr::Str(
            "odesolve: the solution is only defined inside the solved range; it is not extrapolated"
                .to_string(),
        )],
    );
    let body = Expr::call("if", vec![inside, interpolated, refused]);
    let closure = capture(name, &params, &body, &scratch);
    Ok(UserFunction {
        params,
        body,
        closure,
    })
}

/// The first-order system of the ODEs, as the stepper sees it: the state `y` holds, for each
/// unknown `i` in turn, its derivatives of orders `0 .. orders[i]`.
struct System<'b> {
    eqs: Vec<Constraint>,
    t: String,
    t_dim: Dimension,
    unknowns: Vec<String>,
    orders: Vec<usize>,
    /// Per unknown: the dimension of each derivative, orders `0 ..= orders[i]`.
    dims: Vec<Vec<Dimension>>,
    /// The worksheet the ODEs are evaluated in, with the state written into it for each call.
    work: RefCell<Env>,
    budget: &'b Budget,
}

impl System<'_> {
    /// Writes the highest derivative of each unknown from `values`.
    fn set_highest(&self, env: &mut Env, values: &[f64]) {
        for (i, v) in values.iter().enumerate() {
            let k = self.orders[i];
            env.set_variable(
                hidden_name(&self.unknowns[i], k),
                scalar(*v, self.dims[i][k]),
            );
        }
    }

    /// The residuals of the ODEs in `env`, one per equation.
    fn residuals(&self, env: &Env) -> Result<Vec<f64>, EvalError> {
        self.eqs
            .iter()
            .map(|c| c.residual(env, self.budget))
            .collect()
    }

    /// Writes the time `x` and the state `y` (orders `0 .. orders[i]` of each unknown) into `env`.
    fn load_state(&self, env: &mut Env, x: f64, y: &[f64]) {
        env.set_variable(self.t.clone(), scalar(x, self.t_dim));
        let mut offset = 0;
        for i in 0..self.unknowns.len() {
            for k in 0..self.orders[i] {
                env.set_variable(
                    hidden_name(&self.unknowns[i], k),
                    scalar(y[offset + k], self.dims[i][k]),
                );
            }
            offset += self.orders[i];
        }
    }

    /// The help's rule that each ODE is linear in the highest derivatives, checked at the start
    /// state: for each unknown, the residual at `u = -1, 0, 1` (others 0) must lie on a line. A
    /// nonlinear ODE (`x''^2 == 4`) is refused before any step is taken.
    fn check_affine(&self, x: f64, y: &[f64]) -> Result<(), EvalError> {
        let mut env = self.work.borrow_mut();
        self.load_state(&mut env, x, y);
        let m = self.unknowns.len();
        let zero = vec![0.0; m];
        self.set_highest(&mut env, &zero);
        let r0 = self.residuals(&env)?;
        for j in 0..m {
            let mut u = zero.clone();
            u[j] = 1.0;
            self.set_highest(&mut env, &u);
            let rp = self.residuals(&env)?;
            u[j] = -1.0;
            self.set_highest(&mut env, &u);
            let rm = self.residuals(&env)?;
            for (e, r) in r0.iter().enumerate() {
                let curvature = rp[e] + rm[e] - 2.0 * r;
                let scale = 1.0 + r.abs() + rp[e].abs() + rm[e].abs();
                if !curvature.is_finite() || curvature.abs() > LINEAR_TOL * scale {
                    return Err(not_linear());
                }
            }
        }
        Ok(())
    }
}

/// The refusal of an ODE that is not linear in its highest derivatives.
fn not_linear() -> EvalError {
    unsupported("odesolve: the ODEs are not linear in their highest derivatives")
}

impl Stepper for System<'_> {
    fn eval(&self, x: f64, y: &[f64]) -> Result<Vec<f64>, EvalError> {
        self.budget.charge(DERIVATIVE_UNITS)?;
        // A stiff problem makes the explicit steps grow without bound. Past this size the finite
        // differences that give the first-order form lose all their digits, so the solve is refused.
        if y.iter()
            .any(|v| !v.is_finite() || v.abs() > DIVERGENCE_LIMIT)
        {
            return Err(unsupported(
                "odesolve: the solution grows without bound; the problem may be stiff, and odesolve \
                 has no stiff method yet",
            ));
        }
        let mut env = self.work.borrow_mut();
        self.load_state(&mut env, x, y);
        let m = self.unknowns.len();
        let mut starts = Vec::with_capacity(m);
        let mut offset = 0;
        for order in &self.orders {
            starts.push(offset);
            offset += order;
        }
        // Each ODE is affine in the highest derivatives u: residual(u) = r0 + J u.
        let zero = vec![0.0; m];
        self.set_highest(&mut env, &zero);
        let r0 = self.residuals(&env)?;
        let mut jac = vec![vec![0.0; m]; m];
        for j in 0..m {
            let mut u = zero.clone();
            u[j] = 1.0;
            self.set_highest(&mut env, &u);
            let r = self.residuals(&env)?;
            for (e, row) in jac.iter_mut().enumerate() {
                row[j] = r[e] - r0[e];
            }
        }
        // Residuals this large (the explicit steps running away) cancel in the differences of the
        // Jacobian, so the solve is refused as diverging rather than given a zero pivot.
        let diverged = r0
            .iter()
            .any(|v| !v.is_finite() || v.abs() > DIVERGENCE_LIMIT)
            || jac.iter().flatten().any(|v| !v.is_finite());
        if diverged {
            return Err(unsupported(
                "odesolve: the solution is not finite; the problem may be stiff, and odesolve has \
                 no stiff method yet",
            ));
        }
        let rhs: Vec<f64> = r0.iter().map(|r| -r).collect();
        let u = solve_dense(jac, rhs)?;
        // The linear model must hold at the solved highest derivatives. A nonlinear ODE solved as
        // if linear leaves a residual here, and the solve is refused rather than given wrong values.
        self.set_highest(&mut env, &u);
        let solved = self.residuals(&env)?;
        if solved
            .iter()
            .zip(&r0)
            .any(|(r, r0)| !r.is_finite() || r.abs() > LINEAR_TOL * (1.0 + r0.abs()))
        {
            return Err(not_linear());
        }
        let mut dy = vec![0.0; y.len()];
        for i in 0..m {
            let base = starts[i];
            let n = self.orders[i];
            for k in 0..n - 1 {
                dy[base + k] = y[base + k + 1];
            }
            dy[base + n - 1] = u[i];
        }
        Ok(dy)
    }
}

/// Solves `a x = b` by Gaussian elimination with partial pivoting. A singular system means the
/// ODEs are not linear in their highest derivatives (or are degenerate), and is refused.
fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Result<Vec<f64>, EvalError> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|&p, &q| a[p][col].abs().total_cmp(&a[q][col].abs()))
            .unwrap_or(col);
        if a[pivot][col] == 0.0 || !a[pivot][col].is_finite() {
            return Err(unsupported(
                "odesolve: the ODEs are not linear in their highest derivatives",
            ));
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
    Ok(x)
}

/// The unknown functions of `vf` (`x(t)`, or a column `[x(t); y(t)]`): their names and the one
/// variable of integration they take.
fn unknown_functions(vf: &Expr) -> Result<(Vec<String>, String), EvalError> {
    let items: Vec<&Expr> = match strip(vf) {
        Expr::Matrix {
            cols: 1, elements, ..
        } => elements.iter().collect(),
        other => vec![other],
    };
    let mut names: Vec<String> = Vec::with_capacity(items.len());
    let mut var: Option<String> = None;
    for item in items {
        let Expr::Call { func, args } = strip(item) else {
            return Err(unsupported(
                "odesolve takes the unknown functions as they appear in the block, such as x(t)",
            ));
        };
        let (base, order) = base_and_order(func)?;
        if order != 0 {
            return Err(unsupported(
                "name the functions of odesolve without primes, such as x(t)",
            ));
        }
        let [Expr::Id(arg)] = args.as_slice() else {
            return Err(unsupported(
                "each function of odesolve takes one variable, such as x(t)",
            ));
        };
        let key = var_key(arg)?;
        match &var {
            Some(v) if *v != key => {
                return Err(unsupported(
                    "the functions of odesolve must all take the same variable",
                ))
            }
            _ => var = Some(key),
        }
        if names.contains(&base) {
            return Err(unsupported(format!("`{base}` is listed twice")));
        }
        names.push(base);
    }
    let var = var.ok_or_else(|| unsupported("odesolve takes at least one function"))?;
    Ok((names, var))
}

/// One initial condition `f^(k)(a) = value` as (unknown index, k, point a, value).
fn condition(
    c: &Constraint,
    unknowns: &[String],
    env: &Env,
    budget: &Budget,
) -> Result<(usize, usize, Scalar, Scalar), EvalError> {
    let (lhs, rhs) = c.sides();
    let (call, value) = if matches!(strip(lhs), Expr::Call { .. }) {
        (lhs, rhs)
    } else {
        (rhs, lhs)
    };
    let Expr::Call { func, args } = strip(call) else {
        return Err(unsupported(
            "an initial condition of odesolve is f(a) = value or f'(a) = value",
        ));
    };
    let (base, k) = base_and_order(func)?;
    let i = unknowns
        .iter()
        .position(|u| *u == base)
        .ok_or_else(|| unsupported(format!("`{base}` is not a function of this odesolve")))?;
    let [arg] = args.as_slice() else {
        return Err(unsupported(
            "an initial condition of odesolve takes one point",
        ));
    };
    let point = scalar_of(evaluate_budget(arg, env, budget)?, "the initial point")?;
    let value = scalar_of(evaluate_budget(value, env, budget)?, "the initial value")?;
    Ok((i, k, point, value))
}

/// Rebuilds `expr` with `f` tried first at every node: a `Some` answer replaces the node, and
/// `None` descends into it. Forms that cannot appear in an ODE are refused.
pub(super) fn rewrite(
    expr: &Expr,
    f: &mut dyn FnMut(&Expr) -> Option<Result<Expr, EvalError>>,
) -> Result<Expr, EvalError> {
    if let Some(answer) = f(expr) {
        return answer;
    }
    Ok(match expr {
        Expr::Number(_) | Expr::Id(_) | Expr::Str(_) | Expr::Placeholder => expr.clone(),
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: Box::new(rewrite(operand, f)?),
        },
        Expr::Binary { op, lhs, rhs } => {
            let lhs = rewrite(lhs, f)?;
            let rhs = rewrite(rhs, f)?;
            Expr::Binary {
                op: *op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }
        }
        Expr::Call { func, args } => Expr::Call {
            func: func.clone(),
            args: rewrite_all(args, f)?,
        },
        Expr::Parens(inner) => Expr::Parens(Box::new(rewrite(inner, f)?)),
        Expr::Scale { value, unit } => {
            let value = rewrite(value, f)?;
            let unit = rewrite(unit, f)?;
            Expr::Scale {
                value: Box::new(value),
                unit: Box::new(unit),
            }
        }
        Expr::Matrix {
            rows,
            cols,
            elements,
        } => Expr::Matrix {
            rows: *rows,
            cols: *cols,
            elements: rewrite_all(elements, f)?,
        },
        Expr::Index { base, indices } => Expr::Index {
            base: Box::new(rewrite(base, f)?),
            indices: rewrite_all(indices, f)?,
        },
        Expr::Eval {
            expr: inner,
            unit_override,
        } => Expr::Eval {
            expr: Box::new(rewrite(inner, f)?),
            unit_override: match unit_override {
                Some(u) => Some(Box::new(rewrite(u, f)?)),
                None => None,
            },
        },
        Expr::Derivative {
            var,
            order,
            body,
            partial,
        } => Expr::Derivative {
            var: var.clone(),
            order: order.clone(),
            body: Box::new(rewrite(body, f)?),
            partial: *partial,
        },
        _ => {
            return Err(unsupported(
                "this form cannot appear in an odesolve equation",
            ))
        }
    })
}

pub(super) fn rewrite_all(
    exprs: &[Expr],
    f: &mut dyn FnMut(&Expr) -> Option<Result<Expr, EvalError>>,
) -> Result<Vec<Expr>, EvalError> {
    exprs.iter().map(|e| rewrite(e, &mut *f)).collect()
}

/// The rewrite rule for one node of an ODE: a derivative of an unknown becomes its state name, and
/// the highest order seen is recorded. Anything else an unknown is used as is an error, as is a
/// bare name of an unknown. `None` for other nodes.
fn ode_node(
    node: &Expr,
    unknowns: &[String],
    var: &str,
    orders: &mut [usize],
) -> Option<Result<Expr, EvalError>> {
    let index_of = |base: &str| unknowns.iter().position(|u| u.as_str() == base);
    let wrong_use = |base: &str| {
        Some(Err(unsupported(format!(
            "`{base}` in an odesolve equation must be called with the variable {var}, as {base}(...)"
        ))))
    };
    match node {
        Expr::Call { func, args } => {
            let (base, k) = match base_and_order(func) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let i = index_of(&base)?;
            match args.as_slice() {
                [Expr::Id(arg)] if var_key(arg).ok().as_deref() == Some(var) => {
                    Some(Ok(state_node(i, k, unknowns, orders)))
                }
                _ => wrong_use(&base),
            }
        }
        Expr::Derivative {
            var: dvar,
            order,
            body,
            ..
        } => {
            let Expr::Call { func, args } = &**body else {
                return None;
            };
            let (base, _) = match base_and_order(func) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let i = index_of(&base)?;
            let k = match order.as_deref() {
                None => 1,
                Some(Expr::Number(n)) => match number_value(n) {
                    Ok(v) if v >= 1.0 && v.fract() == 0.0 => v as usize,
                    _ => {
                        return Some(Err(unsupported(
                            "a derivative order must be a whole number",
                        )))
                    }
                },
                Some(_) => {
                    return Some(Err(unsupported(
                        "a derivative order must be a whole number",
                    )))
                }
            };
            let [Expr::Id(arg)] = args.as_slice() else {
                return wrong_use(&base);
            };
            if var_key(dvar).ok().as_deref() != Some(var)
                || var_key(arg).ok().as_deref() != Some(var)
            {
                return Some(Err(unsupported(format!(
                    "the derivatives of `{base}` must be taken with respect to {var}"
                ))));
            }
            Some(Ok(state_node(i, k, unknowns, orders)))
        }
        Expr::Id(id) => match var_key(id) {
            Ok(key) if index_of(&key).is_some() => wrong_use(&key),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        },
        _ => None,
    }
}

/// The state name of the `k`-th derivative of unknown `i`, recording the order of the unknown.
fn state_node(i: usize, k: usize, unknowns: &[String], orders: &mut [usize]) -> Expr {
    orders[i] = orders[i].max(k);
    Expr::var(&hidden_name(&unknowns[i], k))
}

/// The scratch name of the `k`-th derivative of `base`.
fn hidden_name(base: &str, k: usize) -> String {
    format!("__odesolve_{base}_{k}")
}

/// The base name and the number of primes of a function name (`x''` is `x` with 2).
fn base_and_order(func: &Id) -> Result<(String, usize), EvalError> {
    let key = var_key(func)?;
    let base = key.trim_end_matches('\'');
    if base.is_empty() {
        return Err(unsupported(format!("`{key}` is not a function name")));
    }
    Ok((base.to_string(), key.len() - base.len()))
}

/// The number of output intervals: a whole, dimensionless number from 1 to [`MAX_INTVLS`].
fn interval_count(value: &Value) -> Result<usize, EvalError> {
    match value {
        Value::Scalar(s)
            if s.dim.is_dimensionless()
                && s.value.im == 0.0
                && s.value.re.fract() == 0.0
                && (1.0..=MAX_INTVLS as f64).contains(&s.value.re) =>
        {
            Ok(s.value.re as usize)
        }
        _ => Err(unsupported(format!(
            "the number of intervals must be a whole number from 1 to {MAX_INTVLS}"
        ))),
    }
}

/// A real, finite scalar value of an odesolve argument.
fn scalar_of(value: Value, what: &str) -> Result<Scalar, EvalError> {
    match value {
        Value::Scalar(s) if s.value.im == 0.0 && s.value.re.is_finite() => Ok(s),
        Value::Scalar(_) => Err(unsupported(format!("{what} must be a real number"))),
        _ => Err(unsupported(format!("{what} must be a number"))),
    }
}

fn scalar(x: f64, dim: Dimension) -> Value {
    Value::Scalar(Scalar::real(x, dim))
}

fn column(values: &[f64], dim: Dimension) -> Result<Value, EvalError> {
    let data = values.iter().map(|v| Scalar::real(*v, dim)).collect();
    Ok(Value::Matrix(Matrix::new(values.len(), 1, data)?))
}

/// An expression without its parentheses and `=`.
fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Parens(inner) | Expr::Eval { expr: inner, .. } => strip(inner),
        other => other,
    }
}

fn unsupported(message: impl Into<String>) -> EvalError {
    EvalError::Unsupported(message.into())
}

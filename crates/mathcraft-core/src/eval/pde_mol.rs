//! The method of lines for one-dimensional PDE systems (help: "Partial Differential Equation
//! Solver", `numol`), shared with `pdesolve` in solve blocks.
//!
//! `numol(x_endpts, xpts, t_endpts, tpts, num_pde, num_pae, pde_func, pinit, bc_func)` solves
//! `u_t = F(x, t, u, u_x, u_xx)` for `num_pde` unknown functions on `[a, b]` with `xpts` points.
//! The spatial derivatives are second-order central differences; the time integration is the
//! adaptive Dormand-Prince solver of `ode` at [`INTEGRATION_TOL`]. The result is `xpts x (tpts *
//! num_pde)`: the solution of each function is a block of `tpts` columns (times `t_endpts`, first
//! column at the start), the blocks appended to the right.
//!
//! Boundary conditions follow the help's rows `[bc_left(t), bc_right(t), "D" or "N"]`, one row per
//! function. A function whose equation has `u_xx` needs both sides, Dirichlet (the value is held
//! at the edge node) or Neumann (the edge derivative is set, with a ghost node for `u_xx`). A
//! function with no spatial derivative needs no condition (its row may be `"NA" "NA" "D"`).
//!
//! Refused rather than guessed: algebraic equations (`num_pae > 0`), a function whose equation uses
//! only a first spatial derivative (the help gives no one-sided rule for its free end), and a
//! function whose row does not match its derivative order. A derivative is detected by probing
//! `F` at several points and at both ends of the time range: `u_xx` (or `u_x`) counts as used when
//! changing its value changes `F` at some probe. A boundary row other than `"NA" "NA" "D"` makes
//! its function second order regardless.
//!
//! Every evaluation of `F` is charged to the budget ([`DERIVATIVE_UNITS`] per grid point).

use super::error::EvalError;
use super::expr_eval::{call_user_uncounted, Ctx};
use super::ode::{self, Stepper, DERIVATIVE_UNITS};
use super::special_forms::{
    column, eval_with, matrix, named_function, real_of, real_value, reals_of, value_of,
};
use super::value::{Value, MAX_ELEMENTS};
use crate::expr::Expr;

/// Relative tolerance of the time integration (the help gives none for numol).
pub(super) const INTEGRATION_TOL: f64 = 1e-6;
/// Accepted time steps one solve may take.
pub(super) const MAX_STEPS: usize = 200_000;
/// Smallest and largest number of grid points per side (the help: "greater than 4").
pub(super) const MIN_POINTS: usize = 5;
pub(super) const MAX_SPACE_POINTS: usize = 2_000;
pub(super) const MAX_TIME_POINTS: usize = 10_000;

/// One boundary condition at one side of an interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Bc {
    /// The value of the function at the edge.
    Dirichlet(f64),
    /// The derivative of the function at the edge.
    Neumann(f64),
}

/// The right-hand side `F(x, t, u, u_x, u_xx)` at one point: one value per function.
pub(super) type RhsFn<'a> =
    dyn Fn(f64, f64, &[f64], &[f64], &[f64]) -> Result<Vec<f64>, EvalError> + 'a;

/// The problem, with its function evaluations as closures so that `numol` and `pdesolve` share
/// the integrator. Each closure is charged by its caller.
pub(super) struct Problem<'a> {
    pub(super) a: f64,
    pub(super) b: f64,
    pub(super) xpts: usize,
    pub(super) t0: f64,
    pub(super) t1: f64,
    pub(super) tpts: usize,
    /// Spatial derivative order of each function: 0 (no condition) or 2 (both edges).
    pub(super) orders: Vec<usize>,
    /// `F(x, t, u, u_x, u_xx)`: the right-hand side of each function at one point.
    pub(super) rhs: &'a RhsFn<'a>,
    /// The conditions at time `t`: for each function, its left and right edge (None for order 0).
    pub(super) bc: &'a dyn Fn(f64) -> Result<Vec<Option<[Bc; 2]>>, EvalError>,
    /// The initial values of the functions at one point.
    pub(super) init: &'a dyn Fn(f64) -> Result<Vec<f64>, EvalError>,
    /// Work of one evaluation of `rhs` at a grid point.
    pub(super) charge: &'a dyn Fn(u64) -> Result<(), EvalError>,
}

/// The solution: for each function `k` and output time `j`, the values over the grid. Index
/// `k * tpts + j` holds the column of function `k` at time `j`.
pub(super) type Solution = Vec<Vec<f64>>;

impl Problem<'_> {
    fn spacing(&self) -> f64 {
        (self.b - self.a) / (self.xpts - 1) as f64
    }

    fn x(&self, i: usize) -> f64 {
        self.a + i as f64 * self.spacing()
    }

    /// The full grid values `u[k][i]` at time `t` for the state `y` (the non-fixed values, in
    /// the order of [`Stepper`] below), the conditions at `t`, and which nodes are fixed.
    fn grid(&self, y: Option<&[f64]>, bcs: &[Option<[Bc; 2]>]) -> (Vec<Vec<f64>>, Vec<Vec<bool>>) {
        let n = self.xpts;
        let m = self.orders.len();
        let mut u = vec![vec![0.0; n]; m];
        let mut fixed = vec![vec![false; n]; m];
        let mut next = 0usize;
        for k in 0..m {
            for i in 0..n {
                let dirichlet = match bcs[k] {
                    Some([Bc::Dirichlet(l), _]) if i == 0 => Some(l),
                    Some([_, Bc::Dirichlet(r)]) if i == n - 1 => Some(r),
                    _ => None,
                };
                match dirichlet {
                    Some(v) => {
                        u[k][i] = v;
                        fixed[k][i] = true;
                    }
                    None => {
                        if let Some(y) = y {
                            u[k][i] = y[next];
                        }
                        next += 1;
                    }
                }
            }
        }
        (u, fixed)
    }
}

/// The right-hand side of the semi-discrete system, as the stepper sees it.
struct Semi<'p, 'a> {
    p: &'p Problem<'a>,
}

impl Semi<'_, '_> {
    /// First and second derivatives at every node from the grid values, with the conditions of
    /// each function applied at the edges.
    fn derivatives(
        &self,
        u: &[Vec<f64>],
        bcs: &[Option<[Bc; 2]>],
    ) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let p = self.p;
        let n = p.xpts;
        let h = p.spacing();
        let mut ux = vec![vec![0.0; n]; u.len()];
        let mut uxx = vec![vec![0.0; n]; u.len()];
        for (k, row) in u.iter().enumerate() {
            if p.orders[k] == 0 {
                continue;
            }
            let (left, right) = match bcs[k] {
                Some([l, r]) => (l, r),
                None => continue,
            };
            for i in 1..n - 1 {
                ux[k][i] = (row[i + 1] - row[i - 1]) / (2.0 * h);
                uxx[k][i] = (row[i + 1] - 2.0 * row[i] + row[i - 1]) / (h * h);
            }
            // Edge nodes: a Neumann value is the derivative itself, and the second derivative
            // uses the ghost node that the value implies. A Dirichlet edge is fixed, so its
            // derivatives are never used.
            if let Bc::Neumann(g) = left {
                ux[k][0] = g;
                uxx[k][0] = (2.0 * row[1] - 2.0 * row[0] - 2.0 * h * g) / (h * h);
            }
            if let Bc::Neumann(g) = right {
                ux[k][n - 1] = g;
                uxx[k][n - 1] = (2.0 * row[n - 2] - 2.0 * row[n - 1] + 2.0 * h * g) / (h * h);
            }
        }
        (ux, uxx)
    }
}

/// The stepper: the derivatives of the unknown (non-fixed) nodes at time `t`.
struct Stepping<'p, 'a> {
    p: &'p Problem<'a>,
}

impl Stepper for Stepping<'_, '_> {
    fn eval(&self, t: f64, y: &[f64]) -> Result<Vec<f64>, EvalError> {
        let p = self.p;
        let bcs = (p.bc)(t)?;
        let (u, fixed) = p.grid(Some(y), &bcs);
        let (ux, uxx) = Semi { p }.derivatives(&u, &bcs);
        let m = p.orders.len();
        let n = p.xpts;
        let mut dy = vec![vec![0.0; n]; m];
        for i in 0..n {
            if (0..m).all(|k| fixed[k][i]) {
                continue;
            }
            (p.charge)(DERIVATIVE_UNITS)?;
            let uc: Vec<f64> = (0..m).map(|k| u[k][i]).collect();
            let uxc: Vec<f64> = (0..m).map(|k| ux[k][i]).collect();
            let uxxc: Vec<f64> = (0..m).map(|k| uxx[k][i]).collect();
            let f = (p.rhs)(p.x(i), t, &uc, &uxc, &uxxc)?;
            if f.len() != m {
                return Err(EvalError::Unsupported(format!(
                    "the PDE returns {} values for {m} functions",
                    f.len()
                )));
            }
            for k in 0..m {
                dy[k][i] = f[k];
            }
        }
        // The state is ordered function by function, then by node (as `Problem::grid` reads it).
        let mut out = Vec::with_capacity(y.len());
        for k in 0..m {
            for i in 0..n {
                if !fixed[k][i] {
                    out.push(dy[k][i]);
                }
            }
        }
        Ok(out)
    }
}

/// Solves the problem. The returned solution has `orders.len() * tpts` columns (see [`Solution`]).
pub(super) fn solve(p: &Problem<'_>) -> Result<Solution, EvalError> {
    let m = p.orders.len();
    let bcs0 = (p.bc)(p.t0)?;
    let n = p.xpts;
    // Initial values at every node; fixed nodes take their edge value.
    let init: Vec<Vec<f64>> = (0..n).map(|i| (p.init)(p.x(i))).collect::<Result<_, _>>()?;
    let mut y0 = Vec::new();
    let (_, fixed) = p.grid(None, &bcs0);
    for (k, fixed_k) in fixed.iter().enumerate() {
        for (i, is_fixed) in fixed_k.iter().enumerate() {
            if *is_fixed {
                continue;
            }
            let v = init[i].get(k).copied().ok_or_else(|| {
                EvalError::Unsupported(format!(
                    "pinit returns fewer than {m} values for the functions"
                ))
            })?;
            y0.push(v);
        }
    }
    let stepping = Stepping { p };
    let intvls = p.tpts - 1;
    let rows = ode::adaptive(
        &stepping,
        p.t0,
        p.t1,
        intvls,
        &y0,
        INTEGRATION_TOL,
        MAX_STEPS,
    )
    .map_err(|e| match e {
        EvalError::NoConvergence(_) => EvalError::NoConvergence("numol".to_string()),
        other => other,
    })?;
    let mut sol: Solution = vec![vec![0.0; n]; m * p.tpts];
    for (j, row) in rows.iter().enumerate() {
        let t = row[0];
        let bcs = (p.bc)(t)?;
        let (u, _) = p.grid(Some(&row[1..]), &bcs);
        for k in 0..m {
            sol[k * p.tpts + j] = u[k].clone();
        }
    }
    Ok(sol)
}

/// `numol(x_endpts, xpts, t_endpts, tpts, num_pde, num_pae, pde_func, pinit, bc_func)`.
pub(super) fn numol_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [x_ends, xpts, t_ends, tpts, num_pde, num_pae, pde, pinit, bc] = args else {
        return Err(EvalError::Unsupported(
            "numol takes nine arguments".to_string(),
        ));
    };
    let (a, b) = endpoints(&value_of(x_ends, ctx)?, "x_endpts")?;
    let (t0, t1) = endpoints(&value_of(t_ends, ctx)?, "t_endpts")?;
    let xpts = point_count(&value_of(xpts, ctx)?, "xpts", MIN_POINTS, MAX_SPACE_POINTS)?;
    let tpts = point_count(&value_of(tpts, ctx)?, "tpts", MIN_POINTS, MAX_TIME_POINTS)?;
    let m = point_count(&value_of(num_pde, ctx)?, "num_pde", 1, usize::MAX)?;
    let pae = real_of(&value_of(num_pae, ctx)?, "num_pae")?;
    if pae != 0.0 {
        return Err(EvalError::Unsupported(
            "numol: algebraic equations (num_pae > 0) are not supported yet".to_string(),
        ));
    }
    if m * tpts > MAX_ELEMENTS || xpts * m * tpts > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(xpts * m * tpts));
    }
    let Some((pde_name, pde_fn)) = named_function(pde, ctx)? else {
        return Err(EvalError::Unsupported(
            "numol: pde_func must be the name of a function F(x, t, u, u_x, u_xx)".to_string(),
        ));
    };
    if pde_fn.params.len() != 5 {
        return Err(EvalError::ArgCount {
            name: pde_name,
            expected: "5".to_string(),
            found: pde_fn.params.len(),
        });
    }
    let Some((init_name, init_fn)) = named_function(pinit, ctx)? else {
        return Err(EvalError::Unsupported(
            "numol: pinit must be the name of a function of x".to_string(),
        ));
    };
    let Some((bc_name, bc_fn)) = named_function(bc, ctx)? else {
        return Err(EvalError::Unsupported(
            "numol: bc_func must be the name of a function of t".to_string(),
        ));
    };
    if bc_fn.params.len() != 1 || init_fn.params.len() != 1 {
        return Err(EvalError::Unsupported(
            "numol: pinit and bc_func each take one variable".to_string(),
        ));
    }
    check_bc_shape(bc_fn, &bc_name, m)?;

    // The unknowns' element names inside F's body: `u_0`, `u_x_0`, `u_xx_0` (help: u_xx_0).
    let elem_names: Vec<String> = pde_fn.params[2..5].to_vec();
    let rhs = |x: f64, t: f64, u: &[f64], ux: &[f64], uxx: &[f64]| -> Result<Vec<f64>, EvalError> {
        let mut extra = Vec::new();
        for (param, vals) in elem_names.iter().zip([u, ux, uxx]) {
            for (k, v) in vals.iter().enumerate() {
                extra.push((format!("{param}_{k}"), real_value(*v)));
            }
        }
        let out = call_user_uncounted(
            &pde_name,
            pde_fn,
            vec![
                real_value(x),
                real_value(t),
                column(u)?,
                column(ux)?,
                column(uxx)?,
            ],
            &extra,
            ctx,
        )?;
        reals_of(&out, "the PDE F")
    };
    let forced: Vec<bool> = plain_bc_rows(bc_fn, m).iter().map(|plain| !plain).collect();
    let orders = probe_orders(&rhs, m, (a, b), (t0, t1), &forced)?;
    let bc = |t: f64| bc_values(bc_fn, bc_name.as_str(), t, &orders, ctx);
    let init = |x: f64| -> Result<Vec<f64>, EvalError> {
        let out = call_user_uncounted(&init_name, init_fn, vec![real_value(x)], &[], ctx)?;
        reals_of(&out, "pinit")
    };
    let charge = |units: u64| ctx.budget.charge(units);
    let problem = Problem {
        a,
        b,
        xpts,
        t0,
        t1,
        tpts,
        orders: orders.clone(),
        rhs: &rhs,
        bc: &bc,
        init: &init,
        charge: &charge,
    };
    let sol = solve(&problem)?;
    let cols = m * tpts;
    let mut data = vec![0.0; xpts * cols];
    for (c, column) in sol.iter().enumerate() {
        for (i, v) in column.iter().enumerate() {
            data[i * cols + c] = *v;
        }
    }
    matrix(xpts, cols, &data)
}

/// Fractions of the space range at which `F` is probed. Several points, so that a coefficient that
/// vanishes at one of them (such as `x` at the midpoint) does not hide a derivative.
const PROBE_FRACTIONS: [f64; 4] = [0.13, 0.37, 0.61, 0.89];

/// The derivative order each function's equation uses. `F` is probed at several points of `[a, b]`
/// and at both `t0` and `t1`, with different values of `u`, `u_x` and `u_xx` at each probe. Order 2
/// when `u_xx` changes `F` at some probe, else 1 (refused) when `u_x` does, else 0. A function whose
/// `forced` flag is set (its boundary row is not `"NA" "NA" "D"`) is order 2 whatever `F` does.
fn probe_orders(
    rhs: &RhsFn<'_>,
    m: usize,
    (a, b): (f64, f64),
    (t0, t1): (f64, f64),
    forced: &[bool],
) -> Result<Vec<usize>, EvalError> {
    let mut uxx_used = vec![false; m];
    let mut ux_used = vec![false; m];
    let mut probe = 0usize;
    for frac in PROBE_FRACTIONS {
        let x = a + (b - a) * frac;
        for t in [t0, t1] {
            probe += 1;
            let s = probe as f64;
            let u: Vec<f64> = (0..m).map(|k| 0.37 + 0.11 * k as f64 + 0.05 * s).collect();
            let ux: Vec<f64> = (0..m).map(|k| 0.23 + 0.07 * k as f64 - 0.03 * s).collect();
            let uxx: Vec<f64> = (0..m).map(|k| 0.41 + 0.13 * k as f64 - 0.02 * s).collect();
            let base = rhs(x, t, &u, &ux, &uxx)?;
            let changed = |f: &[f64]| {
                base.iter()
                    .zip(f)
                    .any(|(a, b)| (a - b).abs() > 1e-12 * (1.0 + a.abs()))
            };
            for k in 0..m {
                if uxx_used[k] {
                    continue;
                }
                let mut bumped = uxx.clone();
                bumped[k] += 1.0;
                if changed(&rhs(x, t, &u, &ux, &bumped)?) {
                    uxx_used[k] = true;
                    continue;
                }
                let mut bumped = ux.clone();
                bumped[k] += 1.0;
                if changed(&rhs(x, t, &u, &bumped, &uxx)?) {
                    ux_used[k] = true;
                }
            }
        }
    }
    if let Some(k) = (0..m).find(|&k| ux_used[k] && !uxx_used[k]) {
        return Err(EvalError::Unsupported(format!(
            "numol: the equation of function {} has only a first spatial derivative, which is not supported yet",
            k + 1
        )));
    }
    Ok((0..m)
        .map(|k| if forced[k] || uxx_used[k] { 2 } else { 0 })
        .collect())
}

/// For each function, whether its boundary row is written `"NA" "NA" "D"` (no spatial condition).
/// Any other row, including one with numbers, marks the function as second order. Needs the shape
/// checked by [`check_bc_shape`].
fn plain_bc_rows(f: &super::env::UserFunction, m: usize) -> Vec<bool> {
    let Expr::Matrix { cols, elements, .. } = strip(&f.body) else {
        return vec![false; m];
    };
    (0..m)
        .map(|k| {
            let row = &elements[k * cols..(k + 1) * cols];
            let is = |e: &Expr, text: &str| matches!(strip(e), Expr::Str(s) if s == text);
            is(&row[0], "NA") && is(&row[1], "NA") && is(&row[2], "D")
        })
        .collect()
}

/// The shape of `bc_func`'s matrix literal: one row per function and three entries. Its entries are
/// read one by one in [`bc_values`], since a boundary type is a string, which a matrix value
/// cannot hold.
fn check_bc_shape(f: &super::env::UserFunction, name: &str, m: usize) -> Result<(), EvalError> {
    match strip(&f.body) {
        Expr::Matrix { rows, cols, .. } if *rows == m && *cols == 3 => Ok(()),
        Expr::Matrix { rows, cols, .. } => Err(EvalError::Unsupported(format!(
            "{name} must be a {m} x 3 matrix of boundary rows, found {rows} x {cols}"
        ))),
        _ => Err(EvalError::Unsupported(format!(
            "{name} must be written as a matrix literal of its boundary rows"
        ))),
    }
}

/// The boundary conditions of every function at time `t`, from the matrix literal of `bc_func`.
/// A row is `[left, right, type]`: values, or `"NA"` for a side with no condition, and the type
/// `"D"` or `"N"` for both sides. Order-0 functions need no condition and get `None`.
fn bc_values(
    f: &super::env::UserFunction,
    name: &str,
    t: f64,
    orders: &[usize],
    ctx: Ctx<'_>,
) -> Result<Vec<Option<[Bc; 2]>>, EvalError> {
    let Expr::Matrix { cols, elements, .. } = strip(&f.body) else {
        return Err(EvalError::Unsupported(format!(
            "{name} must be written as a matrix literal of its boundary rows"
        )));
    };
    let mut out = Vec::with_capacity(orders.len());
    for (k, order) in orders.iter().enumerate() {
        let row = &elements[k * cols..(k + 1) * cols];
        let elem = |e: &Expr| -> Result<Entry, EvalError> {
            match strip(e) {
                Expr::Str(s) => Ok(Entry::Text(s.clone())),
                other => Ok(Entry::Number(real_of(
                    &eval_with(other, ctx, &[(f.params[0].clone(), real_value(t))])?,
                    "a boundary value",
                )?)),
            }
        };
        if *order == 0 {
            out.push(None);
            continue;
        }
        let left = elem(&row[0])?;
        let right = elem(&row[1])?;
        let kind = elem(&row[2])?;
        let (Entry::Number(l), Entry::Number(r)) = (left, right) else {
            return Err(EvalError::Unsupported(format!(
                "{name}: a function with second derivatives needs a value at both edges"
            )));
        };
        let bc = match kind {
            Entry::Text(ref s) if s == "D" => [Bc::Dirichlet(l), Bc::Dirichlet(r)],
            Entry::Text(ref s) if s == "N" => [Bc::Neumann(l), Bc::Neumann(r)],
            _ => {
                return Err(EvalError::Unsupported(format!(
                    "{name}: the type of a boundary row is \"D\" or \"N\""
                )))
            }
        };
        out.push(Some(bc));
    }
    Ok(out)
}

/// A boundary-row entry: a number, or a string such as `"D"`.
enum Entry {
    Number(f64),
    Text(String),
}

fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Parens(inner) => strip(inner),
        other => other,
    }
}

/// Two real end points `[a; b]` with `a < b`.
fn endpoints(v: &Value, what: &str) -> Result<(f64, f64), EvalError> {
    let xs = reals_of(v, what)?;
    match xs.as_slice() {
        [a, b] if a < b => Ok((*a, *b)),
        _ => Err(EvalError::Unsupported(format!(
            "{what} must be two real numbers, the lower end first"
        ))),
    }
}

/// A whole number from `min` to `max`.
fn point_count(v: &Value, what: &str, min: usize, max: usize) -> Result<usize, EvalError> {
    let n = real_of(v, what)?;
    if n.fract() != 0.0 || n < min as f64 || n > max as f64 {
        return Err(EvalError::Unsupported(format!(
            "{what} must be an integer from {min} to {max}"
        )));
    }
    Ok(n as usize)
}

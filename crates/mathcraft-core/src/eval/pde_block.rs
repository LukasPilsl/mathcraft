//! `pdesolve(u, x, xrange, t, trange, [xpts], [tpts])` inside a solve block (help: "Partial
//! Differential Equations in Solve Blocks").
//!
//! The block gives the PDEs as equations `u_t(x,t) = F` (the time derivative on the left; subscript
//! names such as `vz(x,z)`, or `D_t` calls and `d/dt` operators), the boundary conditions as
//! `u(a,t) = f(t)` (Dirichlet) or `ux(a,t) = g(t)` (Neumann) at an end `a` of `xrange`, and the
//! initial conditions as `u(x,t0) = h(x)`. `T := pdesolve(T, x, xrange, t, trange)` binds each
//! solution as a function `T(x, t)`.
//!
//! How it works: the equations are rewritten with the state names `__pdesolve_<i>_<k>` (the k-th
//! x-derivative of unknown i) and evaluated pointwise as the method-of-lines right-hand side of
//! [`pde_mol`]. The grid has `xpts` points (default [`DEFAULT_POINTS`]) and `tpts` output times.
//! The solution is held on that grid and interpolated bilinearly by `pdesolve_interp`; a point
//! outside the solved range is refused, not extrapolated.
//!
//! Refused with a message: units in a range or a condition, inequalities, algebraic constraints,
//! a unknown whose equation uses only a first spatial derivative, a missing or duplicated boundary
//! or initial condition, a boundary condition at a point that is not an end of `xrange`, and a
//! condition on an unknown whose equation needs none. The default point count is an assumption:
//! the help does not state it (see `documentation/MISSING_FUNCTIONS.md`).

use std::cell::RefCell;

use super::closure::{capture, uses_variable};
use super::env::{Env, UserFunction};
use super::error::EvalError;
use super::expr_eval::{evaluate_budget, number_value, var_key};
use super::ode_block::rewrite;
use super::pde_mol::{self, Bc, Problem, MAX_SPACE_POINTS, MAX_TIME_POINTS, MIN_POINTS};
use super::solve_constraints::Constraint;
use super::special_forms::{column, matrix, real_of, real_value, reals_of};
use super::value::Value;
use super::value::MAX_ELEMENTS;
use super::work::Budget;
use crate::expr::{BinaryOp, Expr, UnaryOp};

/// Space points when the call gives none (see the module documentation).
const DEFAULT_POINTS: usize = 20;
/// Scratch names of the solution grid, from which each solution function is built.
const XGRID: &str = "__pdesolve_xgrid";
const TGRID: &str = "__pdesolve_tgrid";
const FIELD: &str = "__pdesolve_field";
/// The builtin that interpolates a solution (registered with `pde_grid`, hidden from the dialog).
pub(super) const INTERP: &str = "pdesolve_interp";

/// True for a solve-block member that is a `pdesolve` call (through parentheses or `=`).
pub(super) fn is_pdesolve(expr: &Expr) -> bool {
    match strip(expr) {
        Expr::Call { func, .. } => func.name.eq_ignore_ascii_case("pdesolve"),
        _ => false,
    }
}

/// The scratch name of the `k`-th x-derivative of unknown `i`.
fn state_name(i: usize, k: usize) -> String {
    format!("__pdesolve_{i}_{k}")
}

/// One use of an unknown: its index, its derivative counts in x and t, and its arguments.
struct Use {
    unknown: usize,
    nx: usize,
    nt: usize,
    args: Vec<Expr>,
}

/// One classified constraint.
enum Kind {
    /// `u_t(x, t) = value`.
    Pde { unknown: usize, value: Expr },
    /// `u(a, t) = value` (Dirichlet, `neumann` false) or `ux(a, t) = value`, with `a` at `side`
    /// (0 for the left end, 1 for the right).
    Edge {
        unknown: usize,
        side: usize,
        neumann: bool,
        value: Expr,
    },
    /// `u(x, t0) = value`.
    Initial { unknown: usize, value: Expr },
}

/// Solves the `pdesolve` member `call` of a solve block (see the module documentation). `names` are
/// the target names, one per unknown in the order of `u`. Returns the solution functions.
pub(super) fn solve_pdesolve(
    env: &Env,
    call: &Expr,
    names: &[String],
    constraints: &[Constraint],
    budget: &Budget,
) -> Result<Vec<(String, UserFunction)>, EvalError> {
    let Expr::Call { args, .. } = strip(call) else {
        return Err(unsupported("pdesolve is a function call"));
    };
    let (u, x, xr, t, tr, points) = match args.as_slice() {
        [u, x, xr, t, tr] => (u, x, xr, t, tr, None),
        [u, x, xr, t, tr, n] => (u, x, xr, t, tr, Some((n, None))),
        [u, x, xr, t, tr, n, m] => (u, x, xr, t, tr, Some((n, Some(m)))),
        _ => {
            return Err(unsupported(
                "pdesolve takes the functions, the space variable and its range, the time variable and its range, and optionally the numbers of points",
            ))
        }
    };
    let unknowns = unknown_names(u)?;
    if names.len() != unknowns.len() {
        return Err(unsupported(format!(
            "pdesolve gives {} functions for {} names",
            unknowns.len(),
            names.len()
        )));
    }
    if constraints.is_empty() {
        return Err(unsupported("solve block has no constraints"));
    }
    let xname = bound_name(x, "space")?;
    let tname = bound_name(t, "time")?;
    if xname == tname {
        return Err(unsupported(
            "pdesolve: the space and time variables must differ",
        ));
    }
    let xv = xname.chars().next().unwrap_or('x');
    let tv = tname.chars().next().unwrap_or('t');
    let (a, b) = range_of(env, xr, budget, "the space range")?;
    let (t0, t1) = range_of(env, tr, budget, "the time range")?;
    let (xpts, tpts) = match points {
        None => (DEFAULT_POINTS, DEFAULT_POINTS),
        Some((n, m)) => (
            count_of(env, n, budget, "xpts")?,
            match m {
                None => DEFAULT_POINTS,
                Some(m) => count_of(env, m, budget, "tpts")?,
            },
        ),
    };

    let cells = xpts * tpts * unknowns.len();
    if cells > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(cells));
    }

    // Classify every constraint.
    let mut kinds = Vec::with_capacity(constraints.len());
    for c in constraints {
        if !c.is_equality() {
            return Err(unsupported(
                "an inequality cannot be a constraint of a pdesolve block",
            ));
        }
        let (lhs, rhs) = c.sides();
        // A PDE has its time derivative on the left, so its right side may use the unknowns.
        let (main, value) = match (
            decode(lhs, &unknowns, xv, tv)?,
            decode(rhs, &unknowns, xv, tv)?,
        ) {
            (Some(u), None) => (u, rhs),
            (None, Some(u)) => (u, lhs),
            (Some(u), Some(_)) if u.nt == 1 && u.nx == 0 => (u, rhs),
            (Some(_), Some(v)) if v.nt == 1 && v.nx == 0 => (v, lhs),
            (None, None) => {
                // The unknown may sit inside a sum, as in `T_t(x,t) - a^2*T_xx(x,t) = 0`.
                let (u, value) = time_term_of_difference(lhs, rhs, &unknowns, xv, tv)?;
                kinds.push(classify(
                    u,
                    &value,
                    env,
                    budget,
                    (&xname, &tname),
                    (a, b),
                    t0,
                    &unknowns,
                )?);
                continue;
            }
            _ => {
                return Err(unsupported(
                    "each pdesolve constraint has one unknown function on one side: u_t(x,t) = ..., u(a,t) = ... or u(x,t0) = ...",
                ))
            }
        };
        kinds.push(classify(
            main,
            value,
            env,
            budget,
            (&xname, &tname),
            (a, b),
            t0,
            &unknowns,
        )?);
    }

    // The PDE of each unknown, rewritten with the state names; the uses that decide its order.
    let m = unknowns.len();
    let mut pdes: Vec<Option<Expr>> = vec![None; m];
    let mut initials: Vec<Option<Expr>> = vec![None; m];
    let mut edges: Vec<[Option<(bool, Expr)>; 2]> = vec![[None, None]; m];
    for kind in kinds {
        match kind {
            Kind::Pde { unknown, value } => {
                let rewritten = rewrite_pde(&value, &unknowns, xv, tv)?;
                if pdes[unknown].replace(rewritten).is_some() {
                    return Err(unsupported(format!(
                        "pdesolve: more than one PDE for `{}`",
                        unknowns[unknown]
                    )));
                }
            }
            Kind::Initial { unknown, value } => {
                if initials[unknown].replace(value).is_some() {
                    return Err(unsupported(format!(
                        "pdesolve: more than one initial condition for `{}`",
                        unknowns[unknown]
                    )));
                }
            }
            Kind::Edge {
                unknown,
                side,
                neumann,
                value,
            } => {
                if edges[unknown][side].replace((neumann, value)).is_some() {
                    return Err(unsupported(format!(
                        "pdesolve: more than one boundary condition at one end of `{}`",
                        unknowns[unknown]
                    )));
                }
            }
        }
    }
    let mut orders = vec![0usize; m];
    for i in 0..m {
        let Some(_) = pdes[i] else {
            return Err(unsupported(format!(
                "pdesolve needs a PDE for `{}`",
                unknowns[i]
            )));
        };
        if initials[i].is_none() {
            return Err(unsupported(format!(
                "pdesolve needs an initial condition u(x, t0) for `{}`",
                unknowns[i]
            )));
        }
        let used = |k: usize| {
            pdes.iter()
                .flatten()
                .any(|e| uses_variable(e, &state_name(i, k)))
        };
        if used(2) {
            orders[i] = 2;
        } else if used(1) {
            return Err(unsupported(
                "pdesolve: an equation with only a first spatial derivative is not supported yet",
            ));
        }
        let given = edges[i].iter().filter(|e| e.is_some()).count();
        if orders[i] == 2 && given != 2 {
            return Err(unsupported(format!(
                "pdesolve needs a boundary condition at each end of `{}`",
                unknowns[i]
            )));
        }
        if orders[i] == 0 && given != 0 {
            return Err(unsupported(format!(
                "pdesolve: `{}` has no spatial derivative, so it takes no boundary condition",
                unknowns[i]
            )));
        }
    }

    // The solve, on closures over one scratch environment.
    let scratch = RefCell::new(env.clone());
    let pdes: Vec<Expr> = pdes.into_iter().flatten().collect();
    let initials: Vec<Expr> = initials.into_iter().flatten().collect();
    let rhs =
        |x: f64, t: f64, uu: &[f64], ux: &[f64], uxx: &[f64]| -> Result<Vec<f64>, EvalError> {
            {
                let mut e = scratch.borrow_mut();
                e.set_variable(xname.clone(), real_value(x));
                e.set_variable(tname.clone(), real_value(t));
                for i in 0..m {
                    e.set_variable(state_name(i, 0), real_value(uu[i]));
                    e.set_variable(state_name(i, 1), real_value(ux[i]));
                    e.set_variable(state_name(i, 2), real_value(uxx[i]));
                }
            }
            let e = scratch.borrow();
            pdes.iter()
                .map(|p| real_of(&evaluate_budget(p, &e, budget)?, "the PDE"))
                .collect()
        };
    let order_of = orders.clone();
    let bc = |t: f64| -> Result<Vec<Option<[Bc; 2]>>, EvalError> {
        scratch
            .borrow_mut()
            .set_variable(tname.clone(), real_value(t));
        let e = scratch.borrow();
        (0..m)
            .map(|i| {
                if order_of[i] == 0 {
                    return Ok(None);
                }
                let mut pair = [Bc::Dirichlet(0.0), Bc::Dirichlet(0.0)];
                for side in 0..2 {
                    let (neumann, value) = edges[i][side].as_ref().expect("checked above");
                    let v = real_of(&evaluate_budget(value, &e, budget)?, "a boundary value")?;
                    pair[side] = if *neumann {
                        Bc::Neumann(v)
                    } else {
                        Bc::Dirichlet(v)
                    };
                }
                Ok(Some(pair))
            })
            .collect()
    };
    let init = |x: f64| -> Result<Vec<f64>, EvalError> {
        scratch
            .borrow_mut()
            .set_variable(xname.clone(), real_value(x));
        let e = scratch.borrow();
        initials
            .iter()
            .map(|v| real_of(&evaluate_budget(v, &e, budget)?, "the initial value"))
            .collect()
    };
    let charge = |units: u64| budget.charge(units);
    let problem = Problem {
        a,
        b,
        xpts,
        t0,
        t1,
        tpts,
        orders,
        rhs: &rhs,
        bc: &bc,
        init: &init,
        charge: &charge,
    };
    let sol = pde_mol::solve(&problem)?;

    // One interpolated function per unknown.
    let hx = (b - a) / (xpts - 1) as f64;
    let ht = (t1 - t0) / (tpts - 1) as f64;
    let xs: Vec<f64> = (0..xpts).map(|i| a + i as f64 * hx).collect();
    let ts: Vec<f64> = (0..tpts).map(|j| t0 + j as f64 * ht).collect();
    let mut functions = Vec::with_capacity(m);
    for i in 0..m {
        let mut field = vec![0.0; xpts * tpts];
        for j in 0..tpts {
            for ix in 0..xpts {
                field[ix * tpts + j] = sol[i * tpts + j][ix];
            }
        }
        let mut scratch_env = env.clone();
        scratch_env.set_variable(XGRID.to_string(), column(&xs)?);
        scratch_env.set_variable(TGRID.to_string(), column(&ts)?);
        scratch_env.set_variable(FIELD.to_string(), matrix(xpts, tpts, &field)?);
        let params = vec![xname.clone(), tname.clone()];
        let body = Expr::call(
            INTERP,
            vec![
                Expr::var(XGRID),
                Expr::var(TGRID),
                Expr::var(FIELD),
                Expr::var(&xname),
                Expr::var(&tname),
            ],
        );
        let closure = capture(&names[i], &params, &body, &scratch_env);
        functions.push((
            names[i].clone(),
            UserFunction {
                params,
                body,
                closure,
            },
        ));
    }
    Ok(functions)
}

/// The builtin `pdesolve_interp(xgrid, tgrid, field, x, t)`: the bilinear interpolation of the
/// solution `field` (rows along `xgrid`, columns along `tgrid`) at `(x, t)`. Outside the grid it is
/// refused, as the help's solution is only defined in the solved range.
pub(super) fn interp_builtin(args: &[Value]) -> Result<Value, EvalError> {
    let [xg, tg, field, x, t] = args else {
        return Err(unsupported("pdesolve_interp takes five arguments"));
    };
    let xs = reals_of(xg, "the space grid")?;
    let ts = reals_of(tg, "the time grid")?;
    let (rows, cols) = match field {
        Value::Matrix(mx) => (mx.rows(), mx.cols()),
        _ => return Err(unsupported("pdesolve_interp needs the solution matrix")),
    };
    if rows != xs.len() || cols != ts.len() || xs.len() < 2 || ts.len() < 2 {
        return Err(unsupported(
            "pdesolve_interp: the grids do not match the solution",
        ));
    }
    let z = reals_of(field, "the solution")?;
    let (x, t) = (real_of(x, "x")?, real_of(t, "t")?);
    let (i, fx) = locate(&xs, x)?;
    let (j, ft) = locate(&ts, t)?;
    let at = |di: usize, dj: usize| z[(i + di) * cols + (j + dj)];
    let value = (1.0 - fx) * (1.0 - ft) * at(0, 0)
        + fx * (1.0 - ft) * at(1, 0)
        + (1.0 - fx) * ft * at(0, 1)
        + fx * ft * at(1, 1);
    Ok(real_value(value))
}

/// The lower index of the cell of an ascending grid that holds `v`, and the fraction into it.
fn locate(grid: &[f64], v: f64) -> Result<(usize, f64), EvalError> {
    let (lo, hi) = (grid[0], grid[grid.len() - 1]);
    if !(v >= lo && v <= hi) {
        return Err(EvalError::Unsupported(
            "pdesolve: the solution is only defined inside the solved range; it is not extrapolated"
                .to_string(),
        ));
    }
    let i = grid
        .partition_point(|g| *g <= v)
        .saturating_sub(1)
        .min(grid.len() - 2);
    Ok((i, (v - grid[i]) / (grid[i + 1] - grid[i])))
}

/// The unknown functions named by the first argument: a name, or a column of names.
fn unknown_names(u: &Expr) -> Result<Vec<String>, EvalError> {
    let items: Vec<&Expr> = match strip(u) {
        Expr::Matrix {
            cols: 1, elements, ..
        } => elements.iter().collect(),
        other => vec![other],
    };
    let mut names = Vec::with_capacity(items.len());
    for item in items {
        let Expr::Id(id) = strip(item) else {
            return Err(unsupported(
                "pdesolve takes the unknown functions by name, such as T or (w; v)",
            ));
        };
        let key = var_key(id)?;
        if names.contains(&key) {
            return Err(unsupported(format!("`{key}` is listed twice")));
        }
        names.push(key);
    }
    if names.is_empty() {
        return Err(unsupported("pdesolve takes at least one unknown function"));
    }
    Ok(names)
}

/// The name of a variable argument (`x` or `t`), which must be a single letter.
fn bound_name(e: &Expr, what: &str) -> Result<String, EvalError> {
    match strip(e) {
        Expr::Id(id) => {
            let key = var_key(id)?;
            if key.chars().count() == 1 {
                Ok(key)
            } else {
                Err(unsupported(format!(
                    "pdesolve: the {what} variable must be a single letter"
                )))
            }
        }
        _ => Err(unsupported(format!(
            "pdesolve: the {what} variable must be a name"
        ))),
    }
}

/// Two real end points, the lower first, of a range given in the call.
fn range_of(env: &Env, e: &Expr, budget: &Budget, what: &str) -> Result<(f64, f64), EvalError> {
    let xs = reals_of(&evaluate_budget(e, env, budget)?, what)?;
    match xs.as_slice() {
        [a, b] if a < b => Ok((*a, *b)),
        _ => Err(unsupported(format!(
            "{what} must be two real numbers, the lower end first"
        ))),
    }
}

/// A whole number of points from [`MIN_POINTS`] to the limit for `what`.
fn count_of(env: &Env, e: &Expr, budget: &Budget, what: &str) -> Result<usize, EvalError> {
    let n = real_of(&evaluate_budget(e, env, budget)?, what)?;
    let max = if what == "xpts" {
        MAX_SPACE_POINTS
    } else {
        MAX_TIME_POINTS
    };
    if n.fract() != 0.0 || n < MIN_POINTS as f64 || n > max as f64 {
        return Err(unsupported(format!(
            "{what} must be an integer from {MIN_POINTS} to {max}"
        )));
    }
    Ok(n as usize)
}

/// The use of an unknown in `node`, when it is one: a call `u_xx(x, t)` (subscript name) or a
/// derivative operator on `u(x, t)`. `None` for any other node.
fn decode(node: &Expr, unknowns: &[String], xv: char, tv: char) -> Result<Option<Use>, EvalError> {
    match strip(node) {
        Expr::Call { func, args } => {
            let name = var_key(func)?;
            for (i, base) in unknowns.iter().enumerate() {
                let Some(rest) = name.strip_prefix(base.as_str()) else {
                    continue;
                };
                let suffix = rest.strip_prefix('_').unwrap_or(rest);
                if suffix.chars().all(|c| c == xv || c == tv) {
                    let nx = suffix.chars().filter(|c| *c == xv).count();
                    let nt = suffix.chars().filter(|c| *c == tv).count();
                    return Ok(Some(Use {
                        unknown: i,
                        nx,
                        nt,
                        args: args.clone(),
                    }));
                }
            }
            Ok(None)
        }
        Expr::Derivative {
            var, order, body, ..
        } => {
            let Expr::Call { func, args } = strip(body) else {
                return Ok(None);
            };
            let name = var_key(func)?;
            let Some(i) = unknowns.iter().position(|u| *u == name) else {
                return Ok(None);
            };
            let k = match order.as_deref() {
                None => 1,
                Some(Expr::Number(n)) => number_value(n)? as usize,
                Some(_) => return Err(unsupported("a derivative order must be a whole number")),
            };
            let dv = var_key(var)?;
            let (nx, nt) = if dv == xv.to_string() {
                (k, 0)
            } else if dv == tv.to_string() {
                (0, k)
            } else {
                return Err(unsupported(format!(
                    "the derivatives of `{name}` must be taken with respect to {xv} or {tv}"
                )));
            };
            Ok(Some(Use {
                unknown: i,
                nx,
                nt,
                args: args.clone(),
            }))
        }
        _ => Ok(None),
    }
}

/// Classifies one equation: `main` is the side that uses an unknown, `value` the other side.
#[allow(clippy::too_many_arguments)]
fn classify(
    main: Use,
    value: &Expr,
    env: &Env,
    budget: &Budget,
    names: (&str, &str),
    (a, b): (f64, f64),
    t0: f64,
    unknowns: &[String],
) -> Result<Kind, EvalError> {
    let (xname, tname) = names;
    let is_var = |e: &Expr, name: &str| is_name(e, name);
    let unknown = main.unknown;
    let xv = xname.chars().next().unwrap_or('x');
    let tv = tname.chars().next().unwrap_or('t');
    let args = &main.args[..];
    match (main.nx, main.nt, args) {
        (0, 1, [px, pt]) if is_var(px, xname) && is_var(pt, tname) => Ok(Kind::Pde {
            unknown,
            value: value.clone(),
        }),
        (nx @ 0..=1, 0, [pos, pt]) if is_var(pt, tname) => {
            forbid_unknowns(value, unknowns, xv, tv)?;
            let p = real_of(&evaluate_budget(pos, env, budget)?, "a boundary point")?;
            let side = if (p - a).abs() <= 1e-12 * (1.0 + a.abs()) {
                0
            } else if (p - b).abs() <= 1e-12 * (1.0 + b.abs()) {
                1
            } else {
                return Err(unsupported(
                    "pdesolve: a boundary condition must be at an end of the space range",
                ));
            };
            Ok(Kind::Edge {
                unknown,
                side,
                neumann: nx == 1,
                value: value.clone(),
            })
        }
        (0, 0, [px, pt]) if is_var(px, xname) => {
            forbid_unknowns(value, unknowns, xv, tv)?;
            let p = real_of(&evaluate_budget(pt, env, budget)?, "the initial time")?;
            if (p - t0).abs() > 1e-12 * (1.0 + t0.abs()) {
                return Err(unsupported(
                    "pdesolve: an initial condition must be at the start of the time range",
                ));
            }
            Ok(Kind::Initial {
                unknown,
                value: value.clone(),
            })
        }
        _ => Err(unsupported(
            "pdesolve: a constraint must be u_t(x,t) = ..., a boundary u(a,t) = ... or ux(a,t) = ..., or an initial u(x,t0) = ...; a mixed or higher-order form is not supported",
        )),
    }
}

/// Solves `lhs = rhs` for the time derivative of the one unknown whose `u_t(x,t)` is a term of
/// `lhs - rhs`. The difference is flattened into its top-level signed terms; the derivative's term
/// may carry a numeric factor `c`, and the equation `c u_t + (other terms) = 0` gives the value
/// `-(other terms) / c`. Refused unless exactly one such term exists.
fn time_term_of_difference(
    lhs: &Expr,
    rhs: &Expr,
    unknowns: &[String],
    xv: char,
    tv: char,
) -> Result<(Use, Expr), EvalError> {
    let mut terms = Vec::new();
    additive_terms(lhs, 1.0, &mut terms);
    additive_terms(rhs, -1.0, &mut terms);
    let mut found: Option<(usize, Use, f64)> = None;
    for (idx, (sign, term)) in terms.iter().enumerate() {
        let Some((u, factor)) = time_derivative_term(term, unknowns, xv, tv)? else {
            continue;
        };
        if found.is_some() {
            return Err(unsupported(
                "pdesolve: a constraint may have only one u_t(x,t) term",
            ));
        }
        found = Some((idx, u, sign * factor));
    }
    let Some((idx, u, coef)) = found else {
        return Err(unsupported(
            "each pdesolve constraint has one unknown function on one side: u_t(x,t) = ..., u(a,t) = ... or u(x,t0) = ...",
        ));
    };
    if coef == 0.0 {
        return Err(unsupported(
            "pdesolve: the coefficient of u_t(x,t) must not be zero",
        ));
    }
    let mut rest: Option<Expr> = None;
    for (j, (sign, term)) in terms.into_iter().enumerate() {
        if j == idx {
            continue;
        }
        let signed = if sign > 0.0 {
            term
        } else {
            Expr::unary(UnaryOp::Neg, term)
        };
        rest = Some(match rest {
            None => signed,
            Some(r) => Expr::binary(BinaryOp::Add, r, signed),
        });
    }
    let rest = rest.unwrap_or_else(|| Expr::num(0.0));
    let value = Expr::binary(
        BinaryOp::Mul,
        Expr::num(-1.0 / coef),
        Expr::Parens(Box::new(rest)),
    );
    Ok((u, value))
}

/// Flattens `sign * e` into its top-level sum, pushing each term with its sign (`+1` or `-1`).
fn additive_terms(e: &Expr, sign: f64, out: &mut Vec<(f64, Expr)>) {
    match strip(e) {
        Expr::Binary {
            op: BinaryOp::Add,
            lhs,
            rhs,
        } => {
            additive_terms(lhs, sign, out);
            additive_terms(rhs, sign, out);
        }
        Expr::Binary {
            op: BinaryOp::Sub,
            lhs,
            rhs,
        } => {
            additive_terms(lhs, sign, out);
            additive_terms(rhs, -sign, out);
        }
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => additive_terms(operand, -sign, out),
        other => out.push((sign, other.clone())),
    }
}

/// `Some((use, factor))` when `term` is `u_t(x,t)` or `c * u_t(x,t)` with a numeric `c`.
fn time_derivative_term(
    term: &Expr,
    unknowns: &[String],
    xv: char,
    tv: char,
) -> Result<Option<(Use, f64)>, EvalError> {
    let pure_time = |u: Use| (u.nx == 0 && u.nt == 1).then_some(u);
    if let Expr::Binary {
        op: BinaryOp::Mul,
        lhs,
        rhs,
    } = strip(term)
    {
        for (factor, other) in [(lhs, rhs), (rhs, lhs)] {
            if let Some(c) = numeric_factor(factor)? {
                return Ok(decode(other, unknowns, xv, tv)?
                    .and_then(pure_time)
                    .map(|u| (u, c)));
            }
        }
        return Ok(None);
    }
    Ok(decode(term, unknowns, xv, tv)?
        .and_then(pure_time)
        .map(|u| (u, 1.0)))
}

/// The value of a literal number, with any signs in front of it; `None` for other expressions.
fn numeric_factor(e: &Expr) -> Result<Option<f64>, EvalError> {
    match strip(e) {
        Expr::Number(n) => Ok(Some(number_value(n)?)),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => Ok(numeric_factor(operand)?.map(|v| -v)),
        _ => Ok(None),
    }
}

/// Rewrites a PDE's right-hand side: each `u_xx(x, t)` becomes its state name. Other uses of the
/// unknowns (at other points, or time derivatives) are refused.
fn rewrite_pde(value: &Expr, unknowns: &[String], xv: char, tv: char) -> Result<Expr, EvalError> {
    let xname = xv.to_string();
    let tname = tv.to_string();
    let mut node = |n: &Expr| -> Option<Result<Expr, EvalError>> {
        match decode(n, unknowns, xv, tv) {
            Ok(Some(u)) => {
                let on_point = matches!(u.args.as_slice(), [px, pt]
                    if is_name(px, &xname) && is_name(pt, &tname));
                if u.nt != 0 || !on_point {
                    return Some(Err(unsupported(
                        "pdesolve: the right-hand side of a PDE may use u, ux and uxx at (x, t) only",
                    )));
                }
                Some(Ok(Expr::var(&state_name(u.unknown, u.nx))))
            }
            Ok(None) => match n {
                Expr::Id(id) => match var_key(id) {
                    Ok(key) if unknowns.contains(&key) => Some(Err(unsupported(format!(
                        "`{key}` in a PDE must be called as {key}(x, t)"
                    )))),
                    _ => None,
                },
                _ => None,
            },
            Err(e) => Some(Err(e)),
        }
    };
    rewrite(value, &mut node)
}

/// Refuses any use of an unknown in `e` (a boundary or initial value depends on the variables only).
fn forbid_unknowns(e: &Expr, unknowns: &[String], xv: char, tv: char) -> Result<(), EvalError> {
    let mut found = false;
    let mut node = |n: &Expr| -> Option<Result<Expr, EvalError>> {
        let bare = match n {
            Expr::Id(id) => var_key(id).is_ok_and(|key| unknowns.contains(&key)),
            _ => false,
        };
        if bare || matches!(decode(n, unknowns, xv, tv), Ok(Some(_))) {
            found = true;
        }
        None
    };
    let _ = rewrite(e, &mut node)?;
    if found {
        return Err(unsupported(
            "pdesolve: a boundary or initial value cannot use the unknown functions",
        ));
    }
    Ok(())
}

fn is_name(e: &Expr, name: &str) -> bool {
    matches!(strip(e), Expr::Id(id) if var_key(id).ok().as_deref() == Some(name))
}

fn strip(expr: &Expr) -> &Expr {
    match expr {
        Expr::Parens(inner) => strip(inner),
        other => other,
    }
}

fn unsupported(message: impl Into<String>) -> EvalError {
    EvalError::Unsupported(message.into())
}

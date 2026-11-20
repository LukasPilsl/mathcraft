//! The grid solvers for Poisson's equation (help: "Relaxation Method for a PDE"): `relax` and
//! `multigrid`.
//!
//! - `relax(A, B, C, D, E, S, U, rjac)`: the discrete equation
//!   `A u[i+1,j] + B u[i-1,j] + C u[i,j+1] + D u[i,j-1] + E u[i,j] = S[i,j]` on the interior of the
//!   square grid, with the edge values of `U` fixed. Solved by successive over-relaxation of
//!   Gauss-Seidel sweeps, with the relaxation factor `2 / (1 + sqrt(1 - rjac^2))` for the spectral
//!   radius `rjac` of the Jacobi iteration. The help's example reproduces with `rjac = 1 - 2 pi / R`.
//!   It is refused with the help's message when it does not converge within [`MAX_SWEEPS`].
//! - `multigrid(M, ncycle)`: the Poisson equation `laplacian(u) = M` with zero boundary values on a
//!   grid of `2^k + 1` points per side, spacing `h = 1 / (N - 1)`. Full multigrid: the solution is
//!   started on the coarsest grid and interpolated up, with `ncycle` V-cycles on each level. The
//!   restricted right-hand sides carry the factor 4 of the doubled spacing.
//!
//! Both are context builtins: they charge every sweep to the statement's budget, so a grid that
//! needs too many sweeps is refused by the budget, not run to the end.

use super::super::error::EvalError;
use super::super::special_forms::{matrix, reals_of};
use super::super::value::Value;
use super::{Arity, BuiltinCtx, Registry};

/// Largest grid side accepted by either solver.
pub const MAX_SIDE: usize = 1025;
/// Sweeps one `relax` solve may take before it is refused.
pub const MAX_SWEEPS: usize = 20_000;
/// Largest `ncycle` accepted by `multigrid`.
pub const MAX_NCYCLE: usize = 100;
/// Work units charged per grid point per sweep (a Gauss-Seidel update or a residual).
const POINT_UNITS: u64 = 20;
/// Relative change at which `relax` is taken as converged.
const RELAX_TOL: f64 = 1e-10;

/// Registers `relax` and `multigrid`.
pub fn register(r: &mut Registry) {
    r.register_ctx("relax", Arity::Exact(8), relax_builtin);
    r.register_ctx("multigrid", Arity::Exact(2), multigrid_builtin);
    r.register(
        "pdesolve_interp",
        Arity::Exact(5),
        super::super::pde_block::interp_builtin,
    );
}

/// A square matrix argument as its side and its values, row-major.
fn square(v: &Value, what: &str) -> Result<(usize, Vec<f64>), EvalError> {
    match v {
        Value::Matrix(m) if m.rows() == m.cols() => Ok((m.rows(), reals_of(v, what)?)),
        _ => Err(EvalError::Unsupported(format!(
            "{what} must be a square matrix"
        ))),
    }
}

fn check_side(n: usize) -> Result<(), EvalError> {
    if !(3..=MAX_SIDE).contains(&n) {
        return Err(EvalError::Unsupported(format!(
            "the grid must be between 3 and {MAX_SIDE} points per side"
        )));
    }
    Ok(())
}

fn relax_builtin(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let [a, b, c, d, e, s, u, rjac] = args else {
        return Err(EvalError::Unsupported("relax takes eight arguments".into()));
    };
    let (n, a) = square(a, "A")?;
    let mut coefs = vec![a];
    for (value, what) in [(b, "B"), (c, "C"), (d, "D"), (e, "E"), (s, "S"), (u, "U")] {
        let (m, data) = square(value, what)?;
        if m != n {
            return Err(EvalError::Unsupported(format!(
                "{what} must be {n} x {n}, like A"
            )));
        }
        coefs.push(data);
    }
    check_side(n)?;
    let rjac = match rjac {
        Value::Scalar(_) => reals_of(rjac, "rjac")?[0],
        _ => return Err(EvalError::Unsupported("rjac must be a number".into())),
    };
    if !(rjac > 0.0 && rjac < 1.0) {
        return Err(EvalError::Unsupported(
            "rjac must be a real number between 0 and 1".into(),
        ));
    }
    let [a, b, c, d, e, s, u] = <[Vec<f64>; 7]>::try_from(coefs)
        .map_err(|_| EvalError::Unsupported("relax: internal shape error".into()))?;
    let out = relax(
        n,
        &Stencil {
            a: &a,
            b: &b,
            c: &c,
            d: &d,
            e: &e,
        },
        &s,
        u,
        rjac,
        &|units| ctx.charge(units),
    )?;
    matrix(n, n, &out)
}

/// The five coefficient matrices of the discrete equation, row-major `n x n`.
pub struct Stencil<'a> {
    pub a: &'a [f64],
    pub b: &'a [f64],
    pub c: &'a [f64],
    pub d: &'a [f64],
    pub e: &'a [f64],
}

/// Solves the discrete equation of [`Stencil`] on the `n x n` grid, starting from `u` (whose edges
/// are the boundary values). `charge` is called with the work of each sweep.
pub fn relax(
    n: usize,
    st: &Stencil<'_>,
    s: &[f64],
    mut u: Vec<f64>,
    rjac: f64,
    charge: &dyn Fn(u64) -> Result<(), EvalError>,
) -> Result<Vec<f64>, EvalError> {
    let omega = 2.0 / (1.0 + (1.0 - rjac * rjac).sqrt());
    let idx = |i: usize, j: usize| i * n + j;
    if (1..n - 1).any(|i| (1..n - 1).any(|j| st.e[idx(i, j)] == 0.0)) {
        return Err(EvalError::Unsupported(
            "relax: the coefficient E is zero inside the grid".into(),
        ));
    }
    let interior = ((n - 2) * (n - 2)) as u64;
    for _ in 0..MAX_SWEEPS {
        charge(interior.saturating_mul(POINT_UNITS))?;
        let mut change = 0.0f64;
        let mut size = 0.0f64;
        for i in 1..n - 1 {
            for j in 1..n - 1 {
                let p = idx(i, j);
                let rest = st.a[p] * u[idx(i + 1, j)]
                    + st.b[p] * u[idx(i - 1, j)]
                    + st.c[p] * u[idx(i, j + 1)]
                    + st.d[p] * u[idx(i, j - 1)];
                let gs = (s[p] - rest) / st.e[p];
                let new = u[p] + omega * (gs - u[p]);
                change = change.max((new - u[p]).abs());
                size = size.max(new.abs());
                u[p] = new;
            }
        }
        if !change.is_finite() {
            return Err(EvalError::NotFinite);
        }
        if change <= RELAX_TOL * size.max(1.0) {
            return Ok(u);
        }
    }
    Err(EvalError::NoConvergence(
        "relax: too many iterations; try reducing rjac".into(),
    ))
}

fn multigrid_builtin(ctx: &BuiltinCtx, args: &[Value]) -> Result<Value, EvalError> {
    let [m, ncycle] = args else {
        return Err(EvalError::Unsupported(
            "multigrid takes two arguments".into(),
        ));
    };
    let (n, data) = square(m, "M")?;
    let cycles = reals_of(ncycle, "ncycle")?;
    let cycles = match cycles.as_slice() {
        [c] if c.fract() == 0.0 && *c >= 1.0 && *c <= MAX_NCYCLE as f64 => *c as usize,
        _ => {
            return Err(EvalError::Unsupported(format!(
                "ncycle must be a whole number from 1 to {MAX_NCYCLE}"
            )))
        }
    };
    if !(3..=MAX_SIDE).contains(&n) || !(n - 1).is_power_of_two() {
        return Err(EvalError::Unsupported(
            "multigrid needs a square matrix of 2^k + 1 points per side (3, 5, 9, 17, 33, ...)"
                .into(),
        ));
    }
    let out = multigrid(n, &data, cycles, &|units| ctx.charge(units))?;
    matrix(n, n, &out)
}

/// Full multigrid for `laplacian(u) = M` on the `n x n` grid with zero boundary values, `n = 2^k + 1`.
/// `M` is the source at each point; the spacing is `1 / (n - 1)`.
pub fn multigrid(
    n: usize,
    source: &[f64],
    ncycle: usize,
    charge: &dyn Fn(u64) -> Result<(), EvalError>,
) -> Result<Vec<f64>, EvalError> {
    // Level `l` has side `2^l + 1`; the coarsest level is `l = 1`, with one interior point.
    let top = (n - 1).trailing_zeros() as usize;
    let h = 1.0 / (n - 1) as f64;
    // The right-hand sides of each level, in the unscaled stencil `neighbours - 4 u = b`: the fine
    // one is `h^2 M`, each coarser one is `4 R(b)` (full weighting R, the doubled spacing).
    let mut rhs: Vec<Vec<f64>> = vec![Vec::new(); top + 1];
    rhs[top] = source.iter().map(|m| h * h * m).collect();
    for l in (2..=top).rev() {
        rhs[l - 1] = restrict(side(l), &rhs[l], 4.0);
    }
    let mut u: Vec<f64> = vec![0.0; side(1) * side(1)];
    v_cycle(side(1), &mut u, &rhs[1], &|units| charge(units))?;
    for (l, level_rhs) in rhs.iter().enumerate().take(top + 1).skip(2) {
        u = prolong(side(l - 1), &u);
        for _ in 0..ncycle {
            v_cycle(side(l), &mut u, level_rhs, &|units| charge(units))?;
        }
    }
    Ok(u)
}

/// Side length of level `l`: `2^l + 1`.
fn side(l: usize) -> usize {
    (1 << l) + 1
}

/// One V-cycle on a grid of side `n`: two smoothing sweeps, the coarse correction, two sweeps. The
/// grid of side 3 is solved exactly (one interior point, `u = -b / 4`).
fn v_cycle(
    n: usize,
    u: &mut [f64],
    b: &[f64],
    charge: &dyn Fn(u64) -> Result<(), EvalError>,
) -> Result<(), EvalError> {
    if n == 3 {
        u[4] = -b[4] / 4.0;
        return Ok(());
    }
    let points = ((n - 2) * (n - 2)) as u64;
    charge(points.saturating_mul(POINT_UNITS * 5))?;
    smooth(n, u, b);
    smooth(n, u, b);
    let res = residual(n, u, b);
    let coarse_rhs = restrict(n, &res, 4.0);
    let mut e = vec![0.0; (n / 2 + 1) * (n / 2 + 1)];
    v_cycle(n / 2 + 1, &mut e, &coarse_rhs, charge)?;
    let corr = prolong(n / 2 + 1, &e);
    for (ui, ei) in u.iter_mut().zip(corr) {
        *ui += ei;
    }
    smooth(n, u, b);
    smooth(n, u, b);
    Ok(())
}

/// One Gauss-Seidel sweep of `neighbours - 4 u = b` over the interior.
fn smooth(n: usize, u: &mut [f64], b: &[f64]) {
    for i in 1..n - 1 {
        for j in 1..n - 1 {
            let p = i * n + j;
            u[p] = (u[p - n] + u[p + n] + u[p - 1] + u[p + 1] - b[p]) / 4.0;
        }
    }
}

/// `b - (neighbours - 4 u)` at every point (zero on the edges).
fn residual(n: usize, u: &[f64], b: &[f64]) -> Vec<f64> {
    let mut r = vec![0.0; n * n];
    for i in 1..n - 1 {
        for j in 1..n - 1 {
            let p = i * n + j;
            r[p] = b[p] - (u[p - n] + u[p + n] + u[p - 1] + u[p + 1] - 4.0 * u[p]);
        }
    }
    r
}

/// Full-weighting restriction of an `n x n` grid to `(n+1)/2` points per side, scaled by `factor`.
fn restrict(n: usize, fine: &[f64], factor: f64) -> Vec<f64> {
    let m = n / 2 + 1;
    let mut out = vec![0.0; m * m];
    for i in 1..m - 1 {
        for j in 1..m - 1 {
            let (fi, fj) = (2 * i, 2 * j);
            let at = |di: isize, dj: isize| {
                fine[(fi as isize + di) as usize * n + (fj as isize + dj) as usize]
            };
            let centre = 4.0 * at(0, 0);
            let edges = 2.0 * (at(1, 0) + at(-1, 0) + at(0, 1) + at(0, -1));
            let corners = at(1, 1) + at(1, -1) + at(-1, 1) + at(-1, -1);
            out[i * m + j] = factor * (centre + edges + corners) / 16.0;
        }
    }
    out
}

/// Bilinear prolongation of an `m x m` coarse grid to `(2m-1) x (2m-1)`.
fn prolong(m: usize, coarse: &[f64]) -> Vec<f64> {
    let n = 2 * m - 1;
    let mut out = vec![0.0; n * n];
    for i in 0..m {
        for j in 0..m {
            out[(2 * i) * n + 2 * j] = coarse[i * m + j];
        }
    }
    for i in 0..m - 1 {
        for j in 0..m {
            out[(2 * i + 1) * n + 2 * j] = 0.5 * (coarse[i * m + j] + coarse[(i + 1) * m + j]);
        }
    }
    for i in 0..m {
        for j in 0..m - 1 {
            out[(2 * i) * n + 2 * j + 1] = 0.5 * (coarse[i * m + j] + coarse[i * m + j + 1]);
        }
    }
    for i in 0..m - 1 {
        for j in 0..m - 1 {
            let s = coarse[i * m + j]
                + coarse[(i + 1) * m + j]
                + coarse[i * m + j + 1]
                + coarse[(i + 1) * m + j + 1];
            out[(2 * i + 1) * n + 2 * j + 1] = 0.25 * s;
        }
    }
    out
}

#[cfg(test)]
#[path = "pde_grid_tests.rs"]
mod tests;

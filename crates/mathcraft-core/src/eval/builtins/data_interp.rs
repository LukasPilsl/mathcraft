//! Interpolation and prediction (Mathcad Prime help: "Interpolation and Prediction").
//!
//! - `linterp(vx, vy, x)`: linear interpolation; beyond the data, the end segments are extended.
//! - `cspline`, `lspline`, `pspline` (vx, vy) build a spline vector `vs`; `interp(vs, vx, vy, x)`
//!   evaluates it at `x`.
//! - `predict(v, m, n)`: linear prediction by Burg's method (help: "Linear Prediction").
//!
//! Spline vector layout (this crate's own; Prime's internal layout is not documented): `vs` has
//! `n + 3` entries `[kind, n, 0, s_0 .. s_{n-1}]`. `kind` is 1 (cspline), 2 (lspline) or 3
//! (pspline); `s_i` are the second derivatives at the data points, of dimension `dy / dx^2`. The
//! first three entries are the ones `interp` checks, as the help describes.
//!
//! End conditions (help: "Cubic Spline Interpolation"): the second derivative at each end is
//! `s_0 = 0` for lspline (linear extrapolation), `s_0 = s_1` for pspline (parabolic) and
//! `s_0 = 2 s_1 - s_2` for cspline (cubic: the straight line through the two nearest points). The
//! right end is symmetric. A cspline needs four points.
//!
//! Not implemented: the two-dimensional spline forms (`Mxy`, `Mz`), `bspline`, `loess`.

use num_complex::Complex64;

use super::data_util::{
    column, complexes, count, grid, one_dim, real_of, reals, scalar_result, vector,
};
use super::{Arity, Registry};
use crate::eval::error::{from_units, EvalError};
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    r.register("linterp", Arity::Exact(3), linterp);
    r.register("cspline", Arity::Exact(2), cspline);
    r.register("lspline", Arity::Exact(2), lspline);
    r.register("pspline", Arity::Exact(2), pspline);
    r.register("interp", Arity::Exact(4), interp);
    r.register("predict", Arity::Exact(3), predict);
}

/// Validated data of one interpolation: strictly increasing real abscissae, the ordinates, and the
/// dimensions of both.
struct Knots {
    x: Vec<f64>,
    y: Vec<Complex64>,
    dx: Dimension,
    dy: Dimension,
}

/// Reads `vx` and `vy`. The ordinates are real when `real_y` (the splines), complex otherwise.
fn knots(vx: &Value, vy: &Value, name: &str, real_y: bool) -> Result<Knots, EvalError> {
    let xs = vector(vx, name)?;
    let ys = vector(vy, name)?;
    if xs.len() != ys.len() {
        return Err(EvalError::Unsupported(format!(
            "{name}: vx and vy differ in length"
        )));
    }
    if xs.len() < 2 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs at least two points"
        )));
    }
    let dx = one_dim(&xs)?;
    let dy = one_dim(&ys)?;
    let x = reals(&xs, name)?;
    if x.windows(2).any(|w| w[1] <= w[0]) {
        return Err(EvalError::Unsupported(format!(
            "{name}: vx must be strictly increasing"
        )));
    }
    let y = if real_y {
        reals(&ys, name)?
            .into_iter()
            .map(|r| Complex64::new(r, 0.0))
            .collect()
    } else {
        complexes(&ys)?
    };
    Ok(Knots { x, y, dx, dy })
}

/// Evaluates `f` at each point of `x` (a scalar or any matrix shape). Every point must have the
/// dimension `dx`; the result has dimension `dy` and the shape of `x`.
fn at_points(
    x: &Value,
    dx: Dimension,
    dy: Dimension,
    name: &str,
    f: impl Fn(f64) -> Complex64,
) -> Result<Value, EvalError> {
    let (rows, cols, data) = grid(x, name)?;
    let mut out = Vec::with_capacity(data.len());
    for s in &data {
        if s.dim != dx {
            return Err(EvalError::DimensionMismatch {
                left: dx,
                right: s.dim,
            });
        }
        let q = real_of(s, name)?;
        out.push(f(q));
    }
    if let Value::Scalar(_) = x {
        return scalar_result(out[0], dy);
    }
    super::data_util::grid_result(rows, cols, &out, dy)
}

/// The index `i` of the segment `[x_i, x_{i+1}]` used for `q`: the last knot at or below `q`,
/// clamped to the first and last segment for `q` outside the data.
fn segment(x: &[f64], q: f64) -> usize {
    let at_or_below = x.partition_point(|&v| v <= q);
    at_or_below.saturating_sub(1).min(x.len() - 2)
}

/// Straight-line interpolation at `q`.
fn lerp(k: &Knots, q: f64) -> Complex64 {
    let i = segment(&k.x, q);
    let t = (q - k.x[i]) / (k.x[i + 1] - k.x[i]);
    k.y[i] + (k.y[i + 1] - k.y[i]) * t
}

/// `linterp(vx, vy, x)`.
fn linterp(args: &[Value]) -> Result<Value, EvalError> {
    let name = "linterp";
    let k = knots(&args[0], &args[1], name, false)?;
    at_points(&args[2], k.dx, k.dy, name, |q| lerp(&k, q))
}

/// The cubic piece of the spline with second derivatives `s` on the segment containing `q`.
fn spline_at(x: &[f64], y: &[Complex64], s: &[f64], q: f64) -> Complex64 {
    let i = segment(x, q);
    let h = x[i + 1] - x[i];
    let a = (x[i + 1] - q) / h;
    let b = (q - x[i]) / h;
    let correction = ((a * a * a - a) * s[i] + (b * b * b - b) * s[i + 1]) * h * h / 6.0;
    y[i] * a + y[i + 1] * b + Complex64::new(correction, 0.0)
}

/// The end kinds of the three spline functions.
#[derive(Clone, Copy, PartialEq)]
enum End {
    Cubic,
    Linear,
    Parabolic,
}

impl End {
    /// The code stored at `vs[0]`.
    fn code(self) -> f64 {
        match self {
            End::Cubic => 1.0,
            End::Linear => 2.0,
            End::Parabolic => 3.0,
        }
    }

    /// The left end relation `s_0 = a1 s_1 + a2 s_2`, with spacings `h0 = x_1 - x_0` and
    /// `h1 = x_2 - x_1`. A cspline continues the straight line through `(x_1, s_1)` and `(x_2, s_2)`
    /// to `x_0`.
    fn left(self, h0: f64, h1: f64) -> (f64, f64) {
        match self {
            End::Cubic => (1.0 + h0 / h1, -h0 / h1),
            End::Linear => (0.0, 0.0),
            End::Parabolic => (1.0, 0.0),
        }
    }

    /// The right end relation `s_{n-1} = b1 s_{n-2} + b2 s_{n-3}`, with `hl = x_{n-1} - x_{n-2}` and
    /// `hp = x_{n-2} - x_{n-3}` (the mirror image of [`End::left`]).
    fn right(self, hl: f64, hp: f64) -> (f64, f64) {
        match self {
            End::Cubic => (1.0 + hl / hp, -hl / hp),
            End::Linear => (0.0, 0.0),
            End::Parabolic => (1.0, 0.0),
        }
    }
}

/// `cspline(vx, vy)`.
fn cspline(args: &[Value]) -> Result<Value, EvalError> {
    spline_vector(&args[0], &args[1], End::Cubic, "cspline")
}

/// `lspline(vx, vy)`.
fn lspline(args: &[Value]) -> Result<Value, EvalError> {
    spline_vector(&args[0], &args[1], End::Linear, "lspline")
}

/// `pspline(vx, vy)`.
fn pspline(args: &[Value]) -> Result<Value, EvalError> {
    spline_vector(&args[0], &args[1], End::Parabolic, "pspline")
}

/// Builds the `vs` vector of a spline (see the module documentation).
fn spline_vector(vx: &Value, vy: &Value, end: End, name: &str) -> Result<Value, EvalError> {
    let k = knots(vx, vy, name, true)?;
    let n = k.x.len();
    if end == End::Cubic && n < 4 {
        return Err(EvalError::Unsupported(
            "cspline needs at least four points".to_string(),
        ));
    }
    let y: Vec<f64> = k.y.iter().map(|z| z.re).collect();
    let s = second_derivatives(&k.x, &y, end)?;
    let sdim =
        k.dy.checked_div(k.dx)
            .and_then(|d| d.checked_div(k.dx))
            .map_err(from_units)?;
    let dimless = Dimension::dimensionless();
    let mut data = vec![
        Scalar::real(end.code(), dimless),
        Scalar::real(n as f64, dimless),
        Scalar::real(0.0, dimless),
    ];
    data.extend(s.iter().map(|&v| Scalar::real(v, sdim)));
    Ok(Value::Matrix(Matrix::new(data.len(), 1, data)?))
}

/// Substitutes an end unknown in one term of a knot equation. A term in column `0` (the left end
/// `s_0 = a1 s_1 + a2 s_2`) or column `n-1` (the right end `s_{n-1} = b1 s_{n-2} + b2 s_{n-3}`) is
/// replaced by the interior unknowns, so the knot equations involve only `s_1 .. s_{n-2}`.
fn resolve(
    col: usize,
    coef: f64,
    n: usize,
    left: (f64, f64),
    right: (f64, f64),
) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    if col == 0 {
        if left.0 != 0.0 {
            out.push((1, coef * left.0));
        }
        if left.1 != 0.0 {
            out.push((2, coef * left.1));
        }
    } else if col == n - 1 {
        if right.0 != 0.0 {
            out.push((n - 2, coef * right.0));
        }
        if right.1 != 0.0 {
            out.push((n - 3, coef * right.1));
        }
    } else {
        out.push((col, coef));
    }
    out
}

/// Second derivatives `s_0 .. s_{n-1}` of the spline through `(x, y)` with the given end kind.
/// Interior knots satisfy `h_{i-1} s_{i-1} + 2 (h_{i-1} + h_i) s_i + h_i s_{i+1} = 6 (d_i - d_{i-1})`
/// with `h_i = x_{i+1} - x_i` and `d_i = (y_{i+1} - y_i) / h_i`.
fn second_derivatives(x: &[f64], y: &[f64], end: End) -> Result<Vec<f64>, EvalError> {
    let n = x.len();
    let mut s = vec![0.0; n];
    if n == 2 {
        return Ok(s);
    }
    let h: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let d: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
    let left = end.left(h[0], h[1]);
    let right = end.right(h[n - 2], h[n - 3]);
    let m = n - 2;
    let mut sub = vec![0.0; m];
    let mut diag = vec![0.0; m];
    let mut sup = vec![0.0; m];
    let mut rhs = vec![0.0; m];
    for i in 1..=n - 2 {
        let u = i - 1;
        rhs[u] = 6.0 * (d[i] - d[i - 1]);
        let terms = [
            (i - 1, h[i - 1]),
            (i, 2.0 * (h[i - 1] + h[i])),
            (i + 1, h[i]),
        ];
        for (col, coef) in terms {
            for (c, mult) in resolve(col, coef, n, left, right) {
                if c == i {
                    diag[u] += mult;
                } else if c + 1 == i {
                    sub[u] += mult;
                } else if c == i + 1 {
                    sup[u] += mult;
                } else {
                    return Err(EvalError::Unsupported("spline band".to_string()));
                }
            }
        }
    }
    let interior = solve_tridiagonal(&sub, &diag, &sup, &rhs)?;
    for (u, value) in interior.iter().enumerate() {
        s[u + 1] = *value;
    }
    s[0] = left.0 * s[1] + left.1 * s[2];
    s[n - 1] = right.0 * s[n - 2] + right.1 * s[n - 3];
    if s.iter().any(|v| !v.is_finite()) {
        return Err(EvalError::NotFinite);
    }
    Ok(s)
}

/// Thomas algorithm for a tridiagonal system: `sub[u]` multiplies `x[u-1]`, `sup[u]` multiplies
/// `x[u+1]` in row `u`.
fn solve_tridiagonal(
    sub: &[f64],
    diag: &[f64],
    sup: &[f64],
    rhs: &[f64],
) -> Result<Vec<f64>, EvalError> {
    let m = diag.len();
    let mut cp = vec![0.0; m];
    let mut dp = vec![0.0; m];
    let mut denom = diag[0];
    if denom == 0.0 || !denom.is_finite() {
        return Err(EvalError::Singular);
    }
    cp[0] = sup[0] / denom;
    dp[0] = rhs[0] / denom;
    for u in 1..m {
        denom = diag[u] - sub[u] * cp[u - 1];
        if denom == 0.0 || !denom.is_finite() {
            return Err(EvalError::Singular);
        }
        cp[u] = sup[u] / denom;
        dp[u] = (rhs[u] - sub[u] * dp[u - 1]) / denom;
    }
    let mut x = vec![0.0; m];
    x[m - 1] = dp[m - 1];
    for u in (0..m - 1).rev() {
        x[u] = dp[u] - cp[u] * x[u + 1];
    }
    Ok(x)
}

/// `interp(vs, vx, vy, x)`.
fn interp(args: &[Value]) -> Result<Value, EvalError> {
    let name = "interp";
    let k = knots(&args[1], &args[2], name, true)?;
    let n = k.x.len();
    let vs = reals(&vector(&args[0], name)?, name)?;
    let kind_ok = matches!(vs.first(), Some(&c) if c == 1.0 || c == 2.0 || c == 3.0);
    if !kind_ok || vs.len() != n + 3 || vs[1] != n as f64 {
        return Err(EvalError::Unsupported(
            "interp: vs is not a spline vector for vx and vy".to_string(),
        ));
    }
    let s = &vs[3..];
    at_points(&args[3], k.dx, k.dy, name, |q| spline_at(&k.x, &k.y, s, q))
}

/// Burg's method: the prediction-error filter `a[0..=m]` (with `a[0] = 1`) of order `m` for the data
/// `x`, so that `x[t] + sum_i a[i] x[t-i]` is the forward prediction error. Requires `m < x.len()`.
fn burg(x: &[f64], m: usize) -> Vec<f64> {
    let n = x.len();
    let mut f = x.to_vec();
    let mut b = x.to_vec();
    let mut a = vec![0.0; m + 1];
    a[0] = 1.0;
    for k in 1..=m {
        let (mut num, mut den) = (0.0, 0.0);
        for j in k..n {
            num += f[j] * b[j - 1];
            den += f[j] * f[j] + b[j - 1] * b[j - 1];
        }
        let kappa = if den > 0.0 { -2.0 * num / den } else { 0.0 };
        let prev = a.clone();
        for i in 1..k {
            a[i] = prev[i] + kappa * prev[k - i];
        }
        a[k] = kappa;
        for j in (k..n).rev() {
            let (fo, bo) = (f[j], b[j - 1]);
            f[j] = fo + kappa * bo;
            b[j] = bo + kappa * fo;
        }
    }
    a
}

/// `predict(v, m, n)`: the next `n` values after `v`, predicted from the last `m` values with the
/// weights of Burg's method fitted to all of `v`.
fn predict(args: &[Value]) -> Result<Value, EvalError> {
    let name = "predict";
    let xs = vector(&args[0], name)?;
    let dim = one_dim(&xs)?;
    let values = reals(&xs, name)?;
    let m = count(&args[1], name)?;
    let steps = count(&args[2], name)?;
    let len = values.len();
    if m >= len {
        return Err(EvalError::Unsupported(
            "predict: m must be smaller than the number of data points".to_string(),
        ));
    }
    let a = burg(&values, m);
    let mut ext = values;
    for _ in 0..steps {
        let t = ext.len();
        let next: f64 = (1..=m).map(|i| -a[i] * ext[t - i]).sum();
        ext.push(next);
    }
    let out: Vec<Complex64> = ext[len..].iter().map(|&r| Complex64::new(r, 0.0)).collect();
    column(&out, dim)
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;
    use super::*;

    type Constructor = fn(&[Value]) -> Result<Value, EvalError>;

    fn run(
        f: fn(&[Value]) -> Result<Value, EvalError>,
        args: &[Value],
    ) -> Result<Value, EvalError> {
        f(args)
    }

    fn ramp_knots() -> (Vec<f64>, Vec<f64>) {
        let x = vec![0.0, 0.5, 1.5, 2.0, 3.25, 4.0, 5.5];
        let y = x.iter().map(|&t| t * t * t - 2.0 * t + 1.0).collect();
        (x, y)
    }

    #[test]
    fn linterp_inside_and_outside_the_data() {
        let vx = col(&[0.0, 1.0, 3.0]);
        let vy = col(&[0.0, 2.0, 6.0]);
        let v = run(linterp, &[vx.clone(), vy.clone(), num(0.5)]).expect("inside");
        assert_all_close(&values(&v), &[1.0], 1e-12);
        let v = run(linterp, &[vx.clone(), vy.clone(), num(2.0)]).expect("inside");
        assert_all_close(&values(&v), &[4.0], 1e-12);
        // Beyond the ends the end segments continue: slope -2 before 0, slope 2 after 3.
        let v = run(linterp, &[vx.clone(), vy.clone(), num(-1.0)]).expect("left");
        assert_all_close(&values(&v), &[-2.0], 1e-12);
        let v = run(linterp, &[vx, vy, num(4.0)]).expect("right");
        assert_all_close(&values(&v), &[8.0], 1e-12);
    }

    #[test]
    fn linterp_keeps_the_shape_of_a_vector_argument() {
        let vx = col(&[0.0, 1.0]);
        let vy = col(&[0.0, 10.0]);
        let v = run(linterp, &[vx, vy, col(&[0.25, 0.5, 0.75])]).expect("vector x");
        assert_all_close(&values(&v), &[2.5, 5.0, 7.5], 1e-12);
    }

    #[test]
    fn linterp_refuses_bad_data() {
        let err = run(linterp, &[col(&[0.0, 0.0]), col(&[1.0, 2.0]), num(0.0)]).unwrap_err();
        assert!(matches!(err, EvalError::Unsupported(_)), "{err:?}");
        let err = run(linterp, &[col(&[0.0]), col(&[1.0]), num(0.0)]).unwrap_err();
        assert!(matches!(err, EvalError::Unsupported(_)), "{err:?}");
        let err = run(
            linterp,
            &[col(&[0.0, 1.0]), col(&[1.0, 2.0]), num(f64::NAN)],
        )
        .unwrap_err();
        assert!(matches!(err, EvalError::NotFinite), "{err:?}");
    }

    fn spline(f: fn(&[Value]) -> Result<Value, EvalError>, x: &[f64], y: &[f64]) -> Value {
        run(f, &[col(x), col(y)]).expect("spline vector")
    }

    fn eval_at(vs: &Value, x: &[f64], y: &[f64], q: f64) -> f64 {
        let v = run(interp, &[vs.clone(), col(x), col(y), num(q)]).expect("interp");
        values(&v)[0]
    }

    #[test]
    fn splines_pass_through_their_knots() {
        let x = [0.0, 1.0, 2.5, 4.0, 5.0];
        let y = [1.0, -2.0, 0.5, 3.0, 2.0];
        let fns: [Constructor; 3] = [cspline, lspline, pspline];
        for f in fns {
            let vs = spline(f, &x, &y);
            for (&xi, &yi) in x.iter().zip(&y) {
                close_enough(eval_at(&vs, &x, &y, xi), yi);
            }
        }
    }

    fn close_enough(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{a} is not {b}");
    }

    #[test]
    fn cspline_reproduces_a_cubic_exactly() {
        let (x, y) = ramp_knots();
        let vs = spline(cspline, &x, &y);
        for q in [0.25, 1.0, 2.7, 3.9, 5.0] {
            let want = q * q * q - 2.0 * q + 1.0;
            assert!((eval_at(&vs, &x, &y, q) - want).abs() < 1e-9, "at {q}");
        }
        // Extrapolation continues the cubic on both ends.
        let want = (-1.0_f64).powi(3) - -2.0 + 1.0;
        assert!((eval_at(&vs, &x, &y, -1.0) - want).abs() < 1e-8);
    }

    #[test]
    fn pspline_reproduces_a_quadratic_and_lspline_a_line() {
        let x = [0.0, 1.0, 2.0, 3.5, 5.0];
        let quad: Vec<f64> = x.iter().map(|&t| t * t - 3.0 * t + 2.0).collect();
        let vs = spline(pspline, &x, &quad);
        assert!(
            (eval_at(&vs, &x, &quad, 2.75) - (2.75f64.powi(2) - 3.0 * 2.75 + 2.0)).abs() < 1e-9
        );
        let line: Vec<f64> = x.iter().map(|&t| 4.0 * t - 1.0).collect();
        let vs = spline(lspline, &x, &line);
        assert!((eval_at(&vs, &x, &line, 4.2) - (4.0 * 4.2 - 1.0)).abs() < 1e-12);
    }

    #[test]
    fn spline_vector_layout_and_its_second_derivative_ends() {
        let x = [0.0, 1.0, 2.0, 3.0, 4.0];
        let y = [0.0, 1.0, 0.0, 1.0, 0.0];
        let vs = values(&spline(pspline, &x, &y));
        assert_eq!(vs.len(), x.len() + 3);
        close_enough(vs[0], 3.0);
        close_enough(vs[1], 5.0);
        // pspline: the end second derivatives equal their neighbours.
        assert!((vs[3] - vs[4]).abs() < 1e-12);
        assert!((vs[7] - vs[6]).abs() < 1e-12);
        let c = values(&spline(cspline, &x, &y));
        close_enough(c[0], 1.0);
        // cspline: s_0 = 2 s_1 - s_2.
        assert!((c[3] - (2.0 * c[4] - c[5])).abs() < 1e-12);
    }

    #[test]
    fn cspline_needs_four_points() {
        let err = run(cspline, &[col(&[0.0, 1.0, 2.0]), col(&[0.0, 1.0, 4.0])]).unwrap_err();
        assert!(matches!(err, EvalError::Unsupported(_)), "{err:?}");
    }

    #[test]
    fn interp_refuses_a_vector_of_another_size() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [0.0, 1.0, 0.0, 1.0];
        let vs = spline(cspline, &x, &y);
        let err = run(interp, &[vs, col(&[0.0, 1.0]), col(&[0.0, 1.0]), num(0.5)]).unwrap_err();
        assert!(matches!(err, EvalError::Unsupported(_)), "{err:?}");
    }

    #[test]
    fn burg_weights_match_the_help_example() {
        // Help, "Example: Linear Prediction 1" (How predict Works): N_previous = 3.
        let x = [2.7, 5.7, 6.8, 2.9, 5.1, 6.2, 2.5];
        let a = burg(&x, 3);
        let c = [-a[3], -a[2], -a[1]];
        let want = [0.8814224392928995, -0.417638429300996, 0.5225643330159371];
        assert_all_close(&c, &want, 1e-9);
    }

    #[test]
    fn predict_reproduces_the_help_predictions() {
        let x = col(&[2.7, 5.7, 6.8, 2.9, 5.1, 6.2, 2.5]);
        let v = run(predict, &[x, num(3.0), num(4.0)]).expect("predict");
        let c = [0.8814224392928995, -0.417638429300996, 0.5225643330159371];
        let x = [2.7, 5.7, 6.8, 2.9, 5.1, 6.2, 2.5];
        let p0 = c[0] * x[4] + c[1] * x[5] + c[2] * x[6];
        let p1 = c[0] * x[5] + c[1] * x[6] + c[2] * p0;
        let p2 = c[0] * x[6] + c[1] * p0 + c[2] * p1;
        let p3 = c[0] * p0 + c[1] * p1 + c[2] * p2;
        assert_all_close(&values(&v), &[p0, p1, p2, p3], 1e-9);
    }

    #[test]
    fn predict_continues_a_pure_sinusoid() {
        // Burg's estimate has a boundary bias of order 1/N (about 4e-4 at N = 400, 4e-5 at N = 4000),
        // and the undamped recursion carries it forward (about 6e-4 after five steps), so the record
        // is long and two steps are checked.
        let w = 0.3_f64;
        let data: Vec<f64> = (0..4000).map(|t| (w * f64::from(t)).cos()).collect();
        let v = run(predict, &[col(&data), num(2.0), num(2.0)]).expect("predict");
        let want: Vec<f64> = (4000..4002).map(|t| (w * f64::from(t)).cos()).collect();
        assert_all_close(&values(&v), &want, 5e-4);
    }

    #[test]
    fn predict_order_limits_follow_the_help() {
        let data = col(&[2.7, 8.2, 1.9, 0.3, 2.7]);
        assert!(matches!(
            run(predict, &[data.clone(), num(5.0), num(3.0)]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(run(predict, &[data, num(4.0), num(3.0)]).is_ok());
    }
}

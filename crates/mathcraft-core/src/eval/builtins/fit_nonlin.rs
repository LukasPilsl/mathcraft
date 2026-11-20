//! Nonlinear least-squares fits (help: Curve Fitting, Nonlinear Regression).
//!
//! - `genfit(vx, vy, vg, F)`: any model. `F` is a user function of `x` and the parameters: either
//!   `F(x, u)` with `u` the vector of parameters (its components are `u_0, u_1, ...` in the body),
//!   or `F(x, u_0, u_1, ...)` with one name per parameter. It returns the model value, or the
//!   model value followed by its `n` partial derivatives (then no numeric derivatives are taken).
//! - `expfit(vx, vy, [vg])`: `A e^(b x) + C`; `logfit(vx, vy, vg)`: `a ln(x + b) + c`;
//!   `lgsfit(vx, vy, vg)`: `a / (1 + b e^(-c x))`; `pwrfit(vx, vy, vg)`: `a x^b + c` (needs
//!   `x >= 0`); `sinfit(vx, vy, vg)`: `a sin(x + b) + c`. Each returns the coefficients as a
//!   column. `expfit` without `vg` starts from a line fitted to the logarithms of `vy` (or of `-vy`
//!   when every `y` is negative), and falls back to a rough bracket if that does not converge.
//!
//! All minimize the sum of squared residuals with the Levenberg-Marquardt method. A fit that does
//! not reach a minimum is an error, never a value. Every model evaluation of `genfit` is charged
//! to the statement's budget; the fixed-form fits are charged by their work-table class.

use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::expr_eval::{call_user_with, Ctx};
use crate::eval::special_forms::{
    column, named_function, real_value, reals_of, vector_of, MAX_UNKNOWNS, SAMPLE_UNITS,
};
use crate::eval::value::Value;
use crate::expr::Expr;

/// Iterations of the Levenberg-Marquardt loop.
const MAX_ITERATIONS: usize = 200;

/// Increases of the damping per iteration before the step search gives up.
const MAX_DAMPING_STEPS: usize = 60;

/// Largest damping; past it no step decreases the sum of squares.
const MAX_LAMBDA: f64 = 1e16;

/// A model at one `x`: its value and, when the model supplies it, its gradient in the parameters.
pub(in crate::eval) type Model<'m> =
    dyn FnMut(f64, &[f64]) -> Result<(f64, Option<Vec<f64>>), EvalError> + 'm;

/// A fixed-form model: its value and gradient at one `x`, for the parameters given.
type FixedModel = dyn Fn(f64, &[f64]) -> (f64, Vec<f64>);

pub(super) fn register(r: &mut Registry) {
    r.register("expfit", Arity::Range(2, 3), expfit);
    r.register("logfit", Arity::Exact(3), logfit);
    r.register("lgsfit", Arity::Exact(3), lgsfit);
    r.register("pwrfit", Arity::Exact(3), pwrfit);
    r.register("sinfit", Arity::Exact(3), sinfit);
}

/// `genfit(vx, vy, vg, F)` (see the module documentation).
pub(in crate::eval) fn genfit_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [vx, vy, vg, f] = args else {
        return Err(EvalError::Unsupported(
            "genfit takes four arguments".to_string(),
        ));
    };
    let xs = reals_of(&vector_of(vx, ctx)?, "the x data")?;
    let ys = reals_of(&vector_of(vy, ctx)?, "the y data")?;
    let guess_value = crate::eval::special_forms::value_of(vg, ctx)?;
    let guess = reals_of(&guess_value, "the guess values")?;
    let n = guess.len();
    if n == 0 || n > MAX_UNKNOWNS {
        return Err(EvalError::Unsupported(format!(
            "genfit takes between 1 and {MAX_UNKNOWNS} parameters"
        )));
    }
    let Some((name, func)) = named_function(f, ctx)? else {
        return Err(EvalError::Unsupported(
            "the fitting function must be the name of a function F(x, u)".to_string(),
        ));
    };
    // `F(x, u)` takes the parameter vector; a scalar guess with two parameters (one unknown) is
    // the one-name form `F(x, A)`.
    let vector_mode = func.params.len() == 2 && (n >= 2 || matches!(guess_value, Value::Matrix(_)));
    if !vector_mode && func.params.len() != n + 1 {
        return Err(EvalError::ArgCount {
            name,
            expected: (n + 1).to_string(),
            found: func.params.len(),
        });
    }
    let param_name = if vector_mode {
        func.params[1].clone()
    } else {
        String::new()
    };
    let mut model = |x: f64, p: &[f64]| -> Result<(f64, Option<Vec<f64>>), EvalError> {
        ctx.budget.charge(SAMPLE_UNITS)?;
        let (call_args, extra) = if vector_mode {
            let state = column(p)?;
            let extra: Vec<(String, Value)> = p
                .iter()
                .enumerate()
                .map(|(k, v)| (format!("{param_name}_{k}"), real_value(*v)))
                .collect();
            (vec![real_value(x), state], extra)
        } else {
            let mut values = vec![real_value(x)];
            values.extend(p.iter().map(|v| real_value(*v)));
            (values, Vec::new())
        };
        let out = call_user_with(&name, func, call_args, &extra, ctx)?;
        let vals = reals_of(&out, "the fitting function F")?;
        match vals.len() {
            1 => Ok((vals[0], None)),
            k if k == n + 1 => Ok((vals[0], Some(vals[1..].to_vec()))),
            _ => Err(EvalError::Unsupported(format!(
                "F must return the model, or the model and its {n} derivatives"
            ))),
        }
    };
    let p = levenberg_marquardt("genfit", &xs, &ys, &guess, &mut model)?;
    crate::eval::special_forms::column(&p)
}

/// Fixed-form fits take the data and the guess as values (plain builtins, see `register`).
fn expfit(args: &[Value]) -> Result<Value, EvalError> {
    let (xs, ys) = data(&args[0], &args[1])?;
    let guesses = match args.get(2) {
        Some(v) => vec![parameters(v, 3)?],
        None => expfit_guesses(&xs, &ys),
    };
    // The first starting point that converges is the answer; only a non-convergence moves on.
    let mut last = None;
    for guess in &guesses {
        match fixed("expfit", &xs, &ys, guess, &exp_model) {
            Ok(v) => return Ok(v),
            Err(e @ EvalError::NoConvergence(_)) => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| EvalError::NoConvergence("expfit".to_string())))
}

fn logfit(args: &[Value]) -> Result<Value, EvalError> {
    let (xs, ys) = data(&args[0], &args[1])?;
    fixed("logfit", &xs, &ys, &parameters(&args[2], 3)?, &log_model)
}

fn lgsfit(args: &[Value]) -> Result<Value, EvalError> {
    let (xs, ys) = data(&args[0], &args[1])?;
    fixed(
        "lgsfit",
        &xs,
        &ys,
        &parameters(&args[2], 3)?,
        &logistic_model,
    )
}

fn pwrfit(args: &[Value]) -> Result<Value, EvalError> {
    let (xs, ys) = data(&args[0], &args[1])?;
    if xs.iter().any(|x| *x < 0.0) {
        return Err(EvalError::Unsupported(
            "pwrfit needs x values of 0 or more".to_string(),
        ));
    }
    fixed("pwrfit", &xs, &ys, &parameters(&args[2], 3)?, &power_model)
}

fn sinfit(args: &[Value]) -> Result<Value, EvalError> {
    let (xs, ys) = data(&args[0], &args[1])?;
    fixed("sinfit", &xs, &ys, &parameters(&args[2], 3)?, &sine_model)
}

/// The data vectors: real, of equal length, with at least three points for every fixed form.
fn data(vx: &Value, vy: &Value) -> Result<(Vec<f64>, Vec<f64>), EvalError> {
    let xs = reals_of(vx, "the x data")?;
    let ys = reals_of(vy, "the y data")?;
    if xs.len() != ys.len() {
        return Err(EvalError::DataLength {
            expected: xs.len(),
            found: ys.len(),
        });
    }
    if xs.len() < 3 {
        return Err(EvalError::Unsupported(
            "a curve fit needs at least three data points".to_string(),
        ));
    }
    Ok((xs, ys))
}

/// A guess vector of exactly `count` real values.
fn parameters(v: &Value, count: usize) -> Result<Vec<f64>, EvalError> {
    let values = reals_of(v, "the guess values")?;
    if values.len() != count {
        return Err(EvalError::DataLength {
            expected: count,
            found: values.len(),
        });
    }
    Ok(values)
}

/// Runs the fixed model and returns its coefficients as a column.
fn fixed(
    name: &str,
    xs: &[f64],
    ys: &[f64],
    guess: &[f64],
    model: &FixedModel,
) -> Result<Value, EvalError> {
    let mut eval = |x: f64, p: &[f64]| -> Result<(f64, Option<Vec<f64>>), EvalError> {
        let (value, grad) = model(x, p);
        Ok((value, Some(grad)))
    };
    let p = levenberg_marquardt(name, xs, ys, guess, &mut eval)?;
    column(&p)
}

/// `A e^(bx) + C` with value and gradient `[e^(bx), A x e^(bx), 1]`.
fn exp_model(x: f64, p: &[f64]) -> (f64, Vec<f64>) {
    let e = (p[1] * x).exp();
    (p[0] * e + p[2], vec![e, p[0] * x * e, 1.0])
}

/// `a ln(x + b) + c`, gradient `[ln(x + b), a / (x + b), 1]`. Undefined where `x + b <= 0`.
fn log_model(x: f64, p: &[f64]) -> (f64, Vec<f64>) {
    let t = x + p[1];
    if t <= 0.0 {
        return (f64::NAN, vec![f64::NAN; 3]);
    }
    (p[0] * t.ln() + p[2], vec![t.ln(), p[0] / t, 1.0])
}

/// `a / (1 + b e^(-cx))`.
fn logistic_model(x: f64, p: &[f64]) -> (f64, Vec<f64>) {
    let e = (-p[2] * x).exp();
    let d = 1.0 + p[1] * e;
    (
        p[0] / d,
        vec![1.0 / d, -p[0] * e / (d * d), p[0] * p[1] * x * e / (d * d)],
    )
}

/// `a x^b + c` for `x >= 0`; the derivative in `b` is taken as 0 at `x = 0` (where `x^b` is
/// independent of `b` for the fits' purposes).
fn power_model(x: f64, p: &[f64]) -> (f64, Vec<f64>) {
    let xb = x.powf(p[1]);
    let dlog = if x > 0.0 { p[0] * xb * x.ln() } else { 0.0 };
    (p[0] * xb + p[2], vec![xb, dlog, 1.0])
}

/// `a sin(x + b) + c`, gradient `[sin(x + b), a cos(x + b), 1]`.
fn sine_model(x: f64, p: &[f64]) -> (f64, Vec<f64>) {
    let t = x + p[1];
    (p[0] * t.sin() + p[2], vec![t.sin(), p[0] * t.cos(), 1.0])
}

/// The `expfit` starting points, in the order tried: the line through `(x, ln y)` when every `y` is
/// positive, the same line through `(x, ln(-y))` (negated back) when every `y` is negative, then a
/// rough bracket of the data, which is always there as the last resort.
fn expfit_guesses(xs: &[f64], ys: &[f64]) -> Vec<Vec<f64>> {
    let mut guesses = Vec::new();
    if ys.iter().all(|y| *y > 0.0) {
        guesses.extend(log_line(xs, ys));
    } else if ys.iter().all(|y| *y < 0.0) {
        let negated: Vec<f64> = ys.iter().map(|y| -y).collect();
        guesses.extend(log_line(xs, &negated).map(|mut g| {
            g[0] = -g[0];
            g
        }));
    }
    guesses.push(bracket_guess(xs, ys));
    guesses
}

/// `[e^(intercept), slope, 0]` of the least-squares line through `(x, ln y)`, for positive `y`.
/// `None` when the `x` values are all equal.
fn log_line(xs: &[f64], ys: &[f64]) -> Option<Vec<f64>> {
    let m = xs.len() as f64;
    let (xm, lm) = (
        xs.iter().sum::<f64>() / m,
        ys.iter().map(|y| y.ln()).sum::<f64>() / m,
    );
    let sxx: f64 = xs.iter().map(|x| (x - xm).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let slope: f64 = xs
        .iter()
        .zip(ys)
        .map(|(x, y)| (x - xm) * (y.ln() - lm))
        .sum::<f64>()
        / sxx;
    Some(vec![(lm - slope * xm).exp(), slope, 0.0])
}

/// A rough bracket of the data: amplitude from the range of `y`, the rate from the span of `x`.
fn bracket_guess(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let lo = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - xs.iter().copied().fold(f64::INFINITY, f64::min);
    let amplitude = if hi > lo { hi - lo } else { 1.0 };
    vec![amplitude, if span > 0.0 { 1.0 / span } else { 1.0 }, lo]
}

/// Levenberg-Marquardt on `sum (model(x_i, p) - y_i)^2`, from `guess`. The Jacobian is the
/// model's gradient when it supplies one, and central differences otherwise. Returns the
/// parameters at the minimum; an unconverged or non-finite start is an error.
pub(in crate::eval) fn levenberg_marquardt(
    name: &str,
    xs: &[f64],
    ys: &[f64],
    guess: &[f64],
    model: &mut Model<'_>,
) -> Result<Vec<f64>, EvalError> {
    let n = guess.len();
    let m = xs.len();
    if n == 0 || n > MAX_UNKNOWNS || ys.len() != m || m < n {
        return Err(EvalError::Unsupported(format!(
            "{name} needs data vectors of equal length, with at least as many points as parameters"
        )));
    }
    let mut p = guess.to_vec();
    let (mut r, mut jac) = linearize(xs, ys, &p, model)?;
    let mut ssr = sum_squares(&r);
    if !ssr.is_finite() {
        return Err(EvalError::NotFinite);
    }
    let mut lambda = 1e-3;
    for _ in 0..MAX_ITERATIONS {
        if ssr == 0.0 {
            return Ok(p);
        }
        let (ata, atr) = normal_equations(&jac, &r, n);
        if atr.iter().all(|g| g.abs() == 0.0) {
            return Ok(p);
        }
        let mut step_accepted = None;
        for _ in 0..MAX_DAMPING_STEPS {
            let mut a = ata.clone();
            for k in 0..n {
                a[k][k] = ata[k][k] * (1.0 + lambda) + lambda * 1e-12;
            }
            let rhs: Vec<f64> = atr.iter().map(|g| -g).collect();
            let Some(delta) = solve_linear(a, rhs) else {
                lambda *= 10.0;
                continue;
            };
            let trial: Vec<f64> = (0..n).map(|k| p[k] + delta[k]).collect();
            let (r2, j2) = match linearize(xs, ys, &trial, model) {
                Ok(evaluated) => evaluated,
                // A trial point where the model is not finite: try a smaller step.
                Err(EvalError::NotFinite) => {
                    lambda *= 10.0;
                    continue;
                }
                // A budget or depth limit, or a model error, ends the fit: it is not a bad step.
                Err(e) => return Err(e),
            };
            let ssr2 = sum_squares(&r2);
            if ssr2 < ssr {
                let small_step = (0..n).all(|k| delta[k].abs() <= 1e-13 * (1.0 + p[k].abs()));
                step_accepted = Some((trial, r2, j2, ssr2, small_step));
                break;
            }
            lambda *= 10.0;
            if lambda > MAX_LAMBDA {
                break;
            }
        }
        match step_accepted {
            Some((trial, r2, j2, ssr2, done)) => {
                p = trial;
                r = r2;
                jac = j2;
                ssr = ssr2;
                lambda = (lambda / 10.0).max(1e-12);
                if done {
                    return Ok(p);
                }
            }
            None => {
                // No damped step lowers the sum of squares: a stationary point when the gradient
                // is numerically zero, otherwise the fit did not converge.
                let grad = atr.iter().fold(0.0_f64, |acc, g| acc.max(g.abs()));
                if grad <= 1e-8 * (1.0 + ssr) {
                    return Ok(p);
                }
                return Err(EvalError::NoConvergence(name.to_string()));
            }
        }
    }
    Err(EvalError::NoConvergence(name.to_string()))
}

/// Residuals `model(x_i) - y_i` and the Jacobian rows at `p`.
fn linearize(
    xs: &[f64],
    ys: &[f64],
    p: &[f64],
    model: &mut Model<'_>,
) -> Result<(Vec<f64>, Vec<Vec<f64>>), EvalError> {
    let mut residuals = Vec::with_capacity(xs.len());
    let mut jac = Vec::with_capacity(xs.len());
    for (x, y) in xs.iter().zip(ys) {
        let (value, grad) = model(*x, p)?;
        residuals.push(value - y);
        let row = match grad {
            Some(g) => g,
            None => numeric_gradient(*x, p, model)?,
        };
        if row.len() != p.len() {
            return Err(EvalError::Unsupported(
                "a model gradient must have one entry per parameter".to_string(),
            ));
        }
        jac.push(row);
    }
    Ok((residuals, jac))
}

/// Central differences of the model in each parameter.
fn numeric_gradient(x: f64, p: &[f64], model: &mut Model<'_>) -> Result<Vec<f64>, EvalError> {
    let mut grad = Vec::with_capacity(p.len());
    let mut q = p.to_vec();
    for k in 0..p.len() {
        let h = 1e-6 * p[k].abs().max(1e-2);
        q[k] = p[k] + h;
        let (up, _) = model(x, &q)?;
        q[k] = p[k] - h;
        let (down, _) = model(x, &q)?;
        q[k] = p[k];
        grad.push((up - down) / (2.0 * h));
    }
    Ok(grad)
}

fn sum_squares(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

/// `JᵀJ` and `Jᵀr`.
fn normal_equations(jac: &[Vec<f64>], r: &[f64], n: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut ata = vec![vec![0.0; n]; n];
    let mut atr = vec![0.0; n];
    for (row, res) in jac.iter().zip(r) {
        for i in 0..n {
            atr[i] += row[i] * res;
            for j in 0..n {
                ata[i][j] += row[i] * row[j];
            }
        }
    }
    (ata, atr)
}

/// Gaussian elimination with partial pivoting. `None` when the matrix is singular to working
/// precision (or the solution is not finite).
fn solve_linear(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    let scale = a.iter().flatten().fold(0.0_f64, |acc, v| acc.max(v.abs()));
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() <= 1e-14 * scale {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let pivot_row = a[col].clone();
        for row in col + 1..n {
            let factor = a[row][col] / pivot_row[col];
            for (value, pivot_value) in a[row].iter_mut().zip(&pivot_row).skip(col) {
                *value -= factor * pivot_value;
            }
            b[row] -= factor * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let tail: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budget_error_in_a_trial_step_ends_the_fit() {
        // The model fails with a budget error on its 11th evaluation: the first trial step of the
        // loop. That ends the fit; it is never taken as a stationary point.
        let xs: Vec<f64> = (0..10).map(f64::from).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 2.0 * x + 1.0).collect();
        let mut calls = 0usize;
        let mut model = |x: f64, p: &[f64]| {
            calls += 1;
            if calls > 10 {
                return Err(EvalError::WorkLimit(1));
            }
            Ok((p[0] + p[1] * x, Some(vec![1.0, x])))
        };
        let out = levenberg_marquardt("test", &xs, &ys, &[0.0, 0.0], &mut model);
        assert!(matches!(out, Err(EvalError::WorkLimit(_))), "{out:?}");
    }
}

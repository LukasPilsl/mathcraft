//! `polyfit(X, Y, n / "terms" / M)`: a multivariate polynomial regression that returns the fitted
//! function (help: "Multivariate Polynomial Regression", "Example: Second-Order Response Surface",
//! "Example: Quality of Fit"). The result is a function value (`Value::Func`), applied by a call:
//! `p := polyfit(X, Y, 1)`, then `p([t; fr])`.
//!
//! - `X` has one column per independent variable and one row per run. A design matrix with the `Run`
//!   and `Block` columns that `fullfact`, `boxwilson` and `doelabel` give is read without them: when
//!   the first column is the run number 1, 2, 3, ... and the second is one constant block number,
//!   those two columns are dropped (a model with an intercept could not use a constant column
//!   anyway).
//! - `Y` is a vector, or a matrix with one row per run and a replicate in each column. `NaN` pads
//!   the rows that have fewer replicates and is skipped.
//! - `n` is the order: the intercept and every term of total degree 1 to `n`. A string lists the
//!   terms by variable letters, `A` for the first column, `B` for the second, and so on (`"A B AB
//!   AA BB"`), with the intercept; the separators are a space, a comma, a colon or a semicolon. A
//!   matrix `M` has one row per term, a guess for the coefficient in the first column (not used:
//!   the fit is linear) and the exponent of each variable in the others; it has no intercept
//!   unless a row of zero exponents is given.
//!
//! The fit is the least squares of all the data points (every non-`NaN` element of `Y`), by the QR
//! of `matrix_linalg::least_squares`, with every variable divided by its largest magnitude so that
//! columns of very different size stay well conditioned. A model with more terms than data points,
//! or with dependent terms, is refused rather than fitted.
//!
//! The columns of `X` carry units: every element of a column must have the same units, and the
//! function wants the same units for its argument. The elements of `Y` share one unit, the result's.
//!
//! Not built: `polyfitc` and `polyfitstat` (their results mix text labels, matrices and numbers in
//! one array).

use super::matrix::expect_matrix;
use super::matrix_linalg::least_squares;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::func_value::{FuncValue, PolyModel, MAX_EXPONENT, MAX_TERMS};
use crate::eval::value::{Matrix, Scalar, Value};
use crate::eval::work;
use crate::units::Dimension;
use std::sync::Arc;

/// Largest design matrix (data points times terms) the fit builds.
const MAX_DESIGN_CELLS: usize = 1_000_000;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("polyfit", Arity::Exact(3), polyfit);
}

fn polyfit(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let x = expect_matrix(&args[0], "polyfit")?;
    let y = expect_matrix(&args[1], "polyfit")?;
    if x.rows() != y.rows() {
        return Err(EvalError::ShapeMismatch {
            left: x.shape(),
            right: y.shape(),
        });
    }
    ctx.charge((x.rows() * (x.cols() + y.cols())) as u64)?;
    let first = first_variable(x)?;
    let vars = x.cols() - first;
    // The intercept and one linear term for each variable are already `vars + 1` terms.
    if vars + 1 > MAX_TERMS {
        return Err(EvalError::TooLarge(vars + 1));
    }
    let (columns, x_dims) = variable_columns(x, first)?;
    let points = data_points(y)?;
    let terms = term_exponents(&args[2], vars, points.len())?;
    let term_count = terms.len() / vars;
    if term_count > points.len() {
        return Err(EvalError::Unsupported(
            "polyfit has more terms than data points".to_string(),
        ));
    }
    let cells = points.len().saturating_mul(term_count);
    if cells > MAX_DESIGN_CELLS {
        return Err(EvalError::TooLarge(cells));
    }
    // The QR of an m x p design costs about m p^2 multiply-adds.
    ctx.charge(work::cubic_units(
        (points.len() as u64).saturating_mul((term_count * term_count) as u64),
    ))?;
    let scales: Vec<f64> = columns
        .iter()
        .map(|c| {
            let m = c.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
            if m > 0.0 {
                m
            } else {
                1.0
            }
        })
        .collect();
    let mut design = Vec::with_capacity(cells);
    let mut observed = Vec::with_capacity(points.len());
    for (row, value) in &points {
        observed.push(Scalar::real(*value, Dimension::dimensionless()));
        for exps in terms.chunks(vars) {
            let mut v = 1.0;
            for (i, e) in exps.iter().enumerate() {
                v *= (columns[i][*row] / scales[i]).powi(*e as i32);
            }
            design.push(Scalar::real(v, Dimension::dimensionless()));
        }
    }
    let a = Matrix::new(points.len(), term_count, design)?;
    let b = Matrix::new(points.len(), 1, observed)?;
    let coefs = least_squares(&a, &b)?;
    let y_dim = y_dimension(y)?;
    Ok(Value::Func(Arc::new(FuncValue::Poly(PolyModel {
        vars,
        exponents: terms,
        coefs: coefs.iter().map(|c| c.re).collect(),
        scales,
        x_dims,
        y_dim,
    }))))
}

/// The first column of `X` that is an independent variable: 2 for a design matrix that begins with
/// the `Run` and `Block` columns (see the module documentation), else 0.
fn first_variable(x: &Matrix) -> Result<usize, EvalError> {
    if x.cols() < 3 {
        return Ok(0);
    }
    let mut block = None;
    for i in 0..x.rows() {
        let run = x.get(i, 0)?;
        let b = x.get(i, 1)?;
        if !run.dim.is_dimensionless() || run.value.im != 0.0 || run.value.re != (i + 1) as f64 {
            return Ok(0);
        }
        if !b.dim.is_dimensionless() || b.value.im != 0.0 {
            return Ok(0);
        }
        match block {
            None => block = Some(b.value.re),
            Some(v) if v != b.value.re => return Ok(0),
            Some(_) => {}
        }
    }
    Ok(2)
}

/// The variable columns of `X` as SI values, with the dimension of each. A column must have one
/// dimension, and real, finite elements.
#[allow(clippy::type_complexity)]
fn variable_columns(
    x: &Matrix,
    first: usize,
) -> Result<(Vec<Vec<f64>>, Vec<Dimension>), EvalError> {
    let mut columns = Vec::new();
    let mut dims = Vec::new();
    for j in first..x.cols() {
        let dim = x.get(0, j)?.dim;
        let mut column = Vec::with_capacity(x.rows());
        for i in 0..x.rows() {
            let s = x.get(i, j)?;
            if s.dim != dim {
                return Err(EvalError::DimensionMismatch {
                    left: dim,
                    right: s.dim,
                });
            }
            column.push(real_finite(s, "X")?);
        }
        columns.push(column);
        dims.push(dim);
    }
    Ok((columns, dims))
}

fn real_finite(s: &Scalar, what: &str) -> Result<f64, EvalError> {
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(format!(
            "polyfit needs real values in {what}"
        )));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// The observations: the row of `X` and the value of each element of `Y` that is not `NaN`.
fn data_points(y: &Matrix) -> Result<Vec<(usize, f64)>, EvalError> {
    let mut points = Vec::new();
    for i in 0..y.rows() {
        for j in 0..y.cols() {
            let s = y.get(i, j)?;
            if s.value.re.is_nan() || s.value.im.is_nan() {
                continue;
            }
            points.push((i, real_finite(s, "Y")?));
        }
    }
    if points.is_empty() {
        return Err(EvalError::Unsupported(
            "polyfit needs at least one data point".to_string(),
        ));
    }
    Ok(points)
}

/// The dimension shared by the observations of `Y` (`NaN` padding has none of its own).
fn y_dimension(y: &Matrix) -> Result<Dimension, EvalError> {
    let mut dim: Option<Dimension> = None;
    for s in y.data() {
        if s.value.re.is_nan() {
            continue;
        }
        match dim {
            None => dim = Some(s.dim),
            Some(d) if d != s.dim => {
                return Err(EvalError::DimensionMismatch {
                    left: d,
                    right: s.dim,
                })
            }
            Some(_) => {}
        }
    }
    Ok(dim.unwrap_or_else(Dimension::dimensionless))
}

/// The exponents of the terms, row-major (`vars` per term), from the third argument.
fn term_exponents(spec: &Value, vars: usize, points: usize) -> Result<Vec<u32>, EvalError> {
    let rows: Vec<Vec<u32>> = match spec {
        Value::Scalar(s) => {
            let n = crate::eval::env::integer_of(s)
                .filter(|_| s.dim.is_dimensionless())
                .ok_or_else(|| {
                    EvalError::Unsupported("the order of polyfit must be an integer".to_string())
                })?;
            if n < 1 || n as usize >= points {
                return Err(EvalError::Unsupported(
                    "the order of polyfit must be from 1 to one less than the data points"
                        .to_string(),
                ));
            }
            order_terms(vars, n as usize)?
        }
        Value::Str(text) => string_terms(text, vars)?,
        Value::Matrix(m) => matrix_terms(m, vars)?,
        _ => {
            return Err(EvalError::Unsupported(
                "polyfit needs an order, a string of terms or a matrix of terms".to_string(),
            ))
        }
    };
    if rows.is_empty() || rows.len() > MAX_TERMS {
        return Err(EvalError::TooLarge(rows.len()));
    }
    let mut seen = std::collections::HashSet::new();
    let mut flat = Vec::with_capacity(rows.len() * vars);
    for r in rows {
        if seen.insert(r.clone()) {
            flat.extend(r);
        }
    }
    Ok(flat)
}

/// The intercept and every combination of exponents of total degree 1 to `n`, by degree.
fn order_terms(vars: usize, n: usize) -> Result<Vec<Vec<u32>>, EvalError> {
    let mut rows = vec![vec![0; vars]];
    let mut level = vec![vec![0u32; vars]];
    for _ in 0..n {
        let mut next: Vec<Vec<u32>> = Vec::new();
        for t in &level {
            // Raise only variables from the last one raised onward, so each term appears once.
            let from = t.iter().rposition(|e| *e > 0).unwrap_or(0);
            for i in from..vars {
                let mut u = t.clone();
                u[i] += 1;
                next.push(u);
            }
        }
        if rows.len() + next.len() > MAX_TERMS {
            return Err(EvalError::TooLarge(rows.len() + next.len()));
        }
        rows.extend(next.iter().cloned());
        level = next;
    }
    Ok(rows)
}

/// `"A B AB AA BB"`: each word is a product of variable letters, and the intercept is added.
fn string_terms(text: &str, vars: usize) -> Result<Vec<Vec<u32>>, EvalError> {
    let mut rows = vec![vec![0; vars]];
    for word in text.split([' ', ',', ':', ';']).filter(|w| !w.is_empty()) {
        let mut exps = vec![0u32; vars];
        for ch in word.chars() {
            let idx = match ch {
                'A'..='Z' => (ch as u8 - b'A') as usize,
                _ => {
                    return Err(EvalError::Unsupported(format!(
                        "polyfit: `{word}` is not a term of the letters A, B, ..."
                    )))
                }
            };
            if idx >= vars {
                return Err(EvalError::Unsupported(format!(
                    "polyfit: term `{word}` names variable {ch}, but X has {vars} variables"
                )));
            }
            exps[idx] += 1;
            if exps[idx] > MAX_EXPONENT {
                return Err(EvalError::Unsupported(
                    "polyfit: an exponent is too large".to_string(),
                ));
            }
        }
        rows.push(exps);
        if rows.len() > MAX_TERMS {
            return Err(EvalError::TooLarge(rows.len()));
        }
    }
    Ok(rows)
}

/// The exponents of a matrix of terms: the first column is the coefficient guess, then one column
/// of exponents per variable.
fn matrix_terms(m: &Matrix, vars: usize) -> Result<Vec<Vec<u32>>, EvalError> {
    if m.cols() != vars + 1 {
        return Err(EvalError::Unsupported(format!(
            "polyfit: the matrix of terms needs {} columns (a guess and an exponent for each of the {vars} variables)",
            vars + 1
        )));
    }
    let mut rows = Vec::with_capacity(m.rows());
    for i in 0..m.rows() {
        let mut exps = Vec::with_capacity(vars);
        for j in 1..m.cols() {
            let s = m.get(i, j)?;
            let e = crate::eval::env::integer_of(s)
                .filter(|e| (0..=i64::from(MAX_EXPONENT)).contains(e))
                .ok_or_else(|| {
                    EvalError::Unsupported(
                        "polyfit: the exponents of the matrix of terms must be integers from 0 to 99"
                            .to_string(),
                    )
                })?;
            exps.push(e as u32);
        }
        rows.push(exps);
    }
    Ok(rows)
}

//! Linear least-squares fits (help: Linear Regression, Linear Combination of Functions).
//!
//! - `linfit(vx, vy, F)`: the coefficients `c` of the linear combination `sum c_j F_j(x)` that best
//!   fits the data in least squares. `F` is a user function `F(x)` returning the vector of
//!   basis-function values (its elements are the terms). The design matrix is built from one call of
//!   `F` per data point, and solved by QR (`matrix_linalg::least_squares`).
//! - `lnfit(vx, vy)`: `a ln(x) + b`, a linear fit in `ln(x)` (so no iteration). Needs `x > 0`.

use super::matrix_linalg::least_squares;
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::expr_eval::{call_user_with, Ctx};
use crate::eval::special_forms::{
    column, named_function, real_value, reals_of, vector_of, SAMPLE_UNITS,
};
use crate::eval::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use crate::eval::work;
use crate::expr::Expr;
use crate::units::Dimension;

pub(super) fn register(r: &mut Registry) {
    r.register("lnfit", Arity::Exact(2), lnfit);
}

/// `linfit(vx, vy, F)` (see the module documentation).
pub(in crate::eval) fn linfit_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [vx, vy, f] = args else {
        return Err(EvalError::Unsupported(
            "linfit takes three arguments".to_string(),
        ));
    };
    let xs = reals_of(&vector_of(vx, ctx)?, "the x data")?;
    let ys = reals_of(&vector_of(vy, ctx)?, "the y data")?;
    if xs.len() != ys.len() {
        return Err(EvalError::DataLength {
            expected: xs.len(),
            found: ys.len(),
        });
    }
    let Some((name, func)) = named_function(f, ctx)? else {
        return Err(EvalError::Unsupported(
            "the basis must be the name of a function F(x)".to_string(),
        ));
    };
    if func.params.len() != 1 {
        return Err(EvalError::ArgCount {
            name,
            expected: "1".to_string(),
            found: func.params.len(),
        });
    }
    // One row of the design matrix per data point: the basis values at that x.
    let mut width = None;
    let mut design: Vec<f64> = Vec::new();
    for x in &xs {
        ctx.budget.charge(SAMPLE_UNITS)?;
        let out = call_user_with(&name, func, vec![real_value(*x)], &[], ctx)?;
        let row = reals_of(&out, "the basis function F")?;
        match width {
            None => width = Some(row.len()),
            Some(w) if w != row.len() => {
                return Err(EvalError::Unsupported(
                    "F must return the same number of terms at every x".to_string(),
                ))
            }
            Some(_) => {}
        }
        design.extend(row);
    }
    let p = width.unwrap_or(0);
    if p == 0 || p > xs.len() {
        return Err(EvalError::Unsupported(
            "linfit needs at least one term and no more terms than data points".to_string(),
        ));
    }
    if xs.len().saturating_mul(p) > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(xs.len() * p));
    }
    // The QR solve of the design matrix, charged as the matrix kernel it is (m p^2 multiply-adds).
    ctx.budget
        .charge(work::cubic_units((xs.len() * p * p) as u64))?;
    let a = matrix_of(xs.len(), p, &design)?;
    let b = matrix_of(ys.len(), 1, &ys)?;
    let sol = least_squares(&a, &b)?;
    let coefficients: Vec<f64> = sol.iter().map(|c| c.re).collect();
    column(&coefficients)
}

/// `lnfit(vx, vy)`: `a ln(x) + b`, as the least-squares line through `(ln x, y)`.
fn lnfit(args: &[Value]) -> Result<Value, EvalError> {
    let xs = reals_of(&args[0], "the x data")?;
    let ys = reals_of(&args[1], "the y data")?;
    if xs.len() != ys.len() {
        return Err(EvalError::DataLength {
            expected: xs.len(),
            found: ys.len(),
        });
    }
    if xs.len() < 2 {
        return Err(EvalError::Unsupported(
            "lnfit needs at least two data points".to_string(),
        ));
    }
    if xs.iter().any(|x| *x <= 0.0) {
        return Err(EvalError::Unsupported(
            "lnfit needs x values above 0".to_string(),
        ));
    }
    let design: Vec<f64> = xs.iter().flat_map(|x| [x.ln(), 1.0]).collect();
    let a = matrix_of(xs.len(), 2, &design)?;
    let b = matrix_of(ys.len(), 1, &ys)?;
    let sol = least_squares(&a, &b)?;
    column(&sol.iter().map(|c| c.re).collect::<Vec<f64>>())
}

/// A dimensionless real matrix of `rows x cols`, row-major.
fn matrix_of(rows: usize, cols: usize, values: &[f64]) -> Result<Matrix, EvalError> {
    let data = values
        .iter()
        .map(|v| Scalar::real(*v, Dimension::dimensionless()))
        .collect();
    Matrix::new(rows, cols, data)
}

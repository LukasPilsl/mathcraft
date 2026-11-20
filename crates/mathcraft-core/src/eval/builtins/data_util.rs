//! Argument handling shared by the data-analysis and signal-processing builtins (`data_interp`,
//! `data_regress`, `data_smooth`, `data_hist`, `signal_fft`): column vectors, real and finite checks,
//! one shared dimension per argument, and the result constructors.
//!
//! Conventions: a vector is a column matrix (a scalar is a vector of one element). A vector argument
//! of another shape is refused with `Unsupported`, naming the function. Every result is checked
//! finite, so a non-finite value never reaches the worksheet.

use num_complex::Complex64;

use super::matrix::uniform_dim;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use crate::units::Dimension;

/// The elements of a column vector, or of a scalar (a vector of one element).
pub(super) fn vector(v: &Value, name: &str) -> Result<Vec<Scalar>, EvalError> {
    match v {
        Value::Scalar(s) => Ok(vec![*s]),
        Value::Matrix(m) if m.cols() == 1 => Ok(m.data().to_vec()),
        _ => Err(EvalError::Unsupported(format!("{name} needs a vector"))),
    }
}

/// The elements of a vector or matrix (row-major) with its row and column counts. A scalar is 1 x 1.
pub(super) fn grid(v: &Value, name: &str) -> Result<(usize, usize, Vec<Scalar>), EvalError> {
    match v {
        Value::Scalar(s) => Ok((1, 1, vec![*s])),
        Value::Matrix(m) => Ok((m.rows(), m.cols(), m.data().to_vec())),
        Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => {
            Err(EvalError::Unsupported(format!("{name} needs numbers")))
        }
    }
}

/// The one dimension shared by all of `xs`. An empty list is refused.
pub(super) fn one_dim(xs: &[Scalar]) -> Result<Dimension, EvalError> {
    if xs.is_empty() {
        return Err(EvalError::Unsupported("no data".to_string()));
    }
    uniform_dim(xs)
}

/// The real part of one element, refusing a non-zero imaginary part and non-finite values.
pub(super) fn real_of(s: &Scalar, name: &str) -> Result<f64, EvalError> {
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(format!("{name} needs real numbers")));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// The real parts of `xs` (see [`real_of`]).
pub(super) fn reals(xs: &[Scalar], name: &str) -> Result<Vec<f64>, EvalError> {
    xs.iter().map(|s| real_of(s, name)).collect()
}

/// The complex values of `xs`, refusing non-finite parts.
pub(super) fn complexes(xs: &[Scalar]) -> Result<Vec<Complex64>, EvalError> {
    xs.iter().map(|s| finite(s.value)).collect()
}

/// A real scalar argument and its dimension.
pub(super) fn real_scalar(v: &Value, name: &str) -> Result<(f64, Dimension), EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!("{name} needs a scalar")));
    };
    Ok((real_of(s, name)?, s.dim))
}

/// A positive integer argument (dimensionless, at most `MAX_ELEMENTS`).
pub(super) fn count(v: &Value, name: &str) -> Result<usize, EvalError> {
    let (x, dim) = real_scalar(v, name)?;
    ops::require_dimensionless(dim)?;
    if x >= 1.0 && x.fract() == 0.0 && x <= MAX_ELEMENTS as f64 {
        Ok(x as usize)
    } else {
        Err(EvalError::Unsupported(format!(
            "{name} needs a positive integer"
        )))
    }
}

/// `z` if both parts are finite.
pub(super) fn finite(z: Complex64) -> Result<Complex64, EvalError> {
    if z.re.is_finite() && z.im.is_finite() {
        Ok(z)
    } else {
        Err(EvalError::NotFinite)
    }
}

/// A real result (a scalar) of dimension `dim`.
pub(super) fn real_result(x: f64, dim: Dimension) -> Result<Value, EvalError> {
    scalar_result(Complex64::new(x, 0.0), dim)
}

/// A complex result (a scalar) of dimension `dim`.
pub(super) fn scalar_result(z: Complex64, dim: Dimension) -> Result<Value, EvalError> {
    Ok(Value::Scalar(Scalar::new(finite(z)?, dim)))
}

/// A column vector of results of dimension `dim`.
pub(super) fn column(zs: &[Complex64], dim: Dimension) -> Result<Value, EvalError> {
    grid_result(zs.len(), 1, zs, dim)
}

/// A `rows x cols` matrix of results (row-major) of dimension `dim`.
pub(super) fn grid_result(
    rows: usize,
    cols: usize,
    zs: &[Complex64],
    dim: Dimension,
) -> Result<Value, EvalError> {
    let data = zs
        .iter()
        .map(|&z| finite(z).map(|z| Scalar::new(z, dim)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// Test helpers shared by the tests of the data-analysis builtins.
#[cfg(test)]
pub(super) mod testing {
    use super::*;

    pub fn dimless() -> Dimension {
        Dimension::dimensionless()
    }

    /// A column vector of the given reals.
    pub fn col(xs: &[f64]) -> Value {
        let data = xs.iter().map(|&x| Scalar::real(x, dimless())).collect();
        Value::Matrix(Matrix::new(xs.len(), 1, data).expect("column"))
    }

    /// A dimensionless scalar.
    pub fn num(x: f64) -> Value {
        Value::Scalar(Scalar::real(x, dimless()))
    }

    /// The real parts of the elements of a vector or scalar result.
    pub fn values(v: &Value) -> Vec<f64> {
        match v {
            Value::Matrix(m) => m.data().iter().map(|s| s.value.re).collect(),
            Value::Scalar(s) => vec![s.value.re],
            Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => {
                panic!("expected numbers")
            }
        }
    }

    /// The complex parts of the elements of a result.
    pub fn complex_values(v: &Value) -> Vec<Complex64> {
        match v {
            Value::Matrix(m) => m.data().iter().map(|s| s.value).collect(),
            Value::Scalar(s) => vec![s.value],
            Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => {
                panic!("expected numbers")
            }
        }
    }

    /// The dimension of the first element of a result.
    pub fn dim_of(v: &Value) -> Dimension {
        match v {
            Value::Matrix(m) => m.data()[0].dim,
            Value::Scalar(s) => s.dim,
            Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => {
                panic!("expected numbers")
            }
        }
    }

    pub fn assert_all_close(actual: &[f64], expected: &[f64], tol: f64) {
        assert_eq!(actual.len(), expected.len(), "length of {actual:?}");
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (a - e).abs() <= tol,
                "element {i}: {a} is not {e} (tolerance {tol})"
            );
        }
    }
}

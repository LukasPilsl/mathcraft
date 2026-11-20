//! Shared argument and result helpers for the image functions of the second tier: an image is a
//! matrix of real (or, for the transforms, complex) elements; a pixel is read as an `f64` after the
//! dimension and finiteness checks, and a result is a dimensionless matrix again.

use num_complex::Complex64;

use crate::eval::builtins::matrix::expect_matrix;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// A real image: row-major values with the shape.
pub(super) struct Plane {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Plane {
    /// The value at `(r, c)` when it lies inside the image.
    pub fn at(&self, r: isize, c: isize) -> Option<f64> {
        if r < 0 || c < 0 || r as usize >= self.rows || c as usize >= self.cols {
            None
        } else {
            Some(self.data[r as usize * self.cols + c as usize])
        }
    }

    /// The value at `(r, c)`, which must be inside the image.
    pub fn get(&self, r: usize, c: usize) -> f64 {
        self.data[r * self.cols + c]
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}

/// A dimensionless real matrix argument. Non-real, non-finite or dimensioned elements are refused.
pub(super) fn plane(v: &Value, name: &str) -> Result<Plane, EvalError> {
    let m = expect_matrix(v, name)?;
    let (rows, cols) = m.shape();
    let data = m
        .data()
        .iter()
        .map(|s| real_entry(s, name))
        .collect::<Result<Vec<f64>, _>>()?;
    Ok(Plane { rows, cols, data })
}

/// A dimensionless complex matrix argument (for the transforms, whose input may be complex).
pub(super) fn complex_plane(
    v: &Value,
    name: &str,
) -> Result<(usize, usize, Vec<Complex64>), EvalError> {
    let m = expect_matrix(v, name)?;
    let (rows, cols) = m.shape();
    let data = m
        .data()
        .iter()
        .map(|s| {
            ops::require_dimensionless(s.dim)?;
            if !s.value.re.is_finite() || !s.value.im.is_finite() {
                return Err(EvalError::NotFinite);
            }
            Ok(s.value)
        })
        .collect::<Result<Vec<Complex64>, EvalError>>()?;
    Ok((rows, cols, data))
}

fn real_entry(s: &Scalar, name: &str) -> Result<f64, EvalError> {
    ops::require_dimensionless(s.dim)?;
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs real pixel values"
        )));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// A dimensionless matrix made from real values.
pub(super) fn plane_value(rows: usize, cols: usize, data: Vec<f64>) -> Result<Value, EvalError> {
    let scalars = data
        .into_iter()
        .map(|x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows, cols, scalars)?))
}

/// A dimensionless matrix made from complex values.
pub(super) fn complex_value(
    rows: usize,
    cols: usize,
    data: Vec<Complex64>,
) -> Result<Value, EvalError> {
    let scalars = data
        .into_iter()
        .map(|z| Scalar::new(z, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows, cols, scalars)?))
}

/// A dimensionless real scalar argument.
pub(super) fn real_arg(v: &Value, name: &str) -> Result<f64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            if s.value.im != 0.0 {
                return Err(EvalError::Unsupported(format!(
                    "{name} needs a real number"
                )));
            }
            if !s.value.re.is_finite() {
                return Err(EvalError::NotFinite);
            }
            Ok(s.value.re)
        }
        _ => Err(EvalError::Unsupported(format!(
            "{name} needs a real number here"
        ))),
    }
}

/// An integer scalar argument (a value within `±1e12` whose fraction is zero).
pub(super) fn int_arg(v: &Value, name: &str) -> Result<i64, EvalError> {
    let x = real_arg(v, name)?;
    if x.fract() != 0.0 || x.abs() > 1e12 {
        return Err(EvalError::Unsupported(format!("{name} needs an integer")));
    }
    Ok(x as i64)
}

/// An index argument, counted from `ORIGIN` as the other matrix functions count theirs. The result
/// is zero-based and must be non-negative.
pub(super) fn index_arg(v: &Value, name: &str, origin: i64) -> Result<usize, EvalError> {
    let i = int_arg(v, name)?
        .checked_sub(origin)
        .ok_or(EvalError::Overflow)?;
    usize::try_from(i).map_err(|_| {
        EvalError::Unsupported(format!("{name} is before the first index of the matrix"))
    })
}

/// Charges `units` to the budget as one lump. Used before each loop whose length is known.
pub(super) fn charge(
    ctx: &crate::eval::builtins::BuiltinCtx<'_>,
    units: usize,
) -> Result<(), EvalError> {
    ctx.charge(units as u64)
}

/// The linearly interpolated quantile `q` (0 to 1) of sorted values, the rule of the median and
/// quantile filters. `sorted` must be non-empty and sorted ascending.
pub(super) fn quantile(sorted: &[f64], q: f64) -> f64 {
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    sorted[lo] + frac * (sorted[hi] - sorted[lo])
}

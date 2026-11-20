//! Function values: a function that a variable holds and a call applies (`p := polyfit(X, Y, 2)`,
//! then `p([80; 170])`).
//!
//! A [`FuncValue`] is a closure over data, not over an expression: it owns what it needs (the fitted
//! coefficients and the units of its arguments), so it does not read the worksheet when it is
//! called. The call itself (`expr_eval::call`) applies it to the evaluated arguments.
//!
//! Built:
//! - [`PolyModel`]: the multivariate polynomial `sum c_k prod_i v_i^e_ki` that `polyfit` returns. The
//!   arguments are one vector with a value for each independent variable, each with the units of the
//!   column of `X` it was fitted on. The result has the units of `Y`.

use super::error::EvalError;
use super::value::{Scalar, Value};
use crate::units::Dimension;

/// Largest exponent of one variable in a [`PolyModel`] term.
pub const MAX_EXPONENT: u32 = 99;

/// Largest number of terms of a [`PolyModel`].
pub const MAX_TERMS: usize = 200;

/// A function value. See the module documentation.
#[derive(Debug, Clone, PartialEq)]
pub enum FuncValue {
    /// A fitted multivariate polynomial.
    Poly(PolyModel),
}

/// A multivariate polynomial in SI base units: term `k` is `coefs[k] * prod_i (v_i / scales[i])^e`
/// with the exponents `e` of row `k` of `exponents`. The variables are divided by a scale (the
/// largest magnitude of their column of `X`) only so that the fit is well conditioned; the scale
/// is part of the model, so a call needs no knowledge of it.
#[derive(Debug, Clone, PartialEq)]
pub struct PolyModel {
    pub(super) vars: usize,
    /// One row of `vars` exponents for each term, row-major.
    pub(super) exponents: Vec<u32>,
    pub(super) coefs: Vec<f64>,
    pub(super) scales: Vec<f64>,
    pub(super) x_dims: Vec<Dimension>,
    pub(super) y_dim: Dimension,
}

impl PolyModel {
    /// The model at the point `args`: one vector (row or column) of `vars` values, or one scalar
    /// when the model has one variable. Each value must have the units of its variable and be real.
    pub fn eval(&self, args: &[Value]) -> Result<Value, EvalError> {
        let [arg] = args else {
            return Err(EvalError::ArgCount {
                name: "a fitted polynomial".to_string(),
                expected: "1".to_string(),
                found: args.len(),
            });
        };
        let point: Vec<Scalar> = match arg {
            Value::Scalar(s) => vec![*s],
            Value::Matrix(m) if m.rows() == 1 || m.cols() == 1 => m.data().to_vec(),
            _ => {
                return Err(EvalError::Unsupported(
                    "a fitted polynomial takes a vector of its variables".to_string(),
                ))
            }
        };
        if point.len() != self.vars {
            return Err(EvalError::DataLength {
                expected: self.vars,
                found: point.len(),
            });
        }
        let mut z = Vec::with_capacity(self.vars);
        for ((s, dim), scale) in point.iter().zip(&self.x_dims).zip(&self.scales) {
            if s.dim != *dim {
                return Err(EvalError::DimensionMismatch {
                    left: *dim,
                    right: s.dim,
                });
            }
            if s.value.im != 0.0 {
                return Err(EvalError::Unsupported(
                    "a fitted polynomial needs real arguments".to_string(),
                ));
            }
            if !s.value.re.is_finite() {
                return Err(EvalError::NotFinite);
            }
            z.push(s.value.re / scale);
        }
        let mut sum = 0.0;
        for (k, c) in self.coefs.iter().enumerate() {
            let mut term = *c;
            for (i, zi) in z.iter().enumerate() {
                // An exponent is at most `MAX_EXPONENT`, which an `i32` holds.
                term *= zi.powi(self.exponents[k * self.vars + i] as i32);
            }
            sum += term;
        }
        if !sum.is_finite() {
            return Err(EvalError::NotFinite);
        }
        Ok(Value::Scalar(Scalar::real(sum, self.y_dim)))
    }
}

impl FuncValue {
    /// The work of one call, in budget units.
    pub(super) fn call_units(&self) -> u64 {
        match self {
            FuncValue::Poly(m) => 4 + (m.coefs.len() as u64).saturating_mul(m.vars as u64 + 1),
        }
    }

    /// Applies the function to evaluated arguments.
    pub(super) fn call(&self, args: &[Value]) -> Result<Value, EvalError> {
        match self {
            FuncValue::Poly(m) => m.eval(args),
        }
    }
}

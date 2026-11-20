//! Runtime values: dimensioned complex scalars, row-major matrices and text.
//!
//! A `Scalar` stores its value in SI base units; `dim` says what the number measures.
//! A `Matrix` holds one `Scalar` per element, so elements may carry different units.

use std::sync::Arc;

use num_complex::Complex64;

use super::error::EvalError;
pub use super::func_value::FuncValue;
use crate::expr::Expr;
use crate::symbolic::Sym;
use crate::units::{Dimension, Quantity};

/// A complex number with a dimension. `value` is in SI base units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scalar {
    pub value: Complex64,
    pub dim: Dimension,
}

impl Scalar {
    /// Builds a scalar. Negative-zero parts become `+0.0` (`-0.0 + 0.0 == +0.0`), so a
    /// result such as `-(4 + 0i)` has the same branch under `arg`, `ln` and `sqrt` as the literal
    /// `-4`. Every scalar the evaluator builds goes through here.
    pub fn new(value: Complex64, dim: Dimension) -> Self {
        Self {
            value: Complex64::new(value.re + 0.0, value.im + 0.0),
            dim,
        }
    }

    /// A real number with dimension `dim`; `value` is already in SI base units.
    pub fn real(value: f64, dim: Dimension) -> Self {
        Self::new(Complex64::new(value, 0.0), dim)
    }

    pub fn from_quantity(q: Quantity) -> Self {
        Self::real(q.value_si, q.dim)
    }
}

/// Largest matrix (in elements) any operation may allocate. Larger requests return
/// `EvalError::TooLarge` instead of aborting the process on allocation.
pub const MAX_ELEMENTS: usize = 10_000_000;

/// A row-major matrix. The invariant `data.len() == rows * cols` with both sizes non-zero is
/// enforced by [`Matrix::new`], so indexing never reads outside `data`.
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<Scalar>,
}

impl Matrix {
    pub fn new(rows: usize, cols: usize, data: Vec<Scalar>) -> Result<Self, EvalError> {
        if rows == 0 || cols == 0 {
            return Err(EvalError::Unsupported("empty matrix".to_string()));
        }
        let expected = rows.checked_mul(cols).ok_or(EvalError::Overflow)?;
        if expected > MAX_ELEMENTS {
            return Err(EvalError::TooLarge(expected));
        }
        if data.len() != expected {
            return Err(EvalError::DataLength {
                expected,
                found: data.len(),
            });
        }
        Ok(Self { rows, cols, data })
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn shape(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    /// Elements in row-major order.
    pub fn data(&self) -> &[Scalar] {
        &self.data
    }

    /// Element at zero-based `(row, col)`.
    pub fn get(&self, row: usize, col: usize) -> Result<&Scalar, EvalError> {
        if row >= self.rows {
            return Err(EvalError::IndexOutOfRange {
                index: row,
                bound: self.rows,
            });
        }
        if col >= self.cols {
            return Err(EvalError::IndexOutOfRange {
                index: col,
                bound: self.cols,
            });
        }
        let flat = row * self.cols + col;
        self.data.get(flat).ok_or(EvalError::IndexOutOfRange {
            index: flat,
            bound: self.data.len(),
        })
    }

    /// Replaces the element at zero-based `(row, col)`. Out of range is an error, never a resize.
    pub fn set(&mut self, row: usize, col: usize, value: Scalar) -> Result<(), EvalError> {
        if row >= self.rows {
            return Err(EvalError::IndexOutOfRange {
                index: row,
                bound: self.rows,
            });
        }
        if col >= self.cols {
            return Err(EvalError::IndexOutOfRange {
                index: col,
                bound: self.cols,
            });
        }
        self.data[row * self.cols + col] = value;
        Ok(())
    }

    /// Grows a vector to `len` elements, filling the new ones with `fill`. A row vector grows in
    /// columns, a column vector in rows (a 1 x 1 matrix counts as a row vector). A matrix that is
    /// not a vector cannot grow, and nor can any vector past `MAX_ELEMENTS`.
    pub fn grow_vector(&mut self, len: usize, fill: Scalar) -> Result<(), EvalError> {
        if len > MAX_ELEMENTS {
            return Err(EvalError::TooLarge(len));
        }
        if self.rows == 1 {
            self.cols = len;
        } else if self.cols == 1 {
            self.rows = len;
        } else {
            return Err(EvalError::Unsupported(
                "only a vector can grow; give two indices for a matrix".to_string(),
            ));
        }
        self.data.resize(len, fill);
        Ok(())
    }
}

/// A symbolic result: a value that still has names in it (`x^2 - 1` with `x` free, or
/// `factor(12)` shown as `2^2*3`). Mathcad shows it as an expression, so it is kept in two forms:
/// the canonical entries (exact, used to read the value back into a later symbolic evaluation) and
/// the printed `shown` expression (fractions, powers, roots, the factors Mathcad shows).
///
/// `entries` are row-major and hold one element for a scalar; `shown` is the scalar's expression,
/// or a matrix of the shown entries when `rows * cols` is more than one (or a column from `solve`).
#[derive(Debug, Clone, PartialEq)]
pub struct Symbolic {
    pub rows: usize,
    pub cols: usize,
    pub entries: Vec<Sym>,
    pub shown: Expr,
}

/// Anything an expression can evaluate to.
///
/// `List` is a nested array (Mathcad's vector of arrays, e.g. the `(P, L, U)` of `LU`). Its
/// elements may be scalars, matrices or lists. Operators and most builtins refuse a list; only
/// subscripting (`list[i]`, see `indexing`) and the matrix factorizations produce or read one.
///
/// `Symbolic` is a result with free names (see [`Symbolic`]). Arithmetic and builtins refuse it;
/// only a symbolic evaluation reads it back, as its canonical entries.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(Scalar),
    Matrix(Matrix),
    Str(String),
    List(Vec<Value>),
    Symbolic(Box<Symbolic>),
    /// A function that is a value (`p := polyfit(X, Y, 2)`, then `p(v)`). It is shared by `Arc`,
    /// so copying it into a closure or a call copies a pointer. Operators and builtins refuse it;
    /// only a call (`expr_eval::call`), `IsFunction` and the function-argument forms read it.
    Func(Arc<FuncValue>),
}

impl From<Scalar> for Value {
    fn from(s: Scalar) -> Self {
        Value::Scalar(s)
    }
}

impl From<Matrix> for Value {
    fn from(m: Matrix) -> Self {
        Value::Matrix(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: f64) -> Scalar {
        Scalar::real(v, Dimension::dimensionless())
    }

    #[test]
    fn matrix_new_rejects_wrong_length() {
        let err = Matrix::new(2, 2, vec![s(1.0), s(2.0), s(3.0)]).unwrap_err();
        assert_eq!(
            err,
            EvalError::DataLength {
                expected: 4,
                found: 3
            }
        );
    }

    #[test]
    fn matrix_new_rejects_empty_shape() {
        assert!(matches!(
            Matrix::new(0, 3, vec![]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            Matrix::new(3, 0, vec![]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn matrix_new_rejects_size_overflow() {
        assert_eq!(Matrix::new(usize::MAX, 2, vec![]), Err(EvalError::Overflow));
    }

    #[test]
    fn get_is_row_major() {
        let m = Matrix::new(2, 3, (1..=6).map(|v| s(f64::from(v))).collect()).unwrap();
        assert_eq!(m.shape(), (2, 3));
        assert_eq!(m.get(0, 2).unwrap().value.re, 3.0);
        assert_eq!(m.get(1, 0).unwrap().value.re, 4.0);
    }

    #[test]
    fn get_reports_out_of_range_indices() {
        let m = Matrix::new(2, 2, vec![s(1.0); 4]).unwrap();
        assert_eq!(
            m.get(2, 0),
            Err(EvalError::IndexOutOfRange { index: 2, bound: 2 })
        );
        assert_eq!(
            m.get(0, 5),
            Err(EvalError::IndexOutOfRange { index: 5, bound: 2 })
        );
    }

    #[test]
    fn from_quantity_keeps_si_value_and_dim() {
        let q = Quantity::from_si(9.81, Dimension::from_ints([1, 0, -2, 0, 0, 0, 0]));
        let sc = Scalar::from_quantity(q);
        assert_eq!(sc.value, Complex64::new(9.81, 0.0));
        assert_eq!(sc.dim, q.dim);
    }

    #[test]
    fn value_from_conversions() {
        let v: Value = s(2.0).into();
        assert!(matches!(v, Value::Scalar(_)));
        let m = Matrix::new(1, 1, vec![s(2.0)]).unwrap();
        let v: Value = m.into();
        assert!(matches!(v, Value::Matrix(_)));
    }
}

//! Error values of the evaluation core.

use thiserror::Error;

use crate::units::{Dimension, UnitsError};

/// Errors from value arithmetic. Every fallible operation returns one of these; nothing panics.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EvalError {
    #[error("dimension mismatch: {left} and {right}")]
    DimensionMismatch { left: Dimension, right: Dimension },
    #[error("expected a dimensionless value, found {dim}")]
    DimensionNotDimensionless { dim: Dimension },
    #[error("shape mismatch: {left:?} and {right:?}")]
    ShapeMismatch {
        left: (usize, usize),
        right: (usize, usize),
    },
    #[error("matrix needs {expected} elements, found {found}")]
    DataLength { expected: usize, found: usize },
    #[error("matrix is not square: {rows} x {cols}")]
    NotSquare { rows: usize, cols: usize },
    #[error("matrix is singular")]
    Singular,
    #[error("division by zero")]
    DivisionByZero,
    #[error("result is not finite")]
    NotFinite,
    #[error("index {index} out of range (bound {bound})")]
    IndexOutOfRange { index: usize, bound: usize },
    #[error("arithmetic overflow")]
    Overflow,
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Units(UnitsError),
    #[error("undefined name `{0}`")]
    Undefined(String),
    #[error("undefined function `{0}`")]
    UndefinedFunction(String),
    #[error("function `{name}` takes {expected} arguments, found {found}")]
    ArgCount {
        name: String,
        expected: String,
        found: usize,
    },
    #[error("evaluation nested deeper than the limit of {0}")]
    DepthLimit(usize),
    #[error("{0} elements exceed the evaluation size limit")]
    TooLarge(usize),
    /// A summation or product over more terms than the limit allows. The count is exact (a float),
    /// so a range of 1e30 terms reads as 1e30, not as a saturated integer.
    #[error("a sum or product over {terms:e} terms exceeds the limit of {limit} terms")]
    TooManyTerms { terms: f64, limit: usize },
    /// More user-function calls than the per-evaluation budget (`MAX_CALLS`). Depth limits do not
    /// bound the work a call DAG does, so this one counts calls.
    #[error("more than {0} function calls in one evaluation")]
    CallLimit(usize),
    /// The statement's work budget (`work::MAX_WORK` units) is spent: builtins and matrix
    /// operations together are too expensive for the size or argument range given.
    #[error("work limit exceeded: this expression is too expensive; reduce the size or the argument range")]
    WorkLimit(u64),
    /// A numeric operator (`integral`, `derivative`) whose error estimate did not meet its
    /// tolerance within its limits. The text is the operator's name. No value is returned.
    #[error("{0} did not converge")]
    NoConvergence(String),
    /// A bracketed root search whose interval does not change sign (or whose sign change is a
    /// pole or a jump, not a root). The text says which.
    #[error("{0}")]
    NoSignChange(String),
    /// `find` or `minerr` used outside a solve block. The text is the function's name.
    #[error("{0} only works inside a solve block")]
    OutsideSolveBlock(String),
    /// A solve block's solver failed. The text is the solver's message.
    #[error("solve block: {0}")]
    Solve(String),
    /// The `error("S")` builtin: a message the author wrote. Shown as the text itself; a `try`
    /// block in a program catches it like any other error.
    #[error("{0}")]
    UserMessage(String),
    /// A file function that could not run: file access is disabled for this worksheet, the name
    /// was refused, the file is missing or too large, or its content is not what the reader needs.
    /// The text says which.
    #[error("{0}")]
    FileAccess(String),
}

/// Lifts a units error into an evaluation error, keeping the variants that have a direct match.
pub(crate) fn from_units(err: UnitsError) -> EvalError {
    match err {
        UnitsError::DimensionMismatch { left, right } => {
            EvalError::DimensionMismatch { left, right }
        }
        UnitsError::NotFinite => EvalError::NotFinite,
        UnitsError::Overflow => EvalError::Overflow,
        other => EvalError::Units(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_length_message_names_expected_count() {
        let err = EvalError::DataLength {
            expected: 6,
            found: 5,
        };
        assert_eq!(err.to_string(), "matrix needs 6 elements, found 5");
    }

    #[test]
    fn units_errors_map_to_direct_variants() {
        assert_eq!(from_units(UnitsError::Overflow), EvalError::Overflow);
        assert_eq!(from_units(UnitsError::NotFinite), EvalError::NotFinite);
        let mismatch = from_units(UnitsError::DimensionMismatch {
            left: Dimension::base(0),
            right: Dimension::base(2),
        });
        assert!(matches!(mismatch, EvalError::DimensionMismatch { .. }));
        assert!(matches!(
            from_units(UnitsError::NotReal),
            EvalError::Units(UnitsError::NotReal)
        ));
    }
}

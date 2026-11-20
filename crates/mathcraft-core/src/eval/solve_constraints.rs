//! Constraints of a solve block: `lhs == rhs` and the inequalities `<`, `<=`, `>`, `>=`.
//!
//! The residual of a constraint is SI and real. An equality gives `lhs - rhs` (zero when met); an
//! inequality `lhs < rhs` gives `rhs - lhs`, and `lhs > rhs` gives `lhs - rhs`, so that `g >= 0`
//! holds when the constraint does. Both sides must have the same dimension, except that a literal
//! zero is accepted against any dimension (`y(t) == 0` with `y` in metres, as in the Prime sample
//! 17_rata_schiefer_wurf). Logical operators and `≠` are refused: the help says they cannot be used
//! in constraints.

use super::env::Env;
use super::error::EvalError;
use super::expr_eval::{evaluate_budget, number_value};
use super::value::{Scalar, Value};
use super::work::Budget;
use crate::expr::{BinaryOp, Expr};

/// Units per unknown written into the scratch worksheet for one residual evaluation. Measured: the
/// write (a name copy and a map insert) takes 50-100 ns per unknown, about 10 units.
pub(super) const VARIABLE_WRITE_UNITS: u64 = 10;

/// Units for writing `vars` unknowns and evaluating once: the writes, plus one unit per constraint
/// (charged by [`Constraint::residual`]).
pub(super) fn evaluation_units(vars: usize) -> u64 {
    VARIABLE_WRITE_UNITS.saturating_mul(vars as u64)
}

/// The relation of a constraint, as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Relation {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

/// One constraint of a solve block: its relation and its two sides.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Constraint {
    relation: Relation,
    lhs: Expr,
    rhs: Expr,
}

impl Constraint {
    /// Reads a constraint from a member's statement.
    pub(super) fn parse(expr: &Expr) -> Result<Self, EvalError> {
        let mut e = expr;
        while let Expr::Parens(inner) = e {
            e = inner;
        }
        let Expr::Binary { op, lhs, rhs } = e else {
            return Err(EvalError::Unsupported(
                "a solve block constraint must be an equation or an inequality".to_string(),
            ));
        };
        let relation = match op {
            BinaryOp::Eq => Relation::Eq,
            BinaryOp::Lt => Relation::Lt,
            BinaryOp::Le => Relation::Le,
            BinaryOp::Gt => Relation::Gt,
            BinaryOp::Ge => Relation::Ge,
            BinaryOp::Ne => {
                return Err(EvalError::Unsupported(
                    "the inequality operator is not allowed in a solve block constraint"
                        .to_string(),
                ))
            }
            BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => {
                return Err(EvalError::Unsupported(
                    "logical operators are not allowed in a solve block constraint".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::Unsupported(
                    "a solve block constraint must be an equation or an inequality".to_string(),
                ))
            }
        };
        Ok(Constraint {
            relation,
            lhs: (**lhs).clone(),
            rhs: (**rhs).clone(),
        })
    }

    /// Whether this constraint is an equality (`==`), as opposed to an inequality.
    pub(super) fn is_equality(&self) -> bool {
        self.relation == Relation::Eq
    }

    /// The left and right sides, as written.
    pub(super) fn sides(&self) -> (&Expr, &Expr) {
        (&self.lhs, &self.rhs)
    }

    /// An equality `lhs == rhs` built from its sides.
    pub(super) fn equation(lhs: Expr, rhs: Expr) -> Self {
        Constraint {
            relation: Relation::Eq,
            lhs,
            rhs,
        }
    }

    /// The SI residual at the current unknowns in `env`: zero when an equality holds, and `>= 0`
    /// when an inequality holds. Every user-function call and all builtin work is charged to `budget`.
    pub(super) fn residual(&self, env: &Env, budget: &Budget) -> Result<f64, EvalError> {
        // At least one unit per evaluation, so a constraint of constants is not free either.
        budget.charge(1)?;
        let a = real_scalar(evaluate_budget(&self.lhs, env, budget)?)?;
        let b = real_scalar(evaluate_budget(&self.rhs, env, budget)?)?;
        let zero_side = is_zero_literal(&self.lhs) || is_zero_literal(&self.rhs);
        if a.dim != b.dim && !zero_side {
            return Err(EvalError::DimensionMismatch {
                left: a.dim,
                right: b.dim,
            });
        }
        let (x, y) = (a.value.re, b.value.re);
        Ok(match self.relation {
            Relation::Eq => x - y,
            Relation::Lt | Relation::Le => y - x,
            Relation::Gt | Relation::Ge => x - y,
        })
    }
}

/// A real scalar operand of a constraint. Vectors and complex values are not handled yet.
fn real_scalar(v: Value) -> Result<Scalar, EvalError> {
    match v {
        Value::Scalar(s) if s.value.im == 0.0 => Ok(s),
        // A complex value is a point outside the constraint's domain (`ln` of a negative): it is
        // reported as not finite, so a solver's trial point backs off instead of aborting.
        Value::Scalar(_) => Err(EvalError::NotFinite),
        _ => Err(EvalError::Unsupported(
            "vector constraints are not supported yet".to_string(),
        )),
    }
}

/// A literal zero, possibly parenthesized.
fn is_zero_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Parens(inner) => is_zero_literal(inner),
        Expr::Number(n) => number_value(n) == Ok(0.0),
        _ => false,
    }
}

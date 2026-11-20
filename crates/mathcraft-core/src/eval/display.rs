//! Unit overrides of evaluations (`expr ->unit=`): the unit a result is shown in.
//!
//! An override is a display unit only. The value keeps its SI quantity; the shown numbers are the
//! SI value divided by the unit's factor, and the override's dimension must match the value's.
//! The same check applies to an evaluation nested inside an expression (`(M*g*L->(N*m)=)`).

use super::error::EvalError;
use super::expr_eval::MAX_NODE_DEPTH;
use super::ops;
use super::value::{Scalar, Value};
use crate::expr::{BinaryOp, Expr, Id, IdKind, Number};
use crate::units::{self, CompoundUnit, Dimension, Exp, UnitDef, UnitsError};
use num_complex::Complex64;

/// The shown numbers of `value` in `unit`: SI value divided by the unit's factor, with the
/// dimension checked for every scalar.
pub(super) fn shown_in(value: &Value, unit: &CompoundUnit) -> Result<Value, EvalError> {
    let factor = unit.factor_to_si();
    let offset = unit.offset_to_si();
    let target = unit.dimension();
    ops::map_scalar(value, |s| {
        if s.dim != target {
            return Err(EvalError::DimensionMismatch {
                left: s.dim,
                right: target,
            });
        }
        let shifted = Complex64::new(units::shift_to_unit(s.value.re, offset), s.value.im);
        let shown: Complex64 = shifted / factor;
        if !(shown.re.is_finite() && shown.im.is_finite()) {
            return Err(EvalError::NotFinite);
        }
        Ok(Scalar::new(shown, Dimension::dimensionless()))
    })
}

/// Checks that `value` can be shown in the unit `unit_expr` (same dimension for every scalar).
/// The value itself is not changed.
pub(super) fn check_override(value: &Value, unit_expr: &Expr) -> Result<(), EvalError> {
    let unit = unit_from_expr(unit_expr)?;
    shown_in(value, &unit).map(|_| ())
}

/// A unit override as a compound unit. Accepts unit names, products, quotients, integer powers
/// and a leading `1` numerator (`1/s`). Anything else is unsupported.
pub(super) fn unit_from_expr(expr: &Expr) -> Result<CompoundUnit, EvalError> {
    let mut terms: Vec<(&'static UnitDef, Exp)> = Vec::new();
    collect_terms(expr, 1, 0, &mut terms)?;
    CompoundUnit::new(terms).map_err(|err| match err {
        UnitsError::AffineInCompound(_) => {
            EvalError::Unsupported("temperature offset units".to_string())
        }
        other => EvalError::Units(other),
    })
}

/// Walks a unit expression. `depth` counts the nested nodes, bounded like evaluation so a deep
/// override cannot exhaust the native stack.
fn collect_terms(
    expr: &Expr,
    sign: i32,
    depth: usize,
    out: &mut Vec<(&'static UnitDef, Exp)>,
) -> Result<(), EvalError> {
    if depth > MAX_NODE_DEPTH {
        return Err(EvalError::DepthLimit(MAX_NODE_DEPTH));
    }
    let next = depth + 1;
    match expr {
        Expr::Parens(inner) => collect_terms(inner, sign, next, out),
        Expr::Number(Number::Real { value, .. }) if *value == 1.0 => Ok(()),
        Expr::Id(id) if id.kind == IdKind::Unit && id.subscript.is_none() => {
            push_unit(id, sign, out)
        }
        Expr::Binary {
            op: BinaryOp::Mul,
            lhs,
            rhs,
        } => {
            collect_terms(lhs, sign, next, out)?;
            collect_terms(rhs, sign, next, out)
        }
        Expr::Binary {
            op: BinaryOp::Div,
            lhs,
            rhs,
        } => {
            collect_terms(lhs, sign, next, out)?;
            collect_terms(rhs, -sign, next, out)
        }
        Expr::Binary {
            op: BinaryOp::Pow,
            lhs,
            rhs,
        } => {
            let k = integer_power(rhs)?;
            let signed = sign.checked_mul(k).ok_or(EvalError::Overflow)?;
            collect_terms(lhs, signed, next, out)
        }
        _ => Err(EvalError::Unsupported(
            "unit override must be a unit expression".to_string(),
        )),
    }
}

fn push_unit(id: &Id, sign: i32, out: &mut Vec<(&'static UnitDef, Exp)>) -> Result<(), EvalError> {
    let def = units::unit(&id.name)
        .ok_or_else(|| EvalError::Units(UnitsError::UnknownUnit(id.name.clone())))?;
    out.push((def, Exp::from_integer(sign)));
    Ok(())
}

/// The exponent of a unit power: an integer literal.
fn integer_power(rhs: &Expr) -> Result<i32, EvalError> {
    match rhs {
        Expr::Number(Number::Real { value, .. })
            if value.fract() == 0.0 && value.abs() <= f64::from(i32::MAX) =>
        {
            Ok(*value as i32)
        }
        _ => Err(EvalError::Unsupported(
            "unit power must be an integer".to_string(),
        )),
    }
}

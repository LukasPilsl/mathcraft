//! The unit group of a `Scale` node (`20 (m/sec)`).
//!
//! A bare name in a unit group that the worksheet does not define, and that is not a constant,
//! is the unit of that name. Prime's sample `17_rata_schiefer_wurf` writes `(m/sec)` with `sec`
//! unlabelled (labelled as a variable on import), and Prime takes it as the unit second. A name
//! the worksheet defines is a variable and keeps its value; a constant keeps its meaning (`h` is
//! Planck's constant here, not the hour).
//!
//! The group is evaluated here, not through `binary`, so that names are read as units only
//! inside the group.

use crate::expr::{BinaryOp, Expr, IdKind};
use crate::units;

use super::constants::constant;
use super::error::EvalError;
use super::expr_eval::{sub, unit_value, var_key, Ctx, MAX_NODE_DEPTH};
use super::ops;
use super::value::Value;

/// Evaluates the unit group `expr` of a `Scale` node.
pub(super) fn unit_group(expr: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    if ctx.nodes > MAX_NODE_DEPTH {
        return Err(EvalError::DepthLimit(MAX_NODE_DEPTH));
    }
    let deeper = Ctx {
        nodes: ctx.nodes + 1,
        ..ctx
    };
    match expr {
        Expr::Id(id) if id.kind == IdKind::Variable && id.subscript.is_none() => {
            let key = var_key(id)?;
            if is_unit_name(&key, ctx) {
                unit_value(&id.name).map(Value::Scalar)
            } else {
                sub(expr, ctx)
            }
        }
        Expr::Parens(inner) => unit_group(inner, deeper),
        Expr::Binary {
            op: BinaryOp::Mul,
            lhs,
            rhs,
        } => ops::mul(&unit_group(lhs, deeper)?, &unit_group(rhs, deeper)?),
        Expr::Binary {
            op: BinaryOp::Div,
            lhs,
            rhs,
        } => ops::div(&unit_group(lhs, deeper)?, &unit_group(rhs, deeper)?),
        Expr::Binary {
            op: BinaryOp::Pow,
            lhs,
            rhs,
        } => ops::pow(&unit_group(lhs, deeper)?, &sub(rhs, ctx)?),
        _ => sub(expr, ctx),
    }
}

/// Whether the bare name `key` is read as a unit here: the unit table knows it, and the
/// worksheet has no variable, local or constant of that name.
fn is_unit_name(key: &str, ctx: Ctx<'_>) -> bool {
    if units::unit(key).is_none() || constant(key).is_some() {
        return false;
    }
    if ctx.locals.iter().any(|(name, _)| name == key) {
        return false;
    }
    match ctx.frame {
        Some(fr) => {
            !fr.func.closure.variables.contains_key(key)
                && !fr.func.closure.range_variables.contains(key)
                && !fr.func.closure.unbound_variables.contains(key)
        }
        None => ctx.env.variable(key).is_none() && !ctx.env.is_range(key),
    }
}

/// The absolute temperature unit (`°C`, `°F`) when `expr` is exactly that bare unit name.
pub(super) fn offset_unit(expr: &Expr, ctx: Ctx<'_>) -> Option<&'static units::UnitDef> {
    match expr {
        Expr::Parens(inner) => offset_unit(inner, ctx),
        Expr::Id(id) if id.subscript.is_none() => {
            let is_unit = id.kind == IdKind::Unit
                || (id.kind == IdKind::Variable && is_unit_name(&id.name, ctx));
            units::unit(&id.name).filter(|d| is_unit && d.offset_to_si.is_some())
        }
        _ => None,
    }
}

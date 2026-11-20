//! Variable lookup: a name's value from the parameters of a function body, the closure captured with it,
//! the worksheet, or a built-in constant. A matrix is copied only after its copy is charged (see `work`).

use std::borrow::Cow;

use super::constants::constant;
use super::env::integer_of;
use super::error::EvalError;
use super::expr_eval::{range_in_expression, unit_value, var_key, Ctx};
use super::indexing::{select, select_list};
use super::value::Value;
use super::work;
use crate::expr::{Expr, Id, IdKind};

/// A variable's value. A matrix is copied out of the worksheet or the closure, so the copy is
/// charged before it is made ([`work::copy_units`]); a scalar is not.
pub(super) fn lookup(id: &Id, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    match lookup_ref(id, ctx)? {
        Cow::Owned(v) => Ok(v),
        Cow::Borrowed(v) => {
            ctx.budget.charge(work::copy_units(v))?;
            Ok(v.clone())
        }
    }
}

/// Finds a name's value without copying it: a borrow of the worksheet, the closure or the
/// parameters, or an owned unit or constant.
fn lookup_ref<'a>(id: &Id, ctx: Ctx<'a>) -> Result<Cow<'a, Value>, EvalError> {
    // A name the worksheet defines as a variable is that variable even where the parser read it
    // as a unit (`L := 6 m`, then `L`): the definition wins from there on.
    if id.kind == IdKind::Unit && (id.subscript.is_some() || !defined_variable(&id.name, ctx)) {
        return unit_value(&id.name).map(|s| Cow::Owned(Value::Scalar(s)));
    }
    if let Some(element) = element_by_range_subscript(id, ctx) {
        return element.map(Cow::Owned);
    }
    let key = var_key(id)?;
    // The bound variable of a calculus operator (in `locals`) is the innermost binding, so it wins
    // over a programming region's local of the same name, and so does a parameter.
    if let Some((_, v)) = ctx.locals.iter().rev().find(|(name, _)| *name == key) {
        return Ok(Cow::Borrowed(v));
    }
    // A programming region's local shadows the worksheet.
    if let Some(scope) = ctx.scope {
        if let Some(local) = scope.get_charged(&key, ctx.budget) {
            return local.map(Cow::Owned);
        }
    }
    match ctx.frame {
        // Inside a function: only what was captured when it was defined.
        Some(fr) => {
            if let Some(v) = fr.func.closure.variables.get(&key) {
                return Ok(Cow::Borrowed(&**v));
            }
            if fr.func.closure.range_variables.contains(&key) {
                return Err(range_in_expression());
            }
            if fr.func.closure.unbound_variables.contains(&key) {
                return Err(EvalError::Undefined(key));
            }
        }
        None => {
            if ctx.env.is_range(&key) {
                return Err(range_in_expression());
            }
            if let Some(v) = ctx.env.variable(&key) {
                return Ok(Cow::Borrowed(v));
            }
            // A name whose definition failed is a worksheet name: a constant of the same name
            // does not stand in for it. Only live-environment lookups get this check; a value
            // captured in a closure frame stays fixed at definition time.
            if ctx.env.is_failed(&key) {
                return Err(EvalError::Undefined(key));
            }
        }
    }
    match constant(&key) {
        Some(value) => value.map(Cow::Owned),
        None => Err(EvalError::Undefined(key)),
    }
}

/// Whether `key` is bound as a variable here: a bound variable, a local, a captured variable or
/// a worksheet variable (also a range variable, or a name whose definition failed).
fn defined_variable(key: &str, ctx: Ctx<'_>) -> bool {
    if ctx.locals.iter().any(|(name, _)| *name == key) {
        return true;
    }
    if ctx.scope.is_some_and(|scope| scope.has(key)) {
        return true;
    }
    match ctx.frame {
        Some(fr) => {
            fr.func.closure.variables.contains_key(key)
                || fr.func.closure.range_variables.contains(key)
                || fr.func.closure.unbound_variables.contains(key)
        }
        None => ctx.env.variable(key).is_some() || ctx.env.is_range(key) || ctx.env.is_failed(key),
    }
}

/// `y_i` where `i` is the range variable being iterated (a local at worksheet level): element `i`
/// of the array `y` (counted from `ORIGIN`). `None` for any other name. Inside a function body the
/// parameters are locals too, so only the worksheet level takes this path.
fn element_by_range_subscript(id: &Id, ctx: Ctx<'_>) -> Option<Result<Value, EvalError>> {
    let Some(Expr::Id(sub)) = id.subscript.as_deref() else {
        return None;
    };
    if sub.subscript.is_some() || sub.kind == IdKind::Unit {
        return None;
    }
    // The bound variable of a calculus operator comes first, as in `lookup_ref`. Inside a function
    // body the parameters are locals too, and they do not index.
    if ctx.frame.is_none() {
        if let Some((_, index)) = ctx.locals.iter().rev().find(|(name, _)| *name == sub.name) {
            return Some(array_element(id, index, ctx));
        }
    }
    // A local scalar of a programming region indexes too (`X_i` with `i` a loop variable).
    let index = ctx.scope?.get_charged(&sub.name, ctx.budget)?;
    Some(index.and_then(|v| array_element(id, &v, ctx)))
}

/// One element of an array, read in place: the array is borrowed from its scope, the closure or the
/// worksheet, never copied. The access costs a constant ([`work::ELEMENT_UNITS`]), so a loop that
/// reads `w_i` on a long array is not charged for the whole array on each pass.
fn array_element(id: &Id, index: &Value, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let Value::Scalar(s) = index else {
        return Err(EvalError::Unsupported("index must be a scalar".to_string()));
    };
    let position = integer_of(s)
        .ok_or_else(|| EvalError::Unsupported("index must be an integer".to_string()))?;
    let array = Id {
        name: id.name.clone(),
        kind: id.kind,
        subscript: None,
    };
    ctx.budget.charge(work::ELEMENT_UNITS)?;
    let read = |value: &Value| match value {
        Value::Matrix(m) => select(m, &[position], ctx),
        // A nested array (a list of matrices, scalars or lists) takes one index, as `list[i]` does.
        Value::List(items) => select_list(items, &[position], ctx),
        _ => Err(EvalError::Unsupported("cannot index a scalar".to_string())),
    };
    if let Some(scope) = ctx.scope {
        let key = var_key(&array)?;
        if let Some(result) = scope.with_local(&key, |v| read(v)) {
            return result;
        }
    }
    match lookup_ref(&array, ctx)? {
        Cow::Borrowed(v) => read(v),
        Cow::Owned(v) => read(&v),
    }
}

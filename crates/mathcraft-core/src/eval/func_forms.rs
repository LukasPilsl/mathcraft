//! Function-argument builtins that apply or test a function (see `special_forms` for the rules):
//! `IsFunction`, `funmap`, `funconv` and `funcdeconv`.
//!
//! A function argument is the bare name of a user function, or of a variable that holds a function
//! value (`p := polyfit(...)`); see [`callable`]. Every application of it is charged to the
//! statement's budget ([`SAMPLE_UNITS`]) on top of the work of its body.
//!
//! - `IsFunction(x)`: 1 for a user function, a builtin function or a function value, else 0.
//! - `funmap(M, f)`: `f` applied to each element of `M`.
//! - `funconv(M, f)` and `funcdeconv(M, f, e)`: frequency-domain filtering by the mask that `f`
//!   makes (help: "Example: Filtering in the Fourier Transform Domain"). The mask has the shape of
//!   `M`. Its upper-left quadrant holds `f(d)` where `d = sqrt((i^2 + j^2) / (r^2 + c^2))` is the
//!   normalised distance from the upper-left corner, `r = ceil((rows - 1) / 2)` and
//!   `c = ceil((cols - 1) / 2)`; the other quadrants mirror it (the distance of row `i` is
//!   `min(i, rows - i)`). `funcdeconv` divides by the mask plus `e` instead of multiplying.

use std::sync::Arc;

use super::builtins::image_tier2::{filter_image, read_image};
use super::env::UserFunction;
use super::error::EvalError;
use super::expr_eval::{call_user_with, flag, held_function, resolve_user, var_key, Ctx};
use super::func_value::FuncValue;
use super::special_forms::{real_of, real_value, value_of, SAMPLE_UNITS};
use super::value::{Matrix, Scalar, Value};
use super::work;
use crate::expr::Expr;

/// A function a form can apply.
enum Callable<'a> {
    User(String, &'a UserFunction),
    Held(Arc<FuncValue>),
}

/// The function a bare-name argument names, or an error for any other expression.
fn callable<'a>(e: &Expr, ctx: Ctx<'a>) -> Result<Callable<'a>, EvalError> {
    if let Expr::Id(id) = e {
        let key = var_key(id)?;
        if let Some(f) = held_function(&key, ctx) {
            return Ok(Callable::Held(f));
        }
        if let Some(f) = resolve_user(&key, ctx)? {
            return Ok(Callable::User(key, f));
        }
    }
    Err(EvalError::Unsupported(
        "the function argument must be the name of a function".to_string(),
    ))
}

/// One application of `f`, charged.
fn apply(f: &Callable<'_>, args: Vec<Value>, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    ctx.budget.charge(SAMPLE_UNITS)?;
    match f {
        Callable::User(name, func) => call_user_with(name, func, args, &[], ctx),
        Callable::Held(func) => {
            ctx.budget.charge(func.call_units())?;
            func.call(&args)
        }
    }
}

/// `IsFunction(x)`.
pub(super) fn is_function_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [arg] = args else {
        return Err(EvalError::Unsupported(
            "IsFunction takes one argument".to_string(),
        ));
    };
    if let Expr::Id(id) = arg {
        let key = var_key(id)?;
        if held_function(&key, ctx).is_some() || resolve_user(&key, ctx)?.is_some() {
            return Ok(flag(true));
        }
        return match value_of(arg, ctx) {
            Ok(v) => Ok(flag(matches!(v, Value::Func(_)))),
            // A builtin function's name is a function, though it is no variable.
            Err(EvalError::Undefined(_)) if ctx.env.builtins().get(&key).is_some() => {
                Ok(flag(true))
            }
            Err(e) => Err(e),
        };
    }
    Ok(flag(matches!(value_of(arg, ctx)?, Value::Func(_))))
}

/// `funmap(M, f)`.
pub(super) fn funmap_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [m, f] = args else {
        return Err(EvalError::Unsupported(
            "funmap takes two arguments".to_string(),
        ));
    };
    let f = callable(f, ctx)?;
    match value_of(m, ctx)? {
        Value::Matrix(matrix) => {
            let mut out = Vec::with_capacity(matrix.data().len());
            for s in matrix.data() {
                out.push(scalar_result(apply(&f, vec![Value::Scalar(*s)], ctx)?)?);
            }
            Ok(Value::Matrix(Matrix::new(
                matrix.rows(),
                matrix.cols(),
                out,
            )?))
        }
        Value::Scalar(s) => apply(&f, vec![Value::Scalar(s)], ctx),
        _ => Err(EvalError::Unsupported("funmap needs a matrix".to_string())),
    }
}

fn scalar_result(v: Value) -> Result<Scalar, EvalError> {
    match v {
        Value::Scalar(s) => Ok(s),
        _ => Err(EvalError::Unsupported(
            "the function must return a number".to_string(),
        )),
    }
}

/// `funconv(M, f)`.
pub(super) fn funconv_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [m, f] = args else {
        return Err(EvalError::Unsupported(
            "funconv takes two arguments".to_string(),
        ));
    };
    filter_form("funconv", m, f, None, ctx)
}

/// `funcdeconv(M, f, e)`.
pub(super) fn funcdeconv_form(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [m, f, e] = args else {
        return Err(EvalError::Unsupported(
            "funcdeconv takes three arguments".to_string(),
        ));
    };
    let e = real_of(&value_of(e, ctx)?, "the error e")?;
    filter_form("funcdeconv", m, f, Some(e), ctx)
}

fn filter_form(
    name: &str,
    m: &Expr,
    f: &Expr,
    deconv: Option<f64>,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let f = callable(f, ctx)?;
    let (rows, cols, x) = read_image(&value_of(m, ctx)?, name)?;
    let (r, c) = ((rows - 1).div_ceil(2), (cols - 1).div_ceil(2));
    let reach = (r * r + c * c) as f64;
    if reach == 0.0 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs an image of at least two pixels"
        )));
    }
    // The upper-left quadrant of the mask: one application of `f` for each distance pair.
    let mut table = Vec::with_capacity((r + 1) * (c + 1));
    for i in 0..=r {
        for j in 0..=c {
            let d = (((i * i + j * j) as f64) / reach).sqrt();
            table.push(real_of(&apply(&f, vec![real_value(d)], ctx)?, "the mask")?);
        }
    }
    ctx.budget
        .charge(super::builtins::image_tier2::filter_units(rows, cols))?;
    ctx.budget
        .charge(work::ELEMENT_UNITS.saturating_mul((rows * cols) as u64))?;
    let mut mask = Vec::with_capacity(rows * cols);
    for i in 0..rows {
        let di = i.min(rows - i);
        for j in 0..cols {
            let dj = j.min(cols - j);
            mask.push(table[di * (c + 1) + dj]);
        }
    }
    filter_image(rows, cols, &x, &mask, deconv)
}

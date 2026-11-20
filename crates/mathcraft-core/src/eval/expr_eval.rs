//! Expression evaluation: `evaluate(&Expr, &Env) -> Result<Value, EvalError>`.
//!
//! Names resolve in this order: function parameters (local), variables, built-in constants.
//! Inside a user function body the variables are the captured ones (see `closure`), never the
//! live worksheet. `Id` with kind `Unit` is the unit's SI factor. Indexing is relative to
//! `ORIGIN`. A subscripted name with a literal subscript (`v_0`) is a variable named `v_0`, never
//! an index.
//!
//! Recursion is bounded twice: [`MAX_CALL_DEPTH`] nested user-function calls, and
//! [`MAX_NODE_DEPTH`] nested expression nodes (which also bounds native stack use).
//!
//! An evaluation inside an expression (`(x=)`, `(x ->unit=)`) is the value of its expression;
//! the unit override is checked against it but does not change the value (see `display`).
//!
//! A range `a, b .. c` evaluates to a column vector (see `range`). A range variable (a name
//! defined by a range) may be shown on its own. A statement that uses one is evaluated once per
//! value (see `range_iter`); this module refuses a range variable it reaches without iterating
//! (`range_in_expression`), such as a plain definition or a solver constraint. A symbolic
//! evaluation (`SymEval`) runs through the exact core (see `symbolic_bridge`). Not supported yet,
//! each returning `EvalError::Unsupported`: factorial, placeholders, and temperature offset units.

use num_traits::ToPrimitive;

use super::builtins::{Arity, Builtin, BuiltinCtx, Call};
use super::display::check_override;
use std::sync::Arc;

use super::env::{Env, UserFunction};
use super::error::EvalError;
use super::func_value::FuncValue;
use super::indexing::{index, origin_in};
use super::lookup::lookup;
use super::ops::{self, CmpOp};
use super::program::{run_program, Scope};
use super::range::range;
use super::range_iter::{check_builtin_arguments, range_vector};
use super::roots::nth_root;
use super::unit_group::unit_group;
use super::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use super::work::{self, Budget};
use crate::expr::{BinaryOp, Expr, Id, Number, UnaryOp};
use crate::units::{self, Dimension, Quantity, UnitsError};

/// Nested user-function calls allowed.
pub const MAX_CALL_DEPTH: usize = 200;

/// User-function calls allowed in one top-level evaluation, nested or not.
///
/// Depth alone does not bound work: `f_k(x) := f_(k-1)(x) + f_(k-2)(x)` has depth `k` but makes
/// about `2 * fib(k)` calls. Once the budget is spent, evaluation fails with
/// `EvalError::CallLimit(MAX_CALLS)`. A million calls is well under a second in a release build
/// and far below the point where a worksheet looks hung. Builtin and matrix work has its own
/// budget (`work::MAX_WORK`), charged alongside.
pub const MAX_CALLS: usize = 1_000_000;

/// Nested expression nodes allowed, counting the nodes inside called function bodies.
///
/// This is a native-stack bound, not a size bound. Each node on the recursion path costs a few
/// hundred bytes of stack in a debug build. 400 levels fit a 2 MiB thread (the default for test
/// threads and the smallest stack we run on), and the UI main thread has 8 MiB.
pub const MAX_NODE_DEPTH: usize = 400;

/// The user function whose body is being evaluated: its name as called, and its definition
/// (whose closure holds the names the body may use).
#[derive(Clone, Copy)]
pub(super) struct Frame<'a> {
    pub name: &'a str,
    pub func: &'a UserFunction,
}

/// Evaluation context: the worksheet, the function being run (if any), the parameters bound in
/// it, and the two depth counters.
#[derive(Clone, Copy)]
pub(super) struct Ctx<'a> {
    pub env: &'a Env,
    pub frame: Option<Frame<'a>>,
    pub locals: &'a [(String, Value)],
    pub calls: usize,
    pub nodes: usize,
    /// The calls and work still allowed in this top-level evaluation (see `work`).
    pub budget: &'a Budget,
    /// The local scope of the programming region being run, if any (see `program`). A function
    /// call starts with `None`.
    pub scope: Option<&'a Scope>,
}

/// Evaluates `expr` against `env`. Definitions and evaluation statements are for
/// [`crate::eval::Session`]; they are refused here.
pub fn evaluate(expr: &Expr, env: &Env) -> Result<Value, EvalError> {
    evaluate_budget(expr, env, &Budget::new())
}

/// [`evaluate`] with the calls and work charged to `budget`. A caller that runs many evaluations
/// in a row (a solve block's constraints) shares one budget across them.
pub(super) fn evaluate_budget(expr: &Expr, env: &Env, budget: &Budget) -> Result<Value, EvalError> {
    eval(
        expr,
        Ctx {
            env,
            frame: None,
            locals: &[],
            calls: 0,
            nodes: 0,
            budget,
            scope: None,
        },
    )
}

/// Dispatches one node. Every arm is a call into its own function so this frame stays small:
/// frames on the recursion path cost native stack, and debug builds use a lot per frame.
pub(super) fn eval(expr: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    if ctx.nodes > MAX_NODE_DEPTH {
        return Err(EvalError::DepthLimit(MAX_NODE_DEPTH));
    }
    match expr {
        Expr::Number(n) => number(n),
        Expr::Id(id) => lookup(id, ctx),
        Expr::Unary { op, operand } => unary(*op, operand, ctx),
        Expr::Binary { op, lhs, rhs } => binary(*op, lhs, rhs, ctx),
        Expr::Call { func, args } => call(func, args, ctx),
        Expr::Matrix {
            rows,
            cols,
            elements,
        } => matrix(*rows, *cols, elements, ctx),
        Expr::Index { base, indices } => index(base, indices, ctx),
        Expr::NthRoot { degree, radicand } => nth_root(degree.as_deref(), radicand, ctx),
        Expr::Scale { value, unit } => scale(value, unit, ctx),
        Expr::Parens(inner) => sub(inner, ctx),
        Expr::Eval {
            expr,
            unit_override,
        } => evaluation(expr, unit_override.as_deref(), ctx),
        Expr::Range { from, step, to } => range(from, step.as_deref(), to, ctx),
        Expr::SymEval { expr, command, .. } => {
            super::symbolic_bridge::evaluate(expr, command.as_ref(), ctx)
        }
        other => other_node(other, &ctx),
    }
}

/// `expr =` inside an expression: the value of `expr`. A unit override must match its dimension;
/// it is a display unit, so the value is returned unchanged.
fn evaluation(expr: &Expr, unit_override: Option<&Expr>, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let value = sub(expr, ctx)?;
    if let Some(unit) = unit_override {
        check_override(&value, unit)?;
    }
    Ok(value)
}

/// Evaluates a child node one level deeper.
#[inline(always)]
pub(super) fn sub(expr: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    eval(
        expr,
        Ctx {
            nodes: ctx.nodes + 1,
            ..ctx
        },
    )
}

fn number(n: &Number) -> Result<Value, EvalError> {
    let x = number_value(n)?;
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::real(x, Dimension::dimensionless())))
}

fn scale(value: &Expr, unit: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let v = sub(value, ctx)?;
    if let Some(def) = super::unit_group::offset_unit(unit, ctx) {
        // `25 °C` is the absolute temperature 298.15 K: the offset applies to the number.
        return ops::map_scalar(&v, |s| {
            ops::require_dimensionless(s.dim)?;
            if s.value.im != 0.0 {
                return Err(EvalError::Unsupported("complex temperature".to_string()));
            }
            Ok(Scalar::from_quantity(Quantity::from_unit(s.value.re, def)))
        });
    }
    let u = unit_group(unit, ctx)?;
    ops::mul(&v, &u)
}

/// Kept out of line so the recursion frame of `other_node` stays small (the depth tests run on a
/// 2 MiB stack).
#[inline(never)]
fn str_literal(text: &str) -> Result<Value, EvalError> {
    Ok(Value::Str(text.to_string()))
}

/// The node kinds that are not dispatched in [`eval`] itself. Kept out of `eval` so its frame, which
/// is on the recursion path, does not grow with each kind added here.
#[inline(never)]
fn other_node(expr: &Expr, ctx: &Ctx<'_>) -> Result<Value, EvalError> {
    match expr {
        Expr::Program(stmts) => run_program(stmts, *ctx),
        Expr::Str(text) => str_literal(text),
        _ => calculus_node(expr, *ctx),
    }
}

/// The calculus operators (see `calculus`), kept out of [`other_node`] so that a programming
/// region's recursion path does not carry their frames.
#[inline(never)]
fn calculus_node(expr: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    match expr {
        Expr::Accumulate {
            op,
            var,
            from,
            to,
            body,
        } => super::calculus::accumulate(*op, var, from, to, body, ctx),
        Expr::Fold { op, operand } => super::calculus::fold(*op, operand, ctx),
        Expr::Integral {
            var,
            lower,
            upper,
            body,
        } => super::calculus::integral(var, lower, upper, body, ctx),
        Expr::Derivative {
            var, order, body, ..
        } => super::calculus::derivative(var, order.as_deref(), body, ctx),
        Expr::Gradient { vars, body } => super::calculus::gradient(vars, body, ctx),
        // Prime's limit is a symbolic operation (its help: the numerical `=` cannot evaluate
        // limits): it goes through the exact core, which answers only what it can prove.
        Expr::Limit { .. } => super::symbolic_bridge::evaluate(expr, None, ctx),
        _ => unsupported_node(expr),
    }
}

/// Nodes that have no expression meaning here.
fn unsupported_node(expr: &Expr) -> Result<Value, EvalError> {
    let what = match expr {
        Expr::Placeholder => "placeholder",
        _ => "definitions and evaluations are statements, not expressions",
    };
    Err(EvalError::Unsupported(what.to_string()))
}

/// The `f64` of a number literal. The literal text is parsed (correctly rounded); a rational
/// is converted from its exact fraction.
pub(super) fn number_value(n: &Number) -> Result<f64, EvalError> {
    match n {
        Number::Real { value, text } => Ok(text.parse::<f64>().unwrap_or(*value)),
        Number::Rational(r) => r.to_f64().ok_or(EvalError::NotFinite),
    }
}

/// The storage key of an identifier: its name, or `name_sub` for a literal subscript.
///
/// A subscript must be a number or a plain name; anything else is unsupported.
pub(super) fn var_key(id: &Id) -> Result<String, EvalError> {
    match id.subscript.as_deref() {
        None => Ok(id.name.clone()),
        Some(
            sub @ (Expr::Number(_)
            | Expr::Id(Id {
                subscript: None, ..
            })),
        ) => Ok(format!("{}_{sub}", id.name)),
        Some(other) => Err(EvalError::Unsupported(format!(
            "subscript `{other}` on `{}` must be a number or a name",
            id.name
        ))),
    }
}

/// The error for a use of a range variable inside an expression. Mathcad evaluates such an
/// expression once for each value of the range; that iteration is not built yet, so it is
/// refused rather than computed as a vector (`t*t` would be a dot product, `t^2` a matrix power).
#[inline(never)]
#[cold]
pub(super) fn range_in_expression() -> EvalError {
    EvalError::Unsupported("range variable used in an expression".to_string())
}

/// A unit as a scalar: its SI factor with its dimension.
pub(super) fn unit_value(name: &str) -> Result<Scalar, EvalError> {
    let def = units::unit(name)
        .ok_or_else(|| EvalError::Units(UnitsError::UnknownUnit(name.to_string())))?;
    if def.offset_to_si.is_some() {
        return Err(EvalError::Unsupported(
            "temperature offset units".to_string(),
        ));
    }
    Ok(Scalar::from_quantity(Quantity::from_unit(1.0, def)))
}

/// A 1 or 0 flag as a dimensionless value.
pub(super) fn flag(truth: bool) -> Value {
    Value::Scalar(Scalar::real(
        if truth { 1.0 } else { 0.0 },
        Dimension::dimensionless(),
    ))
}

/// Reads a boolean operand: a dimensionless scalar equal to 1 or 0.
pub(super) fn truth(v: &Value) -> Result<bool, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            let z = s.value;
            if z.im == 0.0 && z.re == 1.0 {
                Ok(true)
            } else if z.im == 0.0 && z.re == 0.0 {
                Ok(false)
            } else {
                Err(EvalError::Unsupported(
                    "boolean operand must be 1 or 0".to_string(),
                ))
            }
        }
        _ => Err(EvalError::Unsupported(
            "boolean operators need scalars".to_string(),
        )),
    }
}

fn unary(op: UnaryOp, operand: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let v = sub(operand, ctx)?;
    apply_unary(op, v, ctx.budget)
}

/// The operator itself, charged to the budget first. Not on the recursion path.
#[inline(never)]
fn apply_unary(op: UnaryOp, v: Value, budget: &Budget) -> Result<Value, EvalError> {
    budget.charge(work::unary_units(&v))?;
    match op {
        UnaryOp::Neg => ops::neg(&v),
        UnaryOp::Not => Ok(flag(!truth(&v)?)),
        UnaryOp::Transpose => ops::transpose(&v),
        UnaryOp::Abs => ops::abs(&v),
        UnaryOp::Norm => ops::norm(&v),
        UnaryOp::Percent => ops::mul(
            &v,
            &Value::Scalar(Scalar::real(0.01, Dimension::dimensionless())),
        ),
        UnaryOp::Factorial => ops::factorial(&v),
    }
}

fn binary(op: BinaryOp, lhs: &Expr, rhs: &Expr, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let a = sub(lhs, ctx)?;
    let b = sub(rhs, ctx)?;
    apply_binary(op, &a, &b, ctx.budget)
}

/// The operator itself, charged to the budget first. Not on the recursion path, so its frame does
/// not count against depth.
#[inline(never)]
fn apply_binary(op: BinaryOp, a: &Value, b: &Value, budget: &Budget) -> Result<Value, EvalError> {
    budget.charge(work::binary_units(op, a, b)?)?;
    let cmp = |c: CmpOp| ops::compare(c, a, b);
    match op {
        BinaryOp::Add => ops::add(a, b),
        BinaryOp::Sub => ops::sub(a, b),
        BinaryOp::Mul => ops::mul(a, b),
        BinaryOp::Div => ops::div(a, b),
        BinaryOp::Cross => ops::cross(a, b),
        BinaryOp::Pow => ops::pow(a, b),
        BinaryOp::Eq => cmp(CmpOp::Eq),
        BinaryOp::Ne => cmp(CmpOp::Ne),
        BinaryOp::Lt => cmp(CmpOp::Lt),
        BinaryOp::Gt => cmp(CmpOp::Gt),
        BinaryOp::Le => cmp(CmpOp::Le),
        BinaryOp::Ge => cmp(CmpOp::Ge),
        BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => {
            let (x, y) = (truth(a)?, truth(b)?);
            Ok(flag(match op {
                BinaryOp::And => x && y,
                BinaryOp::Or => x || y,
                _ => x != y,
            }))
        }
    }
}

/// `[a, b; c, d]`: elements must be scalars; they may carry units.
fn matrix(rows: usize, cols: usize, elements: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let count = elements.len();
    if rows.checked_mul(cols) != Some(count) {
        return Err(EvalError::Unsupported(
            "malformed matrix: rows x cols does not match the element count".to_string(),
        ));
    }
    if count > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(count));
    }
    ctx.budget.charge(count as u64)?;
    let mut data = Vec::with_capacity(count);
    for e in elements {
        match sub(e, ctx)? {
            Value::Scalar(s) => data.push(s),
            _ => {
                return Err(EvalError::Unsupported(
                    "matrix elements must be scalars".to_string(),
                ))
            }
        }
    }
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// Calls: a user function (arguments bound to its parameters in a fresh scope) or a builtin.
///
/// The callee is keyed like a variable (`var_key`): `f_1(x)` calls the function `f_1`, never `f`.
fn call(func: &Id, args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let key = var_key(func)?;
    match resolve_callee(&key, &ctx)? {
        Callee::User(f) => call_user_expr(&key, f, args, ctx),
        Callee::Held => call_held(&key, args, &ctx),
        Callee::Builtin => call_builtin(&key, args, ctx),
    }
}

/// `f'(x)`, `f''(x)`: Prime's prime operator, the first (second, ...) derivative of the function
/// `f` of one variable at `x`. A name that ends in primes and is not a builtin is read this way when
/// `f` is a user function. Out of line: [`call`] is on the recursion path.
#[inline(never)]
fn call_prime(key: &str, args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let base = key.trim_end_matches('\'');
    let order = key.len() - base.len();
    let user = if order > 0 {
        resolve_user(base, ctx)?
    } else {
        None
    };
    let ([arg], Some(f)) = (args, user) else {
        return Err(undefined_function(key));
    };
    if f.params.len() != 1 {
        return Err(EvalError::Unsupported(
            "the prime operator needs a function of one variable".to_string(),
        ));
    }
    let point = sub(arg, ctx)?;
    let var = Id::new("x'", crate::expr::IdKind::Variable);
    let mut locals = ctx.locals.to_vec();
    locals.push((var.name.clone(), point));
    let inner = Ctx {
        locals: &locals,
        ..ctx
    };
    let body = Expr::call(base, vec![Expr::Id(var.clone())]);
    let order = Expr::num(order as f64);
    super::calculus::derivative(&var, Some(&order), &body, inner)
}

/// What a call's name stands for.
enum Callee<'a> {
    User(&'a UserFunction),
    /// A function value held by a variable or parameter (see [`held_function`]).
    Held,
    Builtin,
}

/// The callee of a name: a held function value first (a parameter `f` of `g(f, x) := f(x)` is not
/// a user function), then a user function, else a builtin. Out of line, and the context is passed
/// by reference, so the frame of [`call`], which is on the recursion path, stays small.
#[inline(never)]
fn resolve_callee<'a>(key: &str, ctx: &Ctx<'a>) -> Result<Callee<'a>, EvalError> {
    if held_function(key, *ctx).is_some() {
        return Ok(Callee::Held);
    }
    Ok(match resolve_user(key, *ctx)? {
        Some(f) => Callee::User(f),
        None => Callee::Builtin,
    })
}

/// The function value a name holds: a parameter or bound variable (`g(f, x) := f(x)`), a local of
/// the programming region, a captured variable, or a worksheet variable (`p := polyfit(...)`). A
/// name that holds anything else is not a function value. Out of line: [`call`] is on the
/// recursion path.
#[inline(never)]
pub(super) fn held_function(key: &str, ctx: Ctx<'_>) -> Option<Arc<FuncValue>> {
    let func_of = |v: &Value| match v {
        Value::Func(f) => Some(Arc::clone(f)),
        _ => None,
    };
    if let Some((_, v)) = ctx.locals.iter().rev().find(|(name, _)| name == key) {
        return func_of(v);
    }
    if let Some(scope) = ctx.scope {
        if let Some(found) = scope.with_local(key, func_of) {
            return found;
        }
    }
    match ctx.frame {
        Some(fr) => fr.func.closure.variables.get(key).and_then(|v| func_of(v)),
        None => ctx.env.variable(key).and_then(func_of),
    }
}

/// A call of the function value a name holds: the arguments are evaluated, the call is charged, then
/// applied.
#[inline(never)]
fn call_held(key: &str, args: &[Expr], ctx: &Ctx<'_>) -> Result<Value, EvalError> {
    let ctx = *ctx;
    let Some(f) = held_function(key, ctx) else {
        return Err(EvalError::UndefinedFunction(key.to_string()));
    };
    let values = eval_args(args, ctx)?;
    ctx.budget.charge(f.call_units())?;
    f.call(&values)
}

fn call_user_expr(
    name: &str,
    f: &UserFunction,
    args: &[Expr],
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    let values = eval_args(args, ctx)?;
    call_user(name, f, values, ctx)
}

/// The user function a call names, or `None` for a builtin. Worksheet level: the worksheet's
/// functions. Inside a body: a captured function, the function itself when it is recursive, or
/// `UndefinedFunction` for a name that was undefined when the body was defined.
pub(super) fn resolve_user<'a>(
    name: &str,
    ctx: Ctx<'a>,
) -> Result<Option<&'a UserFunction>, EvalError> {
    let Some(fr) = ctx.frame else {
        return Ok(ctx.env.function(name));
    };
    let closure = &fr.func.closure;
    if let Some(g) = closure.functions.get(name) {
        return Ok(Some(g.as_ref()));
    }
    if closure.recursive && name == fr.name {
        return Ok(Some(fr.func));
    }
    if closure.unbound_functions.contains(name) {
        return Err(EvalError::UndefinedFunction(name.to_string()));
    }
    Ok(None)
}

/// A builtin call. This frame is on the recursion path while the arguments evaluate, so it holds
/// the argument loop itself and nothing else: the arity check and the call run after it returns.
fn call_builtin(name: &str, args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let Some(builtin) = ctx.env.builtins().get(name) else {
        return call_prime(name, args, ctx);
    };
    // A function-argument builtin (see `special_forms`) takes its arguments unevaluated.
    if super::special_forms::lookup_form(name).is_some() {
        return form_call(name, builtin.arity, args, ctx);
    }
    // Top level only: inside a body the worksheet's ranges are not the body's names. This frame sits
    // on the recursion path of nested calls, so its range handling lives in `range_call`.
    if ctx.frame.is_none() {
        if let Some(result) = range_call(name, args, ctx) {
            return result;
        }
    }
    // `if` is a special form: only the branch its condition picks is evaluated.
    if name == "if" && args.len() == 3 {
        return lazy_if(args, ctx);
    }
    let mut values = Vec::with_capacity(args.len());
    for a in args {
        values.push(sub(a, ctx)?);
    }
    run_builtin(builtin, name, &values, ctx)
}

/// A function-argument builtin: its arity is checked, then its form runs on the unevaluated
/// arguments with one more nesting level charged (see `special_forms::FORM_NODE_COST`). Kept out of
/// [`call_builtin`], whose frame is on the recursion path of nested calls.
#[inline(never)]
fn form_call(name: &str, arity: Arity, args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    arity.check(name, args.len())?;
    let Some(form) = super::special_forms::lookup_form(name) else {
        return Err(undefined_function(name));
    };
    form(
        args,
        Ctx {
            nodes: ctx.nodes + super::special_forms::FORM_NODE_COST,
            ..ctx
        },
    )
}

/// The worksheet's range handling of a builtin call, at the top level only: a call over a range
/// variable is evaluated once per value (`range_vector`), and any other range use among the
/// arguments is refused. `None` when the arguments are taken as they are. Outside the recursion
/// path, so its locals do not add to the native stack of nested calls.
#[inline(never)]
fn range_call(name: &str, args: &[Expr], ctx: Ctx<'_>) -> Option<Result<Value, EvalError>> {
    if let Some(whole) = range_vector(name, args, ctx.env, ctx.budget) {
        return Some(whole);
    }
    check_builtin_arguments(name, args, ctx.env).err().map(Err)
}

/// `if(cond, x, y)`: evaluates `cond`, then only the branch it picks (the help: an error in the other
/// branch goes undetected). A condition is true when it is nonzero.
#[inline(never)]
fn lazy_if(args: &[Expr], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let [cond, when_true, when_false] = args else {
        return Err(EvalError::Unsupported(
            "if takes three arguments".to_string(),
        ));
    };
    let taken = match sub(cond, ctx)? {
        Value::Scalar(s) => s.value.re != 0.0 || s.value.im != 0.0,
        _ => {
            return Err(EvalError::Unsupported(
                "the condition of if must be a scalar".to_string(),
            ))
        }
    };
    sub(if taken { when_true } else { when_false }, ctx)
}

#[inline(never)]
#[cold]
fn undefined_function(name: &str) -> EvalError {
    super::solve_block::unavailable_solver(name)
        .unwrap_or_else(|| EvalError::UndefinedFunction(name.to_string()))
}

/// Checks the arity, charges the call's work to the budget, then runs the builtin. The charge comes
/// before the work, so an over-budget call never runs its kernel. A context builtin gets the
/// environment read from the current scope; its reads cannot fail here (they are lazy).
#[inline(never)]
fn run_builtin(
    builtin: &Builtin,
    name: &str,
    values: &[Value],
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    builtin.arity.check(name, values.len())?;
    ctx.budget.charge(work::builtin_units(name, values))?;
    match builtin.call {
        Call::Plain(f) => f(values),
        Call::WithEnv(f) => f(&builtin_env(ctx), values),
    }
}

/// The environment a context builtin sees from this scope: `ORIGIN` (range-guarded, captured in a
/// function body), and `TOL` and `CTOL` read the same way.
#[inline(never)]
fn builtin_env<'a>(ctx: Ctx<'a>) -> BuiltinCtx<'a> {
    BuiltinCtx::from_scope(
        origin_in(ctx),
        system_real(ctx, "TOL"),
        system_real(ctx, "CTOL"),
        ctx.env.file_options().clone(),
        ctx.budget,
    )
}

/// A system variable (`TOL`, `CTOL`) read as a real number: the captured value inside a function
/// body, the worksheet's otherwise. A range variable is refused like any other use of one, and a
/// name that is undefined (its definition failed) is `Undefined`.
pub(super) fn system_real(ctx: Ctx<'_>, key: &str) -> Result<f64, EvalError> {
    let value = match ctx.frame {
        Some(fr) => {
            if fr.func.closure.range_variables.contains(key) {
                return Err(range_in_expression());
            }
            fr.func.closure.variables.get(key).map(|v| &**v)
        }
        None => {
            if ctx.env.is_range(key) {
                return Err(range_in_expression());
            }
            ctx.env.variable(key)
        }
    };
    match value {
        Some(Value::Scalar(s)) if s.dim.is_dimensionless() && s.value.im == 0.0 => Ok(s.value.re),
        Some(_) => Err(EvalError::Unsupported(format!(
            "{key} must be a real dimensionless number"
        ))),
        None => Err(EvalError::Undefined(key.to_string())),
    }
}

/// Evaluates arguments in order (for user calls).
fn eval_args(args: &[Expr], ctx: Ctx<'_>) -> Result<Vec<Value>, EvalError> {
    let mut values = Vec::with_capacity(args.len());
    for a in args {
        values.push(sub(a, ctx)?);
    }
    Ok(values)
}

fn call_user(
    name: &str,
    f: &UserFunction,
    values: Vec<Value>,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    call_user_with(name, f, values, &[], ctx)
}

/// [`call_user`] with extra locals bound in the body after the parameters, such as the components
/// `y_0, y_1, ...` of a vector parameter `y` (see `special_forms`). Charged like any user call.
pub(super) fn call_user_with(
    name: &str,
    f: &UserFunction,
    values: Vec<Value>,
    extra: &[(String, Value)],
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    call_body(name, f, values, extra, ctx, true)
}

/// [`call_user_with`] without taking a call from the budget. A solver's derivative evaluations use
/// it: each is charged as work by the solver (see `special_forms::SAMPLE_UNITS`), so the call count
/// does not cap the number of steps. The nesting depth is still checked.
pub(super) fn call_user_uncounted(
    name: &str,
    f: &UserFunction,
    values: Vec<Value>,
    extra: &[(String, Value)],
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    call_body(name, f, values, extra, ctx, false)
}

/// The shared body of the two calls above. Inlined into each, so the recursion path keeps one frame.
#[inline(always)]
fn call_body(
    name: &str,
    f: &UserFunction,
    values: Vec<Value>,
    extra: &[(String, Value)],
    ctx: Ctx<'_>,
    counted: bool,
) -> Result<Value, EvalError> {
    if values.len() != f.params.len() {
        return Err(EvalError::ArgCount {
            name: name.to_string(),
            expected: f.params.len().to_string(),
            found: values.len(),
        });
    }
    if ctx.calls >= MAX_CALL_DEPTH {
        return Err(EvalError::DepthLimit(MAX_CALL_DEPTH));
    }
    if counted {
        ctx.budget.take_call()?;
    }
    // The body sees its own arguments and the names it captured at definition, never the
    // caller's parameters or the worksheet's current values.
    let mut locals: Vec<(String, Value)> = f.params.iter().cloned().zip(values).collect();
    locals.extend(extra.iter().cloned());
    eval(
        &f.body,
        Ctx {
            env: ctx.env,
            frame: Some(Frame { name, func: f }),
            locals: &locals,
            calls: ctx.calls + 1,
            nodes: ctx.nodes + 1,
            budget: ctx.budget,
            scope: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::indexing::zero_based;
    use super::*;
    use crate::expr::IdKind;

    #[test]
    fn var_key_keeps_literal_subscripts_in_the_name() {
        let id = Id {
            subscript: Some(Box::new(Expr::num(0.0))),
            ..Id::new("v", IdKind::Variable)
        };
        assert_eq!(var_key(&id).unwrap(), "v_0");
    }

    #[test]
    fn var_key_refuses_compound_subscripts() {
        let id = Id {
            subscript: Some(Box::new(Expr::binary(
                BinaryOp::Add,
                Expr::var("i"),
                Expr::num(1.0),
            ))),
            ..Id::new("v", IdKind::Variable)
        };
        assert!(matches!(var_key(&id), Err(EvalError::Unsupported(_))));
    }

    #[test]
    fn zero_based_is_relative_to_origin() {
        assert_eq!(zero_based(1, 1), Ok(0));
        assert_eq!(zero_based(3, 0), Ok(3));
        assert!(zero_based(0, 1).is_err());
    }
}

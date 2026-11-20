//! Programming regions (Mathcad's program operators): statements run in order against a local
//! scope.
//!
//! - `x ← y` binds `x` in the local scope. A name read before its first local assignment is the
//!   worksheet's (or the function's captured) value, as `local_assignment_operator` says.
//! - `if`, `else if`, `also if` and `else` form a conditional group (see [`run_branch`]).
//! - `for i ∈ range` runs its body once per element; `while c` tests `c` before each pass.
//!   `break` and `continue` act on the innermost loop; `return x` ends the whole program with `x`.
//! - `try ... on error ...` runs the second block when the first fails. The work and call limits
//!   are never caught: a loop cannot escape its budget by failing inside a `try`.
//! - The value of a program is the value of the last line that gives one, or the value of the
//!   `return` that ends it. A program that gives no value is an error.
//!
//! The scope is one list of bindings, shared by a whole top-level program and its nested
//! programs. A nested program truncates the list back to where it started, so its names end with
//! it. A loop variable is bound at the position it was pushed at and removed when its loop ends;
//! names that a loop body defines stay, as they do in Mathcad (a loop is not a scope). A function
//! call starts with no scope, so a body never sees or changes its caller's locals.

use std::cell::RefCell;

use super::env::integer_of;
use super::error::EvalError;
use super::expr_eval::{sub, var_key, Ctx, MAX_NODE_DEPTH};
use super::indexing::{origin_in, zero_based};
use super::lookup::lookup;
use super::ops;
use super::value::{Matrix, Scalar, Value, MAX_ELEMENTS};
use super::work::{self, Budget};
use crate::expr::{BranchKind, Expr, Id, IdKind, Stmt};
use crate::units::Dimension;

/// Work charged before each pass of a loop (a `for` element or a `while` test). A pass whose body
/// does almost no work still costs about what entering the interpreter costs, so an infinite loop
/// ends with `WorkLimit`. At 16 units a loop gets about 2e7 passes in one statement, which is well
/// over any sample and a second or two in a release build.
pub(super) const PASS_UNITS: u64 = 16;

/// The local bindings of a program run, in the order they were made (the last one wins).
pub(super) struct Scope {
    vars: RefCell<Vec<(String, Value)>>,
}

impl Scope {
    pub(super) fn new() -> Self {
        Self {
            vars: RefCell::new(Vec::new()),
        }
    }

    fn len(&self) -> usize {
        self.vars.borrow().len()
    }

    fn truncate(&self, n: usize) {
        self.vars.borrow_mut().truncate(n);
    }

    /// A local's value, copied after its copy is charged (see `work::copy_units`). `None` when the
    /// name is not local.
    pub(super) fn get_charged(
        &self,
        key: &str,
        budget: &Budget,
    ) -> Option<Result<Value, EvalError>> {
        let vars = self.vars.borrow();
        let (_, value) = vars.iter().rev().find(|(name, _)| name == key)?;
        if let Err(e) = budget.charge(work::copy_units(value)) {
            return Some(Err(e));
        }
        Some(Ok(value.clone()))
    }

    /// A local's value without charging (for a scalar, such as an index).
    fn get(&self, key: &str) -> Option<Value> {
        let vars = self.vars.borrow();
        vars.iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, v)| v.clone())
    }

    pub(super) fn has(&self, key: &str) -> bool {
        self.vars.borrow().iter().any(|(name, _)| name == key)
    }

    /// Runs `f` on a local's value without copying it. `None` when the name is not local. `f` must
    /// not touch the scope (it is borrowed while `f` runs).
    pub(super) fn with_local<R>(&self, key: &str, f: impl FnOnce(&Value) -> R) -> Option<R> {
        let vars = self.vars.borrow();
        let (_, value) = vars.iter().rev().find(|(name, _)| name == key)?;
        Some(f(value))
    }

    /// Runs `f` on a local's value in place. `None` when the name is not local.
    fn with_local_mut<R>(&self, key: &str, f: impl FnOnce(&mut Value) -> R) -> Option<R> {
        let mut vars = self.vars.borrow_mut();
        let (_, value) = vars.iter_mut().rev().find(|(name, _)| name == key)?;
        Some(f(value))
    }

    /// `name ← value`: rebinds the nearest local of that name, or makes a new one.
    fn assign(&self, key: String, value: Value) {
        let mut vars = self.vars.borrow_mut();
        match vars.iter_mut().rev().find(|(name, _)| *name == key) {
            Some(slot) => slot.1 = value,
            None => vars.push((key, value)),
        }
    }

    /// Binds a loop variable at `index`, the position it has for the whole loop.
    fn bind_at(&self, index: usize, key: &str, value: Value) {
        let mut vars = self.vars.borrow_mut();
        if index < vars.len() {
            vars[index] = (key.to_string(), value);
        } else {
            vars.push((key.to_string(), value));
        }
    }

    /// Removes the loop variable bound at `index` when its loop ends. Bindings made by the body
    /// are after it and stay.
    fn unbind_at(&self, index: usize) {
        let mut vars = self.vars.borrow_mut();
        if index < vars.len() {
            vars.remove(index);
        }
    }
}

/// How a statement or block ended.
enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
}

/// Runs a programming region and returns its value (see the module docs).
pub(super) fn run_program(stmts: &[Stmt], ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let owned = Scope::new();
    let scope: &Scope = match ctx.scope {
        Some(s) => s,
        None => &owned,
    };
    let mark = scope.len();
    let inner = Ctx {
        scope: Some(scope),
        nodes: ctx.nodes + 1,
        ..ctx
    };
    let mut last = None;
    let flow = run_block(stmts, inner, &mut last);
    scope.truncate(mark);
    match flow? {
        Flow::Return(v) => Ok(v),
        Flow::Normal => {
            last.ok_or_else(|| EvalError::Unsupported("the program gives no value".to_string()))
        }
        Flow::Break | Flow::Continue => Err(EvalError::Unsupported(
            "break and continue need a loop".to_string(),
        )),
    }
}

/// Node levels a loop or a `try` adds to its body. Their frames are several times larger than an
/// `if`'s on the native stack (a debug build overflowed a 2 MiB test thread at about 215 nested
/// `for`s when each level counted one node). At three, 130 nested loops reach `MAX_NODE_DEPTH`.
const LOOP_NODE_COST: usize = 3;

/// A conditional's block one nesting level deeper.
fn deeper(ctx: Ctx<'_>) -> Ctx<'_> {
    Ctx {
        nodes: ctx.nodes + 1,
        ..ctx
    }
}

/// A loop's or `try`'s block (see [`LOOP_NODE_COST`]).
fn deeper_loop(ctx: Ctx<'_>) -> Ctx<'_> {
    Ctx {
        nodes: ctx.nodes + LOOP_NODE_COST,
        ..ctx
    }
}

fn scope_of<'a>(ctx: Ctx<'a>) -> Result<&'a Scope, EvalError> {
    ctx.scope
        .ok_or_else(|| EvalError::Unsupported("a statement outside a program".to_string()))
}

/// Runs the statements of a block in order. Conditional groups are tracked here, because an
/// `else` must directly follow its `if` (or `else if` / `also if`) with nothing in between.
fn run_block(stmts: &[Stmt], ctx: Ctx<'_>, last: &mut Option<Value>) -> Result<Flow, EvalError> {
    if ctx.nodes > MAX_NODE_DEPTH {
        return Err(EvalError::DepthLimit(MAX_NODE_DEPTH));
    }
    let mut group: Option<bool> = None;
    for stmt in stmts {
        let flow = match stmt {
            Stmt::Branch { kind, cond, body } => {
                run_branch(*kind, cond.as_ref(), body, &mut group, ctx, last)?
            }
            other => {
                group = None;
                run_stmt(other, ctx, last)?
            }
        };
        if !matches!(flow, Flow::Normal) {
            return Ok(flow);
        }
    }
    Ok(Flow::Normal)
}

/// One branch of a conditional group. `group` is `Some(taken)` while the group is open: whether an
/// earlier branch of it ran.
///
/// - `if c`: opens the group; runs when `c` is nonzero.
/// - `else if c`: runs only when no earlier branch ran, so `c` is not evaluated after one did.
/// - `also if c`: `c` is always evaluated, and it runs when `c` is nonzero.
/// - `else`: runs only when no earlier branch ran; it closes the group.
fn run_branch(
    kind: BranchKind,
    cond: Option<&Expr>,
    body: &[Stmt],
    group: &mut Option<bool>,
    ctx: Ctx<'_>,
    last: &mut Option<Value>,
) -> Result<Flow, EvalError> {
    let taken_before = match kind {
        BranchKind::If => false,
        _ => group.ok_or_else(|| {
            EvalError::Unsupported("else needs an if directly before it".to_string())
        })?,
    };
    let runs = match (kind, cond) {
        (BranchKind::Else, _) => !taken_before,
        (BranchKind::ElseIf, Some(_)) if taken_before => false,
        (_, Some(c)) => condition(&sub(c, ctx)?)?,
        (_, None) => return Err(EvalError::Unsupported("if needs a condition".to_string())),
    };
    *group = match kind {
        BranchKind::If => Some(runs),
        BranchKind::ElseIf | BranchKind::AlsoIf => Some(taken_before || runs),
        BranchKind::Else => None,
    };
    if runs {
        return run_block(body, deeper(ctx), last);
    }
    Ok(Flow::Normal)
}

/// The truth of a condition: any nonzero scalar is true (`if` and `while`).
fn condition(v: &Value) -> Result<bool, EvalError> {
    match v {
        Value::Scalar(s) => Ok(s.value.re != 0.0 || s.value.im != 0.0),
        _ => Err(EvalError::Unsupported(
            "a condition must be a scalar".to_string(),
        )),
    }
}

/// One statement that is not a branch.
fn run_stmt(stmt: &Stmt, ctx: Ctx<'_>, last: &mut Option<Value>) -> Result<Flow, EvalError> {
    match stmt {
        Stmt::Line(e) => {
            *last = Some(sub(e, ctx)?);
            Ok(Flow::Normal)
        }
        Stmt::Local { target, expr } => {
            // Refuse the target before the value is evaluated: `f(x) ← x+1` must not fail on `x`.
            check_target(target)?;
            let value = sub(expr, ctx)?;
            assign(target, value.clone(), ctx)?;
            *last = Some(value);
            Ok(Flow::Normal)
        }
        Stmt::For { var, range, body } => run_for(var, range, body, ctx, last),
        Stmt::While { cond, body } => run_while(cond, body, ctx, last),
        Stmt::Break => Ok(Flow::Break),
        Stmt::Continue => Ok(Flow::Continue),
        Stmt::Return(e) => Ok(Flow::Return(sub(e, ctx)?)),
        Stmt::Try { body, on_error } => run_try(body, on_error, ctx, last),
        Stmt::Branch { .. } => Err(EvalError::Unsupported(
            "a branch must be in a block".to_string(),
        )),
    }
}

/// `for var ∈ range`: one pass per element. The loop variable is removed when the loop ends,
/// whatever ended it.
fn run_for(
    var: &Id,
    range: &Expr,
    body: &[Stmt],
    ctx: Ctx<'_>,
    last: &mut Option<Value>,
) -> Result<Flow, EvalError> {
    let scope = scope_of(ctx)?;
    let key = var_key(var)?;
    let values = iteration_values(range, ctx)?;
    let mark = scope.len();
    let flow = for_passes(&key, &values, body, ctx, scope, mark, last);
    scope.unbind_at(mark);
    flow
}

fn for_passes(
    key: &str,
    values: &[Scalar],
    body: &[Stmt],
    ctx: Ctx<'_>,
    scope: &Scope,
    mark: usize,
    last: &mut Option<Value>,
) -> Result<Flow, EvalError> {
    for element in values {
        ctx.budget.charge(PASS_UNITS)?;
        scope.bind_at(mark, key, Value::Scalar(*element));
        match run_block(body, deeper_loop(ctx), last)? {
            Flow::Normal | Flow::Continue => {}
            Flow::Break => break,
            ret @ Flow::Return(_) => return Ok(ret),
        }
    }
    Ok(Flow::Normal)
}

/// The elements a `for` loop runs over: a scalar is one element, a vector or matrix gives its
/// elements in row-major order.
///
/// A worksheet range variable (`n := 0 .. 3`) is read as its whole column, because a `for` over it
/// is an iteration, not an expression that is evaluated once per value. Inside a function body a
/// range variable is not readable, so it is refused by `lookup`.
fn iteration_values(range: &Expr, ctx: Ctx<'_>) -> Result<Vec<Scalar>, EvalError> {
    let value = match range_column(range, ctx)? {
        Some(column) => Value::Matrix(column),
        None => sub(range, ctx)?,
    };
    match value {
        Value::Scalar(s) => Ok(vec![s]),
        Value::Matrix(m) => Ok(m.data().to_vec()),
        _ => Err(EvalError::Unsupported(
            "a for loop needs a number or a vector".to_string(),
        )),
    }
}

fn range_column(range: &Expr, ctx: Ctx<'_>) -> Result<Option<Matrix>, EvalError> {
    let Expr::Id(id) = range else {
        return Ok(None);
    };
    if id.kind != IdKind::Variable || id.subscript.is_some() || ctx.frame.is_some() {
        return Ok(None);
    }
    let key = var_key(id)?;
    let shadowed = ctx.locals.iter().any(|(name, _)| *name == key)
        || ctx.scope.is_some_and(|s| s.get(&key).is_some());
    if shadowed {
        return Ok(None);
    }
    let Some(column) = ctx.env.range_variable(&key) else {
        return Ok(None);
    };
    ctx.budget.charge(column.data().len() as u64)?;
    Ok(Some(column.clone()))
}

/// `while cond`: tests before each pass. The test and the body are charged as they run, plus
/// [`PASS_UNITS`] per pass.
fn run_while(
    cond: &Expr,
    body: &[Stmt],
    ctx: Ctx<'_>,
    last: &mut Option<Value>,
) -> Result<Flow, EvalError> {
    loop {
        ctx.budget.charge(PASS_UNITS)?;
        let test = sub(cond, ctx)?;
        if !condition(&test)? {
            return Ok(Flow::Normal);
        }
        match run_block(body, deeper_loop(ctx), last)? {
            Flow::Normal | Flow::Continue => {}
            Flow::Break => return Ok(Flow::Normal),
            ret @ Flow::Return(_) => return Ok(ret),
        }
    }
}

/// `try body on error on_error`. Work and call limits and depth limits are not caught: they end
/// the whole evaluation, and a `try` must not extend it.
fn run_try(
    body: &[Stmt],
    on_error: &[Stmt],
    ctx: Ctx<'_>,
    last: &mut Option<Value>,
) -> Result<Flow, EvalError> {
    match run_block(body, deeper_loop(ctx), last) {
        Ok(flow) => Ok(flow),
        Err(e) if caught(&e) => run_block(on_error, deeper_loop(ctx), last),
        Err(e) => Err(e),
    }
}

fn caught(e: &EvalError) -> bool {
    !matches!(
        e,
        EvalError::WorkLimit(_) | EvalError::CallLimit(_) | EvalError::DepthLimit(_)
    )
}

/// Refuses a target that `assign` cannot store into, before its value is evaluated. A function
/// definition is the one common case: a program cannot define a function.
fn check_target(target: &Expr) -> Result<(), EvalError> {
    match target {
        Expr::Id(id) if id.kind == IdKind::Variable => Ok(()),
        Expr::Index { base, .. } => match &**base {
            Expr::Id(id) if id.kind == IdKind::Variable && id.subscript.is_none() => Ok(()),
            _ => Err(EvalError::Unsupported(
                "only a name can be indexed on the left of ←".to_string(),
            )),
        },
        Expr::Call { .. } => Err(EvalError::Unsupported(
            "a function cannot be defined inside a program; define it on the worksheet".to_string(),
        )),
        _ => Err(EvalError::Unsupported(
            "only names and elements can be assigned with ←".to_string(),
        )),
    }
}

/// Stores `value` into `target`: a local name, a local element (`v[i] ← x`, or `v_i ← x` when `i`
/// is a local), or a literal subscript (`v_1 ← x`, a name of its own).
fn assign(target: &Expr, value: Value, ctx: Ctx<'_>) -> Result<(), EvalError> {
    let scope = scope_of(ctx)?;
    match target {
        Expr::Id(id) if id.kind == IdKind::Variable => match element_subscript(id, scope) {
            Some(index) => assign_element(&id.name, vec![position(index)?], value, ctx),
            None => {
                scope.assign(var_key(id)?, value);
                Ok(())
            }
        },
        Expr::Index { base, indices } => match &**base {
            Expr::Id(id) if id.kind == IdKind::Variable && id.subscript.is_none() => {
                let mut positions = Vec::with_capacity(indices.len());
                for e in indices {
                    positions.push(position(sub(e, ctx)?)?);
                }
                assign_element(&id.name, positions, value, ctx)
            }
            _ => Err(EvalError::Unsupported(
                "only a name can be indexed on the left of ←".to_string(),
            )),
        },
        _ => Err(EvalError::Unsupported(
            "only names and elements can be assigned with ←".to_string(),
        )),
    }
}

/// The local index of `v_i` when `i` is a local scalar, the same reading `lookup` gives it.
fn element_subscript(id: &Id, scope: &Scope) -> Option<Value> {
    let Some(Expr::Id(sub_id)) = id.subscript.as_deref() else {
        return None;
    };
    if sub_id.subscript.is_some() || sub_id.kind == IdKind::Unit {
        return None;
    }
    scope.get(&sub_id.name)
}

/// `name[positions] ← element`. The array is copied into the local scope once (the copy is
/// charged); later element writes change that local in place, so a loop of writes costs a constant
/// per element and never copies the array again.
///
/// A vector written past its end grows with zeros to the index (Mathcad's loop tutorial writes
/// `vec_i ← i` past the end of a vector and the vector grows). A matrix does not grow.
fn assign_element(
    name: &str,
    positions: Vec<i64>,
    value: Value,
    ctx: Ctx<'_>,
) -> Result<(), EvalError> {
    let Value::Scalar(element) = value else {
        return Err(EvalError::Unsupported(
            "an array element must be a scalar".to_string(),
        ));
    };
    let scope = scope_of(ctx)?;
    let origin = origin_in(ctx)?;
    if !scope.has(name) {
        let base_id = Id::new(name, IdKind::Variable);
        let array = match lookup(&base_id, ctx)? {
            Value::Matrix(m) => m,
            _ => {
                return Err(EvalError::Unsupported(
                    "cannot assign an element of a scalar".to_string(),
                ))
            }
        };
        scope.assign(name.to_string(), Value::Matrix(array));
    }
    scope
        .with_local_mut(name, |slot| match slot {
            Value::Matrix(array) => set_element(array, &positions, origin, element, ctx.budget),
            _ => Err(EvalError::Unsupported(
                "cannot assign an element of a scalar".to_string(),
            )),
        })
        .unwrap_or_else(|| {
            Err(EvalError::Unsupported(
                "the array is not a local".to_string(),
            ))
        })
}

/// Writes `element` at `positions` in `array`, growing a vector when a single index is past its end.
/// The element must have the dimension of the array's other elements (a dimensionless element of a
/// dimensionless array, and so on): mixed units in one array are an error (the help's rule for
/// arrays). A growth is charged per new element.
fn set_element(
    array: &mut Matrix,
    positions: &[i64],
    origin: i64,
    element: Scalar,
    budget: &Budget,
) -> Result<(), EvalError> {
    let (row, col, grow_to) = match positions {
        [i] => {
            let k = zero_based(*i, origin)?;
            let row_vector = array.rows() == 1;
            if !row_vector && array.cols() != 1 {
                return Err(EvalError::Unsupported(
                    "a single index needs a vector; give two indices for a matrix".to_string(),
                ));
            }
            let len = if row_vector {
                array.cols()
            } else {
                array.rows()
            };
            let grow_to = (k >= len).then_some(k.saturating_add(1));
            if row_vector {
                (0, k, grow_to)
            } else {
                (k, 0, grow_to)
            }
        }
        [i, j] => (zero_based(*i, origin)?, zero_based(*j, origin)?, None),
        _ => {
            return Err(EvalError::Unsupported(
                "indexing takes one or two indices".to_string(),
            ))
        }
    };
    let flat = row.saturating_mul(array.cols()).saturating_add(col);
    if element.dim != reference_dim(array, flat) {
        return Err(EvalError::Unsupported(
            "units differ from the array".to_string(),
        ));
    }
    if let Some(new_len) = grow_to {
        let len = array.data().len();
        if new_len > MAX_ELEMENTS {
            return Err(EvalError::TooLarge(new_len));
        }
        budget.charge(work::ELEMENT_UNITS.saturating_mul((new_len - len) as u64))?;
        array.grow_vector(new_len, Scalar::real(0.0, element.dim))?;
    }
    array.set(row, col, element)
}

/// The dimension the elements of `array` share, as seen from the element at flat index `target`: the
/// first element, or the second when the target is the first (a one-element array compares with
/// itself). Every element of an array has one dimension, so one element is enough to judge.
fn reference_dim(array: &Matrix, target: usize) -> Dimension {
    let data = array.data();
    match data.get(1) {
        Some(second) if target == 0 => second.dim,
        _ => data[0].dim,
    }
}

/// An index value: a dimensionless integer, counted from `ORIGIN` by the caller.
fn position(v: Value) -> Result<i64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            integer_of(&s)
                .ok_or_else(|| EvalError::Unsupported("index must be an integer".to_string()))
        }
        _ => Err(EvalError::Unsupported("index must be a scalar".to_string())),
    }
}

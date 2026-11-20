//! Function closures: what a user-defined function sees, fixed when it is defined.
//!
//! Mathcad's rule (`about_user-defined_functions`): a variable a definition uses takes the value
//! it has at the point where the function is defined, and a function is usable to the right of
//! and below its definition. So `capture` runs at definition time and records:
//! - each variable the body uses, by value. `ORIGIN`, `TOL` and `CTOL` are always captured, since
//!   indexing and the context builtins in the body depend on them.
//! - each user function the body calls, by its stored definition (which carries its own closure,
//!   so `f` keeps the `g` it saw even after `g` is redefined).
//! - a name undefined at that moment, as unbound: using it later is `Undefined` even if it is
//!   defined afterwards.
//!
//! Built-in constants (`π`, `e`, `i`, `j`) and builtin functions need no capture. A body that
//! calls its own name is recursive, unless that name is a builtin: then the builtin is called, as
//! in `sin(x) := sin(x·π/180)`, which the help says is not recursive.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use super::constants::constant;
use super::env::{Env, UserFunction};
use super::expr_eval::var_key;
use super::value::Value;
use crate::expr::{Expr, Id, IdKind, Stmt};

/// The names a user function body sees, fixed at definition time.
///
/// Immutable after `capture`. Variables and functions are shared handles (`Arc`), so capturing
/// copies pointers and never a matrix or a nested closure.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Closure {
    /// Variables the body uses (and `ORIGIN`), by value (shared).
    pub variables: HashMap<String, Arc<Value>>,
    /// User functions the body calls, by their stored definitions (shared).
    pub functions: HashMap<String, Arc<UserFunction>>,
    /// Range variables the body uses. They count as defined, but a use of one is refused when
    /// the body runs (see `expr_eval::range_in_expression`).
    pub range_variables: HashSet<String>,
    /// Variable names the body uses that were undefined at definition time.
    pub unbound_variables: HashSet<String>,
    /// Function names the body calls that were undefined at definition time.
    pub unbound_functions: HashSet<String>,
    /// The body calls the function itself by name (a recursive definition).
    pub recursive: bool,
}

/// System variables that a body may read without naming them: `ORIGIN` (indexing), and `TOL` and
/// `CTOL` (read by context builtins such as `rank`). They are always captured, whether or not the
/// body names them.
const IMPLICIT_SYSTEM: [&str; 3] = ["ORIGIN", "TOL", "CTOL"];

/// Captures what `body`, the body of function `name` with parameters `params`, uses from `env`.
pub(super) fn capture(name: &str, params: &[String], body: &Expr, env: &Env) -> Closure {
    let mut names = Names::default();
    names.collect_locals(body);
    names.walk(body);
    let mut closure = Closure::default();
    for key in IMPLICIT_SYSTEM {
        if env.is_range(key) {
            closure.range_variables.insert(key.to_string());
        } else if let Some(value) = env.variable_shared(key) {
            closure.variables.insert(key.to_string(), Arc::clone(value));
        }
    }
    for key in names.variables {
        if params.contains(&key) {
            continue;
        }
        match env.variable_shared(&key) {
            Some(value) => {
                closure.variables.insert(key, Arc::clone(value));
            }
            None if env.is_range(&key) => {
                closure.range_variables.insert(key);
            }
            None if constant(&key).is_some() && !env.is_failed(&key) => {}
            // A program's own local: it is bound by the program when it runs, not a free name.
            None if names.locals.contains(&key) => {}
            None => {
                closure.unbound_variables.insert(key);
            }
        }
    }
    for key in names.unit_names {
        if params.contains(&key) || names.locals.contains(&key) {
            continue;
        }
        if let Some(value) = env.variable_shared(&key) {
            closure.variables.insert(key, Arc::clone(value));
        } else if env.is_range(&key) {
            closure.range_variables.insert(key);
        }
    }
    for fname in names.calls {
        // A parameter that is called (`g(f, x) := f(x)`) is bound by the caller, not a free name.
        if params.contains(&fname) {
            continue;
        }
        let builtin = env.builtins().get(&fname).is_some();
        // `f'` is the prime operator on a user function `f`, which is captured by its own name.
        if fname.ends_with('\'')
            && !builtin
            && env.function_shared(fname.trim_end_matches('\'')).is_some()
        {
            continue;
        }
        if fname == name {
            closure.recursive = !builtin;
        } else if let Some(function) = env.function_shared(&fname) {
            closure.functions.insert(fname, Arc::clone(function));
        } else if let Some(held) = env
            .variable_shared(&fname)
            .filter(|v| matches!(v.as_ref(), Value::Func(_)))
        {
            // A function value held by a variable (`p := polyfit(...)`) is captured like one.
            closure.variables.insert(fname, Arc::clone(held));
        } else if !builtin {
            closure.unbound_functions.insert(fname);
        }
    }
    closure
}

/// The variable keys and called function names of an expression.
#[derive(Default)]
struct Names {
    variables: BTreeSet<String>,
    /// Bare names the parser read as units. A worksheet variable of that name wins when the
    /// function body runs, so it is captured when it exists; otherwise the name stays a unit.
    unit_names: BTreeSet<String>,
    /// The bare names of the variable uses (`y` for `y`, `y_i` and `y_2`), whatever the subscript.
    bases: BTreeSet<String>,
    calls: BTreeSet<String>,
    /// Names a programming region binds itself: local assignment targets (and their bases, for
    /// `X[i] ← x`) and loop variables. They are not free names of the body.
    locals: BTreeSet<String>,
}

/// Whether `expr` uses the variable `key` (a name, not a function call).
pub(crate) fn uses_variable(expr: &Expr, key: &str) -> bool {
    let mut names = Names::default();
    names.walk(expr);
    names.variables.contains(key)
}

/// Whether `expr` uses the name `base` under any subscript: `y`, `y_i` and `y_2` all use `y`. This
/// is conservative: a literal subscript (`y_2`) is a name of its own, but it is refused too when it
/// sits in the definition of `y`.
pub(super) fn uses_base(expr: &Expr, base: &str) -> bool {
    let mut names = Names::default();
    names.walk(expr);
    names.bases.contains(base)
}

impl Names {
    fn walk(&mut self, expr: &Expr) {
        match expr {
            Expr::Number(_) | Expr::Str(_) | Expr::Placeholder => {}
            Expr::Id(id) => self.id(id),
            Expr::Unary { operand, .. } => self.walk(operand),
            Expr::Binary { lhs, rhs, .. } => {
                self.walk(lhs);
                self.walk(rhs);
            }
            Expr::Call { func, args } => {
                // A callee that cannot be keyed is left for the call to report.
                if let Ok(key) = var_key(func) {
                    // `f'(x)` calls `f` (the prime operator), so `f` is captured too.
                    if key.ends_with('\'') {
                        self.calls.insert(key.trim_end_matches('\'').to_string());
                    }
                    self.calls.insert(key);
                }
                args.iter().for_each(|a| self.walk(a));
            }
            Expr::Range { from, step, to } => {
                self.walk(from);
                if let Some(s) = step {
                    self.walk(s);
                }
                self.walk(to);
            }
            Expr::Matrix { elements, .. } => elements.iter().for_each(|e| self.walk(e)),
            Expr::Index { base, indices } => {
                self.walk(base);
                indices.iter().for_each(|i| self.walk(i));
            }
            Expr::NthRoot { degree, radicand } => {
                if let Some(d) = degree {
                    self.walk(d);
                }
                self.walk(radicand);
            }
            Expr::Parens(inner) => self.walk(inner),
            Expr::Define { expr, .. } | Expr::GlobalDefine { expr, .. } => self.walk(expr),
            Expr::Eval {
                expr,
                unit_override,
            } => {
                self.walk(expr);
                if let Some(u) = unit_override {
                    self.walk(u);
                }
            }
            Expr::SymEval { expr, .. } => self.walk(expr),
            Expr::Scale { value, unit } => {
                self.walk(value);
                self.walk(unit);
            }
            Expr::Program(stmts) => stmts.iter().for_each(|s| self.stmt(s)),
            // The bound variable is a local at evaluation, which is looked up before captures,
            // so walking it too only over-captures a name that is never read from the closure.
            Expr::Accumulate { from, to, body, .. } => {
                self.walk(from);
                self.walk(to);
                self.walk(body);
            }
            Expr::Fold { operand, .. } => self.walk(operand),
            Expr::Integral {
                lower, upper, body, ..
            } => {
                self.walk(lower);
                self.walk(upper);
                self.walk(body);
            }
            Expr::Derivative { order, body, .. } => {
                if let Some(n) = order {
                    self.walk(n);
                }
                self.walk(body);
            }
            Expr::Gradient { body, .. } => self.walk(body),
            Expr::Limit { target, body, .. } => {
                self.walk(target);
                self.walk(body);
            }
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Line(e) | Stmt::Return(e) => self.walk(e),
            Stmt::Local { target, expr } => {
                self.walk(target);
                self.walk(expr);
            }
            Stmt::Branch { cond, body, .. } => {
                if let Some(c) = cond {
                    self.walk(c);
                }
                body.iter().for_each(|s| self.stmt(s));
            }
            Stmt::For { var, range, body } => {
                self.id(var);
                self.walk(range);
                body.iter().for_each(|s| self.stmt(s));
            }
            Stmt::While { cond, body } => {
                self.walk(cond);
                body.iter().for_each(|s| self.stmt(s));
            }
            Stmt::Break | Stmt::Continue => {}
            Stmt::Try { body, on_error } => {
                body.iter().for_each(|s| self.stmt(s));
                on_error.iter().for_each(|s| self.stmt(s));
            }
        }
    }

    /// Records the names a body binds itself, before the walk: the targets of `←` (the base of an
    /// element target) and the loop variables. Nested programs in a line are searched too.
    fn collect_locals(&mut self, body: &Expr) {
        let mut stack: Vec<&[Stmt]> = Vec::new();
        expr_programs(body, &mut stack);
        while let Some(stmts) = stack.pop() {
            for stmt in stmts {
                self.bind_stmt(stmt, &mut stack);
            }
        }
    }

    fn bind_stmt<'a>(&mut self, stmt: &'a Stmt, stack: &mut Vec<&'a [Stmt]>) {
        match stmt {
            Stmt::Local { target, expr } => {
                let base = match target {
                    Expr::Id(id) => Some(id.name.clone()),
                    Expr::Index { base, .. } => match &**base {
                        Expr::Id(id) => Some(id.name.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                self.locals.extend(base);
                expr_programs(expr, stack);
            }
            Stmt::Line(e) | Stmt::Return(e) => expr_programs(e, stack),
            Stmt::Branch { body, .. } | Stmt::While { body, .. } => stack.push(body),
            Stmt::For { var, body, .. } => {
                self.locals.insert(var.name.clone());
                stack.push(body);
            }
            Stmt::Try { body, on_error } => {
                stack.push(body);
                stack.push(on_error);
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }

    /// A variable use. Units are not variables, and a name that cannot be keyed is left for the
    /// call to report. An element of a local index (`X_i` with `i` a loop variable) uses the array
    /// `X` and the index `i`, not a name `X_i`.
    fn id(&mut self, id: &Id) {
        if id.kind == IdKind::Unit {
            if id.subscript.is_none() {
                self.unit_names.insert(id.name.clone());
            }
            return;
        }
        self.bases.insert(id.name.clone());
        if let Some(Expr::Id(index)) = id.subscript.as_deref() {
            if index.subscript.is_none()
                && index.kind != IdKind::Unit
                && self.locals.contains(&index.name)
            {
                self.variables.insert(id.name.clone());
                self.variables.insert(index.name.clone());
                return;
            }
        }
        if let Ok(key) = var_key(id) {
            self.variables.insert(key);
        }
    }
}

/// Pushes the statement blocks of the programming regions that `expr` contains at its top level
/// (a region in a line, or the region itself).
fn expr_programs<'a>(expr: &'a Expr, stack: &mut Vec<&'a [Stmt]>) {
    match expr {
        Expr::Program(stmts) => stack.push(stmts),
        Expr::Define { expr, .. } | Expr::GlobalDefine { expr, .. } => expr_programs(expr, stack),
        Expr::Eval { expr, .. } => expr_programs(expr, stack),
        Expr::Parens(inner) => expr_programs(inner, stack),
        _ => {}
    }
}

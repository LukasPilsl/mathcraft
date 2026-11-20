//! Worksheet session: runs statements in order, the way a Mathcad worksheet evaluates top to
//! bottom.
//!
//! - `name := expr` evaluates `expr` now and binds the result. If evaluation fails the name is
//!   left undefined, so later uses report `Undefined`. Unverified against Prime: no sample
//!   evidence either way (`failed_variable_definition_known_unverified`).
//! - `name := (expr=)` binds the value of `expr`, exactly as `name := expr` does. The `=` only
//!   shows that value, so Prime stores the displayed result of the definition (see
//!   [`Outcome::Defined`]).
//! - `f(x, y) := body` captures the names it uses, as they are at this point (see `closure`):
//!   variables by value, functions by their stored definitions. A free name that is not yet
//!   defined (not a parameter, constant, unit, builtin or earlier definition) makes the definition
//!   report `Undefined` (or `UndefinedFunction`), but the function is still stored (Prime keeps
//!   it: 3_rata_3 #1 and #2, 4_rata_3_V2 #3 and #4). A later call evaluates the body, so it
//!   fails with the first unbound name, even if that name is defined afterwards. A body may call
//!   its own name (recursion); the call limit bounds it. Function names are keyed like variables:
//!   `f_1` and `f_2` are different functions.
//! - `t := a, b .. c` (a range, possibly parenthesized or shown with `=`) defines a range
//!   variable. Its column is kept apart from variables: `t=` shows the column. Any other
//!   expression that uses `t` is evaluated once per value (see `range_iter`), and so is one that
//!   uses two range variables, which gives a matrix. A plain definition from `t` is refused.
//! - `y_i := expr` and `X[r,c := expr`, with range variables as the indices, build the array
//!   (see `range_iter`). The ranges must start at `ORIGIN` and count up by 1.
//! - `expr =` evaluates. With a unit override (`expr ->unit=`) the value is also shown in that
//!   unit; the SI value is kept alongside, and the override's dimension must match (see `display`).
//! - A redefinition affects only later statements, and never a function defined before it.
//!
//! **Known gap: `≡` (global definition) runs like `:=`.** Mathcad makes two passes: it evaluates
//! global definitions first, ignoring regular ones, so `u := 1` then `f1 ≡ 5 + u` fails there
//! (`example_interleaving_of_local_and_global_definitions`). Here statements run in order, so
//! the same worksheet gives `f1 = 6`. The test `global_definition_known_difference` pins this
//! and must flip when the two-pass rule is implemented.

use super::builtins::fileio::FileOptions;
use super::closure::{capture, uses_variable, Closure};
use super::display::{shown_in, unit_from_expr};
use super::env::{Env, UserFunction};
use super::error::EvalError;
use super::expr_eval::{evaluate_budget, var_key};
use super::range_iter::{array_definition, evaluate_iterated, index_names, range_subscript};
use super::solve_block::unavailable_solver;
use super::value::Value;
use super::work::Budget;
use crate::expr::{Expr, Id, IdKind, UnaryOp};
use crate::units::CompoundUnit;

/// What one statement produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A variable or function was bound under `name`. `display` is what the definition's
    /// right-hand side shows when it is an evaluation (`name := (expr=)`, possibly with a unit
    /// override): the value, or the converted value. It is `None` for a plain definition and for
    /// a function. The golden harness compares a stored result with `display`.
    Defined {
        name: String,
        display: Option<Box<Outcome>>,
    },
    /// An evaluation in SI base units.
    Value(Value),
    /// An evaluation shown in a chosen unit. `value` is SI; `shown` holds dimensionless scalars
    /// whose values are the numbers in `unit`.
    Converted {
        value: Value,
        unit: CompoundUnit,
        shown: Value,
    },
    /// A solve-block constraint, accepted for its block. It shows no value.
    Accepted,
    /// The statement failed.
    Error(EvalError),
}

/// A worksheet: an environment plus the statements run so far.
pub struct Session {
    pub(super) env: Env,
    /// Work units charged by the statements run so far, summed over the statements (see
    /// `work::WORKSHEET_MAX_WORK`).
    pub(super) work_spent: u64,
}

impl Session {
    pub fn new() -> Self {
        Self {
            env: Env::new(),
            work_spent: 0,
        }
    }

    /// A worksheet whose file functions use `files` (see [`FileOptions`]). `Session::new` has no
    /// file access.
    pub fn with_file_options(files: FileOptions) -> Self {
        let mut env = Env::new();
        env.set_file_options(files);
        Self { env, work_spent: 0 }
    }

    pub fn env(&self) -> &Env {
        &self.env
    }

    /// Work units charged by the statements run so far, in all. A worksheet compares this with
    /// `WORKSHEET_MAX_WORK`. A solve block counts its whole budget once, when it has run.
    pub fn work_spent(&self) -> u64 {
        self.work_spent
    }

    /// The column of a range variable shown on its own (`t=`), or `None` for any other
    /// expression. This is the one safe use of a range variable (see the module docs).
    fn range_column(&self, expr: &Expr) -> Option<Value> {
        let Expr::Id(id) = strip_parens(expr) else {
            return None;
        };
        if id.kind == IdKind::Unit {
            return None;
        }
        let key = var_key(id).ok()?;
        self.env
            .range_variable(&key)
            .map(|column| Value::Matrix(column.clone()))
    }

    /// Runs one statement against the current worksheet, with a full budget of its own.
    pub fn run(&mut self, stmt: &Expr) -> Outcome {
        let budget = Budget::new();
        let outcome = self.run_budget(stmt, &budget);
        self.work_spent += budget.spent();
        outcome
    }

    /// [`Session::run`] with the statement's calls and work charged to `budget`. A solve block runs
    /// its guesses this way, so they share the block's budget with its solver members.
    pub(super) fn run_budget(&mut self, stmt: &Expr, budget: &Budget) -> Outcome {
        match stmt {
            Expr::Define { target, expr } | Expr::GlobalDefine { target, expr } => {
                self.define(target, expr, budget)
            }
            Expr::Eval {
                expr,
                unit_override,
            } => self.evaluation(expr, unit_override.as_deref(), budget),
            other => into_outcome(evaluate_iterated(other, &self.env, budget), Outcome::Value),
        }
    }

    fn define(&mut self, target: &Expr, rhs: &Expr, budget: &Budget) -> Outcome {
        match target {
            // `y_i := expr` with `i` a range variable builds the array `y` (see `range_iter`).
            Expr::Id(id) => match range_subscript(id, &self.env) {
                Some(index) => {
                    let indices = vec![index.to_string()];
                    self.define_array(id.name.clone(), indices, rhs, budget)
                }
                None => self.define_variable(id, rhs, budget),
            },
            // `X[r,c := expr` with range variables as indices builds the array `X`.
            Expr::Index { base, indices } => match (&**base, index_names(indices, &self.env)) {
                (Expr::Id(id), Some(names)) if id.subscript.is_none() => {
                    self.define_array(id.name.clone(), names, rhs, budget)
                }
                _ => Outcome::Error(EvalError::Unsupported(
                    "only names and function calls can be defined".to_string(),
                )),
            },
            Expr::Call { func, args } => match (var_key(func), parameter_names(args)) {
                (Ok(key), Ok(params)) => self.define_function(key, params, rhs),
                (Err(e), _) | (_, Err(e)) => Outcome::Error(e),
            },
            // A vector of unknowns solved symbolically (`[a, b] := [...] -> solve, a, b`): the
            // solution column is shared out, element i to name i.
            Expr::Matrix { elements, .. } if is_symbolic(rhs) => {
                self.define_vector(elements, rhs, budget)
            }
            _ => Outcome::Error(EvalError::Unsupported(
                "only names and function calls can be defined".to_string(),
            )),
        }
    }

    /// Builds the array `base` from an indexed definition over the ranges `indices`. A failed
    /// definition leaves `base` undefined, as a failed variable definition does.
    fn define_array(
        &mut self,
        base: String,
        indices: Vec<String>,
        rhs: &Expr,
        budget: &Budget,
    ) -> Outcome {
        match array_definition(&self.env, &base, &indices, rhs, budget) {
            Ok(array) => {
                self.env
                    .set_variable(base.clone(), super::value::Value::Matrix(array));
                Outcome::Defined {
                    name: base,
                    display: None,
                }
            }
            Err(e) => {
                self.env.mark_failed(&base);
                Outcome::Error(e)
            }
        }
    }

    /// Defines each name of a vector target from a symbolic result with as many values. Every name
    /// is marked failed when the evaluation fails, as a plain variable definition does.
    fn define_vector(&mut self, targets: &[Expr], rhs: &Expr, budget: &Budget) -> Outcome {
        let keys: Result<Vec<String>, EvalError> = targets
            .iter()
            .map(|t| match t {
                Expr::Id(id) => var_key(id),
                _ => Err(EvalError::Unsupported(
                    "only names can be defined".to_string(),
                )),
            })
            .collect();
        let keys = match keys {
            Ok(keys) => keys,
            Err(e) => return Outcome::Error(e),
        };
        match evaluate_budget(rhs, &self.env, budget) {
            Ok(Value::Matrix(column)) if column.data().len() == keys.len() => {
                for (key, value) in keys.iter().zip(column.data()) {
                    self.env.set_variable(key.clone(), Value::Scalar(*value));
                }
                Outcome::Defined {
                    name: keys.join(", "),
                    display: None,
                }
            }
            Ok(_) => Outcome::Error(EvalError::Unsupported(
                "symbolic result does not match the vector of names".to_string(),
            )),
            Err(e) => {
                for key in &keys {
                    self.env.mark_failed(key);
                }
                Outcome::Error(e)
            }
        }
    }

    /// Defines function `key` (the callee's full name, subscript included). Every free name of
    /// the body should be known now: a parameter, a constant, a unit, a builtin, the function
    /// itself, or a variable or function defined above. If one is not, the definition reports
    /// the first unknown name, but the function is still stored with that name recorded as
    /// unbound in its closure, so a call fails with it (see the module docs).
    fn define_function(&mut self, key: String, params: Vec<String>, rhs: &Expr) -> Outcome {
        let closure = capture(&key, &params, rhs, &self.env);
        let error = first_undefined(&closure);
        self.env.set_function(
            key.clone(),
            UserFunction {
                params,
                body: rhs.clone(),
                closure,
            },
        );
        match error {
            Some(e) => Outcome::Error(e),
            None => Outcome::Defined {
                name: key,
                display: None,
            },
        }
    }

    fn define_variable(&mut self, id: &Id, rhs: &Expr, budget: &Budget) -> Outcome {
        let key = match var_key(id) {
            Ok(key) => key,
            Err(e) => return Outcome::Error(e),
        };
        // A symbolic or transposing definition that uses its own name shows its result but does not
        // rebind the name: it keeps an earlier value, or stays undefined when it had none. Prime
        // does this: `C':=C'^T` shows the transpose and C' stays a column (16_rata 16, 17, 7_rata
        // 16); `Ax:=0==Ax -> solve, Ax` shows 0 and Ax stays undefined (Mechanik_3 8, and Q_1 is then
        // undefined in 21). A plain numeric `q:=q+1` rebinds q (see `self_reference_keeps_name`).
        if uses_variable(rhs, &key) && self_reference_keeps_name(rhs) {
            let earlier = self.env.variable(&key).is_some();
            let result = evaluate_budget(rhs, &self.env, budget);
            if !earlier {
                self.env.mark_failed(&key);
            }
            return match result {
                Ok(value) => {
                    let shown =
                        display_of(rhs, &value).unwrap_or_else(|| Box::new(Outcome::Value(value)));
                    Outcome::Defined {
                        name: key,
                        display: Some(shown),
                    }
                }
                Err(e) => Outcome::Error(e),
            };
        }
        let is_range = is_range_definition(rhs);
        match evaluate_budget(rhs, &self.env, budget) {
            Ok(value) => {
                let display = display_of(rhs, &value);
                match value {
                    Value::Matrix(column) if is_range => self.env.set_range(key.clone(), column),
                    value => self.env.set_variable(key.clone(), value),
                }
                Outcome::Defined { name: key, display }
            }
            Err(e) => {
                self.env.mark_failed(&key);
                Outcome::Error(e)
            }
        }
    }

    fn evaluation(&self, expr: &Expr, unit_override: Option<&Expr>, budget: &Budget) -> Outcome {
        let result = match self.range_column(expr) {
            Some(column) => Ok(column),
            None => evaluate_iterated(expr, &self.env, budget),
        };
        match result {
            Ok(value) => shown_outcome(value, unit_override),
            Err(e) => Outcome::Error(e),
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

fn into_outcome(r: Result<Value, EvalError>, ok: fn(Value) -> Outcome) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => Outcome::Error(e),
    }
}

/// What an evaluation shows for `value`: the value itself, or the value in the override unit.
fn shown_outcome(value: Value, unit_override: Option<&Expr>) -> Outcome {
    let Some(unit_expr) = unit_override else {
        return Outcome::Value(value);
    };
    let converted = unit_from_expr(unit_expr).and_then(|unit| {
        let shown = shown_in(&value, &unit)?;
        Ok((unit, shown))
    });
    match converted {
        Ok((unit, shown)) => Outcome::Converted { value, unit, shown },
        Err(e) => Outcome::Error(e),
    }
}

/// What a definition's right-hand side shows, when it is an evaluation (possibly parenthesized)
/// or a symbolic evaluation, which shows its result. The override was already checked when `value`
/// was evaluated, so this cannot fail.
fn display_of(rhs: &Expr, value: &Value) -> Option<Box<Outcome>> {
    match strip_parens(rhs) {
        Expr::Eval { unit_override, .. } => Some(Box::new(shown_outcome(
            value.clone(),
            unit_override.as_deref(),
        ))),
        Expr::SymEval { .. } => Some(Box::new(Outcome::Value(value.clone()))),
        _ => None,
    }
}

/// Whether a self-referencing definition keeps the old value of its name: its right-hand side is a
/// symbolic evaluation or a transpose. The samples show only these cases (see `define_variable`).
fn self_reference_keeps_name(rhs: &Expr) -> bool {
    matches!(
        strip_parens(rhs),
        Expr::SymEval { .. }
            | Expr::Unary {
                op: UnaryOp::Transpose,
                ..
            }
    )
}

/// `expr` without its enclosing parentheses.
fn strip_parens(expr: &Expr) -> &Expr {
    let mut e = expr;
    while let Expr::Parens(inner) = e {
        e = inner;
    }
    e
}

/// Whether a definition's right-hand side is a range: `a, b .. c`, possibly parenthesized or
/// shown with `=` (`(a..c=)`).
fn is_range_definition(rhs: &Expr) -> bool {
    let mut e = strip_parens(rhs);
    if let Expr::Eval { expr, .. } = e {
        e = strip_parens(expr);
    }
    matches!(e, Expr::Range { .. })
}

/// Whether a definition's right-hand side is a symbolic evaluation.
fn is_symbolic(rhs: &Expr) -> bool {
    matches!(strip_parens(rhs), Expr::SymEval { .. })
}

/// The error for the first free name a definition uses that is not yet known, if any. Variables
/// are reported before functions, each in name order so the choice is deterministic. A solver
/// name (`find`, `minerr`, ...) is reported as it is at a use outside a block, so `k(x) := find(x)`
/// and `q := find(x)` give the same error.
fn first_undefined(closure: &Closure) -> Option<EvalError> {
    if let Some(name) = closure.unbound_variables.iter().min() {
        return Some(EvalError::Undefined(name.clone()));
    }
    closure.unbound_functions.iter().min().map(|name| {
        unavailable_solver(name).unwrap_or_else(|| EvalError::UndefinedFunction(name.clone()))
    })
}

/// Plain or literally subscripted names: `f(x, y) := ...` binds parameters `x` and `y`, and
/// `f(t_1) := ...` binds `t_1`.
fn parameter_names(args: &[Expr]) -> Result<Vec<String>, EvalError> {
    let mut names: Vec<String> = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            // A literal subscript is part of the name (`t_1`), as everywhere else (`var_key`).
            Expr::Id(id) => {
                let key = var_key(id).map_err(|_| {
                    EvalError::Unsupported("function parameters must be plain names".to_string())
                })?;
                if names.contains(&key) {
                    return Err(EvalError::Unsupported(format!(
                        "parameter `{key}` appears twice"
                    )));
                }
                names.push(key);
            }
            _ => {
                return Err(EvalError::Unsupported(
                    "function parameters must be plain names".to_string(),
                ))
            }
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_names_reject_duplicates_and_accept_subscripts() {
        let dup = [Expr::var("x"), Expr::var("x")];
        assert!(parameter_names(&dup).is_err());
        let sub = [Expr::subscripted("x", Expr::num(1.0))];
        assert_eq!(parameter_names(&sub).unwrap(), vec!["x_1".to_string()]);
        let twice = [
            Expr::subscripted("x", Expr::num(1.0)),
            Expr::subscripted("x", Expr::num(1.0)),
        ];
        assert!(parameter_names(&twice).is_err());
        assert_eq!(
            parameter_names(&[Expr::var("x"), Expr::var("y")]).unwrap(),
            vec!["x".to_string(), "y".to_string()]
        );
    }

    #[test]
    fn solver_name_in_a_definition_reports_outside_a_block_both_ways() {
        // `k(x) := find(x)` and `q := find(x)` give the same error at the definition.
        let find_x = Expr::call("find", vec![Expr::var("x")]);
        let mut s = Session::new();
        let func = s.run(&Expr::Define {
            target: Box::new(Expr::call("k", vec![Expr::var("x")])),
            expr: Box::new(find_x.clone()),
        });
        assert_eq!(
            func,
            Outcome::Error(EvalError::OutsideSolveBlock("Find".into()))
        );
        let var = s.run(&Expr::Define {
            target: Box::new(Expr::var("q")),
            expr: Box::new(find_x),
        });
        assert_eq!(
            var,
            Outcome::Error(EvalError::OutsideSolveBlock("Find".into()))
        );
    }

    #[test]
    fn unknown_function_still_reports_undefined_function() {
        let mut s = Session::new();
        let out = s.run(&Expr::Define {
            target: Box::new(Expr::call("g", vec![Expr::var("x")])),
            expr: Box::new(Expr::call("nosuch", vec![Expr::var("x")])),
        });
        assert_eq!(
            out,
            Outcome::Error(EvalError::UndefinedFunction("nosuch".into()))
        );
    }
}

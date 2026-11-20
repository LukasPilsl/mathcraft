//! Range iteration (help: range_variables.md, methods_for_defining_arrays.md).
//!
//! A statement that uses range variables is evaluated once for each combination of their values,
//! with every range variable bound to one value (a local). One range gives a column (n x 1) of
//! results. Two ranges give a matrix whose rows follow the first range, in the order the variables
//! first appear in the expression. A range variable is never a vector inside the expression, so
//! `sin(i)` is the column of sines and `t + 1` is the column of `t + 1` values.
//!
//! Indexed definitions use the same loop: `y_i := expr` builds the column `y` and `X[r,c := expr`
//! builds the matrix `X`. Their index ranges must start at `ORIGIN` and count up by 1, so every
//! element of the array is set. A plain definition `y := expr` that uses a range variable is still
//! refused (see `expr_eval::range_in_expression`), as the help says.
//!
//! Limits: at most [`MAX_RANGE_VARIABLES`] range variables per expression; the product of their
//! counts is capped at `MAX_RANGE_ELEMENTS` before anything runs; every value is charged
//! `ELEMENT_UNITS` to the statement's work budget before it is evaluated, on top of the work the
//! expression itself charges. A symbolic evaluation inside an iterated expression is refused.

use super::closure::uses_base;
use super::env::Env;
use super::error::EvalError;
use super::expr_eval::{eval, var_key, Ctx};
use super::range::MAX_RANGE_ELEMENTS;
use super::value::{Matrix, Scalar, Value};
use super::work::{Budget, ELEMENT_UNITS};
use crate::expr::{Expr, Id, IdKind, Stmt};

/// Range variables one expression may use (one for a column, two for a matrix).
pub const MAX_RANGE_VARIABLES: usize = 2;

/// One range variable being iterated: its name and its values in order.
#[derive(Clone)]
pub(super) struct Axis {
    pub name: String,
    pub values: Vec<Scalar>,
}

/// What a statement uses that matters to iteration.
#[derive(Default)]
pub(super) struct Scan {
    /// Range variables the statement uses, in order of first appearance.
    pub ranges: Vec<String>,
    /// Whether the statement contains a symbolic evaluation.
    pub symbolic: bool,
}

/// The range variables an expression uses (its names are looked up in `env`), in order of first
/// appearance. Subscripts are scanned too, so `y_i` uses `i`.
pub(super) fn scan(expr: &Expr, env: &Env) -> Scan {
    let mut s = Scan::default();
    scan_into(expr, env, &mut s);
    s
}

fn scan_into(expr: &Expr, env: &Env, s: &mut Scan) {
    match expr {
        Expr::Number(_) | Expr::Str(_) | Expr::Placeholder => {}
        Expr::Id(id) => scan_id(id, env, s),
        Expr::Unary { operand, .. } => scan_into(operand, env, s),
        Expr::Binary { lhs, rhs, .. } => {
            scan_into(lhs, env, s);
            scan_into(rhs, env, s);
        }
        Expr::Call { func, args } => {
            // `vec(t)` takes the whole range, so it is not an iteration over `t` (see `range_vector`).
            if is_vec_of_range(func, args, env) {
                return;
            }
            scan_id(func, env, s);
            // A function-argument form takes a bare range variable as a whole vector (its data, see
            // `special_forms::vector_of`), so it is not an iteration over that range.
            let form = super::special_forms::lookup_form(&func.name).is_some();
            for a in args {
                if form && matches!(a, Expr::Id(id) if is_bare_range(id, env)) {
                    continue;
                }
                scan_into(a, env, s);
            }
        }
        Expr::Range { from, step, to } => {
            scan_into(from, env, s);
            if let Some(st) = step {
                scan_into(st, env, s);
            }
            scan_into(to, env, s);
        }
        Expr::Matrix { elements, .. } => elements.iter().for_each(|e| scan_into(e, env, s)),
        Expr::Index { base, indices } => {
            scan_into(base, env, s);
            indices.iter().for_each(|i| scan_into(i, env, s));
        }
        Expr::NthRoot { degree, radicand } => {
            if let Some(d) = degree {
                scan_into(d, env, s);
            }
            scan_into(radicand, env, s);
        }
        Expr::Parens(inner) => scan_into(inner, env, s),
        Expr::Define { expr, .. } | Expr::GlobalDefine { expr, .. } => scan_into(expr, env, s),
        Expr::Eval {
            expr,
            unit_override,
        } => {
            scan_into(expr, env, s);
            if let Some(u) = unit_override {
                scan_into(u, env, s);
            }
        }
        Expr::SymEval { expr, .. } => {
            s.symbolic = true;
            scan_into(expr, env, s);
        }
        Expr::Scale { value, unit } => {
            scan_into(value, env, s);
            scan_into(unit, env, s);
        }
        Expr::Program(stmts) => scan_statements(stmts, env, s),
        Expr::Accumulate { from, to, body, .. } => {
            scan_into(from, env, s);
            scan_into(to, env, s);
            scan_into(body, env, s);
        }
        Expr::Fold { operand, .. } => scan_into(operand, env, s),
        Expr::Integral {
            lower, upper, body, ..
        } => {
            scan_into(lower, env, s);
            scan_into(upper, env, s);
            scan_into(body, env, s);
        }
        Expr::Derivative { order, body, .. } => {
            if let Some(n) = order {
                scan_into(n, env, s);
            }
            scan_into(body, env, s);
        }
        Expr::Gradient { body, .. } => scan_into(body, env, s),
        Expr::Limit { target, body, .. } => {
            scan_into(target, env, s);
            scan_into(body, env, s);
        }
    }
}

/// The range variables a programming region uses. A `for` loop's range is not one of them: a loop
/// over a range variable is an iteration inside the region, not a per-value evaluation of it.
fn scan_statements(stmts: &[Stmt], env: &Env, s: &mut Scan) {
    for stmt in stmts {
        match stmt {
            Stmt::Line(e) | Stmt::Return(e) => scan_into(e, env, s),
            Stmt::Local { target, expr } => {
                scan_into(target, env, s);
                scan_into(expr, env, s);
            }
            Stmt::Branch { cond, body, .. } => {
                if let Some(c) = cond {
                    scan_into(c, env, s);
                }
                scan_statements(body, env, s);
            }
            Stmt::For { body, .. } => scan_statements(body, env, s),
            Stmt::While { cond, body } => {
                scan_into(cond, env, s);
                scan_statements(body, env, s);
            }
            Stmt::Break | Stmt::Continue => {}
            Stmt::Try { body, on_error } => {
                scan_statements(body, env, s);
                scan_statements(on_error, env, s);
            }
        }
    }
}

fn scan_id(id: &Id, env: &Env, s: &mut Scan) {
    if id.kind == IdKind::Unit {
        return;
    }
    if let Some(sub) = &id.subscript {
        scan_into(sub, env, s);
    }
    if let Ok(key) = var_key(id) {
        if env.is_range(&key) && !s.ranges.contains(&key) {
            s.ranges.push(key);
        }
    }
}

/// The axes of the named range variables, in the order given. Copying the values costs one unit
/// each, charged before the copy.
fn env_axes(env: &Env, names: &[String], budget: &Budget) -> Result<Vec<Axis>, EvalError> {
    if names.is_empty() || names.len() > MAX_RANGE_VARIABLES {
        return Err(EvalError::Unsupported(format!(
            "an expression may use one or two range variables, not {}",
            names.len()
        )));
    }
    names
        .iter()
        .map(|name| {
            let column = env
                .range_variable(name)
                .ok_or_else(|| EvalError::Undefined(name.clone()))?;
            budget.charge(column.data().len() as u64)?;
            Ok(Axis {
                name: name.clone(),
                values: column.data().to_vec(),
            })
        })
        .collect()
}

/// Builtins that take one scalar per value, so a range variable passed to them is iterated like any
/// other use: `sin(i)` is the column of sines. These are the elementary functions (help: range
/// variables are scalars in an expression). Every other builtin is refused with a range argument.
const SCALAR_PER_VALUE: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "exp", "ln", "log",
    "sqrt", "abs", "re", "im", "arg", "ln0", "floor", "ceil", "trunc", "round", "sign", "Floor",
    "Ceil", "Trunc", "Round",
];

/// Refuses a range variable passed to a builtin that takes a vector or matrix (`rows(t)`, `max(t)`,
/// `mean(t)`). Iterating it would pass one value and return a plausible wrong result, and the help
/// does not say these builtins take a range; the error points at `vec(t)`. Called only at the top
/// level (not inside a user function body), where `env` holds the worksheet's ranges.
pub(super) fn check_builtin_arguments(
    name: &str,
    args: &[Expr],
    env: &Env,
) -> Result<(), EvalError> {
    if SCALAR_PER_VALUE.contains(&name) {
        return Ok(());
    }
    if args.iter().any(|a| !scan(a, env).ranges.is_empty()) {
        return Err(EvalError::Unsupported(format!(
            "a range variable is an argument of `{name}`, which takes a vector; \
             use vec(range) to pass the range as a vector"
        )));
    }
    Ok(())
}

/// Whether `id` is a range variable named bare (no subscript, not a unit).
fn is_bare_range(id: &Id, env: &Env) -> bool {
    id.subscript.is_none() && id.kind != IdKind::Unit && env.is_range(&id.name)
}

/// Whether a call is `vec(t)` with `t` a bare range variable.
fn is_vec_of_range(func: &Id, args: &[Expr], env: &Env) -> bool {
    func.name == "vec"
        && func.subscript.is_none()
        && matches!(args, [Expr::Id(id)] if is_bare_range(id, env))
}

/// `vec(t)` for a range variable `t` (help: functions for creating vectors, "vec(x) converts a
/// range variable into a vector"): the whole range as a column, not one value of it. Returns `None`
/// when the call is any other form. The copy is charged one unit per value.
pub(super) fn range_vector(
    name: &str,
    args: &[Expr],
    env: &Env,
    budget: &Budget,
) -> Option<Result<Value, EvalError>> {
    if name != "vec" {
        return None;
    }
    let [Expr::Id(id)] = args else { return None };
    if !is_bare_range(id, env) {
        return None;
    }
    let column = env.range_variable(&id.name)?;
    Some(
        budget
            .charge(column.data().len() as u64)
            .map(|()| Value::Matrix(column.clone())),
    )
}

/// Evaluates a top-level statement's expression: plainly when it uses no range variable, else once
/// for each combination of their values (see the module docs).
pub(super) fn evaluate_iterated(
    expr: &Expr,
    env: &Env,
    budget: &Budget,
) -> Result<Value, EvalError> {
    let s = scan(expr, env);
    if s.ranges.is_empty() {
        return super::expr_eval::evaluate_budget(expr, env, budget);
    }
    if s.symbolic {
        return Err(EvalError::Unsupported(
            "symbolic evaluation of a range expression".to_string(),
        ));
    }
    let axes = env_axes(env, &s.ranges, budget)?;
    iterate(expr, env, &axes, budget).map(Value::Matrix)
}

/// Evaluates `expr` once per combination of the axes' values and returns the results: a column
/// for one axis, a matrix (rows from the first axis) for two. Every result must be a scalar.
pub(super) fn iterate(
    expr: &Expr,
    env: &Env,
    axes: &[Axis],
    budget: &Budget,
) -> Result<Matrix, EvalError> {
    if axes.is_empty() || axes.len() > MAX_RANGE_VARIABLES {
        return Err(EvalError::Unsupported(
            "an expression may use one or two range variables".to_string(),
        ));
    }
    let counts: Vec<usize> = axes.iter().map(|a| a.values.len()).collect();
    if counts.contains(&0) {
        return Err(EvalError::Unsupported("range is empty".to_string()));
    }
    let total = counts
        .iter()
        .try_fold(1usize, |acc, &n| acc.checked_mul(n))
        .filter(|&t| t <= MAX_RANGE_ELEMENTS)
        .ok_or(EvalError::TooLarge(MAX_RANGE_ELEMENTS + 1))?;
    let mut locals: Vec<(String, Value)> = axes
        .iter()
        .map(|a| (a.name.clone(), Value::Scalar(a.values[0])))
        .collect();
    let mut data = Vec::with_capacity(total);
    for flat in 0..total {
        // Row-major: the last axis changes fastest.
        let mut rest = flat;
        for (k, axis) in axes.iter().enumerate().rev() {
            let n = axis.values.len();
            locals[k].1 = Value::Scalar(axis.values[rest % n]);
            rest /= n;
        }
        budget.charge(ELEMENT_UNITS)?;
        let ctx = Ctx {
            env,
            frame: None,
            locals: &locals,
            calls: 0,
            nodes: 0,
            budget,
            scope: None,
        };
        match eval(expr, ctx)? {
            Value::Scalar(s) => data.push(s),
            _ => {
                return Err(EvalError::Unsupported(
                    "a range expression must give one scalar for each value".to_string(),
                ))
            }
        }
    }
    match counts.as_slice() {
        [n] => Matrix::new(*n, 1, data),
        [n, m] => Matrix::new(*n, *m, data),
        _ => Err(EvalError::Unsupported(
            "an expression may use one or two range variables".to_string(),
        )),
    }
}

/// The range variable named by a subscript: `y_i` with `i` a range variable gives `i`.
pub(super) fn range_subscript<'a>(id: &'a Id, env: &Env) -> Option<&'a str> {
    match id.subscript.as_deref() {
        Some(Expr::Id(sub))
            if sub.subscript.is_none() && sub.kind != IdKind::Unit && env.is_range(&sub.name) =>
        {
            Some(&sub.name)
        }
        _ => None,
    }
}

/// The range variables of an index list such as `r, c` in `X[r,c`, if every index is one.
pub(super) fn index_names(indices: &[Expr], env: &Env) -> Option<Vec<String>> {
    indices
        .iter()
        .map(|e| match e {
            Expr::Id(id)
                if id.subscript.is_none() && id.kind != IdKind::Unit && env.is_range(&id.name) =>
            {
                Some(id.name.clone())
            }
            _ => None,
        })
        .collect()
}

/// The array an indexed definition builds: `base[i] := rhs` for one index range, or
/// `base[r,c] := rhs` for two. Each index range must start at `ORIGIN` and count up by 1, so the
/// array has no gaps. The right-hand side may use only the index ranges, and may not read `base`
/// in any form (`y`, `y_i`, `y[i`): Mathcad's help documents no element-wise self reference, and
/// the array does not exist until the definition is done, so the use is refused.
pub(super) fn array_definition(
    env: &Env,
    base: &str,
    indices: &[String],
    rhs: &Expr,
    budget: &Budget,
) -> Result<Matrix, EvalError> {
    for (k, name) in indices.iter().enumerate() {
        if indices[..k].contains(name) {
            return Err(EvalError::Unsupported(format!(
                "index `{name}` appears twice"
            )));
        }
    }
    let axes = env_axes(env, indices, budget)?;
    let origin = env.origin()?;
    for axis in &axes {
        for (k, v) in axis.values.iter().enumerate() {
            let expected = (origin + k as i64) as f64;
            if v.value.im != 0.0 || v.value.re != expected {
                return Err(EvalError::Unsupported(format!(
                    "the range of index `{}` must start at ORIGIN and count up by 1",
                    axis.name
                )));
            }
        }
    }
    let s = scan(rhs, env);
    if s.symbolic {
        return Err(EvalError::Unsupported(
            "a symbolic result cannot fill an array".to_string(),
        ));
    }
    if s.ranges.iter().any(|r| !indices.contains(r)) {
        return Err(EvalError::Unsupported(
            "an indexed definition may use only its own index ranges".to_string(),
        ));
    }
    if uses_base(rhs, base) {
        return Err(EvalError::Unsupported(format!(
            "indexed definition of `{base}` refers to `{base}`"
        )));
    }
    iterate(rhs, env, &axes, budget)
}

/// The message of the error for a complex plotted value. Callers that treat such a value as a
/// gap (Prime stores NaN there) match on it.
pub const NOT_REAL_PLOT_VALUE: &str = "a plotted value must be real";

/// The real value of a result, for plots. A complex or non-finite value is an error.
fn real(s: &Scalar) -> Result<f64, EvalError> {
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(NOT_REAL_PLOT_VALUE.to_string()));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// `expr` evaluated at each value of `values`, with the range variable `range_name` bound to each
/// one in turn (the x values of a plot). Returns `(x, y)` pairs in the order of `values`, where `y`
/// is the result in SI base units. The environment's own range of that name is not used. A
/// non-finite or complex result is an error for the whole series, so callers that need gaps
/// evaluate one value at a time.
pub fn evaluate_series(
    env: &Env,
    expr: &Expr,
    range_name: &str,
    values: &Matrix,
    budget: &Budget,
) -> Result<Vec<(f64, f64)>, EvalError> {
    let axis = Axis {
        name: range_name.to_string(),
        values: values.data().to_vec(),
    };
    budget.charge(axis.values.len() as u64)?;
    let column = iterate(expr, env, std::slice::from_ref(&axis), budget)?;
    axis.values
        .iter()
        .zip(column.data())
        .map(|(x, y)| Ok((real(x)?, real(y)?)))
        .collect()
}

/// `expr` evaluated on the grid of two range variables: `rows` and `cols` each give a name and its
/// values. Returns one vector per row value (in the order of `rows`), each holding one value per
/// column value. Values are SI base units, as for [`evaluate_series`]. A non-finite or complex
/// value is an error for the whole grid, so callers that need holes evaluate smaller grids (see
/// the 3D plots in `mathcraft-import`).
pub fn evaluate_surface(
    env: &Env,
    expr: &Expr,
    rows: (&str, &Matrix),
    cols: (&str, &Matrix),
    budget: &Budget,
) -> Result<Vec<Vec<f64>>, EvalError> {
    if rows.0 == cols.0 {
        return Err(EvalError::Unsupported(format!(
            "index `{}` appears twice",
            rows.0
        )));
    }
    let axes = [
        Axis {
            name: rows.0.to_string(),
            values: rows.1.data().to_vec(),
        },
        Axis {
            name: cols.0.to_string(),
            values: cols.1.data().to_vec(),
        },
    ];
    for axis in &axes {
        budget.charge(axis.values.len() as u64)?;
    }
    let m = iterate(expr, env, &axes, budget)?;
    let (r, c) = m.shape();
    (0..r)
        .map(|i| (0..c).map(|j| real(m.get(i, j)?)).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::work::MAX_WORK;

    fn column(values: &[f64]) -> Matrix {
        let data = values
            .iter()
            .map(|&x| Scalar::real(x, crate::units::Dimension::dimensionless()))
            .collect();
        Matrix::new(values.len(), 1, data).unwrap()
    }

    fn env_with_range(name: &str, values: &[f64]) -> Env {
        let mut env = Env::new();
        env.set_range(name.to_string(), column(values));
        env
    }

    fn id_expr(name: &str) -> Expr {
        Expr::var(name)
    }

    #[test]
    fn scan_finds_subscript_ranges_in_order() {
        let mut env = Env::new();
        env.set_range("i".to_string(), column(&[0.0, 1.0]));
        env.set_range("j".to_string(), column(&[0.0, 1.0]));
        // j + y_i
        let expr = Expr::binary(
            crate::expr::BinaryOp::Add,
            id_expr("j"),
            Expr::subscripted("y", id_expr("i")),
        );
        assert_eq!(
            scan(&expr, &env).ranges,
            vec!["j".to_string(), "i".to_string()]
        );
    }

    #[test]
    fn iterate_with_a_drained_budget_is_refused() {
        let env = env_with_range("t", &[0.0, 1.0, 2.0]);
        let budget = Budget::new();
        budget.charge(MAX_WORK).unwrap();
        let axis = Axis {
            name: "t".to_string(),
            values: column(&[0.0, 1.0, 2.0]).data().to_vec(),
        };
        let r = iterate(&id_expr("t"), &env, std::slice::from_ref(&axis), &budget);
        assert_eq!(r.unwrap_err(), EvalError::WorkLimit(MAX_WORK));
    }

    #[test]
    fn each_value_charges_the_budget() {
        let env = Env::new();
        let budget = Budget::new();
        let axis = Axis {
            name: "t".to_string(),
            values: column(&[0.0, 1.0, 2.0, 3.0, 4.0]).data().to_vec(),
        };
        let before = budget.remaining();
        let m = iterate(&id_expr("t"), &env, std::slice::from_ref(&axis), &budget).unwrap();
        assert_eq!(m.shape(), (5, 1));
        assert!(before - budget.remaining() >= 5 * ELEMENT_UNITS);
    }

    #[test]
    fn copying_an_axis_is_charged() {
        let env = env_with_range("t", &[0.0, 1.0, 2.0, 3.0, 4.0]);
        let budget = Budget::new();
        budget.charge(MAX_WORK - 2).unwrap();
        let out = env_axes(&env, &["t".to_string()], &budget);
        assert!(matches!(out, Err(EvalError::WorkLimit(MAX_WORK))));
    }

    #[test]
    fn product_of_two_ranges_is_capped() {
        let mut env = Env::new();
        env.set_range("a".to_string(), column(&vec![0.0; 1000]));
        env.set_range("b".to_string(), column(&vec![0.0; 1001]));
        let expr = Expr::binary(crate::expr::BinaryOp::Add, id_expr("a"), id_expr("b"));
        assert_eq!(
            evaluate_iterated(&expr, &env, &Budget::new()).unwrap_err(),
            EvalError::TooLarge(MAX_RANGE_ELEMENTS + 1)
        );
    }

    #[test]
    fn empty_axis_is_refused_without_panic() {
        let env = Env::new();
        let axis = Axis {
            name: "t".to_string(),
            values: Vec::new(),
        };
        assert!(iterate(&id_expr("t"), &env, &[axis], &Budget::new()).is_err());
    }
}

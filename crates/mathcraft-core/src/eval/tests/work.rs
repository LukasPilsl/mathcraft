//! The work budget (`eval::work`): builtins and matrix operations are charged before they run, and
//! the charge decides the outcome deterministically. Refusals are decided before any kernel runs.

use std::time::Instant;

use super::helpers::*;
use crate::eval::work::{is_listed, listed_names};
use crate::eval::{evaluate, Env, EvalError, Matrix, Outcome, Scalar, Session, Value};
use crate::expr::Expr;
use crate::units::Dimension;

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

/// A `rows x cols` matrix bound to worksheet name `name`; element `k` is `value(k)`.
fn env_with(name: &str, rows: usize, cols: usize, value: impl Fn(usize) -> f64) -> Env {
    let data = (0..rows * cols)
        .map(|k| Scalar::real(value(k), dimless()))
        .collect();
    let mut env = Env::new();
    env.set_variable(
        name.to_string(),
        Value::Matrix(Matrix::new(rows, cols, data).expect("matrix")),
    );
    env
}

/// Element `k` of `count` values spread evenly over `[0, hi]`.
fn spread(count: usize, hi: f64) -> impl Fn(usize) -> f64 {
    move |k| hi * k as f64 / (count - 1).max(1) as f64
}

/// The column `0, step .. to`, as an expression (the range is evaluated, not referenced by name).
fn ramp(step: f64, to: f64) -> Expr {
    Expr::Range {
        from: Box::new(n(0.0)),
        step: Some(Box::new(n(step))),
        to: Box::new(n(to)),
    }
}

fn is_work_limit(got: &Result<Value, EvalError>) -> bool {
    matches!(got, Err(EvalError::WorkLimit(_)))
}

#[test]
fn j0_over_a_million_elements_fails_fast() {
    let env = env_with("M", 1000, 1000, spread(1_000_000, 100.0));
    let e = call("J0", vec![v("M")]);
    let start = Instant::now();
    let got = evaluate(&e, &env);
    let took = start.elapsed();
    assert!(is_work_limit(&got), "got {:?}", got.as_ref().err());
    assert!(took.as_millis() < 100, "refusal took {took:?}");
}

#[test]
fn j0_over_a_thirty_by_thirty_matrix_on_zero_to_100_succeeds() {
    let env = env_with("M", 30, 30, spread(900, 100.0));
    let got = evaluate(&call("J0", vec![v("M")]), &env);
    assert!(got.is_ok(), "got {:?}", got.as_ref().err());
}

#[test]
fn j0_over_a_thirty_by_thirty_matrix_on_zero_to_1e5_is_refused() {
    let env = env_with("M", 30, 30, spread(900, 1e5));
    let got = evaluate(&call("J0", vec![v("M")]), &env);
    assert!(is_work_limit(&got), "got {:?}", got.as_ref().err());
}

#[test]
fn bessel_y0_at_1e5_on_thirty_by_thirty_is_refused() {
    let env = env_with("M", 30, 30, spread(900, 1e5));
    let got = evaluate(&call("Y0", vec![v("M")]), &env);
    assert!(is_work_limit(&got), "got {:?}", got.as_ref().err());
}

#[test]
fn fresnel_c_over_thirty_by_thirty_on_zero_to_1000_is_refused() {
    let env = env_with("M", 30, 30, spread(900, 1000.0));
    let got = evaluate(&call("FresnelC", vec![v("M")]), &env);
    assert!(is_work_limit(&got), "got {:?}", got.as_ref().err());
}

#[test]
fn fresnel_c_over_five_by_five_on_zero_to_1000_succeeds() {
    let env = env_with("M", 5, 5, spread(25, 1000.0));
    let got = evaluate(&call("FresnelC", vec![v("M")]), &env);
    assert!(got.is_ok(), "got {:?}", got.as_ref().err());
}

#[test]
fn the_same_input_gets_the_same_decision() {
    let env = env_with("M", 30, 30, spread(900, 1e5));
    let e = call("J0", vec![v("M")]);
    let first = evaluate(&e, &env).map(|_| ()).err();
    let second = evaluate(&e, &env).map(|_| ()).err();
    assert_eq!(first, second);
    assert!(first.is_some());
}

#[test]
fn each_statement_gets_a_full_budget() {
    // One call of `erf` over 100001 values costs about 1.8e6 units (erf 12 units per element, the
    // range and the copy of its argument 3 each). The statement below makes 200 of them, 3.6e8
    // units, over the budget. (It was 20 calls at 2e7 each, before erf was recalibrated from 200 to
    // 12 units per element: 20 calls are now 3.6e7 units.)
    let mut s = Session::new();
    let def = def_fn("f", &["x"], call("erf", vec![v("x")]));
    assert!(matches!(s.run(&def), Outcome::Defined { .. }));
    let heavy = (0..200)
        .map(|_| call("f", vec![ramp(1e-5, 1.0)]))
        .reduce(add)
        .expect("twenty calls");
    let refused = s.run(&evaluate_stmt(heavy));
    assert!(matches!(refused, Outcome::Error(EvalError::WorkLimit(_))));
    // The next statement starts with a full budget.
    let after = s.run(&evaluate_stmt(add(n(1.0), n(1.0))));
    assert!(matches!(after, Outcome::Value(_)), "{after:?}");
}

#[test]
fn nested_user_calls_accumulate_their_work() {
    // One call fits the budget; two hundred calls in one statement do not (see the test above for the
    // units of one call).
    let mut s = Session::new();
    assert!(matches!(
        s.run(&def_fn("f", &["x"], call("erf", vec![v("x")]))),
        Outcome::Defined { .. }
    ));
    let one = s.run(&evaluate_stmt(call("f", vec![ramp(1e-5, 1.0)])));
    assert!(matches!(one, Outcome::Value(_)), "{one:?}");
    let many = (0..200)
        .map(|_| call("f", vec![ramp(1e-5, 1.0)]))
        .reduce(add)
        .expect("twenty calls");
    let outcome = s.run(&evaluate_stmt(many));
    assert!(matches!(outcome, Outcome::Error(EvalError::WorkLimit(_))));
}

#[test]
fn a_matrix_product_over_the_work_limit_is_refused_before_it_runs() {
    // 600 x 600 times 600 x 600: 2 * 2.16e8 units, above the whole budget on its own.
    let env = env_with("M", 600, 600, |_| 0.0);
    let got = evaluate(&mul(v("M"), v("M")), &env);
    assert!(
        matches!(got, Err(EvalError::WorkLimit(_))),
        "got {:?}",
        got.as_ref().err()
    );
}

#[test]
fn every_registered_builtin_has_a_cost_class() {
    let env = Env::new();
    let mut missing: Vec<&str> = env
        .builtins()
        .names()
        .filter(|name| !is_listed(name))
        .collect();
    missing.sort_unstable();
    assert!(
        missing.is_empty(),
        "builtins without a cost class (add them to TABLE in eval/work.rs): {missing:?}"
    );
}

#[test]
fn cost_table_lists_each_name_once_and_only_registered_names() {
    let env = Env::new();
    let mut names: Vec<&str> = listed_names().collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "a name is listed twice in TABLE");
    let stale: Vec<&str> = names
        .into_iter()
        .filter(|name| env.builtins().get(name).is_none())
        .collect();
    assert!(
        stale.is_empty(),
        "TABLE names that are not registered: {stale:?}"
    );
}

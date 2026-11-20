//! Work bounds for call DAGs. Depth bounds nesting; the call budget (`MAX_CALLS`) bounds the total
//! number of user-function calls in one top-level evaluation. Definitions must stay cheap even when
//! each function calls two earlier ones.

use std::time::{Duration, Instant};

use super::helpers::*;
use crate::eval::{EvalError, Outcome, MAX_CALLS};
use crate::expr::Expr;
use crate::units::Dimension;

/// `f0(x) := x`, `f1(x) := x`, and `fk(x) := f(k-1)(x) + f(k-2)(x)` for `k` up to `last`.
/// `fk(1) = F(k+1)` (Fibonacci), and a call to `fk` makes about `2 F(k+1)` calls.
fn fibonacci_definitions(last: usize) -> Vec<Expr> {
    let mut stmts = vec![def_fn("f0", &["x"], v("x")), def_fn("f1", &["x"], v("x"))];
    for k in 2..=last {
        let body = add(
            call(&format!("f{}", k - 1), vec![v("x")]),
            call(&format!("f{}", k - 2), vec![v("x")]),
        );
        stmts.push(def_fn(&format!("f{k}"), &["x"], body));
    }
    stmts
}

#[test]
fn defining_a_thirty_function_call_dag_is_fast() {
    let stmts = fibonacci_definitions(30);
    let start = Instant::now();
    let outs = run_all(&stmts);
    let elapsed = start.elapsed();
    assert!(
        outs.iter().all(|o| matches!(o, Outcome::Defined { .. })),
        "{outs:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "defining 31 functions took {elapsed:?}"
    );
}

#[test]
fn calling_f20_gives_fibonacci_f21() {
    let mut stmts = fibonacci_definitions(20);
    stmts.push(evaluate_stmt(call("f20", vec![n(1.0)])));
    let got = expect_value(run_last(&stmts));
    assert_eq!(real_in(&got, Dimension::dimensionless()), 10946.0);
}

#[test]
fn calling_f40_is_refused_by_the_call_budget_and_the_session_recovers() {
    let mut stmts = fibonacci_definitions(40);
    stmts.push(evaluate_stmt(call("f40", vec![n(1.0)])));
    stmts.push(evaluate_stmt(call("f20", vec![n(1.0)])));
    let outs = run_all(&stmts);
    let n_defs = stmts.len() - 2;
    assert_eq!(
        expect_error(outs[n_defs].clone()),
        EvalError::CallLimit(MAX_CALLS)
    );
    // The budget is per top-level evaluation: the next statement starts with a full budget.
    let got = expect_value(outs[n_defs + 1].clone());
    assert_eq!(real_in(&got, Dimension::dimensionless()), 10946.0);
}

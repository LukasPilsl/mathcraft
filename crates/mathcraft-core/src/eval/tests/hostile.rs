//! Hostile inputs: unbounded recursion, huge allocations and deep expressions. Recursion tests
//! run on a 2 MiB thread, the size of a default Rust test thread's stack, so a stack overflow
//! would abort the test.

use super::helpers::*;
use crate::eval::{
    det, evaluate, inverse, mul as mul_values, Env, EvalError, Matrix, Scalar, Session, Value,
    MAX_CALL_DEPTH, MAX_ELEMENTS,
};
use crate::expr::Expr;
use crate::units::Dimension;

#[test]
fn mutual_recursion_returns_an_error_not_a_stack_overflow() {
    // f is defined before g, so f's definition reports the unknown g and f is stored with g
    // unbound: the cycle never forms, and calling f reports g.
    let outcome = on_small_stack(|| {
        let stmts = [
            def_fn("f", &["x"], call("g", vec![v("x")])),
            def_fn("g", &["x"], call("f", vec![v("x")])),
            evaluate_stmt(call("f", vec![n(1.0)])),
        ];
        run_all(&stmts)
    });
    assert_eq!(
        expect_error(outcome[0].clone()),
        EvalError::UndefinedFunction("g".to_string())
    );
    assert_eq!(
        expect_error(outcome[2].clone()),
        EvalError::UndefinedFunction("g".to_string())
    );
}

#[test]
fn direct_self_call_hits_the_call_limit_of_200() {
    // One node per call here, so the call limit is reached before the node limit.
    let outcome = on_small_stack(|| {
        let stmts = [
            def_fn("f", &["x"], call("f", vec![v("x")])),
            evaluate_stmt(call("f", vec![n(1.0)])),
        ];
        run_last(&stmts)
    });
    assert_eq!(expect_error(outcome), EvalError::DepthLimit(MAX_CALL_DEPTH));
}

#[test]
fn self_recursion_through_an_addition_returns_an_error() {
    // Two nodes per call: the node limit trips first, which is also an error.
    let outcome = on_small_stack(|| {
        let stmts = [
            def_fn("f", &["x"], add(call("f", vec![v("x")]), n(1.0))),
            evaluate_stmt(call("f", vec![n(1.0)])),
        ];
        run_last(&stmts)
    });
    assert!(matches!(expect_error(outcome), EvalError::DepthLimit(_)));
}

#[test]
fn chain_of_calls_is_bounded_by_the_call_limit() {
    // f0(x) := x, f1(x) := f0(x), ... : 250 wrappers, one node each. Only the call limit is hit.
    let outcome = on_small_stack(|| {
        let mut stmts = vec![def_fn("w0", &["x"], v("x"))];
        for k in 1..=250 {
            stmts.push(def_fn(
                &format!("w{k}"),
                &["x"],
                call(&format!("w{}", k - 1), vec![v("x")]),
            ));
        }
        stmts.push(evaluate_stmt(call("w250", vec![n(1.0)])));
        run_last(&stmts)
    });
    assert_eq!(expect_error(outcome), EvalError::DepthLimit(MAX_CALL_DEPTH));
}

#[test]
fn bounded_nesting_of_calls_still_evaluates() {
    let outcome = on_small_stack(|| {
        let stmts = [
            def_fn("g", &["x"], add(v("x"), n(1.0))),
            def_fn("f", &["x"], call("g", vec![call("g", vec![v("x")])])),
            evaluate_stmt(call("f", vec![n(1.0)])),
        ];
        run_last(&stmts)
    });
    close(
        real_in(
            &expect_value(outcome),
            crate::units::Dimension::dimensionless(),
        ),
        3.0,
        0.0,
    );
}

#[test]
fn matrix_product_over_the_element_limit_is_refused() {
    // 100000x1 times 1x100000 would need 10^10 elements.
    let column = mat(100_000, 1, vec![n(1.0); 100_000]);
    let row = mat(1, 100_000, vec![n(1.0); 100_000]);
    let e = mul(column, row);
    let got = evaluate(&e, &Env::new());
    assert!(matches!(got, Err(EvalError::TooLarge(_))), "got {got:?}");
}

#[test]
fn matrix_literal_over_the_element_limit_is_refused() {
    let count = MAX_ELEMENTS + 1;
    let e = Expr::Matrix {
        rows: count,
        cols: 1,
        elements: vec![n(0.0); count],
    };
    assert!(matches!(
        evaluate(&e, &Env::new()),
        Err(EvalError::TooLarge(_))
    ));
}

#[test]
fn huge_declared_shape_with_few_elements_is_refused_without_allocating() {
    let e = Expr::Matrix {
        rows: usize::MAX,
        cols: 2,
        elements: vec![n(0.0)],
    };
    assert!(matches!(
        evaluate(&e, &Env::new()),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn deep_expression_of_ninety_levels_evaluates() {
    let mut e = n(1.0);
    for _ in 0..90 {
        e = Expr::Parens(Box::new(add(e, n(1.0))));
    }
    let got = expect_value(run_last(&[evaluate_stmt(e)]));
    close(
        real_in(&got, crate::units::Dimension::dimensionless()),
        91.0,
        0.0,
    );
}

#[test]
fn deeper_than_the_node_limit_is_an_error_not_a_stack_overflow() {
    let outcome = on_small_stack(|| {
        let mut e = n(1.0);
        for _ in 0..(crate::eval::MAX_NODE_DEPTH + 500) {
            e = Expr::Parens(Box::new(e));
        }
        evaluate(&e, &Env::new())
    });
    assert!(
        matches!(outcome, Err(EvalError::DepthLimit(_))),
        "got {outcome:?}"
    );
}

#[test]
fn session_survives_hostile_statement_then_continues() {
    let mut s = Session::new();
    let bad = def_fn("f", &["x"], call("f", vec![v("x")]));
    let _ = s.run(&bad);
    let out = s.run(&evaluate_stmt(call("f", vec![n(1.0)])));
    assert_eq!(expect_error(out), EvalError::DepthLimit(MAX_CALL_DEPTH));
    let after = s.run(&evaluate_stmt(add(n(1.0), n(1.0))));
    close(
        real_in(
            &expect_value(after),
            crate::units::Dimension::dimensionless(),
        ),
        2.0,
        0.0,
    );
}

// ---- work limits: time is bounded, not only element counts ---------------------------------

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

fn zeros(rows: usize, cols: usize) -> Matrix {
    let data = vec![Scalar::real(0.0, dimless()); rows * cols];
    Matrix::new(rows, cols, data).expect("matrix")
}

#[test]
fn product_over_the_work_limit_is_refused_before_any_multiplication() {
    // 800 x 800 times 800 x 800: 5.12e8 multiplications, 1.02e9 units, over the 3e8 budget, with
    // only 640000 elements in each operand.
    let a = Value::Matrix(zeros(800, 800));
    let b = Value::Matrix(zeros(800, 800));
    assert!(matches!(mul_values(&a, &b), Err(EvalError::WorkLimit(_))));
}

#[test]
fn determinant_and_inverse_over_the_work_limit_are_refused() {
    // det: 2000^3 / 21 = 3.8e8 units > 3e8 (the elimination class is 1/21 unit per n^3, measured
    // 0.7 ns per flop; it was 2/3 n^3 before the recalibration, which refused 800). inverse:
    // 2000^3 / 8 = 1e9 units.
    let m = zeros(2000, 2000);
    assert!(matches!(det(&m), Err(EvalError::WorkLimit(_))));
    assert!(matches!(inverse(&m), Err(EvalError::WorkLimit(_))));
}

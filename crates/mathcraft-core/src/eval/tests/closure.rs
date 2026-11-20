//! Function scope. A user function sees the names it captured when it was defined (Mathcad's
//! rule, `about_user-defined_functions`): later variables are undefined, captured variables keep
//! their definition-time values, and functions defined above are used as they were then.

use super::helpers::*;
use crate::eval::{EvalError, Outcome};
use crate::expr::Expr;
use crate::units::Dimension;

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

fn number_at(outs: &[Outcome], i: usize) -> f64 {
    real_in(&expect_value(outs[i].clone()), dimless())
}

#[test]
fn variable_defined_below_the_function_fails_the_definition() {
    // `y` is unknown when f is defined, so the definition reports `y`, but f is still stored: a
    // later call evaluates the body and fails on `y`, even though `y` is defined by then.
    let stmts = [
        def_fn("f", &["x"], add(v("x"), v("y"))),
        def("y", n(2.0)),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(
        expect_error(outs[0].clone()),
        EvalError::Undefined("y".to_string())
    );
    assert_eq!(
        expect_error(outs[2].clone()),
        EvalError::Undefined("y".to_string())
    );
}

#[test]
fn variable_defined_above_the_function_is_fine_later() {
    // The same worksheet with the order fixed: y:=2; f(x):=x+y; y:=5; f(1) = 3.
    let stmts = [
        def("y", n(2.0)),
        def_fn("f", &["x"], add(v("x"), v("y"))),
        def("y", n(5.0)),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert_eq!(number_at(&run_all(&stmts), 3), 3.0);
}

#[test]
fn variable_keeps_its_value_at_definition() {
    let stmts = [
        def("y", n(2.0)),
        def_fn("f", &["x"], add(v("x"), v("y"))),
        def("y", n(5.0)),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert_eq!(number_at(&run_all(&stmts), 3), 3.0);
}

#[test]
fn parameter_shadows_a_captured_name() {
    let stmts = [
        def("y", n(2.0)),
        def_fn("f", &["y"], add(v("y"), n(1.0))),
        def("y", n(5.0)),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert_eq!(number_at(&run_all(&stmts), 3), 2.0);
}

#[test]
fn nested_call_keeps_the_callee_it_saw() {
    let stmts = [
        def_fn("g", &["x"], mul(v("x"), n(10.0))),
        def_fn("f", &["x"], add(call("g", vec![v("x")]), n(1.0))),
        def_fn("g", &["x"], mul(v("x"), n(100.0))),
        evaluate_stmt(call("f", vec![n(2.0)])),
        evaluate_stmt(call("g", vec![n(2.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(number_at(&outs, 3), 21.0);
    assert_eq!(number_at(&outs, 4), 200.0);
}

#[test]
fn function_calling_one_defined_below_fails_the_definition() {
    // g is not defined when f is, so f's definition reports g, and f stays stored: calling it
    // reports g even after g is defined.
    let stmts = [
        def_fn("f", &["x"], call("g", vec![v("x")])),
        def_fn("g", &["x"], v("x")),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(
        expect_error(outs[0].clone()),
        EvalError::UndefinedFunction("g".to_string())
    );
    assert_eq!(
        expect_error(outs[2].clone()),
        EvalError::UndefinedFunction("g".to_string())
    );
}

#[test]
fn redefinition_cannot_close_a_cycle() {
    // f captured the g defined above it, so g's later definition, which calls f, never
    // reaches f's body: the evaluation is bounded and gives g's old value.
    let stmts = [
        def_fn("g", &["x"], v("x")),
        def_fn("f", &["x"], call("g", vec![v("x")])),
        def_fn("g", &["x"], call("f", vec![v("x")])),
        evaluate_stmt(call("f", vec![n(1.0)])),
        evaluate_stmt(call("g", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(number_at(&outs, 3), 1.0);
    assert_eq!(number_at(&outs, 4), 1.0);
}

#[test]
fn captured_matrix_and_unit_values_keep_their_definition() {
    let stmts = [
        def("M", mat(1, 2, vec![n(1.0), n(2.0)])),
        def("L", scale(3.0, u("km"))),
        def_fn("g", &["x"], mul(index(v("M"), vec![n(1.0)]), v("L"))),
        def("M", mat(1, 2, vec![n(7.0), n(8.0)])),
        def("L", scale(1.0, u("m"))),
        evaluate_stmt(call("g", vec![n(0.0)])),
    ];
    let got = expect_value(run_all(&stmts)[5].clone());
    // element 1 of [1, 2] is 2; times 3 km = 6000 m (SI)
    close(real_in(&got, unit_dim("m")), 6000.0, 1e-9);
}

#[test]
fn captured_matrix_is_returned_whole() {
    let stmts = [
        def("M", mat(1, 2, vec![n(1.0), n(2.0)])),
        def_fn("k", &["x"], v("M")),
        def("M", mat(1, 2, vec![n(7.0), n(8.0)])),
        evaluate_stmt(call("k", vec![n(0.0)])),
    ];
    let got = expect_value(run_all(&stmts)[3].clone());
    assert_eq!(matrix(&got).get(0, 0).unwrap().value.re, 1.0);
}

#[test]
fn origin_is_fixed_at_definition() {
    let stmts = [
        def("ORIGIN", n(1.0)),
        def("M", mat(1, 2, vec![n(10.0), n(20.0)])),
        def_fn("first", &["x"], index(v("M"), vec![n(1.0)])),
        def("ORIGIN", n(0.0)),
        evaluate_stmt(call("first", vec![n(0.0)])),
    ];
    assert_eq!(number_at(&run_all(&stmts), 4), 10.0);
}

#[test]
fn self_call_is_recursion_on_the_new_definition() {
    // The second f calls itself; it does not reach the old f (which would give 1).
    let stmts = [
        def_fn("f", &["x"], v("x")),
        def_fn("f", &["x"], add(call("f", vec![v("x")]), n(1.0))),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert!(matches!(
        expect_error(run_last(&stmts)),
        EvalError::DepthLimit(_)
    ));
}

#[test]
fn builtin_name_inside_its_own_definition_is_the_builtin() {
    // sin(x) := sin(0 x): the inner sin is the builtin, so this is not recursion.
    let body = call("sin", vec![mul(v("x"), n(0.0))]);
    let stmts = [
        def_fn("sin", &["x"], body),
        evaluate_stmt(call("sin", vec![n(5.0)])),
    ];
    assert_eq!(number_at(&run_all(&stmts), 1), 0.0);
}

#[test]
fn recursive_body_without_a_base_case_is_refused() {
    // There is no conditional, so every recursive definition here is unbounded.
    let body: Expr = add(call("f", vec![v("x")]), n(1.0));
    let stmts = [
        def_fn("f", &["x"], body),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert!(matches!(
        expect_error(run_last(&stmts)),
        EvalError::DepthLimit(_)
    ));
}

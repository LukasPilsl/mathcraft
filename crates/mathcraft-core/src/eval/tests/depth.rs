//! Native-stack bound on expression nesting. Every recursive path gets two checks on a 2 MiB
//! thread (the default test stack): nesting of `MAX_NODE_DEPTH - 1` evaluates, and nesting of
//! `MAX_NODE_DEPTH + 1` is `DepthLimit`. Debug-build frames are the ones that matter here.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Outcome, Session, MAX_NODE_DEPTH};
use crate::expr::Expr;

/// `layers` wrappers around a zero leaf. Each wrapper adds one level of nesting.
fn nest(layers: usize, wrap: fn(Expr) -> Expr) -> Expr {
    let mut e = n(0.0);
    for _ in 0..layers {
        e = wrap(e);
    }
    e
}

/// Checks one recursive path: just under the limit it is not `DepthLimit`, one over it is.
fn check_expression_path(wrap: fn(Expr) -> Expr) {
    let (under, over) = on_small_stack(move || {
        let under = evaluate(&nest(MAX_NODE_DEPTH - 1, wrap), &Env::new());
        let over = evaluate(&nest(MAX_NODE_DEPTH + 1, wrap), &Env::new());
        (under, over)
    });
    assert!(!matches!(under, Err(EvalError::DepthLimit(_))), "{under:?}");
    assert!(
        matches!(over, Err(EvalError::DepthLimit(_))),
        "expected DepthLimit, got {over:?}"
    );
}

/// Checks a statement that evaluates its expression (`:=` or `=`) the same way.
fn check_statement_path(wrap: fn(Expr) -> Expr, make: fn(Expr) -> Expr) {
    let (under, over): (Outcome, Outcome) = on_small_stack(move || {
        let under = Session::new().run(&make(nest(MAX_NODE_DEPTH - 1, wrap)));
        let over = Session::new().run(&make(nest(MAX_NODE_DEPTH + 1, wrap)));
        (under, over)
    });
    assert!(
        !matches!(under, Outcome::Error(EvalError::DepthLimit(_))),
        "{under:?}"
    );
    assert!(
        matches!(over, Outcome::Error(EvalError::DepthLimit(_))),
        "expected DepthLimit, got {over:?}"
    );
}

#[test]
fn binary_nesting_has_the_node_limit() {
    check_expression_path(|e| add(e, n(0.0)));
}

#[test]
fn unary_nesting_has_the_node_limit() {
    check_expression_path(neg);
}

#[test]
fn call_argument_nesting_has_the_node_limit() {
    check_expression_path(|e| call("abs", vec![e]));
}

#[test]
fn index_nesting_has_the_node_limit() {
    check_expression_path(|e| index(mat(1, 1, vec![n(0.0)]), vec![e]));
}

#[test]
fn matrix_element_nesting_has_the_node_limit() {
    check_expression_path(|e| mat(1, 1, vec![e]));
}

#[test]
fn scale_nesting_has_the_node_limit() {
    check_expression_path(|e| Expr::Scale {
        value: Box::new(e),
        unit: Box::new(u("m")),
    });
}

#[test]
fn evaluation_statement_nesting_has_the_node_limit() {
    check_statement_path(
        |e| add(e, n(0.0)),
        |e| Expr::Eval {
            expr: Box::new(e),
            unit_override: None,
        },
    );
}

#[test]
fn definition_statement_nesting_has_the_node_limit() {
    check_statement_path(|e| add(e, n(0.0)), |e| def("x", e));
}

/// `(((x=)=)=)`: each layer is an evaluation inside the one around it.
#[test]
fn nested_evaluation_has_the_node_limit() {
    check_expression_path(|e| Expr::Eval {
        expr: Box::new(e),
        unit_override: None,
    });
}

/// A nested evaluation with a unit override: the override is checked after its operand, so an
/// operand at the limit is `DepthLimit`, and below it the (dimensionless) operand fails the
/// dimension check instead.
#[test]
fn nested_evaluation_with_override_has_the_node_limit() {
    check_expression_path(|e| Expr::Eval {
        expr: Box::new(e),
        unit_override: Some(Box::new(u("m"))),
    });
}

#[test]
fn definition_of_nested_evaluations_has_the_node_limit() {
    check_statement_path(
        |e| Expr::Eval {
            expr: Box::new(e),
            unit_override: None,
        },
        |e| def("x", e),
    );
}

#[test]
fn ninety_nested_user_calls_on_a_small_debug_stack() {
    let got = on_small_stack(|| {
        let mut e = n(1.0);
        for _ in 0..90 {
            e = call("f", vec![e]);
        }
        let stmts = [
            def_fn("f", &["x"], add(call("abs", vec![v("x")]), n(1.0))),
            evaluate_stmt(e),
        ];
        run_last(&stmts)
    });
    close(
        real_in(&expect_value(got), crate::units::Dimension::dimensionless()),
        91.0,
        0.0,
    );
}

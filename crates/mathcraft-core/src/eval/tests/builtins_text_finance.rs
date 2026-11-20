//! Evaluator-level checks for the new families: a string literal evaluates to text, the string and
//! finance builtins are reached through the registry, `error` raises its message, and a `try` in a
//! program catches it. Value-level tests live beside each builtin (`builtins/strings.rs`,
//! `builtins/finance.rs`, `builtins/datetime.rs`).

use super::helpers::*;
use crate::eval::value::Value;
use crate::eval::{evaluate, Env, EvalError};
use crate::expr::{Expr, Stmt};

fn text(s: &str) -> Expr {
    Expr::Str(s.to_string())
}

fn eval_ok(e: &Expr) -> Value {
    evaluate(e, &Env::new()).unwrap_or_else(|err| panic!("evaluate failed: {err}"))
}

fn eval_err(e: &Expr) -> EvalError {
    evaluate(e, &Env::new()).expect_err("expected an error")
}

#[test]
fn string_literal_evaluates_to_text() {
    assert_eq!(eval_ok(&text("yes")), Value::Str("yes".to_string()));
}

#[test]
fn string_functions_reach_the_registry() {
    let len = eval_ok(&call("strlen", vec![text("héllo")]));
    close(scalar(&len).value.re, 5.0, 0.0);
    assert_eq!(
        eval_ok(&call("concat", vec![text("a"), text("b")])),
        Value::Str("ab".to_string())
    );
    assert_eq!(
        eval_ok(&call("substr", vec![text("hello"), n(1.0), n(3.0)])),
        Value::Str("ell".to_string())
    );
}

#[test]
fn strings_are_refused_by_arithmetic() {
    assert!(matches!(
        eval_err(&add(text("a"), n(1.0))),
        EvalError::Unsupported(_) | EvalError::DimensionMismatch { .. }
    ));
}

#[test]
fn error_raises_its_message() {
    assert_eq!(
        eval_err(&call("error", vec![text("bad input")])),
        EvalError::UserMessage("bad input".to_string())
    );
}

#[test]
fn try_in_a_program_catches_error() {
    // try: error("boom") ; on error: r ← 5 ; r
    let program = Expr::Program(vec![
        Stmt::Try {
            body: vec![Stmt::Line(call("error", vec![text("boom")]))],
            on_error: vec![Stmt::Local {
                target: v("r"),
                expr: n(5.0),
            }],
        },
        Stmt::Line(v("r")),
    ]);
    let value = eval_ok(&program);
    close(scalar(&value).value.re, 5.0, 0.0);
}

#[test]
fn finance_functions_reach_the_registry() {
    let pmt = eval_ok(&call("pmt", vec![n(0.005), n(60.0), n(10000.0)]));
    close(scalar(&pmt).value.re, -193.328, 0.001);
    assert!(matches!(
        eval_err(&call("pmt", vec![n(0.005), n(60.0)])),
        EvalError::ArgCount { .. }
    ));
}

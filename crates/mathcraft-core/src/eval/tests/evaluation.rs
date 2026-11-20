//! Evaluations inside expressions and definitions (`(x=)`, `(x ->unit=)`), unit overrides as
//! display units only, symbolic and indexed targets that stay refused, and failed definitions
//! shadowing built-in constants.

use super::helpers::*;
use crate::eval::{EvalError, Outcome, Session};
use crate::expr::{Expr, Number};
use crate::units::Dimension;

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

/// Runs statements in order and returns the session, so variables can be read afterwards.
fn session_after(stmts: &[Expr]) -> Session {
    let mut s = Session::new();
    for st in stmts {
        s.run(st);
    }
    s
}

/// The shown number of a definition's display, as a plain real.
fn displayed_real(out: &Outcome) -> f64 {
    match out {
        Outcome::Defined {
            display: Some(shown),
            ..
        } => match &**shown {
            Outcome::Value(v) => real_in(v, dimless()),
            Outcome::Converted { shown, .. } => real_in(shown, dimless()),
            other => panic!("display is not a value: {other:?}"),
        },
        other => panic!("expected a definition with a display, got {other:?}"),
    }
}

#[test]
fn nested_evaluation_is_the_value_of_its_expression() {
    // y := (2 + 3 =) * 2
    let stmts = [def("y", mul(evaluate_stmt(add(n(2.0), n(3.0))), n(2.0)))];
    let s = session_after(&stmts);
    close(
        real_in(s.env().variable("y").unwrap(), dimless()),
        10.0,
        0.0,
    );
}

#[test]
fn definition_of_an_evaluation_defines_the_variable_and_shows_it() {
    let out = run_last(&[def("x", evaluate_stmt(n(4.0)))]);
    assert!(
        matches!(&out, Outcome::Defined { name, .. } if name == "x"),
        "{out:?}"
    );
    close(displayed_real(&out), 4.0, 0.0);
    let s = session_after(&[def("x", evaluate_stmt(n(4.0)))]);
    close(real_in(s.env().variable("x").unwrap(), dimless()), 4.0, 0.0);
}

#[test]
fn unit_override_keeps_the_si_value_and_shows_the_converted_number() {
    // x := (2 m ->km=): the quantity is 2 m; the display is 0.002 km.
    let stmt = def("x", evaluate_in(scale(2.0, u("m")), u("km")));
    let out = Session::new().run(&stmt);
    close(displayed_real(&out), 0.002, 1e-15);
    let s = session_after(&[stmt]);
    close(
        real_in(s.env().variable("x").unwrap(), unit_dim("m")),
        2.0,
        0.0,
    );
}

#[test]
fn nested_override_is_checked_and_the_value_is_unchanged() {
    // (3 m ->km=) * 2 is 6 m in SI.
    let stmt = def("y", mul(evaluate_in(scale(3.0, u("m")), u("km")), n(2.0)));
    let s = session_after(&[stmt]);
    close(
        real_in(s.env().variable("y").unwrap(), unit_dim("m")),
        6.0,
        0.0,
    );
}

#[test]
fn override_with_another_dimension_fails_and_leaves_the_name_undefined() {
    // x := (2 m ->s=) has the wrong dimension: the definition fails and x stays undefined.
    let stmts = [
        def("x", evaluate_in(scale(2.0, u("m")), u("s"))),
        def("z", v("x")),
    ];
    let outs = run_all(&stmts);
    assert!(
        matches!(outs[0], Outcome::Error(EvalError::DimensionMismatch { .. })),
        "{:?}",
        outs[0]
    );
    assert!(
        matches!(&outs[1], Outcome::Error(EvalError::Undefined(name)) if name == "x"),
        "{:?}",
        outs[1]
    );
}

#[test]
fn override_on_a_matrix_checks_every_element() {
    // M := ([1 m, 2 m] ->km=) keeps both elements; a dimensionless override is refused.
    let m = mat(1, 2, vec![scale(1.0, u("m")), scale(2.0, u("m"))]);
    let s = session_after(&[def("M", evaluate_in(m.clone(), u("km")))]);
    let mat_value = s.env().variable("M").unwrap();
    assert_eq!(matrix(mat_value).cols(), 2);
    let outs = run_all(&[def("M", evaluate_in(m, Expr::num(1.0)))]);
    assert!(
        matches!(outs[0], Outcome::Error(_)),
        "a bare number is not a unit: {:?}",
        outs[0]
    );
}

#[test]
fn nested_evaluation_of_an_undefined_name_leaves_the_definition_undefined() {
    let stmts = [def("x", evaluate_stmt(v("q"))), def("z", v("x"))];
    let outs = run_all(&stmts);
    assert!(
        matches!(&outs[0], Outcome::Error(EvalError::Undefined(n)) if n == "q"),
        "{:?}",
        outs[0]
    );
    assert!(
        matches!(&outs[1], Outcome::Error(EvalError::Undefined(n)) if n == "x"),
        "{:?}",
        outs[1]
    );
}

#[test]
fn division_by_zero_inside_an_evaluation_is_reported() {
    let out = run_last(&[def("x", evaluate_stmt(div(n(1.0), n(0.0))))]);
    assert!(
        matches!(out, Outcome::Error(EvalError::DivisionByZero)),
        "{out:?}"
    );
}

#[test]
fn symbolic_evaluation_inside_a_definition_keeps_the_name() {
    let sym = Expr::SymEval {
        expr: Box::new(v("y")),
        command: None,
        result: None,
    };
    // `y` is not defined, so the symbolic result keeps it free: a symbolic value.
    let out = run_last(&[def("x", evaluate_stmt(sym))]);
    assert!(
        matches!(&out, Outcome::Defined { display: Some(d), .. } if matches!(**d, Outcome::Value(crate::eval::Value::Symbolic(_)))),
        "{out:?}"
    );
}

#[test]
fn evaluation_inside_a_function_body_is_the_value_of_its_expression() {
    // f(x) := (x*2=), then f(3) = 6.
    let stmts = [
        def_fn("f", &["x"], evaluate_stmt(mul(v("x"), n(2.0)))),
        evaluate_stmt(call("f", vec![n(3.0)])),
    ];
    let out = run_last(&stmts);
    close(real_in(&expect_value(out), dimless()), 6.0, 0.0);
}

#[test]
fn symbolic_vector_define_refuses_a_result_that_is_not_a_solution_column() {
    // `[a, b] := [x] ->`: a 1x1 matrix with a free name is not two values, so nothing is defined.
    let target = Expr::matrix(1, 2, vec![v("a"), v("b")]);
    let rhs = Expr::SymEval {
        expr: Box::new(mat(1, 1, vec![v("x")])),
        command: None,
        result: None,
    };
    let mut session = Session::new();
    let out = session.run(&Expr::Define {
        target: Box::new(target),
        expr: Box::new(rhs),
    });
    assert!(
        matches!(out, Outcome::Error(EvalError::Unsupported(_))),
        "{out:?}"
    );
    assert!(session.env().variable("a").is_none());
}

#[test]
fn vector_target_with_a_number_is_still_refused() {
    let out = Session::new().run(&Expr::Define {
        target: Box::new(Expr::matrix(1, 2, vec![v("a"), v("b")])),
        expr: Box::new(n(1.0)),
    });
    assert_eq!(
        out,
        Outcome::Error(EvalError::Unsupported(
            "only names and function calls can be defined".to_string()
        ))
    );
}

#[test]
fn indexed_target_is_refused_without_creating_the_element() {
    // v[1000000000] := 1 must fail: nothing is allocated and v stays undefined.
    let stmts = [
        Expr::Define {
            target: Box::new(index(v("v"), vec![n(1_000_000_000.0)])),
            expr: Box::new(n(1.0)),
        },
        def("z", v("v")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(
        outs[0],
        Outcome::Error(EvalError::Unsupported(
            "only names and function calls can be defined".to_string()
        ))
    );
    assert!(
        matches!(&outs[1], Outcome::Error(EvalError::Undefined(_))),
        "{:?}",
        outs[1]
    );
}

#[test]
fn a_failed_definition_shadows_the_built_in_constant_of_its_name() {
    // α is a physical constant. Once `α := 1/0` fails, α is the worksheet's undefined name.
    let before = run_last(&[evaluate_stmt(v("α"))]);
    assert!(
        matches!(before, Outcome::Value(_)),
        "α is a constant before any definition: {before:?}"
    );
    let stmts = [def("α", div(n(1.0), n(0.0))), def("z", v("α"))];
    let outs = run_all(&stmts);
    assert_eq!(outs[0], Outcome::Error(EvalError::DivisionByZero));
    assert!(
        matches!(&outs[1], Outcome::Error(EvalError::Undefined(n)) if n == "α"),
        "{:?}",
        outs[1]
    );
}

#[test]
fn a_successful_definition_after_a_failure_replaces_the_shadow() {
    let stmts = [
        def("α", div(n(1.0), n(0.0))),
        def("α", n(2.0)),
        def("z", add(v("α"), n(1.0))),
    ];
    let s = session_after(&stmts);
    close(real_in(s.env().variable("z").unwrap(), dimless()), 3.0, 0.0);
}

#[test]
fn function_capturing_a_failed_name_reports_it_at_definition_and_call() {
    let stmts = [
        def("α", div(n(1.0), n(0.0))),
        def_fn("f", &["x"], mul(v("α"), v("x"))),
        evaluate_stmt(call("f", vec![n(2.0)])),
    ];
    let outs = run_all(&stmts);
    assert!(
        matches!(&outs[1], Outcome::Error(EvalError::Undefined(n)) if n == "α"),
        "{:?}",
        outs[1]
    );
    assert!(
        matches!(&outs[2], Outcome::Error(EvalError::Undefined(n)) if n == "α"),
        "{:?}",
        outs[2]
    );
}

#[test]
fn deeply_nested_unit_override_is_a_depth_error_not_a_crash() {
    let deep = |layers: usize| {
        let mut e = u("m");
        for _ in 0..layers {
            e = Expr::Parens(Box::new(e));
        }
        e
    };
    let (under, over) = on_small_stack(move || {
        let under = Session::new().run(&evaluate_in(n(1.0), deep(399)));
        let over = Session::new().run(&evaluate_in(n(1.0), deep(401)));
        (under, over)
    });
    assert!(
        !matches!(under, Outcome::Error(EvalError::DepthLimit(_))),
        "{under:?}"
    );
    assert!(
        matches!(over, Outcome::Error(EvalError::DepthLimit(_))),
        "{over:?}"
    );
}

#[test]
fn a_number_literal_as_unit_override_is_not_a_unit_expression() {
    let out = Session::new().run(&evaluate_in(
        n(1.0),
        Expr::Number(Number::Real {
            value: 2.0,
            text: "2".to_string(),
        }),
    ));
    assert!(
        matches!(out, Outcome::Error(EvalError::Unsupported(_))),
        "{out:?}"
    );
}

#[test]
fn a_defined_evaluation_is_used_as_its_value_by_later_statements() {
    // q := (1.5 =), then p := q * 2 uses the value 1.5, not the display.
    let s = session_after(&[
        def("q", evaluate_stmt(add(n(1.0), n(0.5)))),
        def("p", mul(v("q"), n(2.0))),
    ]);
    close(real_in(s.env().variable("p").unwrap(), dimless()), 3.0, 0.0);
}

/// The value of an evaluation's outcome as a plain real.
fn value_real(out: &Outcome) -> f64 {
    match out {
        Outcome::Value(v) => real_in(v, dimless()),
        other => panic!("expected a value, got {other:?}"),
    }
}

#[test]
fn function_keeps_a_constant_captured_before_its_name_failed() {
    // fa(x):=α*x; α:=(1/0=); fa(2) still uses α's constant captured at definition.
    let stmts = [
        def_fn("fa", &["x"], mul(v("α"), v("x"))),
        def("α", evaluate_stmt(div(n(1.0), n(0.0)))),
        evaluate_stmt(call("fa", vec![n(2.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[1], Outcome::Error(EvalError::DivisionByZero));
    close(value_real(&outs[2]), 2.0 * 0.0072973525643, 1e-12);
}

#[test]
fn function_keeps_a_captured_user_variable_after_it_failed() {
    // y:=2; g(x):=x+y; y:=(1/0=); g(1) = 3
    let stmts = [
        def("y", n(2.0)),
        def_fn("g", &["x"], add(v("x"), v("y"))),
        def("y", evaluate_stmt(div(n(1.0), n(0.0)))),
        evaluate_stmt(call("g", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[2], Outcome::Error(EvalError::DivisionByZero));
    close(value_real(&outs[3]), 3.0, 0.0);
}

#[test]
fn top_level_use_of_a_name_whose_definition_failed_is_undefined() {
    // α:=(1/0=) at top level; a later use of α is Undefined, not the constant.
    let stmts = [
        def("α", evaluate_stmt(div(n(1.0), n(0.0)))),
        evaluate_stmt(v("α")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[0], Outcome::Error(EvalError::DivisionByZero));
    assert!(
        matches!(&outs[1], Outcome::Error(EvalError::Undefined(n)) if n == "α"),
        "{:?}",
        outs[1]
    );
}

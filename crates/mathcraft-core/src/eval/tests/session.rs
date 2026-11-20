//! Worksheet statements run in order through `Session`.

use super::helpers::*;
use crate::eval::{EvalError, Outcome, Session};
use crate::expr::{BinaryOp, Expr, Id, IdKind, UnaryOp};

fn defined(name: &str) -> Outcome {
    Outcome::Defined {
        name: name.to_string(),
        display: None,
    }
}

#[test]
fn worked_example_force_is_49_05_newtons() {
    // m := 5 kg; a := 9.81 m/s^2; F := m*a; F=  ->  49.05 N
    let stmts = [
        def("mass", scale(5.0, u("kg"))),
        def("a", div(scale(9.81, u("m")), pow(u("s"), n(2.0)))),
        def("F", mul(v("mass"), v("a"))),
        evaluate_stmt(v("F")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[0], defined("mass"));
    assert_eq!(outs[2], defined("F"));
    let got = expect_value(outs[3].clone());
    close(real_in(&got, unit_dim("N")), 49.05, 1e-12);
}

#[test]
fn display_override_shows_kilonewtons() {
    let stmts = [def("F", scale(49.05, u("N"))), evaluate_in(v("F"), u("kN"))];
    match run_last(&stmts) {
        Outcome::Converted { value, unit, shown } => {
            close(real_in(&value, unit_dim("N")), 49.05, 1e-12);
            assert_eq!(unit.to_string(), "kN");
            close(
                real_in(&shown, crate::units::Dimension::dimensionless()),
                0.04905,
                1e-15,
            );
        }
        other => panic!("expected a converted value, got {other:?}"),
    }
}

#[test]
fn display_override_with_compound_unit() {
    let speed = scale(36.0, u("m"));
    let stmts = [evaluate_in(
        div(speed, scale(1.0, u("s"))),
        div(u("km"), u("h")),
    )];
    match run_last(&stmts) {
        Outcome::Converted { unit, shown, .. } => {
            assert_eq!(unit.to_string(), "km/h");
            close(
                real_in(&shown, crate::units::Dimension::dimensionless()),
                129.6,
                1e-9,
            );
        }
        other => panic!("expected a converted value, got {other:?}"),
    }
}

#[test]
fn display_override_with_wrong_dimension_is_an_error() {
    let stmts = [def("F", scale(49.05, u("N"))), evaluate_in(v("F"), u("m"))];
    assert!(matches!(
        expect_error(run_last(&stmts)),
        EvalError::DimensionMismatch { .. }
    ));
}

#[test]
fn display_override_refuses_non_unit_expression() {
    let stmts = [def("x", n(2.0)), evaluate_in(v("x"), add(u("m"), u("s")))];
    assert!(matches!(
        expect_error(run_last(&stmts)),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn failed_definition_leaves_later_uses_undefined() {
    let stmts = [
        def("x", n(2.0)),
        def("x", div(n(1.0), n(0.0))),
        evaluate_stmt(v("x")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[1], Outcome::Error(EvalError::DivisionByZero));
    assert_eq!(
        outs[2],
        Outcome::Error(EvalError::Undefined("x".to_string()))
    );
}

#[test]
fn failed_definition_of_a_new_name_also_leaves_it_undefined() {
    let stmts = [def("y", div(n(1.0), n(0.0))), def("z", add(v("y"), n(1.0)))];
    let outs = run_all(&stmts);
    assert_eq!(
        outs[1],
        Outcome::Error(EvalError::Undefined("y".to_string()))
    );
}

#[test]
fn redefinition_affects_only_later_statements() {
    let stmts = [
        def("a", n(1.0)),
        def("b", add(v("a"), n(1.0))),
        def("a", n(10.0)),
        evaluate_stmt(v("b")),
        evaluate_stmt(v("a")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(real_in(&expect_value(outs[3].clone()), dimless()), 2.0);
    assert_eq!(real_in(&expect_value(outs[4].clone()), dimless()), 10.0);
}

fn dimless() -> crate::units::Dimension {
    crate::units::Dimension::dimensionless()
}

#[test]
fn function_with_two_parameters() {
    let stmts = [
        def_fn("f", &["x", "y"], add(pow(v("x"), n(2.0)), v("y"))),
        evaluate_stmt(call("f", vec![n(2.0), n(3.0)])),
    ];
    assert_eq!(real_in(&expect_value(run_last(&stmts)), dimless()), 7.0);
}

#[test]
fn function_keeps_units_of_its_arguments() {
    let stmts = [
        def_fn("f", &["x"], pow(v("x"), n(2.0))),
        evaluate_stmt(call("f", vec![scale(2.0, u("m"))])),
    ];
    let area = unit_dim("m").pow(2.into()).unwrap();
    assert_eq!(real_in(&expect_value(run_last(&stmts)), area), 4.0);
}

#[test]
fn function_body_is_stored_unevaluated() {
    // The body divides by zero, but defining it is fine; calling it is the error.
    let stmts = [
        def_fn("bad", &["x"], div(v("x"), n(0.0))),
        evaluate_stmt(call("bad", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[0], defined("bad"));
    assert_eq!(outs[1], Outcome::Error(EvalError::DivisionByZero));
}

#[test]
fn parameter_shadows_worksheet_variable() {
    let stmts = [
        def("x", n(100.0)),
        def_fn("f", &["x"], add(v("x"), n(1.0))),
        evaluate_stmt(call("f", vec![n(2.0)])),
        evaluate_stmt(v("x")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(real_in(&expect_value(outs[2].clone()), dimless()), 3.0);
    assert_eq!(real_in(&expect_value(outs[3].clone()), dimless()), 100.0);
}

#[test]
fn function_using_undefined_name_fails_at_definition() {
    // Prime stores an error on the definition (`y(x):=a*x^2` with a undefined), and the function
    // still exists: a call evaluates the body and fails on the undefined name.
    let stmts = [
        def_fn("f", &["x"], add(v("x"), v("later"))),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(
        expect_error(outs[0].clone()),
        EvalError::Undefined("later".to_string())
    );
    assert_eq!(
        expect_error(outs[1].clone()),
        EvalError::Undefined("later".to_string())
    );
}

#[test]
fn numeric_self_redefinition_rebinds_the_name() {
    // q := 3, then q := q+1 rebinds q to 4 (only symbolic and transposing self-references keep
    // the old value; see `define_variable`).
    let stmts = [
        def("q", n(3.0)),
        def("q", add(v("q"), n(1.0))),
        evaluate_stmt(v("q")),
    ];
    let outs = run_all(&stmts);
    close(scalar(&expect_value(outs[2].clone())).value.re, 4.0, 1e-12);
}

#[test]
fn failed_variable_definition_known_unverified() {
    // Unverified against Prime: the sample worksheets give no evidence for a failed variable
    // definition, so the engine keeps its behaviour: the name is left undefined.
    let stmts = [def("y", div(n(1.0), n(0.0))), evaluate_stmt(v("y"))];
    let outs = run_all(&stmts);
    assert_eq!(outs[0], Outcome::Error(EvalError::DivisionByZero));
    assert_eq!(
        outs[1],
        Outcome::Error(EvalError::Undefined("y".to_string()))
    );
}

#[test]
fn undefined_function_is_reported_by_name() {
    let stmts = [evaluate_stmt(call("nosuch", vec![n(1.0)]))];
    assert_eq!(
        expect_error(run_last(&stmts)),
        EvalError::UndefinedFunction("nosuch".to_string())
    );
}

#[test]
fn wrong_argument_count_for_user_function() {
    let stmts = [
        def_fn("f", &["x", "y"], add(v("x"), v("y"))),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert_eq!(
        expect_error(run_last(&stmts)),
        EvalError::ArgCount {
            name: "f".to_string(),
            expected: "2".to_string(),
            found: 1,
        }
    );
}

#[test]
fn redefining_a_function_replaces_it_for_later_statements() {
    let stmts = [
        def_fn("f", &["x"], v("x")),
        evaluate_stmt(call("f", vec![n(1.0)])),
        def_fn("f", &["x"], mul(v("x"), n(10.0))),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(real_in(&expect_value(outs[1].clone()), dimless()), 1.0);
    assert_eq!(real_in(&expect_value(outs[3].clone()), dimless()), 10.0);
}

#[test]
fn duplicate_parameter_names_are_refused() {
    let stmts = [def_fn("f", &["x", "x"], v("x"))];
    assert!(matches!(
        expect_error(run_last(&stmts)),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn vector_indexing_with_origin_one() {
    let stmts = [
        def(
            "v",
            Expr::Unary {
                op: UnaryOp::Transpose,
                operand: Box::new(mat(1, 3, vec![n(1.0), n(2.0), n(3.0)])),
            },
        ),
        evaluate_stmt(index(v("v"), vec![n(0.0)])),
        def("ORIGIN", n(1.0)),
        evaluate_stmt(index(v("v"), vec![n(1.0)])),
        evaluate_stmt(index(v("v"), vec![n(3.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(real_in(&expect_value(outs[1].clone()), dimless()), 1.0);
    assert_eq!(real_in(&expect_value(outs[3].clone()), dimless()), 1.0);
    assert_eq!(real_in(&expect_value(outs[4].clone()), dimless()), 3.0);
}

#[test]
fn matrix_element_with_origin_zero_and_one() {
    let m = mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    let stmts = [
        def("M", m),
        evaluate_stmt(index(v("M"), vec![n(1.0), n(1.0)])),
        def("ORIGIN", n(1.0)),
        evaluate_stmt(index(v("M"), vec![n(1.0), n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(real_in(&expect_value(outs[1].clone()), dimless()), 4.0);
    assert_eq!(real_in(&expect_value(outs[3].clone()), dimless()), 1.0);
}

#[test]
fn origin_must_be_an_integer_when_used() {
    let stmts = [
        def("ORIGIN", n(0.5)),
        evaluate_stmt(index(mat(1, 2, vec![n(1.0), n(2.0)]), vec![n(0.0)])),
    ];
    assert!(matches!(
        expect_error(run_last(&stmts)),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn symbolic_statement_with_a_name_is_a_symbolic_value() {
    let stmt = Expr::Eval {
        expr: Box::new(Expr::SymEval {
            expr: Box::new(v("x")),
            command: None,
            result: None,
        }),
        unit_override: None,
    };
    assert!(matches!(
        run_last(&[stmt]),
        Outcome::Value(crate::eval::Value::Symbolic(_))
    ));
}

#[test]
fn bare_expression_statement_gives_its_value() {
    let mut s = Session::new();
    let out = s.run(&add(n(1.0), n(2.0)));
    assert_eq!(real_in(&expect_value(out), dimless()), 3.0);
}

/// KNOWN GAP. Mathcad evaluates `≡` in a first pass over global definitions only, so
/// `u := 1` followed by `f1 ≡ 5 + u` fails there (`example_interleaving_of_local_and_global_definitions`).
/// This evaluator runs `≡` like `:=`, in order, so the statement succeeds and f1 is 6. When the
/// two-pass rule is implemented this assertion must flip to an error.
#[test]
fn global_definition_known_difference() {
    let stmts = [
        def("u", n(1.0)),
        Expr::GlobalDefine {
            target: Box::new(v("f1")),
            expr: Box::new(add(n(5.0), v("u"))),
        },
        evaluate_stmt(v("f1")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[1], defined("f1"));
    assert_eq!(real_in(&expect_value(outs[2].clone()), dimless()), 6.0);
}

#[test]
fn global_definition_binds_like_definition() {
    let stmts = [
        Expr::GlobalDefine {
            target: Box::new(v("g")),
            expr: Box::new(n(9.0)),
        },
        evaluate_stmt(v("g")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[0], defined("g"));
    assert_eq!(real_in(&expect_value(outs[1].clone()), dimless()), 9.0);
}

#[test]
fn definition_target_must_be_a_name_or_a_call() {
    let stmt = Expr::Define {
        target: Box::new(mat(1, 2, vec![v("a"), v("b")])),
        expr: Box::new(n(1.0)),
    };
    assert!(matches!(
        expect_error(run_last(&[stmt])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn subscripted_definition_binds_a_variable_named_with_its_subscript() {
    let stmts = [
        Expr::Define {
            target: Box::new(Expr::subscripted("v", n(0.0))),
            expr: Box::new(n(5.0)),
        },
        evaluate_stmt(Expr::subscripted("v", n(0.0))),
    ];
    let outs = run_all(&stmts);
    assert_eq!(outs[0], defined("v_0"));
    assert_eq!(real_in(&expect_value(outs[1].clone()), dimless()), 5.0);
}

#[test]
fn unit_identifier_is_the_variable_once_the_worksheet_defines_it() {
    let stmts = [
        def("m", n(7.0)),
        evaluate_stmt(Expr::Id(Id::new("m", IdKind::Unit))),
    ];
    let got = expect_value(run_last(&stmts));
    assert_eq!(real_in(&got, dimless()), 7.0);
}

#[test]
fn constant_tolerance_can_be_redefined() {
    let stmts = [def("TOL", n(0.01)), evaluate_stmt(v("TOL"))];
    assert_eq!(real_in(&expect_value(run_last(&stmts)), dimless()), 0.01);
}

#[test]
fn product_statement_multiplies() {
    let e = bin(BinaryOp::Mul, n(2.0), n(3.0));
    let mut s = Session::new();
    assert_eq!(real_in(&expect_value(s.run(&e)), dimless()), 6.0);
}

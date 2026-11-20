//! Names: subscripted function names are distinct names (`f_1` is not `f`), free names must be
//! known at a function's definition, and the built-in constants and documented unit aliases.
//! Expressions are built directly, as in the other evaluation tests.

use super::helpers::*;
use crate::eval::{EvalError, Outcome, Session, Value};
use crate::expr::{Expr, Id, IdKind};
use crate::units::{self, Dimension};

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

/// `name_sub(args)`: a call whose head has a literal subscript.
fn sub_call(name: &str, sub: f64, args: Vec<Expr>) -> Expr {
    Expr::Call {
        func: Id {
            subscript: Some(Box::new(n(sub))),
            ..Id::new(name, IdKind::Variable)
        },
        args,
    }
}

/// `name_sub(params) := body`, the definition of a subscripted function.
fn def_sub_fn(name: &str, sub: f64, params: &[&str], body: Expr) -> Expr {
    Expr::Define {
        target: Box::new(sub_call(name, sub, params.iter().map(|p| v(p)).collect())),
        expr: Box::new(body),
    }
}

/// `name_sub` as a variable (a name with a literal subscript, not an index).
fn sub_var(name: &str, sub: f64) -> Expr {
    Expr::Id(Id {
        subscript: Some(Box::new(n(sub))),
        ..Id::new(name, IdKind::Variable)
    })
}

fn number(out: &Outcome) -> f64 {
    match out {
        Outcome::Value(Value::Scalar(s)) => {
            assert_eq!(s.dim, dimless());
            s.value.re
        }
        other => panic!("expected a dimensionless value, got {other:?}"),
    }
}

#[test]
fn subscripted_functions_coexist_under_their_full_names() {
    let stmts = [
        def_sub_fn("f", 1.0, &["x"], add(v("x"), n(1.0))),
        def_sub_fn("f", 2.0, &["x"], mul(v("x"), n(10.0))),
        evaluate_stmt(sub_call("f", 1.0, vec![n(0.0)])),
        evaluate_stmt(sub_call("f", 2.0, vec![n(3.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(
        outs[0],
        Outcome::Defined {
            name: "f_1".to_string(),
            display: None,
        }
    );
    assert_eq!(
        outs[1],
        Outcome::Defined {
            name: "f_2".to_string(),
            display: None,
        }
    );
    assert_eq!(number(&outs[2]), 1.0);
    assert_eq!(number(&outs[3]), 30.0);
}

#[test]
fn subscripted_function_is_not_the_plain_function() {
    // `f_1` and `f` are different names: calling `f` after only `f_1` is defined is undefined.
    let stmts = [
        def_sub_fn("f", 1.0, &["x"], v("x")),
        evaluate_stmt(call("f", vec![n(2.0)])),
    ];
    assert_eq!(
        expect_error(run_all(&stmts)[1].clone()),
        EvalError::UndefinedFunction("f".to_string())
    );
}

#[test]
fn subscripted_function_calls_a_subscripted_function_defined_above() {
    let stmts = [
        def_sub_fn("f", 2.0, &["x"], mul(v("x"), n(10.0))),
        def_sub_fn(
            "f",
            1.0,
            &["x"],
            add(sub_call("f", 2.0, vec![v("x")]), n(1.0)),
        ),
        evaluate_stmt(sub_call("f", 1.0, vec![n(3.0)])),
    ];
    assert_eq!(number(&run_all(&stmts)[2]), 31.0);
}

#[test]
fn subscripted_recursion_is_on_the_subscripted_name() {
    // f_1 calls itself: recursion, bounded by the depth limit (no base case here).
    let stmts = [
        def_sub_fn(
            "f",
            1.0,
            &["x"],
            add(sub_call("f", 1.0, vec![v("x")]), n(1.0)),
        ),
        evaluate_stmt(sub_call("f", 1.0, vec![n(1.0)])),
    ];
    assert!(matches!(
        expect_error(run_all(&stmts)[1].clone()),
        EvalError::DepthLimit(_)
    ));
}

#[test]
fn variable_and_function_with_the_same_name_do_not_clash() {
    // Variables and functions are separate namespaces (the help's name conflicts are between
    // functions and builtins, or between two functions).
    let stmts = [
        def("f_1", n(7.0)),
        def_sub_fn("f", 1.0, &["x"], v("x")),
        evaluate_stmt(sub_call("f", 1.0, vec![n(4.0)])),
        evaluate_stmt(sub_var("f", 1.0)),
    ];
    let outs = run_all(&stmts);
    assert_eq!(number(&outs[2]), 4.0);
    assert_eq!(number(&outs[3]), 7.0);
}

#[test]
fn subscripted_name_is_not_a_builtin() {
    // `sin_1` is its own name, so `sin_1(x)` is undefined rather than the builtin sin.
    let stmts = [evaluate_stmt(sub_call("sin", 1.0, vec![n(0.0)]))];
    assert_eq!(
        expect_error(run_all(&stmts)[0].clone()),
        EvalError::UndefinedFunction("sin_1".to_string())
    );
}

#[test]
fn definition_with_an_unknown_variable_fails_at_the_definition() {
    // Prime's `y(x):=a*x^2+b*x+c` with a, b, c undefined: the error is on the definition.
    let body = add(mul(v("a"), pow(v("x"), n(2.0))), v("b"));
    let stmts = [def_fn("y", &["x"], body)];
    assert_eq!(
        expect_error(run_all(&stmts)[0].clone()),
        EvalError::Undefined("a".to_string())
    );
}

#[test]
fn definition_reports_variables_before_functions_in_name_order() {
    let body = add(call("zeta", vec![v("x")]), add(v("beta"), v("alpha")));
    let stmts = [def_fn("f", &["x"], body)];
    assert_eq!(
        expect_error(run_all(&stmts)[0].clone()),
        EvalError::Undefined("alpha".to_string())
    );
}

#[test]
fn failed_function_definition_is_reported_but_function_stays_defined() {
    // Prime keeps a function whose definition failed (3_rata_3 #1 and #2): the call evaluates
    // the body, so it fails on the first undefined name, even if that name is defined later.
    let stmts = [
        def_fn("f", &["x"], v("x")),
        def_fn("f", &["x"], add(v("x"), v("nowhere"))),
        evaluate_stmt(call("f", vec![n(1.0)])),
        def("nowhere", n(5.0)),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(
        expect_error(outs[1].clone()),
        EvalError::Undefined("nowhere".to_string())
    );
    assert_eq!(
        expect_error(outs[2].clone()),
        EvalError::Undefined("nowhere".to_string())
    );
    assert_eq!(
        expect_error(outs[4].clone()),
        EvalError::Undefined("nowhere".to_string())
    );
}

#[test]
fn definition_may_use_constants_units_builtins_and_its_own_name() {
    let body = add(
        mul(call("sin", vec![v("x")]), v("g")),
        mul(scale(2.0, u("m")), call("f", vec![v("x")])),
    );
    let stmts = [
        def_fn("f", &["x"], body),
        evaluate_stmt(call("f", vec![n(0.0)])),
    ];
    // f recurses without a base case, so the call is refused; the definition itself is accepted.
    let outs = run_all(&stmts);
    assert_eq!(
        outs[0],
        Outcome::Defined {
            name: "f".to_string(),
            display: None,
        }
    );
    assert!(matches!(
        expect_error(outs[1].clone()),
        EvalError::DepthLimit(_)
    ));
}

#[test]
fn a_later_variable_with_the_same_name_as_a_constant_shadows_it() {
    let stmts = [def("g", n(2.0)), evaluate_stmt(v("g"))];
    assert_eq!(number(&run_all(&stmts)[1]), 2.0);
}

#[test]
fn every_built_in_constant_name_parses_and_evaluates() {
    // The keys as the help writes them, through the text parser. `R_∞` is not parsed: the text
    // parser refuses `∞` as a subscript character (a known parser limit), so it is checked as the
    // name it is stored under.
    let keys = [
        "π", "e", "i", "j", "g", "c", "e_c", "h", "ħ", "k", "m_u", "N_A", "R", "α", "γ", "ε_0",
        "μ_0", "σ", "Φ_0",
    ];
    for key in keys {
        let expr = crate::parser::parse(key, &crate::parser::DefaultIdContext)
            .unwrap_or_else(|e| panic!("{key}: {e}"));
        assert_evaluates(&expr, key);
    }
    assert_evaluates(&v("R_∞"), "R_∞");
}

fn assert_evaluates(expr: &Expr, key: &str) {
    let mut session = Session::new();
    let outcome = session.run(&Expr::Eval {
        expr: Box::new(expr.clone()),
        unit_override: None,
    });
    assert!(matches!(outcome, Outcome::Value(_)), "{key}: {outcome:?}");
}

#[test]
fn sec_is_the_second() {
    let sec = units::unit("sec").expect("sec is a unit");
    let s = units::unit("s").expect("s is a unit");
    assert_eq!(sec.dimension, s.dimension);
    assert_eq!(sec.factor_to_si, 1.0);
    // The sample spelling `20 (m/sec)`, with `sec` unlabelled (a variable on import), is 20 m/s.
    let mut session = Session::new();
    let stmt = Expr::Define {
        target: Box::new(v("v_0")),
        expr: Box::new(scale(20.0, div(u("m"), v("sec")))),
    };
    assert!(matches!(session.run(&stmt), Outcome::Defined { .. }));
    match session.env().variable("v_0") {
        Some(Value::Scalar(sc)) => {
            assert_eq!(sc.value.re, 20.0);
            assert_eq!(sc.dim, Dimension::from_ints([1, 0, -1, 0, 0, 0, 0]));
        }
        other => panic!("v_0 is {other:?}"),
    }
}

#[test]
fn a_defined_variable_named_like_a_unit_stays_a_variable_in_a_unit_group() {
    // sec := 4 is a variable, so `20 (m/sec)` divides by it: 5 m, dimension length only.
    let stmts = [
        def("sec", n(4.0)),
        evaluate_stmt(scale(20.0, div(u("m"), v("sec")))),
    ];
    let got = expect_value(run_all(&stmts)[1].clone());
    close(real_in(&got, unit_dim("m")), 5.0, 1e-12);
}

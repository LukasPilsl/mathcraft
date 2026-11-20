//! Range variables: a name defined by a range shows its column on its own. A use in an expression
//! is evaluated once per value (see `range_iter`); a plain definition from one is refused.

use super::helpers::*;
use crate::eval::{EvalError, Matrix, Outcome, Value};
use crate::expr::Expr;

const REFUSED: &str = "range variable used in an expression";

fn refused() -> EvalError {
    EvalError::Unsupported(REFUSED.to_string())
}

/// `t := 0, 1 .. 3` (four values).
fn t_range() -> Expr {
    def("t", Expr::range(n(0.0), Some(n(1.0)), n(3.0)))
}

/// `t=` as a statement (the bare range variable).
fn show_t() -> Expr {
    evaluate_stmt(v("t"))
}

fn column(out: Outcome) -> Matrix {
    match expect_value(out) {
        Value::Matrix(m) => m,
        other => panic!("expected a column, got {other:?}"),
    }
}

/// Runs `t := 0,1..3` then `stmt`, returning the outcome of `stmt`.
fn after_t(stmt: Expr) -> Outcome {
    run_last(&[t_range(), stmt])
}

#[test]
fn range_variable_shows_its_column() {
    let m = column(run_last(&[t_range(), show_t()]));
    assert_eq!(m.shape(), (4, 1));
    let xs: Vec<f64> = m.data().iter().map(|s| s.value.re).collect();
    assert_eq!(xs, vec![0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn range_definition_is_defined_not_a_value() {
    let outs = run_all(&[t_range()]);
    assert_eq!(
        outs[0],
        Outcome::Defined {
            name: "t".to_string(),
            display: None
        }
    );
}

#[test]
fn range_with_units_keeps_its_dimension_and_count() {
    let stmts = [
        def(
            "t",
            Expr::range(
                scale(0.0, u("s")),
                Some(scale(0.1, u("s"))),
                scale(2.5, u("s")),
            ),
        ),
        show_t(),
    ];
    let m = column(run_last(&stmts));
    assert_eq!(m.shape(), (26, 1));
    assert_eq!(m.data()[0].dim, unit_dim("s"));
    close(m.data()[25].value.re, 2.5, 1e-9);
}

/// The values of a column, in order.
fn values(m: &Matrix) -> Vec<f64> {
    m.data().iter().map(|s| s.value.re).collect()
}

#[test]
fn range_variable_in_an_expression_is_evaluated_per_value() {
    // Help: an expression with a range variable is evaluated for each value (range_iteration.rs).
    // t = [0; 1; 2; 3], so t*t is element-wise, not a dot product, and t^2 is element-wise too.
    let squares = [0.0, 1.0, 4.0, 9.0];
    assert_eq!(
        values(&column(after_t(evaluate_stmt(mul(v("t"), v("t")))))),
        squares
    );
    assert_eq!(
        values(&column(after_t(evaluate_stmt(pow(v("t"), n(2.0)))))),
        squares
    );
    let sines = column(after_t(evaluate_stmt(call("sin", vec![v("t")]))));
    for (k, s) in values(&sines).iter().enumerate() {
        assert!((s - (k as f64).sin()).abs() < 1e-12);
    }
    let shifted = Expr::Eval {
        expr: Box::new(v("t")),
        unit_override: None,
    };
    assert_eq!(
        values(&column(after_t(evaluate_stmt(add(n(2.0), shifted))))),
        [2.0, 3.0, 4.0, 5.0]
    );
    // Indexing a scalar value of the range is still an error.
    assert_eq!(
        expect_error(after_t(evaluate_stmt(index(v("t"), vec![n(0.0)])))),
        EvalError::Unsupported("cannot index a scalar".to_string())
    );
}

#[test]
fn range_variable_as_a_function_argument_is_evaluated_per_value() {
    let stmts = [
        t_range(),
        def_fn("f", &["x"], add(v("x"), n(1.0))),
        evaluate_stmt(call("f", vec![v("t")])),
    ];
    assert_eq!(values(&column(run_last(&stmts))), [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn definition_from_a_range_variable_is_refused_and_marks_the_name_failed() {
    let stmts = [
        t_range(),
        def("u", add(v("t"), n(1.0))),
        evaluate_stmt(v("u")),
    ];
    let outs = run_all(&stmts);
    assert_eq!(expect_error(outs[1].clone()), refused());
    assert_eq!(
        expect_error(outs[2].clone()),
        EvalError::Undefined("u".to_string())
    );
}

#[test]
fn function_using_a_range_variable_is_defined_but_its_call_is_refused() {
    let stmts = [
        t_range(),
        def_fn("g", &["x"], mul(v("t"), v("x"))),
        evaluate_stmt(call("g", vec![n(2.0)])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(
        outs[1],
        Outcome::Defined {
            name: "g".to_string(),
            display: None
        }
    );
    assert_eq!(expect_error(outs[2].clone()), refused());
}

#[test]
fn redefining_a_range_as_a_variable_replaces_it() {
    let stmts = [
        t_range(),
        def("t", n(5.0)),
        evaluate_stmt(mul(v("t"), v("t"))),
    ];
    assert_eq!(scalar(&expect_value(run_last(&stmts))).value.re, 25.0);
}

#[test]
fn redefining_a_variable_as_a_range_replaces_it() {
    let stmts = [def("t", n(5.0)), t_range(), show_t()];
    assert_eq!(column(run_last(&stmts)).shape(), (4, 1));
    assert_eq!(
        values(&column(run_last(&[
            def("t", n(5.0)),
            t_range(),
            evaluate_stmt(mul(v("t"), v("t")))
        ]))),
        [0.0, 1.0, 4.0, 9.0]
    );
}

#[test]
fn failed_range_definition_leaves_the_name_undefined() {
    // A huge range is refused before allocation, and the name is then undefined.
    let huge = def("t", Expr::range(n(0.0), None, n(1e12)));
    let outs = run_all(&[huge, show_t()]);
    assert!(matches!(outs[0], Outcome::Error(EvalError::TooLarge(_))));
    assert_eq!(
        expect_error(outs[1].clone()),
        EvalError::Undefined("t".to_string())
    );
}

#[test]
fn range_with_a_name_that_is_a_unit_is_not_read_as_the_unit() {
    // `m` is a unit name; once `m` is a range variable it is the range, so `m*2` is the column.
    let stmts = [
        def("m", Expr::range(n(0.0), None, n(2.0))),
        evaluate_stmt(mul(v("m"), n(2.0))),
    ];
    assert_eq!(values(&column(run_last(&stmts))), [0.0, 2.0, 4.0]);
}

#[test]
fn a_range_directly_evaluated_still_gives_its_column() {
    let e = Expr::range(n(0.0), None, n(2.0));
    let m = column(run_last(&[evaluate_stmt(e)]));
    assert_eq!(m.shape(), (3, 1));
}

/// `A := [1, 2, 3]` as a column.
fn a_column() -> Expr {
    def("A", mat(3, 1, vec![n(1.0), n(2.0), n(3.0)]))
}

/// `ORIGIN := 0, 1 .. 3`.
fn origin_range() -> Expr {
    def("ORIGIN", Expr::range(n(0.0), Some(n(1.0)), n(3.0)))
}

#[test]
fn range_origin_is_refused_on_indexing_not_read_as_zero() {
    let stmts = [
        a_column(),
        origin_range(),
        evaluate_stmt(index(v("A"), vec![n(1.0)])),
    ];
    assert_eq!(expect_error(run_last(&stmts)), refused());
}

#[test]
fn function_defined_after_range_origin_is_refused_when_indexing() {
    let stmts = [
        a_column(),
        origin_range(),
        def_fn("f", &["i"], index(v("A"), vec![v("i")])),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert_eq!(expect_error(run_last(&stmts)), refused());
}

#[test]
fn function_defined_before_range_origin_keeps_the_captured_origin() {
    let stmts = [
        a_column(),
        def_fn("f", &["i"], index(v("A"), vec![v("i")])),
        origin_range(),
        evaluate_stmt(call("f", vec![n(1.0)])),
    ];
    assert_eq!(scalar(&expect_value(run_last(&stmts))).value.re, 2.0);
}

#[test]
fn redefining_origin_as_a_number_after_range_works_again() {
    let stmts = [
        a_column(),
        origin_range(),
        def("ORIGIN", n(1.0)),
        evaluate_stmt(index(v("A"), vec![n(1.0)])),
    ];
    assert_eq!(scalar(&expect_value(run_last(&stmts))).value.re, 1.0);
}

#[test]
fn range_tolerance_used_in_arithmetic_is_evaluated_per_value() {
    // Plain arithmetic iterates; a builtin that reads TOL or CTOL as a number is still refused.
    for name in ["TOL", "CTOL"] {
        let stmts = [
            def(name, Expr::range(n(0.0), Some(n(1.0)), n(3.0))),
            evaluate_stmt(mul(v(name), n(2.0))),
        ];
        assert_eq!(
            values(&column(run_last(&stmts))),
            [0.0, 2.0, 4.0, 6.0],
            "{name}"
        );
    }
}

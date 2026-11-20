//! Nested arrays (`Value::List`): what the factorizations return, how a list is subscripted, and
//! how operators and builtins refuse one.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Outcome, Session, Value};
use crate::expr::{BinaryOp, Expr};

fn run(e: &Expr) -> Result<Value, EvalError> {
    evaluate(e, &Env::new())
}

fn grid(rows: usize, cols: usize, vals: &[f64]) -> Expr {
    mat(rows, cols, vals.iter().map(|x| n(*x)).collect())
}

/// `LU(M)` for the help's 4 x 3 matrix M1.
fn lu_m1() -> Expr {
    call(
        "LU",
        vec![grid(
            4,
            3,
            &[
                3.0, 1.0, 2.0, 7.0, -1.0, 2.0, -2.0, 8.0, 2.0, 3.0, -1.0, 9.0,
            ],
        )],
    )
}

fn items_of(v: Value) -> Vec<Value> {
    match v {
        Value::List(items) => items,
        other => panic!("expected a list, got {other:?}"),
    }
}

fn shape_of(v: &Value) -> (usize, usize) {
    match v {
        Value::Matrix(m) => m.shape(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

#[test]
fn lu_returns_a_nested_triple_with_the_expected_shapes() {
    let items = items_of(run(&lu_m1()).expect("lu"));
    let shapes: Vec<(usize, usize)> = items.iter().map(shape_of).collect();
    assert_eq!(shapes, vec![(4, 4), (4, 3), (3, 3)]);
}

#[test]
fn a_list_subscript_gives_the_element_and_a_further_subscript_applies() {
    // ORIGIN is 0 by default: [1] is L, and [2, 0, 0] is U's first entry.
    let l = run(&index(lu_m1(), vec![n(1.0)])).expect("L");
    assert_eq!(shape_of(&l), (4, 3));
    let u00 = run(&index(index(lu_m1(), vec![n(2.0)]), vec![n(0.0), n(0.0)])).expect("U00");
    let Value::Scalar(s) = u00 else {
        panic!("expected a scalar")
    };
    assert!(s.value.re.abs() > 0.0);
}

#[test]
fn a_list_subscript_counts_from_origin() {
    let mut session = Session::new();
    session.run(&def("ORIGIN", n(1.0)));
    session.run(&def("F", lu_m1()));
    // With ORIGIN 1 the first element is index 1, so F[1 is P (4 x 4).
    let out = session.run(&evaluate_stmt(index(v("F"), vec![n(1.0)])));
    let Outcome::Value(value) = out else {
        panic!("expected a value, got {out:?}")
    };
    assert_eq!(shape_of(&value), (4, 4));
}

#[test]
fn a_list_takes_one_index_and_checks_its_range() {
    assert!(matches!(
        run(&index(lu_m1(), vec![n(0.0), n(0.0)])),
        Err(EvalError::Unsupported(msg)) if msg.contains("one index")
    ));
    assert!(matches!(
        run(&index(lu_m1(), vec![n(3.0)])),
        Err(EvalError::IndexOutOfRange { index: 3, bound: 3 })
    ));
}

#[test]
fn operators_refuse_a_list_without_panicking() {
    let list = lu_m1();
    let refused = [
        add(list.clone(), n(1.0)),
        bin(BinaryOp::Sub, n(1.0), list.clone()),
        mul(list.clone(), n(2.0)),
        div(n(1.0), list.clone()),
        pow(list.clone(), n(2.0)),
        neg(list.clone()),
        call("sin", vec![list.clone()]),
        call("abs", vec![list.clone()]),
        call("tr", vec![list.clone()]),
        call("mean", vec![list.clone()]),
        call("det", vec![list.clone()]),
    ];
    for e in refused {
        assert!(
            matches!(run(&e), Err(EvalError::Unsupported(_))),
            "{e:?} was not refused: {:?}",
            run(&e)
        );
    }
}

#[test]
fn a_list_is_not_a_matrix_argument_for_the_matrix_builtins() {
    for e in [
        call("augment", vec![lu_m1(), n(1.0)]),
        call("stack", vec![lu_m1()]),
        call("reverse", vec![lu_m1()]),
        call("sort", vec![lu_m1()]),
        call("svds", vec![lu_m1()]),
        call("QR", vec![lu_m1()]),
        call("LU", vec![lu_m1()]),
        call("svd", vec![lu_m1()]),
        call("Cholesky", vec![lu_m1()]),
    ] {
        assert!(
            matches!(run(&e), Err(EvalError::Unsupported(_))),
            "{e:?}: {:?}",
            run(&e)
        );
    }
}

#[test]
fn a_nested_result_is_a_value_and_can_be_stored_then_subscripted() {
    let mut session = Session::new();
    session.run(&def("m1", lu_m1()));
    let out = session.run(&evaluate_stmt(index(v("m1"), vec![n(2.0)])));
    let Outcome::Value(value) = out else {
        panic!("expected a value, got {out:?}")
    };
    assert_eq!(shape_of(&value), (3, 3));
}

#[test]
fn qr_and_cholesky_return_their_documented_nested_forms() {
    let qr_pivot = run(&call(
        "QR",
        vec![grid(
            4,
            3,
            &[
                3.0, 1.0, 2.0, 7.0, -1.0, 2.0, -2.0, 8.0, 2.0, 3.0, -1.0, 9.0,
            ],
        )],
    ))
    .expect("qr");
    assert_eq!(
        items_of(qr_pivot).iter().map(shape_of).collect::<Vec<_>>(),
        vec![(3, 3), (4, 4), (4, 3)]
    );
    let qr_plain = run(&call(
        "QR",
        vec![
            grid(
                4,
                3,
                &[
                    3.0, 1.0, 2.0, 7.0, -1.0, 2.0, -2.0, 8.0, 2.0, 3.0, -1.0, 9.0,
                ],
            ),
            n(0.0),
        ],
    ))
    .expect("qr");
    assert_eq!(
        items_of(qr_plain).iter().map(shape_of).collect::<Vec<_>>(),
        vec![(4, 4), (4, 3)]
    );
    let chol = run(&call(
        "Cholesky",
        vec![grid(3, 3, &[5.0, 2.0, 3.0, 2.0, 7.0, -4.0, 3.0, -4.0, 9.0])],
    ))
    .expect("cholesky");
    assert_eq!(
        items_of(chol).iter().map(shape_of).collect::<Vec<_>>(),
        vec![(3, 3), (3, 3)]
    );
}

#[test]
fn svd_of_a_wide_matrix_is_refused() {
    let wide = call("svd", vec![grid(2, 3, &[1.0; 6])]);
    assert!(
        matches!(run(&wide), Err(EvalError::Unsupported(msg)) if msg.contains("rows >= columns"))
    );
}

#[test]
fn a_list_outcome_is_a_value_in_the_session() {
    let out = Session::new().run(&evaluate_stmt(lu_m1()));
    assert!(matches!(out, Outcome::Value(Value::List(ref items)) if items.len() == 3));
}

#[test]
fn a_non_scalar_origin_or_tolerance_is_an_error_not_a_default() {
    // ORIGIN as a matrix: an index is refused, not counted from 0.
    let mut session = Session::new();
    session.run(&def("ORIGIN", call("identity", vec![n(2.0)])));
    let out = session.run(&evaluate_stmt(index(
        call("identity", vec![n(2.0)]),
        vec![n(0.0)],
    )));
    assert!(
        matches!(&out, Outcome::Error(EvalError::Unsupported(msg)) if msg.contains("ORIGIN")),
        "{out:?}"
    );
    // TOL as a matrix: rank is refused, not run with the default tolerance.
    let mut session = Session::new();
    session.run(&def("TOL", call("identity", vec![n(2.0)])));
    let out = session.run(&evaluate_stmt(call(
        "rank",
        vec![call("identity", vec![n(2.0)])],
    )));
    assert!(
        matches!(&out, Outcome::Error(EvalError::Unsupported(msg)) if msg.contains("TOL")),
        "{out:?}"
    );
}

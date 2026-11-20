//! Context builtins: `last`, `submatrix`, `csort`, `rsort` (ORIGIN-based), and `rank` and `rref`
//! reading TOL. Expected values come from the help: "Example: Effect of ORIGIN on non-Lookup
//! Functions" (the 6 x 6 matrix M below, ORIGIN 0 and 1), "Example: Sorting Functions" and the
//! "Number of Elements in a Vector" page. Captured ORIGIN and TOL are checked through function
//! definitions, the closure rule.

use super::helpers::*;
use crate::eval::work::builtin_units;
use crate::eval::{EvalError, Matrix, Scalar, Value};
use crate::expr::Expr;
use crate::units::Dimension;

pub(super) fn dimless() -> Dimension {
    Dimension::dimensionless()
}

pub(super) fn close_all(actual: &[f64], expected: &[f64], tol: f64) {
    assert_eq!(actual.len(), expected.len(), "length of {actual:?}");
    for (a, e) in actual.iter().zip(expected) {
        close(*a, *e, tol);
    }
}

pub(super) fn grid(rows: usize, cols: usize, vals: &[f64]) -> Expr {
    mat(rows, cols, vals.iter().map(|x| n(*x)).collect())
}

/// The 6 x 6 matrix of the ORIGIN help example.
pub(super) fn help_m() -> Expr {
    grid(
        6,
        6,
        &[
            0.0, 92.0, 13.0, 15.0, 14.0, 55.0, //
            221.0, 1.0, 133.0, 244.0, 25.0, 150.0, //
            131.0, 232.0, 2.0, 134.0, 13.0, 70.0, //
            91.0, 42.0, 223.0, 3.0, 45.0, 88.0, //
            15.0, 12.0, 66.0, 94.0, 4.0, 99.0, //
            144.0, 23.0, 177.0, 77.0, 66.0, 5.0,
        ],
    )
}

/// The real parts of a matrix value, asserting each element is real.
pub(super) fn reals(v: &Value) -> Vec<f64> {
    matrix(v)
        .data()
        .iter()
        .map(|s| {
            assert!(s.value.im.abs() < 1e-12, "expected real, got {}", s.value);
            s.value.re
        })
        .collect()
}

pub(super) fn dims(v: &Value) -> Vec<Dimension> {
    matrix(v).data().iter().map(|s| s.dim).collect()
}

pub(super) fn def_origin(origin: f64) -> Expr {
    def("ORIGIN", n(origin))
}

/// The value of the last statement of `stmts`.
pub(super) fn value_of(stmts: &[Expr]) -> Value {
    expect_value(run_last(stmts))
}

pub(super) fn error_of(stmts: &[Expr]) -> EvalError {
    expect_error(run_last(stmts))
}

/// `stmts` followed by `expr =`.
pub(super) fn then_eval(mut stmts: Vec<Expr>, expr: Expr) -> Vec<Expr> {
    stmts.push(evaluate_stmt(expr));
    stmts
}

pub(super) fn scalar_real(stmts: &[Expr]) -> f64 {
    real_in(&value_of(stmts), dimless())
}

const REFUSED: &str = "range variable used in an expression";

/// `name := 0, 1 .. 3`, a range variable.
pub(super) fn range_def(name: &str) -> Expr {
    def(name, Expr::range(n(0.0), Some(n(1.0)), n(3.0)))
}

// ---- last -----------------------------------------------------------------------------------

#[test]
fn last_of_a_five_vector_is_4_with_origin_0_and_5_with_origin_1() {
    let v = mat(5, 1, (0..5).map(|k| n(k as f64)).collect());
    assert_eq!(
        scalar_real(&[evaluate_stmt(call("last", vec![v.clone()]))]),
        4.0
    );
    let with_one = then_eval(vec![def_origin(1.0)], call("last", vec![v]));
    assert_eq!(scalar_real(&with_one), 5.0);
}

#[test]
fn last_refuses_row_vectors_and_matrices() {
    let row = grid(1, 3, &[1.0, 2.0, 3.0]);
    let square = grid(2, 2, &[1.0; 4]);
    for m in [row, square] {
        let e = error_of(&[evaluate_stmt(call("last", vec![m]))]);
        assert!(matches!(e, EvalError::Unsupported(_)), "{e:?}");
    }
}

// ---- submatrix ------------------------------------------------------------------------------

#[test]
fn submatrix_help_example_with_origin_0() {
    // Help: submatrix(M, 2, 3, 3, 4) = [134 13; 3 45] with ORIGIN 0.
    let got = value_of(&[evaluate_stmt(call(
        "submatrix",
        vec![help_m(), n(2.0), n(3.0), n(3.0), n(4.0)],
    ))]);
    assert_eq!(reals(&got), vec![134.0, 13.0, 3.0, 45.0]);
    assert_eq!(matrix(&got).shape(), (2, 2));
}

#[test]
fn submatrix_help_example_with_origin_1() {
    // Help: with ORIGIN 1 the same arguments give [133 244; 2 134].
    let stmts = then_eval(
        vec![def_origin(1.0)],
        call("submatrix", vec![help_m(), n(2.0), n(3.0), n(3.0), n(4.0)]),
    );
    assert_eq!(reals(&value_of(&stmts)), vec![133.0, 244.0, 2.0, 134.0]);
}

#[test]
fn submatrix_out_of_range_is_an_index_error() {
    let e = error_of(&[evaluate_stmt(call(
        "submatrix",
        vec![help_m(), n(0.0), n(6.0), n(0.0), n(0.0)],
    ))]);
    assert_eq!(e, EvalError::IndexOutOfRange { index: 6, bound: 6 });
}

#[test]
fn submatrix_below_origin_and_reversed_ranges_are_refused() {
    let below = then_eval(
        vec![def_origin(1.0)],
        call("submatrix", vec![help_m(), n(0.0), n(1.0), n(0.0), n(1.0)]),
    );
    assert!(matches!(error_of(&below), EvalError::Unsupported(_)));
    let reversed = [evaluate_stmt(call(
        "submatrix",
        vec![help_m(), n(3.0), n(2.0), n(0.0), n(1.0)],
    ))];
    assert!(matches!(error_of(&reversed), EvalError::Unsupported(_)));
}

#[test]
fn submatrix_keeps_the_unit_of_each_element() {
    let m = mat(
        2,
        2,
        vec![
            scale(1.0, u("m")),
            scale(2.0, u("m")),
            scale(3.0, u("m")),
            scale(4.0, u("m")),
        ],
    );
    let got = value_of(&[evaluate_stmt(call(
        "submatrix",
        vec![m, n(0.0), n(1.0), n(1.0), n(1.0)],
    ))]);
    assert_eq!(reals(&got), vec![2.0, 4.0]);
    assert!(dims(&got).iter().all(|d| *d == unit_dim("m")));
}

// ---- csort, rsort ---------------------------------------------------------------------------

/// Help: csort(M, 3) with ORIGIN 0 (rows reordered until column 3 ascends).
const CSORT_M3: [f64; 36] = [
    91.0, 42.0, 223.0, 3.0, 45.0, 88.0, //
    0.0, 92.0, 13.0, 15.0, 14.0, 55.0, //
    144.0, 23.0, 177.0, 77.0, 66.0, 5.0, //
    15.0, 12.0, 66.0, 94.0, 4.0, 99.0, //
    131.0, 232.0, 2.0, 134.0, 13.0, 70.0, //
    221.0, 1.0, 133.0, 244.0, 25.0, 150.0,
];

/// Help: rsort(M, 4) with ORIGIN 0 (columns reordered until row 4 ascends).
const RSORT_M4: [f64; 36] = [
    14.0, 92.0, 0.0, 13.0, 15.0, 55.0, //
    25.0, 1.0, 221.0, 133.0, 244.0, 150.0, //
    13.0, 232.0, 131.0, 2.0, 134.0, 70.0, //
    45.0, 42.0, 91.0, 223.0, 3.0, 88.0, //
    4.0, 12.0, 15.0, 66.0, 94.0, 99.0, //
    66.0, 23.0, 144.0, 177.0, 77.0, 5.0,
];

#[test]
fn csort_help_example_with_origin_0() {
    let got = value_of(&[evaluate_stmt(call("csort", vec![help_m(), n(3.0)]))]);
    close_all(&reals(&got), &CSORT_M3, 0.0);
}

#[test]
fn csort_help_example_with_origin_1_uses_one_based_columns() {
    // With ORIGIN 1 the same column is index 4.
    let stmts = then_eval(vec![def_origin(1.0)], call("csort", vec![help_m(), n(4.0)]));
    close_all(&reals(&value_of(&stmts)), &CSORT_M3, 0.0);
}

#[test]
fn rsort_help_example_with_origin_0() {
    let got = value_of(&[evaluate_stmt(call("rsort", vec![help_m(), n(4.0)]))]);
    close_all(&reals(&got), &RSORT_M4, 0.0);
}

#[test]
fn rsort_help_example_with_origin_1_uses_one_based_rows() {
    let stmts = then_eval(vec![def_origin(1.0)], call("rsort", vec![help_m(), n(5.0)]));
    close_all(&reals(&value_of(&stmts)), &RSORT_M4, 0.0);
}

#[test]
fn a_failed_origin_definition_counts_from_zero_but_a_bare_read_is_undefined() {
    // ORIGIN := 1/0 fails, so the name is undefined. Builtins and indexing both count from 0, while
    // reading ORIGIN itself is Undefined. This test pins that the two agree.
    let fail = || def("ORIGIN", div(n(1.0), n(0.0)));
    let column = mat(3, 1, vec![n(10.0), n(20.0), n(30.0)]);
    let stmts = [
        def("M", column.clone()),
        fail(),
        evaluate_stmt(call("last", vec![v("M")])),
    ];
    assert_eq!(scalar_real(&stmts), 2.0);
    let stmts = [
        def("M", column),
        fail(),
        evaluate_stmt(index(v("M"), vec![n(0.0)])),
    ];
    assert_eq!(scalar_real(&stmts), 10.0);
    let e = error_of(&[fail(), evaluate_stmt(v("ORIGIN"))]);
    assert_eq!(e, EvalError::Undefined("ORIGIN".to_string()));
}

#[test]
fn csort_keeps_units_and_refuses_mixed_key_dimensions() {
    let good = mat(
        2,
        2,
        vec![
            scale(2.0, u("m")),
            scale(5.0, u("s")),
            scale(1.0, u("m")),
            scale(6.0, u("s")),
        ],
    );
    let got = value_of(&[evaluate_stmt(call("csort", vec![good, n(0.0)]))]);
    assert_eq!(reals(&got), vec![1.0, 6.0, 2.0, 5.0]);
    assert_eq!(
        dims(&got),
        vec![unit_dim("m"), unit_dim("s"), unit_dim("m"), unit_dim("s")]
    );
    // Column 0 mixes metres and seconds: the key cannot be ordered.
    let mixed = mat(2, 1, vec![scale(1.0, u("m")), scale(2.0, u("s"))]);
    let e = error_of(&[evaluate_stmt(call("csort", vec![mixed, n(0.0)]))]);
    assert!(matches!(e, EvalError::DimensionMismatch { .. }), "{e:?}");
}

#[test]
fn csort_key_out_of_range_is_an_index_error() {
    let e = error_of(&[evaluate_stmt(call("csort", vec![help_m(), n(6.0)]))]);
    assert_eq!(e, EvalError::IndexOutOfRange { index: 6, bound: 6 });
}

// ---- ORIGIN and TOL: range variables, captured values --------------------------------------

#[test]
fn a_range_origin_is_refused_by_last_and_submatrix() {
    let v = mat(3, 1, vec![n(1.0), n(2.0), n(3.0)]);
    let e = error_of(&[range_def("ORIGIN"), evaluate_stmt(call("last", vec![v]))]);
    assert_eq!(e, EvalError::Unsupported(REFUSED.to_string()));
    let e = error_of(&[
        range_def("ORIGIN"),
        evaluate_stmt(call(
            "submatrix",
            vec![help_m(), n(0.0), n(0.0), n(0.0), n(0.0)],
        )),
    ]);
    assert_eq!(e, EvalError::Unsupported(REFUSED.to_string()));
}

#[test]
fn a_range_origin_does_not_stop_rank_which_does_not_index() {
    let stmts = then_eval(
        vec![range_def("ORIGIN")],
        call("rank", vec![call("identity", vec![n(2.0)])]),
    );
    assert_eq!(scalar_real(&stmts), 2.0);
}

#[test]
fn origin_is_captured_at_definition_inside_a_function() {
    // g is defined with ORIGIN 0; after ORIGIN := 1 its `last` still counts from 0: rows - 1 = 1.
    let stmts = [
        def("M", mat(2, 1, vec![n(10.0), n(20.0)])),
        def_fn("g", &["x"], call("last", vec![v("M")])),
        def_origin(1.0),
        evaluate_stmt(call("g", vec![n(0.0)])),
    ];
    assert_eq!(scalar_real(&stmts), 1.0);
}

#[test]
fn tol_is_captured_at_definition_inside_a_function() {
    // D = diag(0.3, 1). r is defined with TOL = 0.5 (pivot 0.3 is zero, rank 1). After TOL := 0.001
    // the captured 0.5 still applies: rank 1, not 2.
    let stmts = [
        def("D", grid(2, 2, &[0.3, 0.0, 0.0, 1.0])),
        def("TOL", n(0.5)),
        def_fn("r", &["x"], call("rank", vec![v("D")])),
        def("TOL", n(0.001)),
        evaluate_stmt(call("r", vec![n(0.0)])),
    ];
    assert_eq!(scalar_real(&stmts), 1.0);
}

#[test]
fn rank_follows_the_worksheet_tol_and_its_redefinition() {
    let d = grid(2, 2, &[0.3, 0.0, 0.0, 1.0]);
    let stmts = [
        def("TOL", n(0.5)),
        evaluate_stmt(call("rank", vec![d.clone()])),
        def("TOL", n(0.001)),
        evaluate_stmt(call("rank", vec![d])),
    ];
    let outs = run_all(&stmts);
    assert_eq!(real_in(&expect_value(outs[1].clone()), dimless()), 1.0);
    assert_eq!(real_in(&expect_value(outs[3].clone()), dimless()), 2.0);
}

#[test]
fn rank_of_diag_1e4_and_1_follows_tol() {
    // Pivot 1e-4 is at most TOL = 1e-3, so it is zero (rank 1). At TOL = 1e-6 it counts (rank 2).
    // At TOL = 1 both pivots are at most TOL (rank 0).
    let d = grid(2, 2, &[1e-4, 0.0, 0.0, 1.0]);
    let at = |tol: f64| {
        let stmts = then_eval(vec![def("TOL", n(tol))], call("rank", vec![d.clone()]));
        scalar_real(&stmts)
    };
    assert_eq!(at(1e-3), 1.0);
    assert_eq!(at(1e-6), 2.0);
    assert_eq!(at(1.0), 0.0);
    // TOL = 0: only exact zero pivots are zero.
    assert_eq!(at(0.0), 2.0);
}

#[test]
fn rref_follows_tol_too() {
    // With TOL = 1e-3 the 1e-4 pivot is zero: column 0 is skipped and the pivot moves to column 1,
    // giving [0 1; 0 0]. With TOL = 1e-6 it is a pivot and the result is the identity.
    let d = grid(2, 2, &[1e-4, 0.0, 0.0, 1.0]);
    let got = value_of(&[evaluate_stmt(call("rref", vec![d.clone()]))]);
    assert_eq!(reals(&got), vec![0.0, 1.0, 0.0, 0.0]);
    let fine = then_eval(vec![def("TOL", n(1e-6))], call("rref", vec![d]));
    assert_eq!(reals(&value_of(&fine)), vec![1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn a_negative_or_non_finite_tol_is_refused() {
    let d = grid(2, 2, &[1e-4, 0.0, 0.0, 1.0]);
    let e = error_of(&then_eval(
        vec![def("TOL", neg(n(0.5)))],
        call("rank", vec![d.clone()]),
    ));
    assert!(matches!(e, EvalError::Unsupported(_)), "{e:?}");
    let e = error_of(&then_eval(
        vec![def("TOL", neg(n(0.5)))],
        call("rref", vec![d]),
    ));
    assert!(matches!(e, EvalError::Unsupported(_)), "{e:?}");
}

#[test]
fn a_failed_tol_definition_leaves_rank_undefined() {
    let d = grid(2, 2, &[1.0, 0.0, 0.0, 1.0]);
    let e = error_of(&then_eval(
        vec![def("TOL", div(n(1.0), n(0.0)))],
        call("rank", vec![d]),
    ));
    assert_eq!(e, EvalError::Undefined("TOL".to_string()));
}

#[test]
fn a_range_tol_is_refused_by_rank() {
    let d = grid(2, 2, &[1.0, 0.0, 0.0, 1.0]);
    let e = error_of(&then_eval(vec![range_def("TOL")], call("rank", vec![d])));
    assert_eq!(e, EvalError::Unsupported(REFUSED.to_string()));
}

// ---- cost classes for the new functions -----------------------------------------------------

#[test]
fn new_functions_are_charged_per_element_and_sorts_per_pass() {
    let square =
        Value::Matrix(Matrix::new(4, 4, vec![Scalar::real(1.0, dimless()); 16]).expect("matrix"));
    // last and submatrix: 3 units per input element (the class of a copy).
    assert_eq!(builtin_units("last", std::slice::from_ref(&square)), 3 * 16);
    assert_eq!(
        builtin_units("submatrix", std::slice::from_ref(&square)),
        3 * 16
    );
    // csort and rsort: 3 units per element per pass, ceil(log2 4) = 2 passes.
    assert_eq!(
        builtin_units("csort", std::slice::from_ref(&square)),
        3 * 16 * 2
    );
    assert_eq!(
        builtin_units("rsort", std::slice::from_ref(&square)),
        3 * 16 * 2
    );
    // A column of 5 lines needs ceil(log2 5) = 3 passes.
    let column =
        Value::Matrix(Matrix::new(5, 1, vec![Scalar::real(1.0, dimless()); 5]).expect("matrix"));
    assert_eq!(
        builtin_units("csort", std::slice::from_ref(&column)),
        3 * 5 * 3
    );
}

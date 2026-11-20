//! Range iteration, from the help: a statement with range variables is evaluated once per value
//! (`sin(i)` is a column), two ranges give a matrix, indexed definitions build arrays, and the
//! plotting entry points return the same values.

use crate::eval::range_iter::{evaluate_series, evaluate_surface};
use crate::eval::work::{Budget, MAX_WORK};
use crate::eval::{EvalError, Matrix, Outcome, Session, Value};
use crate::expr::Expr;
use crate::parser::{parse, DefaultIdContext};

/// Parses one statement as typed.
fn stmt(src: &str) -> Expr {
    parse(src, &DefaultIdContext).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

/// Runs statements in order on a fresh session and returns the outcome of the last one.
fn run_text(lines: &[&str]) -> Outcome {
    let mut s = Session::new();
    let mut last = None;
    for line in lines {
        last = Some(s.run(&stmt(line)));
    }
    last.expect("at least one statement")
}

fn values_of(out: Outcome) -> Vec<f64> {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.data().iter().map(|s| s.value.re).collect(),
        other => panic!("expected a column, got {other:?}"),
    }
}

fn shape_of(out: Outcome) -> (usize, usize) {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.shape(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn assert_close(got: &[f64], want: &[f64], tol: f64) {
    assert_eq!(got.len(), want.len(), "length: {got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < tol, "{got:?} != {want:?}");
    }
}

#[test]
fn sine_of_a_range_is_the_column_of_sines() {
    // Help: range variables are used in expressions; a plot of sin(i) takes the column.
    let got = values_of(run_text(&["i := 0 .. 3", "sin(i)="]));
    let want: Vec<f64> = (0..4).map(|k| (k as f64).sin()).collect();
    assert_close(&got, &want, 1e-12);
}

#[test]
fn parametric_plot_points_of_the_help_example() {
    // Help: i := 0..π-style plot; cos(i) = [1, 0.54, -0.416, -0.99] for i = 0..3.
    let got = values_of(run_text(&["i := 0 .. 3", "cos(i)="]));
    assert_close(&got, &[1.0, 0.54, -0.416, -0.99], 1e-3);
}

#[test]
fn step_range_gives_every_point_up_to_the_end() {
    // Help: j := 0, 0.1 .. π has 32 values (the last is 3.1, not π).
    let got = values_of(run_text(&["j := 0, 0.1 .. π", "sin(j)="]));
    assert_eq!(got.len(), 32);
    assert!((got[10] - 1.0_f64.sin()).abs() < 1e-9);
}

#[test]
fn indexed_definition_builds_the_matrix_of_the_help_example() {
    // Help (methods for defining arrays): r:0..3, c:0..3, X[r,c := r^2 + c/2, then X= is 4 x 4 with
    // rows 0, 0.5, 1, 1.5 / 1, 1.5, 2, 2.5 / 4, 4.5, 5, 5.5 / 9, 9.5, 10, 10.5.
    let mut s = Session::new();
    for line in ["r := 0 .. 3", "c := 0 .. 3", "X[r,c] := r^2 + c/2"] {
        s.run(&stmt(line));
    }
    let out = s.run(&stmt("X="));
    assert_eq!(shape_of(out.clone()), (4, 4));
    let got = values_of(out);
    let want = [
        0.0, 0.5, 1.0, 1.5, 1.0, 1.5, 2.0, 2.5, 4.0, 4.5, 5.0, 5.5, 9.0, 9.5, 10.0, 10.5,
    ];
    assert_close(&got, &want, 1e-12);
}

#[test]
fn two_ranges_give_a_matrix_with_rows_from_the_first_range_seen() {
    // r is seen first, so the rows follow r: 2 x 3.
    let got = values_of(run_text(&["r := 0 .. 1", "c := 0 .. 2", "r*10 + c="]));
    assert_close(&got, &[0.0, 1.0, 2.0, 10.0, 11.0, 12.0], 1e-12);
    assert_eq!(
        shape_of(run_text(&["r := 0 .. 1", "c := 0 .. 2", "r*10 + c="])),
        (2, 3)
    );
}

#[test]
fn order_of_appearance_decides_the_rows() {
    // c is seen first, so the rows follow c: 3 x 2.
    assert_eq!(
        shape_of(run_text(&["r := 0 .. 1", "c := 0 .. 2", "c*10 + r="])),
        (3, 2)
    );
}

#[test]
fn plain_definition_from_a_range_variable_is_refused() {
    // Help: you cannot define a variable in terms of a range variable (t := 1 .. 2, j := t + 1).
    let out = run_text(&["t := 1 .. 2", "j := t + 1"]);
    assert_eq!(
        out,
        Outcome::Error(EvalError::Unsupported(
            "range variable used in an expression".to_string()
        ))
    );
}

#[test]
fn reading_an_element_with_a_range_index_gives_the_column() {
    let got = values_of(run_text(&["x := [10; 20; 30]", "i := 0 .. 2", "x[i]="]));
    assert_close(&got, &[10.0, 20.0, 30.0], 1e-12);
}

#[test]
fn indexed_column_definition_and_reading_back() {
    // y_i := i^2 + 1 for i = 0..3 builds the column [1; 2; 5; 10].
    let mut s = Session::new();
    s.run(&stmt("i := 0 .. 3"));
    assert!(matches!(
        s.run(&stmt("y_i := i^2 + 1")),
        Outcome::Defined { .. }
    ));
    assert_close(
        &values_of(s.run(&stmt("y="))),
        &[1.0, 2.0, 5.0, 10.0],
        1e-12,
    );
    // y_i= reads the array element by element.
    assert_close(
        &values_of(s.run(&stmt("y_i="))),
        &[1.0, 2.0, 5.0, 10.0],
        1e-12,
    );
}

#[test]
fn indexed_definition_counts_from_origin() {
    // With ORIGIN = 1 the range 1..3 fills elements 1..3 of the array.
    let got = values_of(run_text(&[
        "ORIGIN := 1",
        "i := 1 .. 3",
        "y_i := 2*i",
        "y=",
    ]));
    assert_close(&got, &[2.0, 4.0, 6.0], 1e-12);
}

#[test]
fn indexed_definition_must_start_at_origin() {
    // Range 1..3 with ORIGIN 0 would leave element 0 unset: refused, and y stays undefined.
    let mut s = Session::new();
    s.run(&stmt("i := 1 .. 3"));
    assert!(matches!(
        s.run(&stmt("y_i := i")),
        Outcome::Error(EvalError::Unsupported(_))
    ));
    assert!(matches!(s.run(&stmt("y=")), Outcome::Error(_)));
}

#[test]
fn indexed_definition_may_use_only_its_own_ranges() {
    let out = run_text(&["i := 0 .. 1", "j := 0 .. 1", "y_i := i + j"]);
    assert!(matches!(out, Outcome::Error(EvalError::Unsupported(_))));
}

#[test]
fn descending_range_iterates_in_its_own_order() {
    let got = values_of(run_text(&["i := 3, 2 .. 0", "i^2="]));
    assert_close(&got, &[9.0, 4.0, 1.0, 0.0], 1e-12);
}

#[test]
fn empty_and_zero_step_ranges_fail_without_panic() {
    // The range is refused at its definition; a later use is an error, not a value.
    assert!(matches!(
        run_text(&["i := 3, 4 .. 0", "i^2="]),
        Outcome::Error(_)
    ));
    assert!(matches!(
        run_text(&["t := 0, 0 .. 1", "t="]),
        Outcome::Error(_)
    ));
}

#[test]
fn huge_ranges_are_refused_without_running() {
    let mut s = Session::new();
    assert!(matches!(
        s.run(&stmt("t := 0 .. 1000000000")),
        Outcome::Error(EvalError::TooLarge(_))
    ));
    // 1000 x 1001 pairs: the product is over the element cap, refused before any evaluation.
    assert!(matches!(
        run_text(&["t := 0 .. 999", "u := 0 .. 1000", "t + u="]),
        Outcome::Error(EvalError::TooLarge(_))
    ));
}

#[test]
fn series_pairs_match_the_iteration() {
    let mut s = Session::new();
    s.run(&stmt("x := 0, 0.5 .. 1"));
    let xs: Matrix = s.env().range_variable("x").unwrap().clone();
    let pairs = evaluate_series(s.env(), &stmt("sin(x)"), "x", &xs, &Budget::new()).unwrap();
    let want = [(0.0, 0.0), (0.5, 0.5_f64.sin()), (1.0, 1.0_f64.sin())];
    assert_eq!(pairs.len(), 3);
    for ((gx, gy), (wx, wy)) in pairs.iter().zip(want) {
        assert!((gx - wx).abs() < 1e-12 && (gy - wy).abs() < 1e-12);
    }
}

#[test]
fn series_binds_its_own_range_even_when_the_name_is_not_defined() {
    let env = Session::new();
    let u = {
        let mut s = Session::new();
        s.run(&stmt("u := 1 .. 3"));
        s.env().range_variable("u").unwrap().clone()
    };
    let pairs = evaluate_series(env.env(), &stmt("u^2"), "u", &u, &Budget::new()).unwrap();
    assert_eq!(pairs, vec![(1.0, 1.0), (2.0, 4.0), (3.0, 9.0)]);
}

#[test]
fn surface_rows_follow_the_first_range() {
    let mut s = Session::new();
    s.run(&stmt("r := 0 .. 1"));
    s.run(&stmt("c := 0 .. 2"));
    let r = s.env().range_variable("r").unwrap().clone();
    let c = s.env().range_variable("c").unwrap().clone();
    let grid = evaluate_surface(
        s.env(),
        &stmt("r*10 + c"),
        ("r", &r),
        ("c", &c),
        &Budget::new(),
    )
    .unwrap();
    assert_eq!(grid, vec![vec![0.0, 1.0, 2.0], vec![10.0, 11.0, 12.0]]);
}

#[test]
fn series_refuses_a_complex_value_instead_of_plotting_it() {
    let env = Session::new();
    let xs = {
        let mut s = Session::new();
        s.run(&stmt("x := 0 .. 1"));
        s.env().range_variable("x").unwrap().clone()
    };
    // sqrt of a negative is complex.
    let out = evaluate_series(env.env(), &stmt("sqrt(x - 2)"), "x", &xs, &Budget::new());
    assert!(matches!(out, Err(EvalError::Unsupported(_))));
}

#[test]
fn series_charges_the_callers_budget() {
    // A drained budget refuses the series before any value is evaluated.
    let xs = {
        let mut s = Session::new();
        s.run(&stmt("x := 0 .. 2"));
        s.env().range_variable("x").unwrap().clone()
    };
    let budget = Budget::new();
    budget.charge(MAX_WORK).unwrap();
    let out = evaluate_series(Session::new().env(), &stmt("sin(x)"), "x", &xs, &budget);
    assert_eq!(out, Err(EvalError::WorkLimit(MAX_WORK)));
}

#[test]
fn indexed_definition_may_not_read_its_own_array() {
    // Help documents no element-wise self reference (methods for defining arrays), so `y_i := y_i + 1`
    // is refused, and as for any failed definition y is then undefined (not [1; 2; 3], the wrong value
    // the review found).
    let mut s = Session::new();
    for line in ["i := 0 .. 2", "y_i := i", "i := 0 .. 2"] {
        s.run(&stmt(line));
    }
    assert!(matches!(
        s.run(&stmt("y_i := y_i + 1")),
        Outcome::Error(EvalError::Unsupported(_))
    ));
    assert!(matches!(s.run(&stmt("y=")), Outcome::Error(_)));
    // The whole array and a literal-subscript name are refused the same way.
    assert!(matches!(
        run_text(&["i := 0 .. 2", "y_i := y + i"]),
        Outcome::Error(EvalError::Unsupported(_))
    ));
}

#[test]
fn literal_subscript_is_a_name_not_an_element() {
    // Help (using literal subscripts for name definitions): y_2 is part of the name, while the index
    // operator y[2 reads an element. So y_2 after `y_i := ...` is undefined, not element 2.
    let mut s = Session::new();
    s.run(&stmt("i := 0 .. 3"));
    s.run(&stmt("y_i := i^2"));
    assert!(matches!(
        s.run(&stmt("y_2=")),
        Outcome::Error(EvalError::Undefined(ref name)) if name == "y_2"
    ));
}

fn scalar_of(out: Outcome) -> f64 {
    match out {
        Outcome::Value(Value::Scalar(s)) => s.value.re,
        other => panic!("expected a scalar, got {other:?}"),
    }
}

#[test]
fn range_passed_whole_to_a_vector_builtin_is_refused() {
    // rows(t) would otherwise see one value of t per row of the result (0,0,0,0).
    for line in ["rows(t)=", "max(t)=", "mean(t)="] {
        let out = run_text(&["t := 0 .. 3", line]);
        assert!(
            matches!(out, Outcome::Error(EvalError::Unsupported(_))),
            "{line}: {out:?}"
        );
    }
}

#[test]
fn vec_of_a_range_is_the_whole_vector() {
    // Help (functions for creating vectors): vec(x) converts a range variable into a vector.
    assert_eq!(scalar_of(run_text(&["t := 0 .. 3", "rows(vec(t))="])), 4.0);
    assert_eq!(scalar_of(run_text(&["t := 0 .. 3", "max(vec(t))="])), 3.0);
    assert_close(
        &values_of(run_text(&["t := 0 .. 3", "vec(t)="])),
        &[0.0, 1.0, 2.0, 3.0],
        1e-12,
    );
    // Elementwise functions still iterate the range: sin(t) is the column of sines.
    assert_close(
        &values_of(run_text(&["t := 0 .. 2", "sin(t)="])),
        &[0.0, 1.0_f64.sin(), 2.0_f64.sin()],
        1e-12,
    );
}

#[test]
fn range_variables_in_a_function_body_are_still_refused() {
    // A body captures the range variable and refuses it when it runs.
    let out = run_text(&["t := 0 .. 2", "f(x) := x + t", "f(1)="]);
    assert!(matches!(out, Outcome::Error(EvalError::Unsupported(_))));
}

#[test]
fn a_range_index_into_a_nested_array_takes_one_index() {
    // F = LU(M) for the 3 x 2 M = [1 2; 3 4; 5 6]: P is 3 x 3, L is 3 x 2 and U is 2 x 2 with
    // U_00 = 5 (the pivot) and L_00 = 1. A range element of F is a matrix, so the scalar is read
    // from it with an index. With ORIGIN 0 the element at 2 is U; with ORIGIN 1 it is L.
    // A one-value range shows a 1 x 1 column.
    let got = run_text(&["F := LU([1, 2; 3, 4; 5, 6])", "i := 2 .. 2", "F_i[0, 0]="]);
    assert_close(&values_of(got), &[5.0], 1e-12);
    let got = run_text(&[
        "ORIGIN := 1",
        "F := LU([1, 2; 3, 4; 5, 6])",
        "i := 2 .. 2",
        // Element indices count from ORIGIN too: [1, 1] is the first entry under ORIGIN 1.
        "F_i[1, 1]=",
    ]);
    assert_close(&values_of(got), &[1.0], 1e-12);
}

#[test]
fn a_range_index_past_a_nested_array_is_an_error() {
    // Index 3 is past the end of the three-element list (ORIGIN 0).
    let out = run_text(&["F := LU([1, 2; 3, 4])", "i := 3 .. 3", "F_i="]);
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
}

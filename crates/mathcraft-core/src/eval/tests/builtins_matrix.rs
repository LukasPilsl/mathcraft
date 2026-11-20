//! Vector and matrix builtins: shape queries, creation, joining, reversing and sorting, trace,
//! determinant, cross product and the functions still stubbed. Expected values come from the
//! Mathcad Prime help images (example_sorting_functions, example_inserting_and_extracting_
//! subarrays, example_matrix_norm_and_det_functions) and from hand computation. Norms, condition
//! numbers, rref, rank and solves are in `builtins_matrix_linalg`; singular values in
//! `builtins_matrix_svd`.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::expr::Expr;
use crate::units::Dimension;
use num_complex::Complex64;

fn run(e: &Expr) -> Result<Value, EvalError> {
    evaluate(e, &Env::new())
}

fn ok(e: &Expr) -> Value {
    run(e).unwrap_or_else(|err| panic!("failed: {err}"))
}

fn err(e: &Expr) -> EvalError {
    run(e).expect_err("expected an error")
}

/// A matrix literal of plain numbers, row-major.
fn grid(rows: usize, cols: usize, vals: &[f64]) -> Expr {
    mat(rows, cols, vals.iter().map(|x| n(*x)).collect())
}

/// The real parts of a matrix value, asserting every element is real.
fn reals(v: &Value) -> Vec<f64> {
    matrix(v)
        .data()
        .iter()
        .map(|s| {
            assert!(s.value.im.abs() < 1e-12, "expected real, got {}", s.value);
            s.value.re
        })
        .collect()
}

fn dims(v: &Value) -> Vec<Dimension> {
    matrix(v).data().iter().map(|s| s.dim).collect()
}

fn close_all(actual: &[f64], expected: &[f64], tol: f64) {
    assert_eq!(actual.len(), expected.len(), "length of {actual:?}");
    for (a, e) in actual.iter().zip(expected) {
        close(*a, *e, tol);
    }
}

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

fn m_dim(name: &str) -> Dimension {
    unit_dim(name)
}

/// The example matrix of the norm and determinant help page, as its image shows it:
/// [1 0 7; 9 5 8; 3 4 2]. (The page text lists the transpose; see the report.)
fn help_m() -> Expr {
    grid(3, 3, &[1.0, 0.0, 7.0, 9.0, 5.0, 8.0, 3.0, 4.0, 2.0])
}

// ---- rows, cols, length ---------------------------------------------------------------------

#[test]
fn rows_and_cols_count_a_matrix_and_zero_for_a_scalar() {
    assert_eq!(
        real_in(&ok(&call("rows", vec![grid(2, 4, &[0.0; 8])])), dimless()),
        2.0
    );
    assert_eq!(
        real_in(&ok(&call("cols", vec![grid(2, 4, &[0.0; 8])])), dimless()),
        4.0
    );
    assert_eq!(real_in(&ok(&call("rows", vec![n(5.0)])), dimless()), 0.0);
    assert_eq!(real_in(&ok(&call("cols", vec![n(5.0)])), dimless()), 0.0);
}

#[test]
fn length_counts_elements_of_a_column_vector() {
    // Help: length(v) returns the number of elements of a column vector.
    let v = grid(4, 1, &[1.0, 2.0, 3.0, 4.0]);
    assert_eq!(real_in(&ok(&call("length", vec![v])), dimless()), 4.0);
}

#[test]
fn length_of_a_matrix_is_an_error() {
    let e = call("length", vec![grid(1, 3, &[1.0, 2.0, 3.0])]);
    assert!(matches!(err(&e), EvalError::Unsupported(_)));
}

// ---- identity, diag -------------------------------------------------------------------------

#[test]
fn identity_three_is_the_identity_matrix() {
    let got = ok(&call("identity", vec![n(3.0)]));
    assert_eq!(matrix(&got).shape(), (3, 3));
    close_all(
        &reals(&got),
        &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        0.0,
    );
}

#[test]
fn identity_rejects_zero_fractions_and_dimensions() {
    assert!(matches!(
        err(&call("identity", vec![n(0.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("identity", vec![n(2.5)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("identity", vec![scale(2.0, u("m"))])),
        EvalError::DimensionNotDimensionless { .. }
    ));
}

#[test]
fn identity_with_too_many_arguments_is_an_arg_count_error() {
    assert!(matches!(
        err(&call("identity", vec![n(1.0), n(2.0)])),
        EvalError::ArgCount { .. }
    ));
}

#[test]
fn diag_of_a_vector_keeps_its_dimension_on_every_element() {
    let v = mat(2, 1, vec![scale(1.0, u("m")), scale(2.0, u("m"))]);
    let got = ok(&call("diag", vec![v]));
    assert_eq!(matrix(&got).shape(), (2, 2));
    close_all(&reals(&got), &[1.0, 0.0, 0.0, 2.0], 0.0);
    let m = m_dim("m");
    assert!(dims(&got).iter().all(|d| *d == m));
}

#[test]
fn diag_of_the_help_matrix_is_its_diagonal() {
    // Diagonal of [1 0 7; 9 5 8; 3 4 2] is (1, 5, 2).
    let got = ok(&call("diag", vec![help_m()]));
    close_all(&reals(&got), &[1.0, 5.0, 2.0], 0.0);
    assert_eq!(matrix(&got).shape(), (3, 1));
}

#[test]
fn diag_of_a_non_square_matrix_is_not_square() {
    let e = call("diag", vec![grid(2, 3, &[0.0; 6])]);
    assert!(matches!(err(&e), EvalError::NotSquare { rows: 2, cols: 3 }));
}

// ---- augment, stack -------------------------------------------------------------------------

#[test]
fn augment_joins_columns_as_in_the_help_example() {
    // Help: R = augment(A, M) with the first rows (1 1700 5), (0 1701 11), (2 1702 16).
    let a = grid(3, 1, &[1.0, 0.0, 2.0]);
    let m = grid(3, 2, &[1700.0, 5.0, 1701.0, 11.0, 1702.0, 16.0]);
    let got = ok(&call("augment", vec![a, m]));
    assert_eq!(matrix(&got).shape(), (3, 3));
    close_all(
        &reals(&got),
        &[1.0, 1700.0, 5.0, 0.0, 1701.0, 11.0, 2.0, 1702.0, 16.0],
        0.0,
    );
}

#[test]
fn augment_keeps_each_column_dimension() {
    let a = mat(2, 1, vec![scale(1.0, u("m")), scale(2.0, u("m"))]);
    let b = mat(2, 1, vec![scale(3.0, u("s")), scale(4.0, u("s"))]);
    let got = ok(&call("augment", vec![a, b]));
    let d = dims(&got);
    assert_eq!(d[0], m_dim("m"));
    assert_eq!(d[1], m_dim("s"));
    assert_eq!(d[2], m_dim("m"));
    assert_eq!(d[3], m_dim("s"));
}

#[test]
fn augment_with_different_row_counts_is_a_shape_error() {
    let e = call(
        "augment",
        vec![grid(2, 1, &[1.0, 2.0]), grid(3, 1, &[1.0, 2.0, 3.0])],
    );
    assert_eq!(
        err(&e),
        EvalError::ShapeMismatch {
            left: (2, 1),
            right: (3, 1)
        }
    );
}

#[test]
fn stack_joins_rows_top_to_bottom() {
    let got = ok(&call(
        "stack",
        vec![grid(2, 1, &[1.0, 2.0]), grid(2, 1, &[-3.0, 4.0])],
    ));
    assert_eq!(matrix(&got).shape(), (4, 1));
    close_all(&reals(&got), &[1.0, 2.0, -3.0, 4.0], 0.0);
}

#[test]
fn stack_with_different_column_counts_is_a_shape_error() {
    let e = call("stack", vec![grid(2, 2, &[0.0; 4]), grid(2, 3, &[0.0; 6])]);
    assert_eq!(
        err(&e),
        EvalError::ShapeMismatch {
            left: (2, 2),
            right: (2, 3)
        }
    );
}

// ---- reverse, sort --------------------------------------------------------------------------

/// Help vector: x = [0, 0.841, 0.909, 0.141] (sin of 0..3, rounded as in the image).
fn help_x() -> Expr {
    grid(4, 1, &[0.0, 0.841, 0.909, 0.141])
}

#[test]
fn sort_orders_the_help_vector_ascending() {
    // Help: sort(x) = [0, 0.141, 0.841, 0.909]^T.
    let got = ok(&call("sort", vec![help_x()]));
    close_all(&reals(&got), &[0.0, 0.141, 0.841, 0.909], 1e-12);
}

#[test]
fn reverse_of_sort_gives_the_descending_order() {
    // Help: reverse(sort(x)) = [0.909, 0.841, 0.141, 0]^T.
    let got = ok(&call("reverse", vec![call("sort", vec![help_x()])]));
    close_all(&reals(&got), &[0.909, 0.841, 0.141, 0.0], 1e-12);
}

#[test]
fn sort_orders_by_real_part_and_keeps_the_values() {
    // Real parts -1, 0.5, 3 decide the order; imaginary parts travel with their element.
    let z = mat(
        3,
        1,
        vec![
            add(n(3.0), mul(n(2.0), v("i"))),
            n(-1.0),
            add(n(0.5), mul(n(-4.0), v("i"))),
        ],
    );
    let got = ok(&call("sort", vec![z]));
    let data = matrix(&got).data();
    assert_eq!(data[0].value.re, -1.0);
    assert_eq!(data[1].value.re, 0.5);
    assert_eq!(data[1].value.im, -4.0);
    assert_eq!(data[2].value.im, 2.0);
}

#[test]
fn sort_of_mixed_dimensions_is_a_dimension_mismatch() {
    let v = mat(2, 1, vec![scale(1.0, u("m")), scale(2.0, u("s"))]);
    assert!(matches!(
        err(&call("sort", vec![v])),
        EvalError::DimensionMismatch { .. }
    ));
}

#[test]
fn sort_of_a_matrix_with_several_columns_is_unsupported() {
    assert!(matches!(
        err(&call("sort", vec![grid(2, 2, &[1.0, 2.0, 3.0, 4.0])])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn reverse_of_a_matrix_reverses_its_rows() {
    let got = ok(&call(
        "reverse",
        vec![grid(3, 2, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])],
    ));
    close_all(&reals(&got), &[5.0, 6.0, 3.0, 4.0, 1.0, 2.0], 0.0);
}

#[test]
fn reverse_of_a_scalar_is_unsupported() {
    assert!(matches!(
        err(&call("reverse", vec![n(1.0)])),
        EvalError::Unsupported(_)
    ));
}

// ---- tr, det, cross -------------------------------------------------------------------------

#[test]
fn trace_of_the_help_matrix_is_eight() {
    // Diagonal 1 + 5 + 2 = 8; equals the sum of the eigenvalues.
    let got = ok(&call("tr", vec![help_m()]));
    close(real_in(&got, dimless()), 8.0, 0.0);
}

#[test]
fn trace_keeps_the_dimension_of_the_diagonal() {
    let m = mat(
        2,
        2,
        vec![scale(1.0, u("m")), n(0.0), n(0.0), scale(2.0, u("m"))],
    );
    let got = ok(&call("tr", vec![m]));
    close(real_in(&got, m_dim("m")), 3.0, 0.0);
}

#[test]
fn trace_with_mixed_diagonal_dimensions_is_an_error() {
    let m = mat(
        2,
        2,
        vec![scale(1.0, u("m")), n(0.0), n(0.0), scale(2.0, u("s"))],
    );
    assert!(matches!(
        err(&call("tr", vec![m])),
        EvalError::DimensionMismatch { .. }
    ));
}

#[test]
fn trace_of_a_non_square_matrix_is_not_square() {
    assert!(matches!(
        err(&call("tr", vec![grid(1, 2, &[1.0, 2.0])])),
        EvalError::NotSquare { .. }
    ));
}

#[test]
fn determinant_of_the_help_matrix_is_125() {
    // Help image: det(M) = 125.
    close(
        real_in(&ok(&call("det", vec![help_m()])), dimless()),
        125.0,
        1e-12,
    );
}

#[test]
fn determinant_of_a_permutation_is_negative() {
    // [0 1; 1 0] swaps two rows: det = -1.
    let got = ok(&call("det", vec![grid(2, 2, &[0.0, 1.0, 1.0, 0.0])]));
    close(real_in(&got, dimless()), -1.0, 1e-15);
}

#[test]
fn determinant_of_complex_entries_is_complex() {
    // [i 0; 0 i]: det = i * i = -1.
    let z = mat(2, 2, vec![v("i"), n(0.0), n(0.0), v("i")]);
    let got = scalar(&ok(&call("det", vec![z]))).value;
    complex_close(got, Complex64::new(-1.0, 0.0), 1e-12);
}

#[test]
fn determinant_of_dimensioned_matrix_has_power_dimension() {
    // det of [1 2; 3 4] m gives -2 m^2.
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
    let got = scalar(&ok(&call("det", vec![m])));
    close(got.value.re, -2.0, 1e-12);
    let m2 = m_dim("m").checked_mul(m_dim("m")).unwrap();
    assert_eq!(got.dim, m2);
}

#[test]
fn determinant_of_non_square_is_not_square() {
    assert!(matches!(
        err(&call("det", vec![grid(2, 3, &[0.0; 6])])),
        EvalError::NotSquare { .. }
    ));
}

#[test]
fn cross_product_of_123_and_456() {
    // (2*6 - 3*5, 3*4 - 1*6, 1*5 - 2*4) = (-3, 6, -3).
    let got = ok(&call(
        "cross",
        vec![grid(3, 1, &[1.0, 2.0, 3.0]), grid(3, 1, &[4.0, 5.0, 6.0])],
    ));
    close_all(&reals(&got), &[-3.0, 6.0, -3.0], 0.0);
}

#[test]
fn cross_product_multiplies_the_dimensions() {
    // (1 m, 0 m, 0 m) x (0 s, 1 s, 0 s) = (0, 0, 1 m s). Zeros carry their unit: a bare 0 is
    // dimensionless and would not match the other elements.
    let u_vec = mat(
        3,
        1,
        vec![scale(1.0, u("m")), scale(0.0, u("m")), scale(0.0, u("m"))],
    );
    let v_vec = mat(
        3,
        1,
        vec![scale(0.0, u("s")), scale(1.0, u("s")), scale(0.0, u("s"))],
    );
    let got = ok(&call("cross", vec![u_vec, v_vec]));
    let d = dims(&got);
    let ms = m_dim("m").checked_mul(m_dim("s")).unwrap();
    assert_eq!(d[2], ms);
    close(matrix(&got).data()[2].value.re, 1.0, 0.0);
}

#[test]
fn cross_product_with_negative_and_complex_entries() {
    // u = (-1, 0, 2), v = (0, 3, 1): (0*1 - 2*3, 2*0 - (-1)*1, -1*3 - 0) = (-6, 1, -3).
    let neg = ok(&call(
        "cross",
        vec![grid(3, 1, &[-1.0, 0.0, 2.0]), grid(3, 1, &[0.0, 3.0, 1.0])],
    ));
    close_all(&reals(&neg), &[-6.0, 1.0, -3.0], 0.0);
    // u = (i, 0, 0), v = (0, 1, 0): the third component is i * 1 = i.
    let z = mat(3, 1, vec![v("i"), n(0.0), n(0.0)]);
    let got = ok(&call("cross", vec![z, grid(3, 1, &[0.0, 1.0, 0.0])]));
    complex_close(
        matrix(&got).data()[2].value,
        Complex64::new(0.0, 1.0),
        1e-15,
    );
}

#[test]
fn cross_of_a_two_element_vector_is_a_shape_error() {
    let e = call(
        "cross",
        vec![grid(2, 1, &[1.0, 2.0]), grid(3, 1, &[1.0, 2.0, 3.0])],
    );
    assert_eq!(
        err(&e),
        EvalError::ShapeMismatch {
            left: (2, 1),
            right: (3, 1)
        }
    );
}

// ---- functions not implemented yet ----------------------------------------------------------

// Reason for the change: `last`, `csort` and `rsort` are implemented now (see `builtins_env`), and
// the eigen functions, `geninv` and `Cholesky` (single-matrix forms, see `matrix_decomp2`) are
// implemented. LU, QR and svd still return `Unsupported`: their results are nested matrices.
#[test]
fn factorization_functions_return_nested_results_for_a_singular_matrix() {
    // The all-ones matrix is singular: LU keeps a zero pivot, QR and svd still factor it.
    let sizes = [("LU", 3), ("QR", 3), ("svd", 3)];
    for (name, parts) in sizes {
        match ok(&call(name, vec![grid(2, 2, &[1.0; 4])])) {
            Value::List(items) => assert_eq!(items.len(), parts, "{name}"),
            other => panic!("{name}: expected a nested result, got {other:?}"),
        }
    }
}

#[test]
fn stub_arity_is_still_checked() {
    assert!(matches!(
        err(&call("submatrix", vec![n(1.0)])),
        EvalError::ArgCount { .. }
    ));
}

#[test]
fn sort_keeps_the_dimension_of_each_element() {
    let v = mat(
        3,
        1,
        vec![scale(3.0, u("m")), scale(1.0, u("m")), scale(2.0, u("m"))],
    );
    let got = ok(&call("sort", vec![v]));
    close_all(&reals(&got), &[1.0, 2.0, 3.0], 0.0);
    assert!(dims(&got).iter().all(|d| *d == m_dim("m")));
}

#[test]
fn cross_refuses_row_vectors_since_the_help_defines_column_vectors() {
    // Help: the operator takes three-element column vectors only.
    let row = grid(1, 3, &[1.0, 2.0, 3.0]);
    let col = grid(3, 1, &[4.0, 5.0, 6.0]);
    assert!(matches!(
        err(&call("cross", vec![row.clone(), col.clone()])),
        EvalError::ShapeMismatch { .. }
    ));
    assert!(matches!(
        err(&call("cross", vec![col, row])),
        EvalError::ShapeMismatch { .. }
    ));
}

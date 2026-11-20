//! Singular values (`svds`, and through them `norm2` and `cond2`). Expected values come from the
//! Mathcad Prime help images (example_matrix_factorization_svd, example_special_characterisitcs_
//! of_matrices) and from an exact-rational reference computation.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::expr::Expr;
use crate::units::Dimension;

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

// ---- svds, condition numbers ----------------------------------------------------------------

#[test]
fn svds_of_a_tall_matrix_are_sorted_singular_values() {
    // Reference: singular values of [1 2; 3 4; 5 6] = 9.5255181, 0.5143006.
    let got = ok(&call(
        "svds",
        vec![grid(3, 2, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])],
    ));
    assert_eq!(matrix(&got).shape(), (2, 1));
    close_all(
        &reals(&got),
        &[9.525_518_091_565_106, 0.514_300_580_658_644_3],
        1e-9,
    );
}

#[test]
fn svds_of_the_help_matrix() {
    // Reference: 14.841898, 5.097913, 1.652069; the first is the norm2 of the help image.
    let got = ok(&call("svds", vec![help_m()]));
    close_all(
        &reals(&got),
        &[
            14.841_898_441_682_643,
            5.097_913_248_261_961,
            1.652_068_751_587_948,
        ],
        1e-9,
    );
}

#[test]
fn svds_of_a_diagonal_matrix_keeps_the_dimension() {
    let m = mat(
        2,
        2,
        vec![
            scale(3.0, u("m")),
            scale(0.0, u("m")),
            scale(0.0, u("m")),
            scale(4.0, u("m")),
        ],
    );
    let got = ok(&call("svds", vec![m]));
    close_all(&reals(&got), &[4.0, 3.0], 1e-12);
    assert!(dims(&got).iter().all(|d| *d == m_dim("m")));
}

#[test]
fn svds_of_a_complex_matrix_is_unsupported() {
    let z = mat(2, 2, vec![v("i"), n(0.0), n(0.0), n(1.0)]);
    assert!(matches!(
        err(&call("svds", vec![z])),
        EvalError::Unsupported(_)
    ));
}

/// Relative closeness: `|actual - expected| <= rel * |expected|`.
fn close_rel(actual: f64, expected: f64, rel: f64) {
    assert!(
        (actual - expected).abs() <= rel * expected.abs(),
        "{actual} is not {expected} (relative tolerance {rel})"
    );
}

/// The matrix of the SVD help example as its image shows it: [1 2 -1; 2.3 4 4; -2 5.1 1; 0 0.8 6].
/// (The page text lists the transpose.)
fn help_svd_m() -> Expr {
    grid(
        4,
        3,
        &[1.0, 2.0, -1.0, 2.3, 4.0, 4.0, -2.0, 5.1, 1.0, 0.0, 0.8, 6.0],
    )
}

/// Singular values of `help_svd_m`. Reference: exact rational AtA, roots of its characteristic
/// polynomial by bisection, checked against a float one-sided Jacobi. The image shows 8.657,
/// 5.174, 3.036.
const HELP_SVD_SV: [f64; 3] = [
    8.657_433_488_599_099,
    5.174_278_883_842_083,
    3.035_734_412_413_042,
];

#[test]
fn svds_of_the_help_svd_example_match_the_image() {
    let got = ok(&call("svds", vec![help_svd_m()]));
    assert_eq!(matrix(&got).shape(), (3, 1));
    close_all(&reals(&got), &HELP_SVD_SV, 1e-12);
}

#[test]
fn svds_of_scaled_matrices_are_the_scaled_singular_values() {
    for k in [1e200, 1e-200, 1e-300, 1e300] {
        let got = reals(&ok(&call("svds", vec![mul(n(k), help_svd_m())])));
        assert_eq!(got.len(), 3);
        for (g, s) in got.iter().zip(HELP_SVD_SV) {
            close_rel(*g, s * k, 1e-13);
        }
    }
}

#[test]
fn norm2_and_cond2_are_the_same_for_scaled_matrices() {
    // Help 3 x 3 matrix: singular values 14.841898, 5.097913, 1.652069 (reference computation).
    let norm2_ref = 14.841_898_441_682_643;
    let cond2_ref = 8.983_826_143_686_082;
    for k in [1e200, 1e-300] {
        let norm2 = real_in(&ok(&call("norm2", vec![mul(n(k), help_m())])), dimless());
        close_rel(norm2, norm2_ref * k, 1e-13);
        let cond2 = real_in(&ok(&call("cond2", vec![mul(n(k), help_m())])), dimless());
        close_rel(cond2, cond2_ref, 1e-12);
    }
}

#[test]
fn svds_with_huge_and_tiny_entries_keeps_only_the_largest_scale() {
    // diag(1e200, 1e-200): scaled by the largest entry, the tiny one is 1e-400 and rounds to 0.
    // The result is (1e200, 0); the 1e-200 singular value is lost (documented in matrix_svd).
    let m = mat(2, 2, vec![n(1e200), n(0.0), n(0.0), n(1e-200)]);
    let got = reals(&ok(&call("svds", vec![m])));
    close_rel(got[0], 1e200, 1e-13);
    assert_eq!(got[1], 0.0);
}

#[test]
fn svds_of_repeated_singular_values() {
    close_all(
        &reals(&ok(&call("svds", vec![call("identity", vec![n(3.0)])]))),
        &[1.0, 1.0, 1.0],
        1e-15,
    );
    close_all(
        &reals(&ok(&call("svds", vec![grid(2, 2, &[2.0, 0.0, 0.0, 2.0])]))),
        &[2.0, 2.0],
        1e-15,
    );
    // [1 1; 1 -1] is sqrt(2) times an orthogonal matrix: both singular values are sqrt(2).
    let root2 = 2f64.sqrt();
    close_all(
        &reals(&ok(&call("svds", vec![grid(2, 2, &[1.0, 1.0, 1.0, -1.0])]))),
        &[root2, root2],
        1e-14,
    );
}

#[test]
fn svds_of_the_zero_matrix_is_zero() {
    let got = ok(&call("svds", vec![grid(2, 3, &[0.0; 6])]));
    close_all(&reals(&got), &[0.0, 0.0], 0.0);
}

#[test]
fn svds_refuses_non_finite_entries() {
    let inf = call("svds", vec![grid(1, 2, &[f64::INFINITY, 1.0])]);
    assert_eq!(err(&inf), EvalError::NotFinite);
    let nan = call("svds", vec![grid(1, 1, &[f64::NAN])]);
    assert_eq!(err(&nan), EvalError::NotFinite);
}

//! Norms, condition numbers, rref, rank and linear solves (`lsolve`). Expected values come from
//! the Mathcad Prime help images (example_special_characterisitcs_of_matrices,
//! example_elem_row_oper_on_matrices) and from reference computation. Singular values and
//! `svds` are in `builtins_matrix_svd`.

use super::helpers::*;
use crate::eval::work::{check_units, dense_units, Dense};
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

// ---- norms ----------------------------------------------------------------------------------

#[test]
fn norm1_normi_norme_of_the_help_matrix() {
    // Help image: norm1(M) = 17, normi(M) = 22, norme(M) = 15.78 (sqrt(249)).
    close(
        real_in(&ok(&call("norm1", vec![help_m()])), dimless()),
        17.0,
        1e-12,
    );
    close(
        real_in(&ok(&call("normi", vec![help_m()])), dimless()),
        22.0,
        1e-12,
    );
    close(
        real_in(&ok(&call("norme", vec![help_m()])), dimless()),
        249f64.sqrt(),
        1e-12,
    );
}

#[test]
fn norm2_of_the_help_matrix_is_its_largest_singular_value() {
    // Help image: norm2(M) = 14.842; reference value 14.841898441682643.
    close(
        real_in(&ok(&call("norm2", vec![help_m()])), dimless()),
        14.841_898_441_682_643,
        1e-9,
    );
}

#[test]
fn norme_of_negative_entries_is_positive() {
    // sqrt(9 + 16) = 5.
    let got = ok(&call("norme", vec![grid(2, 2, &[-3.0, 0.0, 0.0, -4.0])]));
    close(real_in(&got, dimless()), 5.0, 1e-12);
}

#[test]
fn vector_norm_of_column_zero_and_of_a_dimensioned_vector() {
    // Help: norm of column 0 (1, 9, 3) is sqrt(91) = 9.539.
    let col0 = grid(3, 1, &[1.0, 9.0, 3.0]);
    close(
        real_in(&ok(&call("norm", vec![col0])), dimless()),
        91f64.sqrt(),
        1e-12,
    );
    let v = mat(2, 1, vec![scale(3.0, u("m")), scale(4.0, u("m"))]);
    close(real_in(&ok(&call("norm", vec![v])), m_dim("m")), 5.0, 1e-12);
}

#[test]
fn norms_keep_the_common_dimension() {
    let m = mat(
        2,
        2,
        vec![
            scale(3.0, u("m")),
            scale(0.0, u("m")),
            scale(0.0, u("m")),
            scale(-4.0, u("m")),
        ],
    );
    close(
        real_in(&ok(&call("norm1", vec![m.clone()])), m_dim("m")),
        4.0,
        1e-12,
    );
    close(
        real_in(&ok(&call("normi", vec![m.clone()])), m_dim("m")),
        4.0,
        1e-12,
    );
    close(
        real_in(&ok(&call("norme", vec![m])), m_dim("m")),
        5.0,
        1e-12,
    );
}

#[test]
fn norm_with_mixed_element_dimensions_is_an_error() {
    let m = mat(
        2,
        2,
        vec![scale(1.0, u("m")), n(0.0), n(0.0), scale(2.0, u("s"))],
    );
    assert!(matches!(
        err(&call("norm1", vec![m])),
        EvalError::DimensionMismatch { .. }
    ));
}

#[test]
fn norm1_of_a_non_square_matrix_is_not_square() {
    assert!(matches!(
        err(&call("norm1", vec![grid(2, 3, &[0.0; 6])])),
        EvalError::NotSquare { .. }
    ));
}

#[test]
fn cond1_of_the_help_matrix_is_12_92() {
    // norm1(M) = 17 and norm1(M^-1) = 0.76 (inverse has column sums 0.392, 0.408, 0.76).
    close(
        real_in(&ok(&call("cond1", vec![help_m()])), dimless()),
        12.92,
        1e-12,
    );
}

#[test]
fn condi_of_the_help_matrix_is_14_96() {
    // normi(M) = 22 and normi(M^-1) = 0.68.
    close(
        real_in(&ok(&call("condi", vec![help_m()])), dimless()),
        14.96,
        1e-12,
    );
}

#[test]
fn conde_and_cond2_of_the_help_matrix() {
    // Reference values: conde = norme(M) * norme(M^-1); cond2 = sigma_1 / sigma_3.
    close(
        real_in(&ok(&call("conde", vec![help_m()])), dimless()),
        10.096_662_418_839_207,
        1e-9,
    );
    // cond2 = sigma_max / sigma_min = 14.841898 / 1.652069.
    close(
        real_in(&ok(&call("cond2", vec![help_m()])), dimless()),
        8.983_826_143_686_082,
        1e-9,
    );
}

#[test]
fn condition_number_of_a_dimensioned_matrix_is_dimensionless() {
    // diag(1 m, 2 m): norm1(M) = 2 m and norm1(M^-1) = max(1, 1/2) / m = 1 / m, so cond1 = 2.
    let m = mat(
        2,
        2,
        vec![
            scale(1.0, u("m")),
            scale(0.0, u("m")),
            scale(0.0, u("m")),
            scale(2.0, u("m")),
        ],
    );
    close(real_in(&ok(&call("cond1", vec![m])), dimless()), 2.0, 1e-12);
}

#[test]
fn condition_number_of_a_singular_matrix_is_singular() {
    let e = call("cond1", vec![grid(2, 2, &[1.0, 2.0, 2.0, 4.0])]);
    assert_eq!(err(&e), EvalError::Singular);
}

// ---- rref, rank -----------------------------------------------------------------------------

#[test]
fn rref_of_a_rank_one_matrix() {
    // [1 2 3; 2 4 6] -> [1 2 3; 0 0 0].
    let got = ok(&call(
        "rref",
        vec![grid(2, 3, &[1.0, 2.0, 3.0, 2.0, 4.0, 6.0])],
    ));
    close_all(&reals(&got), &[1.0, 2.0, 3.0, 0.0, 0.0, 0.0], 1e-12);
}

#[test]
fn rref_of_an_invertible_matrix_is_the_identity() {
    let got = ok(&call("rref", vec![grid(2, 2, &[1.0, 2.0, 3.0, 4.0])]));
    close_all(&reals(&got), &[1.0, 0.0, 0.0, 1.0], 1e-12);
}

#[test]
fn rref_of_a_system_with_negative_solution() {
    // 2x + y - z = 8, -3x - y + 2z = -11, -2x + y + 2z = -3 has (x, y, z) = (2, 3, -1).
    let a = grid(
        3,
        4,
        &[
            2.0, 1.0, -1.0, 8.0, -3.0, -1.0, 2.0, -11.0, -2.0, 1.0, 2.0, -3.0,
        ],
    );
    let got = ok(&call("rref", vec![a]));
    close_all(
        &reals(&got),
        &[1.0, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 3.0, 0.0, 0.0, 1.0, -1.0],
        1e-9,
    );
}

#[test]
fn rref_with_units_is_an_error() {
    let m = mat(1, 2, vec![scale(1.0, u("m")), scale(2.0, u("m"))]);
    assert!(matches!(
        err(&call("rref", vec![m])),
        EvalError::DimensionNotDimensionless { .. }
    ));
}

#[test]
fn rank_counts_independent_columns() {
    close(
        real_in(
            &ok(&call(
                "rank",
                vec![grid(2, 3, &[1.0, 2.0, 3.0, 2.0, 4.0, 6.0])],
            )),
            dimless(),
        ),
        1.0,
        0.0,
    );
    close(
        real_in(
            &ok(&call("rank", vec![grid(2, 2, &[1.0, 2.0, 3.0, 4.0])])),
            dimless(),
        ),
        2.0,
        0.0,
    );
    // Rows (1 2 3), (4 5 6), (7 8 9) are dependent: rank 2.
    let dep = grid(3, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    close(real_in(&ok(&call("rank", vec![dep])), dimless()), 2.0, 0.0);
}

#[test]
fn rank_of_the_identity_is_its_size() {
    let got = ok(&call("rank", vec![call("identity", vec![n(4.0)])]));
    close(real_in(&got, dimless()), 4.0, 0.0);
}

#[test]
fn rref_work_is_charged_on_the_shared_budget() {
    // Gauss-Jordan on 3000 x 3000 is 2/3 * 2.7e10 units, far over the statement budget; 500 x 500
    // is 8.3e7 units, which fits.
    assert!(matches!(
        check_units(dense_units(Dense::Elimination, 3000, 3000, 0)),
        Err(EvalError::WorkLimit(_))
    ));
    assert!(check_units(dense_units(Dense::Elimination, 500, 500, 0)).is_ok());
}

// ---- rank and rref with the absolute TOL ----------------------------------------------------

#[test]
fn rank_of_the_help_matrix_is_three() {
    // Help image (example_special_characterisitcs_of_matrices): rank(M) = 3 for this M. Its third
    // partial pivot is -0.00391, above TOL = 0.001, so it counts. A relative threshold of
    // 0.001 * max|a| = 4.33e-3 would have dropped it and returned 2.
    let m = grid(
        3,
        3,
        &[3.02, -1.05, 2.53, 4.33, 0.56, -1.78, -0.83, -0.54, 1.47],
    );
    close(real_in(&ok(&call("rank", vec![m])), dimless()), 3.0, 0.0);
}

#[test]
fn rank_and_rref_use_an_absolute_tolerance() {
    // TOL is absolute, as the worksheet's is. Scaling by 1e-6 puts every pivot below TOL: rank 0,
    // and rref is the zero matrix. A diagonal pivot of 1e-4 is below TOL, the pivot 1 is not.
    let small = mul(n(1e-6), help_m());
    close(
        real_in(&ok(&call("rank", vec![small.clone()])), dimless()),
        0.0,
        0.0,
    );
    let zero = ok(&call("rref", vec![small]));
    close_all(&reals(&zero), &[0.0; 9], 0.0);
    let d = grid(2, 2, &[1e-4, 0.0, 0.0, 1.0]);
    close(real_in(&ok(&call("rank", vec![d])), dimless()), 1.0, 0.0);
}

#[test]
fn rref_of_the_help_example_matches_the_image() {
    // Help (example_elem_row_oper_on_matrices): A = [2 -1 3 2; 1 4 0 -1; 2 6 -1 5] has
    // rref [1 0 0 17/3; 0 1 0 -5/3; 0 0 1 -11/3]; the image shows 5.667, -1.667, -3.667.
    let a = grid(
        3,
        4,
        &[
            2.0, -1.0, 3.0, 2.0, 1.0, 4.0, 0.0, -1.0, 2.0, 6.0, -1.0, 5.0,
        ],
    );
    let got = ok(&call("rref", vec![a]));
    close_all(
        &reals(&got),
        &[
            1.0,
            0.0,
            0.0,
            17.0 / 3.0,
            0.0,
            1.0,
            0.0,
            -5.0 / 3.0,
            0.0,
            0.0,
            1.0,
            -11.0 / 3.0,
        ],
        1e-12,
    );
}

#[test]
fn rank_of_the_rank_deficient_help_style_matrix_is_two() {
    // Rows (1 2 3), (4 5 6), (7 8 9): the third row is 2 * (row 2) - row 1.
    let dep = grid(3, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    close(real_in(&ok(&call("rank", vec![dep])), dimless()), 2.0, 0.0);
}

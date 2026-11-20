//! Determinant and inverse tests.

use super::*;

// ---- determinant and inverse ---------------------------------------------------------------

fn det_of(rows: usize, vals: &[f64]) -> Scalar {
    det(&matrix(&mat(rows, rows, vals))).unwrap()
}

#[test]
fn det_two_by_two() {
    // 4*3 - 3*6 = -6
    let d = det_of(2, &[4.0, 3.0, 6.0, 3.0]);
    assert!(close(d.value.re, -6.0));
    assert!(d.dim.is_dimensionless());
}

#[test]
fn det_three_by_three() {
    // Known value: -306.
    let d = det_of(3, &[6.0, 1.0, 1.0, 4.0, -2.0, 5.0, 2.0, 8.0, 7.0]);
    assert!(close(d.value.re, -306.0));
}

#[test]
fn det_needs_pivoting() {
    // First pivot is zero; partial pivoting swaps rows. det = -1.
    let d = det_of(2, &[0.0, 1.0, 1.0, 0.0]);
    assert!(close(d.value.re, -1.0));
}

#[test]
fn det_of_singular_is_zero() {
    let d = det_of(2, &[1.0, 2.0, 2.0, 4.0]);
    assert_eq!(d.value.re, 0.0);
}

#[test]
fn det_carries_element_dimension_power() {
    let a = matrix(&mat_dim(
        2,
        2,
        &[(1.0, len()), (2.0, len()), (3.0, len()), (4.0, len())],
    ));
    let d = det(&a).unwrap();
    assert!(close(d.value.re, -2.0));
    assert_eq!(d.dim, len().checked_mul(len()).unwrap());
}

#[test]
fn det_of_identity_five_by_five_is_one() {
    let mut vals = vec![0.0; 25];
    for i in 0..5 {
        vals[i * 5 + i] = 1.0;
    }
    assert!(close(det_of(5, &vals).value.re, 1.0));
}

#[test]
fn det_mixed_dimensions_unsupported() {
    let a = matrix(&mat_dim(
        2,
        2,
        &[(1.0, len()), (1.0, time()), (1.0, len()), (1.0, len())],
    ));
    assert!(matches!(det(&a), Err(EvalError::Unsupported(_))));
}

#[test]
fn det_non_square_fails() {
    assert_eq!(
        det(&matrix(&mat(2, 3, &[0.0; 6]))),
        Err(EvalError::NotSquare { rows: 2, cols: 3 })
    );
}

#[test]
fn inverse_two_by_two() {
    // [[4,7],[2,6]] has det 10; inverse = [[0.6,-0.7],[-0.2,0.4]].
    let inv = inverse(&matrix(&mat(2, 2, &[4.0, 7.0, 2.0, 6.0]))).unwrap();
    let expected = [0.6, -0.7, -0.2, 0.4];
    for (got, want) in values_of(&inv).iter().zip(expected) {
        assert!(close(*got, want), "{got} != {want}");
    }
}

#[test]
fn inverse_three_by_three() {
    let a = [2.0, -1.0, 0.0, -1.0, 2.0, -1.0, 0.0, -1.0, 2.0];
    let inv = inverse(&matrix(&mat(3, 3, &a))).unwrap();
    let expected = [0.75, 0.5, 0.25, 0.5, 1.0, 0.5, 0.25, 0.5, 0.75];
    for (got, want) in values_of(&inv).iter().zip(expected) {
        assert!(close(*got, want), "{got} != {want}");
    }
}

#[test]
fn inverse_times_matrix_is_identity_four_by_four() {
    let a = [
        0.0, 2.0, 1.0, 3.0, //
        1.0, 0.0, 4.0, 2.0, //
        5.0, 1.0, 0.0, 1.0, //
        2.0, 3.0, 1.0, 0.0,
    ];
    let am = mat(4, 4, &a);
    let inv = Value::Matrix(inverse(&matrix(&am)).unwrap());
    let prod = matrix(&mul(&am, &inv).unwrap());
    for (i, v) in values_of(&prod).iter().enumerate() {
        let want = if i % 5 == 0 { 1.0 } else { 0.0 };
        assert!(close(*v, want), "entry {i} is {v}");
    }
}

#[test]
fn inverse_handles_row_swap() {
    let inv = inverse(&matrix(&mat(2, 2, &[0.0, 1.0, 1.0, 0.0]))).unwrap();
    assert_eq!(values_of(&inv), vec![0.0, 1.0, 1.0, 0.0]);
}

#[test]
fn inverse_of_singular_fails() {
    assert_eq!(
        inverse(&matrix(&mat(2, 2, &[1.0, 2.0, 2.0, 4.0]))),
        Err(EvalError::Singular)
    );
}

#[test]
fn inverse_of_numerically_singular_matrix_fails() {
    // det is 0 in exact arithmetic; LU leaves a pivot of about 1e-16 that is not exactly zero.
    assert_eq!(
        inverse(&matrix(&mat(
            3,
            3,
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]
        ))),
        Err(EvalError::Singular)
    );
}

#[test]
fn inverse_of_tiny_diagonal_does_not_underflow() {
    // The pivots are 1e-200; their reciprocals are 1e200.
    let inv = inverse(&matrix(&mat(2, 2, &[1e-200, 0.0, 0.0, 1e-200]))).unwrap();
    assert!(close(inv.data()[0].value.re, 1e200));
    assert!(close(inv.data()[3].value.re, 1e200));
    assert_eq!(inv.data()[1].value.re, 0.0);
    assert_eq!(inv.data()[2].value.re, 0.0);
}

#[test]
fn inverse_dimension_is_inverse_of_element_dimension() {
    let a = matrix(&mat_dim(
        2,
        2,
        &[(2.0, len()), (0.0, len()), (0.0, len()), (4.0, len())],
    ));
    let inv = inverse(&a).unwrap();
    let expected = len().inverse().unwrap();
    assert!(inv.data().iter().all(|s| s.dim == expected));
    assert!(close(inv.data()[0].value.re, 0.5));
    assert!(close(inv.data()[3].value.re, 0.25));
}

#[test]
fn inverse_mixed_dimensions_unsupported() {
    let a = matrix(&mat_dim(
        2,
        2,
        &[
            (1.0, len()),
            (0.0, dimless()),
            (0.0, dimless()),
            (1.0, time()),
        ],
    ));
    assert!(matches!(inverse(&a), Err(EvalError::Unsupported(_))));
}

#[test]
fn inverse_of_identity_eight_by_eight() {
    let n = 8;
    let mut vals = vec![0.0; n * n];
    for i in 0..n {
        vals[i * n + i] = 1.0;
    }
    let inv = inverse(&matrix(&mat(n, n, &vals))).unwrap();
    assert_eq!(values_of(&inv), vals);
}

#[test]
fn inverse_non_square_fails() {
    assert_eq!(
        inverse(&matrix(&mat(1, 2, &[1.0, 2.0]))),
        Err(EvalError::NotSquare { rows: 1, cols: 2 })
    );
}

#[test]
fn det_of_complex_matrix() {
    // [[i, 0],[0, i]] has det -1.
    let m = Matrix::new(
        2,
        2,
        vec![
            Scalar::new(Complex64::new(0.0, 1.0), dimless()),
            Scalar::new(Complex64::new(0.0, 0.0), dimless()),
            Scalar::new(Complex64::new(0.0, 0.0), dimless()),
            Scalar::new(Complex64::new(0.0, 1.0), dimless()),
        ],
    )
    .unwrap();
    assert_close(det(&m).unwrap().value, Complex64::new(-1.0, 0.0));
}

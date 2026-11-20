//! Matrix product, element-wise, power, transpose and magnitude tests.

use super::*;

// ---- matrices: element-wise and product ----------------------------------------------------

#[test]
fn scalar_times_matrix() {
    let r = matrix(&mul(&sc(2.0, dimless()), &mat(2, 2, &[1.0, 2.0, 3.0, 4.0])).unwrap());
    assert_eq!(values_of(&r), vec![2.0, 4.0, 6.0, 8.0]);
}

#[test]
fn scalar_with_units_times_matrix_carries_dimension() {
    let r = matrix(&mul(&sc(3.0, mass()), &mat(1, 2, &[1.0, 2.0])).unwrap());
    assert!(r.data().iter().all(|s| s.dim == mass()));
    assert_eq!(values_of(&r), vec![3.0, 6.0]);
}

#[test]
fn matrix_product_shape() {
    let a = mat(2, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let b = mat(3, 2, &[1.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let r = matrix(&mul(&a, &b).unwrap());
    assert_eq!(r.shape(), (2, 2));
    assert_eq!(values_of(&r), vec![4.0, 5.0, 10.0, 11.0]);
}

#[test]
fn matrix_product_shape_mismatch() {
    let a = mat(2, 3, &[0.0; 6]);
    let b = mat(2, 2, &[0.0; 4]);
    assert_eq!(
        mul(&a, &b),
        Err(EvalError::ShapeMismatch {
            left: (2, 3),
            right: (2, 2)
        })
    );
}

#[test]
fn matrix_product_dimensions_multiply() {
    let a = mat_dim(1, 2, &[(1.0, len()), (2.0, len())]);
    let b = mat_dim(2, 1, &[(3.0, time()), (4.0, time())]);
    let r = matrix(&mul(&a, &b).unwrap());
    assert_eq!(r.shape(), (1, 1));
    assert_eq!(r.data()[0].value.re, 11.0);
    assert_eq!(r.data()[0].dim, len().checked_mul(time()).unwrap());
}

#[test]
fn matrix_product_inconsistent_inner_dimensions_fails() {
    // Sum of m*1 and s*1 inside one output element.
    let a = mat_dim(1, 2, &[(1.0, len()), (1.0, time())]);
    let b = mat(2, 1, &[1.0, 1.0]);
    assert!(matches!(
        mul(&a, &b),
        Err(EvalError::DimensionMismatch { .. })
    ));
}

#[test]
fn matrix_product_each_element_has_its_own_dimension() {
    // Column of lengths and times times row of masses: rows differ in dimension.
    let a = mat_dim(2, 1, &[(1.0, len()), (1.0, time())]);
    let b = mat_dim(1, 2, &[(1.0, mass()), (1.0, mass())]);
    let r = matrix(&mul(&a, &b).unwrap());
    assert_eq!(r.data()[0].dim, len().checked_mul(mass()).unwrap());
    assert_eq!(r.data()[2].dim, time().checked_mul(mass()).unwrap());
}

#[test]
fn matrix_add_elementwise() {
    let r = matrix(&add(&mat(1, 2, &[1.0, 2.0]), &mat(1, 2, &[10.0, 20.0])).unwrap());
    assert_eq!(values_of(&r), vec![11.0, 22.0]);
}

#[test]
fn matrix_add_shape_mismatch() {
    assert_eq!(
        add(&mat(1, 2, &[1.0, 2.0]), &mat(2, 1, &[1.0, 2.0])),
        Err(EvalError::ShapeMismatch {
            left: (1, 2),
            right: (2, 1)
        })
    );
}

#[test]
fn matrix_add_element_dimension_mismatch() {
    let a = mat_dim(1, 2, &[(1.0, len()), (1.0, len())]);
    let b = mat_dim(1, 2, &[(1.0, len()), (1.0, time())]);
    assert!(matches!(
        add(&a, &b),
        Err(EvalError::DimensionMismatch { .. })
    ));
}

#[test]
fn matrix_plus_scalar_broadcasts() {
    let r = matrix(&add(&mat(1, 3, &[1.0, 2.0, 3.0]), &sc(1.0, dimless())).unwrap());
    assert_eq!(values_of(&r), vec![2.0, 3.0, 4.0]);
}

#[test]
fn matrix_divided_by_scalar() {
    let r = matrix(&div(&mat(1, 2, &[2.0, 4.0]), &sc(2.0, dimless())).unwrap());
    assert_eq!(values_of(&r), vec![1.0, 2.0]);
}

#[test]
fn scalar_divided_by_matrix_is_elementwise() {
    let r = matrix(&div(&sc(1.0, dimless()), &mat(2, 2, &[1.0, 2.0, 4.0, 8.0])).unwrap());
    assert_eq!(r.shape(), (2, 2));
    assert_eq!(values_of(&r), vec![1.0, 0.5, 0.25, 0.125]);
}

#[test]
fn scalar_divided_by_matrix_keeps_element_dimension() {
    let r = matrix(&div(&sc(2.0, mass()), &mat(1, 2, &[1.0, 4.0])).unwrap());
    assert_eq!(values_of(&r), vec![2.0, 0.5]);
    assert!(r.data().iter().all(|s| s.dim == mass()));
}

#[test]
fn scalar_divided_by_matrix_with_zero_element_fails() {
    assert_eq!(
        div(&sc(1.0, dimless()), &mat(1, 2, &[0.0, 1.0])),
        Err(EvalError::DivisionByZero)
    );
}

#[test]
fn matrix_divided_by_same_shape_matrix_is_elementwise() {
    let r = matrix(&div(&mat(1, 2, &[2.0, 4.0]), &mat(1, 2, &[2.0, 2.0])).unwrap());
    assert_eq!(values_of(&r), vec![1.0, 2.0]);
}

#[test]
fn matrix_divided_by_square_matrix_multiplies_by_inverse() {
    // [1 2] * inverse(diag(1, 2)) = [1 2] * diag(1, 0.5) = [1 1]
    let x = mat(1, 2, &[1.0, 2.0]);
    let y = mat(2, 2, &[1.0, 0.0, 0.0, 2.0]);
    let r = matrix(&div(&x, &y).unwrap());
    assert_eq!(r.shape(), (1, 2));
    assert_eq!(values_of(&r), vec![1.0, 1.0]);
}

#[test]
fn matrix_divided_by_square_matrix_shape_mismatch_fails() {
    // Inner dimensions 3 and 2 do not agree.
    assert_eq!(
        div(
            &mat(1, 3, &[1.0, 2.0, 3.0]),
            &mat(2, 2, &[1.0, 0.0, 0.0, 1.0])
        ),
        Err(EvalError::ShapeMismatch {
            left: (1, 3),
            right: (2, 2)
        })
    );
    // Divisor is not square.
    assert_eq!(
        div(&mat(1, 2, &[1.0, 2.0]), &mat(2, 1, &[1.0, 2.0])),
        Err(EvalError::ShapeMismatch {
            left: (1, 2),
            right: (2, 1)
        })
    );
}

#[test]
fn matrix_divided_by_singular_matrix_is_singular() {
    assert_eq!(
        div(&mat(1, 2, &[1.0, 2.0]), &mat(2, 2, &[1.0, 2.0, 2.0, 4.0])),
        Err(EvalError::Singular)
    );
}

#[test]
fn negate_matrix() {
    let r = matrix(&neg(&mat(1, 2, &[1.0, -2.0])).unwrap());
    assert_eq!(values_of(&r), vec![-1.0, 2.0]);
}

#[test]
fn matrix_integer_power_matches_repeated_product() {
    let a = mat(2, 2, &[1.0, 1.0, 1.0, 0.0]);
    let sq = matrix(&pow(&a, &sc(2.0, dimless())).unwrap());
    let direct = matrix(&mul(&a, &a).unwrap());
    assert_eq!(values_of(&sq), values_of(&direct));
    assert_eq!(values_of(&sq), vec![2.0, 1.0, 1.0, 1.0]);
}

#[test]
fn matrix_power_zero_is_identity() {
    let a = mat(2, 2, &[5.0, 1.0, 2.0, 3.0]);
    let r = matrix(&pow(&a, &sc(0.0, dimless())).unwrap());
    assert_eq!(values_of(&r), vec![1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn matrix_negative_power_uses_inverse() {
    let a = mat(2, 2, &[4.0, 7.0, 2.0, 6.0]);
    let r = matrix(&pow(&a, &sc(-1.0, dimless())).unwrap());
    let inv = inverse(&matrix(&a)).unwrap();
    for (x, y) in values_of(&r).iter().zip(values_of(&inv)) {
        assert!(close(*x, y));
    }
}

#[test]
fn matrix_power_rejects_non_integer_and_non_square() {
    assert!(matches!(
        pow(&mat(2, 2, &[1.0, 0.0, 0.0, 1.0]), &sc(0.5, dimless())),
        Err(EvalError::Unsupported(_))
    ));
    assert!(matches!(
        pow(&mat(1, 2, &[1.0, 2.0]), &sc(2.0, dimless())),
        Err(EvalError::NotSquare { rows: 1, cols: 2 })
    ));
}

#[test]
fn scalar_to_matrix_power_is_elementwise() {
    let r = matrix(&pow(&sc(2.0, dimless()), &mat(2, 2, &[1.0, 2.0, 3.0, 4.0])).unwrap());
    assert_eq!(r.shape(), (2, 2));
    assert_eq!(values_of(&r), vec![2.0, 4.0, 8.0, 16.0]);
}

#[test]
fn scalar_to_matrix_power_needs_dimensionless_exponent() {
    assert_eq!(
        pow(&sc(2.0, dimless()), &mat_dim(1, 1, &[(2.0, len())])),
        Err(EvalError::DimensionNotDimensionless { dim: len() })
    );
}

#[test]
fn column_vector_power_is_elementwise() {
    let r = matrix(&pow(&mat(3, 1, &[1.0, 2.0, 3.0]), &sc(2.0, dimless())).unwrap());
    assert_eq!(r.shape(), (3, 1));
    assert_eq!(values_of(&r), vec![1.0, 4.0, 9.0]);
}

// ---- dot product -----------------------------------------------------------------------------

fn column(vals: &[Complex64]) -> Value {
    let data = vals
        .iter()
        .map(|z| Scalar::new(*z, dimless()))
        .collect::<Vec<_>>();
    Value::Matrix(Matrix::new(vals.len(), 1, data).unwrap())
}

#[test]
fn column_vectors_multiply_to_dot_product() {
    let r = scalar(&mul(&mat(3, 1, &[1.0, 2.0, 3.0]), &mat(3, 1, &[4.0, 5.0, 6.0])).unwrap());
    assert_eq!(r.value.re, 32.0);
    assert!(r.dim.is_dimensionless());
}

#[test]
fn dot_product_conjugates_second_vector() {
    // [1, 0] . [i, 0] = 1 * conj(i) = -i
    let a = column(&[Complex64::new(1.0, 0.0), Complex64::new(0.0, 0.0)]);
    let b = column(&[Complex64::new(0.0, 1.0), Complex64::new(0.0, 0.0)]);
    let r = scalar(&mul(&a, &b).unwrap());
    assert_close(r.value, Complex64::new(0.0, -1.0));
}

#[test]
fn dot_product_of_complex_vector_with_itself_is_squared_norm() {
    // (1+i) * conj(1+i) + 2 * 2 = 2 + 4 = 6
    let v = column(&[Complex64::new(1.0, 1.0), Complex64::new(2.0, 0.0)]);
    assert_close(
        scalar(&mul(&v, &v).unwrap()).value,
        Complex64::new(6.0, 0.0),
    );
}

#[test]
fn dot_product_dimensions_multiply() {
    let a = mat_dim(2, 1, &[(1.0, len()), (2.0, len())]);
    let b = mat_dim(2, 1, &[(3.0, time()), (4.0, time())]);
    let r = scalar(&mul(&a, &b).unwrap());
    assert_eq!(r.value.re, 11.0);
    assert_eq!(r.dim, len().checked_mul(time()).unwrap());
}

#[test]
fn dot_product_inconsistent_term_dimensions_fails() {
    let a = mat_dim(2, 1, &[(1.0, len()), (1.0, time())]);
    let b = mat(2, 1, &[1.0, 1.0]);
    assert!(matches!(
        mul(&a, &b),
        Err(EvalError::DimensionMismatch { .. })
    ));
}

#[test]
fn dot_product_length_mismatch_fails() {
    assert_eq!(
        mul(&mat(2, 1, &[1.0, 2.0]), &mat(3, 1, &[1.0, 2.0, 3.0])),
        Err(EvalError::ShapeMismatch {
            left: (2, 1),
            right: (3, 1)
        })
    );
}

#[test]
fn one_by_one_product_is_plain_product() {
    let r = matrix(&mul(&mat(1, 1, &[2.0]), &mat(1, 1, &[3.0])).unwrap());
    assert_eq!(values_of(&r), vec![6.0]);
}

// ---- transpose, conjugate, magnitude --------------------------------------------------------

#[test]
fn transpose_two_by_three() {
    let r = matrix(&transpose(&mat(2, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])).unwrap());
    assert_eq!(r.shape(), (3, 2));
    assert_eq!(values_of(&r), vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
}

#[test]
fn transpose_of_scalar_is_itself() {
    let s = sc(4.0, len());
    assert_eq!(transpose(&s).unwrap(), s);
}

#[test]
fn transpose_twice_is_identity() {
    let a = mat(2, 2, &[1.0, 2.0, 3.0, 4.0]);
    let back = transpose(&transpose(&a).unwrap()).unwrap();
    assert_eq!(back, a);
}

#[test]
fn conjugate_matrix_elements() {
    let m = Matrix::new(
        1,
        2,
        vec![
            Scalar::new(Complex64::new(1.0, 2.0), dimless()),
            Scalar::new(Complex64::new(-3.0, 4.0), dimless()),
        ],
    )
    .unwrap();
    let r = matrix(&conjugate(&Value::Matrix(m)).unwrap());
    assert_eq!(r.data()[0].value, Complex64::new(1.0, -2.0));
    assert_eq!(r.data()[1].value, Complex64::new(-3.0, -4.0));
}

#[test]
fn magnitude_of_complex_keeps_dimension() {
    let r = scalar(&abs(&csc(3.0, 4.0, len())).unwrap());
    assert!(close(r.value.re, 5.0));
    assert_eq!(r.value.im, 0.0);
    assert_eq!(r.dim, len());
    assert_eq!(
        magnitude(&csc(3.0, 4.0, len())).unwrap(),
        abs(&csc(3.0, 4.0, len())).unwrap()
    );
}

#[test]
fn map_abs_is_elementwise() {
    let r = matrix(&map_abs(&mat(1, 2, &[-1.0, 2.0])).unwrap());
    assert_eq!(values_of(&r), vec![1.0, 2.0]);
    let s = scalar(&map_abs(&csc(3.0, 4.0, len())).unwrap());
    assert!(close(s.value.re, 5.0));
    assert_eq!(s.dim, len());
}

#[test]
fn abs_of_column_vector_is_euclidean_norm() {
    let r = scalar(&abs(&mat_dim(2, 1, &[(3.0, len()), (4.0, len())])).unwrap());
    assert!(close(r.value.re, 5.0));
    assert_eq!(r.dim, len());
}

#[test]
fn abs_of_square_matrix_is_determinant() {
    // Help page "Absolute Value/Determinant Operator": |M| of a square matrix is det(M).
    // det [[1, 2], [3, 4]] = 1*4 - 2*3 = -2, sign included (not the Euclidean norm sqrt(30)).
    let r = scalar(&abs(&mat(2, 2, &[1.0, 2.0, 3.0, 4.0])).unwrap());
    assert!(close(r.value.re, -2.0));
    assert!(r.dim.is_dimensionless());
    let Value::Matrix(m) = mat(2, 2, &[1.0, 2.0, 3.0, 4.0]) else {
        unreachable!()
    };
    assert_eq!(
        abs(&Value::Matrix(m.clone())).unwrap(),
        Value::Scalar(det(&m).unwrap())
    );
}

#[test]
fn abs_of_square_matrix_with_units_has_power_dimension() {
    // det [[2, 0], [0, 3]] = 6, with elements of length: the dimension is length^2.
    let m = mat_dim(
        2,
        2,
        &[(2.0, len()), (0.0, len()), (0.0, len()), (3.0, len())],
    );
    let r = scalar(&abs(&m).unwrap());
    assert!(close(r.value.re, 6.0));
    assert_eq!(
        r.dim,
        len().pow(crate::units::Exp::from_integer(2)).unwrap()
    );
}

#[test]
fn abs_of_singular_square_matrix_is_zero() {
    let r = scalar(&abs(&mat(2, 2, &[1.0, 2.0, 2.0, 4.0])).unwrap());
    assert_eq!(r.value.re, 0.0);
}

#[test]
fn abs_of_one_by_one_matrix_is_its_magnitude() {
    // A 1x1 matrix is a column vector, so the norm rule applies: |[-3]| = 3.
    let r = scalar(&abs(&mat(1, 1, &[-3.0])).unwrap());
    assert!(close(r.value.re, 3.0));
}

#[test]
fn map_abs_of_square_matrix_is_elementwise() {
    // The vectorized form |M| with the arrow applies the magnitude to each element.
    let r = matrix(&map_abs(&mat(2, 2, &[-1.0, 2.0, -3.0, 4.0])).unwrap());
    assert_eq!(values_of(&r), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn abs_of_complex_column_uses_magnitudes() {
    // |1+i|^2 + |1-i|^2 = 4, so the norm is 2.
    let v = column(&[Complex64::new(1.0, 1.0), Complex64::new(1.0, -1.0)]);
    assert!(close(scalar(&abs(&v).unwrap()).value.re, 2.0));
}

#[test]
fn abs_of_large_elements_does_not_overflow() {
    let r = scalar(&abs(&mat(2, 1, &[1e200, 1e200])).unwrap());
    assert!(close(r.value.re, 2.0_f64.sqrt() * 1e200));
}

#[test]
fn abs_of_unsupported_shapes_fails() {
    assert!(matches!(
        abs(&mat(1, 2, &[1.0, 2.0])),
        Err(EvalError::Unsupported(_))
    ));
    assert!(matches!(
        abs(&mat(2, 3, &[0.0; 6])),
        Err(EvalError::Unsupported(_))
    ));
    assert!(matches!(
        abs(&mat(3, 2, &[0.0; 6])),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn abs_of_mixed_dimension_matrix_fails() {
    let m = mat_dim(2, 1, &[(1.0, len()), (1.0, time())]);
    assert!(matches!(abs(&m), Err(EvalError::Unsupported(_))));
}

#[test]
fn magnitude_of_matrix_is_abs() {
    let m = mat(2, 1, &[3.0, 4.0]);
    assert_eq!(magnitude(&m).unwrap(), abs(&m).unwrap());
}

// ---- comparisons of matrices -----------------------------------------------------------------

#[test]
fn scalar_compared_with_matrix_is_elementwise() {
    let r = matrix(&compare(CmpOp::Lt, &sc(2.0, dimless()), &mat(1, 3, &[1.0, 2.0, 3.0])).unwrap());
    assert_eq!(values_of(&r), vec![0.0, 0.0, 1.0]);
    let r = matrix(&compare(CmpOp::Ge, &mat(1, 3, &[1.0, 2.0, 3.0]), &sc(2.0, dimless())).unwrap());
    assert_eq!(values_of(&r), vec![0.0, 1.0, 1.0]);
}

#[test]
fn matrices_compared_elementwise_give_dimensionless_flags() {
    let r = matrix(&compare(CmpOp::Eq, &mat(1, 2, &[1.0, 2.0]), &mat(1, 2, &[1.0, 3.0])).unwrap());
    assert_eq!(values_of(&r), vec![1.0, 0.0]);
    assert!(r.data().iter().all(|s| s.dim.is_dimensionless()));
}

#[test]
fn matrix_comparison_shape_mismatch_fails() {
    assert_eq!(
        compare(CmpOp::Lt, &mat(1, 2, &[1.0, 2.0]), &mat(2, 1, &[1.0, 2.0])),
        Err(EvalError::ShapeMismatch {
            left: (1, 2),
            right: (2, 1)
        })
    );
}

#[test]
fn matrix_comparison_element_dimension_mismatch_fails() {
    let a = mat_dim(1, 2, &[(1.0, len()), (1.0, len())]);
    let b = mat_dim(1, 2, &[(1.0, len()), (1.0, time())]);
    assert!(matches!(
        compare(CmpOp::Eq, &a, &b),
        Err(EvalError::DimensionMismatch { .. })
    ));
}

// ---- non-finite values are refused -----------------------------------------------------------

#[test]
fn conjugate_refuses_non_finite() {
    assert_eq!(
        conjugate(&sc(f64::NAN, dimless())),
        Err(EvalError::NotFinite)
    );
    assert_eq!(
        conjugate(&sc(f64::INFINITY, dimless())),
        Err(EvalError::NotFinite)
    );
    assert_eq!(
        conjugate(&mat_dim(1, 2, &[(1.0, len()), (f64::NAN, len())])),
        Err(EvalError::NotFinite)
    );
}

#[test]
fn transpose_refuses_non_finite_element() {
    let m = mat_dim(1, 2, &[(1.0, dimless()), (f64::NAN, dimless())]);
    assert_eq!(transpose(&m), Err(EvalError::NotFinite));
}

#[test]
fn comparison_refuses_non_finite_operands() {
    let nan = sc(f64::NAN, dimless());
    let inf = sc(f64::INFINITY, dimless());
    let one = sc(1.0, dimless());
    assert_eq!(compare(CmpOp::Eq, &nan, &nan), Err(EvalError::NotFinite));
    assert_eq!(compare(CmpOp::Lt, &one, &inf), Err(EvalError::NotFinite));
    assert_eq!(
        compare(CmpOp::Ne, &mat(1, 2, &[1.0, 2.0]), &nan),
        Err(EvalError::NotFinite)
    );
}

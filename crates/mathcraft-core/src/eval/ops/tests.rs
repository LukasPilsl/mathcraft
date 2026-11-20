//! Tests for dimension-checked arithmetic. Dimensions use SI exponent order
//! (m, kg, s, A, K, mol, cd); `Dimension::base(0)` is length, `base(1)` mass, `base(2)` time.

use num_complex::Complex64;

use super::*;
use crate::eval::EvalError;
use crate::units::{Dimension, Quantity};

fn len() -> Dimension {
    Dimension::base(0)
}

fn mass() -> Dimension {
    Dimension::base(1)
}

fn time() -> Dimension {
    Dimension::base(2)
}

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

/// Newton: kg*m/s^2.
fn force() -> Dimension {
    Dimension::from_ints([1, 1, -2, 0, 0, 0, 0])
}

fn sc(v: f64, dim: Dimension) -> Value {
    Value::Scalar(Scalar::real(v, dim))
}

fn csc(re: f64, im: f64, dim: Dimension) -> Value {
    Value::Scalar(Scalar::new(Complex64::new(re, im), dim))
}

fn mat(rows: usize, cols: usize, vals: &[f64]) -> Value {
    let data = vals.iter().map(|v| Scalar::real(*v, dimless())).collect();
    Value::Matrix(Matrix::new(rows, cols, data).unwrap())
}

fn mat_dim(rows: usize, cols: usize, vals: &[(f64, Dimension)]) -> Value {
    let data = vals.iter().map(|(v, d)| Scalar::real(*v, *d)).collect();
    Value::Matrix(Matrix::new(rows, cols, data).unwrap())
}

fn scalar(v: &Value) -> Scalar {
    match v {
        Value::Scalar(s) => *s,
        other => panic!("expected a scalar, got {other:?}"),
    }
}

fn matrix(v: &Value) -> Matrix {
    match v {
        Value::Matrix(m) => m.clone(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

fn assert_close(a: Complex64, b: Complex64) {
    assert!(
        close(a.re, b.re) && close(a.im, b.im),
        "{a} is not close to {b}"
    );
}

fn values_of(m: &Matrix) -> Vec<f64> {
    m.data().iter().map(|s| s.value.re).collect()
}

// ---- scalar arithmetic with units ----------------------------------------------------------

#[test]
fn force_from_mass_and_acceleration() {
    let accel = Dimension::from_ints([1, 0, -2, 0, 0, 0, 0]);
    let r = scalar(&mul(&sc(5.0, mass()), &sc(9.81, accel)).unwrap());
    assert!(close(r.value.re, 49.05));
    assert_eq!(r.dim, force());
}

#[test]
fn add_same_dimension() {
    let r = scalar(&add(&sc(1.0, len()), &sc(2.0, len())).unwrap());
    assert_eq!(r.value.re, 3.0);
    assert_eq!(r.dim, len());
}

#[test]
fn add_different_dimensions_fails() {
    let err = add(&sc(1.0, len()), &sc(1.0, time())).unwrap_err();
    assert_eq!(
        err,
        EvalError::DimensionMismatch {
            left: len(),
            right: time()
        }
    );
}

#[test]
fn sub_different_dimensions_fails() {
    assert!(matches!(
        sub(&sc(1.0, len()), &sc(1.0, time())),
        Err(EvalError::DimensionMismatch { .. })
    ));
}

#[test]
fn sub_same_dimension() {
    let r = scalar(&sub(&sc(5.0, mass()), &sc(2.0, mass())).unwrap());
    assert_eq!(r.value.re, 3.0);
    assert_eq!(r.dim, mass());
}

#[test]
fn addition_of_equal_derived_dimensions_is_allowed() {
    // N (built from kg*m/s^2) plus kg*m/s^2 written as a different product.
    let a = sc(1.0, force());
    let accel = len()
        .checked_div(time().checked_mul(time()).unwrap())
        .unwrap();
    let b = sc(1.0, mass().checked_mul(accel).unwrap());
    assert_eq!(scalar(&add(&a, &b).unwrap()).value.re, 2.0);
}

#[test]
fn multiply_dimensions_add_exponents() {
    let r = scalar(&mul(&sc(3.0, len()), &sc(4.0, len())).unwrap());
    assert_eq!(r.value.re, 12.0);
    assert_eq!(r.dim, len().checked_mul(len()).unwrap());
}

#[test]
fn divide_units_gives_speed() {
    let r = scalar(&div(&sc(10.0, len()), &sc(2.0, time())).unwrap());
    assert_eq!(r.value.re, 5.0);
    assert_eq!(r.dim, len().checked_div(time()).unwrap());
}

#[test]
fn divide_by_zero_scalar() {
    assert_eq!(
        div(&sc(1.0, len()), &sc(0.0, time())),
        Err(EvalError::DivisionByZero)
    );
}

#[test]
fn divide_by_zero_complex() {
    assert_eq!(
        div(&csc(1.0, 1.0, dimless()), &csc(0.0, 0.0, dimless())),
        Err(EvalError::DivisionByZero)
    );
}

#[test]
fn complex_product() {
    // (2+3i)(1-i) = 5+i
    let r = scalar(&mul(&csc(2.0, 3.0, dimless()), &csc(1.0, -1.0, dimless())).unwrap());
    assert_close(r.value, Complex64::new(5.0, 1.0));
}

#[test]
fn complex_quotient() {
    // (1+i)/(1-i) = i
    let r = scalar(&div(&csc(1.0, 1.0, dimless()), &csc(1.0, -1.0, dimless())).unwrap());
    assert_close(r.value, Complex64::new(0.0, 1.0));
}

#[test]
fn complex_quotient_by_imaginary_divisor() {
    // (1+i)/(2i) = (1 - i)/2
    let r = scalar(&div(&csc(1.0, 1.0, dimless()), &csc(0.0, 2.0, dimless())).unwrap());
    assert_close(r.value, Complex64::new(0.5, -0.5));
}

#[test]
fn complex_quotient_by_real_multiple_of_divisor() {
    // (2+4i)/(1+i) = 3+i
    let r = scalar(&div(&csc(2.0, 4.0, dimless()), &csc(1.0, 1.0, dimless())).unwrap());
    assert_close(r.value, Complex64::new(3.0, 1.0));
}

#[test]
fn reciprocal_of_tiny_real_does_not_underflow() {
    // 1 / 1e-300 = 1e300; the squared modulus of the divisor would underflow.
    let r = scalar(&div(&sc(1.0, dimless()), &sc(1e-300, dimless())).unwrap());
    assert!(close(r.value.re, 1e300));
}

#[test]
fn ratio_of_tiny_reals_does_not_underflow() {
    let r = scalar(&div(&sc(1e-200, dimless()), &sc(1e-200, dimless())).unwrap());
    assert!(close(r.value.re, 1.0));
}

#[test]
fn complex_quotient_of_tiny_numbers_does_not_underflow() {
    // (1+i)e-200 / (1-i)e-200 = i
    let r = scalar(
        &div(
            &csc(1e-200, 1e-200, dimless()),
            &csc(1e-200, -1e-200, dimless()),
        )
        .unwrap(),
    );
    assert_close(r.value, Complex64::new(0.0, 1.0));
}

#[test]
fn negation_keeps_dimension() {
    let r = scalar(&neg(&sc(2.0, len())).unwrap());
    assert_eq!(r.value.re, -2.0);
    assert_eq!(r.dim, len());
}

#[test]
fn overflow_is_not_finite() {
    assert_eq!(
        mul(&sc(1e200, dimless()), &sc(1e200, dimless())),
        Err(EvalError::NotFinite)
    );
}

#[test]
fn dimension_exponent_overflow() {
    let big = Dimension::from_ints([i32::MAX, 0, 0, 0, 0, 0, 0]);
    assert_eq!(mul(&sc(1.0, big), &sc(1.0, big)), Err(EvalError::Overflow));
}

// ---- power ---------------------------------------------------------------------------------

#[test]
fn sqrt_of_area_is_length() {
    let area = len().checked_mul(len()).unwrap();
    let half = sc(0.5, dimless());
    let r = scalar(&pow(&sc(4.0, area), &half).unwrap());
    assert!(close(r.value.re, 2.0));
    assert_eq!(r.dim, len());
}

#[test]
fn integer_power_of_length() {
    let r = scalar(&pow(&sc(2.0, len()), &sc(3.0, dimless())).unwrap());
    assert_eq!(r.value.re, 8.0);
    assert_eq!(r.dim, Dimension::from_ints([3, 0, 0, 0, 0, 0, 0]));
}

#[test]
fn power_with_unit_exponent_fails() {
    assert_eq!(
        pow(&sc(2.0, dimless()), &sc(3.0, len())),
        Err(EvalError::DimensionNotDimensionless { dim: len() })
    );
}

#[test]
fn zero_exponent_gives_dimensionless_one() {
    let r = scalar(&pow(&sc(7.0, len()), &sc(0.0, dimless())).unwrap());
    assert_eq!(r.value.re, 1.0);
    assert_eq!(r.dim, dimless());
}

#[test]
fn zero_to_negative_power_is_division_by_zero() {
    assert_eq!(
        pow(&sc(0.0, len()), &sc(-1.0, dimless())),
        Err(EvalError::DivisionByZero)
    );
}

#[test]
fn power_of_negative_takes_the_principal_branch() {
    // (-8)^(1/3) = 2 e^(i pi/3) = 1 + 1.732i; (-1)^(1/3) = 0.5 + 0.866i.
    let r = scalar(&pow(&sc(-8.0, dimless()), &sc(1.0 / 3.0, dimless())).unwrap());
    assert!(close(r.value.re, 1.0));
    assert!(close(r.value.im, 3f64.sqrt()));
    let r = scalar(&pow(&sc(-1.0, dimless()), &sc(1.0 / 3.0, dimless())).unwrap());
    assert!(close(r.value.re, 0.5));
    assert!(close(r.value.im, 3f64.sqrt() / 2.0));
}

#[test]
fn integer_power_of_negative_stays_real() {
    let r = scalar(&pow(&sc(-2.0, dimless()), &sc(3.0, dimless())).unwrap());
    assert_eq!(r.value, Complex64::new(-8.0, 0.0));
    let r = scalar(&pow(&sc(-1.0, dimless()), &sc(2.0, dimless())).unwrap());
    assert_eq!(r.value, Complex64::new(1.0, 0.0));
}

#[test]
fn negation_gives_positive_zero_parts() {
    let r = scalar(&neg(&csc(4.0, 0.0, dimless())).unwrap());
    assert_eq!(r.value, Complex64::new(-4.0, 0.0));
    assert!(r.value.im.is_sign_positive());
    assert!(r.value.re.is_sign_negative() && r.value.re == -4.0);
    let z = scalar(&neg(&csc(0.0, 0.0, dimless())).unwrap());
    assert!(z.value.re.is_sign_positive() && z.value.im.is_sign_positive());
}

#[test]
fn conjugate_of_real_has_positive_zero_imaginary_part() {
    let r = scalar(&conjugate(&csc(4.0, 0.0, dimless())).unwrap());
    assert!(r.value.im.is_sign_positive());
}

#[test]
fn scalar_constructor_turns_negative_zero_parts_positive() {
    let s = Scalar::new(Complex64::new(-4.0, -0.0), dimless());
    assert!(s.value.im.is_sign_positive());
    assert!(s.value.re.is_sign_negative());
    assert!((s.value.arg() - std::f64::consts::PI).abs() < 1e-15);
}

#[test]
fn even_root_of_negative_is_complex() {
    let r = scalar(&pow(&sc(-4.0, dimless()), &sc(0.5, dimless())).unwrap());
    assert!(r.value.re.abs() < 1e-12);
    assert!(close(r.value.im, 2.0));
}

#[test]
fn irrational_exponent_on_dimensioned_base_is_unsupported() {
    let exp = sc(std::f64::consts::SQRT_2, dimless());
    assert!(matches!(
        pow(&sc(2.0, len()), &exp),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn irrational_exponent_on_dimensionless_base_is_complex() {
    let exp = sc(std::f64::consts::SQRT_2, dimless());
    let r = scalar(&pow(&sc(2.0, dimless()), &exp).unwrap());
    assert!(close(r.value.re, 2.0_f64.powf(std::f64::consts::SQRT_2)));
    assert_eq!(r.value.im, 0.0);
}

// ---- comparisons ---------------------------------------------------------------------------

fn truth(v: Result<Value, EvalError>) -> bool {
    scalar(&v.unwrap()).value.re == 1.0
}

#[test]
fn comparisons_with_units() {
    let a = sc(1.0, len());
    let b = sc(2.0, len());
    assert!(truth(compare(CmpOp::Lt, &a, &b)));
    assert!(!truth(compare(CmpOp::Gt, &a, &b)));
    assert!(truth(compare(CmpOp::Le, &b, &b)));
    assert!(truth(compare(CmpOp::Ge, &b, &b)));
    assert!(!truth(compare(CmpOp::Le, &b, &a)));
    assert!(truth(compare(CmpOp::Eq, &b, &b)));
    assert!(truth(compare(CmpOp::Ne, &a, &b)));
}

#[test]
fn comparison_result_is_dimensionless() {
    let r = scalar(&compare(CmpOp::Lt, &sc(1.0, len()), &sc(2.0, len())).unwrap());
    assert!(r.dim.is_dimensionless());
}

#[test]
fn comparison_of_different_dimensions_fails() {
    assert!(matches!(
        compare(CmpOp::Lt, &sc(1.0, len()), &sc(1.0, time())),
        Err(EvalError::DimensionMismatch { .. })
    ));
    assert!(matches!(
        compare(CmpOp::Eq, &sc(5.0, len()), &sc(5.0, time())),
        Err(EvalError::DimensionMismatch { .. })
    ));
}

#[test]
fn comparison_compares_si_values() {
    // 1 kg and 1000 kg are not equal even with the same dimension.
    assert!(!truth(compare(
        CmpOp::Eq,
        &sc(1.0, mass()),
        &sc(1000.0, mass())
    )));
}

#[test]
fn complex_equality_and_ordering() {
    let z = csc(1.0, 1.0, dimless());
    assert!(truth(compare(CmpOp::Eq, &z, &z)));
    assert!(matches!(
        compare(CmpOp::Lt, &z, &z),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn text_equality() {
    let a = Value::Str("a".to_string());
    let b = Value::Str("b".to_string());
    assert!(truth(compare(CmpOp::Eq, &a, &a)));
    assert!(truth(compare(CmpOp::Ne, &a, &b)));
    assert!(matches!(
        compare(CmpOp::Lt, &a, &b),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn comparison_of_number_and_text_is_unsupported() {
    assert!(matches!(
        compare(CmpOp::Eq, &sc(1.0, dimless()), &Value::Str("1".to_string())),
        Err(EvalError::Unsupported(_))
    ));
}

// ---- mapping and text ----------------------------------------------------------------------

#[test]
fn map_scalar_squares_each_element() {
    let r = matrix(
        &map_scalar(&mat(1, 3, &[1.0, 2.0, 3.0]), |s| {
            Ok(Scalar::new(s.value * s.value, s.dim))
        })
        .unwrap(),
    );
    assert_eq!(values_of(&r), vec![1.0, 4.0, 9.0]);
}

#[test]
fn map_scalar_on_scalar_and_text() {
    let r = scalar(&map_scalar(&sc(3.0, len()), |s| Ok(*s)).unwrap());
    assert_eq!(r.value.re, 3.0);
    assert!(matches!(
        map_scalar(&Value::Str("x".to_string()), |s| Ok(*s)),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn map_scalar_propagates_errors() {
    assert_eq!(
        map_scalar(&mat(1, 2, &[1.0, 2.0]), |_| Err(EvalError::NotFinite)),
        Err(EvalError::NotFinite)
    );
}

#[test]
fn require_dimensionless_rejects_length() {
    // Stands in for `sin(m)`: the function layer calls this before evaluating.
    assert_eq!(
        require_dimensionless(len()),
        Err(EvalError::DimensionNotDimensionless { dim: len() })
    );
    assert!(require_dimensionless(dimless()).is_ok());
}

#[test]
fn text_is_refused_by_arithmetic() {
    let t = Value::Str("a".to_string());
    assert!(matches!(
        add(&t, &sc(1.0, dimless())),
        Err(EvalError::Unsupported(_))
    ));
    assert!(matches!(mul(&t, &t), Err(EvalError::Unsupported(_))));
    assert!(matches!(neg(&t), Err(EvalError::Unsupported(_))));
    assert!(matches!(transpose(&t), Err(EvalError::Unsupported(_))));
}

#[test]
fn quantity_bridge_round_trip() {
    let q = Quantity::from_si(2.5, len());
    let r = scalar(&Value::Scalar(Scalar::from_quantity(q)));
    assert_eq!(r.value.re, 2.5);
    assert_eq!(r.dim, len());
}

mod linalg;
mod matrix;

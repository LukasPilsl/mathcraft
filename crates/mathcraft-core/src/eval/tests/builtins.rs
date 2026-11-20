//! Builtin functions from the elementary tier, with their unit rules.

use super::helpers::*;
use crate::eval::builtins::{Arity, Registry};
use crate::eval::{evaluate, Env, EvalError, Scalar, Value};
use crate::expr::Expr;
use crate::units::Dimension;
use num_complex::Complex64;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

/// Evaluates `e` in a fresh worksheet and returns the real dimensionless result.
fn real(e: &Expr) -> f64 {
    let v = evaluate(e, &Env::new()).unwrap_or_else(|err| panic!("failed: {err}"));
    real_in(&v, dimless())
}

fn value(e: &Expr) -> Value {
    evaluate(e, &Env::new()).unwrap_or_else(|err| panic!("failed: {err}"))
}

fn err(e: &Expr) -> EvalError {
    evaluate(e, &Env::new()).expect_err("expected an error")
}

fn cplx(e: &Expr) -> Complex64 {
    scalar(&value(e)).value
}

fn i_unit() -> Expr {
    v("i")
}

#[test]
fn sin_of_thirty_degrees_is_one_half() {
    let got = real(&call("sin", vec![scale(30.0, u("°"))]));
    close(got, 0.5, 1e-15);
}

#[test]
fn cos_and_tan_at_known_points() {
    close(real(&call("cos", vec![n(0.0)])), 1.0, 0.0);
    close(real(&call("tan", vec![scale(45.0, u("°"))])), 1.0, 1e-15);
}

#[test]
fn inverse_trig_at_known_points() {
    close(real(&call("asin", vec![n(1.0)])), FRAC_PI_2, 1e-15);
    close(real(&call("acos", vec![n(1.0)])), 0.0, 1e-15);
    close(real(&call("atan", vec![n(1.0)])), FRAC_PI_4, 1e-15);
}

#[test]
fn hyperbolic_functions_at_zero() {
    close(real(&call("sinh", vec![n(0.0)])), 0.0, 0.0);
    close(real(&call("cosh", vec![n(0.0)])), 1.0, 0.0);
    close(real(&call("tanh", vec![n(0.0)])), 0.0, 0.0);
}

#[test]
fn trig_of_a_dimensioned_argument_is_an_error() {
    let e = call("sin", vec![scale(1.0, u("m"))]);
    assert!(matches!(
        err(&e),
        EvalError::DimensionNotDimensionless { .. }
    ));
}

#[test]
fn trig_argument_above_limit_is_refused() {
    let e = call("sin", vec![n(2e12)]);
    assert!(matches!(err(&e), EvalError::Unsupported(_)));
}

#[test]
fn trig_is_element_wise_over_a_vector() {
    let vec = mat(2, 1, vec![n(0.0), n(FRAC_PI_2)]);
    let got = value(&call("sin", vec![vec]));
    let m = matrix(&got);
    close(m.data()[0].value.re, 0.0, 1e-15);
    close(m.data()[1].value.re, 1.0, 1e-15);
}

#[test]
fn exp_of_zero_and_one() {
    close(real(&call("exp", vec![n(0.0)])), 1.0, 0.0);
    close(real(&call("exp", vec![n(1.0)])), std::f64::consts::E, 1e-15);
}

#[test]
fn exp_of_a_dimensioned_argument_is_an_error() {
    assert!(matches!(
        err(&call("exp", vec![scale(1.0, u("s"))])),
        EvalError::DimensionNotDimensionless { .. }
    ));
}

#[test]
fn ln_of_e_is_one_and_ln_of_zero_is_not_finite() {
    close(real(&call("ln", vec![v("e")])), 1.0, 1e-15);
    assert_eq!(err(&call("ln", vec![n(0.0)])), EvalError::NotFinite);
}

#[test]
fn ln_of_negative_is_complex_on_the_principal_branch() {
    complex_close(
        cplx(&call("ln", vec![n(-1.0)])),
        Complex64::new(0.0, PI),
        1e-15,
    );
}

#[test]
fn ln_of_a_dimensioned_argument_is_an_error() {
    assert!(matches!(
        err(&call("ln", vec![scale(1.0, u("m"))])),
        EvalError::DimensionNotDimensionless { .. }
    ));
}

#[test]
fn log_defaults_to_base_ten() {
    close(real(&call("log", vec![n(100.0)])), 2.0, 0.0);
    close(real(&call("log", vec![n(1000.0)])), 3.0, 0.0);
}

#[test]
fn log_with_explicit_base() {
    close(real(&call("log", vec![n(8.0), n(2.0)])), 3.0, 1e-12);
}

#[test]
fn log_base_one_or_non_positive_is_refused() {
    assert!(matches!(
        err(&call("log", vec![n(8.0), n(1.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("log", vec![n(8.0), n(-2.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn sqrt_halves_dimensions_exactly() {
    let got = value(&call("sqrt", vec![pow(scale(4.0, u("m")), n(2.0))]));
    let s = scalar(&got);
    assert_eq!(s.dim, unit_dim("m"));
    close(s.value.re, 4.0, 1e-12);
}

#[test]
fn sqrt_of_area_gives_length() {
    let area = scale(9.0, pow(u("m"), n(2.0)));
    let got = value(&call("sqrt", vec![area]));
    close(real_in(&got, unit_dim("m")), 3.0, 1e-12);
}

#[test]
fn sqrt_of_negative_is_imaginary() {
    complex_close(
        cplx(&call("sqrt", vec![n(-4.0)])),
        Complex64::new(0.0, 2.0),
        1e-12,
    );
}

#[test]
fn abs_keeps_dimension() {
    close(real(&call("abs", vec![n(-3.0)])), 3.0, 0.0);
    let got = value(&call("abs", vec![scale(-3.0, u("m"))]));
    close(real_in(&got, unit_dim("m")), 3.0, 0.0);
}

#[test]
fn re_im_and_arg_of_a_complex_number() {
    // z = 3 + 4 i
    let z = add(n(3.0), mul(n(4.0), i_unit()));
    close(real(&call("re", vec![z.clone()])), 3.0, 0.0);
    close(real(&call("im", vec![z.clone()])), 4.0, 0.0);
    close(real(&call("abs", vec![z])), 5.0, 1e-15);
}

#[test]
fn arg_of_i_is_a_right_angle() {
    close(real(&call("arg", vec![i_unit()])), FRAC_PI_2, 1e-15);
}

#[test]
fn floor_and_ceil_of_real_numbers() {
    close(real(&call("floor", vec![n(2.7)])), 2.0, 0.0);
    close(real(&call("floor", vec![n(-2.5)])), -3.0, 0.0);
    close(real(&call("ceil", vec![n(2.1)])), 3.0, 0.0);
}

#[test]
fn floor_of_a_dimensioned_value_is_refused_as_the_help_states() {
    assert!(matches!(
        err(&call("floor", vec![scale(2.5, u("m"))])),
        EvalError::DimensionNotDimensionless { .. }
    ));
}

#[test]
fn round_to_integer_and_to_places() {
    close(real(&call("round", vec![n(2.5)])), 3.0, 0.0);
    close(real(&call("round", vec![n(-2.5)])), -3.0, 0.0);
    close(real(&call("round", vec![n(2.567), n(2.0)])), 2.57, 1e-12);
    close(real(&call("round", vec![n(1234.0), n(-2.0)])), 1200.0, 1e-9);
}

#[test]
fn sign_of_real_numbers() {
    close(real(&call("sign", vec![n(-2.0)])), -1.0, 0.0);
    close(real(&call("sign", vec![n(0.0)])), 0.0, 0.0);
    close(real(&call("sign", vec![n(7.0)])), 1.0, 0.0);
}

#[test]
fn sign_of_complex_is_refused() {
    assert!(matches!(
        err(&call("sign", vec![i_unit()])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn min_and_max_of_several_scalars() {
    close(real(&call("min", vec![n(3.0), n(1.0), n(2.0)])), 1.0, 0.0);
    close(real(&call("max", vec![n(3.0), n(1.0), n(2.0)])), 3.0, 0.0);
}

#[test]
fn min_and_max_of_a_vector() {
    let vec = mat(3, 1, vec![n(3.0), n(-1.0), n(2.0)]);
    close(real(&call("min", vec![vec.clone()])), -1.0, 0.0);
    close(real(&call("max", vec![vec])), 3.0, 0.0);
}

#[test]
fn min_keeps_the_common_dimension() {
    let got = value(&call("min", vec![scale(1.0, u("m")), scale(2.0, u("m"))]));
    close(real_in(&got, unit_dim("m")), 1.0, 0.0);
}

#[test]
fn min_of_mixed_dimensions_is_an_error() {
    let e = call("min", vec![scale(1.0, u("m")), scale(2.0, u("s"))]);
    assert!(matches!(err(&e), EvalError::DimensionMismatch { .. }));
}

#[test]
fn builtin_argument_count_is_checked() {
    assert!(matches!(
        err(&call("sin", vec![n(1.0), n(2.0)])),
        EvalError::ArgCount { .. }
    ));
    assert!(matches!(
        err(&call("min", vec![])),
        EvalError::ArgCount { .. }
    ));
    assert!(matches!(
        err(&call("log", vec![])),
        EvalError::ArgCount { .. }
    ));
}

#[test]
fn registry_accepts_later_tiers() {
    fn double(args: &[Value]) -> Result<Value, EvalError> {
        match args {
            [Value::Scalar(s)] => Ok(Value::Scalar(Scalar::new(s.value * 2.0, s.dim))),
            _ => Err(EvalError::Unsupported(
                "double takes one scalar".to_string(),
            )),
        }
    }
    let mut env = Env::new();
    env.builtins_mut()
        .register("double", Arity::Exact(1), double);
    let got = evaluate(&call("double", vec![n(21.0)]), &env).unwrap();
    close(real_in(&got, dimless()), 42.0, 0.0);
    assert!(Registry::new().get("double").is_none());
}

#[test]
fn trunc_drops_the_fraction_toward_zero_and_floor_does_not() {
    close(real(&call("trunc", vec![n(-2.6)])), -2.0, 0.0);
    close(real(&call("trunc", vec![n(2.6)])), 2.0, 0.0);
    close(real(&call("floor", vec![n(-2.6)])), -3.0, 0.0);
}

#[test]
fn ln0_is_minus_1e307_at_zero() {
    close(real(&call("ln0", vec![n(0.0)])), -1e307, 0.0);
    close(real(&call("ln0", vec![v("e")])), 1.0, 1e-15);
}

#[test]
fn uppercase_two_argument_rounding_to_a_multiple() {
    close(real(&call("Floor", vec![n(7.0), n(2.0)])), 6.0, 0.0);
    close(real(&call("Ceil", vec![n(7.0), n(2.0)])), 8.0, 0.0);
    close(real(&call("Round", vec![n(7.0), n(2.0)])), 8.0, 0.0);
    close(real(&call("Trunc", vec![n(-7.0), n(2.0)])), -6.0, 0.0);
}

#[test]
fn ceil_to_a_multiple_with_units() {
    // Ceil(3.23 m, ft) is 11 ft: the smallest multiple of 1 ft at or above 3.23 m.
    let got = value(&call("Ceil", vec![scale(3.23, u("m")), u("ft")]));
    close(real_in(&got, unit_dim("m")), 11.0 * 0.3048, 1e-12);
}

#[test]
fn two_argument_rounding_refuses_mixed_dimensions() {
    let e = call("Floor", vec![scale(2.0, u("m")), scale(1.0, u("s"))]);
    assert!(matches!(err(&e), EvalError::DimensionMismatch { .. }));
}

#[test]
fn two_argument_rounding_refuses_zero_step() {
    assert_eq!(
        err(&call("Round", vec![n(1.0), n(0.0)])),
        EvalError::DivisionByZero
    );
}

/// OPEN QUESTION for the shareholder (check in live Prime): for a real argument above 1, `asin`
/// and `acos` return the complex principal value of `num_complex`. This test pins today's output
/// so a change is visible. Do not change `asin`/`acos` until the sign convention is confirmed.
#[test]
fn asin_acos_branch_cut_open_question() {
    complex_close(
        cplx(&call("asin", vec![n(2.0)])),
        Complex64::new(FRAC_PI_2, -(2.0 + 3f64.sqrt()).ln()),
        1e-12,
    );
    complex_close(
        cplx(&call("acos", vec![n(2.0)])),
        Complex64::new(0.0, (2.0 + 3f64.sqrt()).ln()),
        1e-12,
    );
}

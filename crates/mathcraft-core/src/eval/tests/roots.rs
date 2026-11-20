//! Powers, nth roots and square roots, with the principal branch and signed zeros.
//!
//! Help (`square_root_and_nth_root_operator`): `nthRoot(x, n)` returns a real root when one
//! exists, so an odd `n` gives a negative root of a negative `x`; `x^(1/n)` is the principal
//! branch, `(-1)^(1/3) = 0.5 + 0.866i`. Integer powers stay real.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::expr::Expr;
use crate::units::Dimension;
use num_complex::Complex64;
use std::f64::consts::PI;

#[test]
fn guess_root_from_a_flat_start_is_found_by_bracketing() {
    // x^3 - 1 from 0: the secant runs away (its slope at 0 is nearly flat), and a bracket search from
    // the guess finds the real root 1.
    use crate::eval::{Outcome, Session, Value};
    use crate::parser::{parse, DefaultIdContext};
    let mut s = Session::new();
    s.run(&parse("x := 0", &DefaultIdContext).expect("parses"));
    match s.run(&parse("root(x^3 - 1, x)=", &DefaultIdContext).expect("parses")) {
        Outcome::Value(Value::Scalar(sc)) => {
            assert!((sc.value.re - 1.0).abs() < 1e-9, "{}", sc.value.re)
        }
        other => panic!("{other:?}"),
    }
}

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

fn eval(e: &Expr) -> Value {
    evaluate(e, &Env::new()).unwrap_or_else(|err| panic!("failed: {err}"))
}

fn cplx(e: &Expr) -> Complex64 {
    scalar(&eval(e)).value
}

#[test]
fn principal_cube_root_of_minus_one() {
    let z = cplx(&pow(neg(n(1.0)), div(n(1.0), n(3.0))));
    complex_close(z, Complex64::new(0.5, 3f64.sqrt() / 2.0), 1e-12);
}

#[test]
fn principal_cube_root_of_minus_eight_is_one_plus_i_root_three() {
    let z = cplx(&pow(n(-8.0), div(n(1.0), n(3.0))));
    complex_close(z, Complex64::new(1.0, 3f64.sqrt()), 1e-12);
}

#[test]
fn nth_root_operator_gives_the_real_odd_root() {
    // nthRoot(-8, 3) = -2, as the help's real-root rule says.
    close(
        real_in(&eval(&Expr::nth_root(n(3.0), n(-8.0))), dimless()),
        -2.0,
        1e-12,
    );
    close(
        real_in(&eval(&Expr::nth_root(n(3.0), neg(n(8.0)))), dimless()),
        -2.0,
        1e-12,
    );
}

#[test]
fn nth_root_operator_of_negative_with_even_degree_is_complex() {
    let z = scalar(&eval(&Expr::nth_root(n(2.0), n(-8.0)))).value;
    complex_close(z, Complex64::new(0.0, 8f64.sqrt()), 1e-12);
}

#[test]
fn square_root_of_negative_is_plus_two_i_for_negation_and_for_literal() {
    complex_close(
        cplx(&Expr::sqrt(neg(n(4.0)))),
        Complex64::new(0.0, 2.0),
        1e-12,
    );
    complex_close(cplx(&Expr::sqrt(n(-4.0))), Complex64::new(0.0, 2.0), 1e-12);
    assert!(scalar(&eval(&Expr::sqrt(neg(n(4.0)))))
        .value
        .im
        .is_sign_positive());
}

#[test]
fn integer_powers_of_negatives_stay_real() {
    assert_eq!(cplx(&pow(neg(n(1.0)), n(2.0))), Complex64::new(1.0, 0.0));
    assert_eq!(cplx(&pow(neg(n(2.0)), n(3.0))), Complex64::new(-8.0, 0.0));
}

#[test]
fn log_of_negated_one_is_plus_i_pi() {
    let z = cplx(&call("ln", vec![neg(n(1.0))]));
    complex_close(z, Complex64::new(0.0, PI), 1e-15);
    assert!(z.im.is_sign_positive());
    complex_close(
        cplx(&call("ln", vec![n(-1.0)])),
        Complex64::new(0.0, PI),
        1e-15,
    );
}

#[test]
fn argument_of_negated_real_is_pi_not_minus_pi() {
    close(
        real_in(&eval(&call("arg", vec![neg(n(4.0))])), dimless()),
        PI,
        1e-15,
    );
    close(
        real_in(&eval(&call("arg", vec![n(-4.0)])), dimless()),
        PI,
        1e-15,
    );
}

#[test]
fn negative_root_degree_is_refused() {
    assert!(matches!(
        evaluate(&Expr::nth_root(n(-3.0), n(8.0)), &Env::new()),
        Err(EvalError::Unsupported(_))
    ));
}

// ---- exact square roots and half powers ----------------------------------------------------

#[test]
fn square_root_of_negative_four_is_exactly_two_i() {
    assert_eq!(cplx(&Expr::sqrt(n(-4.0))), Complex64::new(0.0, 2.0));
}

#[test]
fn half_power_of_negative_four_is_exactly_two_i() {
    assert_eq!(cplx(&pow(n(-4.0), n(0.5))), Complex64::new(0.0, 2.0));
}

#[test]
fn half_power_of_negative_one_is_exactly_i() {
    assert_eq!(cplx(&pow(n(-1.0), n(0.5))), Complex64::new(0.0, 1.0));
}

#[test]
fn square_root_of_negative_nine_metres_squared_is_three_i_metres() {
    let got = scalar(&eval(&Expr::sqrt(scale(-9.0, pow(u("m"), n(2.0))))));
    assert_eq!(got.value, Complex64::new(0.0, 3.0));
    assert_eq!(got.dim, unit_dim("m"));
}

#[test]
fn square_root_of_four_is_exactly_two() {
    assert_eq!(cplx(&Expr::sqrt(n(4.0))), Complex64::new(2.0, 0.0));
}

#[test]
fn square_root_of_two_is_within_one_ulp() {
    let got = cplx(&Expr::sqrt(n(2.0))).re;
    let want = 2f64.sqrt();
    assert!((got - want).abs() <= want * f64::EPSILON, "{got} vs {want}");
}

#[test]
fn three_halves_power_of_negative_four_is_minus_eight_i() {
    complex_close(
        cplx(&pow(n(-4.0), n(1.5))),
        Complex64::new(0.0, -8.0),
        1e-12,
    );
}

#[test]
fn integer_powers_of_negative_base_are_exact() {
    assert_eq!(cplx(&pow(n(-2.0), n(3.0))), Complex64::new(-8.0, 0.0));
    assert_eq!(cplx(&pow(n(-2.0), n(-3.0))), Complex64::new(-0.125, 0.0));
}

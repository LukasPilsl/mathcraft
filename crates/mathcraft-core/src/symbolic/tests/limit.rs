//! Symbolic limits: help examples, refusals, L'Hopital, one-sided limits, and property checks of
//! rational limits against numeric values near the point.

use crate::symbolic::{approximate, limit_of, Point, Side, Sym, SymError};

use super::{sy, Lcg};

fn at(src: &str) -> Point {
    Point::Finite(sy(src))
}

fn lim(src: &str, point: &Point, side: Side) -> Result<Sym, SymError> {
    limit_of(&sy(src), "x", point, side)
}

fn is_unsupported(r: &Result<Sym, SymError>) -> bool {
    matches!(r, Err(SymError::Unsupported(_)))
}

#[test]
fn removable_factor_is_cancelled_before_substituting() {
    assert_eq!(lim("(x^2-1)/(x-1)", &at("1"), Side::Both), Ok(sy("2")));
    assert_eq!(lim("(x^2+3*x)/x", &at("0"), Side::Both), Ok(sy("3")));
    // Both one-sided limits of a removable point are the same value.
    assert_eq!(lim("(x^2-1)/(x-1)", &at("1"), Side::Right), Ok(sy("2")));
    assert_eq!(lim("(x^2-1)/(x-1)", &at("1"), Side::Left), Ok(sy("2")));
}

#[test]
fn a_pole_is_refused_not_guessed() {
    assert!(is_unsupported(&lim("1/x", &at("0"), Side::Both)));
    assert!(is_unsupported(&lim("1/x", &at("0"), Side::Right)));
    assert!(is_unsupported(&lim("1/x", &at("0"), Side::Left)));
    assert!(is_unsupported(&lim("(x^2+1)/(x-1)", &at("1"), Side::Both)));
}

#[test]
fn limits_at_infinity_compare_degrees() {
    let inf = Point::PosInfinity;
    let minf = Point::NegInfinity;
    assert_eq!(lim("(3*x^2+1)/(2*x^2-5)", &inf, Side::Both), Ok(sy("3/2")));
    assert_eq!(lim("(3*x^2+1)/(2*x^2-5)", &minf, Side::Both), Ok(sy("3/2")));
    assert_eq!(lim("1/x", &inf, Side::Both), Ok(sy("0")));
    assert_eq!(lim("x/(x^2+1)", &minf, Side::Both), Ok(sy("0")));
    // A numerator of higher degree grows without bound: refused.
    assert!(is_unsupported(&lim("x^2/x", &inf, Side::Both)));
    assert!(is_unsupported(&lim("x^3/(x^2+1)", &inf, Side::Both)));
}

#[test]
fn a_function_that_is_not_rational_at_infinity_is_refused() {
    assert!(is_unsupported(&lim(
        "sqrt(x)/x",
        &Point::PosInfinity,
        Side::Both
    )));
    assert!(is_unsupported(&lim(
        "exp(x)/x",
        &Point::PosInfinity,
        Side::Both
    )));
}

#[test]
fn standard_limits_by_lhopital() {
    assert_eq!(lim("sin(x)/x", &at("0"), Side::Both), Ok(sy("1")));
    assert_eq!(lim("(1-cos(x))/x^2", &at("0"), Side::Both), Ok(sy("1/2")));
    assert_eq!(lim("(exp(x)-1)/x", &at("0"), Side::Both), Ok(sy("1")));
    assert_eq!(lim("(exp(x)-1-x)/x^2", &at("0"), Side::Both), Ok(sy("1/2")));
    // Away from zero the value is the quotient itself.
    assert_eq!(lim("sin(x)/x", &at("1"), Side::Both), Ok(sy("sin(1)")));
}

#[test]
fn lhopital_rounds_are_counted() {
    // (x - sin x)/x^3 = 1/6 needs three rounds, within the cap.
    assert_eq!(lim("(x-sin(x))/x^3", &at("0"), Side::Both), Ok(sy("1/6")));
    // (1-cos x)/x^10: the denominator keeps vanishing while the numerator does not, so the
    // limit is infinite and refused.
    assert!(is_unsupported(&lim(
        "(1-cos(x))/x^10",
        &at("0"),
        Side::Both
    )));
}

#[test]
fn one_sided_limit_of_a_non_analytic_function_is_refused() {
    // ln(x)/x at 1 is 0 from both sides, but the side is not decided by this stage.
    assert!(is_unsupported(&lim("ln(x)/x", &at("1"), Side::Right)));
    assert_eq!(lim("ln(x)/x", &at("1"), Side::Both), Ok(sy("0")));
}

#[test]
fn symbolic_coefficients_and_constants() {
    // (a x)/x is a for every x other than 0: the monomial cancels, so the limit is a.
    assert_eq!(lim("(a*x)/x", &at("0"), Side::Both), Ok(sy("a")));
    // A symbolic denominator value is not decided here: refused.
    assert!(is_unsupported(&lim("1/(x+a)", &at("0"), Side::Both)));
    // A constant in x is its own limit.
    assert_eq!(lim("a+2", &at("3"), Side::Both), Ok(sy("a+2")));
}

#[test]
fn rational_limits_match_numeric_values_near_the_point() {
    // For a rational function whose denominator does not vanish at a, the limit is the value at
    // a, and the value at a + 1e-6 is close to it (property check, double precision).
    let mut rng = Lcg(7);
    for _ in 0..200 {
        let c = |rng: &mut Lcg| rng.range(-4, 4);
        let (n0, n1, n2) = (c(&mut rng), c(&mut rng), c(&mut rng));
        let (d0, d1, d2) = (c(&mut rng), c(&mut rng), c(&mut rng));
        let a = rng.range(-2, 2);
        let dval = |x: f64| (d0 as f64) + (d1 as f64) * x + (d2 as f64) * x * x;
        let nval = |x: f64| (n0 as f64) + (n1 as f64) * x + (n2 as f64) * x * x;
        if dval(a as f64).abs() < 1e-9 {
            continue;
        }
        let src = format!("(({n0}) + ({n1})*x + ({n2})*x^2)/(({d0}) + ({d1})*x + ({d2})*x^2)");
        let got = limit_of(&sy(&src), "x", &Point::Finite(Sym::int(a)), Side::Both)
            .unwrap_or_else(|e| panic!("{src} at {a}: {e:?}"));
        let value = approximate(&got).expect("exact value");
        let near = nval(a as f64 + 1e-6) / dval(a as f64 + 1e-6);
        assert!(
            (value - near).abs() <= 1e-5 * (1.0 + value.abs()),
            "{src} at {a}: {value} vs {near}"
        );
    }
}

#[test]
fn sin_kx_over_x_is_k_for_every_k() {
    for k in -3..=3i64 {
        let src = format!("sin({k}*x)/x");
        let got = lim(&src, &at("0"), Side::Both).expect("standard limit");
        assert_eq!(got, sy(&k.to_string()), "{src}");
        // The quotient itself is close to k near zero (double precision).
        let h = 1e-6_f64;
        let near = ((k as f64) * h).sin() / h;
        assert!((near - k as f64).abs() < 1e-6, "{src}");
    }
}

#[test]
fn complex_points_and_constants_are_refused() {
    // (-1)^(1/2) is i: the real limit is not answered.
    assert!(is_unsupported(&lim("x", &at("(-1)^(1/2)"), Side::Both)));
    assert!(is_unsupported(&lim("5", &at("(-1)^(1/2)"), Side::Both)));
    assert!(is_unsupported(&lim("(-1)^(1/2)", &at("2"), Side::Both)));
    // A real constant limit is still its value.
    assert_eq!(lim("5", &at("2"), Side::Both), Ok(sy("5")));
}

#[test]
fn a_constant_body_needs_a_numeric_point() {
    // The variable does not appear: the point must be a number (here a symbol is refused).
    assert!(is_unsupported(&lim("5", &at("a"), Side::Both)));
    assert_eq!(lim("5", &at("pi"), Side::Both), Ok(sy("5")));
    // Point using the variable is invalid.
    assert!(matches!(
        lim("x", &at("x"), Side::Both),
        Err(SymError::Invalid(_))
    ));
}

#[test]
fn two_sided_limit_undefined_on_one_side_is_refused() {
    // sqrt(x) is not real for x < 0: the two-sided limit at 0 is refused.
    assert!(is_unsupported(&lim("sqrt(x)", &at("0"), Side::Both)));
    // One-sided limits are not decided by the other side (a non-analytic function is still
    // refused for one-sided limits, as before).
    assert!(is_unsupported(&lim("sqrt(x)", &at("0"), Side::Right)));
}

#[test]
fn standard_limits_with_zero_hyperbolic_values() {
    // sinh(0) = 0 and cosh(0) = 1 fold exactly, so these are not refused.
    assert_eq!(lim("sinh(x)/x", &at("0"), Side::Both), Ok(sy("1")));
    assert_eq!(lim("(cosh(x)-1)/x^2", &at("0"), Side::Both), Ok(sy("1/2")));
    assert_eq!(lim("tanh(x)/x", &at("0"), Side::Both), Ok(sy("1")));
}

//! Tests of the numeric calculus operators (see `calculus.rs`). Expressions are written as text
//! and parsed, so the tests also exercise the text forms.

use super::{derivative_tol, panel, MAX_TERMS};
use crate::eval::{evaluate, EvalError, Outcome, Session, Value};
use crate::expr::{Accumulation, Expr, Id, IdKind, Stmt};
use crate::parser::{parse, DefaultIdContext, IdContext};
use crate::units::{self, Dimension};
use num_complex::Complex64;

/// Names as the worksheet reads them: `m` and `s` are units, `π` a constant.
struct Units;

impl IdContext for Units {
    fn classify(&self, name: &str, called: bool) -> IdKind {
        if called {
            return IdKind::Function;
        }
        match name {
            "m" | "s" => IdKind::Unit,
            "π" => IdKind::Constant,
            _ => IdKind::Variable,
        }
    }
}

/// Runs each statement in one worksheet (definitions first), then evaluates `src`.
fn run(defs: &[&str], src: &str) -> Result<Value, EvalError> {
    let mut session = Session::new();
    for d in defs {
        let stmt = parse(d, &Units).unwrap_or_else(|e| panic!("{d}: {e}"));
        session.run(&stmt);
    }
    let e = parse(src, &Units).unwrap_or_else(|e| panic!("{src}: {e}"));
    evaluate(&e, session.env())
}

/// The real SI value of a dimensionless or unit-carrying scalar result, with its dimension.
fn value(src: &str, defs: &[&str]) -> (f64, Dimension) {
    match run(defs, src) {
        Ok(Value::Scalar(s)) => {
            assert!(s.value.im.abs() < 1e-300, "complex result {}", s.value);
            (s.value.re, s.dim)
        }
        other => panic!("{src}: expected a scalar, got {other:?}"),
    }
}

fn number(src: &str, defs: &[&str]) -> f64 {
    let (x, dim) = value(src, defs);
    assert!(dim.is_dimensionless(), "{src}: dimension {dim}");
    x
}

fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "{actual} is not {expected} (tolerance {tol})"
    );
}

fn dim_of(name: &str) -> Dimension {
    units::unit(name).expect("unit").dimension
}

// ----- the rule itself ----------------------------------------------------------------------------

#[test]
fn kronrod_panel_is_exact_for_polynomials_up_to_degree_22() {
    // One G7K15 panel over [-1, 1] integrates x^k exactly for k <= 22 (the Kronrod degree).
    for k in 0..=22 {
        let mut f = |x: f64| -> Result<Complex64, EvalError> { Ok(Complex64::new(x.powi(k), 0.0)) };
        let p = panel(&mut f, -1.0, 1.0).expect("panel");
        let exact = if k % 2 == 0 {
            2.0 / f64::from(k + 1)
        } else {
            0.0
        };
        close(p.value.re, exact, 1e-13);
        assert!(
            p.err < 1e-12 || k > 13,
            "error estimate for x^{k}: {}",
            p.err
        );
    }
}

// ----- summation and product ----------------------------------------------------------------------

#[test]
fn sum_over_a_range() {
    close(number("sum(i, 1, 10, i)", &[]), 55.0, 1e-12);
    close(number("sum(i, 1, 10, i^2)", &[]), 385.0, 1e-12);
}

#[test]
fn partial_sum_of_inverse_squares() {
    let want: f64 = (1..=100).map(|i| 1.0 / f64::from(i * i)).sum();
    close(number("sum(n, 1, 100, 1/n^2)", &[]), want, 1e-14);
}

#[test]
fn geometric_sum() {
    let want = (1.0 - 0.5f64.powi(11)) / 0.5;
    close(number("sum(i, 0, 10, 0.5^i)", &[]), want, 1e-14);
}

#[test]
fn product_over_a_range() {
    close(number("product(i, 1, 5, i)", &[]), 120.0, 1e-9);
}

#[test]
fn empty_ranges_give_zero_and_one() {
    close(number("sum(i, 5, 1, i)", &[]), 0.0, 0.0);
    close(number("product(i, 5, 1, i)", &[]), 1.0, 0.0);
}

#[test]
fn non_integer_limits_count_the_terms_that_fit() {
    // i = 0.4, 1.4, 2.4, 3.4: four terms, since 4.4 > 3.7.
    close(number("sum(i, 0.4, 3.7, 1)", &[]), 4.0, 1e-12);
}

#[test]
fn bound_variable_shadows_the_worksheet() {
    close(number("sum(i, 1, 3, i)", &["i := 100"]), 6.0, 1e-12);
    // The worksheet's i is not changed by the sum.
    close(number("i", &["i := 100"]), 100.0, 0.0);
}

#[test]
fn vector_form_folds_every_element() {
    close(number("sum([1, 2, 3])", &[]), 6.0, 1e-12);
    close(number("product([1, 2, 3, 4])", &[]), 24.0, 1e-12);
}

#[test]
fn huge_range_is_refused_without_running() {
    match run(&[], "sum(i, 1, 1e12, 1)") {
        Err(EvalError::TooManyTerms { terms, limit }) => {
            assert!(terms > MAX_TERMS as f64);
            assert_eq!(limit, MAX_TERMS);
        }
        other => panic!("expected TooManyTerms, got {other:?}"),
    }
}

#[test]
fn absurd_range_reports_its_true_count() {
    // The count is not saturated to an integer: 1e30 terms reads as about 1e30 in the message.
    let err = run(&[], "sum(i, 1, 1e30, 1)").unwrap_err();
    assert!(
        matches!(err, EvalError::TooManyTerms { terms, .. } if terms > 1e29),
        "{err:?}"
    );
    assert!(err.to_string().contains("e30"), "{err}");
}

#[test]
fn range_rounding_slack_is_relative_to_the_limits() {
    // 0.9999999999 - 0 is just under one: one term, not two (the old absolute 1e-9 slack gave 2).
    close(number("sum(i, 0, 0.9999999999, 1)", &[]), 1.0, 0.0);
    close(number("sum(i, 0, 1, 1)", &[]), 2.0, 0.0);
    close(number("sum(i, 1, 1e6, 1)", &[]), 1e6, 0.0);
}

#[test]
fn one_argument_sum_is_the_vector_form_not_a_function() {
    // `sum(3)` folds the one element 3; it does not call a user function `sum`.
    close(number("sum(3)", &[]), 3.0, 0.0);
}

#[test]
fn operator_names_cannot_be_defined_as_functions() {
    for src in [
        "sum(a) := 2*a",
        "product(a) := 2*a",
        "sum(i, 1, 2, i) := 1",
        "integral(x, 0, 1, x) := 1",
        "derivative(x, x^2) := 1",
    ] {
        let err = parse(src, &DefaultIdContext).unwrap_err();
        assert!(err.message.contains("reserved"), "{src}: {}", err.message);
    }
}

#[test]
fn summation_units_must_agree() {
    let err = run(&[], "sum(i, 1, 2, i * (1 m) + i * (1 s))").unwrap_err();
    assert!(
        matches!(err, EvalError::DimensionMismatch { .. }),
        "{err:?}"
    );
}

// ----- integrals ----------------------------------------------------------------------------------

#[test]
fn sine_over_half_period() {
    close(number("integral(x, 0, π, sin(x))", &[]), 2.0, 1e-10);
}

#[test]
fn gaussian_over_the_real_line() {
    close(
        number("integral(x, -∞, ∞, exp(-x^2))", &[]),
        std::f64::consts::PI.sqrt(),
        1e-9,
    );
}

#[test]
fn half_line_integral() {
    close(number("integral(x, 0, ∞, exp(-x))", &[]), 1.0, 1e-9);
    close(number("integral(x, -∞, 0, exp(x))", &[]), 1.0, 1e-9);
}

#[test]
fn integrable_endpoint_singularity() {
    // The endpoint is never evaluated; the bisection concentrates panels there.
    close(number("integral(x, 0, 1, 1/sqrt(x))", &[]), 2.0, 1e-8);
}

#[test]
fn oscillatory_integrand() {
    // integral of sin^2 over [0, T] is T/2 - sin(2T)/4.
    let t = 100.0f64;
    close(
        number("integral(x, 0, 100, sin(x)^2)", &[]),
        t / 2.0 - (2.0 * t).sin() / 4.0,
        1e-8,
    );
}

#[test]
fn integral_units_are_integrand_times_variable() {
    // integral of 3 x over [0, 2 m] with x in m: 3 * 2^2 / 2 = 6 m^2.
    let (v, dim) = value("integral(x, 0 m, 2 m, 3 x)", &[]);
    close(v, 6.0, 1e-9);
    assert_eq!(dim, dim_of("m").checked_mul(dim_of("m")).unwrap());
}

#[test]
fn integral_limits_must_have_one_unit() {
    let err = run(&[], "integral(x, 0 m, 1 s, x)").unwrap_err();
    assert!(
        matches!(err, EvalError::DimensionMismatch { .. }),
        "{err:?}"
    );
}

#[test]
fn integral_of_zero_is_zero_and_empty_interval() {
    close(number("integral(x, 0, 1, 0)", &[]), 0.0, 0.0);
    close(number("integral(x, 1, 1, x)", &[]), 0.0, 0.0);
}

#[test]
fn iterated_integral_by_nesting() {
    // integral over [0, 1] of (integral over [0, x] of y dy) dx = 1/6.
    close(
        number("integral(x, 0, 1, integral(y, 0, x, y))", &[]),
        1.0 / 6.0,
        1e-9,
    );
}

#[test]
fn divergent_integral_is_an_error_not_a_value() {
    match run(&[], "integral(x, 0, 1, 1/x)") {
        Err(EvalError::NoConvergence(_)) => {}
        other => panic!("expected no convergence, got {other:?}"),
    }
}

// ----- derivatives --------------------------------------------------------------------------------

#[test]
fn first_derivative_of_sine_at_a_point() {
    close(
        number("derivative(x, sin(x))", &["x := 1"]),
        1f64.cos(),
        1e-9,
    );
}

#[test]
fn derivative_of_exponential() {
    close(
        number("derivative(x, exp(x))", &["x := 0.5"]),
        0.5f64.exp(),
        1e-9,
    );
}

#[test]
fn second_and_third_derivatives() {
    close(
        number("derivative(x, 2, sin(x))", &["x := 1"]),
        -1f64.sin(),
        1e-7,
    );
    // d^3/dx^3 x^5 = 60 x^2, at 2 it is 240.
    close(number("derivative(x, 3, x^5)", &["x := 2"]), 240.0, 1e-6);
}

#[test]
fn fifth_derivative_of_fifth_power() {
    close(number("derivative(x, 5, x^5)", &["x := 2"]), 120.0, 1e-5);
}

#[test]
fn zeroth_derivative_is_the_function() {
    close(number("derivative(x, 0, x^3)", &["x := 2"]), 8.0, 1e-12);
}

#[test]
fn order_above_five_is_refused() {
    assert!(run(&["x := 1"], "derivative(x, 6, sin(x))").is_err());
}

#[test]
fn derivative_units_are_function_over_variable_to_the_order() {
    // d/dx (x^2) with x in m: 2 x has unit m, value 2 * 2 = 4 at x = 2 m.
    let (v, dim) = value("derivative(x, x^2)", &["x := 2 m"]);
    close(v, 4.0, 1e-7);
    assert_eq!(dim, dim_of("m"));
}

#[test]
fn derivative_point_is_the_current_value() {
    // The derivative is taken at the worksheet value of x (not a bound variable).
    close(number("derivative(t, t^3)", &["t := 3"]), 27.0, 1e-7);
}

// ----- infinite limits, spellings and corners ------------------------------------------------------

#[test]
fn whole_line_integrals_are_the_sum_of_two_convergent_halves() {
    let want = std::f64::consts::PI.sqrt();
    close(number("integral(x, -inf, inf, exp(-x^2))", &[]), want, 1e-9);
    close(number("integral(x, -∞, ∞, exp(-x^2))", &[]), want, 1e-9);
}

#[test]
fn odd_convergent_integrand_over_the_real_line_is_zero() {
    close(number("integral(x, -∞, ∞, x*exp(-x^2))", &[]), 0.0, 1e-9);
    close(
        number("integral(x, -inf, inf, x*exp(-x^2))", &[]),
        0.0,
        1e-9,
    );
}

#[test]
fn non_convergent_half_is_refused_even_when_halves_would_cancel() {
    // sin oscillates forever on each half; the two halves cancel, but neither converges.
    for src in [
        "integral(x, -∞, ∞, sin(x))",
        "integral(x, -inf, inf, sin(x))",
    ] {
        match run(&[], src) {
            Err(EvalError::NoConvergence(_)) => {}
            other => panic!("{src}: expected no convergence, got {other:?}"),
        }
    }
}

#[test]
fn inf_is_infinity_as_the_infinity_key_is() {
    close(number("integral(x, 0, inf, exp(-x))", &[]), 1.0, 1e-9);
    close(number("integral(x, -inf, 0, exp(x))", &[]), 1.0, 1e-9);
    close(number("integral(x, 0, ∞, exp(-x))", &[]), 1.0, 1e-9);
}

#[test]
fn first_derivative_at_a_corner_is_refused() {
    // abs has no derivative at 0: the two one-sided quotients differ by 2 at every step.
    match run(&["x := 0"], "derivative(x, abs(x))") {
        Err(EvalError::NoConvergence(_)) => {}
        other => panic!("expected no convergence at the kink, got {other:?}"),
    }
    // A jump (floor at 0) is a corner too.
    match run(&["x := 0"], "derivative(x, floor(x))") {
        Err(EvalError::NoConvergence(_)) => {}
        other => panic!("expected no convergence at the jump, got {other:?}"),
    }
}

#[test]
fn smooth_points_are_not_taken_for_corners() {
    close(number("derivative(x, x^2)", &["x := 0"]), 0.0, 1e-9);
    close(number("derivative(x, x^3 + x^2)", &["x := 0"]), 0.0, 1e-9);
    close(number("derivative(x, abs(x))", &["x := 1"]), 1.0, 1e-9);
    close(number("derivative(x, abs(x))", &["x := -2"]), -1.0, 1e-9);
    close(number("derivative(x, sin(x))", &["x := 0"]), 1.0, 1e-9);
}

// ----- derivative tolerance by order ----------------------------------------------------------------

/// The nth derivative of sin at x.
fn sin_derivative(n: i32, x: f64) -> f64 {
    match n.rem_euclid(4) {
        0 => x.sin(),
        1 => x.cos(),
        2 => -x.sin(),
        _ => -x.cos(),
    }
}

#[test]
fn derivatives_of_orders_one_to_five_meet_their_tolerance() {
    // Each value is within derivative_tol(n) of its exact value, relative to its size (or 1).
    for n in 1..=5 {
        for p in [-1.5f64, 0.5, 1.0, 2.0] {
            let at = format!("x := {p}");
            let want_sin = sin_derivative(n, p);
            let got = number(&format!("derivative(x, {n}, sin(x))"), &[at.as_str()]);
            close(got, want_sin, derivative_tol(n) * want_sin.abs().max(1.0));
            let want_exp = p.exp();
            let got = number(&format!("derivative(x, {n}, exp(x))"), &[at.as_str()]);
            close(got, want_exp, derivative_tol(n) * want_exp.max(1.0));
        }
    }
}

#[test]
fn nested_derivative_stops_within_the_budget() {
    // Twenty levels of derivative: an error and no value, in well under a second.
    let mut src = "x^3".to_string();
    for _ in 0..20 {
        src = format!("derivative(x, {src})");
    }
    // Each level's estimate carries the error of the level inside it, so the nest runs out of
    // accuracy (NoConvergence) within a few levels, long before the budget matters.
    let started = std::time::Instant::now();
    let result = run(&["x := 0.5"], &src);
    assert!(
        matches!(
            result,
            Err(EvalError::NoConvergence(_)) | Err(EvalError::WorkLimit(_))
        ),
        "{result:?}"
    );
    assert!(
        started.elapsed().as_secs() < 30,
        "took {:?}",
        started.elapsed()
    );
}

// ----- bound variables and program locals ----------------------------------------------------------

/// Runs a programming region built as syntax (programs have no text form yet) and returns its
/// real scalar value.
fn run_program(stmts: Vec<Stmt>) -> f64 {
    match Session::new().run(&Expr::Program(stmts)) {
        Outcome::Value(Value::Scalar(s)) => s.value.re,
        other => panic!("expected a scalar, got {other:?}"),
    }
}

fn local_named(name: &str, value: f64) -> Stmt {
    Stmt::Local {
        target: Expr::var(name),
        expr: Expr::num(value),
    }
}

#[test]
fn program_local_does_not_override_the_bound_index() {
    // A program local i := 100 must not change the sum's i: it runs over 1, 2, 3.
    let sum = Expr::Accumulate {
        op: Accumulation::Sum,
        var: Box::new(Id::new("i", IdKind::Variable)),
        from: Box::new(Expr::num(1.0)),
        to: Box::new(Expr::num(3.0)),
        body: Box::new(Expr::var("i")),
    };
    close(
        run_program(vec![local_named("i", 100.0), Stmt::Line(sum)]),
        6.0,
        1e-12,
    );
}

#[test]
fn program_local_does_not_override_the_integration_variable() {
    let integral = Expr::Integral {
        var: Box::new(Id::new("x", IdKind::Variable)),
        lower: Box::new(Expr::num(0.0)),
        upper: Box::new(Expr::num(1.0)),
        body: Box::new(Expr::var("x")),
    };
    close(
        run_program(vec![local_named("x", 5.0), Stmt::Line(integral)]),
        0.5,
        1e-9,
    );
}

// ----- text form ----------------------------------------------------------------------------------

#[test]
fn text_forms_round_trip_through_the_parser() {
    let sources = [
        "sum(i, 1, n, 1/i^2)",
        "product(i, 1, 5, i)",
        "sum([1, 2, 3])",
        "integral(x, 0, π, sin(x))",
        "derivative(x, sin(x))",
        "derivative(x, 3, x^5)",
    ];
    for src in sources {
        let e = parse(src, &DefaultIdContext).expect(src);
        let printed = e.to_string();
        let again = parse(&printed, &DefaultIdContext).expect(&printed);
        assert_eq!(again, e, "{src} printed as {printed}");
    }
}

#[test]
fn wrong_arity_is_a_parse_error() {
    assert!(parse("sum(i, 1, 2)", &DefaultIdContext).is_err());
    assert!(parse("integral(x, 0, 1)", &DefaultIdContext).is_err());
    assert!(parse("derivative(x)", &DefaultIdContext).is_err());
}

#[test]
fn hostile_nested_integral_stops_with_an_error() {
    // The inner integral diverges for every x, so the first inner evaluation must fail (no value).
    let started = std::time::Instant::now();
    let result = run(&[], "integral(x, 0, 1, integral(y, 0, 1, 1/(x*y)))");
    assert!(
        matches!(
            result,
            Err(EvalError::NoConvergence(_)) | Err(EvalError::WorkLimit(_))
        ),
        "{result:?}"
    );
    assert!(
        started.elapsed().as_secs() < 30,
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn printed_forms_are_the_text_forms() {
    let e: Expr = parse("integral(x, 0, π, sin(x))", &Units).unwrap();
    assert_eq!(e.to_string(), "integral(x, 0, π, sin(x))");
}

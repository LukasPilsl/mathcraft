//! Tests for the statistics tier: descriptive statistics (location, spread, shape, association).
//! Expected values come from the Mathcad Prime help examples (worked numbers quoted in the
//! pages), from hand computation, and from CPython `math` / closed forms where noted.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::expr::Expr;
use crate::units::Dimension;

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

fn value(e: &Expr) -> Value {
    evaluate(e, &Env::new()).unwrap_or_else(|err| panic!("failed: {err}"))
}

fn err(e: &Expr) -> EvalError {
    evaluate(e, &Env::new()).expect_err("expected an error")
}

/// The real dimensionless result of `e`.
fn real(e: &Expr) -> f64 {
    real_in(&value(e), dimless())
}

fn dim_of(e: &Expr) -> Dimension {
    scalar(&value(e)).dim
}

/// A column vector of the given values.
fn col(values: &[f64]) -> Expr {
    mat(values.len(), 1, values.iter().map(|&x| n(x)).collect())
}

/// The help's voltage data set (example "Covariance and Correlation Coefficient").
const V1: [f64; 10] = [
    0.1, -0.86, -1.075, -1.16, -1.53, -2.72, -2.64, -2.91, -3.58, -3.7,
];
const V2: [f64; 10] = [0.4, 0.72, 1.1, 1.45, 1.8, 2.17, 2.4, 2.8, 3.2, 3.4];

/// The data of the help's percentile example: `data_2` is 46 * 1.2 = 55.2 by then.
fn help_data() -> Expr {
    col(&[79.0, 85.0, 55.2, 86.0, 81.0])
}

/// `0, 1, ..., 10` as a column (the help's percentile example X).
fn ramp() -> Expr {
    col(&(0..=10).map(f64::from).collect::<Vec<_>>())
}

// ---- location ----

#[test]
fn mean_of_help_data_and_of_its_matrix_form() {
    close(
        real(&call(
            "mean",
            vec![n(79.0), n(85.0), n(46.0), n(86.0), n(81.0)],
        )),
        75.4,
        1e-12,
    );
    close(real(&call("mean", vec![help_data()])), 77.24, 1e-12);
}

#[test]
fn mean_keeps_the_unit() {
    let e = call("mean", vec![scale(1.0, u("m")), scale(3.0, u("m"))]);
    real_in(&value(&e), unit_dim("m"));
    close(scalar(&value(&e)).value.re, 2.0, 1e-15);
}

#[test]
fn mean_of_mixed_dimensions_is_an_error() {
    let e = call("mean", vec![scale(1.0, u("m")), scale(3.0, u("s"))]);
    assert!(matches!(err(&e), EvalError::DimensionMismatch { .. }));
}

#[test]
fn mean_of_one_value_is_that_value() {
    close(real(&call("mean", vec![n(-5.0)])), -5.0, 0.0);
}

#[test]
fn gmean_and_hmean_of_small_sets() {
    close(
        real(&call("gmean", vec![n(1.0), n(4.0), n(16.0)])),
        4.0,
        1e-12,
    );
    close(
        real(&call("hmean", vec![n(1.0), n(2.0), n(4.0)])),
        12.0 / 7.0,
        1e-15,
    );
    // hmean <= gmean <= mean (help: "Additional Information").
    let xs = [n(1.0), n(2.0), n(4.0)];
    let h = real(&call("hmean", xs.to_vec()));
    let g = real(&call("gmean", xs.to_vec()));
    let a = real(&call("mean", xs.to_vec()));
    assert!(h < g && g < a);
}

#[test]
fn gmean_and_hmean_keep_the_unit() {
    let e = call("hmean", vec![scale(2.0, u("m")), scale(2.0, u("m"))]);
    close(scalar(&value(&e)).value.re, 2.0, 1e-15);
    real_in(&value(&e), unit_dim("m"));
}

#[test]
fn gmean_and_hmean_refuse_zero_and_negative_values() {
    assert!(matches!(
        err(&call("gmean", vec![n(0.0), n(2.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("hmean", vec![n(-1.0), n(2.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn median_of_help_data_and_even_count() {
    close(real(&call("median", vec![help_data()])), 81.0, 0.0);
    close(
        real(&call("median", vec![n(6.0), n(2.0), n(27.0), n(7.0)])),
        6.5,
        0.0,
    );
}

#[test]
fn median_keeps_the_unit_and_refuses_complex_data() {
    let e = call(
        "median",
        vec![scale(3.0, u("m")), scale(1.0, u("m")), scale(2.0, u("m"))],
    );
    close(scalar(&value(&e)).value.re, 2.0, 0.0);
    real_in(&value(&e), unit_dim("m"));
    assert!(matches!(
        err(&call("median", vec![v("i"), n(1.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn mode_finds_the_repeated_value() {
    close(
        real(&call("mode", vec![n(1.0), n(2.0), n(2.0), n(3.0)])),
        2.0,
        0.0,
    );
    close(real(&call("mode", vec![n(5.0), n(5.0), n(5.0)])), 5.0, 0.0);
}

#[test]
fn mode_is_an_error_without_a_single_repeated_value() {
    // Help example: no repeated values in the data set.
    assert!(matches!(
        err(&call("mode", vec![help_data()])),
        EvalError::Unsupported(_)
    ));
    // Two values with the same frequency tie: the help returns an error.
    assert!(matches!(
        err(&call("mode", vec![n(1.0), n(1.0), n(2.0), n(2.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("mode", vec![n(7.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn percentile_reproduces_the_help_examples() {
    close(real(&call("percentile", vec![ramp(), n(0.5)])), 5.0, 1e-12);
    close(real(&call("percentile", vec![ramp(), n(0.9)])), 9.8, 1e-12);
    close(
        real(&call("percentile", vec![help_data(), n(0.25)])),
        67.1,
        1e-12,
    );
    close(
        real(&call("percentile", vec![help_data(), n(0.75)])),
        85.5,
        1e-12,
    );
    close(
        real(&call("percentile", vec![help_data(), n(0.5)])),
        81.0,
        1e-12,
    );
}

#[test]
fn percentile_accepts_the_closed_interval_and_clamps_the_position() {
    // p = 0 is the minimum and p = 1 the maximum, for any data set size.
    close(real(&call("percentile", vec![ramp(), n(0.0)])), 0.0, 0.0);
    close(real(&call("percentile", vec![ramp(), n(1.0)])), 10.0, 0.0);
    // Positions below 0 and above n-1 clamp to the ends (p < 1/(n+1), p > n/(n+1)).
    close(real(&call("percentile", vec![ramp(), n(0.05)])), 0.0, 0.0);
    close(real(&call("percentile", vec![ramp(), n(0.95)])), 10.0, 0.0);
    close(
        real(&call("percentile", vec![help_data(), n(0.0)])),
        55.2,
        0.0,
    );
    close(
        real(&call("percentile", vec![help_data(), n(1.0)])),
        86.0,
        0.0,
    );
}

#[test]
fn percentile_edge_probabilities_on_one_two_and_n_elements() {
    // One element: every p gives that element.
    close(real(&call("percentile", vec![n(7.0), n(0.0)])), 7.0, 0.0);
    close(real(&call("percentile", vec![n(7.0), n(0.5)])), 7.0, 0.0);
    close(real(&call("percentile", vec![n(7.0), n(1.0)])), 7.0, 0.0);
    // Two elements 2 and 6: 1/(n+1) = 1/3 and n/(n+1) = 2/3 are the interpolation ends.
    let two = || col(&[2.0, 6.0]);
    close(
        real(&call("percentile", vec![two(), n(1.0 / 3.0)])),
        2.0,
        1e-12,
    );
    close(real(&call("percentile", vec![two(), n(0.5)])), 4.0, 1e-12);
    close(
        real(&call("percentile", vec![two(), n(2.0 / 3.0)])),
        6.0,
        1e-12,
    );
    close(real(&call("percentile", vec![two(), n(0.0)])), 2.0, 0.0);
    close(real(&call("percentile", vec![two(), n(1.0)])), 6.0, 0.0);
    // Eleven elements 0..10: the same edge rules at 1/(n+1) and n/(n+1).
    close(
        real(&call("percentile", vec![ramp(), n(1.0 / 12.0)])),
        0.0,
        1e-12,
    );
    close(
        real(&call("percentile", vec![ramp(), n(11.0 / 12.0)])),
        10.0,
        1e-12,
    );
}

#[test]
fn percentile_refuses_probabilities_outside_the_unit_interval() {
    assert!(matches!(
        err(&call("percentile", vec![ramp(), n(-0.1)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("percentile", vec![ramp(), n(1.5)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn percentile_keeps_the_unit() {
    let e = call(
        "percentile",
        vec![
            mat(2, 1, vec![scale(1.0, u("m")), scale(3.0, u("m"))]),
            n(0.5),
        ],
    );
    close(scalar(&value(&e)).value.re, 2.0, 1e-15);
    real_in(&value(&e), unit_dim("m"));
}

// ---- dispersion ----

#[test]
fn population_and_sample_variance_of_one_to_four() {
    // Mean 2.5, squared deviations 5: population 5/4, sample 5/3.
    close(
        real(&call("var", vec![n(1.0), n(2.0), n(3.0), n(4.0)])),
        1.25,
        1e-15,
    );
    close(
        real(&call("Var", vec![n(1.0), n(2.0), n(3.0), n(4.0)])),
        5.0 / 3.0,
        1e-15,
    );
    close(
        real(&call("stdev", vec![n(1.0), n(2.0), n(3.0), n(4.0)])),
        1.25_f64.sqrt(),
        1e-15,
    );
    close(
        real(&call("Stdev", vec![n(1.0), n(2.0), n(3.0), n(4.0)])),
        (5.0_f64 / 3.0).sqrt(),
        1e-15,
    );
}

#[test]
fn variance_of_a_matrix_argument_matches_its_elements() {
    let m = mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    close(real(&call("var", vec![m])), 1.25, 1e-15);
}

#[test]
fn variance_of_several_arguments_pools_their_elements() {
    // Pooled data 1, 2, 3, 4 across a scalar and a vector.
    close(
        real(&call("var", vec![n(1.0), col(&[2.0, 3.0, 4.0])])),
        1.25,
        1e-15,
    );
}

#[test]
fn variance_squares_the_unit_and_stdev_keeps_it() {
    let m = dim_of(&scale(1.0, u("m")));
    let m2 = dim_of(&pow(scale(1.0, u("m")), n(2.0)));
    let v_expr = call("var", vec![scale(1.0, u("m")), scale(3.0, u("m"))]);
    assert_eq!(dim_of(&v_expr), m2);
    close(scalar(&value(&v_expr)).value.re, 1.0, 1e-15);
    let s_expr = call("stdev", vec![scale(1.0, u("m")), scale(3.0, u("m"))]);
    assert_eq!(dim_of(&s_expr), m);
    close(scalar(&value(&s_expr)).value.re, 1.0, 1e-15);
}

#[test]
fn variance_of_one_value_and_of_equal_values() {
    close(real(&call("var", vec![n(5.0)])), 0.0, 0.0);
    close(real(&call("var", vec![n(2.0), n(2.0), n(2.0)])), 0.0, 0.0);
    close(real(&call("stdev", vec![n(2.0), n(2.0), n(2.0)])), 0.0, 0.0);
}

#[test]
fn variance_with_negative_numbers() {
    close(real(&call("var", vec![n(-1.0), n(1.0)])), 1.0, 1e-15);
    close(real(&call("Var", vec![n(-1.0), n(1.0)])), 2.0, 1e-15);
}

#[test]
fn sample_variance_of_one_value_is_an_error() {
    assert!(matches!(
        err(&call("Var", vec![n(3.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("Stdev", vec![n(3.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn skew_and_kurt_of_a_reference_sample() {
    // Reference: closed forms in the help's kurtosis/skewness page, computed in CPython.
    let data = [1.0, 2.0, 4.0, 8.0, 16.0, 3.0];
    let ns: Vec<Expr> = data.iter().map(|&x| n(x)).collect();
    close(
        real(&call("skew", ns.clone())),
        1.600_262_011_136_484_7,
        1e-12,
    );
    close(real(&call("kurt", ns)), 2.335_670_425_165_181_6, 1e-12);
}

#[test]
fn skew_and_kurt_of_one_to_five() {
    let ns: Vec<Expr> = (1..=5).map(|x| n(f64::from(x))).collect();
    close(real(&call("skew", ns.clone())), 0.0, 1e-12);
    // Hand value: z^4 sums to 34/6.25 = 5.44; 1.25 * 5.44 - 8 = -1.2.
    close(real(&call("kurt", ns)), -1.2, 1e-12);
}

#[test]
fn skew_is_dimensionless_and_unit_free() {
    let ms: Vec<Expr> = [1.0, 2.0, 4.0, 8.0, 16.0, 3.0]
        .iter()
        .map(|&x| scale(x, u("m")))
        .collect();
    let e = call("skew", ms);
    assert!(scalar(&value(&e)).dim.is_dimensionless());
    close(real(&e), 1.600_262_011_136_484_7, 1e-12);
}

#[test]
fn skew_and_kurt_size_and_equal_value_errors() {
    assert!(matches!(
        err(&call("skew", vec![n(1.0), n(2.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("kurt", vec![n(1.0), n(2.0), n(3.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("skew", vec![n(2.0), n(2.0), n(2.0)])),
        EvalError::DivisionByZero
    ));
}

// ---- association ----

#[test]
fn covariance_and_correlation_of_the_help_voltages() {
    // Help: cvar(V_1, V_2) = -1.159; corr(V_1, V_2) = -0.981; cvar / var(V_1) = -0.792.
    let v1 = col(&V1);
    let v2 = col(&V2);
    close(
        real(&call("cvar", vec![v1.clone(), v2.clone()])),
        -1.15943,
        1e-12,
    );
    close(
        real(&call("corr", vec![v1.clone(), v2.clone()])),
        -0.980_848_211_350_697_4,
        1e-12,
    );
    let slope = real(&call("cvar", vec![v1.clone(), v2.clone()])) / real(&call("var", vec![v1]));
    close(slope, -0.792_303_362_196_579_2, 1e-12);
}

#[test]
fn covariance_of_a_variable_with_itself_is_its_variance() {
    let x = col(&[1.0, 2.0, 4.0]);
    close(
        real(&call("cvar", vec![x.clone(), x.clone()])),
        real(&call("var", vec![x.clone()])),
        1e-15,
    );
    close(real(&call("corr", vec![x.clone(), x])), 1.0, 1e-14);
}

#[test]
fn correlation_of_a_variable_with_its_negative_is_minus_one() {
    let x = col(&[1.0, 2.0, 4.0]);
    let neg = col(&[-1.0, -2.0, -4.0]);
    close(real(&call("corr", vec![x, neg])), -1.0, 1e-14);
}

#[test]
fn covariance_keeps_the_product_of_units_and_corr_is_dimensionless() {
    let a = mat(2, 1, vec![scale(1.0, u("m")), scale(3.0, u("m"))]);
    let b = mat(2, 1, vec![scale(2.0, u("s")), scale(6.0, u("s"))]);
    let cov = call("cvar", vec![a.clone(), b.clone()]);
    // Means 2 m and 4 s; deviations (-1, 1) m and (-2, 2) s; sum 4 over N = 2.
    close(scalar(&value(&cov)).value.re, 2.0, 1e-15);
    let ms = pow(scale(1.0, u("m")), n(1.0));
    assert_eq!(dim_of(&cov), dim_of(&mul(ms, scale(1.0, u("s")))));
    close(real(&call("corr", vec![a, b])), 1.0, 1e-15);
}

#[test]
fn correlation_of_a_constant_set_is_an_error() {
    assert!(matches!(
        err(&call(
            "corr",
            vec![col(&[1.0, 1.0, 1.0]), col(&[1.0, 2.0, 3.0])]
        )),
        EvalError::DivisionByZero
    ));
}

#[test]
fn covariance_needs_two_arrays_of_one_size() {
    assert!(matches!(
        err(&call("cvar", vec![n(1.0), n(2.0), n(3.0)])),
        EvalError::ArgCount { .. }
    ));
    assert!(matches!(
        err(&call("cvar", vec![col(&[1.0, 2.0]), col(&[1.0, 2.0, 3.0])])),
        EvalError::ShapeMismatch { .. }
    ));
}

// ---- non-finite and empty input ----

#[test]
fn infinite_data_is_refused_not_propagated() {
    let e = call("mean", vec![n(f64::INFINITY), n(1.0)]);
    assert!(evaluate(&e, &Env::new()).is_err());
}

#[test]
fn mean_of_finite_data_whose_sum_overflows_is_finite() {
    // The plain sum is inf; the scaled sum gives the finite mean.
    close(real(&call("mean", vec![n(1e308), n(1e308)])), 1e308, 1e-15);
    close(
        real(&call("mean", vec![n(1.5e308), n(1.5e308), n(-1e308)])),
        6.666_666_666_666_667e307,
        1e-15,
    );
    close(real(&call("mean", vec![n(1e308), n(-1e308)])), 0.0, 0.0);
}

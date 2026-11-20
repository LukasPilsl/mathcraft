//! Tests for the statistics tier: distribution functions (normal, uniform, exponential, binomial,
//! Poisson). Expected values come from closed forms, CPython `math`, and exact integer sums.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Scalar, Value};
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

// ---- normal distribution ----

#[test]
fn dnorm_at_zero_and_one() {
    close(
        real(&call("dnorm", vec![n(0.0), n(0.0), n(1.0)])),
        0.398_942_280_401_432_7,
        1e-15,
    );
    close(
        real(&call("dnorm", vec![n(1.0), n(0.0), n(1.0)])),
        0.241_970_724_519_143_37,
        1e-15,
    );
}

#[test]
fn dnorm_scales_with_sigma() {
    // Density of N(10, 2) at 10 is half the standard density at 0.
    close(
        real(&call("dnorm", vec![n(10.0), n(10.0), n(2.0)])),
        0.398_942_280_401_432_7 / 2.0,
        1e-15,
    );
}

#[test]
fn pnorm_reference_values() {
    close(
        real(&call("pnorm", vec![n(0.0), n(0.0), n(1.0)])),
        0.5,
        1e-16,
    );
    close(
        real(&call("pnorm", vec![n(1.96), n(0.0), n(1.0)])),
        0.975_002_104_851_779_5,
        1e-15,
    );
    close(
        real(&call("pnorm", vec![n(-1.96), n(0.0), n(1.0)])),
        0.024_997_895_148_220_43,
        1e-14,
    );
    // N(10, 2) at 12 is the standard value at 1.
    close(
        real(&call("pnorm", vec![n(12.0), n(10.0), n(2.0)])),
        0.841_344_746_068_542_9,
        1e-15,
    );
}

#[test]
fn pnorm_is_monotone_and_its_lower_tail_is_positive() {
    let mut last = -1.0;
    let mut z = -8.0;
    while z <= 8.0 {
        let p = real(&call("pnorm", vec![n(z), n(0.0), n(1.0)]));
        assert!(p >= last, "not monotone at {z}");
        last = p;
        z += 0.5;
    }
    let tail = real(&call("pnorm", vec![n(-37.0), n(0.0), n(1.0)]));
    assert!(tail > 0.0 && tail < 1e-299, "{tail:e}");
}

#[test]
fn qnorm_reference_values_and_tails() {
    close(
        real(&call("qnorm", vec![n(0.975), n(0.0), n(1.0)])),
        1.959_963_984_540_054,
        1e-14,
    );
    close(real(&call("qnorm", vec![n(0.5), n(0.0), n(1.0)])), 0.0, 0.0);
    let far = real(&call("qnorm", vec![n(1e-300), n(0.0), n(1.0)]));
    assert!(far.is_finite() && far < -36.0, "{far}");
}

#[test]
fn qnorm_inverts_pnorm_on_a_grid() {
    let mut z = -8.0;
    while z <= 3.0 {
        let p = real(&call("pnorm", vec![n(z), n(0.0), n(1.0)]));
        let back = real(&call("qnorm", vec![n(p), n(0.0), n(1.0)]));
        assert!((back - z).abs() <= 1e-12, "z {z}: back {back}");
        z += 0.5;
    }
}

#[test]
fn qnorm_with_location_and_scale() {
    close(
        real(&call(
            "qnorm",
            vec![n(0.841_344_746_068_542_9), n(10.0), n(2.0)],
        )),
        12.0,
        1e-12,
    );
}

#[test]
fn normal_functions_are_element_wise_over_a_matrix() {
    let e = call(
        "pnorm",
        vec![mat(2, 1, vec![n(0.0), n(1.0)]), n(0.0), n(1.0)],
    );
    let m = match value(&e) {
        Value::Matrix(m) => m,
        other => panic!("expected a matrix, got {other:?}"),
    };
    close(m.data()[0].value.re, 0.5, 1e-16);
    close(m.data()[1].value.re, 0.841_344_746_068_542_9, 1e-15);
}

#[test]
fn normal_functions_refuse_units_and_bad_arguments() {
    assert!(matches!(
        err(&call("pnorm", vec![scale(1.0, u("m")), n(0.0), n(1.0)])),
        EvalError::DimensionNotDimensionless { .. }
    ));
    assert!(matches!(
        err(&call("dnorm", vec![n(0.0), n(0.0), n(0.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("qnorm", vec![n(0.0), n(0.0), n(1.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("qnorm", vec![n(1.5), n(0.0), n(1.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn cnorm_is_the_standard_normal_cdf() {
    close(
        real(&call("cnorm", vec![n(1.0)])),
        0.841_344_746_068_542_9,
        1e-15,
    );
}

// ---- uniform and exponential ----

#[test]
fn uniform_density_cdf_and_inverse() {
    close(real(&call("dunif", vec![n(0.5), n(0.0), n(1.0)])), 1.0, 0.0);
    close(real(&call("dunif", vec![n(2.0), n(0.0), n(1.0)])), 0.0, 0.0);
    close(
        real(&call("punif", vec![n(0.25), n(0.0), n(1.0)])),
        0.25,
        0.0,
    );
    close(
        real(&call("punif", vec![n(-1.0), n(0.0), n(1.0)])),
        0.0,
        0.0,
    );
    close(real(&call("punif", vec![n(2.0), n(0.0), n(1.0)])), 1.0, 0.0);
    close(
        real(&call("qunif", vec![n(0.25), n(0.0), n(4.0)])),
        1.0,
        1e-15,
    );
}

#[test]
fn uniform_refuses_an_empty_interval_and_bad_probability() {
    assert!(matches!(
        err(&call("dunif", vec![n(0.0), n(1.0), n(1.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("qunif", vec![n(1.5), n(0.0), n(1.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn exponential_density_and_cdf_and_inverse() {
    close(real(&call("dexp", vec![n(0.0), n(2.0)])), 2.0, 0.0);
    close(real(&call("dexp", vec![n(-1.0), n(2.0)])), 0.0, 0.0);
    close(
        real(&call("pexp", vec![n(1.0), n(1.0)])),
        0.632_120_558_828_557_7,
        1e-15,
    );
    close(
        real(&call("qexp", vec![n(0.5), n(1.0)])),
        std::f64::consts::LN_2,
        1e-15,
    );
    let p = real(&call("pexp", vec![n(2.5), n(0.7)]));
    close(real(&call("qexp", vec![n(p), n(0.7)])), 2.5, 1e-12);
}

#[test]
fn exponential_refuses_a_zero_rate_and_the_infinite_quantile() {
    assert!(matches!(
        err(&call("dexp", vec![n(1.0), n(0.0)])),
        EvalError::Unsupported(_)
    ));
    assert_eq!(
        err(&call("qexp", vec![n(1.0), n(1.0)])),
        EvalError::NotFinite
    );
}

// ---- binomial and Poisson ----

#[test]
fn binomial_density_and_cdf() {
    close(
        real(&call("dbinom", vec![n(3.0), n(10.0), n(0.5)])),
        0.117_187_5,
        1e-13,
    );
    close(
        real(&call("pbinom", vec![n(3.0), n(10.0), n(0.3)])),
        0.649_610_718_399_999_6,
        1e-13,
    );
    close(
        real(&call("pbinom", vec![n(10.0), n(10.0), n(0.3)])),
        1.0,
        0.0,
    );
}

#[test]
fn binomial_outside_the_support_and_degenerate_q() {
    close(
        real(&call("dbinom", vec![n(11.0), n(10.0), n(0.5)])),
        0.0,
        0.0,
    );
    close(
        real(&call("pbinom", vec![n(-1.0), n(10.0), n(0.5)])),
        0.0,
        0.0,
    );
    close(
        real(&call("dbinom", vec![n(0.0), n(10.0), n(0.0)])),
        1.0,
        0.0,
    );
}

#[test]
fn binomial_quantile_is_the_smallest_count_reaching_p() {
    close(
        real(&call("qbinom", vec![n(0.5), n(10.0), n(0.5)])),
        5.0,
        0.0,
    );
    close(
        real(&call("qbinom", vec![n(0.0), n(10.0), n(0.5)])),
        0.0,
        0.0,
    );
    close(
        real(&call("qbinom", vec![n(1.0), n(10.0), n(0.5)])),
        10.0,
        0.0,
    );
}

#[test]
fn binomial_refuses_non_integer_counts_and_bad_q() {
    assert!(matches!(
        err(&call("dbinom", vec![n(1.5), n(10.0), n(0.5)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("pbinom", vec![n(1.0), n(10.0), n(1.5)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("dbinom", vec![n(1.0), n(1e8), n(0.5)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn poisson_density_and_cdf() {
    close(
        real(&call("dpois", vec![n(2.0), n(1.0)])),
        0.183_939_720_585_721_17,
        1e-15,
    );
    close(
        real(&call("ppois", vec![n(2.0), n(1.0)])),
        0.919_698_602_928_605_8,
        1e-14,
    );
    close(real(&call("dpois", vec![n(-1.0), n(1.0)])), 0.0, 0.0);
    close(real(&call("ppois", vec![n(-1.0), n(1.0)])), 0.0, 0.0);
}

#[test]
fn poisson_quantile_and_limits() {
    close(real(&call("qpois", vec![n(0.5), n(3.0)])), 3.0, 0.0);
    // The tail is far below the smallest double, so the cumulative value is zero, not an error.
    close(real(&call("ppois", vec![n(5.0), n(800.0)])), 0.0, 0.0);
    assert!(matches!(
        err(&call("ppois", vec![n(5.0), n(1e13)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("ppois", vec![n(1e8), n(1.0)])),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err(&call("dpois", vec![n(1.0), n(0.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn large_binomial_and_poisson_arguments_are_accurate() {
    // Exact references are in the kernel tests; here the function layer must agree with them.
    close(
        real(&call("dbinom", vec![n(50000.0), n(100000.0), n(0.5)])),
        2.523_126_214_196_74e-3,
        1e-13,
    );
    close(
        real(&call("pbinom", vec![n(50000.0), n(100000.0), n(0.5)])),
        0.501_261_563_107_098_4,
        1e-12,
    );
    close(
        real(&call("ppois", vec![n(1e6), n(1e6)])),
        0.500_265_961_486_283_7,
        1e-12,
    );
    close(
        real(&call("dpois", vec![n(1e6), n(1e6)])),
        3.989_422_471_562_44e-4,
        1e-13,
    );
}

#[test]
fn large_binomial_and_poisson_quantiles_run_without_refusal() {
    // qbinom(0.5, 1000, 0.9) and qpois(0.5, 1e6) were refused before the log-domain rewrite.
    close(
        real(&call("qbinom", vec![n(0.5), n(1000.0), n(0.9)])),
        900.0,
        0.0,
    );
    close(real(&call("qpois", vec![n(0.5), n(1e6)])), 1e6, 1.0);
}

// ---- stubs ----

// ---- continuous and discrete families (distributions_cont.rs, distributions_disc.rs) ----

/// Calls `name` with real arguments and returns the real result.
fn f(name: &str, args: &[f64]) -> f64 {
    real(&call(name, args.iter().map(|&x| n(x)).collect()))
}

/// Checks `q(p) = x` and `p(x) = p` for each probability (the family's own cumulative function).
fn round_trip(q: &str, p_fn: &str, params: &[f64], probs: &[f64], tol: f64) {
    for &p in probs {
        let mut args = vec![p];
        args.extend_from_slice(params);
        let x = f(q, &args);
        let mut back = vec![x];
        back.extend_from_slice(params);
        let got = f(p_fn, &back);
        assert!(
            (got - p).abs() <= tol,
            "{p_fn}({q}({p})) = {got}, want {p} (x = {x})"
        );
    }
}

#[test]
fn gamma_and_chisq_closed_forms() {
    close(f("dgamma", &[1.5, 1.0]), (-1.5f64).exp(), 1e-14);
    close(f("dgamma", &[0.0, 1.0]), 1.0, 1e-14);
    close(f("pgamma", &[3.0, 2.0]), 1.0 - 4.0 * (-3.0f64).exp(), 1e-14);
    close(f("qgamma", &[0.3, 1.0]), -(0.7f64).ln(), 1e-12);
    close(f("dchisq", &[1.0, 2.0]), 0.5 * (-0.5f64).exp(), 1e-14);
    close(f("pchisq", &[3.0, 2.0]), 1.0 - (-1.5f64).exp(), 1e-14);
    close(f("qchisq", &[0.5, 2.0]), 2.0 * 2f64.ln(), 1e-12);
    // Help values (4 significant figures in the examples).
    close(f("dchisq", &[5.5, 11.0]), 0.058, 5e-4);
    close(f("pchisq", &[5.6, 7.0]), 0.413, 5e-4);
    assert_eq!(f("dgamma", &[-1.0, 2.0]), 0.0);
    assert_eq!(f("pgamma", &[0.0, 2.0]), 0.0);
}

#[test]
fn beta_closed_forms_and_help_values() {
    // Beta(3, 2) has density 12 x^2 (1 - x) and cdf x^3 (4 - 3x).
    close(f("dbeta", &[0.8, 3.0, 2.0]), 1.536, 1e-12);
    close(f("pbeta", &[0.8, 3.0, 2.0]), 0.8192, 1e-13);
    close(f("qbeta", &[0.25, 2.0, 1.0]), 0.5, 1e-13);
    close(f("pbeta", &[0.3, 2.0, 1.0]), 0.09, 1e-14);
    close(f("qbeta", &[0.8, 3.0, 2.0]), 0.788, 5e-4);
    assert_eq!(f("dbeta", &[1.5, 3.0, 2.0]), 0.0);
    assert_eq!(f("pbeta", &[2.0, 3.0, 2.0]), 1.0);
    round_trip(
        "qbeta",
        "pbeta",
        &[0.5, 4.0],
        &[0.001, 0.2, 0.5, 0.9, 0.999],
        1e-12,
    );
}

#[test]
fn t_closed_forms_and_help_values() {
    // d = 1 is the Cauchy distribution; d = 2 has cdf 1/2 + x / (2 sqrt(2 + x^2)).
    close(
        f("dt", &[1.0, 1.0]),
        1.0 / (2.0 * std::f64::consts::PI),
        1e-14,
    );
    close(
        f("pt", &[2.0, 1.0]),
        0.5 + 2f64.atan() / std::f64::consts::PI,
        1e-13,
    );
    close(f("pt", &[1.0, 2.0]), 0.5 + 1.0 / (2.0 * 3f64.sqrt()), 1e-13);
    close(f("qt", &[0.9, 1.0]), 3.077_683_537_175_253, 1e-10);
    close(f("qt", &[0.5, 4.0]), 0.0, 0.0);
    close(f("dt", &[-1.56, 4.0]), 0.114, 5e-4);
    close(f("qt", &[0.99, 6.0]), 3.143, 5e-4);
    round_trip("qt", "pt", &[4.0], &[0.001, 0.1, 0.5, 0.9, 0.999], 1e-10);
    round_trip("qt", "pt", &[0.7], &[0.01, 0.3, 0.8], 1e-10);
}

#[test]
fn f_closed_forms_and_help_values() {
    // F(2, 2) has cdf x / (1 + x) and density 1 / (1 + x)^2.
    close(f("pF", &[3.0, 2.0, 2.0]), 0.75, 1e-13);
    close(f("dF", &[1.0, 2.0, 2.0]), 0.25, 1e-13);
    close(f("qF", &[0.75, 2.0, 2.0]), 3.0, 1e-11);
    close(f("pF", &[0.75, 5.0, 7.0]), 0.389, 5e-4);
    close(f("qF", &[0.95, 9.0, 8.0]), 3.388, 5e-4);
    close(f("qF", &[0.65, 4.0, 6.0]), 1.36, 5e-3);
    round_trip("qF", "pF", &[5.0, 7.0], &[0.01, 0.4, 0.5, 0.6, 0.99], 1e-11);
    round_trip("qF", "pF", &[9.0, 8.0], &[0.05, 0.95], 1e-11);
}

#[test]
fn cauchy_weibull_logistic_lognormal_closed_forms() {
    let pi = std::f64::consts::PI;
    close(f("dcauchy", &[0.0, 0.0, 1.0]), 1.0 / pi, 1e-15);
    close(f("pcauchy", &[1.0, 0.0, 1.0]), 0.75, 1e-15);
    close(f("qcauchy", &[0.75, 0.0, 1.0]), 1.0, 1e-14);
    close(
        f("qcauchy", &[0.1, 0.0, 1.0]),
        -3.077_683_537_175_253,
        1e-12,
    );
    close(
        f("qcauchy", &[0.9, 2.0, 3.0]),
        2.0 + 3.0 * 3.077_683_537_175_253,
        1e-12,
    );
    close(f("dweibull", &[1.5, 1.0]), (-1.5f64).exp(), 1e-14);
    close(f("pweibull", &[1.5, 1.0]), 1.0 - (-1.5f64).exp(), 1e-14);
    close(f("qweibull", &[0.3, 1.0]), -(0.7f64).ln(), 1e-13);
    close(f("dlogis", &[0.0, 0.0, 1.0]), 0.25, 1e-15);
    close(f("plogis", &[0.0, 0.0, 1.0]), 0.5, 1e-15);
    close(
        f("plogis", &[-2.0, 0.0, 1.0]),
        1.0 / (1.0 + 2f64.exp()),
        1e-14,
    );
    close(f("qlogis", &[0.75, 0.0, 1.0]), 3f64.ln(), 1e-14);
    close(
        f("dlnorm", &[1.0, 0.0, 1.0]),
        1.0 / (2.0 * pi).sqrt(),
        1e-15,
    );
    close(f("plnorm", &[1.0, 0.0, 1.0]), 0.5, 1e-15);
    close(f("qlnorm", &[0.5, 0.7, 2.0]), 0.7f64.exp(), 1e-13);
    close(
        f("qlnorm", &[0.975, 0.0, 1.0]),
        1.959_963_984_540_054f64.exp(),
        1e-10,
    );
    assert_eq!(f("dlnorm", &[0.0, 0.0, 1.0]), 0.0);
    round_trip("qweibull", "pweibull", &[2.5], &[0.01, 0.5, 0.99], 1e-12);
    round_trip("qlogis", "plogis", &[1.0, 2.0], &[0.01, 0.5, 0.99], 1e-12);
    round_trip("qlnorm", "plnorm", &[0.0, 0.5], &[0.01, 0.5, 0.99], 1e-12);
    round_trip("qcauchy", "pcauchy", &[1.0, 2.0], &[0.01, 0.5, 0.99], 1e-12);
}

#[test]
fn geometric_negative_binomial_and_hypergeometric() {
    // Geometric: P(X = k) = q (1 - q)^k.
    close(f("dgeom", &[2.0, 0.5]), 0.125, 1e-15);
    close(f("pgeom", &[2.0, 0.5]), 0.875, 1e-15);
    close(f("qgeom", &[0.75, 0.5]), 1.0, 0.0);
    assert_eq!(f("dgeom", &[-1.0, 0.5]), 0.0);
    assert_eq!(f("qgeom", &[0.0, 0.5]), 0.0);
    // Negative binomial with size 2 and q = 1/2: P(X = 2) = 3/16, P(X <= 2) = 11/16.
    close(f("dnbinom", &[2.0, 2.0, 0.5]), 0.1875, 1e-14);
    close(f("pnbinom", &[2.0, 2.0, 0.5]), 0.6875, 1e-13);
    close(f("qnbinom", &[0.6, 2.0, 0.5]), 2.0, 0.0);
    // Size 1 is the geometric distribution.
    close(f("dnbinom", &[3.0, 1.0, 0.3]), 0.3 * 0.7f64.powi(3), 1e-14);
    assert_eq!(f("qnbinom", &[0.0, 2.0, 0.5]), 0.0);
    round_trip_integer("qnbinom", "pnbinom", &[3.0, 0.2], &[0.05, 0.5, 0.95]);
    // Hypergeometric: a = 5 successes, b = 7 failures, n = 4 draws; P(m = 2) = 210 / 495.
    close(f("dhypergeom", &[2.0, 5.0, 7.0, 4.0]), 210.0 / 495.0, 1e-13);
    close(f("phypergeom", &[2.0, 5.0, 7.0, 4.0]), 420.0 / 495.0, 1e-13);
    assert_eq!(f("qhypergeom", &[0.8, 5.0, 7.0, 4.0]), 2.0);
    assert_eq!(f("qhypergeom", &[0.1, 5.0, 7.0, 4.0]), 1.0);
    assert_eq!(f("dhypergeom", &[5.0, 5.0, 7.0, 4.0]), 0.0);
    close(f("dhypergeom", &[4.0, 5.0, 7.0, 4.0]), 5.0 / 495.0, 1e-13);
}

/// Integer-valued quantiles: `p(q(p)) >= p` and `p(q(p) - 1) < p`.
fn round_trip_integer(q: &str, p_fn: &str, params: &[f64], probs: &[f64]) {
    for &p in probs {
        let mut args = vec![p];
        args.extend_from_slice(params);
        let k = f(q, &args);
        let mut at = vec![k];
        at.extend_from_slice(params);
        assert!(f(p_fn, &at) >= p, "{p_fn}({k}) below {p}");
        if k > 0.0 {
            let mut below = vec![k - 1.0];
            below.extend_from_slice(params);
            assert!(
                f(p_fn, &below) < p,
                "{k} is not the smallest with cdf >= {p}"
            );
        }
    }
}

#[test]
fn distribution_argument_errors_are_refused() {
    let unsupported = |e: &Expr| matches!(err(e), EvalError::Unsupported(_));
    assert!(unsupported(&call("dgamma", vec![n(1.0), n(0.0)])));
    assert!(unsupported(&call("pbeta", vec![n(0.5), n(-1.0), n(1.0)])));
    assert!(unsupported(&call("dt", vec![n(1.0), n(0.0)])));
    assert!(unsupported(&call("dF", vec![n(1.0), n(0.0), n(2.0)])));
    assert!(unsupported(&call("dgeom", vec![n(1.5), n(0.5)])));
    assert!(unsupported(&call("dgeom", vec![n(1.0), n(1.5)])));
    assert!(unsupported(&call("qgeom", vec![n(0.5), n(0.0)])));
    assert!(unsupported(&call("dnbinom", vec![n(1.0), n(0.0), n(0.5)])));
    assert!(unsupported(&call(
        "dhypergeom",
        vec![n(1.0), n(2.0), n(2.0), n(5.0)]
    )));
    assert!(matches!(
        err(&call("dweibull", vec![n(1.0)])),
        EvalError::ArgCount { .. }
    ));
    // Quantiles at the infinite limits are refused as non-finite.
    assert!(matches!(
        err(&call("qgamma", vec![n(1.0), n(2.0)])),
        EvalError::NotFinite
    ));
    assert!(matches!(
        err(&call("qt", vec![n(0.0), n(3.0)])),
        EvalError::NotFinite
    ));
    assert!(matches!(
        err(&call("qlogis", vec![n(0.0), n(0.0), n(1.0)])),
        EvalError::NotFinite
    ));
}

#[test]
fn random_generators_return_vectors_of_the_right_kind() {
    let column = |e: Expr| -> Vec<f64> {
        let v = value(&e);
        matrix(&v).data().iter().map(|s| s.value.re).collect()
    };
    let rnorm = column(call("rnorm", vec![n(500.0), n(3.0), n(2.0)]));
    assert_eq!(rnorm.len(), 500);
    assert!(rnorm.iter().all(|x| x.is_finite()));
    let shape = matrix(&value(&call("rnorm", vec![n(7.0), n(0.0), n(1.0)]))).shape();
    assert_eq!(shape, (7, 1));
    for x in column(call("runif", vec![n(300.0), n(-6.0), n(6.0)])) {
        assert!((-6.0..6.0).contains(&x), "{x}");
    }
    for k in column(call("rbinom", vec![n(300.0), n(10.0), n(0.5)])) {
        assert!(k.fract() == 0.0 && (0.0..=10.0).contains(&k), "{k}");
    }
    for k in column(call("rpois", vec![n(300.0), n(3.0)])) {
        assert!(k.fract() == 0.0 && k >= 0.0, "{k}");
    }
    for k in column(call("rhypergeom", vec![n(200.0), n(5.0), n(7.0), n(4.0)])) {
        assert!(k.fract() == 0.0 && (0.0..=4.0).contains(&k), "{k}");
    }
    for k in column(call("rgeom", vec![n(200.0), n(0.5)])) {
        assert!(k.fract() == 0.0 && k >= 0.0, "{k}");
    }
    let r = real(&call("rnd", vec![n(10.0)]));
    assert!((0.0..10.0).contains(&r), "{r}");
}

#[test]
fn random_generators_have_the_right_mean() {
    // Sample means of 4000 draws, within about five standard errors.
    let mean = |e: Expr| -> f64 {
        let v = value(&e);
        let xs: Vec<f64> = matrix(&v).data().iter().map(|s| s.value.re).collect();
        xs.iter().sum::<f64>() / xs.len() as f64
    };
    assert!((mean(call("rgamma", vec![n(4000.0), n(3.0)])) - 3.0).abs() < 0.1);
    assert!((mean(call("rexp", vec![n(4000.0), n(2.0)])) - 0.5).abs() < 0.03);
    assert!((mean(call("rbinom", vec![n(4000.0), n(20.0), n(0.3)])) - 6.0).abs() < 0.15);
    assert!((mean(call("rnbinom", vec![n(4000.0), n(3.0), n(0.5)])) - 3.0).abs() < 0.2);
    assert!((mean(call("rt", vec![n(4000.0), n(10.0)])) - 0.0).abs() < 0.1);
    assert!((mean(call("rbeta", vec![n(4000.0), n(2.0), n(6.0)])) - 0.25).abs() < 0.02);
}

#[test]
fn random_generator_argument_errors_are_refused() {
    let unsupported = |e: &Expr| matches!(err(e), EvalError::Unsupported(_));
    assert!(unsupported(&call("rnorm", vec![n(0.0), n(0.0), n(1.0)])));
    assert!(unsupported(&call("rnorm", vec![n(2.5), n(0.0), n(1.0)])));
    assert!(unsupported(&call("runif", vec![n(3.0), n(1.0), n(1.0)])));
    assert!(unsupported(&call("rgamma", vec![n(3.0), n(0.0)])));
    assert!(unsupported(&call("rnd", vec![n(0.0)])));
    assert!(unsupported(&call("rbinom", vec![n(3.0), n(-1.0), n(0.5)])));
    assert!(unsupported(&call("rnorm", vec![n(2e6), n(0.0), n(1.0)])));
}

#[test]
fn scalar_helper_matches_scalar_struct() {
    // Guard the helper used above: a plain scalar keeps its dimension through `value`.
    let s = scalar(&value(&n(2.0)));
    assert_eq!(s, Scalar::real(2.0, dimless()));
}

#[test]
fn binomial_quantile_at_probability_one_is_n() {
    close(
        real(&call("qbinom", vec![n(1.0), n(10.0), n(0.001)])),
        10.0,
        0.0,
    );
    close(
        real(&call("qbinom", vec![n(1.0), n(1000.0), n(0.5)])),
        1000.0,
        0.0,
    );
    close(
        real(&call("qbinom", vec![n(1.0), n(100000.0), n(0.3)])),
        100000.0,
        0.0,
    );
    close(
        real(&call("qbinom", vec![n(0.0), n(1000.0), n(0.5)])),
        0.0,
        0.0,
    );
    close(
        real(&call("qbinom", vec![n(0.5), n(10.0), n(0.0)])),
        0.0,
        0.0,
    );
    close(
        real(&call("qbinom", vec![n(0.5), n(10.0), n(1.0)])),
        10.0,
        0.0,
    );
}

#[test]
fn binomial_quantile_matches_exact_partial_sums() {
    // Smallest k with P(X <= k) >= prob, from exact integer sums (python `math.comb`, Fraction).
    // Rows are (n, q); columns are prob = 0.5, 0.9, 0.99, 1 - 1e-12.
    let cases = [
        (10.0, 0.3, [3.0, 5.0, 7.0, 10.0]),
        (50.0, 0.5, [25.0, 30.0, 33.0, 48.0]),
        (1000.0, 0.001, [1.0, 2.0, 4.0, 14.0]),
    ];
    let probs = [0.5, 0.9, 0.99, 1.0 - 1e-12];
    for (trials, q, expected) in cases {
        for (prob, want) in probs.iter().zip(expected) {
            close(
                real(&call("qbinom", vec![n(*prob), n(trials), n(q)])),
                want,
                0.0,
            );
        }
    }
}

#[test]
fn binomial_quantile_inverts_the_cdf_on_a_grid() {
    for (trials, q) in [(10.0, 0.3), (25.0, 0.7)] {
        for k in 0..=(trials as u32) {
            let p = real(&call("pbinom", vec![n(f64::from(k)), n(trials), n(q)]));
            close(
                real(&call("qbinom", vec![n(p), n(trials), n(q)])),
                f64::from(k),
                0.0,
            );
        }
    }
}

#[test]
fn poisson_quantile_at_one_is_refused_and_at_zero_is_zero() {
    let msg = err(&call("qpois", vec![n(1.0), n(2.0)])).to_string();
    assert!(
        msg.contains("qpois needs p < 1 (the limit is infinite)"),
        "{msg}"
    );
    close(real(&call("qpois", vec![n(0.0), n(2.0)])), 0.0, 0.0);
    // Exact quantile: smallest k with P(X <= k) >= 0.999999 is 12 (python Decimal, 60 digits).
    close(real(&call("qpois", vec![n(0.999_999), n(2.0)])), 12.0, 0.0);
}

#[test]
fn count_limit_errors_name_the_count() {
    let msg = err(&call("ppois", vec![n(1e9), n(1e9)])).to_string();
    assert!(msg.contains("k above the supported limit"), "{msg}");
    let msg = err(&call("dbinom", vec![n(5e7), n(1e8), n(0.5)])).to_string();
    assert!(msg.contains("n above the supported limit"), "{msg}");
}

// ---- accuracy at extreme arguments (reviewer findings on the continuous families) ----

/// `actual` within relative `tol` of `expected`.
fn rel_close(actual: f64, expected: f64, tol: f64) {
    assert!(
        ((actual - expected) / expected).abs() <= tol,
        "got {actual}, want {expected} (relative tolerance {tol})"
    );
}

#[test]
fn quantiles_at_tiny_probabilities_are_not_stalled() {
    // Closed forms for small p: qgamma(p, 1) = -ln(1 - p), qchisq(p, 2) = -2 ln(1 - p), and
    // qF(p, 2, 3) = p (the cdf is 1 - (1 - z)^(3/2) with z = 2x / (2x + 3)).
    rel_close(f("qgamma", &[1e-200, 1.0]), 1e-200, 1e-12);
    rel_close(f("qchisq", &[1e-200, 2.0]), 2e-200, 1e-12);
    rel_close(f("qF", &[1e-200, 2.0, 3.0]), 1e-200, 1e-10);
}

#[test]
fn gamma_quantile_with_tiny_shape_keeps_its_magnitude() {
    // x = (p Gamma(1 + a))^(1/a) for small a; here a = 0.001, p = 0.5 gives 5.2442e-302, which the
    // old floor (1e-300 times the bracket) reported as 1e-300.
    rel_close(f("qgamma", &[0.5, 0.001]), 5.244_206_408_278_684e-302, 1e-9);
}

#[test]
fn t_quantile_for_one_degree_is_the_cauchy_quantile_in_the_far_tail() {
    // qt(p, 1) = -cot(pi p) for p < 1/2, which is -1 / (pi p) to relative 1e-300.
    rel_close(
        f("qt", &[1e-151, 1.0]),
        -1.0 / (std::f64::consts::PI * 1e-151),
        1e-9,
    );
    // Below the representable z the quantile is refused, not the floor's value.
    assert!(matches!(
        err(&call("qt", vec![n(1e-200), n(1.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn t_cdf_is_accurate_for_huge_degrees_of_freedom() {
    // The t law tends to the normal with d -> infinity: pt(0.1, 1e300) -> Phi(0.1) and
    // dt(1, 1e300) -> phi(1), the corrections being O(1/d).
    close(f("pt", &[0.1, 1e300]), 0.539_827_837_277_029, 1e-10);
    let phi1 = (-0.5f64).exp() / (2.0 * std::f64::consts::PI).sqrt();
    rel_close(f("dt", &[1.0, 1e300]), phi1, 1e-12);
    // pt(x, d) = Phi(x) - phi(x) x (x^2 + 1) / (4 d) + O(d^-2); at d = 1e6 and x = 0.001 the
    // correction is about 1e-10, and the old formula was off by 4e-9.
    close(f("pt", &[0.001, 1e6]), 0.500_398_942_114_175_3, 1e-10);
}

#[test]
fn large_shape_densities_keep_their_relative_accuracy() {
    let sqrt = |x: f64| x.sqrt();
    let pi = std::f64::consts::PI;
    // The t density tends to phi(x) (1 - 1/(2 d)) at x = 1 (second order 0.2 / d^2).
    let phi1 = (-0.5f64).exp() / (2.0 * pi).sqrt();
    rel_close(f("dt", &[1.0, 1e8]), phi1 * (1.0 - 1.0 / 2e8), 1e-12);
    rel_close(f("dt", &[1.0, 1e12]), phi1 * (1.0 - 1.0 / 2e12), 1e-11);
    // Beta(a, a) density at 1/2 is sqrt(4 a / pi) (1 - 1.25e-9) at a = 1e8, from the Stirling tail.
    rel_close(
        f("dbeta", &[0.5, 1e8, 1e8]),
        sqrt(4e8 / pi) * (1.0 - 1.25e-9),
        1e-11,
    );
    // F(d1, d2) density at x = 1 with d1 = d2 = 1e8 is beta(1/2; 5e7, 5e7) times dz/dx = 1/4.
    rel_close(
        f("dF", &[1.0, 1e8, 1e8]),
        sqrt(2e8 / pi) / 4.0 * (1.0 - 2.5e-9),
        1e-11,
    );
    // Chi-squared density at its mode, d = 1e10: with s = d / 2 the gamma density at s is
    // (2 pi s)^(-1/2) exp(-1 / (12 s)), and the chi-squared density is half of it at x / 2 = s.
    let s = 5e9f64;
    rel_close(
        f("dchisq", &[1e10, 1e10]),
        0.5 / sqrt(pi * 1e10) * (-1.0 / (12.0 * s)).exp(),
        1e-11,
    );
}

#[test]
fn beta_and_f_cdfs_are_probabilities_for_huge_second_shape() {
    // With q -> infinity, q * x -> y and I_x(p, q) -> P(Gamma(p) <= y): pbeta(1e-300, 1/2, 1e300)
    // -> P(Gamma(1/2) <= 1) = erf(1). F(d1, d2) -> chi2(d1) / d1 as d2 -> infinity, so
    // pF(1/2, 1/2, 1e300) = P(chi2(1/2) <= 1/4) = P(Gamma(1/4) <= 1/8) = 0.640157206083...
    let pb = f("pbeta", &[1e-300, 0.5, 1e300]);
    assert!((0.0..=1.0).contains(&pb), "{pb}");
    close(pb, 0.842_700_792_949_714_9, 1e-9);
    let pf = f("pF", &[0.5, 0.5, 1e300]);
    assert!((0.0..=1.0).contains(&pf), "{pf}");
    close(pf, 0.640_157_206_083_084_4, 1e-9);
}

#[test]
fn lognormal_quantile_at_zero_is_zero() {
    assert_eq!(f("qlnorm", &[0.0, 0.0, 1.0]), 0.0);
    assert!(matches!(
        err(&call("qlnorm", vec![n(1.0), n(0.0), n(1.0)])),
        EvalError::NotFinite
    ));
}

#[test]
fn rbeta_with_tiny_shapes_is_finite_and_in_the_unit_interval() {
    // a = b = 0.001: both gamma draws underflow to zero about half the time, which used to fail.
    let v = value(&call("rbeta", vec![n(1000.0), n(0.001), n(0.001)]));
    let xs: Vec<f64> = matrix(&v).data().iter().map(|s| s.value.re).collect();
    assert_eq!(xs.len(), 1000);
    assert!(xs.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)));
    // Symmetric and U-shaped: about half the draws below 1/2, and most of them near the ends.
    let below = xs.iter().filter(|&&x| x < 0.5).count() as f64 / 1000.0;
    assert!((below - 0.5).abs() < 0.08, "{below}");
    let near_ends = xs.iter().filter(|x| !(0.01..=0.99).contains(*x)).count();
    assert!(near_ends > 900, "{near_ends}");
}

//! The work budget, second part: the charges that were missing or too low (matrix products, element-wise
//! classes, output size, matrix copies, ranges), and the list of stubs that must be re-classed once they
//! are implemented. Each refusal is decided before the work runs, so these tests stay fast.

use super::helpers::*;
use crate::eval::work::{binary_units, builtin_units, class_of_listed, Budget, Cost};
use crate::eval::work_table::SHAPE;
use crate::eval::{evaluate, Env, EvalError, Matrix, Scalar, Value};
use crate::expr::{BinaryOp, Expr};
use crate::units::Dimension;

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

/// A `rows x cols` matrix of ones.
fn ones(rows: usize, cols: usize) -> Value {
    let data = vec![Scalar::real(1.0, dimless()); rows * cols];
    Value::Matrix(Matrix::new(rows, cols, data).expect("matrix"))
}

/// A worksheet holding a `rows x cols` matrix of ones under `name`.
fn env_with(name: &str, rows: usize, cols: usize) -> Env {
    let mut env = Env::new();
    env.set_variable(name.to_string(), ones(rows, cols));
    env
}

fn is_work_limit(got: &Result<Value, EvalError>) -> bool {
    matches!(got, Err(EvalError::WorkLimit(_)))
}

#[test]
fn a_500_by_500_product_is_refused_and_a_200_by_200_product_is_admitted() {
    // One multiply-add is about 110 ns, so 11 units. 500^3 = 1.25e8 multiply-adds is 1.375e9 units,
    // over the whole budget (3e8). 200^3 = 8e6 is 8.8e7 units, about 0.9 s.
    let (big, small) = (ones(500, 500), ones(200, 200));
    assert!(binary_units(BinaryOp::Mul, &big, &big).is_err());
    assert_eq!(binary_units(BinaryOp::Mul, &small, &small), Ok(88_000_000));
}

#[test]
fn det_of_500_is_charged_its_measured_cost() {
    // Elimination of 500 x 500 takes about 0.06 s in release: rows * cols * min / 21 units.
    assert_eq!(builtin_units("det", &[ones(500, 500)]), 5_952_380);
}

#[test]
fn a_scalar_power_of_a_matrix_is_charged_per_element() {
    // Measured 47-157 ns per element of a power: 16 units per element, not 1.
    let base = Value::Scalar(Scalar::real(2.0, dimless()));
    assert_eq!(
        binary_units(BinaryOp::Pow, &base, &ones(1000, 1000)),
        Ok(16_000_000)
    );
}

#[test]
fn element_wise_arithmetic_is_charged_three_units_per_element() {
    // Measured 15-50 ns per element of add: 3 units per element. (Matrix times matrix is a product,
    // so the multiplication here is by a scalar.)
    let m = ones(1000, 1000);
    let two = Value::Scalar(Scalar::real(2.0, dimless()));
    assert_eq!(binary_units(BinaryOp::Add, &m, &m), Ok(3_000_000));
    assert_eq!(binary_units(BinaryOp::Mul, &two, &m), Ok(3_000_000));
}

#[test]
fn a_function_of_a_matrix_is_charged_three_units_per_element() {
    // sin measured 18-23 ns per element.
    assert_eq!(builtin_units("sin", &[ones(1000, 1000)]), 3_000_000);
}

#[test]
fn identity_is_charged_for_its_output_before_it_allocates() {
    // identity(3000) builds 9e6 elements: 3 units each, 2.7e7 units. The argument itself is a scalar
    // (its class charges 3 units), so the total is 27_000_003.
    let n = Value::Scalar(Scalar::real(3000.0, dimless()));
    assert_eq!(builtin_units("identity", &[n]), 27_000_003);
}

#[test]
fn an_identity_too_large_for_the_element_limit_is_still_refused_as_too_large() {
    // The output charge is capped at the element limit, so the builtin reports TooLarge, not work.
    let got = evaluate(&call("identity", vec![n(20_000.0)]), &Env::new());
    assert!(matches!(got, Err(EvalError::TooLarge(_))), "{got:?}");
}

#[test]
fn reading_a_matrix_variable_is_charged_per_copy() {
    // Each reference to M copies 1e6 elements: 3e6 units. 200 references are 6e8 units, so the
    // budget runs out after 100. Before the charge, all 200 were free.
    let env = env_with("M", 1000, 1000);
    let elements = (0..200)
        .map(|_| index(v("M"), vec![n(1.0), n(1.0)]))
        .collect();
    let got = evaluate(&mat(1, 200, elements), &env);
    assert!(is_work_limit(&got), "got {:?}", got.as_ref().err());
}

#[test]
fn building_a_range_is_charged_as_it_is_built() {
    // 0 .. 1e6 is 1e6 elements: 3e6 units. 120 of them are 3.6e8, over the budget.
    let range = || Expr::Range {
        from: Box::new(n(0.0)),
        step: None,
        to: Box::new(n(999_999.0)),
    };
    let elements = (0..120).map(|_| index(range(), vec![n(5.0)])).collect();
    let got = evaluate(&mat(1, 120, elements), &Env::new());
    assert!(is_work_limit(&got), "got {:?}", got.as_ref().err());
}

#[test]
fn one_range_of_a_million_is_still_admitted() {
    let range = Expr::Range {
        from: Box::new(n(0.0)),
        step: None,
        to: Box::new(n(999_999.0)),
    };
    let got = evaluate(&index(range, vec![n(5.0)]), &Env::new());
    assert!(got.is_ok(), "got {:?}", got.as_ref().err());
}

#[test]
fn stub_classes_are_listed_for_reclassing() {
    // Implemented (matrix_decomp2, matrix_svd): none may be classed as shape, which would under-charge
    // a real kernel. The stub list is empty now; a new stub must be classed as shape and listed here.
    for name in ["svd", "LU", "QR", "Cholesky"] {
        assert!(
            matches!(class_of_listed(name), Some(Cost::Dense(_))),
            "{name} is not a dense class"
        );
    }
    // `vec` is still a stub (only vec(range) is built, and it is evaluated before the call).
    assert_eq!(class_of_listed("vec"), Some(SHAPE));
    {}
    assert_eq!(SHAPE, Cost::PerElement(3));
}

#[test]
fn eigen_functions_are_charged_by_their_dense_class() {
    // Reclassed when implemented (matrix_decomp): not measured, see work_table.
    for name in [
        "eigenvals",
        "genvals",
        "eigenvec",
        "eigenvecs",
        "genvecs",
        "geninv",
    ] {
        assert!(
            matches!(class_of_listed(name), Some(Cost::Dense(_))),
            "{name} is not a dense class"
        );
    }
}

#[test]
fn geninv_is_charged_for_its_identity_right_hand_side() {
    // geninv(A) solves A X = I with an m x m identity: least squares with m right-hand columns,
    // 2/3 m n (n + m) units. For 600 x 600 that is 2.88e8 of the 3e8 budget.
    assert_eq!(builtin_units("geninv", &[ones(600, 600)]), 288_000_000);
    // 700 x 700 is 4.57e8 units: over the whole budget, so the charge refuses it before the solve.
    let budget = Budget::new();
    assert!(budget
        .charge(builtin_units("geninv", &[ones(700, 700)]))
        .is_err());
}

// ---- random draws: the charge follows the real cost (reviewer findings) ----

fn refused_by_budget(e: Expr) -> bool {
    matches!(evaluate(&e, &Env::new()), Err(EvalError::WorkLimit(_)))
}

#[test]
fn negative_binomial_draws_are_charged_on_their_mean() {
    // rnbinom(10, 1, 1e-12) has mean 1e12 (about 1 s on the old charge of 880 units), and
    // rnbinom(1000, 1, 1e-6) has mean 1e6 (7 s). Both are refused now; a small mean is admitted.
    assert!(refused_by_budget(call(
        "rnbinom",
        vec![n(10.0), n(1.0), n(1e-12)]
    )));
    assert!(refused_by_budget(call(
        "rnbinom",
        vec![n(1000.0), n(1.0), n(1e-6)]
    )));
    assert!(evaluate(&call("rnbinom", vec![n(10.0), n(3.0), n(0.5)]), &Env::new()).is_ok());
}

#[test]
fn poisson_draws_are_charged_their_real_rate() {
    // rpois(10, 1e10) was charged as if lambda were 1e6 (2.2e8 units, 10 s).
    assert!(refused_by_budget(call("rpois", vec![n(10.0), n(1e10)])));
    assert!(evaluate(&call("rpois", vec![n(10.0), n(3.0)]), &Env::new()).is_ok());
}

#[test]
fn hypergeometric_draws_are_refused_before_three_seconds() {
    // About 100 ns per unit (not 10 ns): rhypergeom(1000, 500, 500, 500) is 2e8 units, about 2 s,
    // and admitted; rhypergeom(2000, ...) is 4e8 units, about 4 s, and refused.
    assert!(evaluate(
        &call("rhypergeom", vec![n(1000.0), n(500.0), n(500.0), n(500.0)]),
        &Env::new()
    )
    .is_ok());
    assert!(refused_by_budget(call(
        "rhypergeom",
        vec![n(2000.0), n(500.0), n(500.0), n(500.0)]
    )));
    assert!(evaluate(
        &call("rhypergeom", vec![n(200.0), n(5.0), n(7.0), n(4.0)]),
        &Env::new()
    )
    .is_ok());
}

fn num_value(x: f64) -> Value {
    Value::Scalar(Scalar::real(x, dimless()))
}

#[test]
fn predict_is_charged_for_its_steps() {
    // predict(2000 values, 1000, 2e6): 3 * 1001 * (2000 + 2e6) = 6e9 units, over the 3e8 budget.
    let env = env_with("v", 2000, 1);
    let refused = evaluate(&call("predict", vec![v("v"), n(1000.0), n(2e6)]), &env);
    assert!(
        matches!(refused, Err(EvalError::WorkLimit(_))),
        "{refused:?}"
    );
    // A short forecast of a short record is admitted.
    let env = env_with("v", 50, 1);
    assert!(evaluate(&call("predict", vec![v("v"), n(3.0), n(4.0)]), &env).is_ok());
}

#[test]
fn hist_is_charged_for_its_edges() {
    // hist(P, data) builds P + 1 edges, so the charge is at least P units.
    let data = ones(10, 1);
    let charge = builtin_units("hist", &[num_value(1e7), data.clone()]);
    assert!(charge >= 10_000_000, "{charge}");
    assert!(builtin_units("histogram", &[num_value(1e7), data]) >= 10_000_000);
}

#[test]
fn fft_of_a_length_that_is_not_a_power_of_two_is_charged_six_times() {
    // Bluestein costs about 6x radix-2 (dft of 2^20 took 91 ms, of 2^20 + 1 took 540 ms). The
    // element and pass counts differ too, so the ratio is checked against a 5x bound.
    let pow2 = builtin_units("dft", &[ones(1024, 1)]);
    let other = builtin_units("dft", &[ones(1025, 1)]);
    assert!(other > 5 * pow2, "{other} vs {pow2}");
    // ifft runs radix-2 on n = 2(L - 1), so a length-3 half spectrum (n = 4) is not charged 6x.
    assert_eq!(builtin_units("ifft", &[ones(3, 1)]), 60);
}

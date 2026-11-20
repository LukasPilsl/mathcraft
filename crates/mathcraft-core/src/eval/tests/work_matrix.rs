//! The shared work budget for matrix builtins: shape-based refusals before any kernel runs, and
//! the charge of each dense family. Kernel-heavy sizes are checked on the charge alone, so the
//! debug-build test run stays fast.

use super::helpers::*;
use crate::eval::work::{check_units, dense_units, Dense, MAX_WORK};
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::expr::Expr;

fn identity(size: f64) -> Expr {
    call("identity", vec![n(size)])
}

fn refused(e: &Expr) -> bool {
    matches!(evaluate(e, &Env::new()), Err(EvalError::WorkLimit(_)))
}

fn accepted(e: &Expr) -> bool {
    matches!(
        evaluate(e, &Env::new()),
        Ok(Value::Matrix(_) | Value::Scalar(_))
    )
}

#[test]
fn svds_of_a_300_square_is_accepted_and_600_is_refused() {
    assert!(accepted(&call("svds", vec![identity(300.0)])));
    assert!(refused(&call("svds", vec![identity(600.0)])));
}

#[test]
fn cond2_follows_the_same_charge_as_svds() {
    assert!(accepted(&call("cond2", vec![identity(300.0)])));
    assert!(refused(&call("cond2", vec![identity(600.0)])));
}

#[test]
fn svd_with_vectors_is_charged_above_svds_and_refuses_from_400() {
    // svd accumulates V as well (5 k^2 m units against 4 for svds): 400 square is 3.2e8 units,
    // over the 3e8 budget, while svds of 400 (2.56e8) is accepted. Identity matrices converge in
    // one sweep, so the accepted cases run quickly.
    // svd returns a nested list, so the check is "not refused by the budget".
    assert!(!refused(&call("svd", vec![identity(300.0)])));
    assert!(refused(&call("svd", vec![identity(400.0)])));
    assert!(accepted(&call("svds", vec![identity(400.0)])));
    assert!(dense_units(Dense::JacobiVectors, 400, 400, 0) > MAX_WORK);
    assert!(dense_units(Dense::Jacobi, 400, 400, 0) <= MAX_WORK);
}

#[test]
fn rref_and_rank_of_500_are_accepted_and_900_is_refused() {
    assert!(accepted(&call("rref", vec![identity(500.0)])));
    assert!(accepted(&call("rank", vec![identity(500.0)])));
    // 900 is accepted now: 900^3 / 21 = 3.5e7 units (about 0.3 s). The elimination class was
    // recalibrated from 2/3 n^3 to n^3 / 21 (measured 0.7 ns per flop against 10 ns per unit), so 2000
    // (3.8e8 units) is the first refused size.
    assert!(accepted(&call("rref", vec![identity(900.0)])));
    assert!(refused(&call("rref", vec![identity(2000.0)])));
    assert!(refused(&call("rank", vec![identity(2000.0)])));
}

#[test]
fn lsolve_charge_is_cubic_in_the_size() {
    // Charged, not run: the least-squares kernel is too slow for a debug test at n = 500.
    assert!(check_units(dense_units(Dense::LeastSquares, 500, 500, 1)).is_ok());
    assert!(refused(&call(
        "lsolve",
        vec![identity(900.0), call("identity", vec![n(900.0)])]
    )));
}

#[test]
fn cond1_charges_an_inverse() {
    // The inverse is max^3 / 8 units (measured 0.65 ns per unit of the old 2 n^3 formula, so about
    // 1.3 n^3 ns). It was 2 n^3 before the recalibration.
    assert_eq!(
        dense_units(Dense::Inverse, 500, 500, 0),
        500 * 500 * 500 / 8
    );
    assert!(check_units(dense_units(Dense::Inverse, 1200, 1200, 0)).is_ok());
    assert!(check_units(dense_units(Dense::Inverse, 1400, 1400, 0)).is_err());
}

#[test]
fn a_statistic_over_a_million_elements_is_accepted() {
    // mean costs 3 units per element: 3e6 units for a 1000 x 1000 matrix.
    assert!(accepted(&call("mean", vec![identity(1000.0)])));
}

#[test]
fn rank_of_2000_square_is_refused_as_work_not_elements() {
    // 2000^3 / 21 = 3.8e8 units, over the budget (1000 is 4.8e7 units, about 0.5 s, and is admitted).
    let got = evaluate(&call("rank", vec![identity(2000.0)]), &Env::new());
    assert!(
        matches!(got, Err(EvalError::WorkLimit(MAX_WORK))),
        "got {:?}",
        got.as_ref().err()
    );
}

#[test]
fn an_oversized_result_is_an_element_refusal() {
    // 100000 x 1 times 1 x 100000 would need 1e10 elements: the element count is judged first.
    let column = mat(100_000, 1, vec![n(1.0); 100_000]);
    let row = mat(1, 100_000, vec![n(1.0); 100_000]);
    let got = evaluate(&mul(column, row), &Env::new());
    assert!(matches!(got, Err(EvalError::TooLarge(_))), "got {got:?}");
}

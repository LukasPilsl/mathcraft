//! Error paths, degenerate Jacobians, overdetermined and underdetermined systems, budgets, sizes,
//! and determinism.

use super::{assert_vec_close, golf_ball_problem, solve};
use crate::solve::{find, Limits, Problem, SolveError};

#[test]
fn singular_jacobian_at_an_exact_root() {
    // f = x^2 at x = 0: the Jacobian is zero, but the guess is already a root.
    let p = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![v[0] * v[0]]));
    let sol = solve(&p);
    assert_eq!(sol.x, vec![0.0]);
}

#[test]
fn singular_jacobian_handled_by_restart() {
    // f = x^2 - 1 at x = 0: zero Jacobian and no root there. The restarts reach +1 or -1.
    let p = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 1.0]));
    let sol = solve(&p);
    assert!((sol.x[0].abs() - 1.0).abs() < 1e-12, "got {:?}", sol.x);
}

#[test]
fn no_real_root_is_no_convergence() {
    let p = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] + 1.0]));
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn nan_value_aborts_with_not_finite() {
    // ln(-1) is NaN at the guess.
    let p = Problem::new(vec![-1.0]).equations(1, |v: &[f64]| Ok(vec![v[0].ln()]));
    assert_eq!(find(&p), Err(SolveError::NotFinite));
}

#[test]
fn infinite_value_aborts_with_not_finite() {
    let p = Problem::new(vec![1.0]).equations(1, |_: &[f64]| Ok(vec![f64::INFINITY]));
    assert_eq!(find(&p), Err(SolveError::NotFinite));
}

#[test]
fn closure_error_is_passed_through() {
    let p = Problem::new(vec![1.0])
        .equations(1, |_: &[f64]| Err(SolveError::Unsupported("test".into())));
    assert_eq!(find(&p), Err(SolveError::Unsupported("test".into())));
}

#[test]
fn wrong_output_length_is_dimension_mismatch() {
    let p = Problem::new(vec![1.0, 2.0]).equations(2, |v: &[f64]| Ok(vec![v[0]]));
    assert_eq!(
        find(&p),
        Err(SolveError::DimensionMismatch {
            expected: 2,
            got: 1
        })
    );
}

#[test]
fn huge_unknown_count_is_too_large() {
    let p = Problem::new(vec![0.0; 10_001]).equations(1, |_: &[f64]| Ok(vec![0.0]));
    assert!(matches!(
        find(&p),
        Err(SolveError::TooLarge { n: 10_001, .. })
    ));
}

#[test]
fn huge_equation_count_is_too_large() {
    let p = Problem::new(vec![0.0]).equations(20_000, |_: &[f64]| Ok(vec![0.0; 20_000]));
    assert!(matches!(find(&p), Err(SolveError::TooLarge { .. })));
}

#[test]
fn product_of_sizes_is_capped() {
    let p = Problem::new(vec![0.0; 5_000]).equations(5_000, |_: &[f64]| Ok(vec![0.0; 5_000]));
    assert!(matches!(find(&p), Err(SolveError::TooLarge { .. })));
}

#[test]
fn empty_problem_is_invalid() {
    let p = Problem::new(vec![]).equations(1, |_: &[f64]| Ok(vec![0.0]));
    assert!(matches!(find(&p), Err(SolveError::InvalidProblem(_))));
}

#[test]
fn no_constraints_is_unsupported() {
    let p = Problem::new(vec![1.0]);
    assert!(matches!(find(&p), Err(SolveError::Unsupported(_))));
}

#[test]
fn bad_tolerances_are_invalid() {
    let f = |v: &[f64]| Ok(vec![v[0] - 1.0]);
    let p = Problem::new(vec![0.0]).equations(1, f).ctol(0.0);
    assert!(matches!(find(&p), Err(SolveError::InvalidProblem(_))));
    let p = Problem::new(vec![0.0]).equations(1, f).tol(f64::NAN);
    assert!(matches!(find(&p), Err(SolveError::InvalidProblem(_))));
}

#[test]
fn evaluation_budget_is_enforced() {
    // x^2 - 2 from 10: guess plus Jacobian (2 calls) use the 3 allowed, so the line search stops.
    let p = Problem::new(vec![10.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 2.0]))
        .limits(Limits {
            max_iterations: 200,
            max_evaluations: Some(3),
        });
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn overdetermined_consistent_system() {
    // x - 1, 2x - 2, 3x - 3 all vanish at x = 1.
    let p = Problem::new(vec![0.0]).equations(3, |v: &[f64]| {
        Ok(vec![v[0] - 1.0, 2.0 * v[0] - 2.0, 3.0 * v[0] - 3.0])
    });
    assert_vec_close(&solve(&p).x, &[1.0], 1e-12);
}

#[test]
fn overdetermined_inconsistent_system_is_an_error() {
    // x = 1 and x = 2 cannot both hold; the least-squares point 1.5 leaves residual 1/sqrt2.
    let p = Problem::new(vec![0.0]).equations(2, |v: &[f64]| Ok(vec![v[0] - 1.0, v[0] - 2.0]));
    match find(&p) {
        Err(SolveError::Inconsistent { residual }) => {
            assert!(
                (residual - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9,
                "{residual}"
            );
        }
        other => panic!("expected Inconsistent, got {other:?}"),
    }
}

#[test]
fn underdetermined_consistent_system_takes_least_norm_step() {
    // x + y = 1 from (0, 0): the least-norm solution is (1/2, 1/2).
    let p = Problem::new(vec![0.0, 0.0]).equations(1, |v: &[f64]| Ok(vec![v[0] + v[1] - 1.0]));
    assert_vec_close(&solve(&p).x, &[0.5, 0.5], 1e-12);
}

#[test]
fn singular_inconsistent_system_is_an_error() {
    // x + y = 1 and x + y = 2 cannot hold together; the Jacobian is singular everywhere.
    let p = Problem::new(vec![0.0, 0.0]).equations(2, |v: &[f64]| {
        Ok(vec![v[0] + v[1] - 1.0, v[0] + v[1] - 2.0])
    });
    assert!(find(&p).is_err());
}

#[test]
fn results_are_deterministic() {
    let a = find(&golf_ball_problem(40.0, vec![30.0, 3.0])).unwrap();
    let b = find(&golf_ball_problem(40.0, vec![30.0, 3.0])).unwrap();
    assert_eq!(a, b);
    let bits_a: Vec<u64> = a.x.iter().map(|v| v.to_bits()).collect();
    let bits_b: Vec<u64> = b.x.iter().map(|v| v.to_bits()).collect();
    assert_eq!(bits_a, bits_b);
}

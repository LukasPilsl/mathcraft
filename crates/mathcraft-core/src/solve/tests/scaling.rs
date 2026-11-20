//! Scale behaviour: badly scaled unknowns and equations, zero roots, large unknown counts, and
//! inequality starts. Expected values are exact roots derived by hand in the comments.

use super::{find, solve};
use crate::solve::{Problem, SolveError};

/// Relative error check with no unit floor (the `close` helper floors at 1, which hides tiny roots).
fn rel_eq(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs()
}

#[test]
fn tiny_unknown_with_huge_unknown_is_not_falsely_converged() {
    // f1 = 1e-12 ((1e9 x1)^2 - 9) has roots x1 = +-3e-9; f2 = 1e12 (1e-9 x2 - 2) has x2 = 2e9.
    // The old global step test (tol * (1 + |x|_inf)) accepted x1 = 3.00009e-9 here.
    let p = Problem::new(vec![1e-9, 1e9]).equations(2, |v: &[f64]| {
        Ok(vec![
            1e-12 * ((v[0] * 1e9).powi(2) - 9.0),
            1e12 * (v[1] * 1e-9 - 2.0),
        ])
    });
    let sol = solve(&p);
    assert!(rel_eq(sol.x[0], 3e-9, 1e-12), "x1 = {:e}", sol.x[0]);
    assert!(rel_eq(sol.x[1], 2e9, 1e-12), "x2 = {:e}", sol.x[1]);
}

#[test]
fn sine_root_at_zero_is_absolute_zero() {
    // sin x = 0 from 0.5 runs to the root 0 (Newton: x -> x - tan x, cubic).
    let p = Problem::new(vec![0.5]).equations(1, |v: &[f64]| Ok(vec![v[0].sin()]));
    let sol = solve(&p);
    assert!(sol.x[0].abs() < 1e-15, "x = {:e}", sol.x[0]);
}

#[test]
fn quadratic_roots_from_both_sides() {
    // x^2 - x = 0 has roots 0 and 1. From 0.1 the iteration heads to 0, from 1.9 to 1.
    let f = |v: &[f64]| Ok(vec![v[0] * v[0] - v[0]]);
    let low = solve(&Problem::new(vec![0.1]).equations(1, f));
    assert!(low.x[0].abs() < 1e-15, "x = {:e}", low.x[0]);
    let high = solve(&Problem::new(vec![1.9]).equations(1, f));
    assert!(rel_eq(high.x[0], 1.0, 1e-12), "x = {:e}", high.x[0]);
}

fn sum_problem(n: usize) -> Problem<'static> {
    // sum(x) = n from zeros: the minimum-norm root is x_i = 1 for every i.
    Problem::new(vec![0.0; n]).equations(1, move |x: &[f64]| {
        Ok(vec![x.iter().sum::<f64>() - n as f64])
    })
}

#[test]
fn large_trivial_problem_converges_at_2000() {
    let sol = solve(&sum_problem(2000));
    assert!(sol.x.iter().all(|v| (v - 1.0).abs() < 1e-9));
}

#[test]
#[ignore = "slow: run with --ignored"]
fn large_trivial_problem_converges_at_10000() {
    // Each Jacobian costs 4n = 40 000 closure calls; the automatic budget
    // (8 n max_iterations, here 1.6e7) covers it. The old fixed 100 000 stopped after 3 iterations.
    let sol = solve(&sum_problem(10_000));
    assert!(sol.x.iter().all(|v| (v - 1.0).abs() < 1e-9));
}

#[test]
#[ignore = "slow: about 44 s in release (25 iterations x 5 starts), far longer in debug; run with --release --ignored"]
fn dense_no_root_at_1000_gives_no_convergence_within_budget() {
    // sum(x^2) + 1 = 0 has no root; every row is the same. Time is printed, not asserted.
    let n = 1000;
    let p = Problem::new(vec![0.0; n]).equations(n, move |x: &[f64]| {
        let s = x.iter().map(|v| v * v).sum::<f64>() + 1.0;
        Ok(vec![s; n])
    });
    let t = std::time::Instant::now();
    let r = find(&p);
    println!(
        "dense no-root n=1000: {:?} in {:?}",
        r.as_ref().err(),
        t.elapsed()
    );
    assert!(matches!(r, Err(SolveError::NoConvergence { .. })));
}

#[test]
fn explicit_evaluation_budget_still_applies_to_large_problems() {
    // n = 2000 keeps this fast in debug. One Jacobian costs 8000 calls, so 10 000 runs out.
    let p = sum_problem(2000).limits(crate::solve::Limits {
        max_iterations: 200,
        max_evaluations: Some(10_000),
    });
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn inequality_pulls_square_root_to_positive_branch() {
    // x^2 = 4 with x >= 0 from -1: the ordinary start lands on -2 (violates x >= 0), so the
    // feasible seed (moved from -1 along the gradient of x to x = 1) finds 2.
    let p = Problem::new(vec![-1.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 4.0]))
        .inequalities(1, |v: &[f64]| Ok(vec![v[0]]));
    let sol = solve(&p);
    assert!(rel_eq(sol.x[0], 2.0, 1e-12), "x = {:e}", sol.x[0]);
}

#[test]
fn inequality_pulls_square_root_to_negative_branch() {
    // x^2 = 4 with -x >= 0 (x <= 0) from 1: the seed is -1, which gives -2.
    let p = Problem::new(vec![1.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 4.0]))
        .inequalities(1, |v: &[f64]| Ok(vec![-v[0]]));
    let sol = solve(&p);
    assert!(rel_eq(sol.x[0], -2.0, 1e-12), "x = {:e}", sol.x[0]);
}

#[test]
fn infeasible_inequality_gives_no_convergence_not_a_wrong_point() {
    // x^2 = 4 with x >= 3 has no solution. Every start must fail, so no point is returned.
    let p = Problem::new(vec![-1.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 4.0]))
        .inequalities(1, |v: &[f64]| Ok(vec![v[0] - 3.0]));
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

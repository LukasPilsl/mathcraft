//! Ill-conditioned linear systems (Hilbert matrices) and false-convergence probes.
//! Hilbert: `H_ij = 1/(i+j+1)`, `b = H * ones`, so the exact root is all ones. Its condition
//! number grows like 1e7 (n = 6), 1e10 (n = 8) and 1e13 (n = 10), so the error is conditioning-
//! limited. `converged` means the constraints hold at the returned point (residual within CTOL,
//! and here the stall rule gives round-off residuals), not that `x` is exact.

use super::{find, solve};
use crate::solve::{Problem, SolveError};

fn hilbert(n: usize) -> Problem<'static> {
    let b: Vec<f64> = (0..n)
        .map(|i| (0..n).map(|j| 1.0 / (i + j + 1) as f64).sum())
        .collect();
    Problem::new(vec![0.0; n]).equations(n, move |x: &[f64]| {
        Ok((0..n)
            .map(|i| (0..n).map(|j| x[j] / (i + j + 1) as f64).sum::<f64>() - b[i])
            .collect())
    })
}

fn max_error_from_ones(x: &[f64]) -> f64 {
    x.iter().map(|v| (v - 1.0).abs()).fold(0.0, f64::max)
}

#[test]
fn hilbert6_converges_tightly() {
    // Measured: residual 0, error 1.7e-10.
    let sol = solve(&hilbert(6));
    assert!(sol.residual < 1e-12, "residual {:e}", sol.residual);
    assert!(max_error_from_ones(&sol.x) < 1e-8, "x = {:?}", sol.x);
}

#[test]
fn hilbert8_converges_to_round_off_residual() {
    // Measured: residual 0, error 1.3e-6 (it was residual 3.5e-16, error 7.5e-8 before the row
    // scales became |r_i| or the Jacobian-based size, which moves the stop to another iterate).
    // The error stays conditioning-limited (condition number about 1e10).
    let sol = solve(&hilbert(8));
    assert!(sol.residual < 1e-12, "residual {:e}", sol.residual);
    assert!(max_error_from_ones(&sol.x) < 1e-5, "x = {:?}", sol.x);
}

#[test]
fn hilbert10_converges_with_small_residual_and_conditioning_limited_error() {
    // Condition number about 1e13. Measured: residual 2.7e-16, error 5e-4. The flag means the
    // constraints are satisfied; the error is set by conditioning, not by the solver.
    let sol = solve(&hilbert(10));
    assert!(sol.residual < 1e-12, "residual {:e}", sol.residual);
    assert!(max_error_from_ones(&sol.x) < 1e-2, "x = {:?}", sol.x);
}

#[test]
fn stall_rule_does_not_accept_atan_beyond_its_range() {
    // atan(x) never reaches 1.6 (its range is (-pi/2, pi/2)): the best residual is 0.03 > CTOL.
    let p = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![v[0].atan() - 1.6]));
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn stall_rule_does_not_accept_a_far_root_as_converged() {
    // 1e-20 x - 1 = 0 has root 1e20. A residual of 1e-12 would still allow x within 1e8 of it,
    // so the point must not be reported as converged on the residual alone.
    let p = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![1e-20 * v[0] - 1.0]));
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn stall_rule_does_not_accept_exp_root_at_1e_minus_300() {
    // exp(x) = 1e-300 has root x = -690.8. Newton creeps down by about 1 per step, so the
    // iteration budget ends first; the tiny residual near x = -700 must not stop it early.
    let p = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![v[0].exp() - 1e-300]));
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

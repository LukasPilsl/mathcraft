//! Scale-invariance and false-convergence probes. A residual of size 1e-15 is not a root unless
//! the point is stationary (`minerr`) or the step test passes (`find`). Expected roots are exact.

use crate::solve::{find, minerr, Problem, SolveError};

/// Relative error with no unit floor, so tiny roots are checked at full relative accuracy.
fn rel_eq(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs()
}

fn linear(scale: f64) -> Problem<'static> {
    // scale * (x - 3) = 0, root x = 3, from 0.
    Problem::new(vec![0.0]).equations(1, move |v: &[f64]| Ok(vec![scale * (v[0] - 3.0)]))
}

#[test]
fn minerr_tiny_equation_reaches_its_root() {
    for scale in [1e-15, 1e-20, 1e15] {
        let sol = minerr(&linear(scale)).unwrap();
        assert!(
            sol.converged,
            "scale {scale:e}: not converged, x = {:e}",
            sol.x[0]
        );
        assert!(
            rel_eq(sol.x[0], 3.0, 1e-9),
            "scale {scale:e}: x = {:e}",
            sol.x[0]
        );
        assert!(
            sol.residual <= 4e-9 * scale.abs(),
            "scale {scale:e}: residual {:e}",
            sol.residual
        );
    }
}

#[test]
fn find_tiny_equation_reaches_its_root() {
    for scale in [1e-15, 1e-20, 1e15] {
        let sol = find(&linear(scale)).unwrap();
        assert!(sol.converged);
        assert!(
            rel_eq(sol.x[0], 3.0, 1e-9),
            "scale {scale:e}: x = {:e}",
            sol.x[0]
        );
    }
}

#[test]
fn find_tiny_equation_on_one_row_of_a_pair() {
    // f1 = (x-1) e-14 has root 1; f2 = y-2 has root 2.
    let p = Problem::new(vec![0.0, 0.0])
        .equations(2, |v: &[f64]| Ok(vec![(v[0] - 1.0) * 1e-14, v[1] - 2.0]));
    let sol = find(&p).unwrap();
    assert!(rel_eq(sol.x[0], 1.0, 1e-9), "x = {:e}", sol.x[0]);
    assert!(rel_eq(sol.x[1], 2.0, 1e-12), "y = {:e}", sol.x[1]);
}

#[test]
fn stalled_points_that_are_not_roots_are_rejected() {
    let atan = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![v[0].atan() - 1.6]));
    assert!(matches!(find(&atan), Err(SolveError::NoConvergence { .. })));
    // minerr: the residual only approaches 0.03 as x grows, so it must not be flagged stationary.
    let fit = minerr(&atan).unwrap();
    assert!(
        !fit.converged,
        "flat tail reported stationary, x = {:e}",
        fit.x[0]
    );
    let sq = Problem::new(vec![0.5]).equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] + 1.0]));
    assert!(matches!(find(&sq), Err(SolveError::NoConvergence { .. })));
    let far = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![1e-20 * v[0] - 1.0]));
    assert!(matches!(find(&far), Err(SolveError::NoConvergence { .. })));
    let exp = Problem::new(vec![0.0]).equations(1, |v: &[f64]| Ok(vec![v[0].exp() - 1e-300]));
    assert!(matches!(find(&exp), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn cubic_with_triple_root_converges_near_one() {
    let p = Problem::new(vec![5.0]).equations(1, |v: &[f64]| Ok(vec![(v[0] - 1.0).powi(3)]));
    let sol = find(&p).unwrap();
    // The triple root is only determined to about residual^(1/3): measured 1.5e-10 error.
    assert!((sol.x[0] - 1.0).abs() < 1e-9, "x = {:e}", sol.x[0]);
}

#[test]
fn scaled_pair_probe_still_gives_the_roots() {
    let p = Problem::new(vec![1e-9, 1e9]).equations(2, |v: &[f64]| {
        Ok(vec![
            1e-12 * ((v[0] * 1e9).powi(2) - 9.0),
            1e12 * (v[1] * 1e-9 - 2.0),
        ])
    });
    let sol = find(&p).unwrap();
    assert!(rel_eq(sol.x[0], 3e-9, 1e-12), "x1 = {:e}", sol.x[0]);
}

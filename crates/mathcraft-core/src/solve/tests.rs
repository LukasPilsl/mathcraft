//! Tests for the solve core. Expected values are hard-coded or derived by hand in the comments.

mod bounds;
mod budget_hook;
mod constraints;
mod errors;
mod illcond;
mod large;
mod linear;
mod minerr_cases;
mod nonlinear;
mod optimize;
mod scaling;
mod stationary;

use super::{find, Problem, Solution};

/// Standard gravity used by the Prime samples (m/s^2).
pub(super) const G: f64 = 9.80665;

/// Relative comparison; absolute below magnitude 1.
pub(super) fn close(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs().max(1.0)
}

pub(super) fn assert_vec_close(got: &[f64], want: &[f64], rel: f64) {
    assert_eq!(got.len(), want.len(), "length: {got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(close(*g, *w, rel), "got {got:?}, want {want:?}");
    }
}

/// Solves with `find` and insists on success.
pub(super) fn solve(problem: &Problem<'_>) -> Solution {
    let sol = find(problem).expect("find should succeed");
    assert!(sol.converged, "find returned converged = false");
    sol
}

/// Projectile-style guess: x = v cos(beta) t, y = v sin(beta) t - g t^2 / 2 (both in SI).
pub(super) fn golf_ball_problem<'a>(beta_deg: f64, guess: Vec<f64>) -> Problem<'a> {
    let b = beta_deg.to_radians();
    Problem::new(guess).equations(2, move |x: &[f64]| {
        let (v, t) = (x[0], x[1]);
        Ok(vec![
            v * b.cos() * t - 240.0,
            v * b.sin() * t - G / 2.0 * t * t,
        ])
    })
}

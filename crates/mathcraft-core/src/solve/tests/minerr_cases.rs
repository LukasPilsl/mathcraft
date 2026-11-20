//! `minerr`: least squares on inconsistent systems, agreement with `find`, and budgets.

use super::{assert_vec_close, close, golf_ball_problem};
use crate::solve::{minerr, Limits, Problem, SolveError};

/// Residuals a + b x_i - y_i for the points (0,1), (1,2), (2,2).
fn line_fit_problem<'a>(guess: Vec<f64>) -> Problem<'a> {
    let xs = [0.0, 1.0, 2.0];
    let ys = [1.0, 2.0, 2.0];
    Problem::new(guess).equations(3, move |p: &[f64]| {
        Ok((0..3).map(|i| p[0] + p[1] * xs[i] - ys[i]).collect())
    })
}

#[test]
fn least_squares_line_fit_matches_normal_equations() {
    // Normal equations: 3a + 3b = 5 and 3a + 5b = 6, so b = 1/2 and a = 7/6.
    // Residuals are 1/6, -1/3, 1/6 and |r|_2 = sqrt(1/6).
    let sol = minerr(&line_fit_problem(vec![0.0, 0.0])).unwrap();
    assert!(sol.converged);
    assert_vec_close(&sol.x, &[7.0 / 6.0, 0.5], 1e-12);
    assert!(
        close(sol.residual, (1.0f64 / 6.0).sqrt(), 1e-12),
        "residual {}",
        sol.residual
    );
}

#[test]
fn least_squares_from_a_different_guess_gives_the_same_point() {
    let a = minerr(&line_fit_problem(vec![10.0, -4.0])).unwrap();
    assert_vec_close(&a.x, &[7.0 / 6.0, 0.5], 1e-12);
}

#[test]
fn minerr_on_a_consistent_square_system_matches_find() {
    let p = Problem::new(vec![1.0]).equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 4.0]));
    let sol = minerr(&p).unwrap();
    assert!(sol.converged);
    assert_vec_close(&sol.x, &[2.0], 1e-12);
    assert!(sol.residual < 1e-12);
}

#[test]
fn minerr_matches_find_on_the_golf_ball_block() {
    let sol = minerr(&golf_ball_problem(40.0, vec![30.0, 3.0])).unwrap();
    assert_vec_close(&sol.x, &[48.88664451593954, 6.408657262978967], 1e-12);
}

#[test]
fn minerr_returns_best_point_when_budget_runs_out() {
    let p = line_fit_problem(vec![10.0, -4.0]).limits(Limits {
        max_iterations: 200,
        max_evaluations: Some(4),
    });
    let sol = minerr(&p).expect("minerr returns the best point, not an error");
    assert!(!sol.converged);
    assert_eq!(sol.x.len(), 2);
    assert!(sol.residual.is_finite());
}

#[test]
fn minerr_reports_non_finite_values() {
    let p = Problem::new(vec![-1.0]).equations(1, |v: &[f64]| Ok(vec![v[0].ln()]));
    assert_eq!(minerr(&p), Err(SolveError::NotFinite));
}

/// Prime help "Example: Using minerr for Nonlinear Least Squares Fitting": Weibull density
/// `a b x^(b-1) exp(-a x^b)` fitted to 12 points. The help shows alpha1 = 0.502 and beta1 = 2
/// (three digits in the image), from guess alpha = 0.8, beta = 1.
#[test]
fn weibull_fit_matches_the_help_example() {
    let xs: [f64; 12] = [
        0.132, 0.322, 0.511, 0.701, 0.891, 1.081, 1.27, 1.46, 1.65, 1.839, 2.029, 2.219,
    ];
    let ys = [
        0.1, 0.258, 0.543, 0.506, 0.606, 0.622, 0.569, 0.453, 0.438, 0.316, 0.29, 0.195,
    ];
    let p = Problem::new(vec![0.8, 1.0]).equations(xs.len(), move |v: &[f64]| {
        let (a, b) = (v[0], v[1]);
        Ok(xs
            .iter()
            .zip(&ys)
            .map(|(x, y): (&f64, &f64)| y - a * b * x.powf(b - 1.0) * (-a * x.powf(b)).exp())
            .collect())
    });
    let sol = minerr(&p).unwrap();
    assert!((sol.x[0] - 0.502).abs() < 5e-4, "alpha {}", sol.x[0]);
    assert!((sol.x[1] - 2.0).abs() < 5e-4, "beta {}", sol.x[1]);
    // ERR is the 2-norm of the residual, so its square is the sum of squares the help mentions.
    assert!(sol.residual > 0.0 && sol.residual < 1.0);
}

/// Residuals `a (x + y - 1)` and `b (x + y - 3)`: inconsistent, since no `s = x + y` makes both
/// zero. Least squares minimises `a^2 (s-1)^2 + b^2 (s-3)^2`, whose minimisers are the line
/// `x + y = 2`. From (0, 0) the nearest one is (1, 1).
fn inconsistent_pair(a: f64, b: f64) -> Problem<'static> {
    Problem::new(vec![0.0, 0.0]).equations(2, move |v: &[f64]| {
        let s = v[0] + v[1];
        Ok(vec![a * (s - 1.0), b * (s - 3.0)])
    })
}

#[test]
fn inconsistent_pair_converges_to_the_nearest_least_squares_point() {
    // Residuals (1, -1) at (1, 1), so ERR = sqrt(2).
    let sol = minerr(&inconsistent_pair(1.0, 1.0)).unwrap();
    assert!(sol.converged, "not converged, x = {:?}", sol.x);
    assert_vec_close(&sol.x, &[1.0, 1.0], 1e-9);
    assert!(
        (sol.residual - 2f64.sqrt()).abs() <= 1e-12,
        "residual {:e}",
        sol.residual
    );
}

#[test]
fn inconsistent_pair_is_converged_when_rows_are_scaled_together() {
    // Scaling every equation by the same constant keeps the minimiser and the stationarity test.
    for c in [1e6, 1e-6] {
        let sol = minerr(&inconsistent_pair(c, c)).unwrap();
        assert!(sol.converged, "scale {c:e}: not converged, x = {:?}", sol.x);
        assert_vec_close(&sol.x, &[1.0, 1.0], 1e-9);
    }
}

#[test]
fn three_equations_two_unknowns_inconsistent_matches_normal_equations() {
    // Residuals x - 1, y - 2, x + y - 4. Normal equations: 2x + y = 5 and x + 2y = 6, so
    // x = 4/3 and y = 7/3. Residuals are 1/3, 1/3, -1/3, so ERR = sqrt(1/3).
    let p = Problem::new(vec![0.0, 0.0]).equations(3, |v: &[f64]| {
        Ok(vec![v[0] - 1.0, v[1] - 2.0, v[0] + v[1] - 4.0])
    });
    let sol = minerr(&p).unwrap();
    assert!(sol.converged, "not converged, x = {:?}", sol.x);
    assert_vec_close(&sol.x, &[4.0 / 3.0, 7.0 / 3.0], 1e-9);
    assert!(
        (sol.residual - (1.0f64 / 3.0).sqrt()).abs() <= 1e-12,
        "residual {:e}",
        sol.residual
    );
}

#[test]
fn three_equations_scaled_together_converge_at_every_scale() {
    // Scaling all residuals by s keeps the minimiser (4/3, 7/3) and the stationarity test.
    for s in [1e-6, 1.0, 1e6, 1e9] {
        let p = Problem::new(vec![0.0, 0.0]).equations(3, move |v: &[f64]| {
            Ok(vec![
                s * (v[0] - 1.0),
                s * (v[1] - 2.0),
                s * (v[0] + v[1] - 4.0),
            ])
        });
        let sol = minerr(&p).unwrap();
        assert!(sol.converged, "scale {s:e}: not converged, x = {:?}", sol.x);
        assert!(
            (sol.x[0] - 4.0 / 3.0).abs() <= 1e-8 * (4.0 / 3.0),
            "scale {s:e}: x = {:?}",
            sol.x
        );
        assert!(
            (sol.x[1] - 7.0 / 3.0).abs() <= 1e-8 * (7.0 / 3.0),
            "scale {s:e}: y = {:?}",
            sol.x
        );
    }
}

#[test]
fn inconsistent_pair_converges_at_scales_one_six_and_one_e_six() {
    // Residuals a (x + y - 1), b (x + y - 3) with a = b = s; the nearest least-squares point is (1, 1).
    for s in [1e-6, 1.0, 1e6] {
        let sol = minerr(&inconsistent_pair(s, s)).unwrap();
        assert!(sol.converged, "scale {s:e}: not converged, x = {:?}", sol.x);
        assert_vec_close(&sol.x, &[1.0, 1.0], 1e-9);
    }
}

//! Nonlinear square systems, scalar roots selected by the guess, and the Prime sample blocks.
//! Prime values are the stored results from documentation/examples (see the accuracy table in
//! the commit report).

use super::{assert_vec_close, golf_ball_problem, solve, G};
use crate::solve::{find, Problem};

#[test]
fn circle_and_diagonal() {
    // x^2 + y^2 = 1 and x = y: solutions (+-1/sqrt2, +-1/sqrt2). Guess in the first quadrant.
    let p = Problem::new(vec![0.5, 0.5]).equations(2, |v: &[f64]| {
        Ok(vec![v[0] * v[0] + v[1] * v[1] - 1.0, v[0] - v[1]])
    });
    let s = std::f64::consts::FRAC_1_SQRT_2;
    assert_vec_close(&solve(&p).x, &[s, s], 1e-12);
}

#[test]
fn rosenbrock_root() {
    // 1 - x = 0 and 10 (y - x^2) = 0: the Rosenbrock valley floor, root (1, 1).
    let p = Problem::new(vec![-1.2, 1.0]).equations(2, |v: &[f64]| {
        Ok(vec![1.0 - v[0], 10.0 * (v[1] - v[0] * v[0])])
    });
    assert_vec_close(&solve(&p).x, &[1.0, 1.0], 1e-12);
}

#[test]
fn trigonometric_system() {
    // sin x = y and cos x = y: x = pi/4 and y = sqrt(2)/2 is the root near the guess.
    let p = Problem::new(vec![0.5, 0.5]).equations(2, |v: &[f64]| {
        Ok(vec![v[0].sin() - v[1], v[0].cos() - v[1]])
    });
    let sol = solve(&p);
    let quarter = std::f64::consts::FRAC_PI_4;
    assert_vec_close(&sol.x, &[quarter, quarter.sin()], 1e-12);
}

#[test]
fn stiff_scaled_system() {
    // 1e4 (x y - 1) = 0 and x + y - 3 = 0: roots (3 -+ sqrt5)/2 with xy = 1.
    // The guess (0.5, 2.5) selects the root with x near 0.38.
    let p = Problem::new(vec![0.5, 2.5]).equations(2, |v: &[f64]| {
        Ok(vec![1e4 * (v[0] * v[1] - 1.0), v[0] + v[1] - 3.0])
    });
    let r5 = 5f64.sqrt();
    assert_vec_close(&solve(&p).x, &[(3.0 - r5) / 2.0, (3.0 + r5) / 2.0], 1e-12);
}

#[test]
fn guess_selects_the_root_of_x_squared_equal_4() {
    let f = |v: &[f64]| Ok(vec![v[0] * v[0] - 4.0]);
    assert_vec_close(
        &solve(&Problem::new(vec![1.0]).equations(1, f)).x,
        &[2.0],
        1e-12,
    );
    assert_vec_close(
        &solve(&Problem::new(vec![-1.0]).equations(1, f)).x,
        &[-2.0],
        1e-12,
    );
}

#[test]
fn projectile_time_of_flight() {
    // Prime help-style: find t with v0 sin(a) t - g t^2 / 2 = 0, nonzero root t = 2 v0 sin(a) / g.
    // v0 = 20 m/s, a = 30 degrees: t = 20 / g. Guess 1.1 s (Prime sample schiefer_wurf).
    let s = 30f64.to_radians().sin();
    let p = Problem::new(vec![1.1]).equations(1, move |t: &[f64]| {
        Ok(vec![20.0 * s * t[0] - G / 2.0 * t[0] * t[0]])
    });
    let sol = solve(&p);
    // Prime stored t1 = 2.039432425955856 s.
    assert_vec_close(&sol.x, &[2.039432425955856], 1e-12);
    assert_vec_close(&sol.x, &[20.0 / G], 1e-12);
}

#[test]
fn projectile_guess_selects_trivial_root() {
    // Prime sample schiefer_wurf_2 (v0 = 22, a = 40 deg) and _3 (v0 = 42, a = 40 deg) store t = 0
    // from guess 1.1 s, although t = 2.8840 and 5.5059 s are also roots. This test pins the
    // behaviour: Newton from 1.1 with backtracking also reaches the zero root.
    for v0 in [22.0, 42.0] {
        let s = 40f64.to_radians().sin();
        let p = Problem::new(vec![1.1]).equations(1, move |t: &[f64]| {
            Ok(vec![v0 * s * t[0] - G / 2.0 * t[0] * t[0]])
        });
        let sol = solve(&p);
        assert!(sol.x[0].abs() < 1e-9, "v0 {v0}: got t = {}", sol.x[0]);
    }
}

#[test]
fn golf_ball_sample_beta_40() {
    // Prime stored (17_rata_golfball_donau_lsg): v = 48.88664451593954 m/s, t = 6.408657262978967 s.
    let sol = solve(&golf_ball_problem(40.0, vec![30.0, 3.0]));
    assert_vec_close(&sol.x, &[48.88664451593954, 6.408657262978967], 1e-12);
}

#[test]
fn golf_ball_sample_beta_45() {
    // Prime stored (17_rata_golfball_donau_lsg_2): v = 48.5138743041617, t = 6.996168824645425.
    let sol = solve(&golf_ball_problem(45.0, vec![40.0, 3.0]));
    assert_vec_close(&sol.x, &[48.5138743041617, 6.996168824645425], 1e-12);
}

#[test]
fn nonlinear_solution_residual_is_at_round_off() {
    let sol = solve(&golf_ball_problem(40.0, vec![30.0, 3.0]));
    assert!(sol.residual < 1e-12, "residual {}", sol.residual);
    assert!(sol.iterations <= 20);
}

#[test]
fn find_matches_for_equal_problems_with_bounds() {
    // Bounds that contain the root must not change the answer.
    let p = Problem::new(vec![1.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 4.0]))
        .bounds(vec![0.0], vec![10.0]);
    assert_vec_close(&find(&p).unwrap().x, &[2.0], 1e-12);
}

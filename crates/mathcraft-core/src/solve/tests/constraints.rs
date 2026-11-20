//! Inequality constraints, CTOL and bounds, plus the Prime sample "Gerade schneidet Kreis".

use super::{assert_vec_close, solve, G};
use crate::solve::{find, Limits, Problem, SolveError};

fn circle_line_problem<'a>(guess: Vec<f64>) -> Problem<'a> {
    // x^2 + y^2 = 6 and x + y = 2 (Prime help: intersection of a circle and a line).
    Problem::new(guess).equations(2, |v: &[f64]| {
        Ok(vec![v[0] * v[0] + v[1] * v[1] - 6.0, v[0] + v[1] - 2.0])
    })
}

#[test]
fn circle_line_without_inequality_follows_guess() {
    // Guess (-1, 1) gives (1 - sqrt2, 1 + sqrt2), the Prime help result.
    let s = 2f64.sqrt();
    assert_vec_close(
        &solve(&circle_line_problem(vec![-1.0, 1.0])).x,
        &[1.0 - s, 1.0 + s],
        1e-12,
    );
}

#[test]
fn circle_line_guess_on_singular_jacobian_returns_a_root() {
    // Guess (1, 1) lies on the symmetric line where the Jacobian is singular, and both
    // intersections are equally close. Prime's help returns (1 + sqrt2, 1 - sqrt2) there; this
    // solver returns the other root. Both satisfy the system; the difference is root selection.
    let s = 2f64.sqrt();
    let sol = solve(&circle_line_problem(vec![1.0, 1.0]));
    let (x, y) = (sol.x[0], sol.x[1]);
    let is_root_x = |px: f64| (px - (1.0 - s)).abs() < 1e-12 || (px - (1.0 + s)).abs() < 1e-12;
    assert!(is_root_x(x), "got {:?}", sol.x);
    assert!((x + y - 2.0).abs() < 1e-12 && (x * x + y * y - 6.0).abs() < 1e-12);
}

#[test]
fn circle_line_with_x_greater_than_2_selects_the_right_point() {
    // Adding x > 2 (as x - 2 >= 0) removes the point (1 - sqrt2, 1 + sqrt2) from the guess (-1, 1).
    let s = 2f64.sqrt();
    let p = circle_line_problem(vec![-1.0, 1.0]).inequalities(1, |v: &[f64]| Ok(vec![v[0] - 2.0]));
    let sol = solve(&p);
    assert_vec_close(&sol.x, &[1.0 + s, 1.0 - s], 1e-12);
}

#[test]
fn exp_root_with_ctol_inequality() {
    // Prime help (CTOL example): exp(x) = 2 and x < 0.693 (as 0.693 - x >= 0), guess 2.
    // With CTOL = 1e-3 the inequality may be violated by 1e-3, so the exact root ln 2 is accepted.
    let p = Problem::new(vec![2.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0].exp() - 2.0]))
        .inequalities(1, |v: &[f64]| Ok(vec![0.693 - v[0]]));
    let sol = solve(&p);
    assert_vec_close(&sol.x, &[std::f64::consts::LN_2], 1e-12);
}

#[test]
fn tighter_ctol_rejects_the_same_problem() {
    // With CTOL = 1e-4 the root ln 2 violates x < 0.693 by 1.5e-4, and no point satisfies both
    // constraints within 1e-4, so find must report failure, as Prime does (help: error).
    let p = Problem::new(vec![2.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0].exp() - 2.0]))
        .inequalities(1, |v: &[f64]| Ok(vec![0.693 - v[0]]))
        .ctol(1e-4);
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn infeasible_inequality_is_an_error() {
    // x - 1 = 0 forces x = 1, but x >= 2 is required.
    let p = Problem::new(vec![0.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] - 1.0]))
        .inequalities(1, |v: &[f64]| Ok(vec![v[0] - 2.0]));
    assert!(find(&p).is_err());
}

#[test]
fn gerade_sample_block_with_y_less_than_0() {
    // Prime sample 5_rata_4_Gerade (block 1): x^2 + y^2 = 28, y = 3 - 2x, y < 0, guess (1, -1).
    // Stored: (3.489104628451919, -3.978209256903839). Exact: x = (12 + sqrt524)/10.
    let p = Problem::new(vec![1.0, -1.0])
        .equations(2, |v: &[f64]| {
            Ok(vec![
                v[0] * v[0] + v[1] * v[1] - 28.0,
                v[1] - (3.0 - 2.0 * v[0]),
            ])
        })
        .inequalities(1, |v: &[f64]| Ok(vec![-v[1]]));
    let sol = solve(&p);
    assert_vec_close(&sol.x, &[3.489104628451919, -3.978209256903839], 1e-12);
}

#[test]
fn gerade_sample_block_with_y_greater_than_0() {
    // Same circle and line, y > 0, guess (-1, 1). Stored: (-1.089104628451919, 5.178209256903839).
    let p = Problem::new(vec![-1.0, 1.0])
        .equations(2, |v: &[f64]| {
            Ok(vec![
                v[0] * v[0] + v[1] * v[1] - 28.0,
                v[1] - (3.0 - 2.0 * v[0]),
            ])
        })
        .inequalities(1, |v: &[f64]| Ok(vec![v[1]]));
    let sol = solve(&p);
    assert_vec_close(&sol.x, &[-1.089104628451919, 5.178209256903839], 1e-12);
}

#[test]
fn bounds_project_the_iterates() {
    // x^2 = 4 with 0 <= x <= 10 from guess 1 converges to 2.
    let p = Problem::new(vec![1.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 4.0]))
        .bounds(vec![0.0], vec![10.0]);
    assert_vec_close(&solve(&p).x, &[2.0], 1e-12);
}

#[test]
fn tolerances_are_respected() {
    // A loose TOL stops no later than a tight one, and both reach the same root.
    let mk = |tol: f64| {
        Problem::new(vec![30.0, 3.0])
            .equations(2, move |v: &[f64]| {
                let b = 40f64.to_radians();
                Ok(vec![
                    v[0] * b.cos() * v[1] - 240.0,
                    v[0] * b.sin() * v[1] - G / 2.0 * v[1] * v[1],
                ])
            })
            .tol(tol)
    };
    let loose = solve(&mk(1e-3));
    let tight = solve(&mk(1e-12));
    assert!(loose.iterations <= tight.iterations);
    assert_vec_close(&loose.x, &tight.x, 1e-9);
}

#[test]
fn iteration_limit_is_enforced() {
    let p = Problem::new(vec![10.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] * v[0] - 2.0]))
        .limits(Limits {
            max_iterations: 1,
            max_evaluations: Some(100_000),
        });
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn circle_line_tie_is_a_known_gap() {
    // Guess (1, 1) sits where the gradients of x^2 + y^2 and x + y are parallel, so the Jacobian is
    // singular. This solver takes the root on the side of round-off: (-0.414, 2.414). The Prime
    // help shows (2.414, -0.414) for this guess; the rule is not reproduced. A guess a little
    // off the tie, (1, 0.99), gives (2.414, -0.414). This test pins the current behaviour.
    let s = 2f64.sqrt();
    assert_vec_close(
        &solve(&circle_line_problem(vec![1.0, 1.0])).x,
        &[1.0 - s, 1.0 + s],
        1e-12,
    );
    assert_vec_close(
        &solve(&circle_line_problem(vec![1.0, 0.99])).x,
        &[1.0 + s, 1.0 - s],
        1e-12,
    );
}

//! Large well-posed square systems: the automatic budget must not starve them. The system is a
//! tri-diagonal cubic, `f_i = 2 x_i - x_{i-1} - x_{i+1} + x_i^3 - b_i` with `x_0 = x_{n+1} = 0`,
//! whose root is all ones (`b = 1` inside, `b_1 = b_n = 2`). The guess is 0.2 everywhere. The
//! n = 200 case runs in every test pass. The larger ones are slow in debug and are `#[ignore]`d.

use crate::solve::problem::Evaluator;
use crate::solve::{auto_iterations, find, Limits, Problem, Solution, SolveError};

fn tri_cubic(n: usize) -> Problem<'static> {
    let b: Vec<f64> = (0..n)
        .map(|i| if i == 0 || i + 1 == n { 2.0 } else { 1.0 })
        .collect();
    Problem::new(vec![0.2; n]).equations(n, move |x: &[f64]| {
        Ok((0..n)
            .map(|i| {
                let left = if i > 0 { x[i - 1] } else { 0.0 };
                let right = if i + 1 < n { x[i + 1] } else { 0.0 };
                2.0 * x[i] - left - right + x[i] * x[i] * x[i] - b[i]
            })
            .collect())
    })
}

fn max_error_from_ones(sol: &Solution) -> f64 {
    sol.x.iter().map(|v| (v - 1.0).abs()).fold(0.0, f64::max)
}

#[test]
fn tri_cubic_200_converges_in_at_most_ten_iterations() {
    let sol = find(&tri_cubic(200)).expect("find should succeed");
    assert!(sol.converged);
    assert!(sol.iterations <= 10, "iterations {}", sol.iterations);
    assert!(
        max_error_from_ones(&sol) < 1e-9,
        "max error {:e}",
        max_error_from_ones(&sol)
    );
}

#[test]
#[ignore = "slow: run with --release --ignored; n = 1000 is about 0.1 s per iteration in release"]
fn tri_cubic_1000_converges_within_the_automatic_budget() {
    // Newton stalls at round-off (step 4e-12, one row about 4e-10 above the 1e-12 floor), and the
    // stall rule accepts it. The automatic cap is max(25, 7e9 / n^2) = 25 iterations per start.
    let sol = find(&tri_cubic(1000)).expect("find should succeed");
    assert!(sol.iterations <= 25, "iterations {}", sol.iterations);
    assert!(
        max_error_from_ones(&sol) < 1e-9,
        "max error {:e}",
        max_error_from_ones(&sol)
    );
}

#[test]
#[ignore = "slow: run with --release --ignored"]
fn tri_cubic_1200_converges_within_the_automatic_budget() {
    let sol = find(&tri_cubic(1200)).expect("find should succeed");
    assert!(
        max_error_from_ones(&sol) < 1e-9,
        "max error {:e}",
        max_error_from_ones(&sol)
    );
}

#[test]
fn tri_cubic_with_an_explicit_budget_is_used_as_given() {
    // An explicit iteration count is not replaced by the automatic one: 2 Newton steps cannot
    // reach the root from 0.2 (about 4 are needed at n = 200).
    let p = tri_cubic(200).limits(Limits {
        max_iterations: 2,
        max_evaluations: Some(10_000_000),
    });
    assert!(matches!(find(&p), Err(SolveError::NoConvergence { .. })));
}

#[test]
fn automatic_iteration_cap_has_a_floor_of_25() {
    // WORK_CAP / (n^2 rows) is 7 at n = 1000 and 4.9 at n = 1200; the floor keeps 25.
    assert_eq!(auto_iterations(1000, 1000, 200), 25);
    assert_eq!(auto_iterations(1200, 1200, 200), 25);
    assert_eq!(auto_iterations(10, 1, 200), 200);
    // An explicit smaller limit is still the upper bound.
    assert_eq!(auto_iterations(1000, 1000, 10), 10);
}

#[test]
fn each_start_gets_its_own_iteration_allowance() {
    // The allowance resets per start (begin_run), while the reported total keeps counting.
    let p = Problem::new(vec![0.0])
        .equations(1, |v: &[f64]| Ok(vec![v[0] - 1.0]))
        .limits(Limits {
            max_iterations: 3,
            max_evaluations: Some(1000),
        });
    let mut ev = Evaluator::new(&p, 1, 1, 0, 1e-12);
    ev.begin_run();
    for _ in 0..3 {
        assert!(ev.iterate().unwrap());
    }
    assert!(
        !ev.iterate().unwrap(),
        "the first start is out of iterations"
    );
    ev.begin_run();
    assert!(
        ev.iterate().unwrap(),
        "the second start has its own allowance"
    );
    assert_eq!(ev.iterations, 4);
}

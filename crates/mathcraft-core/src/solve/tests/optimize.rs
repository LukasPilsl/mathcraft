//! Tests for `optimize` (minimisation with constraints and bounds). Expected values are the exact
//! minima, derived by hand in each comment.

use crate::solve::{optimize, Problem, SolveError};

use super::{assert_vec_close, close};

fn rosenbrock(x: &[f64]) -> f64 {
    (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2)
}

#[test]
fn rosenbrock_unconstrained_reaches_1_1() {
    // Minimum (1, 1) with value 0.
    let p = Problem::new(vec![-1.2, 1.0]).objective(|x| Ok(rosenbrock(x)));
    let o = optimize(&p).expect("rosenbrock converges");
    assert_vec_close(&o.x, &[1.0, 1.0], 1e-6);
    assert!(o.value < 1e-12, "value {}", o.value);
}

#[test]
fn quadratic_with_linear_equality() {
    // min (x-3)^2 + (y-2)^2 with x + y = 1: projection of (3, 2) onto the line is (1, 0), value 8.
    let p = Problem::new(vec![0.0, 0.0])
        .objective(|x| Ok((x[0] - 3.0).powi(2) + (x[1] - 2.0).powi(2)))
        .equations(1, |x| Ok(vec![x[0] + x[1] - 1.0]));
    let o = optimize(&p).expect("linear equality converges");
    assert_vec_close(&o.x, &[1.0, 0.0], 1e-6);
    assert!(close(o.value, 8.0, 1e-9), "value {}", o.value);
}

#[test]
fn linear_objective_on_circle() {
    // min x + y with x^2 + y^2 = 1: the minimum is (-1, -1)/sqrt(2), value -sqrt(2). The search is
    // local: from (-0.5, -0.5) it reaches the minimum.
    let p = Problem::new(vec![-0.5, -0.5])
        .objective(|x| Ok(x[0] + x[1]))
        .equations(1, |x| Ok(vec![x[0] * x[0] + x[1] * x[1] - 1.0]));
    let o = optimize(&p).expect("circle converges");
    let r = 1.0 / 2f64.sqrt();
    assert_vec_close(&o.x, &[-r, -r], 1e-6);
    assert!(close(o.value, -2f64.sqrt(), 1e-8), "value {}", o.value);

    // From (0.5, 0.5) the penalty pulls outward and the same search settles on the other
    // stationary point of the circle, (1, 1)/sqrt(2), which is the maximum of x + y.
    let p = Problem::new(vec![0.5, 0.5])
        .objective(|x| Ok(x[0] + x[1]))
        .equations(1, |x| Ok(vec![x[0] * x[0] + x[1] * x[1] - 1.0]));
    let o = optimize(&p).expect("circle converges from the other side");
    assert_vec_close(&o.x, &[r, r], 1e-6);
}

#[test]
fn inequality_from_infeasible_guess() {
    // min x^2 + y^2 with x + y >= 2: the projection of the origin onto x + y = 2 is (1, 1), value 2.
    let p = Problem::new(vec![0.0, 0.0])
        .objective(|x| Ok(x[0] * x[0] + x[1] * x[1]))
        .inequalities(1, |x| Ok(vec![x[0] + x[1] - 2.0]));
    let o = optimize(&p).expect("inequality converges");
    assert_vec_close(&o.x, &[1.0, 1.0], 1e-6);
    assert!(close(o.value, 2.0, 1e-9), "value {}", o.value);
}

#[test]
fn inactive_inequality_does_not_move_the_optimum() {
    // min (x-1)^2 with x >= -5: the minimum x = 1 is feasible, so the constraint is inactive.
    let p = Problem::new(vec![3.0])
        .objective(|x| Ok((x[0] - 1.0).powi(2)))
        .inequalities(1, |x| Ok(vec![x[0] + 5.0]));
    let o = optimize(&p).expect("inactive inequality");
    assert!(close(o.x[0], 1.0, 1e-6), "x {:?}", o.x);
}

#[test]
fn bounds_clamp_the_minimum() {
    // min (x-5)^2 with 0 <= x <= 3: the minimum is on the upper bound, x = 3, value 4.
    let p = Problem::new(vec![1.0])
        .objective(|x| Ok((x[0] - 5.0).powi(2)))
        .bounds(vec![0.0], vec![3.0]);
    let o = optimize(&p).expect("bounded converges");
    assert!(close(o.x[0], 3.0, 1e-7), "x {:?}", o.x);
    assert!(close(o.value, 4.0, 1e-8), "value {}", o.value);
}

#[test]
fn one_sided_bound_with_infinite_side() {
    // min (x+1)^2 with x >= 0 (upper bound infinite): the minimum is on the lower bound, x = 0.
    let p = Problem::new(vec![2.0])
        .objective(|x| Ok((x[0] + 1.0).powi(2)))
        .bounds(vec![0.0], vec![f64::INFINITY]);
    let o = optimize(&p).expect("one-sided bound converges");
    assert!(close(o.x[0], 0.0, 1e-7), "x {:?}", o.x);
}

#[test]
fn contradictory_inequalities_are_infeasible() {
    // x >= 1 and x <= 0 cannot both hold.
    let p = Problem::new(vec![0.5])
        .objective(|x| Ok(x[0] * x[0]))
        .inequalities(2, |x| Ok(vec![x[0] - 1.0, -x[0]]));
    match optimize(&p) {
        Err(SolveError::Infeasible { .. }) | Err(SolveError::NoConvergence { .. }) => {}
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn linear_objective_without_bounds_is_unbounded() {
    // min -x has no minimum: the search runs away and must be refused.
    let p = Problem::new(vec![0.0]).objective(|x| Ok(-x[0]));
    assert!(optimize(&p).is_err(), "unbounded problem must be an error");
}

#[test]
fn exponential_objective_is_unbounded() {
    // min -exp(x) decreases without bound as x grows.
    let p = Problem::new(vec![0.0]).objective(|x| Ok(-x[0].exp()));
    assert!(optimize(&p).is_err(), "unbounded problem must be an error");
}

#[test]
fn iteration_hook_can_stop_the_search() {
    let calls = std::cell::Cell::new(0usize);
    let p = Problem::new(vec![-1.2, 1.0])
        .objective(|x| Ok(rosenbrock(x)))
        .on_iteration(|_| {
            calls.set(calls.get() + 1);
            if calls.get() > 3 {
                Err(SolveError::BudgetExceeded)
            } else {
                Ok(())
            }
        });
    assert_eq!(optimize(&p).unwrap_err(), SolveError::BudgetExceeded);
    assert_eq!(calls.get(), 4);
}

#[test]
fn objective_error_is_not_swallowed() {
    let p = Problem::new(vec![1.0]).objective(|_| Err(SolveError::Unsupported("boom".into())));
    assert_eq!(
        optimize(&p).unwrap_err(),
        SolveError::Unsupported("boom".into())
    );
}

#[test]
fn nan_objective_is_an_error() {
    let p = Problem::new(vec![1.0]).objective(|_| Ok(f64::NAN));
    assert_eq!(optimize(&p).unwrap_err(), SolveError::NotFinite);
}

#[test]
fn unbounded_problems_are_errors_not_huge_finite_values() {
    // min -x^2 runs away from 1; max x^2 + 1 (the min of -(x^2 + 1)) runs away from 3. Each must be
    // refused, not accepted at x ~ 1e9 where the gradient is small relative to the value.
    let p = Problem::new(vec![1.0]).objective(|x| Ok(-x[0] * x[0]));
    assert!(optimize(&p).is_err(), "min -x^2 must be unbounded");
    let p = Problem::new(vec![3.0]).objective(|x| Ok(-(x[0] * x[0] + 1.0)));
    assert!(optimize(&p).is_err(), "max x^2 + 1 must be unbounded");
}

#[test]
fn small_magnitude_objective_does_not_converge_at_the_guess() {
    // min 1e-12 (x - 3)^2 from 0: the value at the guess is tiny, but the minimum is x = 3.
    let p = Problem::new(vec![0.0]).objective(|x| Ok(1e-12 * (x[0] - 3.0).powi(2)));
    let o = optimize(&p).expect("converges");
    assert!(close(o.x[0], 3.0, 1e-6), "x {:?}", o.x);
}

#[test]
fn stationary_maximum_at_the_guess_is_not_returned_as_a_minimum() {
    // min x^4 - x^2 from 0: 0 is a maximum (Hessian -2), so the search restarts from a point pushed
    // off it. The minima are x = ±1/sqrt(2), value -1/4.
    let p = Problem::new(vec![0.0]).objective(|x| Ok(x[0].powi(4) - x[0] * x[0]));
    let o = optimize(&p).expect("converges to a minimum");
    assert!(close(o.x[0].abs(), 1.0 / 2f64.sqrt(), 1e-6), "x {:?}", o.x);
    assert!(close(o.value, -0.25, 1e-9), "value {}", o.value);
    // min -x^2 from 0 has only the maximum at 0 and no minimum: refused, not returned as 0.
    let p = Problem::new(vec![0.0]).objective(|x| Ok(-x[0] * x[0]));
    assert!(optimize(&p).is_err(), "a maximum is not a minimum");
}

#[test]
fn ctol_below_the_penalty_floor_is_refused_not_returned() {
    // x^2 == 2 from 1 with CTOL 1e-15: the penalty search stalls at a violation of about 1.6e-10,
    // above the clamped target 1e-13, so the solve is refused as not converged (never a value that
    // misses the constraint). With the default CTOL the same equation converges.
    let p = Problem::new(vec![1.0])
        .objective(|x| Ok(x[0]))
        .equations(1, |x| Ok(vec![x[0] * x[0] - 2.0]))
        .ctol(1e-15);
    assert!(
        matches!(optimize(&p), Err(SolveError::NoConvergence { .. })),
        "{:?}",
        optimize(&p)
    );
    let p = Problem::new(vec![1.0])
        .objective(|x| Ok(x[0]))
        .equations(1, |x| Ok(vec![x[0] * x[0] - 2.0]));
    let o = optimize(&p).expect("the equation converges at the default CTOL");
    assert!(close(o.x[0], 2f64.sqrt(), 1e-8), "x {:?}", o.x);
}

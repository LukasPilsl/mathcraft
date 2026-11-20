//! Bounds: every point the solver evaluates must lie inside `[lo, hi]`, including the guess.
//! The guess is clamped before any evaluation, so a guess outside the bounds is never evaluated.

use std::cell::RefCell;

use crate::solve::{find, minerr, Limits, Problem};

const LO: f64 = 0.0;
const HI: f64 = 10.0;

/// `x^2 - 4` with bounds `[0, 10]`; with `ineq`, also `x - 1 >= 0`. Every call logs `x`.
fn logged<'a>(guess: f64, log: &'a RefCell<Vec<f64>>, ineq: bool) -> Problem<'a> {
    let p = Problem::new(vec![guess])
        .equations(1, move |v: &[f64]| {
            log.borrow_mut().push(v[0]);
            Ok(vec![v[0] * v[0] - 4.0])
        })
        .bounds(vec![LO], vec![HI]);
    if ineq {
        p.inequalities(1, move |v: &[f64]| {
            log.borrow_mut().push(v[0]);
            Ok(vec![v[0] - 1.0])
        })
    } else {
        p
    }
}

fn assert_inside(log: &RefCell<Vec<f64>>) {
    let pts = log.borrow();
    assert!(!pts.is_empty(), "no evaluations recorded");
    let min = pts.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = pts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!(
        min >= LO && max <= HI,
        "evaluated outside bounds: min {min}, max {max}"
    );
}

#[test]
fn find_never_evaluates_outside_bounds() {
    for ineq in [false, true] {
        for guess in [20.0, -5.0, 1.0] {
            let log = RefCell::new(Vec::new());
            let sol = find(&logged(guess, &log, ineq)).expect("find should succeed");
            assert_inside(&log);
            assert!(sol.x[0] >= LO && sol.x[0] <= HI, "x = {:?}", sol.x);
            assert!((sol.x[0] - 2.0).abs() < 1e-9, "x = {:?}", sol.x);
        }
    }
}

#[test]
fn minerr_never_evaluates_outside_bounds() {
    for ineq in [false, true] {
        for guess in [20.0, -5.0, 1.0] {
            let log = RefCell::new(Vec::new());
            let _ = minerr(&logged(guess, &log, ineq)).expect("minerr should not fail");
            assert_inside(&log);
        }
    }
}

#[test]
fn minerr_with_one_evaluation_returns_the_clamped_guess() {
    // Guess 20 is clamped to 10 before the only evaluation: residual 100 - 4 = 96.
    let limits = Limits {
        max_iterations: 200,
        max_evaluations: Some(1),
    };
    let log = RefCell::new(Vec::new());
    let sol = minerr(&logged(20.0, &log, false).limits(limits)).unwrap();
    assert_eq!(sol.x, vec![10.0]);
    assert!(!sol.converged);
    assert_eq!(*log.borrow(), vec![10.0]);

    let log = RefCell::new(Vec::new());
    let sol = minerr(&logged(-5.0, &log, false).limits(limits)).unwrap();
    assert_eq!(sol.x, vec![0.0]);
    assert_eq!(*log.borrow(), vec![0.0]);
}

//! The iteration hook: called once per outer iteration with the problem size, so a caller can charge the
//! solver's own work and stop the solve when its budget is spent.

use std::cell::{Cell, RefCell};

use crate::solve::{find, Problem, SolveError};

/// The n = 1000 unsolvable problem `sum x_i^2 + 1 = 0`: no real root, so the solve runs to its
/// iteration allowance unless the hook stops it.
fn unsolvable(n: usize) -> Problem<'static> {
    Problem::new(vec![0.0; n]).equations(1, |x: &[f64]| {
        Ok(vec![x.iter().map(|v| v * v).sum::<f64>() + 1.0])
    })
}

#[test]
fn hook_is_called_once_per_iteration_with_the_problem_size() {
    let seen = RefCell::new(Vec::new());
    let problem = unsolvable(20).on_iteration(|info| {
        seen.borrow_mut()
            .push((info.n, info.m, info.k, info.iteration));
        Ok(())
    });
    let _ = find(&problem);
    let seen = seen.borrow();
    assert!(!seen.is_empty());
    for (i, (n, m, k, it)) in seen.iter().enumerate() {
        assert_eq!((*n, *m, *k), (20, 1, 0));
        assert_eq!(*it, i + 1, "iterations are numbered from 1");
    }
}

#[test]
fn hook_error_aborts_the_solve_at_the_iteration_it_refuses() {
    // n = 1000, one row: each iteration is charged the 1 x 1000 elimination, 1000 * 1 * 1 / 21 =
    // 47 units (the charge formula is the caller's; 47 is used here). With 500 units available the
    // 11th iteration (517 units in all) is the first refused, so the hook runs 11 times.
    let spent = Cell::new(0u64);
    let calls = Cell::new(0usize);
    let problem = unsolvable(1000).on_iteration(|_| {
        calls.set(calls.get() + 1);
        let next = spent.get() + 47;
        if next > 500 {
            return Err(SolveError::BudgetExceeded);
        }
        spent.set(next);
        Ok(())
    });
    let got = find(&problem);
    assert_eq!(got, Err(SolveError::BudgetExceeded));
    assert_eq!(calls.get(), 11, "refused on the 11th iteration");
    assert_eq!(spent.get(), 470, "10 iterations of 47 units were charged");
}

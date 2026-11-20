//! Linear systems whose consistency depends on parameters (leftover rows with symbolic
//! right-hand sides).

use super::{eqn, names};
use crate::symbolic::{solve_linear_system, SolveOutcome, SymError};

fn solve(eqs: &[&str], unknowns: &[&str]) -> Result<SolveOutcome, SymError> {
    let system: Vec<_> = eqs.iter().map(|e| eqn(e)).collect();
    solve_linear_system(&system, &names(unknowns))
}

#[test]
fn symbolic_leftover_rhs_is_refused_not_reported_as_no_solution() {
    // Consistent only when a = b: the answer depends on the parameters.
    assert_eq!(
        solve(&["x+y==a", "x+y==b"], &["x", "y"]),
        Err(SymError::Unsupported(
            "consistency depends on parameters".into()
        ))
    );
}

#[test]
fn constant_leftover_rhs_is_no_solution() {
    assert_eq!(
        solve(&["x+y==1", "x+y==2"], &["x", "y"]),
        Ok(SolveOutcome::NoSolution)
    );
}

#[test]
fn identical_symbolic_rows_are_infinitely_many_solutions() {
    match solve(&["x+y==a", "x+y==a"], &["x", "y"]) {
        Ok(SolveOutcome::Infinite { free, .. }) => assert_eq!(free, names(&["y"])),
        other => panic!("expected Infinite, got {other:?}"),
    }
}

#[test]
fn constant_contradiction_wins_over_a_parametric_one() {
    // x+y=a, x+y=a+1 leaves 0 = 1 whatever a is: a definite contradiction.
    assert_eq!(
        solve(&["x+y==a", "x+y==a+1"], &["x", "y"]),
        Ok(SolveOutcome::NoSolution)
    );
}

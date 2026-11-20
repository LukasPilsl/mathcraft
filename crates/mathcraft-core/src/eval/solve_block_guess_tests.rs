//! Tests for solve blocks with failing guesses and the shared call budget. Expected values are
//! worked out by hand in the comments.

use super::solve_block_tests::{close, constraint, def, def_fn, guess, num, scalar_of, solver, sq};
use super::work::MAX_WORK;
use super::{EvalError, Outcome, Session, MAX_CALLS};
use crate::expr::{BinaryOp, Expr};

/// Defines `f0(x) := x` and `f1..fk`, where `fj(x)` calls `f(j-1)(x)` twice: `fk` costs 2^k calls.
fn define_call_chain(s: &mut Session, k: usize) {
    s.run(&def_fn("f0", vec![Expr::var("x")], Expr::var("x")));
    for j in 1..=k {
        let inner = format!("f{}", j - 1);
        let body = Expr::binary(
            BinaryOp::Add,
            Expr::call(&inner, vec![Expr::var("x")]),
            Expr::call(&inner, vec![Expr::var("x")]),
        );
        s.run(&def_fn(&format!("f{j}"), vec![Expr::var("x")], body));
    }
}

#[test]
fn guess_that_divides_by_zero_is_reported_by_find() {
    // x := 1/0 fails with its own error; find reports that, not Undefined(x).
    let mut s = Session::new();
    let div0 = Expr::binary(BinaryOp::Div, num(1.0), num(0.0));
    let out = s.run_solve_block(&[
        guess("x", div0),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert_eq!(out[0], Outcome::Error(EvalError::DivisionByZero));
    assert_eq!(out[2], Outcome::Error(EvalError::DivisionByZero));
}

#[test]
fn guess_that_overflows_is_reported_by_find() {
    // x := 10^400 is not finite: find reports the guess's own error, the same one as the guess.
    let mut s = Session::new();
    let huge = Expr::binary(BinaryOp::Pow, num(10.0), num(400.0));
    let out = s.run_solve_block(&[
        guess("x", huge),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(matches!(out[0], Outcome::Error(_)), "{out:?}");
    assert_eq!(out[2], out[0], "{out:?}");
}

#[test]
fn failed_guess_then_good_redefinition_solves() {
    // Last definition wins: a broken x := 1/0 followed by x := 1 inside the block still solves.
    let mut s = Session::new();
    let div0 = Expr::binary(BinaryOp::Div, num(1.0), num(0.0));
    let out = s.run_solve_block(&[
        guess("x", div0),
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(matches!(out[3], Outcome::Defined { .. }), "{out:?}");
    assert!(close(scalar_of(&s, "r"), 2.0, 1e-9));
}

#[test]
fn call_budget_is_shared_by_all_solver_members_of_a_block() {
    // f15 costs 2^15 = 32768 calls per evaluation and a solve takes about 24 evaluations here, so
    // one find uses about 786k of MAX_CALLS (1M). The first find fits; the second, sharing the
    // same budget, has only about 214k left and stops with the budget error. With a budget per
    // member, both would succeed.
    let mut s = Session::new();
    define_call_chain(&mut s, 15);
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(
            BinaryOp::Eq,
            Expr::call("f15", vec![Expr::var("x")]),
            num(0.0),
        ),
        solver(Some(Expr::var("a")), "find", &["x"]),
        solver(Some(Expr::var("b")), "find", &["x"]),
    ]);
    assert!(matches!(out[2], Outcome::Defined { .. }), "{out:?}");
    assert_eq!(out[3], Outcome::Error(EvalError::CallLimit(MAX_CALLS)));
}

#[test]
fn nested_find_in_a_constraint_is_not_supported() {
    // A solver call inside a constraint is a nested solve block, with its own message.
    let mut s = Session::new();
    let nested = Expr::call("find", vec![Expr::var("x")]);
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), nested),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert_eq!(
        out[2],
        Outcome::Error(EvalError::Unsupported(
            "nested solve blocks are not supported".into()
        ))
    );
}

#[test]
fn outer_definition_serves_as_the_guess_when_the_block_has_none() {
    // `x := 1.5` above the block is the guess for x^2 == 4 (no guess inside the block).
    let mut s = Session::new();
    s.run(&def("x", num(1.5)));
    s.run_solve_block(&[
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(close(scalar_of(&s, "r"), 2.0, 1e-9));
}

#[test]
fn broken_guess_of_another_name_does_not_fail_the_solve() {
    // x := 1/0 is not an unknown of find(y), so it does not stop y^2 == 4 (y = 2).
    let mut s = Session::new();
    let div0 = Expr::binary(BinaryOp::Div, num(1.0), num(0.0));
    let out = s.run_solve_block(&[
        guess("x", div0),
        guess("y", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("y")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["y"]),
    ]);
    assert_eq!(out[0], Outcome::Error(EvalError::DivisionByZero));
    assert!(matches!(out[3], Outcome::Defined { .. }), "{out:?}");
    assert!(close(scalar_of(&s, "r"), 2.0, 1e-9));
}

#[test]
fn broken_guess_of_another_name_leaves_an_unguessed_unknown_undefined() {
    // y has no guess in the block, and the only broken guess is for x: find(y) reports Undefined(y).
    let mut s = Session::new();
    let div0 = Expr::binary(BinaryOp::Div, num(1.0), num(0.0));
    let out = s.run_solve_block(&[
        guess("x", div0),
        constraint(BinaryOp::Eq, sq(Expr::var("y")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["y"]),
    ]);
    assert_eq!(
        out[2],
        Outcome::Error(EvalError::Undefined("y".to_string()))
    );
}

#[test]
fn broken_guess_of_one_of_two_unknowns_is_the_error_reported() {
    // x has a good guess, y's guess divides by zero: find(x, y) reports y's own error.
    let mut s = Session::new();
    let div0 = Expr::binary(BinaryOp::Div, num(1.0), num(0.0));
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        guess("y", div0),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("y")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x", "y"]),
    ]);
    assert_eq!(out[4], Outcome::Error(EvalError::DivisionByZero));
}

#[test]
fn work_budget_is_shared_by_a_solve_block_and_refuses_its_later_closure_calls() {
    // Each evaluation of mean(erf(x * t)) over the 1e6 values of t costs 2e8 units, so the budget
    // (3e8) is spent by the second evaluation of the constraint, which the solver must make.
    let mut s = Session::new();
    let t = Expr::Range {
        from: Box::new(num(0.0)),
        step: Some(Box::new(num(1e-6))),
        to: Box::new(num(0.999_999)),
    };
    let lhs = Expr::call(
        "mean",
        vec![Expr::call(
            "erf",
            vec![Expr::Binary {
                op: BinaryOp::Mul,
                lhs: Box::new(Expr::var("x")),
                rhs: Box::new(t),
            }],
        )],
    );
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, lhs, num(0.5)),
        solver(Some(Expr::var("a")), "find", &["x"]),
    ]);
    assert_eq!(
        out[2],
        Outcome::Error(EvalError::WorkLimit(MAX_WORK)),
        "{out:?}"
    );
}

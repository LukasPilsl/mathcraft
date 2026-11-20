//! Solve-block budget: guesses share the block's budget, the unknown cap, and what a small linear
//! block costs. Expected values are worked out in the comments.

use std::rc::Rc;

use super::solve_block_tests::{constraint, def, guess, num, solver};
use super::work::{Budget, MAX_WORK};
use super::{BlockMember, EvalError, Outcome, Session, Value};
use crate::expr::{BinaryOp, Expr};

/// `name` as a variable reference.
fn var(name: &str) -> Expr {
    Expr::var(name)
}

/// `(a * a)[1, 1]`, the element of a matrix product.
fn corner_of_square(a: &str) -> Expr {
    let product = Expr::Binary {
        op: BinaryOp::Mul,
        lhs: Box::new(var(a)),
        rhs: Box::new(var(a)),
    };
    Expr::index(product, vec![num(1.0), num(1.0)])
}

#[test]
fn guesses_share_the_block_budget_with_each_other() {
    // A = identity(250). One product is 250^3 = 1.5625e7 multiply-adds, 1.72e8 units (11 per
    // multiply-add). Two products are 3.4e8, over MAX_WORK (3e8), so the second guess is refused.
    // Before the block shared its budget, each guess had a full budget of its own and both passed.
    let mut s = Session::new();
    s.run(&def("A", Expr::call("identity", vec![num(250.0)])));
    let out = s.run_solve_block(&[
        guess("g1", corner_of_square("A")),
        guess("g2", corner_of_square("A")),
    ]);
    assert!(matches!(out[0], Outcome::Defined { .. }), "{out:?}");
    assert_eq!(
        out[1],
        Outcome::Error(EvalError::WorkLimit(MAX_WORK)),
        "{out:?}"
    );
}

#[test]
fn a_block_over_the_unknown_cap_is_refused_with_a_message() {
    // 201 unknowns is one more than MAX_BLOCK_UNKNOWNS (200): the block is refused before any solve.
    let names: Vec<String> = (0..201).map(|i| format!("y{i}")).collect();
    let mut members: Vec<BlockMember> = names.iter().map(|x| guess(x, num(0.0))).collect();
    members.push(constraint(BinaryOp::Eq, var("y0"), num(1.0)));
    let args: Vec<&str> = names.iter().map(String::as_str).collect();
    members.push(solver(Some(var("r")), "find", &args));
    let out = Session::new().run_solve_block(&members);
    match out.last() {
        Some(Outcome::Error(EvalError::Unsupported(msg))) => {
            assert!(msg.contains("at most 200"), "{msg}")
        }
        other => panic!("expected the unknown cap, got {other:?}"),
    }
}

#[test]
fn a_linear_block_of_100_unknowns_solves_within_one_percent_of_the_budget() {
    // x1 == 1 and x(i) == x(i-1) + 1: the solution is x(i) = i. Under 1% of MAX_WORK is 3e6 units.
    let names: Vec<String> = (1..=100).map(|i| format!("x{i}")).collect();
    let mut members: Vec<BlockMember> = names.iter().map(|x| guess(x, num(0.0))).collect();
    members.push(constraint(BinaryOp::Eq, var("x1"), num(1.0)));
    for pair in names.windows(2) {
        let prev_plus_one = Expr::binary(BinaryOp::Add, var(&pair[0]), num(1.0));
        members.push(constraint(BinaryOp::Eq, var(&pair[1]), prev_plus_one));
    }
    let args: Vec<&str> = names.iter().map(String::as_str).collect();
    members.push(solver(Some(var("r")), "find", &args));

    let budget = Rc::new(Budget::new());
    let mut s = Session::new();
    let out = s.run_block_budget(&members, Rc::clone(&budget));
    let Some(Outcome::Defined {
        display: Some(shown),
        ..
    }) = out.last()
    else {
        panic!("the solve did not define r: {out:?}");
    };
    let Outcome::Value(Value::Matrix(x)) = shown.as_ref() else {
        panic!("r is not a column: {shown:?}");
    };
    assert!(
        (x.data()[99].value.re - 100.0).abs() < 1e-6,
        "x100 = {:?}",
        x.data()[99]
    );
    let spent = MAX_WORK - budget.remaining();
    assert!(
        spent * 100 < MAX_WORK,
        "spent {spent} units, over 1% of {MAX_WORK}"
    );
}

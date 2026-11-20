//! Tests for solve blocks. Expected values are hard-coded or worked out by hand in the comments.

use super::env::Env;
use super::{BlockMember, EvalError, MemberKind, Outcome, Session, Value};
use crate::eval::{evaluate, MAX_CALLS, MAX_NODE_DEPTH};
use crate::expr::{BinaryOp, Expr, Id, IdKind};

pub(super) fn def(name: &str, expr: Expr) -> Expr {
    Expr::Define {
        target: Box::new(Expr::var(name)),
        expr: Box::new(expr),
    }
}

pub(super) fn def_fn(name: &str, params: Vec<Expr>, body: Expr) -> Expr {
    Expr::Define {
        target: Box::new(Expr::call(name, params)),
        expr: Box::new(body),
    }
}

pub(super) fn guess(name: &str, value: Expr) -> BlockMember {
    BlockMember {
        kind: MemberKind::Guess,
        body: Ok(def(name, value)),
    }
}

pub(super) fn constraint(op: BinaryOp, lhs: Expr, rhs: Expr) -> BlockMember {
    BlockMember {
        kind: MemberKind::Constraint,
        body: Ok(Expr::binary(op, lhs, rhs)),
    }
}

/// `target := (func(args)=)`, or the bare `(func(args)=)` when `target` is None.
pub(super) fn solver(target: Option<Expr>, func: &str, args: &[&str]) -> BlockMember {
    let call = Expr::call(func, args.iter().map(|n| Expr::var(n)).collect());
    let eval = Expr::Eval {
        expr: Box::new(call),
        unit_override: None,
    };
    let body = match target {
        Some(t) => Expr::Define {
            target: Box::new(t),
            expr: Box::new(eval),
        },
        None => eval,
    };
    BlockMember {
        kind: MemberKind::Solver,
        body: Ok(body),
    }
}

pub(super) fn num(v: f64) -> Expr {
    Expr::num(v)
}

pub(super) fn sq(e: Expr) -> Expr {
    Expr::binary(BinaryOp::Pow, e, num(2.0))
}

fn unit(name: &str) -> Expr {
    Expr::Id(Id::new(name, IdKind::Unit))
}

fn scaled(value: f64, unit_expr: Expr) -> Expr {
    Expr::Scale {
        value: Box::new(num(value)),
        unit: Box::new(unit_expr),
    }
}

pub(super) fn scalar_of(session: &Session, name: &str) -> f64 {
    match session.env().variable(name) {
        Some(Value::Scalar(s)) => s.value.re,
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

pub(super) fn close(got: f64, want: f64, tol: f64) -> bool {
    (got - want).abs() <= tol * want.abs().max(1.0)
}

fn eval_outcome(session: &mut Session, expr: Expr) -> Outcome {
    session.run(&Expr::Eval {
        expr: Box::new(expr),
        unit_override: None,
    })
}

#[test]
fn square_root_from_positive_guess() {
    // x^2 = 4 from x = 1 converges to x = 2.
    let mut s = Session::new();
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(
        matches!(out[2], Outcome::Defined { ref name, .. } if name == "r"),
        "{out:?}"
    );
    assert!(close(scalar_of(&s, "r"), 2.0, 1e-9));
    // Guesses are local to the block.
    assert_eq!(
        eval_outcome(&mut s, Expr::var("x")),
        Outcome::Error(EvalError::Undefined("x".into()))
    );
}

#[test]
fn square_root_from_negative_guess() {
    let mut s = Session::new();
    s.run_solve_block(&[
        guess("x", num(-1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(close(scalar_of(&s, "r"), -2.0, 1e-9));
}

#[test]
fn circle_and_line_give_the_sample_roots() {
    // x^2 + y^2 = 28, y = 3 - 2x, y < 0 from (1, -1): (3.489104628, -3.978209257), the stored
    // values of 5_rata_4_Gerade_schneidet_Kreis.
    let mut s = Session::new();
    let target = Expr::matrix(1, 2, vec![Expr::var("x_1"), Expr::var("y_1")]);
    let sum = Expr::binary(BinaryOp::Add, sq(Expr::var("x")), sq(Expr::var("y")));
    let line = Expr::binary(
        BinaryOp::Sub,
        num(3.0),
        Expr::binary(BinaryOp::Mul, num(2.0), Expr::var("x")),
    );
    s.run_solve_block(&[
        guess("x", num(1.0)),
        guess("y", num(-1.0)),
        constraint(BinaryOp::Eq, sum, num(28.0)),
        constraint(BinaryOp::Eq, Expr::var("y"), line),
        constraint(BinaryOp::Lt, Expr::var("y"), num(0.0)),
        solver(Some(target), "find", &["x", "y"]),
    ]);
    assert!(close(scalar_of(&s, "x_1"), 3.489_104_628_451_919, 1e-9));
    assert!(close(scalar_of(&s, "y_1"), -3.978_209_256_903_839, 1e-9));
}

#[test]
fn unknown_with_units_keeps_its_dimension() {
    // 10 m/s * t - 4.9 m/s^2 * t^2 = 0 has the roots 0 and 10/4.9 s = 2.0408163 s. Guess 1.1 s.
    let mut s = Session::new();
    let v0_t = Expr::binary(
        BinaryOp::Mul,
        scaled(10.0, Expr::binary(BinaryOp::Div, unit("m"), unit("s"))),
        Expr::var("t"),
    );
    let half_g_t2 = Expr::binary(
        BinaryOp::Mul,
        scaled(4.9, Expr::binary(BinaryOp::Div, unit("m"), sq(unit("s")))),
        sq(Expr::var("t")),
    );
    s.run_solve_block(&[
        BlockMember {
            kind: MemberKind::Guess,
            body: Ok(def("t", scaled(1.1, unit("s")))),
        },
        constraint(
            BinaryOp::Eq,
            Expr::binary(BinaryOp::Sub, v0_t, half_g_t2),
            num(0.0),
        ),
        solver(Some(Expr::var("t1")), "find", &["t"]),
    ]);
    assert!(
        close(scalar_of(&s, "t1"), 10.0 / 4.9, 1e-9),
        "{}",
        scalar_of(&s, "t1")
    );
    let dim_of = |session: &Session, name: &str| match session.env().variable(name) {
        Some(Value::Scalar(sc)) => sc.dim,
        other => panic!("{name}: {other:?}"),
    };
    let seconds = match evaluate(&unit("s"), &Env::new()) {
        Ok(Value::Scalar(sc)) => sc.dim,
        other => panic!("{other:?}"),
    };
    assert_eq!(dim_of(&s, "t1"), seconds);
}

#[test]
fn failed_find_is_an_error_and_leaves_the_target_undefined() {
    // x^2 = -4 has no real root.
    let mut s = Session::new();
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(-4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(
        matches!(out[2], Outcome::Error(EvalError::Solve(_))),
        "{out:?}"
    );
    assert!(s.env().variable("r").is_none());
    assert!(s.env().is_failed("r"));
}

#[test]
fn minerr_returns_a_point_and_sets_err() {
    // x^2 = -4 has no root. minerr minimises (x^2 + 4)^2, stationary at x = 0 with residual 4.
    let mut s = Session::new();
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(-4.0)),
        solver(Some(Expr::var("r")), "minerr", &["x"]),
    ]);
    assert!(matches!(out[2], Outcome::Defined { .. }), "{out:?}");
    assert!(
        close(scalar_of(&s, "ERR"), 4.0, 1e-6),
        "ERR = {}",
        scalar_of(&s, "ERR")
    );
}

#[test]
fn inequality_selects_the_feasible_root() {
    // x^2 = 4 and x >= 0 from guess -1 give 2.
    let mut s = Session::new();
    s.run_solve_block(&[
        guess("x", num(-1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        constraint(BinaryOp::Ge, Expr::var("x"), num(0.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(close(scalar_of(&s, "r"), 2.0, 1e-9));
}

#[test]
fn logical_operator_in_a_constraint_is_refused() {
    let mut s = Session::new();
    let both = Expr::binary(
        BinaryOp::And,
        Expr::binary(BinaryOp::Eq, Expr::var("x"), num(2.0)),
        Expr::binary(BinaryOp::Eq, Expr::var("x"), num(2.0)),
    );
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        BlockMember {
            kind: MemberKind::Constraint,
            body: Ok(both),
        },
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(
        matches!(out[1], Outcome::Error(EvalError::Unsupported(_))),
        "{out:?}"
    );
    assert!(
        matches!(out[2], Outcome::Error(EvalError::Unsupported(_))),
        "{out:?}"
    );
}

#[test]
fn dimension_mismatch_in_a_constraint_is_an_error() {
    // t (seconds) == 3 metres.
    let mut s = Session::new();
    let out = s.run_solve_block(&[
        guess("t", scaled(1.0, unit("s"))),
        constraint(BinaryOp::Eq, Expr::var("t"), scaled(3.0, unit("m"))),
        solver(Some(Expr::var("r")), "find", &["t"]),
    ]);
    assert!(
        matches!(out[2], Outcome::Error(EvalError::DimensionMismatch { .. })),
        "{out:?}"
    );
    assert!(s.env().is_failed("r"));
}

#[test]
fn a_literal_zero_is_accepted_against_any_dimension() {
    // (t^2 - 4) m == 0: the left side is in metres, the right side is a plain 0. Root t = 2.
    let mut s = Session::new();
    let t_sq = Expr::binary(BinaryOp::Mul, Expr::var("t"), Expr::var("t"));
    s.run_solve_block(&[
        guess("t", num(1.0)),
        constraint(
            BinaryOp::Eq,
            Expr::Scale {
                value: Box::new(Expr::binary(BinaryOp::Sub, t_sq, num(4.0))),
                unit: Box::new(unit("m")),
            },
            num(0.0),
        ),
        solver(Some(Expr::var("r")), "find", &["t"]),
    ]);
    assert!(close(scalar_of(&s, "r"), 2.0, 1e-9));
}

#[test]
fn find_outside_a_block_is_an_error() {
    let mut s = Session::new();
    let out = eval_outcome(&mut s, Expr::call("find", vec![Expr::var("x")]));
    assert_eq!(
        out,
        Outcome::Error(EvalError::OutsideSolveBlock("Find".into()))
    );
}

#[test]
fn unknown_without_a_guess_is_undefined() {
    // No guess for x above or inside the block: Mathcad requires one.
    let mut s = Session::new();
    let out = s.run_solve_block(&[
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert_eq!(out[1], Outcome::Error(EvalError::Undefined("x".into())));
}

#[test]
fn failed_guess_leaves_the_unknown_undefined() {
    // x := 1/0 fails; find reports the guess's own error, and r stays undefined.
    let mut s = Session::new();
    let div0 = Expr::binary(BinaryOp::Div, num(1.0), num(0.0));
    let out = s.run_solve_block(&[
        guess("x", div0),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert_eq!(out[0], Outcome::Error(EvalError::DivisionByZero));
    assert_eq!(out[2], Outcome::Error(EvalError::DivisionByZero));
    assert!(s.env().variable("r").is_none());
}

#[test]
fn two_blocks_reuse_names_without_leaking_guesses() {
    // Block 1 finds x = 2 from 1; block 2 finds x = -2 from -1. The outer x stays undefined.
    let mut s = Session::new();
    let block = |g: f64, target: &str| {
        vec![
            guess("x", num(g)),
            constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
            solver(Some(Expr::var(target)), "find", &["x"]),
        ]
    };
    s.run_solve_block(&block(1.0, "a"));
    s.run_solve_block(&block(-1.0, "b"));
    assert!(close(scalar_of(&s, "a"), 2.0, 1e-9));
    assert!(close(scalar_of(&s, "b"), -2.0, 1e-9));
    assert!(s.env().variable("x").is_none());
}

#[test]
fn a_block_guess_shadows_an_outer_name_only_inside_the_block() {
    let mut s = Session::new();
    s.run(&def("x", num(7.0)));
    s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(close(scalar_of(&s, "r"), 2.0, 1e-9));
    assert!(close(scalar_of(&s, "x"), 7.0, 1e-12));
}

#[test]
fn function_defined_from_a_find_result_is_usable_afterwards() {
    let mut s = Session::new();
    s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    s.run(&def_fn(
        "f",
        vec![Expr::var("a")],
        Expr::binary(BinaryOp::Mul, Expr::var("r"), Expr::var("a")),
    ));
    match eval_outcome(&mut s, Expr::call("f", vec![num(3.0)])) {
        Outcome::Value(Value::Scalar(sc)) => assert!(close(sc.value.re, 6.0, 1e-9)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn odesolve_with_too_few_arguments_is_unsupported_with_a_reason() {
    // odesolve is implemented (see `ode_block`); a call without its end point is refused with the
    // arguments it takes.
    let mut s = Session::new();
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, Expr::var("x"), num(2.0)),
        solver(Some(Expr::var("r")), "odesolve", &["x"]),
    ]);
    assert!(
        matches!(&out[2], Outcome::Error(EvalError::Unsupported(m)) if m.starts_with("odesolve takes")),
        "{:?}",
        out[2]
    );
}

#[test]
fn shared_call_budget_stops_an_expensive_constraint() {
    // f20(x) calls f19 twice, and so on down to f0(x) = x: 2^20 - 1 user calls, more than
    // MAX_CALLS. The block stops with the budget error instead of running on.
    let mut s = Session::new();
    s.run(&def_fn("f0", vec![Expr::var("x")], Expr::var("x")));
    for k in 1..=20 {
        let inner = format!("f{}", k - 1);
        let body = Expr::binary(
            BinaryOp::Add,
            Expr::call(&inner, vec![Expr::var("x")]),
            Expr::call(&inner, vec![Expr::var("x")]),
        );
        s.run(&def_fn(&format!("f{k}"), vec![Expr::var("x")], body));
    }
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(
            BinaryOp::Eq,
            Expr::call("f20", vec![Expr::var("x")]),
            num(0.0),
        ),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert_eq!(out[2], Outcome::Error(EvalError::CallLimit(MAX_CALLS)));
}

#[test]
fn deep_constraint_hits_the_node_depth_limit_without_overflowing() {
    // A constraint nested beyond MAX_NODE_DEPTH fails cleanly, on a 2 MiB thread.
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut deep = Expr::var("x");
            for _ in 0..(MAX_NODE_DEPTH + 50) {
                deep = Expr::binary(BinaryOp::Add, deep, num(0.0));
            }
            let mut s = Session::new();
            s.run_solve_block(&[
                guess("x", num(1.0)),
                constraint(BinaryOp::Eq, deep, num(2.0)),
                solver(Some(Expr::var("r")), "find", &["x"]),
            ])
        })
        .expect("spawn");
    let out = handle.join().expect("no overflow");
    assert_eq!(
        out[2],
        Outcome::Error(EvalError::DepthLimit(MAX_NODE_DEPTH))
    );
}

#[test]
fn a_non_scalar_ctol_is_an_error_not_the_default() {
    // CTOL = identity(2) is a matrix: the find refuses it instead of using the default tolerance.
    let mut s = Session::new();
    s.run(&def("CTOL", Expr::call("identity", vec![num(2.0)])));
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(
        matches!(&out[2], Outcome::Error(EvalError::Unsupported(msg)) if msg.contains("CTOL")),
        "{out:?}"
    );
    assert!(s.env().variable("r").is_none());
}

#[test]
fn a_non_positive_ctol_is_an_error_not_the_default() {
    let mut s = Session::new();
    s.run(&def("CTOL", num(0.0)));
    let out = s.run_solve_block(&[
        guess("x", num(1.0)),
        constraint(BinaryOp::Eq, sq(Expr::var("x")), num(4.0)),
        solver(Some(Expr::var("r")), "find", &["x"]),
    ]);
    assert!(
        matches!(&out[2], Outcome::Error(EvalError::Unsupported(msg)) if msg.contains("CTOL")),
        "{out:?}"
    );
}

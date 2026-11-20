//! Tests for `odesolve` in solve blocks. Expected values are closed forms.

use super::solve_block_tests::constraint;
use super::{BlockMember, EvalError, MemberKind, Outcome, Session, Value};
use crate::expr::{BinaryOp, Expr, Id, IdKind};

fn t() -> Expr {
    Expr::var("t")
}

/// `name` with `primes` primes, called at `arg`: `y'(t)`, `y''(0)`.
fn deriv(name: &str, primes: &str, arg: Expr) -> Expr {
    Expr::call(&format!("{name}{primes}"), vec![arg])
}

fn sum(a: Expr, b: Expr) -> Expr {
    Expr::binary(BinaryOp::Add, a, b)
}

/// `target := odesolve(vf, end, [intvls])` as a solver member.
fn odesolve(target: Expr, vf: Expr, end: Expr, intvls: Option<Expr>) -> BlockMember {
    let mut args = vec![vf, end];
    if let Some(n) = intvls {
        args.push(n);
    }
    BlockMember {
        kind: MemberKind::Solver,
        body: Ok(Expr::Define {
            target: Box::new(target),
            expr: Box::new(Expr::call("odesolve", args)),
        }),
    }
}

fn eq(lhs: Expr, rhs: Expr) -> BlockMember {
    constraint(BinaryOp::Eq, lhs, rhs)
}

/// The real value of `name(at)`, evaluated in the worksheet.
fn value_at(session: &mut Session, name: &str, at: f64) -> f64 {
    let e = Expr::Eval {
        expr: Box::new(Expr::call(name, vec![Expr::num(at)])),
        unit_override: None,
    };
    match session.run(&e) {
        Outcome::Value(Value::Scalar(s)) => s.value.re,
        other => panic!("{name}({at}): {other:?}"),
    }
}

fn last_error(outcomes: &[Outcome]) -> Option<EvalError> {
    match outcomes.last() {
        Some(Outcome::Error(e)) => Some(e.clone()),
        _ => None,
    }
}

#[test]
fn first_order_decay_matches_the_closed_form() {
    // y' + y = 0, y(0) = 1: y = e^(-t).
    let mut session = Session::new();
    let members = [
        eq(
            sum(deriv("y", "'", t()), deriv("y", "", t())),
            Expr::num(0.0),
        ),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(
            Expr::var("y"),
            deriv("y", "", t()),
            Expr::num(2.0),
            Some(Expr::num(200.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(&outcomes[2], Outcome::Defined { name, .. } if name == "y"),
        "{:?}",
        outcomes[2]
    );
    for at in [0.5, 1.0, 1.5] {
        let got = value_at(&mut session, "y", at);
        assert!((got - (-at).exp()).abs() < 1e-6, "y({at}) = {got}");
    }
}

#[test]
fn ode_nonlinear_in_its_highest_derivative_is_refused() {
    // x''(t)^2 == 4, x(0) = x'(0) = 0. The linear model gives x'' = 2, which is not a solution
    // (the solutions have x'' = ±2 but the residual at the model's value is 12): refused.
    let mut session = Session::new();
    let members = [
        eq(
            Expr::binary(BinaryOp::Pow, deriv("x", "''", t()), Expr::num(2.0)),
            Expr::num(4.0),
        ),
        eq(deriv("x", "", Expr::num(0.0)), Expr::num(0.0)),
        eq(deriv("x", "'", Expr::num(0.0)), Expr::num(0.0)),
        odesolve(
            Expr::var("X"),
            deriv("x", "", t()),
            Expr::num(1.0),
            Some(Expr::num(100.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(last_error(&outcomes), Some(EvalError::Unsupported(_))),
        "{outcomes:?}"
    );
}

#[test]
fn odesolve_refuses_points_outside_the_solved_range() {
    // y' = -y, y(0) = 1 on [0, 2]. The help limits the solution to the solved range: outside it the
    // function refuses (it does not extrapolate the spline).
    let mut session = Session::new();
    let members = [
        eq(
            sum(deriv("y", "'", t()), deriv("y", "", t())),
            Expr::num(0.0),
        ),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(
            Expr::var("Y"),
            deriv("y", "", t()),
            Expr::num(2.0),
            Some(Expr::num(200.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(&outcomes[2], Outcome::Defined { name, .. } if name == "Y"),
        "{:?}",
        outcomes[2]
    );
    let inside = value_at(&mut session, "Y", 1.5);
    assert!((inside - (-1.5f64).exp()).abs() < 1e-6, "Y(1.5) = {inside}");
    for at in [3.0, -1.0] {
        let e = Expr::Eval {
            expr: Box::new(Expr::call("Y", vec![Expr::num(at)])),
            unit_override: None,
        };
        assert!(
            matches!(session.run(&e), Outcome::Error(_)),
            "Y({at}) must be refused, not extrapolated"
        );
    }
}

#[test]
fn derivative_operator_form_is_the_same_equation() {
    // d/dt y(t) + y(t) = 0 is the same ODE as y'(t) + y(t) = 0.
    let mut session = Session::new();
    let ddt = Expr::Derivative {
        var: Box::new(Id::new("t", IdKind::Variable)),
        order: None,
        partial: false,
        body: Box::new(deriv("y", "", t())),
    };
    let members = [
        eq(sum(ddt, deriv("y", "", t())), Expr::num(0.0)),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(
            Expr::var("y"),
            deriv("y", "", t()),
            Expr::num(1.0),
            Some(Expr::num(100.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes[2], Outcome::Defined { .. }),
        "{:?}",
        outcomes[2]
    );
    let got = value_at(&mut session, "y", 0.5);
    assert!((got - (-0.5f64).exp()).abs() < 1e-6, "{got}");
}

#[test]
fn second_order_oscillator_matches_sine() {
    // x'' + x = 0, x(0) = 0, x'(0) = 1: x = sin(t).
    let mut session = Session::new();
    let members = [
        eq(
            sum(deriv("x", "''", t()), deriv("x", "", t())),
            Expr::num(0.0),
        ),
        eq(deriv("x", "", Expr::num(0.0)), Expr::num(0.0)),
        eq(deriv("x", "'", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(
            Expr::var("x"),
            deriv("x", "", t()),
            Expr::num(10.0),
            Some(Expr::num(200.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes[3], Outcome::Defined { .. }),
        "{:?}",
        outcomes[3]
    );
    for at in [1.0, 3.0, 7.5] {
        let got = value_at(&mut session, "x", at);
        assert!((got - at.sin()).abs() < 1e-6, "x({at}) = {got}");
    }
}

#[test]
fn system_of_two_functions_returns_a_vector_of_solutions() {
    // x' = y, y' = -x, x(0) = 1, y(0) = 0: X = cos t, Y = -sin t.
    let mut session = Session::new();
    let vf = Expr::Matrix {
        rows: 2,
        cols: 1,
        elements: vec![deriv("x", "", t()), deriv("y", "", t())],
    };
    let target = Expr::Matrix {
        rows: 2,
        cols: 1,
        elements: vec![Expr::var("X"), Expr::var("Y")],
    };
    let members = [
        eq(deriv("x", "'", t()), deriv("y", "", t())),
        eq(
            deriv("y", "'", t()),
            Expr::Unary {
                op: crate::expr::UnaryOp::Neg,
                operand: Box::new(deriv("x", "", t())),
            },
        ),
        eq(deriv("x", "", Expr::num(0.0)), Expr::num(1.0)),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(0.0)),
        odesolve(target, vf, Expr::num(5.0), Some(Expr::num(200.0))),
    ];
    let outcomes = session.run_solve_block(&members);
    assert_eq!(outcomes[4], Outcome::Accepted, "{:?}", outcomes[4]);
    for at in [2.0, 4.0] {
        let cx = value_at(&mut session, "X", at);
        let cy = value_at(&mut session, "Y", at);
        assert!((cx - at.cos()).abs() < 1e-6, "X({at}) = {cx}");
        assert!((cy + at.sin()).abs() < 1e-6, "Y({at}) = {cy}");
    }
}

#[test]
fn end_point_before_the_start_integrates_backwards() {
    // y' + y = 0, y(0) = 1, from 0 back to -1: y = e^(-t) again, at t = -0.5.
    let mut session = Session::new();
    let members = [
        eq(
            sum(deriv("y", "'", t()), deriv("y", "", t())),
            Expr::num(0.0),
        ),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(
            Expr::var("y"),
            deriv("y", "", t()),
            Expr::num(-1.0),
            Some(Expr::num(100.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes[2], Outcome::Defined { .. }),
        "{:?}",
        outcomes[2]
    );
    let got = value_at(&mut session, "y", -0.5);
    assert!((got - 0.5f64.exp()).abs() < 1e-6, "{got}");
}

fn mul(a: Expr, b: Expr) -> Expr {
    Expr::binary(BinaryOp::Mul, a, b)
}

fn sub(a: Expr, b: Expr) -> Expr {
    Expr::binary(BinaryOp::Sub, a, b)
}

#[test]
fn help_second_order_example_matches_the_reference_solution() {
    // Help "Solving ODEs": 3 x'' + x' + 5 x = sin(pi t), x(0) = -1, x'(0) = 1. Reference: fixed-step
    // RK4 with 20000 steps per unit (x(1), x(3), x(10) = 0.371821514763, 0.174439727847,
    // -0.154781321110).
    let mut session = Session::new();
    let rhs = sub(
        Expr::call("sin", vec![mul(Expr::var("π"), t())]),
        sum(
            deriv("x", "'", t()),
            mul(Expr::num(5.0), deriv("x", "", t())),
        ),
    );
    let members = [
        eq(mul(Expr::num(3.0), deriv("x", "''", t())), rhs),
        eq(deriv("x", "", Expr::num(0.0)), Expr::num(-1.0)),
        eq(deriv("x", "'", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(
            Expr::var("x"),
            deriv("x", "", t()),
            Expr::num(10.0),
            Some(Expr::num(500.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes[3], Outcome::Defined { .. }),
        "{:?}",
        outcomes[3]
    );
    for (at, want, tol) in [
        (1.0, 0.371821514763, 1e-6),
        (3.0, 0.174439727847, 1e-6),
        (10.0, -0.154781321110, 1e-4),
    ] {
        let got = value_at(&mut session, "x", at);
        assert!((got - want).abs() < tol, "x({at}) = {got}, want {want}");
    }
}

#[test]
fn help_first_order_example_matches_the_reference_solution() {
    // Help "Example: Solving a First-Order ODE Initial Value Problem": y' = sin z + 1/y, y(0) = 1.
    // Reference (RK4, 20000 steps per unit): y(2) = 3.459295178981, y(10) = 5.837194241667,
    // y(20) = 6.355825030121.
    let mut session = Session::new();
    let rhs = sum(
        Expr::call("sin", vec![t()]),
        Expr::binary(BinaryOp::Div, Expr::num(1.0), deriv("y", "", t())),
    );
    let members = [
        eq(deriv("y", "'", t()), rhs),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(
            Expr::var("y"),
            deriv("y", "", t()),
            Expr::num(20.0),
            Some(Expr::num(2000.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes[2], Outcome::Defined { .. }),
        "{:?}",
        outcomes[2]
    );
    for (at, want, tol) in [
        (2.0, 3.459295178981, 1e-6),
        (10.0, 5.837194241667, 1e-6),
        (20.0, 6.355825030121, 1e-4),
    ] {
        let got = value_at(&mut session, "y", at);
        assert!((got - want).abs() < tol, "y({at}) = {got}, want {want}");
    }
}

#[test]
fn help_system_example_matches_the_reference_solution() {
    // Help "Example: Solving a First-Order System of ODEs": a nonlinear 3-equation system with
    // y0(0) = -1, y1(0) = 0, y2(0) = 1, solved over [0, 10]. Reference (RK4, 20000 steps per
    // unit): at u = 3, (-0.079956986361, 0.239782592729, 15.033470823432); at u = 10,
    // (-5.195479173682, -9.977682690901, 21.106242762378).
    let mut session = Session::new();
    let y = |k: &str| deriv(k, "", t());
    let vf = Expr::Matrix {
        rows: 3,
        cols: 1,
        elements: vec![y("y0"), y("y1"), y("y2")],
    };
    let target = Expr::Matrix {
        rows: 3,
        cols: 1,
        elements: vec![Expr::var("f"), Expr::var("g"), Expr::var("h")],
    };
    let members = [
        eq(
            deriv("y0", "'", t()),
            sum(mul(Expr::num(-8.0), y("y0")), mul(Expr::num(8.0), y("y1"))),
        ),
        eq(
            deriv("y1", "'", t()),
            sub(
                sum(mul(Expr::num(30.0), y("y0")), y("y1")),
                mul(y("y0"), y("y2")),
            ),
        ),
        eq(
            deriv("y2", "'", t()),
            sub(
                mul(y("y0"), y("y1")),
                mul(
                    Expr::binary(BinaryOp::Div, Expr::num(8.0), Expr::num(3.0)),
                    y("y2"),
                ),
            ),
        ),
        eq(deriv("y0", "", Expr::num(0.0)), Expr::num(-1.0)),
        eq(deriv("y1", "", Expr::num(0.0)), Expr::num(0.0)),
        eq(deriv("y2", "", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(target, vf, Expr::num(10.0), Some(Expr::num(1000.0))),
    ];
    let outcomes = session.run_solve_block(&members);
    assert_eq!(outcomes[6], Outcome::Accepted, "{:?}", outcomes[6]);
    for (name, at, want, tol) in [
        ("f", 3.0, -0.079956986361, 1e-6),
        ("g", 3.0, 0.239782592729, 1e-6),
        ("h", 3.0, 15.033470823432, 1e-6),
        ("f", 10.0, -5.195479173682, 1e-4),
        ("g", 10.0, -9.977682690901, 1e-4),
        ("h", 10.0, 21.106242762378, 1e-4),
    ] {
        let got = value_at(&mut session, name, at);
        assert!(
            (got - want).abs() < tol,
            "{name}({at}) = {got}, want {want}"
        );
    }
}

#[test]
fn stiff_van_der_pol_is_refused_cleanly() {
    // y'' - 1000 (1 - y^2) y' + y = 0, y(0) = 2, y'(0) = 0 over [0, 3000]: stiff, so the explicit
    // steps run out (or the work budget does). It must be refused, not solved slowly or panic.
    let mut session = Session::new();
    let y2 = || deriv("y", "''", t());
    let y1 = || deriv("y", "'", t());
    let y = || deriv("y", "", t());
    let damping = mul(
        Expr::num(1000.0),
        mul(sub(Expr::num(1.0), mul(y(), y())), y1()),
    );
    let members = [
        eq(sum(sub(y2(), damping), y()), Expr::num(0.0)),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(2.0)),
        eq(deriv("y", "'", Expr::num(0.0)), Expr::num(0.0)),
        odesolve(
            Expr::var("y"),
            deriv("y", "", t()),
            Expr::num(3000.0),
            Some(Expr::num(100.0)),
        ),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(last_error(&outcomes), Some(EvalError::Unsupported(m)) if m.contains("stiff"))
            || matches!(last_error(&outcomes), Some(EvalError::WorkLimit(_))),
        "{:?}",
        outcomes[3]
    );
}

#[test]
fn boundary_value_problem_is_refused() {
    // y'' + y = 0 with y(0) = 0 and y(1) = 1: conditions at two points.
    let mut session = Session::new();
    let members = [
        eq(
            sum(deriv("y", "''", t()), deriv("y", "", t())),
            Expr::num(0.0),
        ),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(0.0)),
        eq(deriv("y", "", Expr::num(1.0)), Expr::num(1.0)),
        odesolve(Expr::var("y"), deriv("y", "", t()), Expr::num(2.0), None),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(last_error(&outcomes), Some(EvalError::Unsupported(m)) if m.contains("boundary")),
        "{:?}",
        outcomes[3]
    );
}

#[test]
fn missing_initial_condition_is_refused() {
    // y'' + y = 0 with only y(0) = 0: the derivative's condition is missing.
    let mut session = Session::new();
    let members = [
        eq(
            sum(deriv("y", "''", t()), deriv("y", "", t())),
            Expr::num(0.0),
        ),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(0.0)),
        odesolve(Expr::var("y"), deriv("y", "", t()), Expr::num(2.0), None),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(last_error(&outcomes), Some(EvalError::Unsupported(_))),
        "{:?}",
        outcomes[2]
    );
}

#[test]
fn inequality_in_an_odesolve_block_is_refused() {
    let mut session = Session::new();
    let members = [
        constraint(BinaryOp::Lt, deriv("y", "'", t()), Expr::num(1.0)),
        eq(deriv("y", "", Expr::num(0.0)), Expr::num(1.0)),
        odesolve(Expr::var("y"), deriv("y", "", t()), Expr::num(2.0), None),
    ];
    let outcomes = session.run_solve_block(&members);
    assert!(last_error(&outcomes).is_some(), "{:?}", outcomes[2]);
}

#[test]
fn odesolve_outside_a_solve_block_is_refused() {
    let mut session = Session::new();
    let e = Expr::Define {
        target: Box::new(Expr::var("y")),
        expr: Box::new(Expr::call(
            "odesolve",
            vec![deriv("y", "", t()), Expr::num(2.0)],
        )),
    };
    assert!(matches!(session.run(&e), Outcome::Error(_)));
}

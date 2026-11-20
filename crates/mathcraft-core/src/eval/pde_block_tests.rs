//! Tests for `pdesolve` in solve blocks (see `pde_block.rs`). The expected values are closed forms.

use super::solve_block_tests::constraint;
use super::{BlockMember, MemberKind, Outcome, Session, Value};
use crate::expr::{BinaryOp, Expr};

const PI: f64 = std::f64::consts::PI;

fn x() -> Expr {
    Expr::var("x")
}

fn t() -> Expr {
    Expr::var("t")
}

/// A subscript-named use of `name` at `(a, b)`, such as `Txx(x, t)`.
fn use_of(name: &str, a: Expr, b: Expr) -> Expr {
    Expr::call(name, vec![a, b])
}

fn eq(lhs: Expr, rhs: Expr) -> BlockMember {
    constraint(BinaryOp::Eq, lhs, rhs)
}

fn sin(e: Expr) -> Expr {
    Expr::call("sin", vec![e])
}

/// `T := pdesolve(T, x, [0; pi], t, [0; 1], nx, nt)` as a solver member.
fn pdesolve(target: &str, xpts: f64, tpts: f64) -> BlockMember {
    let range = |a: f64, b: f64| Expr::Matrix {
        rows: 2,
        cols: 1,
        elements: vec![Expr::num(a), Expr::num(b)],
    };
    let args = vec![
        Expr::var(target),
        x(),
        range(0.0, PI),
        t(),
        range(0.0, 1.0),
        Expr::num(xpts),
        Expr::num(tpts),
    ];
    BlockMember {
        kind: MemberKind::Solver,
        body: Ok(Expr::Define {
            target: Box::new(Expr::var(target)),
            expr: Box::new(Expr::call("pdesolve", args)),
        }),
    }
}

/// The real value of `name(a, b)` in the worksheet.
fn value_at(session: &mut Session, name: &str, a: f64, b: f64) -> f64 {
    let e = Expr::Eval {
        expr: Box::new(use_of(name, Expr::num(a), Expr::num(b))),
        unit_override: None,
    };
    match session.run(&e) {
        Outcome::Value(Value::Scalar(s)) => s.value.re,
        other => panic!("{name}({a}, {b}): {other:?}"),
    }
}

/// The heat equation `T_t = T_xx` with zero edges and `T(x, 0) = sin x`, as solve-block members.
/// Its solution is `exp(-t) sin x`.
fn heat_members(with_right_edge: bool) -> Vec<BlockMember> {
    let mut members = vec![
        eq(use_of("Tt", x(), t()), use_of("Txx", x(), t())),
        eq(use_of("T", Expr::num(0.0), t()), Expr::num(0.0)),
    ];
    if with_right_edge {
        members.push(eq(use_of("T", Expr::num(PI), t()), Expr::num(0.0)));
    }
    members.push(eq(use_of("T", x(), Expr::num(0.0)), sin(x())));
    members
}

#[test]
fn heat_equation_matches_the_closed_form() {
    let mut session = Session::new();
    let mut members = heat_members(true);
    members.push(pdesolve("T", 41.0, 21.0));
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes.last(), Some(Outcome::Defined { name, .. }) if name == "T"),
        "{:?}",
        outcomes.last()
    );
    for (xv, tv) in [(0.5, 0.25), (1.5, 0.5), (2.5, 1.0)] {
        let got = value_at(&mut session, "T", xv, tv);
        let exact = (-tv).exp() * xv.sin();
        assert!(
            (got - exact).abs() < 2e-3,
            "T({xv}, {tv}) = {got}, expected {exact}"
        );
    }
}

#[test]
fn a_solution_outside_the_solved_range_is_refused() {
    let mut session = Session::new();
    let mut members = heat_members(true);
    members.push(pdesolve("T", 21.0, 11.0));
    session.run_solve_block(&members);
    let out = session.run(&Expr::Eval {
        expr: Box::new(use_of("T", Expr::num(0.5), Expr::num(2.0))),
        unit_override: None,
    });
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
}

#[test]
fn a_missing_edge_condition_is_refused() {
    let mut session = Session::new();
    let mut members = heat_members(false);
    members.push(pdesolve("T", 21.0, 11.0));
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes.last(), Some(Outcome::Error(_))),
        "{:?}",
        outcomes.last()
    );
}

#[test]
fn a_first_order_equation_is_refused() {
    // T_t = T_x: the help's first-order case, which the solver does not implement yet.
    let mut session = Session::new();
    let mut members = vec![
        eq(use_of("Tt", x(), t()), use_of("Tx", x(), t())),
        eq(use_of("T", Expr::num(0.0), t()), Expr::num(0.0)),
        eq(use_of("T", x(), Expr::num(0.0)), sin(x())),
    ];
    members.push(pdesolve("T", 21.0, 11.0));
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes.last(), Some(Outcome::Error(_))),
        "{:?}",
        outcomes.last()
    );
}

/// The heat members with `pde` as the PDE, which is written with the unknown inside a sum.
fn heat_with_pde(pde: Expr) -> Vec<BlockMember> {
    vec![
        eq(pde, Expr::num(0.0)),
        eq(use_of("T", Expr::num(0.0), t()), Expr::num(0.0)),
        eq(use_of("T", Expr::num(PI), t()), Expr::num(0.0)),
        eq(use_of("T", x(), Expr::num(0.0)), sin(x())),
        pdesolve("T", 41.0, 21.0),
    ]
}

fn heat_exact_check(session: &mut Session) {
    for (xv, tv) in [(0.5, 0.25), (1.5, 0.5), (2.5, 1.0)] {
        let got = value_at(session, "T", xv, tv);
        let exact = (-tv).exp() * xv.sin();
        assert!(
            (got - exact).abs() < 2e-3,
            "T({xv}, {tv}) = {got}, expected {exact}"
        );
    }
}

#[test]
fn pde_with_the_unknown_in_a_difference_is_solved() {
    // T_t - T_xx = 0: the help's form, with the time derivative inside a subtraction.
    let pde = Expr::binary(
        BinaryOp::Sub,
        use_of("Tt", x(), t()),
        use_of("Txx", x(), t()),
    );
    let mut session = Session::new();
    let outcomes = session.run_solve_block(&heat_with_pde(pde));
    assert!(
        matches!(outcomes.last(), Some(Outcome::Defined { name, .. }) if name == "T"),
        "{:?}",
        outcomes.last()
    );
    heat_exact_check(&mut session);
}

#[test]
fn pde_with_the_sign_flipped_difference_is_solved() {
    // T_xx - T_t = 0 is the same equation with the terms the other way round.
    let pde = Expr::binary(
        BinaryOp::Sub,
        use_of("Txx", x(), t()),
        use_of("Tt", x(), t()),
    );
    let mut session = Session::new();
    let outcomes = session.run_solve_block(&heat_with_pde(pde));
    assert!(
        matches!(outcomes.last(), Some(Outcome::Defined { name, .. }) if name == "T"),
        "{:?}",
        outcomes.last()
    );
    heat_exact_check(&mut session);
}

#[test]
fn pde_with_a_numeric_factor_on_the_time_derivative_is_solved() {
    // -2 T_t + 2 T_xx = 0, the same heat equation with the factor 2 on both terms.
    let pde = Expr::binary(
        BinaryOp::Add,
        Expr::unary(
            crate::expr::UnaryOp::Neg,
            Expr::binary(BinaryOp::Mul, Expr::num(2.0), use_of("Tt", x(), t())),
        ),
        Expr::binary(BinaryOp::Mul, Expr::num(2.0), use_of("Txx", x(), t())),
    );
    let mut session = Session::new();
    let outcomes = session.run_solve_block(&heat_with_pde(pde));
    assert!(
        matches!(outcomes.last(), Some(Outcome::Defined { name, .. }) if name == "T"),
        "{:?}",
        outcomes.last()
    );
    heat_exact_check(&mut session);
}

#[test]
fn a_difference_without_a_time_derivative_is_refused() {
    let pde = Expr::binary(BinaryOp::Sub, use_of("Txx", x(), t()), x());
    let mut session = Session::new();
    let outcomes = session.run_solve_block(&heat_with_pde(pde));
    assert!(
        matches!(outcomes.last(), Some(Outcome::Error(_))),
        "{:?}",
        outcomes.last()
    );
}

#[test]
fn a_grid_above_the_element_limit_is_refused_before_solving() {
    // 2000 x 10000 points is 2e7 values, above the 1e7 limit of a matrix.
    let mut session = Session::new();
    let mut members = heat_members(true);
    members.push(pdesolve("T", 2000.0, 10000.0));
    let outcomes = session.run_solve_block(&members);
    assert!(
        matches!(outcomes.last(), Some(Outcome::Error(_))),
        "{:?}",
        outcomes.last()
    );
}

#[test]
fn pdesolve_outside_a_solve_block_is_refused() {
    let mut session = Session::new();
    let out = session.run(&Expr::call("pdesolve", vec![Expr::var("T")]));
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
}

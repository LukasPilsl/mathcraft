//! `maximize` and `minimize`, inside solve blocks and outside them. Expected values are the help's
//! (Example: Optimizing a Function, f(x) = sin(x) e^(0.1 x)) and the exact stationary points:
//! tan(x) = -10 gives x = atan(-10) + k pi = -1.471128 + k pi, so the extrema are -4.612721 (max),
//! -1.471128 (min), 4.812057 (min), 7.953650 (max), 11.095243 (min).

use crate::eval::{BlockMember, EvalError, MemberKind, Outcome, Session, Value};
use crate::expr::Expr;
use crate::parser::{parse, DefaultIdContext};

fn stmt(src: &str) -> Expr {
    parse(src, &DefaultIdContext).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

/// A worksheet with `f(x) := sin(x) e^(0.1 x)` and the given extra statements run first.
fn session_with(lines: &[&str]) -> Session {
    let mut s = Session::new();
    s.run(&stmt("f(x) := sin(x) * exp(0.1 * x)"));
    for line in lines {
        s.run(&stmt(line));
    }
    s
}

/// A solver member `target := rhs` (or the bare `rhs` when there is no `:=`). The text parser
/// does not take a trailing `=` after `:=`, so the target and the call are parsed separately.
fn solver_member(src: &str) -> BlockMember {
    let body = match src.split_once(":=") {
        Some((target, rhs)) => Expr::Define {
            target: Box::new(stmt(target.trim())),
            expr: Box::new(stmt(rhs.trim())),
        },
        None => stmt(src),
    };
    BlockMember {
        kind: MemberKind::Solver,
        body: Ok(body),
    }
}

/// Runs one solve block given as (kind, statement) pairs. Kinds: `G` guess, `C` constraint,
/// `S` solver member.
fn block(s: &mut Session, members: &[(char, &str)]) -> Vec<Outcome> {
    let ms: Vec<BlockMember> = members
        .iter()
        .map(|(k, src)| match k {
            'S' => solver_member(src),
            _ => BlockMember {
                kind: if *k == 'G' {
                    MemberKind::Guess
                } else {
                    MemberKind::Constraint
                },
                body: Ok(stmt(src)),
            },
        })
        .collect();
    s.run_solve_block(&ms)
}

fn scalar_var(s: &Session, name: &str) -> f64 {
    match s.env().variable(name) {
        Some(Value::Scalar(sc)) => sc.value.re,
        other => panic!("{name}: expected a scalar, got {other:?}"),
    }
}

fn column_var(s: &Session, name: &str) -> Vec<f64> {
    match s.env().variable(name) {
        Some(Value::Matrix(m)) => m.data().iter().map(|v| v.value.re).collect(),
        other => panic!("{name}: expected a vector, got {other:?}"),
    }
}

fn assert_error(out: &Outcome) {
    assert!(
        matches!(out, Outcome::Error(_)),
        "expected an error, got {out:?}"
    );
}

// ----- outside a solve block ------------------------------------------------------------------

#[test]
fn maximize_outside_a_block_finds_the_local_maximum_nearest_the_guess() {
    // Help: hi := maximize(f, g) = -4.613 with g := -3.
    let mut s = session_with(&["g := -3"]);
    match s.run(&stmt("maximize(f, g)=")) {
        Outcome::Value(Value::Scalar(sc)) => {
            assert!((sc.value.re + 4.612721).abs() < 1e-6, "{}", sc.value.re)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn minimize_outside_a_block_finds_the_local_minimum_nearest_the_guess() {
    // Help: lo := minimize(f, g) = -1.471 with g := -3.
    let mut s = session_with(&["g := -3"]);
    match s.run(&stmt("minimize(f, g)=")) {
        Outcome::Value(Value::Scalar(sc)) => {
            assert!((sc.value.re + 1.471128).abs() < 1e-6, "{}", sc.value.re)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn minimize_outside_a_block_of_two_unknowns_is_a_column() {
    // Rosenbrock (1 - x)^2 + 100 (y - x^2)^2 from (-1.2, 1): minimum (1, 1).
    let mut s = Session::new();
    s.run(&stmt("R(x, y) := (1 - x)^2 + 100 * (y - x^2)^2"));
    s.run(&stmt("x := -1.2"));
    s.run(&stmt("y := 1"));
    match s.run(&stmt("minimize(R, x, y)=")) {
        Outcome::Value(Value::Matrix(m)) => {
            let v: Vec<f64> = m.data().iter().map(|v| v.value.re).collect();
            assert!(
                (v[0] - 1.0).abs() < 1e-6 && (v[1] - 1.0).abs() < 1e-6,
                "{v:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn maximize_outside_a_block_of_a_divergent_function_is_an_error() {
    // h(x) = x grows without bound as x grows: no maximum, so an error, not a number.
    let mut s = Session::new();
    s.run(&stmt("h(x) := x"));
    s.run(&stmt("g := 0"));
    assert_error(&s.run(&stmt("maximize(h, g)=")));
}

#[test]
fn outside_a_block_takes_scalar_guesses_only() {
    let mut s = session_with(&["v := [1; 2]"]);
    match s.run(&stmt("minimize(f, v)=")) {
        Outcome::Error(EvalError::Unsupported(_)) | Outcome::Error(EvalError::Solve(_)) => {}
        other => panic!("{other:?}"),
    }
}

// ----- inside a solve block ---------------------------------------------------------------------

#[test]
fn help_constrained_maximum_with_lower_limit() {
    // Help: constraint x > 7, guess 10: maximize = 7.954, minimize = 11.095.
    let mut s = session_with(&["c1 := 7"]);
    block(
        &mut s,
        &[
            ('G', "x := 10"),
            ('C', "x > c1"),
            ('S', "sb_hi := maximize(f, x)="),
        ],
    );
    assert!(
        (scalar_var(&s, "sb_hi") - 7.953650).abs() < 1e-5,
        "{}",
        scalar_var(&s, "sb_hi")
    );
    let mut s = session_with(&["c1 := 7"]);
    block(
        &mut s,
        &[
            ('G', "x := 10"),
            ('C', "x > c1"),
            ('S', "sb_lo := minimize(f, x)="),
        ],
    );
    assert!(
        (scalar_var(&s, "sb_lo") - 11.095243).abs() < 1e-5,
        "{}",
        scalar_var(&s, "sb_lo")
    );
}

#[test]
fn help_bounded_interval_with_guess_six() {
    // Help: 0 < x < 10, guess 6: minimize = 4.812, maximize = 7.954.
    let mut s = session_with(&["c1 := 0", "c2 := 10"]);
    block(
        &mut s,
        &[
            ('G', "x := 6"),
            ('C', "c1 < x"),
            ('C', "x < c2"),
            ('S', "sb_lo := minimize(f, x)="),
        ],
    );
    assert!(
        (scalar_var(&s, "sb_lo") - 4.812057).abs() < 1e-5,
        "{}",
        scalar_var(&s, "sb_lo")
    );
    let mut s = session_with(&["c1 := 0", "c2 := 10"]);
    block(
        &mut s,
        &[
            ('G', "x := 6"),
            ('C', "c1 < x"),
            ('C', "x < c2"),
            ('S', "sb_hi := maximize(f, x)="),
        ],
    );
    assert!(
        (scalar_var(&s, "sb_hi") - 7.953650).abs() < 1e-5,
        "{}",
        scalar_var(&s, "sb_hi")
    );
}

#[test]
fn help_minimum_on_the_constraint_limit() {
    // Help: 5 < x < 10, guess 8: the minimum sits on the upper limit, x = 10 (not a stationary
    // point of f: f'(10) is not zero), the maximum is the interior 7.954.
    let mut s = session_with(&["c1 := 5", "c3 := 10"]);
    block(
        &mut s,
        &[
            ('G', "x := 8"),
            ('C', "c1 < x"),
            ('C', "x < c3"),
            ('S', "sb_lo := minimize(f, x)="),
        ],
    );
    assert!(
        (scalar_var(&s, "sb_lo") - 10.0).abs() < 1e-6,
        "{}",
        scalar_var(&s, "sb_lo")
    );
    let mut s = session_with(&["c1 := 5", "c3 := 10"]);
    block(
        &mut s,
        &[
            ('G', "x := 8"),
            ('C', "c1 < x"),
            ('C', "x < c3"),
            ('S', "sb_hi := maximize(f, x)="),
        ],
    );
    assert!(
        (scalar_var(&s, "sb_hi") - 7.953650).abs() < 1e-5,
        "{}",
        scalar_var(&s, "sb_hi")
    );
}

#[test]
fn block_minimum_with_relaxed_upper_limit_is_stationary() {
    // Help: with c3 = 12 the minimum 11.095 falls inside the constraint, where f' = 0.
    let mut s = session_with(&["c1 := 5", "c3 := 12"]);
    block(
        &mut s,
        &[
            ('G', "x := 8"),
            ('C', "c1 < x"),
            ('C', "x < c3"),
            ('S', "sb_lo := minimize(f, x)="),
        ],
    );
    let x = scalar_var(&s, "sb_lo");
    assert!((x - 11.095243).abs() < 1e-5, "{x}");
    // f'(x) = e^(0.1 x) (cos x + 0.1 sin x) is zero there.
    let slope = x.cos() + 0.1 * x.sin();
    assert!(slope.abs() < 1e-6, "f' = {slope}");
}

#[test]
fn a_function_parameter_as_the_objective_is_refused_with_its_reason() {
    // Inside a user function the objective is a parameter (a value), not a named function: the
    // refusal says so rather than suggesting a name is wrong.
    let mut s = Session::new();
    s.run(&stmt("k(v, x) := minimize(v, x)"));
    s.run(&stmt("g := 0"));
    match s.run(&stmt("k(2, g)=")) {
        Outcome::Error(e) => assert!(e.to_string().contains("parameter"), "{e}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_domain_error_at_a_trial_point_backs_the_search_off() {
    // f = 1000 (x - ln x) is complex for x < 0, and the first trial step from 2 (slope -500) goes
    // there. The step is a failed trial, not an error: the minimum is x = 1.
    let mut s = Session::new();
    s.run(&stmt("f(x) := 1000 * (x - ln(x))"));
    s.run(&stmt("g := 2"));
    match s.run(&stmt("minimize(f, g)=")) {
        Outcome::Value(Value::Scalar(sc)) => {
            assert!((sc.value.re - 1.0).abs() < 1e-6, "{}", sc.value.re)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_domain_error_at_a_trial_point_backs_the_block_search_off() {
    // The same objective as a solver member of a solve block (no constraints).
    let mut s = Session::new();
    s.run(&stmt("f(x) := 1000 * (x - ln(x))"));
    block(&mut s, &[('G', "x := 2"), ('S', "lo := minimize(f, x)=")]);
    assert!(
        (scalar_var(&s, "lo") - 1.0).abs() < 1e-6,
        "{}",
        scalar_var(&s, "lo")
    );
}

#[test]
fn block_quadratic_with_linear_equality() {
    // min (x-3)^2 + (y-2)^2 with x + y == 1: (1, 0).
    let mut s = Session::new();
    s.run(&stmt("Q(x, y) := (x - 3)^2 + (y - 2)^2"));
    let target = Expr::Matrix {
        rows: 2,
        cols: 1,
        elements: vec![Expr::var("px"), Expr::var("py")],
    };
    let members = vec![
        BlockMember {
            kind: MemberKind::Guess,
            body: Ok(stmt("x := 0")),
        },
        BlockMember {
            kind: MemberKind::Guess,
            body: Ok(stmt("y := 0")),
        },
        BlockMember {
            kind: MemberKind::Constraint,
            body: Ok(stmt("x + y == 1")),
        },
        BlockMember {
            kind: MemberKind::Solver,
            body: Ok(Expr::Define {
                target: Box::new(target),
                expr: Box::new(stmt("minimize(Q, x, y)=")),
            }),
        },
    ];
    let outs = s.run_solve_block(&members);
    assert!(!matches!(outs.last(), Some(Outcome::Error(_))), "{outs:?}");
    // A vector target binds each element to its own name (see solve_block::build_answer).
    assert!((scalar_var(&s, "px") - 1.0).abs() < 1e-6, "px");
    assert!(scalar_var(&s, "py").abs() < 1e-6, "py");
}

#[test]
fn block_rosenbrock_with_upper_limit_on_x() {
    // min (1 - x)^2 + 100 (y - x^2)^2 with x <= 0.5. For x <= 0.5 the best y is x^2, leaving
    // (1 - x)^2, which falls as x grows, so the minimum is on the limit: x = 0.5, y = 0.25.
    let mut s = Session::new();
    s.run(&stmt("R(x, y) := (1 - x)^2 + 100 * (y - x^2)^2"));
    let outs = block(
        &mut s,
        &[
            ('G', "x := -1"),
            ('G', "y := 1"),
            ('C', "x <= 0.5"),
            ('S', "xs := minimize(R, x, y)="),
        ],
    );
    assert!(!matches!(outs.last(), Some(Outcome::Error(_))), "{outs:?}");
    let v = column_var(&s, "xs");
    assert!((v[0] - 0.5).abs() < 1e-5, "{v:?}");
    assert!((v[1] - 0.25).abs() < 1e-5, "{v:?}");
}

#[test]
fn block_infeasible_constraints_are_an_error() {
    let mut s = session_with(&[]);
    let outs = block(
        &mut s,
        &[
            ('G', "x := 0.5"),
            ('C', "x > 1"),
            ('C', "x < 0"),
            ('S', "xo := minimize(f, x)="),
        ],
    );
    assert_error(outs.last().expect("one outcome per member"));
    assert!(
        s.env().variable("xo").is_none(),
        "a failed solve binds nothing"
    );
}

#[test]
fn block_unbounded_objective_is_an_error() {
    // min -x with x > 0 and no upper limit: the objective has no minimum.
    let mut s = Session::new();
    s.run(&stmt("L(x) := -x"));
    let outs = block(
        &mut s,
        &[
            ('G', "x := 1"),
            ('C', "x > 0"),
            ('S', "xo := minimize(L, x)="),
        ],
    );
    assert_error(outs.last().expect("one outcome per member"));
}

#[test]
fn block_without_constraints_is_an_unconstrained_search() {
    // An optimisation needs no constraints (find and minerr do).
    let mut s = session_with(&[]);
    block(&mut s, &[('G', "x := -3"), ('S', "lo := minimize(f, x)=")]);
    assert!(
        (scalar_var(&s, "lo") + 1.471128).abs() < 1e-6,
        "{}",
        scalar_var(&s, "lo")
    );
}

#[test]
fn block_objective_must_be_a_name() {
    let mut s = session_with(&[]);
    let outs = block(
        &mut s,
        &[('G', "x := 1"), ('S', "xo := minimize(2 * x, x)=")],
    );
    assert_error(outs.last().expect("one outcome per member"));
}

#[test]
fn block_optimisation_charges_the_work_budget() {
    // Every objective evaluation runs (A*A) for a 250 x 250 identity: about 1.7e8 units each (see
    // solve_block_budget_tests), so the second evaluation exceeds MAX_WORK (3e8) and the block
    // fails with the work limit instead of running on.
    let mut s = Session::new();
    s.run(&stmt("A := identity(250)"));
    s.run(&stmt("H(x) := rows(A * A) * (x - 2)^2"));
    let outs = block(&mut s, &[('G', "x := 1"), ('S', "xo := minimize(H, x)=")]);
    assert!(
        matches!(outs.last(), Some(Outcome::Error(EvalError::WorkLimit(_)))),
        "{outs:?}"
    );
}

#[test]
fn nested_optimisation_recursion_is_refused_not_a_stack_overflow() {
    // The objective calls the optimiser on itself: the depth and work limits stop it.
    let mut s = Session::new();
    s.run(&stmt("D(x) := minimize(D, x)"));
    s.run(&stmt("x := 1"));
    assert_error(&s.run(&stmt("minimize(D, x)=")));
}

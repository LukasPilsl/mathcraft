//! Exact solving: linear systems (unique, singular, inconsistent, symbolic), one-unknown
//! polynomials, and simplification by cancellation.

use super::{eqn, names, sy};
use crate::symbolic::num::int;
use crate::symbolic::{
    degree, eval_rational, simplify, solve_linear_system, solve_polynomial, substitute,
    SolveOutcome, Sym,
};

#[test]
fn quadratic_with_rational_roots() {
    // x^2 - 5x + 6 = 0 -> {2, 3}
    // the quadratic formula's `+` root (3) first, then 2
    let roots = solve_polynomial(&eqn("x^2-5*x+6==0"), "x").expect("solve");
    assert_eq!(roots, vec![Sym::int(3), Sym::int(2)]);
}

#[test]
fn quadratic_with_surd_roots() {
    // x^2 - 2 = 0 -> {sqrt(2), -sqrt(2)}: the quadratic formula's `+` root first
    let roots = solve_polynomial(&eqn("x^2-2==0"), "x").expect("solve");
    let root2 = Sym::pow(Sym::int(2), int(1) / int(2)).expect("pow");
    let neg = Sym::product(vec![Sym::int(-1), root2.clone()]).expect("neg");
    assert_eq!(roots, vec![root2, neg]);
}

#[test]
fn root_order_follows_the_help() {
    // help: x^3+11x^2+3x-135 solve -> 3, -5, -9 (rational root first, then the quadratic)
    let roots = solve_polynomial(&eqn("x^3+11*x^2+3*x-135==0"), "x").expect("solve");
    assert_eq!(roots, vec![Sym::int(3), Sym::int(-5), Sym::int(-9)]);
    // help: x^2 + x - 1 solve -> (sqrt(5)-1)/2, (-sqrt(5)-1)/2
    let roots = solve_polynomial(&eqn("x^2+x-1==0"), "x").expect("solve");
    assert_eq!(roots.len(), 2);
    let approx: Vec<f64> = roots
        .iter()
        .map(|r| crate::symbolic::approximate(r).expect("real root"))
        .collect();
    assert!((approx[0] - 0.618_033_988_7).abs() < 1e-9, "{approx:?}");
    assert!((approx[1] + 1.618_033_988_7).abs() < 1e-9, "{approx:?}");
}

#[test]
fn linear_in_one_unknown_with_symbolic_coefficients() {
    // a*x + b = 0 -> x = -b/a
    let root = solve_polynomial(&eqn("a*x+b==0"), "x").expect("solve");
    assert_eq!(root, vec![sy("-b/a")]);
}

#[test]
fn symbolic_linear_system_keeps_parameters() {
    // a x + b y = 1, x - y = 0 -> x = y = 1/(a+b)
    let sys = [eqn("a*x+b*y==1"), eqn("x-y==0")];
    let got = solve_linear_system(&sys, &names(&["x", "y"])).expect("solve");
    let want = sy("1/(a+b)");
    match got {
        SolveOutcome::Unique(v) => {
            assert_eq!(v[0].1, want);
            assert_eq!(v[1].1, want);
        }
        other => panic!("expected unique, got {other:?}"),
    }
}

#[test]
fn cubic_with_rational_roots() {
    // x^3 - 6x^2 + 11x - 6 = (x-1)(x-2)(x-3): the root 1 is found first, then the quadratic's
    // `+` root (3) before its `-` root (2).
    let roots = solve_polynomial(&eqn("x^3-6*x^2+11*x-6==0"), "x").expect("solve");
    assert_eq!(roots, vec![Sym::int(1), Sym::int(3), Sym::int(2)]);
}

#[test]
fn cubic_without_rational_roots_is_unsupported() {
    let r = solve_polynomial(&eqn("x^3-2==0"), "x");
    assert!(matches!(r, Err(crate::symbolic::SymError::Unsupported(_))));
}

#[test]
fn identity_and_contradiction_for_polynomials() {
    let r = solve_polynomial(&eqn("x-x==0"), "x");
    assert!(matches!(r, Err(crate::symbolic::SymError::Unsupported(_))));
    let r = solve_polynomial(&eqn("x-x==3"), "x").expect("solve");
    assert!(r.is_empty());
}

#[test]
fn singular_consistent_system_is_infinite() {
    // x + y = 1 and 2x + 2y = 2: x = 1 - y
    let sys = [eqn("x+y==1"), eqn("2*x+2*y==2")];
    match solve_linear_system(&sys, &names(&["x", "y"])).expect("solve") {
        SolveOutcome::Infinite { free, solution } => {
            assert_eq!(free, names(&["y"]));
            assert_eq!(solution[0].1, sy("1-y"));
            assert_eq!(solution[1].1, Sym::symbol("y"));
        }
        other => panic!("expected infinite, got {other:?}"),
    }
}

#[test]
fn singular_inconsistent_system_has_no_solution() {
    let sys = [eqn("x+y==1"), eqn("2*x+2*y==3")];
    let got = solve_linear_system(&sys, &names(&["x", "y"])).expect("solve");
    assert_eq!(got, SolveOutcome::NoSolution);
    let sys = [eqn("x==1"), eqn("x==2")];
    let got = solve_linear_system(&sys, &names(&["x"])).expect("solve");
    assert_eq!(got, SolveOutcome::NoSolution);
}

#[test]
fn unknown_missing_from_equations_is_free() {
    let got = solve_linear_system(&[eqn("x==1")], &names(&["x", "z"])).expect("solve");
    match got {
        SolveOutcome::Infinite { free, solution } => {
            assert_eq!(free, names(&["z"]));
            assert_eq!(solution[0].1, Sym::int(1));
            assert_eq!(solution[1].1, Sym::symbol("z"));
        }
        other => panic!("expected infinite, got {other:?}"),
    }
}

#[test]
fn empty_system_leaves_every_unknown_free() {
    let got = solve_linear_system(&[], &names(&["x", "y"])).expect("solve");
    match got {
        SolveOutcome::Infinite { free, .. } => assert_eq!(free, names(&["x", "y"])),
        other => panic!("expected infinite, got {other:?}"),
    }
    let got = solve_linear_system(&[], &[]).expect("solve");
    assert_eq!(got, SolveOutcome::Unique(Vec::new()));
}

#[test]
fn nonlinear_equations_are_reported_not_solved() {
    let got = solve_linear_system(&[eqn("x*y==1")], &names(&["x", "y"])).expect("solve");
    assert_eq!(got, SolveOutcome::NotLinear);
    let got = solve_linear_system(&[eqn("x^2==4")], &names(&["x"])).expect("solve");
    assert_eq!(got, SolveOutcome::NotLinear);
}

#[test]
fn duplicate_unknowns_are_invalid() {
    let r = solve_linear_system(&[eqn("x==1")], &names(&["x", "x"]));
    assert!(matches!(r, Err(crate::symbolic::SymError::Invalid(_))));
}

#[test]
fn simplify_cancels_univariate_common_factors() {
    assert_eq!(simplify(&sy("(x^2-1)/(x-1)")).expect("s"), sy("x+1"));
    assert_eq!(simplify(&sy("(x^3-1)/(x-1)")).expect("s"), sy("x^2+x+1"));
    assert_eq!(
        simplify(&sy("(x^2-4)/(x^2+4*x+4)")).expect("s"),
        sy("(x-2)/(x+2)")
    );
    assert_eq!(
        simplify(&sy("(3+4)/(5^2-1)")).expect("s"),
        Sym::rational(7, 24).expect("q")
    );
}

#[test]
fn substitution_and_evaluation() {
    let e = sy("a*x+b");
    let v = substitute(&e, &[("x", Sym::int(2)), ("a", Sym::int(3))]).expect("subst");
    assert_eq!(v, sy("6+b"));
    let exact =
        eval_rational(&sy("a*x+b"), &[("a", int(3)), ("x", int(2)), ("b", int(1))]).expect("eval");
    assert_eq!(exact, Some(int(7)));
    assert_eq!(
        eval_rational(&sy("a*x+b"), &[("a", int(3))]).expect("eval"),
        None
    );
}

#[test]
fn degree_in_a_variable() {
    assert_eq!(degree(&sy("x^3+2*x+y^5"), "x").expect("d"), Some(3));
    assert_eq!(degree(&sy("y^5"), "x").expect("d"), Some(0));
    assert_eq!(degree(&sy("0"), "x").expect("d"), None);
}

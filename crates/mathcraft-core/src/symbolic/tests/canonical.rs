//! Canonical surds, printed bases, and the conditions a generic answer assumes.

use super::{eqn, names, sy};
use crate::symbolic::{
    from_expr, solve_linear_system_with_conditions, solve_polynomial_with_conditions, to_expr,
    Equation, SolveOutcome, Sym,
};

#[test]
fn products_of_square_roots_merge_into_one_surd() {
    assert_eq!(sy("sqrt(2)*sqrt(3)").to_string(), "6^(1/2)");
    assert_eq!(sy("sqrt(8)*sqrt(3)").to_string(), "2*6^(1/2)");
    assert_eq!(sy("sqrt(2)*sqrt(2)").to_string(), "2");
}

#[test]
fn integer_powers_of_surds_split_the_integer_part() {
    // sqrt(2)^3 = 2*sqrt(2); 2^(5/2) = 4*sqrt(2); 2^(-1/2) = sqrt(2)/2.
    assert_eq!(sy("sqrt(2)^3").to_string(), "2*2^(1/2)");
    assert_eq!(sy("2^(5/2)").to_string(), "4*2^(1/2)");
    assert_eq!(sy("2^(-1/2)").to_string(), "1/2*2^(1/2)");
}

#[test]
fn negative_numeric_base_is_parenthesised_and_round_trips() {
    let v = sy("sqrt(-4)");
    assert_eq!(v.to_string(), "(-4)^(1/2)");
    assert_eq!(from_expr(&to_expr(&v)), Ok(v));
    let w = sy("(-4)^(1/3)");
    assert_eq!(w.to_string(), "(-4)^(1/3)");
    assert_eq!(from_expr(&to_expr(&w)), Ok(w));
}

#[test]
fn printed_text_of_negative_bases_reads_back_to_the_same_value() {
    for src in ["sqrt(-4)", "(-4)^(1/3)", "(-3)^(2/5)*x"] {
        let v = sy(src);
        let text = to_expr(&v).to_string();
        let back = from_expr(
            &crate::parser::parse(&text, &crate::parser::DefaultIdContext).expect("re-parses"),
        );
        assert_eq!(back, Ok(v), "{src} printed as {text}");
    }
}

#[test]
fn symbolic_pivot_is_reported_as_a_condition() {
    // a*x = 0: x = 0 provided a is nonzero.
    let (out, conditions) =
        solve_linear_system_with_conditions(&[eqn("a*x==0")], &names(&["x"])).expect("solve");
    assert_eq!(out, SolveOutcome::Unique(vec![("x".into(), Sym::int(0))]));
    assert_eq!(conditions, vec![Sym::symbol("a")]);
}

#[test]
fn numeric_system_assumes_nothing() {
    let sys: [Equation; 2] = [eqn("x+y==1"), eqn("x-y==0")];
    let (_, conditions) =
        solve_linear_system_with_conditions(&sys, &names(&["x", "y"])).expect("s");
    assert!(conditions.is_empty());
}

#[test]
fn symbolic_linear_root_reports_its_coefficient() {
    let (roots, conditions) = solve_polynomial_with_conditions(&eqn("a*x+b==0"), "x").expect("s");
    assert_eq!(roots, vec![sy("-b/a")]);
    assert_eq!(conditions, vec![Sym::symbol("a")]);
}

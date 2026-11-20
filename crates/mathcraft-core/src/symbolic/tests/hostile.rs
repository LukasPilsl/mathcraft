//! Hostile input: budgets, deep nesting, malformed systems. None of these may panic.

use super::{ex, names, sy};
use crate::expr::Expr;
use crate::symbolic::num::int;
use crate::symbolic::{
    equation_from_expr, expand, from_expr, solve_linear_system, solve_polynomial, Equation, Sym,
    SymError,
};

#[test]
fn huge_exponent_is_refused() {
    assert!(matches!(
        from_expr(&ex("x^1000000000")),
        Err(SymError::TooLarge(_))
    ));
    assert!(matches!(
        from_expr(&ex("x^1001")),
        Err(SymError::TooLarge(_))
    ));
}

#[test]
fn huge_numeric_power_is_refused_by_bit_budget() {
    assert!(matches!(
        from_expr(&ex("2^200000")),
        Err(SymError::TooLarge(_))
    ));
    // Within the caps it is exact.
    assert_eq!(
        sy("2^100"),
        Sym::Num(crate::symbolic::num::from_i128(1 << 100))
    );
}

#[test]
fn million_term_sum_is_refused() {
    let terms: Vec<Sym> = (0..1_000_001i64).map(Sym::int).collect();
    assert!(matches!(Sym::sum(terms), Err(SymError::TooLarge(_))));
}

#[test]
fn expansion_blowup_is_refused() {
    // (a1 + ... + a200)^3 has about 1.3 million monomials.
    let atoms: Vec<Sym> = (0..200).map(|i| Sym::symbol(&format!("a{i:03}"))).collect();
    let sum = Sym::sum(atoms).expect("sum");
    let cube = Sym::pow(sum, int(3)).expect("pow stays symbolic");
    assert!(matches!(expand(&cube), Err(SymError::TooLarge(_))));
}

#[test]
fn deep_nesting_is_refused_on_a_small_stack() {
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut parens = Expr::var("x");
            for _ in 0..400 {
                parens = Expr::Parens(Box::new(parens));
            }
            let mut sum = Expr::var("x");
            for _ in 0..400 {
                sum = Expr::binary(crate::expr::BinaryOp::Add, sum, Expr::var("y"));
            }
            (from_expr(&parens), from_expr(&sum))
        })
        .expect("spawn");
    let (a, b) = handle.join().expect("no stack overflow");
    assert_eq!(a, Err(SymError::TooDeep));
    assert_eq!(b, Err(SymError::TooDeep));
}

#[test]
fn empty_and_duplicate_systems() {
    assert!(matches!(
        solve_linear_system(&[], &names(&["x", "x"])),
        Err(SymError::Invalid(_))
    ));
}

#[test]
fn too_many_unknowns_is_refused() {
    let many: Vec<String> = (0..61).map(|i| format!("u{i}")).collect();
    assert!(matches!(
        solve_linear_system(&[], &many),
        Err(SymError::TooLarge(_))
    ));
}

#[test]
fn malformed_equation_is_unsupported() {
    let e = Expr::SymEval {
        expr: Box::new(Expr::var("x")),
        command: None,
        result: None,
    };
    assert!(matches!(
        crate::symbolic::equation_from_expr(&e),
        Err(SymError::Unsupported(_))
    ));
    assert!(equation_from_expr(&ex("x==1")).is_ok());
}

#[test]
fn polynomial_with_identically_zero_leading_part_is_handled() {
    let eq = Equation {
        lhs: Sym::symbol("x"),
        rhs: Sym::symbol("x"),
    };
    assert!(matches!(
        solve_polynomial(&eq, "x"),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn unknown_inside_a_function_is_unsupported() {
    let eq = Equation {
        lhs: Sym::func("sin", vec![Sym::symbol("x")]).expect("shallow"),
        rhs: Sym::int(0),
    };
    assert!(matches!(
        solve_polynomial(&eq, "x"),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn division_by_zero_is_undefined() {
    assert!(matches!(
        from_expr(&ex("1/(2-2)")),
        Err(SymError::Undefined(_))
    ));
    assert!(matches!(
        from_expr(&ex("0^(-1)")),
        Err(SymError::Undefined(_))
    ));
}

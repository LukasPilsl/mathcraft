//! Canonical form: constants, powers, ordering, round trips with `Expr`.

use super::{ex, sy};
use crate::expr::Expr;
use crate::symbolic::num::{int, Q};
use crate::symbolic::{from_expr, to_expr, Sym};

fn half() -> Q {
    int(1) / int(2)
}

#[test]
fn decimal_text_is_exact() {
    let e = Expr::Number(crate::expr::Number::Real {
        value: 0.1,
        text: "0.1".into(),
    });
    assert_eq!(from_expr(&e), Ok(Sym::Num(int(1) / int(10))));
    let e = Expr::Number(crate::expr::Number::Real {
        value: 0.0025,
        text: "2.5e-3".into(),
    });
    assert_eq!(from_expr(&e), Ok(Sym::Num(int(1) / int(400))));
}

#[test]
fn constant_expression_folds_exactly() {
    // 1_rata_1 r0: (3+4)/(5^2-1) -> 7/24
    assert_eq!(sy("(3+4)/(5^2-1)"), Sym::rational(7, 24).expect("q"));
}

#[test]
fn roots_are_exact_where_rational() {
    assert_eq!(sy("sqrt(4)"), Sym::int(2));
    assert_eq!(sy("sqrt(1/4)"), Sym::rational(1, 2).expect("q"));
    assert_eq!(sy("sqrt(2)"), Sym::pow(Sym::int(2), half()).expect("pow"));
}

#[test]
fn square_factors_are_extracted_from_surds() {
    // sqrt(8) = 2 * 2^(1/2)
    let want = Sym::product(vec![
        Sym::int(2),
        Sym::pow(Sym::int(2), half()).expect("pow"),
    ])
    .expect("product");
    assert_eq!(sy("sqrt(8)"), want);
}

#[test]
fn nth_root_of_a_rational_sum() {
    // 1_rata_1 r4: the fifth root of 3 + 2*sqrt(4) -> 7^(1/5). Text form is radicand first
    // (Mathcad's root(x, n)); the MathML nthRoot takes the degree first.
    let want = Sym::pow(Sym::int(7), int(1) / int(5)).expect("pow");
    assert_eq!(sy("root(3+2*sqrt(4),5)"), want);
}

#[test]
fn rational_powers_of_a_symbol_combine() {
    // 1_rata_1 r3: x^5*6x^3/(x^(1/2) x^(5/3)) -> 6*x^(35/6)
    let want = Sym::product(vec![
        Sym::int(6),
        Sym::pow(Sym::symbol("x"), int(35) / int(6)).expect("pow"),
    ])
    .expect("product");
    assert_eq!(sy("x^5*(6*x^3)/(x^(1/2)*x^(5/3))"), want);
    assert_eq!(
        sy("x^5*(6*x^3)/(x^(1/2)*x^(5/3))").to_string(),
        "6*x^(35/6)"
    );
}

#[test]
fn sums_order_reverse_alphabetically_with_constant_last() {
    // 2_rata_2 r4: b + a
    assert_eq!(sy("a+b").to_string(), "b + a");
    // 3_rata_3 r3: a x^2 + b x + c
    assert_eq!(sy("c+b*x+a*x^2").to_string(), "a*x^2 + b*x + c");
    // 3_rata_3 r4 (value form): 3/2 x^2 - 21/2 x + 16
    assert_eq!(sy("16-21/2*x+3/2*x^2").to_string(), "3/2*x^2 - 21/2*x + 16");
}

#[test]
fn products_list_factors_alphabetically() {
    assert_eq!(sy("b*a*c^2").to_string(), "a*b*c^2");
}

#[test]
fn like_terms_combine_and_zero_disappears() {
    assert_eq!(sy("2*x+3*x-5*x"), Sym::int(0));
    assert_eq!(sy("x-x+4"), Sym::int(4));
    assert_eq!(sy("x*x"), sy("x^2"));
    assert_eq!(sy("x*x^-1"), Sym::int(1));
}

#[test]
fn numeric_coefficient_distributes_over_a_sum() {
    // 2*(a+b) is canonical as 2a + 2b; the printed shape follows the sum rule.
    assert_eq!(sy("2*(a+b)"), sy("2*a+2*b"));
}

#[test]
fn rebuild_is_a_fixed_point() {
    let samples = [
        "(a+b)^2",
        "x^5*(6*x^3)/(x^(1/2)*x^(5/3))",
        "3/2*x^2 - 21/2*x + 16",
        "sqrt(8)+sqrt(2)",
        "-(a-b)*(c+2)",
        "root(3+2*sqrt(4),5)",
        "ln(x)+sin(2*x)/x",
    ];
    for s in samples {
        let v = sy(s);
        assert_eq!(v.rebuild().expect("rebuild"), v, "fixed point of {s}");
    }
}

#[test]
fn expr_round_trip_preserves_the_canonical_value() {
    let samples = [
        "(3+4)/(5^2-1)",
        "a^2 - 2*a*b + b^2",
        "sqrt(8)*x",
        "x^(1/3)+1",
        "1/(x+2)",
    ];
    for s in samples {
        let v = sy(s);
        let back = from_expr(&to_expr(&v)).expect("back");
        assert_eq!(back, v, "round trip of {s}");
    }
}

#[test]
fn printed_rational_matches_prime_style() {
    assert_eq!(
        to_expr(&Sym::rational(7, 24).expect("q")).to_string(),
        "7/24"
    );
}

#[test]
fn unsupported_nodes_are_errors_not_panics() {
    let e = Expr::SymEval {
        expr: Box::new(ex("x")),
        command: None,
        result: None,
    };
    assert!(matches!(
        from_expr(&e),
        Err(crate::symbolic::SymError::Unsupported(_))
    ));
    assert!(matches!(
        from_expr(&ex("x==2")),
        Err(crate::symbolic::SymError::Unsupported(_))
    ));
}

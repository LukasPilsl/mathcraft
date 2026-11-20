//! Printed forms of symbolic results and value round trips: `factor` then `expand` gives the
//! polynomial back, `collect` keeps the value, and the printed text reads back to the same
//! canonical value (see `show.rs`).

use super::{ex, sy, Lcg};
use crate::symbolic::{collect, expand, factor, from_expr, shown_expr, simplify, Sym};

/// The canonical value of the printed text of `s` (read back by the parser).
fn read_back(s: &Sym, factored: bool) -> Sym {
    let text = shown_expr(s, factored).expect("prints").to_string();
    from_expr(&ex(&text)).unwrap_or_else(|e| panic!("`{text}` does not read back: {e:?}"))
}

#[test]
fn factor_then_expand_gives_the_polynomial_back() {
    for src in [
        "x^2-1",
        "x^3-x",
        "2*x^2+4*x",
        "x^2+5*x+6",
        "a^2-b^2",
        "4*x^2-9",
        "x^2-2*x+1",
    ] {
        let s = simplify(&sy(src)).unwrap();
        let f = factor(&s).unwrap();
        assert_eq!(expand(&f).unwrap(), expand(&s).unwrap(), "{src} -> {f}");
    }
}

#[test]
fn random_products_of_linear_factors_round_trip() {
    let mut rng = Lcg(7);
    for _ in 0..200 {
        let (a, b, c) = (rng.range(-5, 5), rng.range(-5, 5), rng.range(-5, 5));
        let src = format!("(x+{a})*(x+{b})*(x-{c})");
        let s = simplify(&sy(&src)).unwrap();
        let p = expand(&s).unwrap();
        let f = factor(&p).unwrap();
        assert_eq!(expand(&f).unwrap(), p, "{src} -> {f}");
    }
}

#[test]
fn collect_keeps_the_value_and_groups_by_the_variable() {
    let p = expand(&sy("(x+y)*(2*x-y+3)*(x^2+1)")).unwrap();
    for var in ["x", "y"] {
        let c = collect(&p, var).unwrap();
        assert_eq!(expand(&c).unwrap(), p, "collect {var}");
    }
}

#[test]
fn printed_numbers_read_back_exactly() {
    let mut rng = Lcg(11);
    for _ in 0..300 {
        let n = rng.range(-500, 500);
        let d = rng.range(1, 60);
        let q = Sym::rational(n, d).unwrap();
        assert_eq!(read_back(&q, false), q, "plain {n}/{d}");
        assert_eq!(read_back(&q, true), q, "factored {n}/{d}");
    }
}

#[test]
fn printed_polynomials_and_fractions_read_back_to_the_same_value() {
    for src in [
        "5*x^4+10*x^3+x^2",
        "x^2-1",
        "(x-1)*(x+1)",
        "2/((x+2)*(x+3))",
        "-((b-a)*(b+a))",
        "6*x^(35/6)",
        "7^(1/5)",
        "2*x^(-3)+1/y",
        "x*(7*y^2+y)+x^2*(5*y^2+3*y)",
        "sqrt(2)*sqrt(3)*x",
    ] {
        let s = simplify(&sy(src)).unwrap();
        assert_eq!(read_back(&s, false), s, "{src}");
    }
}

#[test]
fn factored_integer_products_print_as_prime_powers() {
    let cases = [
        (12, "2^2*3"),
        (1, "1"),
        (7, "7"),
        (360, "2^3*3^2*5"),
        (-12, "-(2^2*3)"),
    ];
    for (n, text) in cases {
        let s = Sym::int(n);
        assert_eq!(shown_expr(&s, true).unwrap().to_string(), text, "{n}");
        assert_eq!(read_back(&s, true), s, "{n} reads back");
    }
}

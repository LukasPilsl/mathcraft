//! `factor`: help and sample forms, and property tests (`expand(factor(p)) == expand(p)`).

use super::{sy, Lcg};
use crate::symbolic::num::int;
use crate::symbolic::{eval_rational, expand, factor, simplify, Sym, SymError};

/// Factors source text, checks the value is unchanged at sample points, and returns the result.
fn factored(src: &str) -> Sym {
    let s = simplify(&sy(src)).expect("simplify");
    let f = factor(&s).expect("factor");
    // Points avoid the poles of the rational-function samples (x = -2, -3).
    for x in [-7, -1, 0, 4, 9] {
        let at = [("x", int(x)), ("a", int(2)), ("b", int(-1))];
        assert_eq!(
            eval_rational(&f, &at).expect("eval"),
            eval_rational(&s, &at).expect("eval"),
            "factor changed the value of {src}: {f}"
        );
    }
    f
}

/// The factors of a product (the value itself for a non-product).
fn factor_list(f: &Sym) -> Vec<Sym> {
    match f {
        Sym::Mul(fs) => fs.clone(),
        other => vec![other.clone()],
    }
}

#[test]
fn univariate_rational_roots() {
    // x^2 - 5x + 6 = (x - 2)(x - 3): two linear factors
    assert_eq!(factor_list(&factored("x^2-5*x+6")).len(), 2);
    // 2x^2 - 2 = 2 (x - 1)(x + 1): the constant and two linear factors
    assert_eq!(factor_list(&factored("2*x^2-2")).len(), 3);
    // x^3 - x = x (x - 1)(x + 1)
    assert_eq!(factor_list(&factored("x^3-x")).len(), 3);
}

#[test]
fn univariate_irreducible_remainder_is_kept() {
    // x^2 + 1 has no rational root: a single factor.
    assert_eq!(factor_list(&factored("x^2+1")).len(), 1);
    // x^3 - 2 has no rational root and degree 3: irreducible over Q.
    assert_eq!(factor_list(&factored("x^3-2")).len(), 1);
}

#[test]
fn univariate_fraction_coefficients() {
    // x/2 + 1/3 is linear: irreducible, kept as it is (value checked in `factored`)
    assert_eq!(factor_list(&factored("x/2+1/3")).len(), 1);
    // x^2/2 + x/3 = x (x/2 + 1/3): the root 0 is found; the constant joins the sum (canonical
    // products put a numeric coefficient inside a single sum)
    assert_eq!(factor_list(&factored("x^2/2+x/3")).len(), 2);
}

#[test]
fn univariate_quartic_without_rational_root_is_refused() {
    let s = simplify(&sy("x^4+1")).unwrap();
    assert!(matches!(factor(&s), Err(SymError::Unsupported(_))));
}

#[test]
fn difference_of_squares_and_square_trinomial() {
    // a^2 - b^2 = (a - b)(a + b)  (help sample 2_rata_2 r3, value only)
    assert_eq!(factor_list(&factored("a^2-b^2")).len(), 2);
    // a^2 + 2 a b + b^2 = (a + b)^2  (sample 1_rata_1 r11): one power of a sum
    assert!(matches!(factored("a^2+2*a*b+b^2"), Sym::Pow(_, _)));
    // a^2 - 2 a b + b^2 = (a - b)^2
    assert!(matches!(factored("a^2-2*a*b+b^2"), Sym::Pow(_, _)));
}

#[test]
fn common_monomial_and_content() {
    // 2 x^2 y + 4 x y^2 = 2 x y (x + 2 y): constant, x, y and one sum
    assert_eq!(factor_list(&factored("2*x^2*y+4*x*y^2")).len(), 4);
}

#[test]
fn rational_function_factors_are_kept_as_products() {
    // (1/(x+2))(2/(x+3)) = 2/((x+2)(x+3)) (sample 1_rata_1 r13); values checked in `factored`
    let _ = factored("(1/(x+2))*(2/(x+3))");
}

#[test]
fn multivariate_sum_without_pattern_is_refused() {
    let s = simplify(&sy("a^2+b^3+a*b")).unwrap();
    assert!(matches!(factor(&s), Err(SymError::Unsupported(_))));
}

/// Multivariate sums that split into linear factors: the product must expand back to the input
/// (checked inside `factor` and again here) and have the expected number of factors.
#[test]
fn multivariate_linear_factors() {
    for (src, count) in [
        ("x^2-x*y-2*y^2", 2),
        ("x^2*y-y", 2),
        ("a*x+a*y+b*x+b*y", 2),
        ("x^3+3*x^2*y+3*x*y^2+y^3", 1),
        ("2*x^2+3*x*y+y^2", 2),
        ("x*y+x+y+1", 2),
        ("x^2*y^2-1", 2),
    ] {
        let s = expand(&simplify(&sy(src)).unwrap()).unwrap();
        let f = factor(&s).unwrap_or_else(|e| panic!("{src}: {e}"));
        assert_eq!(expand(&f).unwrap(), s, "round trip of {src}");
        assert!(
            factor_list(&f).len() >= count,
            "{src} gave {f} ({} factors)",
            factor_list(&f).len()
        );
    }
}

/// Random product of linear factors with integer roots and a constant: the expansion must factor
/// back to the same polynomial (property test, deterministic).
#[test]
fn random_integer_root_polynomials_round_trip() {
    let mut rng = Lcg(7);
    let mut shown = 0;
    for _ in 0..60 {
        let mut src = format!("{}", rng.range(-5, 5).max(1));
        let count = rng.range(1, 4);
        for _ in 0..count {
            let r = rng.range(-4, 4);
            src.push_str(&format!("*(x-({r}))"));
        }
        let p = expand(&simplify(&sy(&src)).unwrap()).unwrap();
        // An integer content on a sum (`4x - 12`) cannot be kept in the canonical form: refused.
        match factor(&p) {
            Ok(f) => {
                assert_eq!(expand(&f).unwrap(), p, "round trip of {src}");
                shown += 1;
            }
            Err(SymError::Unsupported(_)) => {}
            Err(e) => panic!("factor of {src}: {e}"),
        }
    }
    assert!(shown >= 20, "only {shown} of 60 factored");
}

/// Random square trinomials p^2 +- 2pq + q^2 over two symbols (property test).
#[test]
fn random_square_trinomials_round_trip() {
    let mut rng = Lcg(11);
    for _ in 0..40 {
        let k = rng.range(1, 3);
        let sign = if rng.range(0, 1) == 0 { "+" } else { "-" };
        let src = format!("({k}*a{sign}b)^2");
        let p = expand(&simplify(&sy(&src)).unwrap()).unwrap();
        let f = factor(&p).unwrap_or_else(|e| panic!("factor of {src}: {e}"));
        assert_eq!(expand(&f).unwrap(), p, "round trip of {src}");
    }
}

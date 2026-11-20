//! Integer factoring (trial division, Pollard's rho) and the refusals that keep `factor` from
//! presenting a partial factorisation as complete.

use super::{sy, Lcg};
use crate::symbolic::factor::integer_factors;
use crate::symbolic::{factor, simplify, Sym, SymError};

#[test]
fn integer_factors_match_the_help() {
    // help: 251357832 = 2^3 * 3^2 * 11 * 317371; 12 = 2^2 * 3
    assert_eq!(
        integer_factors(251_357_832).unwrap(),
        vec![(2, 3), (3, 2), (11, 1), (317_371, 1)]
    );
    assert_eq!(integer_factors(12).unwrap(), vec![(2, 2), (3, 1)]);
}

#[test]
fn trivial_and_prime_integers() {
    assert!(integer_factors(0).unwrap().is_empty());
    assert!(integer_factors(1).unwrap().is_empty());
    assert_eq!(integer_factors(2).unwrap(), vec![(2, 1)]);
    assert_eq!(
        integer_factors(1_000_000_007).unwrap(),
        vec![(1_000_000_007, 1)]
    );
    assert_eq!(integer_factors(1u64 << 40).unwrap(), vec![(2, 40)]);
}

#[test]
fn two_large_primes_need_pollards_rho() {
    // Both factors are above the trial division limit (2^20). The product is below 2^64.
    let (p, q) = (4_294_967_279u64, 4_294_967_291u64);
    assert_eq!(integer_factors(p * q).unwrap(), vec![(p, 1), (q, 1)]);
}

#[test]
fn random_products_factor_back() {
    let mut rng = Lcg(7);
    for _ in 0..200 {
        let n = 2 + rng.next_u64() % 1_000_000_000;
        let product: u64 = integer_factors(n)
            .unwrap()
            .iter()
            .map(|&(p, e)| p.pow(e))
            .product();
        assert_eq!(product, n);
    }
}

#[test]
fn composite_integer_keeps_its_value_and_prints_as_prime_powers() {
    // The canonical value stays 12; the printed form (help: 12 -> 2^2*3) carries the factors.
    assert_eq!(factor(&Sym::int(12)).unwrap(), Sym::int(12));
    let shown = crate::symbolic::shown_expr(&Sym::int(12), true).unwrap();
    assert_eq!(shown.to_string(), "2^2*3");
    assert_eq!(factor(&Sym::int(7)).unwrap(), Sym::int(7));
}

#[test]
fn integer_coefficient_on_a_monomial_is_kept_and_printed_factored() {
    // 12x: the canonical product is kept, and its print is 2^2*3*x.
    let s = simplify(&sy("12*x")).unwrap();
    assert_eq!(factor(&s).unwrap(), s);
    let shown = crate::symbolic::shown_expr(&s, true).unwrap();
    assert_eq!(shown.to_string(), "2^2*3*x");
}

#[test]
fn integer_content_of_a_sum_is_refused_not_returned_unchanged() {
    // 2x + 2y + 4 would come back as the same sum: refused.
    let s = simplify(&sy("2*x+2*y+4")).unwrap();
    assert!(matches!(factor(&s), Err(SymError::Unsupported(_))));
}

#[test]
fn prime_content_with_a_common_monomial_is_kept() {
    // 2x^2 + 4x = 2 x (x + 2): the content 2 is prime and multiplies a monomial, so it is shown.
    let s = simplify(&sy("2*x^2+4*x")).unwrap();
    let f = factor(&s).unwrap();
    match &f {
        Sym::Mul(fs) => assert_eq!(fs.len(), 3, "{f}"),
        other => panic!("expected a product, got {other}"),
    }
}

//! Rational root search: bounded candidates and divisor enumeration.

use std::time::{Duration, Instant};

use super::eqn;
use crate::symbolic::{solve_polynomial, Sym, SymError};

fn solve_timed(src: &str) -> (Result<Vec<Sym>, SymError>, Duration) {
    let e = eqn(src);
    let start = Instant::now();
    let out = solve_polynomial(&e, "x");
    (out, start.elapsed())
}

#[test]
fn large_constant_with_many_divisors_is_refused_quickly() {
    // 963761198400 has 6720 divisors: 6720*2 candidates over the leading coefficient alone.
    // Constant 1 keeps the content at 1, so the cap (not content removal) decides.
    let (out, took) = solve_timed("963761198400*x^4+1==0");
    assert_eq!(
        out,
        Err(SymError::Unsupported(
            "rational root search too large".into()
        ))
    );
    assert!(took < Duration::from_secs(1), "took {took:?}");
}

#[test]
fn constant_term_above_64_bits_is_refused_quickly() {
    // 2^100 + x^4: the constant has 101 bits.
    let (out, took) = solve_timed("x^4+1267650600228229401496703205376==0");
    assert_eq!(
        out,
        Err(SymError::Unsupported(
            "coefficients too large for root search".into()
        ))
    );
    assert!(took < Duration::from_secs(1), "took {took:?}");
}

#[test]
fn cubic_with_three_integer_roots_is_still_found() {
    // Search order: 1 (first candidate that is a root), then the quadratic's `+` root 3, `-` root 2.
    let (out, _) = solve_timed("x^3-6*x^2+11*x-6==0");
    assert_eq!(out, Ok(vec![Sym::int(1), Sym::int(3), Sym::int(2)]));
}

#[test]
fn quadratic_with_rational_roots_is_still_found() {
    // 6x^2 - x - 1 = (3x + 1)(2x - 1): roots 1/2 (the `+` root) and -1/3.
    let (out, _) = solve_timed("6*x^2-x-1==0");
    assert_eq!(
        out,
        Ok(vec![
            Sym::rational(1, 2).expect("q"),
            Sym::rational(-1, 3).expect("q"),
        ])
    );
}

#[test]
fn scaled_cubic_with_integer_roots_is_found() {
    // 2520*(x-1)(x-2)(x+3): content 2520 is removed before the root search. Root 1 is found
    // first, then the quadratic x^2+x-6 gives 2 (`+`) and -3.
    let (out, _) = solve_timed("2520*x^3-17640*x+15120==0");
    assert_eq!(out, Ok(vec![Sym::int(1), Sym::int(2), Sym::int(-3)]));
}

#[test]
fn scaled_cubic_5040_with_integer_roots_is_found() {
    // 5040*(x-1)(x-2)(x+3).
    let (out, _) = solve_timed("5040*x^3-35280*x+30240==0");
    assert_eq!(out, Ok(vec![Sym::int(1), Sym::int(2), Sym::int(-3)]));
}

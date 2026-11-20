//! Partial fractions over Q: the help's examples, recombination, and refusals.

use crate::symbolic::{parfrac, Sym, SymError};

use super::{same_function, sy, Lcg};

#[test]
fn help_example_with_a_quadratic_leftover() {
    // 1/((x^2+1)(x^2-1)) parfrac -> 1/(4(x-1)) - (1/(2(x^2+1)) + 1/(4(x+1)))
    let input = sy("1/((x^2+1)*(x^2-1))");
    let out = parfrac(&input, "x").expect("decomposes");
    assert!(same_function(&out, &input));
    let want = sy("1/(4*(x-1)) - 1/(2*(x^2+1)) - 1/(4*(x+1))");
    assert!(same_function(&out, &want));
}

#[test]
fn help_example_with_two_linear_factors() {
    // 3/(x^2-8x+15) = 3/(2(x-5)) - 3/(2(x-3)).
    let input = sy("3/(x^2-8*x+15)");
    let out = parfrac(&input, "x").expect("decomposes");
    assert!(same_function(&out, &input));
    assert_eq!(out, sy("3/2*(x-5)^(-1) - 3/2*(x-3)^(-1)"));
}

#[test]
fn repeated_roots_and_polynomial_parts() {
    let cube = sy("1/x^3");
    assert_eq!(parfrac(&cube, "x"), Ok(sy("x^(-3)")));
    let mixed = sy("(x^3+2*x+1)/(x^2*(x-1))");
    let out = parfrac(&mixed, "x").expect("decomposes");
    assert!(same_function(&out, &mixed));
    // A polynomial part is kept: (x^3+1)/(x-1) = x^2+x+1 + 2/(x-1).
    let poly_part = sy("(x^3+1)/(x-1)");
    let out = parfrac(&poly_part, "x").expect("decomposes");
    assert!(same_function(&out, &poly_part));
}

#[test]
fn a_leftover_of_degree_four_is_refused() {
    // (x^2+1)(x^2-2) has no rational root and may split into quadratics over Q: refused.
    let input = sy("1/((x^2+1)*(x^2-2))");
    assert!(matches!(
        parfrac(&input, "x"),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn a_non_rational_expression_is_refused() {
    assert!(matches!(
        parfrac(&sy("sin(x)/x"), "x"),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn random_decompositions_recombine_exactly() {
    // Random products of linear factors (roots -3..3, multiplicities 1-2) and an optional
    // irreducible quadratic, with a numerator of lower degree: the answer recombines exactly.
    let mut rng = Lcg(11);
    for _ in 0..120 {
        let mut den = String::from("1");
        let mut roots = Vec::new();
        for _ in 0..rng.range(1, 3) {
            let r = rng.range(-3, 3);
            if roots.contains(&r) {
                continue;
            }
            roots.push(r);
            let m = rng.range(1, 2);
            den.push_str(&format!("*(x-({r}))^{m}"));
        }
        if rng.range(0, 1) == 1 {
            den.push_str("*(x^2+1)");
        }
        let deg = {
            let d = sy(&format!("({den})"));
            crate::symbolic::degree(&d, "x").unwrap().unwrap_or(0) as i64
        };
        let terms: Vec<String> = (0..deg)
            .map(|k| format!("({})*x^{k}", rng.range(-5, 5)))
            .collect();
        let num = if terms.is_empty() {
            "0".to_string()
        } else {
            terms.join(" + ")
        };
        let src = format!("({num})/({den})");
        let input = sy(&src);
        let out: Sym = parfrac(&input, "x").unwrap_or_else(|e| panic!("{src}: {e:?}"));
        assert!(same_function(&out, &input), "{src}");
    }
}

#[test]
fn complex_constant_is_refused() {
    // (-1)^(1/2) is i: parfrac answers only real constants.
    assert!(matches!(
        parfrac(&sy("(-1)^(1/2)"), "x"),
        Err(SymError::Unsupported(_))
    ));
    assert_eq!(parfrac(&sy("3"), "x"), Ok(sy("3")));
}

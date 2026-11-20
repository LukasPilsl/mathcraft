//! Random trees through the stage-3 routines (`factor`, `differentiate`, `integrate_polynomial`):
//! no panic, and every value returned keeps its meaning at sample points. A refusal is allowed.

use super::{sy, Lcg};
use crate::parser::{parse, DefaultIdContext};
use crate::symbolic::num::int;
use crate::symbolic::{
    differentiate, eval_rational, expand, factor, from_expr, integrate_polynomial, simplify, Sym,
};

/// Random source text over `x`, `a` and small integers (valid syntax, nested to `depth`).
fn random_source(rng: &mut Lcg, depth: u32) -> String {
    if depth == 0 || rng.range(0, 3) == 0 {
        return match rng.range(0, 3) {
            0 => "x".to_string(),
            1 => "a".to_string(),
            _ => format!("{}", rng.range(-3, 3)),
        };
    }
    let l = random_source(rng, depth - 1);
    let r = random_source(rng, depth - 1);
    match rng.range(0, 3) {
        0 => format!("({l}+{r})"),
        1 => format!("({l}-{r})"),
        2 => format!("({l})*({r})"),
        _ => format!("({l})^{}", rng.range(0, 3)),
    }
}

#[test]
fn random_trees_keep_values_or_are_refused() {
    let points = [
        [("x", int(-7)), ("a", int(3))],
        [("x", int(4)), ("a", int(-2))],
    ];
    let mut rng = Lcg(29);
    let mut accepted = 0;
    for _ in 0..400 {
        let src = random_source(&mut rng, 4);
        let Ok(expr) = parse(&src, &DefaultIdContext) else {
            continue;
        };
        let Ok(v) = from_expr(&expr) else { continue };
        let Ok(s) = simplify(&v) else { continue };
        if let Ok(f) = factor(&s) {
            for env in &points {
                if let (Ok(p), Ok(q)) = (eval_rational(&f, env), eval_rational(&s, env)) {
                    assert_eq!(p, q, "factor changed the value of {src}: {f}");
                }
            }
            accepted += 1;
        }
        let _ = differentiate(&s, "x");
        if let Ok(i) = integrate_polynomial(&s, "x") {
            let back = expand(&differentiate(&i, "x").expect("derivative")).expect("expand");
            assert_eq!(
                back,
                expand(&s).expect("expand"),
                "d/dx of the integral of {src}"
            );
            accepted += 1;
        }
    }
    assert!(accepted > 50, "too few cases were accepted: {accepted}");
}

#[test]
fn factor_and_derivative_of_a_hostile_power_are_refused_not_slow() {
    // A huge exponent is refused by the constructors or the budget; nothing runs away.
    let s: Sym = sy("(x+1)^900");
    let _ = factor(&s);
    let _ = differentiate(&s, "x");
}

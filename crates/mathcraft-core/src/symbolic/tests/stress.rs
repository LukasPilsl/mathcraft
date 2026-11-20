//! Hostile and random input: deep trees, explosive exponents and sizes, and a deterministic
//! fuzz through every public function. Nothing may panic, overflow the stack, or run for
//! more than a second in a single call (debug build).

use std::time::{Duration, Instant};

use super::{ex, Lcg};
use crate::expr::{BinaryOp, Expr, Number, UnaryOp};
use crate::parser::{parse, DefaultIdContext};
use crate::symbolic::num::int;
use crate::symbolic::{
    degree, equation_from_expr, eval_rational, expand, from_expr, simplify, solve_linear_system,
    solve_polynomial, substitute, to_expr, Sym, SymError,
};

/// Runs `f` on a thread with a 2 MiB stack; a stack overflow would abort the test process.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("no stack overflow")
}

#[test]
fn ten_thousand_deep_input_is_refused_on_a_small_stack() {
    on_small_stack(|| {
        let mut parens = Expr::var("x");
        let mut sum = Expr::var("x");
        let mut neg = Expr::var("x");
        for _ in 0..10_000 {
            parens = Expr::Parens(Box::new(parens));
            sum = Expr::binary(BinaryOp::Add, sum, Expr::var("y"));
            neg = Expr::unary(UnaryOp::Neg, neg);
        }
        assert_eq!(from_expr(&parens), Err(SymError::TooDeep));
        assert_eq!(from_expr(&sum), Err(SymError::TooDeep));
        assert_eq!(from_expr(&neg), Err(SymError::TooDeep));
        let eq = Expr::binary(BinaryOp::Eq, sum, Expr::var("x"));
        assert_eq!(equation_from_expr(&eq).map(|_| ()), Err(SymError::TooDeep));
    });
}

#[test]
fn ten_thousand_deep_value_is_refused_by_the_constructors() {
    // Nesting through function arguments, rational powers, sums and products is bounded at
    // the constructors (MAX_DEPTH), so the 10 000-level attempts stop near level 100 and the
    // value-level functions never see a deep tree.
    on_small_stack(|| {
        let mut f = Sym::symbol("x");
        let mut func_err = None;
        for _ in 0..10_000 {
            match Sym::func("g", vec![f.clone()]) {
                Ok(v) => f = v,
                Err(e) => {
                    func_err = Some(e);
                    break;
                }
            }
        }
        assert_eq!(func_err, Some(SymError::TooDeep));
        let started = Instant::now();
        let _ = expand(&f);
        let _ = simplify(&f);
        let _ = substitute(&f, &[("x", Sym::int(2))]);
        let _ = f.to_string();
        let _ = to_expr(&f);
        assert!(started.elapsed() < Duration::from_secs(5));

        let mut p = Sym::symbol("x");
        let mut pow_refused = false;
        for _ in 0..10_000 {
            match Sym::pow(p.clone(), int(1) / int(3)) {
                Ok(v) => p = v,
                Err(e) => {
                    assert_eq!(e, SymError::TooDeep);
                    pow_refused = true;
                    break;
                }
            }
        }
        assert!(pow_refused);

        let mut acc = Sym::symbol("x");
        let mut prod_refused = false;
        for _ in 0..10_000 {
            let step = Sym::sum(vec![acc.clone(), Sym::symbol("y")])
                .and_then(|sum| Sym::product(vec![sum, Sym::symbol("z")]));
            match step {
                Ok(v) => acc = v,
                Err(e) => {
                    assert_eq!(e, SymError::TooDeep);
                    prod_refused = true;
                    break;
                }
            }
        }
        assert!(prod_refused);
    });
}

#[test]
fn exponent_chain_stays_exact_and_small() {
    // x^(1/3)^(1/3)^... (90 levels): the exponent is 1/3^90, a 143-bit rational.
    let mut e = Expr::var("x");
    for _ in 0..90 {
        e = Expr::binary(
            BinaryOp::Pow,
            e,
            Expr::Number(Number::Rational(int(1) / int(3))),
        );
    }
    let started = Instant::now();
    let v = from_expr(&e).expect("within the depth and size limits");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(v.to_string().len() < 1_000, "{v}");
}

#[test]
fn balanced_million_term_sum_is_refused_without_building_the_tree_twice() {
    // A balanced tree keeps the recursion shallow (depth 20) while the sum has 10^6 terms.
    let mut level: Vec<Expr> = (0..1_000_000)
        .map(|i| Expr::var(&format!("x{i}")))
        .collect();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len() / 2 + 1);
        let mut it = level.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => next.push(Expr::binary(BinaryOp::Add, a, b)),
                None => next.push(a),
            }
        }
        level = next;
    }
    let started = Instant::now();
    let out = from_expr(&level[0]);
    assert!(matches!(out, Err(SymError::TooLarge(_))), "{out:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn power_text_beyond_the_limits_is_refused() {
    let big = parse("10^100000", &DefaultIdContext).expect("parses");
    assert!(matches!(from_expr(&big), Err(SymError::TooLarge(_))));
    let huge_exponent = parse("2^(10^9)", &DefaultIdContext).expect("parses");
    assert!(matches!(
        from_expr(&huge_exponent),
        Err(SymError::TooLarge(_))
    ));
}

#[test]
fn hundred_thousand_digit_literal_is_refused() {
    let digits = format!("1{}", "0".repeat(100_000));
    let started = Instant::now();
    if let Ok(e) = parse(&digits, &DefaultIdContext) {
        assert!(from_expr(&e).is_err());
    }
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// Random fully parenthesised text over `x`, `y`, `a` with small integers and exponents.
fn random_text(rng: &mut Lcg, depth: u32) -> String {
    if depth == 0 || rng.range(0, 9) < 3 {
        return match rng.range(0, 4) {
            0 => format!("{}", rng.range(-9, 9)),
            1 => "x".to_string(),
            2 => "y".to_string(),
            3 => "a".to_string(),
            _ => format!("({}/{})", rng.range(-9, 9), rng.range(1, 9)),
        };
    }
    let l = random_text(rng, depth - 1);
    let r = random_text(rng, depth - 1);
    match rng.range(0, 7) {
        0 => format!("({l}+{r})"),
        1 => format!("({l}-{r})"),
        2 => format!("({l}*{r})"),
        3 => format!("({l}/{r})"),
        4 => format!("(({l})^{})", rng.range(-3, 3)),
        5 => format!("sqrt({l})"),
        6 => format!("(-({l}))"),
        _ => format!("(({l})^(1/{}))", rng.range(2, 3)),
    }
}

/// Times one call; every call must return within one second.
fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = f();
    let took = started.elapsed();
    assert!(took < Duration::from_secs(1), "{what} took {took:?}");
    out
}

#[test]
fn random_small_trees_through_every_public_function() {
    let mut rng = Lcg(0x5EED_2026);
    let mut converted = 0usize;
    for _ in 0..3000 {
        let text = random_text(&mut rng, 2);
        let Ok(e) = parse(&text, &DefaultIdContext) else {
            continue;
        };
        let env = [("x", int(2)), ("y", int(3)), ("a", int(5))];
        let Ok(v) = timed("from_expr", || from_expr(&e)) else {
            continue;
        };
        converted += 1;
        let _ = timed("expand", || expand(&v));
        let _ = timed("simplify", || simplify(&v));
        let _ = timed("substitute", || substitute(&v, &[("x", Sym::int(2))]));
        let _ = timed("eval_rational", || eval_rational(&v, &env));
        let _ = timed("degree", || degree(&v, "x"));
        let _ = timed("to_expr", || to_expr(&v).to_string());
        let eq_text = format!("{text}==0");
        if let Ok(eq_expr) = parse(&eq_text, &DefaultIdContext) {
            if let Ok(eq) = timed("equation_from_expr", || equation_from_expr(&eq_expr)) {
                let _ = timed("solve_polynomial", || solve_polynomial(&eq, "x"));
                let _ = timed("solve_linear_system", || {
                    solve_linear_system(std::slice::from_ref(&eq), &["x".into(), "y".into()])
                });
            }
        }
    }
    assert!(converted > 1000, "converted {converted}");
}

#[test]
fn parser_accepts_the_fuzz_grammar() {
    // Guards the generator: the grammar must be parseable text, not an accidental skip.
    let mut rng = Lcg(7);
    let mut parsed = 0;
    for _ in 0..200 {
        let text = random_text(&mut rng, 3);
        if parse(&text, &DefaultIdContext).is_ok() {
            parsed += 1;
        }
    }
    assert!(parsed > 150, "parsed {parsed}");
    let _ = ex("1");
}

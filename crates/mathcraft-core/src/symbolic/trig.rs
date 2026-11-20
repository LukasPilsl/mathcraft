//! Trigonometric and exponential rewriting for `simplify` (help: `sin(x)^2 + cos(x)^2 simplify
//! -> 1`, `2*sin(x)*cos(x) simplify -> sin(2*x)`, `e^(2*x) * e^(5*y) simplify -> e^(2*x + 5*y)`).
//!
//! Rules (each holds for every complex value of its arguments, so none needs an assumption):
//! - `sin(u)^2 + cos(u)^2 = 1`: a pair with equal coefficients is replaced by that coefficient.
//! - Parity: `sin(-u) = -sin(u)`, `tan`, `asin`, `atan` likewise; `cos(-u) = cos(u)`. "-u" is a
//!   term with a negative leading coefficient (`negated_term`).
//! - Double angle: `2 * sin(u) * cos(u) = sin(2 u)` for a non-sum argument `u`, the only one
//!   spelled with the factor 2 exactly. The other direction is not applied.
//! - `e^a * e^b = e^(a + b)`, and `(e^a)^n = e^(n a)` for an integer `n`. Printed as `e^(...)`.
//! - `exp(0) = 1`.
//!
//! The driver [`simplify_identities`] alternates these rules, the assumption rewrites of
//! [`super::assume`] and the rational simplifier, for at most [`MAX_ROUNDS`] rounds. A round
//! whose result is larger than twice the input plus 8 nodes is discarded, so the term size
//! never grows past that cap. Every rule returns an equal value; a limit only leaves the result
//! less simplified, never wrong.

use super::assume::{self, Assumptions};
use super::canon::split_coef;
use super::num::{int, zero, Q};
use super::sym::Sym;
use super::{budget, ratfunc, SymError};

/// Largest number of rewrite rounds. Each rule either shrinks a term or keeps its size, so
/// rounds beyond the first few change nothing in practice.
pub const MAX_ROUNDS: usize = 8;

/// Simplifies `s` with the assumptions and the identities of this module, then the rational
/// simplifier, until a round changes nothing or [`MAX_ROUNDS`] is reached.
pub fn simplify_identities(s: &Sym, asm: &Assumptions) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let limit = 2 * size(s) + 8;
    let mut current = s.clone();
    for _ in 0..MAX_ROUNDS {
        let assumed = assume::apply(&current, asm)?;
        let rewritten = rewrite(&assumed)?;
        let next = ratfunc::simplify(&rewritten)?;
        if next == current || size(&next) > limit {
            break;
        }
        current = next;
    }
    Ok(current)
}

/// Number of nodes of a canonical value (the size measure of the term cap).
pub fn size(s: &Sym) -> usize {
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => 1,
        Sym::Func(_, args) | Sym::Add(args) | Sym::Mul(args) => {
            1 + args.iter().map(size).sum::<usize>()
        }
        Sym::Pow(b, _) => 1 + size(b),
    }
}

/// One bottom-up pass of the identities of this module.
fn rewrite(s: &Sym) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            let args = args.iter().map(rewrite).collect::<Result<Vec<_>, _>>()?;
            func_rule(name, args)
        }
        Sym::Add(ts) => {
            let ts = ts.iter().map(rewrite).collect::<Result<Vec<_>, _>>()?;
            pythagorean(ts)
        }
        Sym::Mul(fs) => {
            let fs = fs.iter().map(rewrite).collect::<Result<Vec<_>, _>>()?;
            let product = Sym::product(fs)?;
            match product {
                Sym::Mul(factors) => product_rules(factors),
                other => Ok(other),
            }
        }
        Sym::Pow(b, e) => {
            let base = rewrite(b)?;
            match &base {
                Sym::Func(name, args) if name == "exp" && e.is_integer() && args.len() == 1 => {
                    let scaled = Sym::product(vec![Sym::Num(e.clone()), args[0].clone()])?;
                    Sym::func("exp", vec![scaled])
                }
                _ => Sym::pow(base, e.clone()),
            }
        }
    }
}

/// Parity of the odd and even functions, and `exp(0) = 1`.
fn func_rule(name: &str, args: Vec<Sym>) -> Result<Sym, SymError> {
    if args.len() == 1 {
        let arg = &args[0];
        if name == "exp" && arg.is_zero() {
            return Ok(Sym::Num(super::num::one()));
        }
        let odd = matches!(name, "sin" | "tan" | "asin" | "atan");
        let even = name == "cos";
        if odd || even {
            if let Some(positive) = arg.negated_term() {
                let inner = Sym::func(name, vec![positive])?;
                return if odd {
                    Sym::product(vec![Sym::int(-1), inner])
                } else {
                    Ok(inner)
                };
            }
        }
    }
    Sym::func(name, args)
}

/// The argument `u` of `name(u)^2`, when `s` is that square.
fn square_arg<'a>(name: &str, s: &'a Sym) -> Option<&'a Sym> {
    match s {
        Sym::Pow(b, e) if *e == int(2) => match b.as_ref() {
            Sym::Func(n, args) if n == name && args.len() == 1 => Some(&args[0]),
            _ => None,
        },
        _ => None,
    }
}

/// `c sin(u)^2 + c cos(u)^2` becomes `c`, for every pair with the same argument and coefficient.
fn pythagorean(ts: Vec<Sym>) -> Result<Sym, SymError> {
    let mut parts: Vec<Option<(Q, Sym)>> = ts.into_iter().map(|t| Some(split_coef(t))).collect();
    let mut constant = zero();
    let mut changed = false;
    for i in 0..parts.len() {
        budget::spend(1)?;
        let (ci, arg) = match &parts[i] {
            Some((c, m)) => match square_arg("sin", m) {
                Some(u) => (c.clone(), u.clone()),
                None => continue,
            },
            None => continue,
        };
        let partner = (0..parts.len()).find(|&j| {
            j != i
                && matches!(&parts[j], Some((cj, mj))
                    if *cj == ci && square_arg("cos", mj) == Some(&arg))
        });
        if let Some(j) = partner {
            parts[i] = None;
            parts[j] = None;
            constant += ci;
            changed = true;
        }
    }
    if !changed {
        return Sym::sum(
            parts
                .into_iter()
                .flatten()
                .map(|(c, m)| Sym::product(vec![Sym::Num(c), m]))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    let mut terms: Vec<Sym> = parts
        .into_iter()
        .flatten()
        .map(|(c, m)| Sym::product(vec![Sym::Num(c), m]))
        .collect::<Result<Vec<_>, _>>()?;
    terms.push(Sym::Num(constant));
    Sym::sum(terms)
}

/// Rules on a product: `e^a e^b = e^(a+b)`, and `2 sin(u) cos(u) = sin(2u)`.
fn product_rules(factors: Vec<Sym>) -> Result<Sym, SymError> {
    let mut exps = Vec::new();
    let mut rest = Vec::new();
    for f in factors {
        match f {
            Sym::Func(ref name, ref args) if name == "exp" && args.len() == 1 => {
                exps.push(args[0].clone())
            }
            other => rest.push(other),
        }
    }
    if exps.len() >= 2 {
        let total = Sym::sum(exps)?;
        rest.push(Sym::func("exp", vec![total])?);
        return Sym::product(rest);
    }
    // Put the exp factor back when it is alone.
    rest.extend(
        exps.into_iter()
            .map(|a| Sym::func("exp", vec![a]))
            .collect::<Result<Vec<_>, _>>()?,
    );
    if let Some(folded) = double_angle(&rest)? {
        return Ok(folded);
    }
    Sym::product(rest)
}

/// `2 * sin(u) * cos(u)` with a non-sum argument `u`, as `sin(2 u)` (other factors kept).
fn double_angle(factors: &[Sym]) -> Result<Option<Sym>, SymError> {
    let two = int(2);
    let Some(two_at) = factors.iter().position(|f| *f == Sym::Num(two.clone())) else {
        return Ok(None);
    };
    for (i, f) in factors.iter().enumerate() {
        let Sym::Func(name, args) = f else { continue };
        if name != "sin" || args.len() != 1 || matches!(args[0], Sym::Add(_)) {
            continue;
        }
        let u = &args[0];
        let cos_at = factors.iter().enumerate().position(|(j, g)| {
            j != i && matches!(g, Sym::Func(n, a) if n == "cos" && a.len() == 1 && a[0] == *u)
        });
        let Some(j) = cos_at else { continue };
        if j == two_at || i == two_at {
            continue;
        }
        let doubled = Sym::product(vec![Sym::Num(two.clone()), u.clone()])?;
        let angle = Sym::func("sin", vec![doubled])?;
        let mut out: Vec<Sym> = factors
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != two_at && *k != i && *k != j)
            .map(|(_, g)| g.clone())
            .collect();
        out.push(angle);
        return Ok(Some(Sym::product(out)?));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::Sym;

    fn sin(u: Sym) -> Sym {
        Sym::func("sin", vec![u]).expect("sin")
    }
    fn cos(u: Sym) -> Sym {
        Sym::func("cos", vec![u]).expect("cos")
    }
    fn sq(s: Sym) -> Sym {
        Sym::pow(s, int(2)).expect("sq")
    }
    fn x() -> Sym {
        Sym::symbol("x")
    }

    #[test]
    fn pythagorean_identity_is_one() {
        let s = Sym::sum(vec![sq(sin(x())), sq(cos(x()))]).expect("sum");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        assert_eq!(got, Sym::Num(super::super::num::one()));
    }

    #[test]
    fn pythagorean_with_coefficients_and_a_remainder() {
        // 3 sin^2 + 3 cos^2 + y -> 3 + y
        let s = Sym::sum(vec![
            Sym::product(vec![Sym::int(3), sq(sin(x()))]).expect("3s"),
            Sym::product(vec![Sym::int(3), sq(cos(x()))]).expect("3c"),
            Sym::symbol("y"),
        ])
        .expect("sum");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        let want = Sym::sum(vec![Sym::int(3), Sym::symbol("y")]).expect("want");
        assert_eq!(got, want);
    }

    #[test]
    fn unequal_coefficients_are_not_paired() {
        let s = Sym::sum(vec![
            sq(sin(x())),
            Sym::product(vec![Sym::int(2), sq(cos(x()))]).expect("2c"),
        ])
        .expect("sum");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        assert_eq!(got, s);
    }

    #[test]
    fn pythagorean_with_different_arguments_is_not_paired() {
        let s = Sym::sum(vec![sq(sin(x())), sq(cos(Sym::symbol("y")))]).expect("sum");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        assert_eq!(got, s);
    }

    #[test]
    fn double_angle_folds_to_sin_of_twice_the_argument() {
        let s = Sym::product(vec![Sym::int(2), sin(x()), cos(x())]).expect("2sc");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        let want = sin(Sym::product(vec![Sym::int(2), x()]).expect("2x"));
        assert_eq!(got, want);
    }

    #[test]
    fn double_angle_needs_the_factor_two() {
        let s = Sym::product(vec![Sym::int(4), sin(x()), cos(x())]).expect("4sc");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        assert_eq!(got, s);
    }

    #[test]
    fn parity_of_sine_and_cosine() {
        let neg = Sym::product(vec![Sym::int(-1), x()]).expect("-x");
        assert_eq!(
            simplify_identities(&sin(neg.clone()), &Assumptions::default()).expect("s"),
            Sym::product(vec![Sym::int(-1), sin(x())]).expect("-sin x")
        );
        assert_eq!(
            simplify_identities(&cos(neg), &Assumptions::default()).expect("c"),
            cos(x())
        );
    }

    #[test]
    fn exponentials_combine_in_a_product() {
        let e = |a: Sym| Sym::func("exp", vec![a]).expect("exp");
        let two_x = Sym::product(vec![Sym::int(2), x()]).expect("2x");
        let five_y = Sym::product(vec![Sym::int(5), Sym::symbol("y")]).expect("5y");
        let s = Sym::product(vec![e(two_x.clone()), e(five_y.clone())]).expect("product");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        assert_eq!(got, e(Sym::sum(vec![two_x, five_y]).expect("sum")));
    }

    #[test]
    fn integer_power_of_an_exponential() {
        let s = Sym::pow(Sym::func("exp", vec![x()]).expect("exp"), int(3)).expect("pow");
        let got = simplify_identities(&s, &Assumptions::default()).expect("simplify");
        let want = Sym::func(
            "exp",
            vec![Sym::product(vec![Sym::int(3), x()]).expect("3x")],
        )
        .expect("exp");
        assert_eq!(got, want);
    }

    #[test]
    fn exp_of_zero_is_one() {
        let s = Sym::func("exp", vec![Sym::int(0)]).expect("exp0");
        assert_eq!(
            simplify_identities(&s, &Assumptions::default()).expect("simplify"),
            Sym::Num(super::super::num::one())
        );
    }

    #[test]
    fn identities_are_numerically_equal_at_random_points() {
        // Property: each rewritten form equals the original at sample points (1e-9 relative).
        let cases: Vec<Sym> = vec![
            Sym::sum(vec![sq(sin(x())), sq(cos(x())), Sym::symbol("y")]).expect("s1"),
            Sym::product(vec![Sym::int(2), sin(x()), cos(x())]).expect("s2"),
            Sym::product(vec![
                Sym::func("exp", vec![x()]).expect("e"),
                Sym::func("exp", vec![Sym::symbol("y")]).expect("e"),
            ])
            .expect("s3"),
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        for case in &cases {
            let got = simplify_identities(case, &Assumptions::default()).expect("simplify");
            for _ in 0..25 {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let xv = ((seed >> 11) as f64 / (1u64 << 53) as f64) * 6.0 - 3.0;
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let yv = ((seed >> 11) as f64 / (1u64 << 53) as f64) * 6.0 - 3.0;
                let a = eval(case, xv, yv);
                let b = eval(&got, xv, yv);
                assert!(
                    (a - b).abs() <= 1e-9 * (1.0 + a.abs()),
                    "{case} vs {got} at {xv},{yv}: {a} {b}"
                );
            }
        }
    }

    fn eval(s: &Sym, x: f64, y: f64) -> f64 {
        match s {
            Sym::Num(q) => super::super::num::to_f64(q),
            Sym::Symbol(n) => {
                if n == "x" {
                    x
                } else {
                    y
                }
            }
            Sym::Unit(_) => 1.0,
            Sym::Func(n, args) => {
                let v = eval(&args[0], x, y);
                match n.as_str() {
                    "sin" => v.sin(),
                    "cos" => v.cos(),
                    "exp" => v.exp(),
                    "abs" => v.abs(),
                    other => panic!("unexpected {other}"),
                }
            }
            Sym::Add(ts) => ts.iter().map(|t| eval(t, x, y)).sum(),
            Sym::Mul(fs) => fs.iter().map(|f| eval(f, x, y)).product(),
            Sym::Pow(b, e) => eval(b, x, y).powf(super::super::num::to_f64(e)),
        }
    }
}

//! `expand` of sines, cosines and tangents of sums and integer multiples, and the `rewrite`
//! keyword (help: `to_expand_expressions`, `rewriting_expressions`,
//! `example_simplify_rewrite_expressions`).
//!
//! Expansion: `sin(u+v) = sin u cos v + cos u sin v`, `cos(u+v) = cos u cos v - sin u sin v`,
//! `tan(u+v)` as `sin(u+v)/cos(u+v)` expanded, and `sin(k u)`, `cos(k u)`, `tan(k u)` for an
//! integer `k` with `2 <= |k| <= MAX_MULTIPLE` by repeated addition. Any other argument stays.
//!
//! Rewrite (modifier): `exp` (`e`): `sin`, `cos`, `tan`, `sinh`, `cosh`, `tanh` in complex
//! exponentials; `cos` / `sin`: even powers of `sin` / `cos` through `sin^2 = 1 - cos^2`; `ln`:
//! `asinh`, `acosh`, `atanh`; `log`: `ln u = log(u)/log(e)`; `gamma`: `combin`. Everything else is
//! refused.
//!
//! Every result of expansion and rewriting is compared with the input at sample points
//! ([`super::numcheck`]) before it is returned, except the purely definitional rewrites (`log`,
//! `gamma`), where no numeric evaluator exists for the target function.

use num_traits::{Signed, ToPrimitive};

use super::calc::symbols;
use super::num::{half, one, Q};
use super::numcheck::agree;
use super::poly::expand;
use super::ratfunc::simplify;
use super::sym::Sym;
use super::trig::{simplify_identities, size};
use super::{budget, Assumptions, SymError};

/// Largest integer multiple `k` of `sin(k u)` that is expanded.
pub const MAX_MULTIPLE: i64 = 12;
/// Largest number of summands of an argument that is split.
const MAX_SPLIT: usize = 6;
/// Largest node count of an expression accepted for expansion or rewriting.
const MAX_SIZE: usize = 400;

fn unsupported(text: &str) -> SymError {
    SymError::Unsupported(text.into())
}

fn func1(name: &str, u: Sym) -> Result<Sym, SymError> {
    Sym::func(name, vec![u])
}

/// `expand` of `s` with sines, cosines and tangents of sums and integer multiples expanded.
pub fn expand_trig(s: &Sym) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let plain = expand(s)?;
    if size(&plain) > MAX_SIZE {
        return Err(SymError::TooLarge("expression size"));
    }
    let walked = walk(&plain)?;
    if walked == plain {
        return Ok(plain);
    }
    let result = expand(&walked)?;
    if !agree(&plain, &result) {
        return Err(unsupported(
            "trigonometric expansion failed its numeric check",
        ));
    }
    Ok(result)
}

fn walk(s: &Sym) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            let args = args.iter().map(walk).collect::<Result<Vec<_>, _>>()?;
            if args.len() == 1 && matches!(name.as_str(), "sin" | "cos" | "tan") {
                expand_one(name, &args[0])
            } else {
                Sym::func(name, args)
            }
        }
        Sym::Add(ts) => Sym::sum(ts.iter().map(walk).collect::<Result<Vec<_>, _>>()?),
        Sym::Mul(ts) => Sym::product(ts.iter().map(walk).collect::<Result<Vec<_>, _>>()?),
        Sym::Pow(b, e) => Sym::pow(walk(b)?, e.clone()),
    }
}

/// `(k, u)` when `a` is `k * u` with an integer `k` of magnitude 2 or more and `u` not a number.
fn integer_multiple(a: &Sym) -> Option<(i64, Sym)> {
    if let Sym::Mul(fs) = a {
        if let Some(Sym::Num(q)) = fs.first() {
            if q.is_integer() {
                let k = q.to_integer().to_i64()?;
                if k.abs() >= 2 && k.abs() <= MAX_MULTIPLE {
                    let rest = Sym::product(fs[1..].to_vec()).ok()?;
                    if rest.as_num().is_none() {
                        return Some((k, rest));
                    }
                }
            }
        }
    }
    None
}

/// `(sin, cos)` of `a`, expanded as far as the rules go.
fn sin_cos(a: &Sym) -> Result<(Sym, Sym), SymError> {
    budget::spend(1)?;
    if let Sym::Add(ts) = a {
        if ts.len() > MAX_SPLIT {
            return Ok((func1("sin", a.clone())?, func1("cos", a.clone())?));
        }
        let first = ts[0].clone();
        let rest = Sym::sum(ts[1..].to_vec())?;
        let (s1, c1) = sin_cos(&first)?;
        let (s2, c2) = sin_cos(&rest)?;
        return add_angles((&s1, &c1), (&s2, &c2));
    }
    if let Some((k, u)) = integer_multiple(a) {
        let (s1, c1) = sin_cos(&u)?;
        let (mut sk, mut ck) = (s1.clone(), c1.clone());
        for _ in 1..k.abs() {
            let next = add_angles((&sk, &ck), (&s1, &c1))?;
            sk = next.0;
            ck = next.1;
        }
        if k < 0 {
            sk = sk.negate()?;
        }
        return Ok((sk, ck));
    }
    if let Some(positive) = a.negated_term() {
        let (s, c) = sin_cos(&positive)?;
        return Ok((s.negate()?, c));
    }
    Ok((func1("sin", a.clone())?, func1("cos", a.clone())?))
}

/// `sin(u+v)`, `cos(u+v)` from those of `u` and `v`.
fn add_angles(u: (&Sym, &Sym), v: (&Sym, &Sym)) -> Result<(Sym, Sym), SymError> {
    let s = Sym::sum(vec![
        u.0.clone().times(v.1.clone())?,
        u.1.clone().times(v.0.clone())?,
    ])?;
    let c =
        u.1.clone()
            .times(v.1.clone())?
            .minus(u.0.clone().times(v.0.clone())?)?;
    Ok((expand(&s)?, expand(&c)?))
}

fn expand_one(name: &str, arg: &Sym) -> Result<Sym, SymError> {
    let splits = matches!(arg, Sym::Add(_)) || integer_multiple(arg).is_some();
    let negative = arg.negated_term().is_some();
    if !splits && !negative {
        return func1(name, arg.clone());
    }
    let (s, c) = sin_cos(arg)?;
    match name {
        "sin" => Ok(s),
        "cos" => Ok(c),
        _ => s.over(c),
    }
}

// ---------------------------------------------------------------------------------------------
// rewrite
// ---------------------------------------------------------------------------------------------

/// The modifiers of `rewrite` that this module answers.
pub fn rewrite_modifier_known(m: &str) -> bool {
    matches!(
        m,
        "exp" | "e" | "cos" | "sin" | "ln" | "log" | "gamma" | "Gamma" | "Γ"
    )
}

/// Rewrites `s` in terms of the function named by `modifier` (see the module docs).
pub fn rewrite_as(s: &Sym, modifier: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    if size(s) > MAX_SIZE {
        return Err(SymError::TooLarge("expression size"));
    }
    let names = symbols(s);
    let result = match modifier {
        "exp" | "e" => {
            if names.iter().any(|n| n == "i" || n == "j") {
                return Err(unsupported("rewrite with exp when the expression has i"));
            }
            let raw = map_funcs(s, &to_exp)?;
            let reduced = reduce_i(&expand(&raw)?)?;
            simplify_identities(&reduced, &Assumptions::default())?
        }
        "cos" => square_swap(s, "sin", "cos")?,
        "sin" => square_swap(s, "cos", "sin")?,
        "ln" => simplify(&map_funcs(s, &to_ln)?)?,
        "log" => return map_funcs(s, &to_log),
        "gamma" | "Gamma" | "Γ" => return map_funcs(s, &to_gamma),
        other => return Err(unsupported(&format!("rewrite, {other}"))),
    };
    if !agree(s, &result) {
        return Err(unsupported("rewrite failed its numeric check"));
    }
    Ok(result)
}

type FuncMap<'a> = &'a dyn Fn(&str, Vec<Sym>) -> Result<Sym, SymError>;

/// Applies `f` to every function node, bottom up.
fn map_funcs(s: &Sym, f: FuncMap<'_>) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            let args = args
                .iter()
                .map(|a| map_funcs(a, f))
                .collect::<Result<Vec<_>, _>>()?;
            f(name, args)
        }
        Sym::Add(ts) => Sym::sum(
            ts.iter()
                .map(|t| map_funcs(t, f))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Mul(ts) => Sym::product(
            ts.iter()
                .map(|t| map_funcs(t, f))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Pow(b, e) => Sym::pow(map_funcs(b, f)?, e.clone()),
    }
}

fn imag() -> Sym {
    Sym::symbol("i")
}

/// `exp(i u)` and `exp(-i u)`.
fn cis(u: &Sym) -> Result<(Sym, Sym), SymError> {
    let iu = imag().times(u.clone())?;
    Ok((func1("exp", iu.clone())?, func1("exp", iu.negate()?)?))
}

fn to_exp(name: &str, args: Vec<Sym>) -> Result<Sym, SymError> {
    if args.len() != 1 {
        return Sym::func(name, args);
    }
    let u = &args[0];
    match name {
        "sin" => {
            let (p, m) = cis(u)?;
            // sin u = (e^(iu) - e^(-iu)) / (2i) = -i/2 (e^(iu) - e^(-iu))
            Sym::product(vec![Sym::rational(-1, 2)?, imag(), p.minus(m)?])
        }
        "cos" => {
            let (p, m) = cis(u)?;
            Sym::product(vec![Sym::rational(1, 2)?, p.plus(m)?])
        }
        "tan" => {
            let (p, m) = cis(u)?;
            // tan u = -i (e^(iu) - e^(-iu)) / (e^(iu) + e^(-iu))
            let num = Sym::product(vec![Sym::int(-1), imag(), p.clone().minus(m.clone())?])?;
            num.over(p.plus(m)?)
        }
        "sinh" | "cosh" | "tanh" => {
            let p = func1("exp", u.clone())?;
            let m = func1("exp", u.clone().negate()?)?;
            let sinh = Sym::product(vec![Sym::rational(1, 2)?, p.clone().minus(m.clone())?])?;
            let cosh = Sym::product(vec![Sym::rational(1, 2)?, p.plus(m)?])?;
            match name {
                "sinh" => Ok(sinh),
                "cosh" => Ok(cosh),
                _ => sinh.over(cosh),
            }
        }
        other if is_circular(other) => Err(unsupported("rewrite of this function in exp")),
        _ => Sym::func(name, args),
    }
}

/// A trigonometric or hyperbolic function (direct or inverse) that a rewrite must not leave
/// half-done.
fn is_circular(name: &str) -> bool {
    matches!(
        name,
        "sin"
            | "cos"
            | "tan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asin"
            | "acos"
            | "atan"
            | "asinh"
            | "acosh"
            | "atanh"
            | "cot"
            | "coth"
    )
}

/// Replaces `i^k` in a product by its value (`i^2 = -1`) in an expanded expression.
fn reduce_i(s: &Sym) -> Result<Sym, SymError> {
    budget::spend(1)?;
    let is_i = |b: &Sym| matches!(b, Sym::Symbol(n) if n == "i");
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => Sym::func(
            name,
            args.iter().map(reduce_i).collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Add(ts) => Sym::sum(ts.iter().map(reduce_i).collect::<Result<Vec<_>, _>>()?),
        Sym::Pow(b, e) => {
            if is_i(b) && e.is_integer() {
                let k = e
                    .to_integer()
                    .to_i64()
                    .ok_or_else(|| unsupported("power of i"))?;
                return i_power(k);
            }
            Sym::pow(reduce_i(b)?, e.clone())
        }
        Sym::Mul(fs) => {
            let mut exponent: i64 = 0;
            let mut rest = Vec::with_capacity(fs.len());
            for f in fs {
                match f {
                    Sym::Symbol(n) if n == "i" => exponent += 1,
                    Sym::Pow(b, e) if is_i(b) => {
                        if !e.is_integer() {
                            return Err(unsupported("non-integer power of i"));
                        }
                        exponent += e
                            .to_integer()
                            .to_i64()
                            .ok_or_else(|| unsupported("power of i"))?;
                    }
                    other => rest.push(reduce_i(other)?),
                }
            }
            if exponent != 0 {
                rest.push(i_power(exponent)?);
            }
            Sym::product(rest)
        }
    }
}

/// `i^k` for any integer `k`.
fn i_power(k: i64) -> Result<Sym, SymError> {
    Ok(match k.rem_euclid(4) {
        0 => Sym::int(1),
        1 => imag(),
        2 => Sym::int(-1),
        _ => imag().negate()?,
    })
}

/// Every even power `from(u)^(2k)` replaced by `(1 - to(u)^2)^k`; any other `from` is refused.
fn square_swap(s: &Sym, from: &str, to: &str) -> Result<Sym, SymError> {
    let swapped = swap(s, from, to)?;
    let expanded = expand(&swapped)?;
    simplify_identities(&expanded, &Assumptions::default())
}

fn swap(s: &Sym, from: &str, to: &str) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            if name == from {
                return Err(unsupported("rewrite leaves an odd power"));
            }
            Sym::func(
                name,
                args.iter()
                    .map(|a| swap(a, from, to))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        Sym::Add(ts) => Sym::sum(
            ts.iter()
                .map(|t| swap(t, from, to))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Mul(ts) => Sym::product(
            ts.iter()
                .map(|t| swap(t, from, to))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Pow(b, e) => {
            if let Sym::Func(name, args) = &**b {
                if name == from && args.len() == 1 {
                    if !e.is_integer() || e.is_negative() {
                        return Err(unsupported("rewrite of a negative or odd power"));
                    }
                    let n = e
                        .to_integer()
                        .to_i64()
                        .ok_or_else(|| unsupported("power"))?;
                    if n % 2 != 0 || n > 20 {
                        return Err(unsupported("rewrite of an odd or large power"));
                    }
                    let inner = swap(&args[0], from, to)?;
                    let c = func1(to, inner)?;
                    let one_minus = Sym::int(1).minus(Sym::pow(c, Q::from_integer(2.into()))?)?;
                    return Sym::pow(one_minus, Q::from_integer((n / 2).into()));
                }
            }
            Sym::pow(swap(b, from, to)?, e.clone())
        }
    }
}

fn to_ln(name: &str, args: Vec<Sym>) -> Result<Sym, SymError> {
    if args.len() != 1 {
        return Sym::func(name, args);
    }
    let u = &args[0];
    let sqrt = |x: Sym| Sym::pow(x, half());
    match name {
        "asinh" => {
            let inside = u.clone().times(u.clone())?.plus(Sym::int(1))?;
            func1("ln", sqrt(inside)?.plus(u.clone())?)
        }
        "acosh" => {
            let a = sqrt(u.clone().plus(Sym::int(1))?)?;
            let b = sqrt(u.clone().minus(Sym::int(1))?)?;
            func1("ln", u.clone().plus(a.times(b)?)?)
        }
        "atanh" => {
            let p = func1("ln", Sym::int(1).plus(u.clone())?)?;
            let m = func1("ln", Sym::int(1).minus(u.clone())?)?;
            p.minus(m)?.times(Sym::rational(1, 2)?)
        }
        other if is_circular(other) => Err(unsupported("rewrite of this function in ln")),
        _ => Sym::func(name, args),
    }
}

fn to_log(name: &str, args: Vec<Sym>) -> Result<Sym, SymError> {
    if name == "ln" && args.len() == 1 {
        let log_e = func1("log", func1("exp", Sym::int(1))?)?;
        return func1("log", args[0].clone())?.over(log_e);
    }
    Sym::func(name, args)
}

fn to_gamma(name: &str, args: Vec<Sym>) -> Result<Sym, SymError> {
    if name == "combin" && args.len() == 2 {
        let (p, q) = (&args[0], &args[1]);
        let g = |u: Sym| func1("Gamma", u);
        let top = g(p.clone().plus(Sym::Num(one()))?)?;
        let a = g(p.clone().minus(q.clone())?.plus(Sym::Num(one()))?)?;
        let b = g(q.clone().plus(Sym::Num(one()))?)?;
        return top.over(a.times(b)?);
    }
    Sym::func(name, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::from_expr;

    fn s(src: &str) -> Sym {
        from_expr(&crate::parser::parse(src, &crate::parser::DefaultIdContext).expect("parses"))
            .expect("converts")
    }

    #[test]
    fn sums_and_multiples_expand() {
        assert_eq!(
            expand_trig(&s("sin(a+b)")).expect("sin"),
            s("sin(a)*cos(b)+cos(a)*sin(b)")
        );
        assert_eq!(
            expand_trig(&s("cos(a+b)")).expect("cos"),
            s("cos(a)*cos(b)-sin(a)*sin(b)")
        );
        // help: sin(2 x) expand -> 2 cos(x) sin(x).
        assert_eq!(
            expand_trig(&s("sin(2*x)")).expect("2x"),
            s("2*cos(x)*sin(x)")
        );
        assert_eq!(
            expand_trig(&s("cos(2*x)")).expect("cos2x"),
            s("cos(x)^2-sin(x)^2")
        );
        // A plain argument is unchanged.
        assert_eq!(expand_trig(&s("sin(x)+x")).expect("plain"), s("sin(x)+x"));
    }

    #[test]
    fn tan_of_a_sum_is_a_quotient_of_expanded_parts() {
        let got = expand_trig(&s("tan(a+b)")).expect("tan");
        assert!(agree(&got, &s("tan(a+b)")));
    }

    #[test]
    fn rewrite_exp_cos_ln() {
        // help: sin(x) rewrite, exp is -(i (e^(ix) - e^(-ix)))/2.
        let got = rewrite_as(&s("sin(x)"), "exp").expect("exp");
        assert!(agree(&got, &s("sin(x)")));
        assert!(!format!("{got:?}").contains("sin"));
        // help: sin(x)^2 rewrite, cos is 1 - cos(x)^2.
        assert_eq!(
            rewrite_as(&s("sin(x)^2"), "cos").expect("cos"),
            s("1-cos(x)^2")
        );
        assert!(rewrite_as(&s("sin(x)"), "cos").is_err());
        // help: asinh(x) rewrite, ln is ln(sqrt(x^2+1)+x).
        assert_eq!(
            rewrite_as(&s("asinh(x)"), "ln").expect("ln"),
            s("ln(sqrt(x^2+1)+x)")
        );
        assert!(rewrite_as(&s("sin(x)"), "bogus").is_err());
    }
}

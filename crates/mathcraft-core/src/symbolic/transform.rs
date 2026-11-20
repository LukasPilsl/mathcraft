//! Laplace and inverse Laplace transforms (Prime's `laplace` and `invlaplace` keywords) from the
//! standard table. Every answer is exact; anything outside the table is refused.
//!
//! Forward table, with `exp(a t)` shifting `s` to `S = s - a`:
//! - a factor without `t`, `c`: `c/s`
//! - `t^n` (integer `n >= 0`): `n!/S^(n+1)`
//! - `sin(w t)`: `w/(S^2+w^2)`; `cos(w t)`: `S/(S^2+w^2)`
//! - `exp(a t)` alone: `1/S`; `exp(a t + b)` also contributes the constant factor `exp(b)`.
//!
//! Linearity over sums; a product with a sum factor that mentions `t` is expanded first.
//! Refused: other kernels (`sinh`, Heaviside, Dirac delta, ...), two kernels in one product,
//! `sin`/`cos` with a phase, nonlinear exponents.
//!
//! Inverse: `parfrac` in `s`, then each term is matched against the table:
//! - `c/(s-r)^k` gives `c t^(k-1)/(k-1)! e^(r t)`;
//! - `(n1 s + n0)/(c2 ((s-a)^2 + w^2))` with `w^2 > 0` gives `e^(a t)` times `cos(w t)` and
//!   `sin(w t)` terms (a leftover irreducible quadratic, power 1 only).
//!
//! Refused: a polynomial part in `s` (Dirac delta and its derivatives), a constant term, a
//! quadratic with real roots, a symbolic coefficient (`parfrac` refuses it). The result is returned
//! only when its forward transform minus the input simplifies to zero (the round trip).

use num_traits::{Signed, Zero};

use super::budget;
use super::calc::differentiate;
use super::canon::to_i64;
use super::limit::numeric_coefficients;
use super::num::{self, half, int, one, zero, Q};
use super::partial::parfrac;
use super::poly::expand;
use super::ratfunc::simplify;
use super::sym::Sym;
use super::{SymError, MAX_DEGREE};

/// Laplace transform of `f` in `t`, with the new variable named `s`.
pub fn laplace(f: &Sym, t: &str, s: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let f = simplify(f)?;
    laplace_rec(&f, t, s, false)
}

/// Inverse Laplace transform of `f` in `s`, with the new variable named `t`. The result is
/// checked by the forward transform before it is returned.
pub fn inverse_laplace(f: &Sym, s: &str, t: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    if !f.mentions_any(&[s.to_string()]) {
        return Err(SymError::Unsupported(
            "inverse transform of a constant (Dirac delta)".into(),
        ));
    }
    // Each summand is decomposed on its own: parfrac of the combined fraction could leave a
    // quartic factor that is not split over Q, although every summand is simple.
    let summands = match f {
        Sym::Add(ts) => ts.clone(),
        other => vec![other.clone()],
    };
    let mut out = Vec::new();
    for summand in &summands {
        let pf = parfrac(summand, s)?;
        let parts = match &pf {
            Sym::Add(ts) => ts.clone(),
            other => vec![other.clone()],
        };
        for part in &parts {
            out.push(inverse_term(part, s, t)?);
        }
    }
    let result = Sym::sum(out)?;
    let back = laplace(&result, t, s)?;
    if !simplify(&back.minus(f.clone())?)?.is_zero() {
        return Err(SymError::Unsupported(
            "inverse transform failed the round-trip check".into(),
        ));
    }
    Ok(result)
}

fn unsupported(text: &str) -> SymError {
    SymError::Unsupported(text.into())
}

fn laplace_rec(f: &Sym, t: &str, s: &str, expanded: bool) -> Result<Sym, SymError> {
    budget::spend(1)?;
    let tn = [t.to_string()];
    if !f.mentions_any(&tn) {
        return Sym::product(vec![f.clone(), Sym::pow(Sym::symbol(s), int(-1))?]);
    }
    match f {
        Sym::Add(terms) => {
            let mut parts = Vec::with_capacity(terms.len());
            for term in terms {
                parts.push(laplace_rec(term, t, s, expanded)?);
            }
            Sym::sum(parts)
        }
        Sym::Mul(factors) => {
            let mut consts = Vec::new();
            let mut deps = Vec::new();
            for x in factors {
                if x.mentions_any(&tn) {
                    deps.push(x.clone());
                } else {
                    consts.push(x.clone());
                }
            }
            if deps.iter().any(|d| matches!(d, Sym::Add(_))) {
                if expanded {
                    return Err(unsupported("laplace of a product with a sum"));
                }
                let whole = expand(f)?;
                return laplace_rec(&whole, t, s, true);
            }
            let (extra, transform) = kernel(&deps, t, s)?;
            consts.extend(extra);
            consts.push(transform);
            Sym::product(consts)
        }
        Sym::Pow(base, _) if matches!(**base, Sym::Add(_)) => {
            if expanded {
                return Err(unsupported("laplace of a power of a sum"));
            }
            let whole = expand(f)?;
            laplace_rec(&whole, t, s, true)
        }
        _ => {
            let (extra, transform) = kernel(std::slice::from_ref(f), t, s)?;
            let mut all = extra;
            all.push(transform);
            Sym::product(all)
        }
    }
}

/// The table's kernel of one `t`-dependent factor.
enum Kernel {
    /// `t^n`.
    Power(u32),
    /// `sin(w t)` or `cos(w t)`.
    Trig { cos: bool, w: Sym },
}

fn set_kernel(slot: &mut Option<Kernel>, k: Kernel) -> Result<(), SymError> {
    if slot.is_some() {
        return Err(unsupported(
            "laplace of a product with two t-dependent kernels",
        ));
    }
    *slot = Some(k);
    Ok(())
}

/// Constant factors (from `exp(b)`) and the transform of a product of `t`-dependent factors:
/// at most one kernel, any number of exponentials.
fn kernel(deps: &[Sym], t: &str, s: &str) -> Result<(Vec<Sym>, Sym), SymError> {
    let mut extra = Vec::new();
    let mut shift = Sym::int(0);
    let mut kern: Option<Kernel> = None;
    for d in deps {
        match d {
            Sym::Func(name, args) if name == "exp" && args.len() == 1 => {
                let (a, b) = linear_parts(&args[0], t)?;
                shift = shift.plus(a)?;
                if !b.is_zero() {
                    extra.push(Sym::func("exp", vec![b])?);
                }
            }
            Sym::Func(name, args) if (name == "sin" || name == "cos") && args.len() == 1 => {
                let (w, b) = linear_parts(&args[0], t)?;
                if !b.is_zero() || w.is_zero() {
                    return Err(unsupported("sin or cos with a phase or a zero frequency"));
                }
                set_kernel(
                    &mut kern,
                    Kernel::Trig {
                        cos: name == "cos",
                        w,
                    },
                )?;
            }
            Sym::Symbol(n) if n.as_str() == t => set_kernel(&mut kern, Kernel::Power(1))?,
            Sym::Pow(base, e)
                if matches!(&**base, Sym::Symbol(n) if n.as_str() == t)
                    && e.is_integer()
                    && !e.is_negative() =>
            {
                let n = to_i64(e)?;
                if n > i64::from(MAX_DEGREE) {
                    return Err(SymError::TooLarge("laplace power"));
                }
                set_kernel(&mut kern, Kernel::Power(n as u32))?;
            }
            _ => return Err(unsupported("laplace of this function")),
        }
    }
    let sh = Sym::sum(vec![Sym::symbol(s), shift.negate()?])?;
    let transform = match kern {
        None => Sym::pow(sh, int(-1))?,
        Some(Kernel::Power(n)) => {
            let e = i64::from(n) + 1;
            Sym::product(vec![Sym::Num(factorial(n)?), Sym::pow(sh, int(-e))?])?
        }
        Some(Kernel::Trig { cos, w }) => {
            let denom = Sym::sum(vec![
                Sym::pow(sh.clone(), int(2))?,
                Sym::pow(w.clone(), int(2))?,
            ])?;
            let inv = Sym::pow(denom, int(-1))?;
            Sym::product(vec![if cos { sh } else { w }, inv])?
        }
    };
    Ok((extra, transform))
}

/// `arg = a t + b` with `a` and `b` free of `t`: returns `(a, b)`.
fn linear_parts(arg: &Sym, t: &str) -> Result<(Sym, Sym), SymError> {
    let tn = [t.to_string()];
    let a = differentiate(arg, t)?;
    if a.mentions_any(&tn) {
        return Err(unsupported("nonlinear argument in t"));
    }
    let b = arg.clone().minus(a.clone().times(Sym::symbol(t))?)?;
    if b.mentions_any(&tn) {
        return Err(unsupported("nonlinear argument in t"));
    }
    Ok((a, b))
}

fn factorial(n: u32) -> Result<Q, SymError> {
    let mut acc = one();
    for i in 2..=n {
        num::mul_into(&mut acc, &int(i64::from(i)))?;
    }
    Ok(acc)
}

/// The inverse transform of one term of a partial fraction decomposition in `s`.
fn inverse_term(term: &Sym, s: &str, t: &str) -> Result<Sym, SymError> {
    budget::spend(1)?;
    let factors = match term {
        Sym::Mul(fs) => fs.clone(),
        other => vec![other.clone()],
    };
    let sn = [s.to_string()];
    let mut coef = Vec::new();
    let mut numer = Vec::new();
    let mut denom: Option<(Sym, u32)> = None;
    for x in factors {
        if !x.mentions_any(&sn) {
            coef.push(x);
            continue;
        }
        match x {
            Sym::Pow(base, e) if denom.is_none() && e.is_integer() && e.is_negative() => {
                let k = to_i64(&-e.clone())?;
                if k > i64::from(MAX_DEGREE) {
                    return Err(SymError::TooLarge("inverse laplace power"));
                }
                denom = Some((*base, k as u32));
            }
            other => numer.push(other),
        }
    }
    let Some((base, k)) = denom else {
        return Err(unsupported(
            "a term without a denominator in s (polynomial part)",
        ));
    };
    let n = numeric_coefficients(&Sym::product(numer)?, s)?;
    let c = Sym::product(coef)?;
    let n0 = n.first().cloned().unwrap_or_else(zero);
    let n1 = n.get(1).cloned().unwrap_or_else(zero);
    let bc = numeric_coefficients(&base, s)?;
    match bc.as_slice() {
        [c0, c1] if !c1.is_zero() => {
            if n.len() > 1 && !n1.is_zero() {
                return Err(unsupported("improper term in s"));
            }
            if n0.is_zero() {
                return Ok(Sym::int(0));
            }
            let (c0, c1) = (c0.clone(), c1.clone());
            // base = c1 (s - r) with r = -c0/c1, so base^-k = c1^-k (s-r)^-k.
            let r = -(c0 / c1.clone());
            let scale = num::checked_pow(&c1, i64::from(k))?;
            let poly = Sym::product(vec![
                Sym::Num(one() / factorial(k - 1)?),
                Sym::pow(Sym::symbol(t), int(i64::from(k) - 1))?,
            ])?;
            let exp_part = exp_of(&r, t)?;
            Sym::product(vec![c, Sym::Num(n0 / scale), poly, exp_part])
        }
        [c0, c1, c2] if !c2.is_zero() && k == 1 => {
            if n.len() > 2 && !n[2].is_zero() {
                return Err(unsupported("improper term in s"));
            }
            let (c0, c1, c2) = (c0.clone(), c1.clone(), c2.clone());
            // base = c2 ((s-a)^2 + w2) with a = -c1/(2 c2) and w2 = c0/c2 - a^2 > 0.
            let a = -(c1 / (c2.clone() * int(2)));
            let w2 = c0 / c2.clone() - a.clone() * a.clone();
            if w2 <= zero() {
                return Err(unsupported("quadratic with real roots"));
            }
            let w = Sym::pow(Sym::Num(w2), half())?;
            let wt = Sym::product(vec![w.clone(), Sym::symbol(t)])?;
            let exp_part = exp_of(&a, t)?;
            // n1 s + n0 = n1 (s-a) + (n0 + n1 a).
            let sin_num = n0 + n1.clone() * a;
            let cos_term = Sym::product(vec![
                c.clone(),
                Sym::Num(n1 / c2.clone()),
                exp_part.clone(),
                Sym::func("cos", vec![wt.clone()])?,
            ])?;
            let sin_term = Sym::product(vec![
                c,
                Sym::Num(sin_num / c2),
                Sym::pow(w, int(-1))?,
                exp_part,
                Sym::func("sin", vec![wt])?,
            ])?;
            Sym::sum(vec![cos_term, sin_term])
        }
        _ => Err(unsupported("inverse of this term")),
    }
}

/// `exp(r t)`, or 1 when `r` is zero.
fn exp_of(r: &Q, t: &str) -> Result<Sym, SymError> {
    if r.is_zero() {
        return Ok(Sym::int(1));
    }
    Sym::func(
        "exp",
        vec![Sym::product(vec![Sym::Num(r.clone()), Sym::symbol(t)])?],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::tests::sy;

    /// Whether `a` and `b` are equal (their difference simplifies to zero).
    fn same(a: &Sym, b: &Sym) -> bool {
        match a.clone().minus(b.clone()).and_then(|d| simplify(&d)) {
            Ok(d) => d.is_zero(),
            Err(_) => false,
        }
    }

    fn lap(src: &str) -> Result<Sym, SymError> {
        laplace(&sy(src), "t", "s")
    }

    fn inv(src: &str) -> Result<Sym, SymError> {
        inverse_laplace(&sy(src), "s", "t")
    }

    /// Time domain round trip: the inverse of the transform of `src` is `src`.
    fn time_round_trip(src: &str) {
        let f = sy(src);
        let forward = laplace(&f, "t", "s").expect("forward transform");
        let back = inverse_laplace(&forward, "s", "t").expect("inverse transform");
        assert!(same(&back, &f), "round trip of {src} gives {back}");
    }

    /// Transform domain round trip: the transform of the inverse of `src` is `src`.
    fn transform_round_trip(src: &str) {
        let big_f = sy(src);
        let time = inverse_laplace(&big_f, "s", "t").expect("inverse transform");
        let back = laplace(&time, "t", "s").expect("forward transform");
        assert!(
            same(&back, &big_f),
            "transform round trip of {src} gives {back}"
        );
    }

    #[test]
    fn table_entries() {
        assert!(same(&lap("1").unwrap(), &sy("1/s")));
        assert!(same(&lap("t").unwrap(), &sy("1/s^2")));
        assert!(same(&lap("t^3").unwrap(), &sy("6/s^4")));
        assert!(same(&lap("exp(t)").unwrap(), &sy("1/(s-1)")));
        assert!(same(&lap("exp(-(a*t))").unwrap(), &sy("1/(s+a)")));
        assert!(same(&lap("sin(2*t)").unwrap(), &sy("2/(s^2+4)")));
        assert!(same(&lap("cos(3*t)").unwrap(), &sy("s/(s^2+9)")));
    }

    #[test]
    fn shifts_and_linearity() {
        assert!(same(&lap("t^2*exp(3*t)").unwrap(), &sy("2/(s-3)^3")));
        assert!(same(&lap("5*exp(2*t)+3").unwrap(), &sy("5/(s-2)+3/s")));
        assert!(same(&lap("exp(t+1)").unwrap(), &sy("exp(1)/(s-1)")));
        // e^t sin(2t) shifts the sine: 2/((s-1)^2+4).
        assert!(same(&lap("exp(t)*sin(2*t)").unwrap(), &sy("2/((s-1)^2+4)")));
    }

    #[test]
    fn product_with_a_sum_is_expanded() {
        // (t+1) e^t = t e^t + e^t: 1/(s-1)^2 + 1/(s-1).
        assert!(same(
            &lap("(t+1)*exp(t)").unwrap(),
            &sy("1/(s-1)^2+1/(s-1)")
        ));
    }

    #[test]
    fn refused_outside_the_table() {
        for src in [
            "sin(t+1)", "t^(1/2)", "exp(t^2)", "t*sin(t)", "1/t", "sinh(t)",
        ] {
            assert!(
                matches!(lap(src), Err(SymError::Unsupported(_))),
                "{src} should be refused"
            );
        }
    }

    #[test]
    fn inverse_table_entries() {
        assert!(same(&inv("1/s").unwrap(), &sy("1")));
        assert!(same(&inv("1/(s-1)").unwrap(), &sy("exp(t)")));
        assert!(same(&inv("2/(s^2+4)").unwrap(), &sy("sin(2*t)")));
        assert!(same(&inv("1/(s-1)^2").unwrap(), &sy("t*exp(t)")));
    }

    #[test]
    fn time_domain_round_trips() {
        for src in [
            "t^2",
            "exp(2*t)",
            "t^2*exp(-t)",
            "3*sin(t)-cos(2*t)",
            "exp(t)*cos(3*t)",
            "5*exp(-4*t)+t",
        ] {
            time_round_trip(src);
        }
    }

    #[test]
    fn transform_domain_round_trips() {
        for src in [
            "1/(s+1)",
            "s/(s^2+4)",
            "1/((s-1)*(s-2))",
            "3/(s^2+s+1)",
            "(2*s+5)/((s+1)*(s^2+2*s+5))",
            "1/(s^2-4*s+8)",
            "s/((s-1)^2+4)",
            "1/s^3",
        ] {
            transform_round_trip(src);
        }
    }

    #[test]
    fn real_root_quadratic_polynomial_part_and_delta_are_refused() {
        // s^2-2 has real irrational roots: no exp/cos/sin form.
        assert!(matches!(inv("1/(s^2-2)"), Err(SymError::Unsupported(_))));
        // A constant term is a Dirac delta.
        assert!(matches!(inv("2"), Err(SymError::Unsupported(_))));
        // A polynomial part (s/(s+1) = 1 - 1/(s+1)) is a Dirac delta.
        assert!(matches!(inv("s/(s+1)"), Err(SymError::Unsupported(_))));
    }
}

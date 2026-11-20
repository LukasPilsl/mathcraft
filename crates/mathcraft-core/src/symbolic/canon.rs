//! Canonical forms of products, powers and sums' coefficients (the work behind
//! [`Sym::product`] and [`Sym::pow`]): like-base combination, the square-root merge, the
//! integer and fractional split of rational powers, and the coefficient size checks.

use std::collections::BTreeMap;

use num_traits::{Signed, ToPrimitive, Zero};

use super::num::{self, half, one, zero, Q};
use super::sym::Sym;
use super::{SymError, MAX_DEGREE};

/// Splits a canonical term into its coefficient and monomial (`Num(1)` for a constant).
pub(super) fn split_coef(t: Sym) -> (Q, Sym) {
    match t {
        Sym::Num(q) => (q, Sym::Num(one())),
        Sym::Mul(fs) => match fs.first() {
            Some(Sym::Num(c)) => {
                let c = c.clone();
                (c, rest_of(&fs))
            }
            _ => (one(), Sym::Mul(fs)),
        },
        other => (one(), other),
    }
}

/// The factors of a canonical `Mul` after its numeric coefficient, as one value.
pub(super) fn rest_of(fs: &[Sym]) -> Sym {
    let rest: Vec<Sym> = match fs.first() {
        Some(Sym::Num(_)) => fs[1..].to_vec(),
        _ => fs.to_vec(),
    };
    if rest.len() == 1 {
        rest.into_iter().next().unwrap_or(Sym::Num(one()))
    } else {
        Sym::Mul(rest)
    }
}

/// Coefficient times a monomial, canonical without re-running the constructors.
pub(crate) fn with_coef(c: Q, mono: Sym) -> Sym {
    if c == one() {
        return mono;
    }
    match mono {
        Sym::Mul(fs) => {
            let mut out = Vec::with_capacity(fs.len() + 1);
            out.push(Sym::Num(c));
            out.extend(fs);
            Sym::Mul(out)
        }
        other => Sym::Mul(vec![Sym::Num(c), other]),
    }
}

/// Splits a product into its numeric value, the remaining factors, and whether a numeric
/// base had to be re-extracted (so the caller must combine again).
pub(super) fn combine(items: Vec<Sym>) -> Result<(Q, Vec<Sym>, bool), SymError> {
    let mut coef = one();
    let mut bases: BTreeMap<Sym, Q> = BTreeMap::new();
    let mut changed = false;
    let mut work = items;
    while let Some(f) = work.pop() {
        super::budget::spend(1)?;
        match f {
            Sym::Num(q) => num::mul_into(&mut coef, &q)?,
            Sym::Mul(fs) => work.extend(fs),
            Sym::Pow(b, e) if matches!(*b, Sym::Num(_) | Sym::Mul(_)) => {
                let r = Sym::pow(*b, e)?;
                match r {
                    Sym::Pow(rb, re) => {
                        let entry = bases.entry(*rb).or_insert_with(zero);
                        num::add_into(entry, &re)?;
                    }
                    other => {
                        changed = true;
                        work.push(other);
                    }
                }
            }
            Sym::Pow(b, e) => {
                let entry = bases.entry(*b).or_insert_with(zero);
                num::add_into(entry, &e)?;
            }
            other => {
                let entry = bases.entry(other).or_insert_with(zero);
                num::add_into(entry, &one())?;
            }
        }
    }
    let mut extra: Vec<Sym> = Vec::new();
    for (base, e) in bases {
        num::check_bits(&e)?;
        if e.is_zero() {
            continue;
        }
        match base {
            Sym::Num(b) => {
                let r = Sym::pow(Sym::Num(b), e)?;
                match r {
                    Sym::Num(q) => num::mul_into(&mut coef, &q)?,
                    Sym::Mul(fs) => {
                        changed = true;
                        for f in fs {
                            match f {
                                Sym::Num(q) => num::mul_into(&mut coef, &q)?,
                                other => extra.push(other),
                            }
                        }
                    }
                    other => extra.push(other),
                }
            }
            base => {
                let r = Sym::pow(base, e)?;
                match r {
                    Sym::Num(q) => num::mul_into(&mut coef, &q)?,
                    other => extra.push(other),
                }
            }
        }
    }
    let merged = merge_square_roots(&mut coef, &mut extra)?;
    Ok((coef, extra, changed || merged))
}

/// Merges the square roots of positive integers in `extra` into one surd, so that
/// `sqrt(2)*sqrt(3)` becomes `sqrt(6)` and `sqrt(8)*sqrt(3)` becomes `2*sqrt(6)`. The numeric
/// factor the merge produces is multiplied into `coef`. Other root degrees are not merged
/// (see the `super` module docs). Returns whether anything was merged.
pub(super) fn merge_square_roots(coef: &mut Q, extra: &mut Vec<Sym>) -> Result<bool, SymError> {
    let int_root_base = |s: &Sym| match s {
        Sym::Pow(b, e) if *e == half() => match &**b {
            Sym::Num(q) if q.is_integer() && q.is_positive() => Some(q.clone()),
            _ => None,
        },
        _ => None,
    };
    if extra.iter().filter(|s| int_root_base(s).is_some()).count() < 2 {
        return Ok(false);
    }
    let mut merged = one();
    let mut rest = Vec::with_capacity(extra.len());
    for s in extra.drain(..) {
        match int_root_base(&s) {
            Some(q) => num::mul_into(&mut merged, &q)?,
            None => rest.push(s),
        }
    }
    let root = Sym::pow(Sym::Num(merged), half())?;
    let factors = match root {
        Sym::Mul(fs) => fs,
        other => vec![other],
    };
    for f in factors {
        match f {
            Sym::Num(q) => num::mul_into(coef, &q)?,
            other => rest.push(other),
        }
    }
    *extra = rest;
    Ok(true)
}

pub(super) fn to_i64(q: &Q) -> Result<i64, SymError> {
    q.numer()
        .to_i64()
        .filter(|_| q.is_integer())
        .ok_or(SymError::TooLarge("exponent"))
}

pub(super) fn build_product(coef: Q, mut extra: Vec<Sym>) -> Result<Sym, SymError> {
    if extra.is_empty() {
        return Ok(Sym::Num(coef));
    }
    extra.sort_by_key(|s| s.to_string());
    if extra.len() == 1 && coef != one() {
        if let Sym::Add(ts) = &extra[0] {
            let distributed = ts
                .iter()
                .map(|t| Sym::product(vec![Sym::Num(coef.clone()), t.clone()]))
                .collect::<Result<Vec<_>, _>>()?;
            return Sym::sum(distributed);
        }
    }
    if extra.len() == 1 && coef == one() {
        return Ok(extra.remove(0));
    }
    let mut fs = Vec::with_capacity(extra.len() + 1);
    if coef != one() {
        fs.push(Sym::Num(coef));
    }
    fs.extend(extra);
    Ok(Sym::Mul(fs))
}

pub(super) fn check_int_exponent(e: &Q) -> Result<(), SymError> {
    let over = match e.numer().to_i64() {
        Some(n) => n.unsigned_abs() > u64::from(MAX_DEGREE),
        None => true,
    };
    if over {
        Err(SymError::TooLarge("exponent"))
    } else {
        Ok(())
    }
}

pub(super) fn pow_num(b: Q, e: Q) -> Result<Sym, SymError> {
    if b.is_zero() {
        return if e.is_negative() {
            Err(SymError::Undefined("zero to a negative power"))
        } else {
            Ok(Sym::Num(zero()))
        };
    }
    if b == one() {
        return Ok(Sym::Num(one()));
    }
    if e.is_integer() {
        let n = e.numer().to_i64().ok_or(SymError::TooLarge("exponent"))?;
        return Ok(Sym::Num(num::checked_pow(&b, n)?));
    }
    if b.is_negative() {
        return Ok(Sym::Pow(Box::new(Sym::Num(b)), e));
    }
    let whole = e.floor();
    if !whole.is_zero() {
        // b^(n + f) = b^n * b^f with 0 <= f < 1 (n = floor(e) != 0): 2^(3/2) = 2 * 2^(1/2),
        // 2^(-1/2) = 1/2 * 2^(1/2). The fractional part has floor 0, so this terminates.
        let f = e.clone() - whole.clone();
        let head = num::checked_pow(&b, to_i64(&whole)?)?;
        let tail = pow_num(b, f)?;
        return Sym::product(vec![Sym::Num(head), tail]);
    }
    let den = e
        .denom()
        .to_u32()
        .ok_or(SymError::TooLarge("root degree"))?;
    if den > MAX_DEGREE {
        return Err(SymError::TooLarge("root degree"));
    }
    let p = e.numer().to_i64().ok_or(SymError::TooLarge("exponent"))?;
    if let Some(root) = num::exact_root(&b, den) {
        return Ok(Sym::Num(num::checked_pow(&root, p)?));
    }
    if den == 2 && p == 1 {
        let (outside, inside) = num::sqrt_split(&b);
        if inside == one() {
            return Ok(Sym::Num(outside));
        }
        if outside != one() {
            return Ok(Sym::Mul(vec![
                Sym::Num(outside),
                Sym::Pow(Box::new(Sym::Num(inside)), e),
            ]));
        }
    }
    Ok(Sym::Pow(Box::new(Sym::Num(b)), e))
}

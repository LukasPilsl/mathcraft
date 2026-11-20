//! Factoring of sums in several unknowns over Q: the linear factors `p v + q`, where `v` is one
//! unknown and `p`, `q` are polynomials in the others.
//!
//! For each unknown `v` the sum is read as a polynomial in `v`. A linear factor `p v + q` has `p`
//! dividing the leading coefficient and `q` dividing the constant term, so the candidates come from
//! the divisors of those two coefficients (their own factorisation, found recursively, and the
//! divisors of the integer content). A candidate is accepted only by exact polynomial division
//! (`exact_div`), and every extracted factor is divided out, so a repeated factor is found
//! repeatedly.
//!
//! The answer is a factorisation only when what is left is certified irreducible: a constant, a
//! polynomial of total degree one, or one that is linear in some unknown with a constant leading
//! coefficient. Anything else is refused by the caller, so a partial factorisation is never
//! presented as complete. The product is expanded and compared with the input before it is
//! returned.

use std::cmp::Ordering;

use num_traits::{One, Signed, ToPrimitive, Zero};

use super::budget;
use super::factor::{factor_rec, integer_factors};
use super::num::{int, one, Q};
use super::poly::{Mono, Poly};
use super::sym::Sym;
use super::SymError;

/// Candidate linear factors tried per unknown before giving up.
const MAX_CANDIDATES: usize = 2_000;
/// Divisors listed for one coefficient.
const MAX_DIVISORS: usize = 48;
/// Linear factors extracted in total.
const MAX_FACTORS: usize = 24;

/// Lexicographic order of monomials over the union of their atoms (admissible for division).
fn lex_cmp(a: &Mono, b: &Mono) -> Ordering {
    let mut keys: Vec<&Sym> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let x = a.get(k).copied().unwrap_or(0);
        let y = b.get(k).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

/// The lex-largest term of a nonzero polynomial.
fn leading(p: &Poly) -> Option<(&Mono, &Q)> {
    p.terms.iter().max_by(|a, b| lex_cmp(a.0, b.0))
}

/// `a / b` when `b` divides `a` exactly, else `None`.
pub(crate) fn exact_div(a: &Poly, b: &Poly) -> Result<Option<Poly>, SymError> {
    let Some((lm_b, lc_b)) = leading(b).map(|(m, c)| (m.clone(), c.clone())) else {
        return Ok(None);
    };
    let mut rem = a.clone();
    let mut quotient = Poly::zero();
    while !rem.is_zero() {
        budget::spend(1)?;
        let Some((lm_r, lc_r)) = leading(&rem).map(|(m, c)| (m.clone(), c.clone())) else {
            break;
        };
        let mut diff = Mono::new();
        for (atom, e) in &lm_r {
            let f = lm_b.get(atom).copied().unwrap_or(0);
            if *e < f {
                return Ok(None);
            }
            if *e > f {
                diff.insert(atom.clone(), *e - f);
            }
        }
        if lm_b.keys().any(|k| !lm_r.contains_key(k)) {
            return Ok(None);
        }
        let mut single = Poly::zero();
        single.terms.insert(diff, lc_r / lc_b.clone());
        quotient = quotient.add(&single)?;
        rem = rem.add(&single.mul(b)?.scale(&int(-1))?)?;
    }
    Ok(Some(quotient))
}

/// Integer divisors (positive) of `n`, up to [`MAX_DIVISORS`].
fn integer_divisors(n: &Q) -> Vec<Q> {
    let Some(m) = n.abs().to_integer().to_u64() else {
        return vec![one()];
    };
    if m == 0 {
        return vec![one()];
    }
    let Ok(primes) = integer_factors(m) else {
        return vec![one()];
    };
    let mut out: Vec<u64> = vec![1];
    for (p, e) in primes {
        let base = out.clone();
        let mut power = 1u64;
        for _ in 0..e {
            power = power.saturating_mul(p);
            out.extend(base.iter().map(|d| d.saturating_mul(power)));
        }
        if out.len() > MAX_DIVISORS {
            out.truncate(MAX_DIVISORS);
            break;
        }
    }
    out.sort_unstable();
    out.dedup();
    out.into_iter().map(|d| int(d as i64)).collect()
}

/// Divisors of `c` as polynomials (positive integer content divisors times products of the
/// irreducible parts that the recursive factorisation finds), up to [`MAX_DIVISORS`].
fn divisors(c: &Poly) -> Result<Vec<Poly>, SymError> {
    if c.is_zero() {
        return Ok(Vec::new());
    }
    let sym = c.to_sym()?;
    // A coefficient the recursion cannot factor is one indivisible part.
    let factored = factor_rec(&sym).unwrap_or(sym.clone());
    let parts: Vec<Sym> = match factored {
        Sym::Mul(fs) => fs,
        other => vec![other],
    };
    let mut content = one();
    let mut atoms: Vec<(Poly, u32)> = Vec::new();
    for part in parts {
        match part {
            Sym::Num(q) => content = q,
            Sym::Pow(b, e) if e.is_integer() && e.is_positive() => {
                let n = e.to_integer().to_u32().unwrap_or(1).min(8);
                atoms.push((Poly::from_sym(&b)?, n));
            }
            other => atoms.push((Poly::from_sym(&other)?, 1)),
        }
    }
    let mut out: Vec<Poly> = integer_divisors(&content)
        .into_iter()
        .map(Poly::constant)
        .collect();
    for (atom, e) in atoms {
        let base = out.clone();
        let mut power = Poly::constant(one());
        for _ in 0..e {
            power = power.mul(&atom)?;
            for d in &base {
                out.push(d.mul(&power)?);
            }
            if out.len() > MAX_DIVISORS {
                out.truncate(MAX_DIVISORS);
                return Ok(out);
            }
        }
    }
    Ok(out)
}

/// Content of an integer-or-rational polynomial: the positive rational `k` with `p / k` primitive
/// over the integers.
fn content(p: &Poly) -> Q {
    let mut num_gcd = Q::zero();
    let mut den_lcm = Q::one();
    for c in p.terms.values() {
        num_gcd = super::num::gcd_int(num_gcd, Q::from_integer(c.numer().clone()));
        den_lcm = super::num::lcm_int(&den_lcm, &Q::from_integer(c.denom().clone()));
    }
    if num_gcd.is_zero() {
        return Q::one();
    }
    num_gcd / den_lcm
}

/// Whether `p` is certified irreducible over Q (see the module docs).
fn irreducible(p: &Poly) -> Result<bool, SymError> {
    if p.terms.keys().all(|m| m.values().sum::<u32>() <= 1) {
        return Ok(true);
    }
    let atoms: Vec<Sym> = {
        let mut a: Vec<Sym> = p.terms.keys().flat_map(|m| m.keys().cloned()).collect();
        a.sort();
        a.dedup();
        a
    };
    for v in atoms {
        if p.degree_in(&v) == Some(1) {
            let cs = p.coeffs_in(&v)?;
            if cs.get(1).is_some_and(|lc| {
                lc.terms.len() == 1 && lc.terms.keys().all(std::collections::BTreeMap::is_empty)
            }) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A linear factor of `r` in `v`, with the quotient, or `None`.
fn linear_factor(r: &Poly, v: &Sym) -> Result<Option<(Poly, Poly)>, SymError> {
    let cs = r.coeffs_in(v)?;
    if cs.len() < 2 {
        return Ok(None);
    }
    let (c0, cd) = (&cs[0], &cs[cs.len() - 1]);
    if c0.is_zero() {
        return Ok(None);
    }
    let ps = divisors(cd)?;
    let qs = divisors(c0)?;
    let mut tried = 0usize;
    let var = Poly::atom(v.clone());
    for p in &ps {
        for q in &qs {
            for sign in [1i64, -1] {
                tried += 1;
                if tried > MAX_CANDIDATES {
                    return Ok(None);
                }
                budget::spend(1)?;
                let l = p.mul(&var)?.add(&q.scale(&int(sign))?)?;
                // Primitive candidates only: a content shows up as a smaller candidate.
                if content(&l) != Q::one() {
                    continue;
                }
                if let Some(quotient) = exact_div(r, &l)? {
                    return Ok(Some((l, quotient)));
                }
            }
        }
    }
    Ok(None)
}

/// The factorisation of the sum `s` (with the polynomial form `poly`) into linear factors in one
/// unknown and a certified irreducible rest, or `None` when no factor is found or the rest is not
/// certified.
pub(crate) fn factor_multivariate(s: &Sym, poly: &Poly) -> Result<Option<Sym>, SymError> {
    let mut atoms: Vec<Sym> = poly.terms.keys().flat_map(|m| m.keys().cloned()).collect();
    atoms.sort();
    atoms.dedup();
    if atoms.len() < 2 {
        return Ok(None);
    }
    let mut k = content(poly);
    if leading(poly).is_some_and(|(_, c)| c.is_negative()) {
        k = -k;
    }
    let mut rest = poly.scale(&(Q::one() / k.clone()))?;
    let mut factors: Vec<Sym> = Vec::new();
    'extract: while factors.len() < MAX_FACTORS {
        for v in &atoms {
            if let Some((l, quotient)) = linear_factor(&rest, v)? {
                factors.push(l.to_sym()?);
                rest = quotient;
                continue 'extract;
            }
        }
        break;
    }
    if factors.is_empty() {
        return Ok(None);
    }
    // What is left must be a constant or certified irreducible.
    let rest_sym = match rest.terms.len() {
        0 => return Ok(None),
        _ if rest.terms.keys().all(std::collections::BTreeMap::is_empty) => {
            if let Some(c) = rest.terms.values().next() {
                k *= c.clone();
            }
            None
        }
        _ if irreducible(&rest)? => Some(rest.to_sym()?),
        _ => return Ok(None),
    };
    let mut parts = vec![Sym::Num(k)];
    parts.extend(factors);
    parts.extend(rest_sym);
    let product = Sym::product(parts)?;
    // The independent check: the product expands back to the input.
    if &Poly::from_sym(&product)? != poly {
        return Err(SymError::Unsupported(
            "factor: multivariate result failed its expansion check".into(),
        ));
    }
    let _ = s;
    Ok(Some(product))
}

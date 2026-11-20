//! Multivariate polynomials over Q whose variables ("atoms") are arbitrary canonical values:
//! symbols, units, functions and powers with non-integer or negative exponents.
//!
//! Every arithmetic step checks the term, degree and coefficient-size budgets, so hostile
//! input returns [`SymError::TooLarge`] instead of running away.

use std::collections::BTreeMap;

use num_traits::{Signed, ToPrimitive, Zero};

use super::num::{self, check_bits, int, one, zero, Q};
use super::sym::Sym;
use super::{SymError, MAX_DEGREE, MAX_TERMS};

/// Exponents of the atoms in one monomial (absent atom means exponent 0).
pub(crate) type Mono = BTreeMap<Sym, u32>;

/// A polynomial: monomial to nonzero coefficient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Poly {
    pub(crate) terms: BTreeMap<Mono, Q>,
}

impl Poly {
    pub(crate) fn zero() -> Poly {
        Poly {
            terms: BTreeMap::new(),
        }
    }

    pub(crate) fn constant(c: Q) -> Poly {
        let mut terms = BTreeMap::new();
        if !c.is_zero() {
            terms.insert(Mono::new(), c);
        }
        Poly { terms }
    }

    pub(crate) fn atom(a: Sym) -> Poly {
        let mut mono = Mono::new();
        mono.insert(a, 1);
        let mut terms = BTreeMap::new();
        terms.insert(mono, one());
        Poly { terms }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.terms.is_empty()
    }

    /// Sum of two polynomials.
    pub(crate) fn add(&self, other: &Poly) -> Result<Poly, SymError> {
        let mut terms = self.terms.clone();
        for (m, c) in &other.terms {
            let entry = terms.entry(m.clone()).or_insert_with(zero);
            num::add_into(entry, c)?;
        }
        terms.retain(|_, c| !c.is_zero());
        finish(terms)
    }

    /// Product of two polynomials.
    pub(crate) fn mul(&self, other: &Poly) -> Result<Poly, SymError> {
        // Charged per term pair so that a large product cannot escape the work budget.
        super::budget::spend((self.terms.len() as u64).saturating_mul(other.terms.len() as u64))?;
        let mut terms: BTreeMap<Mono, Q> = BTreeMap::new();
        for (m1, c1) in &self.terms {
            for (m2, c2) in &other.terms {
                let mut m = m1.clone();
                for (atom, e) in m2 {
                    let entry = m.entry(atom.clone()).or_insert(0);
                    *entry = entry.saturating_add(*e);
                }
                let degree: u64 = m.values().map(|e| u64::from(*e)).sum();
                if degree > u64::from(MAX_DEGREE) {
                    return Err(SymError::TooLarge("degree"));
                }
                super::budget::spend(1)?;
                let mut prod = c1.clone();
                num::mul_into(&mut prod, c2)?;
                let entry = terms.entry(m).or_insert_with(zero);
                num::add_into(entry, &prod)?;
                if terms.len() > MAX_TERMS {
                    return Err(SymError::TooLarge("polynomial terms"));
                }
            }
        }
        terms.retain(|_, c| !c.is_zero());
        finish(terms)
    }

    /// Integer power by repeated squaring.
    pub(crate) fn pow(&self, n: u32) -> Result<Poly, SymError> {
        let _scope = super::budget::Scope::enter();
        if n > MAX_DEGREE {
            return Err(SymError::TooLarge("exponent"));
        }
        let mut result = Poly::constant(one());
        let mut base = self.clone();
        let mut k = n;
        while k > 0 {
            if k & 1 == 1 {
                result = result.mul(&base)?;
            }
            k >>= 1;
            if k > 0 {
                base = base.mul(&base)?;
            }
        }
        Ok(result)
    }

    /// Multiplies every coefficient by `c`.
    pub(crate) fn scale(&self, c: &Q) -> Result<Poly, SymError> {
        let terms = self
            .terms
            .iter()
            .map(|(m, k)| (m.clone(), k.clone() * c.clone()))
            .filter(|(_, k)| !k.is_zero())
            .collect();
        finish(terms)
    }

    /// Builds the polynomial of a canonical value. Sums and products are multiplied out;
    /// integer powers are expanded; any other value is an atom (with its inside expanded).
    pub(crate) fn from_sym(s: &Sym) -> Result<Poly, SymError> {
        match s {
            Sym::Num(q) => Ok(Poly::constant(q.clone())),
            Sym::Add(ts) => {
                let mut acc = Poly::zero();
                for t in ts {
                    acc = acc.add(&Poly::from_sym(t)?)?;
                }
                Ok(acc)
            }
            Sym::Mul(fs) => {
                let mut acc = Poly::constant(one());
                for f in fs {
                    acc = acc.mul(&Poly::from_sym(f)?)?;
                }
                Ok(acc)
            }
            Sym::Pow(b, e) if e.is_integer() && !e.is_negative() => {
                let n = e
                    .numer()
                    .to_i64()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or(SymError::TooLarge("exponent"))?;
                Poly::from_sym(b)?.pow(n)
            }
            Sym::Pow(b, e) => {
                let inner = expand_inside(b)?;
                Ok(Poly::atom(Sym::pow(inner, e.clone())?))
            }
            Sym::Func(name, args) => {
                let a = args
                    .iter()
                    .map(expand_inside)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Poly::atom(Sym::func(name, a)?))
            }
            Sym::Symbol(_) | Sym::Unit(_) => Ok(Poly::atom(s.clone())),
        }
    }

    /// Canonical value of the polynomial.
    pub(crate) fn to_sym(&self) -> Result<Sym, SymError> {
        let mut terms = Vec::with_capacity(self.terms.len());
        for (mono, c) in &self.terms {
            let mut fs = vec![Sym::Num(c.clone())];
            for (atom, e) in mono {
                fs.push(Sym::pow(atom.clone(), int(i64::from(*e)))?);
            }
            terms.push(Sym::product(fs)?);
        }
        Sym::sum(terms)
    }

    /// Largest exponent of the atom `var` (None for the zero polynomial).
    pub(crate) fn degree_in(&self, var: &Sym) -> Option<u32> {
        self.terms
            .keys()
            .map(|m| m.get(var).copied().unwrap_or(0))
            .max()
    }

    /// Splits the polynomial by powers of `var`: index `k` holds the coefficient of
    /// `var^k` (a polynomial free of `var`).
    pub(crate) fn coeffs_in(&self, var: &Sym) -> Result<Vec<Poly>, SymError> {
        let deg = match self.degree_in(var) {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };
        let mut out: Vec<Poly> = (0..=deg).map(|_| Poly::zero()).collect();
        for (mono, c) in &self.terms {
            let mut rest = mono.clone();
            let e = rest.remove(var).unwrap_or(0);
            let mut single = BTreeMap::new();
            single.insert(rest, c.clone());
            let slot = out
                .get_mut(e as usize)
                .ok_or(SymError::TooLarge("degree"))?;
            *slot = slot.add(&Poly { terms: single })?;
        }
        Ok(out)
    }
}

/// Expands the inside of an atom (function arguments, non-integer power bases).
fn expand_inside(s: &Sym) -> Result<Sym, SymError> {
    expand(s)
}

/// Drops nothing further; checks budgets on the finished term map.
fn finish(terms: BTreeMap<Mono, Q>) -> Result<Poly, SymError> {
    if terms.len() > MAX_TERMS {
        return Err(SymError::TooLarge("polynomial terms"));
    }
    for c in terms.values() {
        check_bits(c)?;
    }
    Ok(Poly { terms })
}

/// Multiplies out a canonical value into the polynomial normal form.
pub fn expand(s: &Sym) -> Result<Sym, SymError> {
    let _scope = super::budget::Scope::enter();
    Poly::from_sym(s)?.to_sym()
}

/// Replaces each named symbol by its value and rebuilds canonically (no expansion).
pub fn substitute(s: &Sym, subs: &[(&str, Sym)]) -> Result<Sym, SymError> {
    let _scope = super::budget::Scope::enter();
    substitute_inner(s, subs)
}

fn substitute_inner(s: &Sym, subs: &[(&str, Sym)]) -> Result<Sym, SymError> {
    match s {
        Sym::Symbol(n) => Ok(subs
            .iter()
            .find(|(k, _)| *k == n.as_str())
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| s.clone())),
        Sym::Num(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            let a = args
                .iter()
                .map(|x| substitute_inner(x, subs))
                .collect::<Result<Vec<_>, _>>()?;
            Sym::func(name, a)
        }
        Sym::Add(ts) => Sym::sum(
            ts.iter()
                .map(|t| substitute_inner(t, subs))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Mul(fs) => Sym::product(
            fs.iter()
                .map(|f| substitute_inner(f, subs))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Pow(b, e) => Sym::pow(substitute_inner(b, subs)?, e.clone()),
    }
}

/// Exact value of `s` when every symbol is given a rational value and the result is a
/// number; `None` when something stays symbolic.
pub fn eval_rational(s: &Sym, env: &[(&str, Q)]) -> Result<Option<Q>, SymError> {
    let _scope = super::budget::Scope::enter();
    let subs: Vec<(&str, Sym)> = env.iter().map(|(k, v)| (*k, Sym::Num(v.clone()))).collect();
    match substitute(s, &subs)? {
        Sym::Num(q) => Ok(Some(q)),
        _ => Ok(None),
    }
}

/// Degree of `s` in the variable `var` after multiplying out (None when `s` is zero).
pub fn degree(s: &Sym, var: &str) -> Result<Option<u32>, SymError> {
    let _scope = super::budget::Scope::enter();
    let p = Poly::from_sym(s)?;
    Ok(p.degree_in(&Sym::symbol(var)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_square_of_sum() {
        let a = Sym::symbol("a");
        let b = Sym::symbol("b");
        let sum = Sym::sum(vec![a, b]).expect("sum");
        let sq = Sym::pow(sum, int(2)).expect("pow");
        let e = expand(&sq).expect("expand");
        assert_eq!(e.to_string(), "b^2 + 2*a*b + a^2");
        assert_eq!(expand(&e).expect("again"), e);
    }
}

//! Rational functions `num / den` over the polynomial layer, with cancellation.
//!
//! Cancellation: when the numerator and denominator share one atom, the univariate gcd over
//! Q (Euclid) is removed. Otherwise the common monomial factor is removed. Coefficients are
//! normalised so the denominator's leading coefficient is 1.

use std::collections::{BTreeMap, BTreeSet};

use num_traits::{ToPrimitive, Zero};

use super::num::{self, one, zero, Q};
use super::poly::{Mono, Poly};
use super::sym::Sym;
use super::{SymError, MAX_DEGREE};

/// A rational function with a nonzero denominator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RatFunc {
    pub(crate) num: Poly,
    pub(crate) den: Poly,
}

impl RatFunc {
    pub(crate) fn from_poly(p: Poly) -> RatFunc {
        RatFunc {
            num: p,
            den: Poly::constant(one()),
        }
    }

    pub(crate) fn zero() -> RatFunc {
        RatFunc::from_poly(Poly::zero())
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    /// The value as a constant rational, if it is one.
    pub(crate) fn as_const(&self) -> Option<Q> {
        let n = const_of(&self.num)?;
        let d = const_of(&self.den)?;
        if d.is_zero() {
            None
        } else {
            Some(n / d)
        }
    }

    /// Builds `num / den` in canonical form.
    pub(crate) fn make(num: Poly, den: Poly) -> Result<RatFunc, SymError> {
        super::budget::spend(1)?;
        if den.is_zero() {
            return Err(SymError::Undefined("division by zero"));
        }
        if num.is_zero() {
            return Ok(RatFunc::zero());
        }
        let (n, d) = cancel(num, den)?;
        let lead = match d.terms.iter().next_back() {
            Some((_, c)) => c.clone(),
            None => return Err(SymError::Undefined("division by zero")),
        };
        let inv = one() / lead;
        Ok(RatFunc {
            num: n.scale(&inv)?,
            den: d.scale(&inv)?,
        })
    }

    pub(crate) fn plus(&self, o: &RatFunc) -> Result<RatFunc, SymError> {
        if self.den == o.den {
            return RatFunc::make(self.num.add(&o.num)?, self.den.clone());
        }
        let n = self.num.mul(&o.den)?.add(&o.num.mul(&self.den)?)?;
        RatFunc::make(n, self.den.mul(&o.den)?)
    }

    pub(crate) fn neg(&self) -> Result<RatFunc, SymError> {
        Ok(RatFunc {
            num: self.num.scale(&(-one()))?,
            den: self.den.clone(),
        })
    }

    pub(crate) fn minus(&self, o: &RatFunc) -> Result<RatFunc, SymError> {
        self.plus(&o.neg()?)
    }

    pub(crate) fn times(&self, o: &RatFunc) -> Result<RatFunc, SymError> {
        RatFunc::make(self.num.mul(&o.num)?, self.den.mul(&o.den)?)
    }

    pub(crate) fn over(&self, o: &RatFunc) -> Result<RatFunc, SymError> {
        if o.is_zero() {
            return Err(SymError::Undefined("division by zero"));
        }
        RatFunc::make(self.num.mul(&o.den)?, self.den.mul(&o.num)?)
    }

    /// Builds the rational function of a canonical value.
    pub(crate) fn from_sym(s: &Sym) -> Result<RatFunc, SymError> {
        match s {
            Sym::Add(ts) => {
                let mut acc = RatFunc::zero();
                for t in ts {
                    acc = acc.plus(&RatFunc::from_sym(t)?)?;
                }
                Ok(acc)
            }
            Sym::Mul(fs) => {
                let mut acc = RatFunc::from_poly(one_poly());
                for f in fs {
                    acc = acc.times(&RatFunc::from_sym(f)?)?;
                }
                Ok(acc)
            }
            Sym::Pow(b, e) if e.is_integer() => {
                let r = RatFunc::from_sym(b)?;
                let n = e.numer().to_i64().ok_or(SymError::TooLarge("exponent"))?;
                let m = n.unsigned_abs();
                if m > u64::from(MAX_DEGREE) {
                    return Err(SymError::TooLarge("exponent"));
                }
                let m = m as u32;
                if n >= 0 {
                    RatFunc::make(r.num.pow(m)?, r.den.pow(m)?)
                } else {
                    if r.is_zero() {
                        return Err(SymError::Undefined("zero to a negative power"));
                    }
                    RatFunc::make(r.den.pow(m)?, r.num.pow(m)?)
                }
            }
            other => Ok(RatFunc::from_poly(Poly::from_sym(other)?)),
        }
    }

    /// Canonical value `num * den^-1`.
    pub(crate) fn to_sym(&self) -> Result<Sym, SymError> {
        let n = self.num.to_sym()?;
        if self.den.terms.len() == 1 {
            if let Some((mono, c)) = self.den.terms.iter().next() {
                if mono.is_empty() && *c == one() {
                    return Ok(n);
                }
            }
        }
        let d = self.den.to_sym()?;
        Sym::product(vec![n, Sym::pow(d, -one())?])
    }
}

fn one_poly() -> Poly {
    Poly::constant(one())
}

/// The coefficient of a polynomial with no atoms (0 for the zero polynomial).
fn const_of(p: &Poly) -> Option<Q> {
    match p.terms.len() {
        0 => Some(zero()),
        1 => {
            let (m, c) = p.terms.iter().next()?;
            if m.is_empty() {
                Some(c.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Removes common factors of `num` and `den`.
fn cancel(num: Poly, den: Poly) -> Result<(Poly, Poly), SymError> {
    let mut atoms: BTreeSet<Sym> = BTreeSet::new();
    for p in [&num, &den] {
        for m in p.terms.keys() {
            atoms.extend(m.keys().cloned());
        }
    }
    if atoms.len() <= 1 {
        if let Some(v) = atoms.iter().next() {
            let un = to_upoly(&num, v);
            let ud = to_upoly(&den, v);
            let g = upoly_gcd(un.clone(), ud.clone())?;
            if g.len() > 1 {
                let qn = upoly_div_exact(&un, &g)?;
                let qd = upoly_div_exact(&ud, &g)?;
                return Ok((from_upoly(&qn, v)?, from_upoly(&qd, v)?));
            }
        }
        return Ok((num, den));
    }
    // Multivariate: remove the monomial content common to every term of num and den.
    let mut common: BTreeMap<Sym, u32> = BTreeMap::new();
    for a in &atoms {
        let mut least = u32::MAX;
        for p in [&num, &den] {
            for m in p.terms.keys() {
                least = least.min(m.get(a).copied().unwrap_or(0));
            }
        }
        if least > 0 && least != u32::MAX {
            common.insert(a.clone(), least);
        }
    }
    if common.is_empty() {
        return Ok((num, den));
    }
    Ok((divide_mono(&num, &common)?, divide_mono(&den, &common)?))
}

fn divide_mono(p: &Poly, common: &BTreeMap<Sym, u32>) -> Result<Poly, SymError> {
    let mut terms: BTreeMap<Mono, Q> = BTreeMap::new();
    for (m, c) in &p.terms {
        let mut nm = m.clone();
        for (a, k) in common {
            if let Some(e) = nm.get_mut(a) {
                *e -= k;
            }
        }
        nm.retain(|_, e| *e > 0);
        terms.insert(nm, c.clone());
    }
    Ok(Poly { terms })
}

type UPoly = Vec<Q>;

fn to_upoly(p: &Poly, atom: &Sym) -> UPoly {
    let deg = p.degree_in(atom).unwrap_or(0) as usize;
    let mut v: UPoly = vec![zero(); deg + 1];
    for (m, c) in &p.terms {
        let e = m.get(atom).copied().unwrap_or(0) as usize;
        v[e] = v[e].clone() + c.clone();
    }
    upoly_trim(v)
}

fn from_upoly(v: &UPoly, atom: &Sym) -> Result<Poly, SymError> {
    let mut terms: BTreeMap<Mono, Q> = BTreeMap::new();
    for (e, c) in v.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let mut m = Mono::new();
        if e > 0 {
            let e32 = u32::try_from(e).map_err(|_| SymError::TooLarge("degree"))?;
            m.insert(atom.clone(), e32);
        }
        terms.insert(m, c.clone());
    }
    Ok(Poly { terms })
}

fn upoly_trim(mut v: UPoly) -> UPoly {
    while v.last().is_some_and(|c| c.is_zero()) {
        v.pop();
    }
    v
}

/// Remainder of `a` divided by `b` (`b` nonzero). Every coefficient update is bit-checked.
fn upoly_rem(mut a: UPoly, b: &UPoly) -> Result<UPoly, SymError> {
    let lead = match b.last() {
        Some(c) => c.clone(),
        None => return Ok(a),
    };
    while a.len() >= b.len() && !a.is_empty() {
        // One step costs b.len() rational operations on possibly large coefficients.
        super::budget::spend(b.len() as u64)?;
        let mut c = a[a.len() - 1].clone();
        num::mul_into(&mut c, &(one() / lead.clone()))?;
        let shift = a.len() - b.len();
        for (i, bi) in b.iter().enumerate() {
            num::submul_into(&mut a[i + shift], &c, bi)?;
        }
        a = upoly_trim(a);
    }
    Ok(a)
}

/// Monic gcd by the Euclidean algorithm.
fn upoly_gcd(mut a: UPoly, mut b: UPoly) -> Result<UPoly, SymError> {
    while !b.is_empty() {
        let r = upoly_rem(a, &b)?;
        a = b;
        b = r;
    }
    Ok(match a.last() {
        Some(lead) => {
            let inv = one() / lead.clone();
            a.into_iter().map(|c| c * inv.clone()).collect()
        }
        None => a,
    })
}

/// Exact quotient of `a` by `g` (which divides `a`).
fn upoly_div_exact(a: &UPoly, g: &UPoly) -> Result<UPoly, SymError> {
    let mut r = a.clone();
    let lead = match g.last() {
        Some(c) => c.clone(),
        None => return Ok(a.clone()),
    };
    let qlen = if a.len() >= g.len() {
        a.len() - g.len() + 1
    } else {
        0
    };
    let mut q: UPoly = vec![zero(); qlen];
    while r.len() >= g.len() && !r.is_empty() {
        super::budget::spend(g.len() as u64)?;
        let mut c = r[r.len() - 1].clone();
        num::mul_into(&mut c, &(one() / lead.clone()))?;
        let shift = r.len() - g.len();
        q[shift] = c.clone();
        for (i, gi) in g.iter().enumerate() {
            num::submul_into(&mut r[i + shift], &c, gi)?;
        }
        r = upoly_trim(r);
    }
    Ok(upoly_trim(q))
}

/// Canonical value of a rational expression, with univariate common factors cancelled.
pub fn simplify(s: &Sym) -> Result<Sym, SymError> {
    let _scope = super::budget::Scope::enter();
    RatFunc::from_sym(s)?.to_sym()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn univariate_cancel() {
        let x = Sym::symbol("x");
        let x2 = Sym::pow(x.clone(), crate::symbolic::num::int(2)).expect("pow");
        let num = x2.clone().minus(Sym::int(1)).expect("minus");
        let den = x.clone().minus(Sym::int(1)).expect("minus");
        let frac = num.over(den).expect("over");
        let got = simplify(&frac).expect("simplify");
        let want = x.plus(Sym::int(1)).expect("plus");
        assert_eq!(got, want);
    }
}

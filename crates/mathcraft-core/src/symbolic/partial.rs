//! Partial fractions over the rationals (Prime's `parfrac` keyword, help: "To Expand a Quotient
//! of Polynomials into Partial Fractions": decomposition over the field of rational numbers).
//!
//! Method: the polynomial part is divided out; the denominator is split by its rational roots
//! (with multiplicity); for a root `r` of multiplicity `m` the coefficients come from the Taylor
//! series of `rem / (den/(x-r)^m)` at `r`. A leftover factor with no rational root is accepted
//! when it has degree 2 or 3 (irreducible over Q); its numerator is found by division. The result
//! is checked by recombining it into one rational function and comparing with the input, so an
//! answer that does not match is refused, never returned.
//!
//! Refused (`Unsupported`): a leftover of degree 4 or more (it may factor into quadratics over Q,
//! which this stage does not split), symbolic coefficients, and the `parfrac, sqrt(2)` form.

use num_traits::{One, Zero};

use super::budget;
use super::limit::{has_complex_value, is_polynomial_ratio, numeric_coefficients};
use super::num::{self, Q};
use super::poly_solve::{deflate, find_rational_root};
use super::ratfunc::{simplify, RatFunc};
use super::sym::Sym;
use super::SymError;

/// Largest degree of the leftover factor (no rational root) that is accepted as irreducible.
const MAX_LEFTOVER_DEGREE: usize = 3;

/// Partial fraction decomposition of `s` in `var` over Q. An expression that is not a quotient
/// of polynomials in `var` is refused; a polynomial or constant is returned as it is.
pub fn parfrac(s: &Sym, var: &str) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    let s = simplify(s)?;
    if !s.mentions_any(&[var.to_string()]) {
        if has_complex_value(&s) {
            return Err(SymError::Unsupported(
                "parfrac of a constant with a complex value (real only)".into(),
            ));
        }
        return Ok(s);
    }
    if !is_polynomial_ratio(&s, var) {
        return Err(SymError::Unsupported(
            "parfrac of an expression that is not a rational function".into(),
        ));
    }
    let rf = RatFunc::from_sym(&s)?;
    let num_c = numeric_coefficients(&rf.num.to_sym()?, var)?;
    let den_c = numeric_coefficients(&rf.den.to_sym()?, var)?;
    if den_c.len() == 1 {
        return Ok(s);
    }
    let (quot, rem) = divmod(&num_c, &den_c)?;

    // Rational roots of the denominator, each with its multiplicity.
    let mut rest = den_c.clone();
    let mut roots: Vec<(Q, usize)> = Vec::new();
    while rest.len() > 1 {
        let Some(r) = next_rational_root(&rest)? else {
            break;
        };
        let mut m = 0;
        while rest.len() > 1 && eval(&rest, &r).is_zero() {
            rest = deflate(&rest, &r)?;
            m += 1;
        }
        roots.push((r, m));
    }
    let leftover = rest.len() - 1;
    if leftover > MAX_LEFTOVER_DEGREE {
        return Err(SymError::Unsupported(
            "parfrac needs a factor of degree 4 or more that is not split over Q".into(),
        ));
    }

    // Linear parts: (root, power k, coefficient A) for A/(x-r)^k.
    let mut linear: Vec<(Q, usize, Q)> = Vec::new();
    for (r, m) in &roots {
        let mut d_r = den_c.clone();
        for _ in 0..*m {
            d_r = deflate(&d_r, r)?;
        }
        let ns = taylor_shift(&rem, r)?;
        let ds = taylor_shift(&d_r, r)?;
        let g = taylor_quotient(&ns, &ds, *m)?;
        for (j, a) in g.into_iter().enumerate() {
            if !a.is_zero() {
                linear.push((r.clone(), m - j, a));
            }
        }
    }

    let mut parts: Vec<Sym> = Vec::new();
    if quot.len() > 1 || !quot[0].is_zero() {
        parts.push(poly_sym(&quot, var)?);
    }
    for (r, k, a) in &linear {
        let base = Sym::sum(vec![Sym::symbol(var), Sym::Num(-r.clone())])?;
        let power = Sym::pow(base, num::int(-(*k as i64)))?;
        parts.push(Sym::product(vec![Sym::Num(a.clone()), power])?);
    }

    if leftover > 0 {
        // W = rem - sum A (den/(x-r)^k) = B * (den/rest), so B is W divided by den/rest.
        let mut w = rem.clone();
        for (r, k, a) in &linear {
            let mut t = den_c.clone();
            for _ in 0..*k {
                t = deflate(&t, r)?;
            }
            w = sub_scaled(&w, &t, a)?;
        }
        let (p, p_rem) = divmod(&den_c, &rest)?;
        if !is_zero_poly(&p_rem) {
            return Err(SymError::Unsupported("parfrac factor check failed".into()));
        }
        let (b, b_rem) = divmod(&w, &p)?;
        if !is_zero_poly(&b_rem) {
            return Err(SymError::Unsupported(
                "parfrac numerator check failed".into(),
            ));
        }
        if !is_zero_poly(&b) {
            let numerator = poly_sym(&b, var)?;
            let factor = Sym::pow(poly_sym(&rest, var)?, -num::one())?;
            parts.push(Sym::product(vec![numerator, factor])?);
        }
    }

    let out = Sym::sum(parts)?;
    // The decomposition must recombine into the input exactly.
    if RatFunc::from_sym(&out)? != RatFunc::from_sym(&s)? {
        return Err(SymError::Unsupported(
            "parfrac recombination check failed".into(),
        ));
    }
    Ok(out)
}

/// A rational root of `c` (low to high, rational coefficients). Zero is a root when the constant
/// term vanishes. The search runs on the polynomial scaled to integers.
fn next_rational_root(c: &[Q]) -> Result<Option<Q>, SymError> {
    if c[0].is_zero() {
        return Ok(Some(Q::zero()));
    }
    let mut scale = num::one();
    for q in c {
        num::mul_into(&mut scale, &Q::from_integer(q.denom().clone()))?;
    }
    let mut scaled = Vec::with_capacity(c.len());
    for q in c {
        let v = q * &scale;
        scaled.push(v);
    }
    find_rational_root(&scaled)
}

/// Polynomial value by Horner's rule (low to high coefficients).
fn eval(c: &[Q], x: &Q) -> Q {
    let mut acc = Q::zero();
    for coef in c.iter().rev() {
        acc = acc * x + coef;
    }
    acc
}

/// Trims trailing zero coefficients (the zero polynomial stays `[0]`).
fn trim(mut p: Vec<Q>) -> Vec<Q> {
    while p.len() > 1 && p.last().is_some_and(|q| q.is_zero()) {
        p.pop();
    }
    if p.is_empty() {
        p.push(Q::zero());
    }
    p
}

fn is_zero_poly(p: &[Q]) -> bool {
    p.iter().all(|q| q.is_zero())
}

/// Quotient and remainder of `a / b` over Q (`b` nonzero).
fn divmod(a: &[Q], b: &[Q]) -> Result<(Vec<Q>, Vec<Q>), SymError> {
    let a = trim(a.to_vec());
    let b = trim(b.to_vec());
    let lead_b = b
        .last()
        .cloned()
        .ok_or(SymError::Undefined("division by zero"))?;
    if lead_b.is_zero() {
        return Err(SymError::Undefined("division by zero"));
    }
    let mut rem = a;
    if rem.len() < b.len() {
        return Ok((vec![Q::zero()], rem));
    }
    let mut quot = vec![Q::zero(); rem.len() - b.len() + 1];
    while !is_zero_poly(&rem) && rem.len() >= b.len() {
        budget::spend(b.len() as u64)?;
        let shift = rem.len() - b.len();
        let coef = rem.last().cloned().unwrap_or_else(Q::zero) / &lead_b;
        for (i, bi) in b.iter().enumerate() {
            let t = &coef * bi;
            rem[shift + i] = &rem[shift + i] - t;
        }
        quot[shift] = coef;
        rem.pop();
        if rem.is_empty() {
            rem.push(Q::zero());
        }
        if rem.len() < b.len() {
            break;
        }
    }
    Ok((trim(quot), trim(rem)))
}

/// The coefficients of `p(y + r)` in `y`, low to high.
fn taylor_shift(p: &[Q], r: &Q) -> Result<Vec<Q>, SymError> {
    let n = p.len();
    let mut powers = vec![Q::one(); n];
    for k in 1..n {
        budget::spend(1)?;
        powers[k] = &powers[k - 1] * r;
    }
    let mut out = vec![Q::zero(); n];
    for (j, slot) in out.iter_mut().enumerate() {
        let mut acc = Q::zero();
        for (i, ci) in p.iter().enumerate().skip(j) {
            budget::spend(1)?;
            let binom = num::int(binomial(i, j));
            acc += ci * &binom * &powers[i - j];
        }
        *slot = acc;
    }
    Ok(out)
}

/// `C(n, k)` for the small degrees used here (exact in i64 for every reachable size).
fn binomial(n: usize, k: usize) -> i64 {
    let mut result: i64 = 1;
    for i in 0..k {
        result = result * (n - i) as i64 / (i as i64 + 1);
    }
    result
}

/// The first `m` coefficients of `ns / ds` as a power series in `y`, where `ds(0)` is nonzero.
fn taylor_quotient(ns: &[Q], ds: &[Q], m: usize) -> Result<Vec<Q>, SymError> {
    let d0 = ds.first().cloned().unwrap_or_else(Q::zero);
    if d0.is_zero() {
        return Err(SymError::Unsupported(
            "parfrac: root of the reduced denominator".into(),
        ));
    }
    let mut g: Vec<Q> = Vec::with_capacity(m);
    for j in 0..m {
        budget::spend(1)?;
        let mut acc = ns.get(j).cloned().unwrap_or_else(Q::zero);
        for i in 1..=j {
            if let Some(di) = ds.get(i) {
                acc -= di * &g[j - i];
            }
        }
        g.push(acc / &d0);
    }
    Ok(g)
}

/// `a - t * c` (coefficient-wise, low to high).
fn sub_scaled(a: &[Q], t: &[Q], c: &Q) -> Result<Vec<Q>, SymError> {
    let n = a.len().max(t.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        budget::spend(1)?;
        let ai = a.get(i).cloned().unwrap_or_else(Q::zero);
        let ti = t.get(i).cloned().unwrap_or_else(Q::zero);
        out.push(ai - ti * c);
    }
    Ok(trim(out))
}

/// The polynomial `sum c_k x^k` as a canonical sum.
fn poly_sym(c: &[Q], var: &str) -> Result<Sym, SymError> {
    let x = Sym::symbol(var);
    let mut terms = Vec::new();
    for (k, coef) in c.iter().enumerate() {
        if coef.is_zero() {
            continue;
        }
        let power = Sym::pow(x.clone(), num::int(k as i64))?;
        terms.push(Sym::product(vec![Sym::Num(coef.clone()), power])?);
    }
    Sym::sum(terms)
}

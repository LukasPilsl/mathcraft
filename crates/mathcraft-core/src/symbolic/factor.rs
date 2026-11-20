//! Exact factoring over Q (`factor`), bounded by the symbolic work budget.
//!
//! What it does, in order, for every sum:
//! - Pulls out the content: the integer gcd of the coefficients (with the sign of the leading
//!   term) and the smallest power of every atom that appears in all terms.
//! - Univariate polynomials in a symbol over Q: removes zero roots and every rational root (rational
//!   root theorem, the solver's search). A remainder of degree 2 or 3 with no rational root is
//!   irreducible over Q and is kept. A remainder of degree 4 or more is refused, because it may still
//!   factor over Q.
//! - Multivariate sums: a difference of two squares `p^2 - q^2 = (p - q)(p + q)`, and a square
//!   trinomial `p^2 +- 2pq + q^2 = (p +- q)^2`, where `p` and `q` are exact square roots of terms.
//!   Other multivariate sums split into linear factors in one unknown (see `factor_multi.rs`), when
//!   what is left is certified irreducible.
//!   A sum of total degree at most one (linear) is irreducible and is kept. Any other multivariate
//!   sum is refused: the result is never a partial factorization presented as complete.
//! - Integer content of a sum that is not a monomial (`2x + 2y + 4`) is refused: a numeric
//!   coefficient distributes over a sum in the canonical form, so the factor cannot be kept.
//! - Integer factors (`12 = 2^2 * 3`) are found by [`integer_factors`] (trial division, then
//!   Pollard's rho). The canonical form cannot hold a product of prime powers, so the printed form
//!   ([`super::shown_expr`] with `factored`) shows them; the canonical value stays `12`.
//!
//! Every returned value equals its input (see the property tests in `tests/factor.rs`).

use std::collections::BTreeMap;

use num_traits::{Signed, Zero};

use super::budget;
use super::num::{self, one, zero, Q};
use super::poly::Poly;
use super::poly_solve::{deflate, find_rational_root};
use super::sym::Sym;
use super::SymError;

/// Factors a canonical value over Q (see the module docs). The value is unchanged.
pub fn factor(s: &Sym) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    factor_rec(s)
}

pub(super) fn factor_rec(s: &Sym) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            let a = args.iter().map(factor_rec).collect::<Result<Vec<_>, _>>()?;
            Sym::func(name, a)
        }
        Sym::Pow(b, e) => Sym::pow(factor_rec(b)?, e.clone()),
        Sym::Mul(fs) => {
            let parts = fs.iter().map(factor_rec).collect::<Result<Vec<_>, _>>()?;
            Sym::product(parts)
        }
        Sym::Add(ts) => factor_sum(s, ts),
    }
}

fn factor_sum(s: &Sym, ts: &[Sym]) -> Result<Sym, SymError> {
    if let Some(out) = pull_monomial(ts)? {
        return Ok(out);
    }
    let poly = Poly::from_sym(s)?;
    if let Some(v) = univariate_atom(&poly) {
        return factor_univariate(s, &poly, &v);
    }
    if let Some(out) = pattern(ts)? {
        return Ok(out);
    }
    if poly.terms.keys().all(|m| m.values().sum::<u32>() <= 1) {
        return Ok(s.clone());
    }
    if let Some(out) = super::factor_multi::factor_multivariate(s, &poly)? {
        return Ok(out);
    }
    Err(SymError::Unsupported(
        "factor: multivariate sum without a known factoring pattern".into(),
    ))
}

/// A term as its rational coefficient and the powers of its atoms, or `None` for other shapes.
fn monomial_parts(t: &Sym) -> Option<(Q, BTreeMap<Sym, Q>)> {
    let mut coef = one();
    let mut exps: BTreeMap<Sym, Q> = BTreeMap::new();
    let factors: Vec<&Sym> = match t {
        Sym::Mul(fs) => fs.iter().collect(),
        other => vec![other],
    };
    for f in factors {
        match f {
            Sym::Num(q) => coef *= q.clone(),
            Sym::Symbol(_) | Sym::Unit(_) => add_exp(&mut exps, f.clone(), one()),
            Sym::Pow(b, e) if matches!(**b, Sym::Symbol(_) | Sym::Unit(_)) => {
                add_exp(&mut exps, (**b).clone(), e.clone());
            }
            _ => return None,
        }
    }
    Some((coef, exps))
}

fn add_exp(exps: &mut BTreeMap<Sym, Q>, atom: Sym, e: Q) {
    let slot = exps.entry(atom).or_insert_with(zero);
    *slot = slot.clone() + e;
}

/// Content and common monomial: `2x^2 y + 4 x y^2 = 2 x y (x + 2 y)`.
fn pull_monomial(ts: &[Sym]) -> Result<Option<Sym>, SymError> {
    let mut parts = Vec::with_capacity(ts.len());
    for t in ts {
        match monomial_parts(t) {
            Some(p) => parts.push(p),
            None => return Ok(None),
        }
    }
    let Some((_, first_exps)) = parts.first() else {
        return Ok(None);
    };
    // Integer content: the gcd of the absolute coefficients, negative when every term is.
    let mut g = if parts.iter().all(|(c, _)| c.is_integer()) {
        parts
            .iter()
            .fold(zero(), |acc, (c, _)| num::gcd_int(acc, c.abs()))
    } else {
        one()
    };
    if g.is_zero() {
        return Ok(None);
    }
    if parts.iter().all(|(c, _)| c.is_negative()) {
        g = -g;
    }
    // Atoms present in every term, with the smallest positive exponent.
    let mut common: BTreeMap<Sym, Q> = BTreeMap::new();
    for (atom, e) in first_exps {
        if e.is_positive() {
            common.insert(atom.clone(), e.clone());
        }
    }
    for (_, exps) in &parts[1..] {
        common.retain(|atom, e| match exps.get(atom) {
            Some(f) if f.is_positive() => {
                if *f < *e {
                    *e = f.clone();
                }
                true
            }
            _ => false,
        });
    }
    // A sign alone (`-x - y` is `-1 * (x + y)`) is not content: the sum is left as it is.
    if (g == one() || g == -one()) && common.is_empty() {
        return Ok(None);
    }
    let mut inner_terms = Vec::with_capacity(parts.len());
    for (c, exps) in &parts {
        let mut fs = vec![Sym::Num(c.clone() / g.clone())];
        for (atom, e) in exps {
            let rest = e.clone() - common.get(atom).cloned().unwrap_or_else(zero);
            if !rest.is_zero() {
                fs.push(Sym::pow(atom.clone(), rest)?);
            }
        }
        inner_terms.push(Sym::product(fs)?);
    }
    let inner = factor_rec(&Sym::sum(inner_terms)?)?;
    // With no monomial left, `g * (sum)` would be rebuilt as `g*t1 + g*t2 + ...`: refused.
    if common.is_empty() && matches!(inner, Sym::Add(_)) {
        return Err(SymError::Unsupported(
            "factor: integer content of a sum cannot be kept (the canonical form distributes it)"
                .into(),
        ));
    }
    let mut out = vec![Sym::Num(g)];
    for (atom, e) in &common {
        out.push(Sym::pow(atom.clone(), e.clone())?);
    }
    out.push(inner);
    Ok(Some(Sym::product(out)?))
}

/// The one symbol the polynomial depends on, when it has exactly one (no other atoms).
fn univariate_atom(poly: &Poly) -> Option<Sym> {
    let mut found: Option<Sym> = None;
    for mono in poly.terms.keys() {
        for (atom, e) in mono {
            if *e == 0 {
                continue;
            }
            match &found {
                None => found = Some(atom.clone()),
                Some(a) if a == atom => {}
                Some(_) => return None,
            }
        }
    }
    match found {
        Some(a @ Sym::Symbol(_)) => Some(a),
        _ => None,
    }
}

/// The value of a polynomial with no symbol, if it is one.
fn constant_of(p: &Poly) -> Option<Q> {
    match p.terms.len() {
        0 => Some(zero()),
        1 => {
            let (mono, c) = p.terms.iter().next()?;
            if mono.is_empty() {
                Some(c.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Scales rational coefficients to a primitive integer vector: `c = content * prim`, with the
/// leading coefficient of `prim` positive or the sign carried by `content`.
fn primitive(c: &[Q]) -> Result<(Q, Vec<Q>), SymError> {
    budget::spend(c.len() as u64)?;
    let mut l = one();
    for q in c {
        l = num::lcm_int(&l, &Q::from_integer(q.denom().clone()));
        num::check_bits(&l)?;
    }
    let ints: Vec<Q> = c.iter().map(|q| q.clone() * l.clone()).collect();
    let g0 = ints
        .iter()
        .fold(zero(), |acc, q| num::gcd_int(acc, q.abs()));
    if g0.is_zero() {
        return Err(SymError::Undefined("zero polynomial"));
    }
    let leading_negative = ints
        .iter()
        .rev()
        .find(|q| !q.is_zero())
        .is_some_and(|q| q.is_negative());
    let g = if leading_negative { -g0 } else { g0 };
    let prim: Vec<Q> = ints.iter().map(|q| q.clone() / g.clone()).collect();
    Ok((g / l, prim))
}

fn factor_univariate(s: &Sym, poly: &Poly, v: &Sym) -> Result<Sym, SymError> {
    let parts = poly.coeffs_in(v)?;
    let mut c: Vec<Q> = Vec::with_capacity(parts.len());
    for p in &parts {
        c.push(constant_of(p).ok_or_else(|| {
            SymError::Unsupported("factor: coefficient is not a rational number".into())
        })?);
    }
    while c.len() > 1 && c.last().is_some_and(|q| q.is_zero()) {
        c.pop();
    }
    if c.len() <= 2 {
        // Degree at most one, with no content (pulled earlier): irreducible.
        return Ok(s.clone());
    }
    let (mut k, mut prim) = primitive(&c)?;
    let mut factors: Vec<Sym> = Vec::new();
    while prim.len() > 1 && prim[0].is_zero() {
        prim.remove(0);
        factors.push(v.clone());
    }
    while prim.len() > 1 {
        let Some(r) = find_rational_root(&prim)? else {
            break;
        };
        let quotient = deflate(&prim, &r)?;
        let (content, next) = primitive(&quotient)?;
        k *= content;
        prim = next;
        factors.push(Sym::sum(vec![v.clone(), Sym::Num(-r)])?);
    }
    match prim.len() {
        0 => return Err(SymError::Undefined("zero polynomial")),
        1 => k *= prim[0].clone(),
        n if n > 4 => {
            return Err(SymError::Unsupported(
                "factor: degree 4 or more without a rational root".into(),
            ))
        }
        _ => {
            let mut terms = Vec::with_capacity(prim.len());
            for (i, cf) in prim.iter().enumerate() {
                let power = Sym::pow(v.clone(), num::int(i as i64))?;
                terms.push(Sym::product(vec![Sym::Num(cf.clone()), power])?);
            }
            factors.push(Sym::sum(terms)?);
        }
    }
    factors.insert(0, Sym::Num(k));
    Sym::product(factors)
}

/// Trial division stops at this divisor; a larger cofactor is split by Pollard's rho.
const TRIAL_LIMIT: u64 = 1 << 20;
/// Pollard's rho gives up after this many steps (refused as `Unsupported`, never a wrong answer).
const RHO_STEP_LIMIT: u64 = 1 << 22;
/// Witnesses for the deterministic Miller-Rabin test (correct for every n below 3.3 * 10^24).
const WITNESSES: [u64; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];

/// Prime factorisation of an integer below 2^64: `(prime, exponent)` in increasing prime order,
/// empty for 0 and 1. Trial division up to 2^20, then Pollard's rho on what is left. Charged to
/// the work budget.
pub(crate) fn integer_factors(n: u64) -> Result<Vec<(u64, u32)>, SymError> {
    let mut primes: Vec<u64> = Vec::new();
    if n >= 2 {
        let mut m = n;
        let mut d: u64 = 2;
        let mut steps: u64 = 0;
        while m > 1 && d <= TRIAL_LIMIT && d * d <= m {
            steps += 1;
            if steps.is_multiple_of(64) {
                budget::spend(1)?;
            }
            while m.is_multiple_of(d) {
                primes.push(d);
                m /= d;
            }
            d += if d == 2 { 1 } else { 2 };
        }
        let mut rho_steps = 0;
        split(m, &mut primes, &mut rho_steps)?;
    }
    primes.sort_unstable();
    let mut out: Vec<(u64, u32)> = Vec::new();
    for p in primes {
        match out.last_mut() {
            Some((q, e)) if *q == p => *e += 1,
            _ => out.push((p, 1)),
        }
    }
    Ok(out)
}

fn split(m: u64, primes: &mut Vec<u64>, steps: &mut u64) -> Result<(), SymError> {
    if m <= 1 {
        return Ok(());
    }
    if is_prime(m) {
        primes.push(m);
        return Ok(());
    }
    let d = rho(m, steps)?;
    split(d, primes, steps)?;
    split(m / d, primes, steps)
}

/// A nontrivial divisor of the composite `n` (Floyd cycle detection, `x^2 + c` with `c` growing).
fn rho(n: u64, steps: &mut u64) -> Result<u64, SymError> {
    let mut c: u64 = 1;
    loop {
        let (mut x, mut y, mut d) = (2u64, 2u64, 1u64);
        while d == 1 {
            x = rho_step(x, c, n);
            y = rho_step(rho_step(y, c, n), c, n);
            d = gcd_u64(x.abs_diff(y), n);
            *steps += 1;
            if steps.is_multiple_of(256) {
                budget::spend(1)?;
            }
            if *steps > RHO_STEP_LIMIT {
                return Err(SymError::Unsupported(
                    "factor: integer too hard to factor".into(),
                ));
            }
        }
        if d != n {
            return Ok(d);
        }
        c += 1;
    }
}

fn rho_step(x: u64, c: u64, n: u64) -> u64 {
    ((u128::from(x) * u128::from(x) + u128::from(c)) % u128::from(n)) as u64
}

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) % u128::from(m)) as u64
}

fn pow_mod(mut base: u64, mut e: u64, m: u64) -> u64 {
    let mut acc = 1 % m;
    base %= m;
    while e > 0 {
        if e & 1 == 1 {
            acc = mul_mod(acc, base, m);
        }
        base = mul_mod(base, base, m);
        e >>= 1;
    }
    acc
}

/// Deterministic Miller-Rabin for u64 (the witnesses in [`WITNESSES`]).
fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    for p in WITNESSES {
        if n.is_multiple_of(p) {
            return n == p;
        }
    }
    let mut d = n - 1;
    let mut s = 0;
    while d.is_multiple_of(2) {
        d /= 2;
        s += 1;
    }
    'witness: for a in WITNESSES {
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 1..s {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'witness;
            }
        }
        return false;
    }
    true
}

/// Square root of a term when it is exact: rational squares, even integer powers and products
/// of those. Polynomial square roots only (`(a^2)^(1/2)` is `a` here, as a polynomial identity).
fn sqrt_exact(s: &Sym) -> Option<Sym> {
    match s {
        Sym::Num(q) if !q.is_negative() => num::exact_root(q, 2).map(Sym::Num),
        Sym::Pow(b, e) if e.is_integer() && (e.clone() / num::int(2)).is_integer() => {
            Sym::pow((**b).clone(), e.clone() / num::int(2)).ok()
        }
        Sym::Mul(fs) => {
            let parts = fs.iter().map(sqrt_exact).collect::<Option<Vec<_>>>()?;
            Sym::product(parts).ok()
        }
        _ => None,
    }
}

fn pattern(ts: &[Sym]) -> Result<Option<Sym>, SymError> {
    match ts.len() {
        2 => {
            for (pos, neg) in [(0usize, 1usize), (1, 0)] {
                let minus = Sym::negate(ts[neg].clone())?;
                if let (Some(p), Some(q)) = (sqrt_exact(&ts[pos]), sqrt_exact(&minus)) {
                    let low = Sym::sum(vec![
                        p.clone(),
                        Sym::product(vec![Sym::int(-1), q.clone()])?,
                    ])?;
                    let high = Sym::sum(vec![p, q])?;
                    let out = Sym::product(vec![factor_rec(&low)?, factor_rec(&high)?])?;
                    return Ok(Some(out));
                }
            }
            Ok(None)
        }
        3 => {
            for mid in 0..3 {
                let others: Vec<usize> = (0..3).filter(|&i| i != mid).collect();
                let (i, j) = (others[0], others[1]);
                let (Some(p), Some(q)) = (sqrt_exact(&ts[i]), sqrt_exact(&ts[j])) else {
                    continue;
                };
                let cross = Sym::product(vec![Sym::int(2), p.clone(), q.clone()])?;
                let sign = if ts[mid] == cross {
                    1
                } else if ts[mid] == Sym::product(vec![Sym::int(-2), p.clone(), q.clone()])? {
                    -1
                } else {
                    continue;
                };
                let base = Sym::sum(vec![p, Sym::product(vec![Sym::int(sign), q])?])?;
                return Ok(Some(Sym::pow(factor_rec(&base)?, num::int(2))?));
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

//! Solving one polynomial equation in one unknown, exactly.
//!
//! - Degree 1: `x = -c0 / c1` (coefficients may be symbolic).
//! - Degree 2, numeric: exact roots, with surds in the form `k * m^(1/2)`
//!   (`x^2-2` gives `-2^(1/2)` and `2^(1/2)`). Symbolic coefficients give `(-b +- sqrt(D))/(2a)`
//!   with `D = b^2 - 4ac` (a nonzero assumed).
//! - Degree 3 and more, numeric: rational roots only (rational root theorem and synthetic
//!   division). A quadratic remainder is solved exactly; any other remainder is `Unsupported`.
//!
//! Root order is the order the search finds them, not sorted by value (see [`unique_in_order`]):
//! zero roots, then the rational roots in search order, then the quadratic formula's `+` root
//! before its `-` root. Duplicates are dropped. The help confirms this order:
//! `x^3+11x^2+3x-135` gives `3, -5, -9` and `x^2+x-1` gives `(sqrt(5)-1)/2, (-sqrt(5)-1)/2`.

use num_traits::{Signed, ToPrimitive, Zero};

use super::linear::Equation;
use super::num::{self, int, one, zero, Q};
use super::poly::Poly;
use super::sym::Sym;
use super::{SymError, MAX_DEGREE};

/// Solves `lhs = rhs` for `unknown`. Returns the roots; an empty list means no solution.
pub fn solve_polynomial(eq: &Equation, unknown: &str) -> Result<Vec<Sym>, SymError> {
    solve_polynomial_with_conditions(eq, unknown).map(|(roots, _)| roots)
}

/// Like [`solve_polynomial`], and also returns the conditions the generic roots assume: the
/// symbolic leading coefficient is nonzero (empty for numeric coefficients).
pub fn solve_polynomial_with_conditions(
    eq: &Equation,
    unknown: &str,
) -> Result<(Vec<Sym>, Vec<Sym>), SymError> {
    solve_impl(eq, unknown, false)
}

/// Like [`solve_polynomial_with_conditions`], and complex roots of numeric polynomials are
/// returned too, as `re + im*i` with the symbol `i` (the worksheet's imaginary unit). Only the
/// `solve` command uses this; the callers that need real roots (eigenvalues, transcendental
/// substitution) keep the real-only form.
pub fn solve_polynomial_complex(
    eq: &Equation,
    unknown: &str,
) -> Result<(Vec<Sym>, Vec<Sym>), SymError> {
    solve_impl(eq, unknown, true)
}

fn solve_impl(
    eq: &Equation,
    unknown: &str,
    complex: bool,
) -> Result<(Vec<Sym>, Vec<Sym>), SymError> {
    let _scope = super::budget::Scope::enter();
    let diff = eq.lhs.clone().minus(eq.rhs.clone())?;
    let poly = Poly::from_sym(&diff)?;
    let x = Sym::symbol(unknown);
    if poly.terms.keys().any(|m| {
        m.iter()
            .any(|(a, _)| a != &x && a.mentions_any(&[unknown.to_string()]))
    }) {
        return Err(SymError::Unsupported(
            "unknown inside a function or fractional power".to_string(),
        ));
    }
    let coeffs = poly.coeffs_in(&x)?;
    let deg = match coeffs.iter().rposition(|p| !p.is_zero()) {
        Some(d) => d,
        None => {
            return Err(SymError::Unsupported(
                "identity: every value is a root".into(),
            ))
        }
    };
    if deg as u64 > u64::from(MAX_DEGREE) {
        return Err(SymError::TooLarge("degree"));
    }
    let syms: Vec<Sym> = coeffs[..=deg]
        .iter()
        .map(|p| p.to_sym())
        .collect::<Result<_, _>>()?;
    let lead_condition = if syms[deg].as_num().is_none() {
        vec![syms[deg].clone()]
    } else {
        Vec::new()
    };
    match deg {
        0 => match &syms[0] {
            Sym::Num(_) => Ok((Vec::new(), Vec::new())),
            _ => Err(SymError::Unsupported("symbolic constant equation".into())),
        },
        1 => {
            let root = Sym::product(vec![
                Sym::int(-1),
                syms[0].clone(),
                Sym::pow(syms[1].clone(), -one())?,
            ])?;
            Ok((vec![root], lead_condition))
        }
        2 => {
            if let Some(q) = all_numeric(&syms) {
                let roots = quadratic_numeric(&q[2], &q[1], &q[0], complex)?;
                Ok((unique_in_order(roots), Vec::new()))
            } else {
                Ok((
                    quadratic_symbolic(&syms[2], &syms[1], &syms[0])?,
                    lead_condition,
                ))
            }
        }
        _ => match all_numeric(&syms) {
            Some(q) => {
                let roots = unique_in_order(rational_roots(&q, complex)?);
                check_roots(&q, &roots)?;
                Ok((roots, Vec::new()))
            }
            None => Err(SymError::Unsupported(
                "symbolic polynomial of degree 3 or more".into(),
            )),
        },
    }
}

/// The coefficients as rationals, when every one is a number.
fn all_numeric(syms: &[Sym]) -> Option<Vec<Q>> {
    syms.iter().map(|s| s.as_num().cloned()).collect()
}

/// Exact roots of `a x^2 + b x + c` for rational coefficients (`a` nonzero).
fn quadratic_numeric(a: &Q, b: &Q, c: &Q, complex: bool) -> Result<Vec<Sym>, SymError> {
    let mut disc = b.clone();
    num::mul_into(&mut disc, b)?;
    num::submul_into(&mut disc, &(int(4) * a.clone()), c)?;
    if disc.is_negative() {
        if !complex {
            return Err(SymError::Unsupported("complex roots".into()));
        }
        return complex_pair(a, b, &disc);
    }
    let neg_b = Sym::Num(-b.clone());
    let denom = Sym::Num(int(2) * a.clone());
    if disc.is_zero() {
        return Ok(vec![Sym::product(vec![neg_b, Sym::pow(denom, -one())?])?]);
    }
    let (outside, inside) = num::sqrt_split(&disc);
    let root_part = if inside == one() {
        Sym::Num(outside)
    } else {
        Sym::product(vec![
            Sym::Num(outside),
            Sym::pow(Sym::Num(inside), int(1) / int(2))?,
        ])?
    };
    let plus = Sym::product(vec![
        Sym::sum(vec![neg_b.clone(), root_part.clone()])?,
        Sym::pow(denom.clone(), -one())?,
    ])?;
    let minus = Sym::product(vec![
        Sym::sum(vec![neg_b, Sym::product(vec![Sym::int(-1), root_part])?])?,
        Sym::pow(denom, -one())?,
    ])?;
    Ok(vec![plus, minus])
}

/// Roots of `a x^2 + b x + c` with symbolic coefficients (generic, `a` nonzero).
fn quadratic_symbolic(a: &Sym, b: &Sym, c: &Sym) -> Result<Vec<Sym>, SymError> {
    let four_ac = Sym::product(vec![Sym::int(4), a.clone(), c.clone()])?;
    let disc = Sym::pow(b.clone(), int(2))?.minus(four_ac)?;
    let disc = super::expand(&disc)?;
    let two_a = Sym::product(vec![Sym::int(2), a.clone()])?;
    let neg_b = Sym::product(vec![Sym::int(-1), b.clone()])?;
    if disc.is_zero() {
        return Ok(vec![Sym::product(vec![neg_b, Sym::pow(two_a, -one())?])?]);
    }
    let root = Sym::pow(disc, int(1) / int(2))?;
    let plus = Sym::product(vec![
        Sym::sum(vec![neg_b.clone(), root.clone()])?,
        Sym::pow(two_a.clone(), -one())?,
    ])?;
    let minus = Sym::product(vec![
        Sym::sum(vec![neg_b, Sym::product(vec![Sym::int(-1), root])?])?,
        Sym::pow(two_a, -one())?,
    ])?;
    Ok(vec![plus, minus])
}

/// Rational roots of an integer-free rational polynomial `c[0] + c[1] x + ...`, plus exact
/// roots of a quadratic remainder. Other remainders are unsupported.
fn rational_roots(coeffs: &[Q], complex: bool) -> Result<Vec<Sym>, SymError> {
    let mut c: Vec<Q> = coeffs.to_vec();
    let mut roots: Vec<Sym> = Vec::new();
    // Clear denominators: multiply by the lcm of the denominators.
    let mut l = one();
    for q in &c {
        l = num::lcm_int(&l, &Q::from_integer(q.denom().clone()));
        num::check_bits(&l)?;
    }
    for q in c.iter_mut() {
        num::mul_into(q, &l)?;
    }
    // Remove the content: divide by the gcd of the integer coefficients, so a scaled
    // polynomial (`2520*x^3-...`) gets the same candidate search as its primitive form.
    let g = c.iter().fold(zero(), |acc, q| num::gcd_int(acc, q.clone()));
    if g > one() {
        for q in c.iter_mut() {
            *q = q.clone() / g.clone();
        }
    }
    // Zero root(s): the constant term is zero.
    while c.len() > 1 && c[0].is_zero() {
        roots.push(Sym::int(0));
        c.remove(0);
    }
    while c.len() > 3 {
        match find_rational_root(&c)? {
            Some(r) => {
                c = deflate(&c, &r)?;
                roots.push(Sym::Num(r));
            }
            None => break,
        }
    }
    match c.len() {
        0 | 1 => {}
        2 => roots.push(Sym::Num(-c[0].clone() / c[1].clone())),
        3 => roots.extend(quadratic_numeric(&c[2], &c[1], &c[0], complex)?),
        _ => roots.extend(binomial_or_biquadratic(&c, complex)?),
    }
    Ok(roots)
}

/// The imaginary unit symbol, as the `rectangular` keyword writes it.
fn imaginary() -> Sym {
    Sym::symbol("i")
}

/// `re + coef*i` for the exact parts; `coef` may be negative.
fn complex_sym(re: Sym, coef: Sym) -> Result<Sym, SymError> {
    Sym::sum(vec![re, Sym::product(vec![coef, imaginary()])?])
}

/// The two roots `(-b +- i sqrt(-disc)) / (2a)` of a quadratic with a negative discriminant,
/// the `+` root first.
fn complex_pair(a: &Q, b: &Q, disc: &Q) -> Result<Vec<Sym>, SymError> {
    let two_a = int(2) * a.clone();
    let re = Sym::Num(-b.clone() / two_a.clone());
    let (outside, inside) = num::sqrt_split(&-disc.clone());
    let scale = outside / two_a.abs();
    let magnitude = if inside == one() {
        Sym::Num(scale)
    } else {
        Sym::product(vec![
            Sym::Num(scale),
            Sym::pow(Sym::Num(inside), int(1) / int(2))?,
        ])?
    };
    Ok(vec![
        complex_sym(re.clone(), magnitude.clone())?,
        complex_sym(re, Sym::product(vec![Sym::int(-1), magnitude])?)?,
    ])
}

/// Roots of an integer remainder of degree 3 or 4 that has no rational root, when it is a
/// binomial `a x^3 + c` (the real cube root, then the complex pair) or a biquadratic
/// `a x^4 + b x^2 + c` (square roots of the roots of the quadratic in `x^2`). Complex roots need
/// `complex`. Every other cubic or quartic is refused: no exact form is produced that the
/// engine could not verify.
fn binomial_or_biquadratic(c: &[Q], complex: bool) -> Result<Vec<Sym>, SymError> {
    let refuse =
        || SymError::Unsupported("polynomial of degree 3 or more without rational roots".into());
    let zero_at = |k: usize| c[k].is_zero();
    if c.len() == 4 && zero_at(1) && zero_at(2) {
        if !complex {
            return Err(refuse());
        }
        let q = -c[0].clone() / c[3].clone();
        let real = Sym::pow(Sym::Num(q.abs()), int(1) / int(3))?;
        let real = if q.is_negative() {
            Sym::product(vec![Sym::int(-1), real])?
        } else {
            real
        };
        let re = Sym::product(vec![Sym::Num(int(-1) / int(2)), real.clone()])?;
        let im = Sym::product(vec![
            Sym::Num(int(1) / int(2)),
            Sym::pow(Sym::int(3), int(1) / int(2))?,
            real.clone(),
        ])?;
        return Ok(vec![
            real,
            complex_sym(re.clone(), im.clone())?,
            complex_sym(re, Sym::product(vec![Sym::int(-1), im])?)?,
        ]);
    }
    if c.len() == 5 && zero_at(1) && zero_at(3) {
        let mut disc = c[2].clone();
        num::mul_into(&mut disc, &c[2])?;
        num::submul_into(&mut disc, &(int(4) * c[4].clone()), &c[0])?;
        if disc.is_negative() {
            // x^2 would be complex: a nested complex radical, not produced
            return Err(refuse());
        }
        let mut roots = Vec::new();
        for y in quadratic_numeric(&c[4], &c[2], &c[0], false)? {
            let sign = approximate(&y).ok_or_else(refuse)?;
            let positive = if sign < 0.0 {
                if !complex {
                    return Err(refuse());
                }
                Sym::product(vec![Sym::int(-1), y.clone()])?
            } else {
                y.clone()
            };
            let root = match &positive {
                Sym::Num(q) => {
                    let (outside, inside) = num::sqrt_split(q);
                    if inside == one() {
                        Sym::Num(outside)
                    } else {
                        Sym::product(vec![
                            Sym::Num(outside),
                            Sym::pow(Sym::Num(inside), int(1) / int(2))?,
                        ])?
                    }
                }
                other => Sym::pow(other.clone(), int(1) / int(2))?,
            };
            let negated = Sym::product(vec![Sym::int(-1), root.clone()])?;
            if sign < 0.0 {
                roots.push(Sym::product(vec![root, imaginary()])?);
                roots.push(Sym::product(vec![negated, imaginary()])?);
            } else {
                roots.push(root);
                roots.push(negated);
            }
        }
        return Ok(roots);
    }
    Err(refuse())
}

/// Substitutes every root back into the integer-coefficient polynomial `coeffs` (low to high) and
/// refuses the whole answer when one is not a root to 1e-8 of the coefficient scale.
fn check_roots(coeffs: &[Q], roots: &[Sym]) -> Result<(), SymError> {
    use num_complex::Complex64;
    let at_i = |name: &str| (name == "i").then(|| Complex64::new(0.0, 1.0));
    for r in roots {
        let x = super::numcheck::eval(r, &at_i)
            .ok_or_else(|| SymError::Unsupported("root check not evaluable".into()))?;
        let mut acc = Complex64::new(0.0, 0.0);
        let mut scale = 0.0f64;
        for c in coeffs.iter().rev() {
            acc = acc * x + num::to_f64(c);
            scale = scale * x.norm() + num::to_f64(c).abs();
        }
        if !acc.is_finite() || acc.norm() > 1e-8 * (1.0 + scale) {
            return Err(SymError::Unsupported(
                "a root failed the numeric check".into(),
            ));
        }
    }
    Ok(())
}

/// Trial division for divisors stops at this square root: a constant term whose square root
/// exceeds it (about 10^12 and up) is `Unsupported` rather than factored.
const DIVISOR_TRIAL_LIMIT: u64 = 1_000_000;
/// Largest number of candidates `p/q` the rational root search tries for one polynomial.
const ROOT_CANDIDATE_LIMIT: usize = 2_000;

/// Finds one rational root by the rational root theorem; `c` is low-to-high, integer-valued,
/// with nonzero constant term. Refuses (`Unsupported`) when the search is too large, so a
/// hostile constant can never run for minutes.
pub(crate) fn find_rational_root(c: &[Q]) -> Result<Option<Q>, SymError> {
    let a0 = small_magnitude(&c[0])?;
    let an = small_magnitude(&c[c.len() - 1])?;
    let p_divs = divisors(a0)?;
    let q_divs = divisors(an)?;
    if p_divs.len().saturating_mul(q_divs.len()) > ROOT_CANDIDATE_LIMIT {
        return Err(SymError::Unsupported(
            "rational root search too large".into(),
        ));
    }
    for &p in &p_divs {
        for &q in &q_divs {
            for sign in [1i128, -1] {
                super::budget::spend(1)?;
                let cand = num::from_i128(sign * i128::from(p)) / num::from_i128(i128::from(q));
                if eval_low(c, &cand)?.is_zero() {
                    return Ok(Some(cand));
                }
            }
        }
    }
    Ok(None)
}

/// Absolute value of an integer-valued rational as `u64`, or `Unsupported` above 64 bits.
fn small_magnitude(q: &Q) -> Result<u64, SymError> {
    q.numer()
        .abs()
        .to_u64()
        .filter(|&v| v > 0)
        .ok_or_else(|| SymError::Unsupported("coefficients too large for root search".into()))
}

/// Divisors of a positive integer, found by trial division up to its square root. Refuses
/// (`Unsupported`) when the square root exceeds [`DIVISOR_TRIAL_LIMIT`].
fn divisors(n: u64) -> Result<Vec<u64>, SymError> {
    let root = isqrt(n);
    if root > DIVISOR_TRIAL_LIMIT {
        return Err(SymError::Unsupported(
            "constant term too large to factor".into(),
        ));
    }
    let mut small = Vec::new();
    let mut large = Vec::new();
    for d in 1..=root {
        if n.is_multiple_of(d) {
            small.push(d);
            if d * d != n {
                large.push(n / d);
            }
        }
    }
    large.reverse();
    small.extend(large);
    Ok(small)
}

/// Integer square root (floor), exact.
fn isqrt(n: u64) -> u64 {
    let mut r = (n as f64).sqrt() as u64;
    while u128::from(r) * u128::from(r) > u128::from(n) {
        r -= 1;
    }
    while u128::from(r + 1) * u128::from(r + 1) <= u128::from(n) {
        r += 1;
    }
    r
}

/// Horner evaluation. Every intermediate value is bit-checked: a candidate whose evaluation
/// outgrows the coefficient cap is refused (`TooLarge`), never silently accepted or rejected.
fn eval_low(c: &[Q], x: &Q) -> Result<Q, SymError> {
    let mut acc = zero();
    for coef in c.iter().rev() {
        super::budget::spend(1)?;
        num::mul_into(&mut acc, x)?;
        num::add_into(&mut acc, coef)?;
    }
    Ok(acc)
}

/// Divides the polynomial by `(x - r)` (exact, `r` a root).
pub(crate) fn deflate(c: &[Q], r: &Q) -> Result<Vec<Q>, SymError> {
    let n = c.len();
    super::budget::spend(n as u64)?;
    let mut q = vec![zero(); n - 1];
    let mut carry = zero();
    for k in (0..n).rev() {
        num::mul_into(&mut carry, r)?;
        num::add_into(&mut carry, &c[k])?;
        if k > 0 {
            q[k - 1] = carry.clone();
        }
    }
    Ok(q)
}

/// Drops repeated roots, keeping the first occurrence. The order is the search order (see the
/// module docs), not a sort: `x^3+11x^2+3x-135` gives `3, -5, -9` as in the help.
fn unique_in_order(roots: Vec<Sym>) -> Vec<Sym> {
    let mut out: Vec<Sym> = Vec::with_capacity(roots.len());
    for r in roots {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// Approximate `f64` value of an exact value built from numbers, powers, sums and products.
/// `None` for anything else (a symbol, a function, a unit, a complex or non-finite value).
/// Accurate to double precision only; used for `float` of surds and for ordering.
pub fn approximate(s: &Sym) -> Option<f64> {
    let v = match s {
        Sym::Num(q) => Some(num::to_f64(q)),
        Sym::Pow(b, e) => match &**b {
            Sym::Num(bq) => Some(num::to_f64(bq).powf(num::to_f64(e))),
            _ => None,
        },
        Sym::Mul(fs) => fs
            .iter()
            .map(approximate)
            .try_fold(1.0, |acc, v| v.map(|x| acc * x)),
        Sym::Add(ts) => ts
            .iter()
            .map(approximate)
            .try_fold(0.0, |acc, v| v.map(|x| acc + x)),
        _ => None,
    };
    v.filter(|x| x.is_finite())
}

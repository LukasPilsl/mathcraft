//! Taylor series at a finite point (Prime's `series` keyword, help: "Expanding an Expression into
//! a Series").
//!
//! Two methods give the same coefficients `f^(k)(a)/k!`:
//! - Arithmetic (the default): truncated power series in `y = x - a` with exact coefficients.
//!   Sums, products, integer powers, quotients (`1/g` when `g(a)` is provably nonzero), rational
//!   powers `g^r` (with `g(a)` provably positive when `r` is not an integer), and `exp`, `sin`,
//!   `cos`, `sinh`, `cosh`, `ln` of a series are computed by recurrences. Cost is quadratic in the
//!   number of coefficients, not exponential in the derivative order.
//! - Differentiation: the coefficient of `(x-a)^k` by repeated differentiation and exact evaluation
//!   at `a`. Used only for functions the arithmetic does not know (`tan`, `asin`, ...). Every
//!   derivative is charged to the work budget by its size, so a swelling derivative ends in
//!   `TooLarge` instead of running on.
//!
//! A coefficient that does not evaluate to an exact number is refused, so a point where the
//! function is not analytic (a pole, `sqrt(x)` at 0, `ln(x)` at 0) gives `Unsupported`, never a
//! wrong series.
//!
//! Output (as the help shows): the terms from the first nonzero power `n` up to `n + terms - 1`,
//! zero coefficients omitted. The default is 6 terms, the expansion point 0. A polynomial is
//! expanded exactly whatever its degree (its coefficients are all known); any other function
//! needs `n + terms - 1 <= 24`. Prime's `standard` keyword adds the `O(...)` remainder term; it is
//! not implemented here.

use num_traits::{Signed, ToPrimitive, Zero};

use super::budget;
use super::calc::{differentiate, fold_exact};
use super::limit::value_at;
use super::num::{self, Q};
use super::ratfunc::simplify;
use super::sym::Sym;
use super::SymError;

/// Largest number of terms of one series (the help's example uses 12).
pub const MAX_SERIES_TERMS: usize = 12;
/// Largest derivative order (and power of `y`) taken for a non-polynomial series.
const MAX_ORDER: usize = 24;
/// Coefficients computed for a non-polynomial: orders `0..=MAX_ORDER`.
const PREC: usize = MAX_ORDER + 1;
/// Largest polynomial degree expanded exactly.
const MAX_POLY_DEGREE: usize = 1_000;

/// Which method computes the coefficients (both give the same series; see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    /// Power-series arithmetic, differentiation only for functions it does not know.
    Arithmetic,
    /// Repeated differentiation for every function (the reference method, used by the tests
    /// that compare the two; the public entry point never chooses it).
    #[allow(dead_code)]
    Differentiation,
}

/// The Taylor series of `s` in `var` around `point`, with `terms` terms from the first nonzero
/// power on. Refused (`Unsupported`) when a coefficient is not an exact number within the order
/// cap, so the series would be incomplete.
pub fn series_of(s: &Sym, var: &str, point: &Sym, terms: usize) -> Result<Sym, SymError> {
    series_with(s, var, point, terms, Method::Arithmetic)
}

/// [`series_of`] with the method chosen by the caller (tests compare the two methods).
pub(crate) fn series_with(
    s: &Sym,
    var: &str,
    point: &Sym,
    terms: usize,
    method: Method,
) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    if terms == 0 || terms > MAX_SERIES_TERMS {
        return Err(SymError::Invalid(format!(
            "a series has from 1 to {MAX_SERIES_TERMS} terms"
        )));
    }
    if point.mentions_any(&[var.to_string()]) {
        return Err(SymError::Invalid(
            "the expansion point uses the variable".into(),
        ));
    }
    let f = simplify(s)?;
    let (coeffs, polynomial) = match (method, poly_degree(&f, var)) {
        (Method::Differentiation, _) => {
            (differentiation_coefficients(&f, var, point, terms)?, false)
        }
        (Method::Arithmetic, Some(d)) => {
            let ctx = Ctx {
                var,
                point,
                prec: d + 1,
            };
            (ctx.stop_as_error(ctx.series(&f))?, true)
        }
        (Method::Arithmetic, None) => match arithmetic_coefficients(&f, var, point, terms) {
            Ok(c) => (c, false),
            Err(Stop::Unknown) => (differentiation_coefficients(&f, var, point, terms)?, false),
            Err(Stop::Error(e)) => return Err(e),
        },
    };
    assemble(&coeffs, var, point, terms, polynomial)
}

/// Coefficients of a non-polynomial by arithmetic, at the least precision whose window fits:
/// starts at `terms + 2` coefficients and doubles up to the order cap. A coefficient depends
/// only on lower ones, so a short run gives the same values as a long one.
fn arithmetic_coefficients(f: &Sym, var: &str, point: &Sym, terms: usize) -> Result<Ser, Stop> {
    let mut prec = (terms + 2).min(PREC);
    loop {
        let ctx = Ctx { var, point, prec };
        let coeffs = ctx.series(f)?;
        if prec == PREC || window_fits(&coeffs, terms)? {
            return Ok(coeffs);
        }
        prec = (2 * prec).min(PREC);
    }
}

/// Whether the window of `terms` terms from the first nonzero coefficient lies inside `coeffs`.
fn window_fits(coeffs: &[Sym], terms: usize) -> Result<bool, SymError> {
    for (k, c) in coeffs.iter().enumerate() {
        if nonzero_coefficient(c)? {
            return Ok(k + terms <= coeffs.len());
        }
    }
    Ok(false)
}

/// Sums the coefficients in the window of `terms` terms from the first nonzero one.
fn assemble(
    coeffs: &[Sym],
    var: &str,
    point: &Sym,
    terms: usize,
    polynomial: bool,
) -> Result<Sym, SymError> {
    let mut first: Option<usize> = None;
    let mut found: Vec<(usize, Sym)> = Vec::new();
    for (k, c) in coeffs.iter().enumerate() {
        if let Some(n) = first {
            if k >= n + terms {
                break;
            }
        }
        if nonzero_coefficient(c)? {
            first.get_or_insert(k);
            found.push((k, c.clone()));
        }
    }
    let Some(n) = first else {
        return Err(SymError::Unsupported(
            "series has no nonzero term within the order cap".into(),
        ));
    };
    if !polynomial && n + terms - 1 > MAX_ORDER {
        return Err(SymError::Unsupported(
            "series needs more derivatives than the cap".into(),
        ));
    }
    let base = if point.is_zero() {
        Sym::symbol(var)
    } else {
        Sym::sum(vec![
            Sym::symbol(var),
            Sym::product(vec![Sym::int(-1), point.clone()])?,
        ])?
    };
    let mut parts = Vec::with_capacity(found.len());
    for (k, c) in found {
        let power = Sym::pow(base.clone(), num::int(k as i64))?;
        parts.push(Sym::product(vec![c, power])?);
    }
    Sym::sum(parts)
}

/// Whether a coefficient is nonzero. A symbolic value must have a clearly nonzero double value;
/// one whose sign is unknown is refused, not dropped.
fn nonzero_coefficient(c: &Sym) -> Result<bool, SymError> {
    match c {
        Sym::Num(q) => Ok(!q.is_zero()),
        other => match numeric_value(other) {
            Some(x) if x.abs() > 1e-9 => Ok(true),
            _ => Err(SymError::Unsupported(
                "series coefficient is not provably zero or nonzero".into(),
            )),
        },
    }
}

/// `value_at` with an undefined point reported as a refusal of the series.
fn at_point(s: &Sym, var: &str, point: &Sym) -> Result<Sym, SymError> {
    value_at(s, var, point).map_err(|e| match e {
        SymError::Undefined(_) => {
            SymError::Unsupported("series at a point where the function is undefined".into())
        }
        other => other,
    })
}

/// Coefficients `f^(k)(a)/k!` by repeated differentiation, for `k` up to the first nonzero one
/// plus `terms - 1` (or the order cap). Each step is charged to the budget by its size.
fn differentiation_coefficients(
    f: &Sym,
    var: &str,
    point: &Sym,
    terms: usize,
) -> Result<Vec<Sym>, SymError> {
    let mut out = Vec::new();
    let mut current = f.clone();
    let mut factorial = num::one();
    let mut first: Option<usize> = None;
    for k in 0..=MAX_ORDER {
        budget::spend(node_count(&current))?;
        if let Some(n) = first {
            if k >= n + terms {
                break;
            }
        }
        let v = at_point(&current, var, point)?;
        if first.is_none() && nonzero_coefficient(&v)? {
            first = Some(k);
        }
        out.push(Sym::product(vec![v, Sym::Num(factorial.recip())])?);
        if k < MAX_ORDER {
            current = simplify(&differentiate(&current, var)?)?;
            num::mul_into(&mut factorial, &num::int((k + 1) as i64))?;
        }
    }
    Ok(out)
}

/// Number of nodes of an expression (the cost charged for one derivative).
fn node_count(s: &Sym) -> u64 {
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => 1,
        Sym::Func(_, args) => 1 + args.iter().map(node_count).sum::<u64>(),
        Sym::Add(ts) | Sym::Mul(ts) => 1 + ts.iter().map(node_count).sum::<u64>(),
        Sym::Pow(b, _) => 1 + node_count(b),
    }
}

/// The degree of `s` in `var` when it is a polynomial with integer exponents (structural, so
/// `sin(x)` or `x^(1/2)` is never a polynomial). `None` otherwise or above [`MAX_POLY_DEGREE`].
fn poly_degree(s: &Sym, var: &str) -> Option<usize> {
    if !s.mentions_any(&[var.to_string()]) {
        return Some(0);
    }
    let d = match s {
        Sym::Symbol(n) if n == var => 1,
        Sym::Add(ts) => {
            let mut best = 0;
            for t in ts {
                best = best.max(poly_degree(t, var)?);
            }
            best
        }
        Sym::Mul(fs) => {
            let mut total = 0;
            for f in fs {
                total += poly_degree(f, var)?;
            }
            total
        }
        Sym::Pow(b, e) if e.is_integer() && !e.is_negative() => {
            let n = e.to_integer().to_usize()?;
            poly_degree(b, var)?.checked_mul(n)?
        }
        _ => return None,
    };
    (d <= MAX_POLY_DEGREE).then_some(d)
}

/// Why the arithmetic method stopped: an operation it does not know (use differentiation), or a
/// real refusal.
enum Stop {
    Unknown,
    Error(SymError),
}

impl From<SymError> for Stop {
    fn from(e: SymError) -> Stop {
        Stop::Error(e)
    }
}

/// A truncated power series in `y = x - point`: `prec` coefficients, lowest order first.
type Ser = Vec<Sym>;

struct Ctx<'a> {
    var: &'a str,
    point: &'a Sym,
    prec: usize,
}

impl Ctx<'_> {
    fn stop_as_error(&self, r: Result<Ser, Stop>) -> Result<Ser, SymError> {
        match r {
            Ok(c) => Ok(c),
            Err(Stop::Error(e)) => Err(e),
            Err(Stop::Unknown) => Err(SymError::Unsupported(
                "series of a function the expansion does not cover".into(),
            )),
        }
    }

    fn zeros(&self) -> Ser {
        vec![Sym::int(0); self.prec]
    }

    fn constant(&self, c: Sym) -> Ser {
        let mut v = self.zeros();
        v[0] = c;
        v
    }

    fn series(&self, s: &Sym) -> Result<Ser, Stop> {
        if !s.mentions_any(&[self.var.to_string()]) {
            return Ok(self.constant(at_point(s, self.var, self.point)?));
        }
        match s {
            Sym::Symbol(_) => {
                // x = point + y
                let mut v = self.constant(self.point.clone());
                if self.prec > 1 {
                    v[1] = Sym::int(1);
                }
                Ok(v)
            }
            Sym::Add(ts) => {
                let mut acc = self.zeros();
                for t in ts {
                    let c = self.series(t)?;
                    acc = self.add(&acc, &c)?;
                }
                Ok(acc)
            }
            Sym::Mul(fs) => {
                let mut acc = self.constant(Sym::int(1));
                for f in fs {
                    let c = self.series(f)?;
                    acc = self.mul(&acc, &c)?;
                }
                Ok(acc)
            }
            Sym::Pow(b, e) => {
                let base = self.series(b)?;
                self.pow(&base, e)
            }
            Sym::Func(name, args) if args.len() == 1 => {
                let arg = self.series(&args[0])?;
                self.func(name, &arg)
            }
            _ => Err(Stop::Unknown),
        }
    }

    fn add(&self, a: &Ser, b: &Ser) -> Result<Ser, SymError> {
        (0..self.prec)
            .map(|k| simplify_exact(Sym::sum(vec![a[k].clone(), b[k].clone()])?))
            .collect()
    }

    /// Cauchy product, truncated at `prec`.
    fn mul(&self, a: &Ser, b: &Ser) -> Result<Ser, SymError> {
        let mut out = Vec::with_capacity(self.prec);
        for k in 0..self.prec {
            let mut terms = Vec::new();
            for i in 0..=k {
                if a[i].is_zero() || b[k - i].is_zero() {
                    continue;
                }
                budget::spend(1)?;
                terms.push(Sym::product(vec![a[i].clone(), b[k - i].clone()])?);
            }
            out.push(simplify_exact(Sym::sum(terms)?)?);
        }
        Ok(out)
    }

    /// `1 / a` for `a(0)` provably nonzero.
    fn inverse(&self, a: &Ser) -> Result<Ser, Stop> {
        let a0 = &a[0];
        if a0.is_zero() {
            return Err(Stop::Error(SymError::Unsupported(
                "series at a point where the function is undefined".into(),
            )));
        }
        if !provably_nonzero(a0) {
            return Err(Stop::Error(SymError::Unsupported(
                "series coefficient is not provably zero or nonzero".into(),
            )));
        }
        // 1/a = (1/a0) * 1/(a/a0): the recurrence runs on a/a0 (constant term 1), so a surd
        // a0 stays out of every coefficient.
        let inv0 = simplify_exact(Sym::pow(a0.clone(), -num::one())?)?;
        let norm = self.normalized(a, &inv0)?;
        let mut q: Ser = vec![Sym::int(1)];
        for k in 1..self.prec {
            let mut terms = Vec::new();
            for i in 1..=k {
                budget::spend(1)?;
                terms.push(Sym::product(vec![norm[i].clone(), q[k - i].clone()])?);
            }
            let sum = Sym::sum(terms)?;
            q.push(Sym::product(vec![Sym::int(-1), sum])?);
        }
        self.scale(&q, &inv0).map_err(Stop::from)
    }

    /// `a / c` coefficientwise, simplified; with `c = a(0)^(-1)` the constant term is 1.
    fn normalized(&self, a: &Ser, inv_c: &Sym) -> Result<Ser, SymError> {
        a.iter()
            .map(|x| simplify_exact(Sym::product(vec![inv_c.clone(), x.clone()])?))
            .collect()
    }

    fn pow(&self, base: &Ser, e: &Q) -> Result<Ser, Stop> {
        if e.is_zero() {
            return Ok(self.constant(Sym::int(1)));
        }
        if e.is_integer() {
            let n = e
                .to_integer()
                .to_i64()
                .ok_or(SymError::TooLarge("exponent"))?;
            return if n < 0 {
                let inv = self.inverse(base)?;
                self.pow_nonnegative(&inv, n.unsigned_abs())
            } else {
                self.pow_nonnegative(base, n as u64)
            };
        }
        // g^r with r not an integer: needs g(0) provably positive (a real power).
        let a0 = &base[0];
        if a0.is_zero() {
            return Err(Stop::Error(SymError::Unsupported(
                "series of a fractional power at a point where the base vanishes".into(),
            )));
        }
        if !provably_nonzero(a0) || numeric_value(a0).is_none_or(|v| v <= 0.0) {
            return Err(Stop::Error(SymError::Unsupported(
                "fractional power of a base that is not provably positive at the point".into(),
            )));
        }
        // g^r = g0^r * (g/g0)^r, and for n = g/g0 (constant term 1): k p_k = sum_{i=1..k}
        // ((r+1) i - k) n_i p_{k-i}. The surd g0^r stays out of the recurrence.
        let inv_a0 = simplify_exact(Sym::pow(a0.clone(), -num::one())?)?;
        let norm = self.normalized(base, &inv_a0)?;
        let mut p: Ser = vec![Sym::int(1)];
        for k in 1..self.prec {
            let mut terms = Vec::new();
            for i in 1..=k {
                budget::spend(1)?;
                let w = (e.clone() + num::one()) * num::int(i as i64) - num::int(k as i64);
                let coef = w / num::int(k as i64);
                terms.push(Sym::product(vec![
                    Sym::Num(coef),
                    norm[i].clone(),
                    p[k - i].clone(),
                ])?);
            }
            p.push(Sym::sum(terms)?);
        }
        let p0 = simplify_exact(Sym::pow(a0.clone(), e.clone())?)?;
        self.scale(&p, &p0).map_err(Stop::from)
    }

    fn pow_nonnegative(&self, base: &Ser, mut n: u64) -> Result<Ser, Stop> {
        let mut result = self.constant(Sym::int(1));
        let mut b = base.clone();
        while n > 0 {
            if n & 1 == 1 {
                result = self.mul(&result, &b)?;
            }
            n >>= 1;
            if n > 0 {
                b = self.mul(&b, &b)?;
            }
        }
        Ok(result)
    }

    /// `f(g)` for the functions the arithmetic knows; anything else is `Stop::Unknown`.
    fn func(&self, name: &str, arg: &Ser) -> Result<Ser, Stop> {
        let c0 = arg[0].clone();
        // h = g - g(0): zero constant term, so exp(c0 + h) = exp(c0) exp(h) and so on.
        let mut h = arg.clone();
        h[0] = Sym::int(0);
        match name {
            "exp" => {
                // E' = h' E
                let mut e: Ser = self.constant(Sym::int(1));
                for k in 1..self.prec {
                    e[k] = self.weighted(&h, &e, k, num::one() / num::int(k as i64))?;
                }
                let factor = simplify_exact(Sym::func("exp", vec![c0])?)?;
                Ok(self.scale(&e, &factor)?)
            }
            "sin" | "cos" | "sinh" | "cosh" => {
                let sign = match name {
                    // sinh and cosh: C' = S h'
                    "sinh" | "cosh" => num::int(1),
                    // sin and cos: C' = -S h'
                    _ => num::int(-1),
                };
                let (s, c) = self.sin_cos_like(&h, sign)?;
                // f(c0 + h) = A cos_like(h) + B sin_like(h), from the addition formulas.
                let (partner, sgn) = match name {
                    "sin" => ("cos", 1),
                    "cos" => ("sin", -1),
                    "sinh" => ("cosh", 1),
                    _ => ("sinh", 1),
                };
                let a = simplify_exact(Sym::func(name, vec![c0.clone()])?)?;
                let b = simplify_exact(Sym::product(vec![
                    Sym::int(sgn),
                    Sym::func(partner, vec![c0])?,
                ])?)?;
                let mut out = Vec::with_capacity(self.prec);
                for k in 0..self.prec {
                    budget::spend(1)?;
                    out.push(simplify_exact(Sym::sum(vec![
                        Sym::product(vec![a.clone(), c[k].clone()])?,
                        Sym::product(vec![b.clone(), s[k].clone()])?,
                    ])?)?);
                }
                Ok(out)
            }
            "ln" => {
                // ln g = ln c0 + L with L' = g' / g.
                if !numeric_value(&c0).is_some_and(|v| v > 0.0) {
                    return Err(Stop::Error(SymError::Unsupported(
                        "logarithm at a point where the argument is not positive".into(),
                    )));
                }
                let inv = self.inverse(arg)?;
                let mut deriv = self.zeros();
                for i in 0..self.prec.saturating_sub(1) {
                    deriv[i] = simplify_exact(Sym::product(vec![
                        Sym::int(i as i64 + 1),
                        arg[i + 1].clone(),
                    ])?)?;
                }
                let q = self.mul(&deriv, &inv)?;
                let mut out = self.zeros();
                out[0] = simplify_exact(Sym::func("ln", vec![c0])?)?;
                for k in 1..self.prec {
                    out[k] = simplify_exact(Sym::product(vec![
                        Sym::Num(num::one() / num::int(k as i64)),
                        q[k - 1].clone(),
                    ])?)?;
                }
                Ok(out)
            }
            _ => Err(Stop::Unknown),
        }
    }

    /// Sine-like and cosine-like series of `h` (zero constant term) with `S' = C h'` and
    /// `C' = sign * S h'`: `(sin h, cos h)` for `sign = -1`, `(sinh h, cosh h)` for `+1`.
    fn sin_cos_like(&self, h: &Ser, sign: Q) -> Result<(Ser, Ser), Stop> {
        let mut s = self.zeros();
        let mut c = self.constant(Sym::int(1));
        for k in 1..self.prec {
            let inv_k = num::one() / num::int(k as i64);
            s[k] = self.weighted(h, &c, k, inv_k.clone())?;
            c[k] = self.weighted(h, &s, k, inv_k * &sign)?;
        }
        Ok((s, c))
    }

    /// `factor * sum_{i=1..k} i h_i other_{k-i}`, one coefficient of `factor * h' * other`.
    fn weighted(&self, h: &Ser, other: &Ser, k: usize, factor: Q) -> Result<Sym, SymError> {
        let mut terms = Vec::new();
        for i in 1..=k {
            budget::spend(1)?;
            terms.push(Sym::product(vec![
                Sym::Num(&factor * num::int(i as i64)),
                h[i].clone(),
                other[k - i].clone(),
            ])?);
        }
        simplify_exact(Sym::sum(terms)?)
    }

    fn scale(&self, a: &Ser, c: &Sym) -> Result<Ser, SymError> {
        a.iter()
            .map(|x| simplify_exact(Sym::product(vec![c.clone(), x.clone()])?))
            .collect()
    }
}

/// Canonical form of a coefficient: exact folding of elementary values, then simplification.
fn simplify_exact(s: Sym) -> Result<Sym, SymError> {
    simplify(&fold_exact(&s)?)
}

/// Whether a constant is provably nonzero: a nonzero number, or a value with a clearly nonzero
/// double value.
fn provably_nonzero(s: &Sym) -> bool {
    match s {
        Sym::Num(q) => !q.is_zero(),
        other => numeric_value(other).is_some_and(|v| v.abs() > 1e-9),
    }
}

/// The double value of a symbolic constant built from numbers, sums, products, powers and the
/// elementary functions of a number. `None` for a symbol, a unit, or any other function.
fn numeric_value(s: &Sym) -> Option<f64> {
    let v = match s {
        Sym::Num(q) => num::to_f64(q),
        Sym::Add(ts) => ts
            .iter()
            .map(numeric_value)
            .try_fold(0.0, |acc, v| v.map(|x| acc + x))?,
        Sym::Mul(fs) => fs
            .iter()
            .map(numeric_value)
            .try_fold(1.0, |acc, v| v.map(|x| acc * x))?,
        Sym::Pow(b, e) => {
            let base = numeric_value(b)?;
            let exp = num::to_f64(e);
            if base <= 0.0 && !e.is_integer() {
                return None;
            }
            base.powf(exp)
        }
        Sym::Func(name, args) => {
            let [arg] = args.as_slice() else {
                return None;
            };
            let a = numeric_value(arg)?;
            match name.as_str() {
                "sin" => a.sin(),
                "cos" => a.cos(),
                "tan" => a.tan(),
                "exp" => a.exp(),
                "sinh" => a.sinh(),
                "cosh" => a.cosh(),
                "ln" if a > 0.0 => a.ln(),
                _ => return None,
            }
        }
        Sym::Symbol(_) | Sym::Unit(_) => return None,
    };
    v.is_finite().then_some(v)
}

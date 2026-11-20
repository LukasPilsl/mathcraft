//! Symbolic limits (Prime's limit operator, `limit(x, a, f)`). Exact or refused, never guessed.
//!
//! What is answered:
//! - A rational function of the variable with rational coefficients (other symbols and
//!   functions of the variable are refused here). At a finite point the common factors are
//!   cancelled first, then the value is `num(a)/den(a)` when the denominator does not vanish.
//!   At `+infinity` or `-infinity`: 0 when the numerator's degree is below the denominator's,
//!   the ratio of the leading coefficients when they are equal, and refused when it is above.
//! - A quotient `n/d` at a finite point where `n(a) = d(a) = 0` exactly (for example
//!   `sin(x)/x` at 0, `(1-cos x)/x^2` at 0): L'Hopital's rule by differentiation, at most
//!   [`MAX_LHOPITAL`] rounds. Each round needs an exact value at `a` for both sides, and a
//!   nonzero denominator must be provably nonzero.
//!
//! One-sided limits (`Side::Right`, `Side::Left`): for a rational function, the value at a
//! point where the denominator does not vanish (or after cancelling a removable factor) is the
//! same from both sides. A pole is refused (the sign of the infinity is not decided here). For
//! L'Hopital, one-sided limits are answered only when every function in the expression is
//! analytic at the point (`sin`, `cos`, `exp`, `sinh`, `cosh`, integer powers, sums, products),
//! so both sides agree.
//!
//! Refused (`Unsupported`): infinite limits, limits at infinity of anything that is not a rational
//! function, symbolic coefficients in a rational limit, limits whose denominator value is not
//! provably nonzero, and L'Hopital beyond the depth cap.

use num_traits::{Signed, Zero};

use super::budget;
use super::calc::{differentiate, fold_exact};
use super::num::{self, Q};
use super::poly::substitute;
use super::poly_solve::approximate;
use super::ratfunc::RatFunc;
use super::sym::Sym;
use super::SymError;

/// Largest number of L'Hopital rounds for one limit.
pub const MAX_LHOPITAL: usize = 4;

/// Which side of the point the limit is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Two-sided: both sides must agree (the answer is the common value).
    Both,
    /// From above (`x -> a+`).
    Right,
    /// From below (`x -> a-`).
    Left,
}

/// The point the variable approaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Point {
    /// A finite point, an exact value that does not contain the variable.
    Finite(Sym),
    /// `+infinity`.
    PosInfinity,
    /// `-infinity`.
    NegInfinity,
}

/// The limit of `s` as `var` approaches `point` from `side`, exact or refused.
pub fn limit_of(s: &Sym, var: &str, point: &Point, side: Side) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    if let Point::Finite(a) = point {
        if a.mentions_any(&[var.to_string()]) {
            return Err(SymError::Invalid(
                "the limit point uses the limiting variable".into(),
            ));
        }
        if has_complex_value(a) {
            return Err(SymError::Unsupported(
                "limit at a point with a complex value (real limits only)".into(),
            ));
        }
    }
    if !s.mentions_any(&[var.to_string()]) {
        // A constant in the variable: its own value, but only when the point is a number
        // (a real constant or an infinity), so `limit(5, x, a)` is not answered.
        if let Point::Finite(a) = point {
            if !is_number(a) {
                return Err(SymError::Unsupported(
                    "limit of a constant at a point that is not a number".into(),
                ));
            }
        }
        let v = fold_exact(s).and_then(|v| simplify(&v))?;
        if has_complex_value(&v) {
            return Err(SymError::Unsupported(
                "limit with a complex value (real limits only)".into(),
            ));
        }
        return Ok(v);
    }
    match point {
        Point::Finite(a) => {
            if side == Side::Both && undefined_on_one_side(s, var, a) {
                return Err(SymError::Unsupported(
                    "function is undefined on one side of the point".into(),
                ));
            }
            finite_limit(s, var, a, side)
        }
        Point::PosInfinity | Point::NegInfinity => infinite_limit(s, var),
    }
}

/// A finite point written with numbers only: a rational, a surd, `pi` or `e` (no other symbol).
fn is_number(a: &Sym) -> bool {
    super::calc::symbols(a)
        .iter()
        .all(|n| n == "pi" || n == "e")
        && !has_unit(a)
}

fn has_unit(s: &Sym) -> bool {
    match s {
        Sym::Unit(_) => true,
        Sym::Num(_) | Sym::Symbol(_) => false,
        Sym::Func(_, args) => args.iter().any(has_unit),
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().any(has_unit),
        Sym::Pow(b, _) => has_unit(b),
    }
}

/// Whether a constant has a complex value: a negative base with a fractional power, or a
/// logarithm of a nonpositive number. Real limits only: such a value is refused.
pub(crate) fn has_complex_value(s: &Sym) -> bool {
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => false,
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().any(has_complex_value),
        Sym::Pow(b, e) => {
            (!e.is_integer() && approximate(b).is_some_and(|v| v < 0.0)) || has_complex_value(b)
        }
        Sym::Func(name, args) => {
            (name == "ln"
                && args
                    .iter()
                    .any(|a| approximate(a).is_some_and(|v| v <= 0.0)))
                || args.iter().any(has_complex_value)
        }
    }
}

/// Whether the expression (in `var` only) is undefined on one side of the finite point `a`
/// but not the other: `sqrt(x)` at 0. Sampled at two small distances; a body with other
/// symbols is not judged.
fn undefined_on_one_side(s: &Sym, var: &str, a: &Sym) -> bool {
    if super::calc::symbols(s).iter().any(|n| n != var) {
        return false;
    }
    let Some(x0) = approximate(a) else {
        return false;
    };
    [1e-3, 1e-6].iter().any(|h| {
        let left = super::calc::approx_at(s, var, x0 - h);
        let right = super::calc::approx_at(s, var, x0 + h);
        left.is_some() != right.is_some()
    })
}

fn simplify(s: &Sym) -> Result<Sym, SymError> {
    super::ratfunc::simplify(s)
}

fn finite_limit(s: &Sym, var: &str, a: &Sym, side: Side) -> Result<Sym, SymError> {
    let s = simplify(s)?;
    if is_polynomial_ratio(&s, var) {
        return rational_at(&s, var, a);
    }
    if side != Side::Both && !analytic(&s, var) {
        return Err(SymError::Unsupported(
            "one-sided limit of a function that is not analytic at the point".into(),
        ));
    }
    let (n, d) = split_quotient(&s)?;
    lhopital(&n, &d, var, a, 0)
}

/// `num(a)/den(a)` for a rational function, after cancellation. Refused when the denominator
/// vanishes (a pole or an unsure point).
fn rational_at(s: &Sym, var: &str, a: &Sym) -> Result<Sym, SymError> {
    let (num_s, den_s) = rational_parts(s)?;
    if num_s.is_zero() {
        return Ok(Sym::int(0));
    }
    let n_a = simplify(&substitute(&num_s, &[(var, a.clone())])?)?;
    let d_a = simplify(&substitute(&den_s, &[(var, a.clone())])?)?;
    if d_a.is_zero() {
        return Err(SymError::Unsupported(
            "limit is infinite or undecided at the point (denominator vanishes)".into(),
        ));
    }
    simplify(&Sym::product(vec![n_a, Sym::pow(d_a, -num::one())?])?)
}

/// The limit at `+infinity` or `-infinity` of a rational function with numeric coefficients.
fn infinite_limit(s: &Sym, var: &str) -> Result<Sym, SymError> {
    let s = simplify(s)?;
    if !is_polynomial_ratio(&s, var) {
        return Err(SymError::Unsupported(
            "limit at infinity of an expression that is not a rational function".into(),
        ));
    }
    let (num_s, den_s) = rational_parts(&s)?;
    if num_s.is_zero() {
        return Ok(Sym::int(0));
    }
    let cn = numeric_coefficients(&num_s, var)?;
    let cd = numeric_coefficients(&den_s, var)?;
    let (dn, dd) = (cn.len() - 1, cd.len() - 1);
    match dn.cmp(&dd) {
        std::cmp::Ordering::Less => Ok(Sym::int(0)),
        std::cmp::Ordering::Equal => Ok(Sym::Num(&cn[dn] / &cd[dd])),
        std::cmp::Ordering::Greater => Err(SymError::Unsupported(
            "limit at infinity is infinite".into(),
        )),
    }
}

/// Numerator and denominator of a rational function in canonical (cancelled) form.
fn rational_parts(s: &Sym) -> Result<(Sym, Sym), SymError> {
    let rf = RatFunc::from_sym(s)?;
    Ok((rf.num.to_sym()?, rf.den.to_sym()?))
}

/// The coefficients, lowest power first, with no trailing zeros (a zero polynomial is `[0]`).
/// Each coefficient must be a number: a symbolic coefficient is refused.
pub(crate) fn numeric_coefficients(p: &Sym, var: &str) -> Result<Vec<Q>, SymError> {
    let mut out: Vec<Q> = Vec::new();
    for c in super::calc::coefficients(p, var)? {
        match c {
            Sym::Num(q) => out.push(q),
            _ => {
                return Err(SymError::Unsupported(
                    "limit with a symbolic coefficient".into(),
                ))
            }
        }
    }
    while out.len() > 1 && out.last().is_some_and(|q| q.is_zero()) {
        out.pop();
    }
    Ok(out)
}

/// Whether `s` is a polynomial or a quotient of polynomials in `var` with rational coefficients:
/// numbers, the variable, sums, products, and integer powers. Any other symbol, unit or
/// function (even one free of the variable) makes it `false`.
pub(crate) fn is_polynomial_ratio(s: &Sym, var: &str) -> bool {
    match s {
        Sym::Num(_) => true,
        Sym::Symbol(n) => n == var,
        Sym::Unit(_) | Sym::Func(_, _) => false,
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().all(|t| is_polynomial_ratio(t, var)),
        Sym::Pow(b, e) => e.is_integer() && is_polynomial_ratio(b, var),
    }
}

/// Whether every function in `s` is analytic at every point where it is finite: integer powers,
/// sums, products, and `sin`, `cos`, `exp`, `sinh`, `cosh` of an analytic argument. A non-integer
/// power of an expression with the variable is not analytic at its zero; other functions are
/// refused. Symbols other than the variable are constants.
fn analytic(s: &Sym, var: &str) -> bool {
    match s {
        Sym::Num(_) | Sym::Symbol(_) => true,
        Sym::Unit(_) => false,
        Sym::Func(name, args) => {
            matches!(name.as_str(), "sin" | "cos" | "exp" | "sinh" | "cosh")
                && args.iter().all(|a| analytic(a, var))
        }
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().all(|t| analytic(t, var)),
        Sym::Pow(b, e) => {
            if e.is_integer() {
                analytic(b, var)
            } else {
                !b.mentions_any(&[var.to_string()]) && analytic(b, var)
            }
        }
    }
}

/// Splits a product into its numerator and denominator factors (`x^-2` goes to the denominator).
fn split_quotient(s: &Sym) -> Result<(Sym, Sym), SymError> {
    let factors: Vec<Sym> = match s {
        Sym::Mul(fs) => fs.clone(),
        other => vec![other.clone()],
    };
    let mut num_f = Vec::new();
    let mut den_f = Vec::new();
    for f in factors {
        match f {
            Sym::Pow(b, e) if e.is_negative() => {
                den_f.push(Sym::pow(*b, -e)?);
            }
            other => num_f.push(other),
        }
    }
    let n = Sym::product(num_f)?;
    let d = Sym::product(den_f)?;
    Ok((n, d))
}

/// `value of s at a` with exact folding: `substitute`, `fold_exact`, `simplify`.
pub(crate) fn value_at(s: &Sym, var: &str, a: &Sym) -> Result<Sym, SymError> {
    simplify(&fold_exact(&substitute(s, &[(var, a.clone())])?)?)
}

/// The limit of `n/d` at `a` by L'Hopital's rule, with `depth` rounds already used.
fn lhopital(n: &Sym, d: &Sym, var: &str, a: &Sym, depth: usize) -> Result<Sym, SymError> {
    budget::spend(1)?;
    let na = value_at(n, var, a)?;
    let da = value_at(d, var, a)?;
    if da.is_zero() {
        if !na.is_zero() {
            return Err(SymError::Unsupported(
                "limit is infinite or undecided (zero denominator)".into(),
            ));
        }
        if depth >= MAX_LHOPITAL {
            return Err(SymError::Unsupported(
                "L'Hopital rule needs more rounds than the cap".into(),
            ));
        }
        let n2 = simplify(&differentiate(n, var)?)?;
        let d2 = simplify(&differentiate(d, var)?)?;
        return lhopital(&n2, &d2, var, a, depth + 1);
    }
    if !provably_nonzero(&da) {
        return Err(SymError::Unsupported(
            "denominator value at the point is not provably nonzero".into(),
        ));
    }
    if !provably_finite(&na) {
        return Err(SymError::Unsupported(
            "numerator value at the point is not provably finite".into(),
        ));
    }
    simplify(&Sym::product(vec![na, Sym::pow(da, -num::one())?])?)
}

/// Exact nonzero numbers, and surds or other values with a clearly nonzero double value.
fn provably_nonzero(s: &Sym) -> bool {
    match s {
        Sym::Num(q) => !q.is_zero(),
        _ => approximate(s).is_some_and(|v| v.abs() > 1e-9),
    }
}

/// Values made of numbers, with functions only where they are finite at a number. `ln` needs a
/// positive argument; a symbol or an unknown function is not provably finite.
fn provably_finite(s: &Sym) -> bool {
    match s {
        Sym::Num(_) => true,
        Sym::Pow(b, _) => provably_finite(b) && approximate(s).is_some(),
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().all(provably_finite),
        Sym::Func(name, args) => {
            let [arg] = args.as_slice() else {
                return false;
            };
            match name.as_str() {
                "sin" | "cos" | "exp" | "sinh" | "cosh" => provably_finite(arg),
                "ln" => approximate(arg).is_some_and(|v| v > 0.0) && provably_finite(arg),
                _ => false,
            }
        }
        Sym::Symbol(_) | Sym::Unit(_) => false,
    }
}

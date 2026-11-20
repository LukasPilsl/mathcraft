//! The printed form of a canonical value: the expression Mathcad shows after `->`.
//!
//! Sums print with `+` and `-`, in canonical order. A product puts its negative powers under a
//! fraction bar, a rational coefficient is a fraction, `1/2` powers are roots and other rational
//! powers are superscripts (`7^(1/5)`). A sum used as a factor is parenthesized.
//!
//! With `factored`, an integer that is a product of primes is shown as its prime powers
//! (`12` as `2^2*3`), which is how `factor` shows integers. A canonical value cannot hold such a
//! product (`2^2` folds to `4`), so only the printed form carries it. Integers inside a sum are
//! terms, never factored.

use num_traits::{Signed, ToPrimitive};

use super::factor::integer_factors;
use super::num::{from_i128, half, one, Q};
use super::sym::Sym;
use super::SymError;
use crate::expr::{BinaryOp, Expr, Id, IdKind, Number, UnaryOp};

/// The printed expression of `s`. Fails only when `factored` must split an integer too large to
/// factor (`SymError::Unsupported`), so an unfactored form is never shown in its place.
pub fn shown_expr(s: &Sym, factored: bool) -> Result<Expr, SymError> {
    show(s, factored)
}

fn show(s: &Sym, factored: bool) -> Result<Expr, SymError> {
    match s {
        Sym::Num(q) => rational(q, factored),
        Sym::Symbol(n) => Ok(Expr::Id(Id::new(n, IdKind::Variable))),
        Sym::Unit(n) => Ok(Expr::Id(Id::new(n, IdKind::Unit))),
        Sym::Func(n, args) if n == "exp" && args.len() == 1 => Ok(Expr::binary(
            BinaryOp::Pow,
            Expr::Id(Id::new("e", IdKind::Constant)),
            show(&args[0], factored)?,
        )),
        Sym::Func(n, args) => {
            let shown = args
                .iter()
                .map(|a| show(a, factored))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Expr::call(n, shown))
        }
        Sym::Add(ts) => sum(ts, factored),
        Sym::Mul(fs) => product(fs, factored),
        Sym::Pow(b, e) => {
            if e.is_negative() {
                let denominator = pow_positive(b, &-e.clone(), factored)?;
                Ok(Expr::binary(BinaryOp::Div, number(one()), denominator))
            } else {
                pow_positive(b, e, factored)
            }
        }
    }
}

fn sum(ts: &[Sym], factored: bool) -> Result<Expr, SymError> {
    let mut acc: Option<Expr> = None;
    for t in ts {
        acc = Some(match (acc.take(), t.negated_term()) {
            (None, _) => show(t, factored)?,
            (Some(a), Some(pos)) => Expr::binary(BinaryOp::Sub, a, show(&pos, factored)?),
            (Some(a), None) => Expr::binary(BinaryOp::Add, a, show(t, factored)?),
        });
    }
    acc.ok_or_else(|| SymError::Invalid("empty sum".into()))
}

/// A product: a rational coefficient, the factors with positive powers, and the factors with
/// negative powers under the fraction bar.
fn product(fs: &[Sym], factored: bool) -> Result<Expr, SymError> {
    let mut numer: Vec<Expr> = Vec::new();
    let mut denom: Vec<Expr> = Vec::new();
    let mut sums: Vec<&Sym> = Vec::new();
    let mut coef: Option<Q> = None;
    for f in fs {
        match f {
            Sym::Num(q) => coef = Some(q.clone()),
            Sym::Pow(b, e) if e.is_negative() => {
                denom.push(pow_positive(b, &-e.clone(), factored)?)
            }
            Sym::Add(_) => sums.push(f),
            other => numer.push(factor_expr(other, factored)?),
        }
    }
    // Sums come after the other factors, the one with the smaller last coefficient first:
    // `(x - 1)(x + 1)` and `(b - a)(b + a)` as Prime shows them.
    sums.sort_by_key(|s| last_coefficient(s));
    for s in sums {
        numer.push(factor_expr(s, factored)?);
    }
    let mut negative = false;
    if let Some(q) = coef {
        negative = q.is_negative();
        let magnitude = q.abs();
        let top = Q::from_integer(magnitude.numer().clone());
        let bottom = Q::from_integer(magnitude.denom().clone());
        if top != one() {
            numer.insert(0, integer(&top, factored)?);
        }
        if bottom != one() {
            denom.insert(0, integer(&bottom, factored)?);
        }
    }
    let top = fold_mul(numer).unwrap_or_else(|| number(one()));
    let body = match fold_mul(denom) {
        Some(bottom) => Expr::binary(BinaryOp::Div, top, bottom),
        None => top,
    };
    Ok(if negative {
        Expr::unary(UnaryOp::Neg, body)
    } else {
        body
    })
}

/// The coefficient of the last term of a sum (its constant term when it has one), the key that
/// orders sum factors in a product. Other shapes have coefficient 1.
fn last_coefficient(s: &Sym) -> Q {
    let Sym::Add(ts) = s else {
        return one();
    };
    match ts.last() {
        Some(Sym::Num(q)) => q.clone(),
        Some(Sym::Mul(fs)) => match fs.first() {
            Some(Sym::Num(q)) => q.clone(),
            _ => one(),
        },
        _ => one(),
    }
}

fn fold_mul(parts: Vec<Expr>) -> Option<Expr> {
    parts
        .into_iter()
        .reduce(|acc, p| Expr::binary(BinaryOp::Mul, acc, p))
}

/// A factor of a product: a sum is parenthesized, anything else prints as it is.
fn factor_expr(f: &Sym, factored: bool) -> Result<Expr, SymError> {
    let e = show(f, factored)?;
    Ok(match f {
        Sym::Add(_) => Expr::Parens(Box::new(e)),
        _ => e,
    })
}

/// The base of a power: a name, a function or a prime integer stays bare.
fn base_expr(b: &Sym, factored: bool) -> Result<Expr, SymError> {
    let e = show(b, factored)?;
    Ok(match b {
        Sym::Symbol(_) | Sym::Unit(_) | Sym::Func(..) => e,
        Sym::Num(q) if q.is_integer() && !q.is_negative() && is_prime(q) => e,
        _ => Expr::Parens(Box::new(e)),
    })
}

/// `b^e` for a positive exponent: a square root when `e` is 1/2, a superscript otherwise.
fn pow_positive(b: &Sym, e: &Q, factored: bool) -> Result<Expr, SymError> {
    if *e == one() {
        return base_expr(b, factored);
    }
    if *e == half() {
        return Ok(Expr::sqrt(show(b, factored)?));
    }
    Ok(Expr::binary(
        BinaryOp::Pow,
        base_expr(b, factored)?,
        exponent(e),
    ))
}

fn exponent(e: &Q) -> Expr {
    if e.is_integer() {
        number(e.clone())
    } else {
        Expr::binary(
            BinaryOp::Div,
            number(Q::from_integer(e.numer().clone())),
            number(Q::from_integer(e.denom().clone())),
        )
    }
}

/// A rational, as a fraction when it is not an integer. A negative value is negated.
fn rational(q: &Q, factored: bool) -> Result<Expr, SymError> {
    let magnitude = q.abs();
    let top = integer(&Q::from_integer(magnitude.numer().clone()), factored)?;
    let body = if magnitude.is_integer() {
        top
    } else {
        let bottom = integer(&Q::from_integer(magnitude.denom().clone()), factored)?;
        Expr::binary(BinaryOp::Div, top, bottom)
    };
    Ok(if q.is_negative() {
        Expr::unary(UnaryOp::Neg, body)
    } else {
        body
    })
}

/// A non-negative integer; with `factored`, a product of prime powers when it has several factors.
fn integer(n: &Q, factored: bool) -> Result<Expr, SymError> {
    if !factored {
        return Ok(number(n.clone()));
    }
    let Some(v) = n.numer().to_u64() else {
        return Err(SymError::Unsupported(
            "factor: integer factor too large to factor".into(),
        ));
    };
    if v <= 1 {
        return Ok(number(n.clone()));
    }
    let parts = integer_factors(v)?;
    if let [(_, 1)] = parts.as_slice() {
        return Ok(number(n.clone()));
    }
    let mut factors = Vec::with_capacity(parts.len());
    for (p, e) in parts {
        let prime = number(from_i128(i128::from(p)));
        factors.push(if e == 1 {
            prime
        } else {
            Expr::binary(BinaryOp::Pow, prime, number(from_i128(i128::from(e))))
        });
    }
    Ok(fold_mul(factors).unwrap_or_else(|| number(one())))
}

fn number(q: Q) -> Expr {
    Expr::Number(Number::Rational(q))
}

/// Whether a positive integer is prime (its only factor is itself, once).
fn is_prime(q: &Q) -> bool {
    match q.numer().to_u64() {
        Some(v) if v >= 2 => matches!(integer_factors(v).as_deref(), Ok([(_, 1)])),
        _ => false,
    }
}

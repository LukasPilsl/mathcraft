//! The canonical symbolic tree and its constructors.
//!
//! Invariants (kept by the constructors; the enum is public for matching only, so build
//! values through the methods):
//! - `Add`: at least two terms, no `Add` child, no two terms with the same monomial, the
//!   constant term (if any) last, the other terms in [`super::order::cmp_terms`] order.
//! - `Mul`: at least two factors, no `Mul` child, a numeric coefficient first (if it is not
//!   1), the other factors sorted by their printed text, each base appearing once.
//! - `Pow(b, e)`: `e` is neither 0 nor 1; `b` is never a `Num` with an integer exponent
//!   (that is a number), and a `Num` base with a rational exponent is kept only when no exact
//!   root or square-free extraction applies.
//! - `Num` holds every exact rational value.

use std::collections::BTreeMap;
use std::fmt;

use super::canon::{
    build_product, check_int_exponent, combine, pow_num, rest_of, split_coef, with_coef,
};
use super::num::{self, int, one, render, zero, Q};
use super::order;
use num_traits::{Signed, Zero};

use super::{SymError, MAX_DEPTH, MAX_TERMS};

/// A canonical exact symbolic expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sym {
    /// Exact rational number.
    Num(Q),
    /// A variable such as `x` or `a`.
    Symbol(String),
    /// A unit name; behaves as a symbol but keeps its label distinct.
    Unit(String),
    /// An opaque function (`sin`, `ln`, `abs`, user functions): never evaluated here.
    Func(String, Vec<Sym>),
    /// Sum of at least two canonical terms.
    Add(Vec<Sym>),
    /// Product of at least two canonical factors.
    Mul(Vec<Sym>),
    /// Power with a rational exponent.
    Pow(Box<Sym>, Q),
}

impl Sym {
    /// The integer `n`.
    pub fn int(n: i64) -> Sym {
        Sym::Num(int(n))
    }

    /// The exact rational `n/d`.
    pub fn rational(n: i64, d: i64) -> Result<Sym, SymError> {
        if d == 0 {
            return Err(SymError::Undefined("division by zero"));
        }
        Ok(Sym::Num(int(n) / int(d)))
    }

    /// A variable.
    pub fn symbol(name: &str) -> Sym {
        Sym::Symbol(name.to_string())
    }

    /// A unit symbol.
    pub fn unit(name: &str) -> Sym {
        Sym::Unit(name.to_string())
    }

    /// An opaque function application (no evaluation). `TooDeep` when an argument nests more
    /// than [`MAX_DEPTH`] levels: every constructor enforces that bound, so recursive functions
    /// and `Drop` never see deeper values.
    pub fn func(name: &str, args: Vec<Sym>) -> Result<Sym, SymError> {
        check_depth(&args)?;
        Ok(Sym::Func(name.to_string(), args))
    }

    /// The exact value, if this is a number.
    pub fn as_num(&self) -> Option<&Q> {
        match self {
            Sym::Num(q) => Some(q),
            _ => None,
        }
    }

    /// Whether this is the number zero.
    pub fn is_zero(&self) -> bool {
        matches!(self, Sym::Num(q) if q.is_zero())
    }

    /// Sum of any number of terms, canonical.
    pub fn sum(terms: Vec<Sym>) -> Result<Sym, SymError> {
        let _scope = super::budget::Scope::enter();
        check_depth(&terms)?;
        if terms.len() > MAX_TERMS {
            return Err(SymError::TooLarge("sum terms"));
        }
        let flat: Vec<Sym> = terms
            .into_iter()
            .flat_map(|t| match t {
                Sym::Add(ts) => ts,
                other => vec![other],
            })
            .collect();
        if flat.len() > MAX_TERMS {
            return Err(SymError::TooLarge("sum terms"));
        }
        let mut acc: BTreeMap<Sym, Q> = BTreeMap::new();
        for t in flat {
            super::budget::spend(1)?;
            let (c, mono) = split_coef(t);
            let entry = acc.entry(mono).or_insert_with(zero);
            num::add_into(entry, &c)?;
        }
        let mut out: Vec<Sym> = Vec::with_capacity(acc.len());
        for (mono, c) in acc {
            if c.is_zero() {
                continue;
            }
            num::check_bits(&c)?;
            out.push(match mono {
                Sym::Num(_) => Sym::Num(c),
                other => with_coef(c, other),
            });
        }
        out.sort_by(order::cmp_terms);
        Ok(match out.len() {
            0 => Sym::Num(zero()),
            1 => out.remove(0),
            _ => Sym::Add(out),
        })
    }

    /// Product of any number of factors, canonical (combines powers, distributes a numeric
    /// coefficient over a single sum).
    pub fn product(factors: Vec<Sym>) -> Result<Sym, SymError> {
        let _scope = super::budget::Scope::enter();
        check_depth(&factors)?;
        if factors.len() > MAX_TERMS {
            return Err(SymError::TooLarge("product factors"));
        }
        let mut items = factors;
        let mut rounds = 0;
        loop {
            let (coef, extra, changed) = combine(items)?;
            if coef.is_zero() {
                return Ok(Sym::Num(zero()));
            }
            if !changed || rounds >= 4 {
                return build_product(coef, extra);
            }
            rounds += 1;
            items = extra;
            items.push(Sym::Num(coef));
        }
    }

    /// `base ^ exp` with a rational exponent, canonical.
    pub fn pow(base: Sym, exp: Q) -> Result<Sym, SymError> {
        let _scope = super::budget::Scope::enter();
        check_depth(std::slice::from_ref(&base))?;
        if exp.is_zero() {
            return Ok(Sym::Num(one()));
        }
        if exp == one() {
            return Ok(base);
        }
        match base {
            Sym::Num(b) => pow_num(b, exp),
            Sym::Pow(inner, e1) if exp.is_integer() => {
                let mut e = e1;
                num::mul_into(&mut e, &exp)?;
                Sym::pow(*inner, e)
            }
            Sym::Mul(fs) if exp.is_integer() => {
                check_int_exponent(&exp)?;
                let parts = fs
                    .into_iter()
                    .map(|f| Sym::pow(f, exp.clone()))
                    .collect::<Result<Vec<_>, _>>()?;
                Sym::product(parts)
            }
            other => {
                if exp.is_integer() {
                    check_int_exponent(&exp)?;
                }
                Ok(Sym::Pow(Box::new(other), exp))
            }
        }
    }

    /// `self + other`.
    pub fn plus(self, other: Sym) -> Result<Sym, SymError> {
        Sym::sum(vec![self, other])
    }

    /// `self - other`.
    pub fn minus(self, other: Sym) -> Result<Sym, SymError> {
        Sym::sum(vec![self, Sym::product(vec![Sym::int(-1), other])?])
    }

    /// `self * other`.
    pub fn times(self, other: Sym) -> Result<Sym, SymError> {
        Sym::product(vec![self, other])
    }

    /// `self / other`; `Undefined` when `other` is zero.
    pub fn over(self, other: Sym) -> Result<Sym, SymError> {
        if other.is_zero() {
            return Err(SymError::Undefined("division by zero"));
        }
        Sym::product(vec![self, Sym::pow(other, int(-1))?])
    }

    /// `-self`.
    pub fn negate(self) -> Result<Sym, SymError> {
        Sym::product(vec![Sym::int(-1), self])
    }

    /// Rebuilds the value through the constructors. Canonical values are fixed points.
    pub fn rebuild(&self) -> Result<Sym, SymError> {
        match self {
            Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(self.clone()),
            Sym::Func(n, args) => {
                let a = args
                    .iter()
                    .map(Sym::rebuild)
                    .collect::<Result<Vec<_>, _>>()?;
                Sym::func(n, a)
            }
            Sym::Add(ts) => Sym::sum(ts.iter().map(Sym::rebuild).collect::<Result<Vec<_>, _>>()?),
            Sym::Mul(fs) => {
                Sym::product(fs.iter().map(Sym::rebuild).collect::<Result<Vec<_>, _>>()?)
            }
            Sym::Pow(b, e) => Sym::pow(b.rebuild()?, e.clone()),
        }
    }

    /// Whether `name` occurs anywhere in the value (as a symbol, inside functions or powers).
    pub fn mentions_any(&self, names: &[String]) -> bool {
        match self {
            Sym::Num(_) | Sym::Unit(_) => false,
            Sym::Symbol(n) => names.iter().any(|x| x == n),
            Sym::Func(_, args) | Sym::Add(args) | Sym::Mul(args) => {
                args.iter().any(|a| a.mentions_any(names))
            }
            Sym::Pow(b, _) => b.mentions_any(names),
        }
    }

    /// Printed text of a term without its sign: used when a sum prints `- term`.
    pub(crate) fn negated_term(&self) -> Option<Sym> {
        match self {
            Sym::Num(q) if q.is_negative() => Some(Sym::Num(-q.clone())),
            Sym::Mul(fs) => match fs.first() {
                Some(Sym::Num(q)) if q.is_negative() => {
                    let c = -q.clone();
                    Some(with_coef(c, rest_of(fs)))
                }
                _ => None,
            },
            _ => None,
        }
    }
}

/// Refuses children that nest more than [`MAX_DEPTH`] levels (see [`Sym::func`]).
fn check_depth(children: &[Sym]) -> Result<(), SymError> {
    if children.iter().any(|c| c.deeper_than(MAX_DEPTH)) {
        Err(SymError::TooDeep)
    } else {
        Ok(())
    }
}

impl Sym {
    /// Whether the value nests more than `limit` levels. Stops descending at `limit`, so the
    /// cost is bounded by the size of the first `limit` levels.
    pub(crate) fn deeper_than(&self, limit: usize) -> bool {
        match self {
            Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => false,
            _ if limit == 0 => true,
            Sym::Func(_, args) | Sym::Add(args) | Sym::Mul(args) => {
                args.iter().any(|a| a.deeper_than(limit - 1))
            }
            Sym::Pow(b, _) => b.deeper_than(limit - 1),
        }
    }
}

impl fmt::Display for Sym {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Sym::Num(q) => write!(f, "{}", render(q)),
            Sym::Symbol(n) | Sym::Unit(n) => write!(f, "{n}"),
            Sym::Func(n, args) => {
                write!(f, "{n}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            Sym::Add(ts) => {
                for (i, t) in ts.iter().enumerate() {
                    match (i, t.negated_term()) {
                        (0, _) => write!(f, "{t}")?,
                        (_, Some(pos)) => write!(f, " - {pos}")?,
                        (_, None) => write!(f, " + {t}")?,
                    }
                }
                Ok(())
            }
            Sym::Mul(fs) => {
                for (i, x) in fs.iter().enumerate() {
                    if i > 0 {
                        write!(f, "*")?;
                    }
                    match x {
                        Sym::Add(_) => write!(f, "({x})")?,
                        _ => write!(f, "{x}")?,
                    }
                }
                Ok(())
            }
            Sym::Pow(b, e) => {
                match &**b {
                    Sym::Num(q) if q.is_integer() && !q.is_negative() => write!(f, "{b}")?,
                    Sym::Symbol(_) | Sym::Unit(_) | Sym::Func(..) => write!(f, "{b}")?,
                    _ => write!(f, "({b})")?,
                }
                if e.is_integer() {
                    write!(f, "^{}", render(e))
                } else {
                    write!(f, "^({})", render(e))
                }
            }
        }
    }
}

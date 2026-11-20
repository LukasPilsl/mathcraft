//! Deterministic term order used by sums.
//!
//! Derived from the Prime samples (see the module docs of [`super`]): compare two terms by
//! their monomials, lexicographically over the variables taken in reverse alphabetical order;
//! at the first variable whose exponents differ, the larger exponent comes first. A term
//! without a variable (a constant) comes last. Ties fall back to the structural order, so the
//! result is deterministic.

use std::cmp::Ordering;

use super::num::{zero, Q};
use super::sym::Sym;

/// Exponents of a term's bases, as `(printed base, exponent)`.
fn exponents(t: &Sym) -> Vec<(String, Q)> {
    match t {
        Sym::Num(_) => Vec::new(),
        Sym::Mul(fs) => fs
            .iter()
            .filter_map(|f| match f {
                Sym::Num(_) => None,
                Sym::Pow(b, e) => Some((b.to_string(), e.clone())),
                other => Some((other.to_string(), super::num::one())),
            })
            .collect(),
        Sym::Pow(b, e) => vec![(b.to_string(), e.clone())],
        other => vec![(other.to_string(), super::num::one())],
    }
}

fn lookup(exps: &[(String, Q)], var: &str) -> Q {
    exps.iter()
        .find(|(k, _)| k == var)
        .map(|(_, q)| q.clone())
        .unwrap_or_else(zero)
}

/// Comparator for the terms of a sum (the constant term sorts last).
pub(crate) fn cmp_terms(a: &Sym, b: &Sym) -> Ordering {
    let ea = exponents(a);
    let eb = exponents(b);
    let mut vars: Vec<&String> = ea.iter().chain(eb.iter()).map(|(k, _)| k).collect();
    vars.sort();
    vars.dedup();
    vars.reverse();
    for v in vars {
        let xa = lookup(&ea, v);
        let xb = lookup(&eb, v);
        match xa.cmp(&xb) {
            Ordering::Equal => continue,
            // The larger exponent comes first.
            other => return other.reverse(),
        }
    }
    a.cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::num::int;

    #[test]
    fn constant_sorts_last_and_reverse_alphabetical() {
        let x2 = Sym::Pow(Box::new(Sym::symbol("x")), int(2));
        let x = Sym::symbol("x");
        assert_eq!(cmp_terms(&x2, &x), Ordering::Less);
        assert_eq!(cmp_terms(&Sym::int(5), &x), Ordering::Greater);
        let a = Sym::symbol("a");
        let b = Sym::symbol("b");
        assert_eq!(cmp_terms(&b, &a), Ordering::Less);
    }
}

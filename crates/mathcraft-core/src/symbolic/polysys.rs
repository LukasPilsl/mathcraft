//! Systems of polynomial equations by triangular elimination.
//!
//! Each step does one of two things, and only these:
//! - an equation that is univariate in an unknown `u` (no other name, no parameter): its roots
//!   come from [`solve_polynomial_with_conditions`]; every root is a branch, and the rest of the
//!   system is substituted with that root;
//! - an equation linear in an unknown `u` with a nonzero numeric coefficient: `u` is replaced by
//!   its value in the other equations (no branch).
//!
//! Anything else (no such equation, a parameter in the way, a root that mentions a name, a
//! generic root with a condition) is refused. Every returned row is checked by substitution:
//! each original equation must reduce to exactly zero, or the whole answer is refused.

use super::calc::{coefficients, symbols};
use super::linear::Equation;
use super::poly::{expand, substitute};
use super::poly_solve::solve_polynomial_with_conditions;
use super::sym::Sym;
use super::{budget, SymError};
use num_traits::Zero;

/// Most rows (solutions) one system may produce; more is `TooLarge`.
pub const MAX_SOLUTIONS: usize = 64;

/// All solutions of `eqs` in `unknowns`: one row per solution, values in the order of
/// `unknowns`. Refuses (never guesses) every system outside the triangular shapes above.
pub fn solve_triangular(eqs: &[Equation], unknowns: &[String]) -> Result<Vec<Vec<Sym>>, SymError> {
    let _scope = budget::Scope::enter();
    let residuals = residuals_of(eqs)?;
    let mut assigned: Vec<(String, Sym)> = Vec::new();
    let mut rows: Vec<Vec<Sym>> = Vec::new();
    branch(
        residuals.clone(),
        unknowns.to_vec(),
        &mut assigned,
        &mut rows,
        unknowns,
    )?;
    for row in &rows {
        for r in &residuals {
            let subs: Vec<(&str, Sym)> = unknowns
                .iter()
                .map(|n| n.as_str())
                .zip(row.iter().cloned())
                .collect();
            let check = expand(&substitute(r, &subs)?)?;
            if !check.is_zero() {
                return Err(SymError::Unsupported(
                    "a solution does not satisfy the system exactly".into(),
                ));
            }
        }
    }
    Ok(rows)
}

fn residuals_of(eqs: &[Equation]) -> Result<Vec<Sym>, SymError> {
    eqs.iter()
        .map(|e| {
            Sym::sum(vec![
                e.lhs.clone(),
                Sym::product(vec![Sym::int(-1), e.rhs.clone()])?,
            ])
        })
        .collect()
}

/// One elimination branch. `assigned` lists the unknowns fixed so far, each with its value,
/// which may name unknowns assigned later. When all unknowns are fixed, the row is built by
/// back-substitution in reverse assignment order.
fn branch(
    eqs: Vec<Sym>,
    unknowns: Vec<String>,
    assigned: &mut Vec<(String, Sym)>,
    rows: &mut Vec<Vec<Sym>>,
    order: &[String],
) -> Result<(), SymError> {
    budget::spend(1)?;
    let mut live = Vec::with_capacity(eqs.len());
    for e in eqs {
        let e = expand(&e)?;
        match e {
            Sym::Num(q) if q.is_zero() => {}
            Sym::Num(_) => return Ok(()), // a nonzero constant: no solution on this branch
            other => live.push(other),
        }
    }
    if unknowns.is_empty() {
        if !live.is_empty() {
            return Err(SymError::Unsupported(
                "system with a parameter left over".into(),
            ));
        }
        if rows.len() >= MAX_SOLUTIONS {
            return Err(SymError::TooLarge("solutions"));
        }
        rows.push(row_of(order, assigned)?);
        return Ok(());
    }
    // Univariate equation in one unknown: branch over its roots.
    for e in &live {
        let names = symbols(e);
        let [only] = names.as_slice() else { continue };
        if !unknowns.contains(only) {
            continue;
        }
        let eq = Equation {
            lhs: e.clone(),
            rhs: Sym::Num(super::num::zero()),
        };
        let (roots, conditions) = solve_polynomial_with_conditions(&eq, only)?;
        if !conditions.is_empty() {
            return Err(SymError::Unsupported(
                "a root depends on a parameter".into(),
            ));
        }
        let rest: Vec<String> = unknowns.iter().filter(|n| *n != only).cloned().collect();
        for root in roots {
            if !symbols(&root).is_empty() {
                return Err(SymError::Unsupported("a root mentions a name".into()));
            }
            let next = substitute_all(&live, only, &root)?;
            assigned.push((only.clone(), root));
            branch(next, rest.clone(), assigned, rows, order)?;
            assigned.pop();
        }
        return Ok(());
    }
    // Linear in an unknown with a nonzero numeric coefficient: solve for it, no branch.
    for e in &live {
        for u in &unknowns {
            if !symbols(e).contains(u) {
                continue;
            }
            let parts = coefficients(e, u)?;
            let [c0, c1] = parts.as_slice() else { continue };
            let Sym::Num(slope) = c1 else { continue };
            if slope.is_zero() {
                continue;
            }
            let value = Sym::product(vec![
                Sym::Num(-(super::num::one() / slope.clone())),
                c0.clone(),
            ])?;
            let next = substitute_all(&live, u, &value)?;
            let rest: Vec<String> = unknowns.iter().filter(|n| *n != u).cloned().collect();
            assigned.push((u.clone(), value));
            branch(next, rest, assigned, rows, order)?;
            assigned.pop();
            return Ok(());
        }
    }
    Err(SymError::Unsupported("system is not triangular".into()))
}

fn substitute_all(eqs: &[Sym], name: &str, value: &Sym) -> Result<Vec<Sym>, SymError> {
    eqs.iter()
        .map(|e| substitute(e, &[(name, value.clone())]))
        .collect()
}

/// The values of `order` from the assignments: each assignment may name later ones, so the list
/// is read backwards and every name is replaced by its value.
fn row_of(order: &[String], assigned: &[(String, Sym)]) -> Result<Vec<Sym>, SymError> {
    let mut values: Vec<(String, Sym)> = Vec::new();
    for (name, expr) in assigned.iter().rev() {
        let subs: Vec<(&str, Sym)> = values
            .iter()
            .map(|(n, v)| (n.as_str(), v.clone()))
            .collect();
        let value = substitute(expr, &subs)?;
        if !symbols(&value).is_empty() {
            return Err(SymError::Unsupported("a solution mentions a name".into()));
        }
        values.push((name.clone(), value));
    }
    order
        .iter()
        .map(|n| {
            values
                .iter()
                .find(|(m, _)| m == n)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| SymError::Unsupported(format!("no value for `{n}`")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::num::int;

    fn eq(lhs: Sym, rhs: Sym) -> Equation {
        Equation { lhs, rhs }
    }
    fn s(n: &str) -> Sym {
        Sym::symbol(n)
    }
    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn sum_and_product_give_both_orders() {
        // x + y = 3, x y = 2 -> (2,1) and (1,2)
        let e1 = eq(Sym::sum(vec![s("x"), s("y")]).expect("sum"), Sym::int(3));
        let e2 = eq(
            Sym::product(vec![s("x"), s("y")]).expect("prod"),
            Sym::int(2),
        );
        let rows = solve_triangular(&[e1, e2], &names(&["x", "y"])).expect("solve");
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&vec![Sym::int(2), Sym::int(1)]));
        assert!(rows.contains(&vec![Sym::int(1), Sym::int(2)]));
    }

    #[test]
    fn circle_and_line_with_surd_roots() {
        // x^2 + y^2 = 3, y = x: x = y = +-sqrt(3/2)
        let circle = eq(
            Sym::sum(vec![
                Sym::pow(s("x"), int(2)).expect("x2"),
                Sym::pow(s("y"), int(2)).expect("y2"),
            ])
            .expect("sum"),
            Sym::int(3),
        );
        let line = eq(s("y"), s("x"));
        let rows = solve_triangular(&[circle, line], &names(&["x", "y"])).expect("solve");
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row[0], row[1]);
        }
    }

    #[test]
    fn inconsistent_system_has_no_rows() {
        // x = 1, x = 2
        let rows = solve_triangular(
            &[eq(s("x"), Sym::int(1)), eq(s("x"), Sym::int(2))],
            &names(&["x"]),
        )
        .expect("solve");
        assert!(rows.is_empty());
    }

    #[test]
    fn underdetermined_system_is_refused() {
        let r = solve_triangular(&[eq(s("x"), s("y"))], &names(&["x", "y"]));
        assert!(matches!(r, Err(SymError::Unsupported(_))));
    }

    #[test]
    fn parameter_system_is_refused() {
        // a x = 1 with a symbolic a: not numeric, refused.
        let ax = Sym::product(vec![s("a"), s("x")]).expect("ax");
        let r = solve_triangular(&[eq(ax, Sym::int(1))], &names(&["x"]));
        assert!(r.is_err());
    }

    #[test]
    fn cubic_without_rational_roots_is_refused() {
        // x^3 - 2 = 0 is not triangular with rational roots: refused by the univariate solver.
        let cube =
            Sym::sum(vec![Sym::pow(s("x"), int(3)).expect("x3"), Sym::int(-2)]).expect("cube");
        let r = solve_triangular(&[eq(cube, Sym::int(0))], &names(&["x"]));
        assert!(r.is_err());
    }
}

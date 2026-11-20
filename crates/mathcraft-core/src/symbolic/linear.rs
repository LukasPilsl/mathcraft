//! Exact linear systems: Gauss-Jordan elimination over the field of rational functions.
//!
//! Coefficients may be any canonical values (numbers, symbols such as `a` and `b`), so
//! `a*x+b=0` gives `x = -b/a`. Pivots are chosen to be nonzero constants when possible. A
//! symbolic pivot is assumed nonzero: the answer is the generic one, as in a textbook
//! derivation. Fraction-free (Bareiss) elimination is not used: the same canonical results come
//! from cancelled rational functions, which keeps the code small.

use num_traits::Zero;

use super::num::int;
use super::poly::Poly;
use super::ratfunc::RatFunc;
use super::sym::Sym;
use super::{SymError, MAX_UNKNOWNS};

/// An equation `lhs = rhs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Equation {
    pub lhs: Sym,
    pub rhs: Sym,
}

/// Result of [`solve_linear_system`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveOutcome {
    /// Exactly one solution: `(unknown, value)` in the order of the unknowns.
    Unique(Vec<(String, Sym)>),
    /// The equations contradict each other.
    NoSolution,
    /// Infinitely many solutions. `free` lists the unknowns that are free parameters; `solution`
    /// gives every unknown in order, the free ones as themselves and the others in terms of
    /// the free ones.
    Infinite {
        free: Vec<String>,
        solution: Vec<(String, Sym)>,
    },
    /// Some equation is not linear in the unknowns (a product of unknowns, an unknown inside a
    /// function or a fractional power).
    NotLinear,
}

/// One row: `sum coef[j] * x_j = rhs`.
#[derive(Clone)]
struct Row {
    coef: Vec<RatFunc>,
    rhs: RatFunc,
}

/// Solves a linear system exactly for the given unknowns.
pub fn solve_linear_system(
    equations: &[Equation],
    unknowns: &[String],
) -> Result<SolveOutcome, SymError> {
    solve_linear_system_with_conditions(equations, unknowns).map(|(outcome, _)| outcome)
}

/// Like [`solve_linear_system`], and also returns the conditions the generic answer assumes:
/// every symbolic pivot is nonzero (each is listed once, as a value that must not vanish).
/// With numeric coefficients the list is empty.
pub fn solve_linear_system_with_conditions(
    equations: &[Equation],
    unknowns: &[String],
) -> Result<(SolveOutcome, Vec<Sym>), SymError> {
    let _scope = super::budget::Scope::enter();
    let n = unknowns.len();
    if n > MAX_UNKNOWNS {
        return Err(SymError::TooLarge("unknowns"));
    }
    for (i, u) in unknowns.iter().enumerate() {
        if unknowns[..i].contains(u) {
            return Err(SymError::Invalid(format!("duplicate unknown {u}")));
        }
    }
    let mut rows = Vec::with_capacity(equations.len());
    for eq in equations {
        match build_row(eq, unknowns)? {
            Some(r) => rows.push(r),
            None => return Ok((SolveOutcome::NotLinear, Vec::new())),
        }
    }
    let mut conditions = Vec::new();
    let pivots = gauss_jordan(&mut rows, n, &mut conditions)?;
    // Leftover rows (no pivot) say `0 = rhs`. A nonzero constant contradicts the system for
    // every value of the parameters. A nonzero symbolic rhs makes consistency depend on the
    // parameters: that case is refused, never reported as a definite answer.
    let leftover = &rows[pivots.len()..];
    if leftover
        .iter()
        .any(|r| r.rhs.as_const().is_some_and(|c| !c.is_zero()))
    {
        return Ok((SolveOutcome::NoSolution, conditions));
    }
    if leftover.iter().any(|r| !r.rhs.is_zero()) {
        return Err(SymError::Unsupported(
            "consistency depends on parameters".into(),
        ));
    }
    if pivots.len() == n {
        let mut solution = Vec::with_capacity(n);
        for (col, name) in unknowns.iter().enumerate() {
            let k = pivots
                .iter()
                .position(|&c| c == col)
                .ok_or(SymError::Invalid("internal: missing pivot".to_string()))?;
            solution.push((name.clone(), rows[k].rhs.to_sym()?));
        }
        return Ok((SolveOutcome::Unique(solution), conditions));
    }
    let free_cols: Vec<usize> = (0..n).filter(|c| !pivots.contains(c)).collect();
    let free: Vec<String> = free_cols.iter().map(|&c| unknowns[c].clone()).collect();
    let mut solution = Vec::with_capacity(n);
    for (col, name) in unknowns.iter().enumerate() {
        match pivots.iter().position(|&c| c == col) {
            Some(k) => {
                let mut value = rows[k].rhs.clone();
                for &f in &free_cols {
                    let term =
                        rows[k].coef[f].times(&RatFunc::from_sym(&Sym::symbol(&unknowns[f]))?)?;
                    value = value.minus(&term)?;
                }
                solution.push((name.clone(), value.to_sym()?));
            }
            None => solution.push((name.clone(), Sym::symbol(name))),
        }
    }
    Ok((SolveOutcome::Infinite { free, solution }, conditions))
}

/// Builds the row `sum a_j x_j = -c` of `lhs - rhs = sum a_j x_j + c`. `None` when the
/// equation is not linear in the unknowns.
fn build_row(eq: &Equation, unknowns: &[String]) -> Result<Option<Row>, SymError> {
    let diff = eq.lhs.clone().minus(eq.rhs.clone())?;
    let poly = Poly::from_sym(&diff)?;
    let n = unknowns.len();
    let mut coef_polys: Vec<Poly> = (0..n).map(|_| Poly::zero()).collect();
    let mut konst = Poly::zero();
    for (mono, c) in &poly.terms {
        let mut rest = mono.clone();
        let mut deg = 0u32;
        let mut which: Option<usize> = None;
        let mut nonlinear_atom = false;
        for (atom, e) in mono {
            match atom {
                Sym::Symbol(name) => {
                    if let Some(j) = unknowns.iter().position(|u| u == name) {
                        deg += e;
                        which = Some(j);
                        rest.remove(atom);
                        continue;
                    }
                    rest.insert(atom.clone(), *e);
                }
                other => {
                    if other.mentions_any(unknowns) {
                        nonlinear_atom = true;
                    }
                    rest.insert(atom.clone(), *e);
                }
            }
        }
        if nonlinear_atom || deg > 1 {
            return Ok(None);
        }
        let mut single = std::collections::BTreeMap::new();
        single.insert(rest, c.clone());
        let term = Poly { terms: single };
        match which {
            Some(j) => coef_polys[j] = coef_polys[j].add(&term)?,
            None => konst = konst.add(&term)?,
        }
    }
    let coef = coef_polys
        .into_iter()
        .map(|p| RatFunc::make(p, Poly::constant(int(1))))
        .collect::<Result<Vec<_>, _>>()?;
    let rhs = RatFunc::make(konst.scale(&int(-1))?, Poly::constant(int(1)))?;
    Ok(Some(Row { coef, rhs }))
}

/// Reduces `rows` in place and returns the pivot column of each leading row.
fn gauss_jordan(
    rows: &mut [Row],
    n: usize,
    conditions: &mut Vec<Sym>,
) -> Result<Vec<usize>, SymError> {
    let mut pivots = Vec::new();
    let mut row_idx = 0;
    for col in 0..n {
        if row_idx >= rows.len() {
            break;
        }
        let candidate = (row_idx..rows.len())
            .find(|&r| rows[r].coef[col].as_const().is_some_and(|c| !c.is_zero()))
            .or_else(|| (row_idx..rows.len()).find(|&r| !rows[r].coef[col].is_zero()));
        let Some(p) = candidate else { continue };
        rows.swap(row_idx, p);
        let prow = rows[row_idx].clone();
        let pv = prow.coef[col].clone();
        if pv.as_const().is_none() {
            let cond = pv.to_sym()?;
            if !conditions.contains(&cond) {
                conditions.push(cond);
            }
        }
        let pivot_row = Row {
            coef: prow
                .coef
                .iter()
                .map(|c| c.over(&pv))
                .collect::<Result<Vec<_>, _>>()?,
            rhs: prow.rhs.over(&pv)?,
        };
        rows[row_idx] = pivot_row.clone();
        for (r, row) in rows.iter_mut().enumerate() {
            if r == row_idx {
                continue;
            }
            let f = row.coef[col].clone();
            if f.is_zero() {
                continue;
            }
            for j in 0..n {
                super::budget::spend(1)?;
                row.coef[j] = row.coef[j].minus(&f.times(&pivot_row.coef[j])?)?;
            }
            row.rhs = row.rhs.minus(&f.times(&pivot_row.rhs)?)?;
        }
        pivots.push(col);
        row_idx += 1;
    }
    Ok(pivots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_by_two_unique() {
        // a + 2b = 7, 2a - b = 2  ->  a = 11/5, b = 12/5
        let a = Sym::symbol("a");
        let b = Sym::symbol("b");
        let e1 = Equation {
            lhs: a
                .clone()
                .plus(Sym::int(2).times(b.clone()).expect("t"))
                .expect("p"),
            rhs: Sym::int(7),
        };
        let e2 = Equation {
            lhs: Sym::int(2).times(a).expect("t").minus(b).expect("m"),
            rhs: Sym::int(2),
        };
        let out = solve_linear_system(&[e1, e2], &["a".into(), "b".into()]).expect("solve");
        let want = SolveOutcome::Unique(vec![
            ("a".into(), Sym::rational(11, 5).expect("q")),
            ("b".into(), Sym::rational(12, 5).expect("q")),
        ]);
        assert_eq!(out, want);
    }
}

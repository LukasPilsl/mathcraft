//! Symbolic matrices over canonical [`Sym`] entries.
//!
//! Every operation works in the field of rational functions ([`RatFunc`]): entries are
//! cancelled after each step, so `det`, `inverse` and products are exact identities of
//! rational functions. Pivoting swaps rows only when a pivot is identically zero, so the
//! results do not depend on a hidden assumption about parameter values. A symbolic pivot
//! that is not a constant is treated as nonzero; the inverse is reported with the condition
//! `det != 0`, and [`SymMatrix::rank`] reports the pivots that must not vanish.
//!
//! Limits: matrices up to [`MAX_MATRIX_DIM`] x [`MAX_MATRIX_DIM`], integer powers up to
//! [`MAX_MATRIX_POWER`]. Every public operation opens a work-budget scope.

use num_traits::Zero;

use super::linear::{solve_linear_system_with_conditions, Equation, SolveOutcome};
use super::poly::substitute;
use super::poly_solve::solve_polynomial;
use super::ratfunc::RatFunc;
use super::sym::Sym;
use super::{budget, SymError};

/// Largest number of rows and of columns.
pub const MAX_MATRIX_DIM: usize = 5;
/// Largest absolute integer power computed by repeated multiplication.
pub const MAX_MATRIX_POWER: i64 = 64;

/// A dense row-major matrix of canonical symbolic entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymMatrix {
    rows: usize,
    cols: usize,
    data: Vec<Sym>,
}

/// Result of [`SymMatrix::inverse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inverse {
    /// The inverse, valid wherever `condition` holds.
    pub matrix: SymMatrix,
    /// `None` when the determinant is a nonzero constant; otherwise the determinant, which
    /// must be nonzero for the inverse to exist.
    pub condition: Option<Sym>,
}

/// Result of [`SymMatrix::rank`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rank {
    /// Generic rank: the rank for parameter values outside the conditions below.
    pub rank: usize,
    /// Symbolic pivots that the generic rank assumes nonzero (empty for numeric matrices).
    pub conditions: Vec<Sym>,
}

impl SymMatrix {
    /// Builds a matrix from rows. Rejects empty, ragged and oversized input.
    pub fn new(rows: Vec<Vec<Sym>>) -> Result<SymMatrix, SymError> {
        let _scope = budget::Scope::enter();
        let r = rows.len();
        let c = rows.first().map_or(0, Vec::len);
        if r == 0 || c == 0 {
            return Err(SymError::Invalid("empty matrix".into()));
        }
        if rows.iter().any(|row| row.len() != c) {
            return Err(SymError::Invalid("ragged matrix rows".into()));
        }
        if r > MAX_MATRIX_DIM || c > MAX_MATRIX_DIM {
            return Err(SymError::TooLarge("matrix dimension"));
        }
        Ok(SymMatrix {
            rows: r,
            cols: c,
            data: rows.into_iter().flatten().collect(),
        })
    }

    /// The n x n identity.
    pub fn identity(n: usize) -> Result<SymMatrix, SymError> {
        if n == 0 || n > MAX_MATRIX_DIM {
            return Err(SymError::TooLarge("matrix dimension"));
        }
        let mut data = vec![Sym::int(0); n * n];
        for i in 0..n {
            data[i * n + i] = Sym::int(1);
        }
        Ok(SymMatrix {
            rows: n,
            cols: n,
            data,
        })
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Entry `(i, j)`, zero-based.
    pub fn get(&self, i: usize, j: usize) -> Option<&Sym> {
        if i < self.rows && j < self.cols {
            self.data.get(i * self.cols + j)
        } else {
            None
        }
    }

    /// Row lists, for printing and conversion.
    pub fn to_rows(&self) -> Vec<Vec<Sym>> {
        self.data.chunks(self.cols).map(<[Sym]>::to_vec).collect()
    }

    /// Transpose (entries unchanged).
    pub fn transpose(&self) -> SymMatrix {
        let mut data = Vec::with_capacity(self.data.len());
        for j in 0..self.cols {
            for i in 0..self.rows {
                data.push(self.data[i * self.cols + j].clone());
            }
        }
        SymMatrix {
            rows: self.cols,
            cols: self.rows,
            data,
        }
    }

    /// Matrix product `self * other`, each entry cancelled to lowest terms.
    pub fn multiply(&self, other: &SymMatrix) -> Result<SymMatrix, SymError> {
        let _scope = budget::Scope::enter();
        if self.cols != other.rows {
            return Err(SymError::Invalid(format!(
                "product of {}x{} and {}x{}",
                self.rows, self.cols, other.rows, other.cols
            )));
        }
        let a = self.to_ratfuncs()?;
        let b = other.to_ratfuncs()?;
        let mut data = Vec::with_capacity(self.rows * other.cols);
        for i in 0..self.rows {
            for j in 0..other.cols {
                let mut acc = RatFunc::zero();
                for k in 0..self.cols {
                    budget::spend(1)?;
                    let t = a[i * self.cols + k].times(&b[k * other.cols + j])?;
                    acc = acc.plus(&t)?;
                }
                data.push(acc.to_sym()?);
            }
        }
        Ok(SymMatrix {
            rows: self.rows,
            cols: other.cols,
            data,
        })
    }

    /// Determinant of a square matrix, cancelled to lowest terms.
    pub fn determinant(&self) -> Result<Sym, SymError> {
        let _scope = budget::Scope::enter();
        self.require_square()?;
        det_ratfunc(&self.to_ratfuncs()?, self.rows)?.to_sym()
    }

    /// Inverse of a square matrix. `Undefined` when the determinant is identically zero.
    pub fn inverse(&self) -> Result<Inverse, SymError> {
        let _scope = budget::Scope::enter();
        self.require_square()?;
        let n = self.rows;
        let det = det_ratfunc(&self.to_ratfuncs()?, n)?;
        if det.is_zero() {
            return Err(SymError::Undefined("singular matrix"));
        }
        let condition = match det.as_const() {
            Some(_) => None,
            None => Some(det.to_sym()?),
        };
        // Gauss-Jordan on [A | I].
        let mut m = self.to_ratfuncs()?;
        let mut aug: Vec<RatFunc> = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                aug.push(RatFunc::from_sym(&Sym::int(i64::from(i == j)))?);
            }
        }
        for col in 0..n {
            let p = pivot_row(&m, n, col).ok_or(SymError::Undefined("singular matrix"))?;
            swap_rows(&mut m, n, col, p);
            swap_rows(&mut aug, n, col, p);
            let pv = m[col * n + col].clone();
            for j in 0..n {
                m[col * n + j] = m[col * n + j].over(&pv)?;
                aug[col * n + j] = aug[col * n + j].over(&pv)?;
            }
            for r in 0..n {
                if r == col {
                    continue;
                }
                let f = m[r * n + col].clone();
                if f.is_zero() {
                    continue;
                }
                for j in 0..n {
                    budget::spend(1)?;
                    let tm = f.times(&m[col * n + j])?;
                    m[r * n + j] = m[r * n + j].minus(&tm)?;
                    let ta = f.times(&aug[col * n + j])?;
                    aug[r * n + j] = aug[r * n + j].minus(&ta)?;
                }
            }
        }
        let data = aug
            .iter()
            .map(RatFunc::to_sym)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Inverse {
            matrix: SymMatrix {
                rows: n,
                cols: n,
                data,
            },
            condition,
        })
    }

    /// Integer power `self^k`: `k = 0` gives the identity, negative `k` uses the inverse.
    pub fn power(&self, k: i64) -> Result<SymMatrix, SymError> {
        let _scope = budget::Scope::enter();
        self.require_square()?;
        if k.unsigned_abs() > MAX_MATRIX_POWER as u64 {
            return Err(SymError::TooLarge("matrix power"));
        }
        let base = if k < 0 {
            self.inverse()?.matrix
        } else {
            self.clone()
        };
        let mut result = SymMatrix::identity(self.rows)?;
        let mut sq = base;
        let mut e = k.unsigned_abs();
        while e > 0 {
            if e & 1 == 1 {
                result = result.multiply(&sq)?;
            }
            e >>= 1;
            if e > 0 {
                sq = sq.multiply(&sq)?;
            }
        }
        Ok(result)
    }

    /// Characteristic polynomial `det(var*I - A)` in the new variable `var`.
    /// `Invalid` when `var` already occurs in the matrix.
    pub fn characteristic_polynomial(&self, var: &str) -> Result<Sym, SymError> {
        let _scope = budget::Scope::enter();
        self.require_square()?;
        if self
            .data
            .iter()
            .any(|e| super::calc::symbols(e).iter().any(|s| s == var))
        {
            return Err(SymError::Invalid(format!("{var} occurs in the matrix")));
        }
        let n = self.rows;
        let x = Sym::symbol(var);
        let mut data = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                let a = self.data[i * n + j].clone();
                let entry = if i == j {
                    x.clone().minus(a)?
                } else {
                    a.negate()?
                };
                data.push(RatFunc::from_sym(&entry)?);
            }
        }
        det_ratfunc(&data, n)?.to_sym()
    }

    /// Eigenvalues of a 2x2 or 3x3 matrix: the roots of the characteristic polynomial in
    /// `var`, as far as [`solve_polynomial`] reaches (quadratics with real roots, cubics with
    /// rational roots). Complex or irrational cubic roots are refused.
    pub fn eigenvalues(&self, var: &str) -> Result<Vec<Sym>, SymError> {
        let _scope = budget::Scope::enter();
        if !(2..=3).contains(&self.rows) || self.rows != self.cols {
            return Err(SymError::Unsupported(
                "eigenvalues only for 2x2 and 3x3 matrices".into(),
            ));
        }
        let cp = self.characteristic_polynomial(var)?;
        let eq = Equation {
            lhs: cp,
            rhs: Sym::int(0),
        };
        solve_polynomial(&eq, var)
    }

    /// Rank of a matrix with generic (symbolic) entries. Pivots that are not constants are
    /// assumed nonzero and are listed in `conditions`.
    pub fn rank(&self) -> Result<Rank, SymError> {
        let _scope = budget::Scope::enter();
        let (rows, cols) = (self.rows, self.cols);
        let mut m = self.to_ratfuncs()?;
        let mut conditions: Vec<Sym> = Vec::new();
        let mut rank = 0;
        for col in 0..cols {
            if rank == rows {
                break;
            }
            let p = (rank..rows)
                .find(|&r| m[r * cols + col].as_const().is_some_and(|c| !c.is_zero()))
                .or_else(|| (rank..rows).find(|&r| !m[r * cols + col].is_zero()));
            let Some(p) = p else { continue };
            swap_rows_wide(&mut m, cols, rank, p);
            let pv = m[rank * cols + col].clone();
            if pv.as_const().is_none() {
                let c = pv.to_sym()?;
                if !conditions.contains(&c) {
                    conditions.push(c);
                }
            }
            for r in rank + 1..rows {
                let f = m[r * cols + col].clone();
                if f.is_zero() {
                    continue;
                }
                let factor = f.over(&pv)?;
                for j in col..cols {
                    budget::spend(1)?;
                    let t = factor.times(&m[rank * cols + j])?;
                    m[r * cols + j] = m[r * cols + j].minus(&t)?;
                }
            }
            rank += 1;
        }
        Ok(Rank { rank, conditions })
    }

    /// Basis of the null space of a matrix with symbolic entries: the general solution of
    /// `A x = 0` as the generic answer, with the nonzero-pivot conditions reported.
    /// Returns the basis vectors (empty for a trivial null space).
    pub fn nullspace(&self) -> Result<(Vec<Vec<Sym>>, Vec<Sym>), SymError> {
        let _scope = budget::Scope::enter();
        let names: Vec<String> = (0..self.cols).map(|j| format!("x{j}")).collect();
        let equations: Vec<Equation> = self
            .to_rows()
            .into_iter()
            .map(|row| {
                let mut lhs = Sym::int(0);
                for (j, a) in row.into_iter().enumerate() {
                    lhs = lhs.plus(a.times(Sym::symbol(&names[j]))?)?;
                }
                Ok(Equation {
                    lhs,
                    rhs: Sym::int(0),
                })
            })
            .collect::<Result<_, SymError>>()?;
        let (outcome, conditions) = solve_linear_system_with_conditions(&equations, &names)?;
        match outcome {
            SolveOutcome::Unique(_) => Ok((Vec::new(), conditions)),
            SolveOutcome::Infinite { free, solution } => {
                let mut basis = Vec::with_capacity(free.len());
                for f in &free {
                    let subs: Vec<(&str, Sym)> = free
                        .iter()
                        .map(|g| (g.as_str(), Sym::int(i64::from(g == f))))
                        .collect();
                    let mut v = Vec::with_capacity(solution.len());
                    for (_, value) in &solution {
                        v.push(substitute(value, &subs)?);
                    }
                    basis.push(v);
                }
                Ok((basis, conditions))
            }
            SolveOutcome::NoSolution => Err(SymError::Invalid("homogeneous system".into())),
            SolveOutcome::NotLinear => Err(SymError::Unsupported("nonlinear matrix".into())),
        }
    }

    fn require_square(&self) -> Result<(), SymError> {
        if self.rows != self.cols {
            return Err(SymError::Invalid(format!(
                "{}x{} matrix is not square",
                self.rows, self.cols
            )));
        }
        Ok(())
    }

    fn to_ratfuncs(&self) -> Result<Vec<RatFunc>, SymError> {
        self.data.iter().map(RatFunc::from_sym).collect()
    }
}

/// Determinant of a square row-major field matrix by Laplace expansion along the first row.
/// Only products and sums are formed (no pivot division), so polynomial entries give a
/// polynomial determinant with no spurious denominator. Cost is n! terms, n <= 5.
fn det_ratfunc(m: &[RatFunc], n: usize) -> Result<RatFunc, SymError> {
    if n == 0 {
        return RatFunc::from_sym(&Sym::int(1));
    }
    if n == 1 {
        return Ok(m[0].clone());
    }
    let mut acc = RatFunc::zero();
    for j in 0..n {
        let a = &m[j];
        if a.is_zero() {
            continue;
        }
        budget::spend(1)?;
        let mut minor = Vec::with_capacity((n - 1) * (n - 1));
        for r in 1..n {
            for c in 0..n {
                if c != j {
                    minor.push(m[r * n + c].clone());
                }
            }
        }
        let term = a.times(&det_ratfunc(&minor, n - 1)?)?;
        acc = if j % 2 == 0 {
            acc.plus(&term)?
        } else {
            acc.minus(&term)?
        };
    }
    Ok(acc)
}

/// Row `p >= col` with a nonzero entry in `col`, preferring a nonzero constant.
fn pivot_row(m: &[RatFunc], n: usize, col: usize) -> Option<usize> {
    (col..n)
        .find(|&r| m[r * n + col].as_const().is_some_and(|c| !c.is_zero()))
        .or_else(|| (col..n).find(|&r| !m[r * n + col].is_zero()))
}

fn swap_rows(m: &mut [RatFunc], n: usize, a: usize, b: usize) {
    if a != b {
        for j in 0..n {
            m.swap(a * n + j, b * n + j);
        }
    }
}

fn swap_rows_wide(m: &mut [RatFunc], cols: usize, a: usize, b: usize) {
    if a != b {
        for j in 0..cols {
            m.swap(a * cols + j, b * cols + j);
        }
    }
}

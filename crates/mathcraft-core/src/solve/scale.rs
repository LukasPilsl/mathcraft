//! Row scales for the solve core. Each equation is divided by a scale fixed at the guess, so the
//! Newton line search judges residuals in their own units. Levenberg-Marquardt does not use the
//! scales: it minimises the unweighted sum of squares (see `lm::stationarity`).

use super::problem::{Evaluator, Halt, Kind};

/// Relative floor on the scale of an equation (see `Evaluator::prepare`). Not an absolute floor:
/// an equation of size 1e-15 keeps its own scale.
const SCALE_TINY: f64 = 1e-300;

impl<'p, 'a> Evaluator<'p, 'a> {
    /// Fixes the row scales from the residual and Jacobian at `guess`. An equality row's scale is
    /// `max(|r_i|, ||J_i||_1 * (||x||_inf + 1e-300), 1e-300)`: its own value at the guess, or the
    /// size its unknowns contribute to it, with no absolute floor of 1. So `1e-15 (x - 3)` keeps
    /// a scale of 3e-15 and its residual is judged in its own units. An inequality row (zero when
    /// satisfied, so it has no natural scale) uses `max(.., CTOL)`. Counts the evaluations.
    pub fn prepare(&mut self, guess: &[f64]) -> Result<(), Halt> {
        let r0 = self.eval(Kind::Aug, guess)?;
        let rows = r0.len();
        let n = guess.len();
        let j0 = self.jacobian(Kind::Aug, guess, rows)?;
        let x_inf = guess.iter().fold(0.0_f64, |s, v| s.max(v.abs()));
        self.scale = (0..rows)
            .map(|i| {
                let row_l1: f64 = j0[i * n..(i + 1) * n].iter().map(|v| v.abs()).sum();
                let natural = row_l1 * (x_inf + SCALE_TINY);
                let s = r0[i].abs().max(natural).max(SCALE_TINY);
                if i >= self.m {
                    s.max(self.ctol)
                } else {
                    s
                }
            })
            .collect();
        Ok(())
    }

    /// Residual divided row by row by its scale (a prefix of the scales when `v` is `Eq`-sized).
    pub fn scaled(&self, v: &[f64]) -> Vec<f64> {
        v.iter().zip(&self.scale).map(|(a, s)| a / s).collect()
    }
}

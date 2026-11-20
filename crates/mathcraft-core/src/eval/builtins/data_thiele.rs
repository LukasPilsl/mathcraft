//! Thiele continued-fraction interpolation (Mathcad Prime help: "Thiele Continued Fraction
//! Interpolation", "Example: Thiele Interpolation").
//!
//! - `Thielecoeff(vx, vy)`: the coefficients `G_0 .. G_{n-1}` of the continued fraction
//!   `G_0 + (x - x_0) / (G_1 + (x - x_1) / (G_2 + ... + (x - x_{n-2}) / G_{n-1}))` through the
//!   `n` points. The coefficients are the inverse differences of the standard recurrence
//!   (Hildebrand 1974): `R_0[i] = y_i`, `R_k[i] = (x_{i+k} - x_i) / (R_{k-1}[i+1] - R_{k-1}[i]) +
//!   R_{k-2}[i+1]` with `R_{-1} = 0`, and `G_k = R_k[0] - R_{k-2}[0]`. The points keep their
//!   order; the help's example gives `G = [1, -2.5, -1.2, 17.5, 0.2]` for `X = [1 2 3 4 0]`,
//!   `Y = [1 3/5 2/5 5/17 1]`, which this recurrence reproduces.
//! - `Thiele(vx, coeff, x)`: the continued fraction evaluated bottom-up at `x`.
//!
//! Units: the arithmetic runs on the SI magnitudes, which is exact for the continued fraction
//! (scaling `x` or `y` scales the interpolant the same way). `G_k` has the dimension of `y` for
//! even `k` and of `x / y` for odd `k`; `Thiele` returns a value of the dimension of `G_0`, and
//! requires `x` to have the dimension of `vx`.
//!
//! Refused rather than returned wrong: repeated `x` values, a zero denominator in the recurrence
//! (two adjacent points with equal `y`, which the help warns about), and a zero denominator in the
//! evaluation. The help's example with `Y = [1 1 3/5 ...]` is refused, and the reordered points
//! are the way out.

use super::data_util::{one_dim, real_result, real_scalar, reals, vector};
use super::{Arity, Registry};
use crate::eval::error::{from_units, EvalError};
use crate::eval::value::{Matrix, Scalar, Value};

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    r.register("Thielecoeff", Arity::Exact(2), thielecoeff);
    r.register("Thiele", Arity::Exact(3), thiele);
}

/// `Thielecoeff(vx, vy)`: the continued-fraction coefficients, one per point.
fn thielecoeff(args: &[Value]) -> Result<Value, EvalError> {
    let name = "Thielecoeff";
    let xs = vector(&args[0], name)?;
    let ys = vector(&args[1], name)?;
    if xs.len() != ys.len() {
        return Err(EvalError::Unsupported(format!(
            "{name}: vx and vy differ in length"
        )));
    }
    let dx = one_dim(&xs)?;
    let dy = one_dim(&ys)?;
    let x = reals(&xs, name)?;
    let y = reals(&ys, name)?;
    let n = x.len();

    let mut sorted = x.clone();
    sorted.sort_by(f64::total_cmp);
    if sorted.windows(2).any(|w| w[0] == w[1]) {
        return Err(EvalError::Unsupported(format!(
            "{name} needs distinct x values"
        )));
    }

    let dim_odd = dx.checked_div(dy).map_err(from_units)?;
    // Rolling levels of the recurrence: `prev2` is R_{k-2}, `prev1` is R_{k-1}, each indexed by the
    // first point of its window. R_{-1} is zero.
    let mut coeffs = vec![y[0]];
    let mut prev2 = vec![0.0; n + 1];
    let mut prev1 = y.clone();
    for k in 1..n {
        let len = n - k;
        let mut next = Vec::with_capacity(len);
        for i in 0..len {
            let denom = prev1[i + 1] - prev1[i];
            if denom == 0.0 {
                return Err(EvalError::Unsupported(format!(
                    "{name}: a zero denominator (two neighbouring points have equal y); reorder the points"
                )));
            }
            let r = (x[i + k] - x[i]) / denom + prev2[i + 1];
            if !r.is_finite() {
                return Err(EvalError::NotFinite);
            }
            next.push(r);
        }
        let a = next[0] - prev2[0];
        if !a.is_finite() {
            return Err(EvalError::NotFinite);
        }
        coeffs.push(a);
        prev2 = prev1;
        prev1 = next;
    }

    let data = coeffs
        .iter()
        .enumerate()
        .map(|(k, &a)| Scalar::real(a, if k % 2 == 0 { dy } else { dim_odd }))
        .collect();
    Ok(Value::Matrix(Matrix::new(n, 1, data)?))
}

/// `Thiele(vx, coeff, x)`: the continued fraction at `x`.
fn thiele(args: &[Value]) -> Result<Value, EvalError> {
    let name = "Thiele";
    let xs = vector(&args[0], name)?;
    let cs = vector(&args[1], name)?;
    if xs.len() != cs.len() {
        return Err(EvalError::Unsupported(format!(
            "{name}: vx and coeff differ in length"
        )));
    }
    let (x, xdim) = real_scalar(&args[2], name)?;
    let dx = one_dim(&xs)?;
    if xdim != dx {
        return Err(EvalError::DimensionMismatch {
            left: dx,
            right: xdim,
        });
    }
    let vx = reals(&xs, name)?;
    let g = reals(&cs, name)?;
    let n = vx.len();
    let mut t = g[n - 1];
    for k in (0..n - 1).rev() {
        if t == 0.0 {
            return Err(EvalError::Unsupported(format!(
                "{name}: a zero denominator in the continued fraction at this x"
            )));
        }
        t = g[k] + (x - vx[k]) / t;
        if !t.is_finite() {
            return Err(EvalError::NotFinite);
        }
    }
    real_result(t, cs[0].dim)
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::{col, num, values};
    use super::*;

    fn coeff(xs: &[f64], ys: &[f64]) -> Result<Value, EvalError> {
        thielecoeff(&[col(xs), col(ys)])
    }

    #[test]
    fn coefficients_match_the_help_example() {
        // Example: Thiele Interpolation, "Order of Points": X = [1 2 3 4 0], Y = [1 3/5 2/5 5/17 1].
        let x = [1.0, 2.0, 3.0, 4.0, 0.0];
        let y = [1.0, 3.0 / 5.0, 2.0 / 5.0, 5.0 / 17.0, 1.0];
        let g = values(&coeff(&x, &y).expect("Thielecoeff"));
        crate::eval::builtins::data_util::testing::assert_all_close(
            &g,
            &[1.0, -2.5, -1.2, 17.5, 0.2],
            1e-9,
        );
    }

    #[test]
    fn interpolant_passes_through_the_data_points() {
        let x = [1.0, 2.0, 3.0, 4.0, 0.0];
        let y = [1.0, 3.0 / 5.0, 2.0 / 5.0, 5.0 / 17.0, 1.0];
        let g = coeff(&x, &y).expect("Thielecoeff");
        for (xi, yi) in x.iter().zip(&y) {
            let v = thiele(&[col(&x), g.clone(), num(*xi)]).expect("Thiele");
            assert!((values(&v)[0] - yi).abs() < 1e-9, "at {xi}");
        }
    }

    #[test]
    fn adjacent_equal_y_values_are_refused() {
        // The help's first attempt, Y = [1 1 3/5 2/5 5/17]: the algorithm fails there.
        let r = coeff(
            &[0.0, 1.0, 2.0, 3.0, 4.0],
            &[1.0, 1.0, 0.6, 0.4, 5.0 / 17.0],
        );
        assert!(matches!(r, Err(EvalError::Unsupported(_))));
    }

    #[test]
    fn repeated_x_is_refused() {
        assert!(matches!(
            coeff(&[1.0, 1.0, 2.0], &[1.0, 2.0, 3.0]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn length_mismatch_is_refused() {
        assert!(matches!(
            coeff(&[1.0, 2.0], &[1.0]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn one_point_is_a_constant() {
        let g = coeff(&[3.0], &[7.0]).expect("Thielecoeff");
        assert_eq!(values(&g), vec![7.0]);
        let v = thiele(&[col(&[3.0]), g, num(100.0)]).expect("Thiele");
        assert_eq!(values(&v), vec![7.0]);
    }

    #[test]
    fn boiling_point_regression_matches_the_help() {
        // Example: Thiele Interpolation and Regression. Pressure in bar, temperature in deg C.
        let p = [
            0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0, 150.0, 200.0, 220.0,
        ];
        let t = [
            45.81, 81.32, 99.61, 127.41, 151.84, 179.89, 223.96, 263.94, 311.0, 342.16, 365.75,
            373.71,
        ];
        let g = coeff(&p, &t).expect("Thielecoeff");
        // The help gives TI(75 bar) = 563.692 K and TI(125 bar) = 600.963 K.
        let at75 = values(&thiele(&[col(&p), g.clone(), num(75.0)]).expect("Thiele"))[0];
        let at125 = values(&thiele(&[col(&p), g.clone(), num(125.0)]).expect("Thiele"))[0];
        assert!((at75 + 273.15 - 563.692).abs() < 0.01, "TI(75) = {at75}");
        assert!((at125 + 273.15 - 600.963).abs() < 0.01, "TI(125) = {at125}");
        // The help gives TI(0.771 bar) = 92.52 deg C.
        let mexico = values(&thiele(&[col(&p), g, num(0.771)]).expect("Thiele"))[0];
        assert!((mexico - 92.52).abs() < 0.01, "TI(0.771) = {mexico}");
    }

    #[test]
    fn thiele_refuses_mismatched_lengths() {
        let g = coeff(&[0.0, 1.0], &[0.0, 2.0]).expect("Thielecoeff");
        assert!(matches!(
            thiele(&[col(&[0.0, 1.0, 2.0]), g, num(0.5)]),
            Err(EvalError::Unsupported(_))
        ));
    }
}

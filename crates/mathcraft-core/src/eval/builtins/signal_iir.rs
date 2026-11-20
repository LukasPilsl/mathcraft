//! IIR filter design: the analog prototypes `butter(n)` and `cheby1(n, e)`, and the digital lowpass
//! and highpass designs `iirlow(C, f)` and `iirhigh(C, f)` (Mathcad Prime help: "IIR Filter
//! Coefficients" and "IIR Filters"; the example "IIR Filter Design" gives the reference values).
//!
//! Layout (the help's cascade form, the same column pairs that `gain` and `response` read): a
//! matrix with 3 rows and two columns per section, numerator then denominator. Digital rows are the
//! coefficients of `z^-0, z^-1, z^-2`. A first-order section has zeros in row 2. The analog
//! prototype uses the same shape with rows holding the powers `s^0, s^1, s^2`; the help does not
//! give the analog layout, so this is this implementation's own and is accepted only by `iirlow` and
//! `iirhigh`.
//!
//! Method: the prototype has cutoff 1 rad/s and DC gain 1 (Butterworth), or DC gain `1/sqrt(1+e^2)`
//! for even-order Chebyshev I (the ripple's lower edge), which is put in the first section. Poles:
//! Butterworth `-sin t + j cos t`, Chebyshev I `-sinh(a) sin t + j cosh(a) cos t` with
//! `a = asinh(1/e)/n`, `t = (2k-1) pi / (2n)`. Sections go in order of `k` (the pole nearest the
//! imaginary axis first), a real pole (odd order) last. For the digital filter the cutoff is
//! prewarped, `W = 2 tan(pi f)` with `f` in cycles per sample (the help's normalised frequency), the
//! analog variable is mapped `s -> s/W` (lowpass) or `s -> W/s` (highpass), and the bilinear map
//! `s = 2(1 - z^-1)/(1 + z^-1)` is applied. Each denominator is normalised to a constant term 1.
//!
//! Refused: an order outside 1..200, an epsilon that is not positive, a frequency outside (0, 0.5),
//! a dimensioned argument, and a coefficient array that is not 3 rows of column pairs.

use std::f64::consts::PI;

use num_complex::Complex64;

use super::data_util::{count, grid, grid_result, real_scalar, reals};
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::Value;
use crate::units::Dimension;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    r.register("butter", Arity::Exact(1), butter);
    r.register("cheby1", Arity::Exact(2), cheby1);
    r.register("iirlow", Arity::Exact(2), iirlow);
    r.register("iirhigh", Arity::Exact(2), iirhigh);
}

/// One section: numerator and denominator coefficients in ascending powers, length 3.
#[derive(Clone, Copy, Debug)]
struct Section {
    num: [f64; 3],
    den: [f64; 3],
}

/// A complex-conjugate pole pair with DC gain 1: `(s - p)(s - conj p) = s^2 - 2 Re(p) s + |p|^2`.
fn pair_section(p: Complex64) -> Section {
    let q = p.norm_sqr();
    Section {
        num: [q, 0.0, 0.0],
        den: [q, -2.0 * p.re, 1.0],
    }
}

/// A real pole `p` with DC gain 1: `s - p`.
fn real_section(p: f64) -> Section {
    Section {
        num: [-p, 0.0, 0.0],
        den: [-p, 1.0, 0.0],
    }
}

fn check_order(n: usize, name: &str) -> Result<(), EvalError> {
    if (1..=200).contains(&n) {
        Ok(())
    } else {
        Err(EvalError::Unsupported(format!(
            "{name} needs an order from 1 to 200"
        )))
    }
}

/// The Butterworth prototype: poles `-sin t + j cos t`, `t = (2k-1) pi / (2n)`.
fn butter_prototype(n: usize) -> Vec<Section> {
    let mut out = Vec::with_capacity(n.div_ceil(2));
    for k in 1..=n / 2 {
        let t = PI * (2 * k - 1) as f64 / (2 * n) as f64;
        out.push(pair_section(Complex64::new(-t.sin(), t.cos())));
    }
    if !n.is_multiple_of(2) {
        out.push(real_section(-1.0));
    }
    out
}

/// The Chebyshev Type I prototype with ripple `e`: poles on an ellipse, the gain `1/sqrt(1+e^2)` at
/// DC for even order (put in the first section).
fn cheby1_prototype(n: usize, e: f64) -> Vec<Section> {
    let a = (1.0 / e).asinh() / n as f64;
    let (sh, ch) = (a.sinh(), a.cosh());
    let mut out = Vec::with_capacity(n.div_ceil(2));
    for k in 1..=n / 2 {
        let t = PI * (2 * k - 1) as f64 / (2 * n) as f64;
        out.push(pair_section(Complex64::new(-sh * t.sin(), ch * t.cos())));
    }
    if !n.is_multiple_of(2) {
        out.push(real_section(-sh));
    }
    if n.is_multiple_of(2) {
        if let Some(first) = out.first_mut() {
            let gain = 1.0 / (1.0 + e * e).sqrt();
            for c in &mut first.num {
                *c *= gain;
            }
        }
    }
    out
}

/// Product of two polynomials given by ascending coefficients.
fn poly_mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

/// Bilinear design of one analog section of degree `d` (1 or 2), with prewarped cutoff `w`.
/// Returns the digital numerator and denominator (ascending in `z^-1`, normalised so the constant
/// term of the denominator is 1), zero-padded to length 3.
fn bilinear(section: &Section, d: usize, w: f64, highpass: bool) -> ([f64; 3], [f64; 3]) {
    // Scaled analog coefficients: lowpass s -> s/w, highpass s -> w/s, both multiplied through by
    // s^d so that they stay polynomials of degree d.
    let map = |c: &[f64; 3]| -> Vec<f64> {
        (0..=d)
            .map(|j| {
                if highpass {
                    c[d - j] * w.powi((d - j) as i32)
                } else {
                    c[j] * w.powi((d - j) as i32)
                }
            })
            .collect()
    };
    let m = map(&section.num);
    let l = map(&section.den);
    // s^j (1 + x)^d with s = 2 (1 - x) / (1 + x), i.e. (2(1-x))^j (1+x)^(d-j).
    let term = |j: usize| -> Vec<f64> {
        let mut p = vec![2f64.powi(j as i32)];
        for _ in 0..j {
            p = poly_mul(&p, &[1.0, -1.0]);
        }
        for _ in 0..(d - j) {
            p = poly_mul(&p, &[1.0, 1.0]);
        }
        p
    };
    let mut num = vec![0.0; d + 1];
    let mut den = vec![0.0; d + 1];
    for j in 0..=d {
        let t = term(j);
        for (k, tk) in t.iter().enumerate() {
            num[k] += m[j] * tk;
            den[k] += l[j] * tk;
        }
    }
    let d0 = den[0];
    let mut b = [0.0; 3];
    let mut a = [0.0; 3];
    for k in 0..=d {
        b[k] = num[k] / d0;
        a[k] = den[k] / d0;
    }
    (b, a)
}

/// The analog section's degree: 1 when its second-order coefficients are zero (a padded first-order
/// section), else 2.
fn degree(section: &Section) -> usize {
    if section.num[2] == 0.0 && section.den[2] == 0.0 {
        1
    } else {
        2
    }
}

/// Writes the sections as 3 rows of column pairs (numerator, denominator) per section.
fn cascade_value(sections: &[([f64; 3], [f64; 3])]) -> Result<Value, EvalError> {
    let cols = 2 * sections.len();
    let mut out = vec![Complex64::new(0.0, 0.0); 3 * cols];
    for (s, (b, a)) in sections.iter().enumerate() {
        for r in 0..3 {
            out[r * cols + 2 * s] = Complex64::new(b[r], 0.0);
            out[r * cols + 2 * s + 1] = Complex64::new(a[r], 0.0);
        }
    }
    grid_result(3, cols, &out, Dimension::dimensionless())
}

/// Reads an analog prototype (3 rows, column pairs, dimensionless) into sections.
fn prototype_sections(v: &Value, name: &str) -> Result<Vec<Section>, EvalError> {
    let (rows, cols, data) = grid(v, name)?;
    for s in &data {
        ops::require_dimensionless(s.dim)?;
    }
    if rows != 3 || cols < 2 || !cols.is_multiple_of(2) {
        return Err(EvalError::Unsupported(format!(
            "{name} needs the coefficient array of an analog filter: 3 rows and column pairs"
        )));
    }
    let x = reals(&data, name)?;
    let mut out = Vec::with_capacity(cols / 2);
    for pair in 0..cols / 2 {
        let mut num = [0.0; 3];
        let mut den = [0.0; 3];
        for r in 0..3 {
            num[r] = x[r * cols + 2 * pair];
            den[r] = x[r * cols + 2 * pair + 1];
        }
        if den[0] == 0.0 || den.iter().chain(num.iter()).any(|c| !c.is_finite()) {
            return Err(EvalError::Unsupported(format!(
                "{name}: a section's denominator needs a finite, non-zero constant term"
            )));
        }
        out.push(Section { num, den });
    }
    Ok(out)
}

/// The digital filter from an analog prototype at cutoff `f` (cycles per sample, 0 < f < 0.5).
fn digitise(v: &Value, f: f64, highpass: bool, name: &str) -> Result<Value, EvalError> {
    if !(f > 0.0 && f < 0.5) {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a cutoff frequency between 0 and 0.5"
        )));
    }
    let secs = prototype_sections(v, name)?;
    let w = 2.0 * (PI * f).tan();
    let out: Vec<([f64; 3], [f64; 3])> = secs
        .iter()
        .map(|s| bilinear(s, degree(s), w, highpass))
        .collect();
    cascade_value(&out)
}

fn butter(args: &[Value]) -> Result<Value, EvalError> {
    let n = count(&args[0], "butter")?;
    check_order(n, "butter")?;
    prototype_value(&butter_prototype(n))
}

fn cheby1(args: &[Value]) -> Result<Value, EvalError> {
    let n = count(&args[0], "cheby1")?;
    check_order(n, "cheby1")?;
    let (e, dim) = real_scalar(&args[1], "cheby1")?;
    ops::require_dimensionless(dim)?;
    if !(e > 0.0 && e.is_finite()) {
        return Err(EvalError::Unsupported(
            "cheby1 needs a positive ripple parameter".to_string(),
        ));
    }
    prototype_value(&cheby1_prototype(n, e))
}

/// The analog prototype as a cascade array (rows are the powers of `s`).
fn prototype_value(secs: &[Section]) -> Result<Value, EvalError> {
    let pairs: Vec<([f64; 3], [f64; 3])> = secs.iter().map(|s| (s.num, s.den)).collect();
    cascade_value(&pairs)
}

fn iirlow(args: &[Value]) -> Result<Value, EvalError> {
    let (f, dim) = real_scalar(&args[1], "iirlow")?;
    ops::require_dimensionless(dim)?;
    digitise(&args[0], f, false, "iirlow")
}

fn iirhigh(args: &[Value]) -> Result<Value, EvalError> {
    let (f, dim) = real_scalar(&args[1], "iirhigh")?;
    ops::require_dimensionless(dim)?;
    digitise(&args[0], f, true, "iirhigh")
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;
    use super::*;
    use crate::eval::value::{Matrix, Scalar};

    fn num(x: f64) -> Value {
        Value::Scalar(Scalar::new(Complex64::new(x, 0.0), dimless()))
    }

    /// Values of a matrix, row-major.
    fn grid_values(v: &Value) -> (usize, usize, Vec<f64>) {
        let Value::Matrix(m) = v else {
            panic!("expected a matrix");
        };
        let xs = m.data().iter().map(|s| s.value.re).collect();
        (m.rows(), m.cols(), xs)
    }

    /// `|H(e^{i 2 pi f})|` of a digital cascade in the layout above.
    fn magnitude(v: &Value, f: f64) -> f64 {
        let (rows, cols, x) = grid_values(v);
        let z = Complex64::from_polar(1.0, 2.0 * PI * f);
        let mut h = Complex64::new(1.0, 0.0);
        for s in 0..cols / 2 {
            let poly = |col: usize| -> Complex64 {
                (0..rows)
                    .map(|r| Complex64::new(x[r * cols + col], 0.0) * z.powi(-(r as i32)))
                    .sum()
            };
            h *= poly(2 * s) / poly(2 * s + 1);
        }
        h.norm()
    }

    fn design(name: &str, n: usize, e: Option<f64>, f: f64, high: bool) -> Value {
        let proto = match (name, e) {
            ("butter", _) => butter(&[num(n as f64)]).expect("butter"),
            _ => cheby1(&[num(n as f64), num(e.expect("ripple"))]).expect("cheby1"),
        };
        if high {
            iirhigh(&[proto, num(f)]).expect("iirhigh")
        } else {
            iirlow(&[proto, num(f)]).expect("iirlow")
        }
    }

    #[test]
    fn cheby1_matches_the_help_example_coefficients() {
        // Help example "IIR Filter Design": A := iirlow(cheby1(8, 0.5), 0.25), printed to 3 decimals.
        let a = design("cheby1", 8, Some(0.5), 0.25, false);
        let (rows, cols, x) = grid_values(&a);
        assert_eq!((rows, cols), (3, 8));
        let want = [
            // rows: b0 b1 b2 for each section pair, a0 a1 a2 (printed values).
            [0.431, 1.0, 0.376, 1.0, 0.208, 1.0, 0.05, 1.0],
            [0.862, -0.005, 0.752, -0.286, 0.416, -0.801, 0.099, -1.302],
            [0.431, 0.931, 0.376, 0.791, 0.208, 0.633, 0.05, 0.501],
        ];
        // The printed b1 is 2 b0 (the (1 + z^-1)^2 numerator), and the a0 column is 1.
        for r in 0..3 {
            for c in 0..8 {
                let got = x[r * cols + c];
                let w = want[r][c];
                // Printed values are rounded to 3 decimals (2 for 0.05 and 0.1 forms).
                assert!((got - w).abs() < 1.5e-3, "row {r} col {c}: {got} vs {w}");
            }
        }
    }

    #[test]
    fn butterworth_is_flat_at_dc_and_three_db_down_at_the_cutoff() {
        for n in 1..=7 {
            let v = design("butter", n, None, 0.2, false);
            assert!((magnitude(&v, 0.0) - 1.0).abs() < 1e-12, "dc n={n}");
            let at = magnitude(&v, 0.2).powi(2);
            assert!((at - 0.5).abs() < 1e-9, "cutoff power n={n}: {at}");
        }
    }

    #[test]
    fn chebyshev_passband_edge_is_one_over_root_one_plus_e_squared() {
        for n in [3usize, 4, 8] {
            let e = 0.5;
            let v = design("cheby1", n, Some(e), 0.25, false);
            let at = magnitude(&v, 0.25).powi(2);
            assert!((at - 1.0 / (1.0 + e * e)).abs() < 1e-9, "n={n}: {at}");
            if !n.is_multiple_of(2) {
                assert!((magnitude(&v, 0.0) - 1.0).abs() < 1e-12, "odd dc n={n}");
            } else {
                assert!((magnitude(&v, 0.0) - 1.0 / (1.0 + e * e).sqrt()).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn highpass_mirrors_the_lowpass_about_the_nyquist_frequency() {
        for n in [2usize, 3, 5] {
            let v = design("butter", n, None, 0.2, true);
            assert!((magnitude(&v, 0.5) - 1.0).abs() < 1e-12, "nyquist n={n}");
            assert!(magnitude(&v, 0.0) < 1e-12, "dc n={n}");
            assert!(
                (magnitude(&v, 0.2).powi(2) - 0.5).abs() < 1e-9,
                "cutoff n={n}"
            );
        }
    }

    #[test]
    fn refuses_bad_orders_epsilons_and_cutoffs() {
        assert!(butter(&[num(0.0)]).is_err());
        assert!(butter(&[num(201.0)]).is_err());
        assert!(cheby1(&[num(4.0), num(0.0)]).is_err());
        let proto = butter(&[num(2.0)]).expect("butter");
        assert!(iirlow(&[proto.clone(), num(0.5)]).is_err());
        assert!(iirlow(&[proto.clone(), num(0.0)]).is_err());
        let bad = Value::Matrix(
            Matrix::new(
                2,
                2,
                vec![Scalar::new(Complex64::new(1.0, 0.0), dimless()); 4],
            )
            .expect("m"),
        );
        assert!(iirlow(&[bad, num(0.2)]).is_err());
    }
}

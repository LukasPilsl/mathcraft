//! Hartley and Hilbert transforms (Mathcad Prime help: "Additional Transforms").
//!
//! - `dht(x)_n = (1/N) sum_k x_k cas(2 pi n k / N)`, with `cas = cos + sin` (the help's defining
//!   sum, checked against its example). `dht` is the real part minus the imaginary part of the
//!   unscaled DFT, divided by `N`.
//! - `idht(v)_k = sum_n v_n cas(2 pi n k / N)`: the unscaled inverse, so `idht(dht(x)) = x`.
//! - `hilbert(v)`: the imaginary part of the analytic signal. It multiplies the DFT by `-j` on
//!   `0 < k < N/2` and by `+j` on `N/2 < k < N` (the help's Hilbert transformer), drops the DC and
//!   Nyquist terms (they have no real Hilbert pair), and returns the real part of the inverse. For a
//!   bin-centred cosine it returns the sine: `hilbert(cos(2 pi k n / N)) = sin(2 pi k n / N)`.
//!
//! Only vectors are accepted (the help's "vector or matrix" for `dht` has no defined matrix form
//! here, so a matrix is refused). Arguments must be real. Cost: one FFT per call (`FFT_CLASS`).

use num_complex::Complex64;

use super::data_util::{column, finite, one_dim, vector};
use super::signal_fft::transform;
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::Value;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    let one = Arity::Exact(1);
    r.register("dht", one, dht);
    r.register("idht", one, idht);
    r.register("hilbert", one, hilbert);
}

/// The real elements of a vector argument and their dimension.
fn real_vector(v: &Value, name: &str) -> Result<(Vec<f64>, crate::units::Dimension), EvalError> {
    let xs = vector(v, name)?;
    let dim = one_dim(&xs)?;
    let re = xs
        .iter()
        .map(|s| {
            let z = finite(s.value)?;
            if z.im != 0.0 {
                return Err(EvalError::Unsupported(format!("{name} needs real data")));
            }
            Ok(z.re)
        })
        .collect::<Result<Vec<f64>, EvalError>>()?;
    Ok((re, dim))
}

/// The unscaled DFT (sign -) of a real vector.
fn real_dft(x: &[f64]) -> Vec<Complex64> {
    let z: Vec<Complex64> = x.iter().map(|&v| Complex64::new(v, 0.0)).collect();
    transform(&z, -1.0)
}

/// `cas` sum of the unscaled DFT: `sum_p x_p cas(2 pi p u / N)` equals `Re X_u - Im X_u`, since
/// `X_u = sum_p x_p (cos - i sin)`.
fn cas_sums(spectrum: &[Complex64]) -> Vec<f64> {
    spectrum.iter().map(|z| z.re - z.im).collect()
}

fn dht(args: &[Value]) -> Result<Value, EvalError> {
    let (x, dim) = real_vector(&args[0], "dht")?;
    let n = x.len();
    if n == 0 {
        return Err(EvalError::Unsupported(
            "dht needs at least one element".into(),
        ));
    }
    let scale = 1.0 / n as f64;
    let out: Vec<Complex64> = cas_sums(&real_dft(&x))
        .into_iter()
        .map(|v| Complex64::new(v * scale, 0.0))
        .collect();
    column(&out, dim)
}

fn idht(args: &[Value]) -> Result<Value, EvalError> {
    let (v, dim) = real_vector(&args[0], "idht")?;
    if v.is_empty() {
        return Err(EvalError::Unsupported(
            "idht needs at least one element".into(),
        ));
    }
    let out: Vec<Complex64> = cas_sums(&real_dft(&v))
        .into_iter()
        .map(|x| Complex64::new(x, 0.0))
        .collect();
    column(&out, dim)
}

fn hilbert(args: &[Value]) -> Result<Value, EvalError> {
    let (x, dim) = real_vector(&args[0], "hilbert")?;
    let n = x.len();
    if n == 0 {
        return Err(EvalError::Unsupported(
            "hilbert needs at least one element".into(),
        ));
    }
    let mut spectrum = real_dft(&x);
    let half = n / 2;
    for (k, z) in spectrum.iter_mut().enumerate() {
        *z = if k == 0 || (n % 2 == 0 && k == half) {
            Complex64::new(0.0, 0.0)
        } else if k < n.div_ceil(2) {
            // -j * X_k on the positive frequencies.
            Complex64::new(z.im, -z.re)
        } else {
            // +j * X_k on the negative frequencies.
            Complex64::new(-z.im, z.re)
        };
    }
    // Inverse DFT (sign +, scale 1/N); the real part is the Hilbert transform.
    let scale = 1.0 / n as f64;
    let back = transform(&spectrum, 1.0);
    let out: Vec<Complex64> = back
        .into_iter()
        .map(|z| Complex64::new(z.re * scale, 0.0))
        .collect();
    column(&out, dim)
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn dht_matches_the_help_sum() {
        let x = [1.0, -2.0, 0.5, 3.0, 0.25];
        let n = x.len();
        let got = values(&dht(&[col(&x)]).expect("dht"));
        for (m, g) in got.iter().enumerate() {
            let want: f64 = (0..n)
                .map(|k| {
                    let t = 2.0 * PI * (m * k) as f64 / n as f64;
                    x[k] * (t.cos() + t.sin())
                })
                .sum::<f64>()
                / n as f64;
            assert!((g - want).abs() < 1e-12, "m={m}: {g} vs {want}");
        }
    }

    #[test]
    fn idht_inverts_dht() {
        let x = [0.3, 1.0, -0.7, 2.0, 0.0, 0.5, -1.5];
        let back = values(&idht(&[dht(&[col(&x)]).expect("dht")]).expect("idht"));
        assert_all_close(&back, &x, 1e-12);
    }

    #[test]
    fn hilbert_of_a_bin_centred_cosine_is_the_sine() {
        let n = 32;
        for k in [1usize, 3, 5] {
            let x: Vec<f64> = (0..n)
                .map(|i| (2.0 * PI * (k * i) as f64 / n as f64).cos())
                .collect();
            let want: Vec<f64> = (0..n)
                .map(|i| (2.0 * PI * (k * i) as f64 / n as f64).sin())
                .collect();
            let got = values(&hilbert(&[col(&x)]).expect("hilbert"));
            assert_all_close(&got, &want, 1e-12);
        }
    }

    #[test]
    fn hilbert_of_odd_length_is_exact_too() {
        let n = 9;
        let x: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 2.0 * i as f64 / n as f64).cos())
            .collect();
        let want: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 2.0 * i as f64 / n as f64).sin())
            .collect();
        let got = values(&hilbert(&[col(&x)]).expect("hilbert"));
        assert_all_close(&got, &want, 1e-12);
    }

    #[test]
    fn matrices_and_complex_input_are_refused() {
        let m = Value::Matrix(
            crate::eval::value::Matrix::new(
                2,
                2,
                vec![crate::eval::value::Scalar::new(Complex64::new(1.0, 0.0), dimless()); 4],
            )
            .expect("matrix"),
        );
        assert!(dht(std::slice::from_ref(&m)).is_err());
        assert!(hilbert(std::slice::from_ref(&m)).is_err());
    }
}

//! Discrete Fourier transforms (Mathcad Prime help: "Discrete Fourier Transform of Data").
//!
//! Current functions: `dft`, `idft` (complex, any size, vector or matrix), `dftr`, `idftr` (real
//! input, half spectrum, `L = floor(r/2) + 1`). Deprecated functions, still named in the help, with
//! the help's table of signs and scalings (`N` is the number of elements, the help's `m n`, and
//! for a vector `N` is its length):
//!
//! | name  | exponent sign | scale      | input                         |
//! |-------|---------------|------------|-------------------------------|
//! | dft   | -             | 1          | complex, vector or matrix     |
//! | idft  | +             | 1/N        | complex, vector or matrix     |
//! | cfft  | +             | 1/sqrt(N)  | complex, vector or matrix     |
//! | icfft | -             | 1/sqrt(N)  | complex, vector or matrix     |
//! | CFFT  | -             | 1/N        | complex, vector or matrix     |
//! | ICFFT | +             | 1          | complex, vector or matrix     |
//! | fft   | +             | 1/sqrt(n)  | real, power-of-2 vector, half  |
//! | ifft  | -             | 1/sqrt(n)  | half vector (n = 2(L-1))       |
//! | FFT   | -             | 1/n        | real, power-of-2 vector, half  |
//! | IFFT  | +             | 1          | half vector (n = 2(L-1))       |
//!
//! `fft` returns the `n/2 + 1` values of the half spectrum and `ifft` takes that half and returns the
//! `n` real values; `ifft(fft(v)) = v`. The matrix forms of `fft` are not implemented (the help
//! restricts `fft` to vectors). Exponents: `dft(v)_u = sum_p v_p exp(sign 2 pi i p u / N)`.
//!
//! Algorithm: radix-2 for power-of-two lengths; Bluestein's chirp-z method (a power-of-two
//! convolution of length at least `2n - 1`) for other lengths, so every length costs about `n log n`.
//!
//! Units: the transforms keep the dimension of their input (the help requires compatible units).

use std::f64::consts::PI;

use num_complex::Complex64;

use super::data_util::{column, complexes, finite, grid, grid_result, one_dim};
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::Value;
use crate::units::Dimension;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    let one = Arity::Exact(1);
    r.register("dft", one, dft);
    r.register("idft", one, idft);
    r.register("dftr", one, dftr);
    r.register("idftr", one, idftr);
    r.register("cfft", one, cfft);
    r.register("icfft", one, icfft);
    r.register("CFFT", one, cfft_upper);
    r.register("ICFFT", one, icfft_upper);
    r.register("fft", one, fft);
    r.register("ifft", one, ifft);
    r.register("FFT", one, fft_upper);
    r.register("IFFT", one, ifft_upper);
}

/// The scalar a transform multiplies its sum by.
#[derive(Clone, Copy)]
enum Scale {
    One,
    /// `1 / N`.
    OverN,
    /// `1 / sqrt(N)`.
    OverRootN,
}

impl Scale {
    fn factor(self, n: usize) -> f64 {
        match self {
            Scale::One => 1.0,
            Scale::OverN => 1.0 / n as f64,
            Scale::OverRootN => 1.0 / (n as f64).sqrt(),
        }
    }
}

// ---- the 1-D kernel ---------------------------------------------------------------------------

/// Unscaled transform of any length: `out[u] = sum_p x[p] exp(sign 2 pi i p u / n)`.
pub(super) fn transform(x: &[Complex64], sign: f64) -> Vec<Complex64> {
    let n = x.len();
    if n <= 1 {
        return x.to_vec();
    }
    if n.is_power_of_two() {
        let mut y = x.to_vec();
        radix2(&mut y, sign);
        return y;
    }
    bluestein(x, sign)
}

/// In-place iterative radix-2 transform of a power-of-two length.
fn radix2(a: &mut [Complex64], sign: f64) {
    let n = a.len();
    if n < 2 {
        return;
    }
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let twiddle: Vec<Complex64> = (0..n / 2)
        .map(|k| Complex64::from_polar(1.0, sign * 2.0 * PI * k as f64 / n as f64))
        .collect();
    let mut len = 2;
    while len <= n {
        let step = n / len;
        let half = len / 2;
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let w = twiddle[k * step];
                let u = a[start + k];
                let v = a[start + k + half] * w;
                a[start + k] = u + v;
                a[start + k + half] = u - v;
            }
        }
        len <<= 1;
    }
}

/// Bluestein's chirp-z transform: `jk = (j^2 + k^2 - (k - j)^2) / 2` turns the transform into a
/// convolution of `x_j c_j` with `conj(c_t)`, where `c_t = exp(sign i pi t^2 / n)`.
fn bluestein(x: &[Complex64], sign: f64) -> Vec<Complex64> {
    let n = x.len();
    let m = (2 * n - 1).next_power_of_two();
    // c_k depends on k^2 modulo 2n only, which keeps the argument small.
    let period = 2 * n as u64;
    let chirp: Vec<Complex64> = (0..n)
        .map(|k| {
            let e = ((k as u64 * k as u64) % period) as f64;
            Complex64::from_polar(1.0, sign * PI * e / n as f64)
        })
        .collect();
    let mut a = vec![Complex64::new(0.0, 0.0); m];
    for j in 0..n {
        a[j] = x[j] * chirp[j];
    }
    let mut b = vec![Complex64::new(0.0, 0.0); m];
    b[0] = chirp[0].conj();
    for k in 1..n {
        let c = chirp[k].conj();
        b[k] = c;
        b[m - k] = c;
    }
    radix2(&mut a, -1.0);
    radix2(&mut b, -1.0);
    for (p, q) in a.iter_mut().zip(&b) {
        *p *= q;
    }
    radix2(&mut a, 1.0);
    let scale = 1.0 / m as f64;
    (0..n).map(|k| chirp[k] * a[k] * scale).collect()
}

/// Transform of a `rows x cols` row-major grid: every column, then every row (separable).
fn transform_grid(rows: usize, cols: usize, z: &[Complex64], sign: f64) -> Vec<Complex64> {
    let mut out = z.to_vec();
    if rows > 1 {
        for c in 0..cols {
            let col: Vec<Complex64> = (0..rows).map(|r| out[r * cols + c]).collect();
            let t = transform(&col, sign);
            for (r, value) in t.into_iter().enumerate() {
                out[r * cols + c] = value;
            }
        }
    }
    if cols > 1 {
        for r in 0..rows {
            let row = out[r * cols..(r + 1) * cols].to_vec();
            let t = transform(&row, sign);
            out[r * cols..(r + 1) * cols].copy_from_slice(&t);
        }
    }
    out
}

// ---- argument shapes --------------------------------------------------------------------------

/// The complex transform of a vector or matrix argument: `(rows, cols, values, dimension)`.
fn complex_grid(
    v: &Value,
    name: &str,
) -> Result<(usize, usize, Vec<Complex64>, Dimension), EvalError> {
    let (rows, cols, data) = grid(v, name)?;
    let dim = one_dim(&data)?;
    Ok((rows, cols, complexes(&data)?, dim))
}

/// Complex transform with the given exponent sign and scale, on a vector or matrix.
fn full(args: &[Value], name: &str, sign: f64, scale: Scale) -> Result<Value, EvalError> {
    let (rows, cols, z, dim) = complex_grid(&args[0], name)?;
    let factor = scale.factor(rows * cols);
    let out: Vec<Complex64> = transform_grid(rows, cols, &z, sign)
        .into_iter()
        .map(|v| v * factor)
        .collect();
    grid_result(rows, cols, &out, dim)
}

/// `dftr` needs a real grid: this returns the real parts (the help says any imaginary part is
/// ignored).
fn real_grid(v: &Value, name: &str) -> Result<(usize, usize, Vec<f64>, Dimension), EvalError> {
    let (rows, cols, data) = grid(v, name)?;
    let dim = one_dim(&data)?;
    let re = data
        .iter()
        .map(|s| finite(s.value).map(|z| z.re))
        .collect::<Result<Vec<f64>, _>>()?;
    Ok((rows, cols, re, dim))
}

/// The real vector argument of `fft` and `FFT`: its values and dimension.
fn power_of_two_vector(v: &Value, name: &str) -> Result<(Vec<Complex64>, Dimension), EvalError> {
    let (rows, cols, z, dim) = complex_grid(v, name)?;
    if cols != 1 {
        return Err(EvalError::Unsupported(format!("{name} needs a vector")));
    }
    if !rows.is_power_of_two() {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a length that is a power of 2"
        )));
    }
    if z.iter().any(|c| c.im != 0.0) {
        return Err(EvalError::Unsupported(format!("{name} needs real data")));
    }
    Ok((z, dim))
}

/// The full spectrum of a real signal of length `n` from its half `z` (`n/2 + 1` values) by
/// conjugate symmetry.
fn hermitian_full(z: &[Complex64], n: usize) -> Vec<Complex64> {
    (0..n)
        .map(|k| if k < z.len() { z[k] } else { z[n - k].conj() })
        .collect()
}

/// Half-spectrum forward transform of a real power-of-two vector (`fft`, `FFT`).
fn real_forward(args: &[Value], name: &str, sign: f64, scale: Scale) -> Result<Value, EvalError> {
    let (z, dim) = power_of_two_vector(&args[0], name)?;
    let n = z.len();
    let factor = scale.factor(n);
    let all = transform(&z, sign);
    let half: Vec<Complex64> = all[..n / 2 + 1].iter().map(|v| v * factor).collect();
    column(&half, dim)
}

/// Inverse of a half spectrum to `2(L-1)` real values (`ifft`, `IFFT`).
fn half_inverse(args: &[Value], name: &str, sign: f64, scale: Scale) -> Result<Value, EvalError> {
    let (rows, cols, z, dim) = complex_grid(&args[0], name)?;
    if cols != 1 {
        return Err(EvalError::Unsupported(format!("{name} needs a vector")));
    }
    // The help: the input has 2^(k-1) + 1 values with k > 1, so at least three.
    if rows < 3 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs at least three values"
        )));
    }
    let n = 2 * (rows - 1);
    if !n.is_power_of_two() {
        return Err(EvalError::Unsupported(format!(
            "{name} needs half a power-of-2 length plus one values"
        )));
    }
    let factor = scale.factor(n);
    let all = transform(&hermitian_full(&z, n), sign);
    let real: Vec<Complex64> = all
        .iter()
        .map(|v| Complex64::new(v.re * factor, 0.0))
        .collect();
    column(&real, dim)
}

// ---- the functions ----------------------------------------------------------------------------

fn dft(args: &[Value]) -> Result<Value, EvalError> {
    full(args, "dft", -1.0, Scale::One)
}

fn idft(args: &[Value]) -> Result<Value, EvalError> {
    full(args, "idft", 1.0, Scale::OverN)
}

fn cfft(args: &[Value]) -> Result<Value, EvalError> {
    full(args, "cfft", 1.0, Scale::OverRootN)
}

fn icfft(args: &[Value]) -> Result<Value, EvalError> {
    full(args, "icfft", -1.0, Scale::OverRootN)
}

fn cfft_upper(args: &[Value]) -> Result<Value, EvalError> {
    full(args, "CFFT", -1.0, Scale::OverN)
}

fn icfft_upper(args: &[Value]) -> Result<Value, EvalError> {
    full(args, "ICFFT", 1.0, Scale::One)
}

fn fft(args: &[Value]) -> Result<Value, EvalError> {
    real_forward(args, "fft", 1.0, Scale::OverRootN)
}

fn ifft(args: &[Value]) -> Result<Value, EvalError> {
    half_inverse(args, "ifft", -1.0, Scale::OverRootN)
}

fn fft_upper(args: &[Value]) -> Result<Value, EvalError> {
    real_forward(args, "FFT", -1.0, Scale::OverN)
}

fn ifft_upper(args: &[Value]) -> Result<Value, EvalError> {
    half_inverse(args, "IFFT", 1.0, Scale::One)
}

/// `dftr(B)`: the first `floor(r/2) + 1` rows (vector) or columns (matrix) of `dft(B)`.
fn dftr(args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, x, dim) = real_grid(&args[0], "dftr")?;
    let z: Vec<Complex64> = x.iter().map(|&r| Complex64::new(r, 0.0)).collect();
    if cols == 1 {
        if rows % 2 != 0 {
            return Err(EvalError::Unsupported(
                "dftr needs an even number of rows".to_string(),
            ));
        }
        let all = transform_grid(rows, 1, &z, -1.0);
        return column(&all[..rows / 2 + 1], dim);
    }
    if cols % 2 != 0 {
        return Err(EvalError::Unsupported(
            "dftr needs an even number of columns".to_string(),
        ));
    }
    let all = transform_grid(rows, cols, &z, -1.0);
    let keep = cols / 2 + 1;
    let mut out = Vec::with_capacity(rows * keep);
    for r in 0..rows {
        out.extend_from_slice(&all[r * cols..r * cols + keep]);
    }
    grid_result(rows, keep, &out, dim)
}

/// `idftr(Z)`: the real inverse of a half spectrum (the rows or columns of `dftr`), with
/// `r = 2 (L - 1)` values along the transformed direction.
fn idftr(args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, z, dim) = complex_grid(&args[0], "idftr")?;
    if cols == 1 {
        if rows < 2 {
            return Err(EvalError::Unsupported(
                "idftr needs at least two values".to_string(),
            ));
        }
        let n = 2 * (rows - 1);
        let all = transform(&hermitian_full(&z, n), 1.0);
        let out: Vec<Complex64> = all
            .iter()
            .map(|v| Complex64::new(v.re / n as f64, 0.0))
            .collect();
        return column(&out, dim);
    }
    if cols < 2 {
        return Err(EvalError::Unsupported(
            "idftr needs at least two columns".to_string(),
        ));
    }
    let keep = cols;
    let c = 2 * (keep - 1);
    let mut full_spec = vec![Complex64::new(0.0, 0.0); rows * c];
    for u in 0..rows {
        for k in 0..c {
            full_spec[u * c + k] = if k < keep {
                z[u * cols + k]
            } else {
                z[((rows - u) % rows) * cols + (c - k)].conj()
            };
        }
    }
    let all = transform_grid(rows, c, &full_spec, 1.0);
    let scale = 1.0 / (rows * c) as f64;
    let out: Vec<Complex64> = all
        .iter()
        .map(|v| Complex64::new(v.re * scale, 0.0))
        .collect();
    grid_result(rows, c, &out, dim)
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;
    use super::*;
    use crate::eval::value::{Matrix, Scalar};

    /// Naive O(n^2) transform, the reference for every length.
    fn naive(x: &[Complex64], sign: f64) -> Vec<Complex64> {
        let n = x.len();
        (0..n)
            .map(|u| {
                (0..n)
                    .map(|p| {
                        let ang = sign * 2.0 * PI * ((p * u) % n) as f64 / n as f64;
                        x[p] * Complex64::from_polar(1.0, ang)
                    })
                    .sum()
            })
            .collect()
    }

    fn signal(n: usize) -> Vec<Complex64> {
        (0..n)
            .map(|k| {
                let t = k as f64;
                Complex64::new(
                    (0.7 * t).sin() + 0.3 * (t * t * 0.01).cos(),
                    0.2 * (1.3 * t).cos(),
                )
            })
            .collect()
    }

    fn cvalue(z: &[Complex64]) -> Value {
        let data = z.iter().map(|&c| Scalar::new(c, dimless())).collect();
        Value::Matrix(Matrix::new(z.len(), 1, data).expect("column"))
    }

    fn max_diff(a: &[Complex64], b: &[Complex64]) -> f64 {
        assert_eq!(a.len(), b.len());
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).norm())
            .fold(0.0, f64::max)
    }

    #[test]
    fn radix2_and_bluestein_match_the_naive_transform() {
        for n in [1, 2, 3, 4, 5, 7, 8, 12, 16, 17, 31, 64, 100] {
            let x = signal(n);
            for sign in [-1.0, 1.0] {
                let got = transform(&x, sign);
                let want = naive(&x, sign);
                assert!(max_diff(&got, &want) < 1e-9 * n as f64, "n={n} sign={sign}");
            }
        }
    }

    #[test]
    fn dft_then_idft_is_the_identity_for_any_length() {
        for n in [1, 2, 3, 6, 10, 15, 16, 33] {
            let x = signal(n);
            let v = cvalue(&x);
            let back = complex_values(&call_dft(idft, &call_dft(dft, &v)));
            assert!(max_diff(&back, &x) < 1e-12, "n={n}");
        }
    }

    fn call_dft(f: fn(&[Value]) -> Result<Value, EvalError>, v: &Value) -> Value {
        f(std::slice::from_ref(v)).expect("transform")
    }

    #[test]
    fn dft_of_a_unit_impulse_is_all_ones_and_of_ones_is_a_spike() {
        let mut impulse = vec![Complex64::new(0.0, 0.0); 9];
        impulse[0] = Complex64::new(1.0, 0.0);
        let d = complex_values(&call_dft(dft, &cvalue(&impulse)));
        assert!(max_diff(&d, &[Complex64::new(1.0, 0.0); 9]) < 1e-12);
        let ones = vec![Complex64::new(1.0, 0.0); 8];
        let d = complex_values(&call_dft(dft, &cvalue(&ones)));
        assert!((d[0] - Complex64::new(8.0, 0.0)).norm() < 1e-12);
        assert!(d[1..].iter().all(|z| z.norm() < 1e-12));
    }

    #[test]
    fn help_sinusoid_peak_is_the_unscaled_dirichlet_value() {
        // Help, "Example: Forward Transform dft": N = 100, d_n = exp(i pi n / 8 * T) with T = 0.5,
        // so the peak at bin 3 has |D_3| = sin(N theta / 2) / sin(theta / 2), theta = pi/16 - 2 pi 3/100.
        let n = 100;
        let t = 0.5;
        let w0 = PI / 8.0;
        let d: Vec<Complex64> = (0..n)
            .map(|k| Complex64::from_polar(1.0, w0 * k as f64 * t))
            .collect();
        let big = complex_values(&call_dft(dft, &cvalue(&d)));
        let theta = w0 * t - 2.0 * PI * 3.0 / n as f64;
        let want = (n as f64 * theta / 2.0).sin() / (theta / 2.0).sin();
        assert!(
            (big[3].norm() - want).abs() < 1e-9,
            "{} vs {want}",
            big[3].norm()
        );
        assert!((want - 97.45).abs() < 0.01);
    }

    #[test]
    fn scalings_follow_the_help_table() {
        let x = signal(8);
        let v = cvalue(&x);
        let raw = naive(&x, -1.0);
        let root = 1.0 / 8f64.sqrt();
        // dft: unscaled, negative exponent.
        assert!(max_diff(&complex_values(&call_dft(dft, &v)), &raw) < 1e-9);
        // CFFT: negative exponent, 1/N.
        let cfft_up = complex_values(&call_dft(cfft_upper, &v));
        assert!(max_diff(&cfft_up, &raw.iter().map(|z| z / 8.0).collect::<Vec<_>>()) < 1e-12);
        // cfft: positive exponent, 1/sqrt(N).
        let pos = naive(&x, 1.0);
        let cf = complex_values(&call_dft(cfft, &v));
        assert!(max_diff(&cf, &pos.iter().map(|z| z * root).collect::<Vec<_>>()) < 1e-9);
        // ICFFT: positive exponent, unscaled.
        let icf = complex_values(&call_dft(icfft_upper, &v));
        assert!(max_diff(&icf, &pos) < 1e-9);
    }

    #[test]
    fn fft_pairs_round_trip_on_power_of_two_vectors() {
        let x: Vec<f64> = signal(16).iter().map(|z| z.re).collect();
        let v = col(&x);
        let half = call_dft(fft, &v);
        assert_eq!(values(&half).len(), 9, "half spectrum of 16 values");
        let back = values(&call_dft(ifft, &half));
        assert_all_close(&back, &x, 1e-12);
        // FFT / IFFT pair with the 1/n scaling on the forward side.
        let half_u = call_dft(fft_upper, &v);
        let back_u = values(&call_dft(ifft_upper, &half_u));
        assert_all_close(&back_u, &x, 1e-12);
    }

    #[test]
    fn fft_scalings_on_a_known_signal() {
        let x = [1.0, 2.0, 0.0, -1.0];
        let all = naive(
            &x.iter()
                .map(|&r| Complex64::new(r, 0.0))
                .collect::<Vec<_>>(),
            1.0,
        );
        let half = complex_values(&call_dft(fft, &col(&x)));
        assert_eq!(half.len(), 3);
        for k in 0..3 {
            assert!((half[k] - all[k] / 2.0).norm() < 1e-12, "k={k}");
        }
        let upper = complex_values(&call_dft(fft_upper, &col(&x)));
        let neg = naive(
            &x.iter()
                .map(|&r| Complex64::new(r, 0.0))
                .collect::<Vec<_>>(),
            -1.0,
        );
        for k in 0..3 {
            assert!((upper[k] - neg[k] / 4.0).norm() < 1e-12, "k={k}");
        }
    }

    #[test]
    fn fft_refuses_lengths_that_are_not_powers_of_two_and_complex_input() {
        assert!(matches!(
            fft(std::slice::from_ref(&col(&[1.0, 2.0, 3.0]))),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            fft(std::slice::from_ref(&cvalue(&[
                Complex64::new(1.0, 1.0),
                Complex64::new(0.0, 0.0)
            ]))),
            Err(EvalError::Unsupported(_))
        ));
        // Four half-spectrum values give n = 6 (not a power of 2).
        assert!(matches!(
            ifft(std::slice::from_ref(&col(&[1.0, 2.0, 3.0, 4.0]))),
            Err(EvalError::Unsupported(_))
        ));
        // Two values give n = 2, but the help needs 2^(k-1) + 1 values with k > 1, so refuse them.
        assert!(matches!(
            ifft(std::slice::from_ref(&col(&[1.0, 2.0]))),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            ifft_upper(std::slice::from_ref(&col(&[1.0, 2.0]))),
            Err(EvalError::Unsupported(_))
        ));
        // Three values give n = 4, the smallest valid length.
        assert!(ifft(std::slice::from_ref(&col(&[1.0, 2.0, 3.0]))).is_ok());
    }

    #[test]
    fn dftr_is_the_first_half_of_dft_and_idftr_inverts_it() {
        let x: Vec<f64> = signal(10).iter().map(|z| z.re).collect();
        let full_spec = complex_values(&call_dft(dft, &col(&x)));
        let half = complex_values(&call_dft(dftr, &col(&x)));
        assert_eq!(half.len(), 6);
        assert!(max_diff(&half, &full_spec[..6]) < 1e-12);
        let back = values(&call_dft(idftr, &cvalue(&half)));
        assert_all_close(&back, &x, 1e-12);
    }

    #[test]
    fn dftr_ignores_the_imaginary_part_as_the_help_says() {
        let x: Vec<f64> = signal(8).iter().map(|z| z.re).collect();
        let spectrum = call_dft(dft, &col(&x));
        assert!(complex_values(&spectrum).iter().any(|z| z.im.abs() > 1e-9));
        let re: Vec<f64> = complex_values(&spectrum).iter().map(|z| z.re).collect();
        let from_complex = complex_values(&call_dft(dftr, &spectrum));
        let from_real = complex_values(&call_dft(dftr, &col(&re)));
        assert_eq!(from_complex.len(), 5);
        assert!(max_diff(&from_complex, &from_real) < 1e-12);
    }

    #[test]
    fn dftr_refuses_odd_lengths() {
        assert!(matches!(
            dftr(std::slice::from_ref(&col(&[1.0, 2.0, 3.0]))),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn matrix_dft_round_trips_and_matches_the_separable_definition() {
        let rows = 3;
        let cols = 4;
        let data: Vec<Complex64> = signal(rows * cols);
        let m = Value::Matrix(
            Matrix::new(
                rows,
                cols,
                data.iter().map(|&c| Scalar::new(c, dimless())).collect(),
            )
            .expect("matrix"),
        );
        let f = call_dft(dft, &m);
        let back = call_dft(idft, &f);
        assert!(max_diff(&complex_values(&back), &data) < 1e-12);
        // Element (0, 0) is the sum of all elements.
        let total: Complex64 = data.iter().sum();
        assert!((complex_values(&f)[0] - total).norm() < 1e-9);
    }

    #[test]
    fn units_are_kept() {
        let s = Dimension::base(0);
        let v = Value::Matrix(
            Matrix::new(
                4,
                1,
                (0..4).map(|k| Scalar::real(f64::from(k), s)).collect(),
            )
            .expect("m"),
        );
        assert_eq!(dim_of(&call_dft(dft, &v)), s);
        assert_eq!(dim_of(&call_dft(fft_upper, &v)), s);
    }
}

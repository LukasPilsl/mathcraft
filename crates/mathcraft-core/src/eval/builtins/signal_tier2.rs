//! Signal windows, FIR filter design, filter application and the phase and detrend utilities
//! (Mathcad Prime help: "Signal Windowing", "FIR Filter Design", "Digital Filtering", "Time Response
//! of Digital Filters", "Zero-Phase Filtering", "Filter Gain", "Linear Detrending", "Quantizing a
//! Signal", "Elementwise Phase").
//!
//! Functions: `blackman`, `hamming`, `hanning`, `nuttall`, `triangular`, `taprect`, `kaiser`,
//! `gaussian`, `costaper`, `cheby` (windows); `lowpass`, `highpass`, `bandpass`, `bandstop` (FIR design);
//! `response`, `fftfilt`, `filtfilt`, `gain` (filtering); `detrend`, `quantize`, `phase`, `phasecor`.
//!
//! Conventions:
//! - a window of length `n` is a column of `n` real values, `k = 0 .. n-1`, symmetric about its
//!   centre (the symmetric form, period `n - 1`). Windows follow the standard published definitions
//!   named in each function's comment, because the help gives the shapes but not every formula;
//! - filter coefficients are dimensionless. A FIR filter is a column vector. An IIR filter (help:
//!   "Filter Gain") is a matrix with column pairs `(numerator, denominator)` in cascade form; each
//!   pair is normalised so that its denominator's constant term is 1;
//! - a filter's output is the causal response with zero initial conditions;
//! - every loop is charged by the cost class of its name in `work_table` (the loops here run inside
//!   one builtin call, so the class is the only charge).

use std::f64::consts::{PI, TAU};

use num_complex::Complex64;

use super::data_util::{
    column, complexes, count, grid, grid_result, one_dim, real_scalar, reals, scalar_result, vector,
};
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

const ZERO: Complex64 = Complex64::new(0.0, 0.0);
const ONE: Complex64 = Complex64::new(1.0, 0.0);

/// Registers the functions of this file.
pub fn register_signal_tier2(r: &mut Registry) {
    r.register("blackman", Arity::Exact(1), blackman);
    r.register("hamming", Arity::Exact(1), hamming);
    r.register("hanning", Arity::Exact(1), hanning);
    r.register("nuttall", Arity::Exact(1), nuttall);
    r.register("triangular", Arity::Exact(1), triangular);
    r.register("taprect", Arity::Exact(1), taprect);
    r.register("kaiser", Arity::Exact(2), kaiser);
    r.register("gaussian", Arity::Exact(2), gaussian);
    r.register("costaper", Arity::Exact(2), costaper);
    r.register("cheby", Arity::Exact(2), cheby);
    r.register("lowpass", Arity::Range(2, 3), lowpass);
    r.register("highpass", Arity::Range(2, 3), highpass);
    r.register("bandpass", Arity::Range(3, 4), bandpass);
    r.register("bandstop", Arity::Range(3, 4), bandstop);
    r.register("response", Arity::Exact(3), response);
    r.register("fftfilt", Arity::Range(2, 3), fftfilt);
    r.register("filtfilt", Arity::Exact(2), filtfilt);
    r.register("gain", Arity::Exact(2), gain);
    r.register("detrend", Arity::Exact(1), detrend);
    r.register("quantize", Arity::Exact(2), quantize);
    r.register("phase", Arity::Exact(1), phase);
    r.register("phasecor", Arity::Exact(1), phasecor);
}

// ---- argument handling ------------------------------------------------------------------------

/// A dimensionless real argument (finite, real).
fn real_param(v: &Value, name: &str) -> Result<f64, EvalError> {
    let (x, dim) = real_scalar(v, name)?;
    ops::require_dimensionless(dim)?;
    Ok(x)
}

/// A window length: an integer of at least 2.
fn window_len(v: &Value, name: &str) -> Result<usize, EvalError> {
    let n = count(v, name)?;
    if n < 2 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a window length of at least 2"
        )));
    }
    Ok(n)
}

/// A real parameter that must lie in `lo..=hi`.
fn ranged(v: &Value, name: &str, what: &str, lo: f64, hi: f64) -> Result<f64, EvalError> {
    let x = real_param(v, name)?;
    if (lo..=hi).contains(&x) {
        Ok(x)
    } else {
        Err(EvalError::Unsupported(format!(
            "{name} needs {what} between {lo} and {hi}"
        )))
    }
}

/// A window as a real column result.
fn window_result(w: Vec<f64>) -> Result<Value, EvalError> {
    let zs: Vec<Complex64> = w.into_iter().map(|x| Complex64::new(x, 0.0)).collect();
    column(&zs, Dimension::dimensionless())
}

/// The FIR filter length of the design functions: an integer greater than 2.
fn fir_len(v: &Value, name: &str) -> Result<usize, EvalError> {
    let n = count(v, name)?;
    if n < 3 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs more than two coefficients"
        )));
    }
    Ok(n)
}

/// A cutoff frequency, a fraction of the sampling frequency between 0 and 0.5.
fn cutoff(v: &Value, name: &str) -> Result<f64, EvalError> {
    ranged(v, name, "a cutoff frequency", 0.0, 0.5)
}

// ---- windows ----------------------------------------------------------------------------------

/// `sum_j (-1)^j a_j cos(j x_k)` with `x_k = 2 pi k / (n - 1)`: the cosine-sum windows. The
/// coefficients are the standard ones named at each caller.
fn cos_sum(n: usize, coeffs: &[f64]) -> Vec<f64> {
    let period = (n - 1) as f64;
    (0..n)
        .map(|k| {
            let x = TAU * k as f64 / period;
            coeffs
                .iter()
                .enumerate()
                .map(|(j, a)| {
                    let sign = if j % 2 == 0 { 1.0 } else { -1.0 };
                    sign * a * (j as f64 * x).cos()
                })
                .sum()
        })
        .collect()
}

/// Hanning (Hann): `0.5 - 0.5 cos(x)`, the standard raised cosine.
const HANNING: [f64; 2] = [0.5, 0.5];
/// Hamming: `0.54 - 0.46 cos(x)`, the standard coefficients of Hamming (1977).
const HAMMING: [f64; 2] = [0.54, 0.46];
/// Blackman: `0.42 - 0.5 cos(x) + 0.08 cos(2x)`, the standard exact-Blackman coefficients.
const BLACKMAN: [f64; 3] = [0.42, 0.5, 0.08];
/// Nuttall: the minimum four-term window with continuous first derivative (Nuttall 1981), the
/// coefficients `a0..a3` = 0.3635819, 0.4891775, 0.1365995, 0.0106411.
const NUTTALL: [f64; 4] = [0.3635819, 0.4891775, 0.1365995, 0.0106411];

/// Triangular window (Bartlett, help "Signal Windowing"): `2k / (n-1)` for `k <= (n-1)/2` and
/// `2 - 2k / (n-1)` after it, so both ends are zero and the peak is 1. Written as `1 - |2k - m| / m`
/// with `m = n - 1`.
fn triangular_values(n: usize) -> Vec<f64> {
    let m = (n - 1) as f64;
    (0..n)
        .map(|k| 1.0 - (2.0 * k as f64 - m).abs() / m)
        .collect()
}

/// Tapered rectangular window (help "Signal Windowing", `taprect`): with `h = floor((n-2)/10)`, the
/// value at distance `j` from the nearer end is `0.5 (1 - cos(pi j / (h+1)))` for `j <= h`, and 1
/// elsewhere. Both ends are zero.
fn taprect_values(n: usize) -> Vec<f64> {
    let h = (n - 2) / 10;
    let ramp = |j: usize| 0.5 * (1.0 - (PI * j as f64 / (h as f64 + 1.0)).cos());
    (0..n)
        .map(|k| {
            let edge = k.min(n - 1 - k);
            if edge <= h {
                ramp(edge)
            } else {
                1.0
            }
        })
        .collect()
}

/// Modified Bessel function of the first kind, order 0, by its power series (all terms positive,
/// so there is no cancellation). Converges for the Kaiser parameters the help allows (at most 20).
fn bessel_i0(x: f64) -> f64 {
    let q = x * x / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..=500u32 {
        term *= q / (k as f64 * k as f64);
        sum += term;
        if term <= sum * 1e-17 {
            break;
        }
    }
    sum
}

fn blackman(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "blackman")?;
    window_result(cos_sum(n, &BLACKMAN))
}

fn hamming(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "hamming")?;
    window_result(cos_sum(n, &HAMMING))
}

fn hanning(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "hanning")?;
    window_result(cos_sum(n, &HANNING))
}

fn nuttall(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "nuttall")?;
    window_result(cos_sum(n, &NUTTALL))
}

fn triangular(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "triangular")?;
    window_result(triangular_values(n))
}

fn taprect(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "taprect")?;
    window_result(taprect_values(n))
}

/// Kaiser window (Kaiser 1966): `I0(b sqrt(1 - t^2)) / I0(b)` with `t = (2k - (n-1)) / (n-1)`.
/// The help gives `2 <= b <= 20`.
fn kaiser_values(n: usize, b: f64) -> Vec<f64> {
    let m = (n - 1) as f64;
    let denominator = bessel_i0(b);
    (0..n)
        .map(|k| {
            let t = (2.0 * k as f64 - m) / m;
            bessel_i0(b * (1.0 - t * t).max(0.0).sqrt()) / denominator
        })
        .collect()
}

fn kaiser(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "kaiser")?;
    let b = ranged(&args[1], "kaiser", "the parameter b", 2.0, 20.0)?;
    window_result(kaiser_values(n, b))
}

/// Gaussian window (help "Signal Windowing"): the k-th value is
/// `exp(-(1/2) (2a (1/2 - k/(n-1)))^2)`. Since `2a (1/2 - k/(n-1)) = a (k - (n-1)/2) / ((n-1)/2)`,
/// this is `exp(-(1/2) t^2)` with `t = a (k - half) / half`. The help allows `2 <= a <= 20`.
fn gaussian_values(n: usize, a: f64) -> Vec<f64> {
    let half = (n - 1) as f64 / 2.0;
    (0..n)
        .map(|k| {
            let t = a * (k as f64 - half) / half;
            (-0.5 * t * t).exp()
        })
        .collect()
}

fn gaussian(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "gaussian")?;
    let a = ranged(&args[1], "gaussian", "the parameter a", 2.0, 20.0)?;
    window_result(gaussian_values(n, a))
}

/// Cosine taper (the Tukey window, Harris 1978): the window is a rectangle whose two ends follow a
/// raised cosine over the fraction `p` of its length. `p = 0` is rectangular, `p = 1` is Hanning.
fn costaper_values(n: usize, p: f64) -> Vec<f64> {
    let last = (n - 1) as f64;
    let m = p * last;
    (0..n)
        .map(|k| {
            let pos = k as f64;
            if m <= 0.0 {
                1.0
            } else if pos < m / 2.0 {
                0.5 * (1.0 - (TAU * pos / m).cos())
            } else if pos > last - m / 2.0 {
                0.5 * (1.0 - (TAU * (last - pos) / m).cos())
            } else {
                1.0
            }
        })
        .collect()
}

fn costaper(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "costaper")?;
    let p = ranged(&args[1], "costaper", "the taper fraction a", 0.0, 1.0)?;
    window_result(costaper_values(n, p))
}

/// The Chebyshev polynomial `T_m(p)` for any real `p`.
fn chebyshev_t(m: usize, p: f64) -> f64 {
    let order = m as f64;
    if p.abs() <= 1.0 {
        (order * p.acos()).cos()
    } else {
        let v = (order * p.abs().acosh()).cosh();
        if p < 0.0 && m % 2 == 1 {
            -v
        } else {
            v
        }
    }
}

/// Dolph-Chebyshev window (Dolph 1946) of odd length `n`, with the sidelobe level `r` in dB (the
/// help says `r > 1`, typically above 50). Its DFT is `T_{n-1}(x0 cos(pi k / n))` with
/// `x0 = cosh(acosh(10^(r/20)) / (n-1))`; the window is the inverse DFT of that, shifted to the
/// centre, and it is normalised to a peak of 1. The cost is `n^2` cosine evaluations.
fn cheby_values(n: usize, r: f64) -> Vec<f64> {
    let m = n - 1;
    let x0 = ((10f64.powf(r / 20.0)).acosh() / m as f64).cosh();
    let spectrum: Vec<f64> = (0..n)
        .map(|k| chebyshev_t(m, x0 * (PI * k as f64 / n as f64).cos()))
        .collect();
    let centre = m as f64 / 2.0;
    let mut w: Vec<f64> = (0..n)
        .map(|j| {
            let shift = j as f64 - centre;
            spectrum
                .iter()
                .enumerate()
                .map(|(k, s)| s * (TAU * k as f64 * shift / n as f64).cos())
                .sum()
        })
        .collect();
    let peak = w.iter().fold(0.0f64, |acc, x| acc.max(x.abs()));
    if peak.is_finite() && peak > 0.0 {
        for x in &mut w {
            *x /= peak;
        }
    }
    w
}

fn cheby(args: &[Value]) -> Result<Value, EvalError> {
    let n = window_len(&args[0], "cheby")?;
    if n % 2 == 0 {
        return Err(EvalError::Unsupported(
            "cheby needs an odd window length".to_string(),
        ));
    }
    let r = real_param(&args[1], "cheby")?;
    if r <= 1.0 {
        return Err(EvalError::Unsupported(
            "cheby needs a sidelobe ripple above 1 dB".to_string(),
        ));
    }
    let w = cheby_values(n, r);
    if w.iter().any(|x| !x.is_finite()) {
        return Err(EvalError::Unsupported(
            "cheby: the sidelobe ripple is too large for this window length".to_string(),
        ));
    }
    window_result(w)
}

// ---- FIR design -------------------------------------------------------------------------------

/// The window index `w` of the design functions (help table): 0 and 1 rectangular, 2 tapered
/// rectangular, 3 triangular, 4 to 7 the named windows.
fn taper_values(w: Option<&Value>, n: usize, name: &str) -> Result<Vec<f64>, EvalError> {
    let index = match w {
        None => 1,
        Some(v) => {
            let x = real_param(v, name)?;
            if x.fract() != 0.0 || !(0.0..=7.0).contains(&x) {
                return Err(EvalError::Unsupported(format!(
                    "{name} needs a window index from 0 to 7"
                )));
            }
            x as usize
        }
    };
    Ok(match index {
        0 | 1 => vec![1.0; n],
        2 => taprect_values(n),
        3 => triangular_values(n),
        4 => cos_sum(n, &HANNING),
        5 => cos_sum(n, &HAMMING),
        6 => cos_sum(n, &BLACKMAN),
        _ => cos_sum(n, &NUTTALL),
    })
}

/// The ideal lowpass impulse response at lag `t` for cutoff `f`: `2 f sinc(2 pi f t)` with the
/// unnormalised sinc, as the help's example writes it.
fn lowpass_term(f: f64, t: f64) -> f64 {
    let x = TAU * f * t;
    if x == 0.0 {
        2.0 * f
    } else {
        2.0 * f * x.sin() / x
    }
}

/// The unit impulse at lag `t`.
fn impulse(t: f64) -> f64 {
    if t == 0.0 {
        1.0
    } else {
        0.0
    }
}

/// A windowed FIR design: the ideal response `ideal(t)` at the centred lags `t = k - (n-1)/2`,
/// multiplied by the window.
fn fir_design(
    n: usize,
    w: Option<&Value>,
    name: &str,
    ideal: impl Fn(f64) -> f64,
) -> Result<Value, EvalError> {
    let taper = taper_values(w, n, name)?;
    let centre = (n - 1) as f64 / 2.0;
    let h: Vec<f64> = (0..n)
        .map(|k| ideal(k as f64 - centre) * taper[k])
        .collect();
    window_result(h)
}

fn lowpass(args: &[Value]) -> Result<Value, EvalError> {
    let f = cutoff(&args[0], "lowpass")?;
    let n = fir_len(&args[1], "lowpass")?;
    fir_design(n, args.get(2), "lowpass", |t| lowpass_term(f, t))
}

fn highpass(args: &[Value]) -> Result<Value, EvalError> {
    let f = cutoff(&args[0], "highpass")?;
    let n = fir_len(&args[1], "highpass")?;
    fir_design(n, args.get(2), "highpass", |t| {
        impulse(t) - lowpass_term(f, t)
    })
}

fn band_edges(args: &[Value], name: &str) -> Result<(f64, f64, usize), EvalError> {
    let f1 = cutoff(&args[0], name)?;
    let f2 = cutoff(&args[1], name)?;
    let n = fir_len(&args[2], name)?;
    Ok((f1, f2, n))
}

fn bandpass(args: &[Value]) -> Result<Value, EvalError> {
    let (f1, f2, n) = band_edges(args, "bandpass")?;
    fir_design(n, args.get(3), "bandpass", |t| {
        lowpass_term(f2, t) - lowpass_term(f1, t)
    })
}

fn bandstop(args: &[Value]) -> Result<Value, EvalError> {
    let (f1, f2, n) = band_edges(args, "bandstop")?;
    fir_design(n, args.get(3), "bandstop", |t| {
        impulse(t) - (lowpass_term(f2, t) - lowpass_term(f1, t))
    })
}

// ---- filtering --------------------------------------------------------------------------------

/// One section of a filter in cascade form, normalised so that `a[0] = 1`. A FIR filter is one
/// section with `a = [1]`.
struct Section {
    b: Vec<Complex64>,
    a: Vec<Complex64>,
}

/// The sections of a filter argument: a FIR vector, or an IIR matrix of numerator and denominator
/// column pairs.
fn sections(c: &Value, name: &str) -> Result<Vec<Section>, EvalError> {
    let (rows, cols, data) = grid(c, name)?;
    for s in &data {
        ops::require_dimensionless(s.dim)?;
    }
    let z = complexes(&data)?;
    if cols == 1 {
        return Ok(vec![Section { b: z, a: vec![ONE] }]);
    }
    // A row vector is not a FIR filter: it would be read as column pairs of one coefficient each.
    if rows < 2 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a column vector (FIR) or column pairs (IIR)"
        )));
    }
    if cols % 2 != 0 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a FIR vector or numerator and denominator column pairs"
        )));
    }
    let mut out = Vec::with_capacity(cols / 2);
    for pair in 0..cols / 2 {
        let b: Vec<Complex64> = (0..rows).map(|r| z[r * cols + 2 * pair]).collect();
        let a: Vec<Complex64> = (0..rows).map(|r| z[r * cols + 2 * pair + 1]).collect();
        let a0 = a[0];
        if a0 == ZERO {
            return Err(EvalError::Unsupported(format!(
                "{name}: a denominator's constant term is 0"
            )));
        }
        out.push(Section {
            b: b.iter().map(|x| *x / a0).collect(),
            a: a.iter().map(|x| *x / a0).collect(),
        });
    }
    Ok(out)
}

/// The first `input.len()` outputs of the cascade on `input`, direct form, zero initial state:
/// `y[k] = sum_j b_j x[k-j] - sum_{j>=1} a_j y[k-j]` for each section in turn.
fn run_sections(input: &[Complex64], secs: &[Section]) -> Vec<Complex64> {
    let n = input.len();
    let mut x = input.to_vec();
    for s in secs {
        let mut y = vec![ZERO; n];
        for k in 0..n {
            let mut acc = ZERO;
            for (j, bj) in s.b.iter().enumerate().take(k + 1) {
                acc += *bj * x[k - j];
            }
            for (j, aj) in s.a.iter().enumerate().take(k + 1).skip(1) {
                acc -= *aj * y[k - j];
            }
            y[k] = acc;
        }
        x = y;
    }
    x
}

/// The first `n` outputs of the filter on each column of `v`, as an `n x cols` matrix.
fn filter_grid(v: &Value, secs: &[Section], n: usize, name: &str) -> Result<Value, EvalError> {
    let (rows, cols, data) = grid(v, name)?;
    if n == 0 || n > rows {
        return Err(EvalError::Unsupported(format!(
            "{name}: the output size must be at most the input length"
        )));
    }
    let dim = one_dim(&data)?;
    let x = complexes(&data)?;
    let mut out = vec![ZERO; n * cols];
    for c in 0..cols {
        let col: Vec<Complex64> = (0..n).map(|r| x[r * cols + c]).collect();
        for (r, value) in run_sections(&col, secs).into_iter().enumerate() {
            out[r * cols + c] = value;
        }
    }
    grid_result(n, cols, &out, dim)
}

/// `response(v, C, n)`: the first `n` samples of the response of `v` to the filter `C`.
fn response(args: &[Value]) -> Result<Value, EvalError> {
    let secs = sections(&args[1], "response")?;
    let n = count(&args[2], "response")?;
    filter_grid(&args[0], &secs, n, "response")
}

/// `fftfilt(v, C, [nfft])`: the FIR response to `v`, with one output per input sample. The value
/// is the causal convolution, the same numbers the overlap-add method gives. `nfft` must lie between
/// the filter length and the signal length.
fn fftfilt(args: &[Value]) -> Result<Value, EvalError> {
    let secs = sections(&args[1], "fftfilt")?;
    if secs.len() != 1 || secs[0].a.len() != 1 {
        return Err(EvalError::Unsupported(
            "fftfilt needs a FIR filter vector".to_string(),
        ));
    }
    let (rows, _, _) = grid(&args[0], "fftfilt")?;
    let taps = secs[0].b.len();
    if let Some(nfft) = args.get(2) {
        let m = count(nfft, "fftfilt")?;
        if m > rows || m < taps {
            return Err(EvalError::Unsupported(
                "fftfilt: nfft must be between the filter length and the signal length".to_string(),
            ));
        }
    }
    filter_grid(&args[0], &secs, rows, "fftfilt")
}

/// `filtfilt(x, C)`: the zero-phase response. The signal is filtered forward, the output reversed,
/// filtered again and reversed back, so the phase cancels (the help's two calls of `response`).
fn filtfilt(args: &[Value]) -> Result<Value, EvalError> {
    let secs = sections(&args[1], "filtfilt")?;
    let (_, cols, data) = grid(&args[0], "filtfilt")?;
    if cols != 1 {
        return Err(EvalError::Unsupported(
            "filtfilt needs a vector".to_string(),
        ));
    }
    let dim = one_dim(&data)?;
    let x = complexes(&data)?;
    let mut forward = run_sections(&x, &secs);
    forward.reverse();
    let mut backward = run_sections(&forward, &secs);
    backward.reverse();
    column(&backward, dim)
}

/// `P(w) = sum_j c_j w^j` (Horner).
fn poly(c: &[Complex64], w: Complex64) -> Complex64 {
    c.iter().rev().fold(ZERO, |acc, &cj| acc * w + cj)
}

/// `gain(C, f)`: the complex gain `H(e^{i 2 pi f})` of the filter at the fraction `f` of the
/// sampling frequency (`0 <= f <= 1`). A vector of frequencies gives a column of gains. The gain
/// of a cascade is the product of its sections, `B(z) / A(z)` with `z^{-1} = e^{-i 2 pi f}`.
fn gain(args: &[Value]) -> Result<Value, EvalError> {
    let secs = sections(&args[0], "gain")?;
    let (rows, cols, data) = grid(&args[1], "gain")?;
    let dim = one_dim(&data)?;
    ops::require_dimensionless(dim)?;
    if cols != 1 {
        return Err(EvalError::Unsupported(
            "gain needs a frequency or a column of frequencies".to_string(),
        ));
    }
    let freqs = reals(&data, "gain")?;
    let mut values = Vec::with_capacity(rows);
    for f in freqs {
        if !(0.0..=1.0).contains(&f) {
            return Err(EvalError::Unsupported(
                "gain needs frequencies between 0 and 1".to_string(),
            ));
        }
        let w = Complex64::from_polar(1.0, -TAU * f);
        let mut h = ONE;
        for s in &secs {
            h *= poly(&s.b, w) / poly(&s.a, w);
        }
        values.push(h);
    }
    let dimless = Dimension::dimensionless();
    match &args[1] {
        Value::Scalar(_) => scalar_result(values[0], dimless),
        _ => column(&values, dimless),
    }
}

// ---- detrend, quantize, phase -----------------------------------------------------------------

/// `detrend(v)`: the residuals of the least-squares straight line through `v` against its index.
/// Complex data is fitted with a complex slope, which is the same as fitting the two parts.
fn detrend(args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, data) = grid(&args[0], "detrend")?;
    if cols != 1 {
        return Err(EvalError::Unsupported("detrend needs a vector".to_string()));
    }
    if rows < 3 {
        return Err(EvalError::Unsupported(
            "detrend needs at least three values".to_string(),
        ));
    }
    let dim = one_dim(&data)?;
    let z = complexes(&data)?;
    let n = rows as f64;
    let centre = (n - 1.0) / 2.0;
    let mean = z.iter().fold(ZERO, |acc, &v| acc + v) / n;
    let mut numerator = ZERO;
    let mut denominator = 0.0;
    for (k, &v) in z.iter().enumerate() {
        let t = k as f64 - centre;
        numerator += (v - mean) * t;
        denominator += t * t;
    }
    let slope = numerator / denominator;
    let residuals: Vec<Complex64> = z
        .iter()
        .enumerate()
        .map(|(k, &v)| v - mean - slope * (k as f64 - centre))
        .collect();
    column(&residuals, dim)
}

/// `quantize(v, n)`: each element moves to the nearest of `n` equally spaced levels from `min(v)` to
/// `max(v)`, spaced `(max - min) / (n - 1)`. A tie moves up. Units are kept. For one level the
/// spacing formula has no value; the only level that is the lowest level `min(v)` is used, so every
/// element becomes `min(v)` (the limit of the rule as `n` falls to 1: every element rounds to level 0).
fn quantize(args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, data) = grid(&args[0], "quantize")?;
    let dim = one_dim(&data)?;
    let x = reals(&data, "quantize")?;
    let levels = count(&args[1], "quantize")?;
    if levels < 1 {
        return Err(EvalError::Unsupported(
            "quantize needs at least one level".to_string(),
        ));
    }
    let lo = x.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let hi = x.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let out: Vec<Complex64> = if levels == 1 {
        x.iter().map(|_| Complex64::new(lo, 0.0)).collect()
    } else {
        let step = (hi - lo) / (levels - 1) as f64;
        x.iter()
            .map(|&v| {
                if step == 0.0 {
                    Complex64::new(v, 0.0)
                } else {
                    Complex64::new(lo + ((v - lo) / step + 0.5).floor() * step, 0.0)
                }
            })
            .collect()
    };
    grid_result(rows, cols, &out, dim)
}

/// `phase(v)`: the angle of each element in `(-pi, pi]`, `atan2(im, re)`, dimensionless.
fn phase(args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, data) = grid(&args[0], "phase")?;
    let z = complexes(&data)?;
    let angles: Vec<Complex64> = z
        .iter()
        .map(|v| Complex64::new(v.im.atan2(v.re), 0.0))
        .collect();
    let dimless = Dimension::dimensionless();
    match &args[0] {
        Value::Scalar(_) => scalar_result(angles[0], dimless),
        _ => grid_result(rows, cols, &angles, dimless),
    }
}

/// `phasecor(p)`: the phase vector `p` with multiples of `2 pi` added so that consecutive values
/// differ by less than `pi`.
fn phasecor(args: &[Value]) -> Result<Value, EvalError> {
    let xs: Vec<Scalar> = vector(&args[0], "phasecor")?;
    let values = reals(&xs, "phasecor")?;
    for s in &xs {
        ops::require_dimensionless(s.dim)?;
    }
    let mut offset = 0.0f64;
    let mut out = Vec::with_capacity(values.len());
    for (i, &p) in values.iter().enumerate() {
        if i > 0 {
            let jump = p - values[i - 1];
            offset -= TAU * (jump / TAU).round();
        }
        out.push(Complex64::new(p + offset, 0.0));
    }
    column(&out, Dimension::dimensionless())
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;
    use super::super::BuiltinFn;
    use super::*;
    use crate::eval::value::Matrix;

    fn close(a: &[f64], b: &[f64], tol: f64) {
        assert_eq!(a.len(), b.len(), "length");
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() <= tol, "{x} vs {y}");
        }
    }

    fn real(xs: &[f64]) -> Value {
        col(xs)
    }

    fn mat(rows: usize, cols: usize, xs: &[f64]) -> Value {
        let data = xs
            .iter()
            .map(|&x| Scalar::new(Complex64::new(x, 0.0), dimless()))
            .collect();
        Value::Matrix(Matrix::new(rows, cols, data).unwrap())
    }

    fn run(f: BuiltinFn, args: &[Value]) -> Vec<f64> {
        values(&f(args).unwrap())
    }

    fn symmetric(w: &[f64]) {
        let n = w.len();
        for k in 0..n {
            assert!((w[k] - w[n - 1 - k]).abs() < 1e-12, "not symmetric at {k}");
        }
    }

    #[test]
    fn hanning_hamming_blackman_nuttall_values() {
        close(
            &run(hanning, &[num(5.0)]),
            &[0.0, 0.5, 1.0, 0.5, 0.0],
            1e-12,
        );
        close(
            &run(hamming, &[num(5.0)]),
            &[0.08, 0.54, 1.0, 0.54, 0.08],
            1e-12,
        );
        close(&run(blackman, &[num(3.0)]), &[0.0, 1.0, 0.0], 1e-12);
        let nut = run(nuttall, &[num(9.0)]);
        symmetric(&nut);
        assert!((nut[4] - 1.0).abs() < 1e-3, "nuttall peak {}", nut[4]);
    }

    #[test]
    fn triangular_is_two_j_over_n_minus_one_with_zero_ends() {
        // Help "Signal Windowing": 2j/(N-1) for j <= (N-1)/2, then 2 - 2j/(N-1).
        let w = run(triangular, &[num(5.0)]);
        close(&w, &[0.0, 0.5, 1.0, 0.5, 0.0], 1e-12);
        let w = run(triangular, &[num(4.0)]);
        close(&w, &[0.0, 2.0 / 3.0, 2.0 / 3.0, 0.0], 1e-12);
    }

    #[test]
    fn taprect_follows_the_help_ramp() {
        // N = 64: h = floor(62 / 10) = 6; ramp 0.5 (1 - cos(pi j / 7)) for j = 0..6, then flat 1.
        let w = run(taprect, &[num(64.0)]);
        assert_eq!(w.len(), 64);
        assert!(w[0].abs() < 1e-15 && w[63].abs() < 1e-15);
        for j in 0..=6 {
            let expected = 0.5 * (1.0 - (PI * j as f64 / 7.0).cos());
            assert!((w[j] - expected).abs() < 1e-12, "left {j}");
            assert!((w[63 - j] - expected).abs() < 1e-12, "right {j}");
        }
        assert!((w[7] - 1.0).abs() < 1e-15 && (w[56] - 1.0).abs() < 1e-15);
        symmetric(&w);
        // Index 2 of the design functions is this window.
        let fir = run(lowpass, &[num(0.1), num(64.0), num(2.0)]);
        let plain = run(lowpass, &[num(0.1), num(64.0)]);
        for k in 0..64 {
            assert!((fir[k] - plain[k] * w[k]).abs() < 1e-12, "tap {k}");
        }
        assert!(run(taprect, &[num(2.0)]).iter().all(|x| x.abs() < 1e-15));
    }

    #[test]
    fn bessel_i0_known_values() {
        assert!((bessel_i0(0.0) - 1.0).abs() < 1e-15);
        assert!((bessel_i0(1.0) - 1.266_065_877_752_008_4).abs() < 1e-14);
        assert!((bessel_i0(20.0) - 4.355_828_255_955_353e7).abs() / 4.4e7 < 1e-10);
    }

    #[test]
    fn kaiser_endpoints_are_one_over_i0() {
        let w = run(kaiser, &[num(7.0), num(2.0)]);
        symmetric(&w);
        let end = 1.0 / bessel_i0(2.0);
        assert!((w[0] - end).abs() < 1e-14);
        assert!((w[3] - 1.0).abs() < 1e-14);
    }

    #[test]
    fn gaussian_and_costaper_shapes() {
        let g = run(gaussian, &[num(11.0), num(3.0)]);
        symmetric(&g);
        assert!((g[5] - 1.0).abs() < 1e-15);
        // Help formula exp(-(1/2)(2a(1/2 - k/(N-1)))^2): gaussian(9, 3.6) ends at about 0.00153.
        let g9 = run(gaussian, &[num(9.0), num(3.6)]);
        assert!((g9[0] - 0.001_53).abs() < 1e-5, "end {}", g9[0]);
        assert!((g9[0] - (-6.48f64).exp()).abs() < 1e-12);
        assert!((g9[2] - (-1.62f64).exp()).abs() < 1e-12);
        let flat = run(costaper, &[num(8.0), num(0.0)]);
        close(&flat, &[1.0; 8], 0.0);
        let hann_like = run(costaper, &[num(9.0), num(1.0)]);
        let expected: Vec<f64> = (0..9)
            .map(|k| 0.5 * (1.0 - (TAU * k as f64 / 8.0).cos()))
            .collect();
        close(&hann_like, &expected, 1e-12);
    }

    #[test]
    fn cheby_is_symmetric_and_meets_its_sidelobe_level() {
        let n = 21;
        let r = 50.0;
        let w = cheby_values(n, r);
        symmetric(&w);
        assert!(w.iter().any(|x| (x.abs() - 1.0).abs() < 1e-12), "peak is 1");
        // Zero-padded DFT: the response is at most the ripple beyond its first drop to that level.
        let points = 1024;
        let response: Vec<f64> = (0..points)
            .map(|q| {
                let f = q as f64 / points as f64;
                w.iter()
                    .enumerate()
                    .map(|(k, &x)| Complex64::from_polar(x, -TAU * f * k as f64))
                    .sum::<Complex64>()
                    .norm()
            })
            .collect();
        let main = response[0];
        let ripple = 10f64.powf(-r / 20.0);
        let first = (1..points)
            .find(|&q| response[q] / main <= ripple)
            .expect("the response falls to the ripple level");
        for (q, value) in response.iter().enumerate().take(points / 2).skip(first) {
            assert!(
                value / main <= ripple * 1.01,
                "sidelobe at {q}: {}",
                value / main
            );
        }
    }

    #[test]
    fn cheby_refuses_even_length_and_small_ripple() {
        assert!(cheby(&[num(8.0), num(60.0)]).is_err());
        assert!(cheby(&[num(9.0), num(1.0)]).is_err());
    }

    #[test]
    fn windows_refuse_short_and_huge_lengths() {
        assert!(hanning(&[num(1.0)]).is_err());
        assert!(hanning(&[num(0.5)]).is_err());
        assert!(hanning(&[num(1e12)]).is_err());
        assert!(kaiser(&[num(5.0), num(1.0)]).is_err());
        assert!(gaussian(&[num(5.0), num(21.0)]).is_err());
        assert!(costaper(&[num(5.0), num(1.5)]).is_err());
    }

    #[test]
    fn lowpass_is_symmetric_with_unit_dc_gain() {
        let h = run(lowpass, &[num(0.1), num(101.0)]);
        symmetric(&h);
        let sum: f64 = h.iter().sum();
        // Rectangular truncation of the ideal response: the DC gain is close to 1, not exact.
        assert!((sum - 1.0).abs() < 0.03, "dc gain {sum}");
        assert!((h[50] - 0.2).abs() < 1e-12, "centre tap {}", h[50]);
    }

    #[test]
    fn highpass_plus_lowpass_is_an_impulse() {
        let lp = run(lowpass, &[num(0.1), num(31.0)]);
        let hp = run(highpass, &[num(0.1), num(31.0)]);
        for k in 0..31 {
            let total = lp[k] + hp[k];
            let expected = if k == 15 { 1.0 } else { 0.0 };
            assert!((total - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn bandstop_plus_bandpass_is_an_impulse() {
        let bp = run(bandpass, &[num(0.1), num(0.2), num(31.0)]);
        let bs = run(bandstop, &[num(0.1), num(0.2), num(31.0)]);
        for k in 0..31 {
            let expected = if k == 15 { 1.0 } else { 0.0 };
            assert!((bp[k] + bs[k] - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn design_refuses_bad_index() {
        assert!(lowpass(&[num(0.1), num(11.0), num(8.0)]).is_err());
        assert!(lowpass(&[num(0.1), num(11.0), num(2.5)]).is_err());
        assert!(lowpass(&[num(0.7), num(11.0)]).is_err());
        assert!(lowpass(&[num(0.1), num(2.0)]).is_err());
    }

    #[test]
    fn response_of_an_impulse_is_the_filter() {
        let delta = real(&[1.0, 0.0, 0.0, 0.0, 0.0]);
        let fir = real(&[1.0, 2.0, 3.0]);
        let out = response(&[delta.clone(), fir, num(5.0)]).unwrap();
        close(&values(&out), &[1.0, 2.0, 3.0, 0.0, 0.0], 1e-12);
        // y[k] = x[k] + 0.5 y[k-1]: b = [1, 0], a = [1, -0.5].
        let iir = mat(2, 2, &[1.0, 1.0, 0.0, -0.5]);
        let out = response(&[delta, iir, num(4.0)]).unwrap();
        close(&values(&out), &[1.0, 0.5, 0.25, 0.125], 1e-12);
    }

    #[test]
    fn cascade_equals_its_sections_applied_in_turn() {
        let x: Vec<f64> = (0..30).map(|k| ((k * 7 % 11) as f64) - 5.0).collect();
        let s1 = mat(2, 2, &[1.0, 1.0, 0.0, -0.5]);
        let s2 = mat(2, 2, &[1.0, 1.0, 0.4, -0.2]);
        let cascade = mat(2, 4, &[1.0, 1.0, 1.0, 1.0, 0.0, -0.5, 0.4, -0.2]);
        let both = response(&[real(&x), cascade, num(30.0)]).unwrap();
        let first = response(&[real(&x), s1, num(30.0)]).unwrap();
        let second = response(&[first, s2, num(30.0)]).unwrap();
        close(&values(&both), &values(&second), 1e-10);
    }

    #[test]
    fn fftfilt_equals_response_over_the_whole_signal() {
        let x: Vec<f64> = (0..40).map(|k| (k as f64 * 0.37).sin()).collect();
        let fir = real(&[0.2, 0.5, -0.3, 0.1, 0.05, 0.02, -0.01]);
        let a = fftfilt(&[real(&x), fir.clone()]).unwrap();
        let b = response(&[real(&x), fir.clone(), num(40.0)]).unwrap();
        close(&values(&a), &values(&b), 1e-12);
        assert!(fftfilt(&[real(&x), fir, num(3.0)]).is_err());
    }

    #[test]
    fn filtfilt_of_a_constant_has_squared_dc_gain_in_the_interior() {
        let h = run(lowpass, &[num(0.1), num(7.0)]);
        let g: f64 = h.iter().sum();
        let x = vec![1.0; 100];
        let out = values(&filtfilt(&[real(&x), real(&h)]).unwrap());
        assert_eq!(out.len(), 100);
        for &v in &out[10..90] {
            assert!((v - g * g).abs() < 1e-12, "{v} vs {}", g * g);
        }
    }

    #[test]
    fn gain_of_simple_filters() {
        let fir = real(&[1.0, 1.0]);
        let at0 = gain(&[fir.clone(), num(0.0)]).unwrap();
        assert!((complex_values(&at0)[0].re - 2.0).abs() < 1e-12);
        let at_half = gain(&[fir, num(0.5)]).unwrap();
        assert!(complex_values(&at_half)[0].norm() < 1e-12);
        // y[k] = x[k] + 0.5 y[k-1] has gain 1 / (1 - 0.5) = 2 at DC.
        let iir = mat(2, 2, &[1.0, 1.0, 0.0, -0.5]);
        let dc = gain(&[iir, num(0.0)]).unwrap();
        assert!((complex_values(&dc)[0].re - 2.0).abs() < 1e-12);
    }

    #[test]
    fn gain_matches_the_help_example_magnitude() {
        // Help "Example: Filter Gain": the second-order Butterworth coefficients give |G(0.2)| = 0.707.
        let a = mat(3, 2, &[0.207, 1.0, 0.413, -0.37, 0.207, 0.196]);
        let g = gain(&[a, num(0.2)]).unwrap();
        assert!((complex_values(&g)[0].norm() - 0.707).abs() < 1e-3);
    }

    #[test]
    fn gain_refuses_frequencies_outside_the_unit_interval() {
        let fir = real(&[1.0, 1.0]);
        assert!(gain(&[fir.clone(), num(1.5)]).is_err());
        assert!(gain(&[fir, num(-0.1)]).is_err());
    }

    #[test]
    fn detrend_removes_a_straight_line() {
        let line: Vec<f64> = (0..10).map(|k| 2.0 + 3.0 * k as f64).collect();
        let r = run(detrend, &[real(&line)]);
        close(&r, &[0.0; 10], 1e-9);
        let noisy = vec![1.0, 4.0, 2.0, 8.0, 5.0];
        let r = run(detrend, &[real(&noisy)]);
        assert!(r.iter().sum::<f64>().abs() < 1e-12);
        assert!(detrend(&[real(&[1.0, 2.0])]).is_err());
    }

    #[test]
    fn quantize_moves_to_the_nearest_level() {
        let out = run(quantize, &[real(&[0.0, 0.3, 0.6, 1.0]), num(3.0)]);
        close(&out, &[0.0, 0.5, 0.5, 1.0], 1e-12);
        let two = run(quantize, &[real(&[0.0, 0.4, 0.6, 1.0]), num(2.0)]);
        close(&two, &[0.0, 0.0, 1.0, 1.0], 1e-12);
        // One level: every element moves to the lowest level min(v).
        let one = run(quantize, &[real(&[0.4, 1.0, -2.0]), num(1.0)]);
        close(&one, &[-2.0, -2.0, -2.0], 1e-12);
        assert!(quantize(&[real(&[0.0, 1.0]), num(0.0)]).is_err());
    }

    #[test]
    fn row_vector_is_not_a_fir_filter() {
        let x = real(&[1.0, 2.0, 3.0, 4.0]);
        let row = mat(1, 2, &[0.5, 0.25]);
        assert!(fftfilt(&[x.clone(), row.clone()]).is_err());
        assert!(response(&[x.clone(), row.clone(), num(4.0)]).is_err());
        assert!(gain(&[row, num(0.1)]).is_err());
        // The column form is the FIR filter: fftfilt of [1] is the identity.
        let out = run(fftfilt, &[x, real(&[0.5, 0.25])]);
        close(&out, &[0.5, 1.25, 2.0, 2.75], 1e-12);
    }

    #[test]
    fn phase_and_phasecor() {
        let p = run(
            phase,
            &[Value::Scalar(Scalar::new(
                Complex64::new(1.0, 1.0),
                dimless(),
            ))],
        );
        assert!((p[0] - PI / 4.0).abs() < 1e-12);
        let un = run(phasecor, &[real(&[3.0, -3.0])]);
        close(&un, &[3.0, TAU - 3.0], 1e-12);
    }

    #[test]
    fn hostile_inputs_are_refused_not_panicking() {
        let nan = Value::Scalar(Scalar::new(Complex64::new(f64::NAN, 0.0), dimless()));
        assert!(hanning(std::slice::from_ref(&nan)).is_err());
        assert!(response(&[real(&[1.0]), nan.clone(), num(1.0)]).is_err());
        assert!(response(&[real(&[1.0, 2.0]), real(&[1.0]), num(3.0)]).is_err());
        assert!(filtfilt(&[real(&[1.0, 2.0]), mat(2, 3, &[1.0; 6])]).is_err());
        assert!(detrend(&[real(&[1.0])]).is_err());
        assert!(phasecor(std::slice::from_ref(&nan)).is_err());
        assert!(cheby(&[num(301.0), num(1e300)]).is_err());
        assert!(gain(&[real(&[1.0, 1.0]), nan.clone()]).is_err());
        assert!(gain(&[real(&[1.0, 1.0]), real(&[0.1, 0.2]),]).is_ok());
    }
}

//! Transforms: `dct` and `idct` (orthonormal two-dimensional DCT-II and its inverse), `center` (the
//! two-dimensional discrete Fourier transform with the zero frequency moved to the middle), and
//! `matconv` (filtering by a frequency-domain mask).
//!
//! The DCT is the help's worked formula, `DCT(u,v) = G(u,v) sum sum M(n,p) cos(...) cos(...)` with
//! `G = 2 c(u) c(v) / sqrt(N P)` and `c(0) = 1/sqrt 2`, which is separable and orthonormal, so `idct`
//! is its transpose. Its input may be complex, as the help says.
//!
//! The Fourier transform is the naive separable sum with the `1/sqrt(N)` scaling of Mathcad's `dft`
//! (the help does not print the scaling; it follows the discrete Fourier transforms of the signal
//! tier). Its cost is `rows * cols * (rows + cols)` operations, so it is charged that way.

use std::f64::consts::PI;

use num_complex::Complex64;

use crate::eval::builtins::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::Value;

use super::util::{charge, complex_plane, complex_value};

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("dct", Arity::Exact(1), dct);
    r.register_ctx("idct", Arity::Exact(1), idct);
    r.register_ctx("center", Arity::Exact(1), center);
    r.register_ctx("matconv", Arity::Exact(2), matconv);
}

/// The `n x n` DCT-II basis, row-major: row `u` holds `a(u) cos((2k+1) u pi / 2n)` in column `k`,
/// with `a(0) = sqrt(1/n)` and `a(u) = sqrt(2/n)` otherwise.
fn dct_basis(n: usize) -> Vec<f64> {
    let nf = n as f64;
    let mut m = vec![0.0; n * n];
    for u in 0..n {
        let a = if u == 0 {
            (1.0 / nf).sqrt()
        } else {
            (2.0 / nf).sqrt()
        };
        for k in 0..n {
            m[u * n + k] = a * (((2 * k + 1) as f64) * (u as f64) * PI / (2.0 * nf)).cos();
        }
    }
    m
}

/// `Z = X * Mᵀ`: `Z[i][v] = sum_j X[i][j] M[v][j]` for `X` of `rows x cols` and `M` of `cols x cols`.
fn right_transposed(x: &[Complex64], rows: usize, cols: usize, m: &[f64]) -> Vec<Complex64> {
    let mut z = vec![Complex64::new(0.0, 0.0); rows * cols];
    for i in 0..rows {
        for v in 0..cols {
            let mut s = Complex64::new(0.0, 0.0);
            for j in 0..cols {
                s += x[i * cols + j] * m[v * cols + j];
            }
            z[i * cols + v] = s;
        }
    }
    z
}

/// `Z = X * M`: `Z[i][v] = sum_j X[i][j] M[j][v]`.
fn right_plain(x: &[Complex64], rows: usize, cols: usize, m: &[f64]) -> Vec<Complex64> {
    let mut z = vec![Complex64::new(0.0, 0.0); rows * cols];
    for i in 0..rows {
        for v in 0..cols {
            let mut s = Complex64::new(0.0, 0.0);
            for j in 0..cols {
                s += x[i * cols + j] * m[j * cols + v];
            }
            z[i * cols + v] = s;
        }
    }
    z
}

/// `Y = M * Z` for `M` of `rows x rows`: `Y[u][j] = sum_i M[u][i] Z[i][j]`.
fn left_plain(z: &[Complex64], rows: usize, cols: usize, m: &[f64]) -> Vec<Complex64> {
    let mut y = vec![Complex64::new(0.0, 0.0); rows * cols];
    for u in 0..rows {
        for j in 0..cols {
            let mut s = Complex64::new(0.0, 0.0);
            for i in 0..rows {
                s += z[i * cols + j] * m[u * rows + i];
            }
            y[u * cols + j] = s;
        }
    }
    y
}

/// `Y = Mᵀ * Z` for `M` of `rows x rows`: `Y[i][j] = sum_u M[u][i] Z[u][j]`.
fn left_transposed(z: &[Complex64], rows: usize, cols: usize, m: &[f64]) -> Vec<Complex64> {
    let mut y = vec![Complex64::new(0.0, 0.0); rows * cols];
    for i in 0..rows {
        for j in 0..cols {
            let mut s = Complex64::new(0.0, 0.0);
            for u in 0..rows {
                s += z[u * cols + j] * m[u * rows + i];
            }
            y[i * cols + j] = s;
        }
    }
    y
}

fn dct_args(
    ctx: &BuiltinCtx<'_>,
    arg: &Value,
    name: &str,
) -> Result<(usize, usize, Vec<Complex64>), EvalError> {
    let (rows, cols, data) = complex_plane(arg, name)?;
    if rows < 2 || cols < 2 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs at least two rows and two columns"
        )));
    }
    charge(
        ctx,
        rows.saturating_mul(cols)
            .saturating_mul(rows + cols)
            .saturating_mul(2),
    )?;
    Ok((rows, cols, data))
}

/// `dct(M)`: the two-dimensional DCT-II of the matrix as one array.
fn dct(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, x) = dct_args(ctx, &args[0], "dct")?;
    let cr = dct_basis(rows);
    let cc = dct_basis(cols);
    let z = right_transposed(&x, rows, cols, &cc);
    let d = left_plain(&z, rows, cols, &cr);
    complex_value(rows, cols, d)
}

/// `idct(M)`: the inverse of [`dct`].
fn idct(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, d) = dct_args(ctx, &args[0], "idct")?;
    let cr = dct_basis(rows);
    let cc = dct_basis(cols);
    let z = right_plain(&d, rows, cols, &cc);
    let x = left_transposed(&z, rows, cols, &cr);
    complex_value(rows, cols, x)
}

/// Twiddle factors `exp(sign 2 pi i k / n)` for `k < n`.
fn twiddles(n: usize, sign: f64) -> Vec<Complex64> {
    (0..n)
        .map(|k| Complex64::from_polar(1.0, sign * 2.0 * PI * k as f64 / n as f64))
        .collect()
}

/// The two-dimensional DFT (sign `-1` forward, `+1` inverse), scaled by `1/sqrt(rows cols)`.
fn dft2(rows: usize, cols: usize, x: &[Complex64], sign: f64) -> Vec<Complex64> {
    let tc = twiddles(cols, sign);
    let tr = twiddles(rows, sign);
    // Along each row: Z[i][v] = sum_j X[i][j] w_cols^(j v).
    let mut z = vec![Complex64::new(0.0, 0.0); rows * cols];
    for i in 0..rows {
        for v in 0..cols {
            let mut s = Complex64::new(0.0, 0.0);
            for j in 0..cols {
                s += x[i * cols + j] * tc[(j * v) % cols];
            }
            z[i * cols + v] = s;
        }
    }
    // Along each column: Y[u][v] = sum_i Z[i][v] w_rows^(i u).
    let mut y = vec![Complex64::new(0.0, 0.0); rows * cols];
    for u in 0..rows {
        for v in 0..cols {
            let mut s = Complex64::new(0.0, 0.0);
            for i in 0..rows {
                s += z[i * cols + v] * tr[(i * u) % rows];
            }
            y[u * cols + v] = s;
        }
    }
    let scale = 1.0 / ((rows * cols) as f64).sqrt();
    y.iter_mut().for_each(|v| *v *= scale);
    y
}

/// `center(M)`: the two-dimensional Fourier transform with the zero frequency moved to the centre
/// (`fftshift`: entry `(r, c)` takes the transform entry `(r + rows - floor(rows/2), ...) mod`).
fn center(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, x) = complex_plane(&args[0], "center")?;
    charge(
        ctx,
        rows.saturating_mul(cols)
            .saturating_mul(rows + cols)
            .saturating_mul(2),
    )?;
    let f = dft2(rows, cols, &x, -1.0);
    let out = shifted(&f, rows, cols);
    complex_value(rows, cols, out)
}

/// The grid with the zero frequency moved to the centre.
fn shifted(f: &[Complex64], rows: usize, cols: usize) -> Vec<Complex64> {
    let mut out = vec![Complex64::new(0.0, 0.0); rows * cols];
    for r in 0..rows {
        let sr = (r + rows - rows / 2) % rows;
        for c in 0..cols {
            let sc = (c + cols - cols / 2) % cols;
            out[r * cols + c] = f[sr * cols + sc];
        }
    }
    out
}

/// The matrix `M` of a frequency-domain filter (`funconv`, `funcdeconv`): its shape and its
/// elements, which must be dimensionless and finite.
pub(in crate::eval) fn read_image(
    v: &Value,
    name: &str,
) -> Result<(usize, usize, Vec<Complex64>), EvalError> {
    complex_plane(v, name)
}

/// The work of [`filter_image`] on an image of `rows x cols`, in budget units.
pub(in crate::eval) fn filter_units(rows: usize, cols: usize) -> u64 {
    (rows as u64)
        .saturating_mul(cols as u64)
        .saturating_mul((rows + cols) as u64)
        .saturating_mul(4)
}

/// The image `x` filtered in the frequency domain: its transform is multiplied by the real `mask`
/// (zero frequency at the upper left), or, for `deconv = Some(e)`, divided by `mask + e`; the real
/// part of the inverse transform is returned. A zero divisor is `DivisionByZero`.
pub(in crate::eval) fn filter_image(
    rows: usize,
    cols: usize,
    x: &[Complex64],
    mask: &[f64],
    deconv: Option<f64>,
) -> Result<Value, EvalError> {
    let f = dft2(rows, cols, x, -1.0);
    let mut g = Vec::with_capacity(f.len());
    for (a, m) in f.iter().zip(mask) {
        g.push(match deconv {
            None => a * *m,
            Some(e) => {
                let d = *m + e;
                if d == 0.0 {
                    return Err(EvalError::DivisionByZero);
                }
                a / d
            }
        });
    }
    let back = dft2(rows, cols, &g, 1.0);
    let real: Vec<Complex64> = back.iter().map(|z| Complex64::new(z.re, 0.0)).collect();
    complex_value(rows, cols, real)
}

/// `matconv(M, N)`: filtering in the frequency domain. The transform of `M` is multiplied element by
/// element by the mask `N` (same shape, real values, zero frequency at the upper left as the help's
/// Fourier functions place it), and the real part of the inverse transform is returned.
fn matconv(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (rows, cols, x) = complex_plane(&args[0], "matconv")?;
    let (mr, mc, mask) = complex_plane(&args[1], "matconv")?;
    if (mr, mc) != (rows, cols) {
        return Err(EvalError::ShapeMismatch {
            left: (rows, cols),
            right: (mr, mc),
        });
    }
    if mask.iter().any(|m| m.im != 0.0) {
        return Err(EvalError::Unsupported(
            "matconv needs a real mask".to_string(),
        ));
    }
    charge(
        ctx,
        rows.saturating_mul(cols)
            .saturating_mul(rows + cols)
            .saturating_mul(4),
    )?;
    let f = dft2(rows, cols, &x, -1.0);
    let g: Vec<Complex64> = f.iter().zip(mask.iter()).map(|(a, m)| a * m.re).collect();
    let back = dft2(rows, cols, &g, 1.0);
    let real: Vec<Complex64> = back.iter().map(|z| Complex64::new(z.re, 0.0)).collect();
    complex_value(rows, cols, real)
}

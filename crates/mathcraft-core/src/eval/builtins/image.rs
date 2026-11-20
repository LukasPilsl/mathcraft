//! Image functions that work on matrices: the colour conversions `rgb_to_hsv`, `rgb_to_hls`,
//! `hsv_to_rgb` and `hls_to_rgb`, and the intensity functions `invertimage`, `clip` and `scale`
//! (help: "Converting Images or Colors - RGB and HLS/HSV", "HLS and HSV Color Representations",
//! "Inverting", "Clipping and Scaling", "Example: HLS and HSV Color Systems").
//!
//! Colours are on a 0 to 255 scale. Hue is in degrees (0 to 360); in HSV and HLS, saturation and
//! value (or lightness) are on the 0 to 255 scale too. The help's examples fix the output:
//! `rgb_to_hsv([255; 0; 0]) = [0; 255; 255]`, `rgb_to_hls([255; 0; 0]) = [0; 127.5; 255]`.
//!
//! Argument forms of the conversions: one 3 x 1 colour vector, one packed image matrix, or three
//! scalars (R, G, B), which give a 3 x 1 vector. A packed image matrix is read as the help
//! describes the packed matrices: the red, green and blue planes side by side, so a matrix with
//! `3w` columns is `w` pixels, with the red plane in columns `0..w`, green in `w..2w`, blue in
//! `2w..3w`. A conversion of a three-matrix form (R, G, B as matrices) is not built.
//!
//! Not built: `threshold` (its help gives two different rules for the same bound: "set to thresh"
//! in the summary and "set to zero" in the notes), `binarize`, `equalize`, `gray_to_rgb` and
//! `rgb_to_gray` (the grey formula is not stated in the help), the television colour spaces (YIQ,
//! YCbCr), the filters, edge finders, morphology and transforms.
//!
//! Unverified choices: `scale` of a constant matrix gives the lower limit everywhere, since the
//! help does not say.

use super::matrix::expect_matrix;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("rgb_to_hsv", Arity::Range(1, 3), rgb_to_hsv);
    r.register_ctx("rgb_to_hls", Arity::Range(1, 3), rgb_to_hls);
    r.register_ctx("hsv_to_rgb", Arity::Range(1, 3), hsv_to_rgb);
    r.register_ctx("hls_to_rgb", Arity::Range(1, 3), hls_to_rgb);
    r.register_ctx("invertimage", Arity::Exact(1), invertimage);
    r.register_ctx("clip", Arity::Range(1, 3), clip);
    r.register_ctx("scale", Arity::Range(1, 3), scale);
    r.register_ctx("horzflip", Arity::Exact(1), horzflip);
    r.register_ctx("vertflip", Arity::Exact(1), vertflip);
}

/// `horzflip(M)`: the matrix with its columns in reverse order (mirror left to right).
fn horzflip(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    flip(ctx, "horzflip", &args[0], true)
}

/// `vertflip(M)`: the matrix with its rows in reverse order (mirror top to bottom).
fn vertflip(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    flip(ctx, "vertflip", &args[0], false)
}

/// Copies the elements of `M` in mirrored order: columns reversed when `horizontal`, rows reversed
/// otherwise. Costs one unit per element.
fn flip(
    ctx: &BuiltinCtx<'_>,
    name: &str,
    arg: &Value,
    horizontal: bool,
) -> Result<Value, EvalError> {
    let m = expect_matrix(arg, name)?;
    let (rows, cols) = m.shape();
    ctx.charge(m.data().len() as u64)?;
    let data = m.data();
    let mut out = Vec::with_capacity(data.len());
    for r in 0..rows {
        for c in 0..cols {
            let (sr, sc) = if horizontal {
                (r, cols - 1 - c)
            } else {
                (rows - 1 - r, c)
            };
            out.push(data[sr * cols + sc]);
        }
    }
    Ok(Value::Matrix(Matrix::new(rows, cols, out)?))
}

fn rgb_to_hsv(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    convert(ctx, "rgb_to_hsv", args, to_hsv)
}

fn rgb_to_hls(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    convert(ctx, "rgb_to_hls", args, to_hls)
}

fn hsv_to_rgb(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    convert(ctx, "hsv_to_rgb", args, from_hsv)
}

fn hls_to_rgb(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    convert(ctx, "hls_to_rgb", args, from_hls)
}

/// Applies one colour conversion to a colour vector, three scalars, or a packed image matrix.
fn convert(
    ctx: &BuiltinCtx<'_>,
    name: &str,
    args: &[Value],
    pixel: fn([f64; 3]) -> [f64; 3],
) -> Result<Value, EvalError> {
    if args.len() == 2 {
        return Err(EvalError::Unsupported(format!(
            "{name} takes one colour vector or image matrix, or three scalars"
        )));
    }
    if args.len() == 3 {
        let colour = [
            real_of(&args[0], name)?,
            real_of(&args[1], name)?,
            real_of(&args[2], name)?,
        ];
        return Ok(Value::Matrix(column(pixel(colour))?));
    }
    let m = expect_matrix(&args[0], name)?;
    let (rows, cols) = m.shape();
    ctx.charge(4 * m.data().len() as u64)?;
    if rows == 3 && cols == 1 {
        let colour = [
            scalar_real(m.get(0, 0)?, name)?,
            scalar_real(m.get(1, 0)?, name)?,
            scalar_real(m.get(2, 0)?, name)?,
        ];
        return Ok(Value::Matrix(column(pixel(colour))?));
    }
    if cols == 0 || cols % 3 != 0 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a 3 x 1 vector, three scalars, or a packed image matrix"
        )));
    }
    let w = cols / 3;
    let mut data = vec![Scalar::real(0.0, Dimension::dimensionless()); rows * cols];
    for i in 0..rows {
        for j in 0..w {
            let colour = [
                scalar_real(m.get(i, j)?, name)?,
                scalar_real(m.get(i, j + w)?, name)?,
                scalar_real(m.get(i, j + 2 * w)?, name)?,
            ];
            let out = pixel(colour);
            data[i * cols + j] = real_scalar(out[0]);
            data[i * cols + j + w] = real_scalar(out[1]);
            data[i * cols + j + 2 * w] = real_scalar(out[2]);
        }
    }
    Ok(Value::Matrix(Matrix::new(rows, cols, data)?))
}

/// `invertimage(M)`: each element becomes `255 - x` when the largest element is at most 255,
/// otherwise `max + min - x`.
fn invertimage(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let m = expect_matrix(&args[0], "invertimage")?;
    let values = real_values(m, "invertimage")?;
    ctx.charge(values.len() as u64)?;
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let flip = if max <= 255.0 { 255.0 } else { max + min };
    map_values(m, |x| flip - x)
}

/// `clip(M, [min, max])` or `clip(M, min, max)`: elements are limited to the range; the default
/// range is 0 to 255.
fn clip(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (lo, hi) = limits(args, "clip")?;
    let m = expect_matrix(&args[0], "clip")?;
    ctx.charge(m.data().len() as u64)?;
    map_values(m, |x| x.clamp(lo, hi))
}

/// `scale(M, [min, max])` or `scale(M, min, max)`: the smallest element goes to `min` and the
/// largest to `max`, the rest linearly between. A constant matrix gives `min` everywhere.
fn scale(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (lo, hi) = limits(args, "scale")?;
    let m = expect_matrix(&args[0], "scale")?;
    let values = real_values(m, "scale")?;
    ctx.charge(values.len() as u64)?;
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    if max == min {
        return map_values(m, |_| lo);
    }
    let slope = (hi - lo) / (max - min);
    map_values(m, |x| lo + (x - min) * slope)
}

/// The `[min, max]` limits of `clip` and `scale`: the default 0 and 255, a two-element vector, or
/// two scalars.
fn limits(args: &[Value], name: &str) -> Result<(f64, f64), EvalError> {
    let (lo, hi) = match args.len() {
        1 => (0.0, 255.0),
        2 => {
            let v = expect_matrix(&args[1], name)?;
            if v.data().len() != 2 {
                return Err(EvalError::Unsupported(format!(
                    "{name} needs its limits as [min, max]"
                )));
            }
            (
                scalar_real(v.get(0, 0)?, name)?,
                scalar_real(v.get(v.rows() - 1, v.cols() - 1)?, name)?,
            )
        }
        _ => (real_of(&args[1], name)?, real_of(&args[2], name)?),
    };
    if lo > hi {
        return Err(EvalError::Unsupported(format!("{name} needs min <= max")));
    }
    Ok((lo, hi))
}

/// RGB (0 to 255) to HSV: `[hue in degrees; saturation 0..255; value 0..255]`.
fn to_hsv(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = unit(rgb);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    [hue(r, g, b, max, delta), saturation * 255.0, max * 255.0]
}

/// RGB (0 to 255) to HLS: `[hue in degrees; lightness 0..255; saturation 0..255]`.
fn to_hls(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = unit(rgb);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let lightness = (max + min) / 2.0;
    let saturation = if delta == 0.0 {
        0.0
    } else if lightness <= 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };
    [
        hue(r, g, b, max, delta),
        lightness * 255.0,
        saturation * 255.0,
    ]
}

/// HSV (hue in degrees, 0 to 255 for the other two) to RGB (0 to 255).
fn from_hsv(hsv: [f64; 3]) -> [f64; 3] {
    let h = hsv[0].rem_euclid(360.0);
    let s = (hsv[1] / 255.0).clamp(0.0, 1.0);
    let v = (hsv[2] / 255.0).clamp(0.0, 1.0);
    let c = v * s;
    let [r1, g1, b1] = sector_rgb(h, c);
    let m = v - c;
    [(r1 + m) * 255.0, (g1 + m) * 255.0, (b1 + m) * 255.0]
}

/// HLS (hue in degrees, 0 to 255 for the other two) to RGB (0 to 255).
fn from_hls(hls: [f64; 3]) -> [f64; 3] {
    let h = hls[0].rem_euclid(360.0);
    let l = (hls[1] / 255.0).clamp(0.0, 1.0);
    let s = (hls[2] / 255.0).clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let [r1, g1, b1] = sector_rgb(h, c);
    let m = l - c / 2.0;
    [(r1 + m) * 255.0, (g1 + m) * 255.0, (b1 + m) * 255.0]
}

/// The hue (degrees, 0 to 360) of an RGB colour on the unit scale. Grey has hue 0.
fn hue(r: f64, g: f64, b: f64, max: f64, delta: f64) -> f64 {
    if delta == 0.0 {
        return 0.0;
    }
    let sector = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    60.0 * sector
}

/// The unit-scale RGB of a 0 to 255 colour part of the conversion, with the chroma `c` of the
/// hue sector of `h` (degrees in [0, 360)). Returns the colour before the lightness offset.
fn sector_rgb(h: f64, c: f64) -> [f64; 3] {
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    match hp.floor() as i64 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    }
}

fn unit(rgb: [f64; 3]) -> [f64; 3] {
    rgb.map(|x| (x / 255.0).clamp(0.0, 1.0))
}

/// A three-element column vector.
fn column(xs: [f64; 3]) -> Result<Matrix, EvalError> {
    Matrix::new(3, 1, xs.iter().map(|&x| real_scalar(x)).collect())
}

/// The matrix `m` with `f` applied to every element (dimensionless).
fn map_values(m: &Matrix, f: impl Fn(f64) -> f64) -> Result<Value, EvalError> {
    let data = real_values(m, "image")?
        .into_iter()
        .map(|x| real_scalar(f(x)))
        .collect();
    Ok(Value::Matrix(Matrix::new(m.rows(), m.cols(), data)?))
}

/// The real values of a matrix, which must be dimensionless.
fn real_values(m: &Matrix, name: &str) -> Result<Vec<f64>, EvalError> {
    m.data().iter().map(|s| scalar_real(s, name)).collect()
}

fn real_of(v: &Value, name: &str) -> Result<f64, EvalError> {
    match v {
        Value::Scalar(s) => scalar_real(s, name),
        _ => Err(EvalError::Unsupported(format!(
            "{name} needs scalar colour values here"
        ))),
    }
}

fn scalar_real(s: &Scalar, name: &str) -> Result<f64, EvalError> {
    ops::require_dimensionless(s.dim)?;
    if s.value.im != 0.0 || !s.value.re.is_finite() {
        return Err(EvalError::Unsupported(format!(
            "{name} needs real, finite values"
        )));
    }
    Ok(s.value.re)
}

fn real_scalar(x: f64) -> Scalar {
    Scalar::real(x, Dimension::dimensionless())
}

#[cfg(test)]
mod flip_tests {
    use super::*;

    fn grid(rows: usize, cols: usize) -> Value {
        let data = (1..=rows * cols).map(|k| real_scalar(k as f64)).collect();
        Value::Matrix(Matrix::new(rows, cols, data).unwrap())
    }

    fn values(v: Result<Value, EvalError>) -> Vec<f64> {
        match v {
            Ok(Value::Matrix(m)) => m.data().iter().map(|s| s.value.re).collect(),
            other => panic!("expected a matrix, got {other:?}"),
        }
    }

    #[test]
    fn horzflip_reverses_columns_and_vertflip_reverses_rows() {
        let ctx = BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3));
        assert_eq!(
            values(horzflip(&ctx, &[grid(2, 3)])),
            vec![3.0, 2.0, 1.0, 6.0, 5.0, 4.0]
        );
        assert_eq!(
            values(vertflip(&ctx, &[grid(2, 3)])),
            vec![4.0, 5.0, 6.0, 1.0, 2.0, 3.0]
        );
    }

    #[test]
    fn flips_refuse_scalars() {
        let ctx = BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3));
        let s = real_scalar(2.0);
        assert!(horzflip(&ctx, &[Value::Scalar(s)]).is_err());
    }
}

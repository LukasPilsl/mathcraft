//! Geometric transforms: `translate`, `zoom`, `rotate`, and `putregion` (copying a block into an
//! image).
//!
//! - `translate(M, rows, cols, pad)`: `out(r, c) = M(r - rows, c - cols)`, so a positive `rows` moves
//!   the content down and a positive `cols` to the right (the help's example: `translate(M2, 2, -1,
//!   150)` moves the rows down by two and left by one). Uncovered elements are `pad`.
//! - `zoom(M, hscale, vscale)`: the help's example fixes the roles: `zoom(M, 2, 3)` of a 3 x 4 matrix
//!   is 6 x 12, and `hscale = 2, vscale = 0.5` doubles the height and halves the width. So the first
//!   argument scales the number of rows and the second the number of columns, whatever the names say.
//!   Sizes are `round(n * scale)` (at least 1); each output element copies its nearest source element
//!   (nearest in the sense of the cell it falls in).
//! - `rotate(M, angle)`: counter-clockwise in screen orientation (row 0 at the top). The canvas is the
//!   bounding box of the rotated image; uncovered elements are 0; the value at a point is bilinear
//!   between the four source elements around it, a source element outside the image counting as 0
//!   (the OpenCV convention). The help's example output (`rotate(M, 35)` for the 3 x 4 matrix with
//!   `M[i][j] = i + j^2`, a 5 x 4 matrix) is NOT reproduced. Finding (see the test
//!   `rotate_help_example_is_not_reproduced`): no rigid rotation, affine map or interpolation kernel
//!   (nearest, bilinear with clamping or zero taps, bicubic, forward splat) matches all 20 entries.
//!   The best rigid fit (zero-tap bilinear, translation free) leaves a squared error of about 7.8
//!   over the 20 entries, and its canvas is 5 x 4 where this bounding box gives 5 x 5 (the bounding
//!   width is 4.998, so no rounding rule gives 4 columns while also giving 5 rows).
//! - `putregion(M, N, row, col)`: `N` written into a copy of `M` with its top-left at `(row, col)`,
//!   counted from `ORIGIN`; a block that does not fit is refused.

use std::f64::consts::PI;

use crate::eval::builtins::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Value, MAX_ELEMENTS};

use super::util::{charge, index_arg, int_arg, plane, plane_value, real_arg, Plane};

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("translate", Arity::Exact(4), translate);
    r.register_ctx("zoom", Arity::Exact(3), zoom);
    r.register_ctx("rotate", Arity::Exact(2), rotate);
    r.register_ctx("putregion", Arity::Exact(4), putregion);
}

fn translate(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "translate")?;
    let dr = int_arg(&args[1], "translate")?;
    let dc = int_arg(&args[2], "translate")?;
    let pad = real_arg(&args[3], "translate")?;
    charge(ctx, p.len())?;
    let mut out = vec![pad; p.len()];
    for r in 0..p.rows {
        for c in 0..p.cols {
            let sr = r as i64 - dr;
            let sc = c as i64 - dc;
            if sr >= 0 && sc >= 0 && (sr as usize) < p.rows && (sc as usize) < p.cols {
                out[r * p.cols + c] = p.get(sr as usize, sc as usize);
            }
        }
    }
    plane_value(p.rows, p.cols, out)
}

/// The size `round(n * scale)`, at least 1, for a positive finite scale.
fn scaled_len(n: usize, scale: f64, name: &str) -> Result<usize, EvalError> {
    if scale.is_nan() || scale <= 0.0 || !scale.is_finite() {
        return Err(EvalError::Unsupported(format!(
            "{name} needs positive scale factors"
        )));
    }
    let len = (n as f64 * scale).round();
    if len < 1.0 || len > MAX_ELEMENTS as f64 {
        return Err(EvalError::TooLarge(len.min(usize::MAX as f64) as usize));
    }
    Ok(len as usize)
}

fn zoom(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "zoom")?;
    let a = real_arg(&args[1], "zoom")?;
    let b = real_arg(&args[2], "zoom")?;
    let rows = scaled_len(p.rows, a, "zoom")?;
    let cols = scaled_len(p.cols, b, "zoom")?;
    let total = rows.checked_mul(cols).ok_or(EvalError::Overflow)?;
    if total > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(total));
    }
    charge(ctx, total.saturating_mul(3))?;
    let mut out = Vec::with_capacity(total);
    for r in 0..rows {
        let sr =
            (((r as f64 + 0.5) * p.rows as f64 / rows as f64).floor() as usize).min(p.rows - 1);
        for c in 0..cols {
            let sc =
                (((c as f64 + 0.5) * p.cols as f64 / cols as f64).floor() as usize).min(p.cols - 1);
            out.push(p.get(sr, sc));
        }
    }
    plane_value(rows, cols, out)
}

/// `rotate(M, angle)`: see the module note for the canvas and the sampling.
fn rotate(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "rotate")?;
    let angle = real_arg(&args[1], "rotate")?;
    let theta = angle * PI / 180.0;
    let (s, c) = theta.sin_cos();
    let (rf, cf) = (p.rows as f64, p.cols as f64);
    // The bounding box, with a tolerance so that a quarter turn keeps its exact size.
    let w = (cf * c.abs() + rf * s.abs() - 1e-9).ceil().max(1.0) as usize;
    let h = (rf * c.abs() + cf * s.abs() - 1e-9).ceil().max(1.0) as usize;
    let total = w.checked_mul(h).ok_or(EvalError::Overflow)?;
    if total > MAX_ELEMENTS {
        return Err(EvalError::TooLarge(total));
    }
    charge(ctx, total.saturating_mul(4))?;
    let (scy, scx) = ((rf - 1.0) / 2.0, (cf - 1.0) / 2.0);
    let (dcy, dcx) = ((h as f64 - 1.0) / 2.0, (w as f64 - 1.0) / 2.0);
    let mut out = vec![0.0; total];
    for r in 0..h {
        for col in 0..w {
            // Offsets from the canvas centre, y downwards. The source point is found by the clockwise
            // turn that undoes the counter-clockwise turn of the picture.
            let u = col as f64 - dcx;
            let v = r as f64 - dcy;
            let sx = scx + u * c - v * s;
            let sy = scy + u * s + v * c;
            out[r * w + col] = sample(&p, sy, sx);
        }
    }
    plane_value(h, w, out)
}

/// Bilinear sample of the image at `(y, x)`. The point is valid inside `(-1, rows) x (-1, cols)`
/// (0 outside); each of the four taps outside the image contributes 0.
fn sample(p: &Plane, y: f64, x: f64) -> f64 {
    let (rows, cols) = (p.rows as isize, p.cols as isize);
    if y <= -1.0 || x <= -1.0 || y >= rows as f64 || x >= cols as f64 {
        return 0.0;
    }
    let y0 = y.floor();
    let x0 = x.floor();
    let fy = y - y0;
    let fx = x - x0;
    let (y0, x0) = (y0 as isize, x0 as isize);
    let tap = |r: isize, c: isize| p.at(r, c).unwrap_or(0.0);
    tap(y0, x0) * (1.0 - fy) * (1.0 - fx)
        + tap(y0, x0 + 1) * (1.0 - fy) * fx
        + tap(y0 + 1, x0) * fy * (1.0 - fx)
        + tap(y0 + 1, x0 + 1) * fy * fx
}

/// `putregion(M, N, row, col)`: `N` copied into `M` at `(row, col)` (counted from `ORIGIN`).
fn putregion(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let m = plane(&args[0], "putregion")?;
    let n = plane(&args[1], "putregion")?;
    let origin = ctx.origin()?;
    let row = index_arg(&args[2], "putregion", origin)?;
    let col = index_arg(&args[3], "putregion", origin)?;
    if row + n.rows > m.rows || col + n.cols > m.cols {
        return Err(EvalError::Unsupported(
            "putregion: the block does not fit inside the image".to_string(),
        ));
    }
    charge(ctx, m.len().saturating_add(n.len()))?;
    let mut out = m.data.clone();
    for i in 0..n.rows {
        for j in 0..n.cols {
            out[(row + i) * m.cols + col + j] = n.get(i, j);
        }
    }
    plane_value(m.rows, m.cols, out)
}

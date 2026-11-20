//! Convolution filters: the edge finders (`sobel`, `prewitt`, `roberts`, `freichen`, the compass
//! comparisons `kirsch`, `robinson3`, `robinson5`, `compgrad`), the row and column gradients, the
//! centre-weighted smoothing `centsmooth`, the median and quantile filters `getnoise` and
//! `quantfilt`, and the Wiener filter `wiener2d`.
//!
//! Kernels are the ones the help's worked examples print (the scale factors are in the examples).
//! The two-kernel edge finders return `sqrt(g1^2 + g2^2)`: the help gives the two kernels but not
//! how they are combined; the gradient magnitude is the usual rule (assumption). The compass
//! finders return the absolute maximum of their eight responses, as the help states. The perimeter
//! of an edge finder's output is zero, since the help says the kernels do not overlap there.
//! Results are real; the help gives no rounding rule for them, so none is applied.

use crate::eval::builtins::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::Value;

use super::util::{charge, int_arg, plane, plane_value, quantile, real_arg, Plane};

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("sobel", Arity::Exact(1), sobel);
    r.register_ctx("prewitt", Arity::Exact(1), prewitt);
    r.register_ctx("roberts", Arity::Exact(1), roberts);
    r.register_ctx("freichen", Arity::Exact(1), freichen);
    r.register_ctx("kirsch", Arity::Exact(1), kirsch);
    r.register_ctx("robinson3", Arity::Exact(1), robinson3);
    r.register_ctx("robinson5", Arity::Exact(1), robinson5);
    r.register_ctx("compgrad", Arity::Exact(1), compgrad);
    r.register_ctx("rowgrad", Arity::Exact(1), rowgrad);
    r.register_ctx("colgrad", Arity::Exact(1), colgrad);
    r.register_ctx("centsmooth", Arity::Exact(1), centsmooth);
    r.register_ctx("getnoise", Arity::Exact(1), getnoise);
    r.register_ctx("quantfilt", Arity::Exact(3), quantfilt);
    r.register_ctx("wiener2d", Arity::Exact(3), wiener2d);
}

type Kernel = [[f64; 3]; 3];

const SOBEL_X: Kernel = [[1.0, 0.0, -1.0], [2.0, 0.0, -2.0], [1.0, 0.0, -1.0]];
const SOBEL_Y: Kernel = [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]];
const PREWITT_X: Kernel = [[1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 0.0, -1.0]];
const PREWITT_Y: Kernel = [[-1.0, -1.0, -1.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
const ROBERTS_1: Kernel = [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];
const ROBERTS_2: Kernel = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];

/// Frei-Chen kernels, already scaled by `1 / (2 + sqrt 2)`.
fn freichen_kernels() -> (Kernel, Kernel) {
    let s = std::f64::consts::SQRT_2;
    let k = 1.0 / (2.0 + s);
    let scale = |m: Kernel| m.map(|row| row.map(|x| x * k));
    (
        scale([[1.0, 0.0, -1.0], [s, 0.0, -s], [1.0, 0.0, -1.0]]),
        scale([[-1.0, -s, -1.0], [0.0, 0.0, 0.0], [1.0, s, 1.0]]),
    )
}

/// The eight Kirsch masks (scale 1/15), the Robinson 3-level masks (scale 1/3) and the Robinson
/// 5-level masks (scale 1/4), and the compass gradient masks (scale 1/5), in the help's order.
const KIRSCH: [Kernel; 8] = [
    [[5.0, -3.0, -3.0], [5.0, 0.0, -3.0], [5.0, -3.0, -3.0]],
    [[5.0, 5.0, -3.0], [5.0, 0.0, -3.0], [-3.0, -3.0, -3.0]],
    [[5.0, 5.0, 5.0], [-3.0, 0.0, -3.0], [-3.0, -3.0, -3.0]],
    [[-3.0, 5.0, 5.0], [-3.0, 0.0, 5.0], [-3.0, -3.0, -3.0]],
    [[-3.0, -3.0, 5.0], [-3.0, 0.0, 5.0], [-3.0, -3.0, 5.0]],
    [[-3.0, -3.0, -3.0], [-3.0, 0.0, 5.0], [-3.0, 5.0, 5.0]],
    [[-3.0, -3.0, -3.0], [-3.0, 0.0, -3.0], [5.0, 5.0, 5.0]],
    [[-3.0, -3.0, -3.0], [5.0, 0.0, -3.0], [5.0, 5.0, -3.0]],
];
const ROBINSON3: [Kernel; 8] = [
    [[1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 0.0, -1.0]],
    [[1.0, 1.0, 0.0], [1.0, 0.0, -1.0], [0.0, -1.0, -1.0]],
    [[1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [-1.0, -1.0, -1.0]],
    [[0.0, 1.0, 1.0], [-1.0, 0.0, 1.0], [-1.0, -1.0, 0.0]],
    [[-1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], [-1.0, 0.0, 1.0]],
    [[-1.0, -1.0, 0.0], [-1.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
    [[-1.0, -1.0, -1.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
    [[0.0, -1.0, -1.0], [1.0, 0.0, -1.0], [1.0, 1.0, 0.0]],
];
const ROBINSON5: [Kernel; 8] = [
    [[1.0, 0.0, -1.0], [2.0, 0.0, -2.0], [1.0, 0.0, -1.0]],
    [[2.0, 1.0, 0.0], [1.0, 0.0, -1.0], [0.0, -1.0, -2.0]],
    [[1.0, 2.0, 1.0], [0.0, 0.0, 0.0], [-1.0, -2.0, -1.0]],
    [[0.0, 1.0, 2.0], [-1.0, 0.0, 1.0], [-2.0, -1.0, 0.0]],
    [[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]],
    [[-2.0, -1.0, 0.0], [-1.0, 0.0, 1.0], [0.0, 1.0, 2.0]],
    [[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]],
    [[0.0, -1.0, -2.0], [1.0, 0.0, -1.0], [2.0, 1.0, 0.0]],
];
const COMPGRAD: [Kernel; 8] = [
    [[1.0, 1.0, -1.0], [1.0, -2.0, -1.0], [1.0, 1.0, -1.0]],
    [[1.0, 1.0, 1.0], [1.0, -2.0, -1.0], [1.0, -1.0, -1.0]],
    [[1.0, 1.0, 1.0], [1.0, -2.0, 1.0], [-1.0, -1.0, -1.0]],
    [[1.0, 1.0, 1.0], [-1.0, -2.0, 1.0], [-1.0, -1.0, 1.0]],
    [[-1.0, 1.0, 1.0], [-1.0, -2.0, 1.0], [-1.0, 1.0, 1.0]],
    [[-1.0, -1.0, 1.0], [-1.0, -2.0, 1.0], [1.0, 1.0, 1.0]],
    [[-1.0, -1.0, -1.0], [1.0, -2.0, 1.0], [1.0, 1.0, 1.0]],
    [[1.0, -1.0, -1.0], [1.0, -2.0, -1.0], [1.0, 1.0, 1.0]],
];

/// The 3 x 3 window sum at `(r, c)` with the kernel's centre on `(r, c)`, for interior pixels.
fn window_sum(p: &Plane, r: usize, c: usize, k: &Kernel) -> f64 {
    let mut s = 0.0;
    for (i, row) in k.iter().enumerate() {
        for (j, w) in row.iter().enumerate() {
            if *w != 0.0 {
                let y = r as isize + i as isize - 1;
                let x = c as isize + j as isize - 1;
                // Interior pixels only are asked for, so the window is inside the image.
                s += w * p.at(y, x).unwrap_or(0.0);
            }
        }
    }
    s
}

/// Output of an edge finder: `f(r, c)` for interior pixels, zero on the perimeter.
fn interior(p: &Plane, mut f: impl FnMut(usize, usize) -> f64) -> Result<Value, EvalError> {
    let mut out = vec![0.0; p.len()];
    for r in 1..p.rows.saturating_sub(1) {
        for c in 1..p.cols.saturating_sub(1) {
            out[r * p.cols + c] = f(r, c);
        }
    }
    plane_value(p.rows, p.cols, out)
}

fn two_kernel(
    ctx: &BuiltinCtx<'_>,
    name: &str,
    arg: &Value,
    kx: Kernel,
    ky: Kernel,
) -> Result<Value, EvalError> {
    let p = plane(arg, name)?;
    charge(ctx, p.len() * 18)?;
    interior(&p, |r, c| {
        let gx = window_sum(&p, r, c, &kx);
        let gy = window_sum(&p, r, c, &ky);
        gx.hypot(gy)
    })
}

fn compass(
    ctx: &BuiltinCtx<'_>,
    name: &str,
    arg: &Value,
    masks: &[Kernel; 8],
    divisor: f64,
) -> Result<Value, EvalError> {
    let p = plane(arg, name)?;
    charge(ctx, p.len() * 80)?;
    interior(&p, |r, c| {
        masks
            .iter()
            .map(|k| (window_sum(&p, r, c, k) / divisor).abs())
            .fold(0.0, f64::max)
    })
}

/// `sobel(M)`: gradient magnitude of the Sobel kernels (scale 1/4).
fn sobel(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let s = 0.25;
    let kx = SOBEL_X.map(|row| row.map(|x| x * s));
    let ky = SOBEL_Y.map(|row| row.map(|x| x * s));
    two_kernel(ctx, "sobel", &args[0], kx, ky)
}

/// `prewitt(M)`: gradient magnitude of the Prewitt kernels (scale 1/3).
fn prewitt(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let s = 1.0 / 3.0;
    let kx = PREWITT_X.map(|row| row.map(|x| x * s));
    let ky = PREWITT_Y.map(|row| row.map(|x| x * s));
    two_kernel(ctx, "prewitt", &args[0], kx, ky)
}

/// `roberts(M)`: gradient magnitude of the Roberts diagonal kernels.
fn roberts(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    two_kernel(ctx, "roberts", &args[0], ROBERTS_1, ROBERTS_2)
}

/// `freichen(M)`: gradient magnitude of the two Frei-Chen kernels.
fn freichen(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (kx, ky) = freichen_kernels();
    two_kernel(ctx, "freichen", &args[0], kx, ky)
}

/// `kirsch(M)`: largest absolute Kirsch response (scale 1/15).
fn kirsch(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    compass(ctx, "kirsch", &args[0], &KIRSCH, 15.0)
}

/// `robinson3(M)`: largest absolute 3-level Robinson response (scale 1/3).
fn robinson3(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    compass(ctx, "robinson3", &args[0], &ROBINSON3, 3.0)
}

/// `robinson5(M)`: largest absolute 5-level Robinson response (scale 1/4). The help's text says
/// 5 x 5 masks, but its worked example prints these 3 x 3 masks; the example is followed.
fn robinson5(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    compass(ctx, "robinson5", &args[0], &ROBINSON5, 4.0)
}

/// `compgrad(M)`: largest absolute compass-gradient response (scale 1/5).
fn compgrad(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    compass(ctx, "compgrad", &args[0], &COMPGRAD, 5.0)
}

/// `rowgrad(M)`: each row minus the row above it; the first row is zero.
fn rowgrad(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "rowgrad")?;
    charge(ctx, p.len())?;
    let mut out = vec![0.0; p.len()];
    for r in 1..p.rows {
        for c in 0..p.cols {
            out[r * p.cols + c] = p.get(r, c) - p.get(r - 1, c);
        }
    }
    plane_value(p.rows, p.cols, out)
}

/// `colgrad(M)`: each column minus the column before it; the first column is zero.
fn colgrad(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "colgrad")?;
    charge(ctx, p.len())?;
    let mut out = vec![0.0; p.len()];
    for r in 0..p.rows {
        for c in 1..p.cols {
            out[r * p.cols + c] = p.get(r, c) - p.get(r, c - 1);
        }
    }
    plane_value(p.rows, p.cols, out)
}

/// `centsmooth(M)`: 3 x 3 smoothing with kernel `[1 1 1; 1 2 1; 1 1 1] / 10`. At the edges only the
/// part of the kernel over the image is used, divided by the sum of its weights (as the help says).
fn centsmooth(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "centsmooth")?;
    charge(ctx, p.len() * 18)?;
    let mut out = vec![0.0; p.len()];
    for r in 0..p.rows {
        for c in 0..p.cols {
            let mut num = 0.0;
            let mut den = 0.0;
            for i in -1isize..=1 {
                for j in -1isize..=1 {
                    if let Some(x) = p.at(r as isize + i, c as isize + j) {
                        let w = if i == 0 && j == 0 { 2.0 } else { 1.0 };
                        num += w * x;
                        den += w;
                    }
                }
            }
            out[r * p.cols + c] = num / den;
        }
    }
    plane_value(p.rows, p.cols, out)
}

/// `getnoise(M)`: the 3 x 3 median-filtered image minus `M`, as the help's example computes it
/// (`F - noisy`). The median uses the pixels of the window that lie in the image.
fn getnoise(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "getnoise")?;
    charge(ctx, p.len() * 30)?;
    let mut out = vec![0.0; p.len()];
    let mut window = Vec::with_capacity(9);
    for r in 0..p.rows {
        for c in 0..p.cols {
            window.clear();
            for i in -1isize..=1 {
                for j in -1isize..=1 {
                    if let Some(x) = p.at(r as isize + i, c as isize + j) {
                        window.push(x);
                    }
                }
            }
            window.sort_by(f64::total_cmp);
            let median = quantile(&window, 0.5);
            out[r * p.cols + c] = median - p.get(r, c);
        }
    }
    plane_value(p.rows, p.cols, out)
}

/// `quantfilt(M, elem, q)`: the `q` quantile (0 to 1, linear between ranks) of the pixels under the
/// non-zero elements of `elem`, centred on each pixel. The centre of an even-sized kernel is at
/// `floor(size / 2)`. Pixels outside the image are left out of the window.
fn quantfilt(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "quantfilt")?;
    let k = plane(&args[1], "quantfilt")?;
    let q = real_arg(&args[2], "quantfilt")?;
    if !(0.0..=1.0).contains(&q) {
        return Err(EvalError::Unsupported(
            "quantfilt needs a quantile between 0 and 1".to_string(),
        ));
    }
    let (ar, ac) = ((k.rows / 2) as isize, (k.cols / 2) as isize);
    let offsets: Vec<(isize, isize)> = (0..k.rows)
        .flat_map(|i| (0..k.cols).map(move |j| (i, j)))
        .filter(|&(i, j)| k.get(i, j) != 0.0)
        .map(|(i, j)| (i as isize - ar, j as isize - ac))
        .collect();
    if offsets.is_empty() {
        return Err(EvalError::Unsupported(
            "quantfilt needs a kernel with a non-zero element".to_string(),
        ));
    }
    charge(ctx, p.len() * offsets.len() * 8)?;
    let mut out = vec![0.0; p.len()];
    let mut window = Vec::with_capacity(offsets.len());
    for r in 0..p.rows {
        for c in 0..p.cols {
            window.clear();
            for &(i, j) in &offsets {
                if let Some(x) = p.at(r as isize + i, c as isize + j) {
                    window.push(x);
                }
            }
            if window.is_empty() {
                out[r * p.cols + c] = 0.0;
                continue;
            }
            window.sort_by(f64::total_cmp);
            out[r * p.cols + c] = quantile(&window, q);
        }
    }
    plane_value(p.rows, p.cols, out)
}

/// `wiener2d(M, win_h, win_w)`: the pixel-by-pixel adaptive Wiener filter of Lee (1980), as the help
/// gives it: `y = mean + (x - mean) * vx / (vx + vn)`, with the local mean and variance `vx` over the
/// window (zero outside the image, as the help says) and the noise variance `vn` taken as the mean
/// of the local variances over the image (Lee's estimate; the help says only "estimated from the
/// input image").
fn wiener2d(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "wiener2d")?;
    let wh = int_arg(&args[1], "wiener2d")?;
    let ww = int_arg(&args[2], "wiener2d")?;
    if !(1..=1 << 20).contains(&wh) || !(1..=1 << 20).contains(&ww) {
        return Err(EvalError::Unsupported(
            "wiener2d needs a window between 1 x 1 and 2^20 x 2^20".to_string(),
        ));
    }
    let (wh, ww) = (wh as usize, ww as usize);
    let n = (wh * ww) as f64;
    charge(ctx, p.len().saturating_mul(wh * ww).saturating_mul(4))?;
    let (oy, ox) = ((wh / 2) as isize, (ww / 2) as isize);
    let mut means = vec![0.0; p.len()];
    let mut vars = vec![0.0; p.len()];
    for r in 0..p.rows {
        for c in 0..p.cols {
            let (mut s, mut s2) = (0.0, 0.0);
            for i in 0..wh {
                for j in 0..ww {
                    let y = r as isize + i as isize - oy;
                    let x = c as isize + j as isize - ox;
                    let v = p.at(y, x).unwrap_or(0.0);
                    s += v;
                    s2 += v * v;
                }
            }
            let mean = s / n;
            means[r * p.cols + c] = mean;
            vars[r * p.cols + c] = (s2 / n - mean * mean).max(0.0);
        }
    }
    let noise = vars.iter().sum::<f64>() / vars.len() as f64;
    let mut out = vec![0.0; p.len()];
    for k in 0..p.len() {
        let total = vars[k] + noise;
        out[k] = if total > 0.0 {
            means[k] + (p.data[k] - means[k]) * vars[k] / total
        } else {
            means[k]
        };
    }
    plane_value(p.rows, p.cols, out)
}

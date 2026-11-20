//! Intensity maps and histograms: `threshold`, `binarize`, `equalize`, `levelmap`, and the
//! histograms `imhist`, `imhist2` and `hist2d`.
//!
//! Histogram bins follow the help's example: a value `x` in `[0, 256)` falls in bin
//! `floor(x / 256 * n)` (so `n = 256` gives one bin per level), and `imhist` ignores values outside
//! that range. `imhist2` spreads its bins over the data's own range `[min, max]`, the top value in
//! the last bin.

use crate::eval::builtins::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Value, MAX_ELEMENTS};

use super::util::{charge, int_arg, plane, plane_value, real_arg, Plane};

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("threshold", Arity::Exact(2), threshold);
    r.register_ctx("binarize", Arity::Range(1, 5), binarize);
    r.register_ctx("equalize", Arity::Exact(1), equalize);
    r.register_ctx("levelmap", Arity::Exact(2), levelmap);
    r.register_ctx("imhist", Arity::Exact(2), imhist);
    r.register_ctx("imhist2", Arity::Exact(2), imhist2);
    r.register_ctx("hist2d", Arity::Exact(3), hist2d);
}

/// `threshold(M, t)`: elements below `t` become zero when `t` is positive, elements above `-t`
/// become zero when `t` is negative, and every element becomes zero when `t` is zero. The help's
/// summary says "set to thresh" for the first case, its note and example say zero; the note is
/// followed.
fn threshold(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "threshold")?;
    let t = real_arg(&args[1], "threshold")?;
    charge(ctx, p.len())?;
    let out = p
        .data
        .iter()
        .map(|&x| {
            if t == 0.0 {
                0.0
            } else if t > 0.0 {
                if x < t {
                    0.0
                } else {
                    x
                }
            } else if x > -t {
                0.0
            } else {
                x
            }
        })
        .collect();
    plane_value(p.rows, p.cols, out)
}

/// `binarize(M)`, `binarize(M, low)`, or `binarize(M, low, high, in, out)`. The help's forms are
/// these three; a high threshold without its in and out values is refused, as the help says.
///
/// - No threshold: the Kittler and Illingworth minimum-error threshold of the grey-level histogram
///   (the criterion the help names). Elements at or below it are 0, the rest 1.
/// - One threshold: elements at or below it are 0, the rest 1.
/// - Four values: elements from `low` to `high` (inclusive: the help does not say which ends count)
///   become `in`, the rest `out`.
fn binarize(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "binarize")?;
    charge(ctx, p.len() + 256 * 256)?;
    let bins: Vec<usize> = p.data.iter().map(|&x| level_bin(x)).collect();
    let out: Vec<f64> = match args.len() {
        1 => {
            let t = kittler_threshold(&bins);
            bins.iter()
                .map(|&b| if b <= t { 0.0 } else { 1.0 })
                .collect()
        }
        2 => {
            let low = real_arg(&args[1], "binarize")?;
            p.data
                .iter()
                .map(|&x| if x <= low { 0.0 } else { 1.0 })
                .collect()
        }
        5 => {
            let low = real_arg(&args[1], "binarize")?;
            let high = real_arg(&args[2], "binarize")?;
            let inside = real_arg(&args[3], "binarize")?;
            let outside = real_arg(&args[4], "binarize")?;
            p.data
                .iter()
                .map(|&x| {
                    if (low..=high).contains(&x) {
                        inside
                    } else {
                        outside
                    }
                })
                .collect()
        }
        _ => {
            return Err(EvalError::Unsupported(
                "binarize takes (M), (M, low), or (M, low, high, in, out)".to_string(),
            ))
        }
    };
    plane_value(p.rows, p.cols, out)
}

/// The grey level of a pixel for histogram purposes: `floor(x)` clamped to `0..=255`.
fn level_bin(x: f64) -> usize {
    x.floor().clamp(0.0, 255.0) as usize
}

/// The Kittler and Illingworth minimum-error threshold over the 256-level histogram of `bins`: the
/// level `t` that minimises `J(t) = 1 + 2 (P1 ln s1 + P2 ln s2) - 2 (P1 ln P1 + P2 ln P2)` for the
/// two classes `0..=t` and `t+1..=255`. A class variance is floored at `1e-6` to keep the logarithm
/// finite on a flat class. Returns 0 when no split has two non-empty classes.
fn kittler_threshold(bins: &[usize]) -> usize {
    let mut hist = [0.0f64; 256];
    for &b in bins {
        hist[b] += 1.0;
    }
    let total = bins.len() as f64;
    let p: Vec<f64> = hist.iter().map(|h| h / total).collect();
    let class = |range: std::ops::RangeInclusive<usize>| -> (f64, f64) {
        let w: f64 = range.clone().map(|i| p[i]).sum();
        if w <= 0.0 {
            return (0.0, 0.0);
        }
        let mean: f64 = range.clone().map(|i| i as f64 * p[i]).sum::<f64>() / w;
        let var: f64 = range.map(|i| (i as f64 - mean).powi(2) * p[i]).sum::<f64>() / w;
        (w, var)
    };
    let mut best = (f64::INFINITY, 0usize);
    for t in 0..255 {
        let (p1, v1) = class(0..=t);
        let (p2, v2) = class(t + 1..=255);
        if p1 <= 0.0 || p2 <= 0.0 {
            continue;
        }
        let s1 = v1.max(1e-6).sqrt();
        let s2 = v2.max(1e-6).sqrt();
        let j = 1.0 + 2.0 * (p1 * s1.ln() + p2 * s2.ln()) - 2.0 * (p1 * p1.ln() + p2 * p2.ln());
        if j < best.0 {
            best = (j, t);
        }
    }
    best.1
}

/// `equalize(M)`: histogram equalization to a linear cumulative histogram. With `CK(k)` the number
/// of pixels at levels up to `k`, the level `k` maps to `round(255 (CK(k) - c0) / (N - c0))`, where
/// `c0` is the count at the lowest occupied level and `N` the number of pixels. This is the usual
/// rule (the help gives the cumulative histogram but not the final rounding). A flat image keeps
/// its level.
fn equalize(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "equalize")?;
    charge(ctx, p.len() + 256)?;
    let bins: Vec<usize> = p.data.iter().map(|&x| level_bin(x)).collect();
    let mut hist = [0.0f64; 256];
    for &b in &bins {
        hist[b] += 1.0;
    }
    let mut cum = [0.0f64; 256];
    let mut run = 0.0;
    for k in 0..256 {
        run += hist[k];
        cum[k] = run;
    }
    let n = p.len() as f64;
    let c0 = cum
        .iter()
        .zip(hist.iter())
        .find(|(_, h)| **h > 0.0)
        .map_or(0.0, |(c, _)| *c);
    let map: Vec<f64> = (0..256)
        .map(|k| {
            if n <= c0 {
                k as f64
            } else {
                (255.0 * (cum[k] - c0) / (n - c0)).round()
            }
        })
        .collect();
    let out = bins.iter().map(|&b| map[b]).collect();
    plane_value(p.rows, p.cols, out)
}

/// `levelmap(M, vec)`: each element `x` of `M` becomes `vec[x]`, counting the vector's elements
/// from zero (the help says levelmap ignores `ORIGIN`). Elements must be integers from 0 up to the
/// last index of `vec`. The help says the last index must equal the highest intensity; this reads
/// it as "at most", so a longer vector is accepted.
fn levelmap(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "levelmap")?;
    let v = plane(&args[1], "levelmap")?;
    if v.rows != 1 && v.cols != 1 {
        return Err(EvalError::Unsupported(
            "levelmap needs a vector of levels".to_string(),
        ));
    }
    charge(ctx, p.len() + v.len())?;
    let out = p
        .data
        .iter()
        .map(|&x| level_index(x, v.len()).map(|i| v.data[i]))
        .collect::<Result<Vec<f64>, _>>()?;
    plane_value(p.rows, p.cols, out)
}

fn level_index(x: f64, len: usize) -> Result<usize, EvalError> {
    if x.fract() != 0.0 || x < 0.0 || x >= len as f64 {
        return Err(EvalError::Unsupported(format!(
            "levelmap needs integer intensities from 0 to {}, found {x}",
            len.saturating_sub(1)
        )));
    }
    Ok(x as usize)
}

/// The bin of `x` among `n` bins over `[0, 256)`, or `None` when `x` lies outside that range.
fn fixed_bin(x: f64, n: usize) -> Option<usize> {
    if (0.0..256.0).contains(&x) {
        Some(((x * n as f64 / 256.0).floor() as usize).min(n - 1))
    } else {
        None
    }
}

/// Bin count argument: a positive integer no larger than the matrix size limit.
fn bin_count(v: &Value, name: &str) -> Result<usize, EvalError> {
    let n = int_arg(v, name)?;
    if n < 1 || n as u64 > MAX_ELEMENTS as u64 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a bin count from 1 up"
        )));
    }
    Ok(n as usize)
}

/// `imhist(M, n)`: an `n` x 1 column of counts over `n` bins of `[0, 256)`. Values outside that
/// range are not counted.
fn imhist(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "imhist")?;
    let n = bin_count(&args[1], "imhist")?;
    charge(ctx, p.len() + n)?;
    let mut counts = vec![0.0; n];
    for &x in &p.data {
        if let Some(b) = fixed_bin(x, n) {
            counts[b] += 1.0;
        }
    }
    plane_value(n, 1, counts)
}

/// `imhist2(M, n)`: an `n` x 1 column of counts over `n` bins spread across the data's range
/// `[min, max]`. The maximum falls in the last bin. A flat image puts everything in bin 0.
fn imhist2(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "imhist2")?;
    let n = bin_count(&args[1], "imhist2")?;
    charge(ctx, p.len() + n)?;
    let lo = p.data.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = p.data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut counts = vec![0.0; n];
    for &x in &p.data {
        let b = if hi > lo {
            (((x - lo) / (hi - lo)) * n as f64)
                .floor()
                .clamp(0.0, (n - 1) as f64) as usize
        } else {
            0
        };
        counts[b] += 1.0;
    }
    plane_value(n, 1, counts)
}

/// `hist2d(M, N, n)`: the `n` x `n` joint histogram of two equal-sized images. Entry `(i, j)` counts
/// the pixels whose value in `N` falls in bin `i` and whose value in `M` falls in bin `j`. This is
/// the layout of the help's worked example (`hist[Qbins, Pbins]`). Pairs with a value outside
/// `[0, 256)` are not counted.
fn hist2d(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let a: Plane = plane(&args[0], "hist2d")?;
    let b: Plane = plane(&args[1], "hist2d")?;
    if a.rows != b.rows || a.cols != b.cols {
        return Err(EvalError::ShapeMismatch {
            left: (a.rows, a.cols),
            right: (b.rows, b.cols),
        });
    }
    let n = bin_count(&args[2], "hist2d")?;
    match n.checked_mul(n) {
        Some(k) if k <= MAX_ELEMENTS => {}
        _ => return Err(EvalError::TooLarge(n.saturating_mul(n))),
    }
    charge(ctx, a.len() + n * n)?;
    let mut hist = vec![0.0; n * n];
    for (&x, &y) in a.data.iter().zip(b.data.iter()) {
        if let (Some(j), Some(i)) = (fixed_bin(x, n), fixed_bin(y, n)) {
            hist[i * n + j] += 1.0;
        }
    }
    plane_value(n, n, hist)
}

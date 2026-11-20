//! Binary morphology and shape functions: `erode`, `dilate`, `open`, `close`, `thin`, `skeleton`,
//! `cnvxhull` and `distform`.
//!
//! For `erode`, `dilate`, `open` and `close` a pixel is foreground when its value is above the
//! threshold `b` (the help: "above b are foreground"). For `thin` and `skeleton` it is at or above
//! `b` (see `foreground_at_least`). The structuring element's non-zero entries are its members. Its
//! origin is the entry at `(r_origin, c_origin)`, counted from zero; the help says only that the
//! signs "indicate direction", which is not described, so a negative origin is refused. Erosion keeps
//! `p` when every member offset from `p` is foreground; dilation sets `p` when some member offset
//! reflected through the origin is foreground. Pixels outside the image count as background. With the
//! origin among the members, erosion <= original <= dilation.
//!
//! `erode`, `dilate`, `open` and `close` give 255 and 0 (the help: "foreground is always set to
//! 255"). `thin` and `skeleton` give 1 and 0 (the help is silent; assumption). `open` and `close`
//! take no origin: the origin is the centre of the element, `(rows / 2, cols / 2)` rounded down
//! (assumption; the help gives no origin for them).
//!
//! Grayscale: `gray_erode(M, Melem, r, c)` and `gray_dilate` pad the image with zeros (the help's
//! example reproduced exactly, see the tests); `gray_open` and `gray_close` use origin (0, 0).

use crate::eval::builtins::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::Value;

use super::util::{charge, int_arg, plane, plane_value, real_arg, Plane};

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("erode", Arity::Exact(5), erode);
    r.register_ctx("dilate", Arity::Exact(5), dilate);
    r.register_ctx("open", Arity::Exact(3), open);
    r.register_ctx("close", Arity::Exact(3), close);
    r.register_ctx("gray_erode", Arity::Exact(4), gray_erode_fn);
    r.register_ctx("gray_dilate", Arity::Exact(4), gray_dilate_fn);
    r.register_ctx("gray_open", Arity::Exact(2), gray_open);
    r.register_ctx("gray_close", Arity::Exact(2), gray_close);
    r.register_ctx("thin", Arity::Exact(2), thin);
    r.register_ctx("skeleton", Arity::Range(1, 2), skeleton);
    r.register_ctx("cnvxhull", Arity::Exact(2), cnvxhull);
    r.register_ctx("distform", Arity::Exact(2), distform);
}

/// The members of a structuring element as offsets from its origin.
struct Element {
    members: Vec<(isize, isize)>,
}

/// Reads a structuring element and its origin (zero-based, inside the element).
fn element(v: &Value, r0: i64, c0: i64, name: &str) -> Result<Element, EvalError> {
    let k = plane(v, name)?;
    if r0 < 0 || c0 < 0 || r0 as usize >= k.rows || c0 as usize >= k.cols {
        return Err(EvalError::Unsupported(format!(
            "{name} needs the origin inside the structuring element"
        )));
    }
    let members: Vec<(isize, isize)> = (0..k.rows)
        .flat_map(|i| (0..k.cols).map(move |j| (i, j)))
        .filter(|&(i, j)| k.get(i, j) != 0.0)
        .map(|(i, j)| (i as isize - r0 as isize, j as isize - c0 as isize))
        .collect();
    if members.is_empty() {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a structuring element with a non-zero entry"
        )));
    }
    Ok(Element { members })
}

/// The 3 x 3 square element, origin at its centre.
fn square() -> Vec<(isize, isize)> {
    (-1..=1)
        .flat_map(|i| (-1..=1).map(move |j| (i, j)))
        .collect()
}

fn fg_at(fg: &[bool], rows: usize, cols: usize, r: isize, c: isize) -> bool {
    r >= 0
        && c >= 0
        && (r as usize) < rows
        && (c as usize) < cols
        && fg[r as usize * cols + c as usize]
}

fn erode_bin(fg: &[bool], rows: usize, cols: usize, members: &[(isize, isize)]) -> Vec<bool> {
    let mut out = vec![false; fg.len()];
    for r in 0..rows {
        for c in 0..cols {
            out[r * cols + c] = members
                .iter()
                .all(|&(i, j)| fg_at(fg, rows, cols, r as isize + i, c as isize + j));
        }
    }
    out
}

fn dilate_bin(fg: &[bool], rows: usize, cols: usize, members: &[(isize, isize)]) -> Vec<bool> {
    let mut out = vec![false; fg.len()];
    for r in 0..rows {
        for c in 0..cols {
            out[r * cols + c] = members
                .iter()
                .any(|&(i, j)| fg_at(fg, rows, cols, r as isize - i, c as isize - j));
        }
    }
    out
}

/// Foreground strictly above `b` (the help of `erode`, `dilate`, `open` and `close`).
fn foreground(p: &Plane, b: f64) -> Vec<bool> {
    p.data.iter().map(|&x| x > b).collect()
}

/// Foreground at or above `b`. `thin` and `skeleton` use this rule: the help's examples apply
/// `thin(S, 1)` and `skeleton(S, 1)` to a 0/1 matrix and show a result, which the strict rule
/// would leave empty. The help's text for them says only "binarized with threshold b".
fn foreground_at_least(p: &Plane, b: f64) -> Vec<bool> {
    p.data.iter().map(|&x| x >= b).collect()
}

fn to_255(bits: &[bool]) -> Vec<f64> {
    bits.iter().map(|&x| if x { 255.0 } else { 0.0 }).collect()
}

fn to_01(bits: &[bool]) -> Vec<f64> {
    bits.iter().map(|&x| if x { 1.0 } else { 0.0 }).collect()
}

/// `erode(M, Melem, r_origin, c_origin, b)`: binary erosion, output 255 and 0.
fn erode(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (p, e) = image_and_element(ctx, args, "erode")?;
    let bits = erode_bin(
        &foreground(&p, real_arg(&args[4], "erode")?),
        p.rows,
        p.cols,
        &e.members,
    );
    plane_value(p.rows, p.cols, to_255(&bits))
}

/// `dilate(M, Melem, r_origin, c_origin, b)`: binary dilation, output 255 and 0.
fn dilate(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (p, e) = image_and_element(ctx, args, "dilate")?;
    let bits = dilate_bin(
        &foreground(&p, real_arg(&args[4], "dilate")?),
        p.rows,
        p.cols,
        &e.members,
    );
    plane_value(p.rows, p.cols, to_255(&bits))
}

/// Reads `(M, Melem, r_origin, c_origin, b)` and charges the work of one erosion or dilation.
fn image_and_element(
    ctx: &BuiltinCtx<'_>,
    args: &[Value],
    name: &str,
) -> Result<(Plane, Element), EvalError> {
    let p = plane(&args[0], name)?;
    let r0 = int_arg(&args[2], name)?;
    let c0 = int_arg(&args[3], name)?;
    let e = element(&args[1], r0, c0, name)?;
    charge(ctx, p.len().saturating_mul(e.members.len()))?;
    Ok((p, e))
}

/// `open(M, Melem, b)`: erosion then dilation, output 1 and 0.
fn open(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (p, members) = binary_element(ctx, args, "open")?;
    let fg = foreground(&p, real_arg(&args[2], "open")?);
    let eroded = erode_bin(&fg, p.rows, p.cols, &members);
    let bits = dilate_bin(&eroded, p.rows, p.cols, &members);
    plane_value(p.rows, p.cols, to_255(&bits))
}

/// `close(M, Melem, b)`: dilation then erosion, output 1 and 0.
fn close(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let (p, members) = binary_element(ctx, args, "close")?;
    let fg = foreground(&p, real_arg(&args[2], "close")?);
    let dilated = dilate_bin(&fg, p.rows, p.cols, &members);
    let bits = erode_bin(&dilated, p.rows, p.cols, &members);
    plane_value(p.rows, p.cols, to_255(&bits))
}

/// A grayscale structuring element: every entry (zero entries included) is a member, with its offset
/// from the origin and its value.
fn gray_element(
    v: &Value,
    r0: i64,
    c0: i64,
    name: &str,
) -> Result<Vec<(isize, isize, f64)>, EvalError> {
    let k = plane(v, name)?;
    if k.len() == 0 || r0 < 0 || c0 < 0 || r0 as usize >= k.rows || c0 as usize >= k.cols {
        return Err(EvalError::Unsupported(format!(
            "{name} needs the origin inside a non-empty structuring element"
        )));
    }
    Ok((0..k.rows)
        .flat_map(|i| (0..k.cols).map(move |j| (i, j)))
        .map(|(i, j)| {
            (
                i as isize - r0 as isize,
                j as isize - c0 as isize,
                k.get(i, j),
            )
        })
        .collect())
}

/// Grayscale erosion: `out(p) = min over q of (M(p + q - origin) - Melem(q))`. The image counts as
/// zero outside its grid, so the border output can be negative (the help's example: a 5 x 4 ramp
/// eroded by `[1 1; 1 1]` gives -1 along the last column and row).
fn gray_erode_grid(p: &Plane, members: &[(isize, isize, f64)]) -> Vec<f64> {
    let mut out = vec![0.0; p.len()];
    for r in 0..p.rows {
        for c in 0..p.cols {
            out[r * p.cols + c] = members
                .iter()
                .map(|&(i, j, s)| p.at(r as isize + i, c as isize + j).unwrap_or(0.0) - s)
                .fold(f64::INFINITY, f64::min);
        }
    }
    out
}

/// Grayscale dilation: `out(p) = max over q of (M(p - (q - origin)) + Melem(q))`, the element
/// reflected through its origin; zero outside the grid (the help's example: `[1 1; 1 1]` at origin
/// (0, 0) adds 1 to the maximum of each ramp pixel and its neighbours).
fn gray_dilate_grid(p: &Plane, members: &[(isize, isize, f64)]) -> Vec<f64> {
    let mut out = vec![0.0; p.len()];
    for r in 0..p.rows {
        for c in 0..p.cols {
            out[r * p.cols + c] = members
                .iter()
                .map(|&(i, j, s)| p.at(r as isize - i, c as isize - j).unwrap_or(0.0) + s)
                .fold(f64::NEG_INFINITY, f64::max);
        }
    }
    out
}

/// `gray_erode(M, Melem, r_origin, c_origin)`.
fn gray_erode_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "gray_erode")?;
    let r0 = int_arg(&args[2], "gray_erode")?;
    let c0 = int_arg(&args[3], "gray_erode")?;
    let members = gray_element(&args[1], r0, c0, "gray_erode")?;
    charge(ctx, p.len().saturating_mul(members.len()))?;
    plane_value(p.rows, p.cols, gray_erode_grid(&p, &members))
}

/// `gray_dilate(M, Melem, r_origin, c_origin)`.
fn gray_dilate_fn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "gray_dilate")?;
    let r0 = int_arg(&args[2], "gray_dilate")?;
    let c0 = int_arg(&args[3], "gray_dilate")?;
    let members = gray_element(&args[1], r0, c0, "gray_dilate")?;
    charge(ctx, p.len().saturating_mul(members.len()))?;
    plane_value(p.rows, p.cols, gray_dilate_grid(&p, &members))
}

/// `gray_open(M, Melem)`: erosion then dilation, both with the origin at (0, 0). The help says the
/// result does not depend on the origin, so this is the origin the help's example uses.
fn gray_open(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "gray_open")?;
    let members = gray_element(&args[1], 0, 0, "gray_open")?;
    charge(ctx, p.len().saturating_mul(members.len()).saturating_mul(2))?;
    let eroded = Plane {
        rows: p.rows,
        cols: p.cols,
        data: gray_erode_grid(&p, &members),
    };
    plane_value(p.rows, p.cols, gray_dilate_grid(&eroded, &members))
}

/// `gray_close(M, Melem)`: dilation then erosion, both with the origin at (0, 0).
fn gray_close(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "gray_close")?;
    let members = gray_element(&args[1], 0, 0, "gray_close")?;
    charge(ctx, p.len().saturating_mul(members.len()).saturating_mul(2))?;
    let dilated = Plane {
        rows: p.rows,
        cols: p.cols,
        data: gray_dilate_grid(&p, &members),
    };
    plane_value(p.rows, p.cols, gray_erode_grid(&dilated, &members))
}

/// `(M, Melem, b)` for `open` and `close`: the element's origin is its centre.
fn binary_element(
    ctx: &BuiltinCtx<'_>,
    args: &[Value],
    name: &str,
) -> Result<(Plane, Vec<(isize, isize)>), EvalError> {
    let p = plane(&args[0], name)?;
    let k = plane(&args[1], name)?;
    let e = element(&args[1], (k.rows / 2) as i64, (k.cols / 2) as i64, name)?;
    charge(
        ctx,
        p.len().saturating_mul(e.members.len()).saturating_mul(2),
    )?;
    Ok((p, e.members))
}

/// Zhang-Suen thinning of a binary grid (the help's "recursively deletes simple border points that
/// have more than one neighbour", with the end points of arcs kept). Each pass removes the border
/// points of the first and then the second sub-iteration that satisfy the conditions; the passes
/// repeat until nothing is removed. Pixels outside the grid count as background.
fn thin_grid(
    ctx: &BuiltinCtx<'_>,
    mut fg: Vec<bool>,
    rows: usize,
    cols: usize,
) -> Result<Vec<bool>, EvalError> {
    loop {
        let mut changed = false;
        for step in 0..2 {
            charge(ctx, rows * cols * 16)?;
            let mut delete = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    if !fg[r * cols + c] {
                        continue;
                    }
                    let n = neighbours(&fg, rows, cols, r, c);
                    let b = n.iter().filter(|&&x| x).count();
                    let a = (0..8).filter(|&i| !n[i] && n[(i + 1) % 8]).count();
                    let (p2, p4, p6, p8) = (n[0], n[2], n[4], n[6]);
                    // Zhang-Suen: step 0 needs not(P2 P4 P6) and not(P4 P6 P8); step 1 needs
                    // not(P2 P4 P8) and not(P2 P6 P8). Written as the equivalent disjunctions.
                    let cond = if step == 0 {
                        !p4 || !p6 || (!p2 && !p8)
                    } else {
                        !p2 || !p8 || (!p4 && !p6)
                    };
                    if (2..=6).contains(&b) && a == 1 && cond {
                        delete.push(r * cols + c);
                    }
                }
            }
            if !delete.is_empty() {
                changed = true;
                for i in delete {
                    fg[i] = false;
                }
            }
        }
        if !changed {
            return Ok(fg);
        }
    }
}

/// The eight neighbours P2..P9 of `(r, c)`, clockwise from the pixel above.
fn neighbours(fg: &[bool], rows: usize, cols: usize, r: usize, c: usize) -> [bool; 8] {
    const OFF: [(isize, isize); 8] = [
        (-1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
    ];
    OFF.map(|(i, j)| fg_at(fg, rows, cols, r as isize + i, c as isize + j))
}

/// `thin(M, b)`: thinning of the image binarized at `b`, output 1 and 0.
fn thin(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "thin")?;
    let b = real_arg(&args[1], "thin")?;
    let bits = thin_grid(ctx, foreground_at_least(&p, b), p.rows, p.cols)?;
    plane_value(p.rows, p.cols, to_01(&bits))
}

/// `skeleton(B)` or `skeleton(B, b)`: the morphological skeleton of the binary image (Serra's
/// definition with the 3 x 3 square element: the union over `k` of `(A eroded k times) minus its
/// opening`). With `b`, the image is first binarized at `b`; without it, a non-zero element is
/// foreground. The help describes two algorithms for the no-threshold case and does not say which
/// applies, so the morphological skeleton is the one used (assumption). Output 1 and 0.
fn skeleton(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "skeleton")?;
    let mut a = match args.get(1) {
        Some(v) => foreground_at_least(&p, real_arg(v, "skeleton")?),
        None => p.data.iter().map(|&x| x != 0.0).collect(),
    };
    let sq = square();
    let n = p.len();
    let mut s = vec![false; n];
    while a.iter().any(|&x| x) {
        charge(ctx, n * 18)?;
        let eroded = erode_bin(&a, p.rows, p.cols, &sq);
        let opened = dilate_bin(&eroded, p.rows, p.cols, &sq);
        for i in 0..n {
            if a[i] && !opened[i] {
                s[i] = true;
            }
        }
        a = eroded;
    }
    plane_value(p.rows, p.cols, to_01(&s))
}

/// `cnvxhull(M, fg)`: the convex hull of the pixels equal to `fg`, as a 1 and 0 image; pixels on the
/// hull's boundary count as inside. The hull is Andrew's monotone chain on the pixel coordinates
/// (the help names the Rosenfeld and Kak algorithm, which finds the same hull; assumption that the
/// hulls agree).
fn cnvxhull(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "cnvxhull")?;
    let fg = real_arg(&args[1], "cnvxhull")?;
    let mut pts: Vec<(i64, i64)> = Vec::new();
    for r in 0..p.rows {
        for c in 0..p.cols {
            if p.get(r, c) == fg {
                pts.push((r as i64, c as i64));
            }
        }
    }
    let hull = monotone_chain(pts);
    charge(ctx, p.len().saturating_mul(hull.len().max(1)))?;
    let mut out = vec![0.0; p.len()];
    for r in 0..p.rows {
        for c in 0..p.cols {
            if inside_hull(&hull, (r as i64, c as i64)) {
                out[r * p.cols + c] = 1.0;
            }
        }
    }
    plane_value(p.rows, p.cols, out)
}

fn cross(o: (i64, i64), a: (i64, i64), b: (i64, i64)) -> i128 {
    let (ax, ay) = ((a.0 - o.0) as i128, (a.1 - o.1) as i128);
    let (bx, by) = ((b.0 - o.0) as i128, (b.1 - o.1) as i128);
    ax * by - ay * bx
}

/// Counter-clockwise hull vertices (no repeated last point). Fewer than three input points, or
/// collinear ones, give a degenerate hull of one or two vertices.
fn monotone_chain(mut pts: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    pts.sort_unstable();
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<(i64, i64)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(i64, i64)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Whether `p` is inside or on the boundary of the hull `h` (see [`monotone_chain`]).
fn inside_hull(h: &[(i64, i64)], p: (i64, i64)) -> bool {
    match h.len() {
        0 => false,
        1 => h[0] == p,
        2 => {
            cross(h[0], h[1], p) == 0
                && p.0 >= h[0].0.min(h[1].0)
                && p.0 <= h[0].0.max(h[1].0)
                && p.1 >= h[0].1.min(h[1].1)
                && p.1 <= h[0].1.max(h[1].1)
        }
        n => (0..n).all(|i| cross(h[i], h[(i + 1) % n], p) >= 0),
    }
}

/// `distform(M, fg)`: the Euclidean distance from each pixel to the nearest pixel equal to `fg`.
/// Refused when the image has no such pixel, since the distance is then undefined.
fn distform(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let p = plane(&args[0], "distform")?;
    let fg = real_arg(&args[1], "distform")?;
    let sources: Vec<(f64, f64)> = (0..p.rows)
        .flat_map(|r| (0..p.cols).map(move |c| (r, c)))
        .filter(|&(r, c)| p.get(r, c) == fg)
        .map(|(r, c)| (r as f64, c as f64))
        .collect();
    if sources.is_empty() {
        return Err(EvalError::Unsupported(
            "distform needs at least one pixel equal to fg".to_string(),
        ));
    }
    charge(ctx, p.len().saturating_mul(sources.len()))?;
    let mut out = vec![0.0; p.len()];
    for r in 0..p.rows {
        for c in 0..p.cols {
            let (y, x) = (r as f64, c as f64);
            out[r * p.cols + c] = sources
                .iter()
                .map(|&(sy, sx)| (y - sy).hypot(x - sx))
                .fold(f64::INFINITY, f64::min);
        }
    }
    plane_value(p.rows, p.cols, out)
}

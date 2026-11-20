//! Second data-analysis tier (Mathcad Prime help: "Polynomial Interpolation", "Local Maximum and
//! Minimum", "Median Smoothing", "Correlation and Contingency Tables", "Unit Scaling in Common
//! Formats", and the `conj`, `nthRoot` and `IsNaN` helpers).
//!
//! - `polyint(vx, vy, x)`: Neville's algorithm (Numerical Recipes `polint`), returns `[value; error]`
//!   with the error estimate of that algorithm.
//! - `polycoeff(vx, vy)`: the coefficients (ascending powers) of the interpolating polynomial, by
//!   building `prod (z - x_i)` and dividing out each factor (the Lagrange form). Unitless data only.
//! - `polyiter(vx, vy, x, N, e)`: Aitken-Neville iteration on the `k + 1` data points nearest `x`,
//!   for `k = 1 .. N`. Returns `[converged; iterations; value]`; converged when two successive
//!   estimates agree within `e`. The help's example pins the output shape only.
//! - `localmax(M1, [w])`, `localmin(M1, [w])`: rows `(x, y)` of `M1` (column 1 is x, column 2 the
//!   response) that exceed every point within `w` positions on each side. `localmax(x, y, M2, [w])`
//!   is the grid form: rows `(x_i, y_j, z)` of the matrix `M2` that exceed every grid neighbour
//!   within `w` in both directions. Points within `w` of an edge are not candidates.
//! - `VSmooth(v, w)`: repeated median smoothing (the `medsmooth` pass) per width in `w`, each width
//!   repeated until a pass changes nothing.
//! - `contingtbl(M)`: `[chi-squared; degrees of freedom; probability; Cramer's V; contingency C]`.
//! - `conj(z)`, `nthRoot(x, n)`, `IsNaN(x)`, `DMS(v)` and `FIF(s)`.
//!
//! Not here: `pareto` and `qqplot` (plots), and `markNaN`, `matchNaN`, `filterNaN`: NaN is refused by
//! the value model (see `eval/constants.rs`), so there is no NaN entry to mark, match or remove.
//! `IsNaN` is therefore always 0 for a value that exists.
//!
//! Work: every loop is charged before it runs, in `work_table` (class) or `work::builtin_units`
//! (formulas for the cubic and window loops).

use std::f64::consts::PI;

use num_complex::Complex64;

use super::data_util::{
    column, count, finite, grid, grid_result, one_dim, real_of, real_result, real_scalar, reals,
    vector,
};
use super::special::special_gamma as gam;
use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::{unit, Dimension, Quantity};

/// Registers the functions of this file.
pub fn register_data_tier2(r: &mut Registry) {
    r.register("conj", Arity::Exact(1), conj);
    r.register("nthRoot", Arity::Exact(2), nth_root);
    r.register("IsNaN", Arity::Exact(1), is_nan);
    r.register("polyint", Arity::Exact(3), polyint);
    r.register("polycoeff", Arity::Exact(2), polycoeff);
    r.register("polyiter", Arity::Exact(5), polyiter);
    r.register("localmax", Arity::Range(1, 4), localmax);
    r.register("localmin", Arity::Range(1, 4), localmin);
    r.register("VSmooth", Arity::Exact(2), vsmooth);
    r.register("contingtbl", Arity::Exact(1), contingtbl);
    r.register("DMS", Arity::Range(1, 3), dms);
    r.register("FIF", Arity::Range(1, 3), fif);
}

fn unsupported(msg: &str) -> EvalError {
    EvalError::Unsupported(msg.to_string())
}

fn dimensionless() -> Dimension {
    Dimension::dimensionless()
}

/// Two data vectors of one length, each of one dimension. Returns the reals and both dimensions.
fn data_pair(
    a: &Value,
    b: &Value,
    name: &str,
) -> Result<(Vec<f64>, Dimension, Vec<f64>, Dimension), EvalError> {
    let xs = vector(a, name)?;
    let ys = vector(b, name)?;
    if xs.len() != ys.len() {
        return Err(unsupported(&format!("{name}: vx and vy differ in length")));
    }
    let dx = one_dim(&xs)?;
    let dy = one_dim(&ys)?;
    Ok((reals(&xs, name)?, dx, reals(&ys, name)?, dy))
}

/// `conj(z)`: the complex conjugate, element-wise.
fn conj(args: &[Value]) -> Result<Value, EvalError> {
    ops::conjugate(&args[0])
}

/// `nthRoot(x, n)` for a dimensionless `x`. A negative real `x` with odd `n` has the real root
/// `-((-x)^(1/n))`; otherwise the principal root.
fn nth_root(args: &[Value]) -> Result<Value, EvalError> {
    let name = "nthRoot";
    let k = count(&args[1], name)?;
    let (rows, cols, data) = grid(&args[0], name)?;
    let e = 1.0 / k as f64;
    let out = data
        .iter()
        .map(|s| {
            ops::require_dimensionless(s.dim)?;
            let z = s.value;
            if z.im == 0.0 && z.re < 0.0 && k % 2 == 1 {
                finite(Complex64::new(-(-z.re).powf(e), 0.0))
            } else if z == Complex64::new(0.0, 0.0) {
                Ok(z)
            } else if z.im == 0.0 && z.re > 0.0 {
                finite(Complex64::new(z.re.powf(e), 0.0))
            } else {
                finite(z.powf(e))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    match &args[0] {
        Value::Scalar(_) => real_result_of(out[0]),
        _ => grid_result(rows, cols, &out, dimensionless()),
    }
}

fn real_result_of(z: Complex64) -> Result<Value, EvalError> {
    Ok(Value::Scalar(Scalar::new(finite(z)?, dimensionless())))
}

/// `IsNaN(x)`: 0 for every element. A NaN cannot exist in the value model, so no element is NaN.
fn is_nan(args: &[Value]) -> Result<Value, EvalError> {
    match &args[0] {
        Value::Scalar(_) => Ok(Value::Scalar(Scalar::real(0.0, dimensionless()))),
        _ => {
            let (rows, cols, _) = grid(&args[0], "IsNaN")?;
            grid_result(
                rows,
                cols,
                &vec![Complex64::new(0.0, 0.0); rows * cols],
                dimensionless(),
            )
        }
    }
}

/// Neville's algorithm at `q` over the points `(x_i, y_i)`, with the error estimate of Numerical
/// Recipes `polint`. Works on any order of the points, as `polyiter` relies on.
/// Refuses two equal abscissae, which make the recursion divide by zero.
fn neville(x: &[f64], y: &[f64], q: f64) -> Result<(f64, f64), EvalError> {
    let n = x.len();
    // 1-based working arrays, as in the reference recursion.
    let mut c = vec![0.0; n + 1];
    let mut d = vec![0.0; n + 1];
    let mut ns = 1usize;
    let mut best = (x[0] - q).abs();
    for i in 1..=n {
        let dist = (x[i - 1] - q).abs();
        if dist < best {
            ns = i;
            best = dist;
        }
        c[i] = y[i - 1];
        d[i] = y[i - 1];
    }
    let mut value = y[ns - 1];
    let mut ns = ns as i64 - 1;
    let mut last = 0.0;
    for m in 1..n {
        for i in 1..=(n - m) {
            let ho = x[i - 1] - q;
            let hp = x[i + m - 1] - q;
            let w = c[i + 1] - d[i];
            let den = ho - hp;
            if den == 0.0 {
                return Err(unsupported("polyint needs distinct x values"));
            }
            let t = w / den;
            d[i] = hp * t;
            c[i] = ho * t;
        }
        last = if 2 * ns < (n - m) as i64 {
            c[(ns + 1) as usize]
        } else {
            let v = d[ns as usize];
            ns -= 1;
            v
        };
        value += last;
    }
    Ok((value, last))
}

/// `polyint(vx, vy, x)`: `[value; error]`.
fn polyint(args: &[Value]) -> Result<Value, EvalError> {
    let name = "polyint";
    let (x, dx, y, dy) = data_pair(&args[0], &args[1], name)?;
    let (q, qdim) = real_scalar(&args[2], name)?;
    if qdim != dx {
        return Err(EvalError::DimensionMismatch {
            left: dx,
            right: qdim,
        });
    }
    let (value, err) = neville(&x, &y, q)?;
    column(&[Complex64::new(value, 0.0), Complex64::new(err, 0.0)], dy)
}

/// `polycoeff(vx, vy)`: the ascending coefficients of the interpolating polynomial.
fn polycoeff(args: &[Value]) -> Result<Value, EvalError> {
    let name = "polycoeff";
    let (x, dx, y, dy) = data_pair(&args[0], &args[1], name)?;
    ops::require_dimensionless(dx)?;
    ops::require_dimensionless(dy)?;
    let n = x.len();
    // p = prod (z - x_i), ascending: multiply in one factor at a time.
    let mut p = vec![0.0; n + 1];
    p[0] = 1.0;
    for (deg, &xi) in x.iter().enumerate() {
        for k in (1..=deg + 1).rev() {
            p[k] = p[k - 1] - xi * p[k];
        }
        p[0] *= -xi;
    }
    // The Lagrange form: for each point, divide p by (z - x_j) and scale by y_j / Q(x_j).
    let mut c = vec![0.0; n];
    for j in 0..n {
        let mut q = vec![0.0; n];
        q[n - 1] = p[n];
        for k in (1..n).rev() {
            q[k - 1] = p[k] + x[j] * q[k];
        }
        let denom = q.iter().rev().fold(0.0, |acc, &qk| acc * x[j] + qk);
        if denom == 0.0 {
            return Err(unsupported("polycoeff needs distinct x values"));
        }
        let f = y[j] / denom;
        for (ck, qk) in c.iter_mut().zip(&q) {
            *ck += f * qk;
        }
    }
    let zs: Vec<Complex64> = c.iter().map(|&v| Complex64::new(v, 0.0)).collect();
    column(&zs, dimensionless())
}

/// `polyiter(vx, vy, x, N, e)`: `[converged; iterations; value]`.
fn polyiter(args: &[Value]) -> Result<Value, EvalError> {
    let name = "polyiter";
    let (x, dx, y, dy) = data_pair(&args[0], &args[1], name)?;
    let (q, qdim) = real_scalar(&args[2], name)?;
    if qdim != dx {
        return Err(EvalError::DimensionMismatch {
            left: dx,
            right: qdim,
        });
    }
    let nmax = count(&args[3], name)?;
    let (e, edim) = real_scalar(&args[4], name)?;
    if edim != dy {
        return Err(EvalError::DimensionMismatch {
            left: dy,
            right: edim,
        });
    }
    if e < 0.0 {
        return Err(unsupported("polyiter needs a tolerance of zero or more"));
    }
    let n = x.len();
    let nmax = nmax.min(n - 1);
    // The points nearest to x first; iteration k interpolates through the first k + 1 of them.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| (x[a] - q).abs().total_cmp(&(x[b] - q).abs()));
    let mut prev = y[order[0]];
    let mut outcome = (0.0, nmax as f64, prev);
    for k in 1..=nmax {
        let px: Vec<f64> = order[..=k].iter().map(|&i| x[i]).collect();
        let py: Vec<f64> = order[..=k].iter().map(|&i| y[i]).collect();
        let (t, _) = neville(&px, &py, q)?;
        if (t - prev).abs() <= e {
            outcome = (1.0, k as f64, t);
            break;
        }
        prev = t;
        outcome = (0.0, nmax as f64, t);
    }
    let data = vec![
        Scalar::real(outcome.0, dimensionless()),
        Scalar::real(outcome.1, dimensionless()),
        Scalar::real(outcome.2, dy),
    ];
    Ok(Value::Matrix(Matrix::new(3, 1, data)?))
}

/// `localmax` (`maximum` true) or `localmin` (false).
fn local_extremes(args: &[Value], maximum: bool) -> Result<Value, EvalError> {
    let name = if maximum { "localmax" } else { "localmin" };
    if args.len() <= 2 {
        // Two-column form: rows (x, y) of an n x 2 matrix.
        let (rows, cols, data) = grid(&args[0], name)?;
        if cols != 2 {
            return Err(unsupported(&format!(
                "{name} needs an n x 2 matrix (x in column 1, response in column 2)"
            )));
        }
        let w = if args.len() == 2 {
            count(&args[1], name)?
        } else {
            1
        };
        let xs: Vec<f64> = (0..rows)
            .map(|i| real_of(&data[2 * i], name))
            .collect::<Result<_, _>>()?;
        let ys: Vec<f64> = (0..rows)
            .map(|i| real_of(&data[2 * i + 1], name))
            .collect::<Result<_, _>>()?;
        let mut found = Vec::new();
        if rows > 2 * w {
            for i in w..rows - w {
                let z = ys[i];
                let beaten = (i - w..=i + w).filter(|&j| j != i).any(|j| {
                    if maximum {
                        ys[j] >= z
                    } else {
                        ys[j] <= z
                    }
                });
                if !beaten {
                    found.push(i);
                }
            }
        }
        if found.is_empty() {
            return Err(unsupported(&format!("{name} found no extremum")));
        }
        let dx = data[0].dim;
        let dy = data[1].dim;
        let mut out = Vec::with_capacity(found.len() * 2);
        for &i in &found {
            out.push(Scalar::real(xs[i], dx));
            out.push(Scalar::real(ys[i], dy));
        }
        return Ok(Value::Matrix(Matrix::new(found.len(), 2, out)?));
    }
    // Grid form: x and y vectors, M2 with rows(M2) = len(x) and cols(M2) = len(y).
    let xv = vector(&args[0], name)?;
    let yv = vector(&args[1], name)?;
    let dx = one_dim(&xv)?;
    let dy = one_dim(&yv)?;
    let (rows, cols, zdata) = grid(&args[2], name)?;
    if rows != xv.len() || cols != yv.len() {
        return Err(unsupported(&format!(
            "{name}: the matrix must have len(x) rows and len(y) columns"
        )));
    }
    let w = if args.len() == 4 {
        count(&args[3], name)?
    } else {
        1
    };
    let xs = reals(&xv, name)?;
    let ys = reals(&yv, name)?;
    let zs = reals(&zdata, name)?;
    let dz = zdata.first().map_or(dimensionless(), |s| s.dim);
    let mut out = Vec::new();
    if rows > 2 * w && cols > 2 * w {
        for i in w..rows - w {
            for j in w..cols - w {
                let z = zs[i * cols + j];
                let mut beaten = false;
                'window: for a in i - w..=i + w {
                    for b in j - w..=j + w {
                        if (a, b) == (i, j) {
                            continue;
                        }
                        let other = zs[a * cols + b];
                        if (maximum && other >= z) || (!maximum && other <= z) {
                            beaten = true;
                            break 'window;
                        }
                    }
                }
                if !beaten {
                    out.push((xs[i], ys[j], z));
                }
            }
        }
    }
    if out.is_empty() {
        return Err(unsupported(&format!("{name} found no extremum")));
    }
    let mut data = Vec::with_capacity(out.len() * 3);
    for (x, y, z) in &out {
        data.push(Scalar::real(*x, dx));
        data.push(Scalar::real(*y, dy));
        data.push(Scalar::real(*z, dz));
    }
    Ok(Value::Matrix(Matrix::new(out.len(), 3, data)?))
}

fn localmax(args: &[Value]) -> Result<Value, EvalError> {
    local_extremes(args, true)
}

fn localmin(args: &[Value]) -> Result<Value, EvalError> {
    local_extremes(args, false)
}

/// One median pass with half-window `half`; the window contracts at the ends (as `medsmooth`).
fn median_pass(v: &[f64], half: usize) -> Vec<f64> {
    let n = v.len();
    (0..n)
        .map(|i| {
            let h = half.min(i).min(n - 1 - i);
            let mut window = v[i - h..=i + h].to_vec();
            window.sort_by(f64::total_cmp);
            window[window.len() / 2]
        })
        .collect()
}

/// `VSmooth(v, w)`: repeated median smoothing, one width after another.
fn vsmooth(args: &[Value]) -> Result<Value, EvalError> {
    let name = "VSmooth";
    let vs = vector(&args[0], name)?;
    let dim = one_dim(&vs)?;
    let mut v = reals(&vs, name)?;
    let n = v.len();
    let widths = vector(&args[1], name)?;
    for s in &widths {
        ops::require_dimensionless(s.dim)?;
        let w = real_of(s, name)?;
        // Width 0 means no smoothing, as in `medsmooth`; otherwise odd and shorter than the data.
        let valid = w == 0.0 || (w >= 1.0 && w.fract() == 0.0 && w < n as f64 && w % 2.0 == 1.0);
        if w < 0.0 || !valid {
            return Err(unsupported(
                "VSmooth needs odd, nonnegative window widths smaller than the vector",
            ));
        }
        let width = w as usize;
        if width <= 1 {
            continue;
        }
        let half = width / 2;
        // Each pass that changes a value is followed by another; a median filter reaches a fixed
        // point in at most about n passes, so more than n + 1 passes is refused.
        let mut passes = 0usize;
        loop {
            let next = median_pass(&v, half);
            if next == v {
                break;
            }
            v = next;
            passes += 1;
            if passes > n + 1 {
                return Err(unsupported("VSmooth did not converge"));
            }
        }
    }
    let zs: Vec<Complex64> = v.iter().map(|&x| Complex64::new(x, 0.0)).collect();
    column(&zs, dim)
}

/// `contingtbl(M)`: chi-squared, degrees of freedom, its upper-tail probability, Cramer's V and the
/// contingency coefficient C, for a table of non-negative counts.
fn contingtbl(args: &[Value]) -> Result<Value, EvalError> {
    let name = "contingtbl";
    let (rows, cols, data) = grid(&args[0], name)?;
    if rows < 2 || cols < 2 {
        return Err(unsupported("contingtbl needs at least a 2 x 2 table"));
    }
    let counts = reals(&data, name)?;
    for s in &data {
        ops::require_dimensionless(s.dim)?;
    }
    if counts.iter().any(|&o| o < 0.0) {
        return Err(unsupported("contingtbl needs non-negative counts"));
    }
    let total: f64 = counts.iter().sum();
    if total <= 0.0 {
        return Err(unsupported("contingtbl needs at least one count"));
    }
    let row_sums: Vec<f64> = (0..rows)
        .map(|i| counts[i * cols..(i + 1) * cols].iter().sum())
        .collect();
    let col_sums: Vec<f64> = (0..cols)
        .map(|j| (0..rows).map(|i| counts[i * cols + j]).sum())
        .collect();
    let mut chi2 = 0.0;
    for i in 0..rows {
        for j in 0..cols {
            let expected = row_sums[i] * col_sums[j] / total;
            if expected > 0.0 {
                let diff = counts[i * cols + j] - expected;
                chi2 += diff * diff / expected;
            }
        }
    }
    let df = ((rows - 1) * (cols - 1)) as f64;
    let p = if chi2 <= 0.0 {
        1.0
    } else {
        gam::gamma_q(df / 2.0, chi2 / 2.0)?
    };
    let k = (rows.min(cols) - 1) as f64;
    let cramer = (chi2 / (total * k)).sqrt();
    let contingency = (chi2 / (chi2 + total)).sqrt();
    column(
        &[chi2, df, p, cramer, contingency].map(|v| Complex64::new(v, 0.0)),
        dimensionless(),
    )
}

/// `DMS(v)` or `DMS(d, m, s)`: degrees, minutes and seconds to radians (a dimensionless angle).
/// The help gives no sign rule. The angle takes the sign of its first nonzero part and the parts
/// are added in magnitude, so `DMS(-30, 15, 0)` is `-(30 + 15/60)` degrees, `DMS(0, -15, 0)` is
/// `-15/60` degrees. A negative part after a positive angle is refused.
fn dms(args: &[Value]) -> Result<Value, EvalError> {
    let name = "DMS";
    let parts: Vec<f64> = match args.len() {
        1 => {
            let v = vector(&args[0], name)?;
            if v.len() != 3 {
                return Err(unsupported(
                    "DMS needs three elements: degrees, minutes, seconds",
                ));
            }
            for s in &v {
                ops::require_dimensionless(s.dim)?;
            }
            reals(&v, name)?
        }
        3 => args
            .iter()
            .map(|a| {
                let (x, d) = real_scalar(a, name)?;
                ops::require_dimensionless(d)?;
                Ok(x)
            })
            .collect::<Result<_, EvalError>>()?,
        found => {
            return Err(EvalError::ArgCount {
                name: name.to_string(),
                expected: "1 or 3".to_string(),
                found,
            })
        }
    };
    // The angle's sign is that of its first nonzero part; a negative part after a positive angle
    // has no reading, so it is refused.
    let lead = parts.iter().copied().find(|&p| p != 0.0).unwrap_or(0.0);
    let negative = lead < 0.0;
    if !negative && parts.iter().any(|&p| p < 0.0) {
        return Err(unsupported("DMS: a negative part after a positive angle"));
    }
    let magnitude = parts[0].abs() + parts[1].abs() / 60.0 + parts[2].abs() / 3600.0;
    let degrees = if negative { -magnitude } else { magnitude };
    real_result(degrees * PI / 180.0, dimensionless())
}

/// The inches part of a FIF string, `m`, `p/q`, `m-p/q` or `m p/q` (each part optional, missing
/// parts are zero). Returns the inches, or `None` for anything else.
fn fif_inches(s: &str) -> Option<f64> {
    let s = s.trim();
    let (whole, fraction) = match s.find(['-', ' ']) {
        Some(i) => (&s[..i], s[i + 1..].trim()),
        None if s.contains('/') => ("", s),
        None => (s, ""),
    };
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let fraction = if fraction.is_empty() {
        0.0
    } else {
        let (p, q) = fraction.split_once('/')?;
        let p: i64 = p.trim().parse().ok()?;
        let q: i64 = q.trim().parse().ok()?;
        if q <= 0 {
            return None;
        }
        p as f64 / q as f64
    };
    Some(whole as f64 + fraction)
}

/// Parses a FIF string into inches (help "Unit Scaling in Common Formats"):
/// - `n' m-p/q`: feet (digits before the apostrophe), then inches and a fraction of an inch;
/// - `m"` or `p/q`, or `m-p/q`: inches, the inch mark optional;
/// - a bare integer: feet (`"9"` is 9 feet, as the help's `FIF("9") = 2.7432 m` shows).
fn parse_fif(s: &str) -> Result<f64, EvalError> {
    let bad =
        || unsupported("FIF needs a string of the form n' m-p/q, or feet, inches and fractions");
    let s = s.trim();
    if let Some((feet, rest)) = s.split_once('\'') {
        let feet: i64 = feet.trim().parse().map_err(|_| bad())?;
        let inches = fif_inches(rest.trim_end_matches('"')).ok_or_else(bad)?;
        return Ok(12.0 * feet as f64 + inches);
    }
    if let Some(inch) = s.strip_suffix('"') {
        return fif_inches(inch).ok_or_else(bad);
    }
    if s.contains(['-', ' ', '/']) {
        return fif_inches(s).ok_or_else(bad);
    }
    let feet: i64 = s.parse().map_err(|_| bad())?;
    Ok(12.0 * feet as f64)
}

/// `FIF(s)` or `FIF(ft, in, fractions)` (one to three numbers, each optional from the left): a
/// length in metres. A single number is feet, two are feet and inches.
fn fif(args: &[Value]) -> Result<Value, EvalError> {
    let name = "FIF";
    let inches = match args {
        [Value::Str(s)] => parse_fif(s)?,
        _ if (1..=3).contains(&args.len()) => {
            let mut total = 0.0;
            for (k, a) in args.iter().enumerate() {
                let (x, d) = real_scalar(a, name)?;
                ops::require_dimensionless(d)?;
                total += if k == 0 { 12.0 * x } else { x };
            }
            total
        }
        _ => {
            return Err(EvalError::ArgCount {
                name: name.to_string(),
                expected: "1 or 3".to_string(),
                found: args.len(),
            })
        }
    };
    let inch = unit("in").ok_or_else(|| unsupported("FIF: no inch unit"))?;
    let quantity: Quantity = Quantity::from_unit(inches, inch);
    Ok(Value::Scalar(Scalar::from_quantity(quantity)))
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::{assert_all_close, col, complex_values, num, values};
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, EvalError> {
        let mut r = Registry::new();
        register_data_tier2(&mut r);
        let b = r.get(name).expect("registered");
        match b.call {
            super::super::Call::Plain(f) => f(args),
            super::super::Call::WithEnv(_) => panic!("{name} is a plain builtin"),
        }
    }

    fn mat(rows: usize, cols: usize, xs: &[f64]) -> Value {
        let data = xs
            .iter()
            .map(|&x| Scalar::real(x, dimensionless()))
            .collect();
        Value::Matrix(Matrix::new(rows, cols, data).expect("matrix"))
    }

    /// The help's polynomial example: x = [301; 306; 318; 332; 333], y = [0.02; 0.2; -0.04; 0.06; 0.17].
    fn help_poly() -> (Value, Value) {
        (
            col(&[301.0, 306.0, 318.0, 332.0, 333.0]),
            col(&[0.02, 0.2, -0.04, 0.06, 0.17]),
        )
    }

    #[test]
    fn conj_flips_the_imaginary_part() {
        let v = Value::Scalar(Scalar::new(Complex64::new(1.0, 2.0), dimensionless()));
        let r = call("conj", &[v]).expect("conj");
        assert_eq!(complex_values(&r), vec![Complex64::new(1.0, -2.0)]);
    }

    #[test]
    fn nth_root_real_odd_and_principal() {
        assert_all_close(
            &values(&call("nthRoot", &[num(-8.0), num(3.0)]).unwrap()),
            &[-2.0],
            1e-12,
        );
        assert_all_close(
            &values(&call("nthRoot", &[num(81.0), num(4.0)]).unwrap()),
            &[3.0],
            1e-12,
        );
        // Even root of a negative: principal complex root, sqrt(-4) = 2i.
        let z = complex_values(&call("nthRoot", &[num(-4.0), num(2.0)]).unwrap());
        assert!((z[0] - Complex64::new(0.0, 2.0)).norm() < 1e-12);
        // The n-th power of the root gives back the radicand (property).
        for &x in &[0.5, 7.0, 1e6] {
            for k in 1..6 {
                let r = values(&call("nthRoot", &[num(x), num(k as f64)]).unwrap())[0];
                assert!((r.powi(k) - x).abs() <= 1e-9 * x, "x {x} k {k}");
            }
        }
    }

    #[test]
    fn is_nan_is_zero_everywhere() {
        assert_all_close(
            &values(&call("IsNaN", &[col(&[1.0, -3.0])]).unwrap()),
            &[0.0, 0.0],
            0.0,
        );
        assert_all_close(&values(&call("IsNaN", &[num(5.0)]).unwrap()), &[0.0], 0.0);
    }

    #[test]
    fn polyint_matches_the_help_example() {
        // polyint(x, y, 328) = [-0.189; -0.056]; the help shows the error as 0.056 in its text.
        let (x, y) = help_poly();
        let r = values(&call("polyint", &[x, y, num(328.0)]).expect("polyint"));
        assert!((r[0] + 0.189).abs() < 5e-4, "value {}", r[0]);
        assert!((r[1].abs() - 0.056).abs() < 5e-4, "error {}", r[1]);
    }

    #[test]
    fn polyint_reproduces_the_data_at_the_knots() {
        let x = col(&[1.0, 2.0, 4.0, 7.0]);
        let y = col(&[3.0, -1.0, 2.5, 0.25]);
        for (xi, yi) in [(1.0, 3.0), (2.0, -1.0), (4.0, 2.5), (7.0, 0.25)] {
            let r = values(&call("polyint", &[x.clone(), y.clone(), num(xi)]).unwrap());
            assert!((r[0] - yi).abs() < 1e-12, "at {xi}: {}", r[0]);
        }
    }

    #[test]
    fn polycoeff_passes_through_the_data() {
        let x = col(&[1.0, 2.0, 4.0]);
        let y = col(&[3.0, -1.0, 2.5]);
        let c = values(&call("polycoeff", &[x, y]).unwrap());
        assert_eq!(c.len(), 3);
        for (xi, yi) in [(1.0f64, 3.0f64), (2.0, -1.0), (4.0, 2.5)] {
            let poly: f64 = c
                .iter()
                .enumerate()
                .map(|(k, ck)| ck * xi.powi(k as i32))
                .sum();
            assert!((poly - yi).abs() < 1e-10, "at {xi}: {poly}");
        }
    }

    #[test]
    fn polycoeff_of_the_help_data_has_a_constant_term_near_18489() {
        let (x, y) = help_poly();
        let c = values(&call("polycoeff", &[x, y]).unwrap());
        assert!((c[0] - 18489.115).abs() < 0.01, "c0 {}", c[0]);
        assert!((c[1] + 249.995).abs() < 1e-3, "c1 {}", c[1]);
    }

    #[test]
    fn polyiter_gives_up_at_n_max_like_the_help() {
        // The help: polyiter(x, y, 328, 4, 0.01) = [0; 4; -0.189] (no convergence, same as polyint).
        let (x, y) = help_poly();
        let r = values(&call("polyiter", &[x, y, num(328.0), num(4.0), num(0.01)]).unwrap());
        assert_eq!(r[0], 0.0);
        assert_eq!(r[1], 4.0);
        assert!((r[2] + 0.189).abs() < 5e-4);
    }

    #[test]
    fn polyiter_converges_on_a_line() {
        let x = col(&[0.0, 1.0, 2.0, 3.0]);
        let y = col(&[1.0, 3.0, 5.0, 7.0]);
        let r = values(&call("polyiter", &[x, y, num(1.5), num(3.0), num(1e-9)]).unwrap());
        assert_eq!(r[0], 1.0);
        assert!((r[2] - 4.0).abs() < 1e-9);
    }

    #[test]
    fn repeated_x_values_are_refused() {
        let x = col(&[1.0, 1.0, 2.0]);
        let y = col(&[1.0, 2.0, 3.0]);
        assert!(call("polyint", &[x.clone(), y.clone(), num(1.5)]).is_err());
        assert!(call("polycoeff", &[x, y]).is_err());
    }

    #[test]
    fn mismatched_and_single_point_data_are_handled() {
        assert!(call("polyint", &[col(&[1.0, 2.0]), col(&[1.0]), num(1.0)]).is_err());
        // One point: the constant polynomial.
        let r = values(&call("polyint", &[col(&[2.0]), col(&[5.0]), num(9.0)]).unwrap());
        assert_all_close(&r, &[5.0, 0.0], 1e-15);
        let c = values(&call("polycoeff", &[col(&[2.0]), col(&[5.0])]).unwrap());
        assert_all_close(&c, &[5.0], 1e-15);
    }

    #[test]
    fn local_maximum_of_a_two_column_series() {
        // y = 0, 2, 1, 4, 3, 0 at x = 0..5: maxima at x = 1 and x = 3 (w = 1).
        let m = mat(
            6,
            2,
            &[0.0, 0.0, 1.0, 2.0, 2.0, 1.0, 3.0, 4.0, 4.0, 3.0, 5.0, 0.0],
        );
        let r = values(&call("localmax", std::slice::from_ref(&m)).unwrap());
        assert_all_close(&r, &[1.0, 2.0, 3.0, 4.0], 0.0);
        let r = values(&call("localmin", &[m]).unwrap());
        assert_all_close(&r, &[2.0, 1.0], 0.0);
    }

    #[test]
    fn local_extremes_refuse_flat_tops_and_no_extremum() {
        // A flat top is not strictly greater than its neighbour.
        let m = mat(3, 2, &[0.0, 1.0, 1.0, 1.0, 2.0, 1.0]);
        assert!(call("localmax", &[m]).is_err());
        // A monotone series has no interior maximum.
        let m = mat(3, 2, &[0.0, 1.0, 1.0, 2.0, 2.0, 3.0]);
        assert!(call("localmax", &[m]).is_err());
    }

    #[test]
    fn grid_local_maximum() {
        // z peaks at (1, 1) in a 3 x 3 grid.
        let x = col(&[0.0, 1.0, 2.0]);
        let y = col(&[10.0, 11.0, 12.0]);
        let z = mat(3, 3, &[0.0, 1.0, 0.0, 1.0, 5.0, 1.0, 0.0, 1.0, 0.0]);
        let r = values(&call("localmax", &[x, y, z]).unwrap());
        assert_all_close(&r, &[1.0, 11.0, 5.0], 0.0);
    }

    #[test]
    fn vsmooth_removes_a_spike_and_stops() {
        let v = col(&[1.0, 1.0, 1.0, 9.0, 1.0, 1.0, 1.0]);
        let r = values(&call("VSmooth", &[v, num(3.0)]).unwrap());
        assert_all_close(&r, &[1.0; 7], 0.0);
    }

    #[test]
    fn vsmooth_refuses_even_widths() {
        assert!(call("VSmooth", &[col(&[1.0, 2.0, 3.0, 4.0]), num(2.0)]).is_err());
    }

    #[test]
    fn contingency_table_matches_the_help_example() {
        // Help: contingtbl(Table) = (21.218; 6; 0.002; 0.465; 0.55).
        let t = mat(
            3,
            4,
            &[0.0, 2.0, 13.0, 4.0, 2.0, 5.0, 7.0, 10.0, 3.0, 2.0, 1.0, 0.0],
        );
        let r = values(&call("contingtbl", &[t]).unwrap());
        assert!((r[0] - 21.218).abs() < 1e-3, "chi2 {}", r[0]);
        assert_eq!(r[1], 6.0);
        assert!((r[2] - 0.002).abs() < 5e-4, "p {}", r[2]);
        assert!((r[3] - 0.465).abs() < 5e-4, "V {}", r[3]);
        assert!((r[4] - 0.55).abs() < 5e-3, "C {}", r[4]);
    }

    #[test]
    fn contingency_tail_does_not_underflow() {
        // [[50, 0], [0, 50]]: chi2 = 100 on 1 degree of freedom, upper tail erfc(sqrt(50)) = 1.524e-23.
        // The upper tail is computed directly, so it is not lost to 1 - P.
        let t = mat(2, 2, &[50.0, 0.0, 0.0, 50.0]);
        let r = values(&call("contingtbl", &[t]).unwrap());
        assert!((r[0] - 100.0).abs() < 1e-9, "chi2 {}", r[0]);
        assert!(
            r[2] > 0.0 && (r[2] / 1.523_970_604_832_099e-23 - 1.0).abs() < 1e-8,
            "p {}",
            r[2]
        );
    }

    #[test]
    fn contingency_refuses_negative_counts_and_small_tables() {
        assert!(call("contingtbl", &[mat(2, 2, &[1.0, -1.0, 2.0, 3.0])]).is_err());
        assert!(call("contingtbl", &[mat(1, 3, &[1.0, 2.0, 3.0])]).is_err());
    }

    #[test]
    fn dms_to_radians() {
        // 180 degrees is pi radians.
        let r = values(&call("DMS", &[num(180.0), num(0.0), num(0.0)]).unwrap());
        assert!((r[0] - PI).abs() < 1e-12);
        // 30 degrees 36 minutes = 30.6 degrees.
        let r = values(&call("DMS", &[col(&[30.0, 36.0, 0.0])]).unwrap());
        assert!((r[0] - 30.6 * PI / 180.0).abs() < 1e-12);
        assert!(call("DMS", &[num(1.0), num(2.0)]).is_err());
    }

    #[test]
    fn dms_sign_applies_to_the_whole_angle() {
        // DMS(-30, 15, 0) is -(30 + 15/60) degrees, not -30 + 15/60.
        let r = values(&call("DMS", &[num(-30.0), num(15.0), num(0.0)]).unwrap());
        assert!((r[0] - (-30.25f64) * PI / 180.0).abs() < 1e-12, "{}", r[0]);
        let r = values(&call("DMS", &[col(&[0.0, -15.0, 0.0])]).unwrap());
        assert!((r[0] - (-0.25f64) * PI / 180.0).abs() < 1e-12, "{}", r[0]);
        // Parts of mixed sign have no reading in the help: refused.
        assert!(call("DMS", &[num(30.0), num(-15.0), num(0.0)]).is_err());
    }

    #[test]
    fn fif_string_is_a_length_in_metres() {
        // 5' 3-1/2: 5 ft 3.5 in = 63.5 in = 1.6129 m.
        let s = Value::Str("5' 3-1/2".to_string());
        let r = values(&call("FIF", &[s]).unwrap());
        assert!((r[0] - 63.5 * 0.0254).abs() < 1e-12);
        let r = values(&call("FIF", &[num(5.0), num(3.0), num(0.5)]).unwrap());
        assert!((r[0] - 63.5 * 0.0254).abs() < 1e-12);
    }

    #[test]
    fn cubic_and_window_loops_are_over_budget_when_large() {
        use crate::eval::work::{builtin_units, MAX_WORK};
        // polycoeff is O(n^2): 3 n^2 units. 1000 points is well within budget, 12000 is not.
        let small = col(&vec![1.0; 1000]);
        assert!(builtin_units("polycoeff", &[small.clone(), small]) < MAX_WORK);
        let big = col(&vec![1.0; 12_000]);
        assert!(builtin_units("polycoeff", &[big.clone(), big.clone()]) > MAX_WORK);
        // VSmooth at width 3 runs in about 3 ms for 4900 points: the charge admits it, and refuses a
        // far larger vector.
        let v = col(&vec![1.0; 4900]);
        assert!(builtin_units("VSmooth", &[v, num(3.0)]) < MAX_WORK);
        let huge = col(&vec![1.0; 20_000]);
        assert!(builtin_units("VSmooth", &[huge, num(3.0)]) > MAX_WORK);
        // A 2000-point series with a window of 1000 is about 3 * 4001 * 2000 units: within budget.
        let w = num(1000.0);
        assert!(builtin_units("localmax", &[mat(2000, 2, &vec![0.0; 4000]), w]) < MAX_WORK);
        // The grid form charges the square of the window.
        let grid = mat(200, 200, &vec![0.0; 40_000]);
        let x = col(&vec![0.0; 200]);
        assert!(builtin_units("localmax", &[x.clone(), x, grid, num(1000.0)]) > MAX_WORK);
    }

    #[test]
    fn fif_help_examples() {
        // Help "Example: Converting Feet-Inches-Fractions to Length" (metres).
        let cases: Vec<(Vec<Value>, f64)> = vec![
            (vec![Value::Str("9' 6-1/4".into())], 2.902),
            (vec![Value::Str("9".into())], 2.7432),
            (vec![num(9.0)], 2.7432),
            (vec![Value::Str("9\"".into())], 0.2286),
            (vec![Value::Str("3/4".into())], 0.01905),
            (vec![num(0.0), num(9.0)], 0.2286),
            (vec![num(0.0), num(0.0), num(0.75)], 0.01905),
            (vec![num(3.0), num(3.0), num(0.37008)], 1.0),
        ];
        for (args, expected) in cases {
            let r = values(&call("FIF", &args).unwrap());
            let tol = 1e-3;
            assert!(
                (r[0] - expected).abs() < tol,
                "{args:?}: {} vs {expected}",
                r[0]
            );
        }
        // The 1-argument string forms have no apostrophe.
        assert!(
            (values(&call("FIF", &[Value::Str("3/4".into())]).unwrap())[0] - 0.75 * 0.0254).abs()
                < 1e-12
        );
    }

    #[test]
    fn fif_refuses_malformed_strings() {
        for bad in ["5 3", "x' 3", "5' 3-1/0", "5' 3-1/x", "5'\u{0}"] {
            assert!(
                call("FIF", &[Value::Str(bad.to_string())]).is_err(),
                "{bad}"
            );
        }
    }
}

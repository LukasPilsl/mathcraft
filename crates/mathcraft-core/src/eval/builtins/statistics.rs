//! Descriptive statistics and the probability-distribution functions of the statistics tier.
//!
//! Rules (Mathcad Prime help, "Descriptive Statistics" and "Probability Distributions"):
//! - Arguments `A, B, C, ...` are scalars or matrices. Every element of every argument is one data
//!   point, so `var(A, B)` is taken over all elements of both.
//! - Lowercase `var`, `stdev` divide by N (population). Uppercase `Var`, `Stdev` divide by N-1.
//! - Units: `mean`, `gmean`, `hmean`, `median`, `mode`, `percentile` keep the dimension; `var`
//!   has the square of it and `stdev` its square root; `cvar` has the product of the two
//!   dimensions; `corr`, `skew`, `kurt` are dimensionless. All data points of one call share one
//!   dimension.
//! - `median`, `gmean`, `hmean`, `skew`, `kurt` take real data. `var`, `stdev`, `cvar` and `corr`
//!   take complex data with the conjugate in the cross term (the help's overbar), so `var` is the
//!   mean of `|x - mean|^2`. `mean` and `mode` take complex data.
//! - The distribution functions are in `statistics_dist`.
//!
//! Not in this tier yet (not registered, so the evaluator reports them as undefined): `histogram`,
//! `Ftest`, `Spear`, `kendtau`, `kendtau2`, `contingtbl`, `rank`, `sort`.

use num_complex::Complex64;

use super::{Arity, Registry};
use crate::eval::error::{from_units, EvalError};
use crate::eval::value::{Scalar, Value, MAX_ELEMENTS};
use crate::units::Dimension;

#[path = "statistics_dist.rs"]
pub(crate) mod dist;
#[path = "statistics_math.rs"]
pub(crate) mod math;

/// Registers every function of this tier.
pub fn register(r: &mut Registry) {
    r.register("mean", Arity::AtLeast(1), mean);
    r.register("gmean", Arity::AtLeast(1), gmean);
    r.register("hmean", Arity::AtLeast(1), hmean);
    r.register("median", Arity::AtLeast(1), median);
    r.register("mode", Arity::AtLeast(1), mode);
    r.register("percentile", Arity::Exact(2), percentile);
    r.register("var", Arity::AtLeast(1), var_population);
    r.register("Var", Arity::AtLeast(1), var_sample);
    r.register("stdev", Arity::AtLeast(1), stdev_population);
    r.register("Stdev", Arity::AtLeast(1), stdev_sample);
    r.register("skew", Arity::AtLeast(1), skew);
    r.register("kurt", Arity::AtLeast(1), kurt);
    r.register("cvar", Arity::Exact(2), cvar);
    r.register("corr", Arity::Exact(2), corr);
    dist::register(r);
    super::distributions_cont::register(r);
    super::distributions_disc::register(r);
    super::random::register(r);
}

/// The elements of all arguments, in order, with one dimension and finite values. A matrix
/// argument contributes all its elements; a scalar contributes one.
fn data(args: &[Value]) -> Result<Vec<Scalar>, EvalError> {
    let mut out: Vec<Scalar> = Vec::new();
    for arg in args {
        match arg {
            Value::Scalar(s) => out.push(*s),
            Value::Matrix(m) => out.extend_from_slice(m.data()),
            Value::Str(_) | Value::List(_) | Value::Symbolic(_) | Value::Func(_) => {
                return Err(EvalError::Unsupported(
                    "statistics functions take numbers and matrices".to_string(),
                ))
            }
        }
        if out.len() > MAX_ELEMENTS {
            return Err(EvalError::TooLarge(out.len()));
        }
    }
    let first = out
        .first()
        .ok_or_else(|| EvalError::Unsupported("statistics of no data".to_string()))?;
    let dim = first.dim;
    for s in &out {
        if s.dim != dim {
            return Err(EvalError::DimensionMismatch {
                left: dim,
                right: s.dim,
            });
        }
        if !(s.value.re.is_finite() && s.value.im.is_finite()) {
            return Err(EvalError::NotFinite);
        }
    }
    Ok(out)
}

/// The shared dimension of `data`; `data` already guarantees one.
fn dim_of(xs: &[Scalar]) -> Dimension {
    xs.first().map_or_else(Dimension::dimensionless, |s| s.dim)
}

/// Real parts of `xs`, refusing complex data with the name of the function.
fn reals(xs: &[Scalar], name: &str) -> Result<Vec<f64>, EvalError> {
    xs.iter()
        .map(|s| {
            if s.value.im == 0.0 {
                Ok(s.value.re)
            } else {
                Err(EvalError::Unsupported(format!("{name} of complex numbers")))
            }
        })
        .collect()
}

/// Real, strictly positive data (for `gmean` and `hmean`).
fn positive_reals(xs: &[Scalar], name: &str) -> Result<Vec<f64>, EvalError> {
    let values = reals(xs, name)?;
    if values.iter().all(|&x| x > 0.0) {
        Ok(values)
    } else {
        Err(EvalError::Unsupported(format!(
            "{name} needs positive real numbers"
        )))
    }
}

/// The result as a scalar of dimension `dim`, refusing non-finite values.
fn result(z: Complex64, dim: Dimension) -> Result<Value, EvalError> {
    if z.re.is_finite() && z.im.is_finite() {
        Ok(Value::Scalar(Scalar::new(z, dim)))
    } else {
        Err(EvalError::NotFinite)
    }
}

fn real_result(x: f64, dim: Dimension) -> Result<Value, EvalError> {
    result(Complex64::new(x, 0.0), dim)
}

fn count(xs: &[Scalar]) -> f64 {
    xs.len() as f64
}

/// Mean of the data. The plain sum is used when it is finite. Otherwise the data are divided by the
/// largest magnitude before summing, so finite data with a finite mean never overflow.
fn mean_of(xs: &[Scalar]) -> Complex64 {
    let sum: Complex64 = xs.iter().map(|s| s.value).sum();
    if sum.re.is_finite() && sum.im.is_finite() {
        return sum / count(xs);
    }
    let scale = xs
        .iter()
        .map(|s| s.value.re.abs().max(s.value.im.abs()))
        .fold(0.0_f64, f64::max);
    let scaled: Complex64 = xs.iter().map(|s| s.value / scale).sum();
    scaled / count(xs) * scale
}

/// Population `sum |x - mean|^2 / N` (or `N - 1` for the sample), real.
fn variance_si(xs: &[Scalar], sample: bool) -> Result<f64, EvalError> {
    let n = xs.len();
    if sample && n < 2 {
        return Err(EvalError::Unsupported(
            "sample variance needs at least two values".to_string(),
        ));
    }
    let m = mean_of(xs);
    let ss: f64 = xs.iter().map(|s| (s.value - m).norm_sqr()).sum();
    let denom = if sample { count(xs) - 1.0 } else { count(xs) };
    Ok(ss / denom)
}

fn mean(args: &[Value]) -> Result<Value, EvalError> {
    let xs = data(args)?;
    result(mean_of(&xs), dim_of(&xs))
}

fn gmean(args: &[Value]) -> Result<Value, EvalError> {
    let xs = data(args)?;
    let values = positive_reals(&xs, "gmean")?;
    let mean_ln: f64 = values.iter().map(|x| x.ln()).sum::<f64>() / values.len() as f64;
    real_result(mean_ln.exp(), dim_of(&xs))
}

fn hmean(args: &[Value]) -> Result<Value, EvalError> {
    let xs = data(args)?;
    let values = positive_reals(&xs, "hmean")?;
    let inv_sum: f64 = values.iter().map(|x| 1.0 / x).sum();
    real_result(values.len() as f64 / inv_sum, dim_of(&xs))
}

/// Sorted real values; `reals` has already refused complex data and `data` refused non-finite.
fn sorted_reals(xs: &[Scalar], name: &str) -> Result<Vec<f64>, EvalError> {
    let mut values = reals(xs, name)?;
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(values)
}

fn median(args: &[Value]) -> Result<Value, EvalError> {
    let xs = data(args)?;
    let v = sorted_reals(&xs, "median")?;
    let n = v.len();
    let mid = if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    };
    real_result(mid, dim_of(&xs))
}

/// The one value that occurs most often, provided it occurs at least twice and no other value
/// ties it. The help returns an error otherwise.
fn mode(args: &[Value]) -> Result<Value, EvalError> {
    let xs = data(args)?;
    let mut sorted = xs.clone();
    sorted.sort_by(|a, b| {
        a.value
            .re
            .partial_cmp(&b.value.re)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.value
                    .im
                    .partial_cmp(&b.value.im)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let mut runs: Vec<(Complex64, usize)> = Vec::new();
    for s in &sorted {
        match runs.last().copied() {
            Some((value, n)) if value == s.value => {
                if let Some(last) = runs.last_mut() {
                    last.1 = n + 1;
                }
            }
            _ => runs.push((s.value, 1)),
        }
    }
    let best = runs.iter().map(|&(_, n)| n).max().unwrap_or(0);
    let winners: Vec<Complex64> = runs
        .iter()
        .filter(|&&(_, n)| n == best)
        .map(|&(value, _)| value)
        .collect();
    match winners.as_slice() {
        [only] if best >= 2 => result(*only, dim_of(&xs)),
        _ => Err(EvalError::Unsupported(
            "mode: no single value occurs most often".to_string(),
        )),
    }
}

/// `percentile(v, p)`: linear interpolation at position `p (n+1) - 1` of the sorted data. The
/// help does not state this rule; it reproduces its examples (`percentile(0..10, 0.9) = 9.8`,
/// and the quartiles of its five-value data set). Imaginary parts are ignored, as the help says.
/// The position is clamped to `0..n-1`, so p below 1/(n+1) gives the minimum and p above n/(n+1)
/// the maximum; p = 0 and p = 1 are accepted.
fn percentile(args: &[Value]) -> Result<Value, EvalError> {
    let (v, p) = match args {
        [v, Value::Scalar(p)] => (v, *p),
        _ => {
            return Err(EvalError::Unsupported(
                "percentile takes a data set and a probability".to_string(),
            ))
        }
    };
    let xs = data(std::slice::from_ref(v))?;
    crate::eval::ops::require_dimensionless(p.dim)?;
    if p.value.im != 0.0 || !(0.0..=1.0).contains(&p.value.re) {
        return Err(EvalError::Unsupported(
            "percentile needs a real probability between 0 and 1".to_string(),
        ));
    }
    let mut sorted: Vec<f64> = xs.iter().map(|s| s.value.re).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let h = (p.value.re * (count(&xs) + 1.0) - 1.0).clamp(0.0, (n - 1) as f64);
    let lo = h.floor() as usize;
    let frac = h - h.floor();
    let value = if lo + 1 >= n {
        sorted[n - 1]
    } else {
        sorted[lo] + frac * (sorted[lo + 1] - sorted[lo])
    };
    real_result(value, dim_of(&xs))
}

fn var_population(args: &[Value]) -> Result<Value, EvalError> {
    variance_value(args, false)
}

fn var_sample(args: &[Value]) -> Result<Value, EvalError> {
    variance_value(args, true)
}

fn variance_value(args: &[Value], sample: bool) -> Result<Value, EvalError> {
    let xs = data(args)?;
    let dim = dim_of(&xs);
    let squared = dim.checked_mul(dim).map_err(from_units)?;
    real_result(variance_si(&xs, sample)?, squared)
}

fn stdev_population(args: &[Value]) -> Result<Value, EvalError> {
    stdev_value(args, false)
}

fn stdev_sample(args: &[Value]) -> Result<Value, EvalError> {
    stdev_value(args, true)
}

fn stdev_value(args: &[Value], sample: bool) -> Result<Value, EvalError> {
    // The square root of a variance of dimension d^2 has dimension d.
    let xs = data(args)?;
    real_result(variance_si(&xs, sample)?.sqrt(), dim_of(&xs))
}

/// The standardized values `(x - mean) / Stdev` of real data, with the sample standard deviation.
fn standardized(xs: &[Scalar], name: &str) -> Result<Vec<f64>, EvalError> {
    let values = reals(xs, name)?;
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let s = (values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    if s == 0.0 {
        return Err(EvalError::DivisionByZero);
    }
    Ok(values.iter().map(|x| (x - mean) / s).collect())
}

/// Sample skewness `n / ((n-1)(n-2)) * sum z^3` (help: "Kurtosis and Skewness").
fn skew(args: &[Value]) -> Result<Value, EvalError> {
    let xs = data(args)?;
    if xs.len() < 3 {
        return Err(EvalError::Unsupported(
            "skew needs at least three values".to_string(),
        ));
    }
    let n = count(&xs);
    let z = standardized(&xs, "skew")?;
    let s3: f64 = z.iter().map(|v| v.powi(3)).sum();
    real_result(n / ((n - 1.0) * (n - 2.0)) * s3, Dimension::dimensionless())
}

/// Sample kurtosis `n(n+1)/((n-1)(n-2)(n-3)) * sum z^4 - 3(n-1)^2/((n-2)(n-3))`.
fn kurt(args: &[Value]) -> Result<Value, EvalError> {
    let xs = data(args)?;
    if xs.len() < 4 {
        return Err(EvalError::Unsupported(
            "kurt needs at least four values".to_string(),
        ));
    }
    let n = count(&xs);
    let z = standardized(&xs, "kurt")?;
    let s4: f64 = z.iter().map(|v| v.powi(4)).sum();
    let value = n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * s4
        - 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0));
    real_result(value, Dimension::dimensionless())
}

/// The two arguments of `cvar` and `corr` as element lists of one size each.
fn paired(args: &[Value]) -> Result<(Vec<Scalar>, Vec<Scalar>), EvalError> {
    let (a, b) = match args {
        [a, b] => (a, b),
        _ => {
            return Err(EvalError::Unsupported(
                "covariance and correlation take two arrays".to_string(),
            ))
        }
    };
    let xs = data(std::slice::from_ref(a))?;
    let ys = data(std::slice::from_ref(b))?;
    if xs.len() != ys.len() {
        return Err(EvalError::ShapeMismatch {
            left: shape_of(a),
            right: shape_of(b),
        });
    }
    Ok((xs, ys))
}

fn shape_of(v: &Value) -> (usize, usize) {
    match v {
        Value::Matrix(m) => m.shape(),
        _ => (1, 1),
    }
}

/// Population covariance `sum (a - mean a) conj(b - mean b) / N`, with its dimension.
fn covariance(xs: &[Scalar], ys: &[Scalar]) -> Complex64 {
    let ma = mean_of(xs);
    let mb = mean_of(ys);
    let sum: Complex64 = xs
        .iter()
        .zip(ys)
        .map(|(a, b)| (a.value - ma) * (b.value - mb).conj())
        .sum();
    sum / count(xs)
}

fn cvar(args: &[Value]) -> Result<Value, EvalError> {
    let (xs, ys) = paired(args)?;
    let dim = dim_of(&xs).checked_mul(dim_of(&ys)).map_err(from_units)?;
    result(covariance(&xs, &ys), dim)
}

/// Pearson's r: `cvar(A, B) / (stdev(A) stdev(B))` with population standard deviations.
fn corr(args: &[Value]) -> Result<Value, EvalError> {
    let (xs, ys) = paired(args)?;
    let va = variance_si(&xs, false)?;
    let vb = variance_si(&ys, false)?;
    if va == 0.0 || vb == 0.0 {
        return Err(EvalError::DivisionByZero);
    }
    let r = covariance(&xs, &ys) / (va * vb).sqrt();
    result(r, Dimension::dimensionless())
}

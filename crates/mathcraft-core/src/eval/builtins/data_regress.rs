//! Least-squares line through data: `line`, `slope`, `intercept`, `stderr`
//! (Mathcad Prime help: "Linear Regression").
//!
//! - `line(vx, vy)`: the column `[a; b]` with `a + b x` the least-squares line (`a` is the intercept).
//! - `slope(vx, vy)`, `intercept(vx, vy)`: the two coefficients.
//! - `stderr(vx, vy)`: `sqrt(sum of squared residuals / (n - 2))`, needs more than two points.
//!
//! Units: the intercept has the dimension of `vy`, the slope that of `vy / vx`, the standard error
//! that of `vy`. Data must be real. Equal `vx` values have no line (`Singular`).

use num_complex::Complex64;

use super::data_util::{one_dim, real_result, reals, scalar_result, vector};
use super::{Arity, Registry};
use crate::eval::error::{from_units, EvalError};
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    let two = Arity::Exact(2);
    r.register("line", two, line);
    r.register("slope", two, slope);
    r.register("intercept", two, intercept);
    r.register("stderr", two, stderr);
    r.register("medfit", two, medfit);
}

/// The median of a non-empty set of values.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        0.5 * (values[n / 2 - 1] + values[n / 2])
    }
}

/// `medfit(vx, vy)`: the median-median line `[a; b]` (help: "Median-Median Regression"). The data,
/// sorted by `x`, split into three groups of about a third each (the outer groups have `floor(n/3)`
/// points); each group is one point at the medians of its `x` and `y`. A least-squares line through
/// the three points gives the slope, and the intercept makes it pass through the mean of the three
/// medians. Deviation: the help gives the intercept formula (an image) and says a line is fitted to
/// the three points, so the slope is read as that least-squares slope; it is not checked against
/// Mathcad's numbers.
fn medfit(args: &[Value]) -> Result<Value, EvalError> {
    let name = "medfit";
    let xs = vector(&args[0], name)?;
    let ys = vector(&args[1], name)?;
    if xs.len() != ys.len() {
        return Err(EvalError::Unsupported(
            "medfit: vx and vy differ in length".to_string(),
        ));
    }
    if xs.len() < 3 {
        return Err(EvalError::Unsupported(
            "medfit needs at least three points".to_string(),
        ));
    }
    let dx = one_dim(&xs)?;
    let dy = one_dim(&ys)?;
    let x = reals(&xs, name)?;
    let y = reals(&ys, name)?;
    let mut order: Vec<usize> = (0..x.len()).collect();
    order.sort_by(|&p, &q| x[p].total_cmp(&x[q]));
    let k = x.len() / 3;
    let groups = [(0, k), (k, x.len() - k), (x.len() - k, x.len())];
    let mut points = Vec::with_capacity(3);
    for (lo, hi) in groups {
        let mut gx: Vec<f64> = order[lo..hi].iter().map(|&i| x[i]).collect();
        let mut gy: Vec<f64> = order[lo..hi].iter().map(|&i| y[i]).collect();
        points.push((median(&mut gx), median(&mut gy)));
    }
    let mx = points.iter().map(|p| p.0).sum::<f64>() / 3.0;
    let my = points.iter().map(|p| p.1).sum::<f64>() / 3.0;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    if !(sxx > 0.0 && sxx.is_finite()) {
        return Err(EvalError::Singular);
    }
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let slope_dim = dy.checked_div(dx).map_err(from_units)?;
    let data = vec![
        Scalar::new(Complex64::new(intercept, 0.0), dy),
        Scalar::new(Complex64::new(slope, 0.0), slope_dim),
    ];
    Ok(Value::Matrix(Matrix::new(2, 1, data)?))
}

/// The least-squares fit of one data set, with the data kept for `stderr`.
struct Fit {
    slope: f64,
    intercept: f64,
    x: Vec<f64>,
    y: Vec<f64>,
    dx: Dimension,
    dy: Dimension,
}

fn fit(args: &[Value], name: &str) -> Result<Fit, EvalError> {
    let xs = vector(&args[0], name)?;
    let ys = vector(&args[1], name)?;
    if xs.len() != ys.len() {
        return Err(EvalError::Unsupported(format!(
            "{name}: vx and vy differ in length"
        )));
    }
    if xs.len() < 2 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs at least two points"
        )));
    }
    let dx = one_dim(&xs)?;
    let dy = one_dim(&ys)?;
    let x = reals(&xs, name)?;
    let y = reals(&ys, name)?;
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let sxx: f64 = x.iter().map(|&v| (v - mx) * (v - mx)).sum();
    // The spread of x must exceed rounding noise relative to its magnitude; equal values can leave
    // a tiny non-zero sxx after the mean is rounded.
    let scale: f64 = x.iter().map(|v| v * v).sum();
    if !sxx.is_finite() || sxx <= 1e-24 * scale {
        return Err(EvalError::Singular);
    }
    let sxy: f64 = x.iter().zip(&y).map(|(&a, &b)| (a - mx) * (b - my)).sum();
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    if !(slope.is_finite() && intercept.is_finite()) {
        return Err(EvalError::NotFinite);
    }
    Ok(Fit {
        slope,
        intercept,
        x,
        y,
        dx,
        dy,
    })
}

/// The dimension of the slope: `dy / dx`.
fn slope_dim(f: &Fit) -> Result<Dimension, EvalError> {
    f.dy.checked_div(f.dx).map_err(from_units)
}

/// `line(vx, vy)`: the column `[intercept; slope]`.
fn line(args: &[Value]) -> Result<Value, EvalError> {
    let f = fit(args, "line")?;
    let data = vec![
        Scalar::new(Complex64::new(f.intercept, 0.0), f.dy),
        Scalar::new(Complex64::new(f.slope, 0.0), slope_dim(&f)?),
    ];
    Ok(Value::Matrix(Matrix::new(2, 1, data)?))
}

/// `slope(vx, vy)`.
fn slope(args: &[Value]) -> Result<Value, EvalError> {
    let f = fit(args, "slope")?;
    scalar_result(Complex64::new(f.slope, 0.0), slope_dim(&f)?)
}

/// `intercept(vx, vy)`.
fn intercept(args: &[Value]) -> Result<Value, EvalError> {
    let f = fit(args, "intercept")?;
    real_result(f.intercept, f.dy)
}

/// `stderr(vx, vy)`.
fn stderr(args: &[Value]) -> Result<Value, EvalError> {
    let f = fit(args, "stderr")?;
    let n = f.x.len();
    if n < 3 {
        return Err(EvalError::Unsupported(
            "stderr needs more than two points".to_string(),
        ));
    }
    let ssr: f64 =
        f.x.iter()
            .zip(&f.y)
            .map(|(&x, &y)| {
                let r = y - (f.intercept + f.slope * x);
                r * r
            })
            .sum();
    real_result((ssr / (n as f64 - 2.0)).sqrt(), f.dy)
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;

    #[test]
    fn medfit_recovers_an_exact_line() {
        let x: Vec<f64> = (0..9).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|v| 2.0 * v + 1.0).collect();
        let v = medfit(&[col(&x), col(&y)]).expect("medfit");
        assert_all_close(&values(&v), &[1.0, 2.0], 1e-12);
    }

    #[test]
    fn medfit_of_the_help_data_uses_the_three_group_medians() {
        // The help's data with the outlier 70. Groups (1,2), (3,4,5), (6,7) have medians
        // (1.5, 5.5), (4, 13), (6.5, 44.5): slope 7.8, intercept 21 - 7.8 * 4 = -10.2.
        let x: Vec<f64> = (1..=7).map(f64::from).collect();
        let y = [4.0, 7.0, 10.0, 13.0, 16.0, 19.0, 70.0];
        let v = medfit(&[col(&x), col(&y)]).expect("medfit");
        assert_all_close(&values(&v), &[-10.2, 7.8], 1e-9);
    }

    #[test]
    fn medfit_needs_three_points() {
        assert!(matches!(
            medfit(&[col(&[1.0, 2.0]), col(&[1.0, 2.0])]),
            Err(EvalError::Unsupported(_))
        ));
    }
    use super::*;
    use crate::units::Dimension;

    /// The help's distance and time data (Example: Linear Regression).
    const X: [f64; 9] = [8.0, 3.0, 4.0, 2.0, 5.0, 4.0, 12.0, 18.0, 3.0];
    const T: [f64; 9] = [30.0, 15.0, 30.0, 10.0, 16.0, 15.0, 30.0, 60.0, 15.0];

    #[test]
    fn exact_line_is_recovered() {
        let x = col(&[0.0, 1.0, 2.0, 5.0]);
        let y = col(&[2.0, 5.0, 8.0, 17.0]);
        let v = line(&[x.clone(), y.clone()]).expect("line");
        assert_all_close(&values(&v), &[2.0, 3.0], 1e-12);
        assert_all_close(
            &values(&slope(&[x.clone(), y.clone()]).expect("slope")),
            &[3.0],
            1e-12,
        );
        assert_all_close(
            &values(&intercept(&[x.clone(), y.clone()]).expect("intercept")),
            &[2.0],
            1e-12,
        );
        assert_all_close(&values(&stderr(&[x, y]).expect("stderr")), &[0.0], 1e-12);
    }

    #[test]
    fn help_distance_data() {
        let x = col(&X);
        let t = col(&T);
        let s = values(&slope(&[x.clone(), t.clone()]).expect("slope"))[0];
        let b = values(&intercept(&[x.clone(), t.clone()]).expect("intercept"))[0];
        assert_all_close(&[s], &[SLOPE_REF], 1e-9);
        assert_all_close(&[b], &[INTERCEPT_REF], 1e-9);
        let l = values(&line(&[x.clone(), t.clone()]).expect("line"));
        assert_all_close(&l, &[b, s], 1e-12);
        let se = values(&stderr(&[x, t]).expect("stderr"))[0];
        assert_all_close(&[se], &[STDERR_REF], 1e-9);
    }

    /// Reference values from exact rational arithmetic on the help data (Python `fractions`).
    const SLOPE_REF: f64 = 2.681_367_690_782_953;
    const INTERCEPT_REF: f64 = 6.977_700_693_756_194;
    const STDERR_REF: f64 = 6.460_035_167_693_158;

    #[test]
    fn equal_x_values_are_singular_despite_rounding_noise() {
        let y = col(&[1.0, 2.0, 4.0]);
        // The mean of three equal values can round, leaving a tiny non-zero sxx.
        let same = col(&[0.1, 0.1, 0.1]);
        assert!(matches!(
            slope(&[same.clone(), y.clone()]),
            Err(EvalError::Singular)
        ));
        assert!(matches!(
            line(&[same.clone(), y.clone()]),
            Err(EvalError::Singular)
        ));
        assert!(matches!(
            stderr(&[same, y.clone()]),
            Err(EvalError::Singular)
        ));
        let noisy = col(&[0.1 + 0.2, 0.3, 0.3]);
        assert!(matches!(slope(&[noisy, y]), Err(EvalError::Singular)));
    }

    #[test]
    fn slope_of_the_help_data_matches_the_closed_form() {
        let sxx = 611.0 - 59.0 * 59.0 / 9.0;
        let sxy = 2050.0 - 59.0 * 221.0 / 9.0;
        let s = values(&slope(&[col(&X), col(&T)]).expect("slope"))[0];
        assert!((s - sxy / sxx).abs() < 1e-12);
    }

    #[test]
    fn stderr_is_the_residual_standard_error() {
        let x = col(&X);
        let t = col(&T);
        let s = values(&slope(&[x.clone(), t.clone()]).expect("slope"))[0];
        let b = values(&intercept(&[x.clone(), t.clone()]).expect("intercept"))[0];
        let ssr: f64 = X
            .iter()
            .zip(&T)
            .map(|(&a, &c)| (c - (b + s * a)).powi(2))
            .sum();
        let want = (ssr / 7.0).sqrt();
        let got = values(&stderr(&[x, t]).expect("stderr"))[0];
        assert!((got - want).abs() < 1e-12, "{got} vs {want}");
    }

    #[test]
    fn units_follow_the_data() {
        let m = Dimension::base(0);
        let s = Dimension::base(1);
        let x = Value::Matrix(
            Matrix::new(
                3,
                1,
                vec![
                    Scalar::real(1.0, m),
                    Scalar::real(2.0, m),
                    Scalar::real(3.0, m),
                ],
            )
            .expect("x"),
        );
        let y = Value::Matrix(
            Matrix::new(
                3,
                1,
                vec![
                    Scalar::real(2.0, s),
                    Scalar::real(4.0, s),
                    Scalar::real(6.0, s),
                ],
            )
            .expect("y"),
        );
        let slope_dim = s.checked_div(m).expect("dim");
        assert_eq!(
            dim_of(&slope(&[x.clone(), y.clone()]).expect("slope")),
            slope_dim
        );
        assert_eq!(
            dim_of(&intercept(&[x.clone(), y.clone()]).expect("intercept")),
            s
        );
        assert_eq!(dim_of(&stderr(&[x.clone(), y.clone()]).expect("stderr")), s);
        let l = match line(&[x, y]).expect("line") {
            Value::Matrix(mm) => mm,
            other => panic!("{other:?}"),
        };
        assert_eq!(l.data()[0].dim, s);
        assert_eq!(l.data()[1].dim, slope_dim);
    }

    #[test]
    fn degenerate_data_is_refused() {
        assert!(matches!(
            slope(&[col(&[1.0, 1.0, 1.0]), col(&[1.0, 2.0, 3.0])]),
            Err(EvalError::Singular)
        ));
        assert!(matches!(
            stderr(&[col(&[1.0, 2.0]), col(&[1.0, 2.0])]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            line(&[col(&[1.0, 2.0]), col(&[1.0])]),
            Err(EvalError::Unsupported(_))
        ));
    }
}

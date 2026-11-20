//! Smoothing and convolution (Mathcad Prime help: "Data Smoothing", "Median Smoothing",
//! "Gaussian Kernel Smoothing", "Convolution and Cross Correlation").
//!
//! - `medsmooth(v, [n])`: each value replaced by the median of the `n` points centred on it (`n`
//!   odd, default 3). The window is contracted near the ends so it never leaves the vector.
//! - `ksmooth(vx, vy, b)`: Gaussian kernel smoothing, `vy'_i = sum_j K_ij vy_j / sum_j K_ij` with
//!   `K_ij = exp(-((vx_i - vx_j) / b)^2 / (2 * 0.37^2))` (the help's constant factor cancels).
//! - `convolve(S, K)`: one-dimensional full linear convolution, length `len(S) + len(K) - 1`.
//! - `deconvolve(vz, vx)`: the exact inverse, `vz = convolve(vx, h)`; refused when not exact.
//! - `movavg(v, n)`: the trailing moving average, `y_t` the mean of `v` over the last `n` points up
//!   to `t` (fewer at the start, where it averages all points so far), as the help's example text
//!   states. Same length as `v`.
//! - `expsmooth(v, a)`: `y_0 = v_0`, `y_t = a v_t + (1 - a) y_{t-1}` (the help's formula, `0 <= a <= 1`).
//! - `supsmooth(vx, vy)`: Friedman's super smoother (1984): running-line smoothers with spans 0.05,
//!   0.2 and 0.5, the span chosen per point from the cross-validated residuals smoothed with span
//!   0.2, the chosen smooth interpolated between the three, and a final smooth with span 0.05. The
//!   bass enhancement is not used (the help does not describe it). Ties in `vx` are averaged as in
//!   Friedman's code. Deviation: Mathcad's own implementation is not checked against.
//!
//! Not implemented: `medsmooth` on matrices and `VSmooth`; `convolve` on matrices and its BC and OV
//! arguments (the help's two-dimensional form); `movavg` and `expsmooth` on matrices.

use num_complex::Complex64;

use super::data_util::{column, complexes, count, grid, one_dim, real_scalar, reals, vector};
use super::{Arity, Registry};
use crate::eval::error::{from_units, EvalError};
use crate::eval::value::Value;

/// Registers the functions of this file.
pub fn register(r: &mut Registry) {
    r.register("medsmooth", Arity::Range(1, 2), medsmooth);
    r.register("ksmooth", Arity::Exact(3), ksmooth);
    r.register("convolve", Arity::Exact(2), convolve);
    r.register("deconvolve", Arity::Exact(2), deconvolve);
    r.register("movavg", Arity::Exact(2), movavg);
    r.register("expsmooth", Arity::Exact(2), expsmooth);
    r.register("supsmooth", Arity::Exact(2), supsmooth);
}

/// `medsmooth(v, [n])` for a vector.
fn medsmooth(args: &[Value]) -> Result<Value, EvalError> {
    let name = "medsmooth";
    let (_, cols, data) = grid(&args[0], name)?;
    if cols != 1 {
        return Err(EvalError::Unsupported(
            "medsmooth of a matrix is not implemented (vectors only)".to_string(),
        ));
    }
    let dim = one_dim(&data)?;
    let v = reals(&data, name)?;
    let width = if args.len() > 1 {
        count(&args[1], name)?
    } else {
        3
    };
    if width % 2 == 0 {
        return Err(EvalError::Unsupported(
            "medsmooth needs an odd window width".to_string(),
        ));
    }
    let half = width / 2;
    let n = v.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let h = half.min(i).min(n - 1 - i);
        let mut window = v[i - h..=i + h].to_vec();
        window.sort_by(f64::total_cmp);
        out.push(Complex64::new(window[window.len() / 2], 0.0));
    }
    column(&out, dim)
}

/// `ksmooth(vx, vy, b)`.
fn ksmooth(args: &[Value]) -> Result<Value, EvalError> {
    let name = "ksmooth";
    let xs = vector(&args[0], name)?;
    let ys = vector(&args[1], name)?;
    if xs.len() != ys.len() {
        return Err(EvalError::Unsupported(
            "ksmooth: vx and vy differ in length".to_string(),
        ));
    }
    let dx = one_dim(&xs)?;
    let dy = one_dim(&ys)?;
    let x = reals(&xs, name)?;
    let y = reals(&ys, name)?;
    let (b, bdim) = real_scalar(&args[2], name)?;
    if bdim != dx {
        return Err(EvalError::DimensionMismatch {
            left: dx,
            right: bdim,
        });
    }
    if b <= 0.0 {
        return Err(EvalError::Unsupported(
            "ksmooth needs a positive bandwidth".to_string(),
        ));
    }
    // 2 * 0.37^2: the kernel's normalising factor cancels in the ratio.
    let denom = 2.0 * 0.37 * 0.37;
    let out: Vec<Complex64> = (0..x.len())
        .map(|i| {
            let (mut num, mut den) = (0.0, 0.0);
            for (&xj, &yj) in x.iter().zip(&y) {
                let t = (x[i] - xj) / b;
                let w = (-t * t / denom).exp();
                num += w * yj;
                den += w;
            }
            // The term j = i has weight 1, so `den >= 1` and the quotient is well defined.
            Complex64::new(num / den, 0.0)
        })
        .collect();
    column(&out, dy)
}

/// `convolve(S, K)`: the full linear convolution of two vectors.
fn convolve(args: &[Value]) -> Result<Value, EvalError> {
    let name = "convolve";
    let s = vector(&args[0], name)?;
    let k = vector(&args[1], name)?;
    let ds = one_dim(&s)?;
    let dk = one_dim(&k)?;
    let sv = complexes(&s)?;
    let kv = complexes(&k)?;
    let dim = ds.checked_mul(dk).map_err(from_units)?;
    let mut out = vec![Complex64::new(0.0, 0.0); sv.len() + kv.len() - 1];
    for (j, &a) in sv.iter().enumerate() {
        for (i, &b) in kv.iter().enumerate() {
            out[j + i] += a * b;
        }
    }
    column(&out, dim)
}

/// `deconvolve(vz, vx)`: the kernel `h` with `vz = convolve(vx, h)`, by polynomial long division
/// from the leading element of `vx`. The quotient has `len(vz) - len(vx) + 1` elements. The
/// division must be exact: a remainder beyond rounding is refused (the help gives no rule for
/// it), and so is a zero leading element of `vx`.
fn deconvolve(args: &[Value]) -> Result<Value, EvalError> {
    let name = "deconvolve";
    let z = vector(&args[0], name)?;
    let x = vector(&args[1], name)?;
    let dz = one_dim(&z)?;
    let dx = one_dim(&x)?;
    let zv = complexes(&z)?;
    let xv = complexes(&x)?;
    let (nz, nx) = (zv.len(), xv.len());
    if nx > nz {
        return Err(EvalError::Unsupported(
            "deconvolve needs vz at least as long as vx".to_string(),
        ));
    }
    if xv[0].re == 0.0 && xv[0].im == 0.0 {
        return Err(EvalError::Unsupported(
            "deconvolve needs a non-zero first element of vx".to_string(),
        ));
    }
    let m = nz - nx + 1;
    let mut h: Vec<Complex64> = Vec::with_capacity(m);
    for j in 0..m {
        let mut acc = zv[j];
        for i in 1..nx.min(j + 1) {
            acc -= xv[i] * h[j - i];
        }
        h.push(acc / xv[0]);
    }
    // Check that the quotient reproduces vz exactly (up to rounding).
    let scale = zv.iter().map(|z| z.norm()).fold(0.0, f64::max).max(1e-300);
    for j in 0..nz {
        let mut acc = Complex64::new(0.0, 0.0);
        for i in 0..nx {
            if j >= i && j - i < m {
                acc += xv[i] * h[j - i];
            }
        }
        if (acc - zv[j]).norm() > 1e-9 * scale {
            return Err(EvalError::Unsupported(
                "deconvolve: vz is not an exact convolution of vx (non-zero remainder)".to_string(),
            ));
        }
    }
    let dim = dz.checked_div(dx).map_err(from_units)?;
    column(&h, dim)
}

/// `movavg(v, n)`: the trailing moving average of a column vector.
fn movavg(args: &[Value]) -> Result<Value, EvalError> {
    let name = "movavg";
    let xs = vector(&args[0], name)?;
    let dim = one_dim(&xs)?;
    let v = complexes(&xs)?;
    let width = count(&args[1], name)?;
    let out: Vec<Complex64> = (0..v.len())
        .map(|t| {
            let first = (t + 1).saturating_sub(width);
            let window = &v[first..=t];
            window.iter().sum::<Complex64>() / window.len() as f64
        })
        .collect();
    column(&out, dim)
}

/// `expsmooth(v, a)`: exponential smoothing of a column vector with weight `a` in `[0, 1]`.
fn expsmooth(args: &[Value]) -> Result<Value, EvalError> {
    let name = "expsmooth";
    let xs = vector(&args[0], name)?;
    let dim = one_dim(&xs)?;
    let v = complexes(&xs)?;
    let (a, adim) = real_scalar(&args[1], name)?;
    if !adim.is_dimensionless() {
        return Err(EvalError::DimensionNotDimensionless { dim: adim });
    }
    if !(0.0..=1.0).contains(&a) {
        return Err(EvalError::Unsupported(
            "expsmooth needs a weight between 0 and 1".to_string(),
        ));
    }
    let mut out: Vec<Complex64> = Vec::with_capacity(v.len());
    for (t, &x) in v.iter().enumerate() {
        let next = match out.last() {
            Some(&prev) if t > 0 => x * a + prev * (1.0 - a),
            _ => x,
        };
        out.push(next);
    }
    column(&out, dim)
}

/// `supsmooth(vx, vy)`: Friedman's super smoother (see the module comment).
fn supsmooth(args: &[Value]) -> Result<Value, EvalError> {
    let name = "supsmooth";
    let xs = vector(&args[0], name)?;
    let ys = vector(&args[1], name)?;
    if xs.len() != ys.len() {
        return Err(EvalError::Unsupported(
            "supsmooth: vx and vy differ in length".to_string(),
        ));
    }
    one_dim(&xs)?;
    let dy = one_dim(&ys)?;
    let x = reals(&xs, name)?;
    let y = reals(&ys, name)?;
    if x.windows(2).any(|w| w[1] < w[0]) {
        return Err(EvalError::Unsupported(
            "supsmooth needs vx in ascending order".to_string(),
        ));
    }
    let smo = super_smoother(&x, &y);
    let zs: Vec<Complex64> = smo.iter().map(|&v| Complex64::new(v, 0.0)).collect();
    column(&zs, dy)
}

/// The spans of the super smoother: tweeter, midrange and woofer.
const SPANS: [f64; 3] = [0.05, 0.2, 0.5];

/// Friedman's super smoother with unit weights, for `x` ascending.
fn super_smoother(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    let w = vec![1.0; n];
    if n < 2 || x[n - 1] <= x[0] {
        let mean = y.iter().sum::<f64>() / n as f64;
        return vec![mean; n];
    }
    // The scale of x: the spread of its inner quartiles (Friedman's `scale`, with i = n/4, j = 3i).
    let quarter = (n / 4).max(1);
    let mut i = quarter - 1;
    let mut j = (3 * quarter - 1).min(n - 1);
    let mut scale = x[j] - x[i];
    while scale <= 0.0 {
        if j < n - 1 {
            j += 1;
        }
        i = i.saturating_sub(1);
        scale = x[j] - x[i];
    }
    let vsmlsq = (1e-3 * scale) * (1e-3 * scale);

    let mut smooths: Vec<Vec<f64>> = Vec::with_capacity(3);
    let mut midrange_residuals: Vec<Vec<f64>> = Vec::with_capacity(3);
    for &span in &SPANS {
        let (smo, cv) = running_lines(x, y, &w, span, true, vsmlsq);
        midrange_residuals.push(running_lines(x, &cv, &w, SPANS[1], false, vsmlsq).0);
        smooths.push(smo);
    }
    // The span with the least smoothed cross-validated residual, per point.
    let best: Vec<f64> = (0..n)
        .map(|k| {
            let mut best_span = SPANS[0];
            let mut least = f64::INFINITY;
            for (s, res) in SPANS.iter().zip(&midrange_residuals) {
                if res[k] < least {
                    least = res[k];
                    best_span = *s;
                }
            }
            best_span
        })
        .collect();
    let chosen = running_lines(x, &best, &w, SPANS[1], false, vsmlsq).0;
    // Interpolate between the smooths of the two spans either side of the chosen one.
    let blended: Vec<f64> = (0..n)
        .map(|k| {
            let s = chosen[k].clamp(SPANS[0], SPANS[2]);
            let (lo, mid, hi) = (smooths[0][k], smooths[1][k], smooths[2][k]);
            if s >= SPANS[1] {
                let f = (s - SPANS[1]) / (SPANS[2] - SPANS[1]);
                (1.0 - f) * mid + f * hi
            } else {
                let f = (SPANS[1] - s) / (SPANS[1] - SPANS[0]);
                (1.0 - f) * mid + f * lo
            }
        })
        .collect();
    running_lines(x, &blended, &w, SPANS[0], false, vsmlsq).0
}

/// One running-lines smooth with span `span` (a fraction of `n`): at each point, the weighted least
/// squares line through the `2 k + 1` nearest points (the window stays at the first or last
/// `2 k + 1` points near the ends). With `cv`, also the absolute cross-validated residual of each
/// point. Equal `x` values share their mean. Friedman's `smooth` routine.
fn running_lines(
    x: &[f64],
    y: &[f64],
    w: &[f64],
    span: f64,
    cv: bool,
    vsmlsq: f64,
) -> (Vec<f64>, Vec<f64>) {
    let n = x.len();
    let ibw = ((0.5 * span * n as f64 + 0.5) as usize).max(2);
    let it = (2 * ibw + 1).min(n);
    // Window sums: weighted means of x and y, the variance of x and the covariance.
    let mut win = Window::default();
    for k in 0..it {
        win.add(x[k], w[k], y[k]);
    }
    let mut smo = vec![0.0; n];
    let mut acvr = vec![0.0; n];
    for j in 0..n {
        // Slide the window once it is centred: drop the point `ibw + 1` before `j`, add `ibw` after.
        let out = j as isize - ibw as isize - 1;
        let inn = j + ibw;
        if out >= 0 && inn < n {
            let k = out as usize;
            win.remove(x[k], w[k], y[k]);
            win.add(x[inn], w[inn], y[inn]);
        }
        let a = if win.var > vsmlsq {
            win.cvar / win.var
        } else {
            0.0
        };
        smo[j] = a * (x[j] - win.xm) + win.ym;
        if cv {
            let mut h = if win.fbw > 0.0 { 1.0 / win.fbw } else { 0.0 };
            if win.var > vsmlsq {
                h += (x[j] - win.xm) * (x[j] - win.xm) / win.var;
            }
            let d = 1.0 - w[j] * h;
            acvr[j] = if d > 0.0 {
                (y[j] - smo[j]).abs() / d
            } else if j > 0 {
                acvr[j - 1]
            } else {
                0.0
            };
        }
    }
    // Equal x values share one smoothed value: the mean over the tie.
    let mut j = 0;
    while j < n {
        let start = j;
        let mut sum = w[j] * smo[j];
        let mut weight = w[j];
        while j + 1 < n && x[j + 1] <= x[j] {
            j += 1;
            sum += w[j] * smo[j];
            weight += w[j];
        }
        if j > start {
            let mean = sum / weight;
            for v in &mut smo[start..=j] {
                *v = mean;
            }
        }
        j += 1;
    }
    (smo, acvr)
}

/// The weighted running sums of one window of points for [`running_lines`].
#[derive(Default)]
struct Window {
    xm: f64,
    ym: f64,
    var: f64,
    cvar: f64,
    fbw: f64,
}

impl Window {
    fn add(&mut self, xt: f64, wt: f64, yt: f64) {
        let fbo = self.fbw;
        self.fbw += wt;
        if self.fbw > 0.0 {
            self.xm = (fbo * self.xm + wt * xt) / self.fbw;
            self.ym = (fbo * self.ym + wt * yt) / self.fbw;
        }
        let tmp = if fbo > 0.0 {
            self.fbw * wt * (xt - self.xm) / fbo
        } else {
            0.0
        };
        self.var += tmp * (xt - self.xm);
        self.cvar += tmp * (yt - self.ym);
    }

    fn remove(&mut self, xt: f64, wt: f64, yt: f64) {
        let fbo = self.fbw;
        self.fbw -= wt;
        if self.fbw > 0.0 {
            self.xm = (fbo * self.xm - wt * xt) / self.fbw;
            self.ym = (fbo * self.ym - wt * yt) / self.fbw;
        }
        // The inverse of `add`: the same term, with the new total in the numerator.
        let tmp = if self.fbw > 0.0 && fbo > 0.0 {
            self.fbw * wt * (xt - self.xm) / fbo
        } else {
            0.0
        };
        self.var -= tmp * (xt - self.xm);
        self.cvar -= tmp * (yt - self.ym);
    }
}

#[cfg(test)]
mod tests {
    use super::super::data_util::testing::*;
    use super::*;

    #[test]
    fn medsmooth_contracts_the_window_at_the_ends() {
        let v = medsmooth(&[col(&[1.0, 5.0, 2.0, 8.0, 3.0])]).expect("medsmooth");
        assert_all_close(&values(&v), &[1.0, 2.0, 5.0, 3.0, 3.0], 0.0);
    }

    #[test]
    fn medsmooth_removes_a_spike_and_keeps_a_line() {
        let spike = col(&[1.0, 1.0, 1.0, 100.0, 1.0, 1.0, 1.0]);
        let v = medsmooth(&[spike, num(3.0)]).expect("medsmooth");
        assert_all_close(&values(&v), &[1.0; 7], 0.0);
        let line = col(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let v = medsmooth(&[line.clone(), num(5.0)]).expect("medsmooth");
        assert_all_close(&values(&v), &[0.0, 1.0, 2.0, 3.0, 4.0], 0.0);
    }

    #[test]
    fn medsmooth_refuses_even_windows_and_matrices() {
        assert!(matches!(
            medsmooth(&[col(&[1.0, 2.0, 3.0]), num(2.0)]),
            Err(EvalError::Unsupported(_))
        ));
        let m = Value::Matrix(
            crate::eval::value::Matrix::new(
                2,
                2,
                (0..4)
                    .map(|k| crate::eval::value::Scalar::real(f64::from(k), dimless()))
                    .collect(),
            )
            .expect("m"),
        );
        assert!(matches!(medsmooth(&[m]), Err(EvalError::Unsupported(_))));
    }

    #[test]
    fn ksmooth_of_constant_data_is_constant_and_of_a_wide_kernel_is_the_mean() {
        let x = col(&[0.0, 1.0, 2.0, 3.0]);
        let c = ksmooth(&[x.clone(), col(&[4.0; 4]), num(0.5)]).expect("ksmooth");
        assert_all_close(&values(&c), &[4.0; 4], 1e-12);
        let y = col(&[1.0, 5.0, 2.0, 8.0]);
        let wide = ksmooth(&[x, y, num(1e6)]).expect("ksmooth");
        assert_all_close(&values(&wide), &[4.0; 4], 1e-6);
    }

    #[test]
    fn ksmooth_with_a_narrow_kernel_keeps_the_data() {
        let x = col(&[0.0, 1.0, 2.0]);
        let y = col(&[3.0, -1.0, 7.0]);
        let v = ksmooth(&[x, y.clone(), num(1e-4)]).expect("ksmooth");
        assert_all_close(&values(&v), &values(&y), 1e-9);
    }

    #[test]
    fn ksmooth_refuses_a_bandwidth_of_another_dimension() {
        let m = crate::units::Dimension::base(0);
        let b = Value::Scalar(crate::eval::value::Scalar::real(
            1.0,
            crate::units::Dimension::base(1),
        ));
        let x = Value::Matrix(
            crate::eval::value::Matrix::new(
                2,
                1,
                vec![
                    crate::eval::value::Scalar::real(0.0, m),
                    crate::eval::value::Scalar::real(1.0, m),
                ],
            )
            .expect("x"),
        );
        assert!(matches!(
            ksmooth(&[x.clone(), x, b]),
            Err(EvalError::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn convolve_is_the_full_linear_convolution() {
        let v = convolve(&[col(&[1.0, 2.0, 3.0]), col(&[1.0, 1.0])]).expect("convolve");
        assert_all_close(&values(&v), &[1.0, 3.0, 5.0, 3.0], 1e-12);
        let v = convolve(&[col(&[1.0, 0.0, 0.0]), col(&[0.5, -2.0])]).expect("convolve");
        assert_all_close(&values(&v), &[0.5, -2.0, 0.0, 0.0], 1e-12);
    }

    #[test]
    fn movavg_is_the_trailing_mean_and_starts_with_the_mean_so_far() {
        let v = movavg(&[col(&[1.0, 2.0, 3.0, 4.0]), num(2.0)]).expect("movavg");
        assert_all_close(&values(&v), &[1.0, 1.5, 2.5, 3.5], 1e-12);
        let v = movavg(&[col(&[1.0, 2.0, 3.0, 4.0]), num(3.0)]).expect("movavg");
        assert_all_close(&values(&v), &[1.0, 1.5, 2.0, 3.0], 1e-12);
        let v = movavg(&[col(&[5.0, 6.0]), num(1.0)]).expect("movavg");
        assert_all_close(&values(&v), &[5.0, 6.0], 0.0);
    }

    #[test]
    fn expsmooth_follows_the_help_recurrence() {
        let v = expsmooth(&[col(&[1.0, 2.0, 3.0]), num(0.5)]).expect("expsmooth");
        assert_all_close(&values(&v), &[1.0, 1.5, 2.25], 1e-12);
        let id = expsmooth(&[col(&[4.0, -1.0, 7.0]), num(1.0)]).expect("expsmooth");
        assert_all_close(&values(&id), &[4.0, -1.0, 7.0], 0.0);
        let held = expsmooth(&[col(&[4.0, -1.0, 7.0]), num(0.0)]).expect("expsmooth");
        assert_all_close(&values(&held), &[4.0, 4.0, 4.0], 0.0);
        assert!(matches!(
            expsmooth(&[col(&[1.0]), num(1.5)]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn supsmooth_reproduces_lines_and_constants_exactly() {
        let x: Vec<f64> = (0..30).map(f64::from).collect();
        let line: Vec<f64> = x.iter().map(|v| 2.0 * v + 1.0).collect();
        let v = supsmooth(&[col(&x), col(&line)]).expect("supsmooth");
        assert_all_close(&values(&v), &line, 1e-9);
        let flat = supsmooth(&[col(&x), col(&[3.0; 30])]).expect("supsmooth");
        assert_all_close(&values(&flat), &[3.0; 30], 1e-9);
    }

    #[test]
    fn supsmooth_removes_alternating_noise_from_a_sine() {
        let x: Vec<f64> = (0..200).map(|i| f64::from(i) / 50.0).collect();
        let truth: Vec<f64> = x.iter().map(|v| v.sin()).collect();
        let noisy: Vec<f64> = truth
            .iter()
            .enumerate()
            .map(|(i, t)| t + if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect();
        let v = values(&supsmooth(&[col(&x), col(&noisy)]).expect("supsmooth"));
        let worst = v
            .iter()
            .zip(&truth)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        assert!(worst < 0.05, "worst deviation {worst}");
    }

    #[test]
    fn supsmooth_refuses_unsorted_x() {
        assert!(matches!(
            supsmooth(&[col(&[0.0, 2.0, 1.0]), col(&[1.0, 2.0, 3.0])]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn deconvolve_reproduces_the_help_ratio() {
        // Help example: deconvolve(M1^(0), M2^(0)) = 0.5 with M2 = 2 M1 (first column shown).
        let m1 = [30.0, 38.0, 30.0, 38.0, 30.0];
        let m2: Vec<f64> = m1.iter().map(|v| 2.0 * v).collect();
        let v = deconvolve(&[col(&m1), col(&m2)]).expect("deconvolve");
        assert_all_close(&values(&v), &[0.5], 1e-12);
    }

    #[test]
    fn deconvolve_inverts_convolve() {
        let s = [1.0, -2.0, 0.5, 3.0, 4.0];
        let k = [2.0, 1.0, -1.0];
        let full = convolve(&[col(&s), col(&k)]).expect("convolve");
        let h = deconvolve(&[full, col(&k)]).expect("deconvolve");
        assert_all_close(&values(&h), &s, 1e-9);
    }

    #[test]
    fn deconvolve_refuses_a_remainder_and_a_zero_leading_element() {
        assert!(matches!(
            deconvolve(&[col(&[1.0, 2.0, 3.0, 5.0]), col(&[1.0, 1.0])]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            deconvolve(&[col(&[1.0, 2.0]), col(&[0.0, 1.0])]),
            Err(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            deconvolve(&[col(&[1.0]), col(&[1.0, 2.0])]),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn convolve_length_is_one_less_than_the_sum_of_lengths() {
        let s = col(&[1.0; 201]);
        let k = col(&[1.0; 51]);
        let v = convolve(&[s, k]).expect("convolve");
        assert_eq!(values(&v).len(), 201 + 51 - 1);
    }
}

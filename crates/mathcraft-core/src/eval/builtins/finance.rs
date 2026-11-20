//! Financial functions: time value of money, cash-flow rates and loan schedules.
//!
//! Help: `about_finance_functions` (the sixteen functions and the sign rule), `present_value` (`pv`,
//! `npv`), `future_value` (`fv`, `fvadj`, `fvc`), `payment` (`pmt`, `ppmt`, `ipmt`),
//! `number_of_periods` (`cnper`, `nper`), `interest_rate` (`crate`, `nom`, `rate`), `internal_rate`
//! (`irr`, `mirr`) and `cum_interest_cum_principal_and_apr` (`cumint`, `cumprn`, `eff`).
//!
//! Conventions. Money paid out is negative and money received positive. Every value is a real,
//! dimensionless number (a currency unit is not modelled). `type` is 0 for payments at the end of a
//! period and 1 for the beginning. The periods `nper`, `per`, `start` and `end` are integers in
//! `1..=MAX_PERIODS`. Rates are per period; a rate must be greater than -1.
//!
//! The rate solvers (`rate`, `irr`) are Newton iterations with the help's bound of 20 iterations.
//! When the step is not below 1e-9 (the help's 1e-7 percent) after 20 steps the call fails with
//! `NoConvergence`, as the help says. Each iteration is charged to the work budget through the
//! class in `work_table`.
//!
//! Closed forms in the time value of money, with `G(r, n) = ((1 + r)^n - 1) / r` (the series `n`
//! when `r` is 0):
//! `fv = -(pv (1+r)^n + pmt (1+rt) G)`, `pv = -(pmt (1+rt) G + fv) / (1+r)^n`,
//! `pmt = -(pv (1+r)^n + fv) / ((1+rt) G)`, and `nper` solves the same equation for `n`.
//!
//! `ipmt` follows the spreadsheet convention (Excel's `IPMT`, LibreOffice's implementation): the
//! interest of period `per` is `r` times the balance `fv(r, per-1-type, ...)`, and it is 0 for
//! `type = 1` in period 1. The help images show only `type = 0` results, so the `type = 1` form is
//! not checked against Prime.

use super::{Arity, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

/// Largest period count (`nper`, `per`, `start`, `end`) a finance function accepts. The loops over
/// periods (`cumint`, `cumprn`) take time in proportion to it.
pub const MAX_PERIODS: i64 = 1_000_000;

/// Newton iterations allowed to `rate` and `irr`, as the help states.
const SOLVER_ITERATIONS: usize = 20;

/// The help's accuracy, 1e-7 percent, as a step on a fraction.
const SOLVER_TOL: f64 = 1e-9;

/// Registers the finance functions.
pub fn register(r: &mut Registry) {
    r.register("fv", Arity::Range(3, 5), fv);
    r.register("fvadj", Arity::Exact(2), fvadj);
    r.register("fvc", Arity::Exact(2), fvc);
    r.register("pv", Arity::Range(3, 5), pv);
    r.register("npv", Arity::Exact(2), npv);
    r.register("pmt", Arity::Range(3, 5), pmt);
    r.register("ppmt", Arity::Range(4, 6), ppmt);
    r.register("ipmt", Arity::Range(4, 6), ipmt);
    r.register("nper", Arity::Range(3, 5), nper);
    r.register("cnper", Arity::Exact(3), cnper);
    r.register("rate", Arity::Range(3, 6), rate);
    r.register("crate", Arity::Exact(3), crate_fn);
    r.register("nom", Arity::Exact(2), nom);
    r.register("eff", Arity::Exact(2), eff);
    r.register("cumint", Arity::Range(5, 6), cumint);
    r.register("cumprn", Arity::Range(5, 6), cumprn);
    r.register("irr", Arity::Range(1, 2), irr);
    r.register("mirr", Arity::Exact(3), mirr);
}

/// A real, dimensionless scalar argument.
fn number(v: &Value, name: &str) -> Result<f64, EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!(
            "{name} takes a scalar here"
        )));
    };
    ops::require_dimensionless(s.dim)?;
    if s.value.im != 0.0 {
        return Err(EvalError::Unsupported(format!(
            "{name} needs a real number"
        )));
    }
    if !s.value.re.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(s.value.re)
}

/// A period count: an integer in `1..=MAX_PERIODS`.
fn count(v: &Value, name: &str) -> Result<i64, EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!(
            "{name} takes a scalar here"
        )));
    };
    ops::require_dimensionless(s.dim)?;
    let k = integer_of(s)
        .ok_or_else(|| EvalError::Unsupported(format!("{name} must be an integer")))?;
    if !(1..=MAX_PERIODS).contains(&k) {
        return Err(EvalError::Unsupported(format!(
            "{name} must be between 1 and {MAX_PERIODS}"
        )));
    }
    Ok(k)
}

/// The `type` flag: 0 (end of period) or 1 (beginning), as `f64`.
fn timing(v: &Value) -> Result<f64, EvalError> {
    match count_or_zero(v)? {
        0 => Ok(0.0),
        1 => Ok(1.0),
        _ => Err(EvalError::Unsupported("type must be 0 or 1".to_string())),
    }
}

/// An integer in `0..=MAX_PERIODS`, used for `type`.
fn count_or_zero(v: &Value) -> Result<i64, EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported("type must be 0 or 1".to_string()));
    };
    ops::require_dimensionless(s.dim)?;
    integer_of(s).ok_or_else(|| EvalError::Unsupported("type must be 0 or 1".to_string()))
}

/// A vector of real dimensionless numbers (a row or a column matrix).
fn vector(v: &Value, name: &str) -> Result<Vec<f64>, EvalError> {
    let Value::Matrix(m) = v else {
        return Err(EvalError::Unsupported(format!("{name} takes a vector")));
    };
    if m.rows() != 1 && m.cols() != 1 {
        return Err(EvalError::Unsupported(format!("{name} takes a vector")));
    }
    m.data()
        .iter()
        .map(|s| {
            ops::require_dimensionless(s.dim)?;
            if s.value.im != 0.0 {
                return Err(EvalError::Unsupported(format!("{name} needs real numbers")));
            }
            if !s.value.re.is_finite() {
                return Err(EvalError::NotFinite);
            }
            Ok(s.value.re)
        })
        .collect()
}

/// The argument at `i`, when the call gave it.
fn optional(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i)
}

/// A dimensionless result, refused when it is not finite.
fn result(x: f64) -> Result<Value, EvalError> {
    if !x.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::real(x, Dimension::dimensionless())))
}

/// The rate check shared by the functions that compound: `1 + r > 0`.
fn check_rate(r: f64) -> Result<(), EvalError> {
    if r > -1.0 {
        Ok(())
    } else {
        Err(EvalError::Unsupported(
            "rate must be greater than -1".to_string(),
        ))
    }
}

/// `G(r, n) = ((1 + r)^n - 1) / r`, the sum of `(1 + r)^k` for `k < n`. The series `n + n (n-1) r / 2`
/// stands in for `r` near 0, where the quotient cancels.
fn growth_sum(r: f64, n: f64) -> f64 {
    if r.abs() < 1e-12 {
        n + n * (n - 1.0) * r / 2.0
    } else {
        ((1.0 + r).powf(n) - 1.0) / r
    }
}

/// The future value of the cash flows of an annuity: `-(pv (1+r)^n + pmt (1+rt) G)`.
fn future_balance(r: f64, n: f64, pmt: f64, pv: f64, t: f64) -> f64 {
    -(pv * (1.0 + r).powf(n) + pmt * (1.0 + r * t) * growth_sum(r, n))
}

/// The payment of a loan or investment, from the same equation solved for `pmt`.
fn payment(r: f64, n: f64, pv: f64, fv: f64, t: f64) -> f64 {
    -(pv * (1.0 + r).powf(n) + fv) / ((1.0 + r * t) * growth_sum(r, n))
}

/// `fv(rate, nper, pmt, [pv], [type])`: the future value of periodic constant payments.
fn fv(args: &[Value]) -> Result<Value, EvalError> {
    let r = number(&args[0], "fv")?;
    check_rate(r)?;
    let n = count(&args[1], "nper")? as f64;
    let pmt = number(&args[2], "fv")?;
    let pv = optional(args, 3).map_or(Ok(0.0), |v| number(v, "fv"))?;
    let t = optional(args, 4).map_or(Ok(0.0), timing)?;
    result(future_balance(r, n, pmt, pv, t))
}

/// `fvadj(prin, sched)`: the initial principal after each rate of `sched` is applied in turn.
fn fvadj(args: &[Value]) -> Result<Value, EvalError> {
    let prin = number(&args[0], "fvadj")?;
    let mut value = prin;
    for s in vector(&args[1], "fvadj")? {
        value *= 1.0 + s;
    }
    result(value)
}

/// `fvc(rate, cash)`: the value after the last cash flow of a series, each earning `rate` until then.
/// Horner's rule: `fv = fv (1+r) + c` over the flows in order, so the first flow earns `N - 1`
/// periods and the last none.
fn fvc(args: &[Value]) -> Result<Value, EvalError> {
    let r = number(&args[0], "fvc")?;
    check_rate(r)?;
    let mut value = 0.0;
    for c in vector(&args[1], "fvc")? {
        value = value * (1.0 + r) + c;
    }
    result(value)
}

/// `pv(rate, nper, pmt, [fv], [type])`: the present value of periodic constant payments.
fn pv(args: &[Value]) -> Result<Value, EvalError> {
    let r = number(&args[0], "pv")?;
    check_rate(r)?;
    let n = count(&args[1], "nper")? as f64;
    let pmt = number(&args[2], "pv")?;
    let fv = optional(args, 3).map_or(Ok(0.0), |v| number(v, "pv"))?;
    let t = optional(args, 4).map_or(Ok(0.0), timing)?;
    result(-(pmt * (1.0 + r * t) * growth_sum(r, n) + fv) / (1.0 + r).powf(n))
}

/// `npv(rate, v)`: the net present value of cash flows `v_0, v_1, ...`. The help's convention: the
/// first flow is one period after the investment, so `v_i` is discounted by `(1+r)^(i+1)`. Horner's
/// rule over the flows in reverse.
fn npv(args: &[Value]) -> Result<Value, EvalError> {
    let r = number(&args[0], "npv")?;
    check_rate(r)?;
    let flows = vector(&args[1], "npv")?;
    let mut value = 0.0;
    for v in flows.iter().rev() {
        value = (value + v) / (1.0 + r);
    }
    result(value)
}

/// `pmt(rate, nper, pv, [fv], [type])`: the periodic payment of a loan or investment.
fn pmt(args: &[Value]) -> Result<Value, EvalError> {
    let r = number(&args[0], "pmt")?;
    check_rate(r)?;
    let n = count(&args[1], "nper")? as f64;
    let pv = number(&args[2], "pmt")?;
    let fv = optional(args, 3).map_or(Ok(0.0), |v| number(v, "pmt"))?;
    let t = optional(args, 4).map_or(Ok(0.0), timing)?;
    result(payment(r, n, pv, fv, t))
}

/// The interest part of the payment of period `per` (the help's `ipmt`), given the payment `p`.
fn interest_part(r: f64, per: i64, p: f64, pv: f64, t: f64) -> f64 {
    if t == 1.0 && per == 1 {
        return 0.0;
    }
    // Balance after `per - 1 - t` periods, then the period's interest.
    let k = (per - 1) as f64 - t;
    r * future_balance(r, k, p, pv, t)
}

/// `ipmt(rate, per, nper, pv, [fv], [type])`: the interest paid in period `per`.
fn ipmt(args: &[Value]) -> Result<Value, EvalError> {
    let (r, per, n, pv, fv, t) = loan_args(args, "ipmt")?;
    let p = payment(r, n, pv, fv, t);
    result(interest_part(r, per, p, pv, t))
}

/// `ppmt(rate, per, nper, pv, [fv], [type])`: the principal part of the payment of period `per`.
fn ppmt(args: &[Value]) -> Result<Value, EvalError> {
    let (r, per, n, pv, fv, t) = loan_args(args, "ppmt")?;
    let p = payment(r, n, pv, fv, t);
    result(p - interest_part(r, per, p, pv, t))
}

/// The shared arguments of `ipmt` and `ppmt`: `(rate, per, nper, pv, fv, type)` with `per <= nper`.
fn loan_args(args: &[Value], name: &str) -> Result<(f64, i64, f64, f64, f64, f64), EvalError> {
    let r = number(&args[0], name)?;
    check_rate(r)?;
    let per = count(&args[1], "per")?;
    let n = count(&args[2], "nper")?;
    if per > n {
        return Err(EvalError::Unsupported(format!("{name} needs per <= nper")));
    }
    let pv = number(&args[3], name)?;
    let fv = optional(args, 4).map_or(Ok(0.0), |v| number(v, name))?;
    let t = optional(args, 5).map_or(Ok(0.0), timing)?;
    Ok((r, per, n as f64, pv, fv, t))
}

/// `nper(rate, pmt, pv, [fv], [type])`: the number of periods of a loan or investment. Solves the
/// payment equation for `n`: `(1+r)^n (pv + A) = A - fv` with `A = pmt (1+rt) / r`.
fn nper(args: &[Value]) -> Result<Value, EvalError> {
    let r = number(&args[0], "nper")?;
    check_rate(r)?;
    let pmt = number(&args[1], "nper")?;
    let pv = number(&args[2], "nper")?;
    let fv = optional(args, 3).map_or(Ok(0.0), |v| number(v, "nper"))?;
    let t = optional(args, 4).map_or(Ok(0.0), timing)?;
    if r == 0.0 {
        if pmt == 0.0 {
            return Err(EvalError::DivisionByZero);
        }
        return result(-(pv + fv) / pmt);
    }
    let a = pmt * (1.0 + r * t) / r;
    let x = (a - fv) / (pv + a);
    if x.is_nan() || x <= 0.0 || pv + a == 0.0 {
        return Err(EvalError::Unsupported(
            "nper has no solution: the payments and balances do not reach the future value"
                .to_string(),
        ));
    }
    result(x.ln() / (1.0 + r).ln())
}

/// `cnper(rate, pv, fv)`: the number of compounding periods for `pv` to grow to `fv`.
fn cnper(args: &[Value]) -> Result<Value, EvalError> {
    let r = number(&args[0], "cnper")?;
    check_rate(r)?;
    let pv = number(&args[1], "cnper")?;
    let fv = number(&args[2], "cnper")?;
    if r == 0.0 || pv == 0.0 || fv / pv <= 0.0 {
        return Err(EvalError::Unsupported(
            "cnper needs a nonzero rate and values of one sign with fv/pv > 0".to_string(),
        ));
    }
    result((fv / pv).ln() / (1.0 + r).ln())
}

/// Newton's method for a root of `f` (with derivative `df`) from `start`, at most
/// [`SOLVER_ITERATIONS`] steps. A step that would leave `(-1, inf)` is halved toward -1.
fn newton(name: &str, start: f64, f: impl Fn(f64) -> (f64, f64)) -> Result<f64, EvalError> {
    let mut x = start;
    for _ in 0..SOLVER_ITERATIONS {
        let (value, slope) = f(x);
        if !value.is_finite() || !slope.is_finite() || slope == 0.0 {
            return Err(EvalError::NoConvergence(name.to_string()));
        }
        let step = value / slope;
        if !step.is_finite() {
            return Err(EvalError::NoConvergence(name.to_string()));
        }
        let next = x - step;
        let next = if next > -1.0 { next } else { (x - 1.0) / 2.0 };
        if (next - x).abs() < SOLVER_TOL {
            return Ok(next);
        }
        x = next;
    }
    Err(EvalError::NoConvergence(name.to_string()))
}

/// `rate(nper, pmt, pv, [fv], [type], [guess])`: the rate per period of a loan or investment.
fn rate(args: &[Value]) -> Result<Value, EvalError> {
    let n = count(&args[0], "rate")? as f64;
    let pmt = number(&args[1], "rate")?;
    let pv = number(&args[2], "rate")?;
    let fv = optional(args, 3).map_or(Ok(0.0), |v| number(v, "rate"))?;
    let t = optional(args, 4).map_or(Ok(0.0), timing)?;
    let guess = optional(args, 5).map_or(Ok(0.1), |v| number(v, "rate"))?;
    check_rate(guess)?;
    // f(r) = pv (1+r)^n + pmt (1+rt) G(r, n) + fv, with a central difference for f'.
    let f = |r: f64| {
        let value = pv * (1.0 + r).powf(n) + pmt * (1.0 + r * t) * growth_sum(r, n) + fv;
        let h = 1e-7 * r.abs().max(1.0);
        let slope_at =
            |x: f64| pv * (1.0 + x).powf(n) + pmt * (1.0 + x * t) * growth_sum(x, n) + fv;
        let slope = (slope_at(r + h) - slope_at(r - h)) / (2.0 * h);
        (value, slope)
    };
    result(newton("rate", guess, f)?)
}

/// `crate(nper, pv, fv)`: the rate per period that grows `pv` to `fv` in `nper` periods.
fn crate_fn(args: &[Value]) -> Result<Value, EvalError> {
    let n = count(&args[0], "crate")? as f64;
    let pv = number(&args[1], "crate")?;
    let fv = number(&args[2], "crate")?;
    if pv == 0.0 || fv / pv <= 0.0 {
        return Err(EvalError::Unsupported(
            "crate needs values of one sign with fv/pv > 0".to_string(),
        ));
    }
    result((fv / pv).powf(1.0 / n) - 1.0)
}

/// `nom(apr, npery)`: the nominal annual rate whose effective APR is `apr`.
fn nom(args: &[Value]) -> Result<Value, EvalError> {
    let apr = number(&args[0], "nom")?;
    let m = count(&args[1], "npery")? as f64;
    check_rate(apr)?;
    result(m * ((1.0 + apr).powf(1.0 / m) - 1.0))
}

/// `eff(nrate, npery)`: the effective annual rate of a nominal rate compounded `npery` times a year.
fn eff(args: &[Value]) -> Result<Value, EvalError> {
    let nrate = number(&args[0], "eff")?;
    let m = count(&args[1], "npery")? as f64;
    check_rate(nrate / m)?;
    result((1.0 + nrate / m).powf(m) - 1.0)
}

/// Shared arguments of `cumint` and `cumprn`: `(rate, pv, start, end, type, payment)`, the payment
/// being the level payment of the loan (no `fv`).
fn cumulative_args(
    args: &[Value],
    name: &str,
) -> Result<(f64, f64, i64, i64, f64, f64), EvalError> {
    let r = number(&args[0], name)?;
    check_rate(r)?;
    let n = count(&args[1], "nper")?;
    let pv = number(&args[2], name)?;
    let start = count(&args[3], "start")?;
    let end = count(&args[4], "end")?;
    if start > end || end > n {
        return Err(EvalError::Unsupported(format!(
            "{name} needs start <= end <= nper"
        )));
    }
    let t = optional(args, 5).map_or(Ok(0.0), timing)?;
    let p = payment(r, n as f64, pv, 0.0, t);
    Ok((r, pv, start, end, t, p))
}

/// `cumint(rate, nper, pv, start, end, [type])`: the interest paid in periods `start` to `end`
/// (negative, as the payments are).
fn cumint(args: &[Value]) -> Result<Value, EvalError> {
    let (r, pv, start, end, t, p) = cumulative_args(args, "cumint")?;
    let mut total = 0.0;
    for per in start..=end {
        total += interest_part(r, per, p, pv, t);
    }
    result(total)
}

/// `cumprn(rate, nper, pv, start, end, [type])`: the principal paid in periods `start` to `end`.
fn cumprn(args: &[Value]) -> Result<Value, EvalError> {
    let (r, pv, start, end, t, p) = cumulative_args(args, "cumprn")?;
    let mut total = 0.0;
    for per in start..=end {
        total += p - interest_part(r, per, p, pv, t);
    }
    result(total)
}

/// `irr(v, [guess])`: the rate per period at which the flows `v_0, v_1, ...` (at `t = 0, 1, ...`)
/// have zero net present value. The flows need one positive and one negative value.
fn irr(args: &[Value]) -> Result<Value, EvalError> {
    let flows = vector(&args[0], "irr")?;
    check_sign_change(&flows, "irr")?;
    let guess = optional(args, 1).map_or(Ok(0.1), |v| number(v, "irr"))?;
    check_rate(guess)?;
    let f = |r: f64| {
        let d = 1.0 / (1.0 + r);
        let mut value = 0.0;
        let mut slope = 0.0;
        let mut factor = d;
        for (i, v) in flows.iter().enumerate() {
            if i > 0 {
                value += v * factor;
                slope -= (i as f64) * v * factor * d;
                factor *= d;
            } else {
                value += v;
            }
        }
        (value, slope)
    };
    result(newton("irr", guess, f)?)
}

/// `mirr(v, fin_rate, rein_rate)`: the modified IRR. Negative flows are discounted to `t = 0` at the
/// finance rate, positive flows compounded to the last period at the reinvestment rate, and the
/// result is the per-period rate over the `n - 1` periods between the first and last flow.
fn mirr(args: &[Value]) -> Result<Value, EvalError> {
    let flows = vector(&args[0], "mirr")?;
    check_sign_change(&flows, "mirr")?;
    if flows.len() < 2 {
        return Err(EvalError::Unsupported(
            "mirr needs at least two cash flows".to_string(),
        ));
    }
    let fin = number(&args[1], "mirr")?;
    let rein = number(&args[2], "mirr")?;
    check_rate(fin)?;
    check_rate(rein)?;
    let last = flows.len() - 1;
    let mut pv_negative = 0.0;
    let mut fv_positive = 0.0;
    for (i, v) in flows.iter().enumerate() {
        if *v < 0.0 {
            pv_negative += v / (1.0 + fin).powf(i as f64);
        } else if *v > 0.0 {
            fv_positive += v * (1.0 + rein).powf((last - i) as f64);
        }
    }
    let ratio = fv_positive / -pv_negative;
    result(ratio.powf(1.0 / last as f64) - 1.0)
}

/// `irr` and `mirr` need one positive and one negative flow.
fn check_sign_change(flows: &[f64], name: &str) -> Result<(), EvalError> {
    let positive = flows.iter().any(|v| *v > 0.0);
    let negative = flows.iter().any(|v| *v < 0.0);
    if positive && negative {
        Ok(())
    } else {
        Err(EvalError::Unsupported(format!(
            "{name} needs at least one positive and one negative cash flow"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::builtins::Registry;
    use num_complex::Complex64;

    fn s(x: f64) -> Value {
        Value::Scalar(Scalar::real(x, Dimension::dimensionless()))
    }

    fn vec_of(xs: &[f64]) -> Value {
        use crate::eval::value::Matrix;
        let data = xs
            .iter()
            .map(|x| Scalar::real(*x, Dimension::dimensionless()))
            .collect();
        Value::Matrix(Matrix::new(1, xs.len(), data).unwrap())
    }

    fn call(name: &str, args: &[Value]) -> Result<f64, EvalError> {
        let mut r = Registry::new();
        register(&mut r);
        let b = r.get(name).expect("registered");
        b.arity.check(name, args.len())?;
        let crate::eval::builtins::Call::Plain(f) = b.call else {
            panic!("finance builtins are plain");
        };
        match f(args)? {
            Value::Scalar(sc) => {
                assert_eq!(sc.value.im, 0.0);
                Ok(sc.value.re)
            }
            other => panic!("expected a scalar, got {other:?}"),
        }
    }

    fn close(got: f64, want: f64, tol: f64) {
        assert!((got - want).abs() <= tol, "got {got}, want {want}");
    }

    // The help example: 10 000 at 6% a year, monthly, 60 periods.
    const PRATE: f64 = 0.005;

    #[test]
    fn payment_matches_help_example() {
        let p = call("pmt", &[s(PRATE), s(60.0), s(10000.0)]).unwrap();
        close(p, -193.328, 0.001);
        close(
            call("fv", &[s(PRATE), s(60.0), s(p), s(10000.0)]).unwrap(),
            0.0,
            1e-6,
        );
    }

    #[test]
    fn principal_and_interest_add_up_to_the_payment() {
        let p = call("pmt", &[s(PRATE), s(60.0), s(10000.0)]).unwrap();
        let principal = call("ppmt", &[s(PRATE), s(36.0), s(60.0), s(10000.0)]).unwrap();
        let interest = call("ipmt", &[s(PRATE), s(36.0), s(60.0), s(10000.0)]).unwrap();
        close(principal + interest, p, 1e-9);
        assert!(interest < 0.0 && principal < 0.0);
    }

    #[test]
    fn cumulative_interest_and_principal_match_help_example() {
        close(
            call("cumint", &[s(PRATE), s(60.0), s(10000.0), s(1.0), s(60.0)]).unwrap(),
            -1599.68,
            0.01,
        );
        close(
            call("cumprn", &[s(PRATE), s(60.0), s(10000.0), s(1.0), s(60.0)]).unwrap(),
            -10000.0,
            0.01,
        );
    }

    #[test]
    fn effective_and_nominal_rates() {
        let apr = call("eff", &[s(0.06), s(12.0)]).unwrap();
        close(apr, 0.0616778, 1e-6);
        close(call("nom", &[s(apr), s(12.0)]).unwrap(), 0.06, 1e-9);
    }

    #[test]
    fn rates_from_help_example() {
        close(
            call("crate", &[s(60.0), s(10000.0), s(25000.0)]).unwrap(),
            0.015,
            0.0005,
        );
        let p = call("pmt", &[s(PRATE), s(60.0), s(10000.0)]).unwrap();
        close(
            call("rate", &[s(60.0), s(p), s(10000.0), s(0.0)]).unwrap(),
            PRATE,
            1e-9,
        );
        close(
            call("rate", &[s(60.0), s(p), s(10000.0)]).unwrap(),
            PRATE,
            1e-9,
        );
    }

    #[test]
    fn number_of_periods_from_help_example() {
        let p = call("pmt", &[s(PRATE), s(60.0), s(10000.0)]).unwrap();
        let n = call("nper", &[s(PRATE), s(p), s(10000.0), s(25000.0)]).unwrap();
        close(n, 159.99, 0.02);
        // cnper is the period count at which pv (1+r)^n reaches fv.
        let n = call("cnper", &[s(PRATE), s(10000.0), s(25000.0)]).unwrap();
        close(10000.0 * (1.0 + PRATE).powf(n), 25000.0, 1e-6);
    }

    #[test]
    fn present_value_and_npv_conventions() {
        let pv = call("pv", &[s(PRATE), s(60.0), s(-193.328)]).unwrap();
        close(pv, 10000.0, 0.01);
        // npv discounts the first flow by one period.
        close(
            call("npv", &[s(0.1), vec_of(&[100.0])]).unwrap(),
            100.0 / 1.1,
            1e-12,
        );
        close(
            call("npv", &[s(0.1), vec_of(&[100.0, 100.0])]).unwrap(),
            100.0 / 1.1 + 100.0 / 1.21,
            1e-12,
        );
    }

    #[test]
    fn future_value_of_a_schedule_and_of_flows() {
        close(
            call(
                "fvadj",
                &[s(12000.0), vec_of(&[0.375, 5.0 / 12.0, 5.5 / 12.0])],
            )
            .unwrap(),
            34088.54,
            0.01,
        );
        let cash = [100.0, 50.0, 0.0, 200.0, 0.0, 300.0];
        close(
            call("fvc", &[s(PRATE), vec_of(&cash)]).unwrap(),
            655.54,
            0.01,
        );
    }

    #[test]
    fn irr_and_mirr() {
        close(call("irr", &[vec_of(&[-100.0, 110.0])]).unwrap(), 0.1, 1e-9);
        let flows = [-1000.0, 300.0, 400.0, 500.0];
        let r = call("irr", &[vec_of(&flows)]).unwrap();
        close(
            call("npv", &[s(r), vec_of(&flows[1..])]).unwrap() + flows[0],
            0.0,
            1e-6,
        );
        close(
            call("mirr", &[vec_of(&[-100.0, 110.0]), s(0.1), s(0.1)]).unwrap(),
            0.1,
            1e-9,
        );
    }

    #[test]
    fn domain_errors() {
        // irr needs a sign change.
        assert!(call("irr", &[vec_of(&[1.0, 2.0])]).is_err());
        // Type must be 0 or 1.
        assert!(call("pmt", &[s(PRATE), s(60.0), s(10000.0), s(0.0), s(2.0)]).is_err());
        // Fractional and out-of-range period counts.
        assert!(call("pmt", &[s(PRATE), s(60.5), s(10000.0)]).is_err());
        assert!(call("fv", &[s(PRATE), s(2e6), s(1.0)]).is_err());
        // per above nper, and cumint with start > end.
        assert!(call("ipmt", &[s(PRATE), s(61.0), s(60.0), s(10000.0)]).is_err());
        assert!(call("cumint", &[s(PRATE), s(60.0), s(10000.0), s(5.0), s(4.0)]).is_err());
        // A rate at or below -1.
        assert!(call("pv", &[s(-1.0), s(3.0), s(1.0)]).is_err());
        // Wrong arity.
        assert!(call("fv", &[s(PRATE)]).is_err());
        // Dimensions are refused.
        let metre = Value::Scalar(Scalar::real(1.0, Dimension::base(0)));
        assert!(call("pv", &[s(PRATE), s(3.0), metre]).is_err());
        // A value of imaginary part is refused.
        let complex = Value::Scalar(Scalar::new(
            Complex64::new(1.0, 1.0),
            Dimension::dimensionless(),
        ));
        assert!(call("pv", &[s(PRATE), s(3.0), complex]).is_err());
    }

    #[test]
    fn rate_that_cannot_converge_reports_no_convergence() {
        // No real rate solves this: every payment is negative and the balance never reaches zero.
        let err = call("rate", &[s(10.0), s(-1.0), s(0.0), s(1.0e9)]).unwrap_err();
        assert!(matches!(
            err,
            EvalError::NoConvergence(_) | EvalError::NotFinite | EvalError::Unsupported(_)
        ));
    }
}

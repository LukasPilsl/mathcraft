//! Time functions: `time(z)` (the current system time) and `hhmmss(shms)` (a time from a string).
//!
//! The help (`about_misc_functions`, `timing_execution`, `unit_scaling_in_common_formats`) documents
//! only these two in the time family. Prime 12's help has no calendar or date-arithmetic functions
//! (no `today`, `now`, `weekday`, `day`, `month`, `year`), so none are implemented.
//!
//! The clock is read through [`now_seconds`]. Its source is a per-thread override set by
//! [`with_clock`] (tests use it) and the system clock otherwise, so a test can fix the time.

use std::cell::RefCell;
use std::rc::Rc;

use super::{Arity, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Scalar, Value};
use crate::units::Dimension;

/// The `time` base dimension: `s` is index 2 in `units::BASE_SYMBOLS`.
const TIME_BASE: usize = 2;

thread_local! {
    static CLOCK: RefCell<Option<Rc<dyn Fn() -> f64>>> = const { RefCell::new(None) };
}

/// Restores the previous clock when dropped, so a panic in the body does not leave a fake clock.
struct ClockGuard(Option<Rc<dyn Fn() -> f64>>);

impl Drop for ClockGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        CLOCK.with(|c| *c.borrow_mut() = previous);
    }
}

/// Runs `body` with `clock` as the source of [`now_seconds`] on this thread, then restores it.
pub fn with_clock<T>(clock: impl Fn() -> f64 + 'static, body: impl FnOnce() -> T) -> T {
    let previous = CLOCK.with(|c| c.borrow_mut().replace(Rc::new(clock)));
    let _guard = ClockGuard(previous);
    body()
}

/// The current time in seconds since the Unix epoch: the override when one is set, else the system
/// clock. A clock before the epoch reads as 0.
pub fn now_seconds() -> f64 {
    let fixed = CLOCK.with(|c| c.borrow().clone());
    match fixed {
        Some(clock) => clock(),
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64()),
    }
}

/// Registers the time functions.
pub fn register(r: &mut Registry) {
    r.register("time", Arity::Exact(1), time);
    r.register("hhmmss", Arity::Exact(1), hhmmss);
}

fn seconds(value: f64) -> Result<Value, EvalError> {
    if !value.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(Value::Scalar(Scalar::real(
        value,
        Dimension::base(TIME_BASE),
    )))
}

/// `time(z)`: the current system time in seconds. The value of `z` is not used (the help: "its value
/// has no impact on the calculation"). The result is relative to an arbitrary origin, so differences
/// measure elapsed time.
fn time(_args: &[Value]) -> Result<Value, EvalError> {
    seconds(now_seconds())
}

/// A field of `hhmmss`: a nonnegative real number, or a whole number for hours and minutes.
fn field(text: &str, whole: bool) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() || t.starts_with(['+', '-']) {
        return None;
    }
    if whole && !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let x: f64 = t.parse().ok()?;
    (x.is_finite() && x >= 0.0).then_some(x)
}

/// `hhmmss(shms)`: the time in seconds of a string `"hh:mm:ss.sss"` (all three fields given, hours
/// and minutes whole) or of a comma-separated list of one to three nonnegative numbers. The list is
/// read from the right: one number is seconds, two are minutes and seconds, three are hours, minutes
/// and seconds. (The help's examples use three numbers; the shorter lists are this rule's reading.)
fn hhmmss(args: &[Value]) -> Result<Value, EvalError> {
    let Value::Str(text) = &args[0] else {
        return Err(EvalError::Unsupported("hhmmss takes a string".to_string()));
    };
    let bad = || EvalError::Unsupported(format!("hhmmss: {text:?} is not a time"));
    let parts: Vec<&str> = if text.contains(':') {
        text.split(':').collect()
    } else {
        text.split(',').collect()
    };
    let colon = text.contains(':');
    let (h, m, s) = match (colon, parts.as_slice()) {
        (true, [h, m, s]) => (
            field(h, true).ok_or_else(bad)?,
            field(m, true).ok_or_else(bad)?,
            field(s, false).ok_or_else(bad)?,
        ),
        (true, _) => return Err(bad()),
        (false, [s]) => (0.0, 0.0, field(s, false).ok_or_else(bad)?),
        (false, [m, s]) => (
            0.0,
            field(m, false).ok_or_else(bad)?,
            field(s, false).ok_or_else(bad)?,
        ),
        (false, [h, m, s]) => (
            field(h, false).ok_or_else(bad)?,
            field(m, false).ok_or_else(bad)?,
            field(s, false).ok_or_else(bad)?,
        ),
        _ => return Err(bad()),
    };
    seconds(h * 3600.0 + m * 60.0 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(v: Value) -> f64 {
        match v {
            Value::Scalar(s) => s.value.re,
            other => panic!("expected a scalar, got {other:?}"),
        }
    }

    fn st(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    #[test]
    fn time_reads_the_clock_it_is_given() {
        let got = with_clock(|| 1234.5, || secs(time(&[Value::Str("x".into())]).unwrap()));
        assert_eq!(got, 1234.5);
        // Restored afterwards: the system clock is past 2020.
        assert!(now_seconds() > 1.5e9);
    }

    #[test]
    fn time_is_in_seconds() {
        let v = with_clock(|| 5.0, || time(&[Value::Str("x".into())]).unwrap());
        match v {
            Value::Scalar(s) => assert_eq!(s.dim, Dimension::base(TIME_BASE)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hhmmss_matches_help_examples() {
        assert_eq!(secs(hhmmss(&[st("1:20:36.72")]).unwrap()), 4836.72);
        assert_eq!(secs(hhmmss(&[st("1:0:0")]).unwrap()), 3600.0);
        assert_eq!(secs(hhmmss(&[st("0:20:0")]).unwrap()), 1200.0);
        assert_eq!(secs(hhmmss(&[st("0:0:36.72")]).unwrap()), 36.72);
        assert_eq!(secs(hhmmss(&[st("1, 20, 36.72")]).unwrap()), 4836.72);
        assert_eq!(secs(hhmmss(&[st("36.72")]).unwrap()), 36.72);
    }

    #[test]
    fn hhmmss_refuses_malformed_input() {
        assert!(hhmmss(&[st("1:20")]).is_err());
        assert!(hhmmss(&[st("1.5:20:0")]).is_err());
        assert!(hhmmss(&[st("-1:20:0")]).is_err());
        assert!(hhmmss(&[st("a")]).is_err());
        assert!(hhmmss(&[st("1,2,3,4")]).is_err());
        assert!(hhmmss(&[Value::Scalar(Scalar::real(1.0, Dimension::dimensionless()))]).is_err());
    }
}

//! Comparison of computed values with stored results, and the classification of each region.
//!
//! Tolerance: a value passes when `|actual - expected| <= ABS_TOL + REL_TOL * max(|actual|,
//! |expected|)`, per real and imaginary part. NaN never matches. Infinities match only when equal.
//!
//! Units of a stored result (verified from Prime's `result.xml` samples):
//! - A result shown with a unit override (`->mm=`, `->°=`, `->(N*m)=`) is stored as the number
//!   AS SHOWN in the override unit, with NO unit references. Example: Mechanik_2 #18 `f_A->mm=`
//!   stores 0.13150772536329733, while the SI value is 1.315e-4 m. Example: 16_rata #18
//!   `α->°` stores 36.87, while the SI value is 0.6435 rad. 16 such regions in the samples.
//! - Without an override, stored values are SI base units, with unit references for the
//!   dimension.
//! - UNTESTED: an override on a result that is stored WITH unit references, and offset units
//!   such as °C. [`classify`] reports such a region as `Unsupported("override with united
//!   stored value")` and does not guess.
//!
//! GUI use: `classify(&row)` for one [`RegionEvaluation`] gives the category to colour it by.

use mathcraft_core::eval::{EvalError, Matrix, Outcome, Scalar, Value};
use mathcraft_core::units::{self, Dimension, Exp};

use crate::describe::{describe_stored, describe_value, fmt_complex};
use crate::evaluate::{EvalOutcome, RegionEvaluation};
use crate::evaluate_plots::{PlotData, TraceData};
use crate::evaluate_plots3d::{Plot3dData, Trace3dData};
use crate::results::{ResultData, ResultValue, Trace2d, Trace3dStored, UnitRef};

/// Relative tolerance of the comparison.
pub const REL_TOL: f64 = 1e-12;
/// Absolute tolerance of the comparison.
pub const ABS_TOL: f64 = 1e-12;
/// Absolute and relative tolerance of plot series (`compare_trace`).
pub const PLOT_TOL: f64 = 1e-9;

/// The classification of one region.
#[derive(Debug, Clone, PartialEq)]
pub enum Category {
    /// The computed value matches the stored one (value, dimension, shape, or error-ness).
    Pass,
    /// Passes as [`Category::Pass`] does, but the stored result is an error with a code other
    /// than `bad%_variable`, so any engine error was accepted. Counted in `weak_pass` too.
    WeakPass(String),
    /// The engine produced a result that differs from the stored one. The text says how.
    Mismatch(String),
    /// The region uses something the evaluator does not handle. The text is the reason.
    Unsupported(String),
    /// The engine returned an error (or panicked) where a value was stored. The text is the
    /// error, or `Panic: ...`.
    EngineError(String),
    /// An engine error (or panic) on a region that has NO stored reference (nothing stored, or an
    /// empty entry). Kept apart from [`Category::NoReference`] so that engine failures are not
    /// hidden among regions that merely have no reference. Counts as a failure for `--strict`.
    EngineErrorNoRef(String),
    /// Nothing is stored to compare with. The region was still executed.
    NoReference,
    /// A 2D plot whose traces evaluated, but not all of them have stored data to compare with,
    /// so the plot is not verified. The text summarises the points.
    PlotOk(String),
}

/// A stored result as the comparison needs it: SI values with their dimensions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expected {
    Scalar {
        re: f64,
        im: f64,
        dim: Dimension,
    },
    Matrix {
        rows: usize,
        cols: usize,
        elements: Vec<Expected>,
    },
}

/// Turns a stored numeric result into an [`Expected`]. The unit references give the dimension
/// through the units table. Returns the reason as `Err` when the stored result cannot be used.
pub fn expected_from_stored(value: &ResultValue) -> Result<Expected, String> {
    match value {
        ResultValue::Real(x) => Ok(Expected::Scalar {
            re: *x,
            im: 0.0,
            dim: Dimension::dimensionless(),
        }),
        ResultValue::United { value, units } => Ok(Expected::Scalar {
            re: *value,
            im: 0.0,
            dim: dimension_of(units)?,
        }),
        ResultValue::Matrix {
            rows,
            cols,
            elements,
        } => {
            let elements = elements
                .iter()
                .map(|e| match e {
                    ResultValue::Real(_) | ResultValue::United { .. } => expected_from_stored(e),
                    _ => Err("stored matrix with a non-scalar element".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Expected::Matrix {
                rows: *rows,
                cols: *cols,
                elements,
            })
        }
        other => Err(format!("stored {} result", describe_stored(other))),
    }
}

/// The dimension of a stored unit monomial, from the units table.
pub fn dimension_of(units_ref: &[UnitRef]) -> Result<Dimension, String> {
    let mut dim = Dimension::dimensionless();
    for u in units_ref {
        let def = units::unit(&u.unit)
            .ok_or_else(|| format!("stored unit `{}` is not in the units table", u.unit))?;
        if u.den == 0 {
            return Err(format!(
                "stored unit `{}` has a zero power denominator",
                u.unit
            ));
        }
        let power = def
            .dimension
            .pow(Exp::new(u.num, u.den))
            .map_err(|e| format!("stored unit `{}`: {e}", u.unit))?;
        dim = dim
            .checked_mul(power)
            .map_err(|e| format!("stored units: {e}"))?;
    }
    Ok(dim)
}

/// Whether two real numbers agree within [`REL_TOL`] and [`ABS_TOL`]. NaN never agrees. An
/// infinity agrees only with the same infinity.
pub fn close(expected: f64, actual: f64) -> bool {
    if expected.is_nan() || actual.is_nan() {
        return false;
    }
    if expected == actual {
        return true;
    }
    if !expected.is_finite() || !actual.is_finite() {
        return false;
    }
    let scale = expected.abs().max(actual.abs());
    (actual - expected).abs() <= ABS_TOL + REL_TOL * scale
}

/// Compares one computed scalar with an expected complex number and dimension. Both parts must
/// agree. The dimension is checked first.
pub fn compare_scalar(re: f64, im: f64, dim: Dimension, actual: &Scalar) -> Result<(), String> {
    if actual.dim != dim {
        return Err(format!(
            "dimension mismatch: expected [{dim}], got [{}]",
            actual.dim
        ));
    }
    let (ar, ai) = (actual.value.re, actual.value.im);
    if close(re, ar) && close(im, ai) {
        Ok(())
    } else {
        Err(format!(
            "value mismatch: expected {}, got {}",
            fmt_complex(re, im),
            fmt_complex(ar, ai)
        ))
    }
}

/// Compares a computed value with an expected one. Shape must match for matrices, element by
/// element in row-major order.
pub fn compare_value(expected: &Expected, actual: &Value) -> Result<(), String> {
    match (expected, actual) {
        (Expected::Scalar { re, im, dim }, Value::Scalar(s)) => compare_scalar(*re, *im, *dim, s),
        (
            Expected::Matrix {
                rows,
                cols,
                elements,
            },
            Value::Matrix(m),
        ) => compare_matrix(*rows, *cols, elements, m),
        (Expected::Scalar { .. }, Value::Matrix(m)) => {
            let (r, c) = m.shape();
            Err(format!("expected a scalar, got a {r}x{c} matrix"))
        }
        (Expected::Matrix { rows, cols, .. }, Value::Scalar(_)) => {
            Err(format!("expected a {rows}x{cols} matrix, got a scalar"))
        }
        (_, Value::Str(s)) => Err(format!("expected a number, got text {s:?}")),
        (_, Value::List(_)) => Err("nested array: comparison not supported".to_string()),
        (_, Value::Func(_)) => Err("a function value: not compared with a number".to_string()),
        (_, Value::Symbolic(s)) => Err(format!(
            "symbolic result {}: not compared with a number",
            s.shown
        )),
    }
}

fn compare_matrix(
    rows: usize,
    cols: usize,
    elements: &[Expected],
    actual: &Matrix,
) -> Result<(), String> {
    let (ar, ac) = actual.shape();
    if (ar, ac) != (rows, cols) {
        return Err(format!(
            "shape mismatch: expected {rows}x{cols}, got {ar}x{ac}"
        ));
    }
    if elements.len() != rows * cols {
        return Err(format!(
            "stored matrix has {} elements for {rows}x{cols}",
            elements.len()
        ));
    }
    for (index, (e, a)) in elements.iter().zip(actual.data()).enumerate() {
        match e {
            Expected::Scalar { re, im, dim } => compare_scalar(*re, *im, *dim, a)
                .map_err(|m| format!("element {index} (row-major): {m}"))?,
            Expected::Matrix { .. } => return Err("nested matrix in stored result".to_string()),
        }
    }
    Ok(())
}

/// Whether two real numbers agree within `tol`, both absolutely and relatively: `|a - e| <= tol *
/// (1 + max(|e|, |a|))`. NaN never agrees; an infinity agrees only with the same infinity.
pub fn close_within(expected: f64, actual: f64, tol: f64) -> bool {
    if expected.is_nan() || actual.is_nan() {
        return false;
    }
    if expected == actual {
        return true;
    }
    if !expected.is_finite() || !actual.is_finite() {
        return false;
    }
    let scale = expected.abs().max(actual.abs());
    (actual - expected).abs() <= tol * (1.0 + scale)
}

/// Compares the computed series of one trace with Prime's stored `<Trace2dResult>`. See
/// [`compare_points`].
pub fn compare_trace(stored: &Trace2d, trace: &TraceData) -> Result<(), String> {
    compare_points(&stored.points()?, trace)
}

/// Compares the computed series of one trace with stored points (see [`Trace2d::points`]), point
/// by point in the order of sampling. Counts must be equal. Each point is `(x, y)` within
/// [`PLOT_TOL`] (absolute and relative), or a gap on both sides. The first difference is reported
/// with its index.
pub fn compare_points(expected: &[Option<(f64, f64)>], trace: &TraceData) -> Result<(), String> {
    let actual = &trace.samples;
    if expected.len() != actual.len() {
        return Err(format!(
            "point count: stored {}, computed {}",
            expected.len(),
            actual.len()
        ));
    }
    for (index, (e, a)) in expected.iter().zip(actual).enumerate() {
        match (e, a) {
            (None, None) => {}
            (Some((ex, ey)), Some((ax, ay))) => {
                if !close_within(*ex, *ax, PLOT_TOL) || !close_within(*ey, *ay, PLOT_TOL) {
                    return Err(format!(
                        "point {index}: expected ({ex}, {ey}), got ({ax}, {ay})"
                    ));
                }
            }
            (Some((ex, ey)), None) => {
                return Err(format!("point {index}: expected ({ex}, {ey}), got a gap"))
            }
            (None, Some((ax, ay))) => {
                return Err(format!("point {index}: expected a gap, got ({ax}, {ay})"))
            }
        }
    }
    Ok(())
}

/// Classifies one evaluated region against its stored result.
///
/// A skipped region is `Unsupported` with its reason. A panic is an engine error: `EngineError`
/// when a value is stored, `EngineErrorNoRef` otherwise. Any other outcome goes to the rules of
/// [`classify_outcome`].
pub fn classify(row: &RegionEvaluation) -> Category {
    match &row.outcome {
        EvalOutcome::Skipped(reason) => Category::Unsupported(reason.clone()),
        EvalOutcome::Panic(message) => {
            let text = format!("Panic: {message}");
            if has_reference(row.stored.as_ref()) {
                Category::EngineError(text)
            } else {
                Category::EngineErrorNoRef(text)
            }
        }
        EvalOutcome::Engine(outcome) => classify_outcome(row.stored.as_ref(), outcome),
        EvalOutcome::Plot(plot) => classify_plot(plot),
        EvalOutcome::Plot3d(plot) => classify_plot3d(plot),
    }
}

/// Classifies a plot region from its traces (see [`Category::Pass`], [`Category::Mismatch`] and
/// [`Category::PlotOk`]). Each trace with stored data is compared with [`compare_trace`]; a
/// trace without stored data is only evaluated.
///
/// Order of the rules:
/// 1. Any stored trace that differs: `Mismatch`, naming the first difference of each trace.
/// 2. Any trace error with stored data: `EngineError`. A trace error the evaluator does not
///    handle: `Unsupported`. Any other trace error (no stored data): `EngineErrorNoRef`.
/// 3. Every trace has stored data and matches: `Pass`.
/// 4. Otherwise (no stored data, or some traces without it): `PlotOk`.
fn classify_plot(plot: &PlotData) -> Category {
    let mut mismatches = Vec::new();
    let mut engine_error = None;
    let mut unsupported = None;
    let mut error_no_ref = None;
    let mut all_compared = !plot.traces.is_empty();
    for (index, trace) in plot.traces.iter().enumerate() {
        let number = index + 1;
        if let Some(error) = &trace.error {
            if error.unsupported {
                unsupported.get_or_insert_with(|| error.message.clone());
            } else if trace.stored.is_some() {
                engine_error
                    .get_or_insert_with(|| format!("plot trace {number}: {}", error.message));
            } else {
                error_no_ref
                    .get_or_insert_with(|| format!("plot trace {number}: {}", error.message));
            }
            all_compared = false;
            continue;
        }
        match &trace.stored {
            Some(Ok(points)) => {
                if let Err(reason) = compare_points(points, trace) {
                    mismatches.push(format!("plot trace {number}: {reason}"));
                }
            }
            Some(Err(reason)) => {
                unsupported.get_or_insert_with(|| format!("stored trace data: {reason}"));
                all_compared = false;
            }
            None => all_compared = false,
        }
    }
    if !mismatches.is_empty() {
        return Category::Mismatch(mismatches.join("; "));
    }
    if let Some(text) = engine_error {
        return Category::EngineError(text);
    }
    if let Some(reason) = unsupported {
        return Category::Unsupported(reason);
    }
    if let Some(text) = error_no_ref {
        return Category::EngineErrorNoRef(text);
    }
    if all_compared {
        return Category::Pass;
    }
    Category::PlotOk(format!(
        "{} traces, {} points",
        plot.traces.len(),
        plot.point_count()
    ))
}

/// Compares the computed grid of one 3D trace with its stored data (see
/// [`Trace3dStored`]): same grid shape and kind, then `X`, `Y` and `Z` value by value. Each value
/// is within [`PLOT_TOL`] (absolute and relative), or a gap (`NaN`) on both sides. The first
/// difference is reported with its index.
pub fn compare_trace3d(stored: &Trace3dStored, trace: &Trace3dData) -> Result<(), String> {
    if (stored.rows, stored.cols) != (trace.rows, trace.cols) {
        return Err(format!(
            "grid: stored {}x{}, computed {}x{}",
            stored.rows, stored.cols, trace.rows, trace.cols
        ));
    }
    if (stored.kind == 2.0) != trace.surface {
        return Err(format!("trace kind: stored {}", stored.kind));
    }
    // The first difference of each component, so that a grid offset does not hide the heights.
    let mut differences = Vec::new();
    for (name, expected, actual) in [
        ("X", &stored.x, &trace.x),
        ("Y", &stored.y, &trace.y),
        ("Z", &stored.z, &trace.z),
    ] {
        let first = expected.iter().zip(actual).enumerate().find(|(_, (e, a))| {
            match (e.is_nan(), a.is_nan()) {
                (true, true) => false,
                (false, false) => !close_within(**e, **a, PLOT_TOL),
                _ => true,
            }
        });
        if let Some((index, (e, a))) = first {
            differences.push(format!("{name}[{index}]: expected {e}, got {a}"));
        }
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences.join("; "))
    }
}

/// Classifies a 3D plot from its traces, by the rules of [`classify_plot`]: a trace with stored
/// data is compared with [`compare_trace3d`].
fn classify_plot3d(plot: &Plot3dData) -> Category {
    let mut mismatches = Vec::new();
    let mut engine_error = None;
    let mut unsupported = None;
    let mut error_no_ref = None;
    let mut all_compared = !plot.traces.is_empty();
    for (index, trace) in plot.traces.iter().enumerate() {
        let number = index + 1;
        if let Some(error) = &trace.error {
            if error.unsupported {
                unsupported.get_or_insert_with(|| error.message.clone());
            } else if trace.stored.is_some() {
                engine_error
                    .get_or_insert_with(|| format!("plot trace {number}: {}", error.message));
            } else {
                error_no_ref
                    .get_or_insert_with(|| format!("plot trace {number}: {}", error.message));
            }
            all_compared = false;
            continue;
        }
        match &trace.stored {
            Some(Ok(stored)) => {
                if let Err(reason) = compare_trace3d(stored, trace) {
                    mismatches.push(format!("plot trace {number}: {reason}"));
                }
            }
            Some(Err(reason)) => {
                unsupported.get_or_insert_with(|| format!("stored trace data: {reason}"));
                all_compared = false;
            }
            None => all_compared = false,
        }
    }
    if !mismatches.is_empty() {
        return Category::Mismatch(mismatches.join("; "));
    }
    if let Some(text) = engine_error {
        return Category::EngineError(text);
    }
    if let Some(reason) = unsupported {
        return Category::Unsupported(reason);
    }
    if let Some(text) = error_no_ref {
        return Category::EngineErrorNoRef(text);
    }
    if all_compared {
        return Category::Pass;
    }
    Category::PlotOk(format!(
        "{} traces, {} points",
        plot.traces.len(),
        plot.traces.iter().map(|t| t.rows * t.cols).sum::<usize>()
    ))
}

/// The stored error code of Prime's undefined-variable errors. Prime appends detail to it, so
/// codes are matched by prefix.
const UNDEFINED_CODE: &str = "bad%_variable";

/// Whether a stored result exists and is not an empty entry.
fn has_reference(stored: Option<&ResultData>) -> bool {
    stored.is_some_and(|d| !matches!(d.value, ResultValue::Empty))
}

/// Classifies one engine outcome against the stored result of the same region.
///
/// Order of the rules:
/// 1. An `Unsupported` error is always `Unsupported`.
/// 2. Nothing stored, or only an empty entry: an engine error gives `EngineErrorNoRef`, any other
///    outcome gives `NoReference`.
/// 3. A stored error with code `bad%_variable` (prefix) passes only on `Undefined` or
///    `UndefinedFunction`. Any other engine error is a `Mismatch`. A stored error with any other
///    code passes on ANY engine error, as `WeakPass`, which the summary counts separately. A
///    stored error with a computed value is a `Mismatch`.
/// 4. A stored value needs a value of the same dimension, shape and magnitude.
/// 5. A computed value with a unit override (`Converted`) is compared with the number as shown
///    when the stored value is plain (no unit references). When the stored value HAS unit
///    references, the override rule is untested, so the region is `Unsupported("override with
///    united stored value")` and is not guessed.
fn classify_outcome(stored: Option<&ResultData>, outcome: &Outcome) -> Category {
    if let Outcome::Error(EvalError::Unsupported(reason)) = outcome {
        return Category::Unsupported(reason.clone());
    }
    let no_reference = || match outcome {
        Outcome::Error(e) => Category::EngineErrorNoRef(e.to_string()),
        _ => Category::NoReference,
    };
    let Some(stored) = stored else {
        return no_reference();
    };
    match &stored.value {
        ResultValue::Empty => no_reference(),
        ResultValue::Error { code, .. } => match outcome {
            // Rule 3. Unsupported returned above, so an engine error is any other error.
            Outcome::Error(e) if code.starts_with(UNDEFINED_CODE) => match e {
                EvalError::Undefined(_) | EvalError::UndefinedFunction(_) => Category::Pass,
                other => Category::Mismatch(format!(
                    "expected error {code} (undefined name), got engine error {other}"
                )),
            },
            Outcome::Error(_) => Category::WeakPass(code.clone()),
            Outcome::Value(v) | Outcome::Converted { value: v, .. } => Category::Mismatch(format!(
                "expected error {code}, got a value {}",
                describe_value(v)
            )),
            Outcome::Defined { name, .. } => {
                Category::Mismatch(format!("expected error {code}, got a definition of {name}"))
            }
            Outcome::Accepted => {
                Category::Mismatch(format!("expected error {code}, got a constraint"))
            }
        },
        ResultValue::Trace2d(_) => Category::Unsupported("stored Trace2d (plot data)".to_string()),
        ResultValue::Symbolic(_) => Category::Unsupported("stored symbolic result".to_string()),
        ResultValue::Unknown(name) => {
            Category::Unsupported(format!("stored <{name}> result not understood"))
        }
        numeric => match expected_from_stored(numeric) {
            Err(reason) => Category::Unsupported(reason),
            Ok(expected) => match outcome {
                // Unsupported errors returned above; any other error is an engine error.
                Outcome::Error(e) => Category::EngineError(e.to_string()),
                // A definition whose right-hand side is an evaluation stored that evaluation's
                // display: classify the display, as for a plain evaluation.
                Outcome::Defined {
                    display: Some(shown),
                    ..
                } => classify_outcome(Some(stored), shown),
                Outcome::Defined {
                    name,
                    display: None,
                } => Category::Mismatch(format!("expected a value, got a definition of {name}")),
                Outcome::Accepted => {
                    Category::Mismatch("expected a value, got a constraint".to_string())
                }
                // The stored format of nested arrays is not verified (no sample has one), so a
                // computed nested array is not compared.
                Outcome::Value(Value::List(_)) => Category::Unsupported(
                    "nested array result: comparison not supported".to_string(),
                ),
                Outcome::Value(Value::Symbolic(s)) => Category::Unsupported(format!(
                    "symbolic result {}: not compared with a number",
                    s.shown
                )),
                Outcome::Value(v) => match compare_value(&expected, v) {
                    Ok(()) => Category::Pass,
                    Err(reason) => Category::Mismatch(reason),
                },
                Outcome::Converted { shown, .. } => {
                    // Prime stores a plain number (no unit references) for a result shown with
                    // a unit override: it is the number as shown, not the SI value. A united
                    // stored value with an override is untested, so it is refused (rule 5).
                    if !is_plain_number(&stored.value) {
                        return Category::Unsupported("override with united stored value".into());
                    }
                    match compare_value(&expected, shown) {
                        Ok(()) => Category::Pass,
                        Err(reason) => Category::Mismatch(reason),
                    }
                }
            },
        },
    }
}

/// Whether a stored numeric result has no unit references: a `<ml:real>`, or a matrix of them.
fn is_plain_number(value: &ResultValue) -> bool {
    match value {
        ResultValue::Real(_) => true,
        ResultValue::Matrix { elements, .. } => elements.iter().all(is_plain_number),
        _ => false,
    }
}

#[cfg(test)]
#[path = "compare/tests.rs"]
mod tests;

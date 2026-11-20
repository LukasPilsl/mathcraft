//! Text of stored and computed values, in full precision. Used by the comparison messages and by
//! the CLI report.

use mathcraft_core::eval::Value;

use crate::results::{ResultValue, UnitRef};

/// Renders a stored result in full precision, for the expected column.
pub fn describe_stored(value: &ResultValue) -> String {
    match value {
        ResultValue::Empty => "n/a".to_string(),
        ResultValue::Real(x) => format!("{x:?}"),
        ResultValue::United { value, units } => {
            format!("{value:?} SI [{}]", units_text(units))
        }
        ResultValue::Matrix {
            rows,
            cols,
            elements,
        } => {
            let shown: Vec<String> = elements.iter().take(12).map(describe_stored).collect();
            let more = if elements.len() > 12 { ", ..." } else { "" };
            format!("matrix {rows}x{cols} [{}{more}]", shown.join(", "))
        }
        ResultValue::Error { code, message } => format!("error {code}: {message}"),
        ResultValue::Trace2d(t) => format!(
            "Trace2d {} with {} points",
            t.trace_type,
            t.range_points.len()
        ),
        ResultValue::Symbolic(name) => format!("symbolic <{name}>"),
        ResultValue::Unknown(name) => format!("unknown <{name}>"),
    }
}

fn units_text(units_ref: &[UnitRef]) -> String {
    if units_ref.is_empty() {
        return "dimensionless".to_string();
    }
    units_ref
        .iter()
        .map(|u| {
            let mut s = u.unit.clone();
            if u.num != 1 || u.den != 1 {
                s.push_str(&format!("^{}", u.num));
                if u.den != 1 {
                    s.push_str(&format!("/{}", u.den));
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join("*")
}

/// Renders a computed value with its dimension, in full precision.
pub fn describe_value(value: &Value) -> String {
    match value {
        Value::Scalar(s) => {
            let dim = if s.dim.is_dimensionless() {
                String::new()
            } else {
                format!(" [{}]", s.dim)
            };
            format!("{}{dim}", fmt_complex(s.value.re, s.value.im))
        }
        Value::Matrix(m) => {
            let (r, c) = m.shape();
            let shown: Vec<String> = m
                .data()
                .iter()
                .take(12)
                .map(|s| fmt_complex(s.value.re, s.value.im))
                .collect();
            let more = if m.data().len() > 12 { ", ..." } else { "" };
            format!("matrix {r}x{c} [{}{more}]", shown.join(", "))
        }
        Value::Str(s) => format!("{s:?}"),
        Value::List(items) => {
            let shown: Vec<String> = items.iter().map(describe_value).collect();
            format!("list [{}]", shown.join(", "))
        }
        Value::Symbolic(s) => format!("symbolic {}", s.shown),
        Value::Func(_) => "function".to_string(),
    }
}

pub(crate) fn fmt_complex(re: f64, im: f64) -> String {
    if im == 0.0 && im.is_sign_positive() {
        format!("{re:?}")
    } else {
        format!("{re:?}{im:+?}i")
    }
}

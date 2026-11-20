//! String functions (help: `string_functions`): `concat`, `substr`, `search`, `strlen`, `num2str`,
//! `format`, `str2num`, `str2vec`, `vec2str` and `error`.
//!
//! Positions are counted in characters (Unicode scalar values), from 0. The help's option "ORIGIN in
//! Strings" is not modelled: positions always start at 0, and `search` returns -1 when the text is not
//! found. Every string is capped at [`MAX_STRING_BYTES`] (1 MiB): a longer argument or result is
//! refused, so no operation copies more than that. Indices are sliced at character boundaries, so
//! no operation can split a UTF-8 sequence or panic.
//!
//! `error("S")` returns [`EvalError::UserMessage`]. Inside a program it is an ordinary error, so a
//! `try` block catches it.

use super::{Arity, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;
use num_complex::Complex64;

/// Largest string (in bytes) a string function accepts or returns.
pub const MAX_STRING_BYTES: usize = 1 << 20;

/// Registers the string functions.
pub fn register(r: &mut Registry) {
    r.register("concat", Arity::AtLeast(1), concat);
    r.register("substr", Arity::Exact(3), substr);
    r.register("search", Arity::Exact(3), search);
    r.register("strlen", Arity::Exact(1), strlen);
    r.register("num2str", Arity::Exact(1), num2str);
    r.register("format", Arity::AtLeast(1), format);
    r.register("str2num", Arity::Exact(1), str2num);
    r.register("str2vec", Arity::Exact(1), str2vec);
    r.register("vec2str", Arity::Exact(1), vec2str);
    r.register("error", Arity::Exact(1), error);
}

/// A string argument, refused when longer than [`MAX_STRING_BYTES`].
fn text<'a>(v: &'a Value, name: &str) -> Result<&'a str, EvalError> {
    let Value::Str(s) = v else {
        return Err(EvalError::Unsupported(format!("{name} takes a string")));
    };
    check_size(s.len())?;
    Ok(s)
}

/// Refuses a string longer than the cap.
fn check_size(bytes: usize) -> Result<(), EvalError> {
    if bytes > MAX_STRING_BYTES {
        Err(EvalError::Unsupported(format!(
            "string longer than {MAX_STRING_BYTES} bytes"
        )))
    } else {
        Ok(())
    }
}

/// A nonnegative integer scalar (a position or a count).
fn position(v: &Value, name: &str) -> Result<usize, EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!("{name} takes an integer")));
    };
    ops::require_dimensionless(s.dim)?;
    let k = integer_of(s)
        .ok_or_else(|| EvalError::Unsupported(format!("{name} must be an integer")))?;
    usize::try_from(k).map_err(|_| EvalError::Unsupported(format!("{name} must not be negative")))
}

/// A dimensionless real scalar, for the number conversions.
fn dimensionless(x: Complex64) -> Value {
    Value::Scalar(Scalar::new(x, Dimension::dimensionless()))
}

fn str_value(s: String) -> Result<Value, EvalError> {
    check_size(s.len())?;
    Ok(Value::Str(s))
}

/// `concat(S1, S2, ...)`: the strings joined in order.
fn concat(args: &[Value]) -> Result<Value, EvalError> {
    let mut out = String::new();
    for a in args {
        out.push_str(text(a, "concat")?);
        check_size(out.len())?;
    }
    str_value(out)
}

/// `substr(S, m, n)`: the `n` characters of `S` starting at character `m`. Refused when they do not
/// all lie in `S`.
fn substr(args: &[Value]) -> Result<Value, EvalError> {
    let s = text(&args[0], "substr")?;
    let m = position(&args[1], "substr")?;
    let n = position(&args[2], "substr")?;
    let len = s.chars().count();
    let end = m.checked_add(n).filter(|end| *end <= len).ok_or_else(|| {
        EvalError::Unsupported("substr: m + n is past the end of the string".to_string())
    })?;
    let start_byte = byte_offset(s, m);
    let end_byte = byte_offset(s, end);
    str_value(s[start_byte..end_byte].to_string())
}

/// The byte index of character `k` of `s`, or the length when `k` is at or past the end.
fn byte_offset(s: &str, k: usize) -> usize {
    s.char_indices().nth(k).map_or(s.len(), |(i, _)| i)
}

/// `search(S, SubS, m)`: the character position of the first `SubS` in `S` at or after position `m`,
/// or -1 when there is none.
fn search(args: &[Value]) -> Result<Value, EvalError> {
    let s = text(&args[0], "search")?;
    let sub = text(&args[1], "search")?;
    let m = position(&args[2], "search")?;
    let len = s.chars().count();
    if m > len {
        return Ok(dimensionless(Complex64::new(-1.0, 0.0)));
    }
    let from = byte_offset(s, m);
    let found = match s[from..].find(sub) {
        Some(rel) => {
            let at = from + rel;
            (m + s[from..at].chars().count()) as f64
        }
        None => -1.0,
    };
    Ok(dimensionless(Complex64::new(found, 0.0)))
}

/// `strlen(S)`: the number of characters in `S`.
fn strlen(args: &[Value]) -> Result<Value, EvalError> {
    let s = text(&args[0], "strlen")?;
    Ok(dimensionless(Complex64::new(s.chars().count() as f64, 0.0)))
}

/// A scalar as text: a real number in its shortest exact form (`2`, `0.5`), a complex number as
/// `a+bi`. Only dimensionless scalars are converted.
fn number_text(v: &Value, name: &str) -> Result<String, EvalError> {
    let Value::Scalar(s) = v else {
        return Err(EvalError::Unsupported(format!("{name} takes a scalar")));
    };
    ops::require_dimensionless(s.dim)?;
    let z = s.value;
    if !z.re.is_finite() || !z.im.is_finite() {
        return Err(EvalError::NotFinite);
    }
    Ok(if z.im == 0.0 {
        format!("{}", z.re)
    } else if z.im < 0.0 {
        format!("{}{}i", z.re, z.im)
    } else {
        format!("{}+{}i", z.re, z.im)
    })
}

/// `num2str(z)`: the scalar `z` as a string.
fn num2str(args: &[Value]) -> Result<Value, EvalError> {
    str_value(number_text(&args[0], "num2str")?)
}

/// `format(Sf, x1, x2, ...)`: `Sf` with each `{k}` replaced by the text of argument `xk` (counted
/// from 0). Scalars are written as `num2str` writes them; strings as they are.
fn format(args: &[Value]) -> Result<Value, EvalError> {
    let template = text(&args[0], "format")?;
    let values = &args[1..];
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| EvalError::Unsupported("format: unmatched '{'".to_string()))?;
        let index = after[..close].parse::<usize>().map_err(|_| {
            EvalError::Unsupported("format: a field must be {k} with k a number".to_string())
        })?;
        let v = values
            .get(index)
            .ok_or_else(|| EvalError::Unsupported(format!("format: no argument {{{index}}}")))?;
        match v {
            Value::Str(s) => out.push_str(s),
            _ => out.push_str(&number_text(v, "format")?),
        }
        check_size(out.len())?;
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    str_value(out)
}

/// Parses a real number with an optional SI prefix (`k`, `M`, `G`, `m`, `u`, `n`, `p`) after it, as
/// engineering notation writes it. Non-finite values are refused.
fn parse_real(t: &str) -> Option<f64> {
    if let Ok(x) = t.parse::<f64>() {
        return x.is_finite().then_some(x);
    }
    let (body, scale) = match t.char_indices().last()? {
        (i, 'k') => (&t[..i], 1e3),
        (i, 'M') => (&t[..i], 1e6),
        (i, 'G') => (&t[..i], 1e9),
        (i, 'm') => (&t[..i], 1e-3),
        (i, 'u') | (i, 'µ') => (&t[..i], 1e-6),
        (i, 'n') => (&t[..i], 1e-9),
        (i, 'p') => (&t[..i], 1e-12),
        _ => return None,
    };
    let x = body.parse::<f64>().ok()?;
    let y = x * scale;
    y.is_finite().then_some(y)
}

/// Parses a complex number: a real number, `bi` (imaginary only), or `a+bi` / `a-bi`.
fn parse_complex(t: &str) -> Option<Complex64> {
    let t = t.trim();
    if let Some(re) = parse_real(t) {
        return Some(Complex64::new(re, 0.0));
    }
    let body = t.strip_suffix('i').or_else(|| t.strip_suffix('j'))?;
    // The split is the last sign that is not the sign of an exponent (`1e-3+2i`).
    let split = body
        .char_indices()
        .rev()
        .find(|(i, c)| {
            *i > 0 && (*c == '+' || *c == '-') && !matches!(body.as_bytes()[i - 1], b'e' | b'E')
        })
        .map(|(i, _)| i);
    let (re_text, im_text) = match split {
        Some(i) => (&body[..i], &body[i..]),
        None => ("", body),
    };
    let re = if re_text.is_empty() {
        0.0
    } else {
        parse_real(re_text)?
    };
    let im = match im_text {
        "" | "+" => 1.0,
        "-" => -1.0,
        other => parse_real(other)?,
    };
    Some(Complex64::new(re, im))
}

/// `str2num(Sn)`: the number a string spells: decimal, exponent or engineering notation, or complex.
fn str2num(args: &[Value]) -> Result<Value, EvalError> {
    let t = text(&args[0], "str2num")?;
    let z = parse_complex(t)
        .ok_or_else(|| EvalError::Unsupported(format!("str2num: {t:?} is not a number")))?;
    Ok(dimensionless(z))
}

/// `str2vec(S)`: the Unicode code points of `S` as a column vector. The empty string gives the scalar 0,
/// as the help says.
fn str2vec(args: &[Value]) -> Result<Value, EvalError> {
    let s = text(&args[0], "str2vec")?;
    if s.is_empty() {
        return Ok(dimensionless(Complex64::new(0.0, 0.0)));
    }
    let data: Vec<Scalar> = s
        .chars()
        .map(|c| Scalar::real(f64::from(u32::from(c)), Dimension::dimensionless()))
        .collect();
    let n = data.len();
    Ok(Value::Matrix(Matrix::new(n, 1, data)?))
}

/// Whether a code point is one `vec2str` accepts: tab, line feed, carriage return, or 32 to 255.
fn is_string_code(code: u32) -> bool {
    matches!(code, 9 | 10 | 13 | 32..=255)
}

/// `vec2str(v)`: the string whose characters have the codes in `v`. The scalar 0 is the empty string.
fn vec2str(args: &[Value]) -> Result<Value, EvalError> {
    let codes: Vec<f64> = match &args[0] {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            vec![s.value.re]
        }
        Value::Matrix(m) if m.rows() == 1 || m.cols() == 1 => {
            let mut out = Vec::with_capacity(m.data().len());
            for s in m.data() {
                ops::require_dimensionless(s.dim)?;
                out.push(s.value.re);
            }
            out
        }
        _ => {
            return Err(EvalError::Unsupported(
                "vec2str takes a vector of character codes".to_string(),
            ))
        }
    };
    if codes.len() == 1 && codes[0] == 0.0 {
        return str_value(String::new());
    }
    let mut out = String::with_capacity(codes.len());
    for c in codes {
        let code = integer_of(&Scalar::real(c, Dimension::dimensionless()))
            .and_then(|k| u32::try_from(k).ok())
            .filter(|k| is_string_code(*k))
            .ok_or_else(|| {
                EvalError::Unsupported(format!(
                    "vec2str: {c} is not a character code (9, 10, 13 or 32 to 255)"
                ))
            })?;
        out.push(char::from_u32(code).ok_or(EvalError::NotFinite)?);
    }
    str_value(out)
}

/// `error(S)`: the error message `S`, raised as an error of the calculation.
fn error(args: &[Value]) -> Result<Value, EvalError> {
    let s = text(&args[0], "error")?;
    Err(EvalError::UserMessage(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    fn n(x: f64) -> Value {
        Value::Scalar(Scalar::real(x, Dimension::dimensionless()))
    }

    fn text_of(v: Value) -> String {
        match v {
            Value::Str(s) => s,
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn real_of(v: Value) -> f64 {
        match v {
            Value::Scalar(s) => s.value.re,
            other => panic!("expected a scalar, got {other:?}"),
        }
    }

    #[test]
    fn strlen_counts_characters_not_bytes() {
        assert_eq!(real_of(strlen(&[st("héllo")]).unwrap()), 5.0);
        assert_eq!(real_of(strlen(&[st("")]).unwrap()), 0.0);
    }

    #[test]
    fn concat_joins_in_order() {
        assert_eq!(
            text_of(concat(&[st("ab"), st("é"), st("c")]).unwrap()),
            "abéc"
        );
        assert_eq!(text_of(concat(&[st("x")]).unwrap()), "x");
    }

    #[test]
    fn substr_slices_on_character_boundaries() {
        assert_eq!(text_of(substr(&[st("aéb"), n(1.0), n(2.0)]).unwrap()), "éb");
        assert_eq!(text_of(substr(&[st("abc"), n(3.0), n(0.0)]).unwrap()), "");
        assert!(substr(&[st("abc"), n(2.0), n(2.0)]).is_err());
        assert!(substr(&[st("abc"), n(-1.0), n(1.0)]).is_err());
    }

    #[test]
    fn search_returns_character_positions_or_minus_one() {
        assert_eq!(
            real_of(search(&[st("yes no yes"), st("yes"), n(0.0)]).unwrap()),
            0.0
        );
        assert_eq!(
            real_of(search(&[st("yes no yes"), st("yes"), n(1.0)]).unwrap()),
            7.0
        );
        assert_eq!(real_of(search(&[st("aéa"), st("a"), n(1.0)]).unwrap()), 2.0);
        assert_eq!(
            real_of(search(&[st("abc"), st("z"), n(0.0)]).unwrap()),
            -1.0
        );
        assert_eq!(
            real_of(search(&[st("abc"), st("a"), n(9.0)]).unwrap()),
            -1.0
        );
    }

    #[test]
    fn num2str_and_str2num_round_trip() {
        assert_eq!(text_of(num2str(&[n(2.0)]).unwrap()), "2");
        assert_eq!(text_of(num2str(&[n(0.5)]).unwrap()), "0.5");
        assert_eq!(real_of(str2num(&[st("1.5e3")]).unwrap()), 1500.0);
        assert_eq!(real_of(str2num(&[st("2k")]).unwrap()), 2000.0);
        assert_eq!(real_of(str2num(&[st("3m")]).unwrap()), 0.003);
        let z = match str2num(&[st("1+2i")]).unwrap() {
            Value::Scalar(s) => s.value,
            other => panic!("{other:?}"),
        };
        assert_eq!(z, Complex64::new(1.0, 2.0));
        let z = match str2num(&[st("1e-3-2j")]).unwrap() {
            Value::Scalar(s) => s.value,
            other => panic!("{other:?}"),
        };
        assert_eq!(z, Complex64::new(1e-3, -2.0));
        assert!(str2num(&[st("inf")]).is_err());
        assert!(str2num(&[st("abc")]).is_err());
    }

    #[test]
    fn str2vec_and_vec2str_are_inverse() {
        let v = str2vec(&[st("Hi")]).unwrap();
        let Value::Matrix(m) = &v else {
            panic!("expected a column")
        };
        assert_eq!(m.shape(), (2, 1));
        assert_eq!(text_of(vec2str(&[v]).unwrap()), "Hi");
        assert_eq!(real_of(str2vec(&[st("")]).unwrap()), 0.0);
        assert_eq!(text_of(vec2str(&[n(0.0)]).unwrap()), "");
        assert!(vec2str(&[n(7.0)]).is_err());
        assert!(vec2str(&[n(300.0)]).is_err());
    }

    #[test]
    fn format_fills_numbered_fields() {
        let out = format(&[st("The date is {0}/{1}/{2}"), n(4.0), n(7.0), st("x")]).unwrap();
        assert_eq!(text_of(out), "The date is 4/7/x");
        assert!(format(&[st("{3}"), n(1.0)]).is_err());
        assert!(format(&[st("{0")]).is_err());
    }

    #[test]
    fn error_raises_a_user_message() {
        assert_eq!(
            error(&[st("bad input")]),
            Err(EvalError::UserMessage("bad input".to_string()))
        );
    }

    #[test]
    fn size_cap_refuses_long_results() {
        let big = "a".repeat(MAX_STRING_BYTES);
        assert!(concat(&[st(&big), st("b")]).is_err());
        assert!(text(&st(&"a".repeat(MAX_STRING_BYTES + 1)), "x").is_err());
    }

    #[test]
    fn non_strings_are_refused() {
        assert!(strlen(&[n(1.0)]).is_err());
        assert!(substr(&[n(1.0), n(0.0), n(0.0)]).is_err());
    }
}

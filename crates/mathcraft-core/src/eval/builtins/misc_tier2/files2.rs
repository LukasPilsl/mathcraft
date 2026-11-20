//! The general and fixed-width text file functions: `READPRN`, `WRITEPRN`, `READFILE` and `WRITEFILE`
//! (help: "Fixed Width (PRN) Files", "Data Files").
//!
//! Built:
//! - `READPRN("file")`: the numbers of a PRN file, separated by spaces (or tabs). Only numeric
//!   files: a complex, string or nested PRN file is refused.
//! - `WRITEPRN("file", M)`: writes `M` as space-separated numbers. The optional `rows` and `cols`
//!   arguments are not built.
//! - `READFILE("file", ["delimited" | "fixed"], [colwidths])`: a delimited file (the delimiter is
//!   detected from its first line), or a fixed-width file cut into the columns of `colwidths`.
//!   `emptyfill`, `rows`, `cols` and `decsymb` are not built.
//! - `WRITEFILE("file", M)`: writes a tab-delimited text file (`.txt`, `.dat`). A scalar is a 1 by 1
//!   matrix. Excel files (`.xls`, `.xlsx`) are not built.
//!
//! The same access rules and 64 MiB cap as the CSV functions apply (see `fileio`).

use super::super::fileio::{FileError, FileResult, MAX_FILE_BYTES};
use super::super::matrix::expect_matrix;
use super::super::readwrite::{
    access_for, detect_delimiter, file_name, read_matrix, with_charge, write_matrix,
};
use super::super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Most cells one read may produce (the same cap as the CSV reader).
const MAX_CELLS: usize = 1_000_000;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("READPRN", Arity::Exact(1), read_prn);
    r.register_ctx("WRITEPRN", Arity::Exact(2), write_prn);
    r.register_ctx("READFILE", Arity::Range(1, 3), read_file);
    r.register_ctx("WRITEFILE", Arity::Exact(2), write_file);
}

/// `READPRN("file")`: the numbers of a space-separated PRN file.
fn read_prn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "READPRN")?;
    read_matrix(ctx, "READPRN", file, |_| ' ')
}

/// `WRITEPRN("file", M)`: writes `M` with spaces between the numbers, and returns `M`.
fn write_prn(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "WRITEPRN")?;
    let m = as_matrix(&args[1]);
    write_matrix(ctx, "WRITEPRN", file, &m, ' ')
}

/// `READFILE("file", [type], [colwidths])`.
fn read_file(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "READFILE")?;
    let kind = match args.get(1) {
        None => "delimited".to_string(),
        Some(Value::Str(s)) => s.to_ascii_lowercase(),
        Some(_) => {
            return Err(EvalError::Unsupported(
                "READFILE needs the type as text, in quotes".to_string(),
            ))
        }
    };
    match kind.as_str() {
        "delimited" => {
            if args.len() == 3 {
                return Err(EvalError::Unsupported(
                    "READFILE takes column widths only with type \"fixed\"".to_string(),
                ));
            }
            read_matrix(ctx, "READFILE", file, detect_delimiter)
        }
        "fixed" => {
            let widths = match args.get(2) {
                Some(v) => widths_of(v)?,
                None => {
                    return Err(EvalError::Unsupported(
                        "READFILE with type \"fixed\" needs the column widths".to_string(),
                    ))
                }
            };
            read_fixed(ctx, file, &widths)
        }
        "excel" | "xls" | "xlsx" => Err(EvalError::Unsupported(
            "Excel files are not implemented".to_string(),
        )),
        other => Err(EvalError::Unsupported(format!(
            "READFILE does not read the type {other:?}"
        ))),
    }
}

/// `WRITEFILE("file", M)`: a tab-delimited text file. Excel names are refused.
fn write_file(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "WRITEFILE")?;
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".xls") || lower.ends_with(".xlsx") {
        return Err(EvalError::Unsupported(
            "Excel files are not implemented: WRITEFILE writes text files (.txt, .dat)".to_string(),
        ));
    }
    let m = as_matrix(&args[1]);
    write_matrix(ctx, "WRITEFILE", file, &m, '\t')
}

/// A scalar as a 1 by 1 matrix; other values as they are.
fn as_matrix(v: &Value) -> Value {
    match v {
        Value::Scalar(s) => Matrix::new(1, 1, vec![*s])
            .map(Value::Matrix)
            .unwrap_or_else(|_| v.clone()),
        other => other.clone(),
    }
}

/// The column widths of a fixed-width file: a vector of positive integers.
fn widths_of(v: &Value) -> Result<Vec<usize>, EvalError> {
    let m = expect_matrix(v, "READFILE")?;
    let mut widths = Vec::with_capacity(m.data().len());
    for s in m.data() {
        ops_dimensionless(s)?;
        if s.value.im != 0.0 || s.value.re.fract() != 0.0 || s.value.re < 1.0 || s.value.re > 4096.0
        {
            return Err(EvalError::Unsupported(
                "READFILE column widths are whole numbers from 1 to 4096".to_string(),
            ));
        }
        widths.push(s.value.re as usize);
    }
    if widths.is_empty() {
        return Err(EvalError::Unsupported(
            "READFILE needs at least one column width".to_string(),
        ));
    }
    Ok(widths)
}

fn ops_dimensionless(s: &Scalar) -> Result<(), EvalError> {
    crate::eval::ops::require_dimensionless(s.dim)
}

/// Reads a fixed-width file: each non-blank line is cut at the column widths, and every cut field
/// must be a finite number. An empty field has no fill value here and is refused.
fn read_fixed(ctx: &BuiltinCtx<'_>, file: &str, widths: &[usize]) -> Result<Value, EvalError> {
    let (access, policy) = access_for(ctx, "READFILE", false)?;
    let total: usize = widths.iter().sum();
    let rows = with_charge(ctx, |charge| -> FileResult<Vec<Vec<f64>>> {
        let bytes = access.read(file, policy, MAX_FILE_BYTES)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| FileError::Format("the file is not UTF-8 text".to_string()))?;
        let mut out: Vec<Vec<f64>> = Vec::new();
        for line in text.lines() {
            charge(1 + line.len() as u64 / 16)?;
            if line.trim().is_empty() {
                continue;
            }
            if line.len() < total {
                return Err(FileError::Format(
                    "a line is shorter than the column widths".to_string(),
                ));
            }
            let mut row = Vec::with_capacity(widths.len());
            let mut start = 0usize;
            for &w in widths {
                let field = line.get(start..start + w).ok_or_else(|| {
                    FileError::Format("a column cuts a character in two".to_string())
                })?;
                let field = field.trim();
                if field.is_empty() {
                    return Err(FileError::Format(
                        "an empty field has no fill value (emptyfill is not built)".to_string(),
                    ));
                }
                let x: f64 = field
                    .parse()
                    .map_err(|_| FileError::Format(format!("not a number: {field:?}")))?;
                if !x.is_finite() {
                    return Err(FileError::Format("values must be finite".to_string()));
                }
                row.push(x);
                start += w;
            }
            if (out.len() + 1) * widths.len() > MAX_CELLS {
                return Err(FileError::TooLarge {
                    size: ((out.len() + 1) * widths.len()) as u64,
                    limit: MAX_CELLS as u64,
                });
            }
            out.push(row);
        }
        if out.is_empty() {
            return Err(FileError::Format("the file has no numbers".to_string()));
        }
        Ok(out)
    })?;
    let cols = widths.len();
    let data = rows
        .iter()
        .flatten()
        .map(|&x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(rows.len(), cols, data)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::builtins::BuiltinCtx;

    #[test]
    fn widths_must_be_whole_positive_numbers() {
        let ok = Value::Matrix(
            Matrix::new(
                2,
                1,
                vec![
                    Scalar::real(3.0, Dimension::dimensionless()),
                    Scalar::real(2.0, Dimension::dimensionless()),
                ],
            )
            .unwrap(),
        );
        assert_eq!(widths_of(&ok).unwrap(), vec![3, 2]);
        let bad = Value::Matrix(
            Matrix::new(1, 1, vec![Scalar::real(2.5, Dimension::dimensionless())]).unwrap(),
        );
        assert!(widths_of(&bad).is_err());
    }

    #[test]
    fn write_file_refuses_excel_names() {
        let ctx = BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3));
        let m = Value::Scalar(Scalar::real(1.0, Dimension::dimensionless()));
        let r = write_file(&ctx, &[Value::Str("book.xlsx".to_string()), m]);
        assert!(matches!(r, Err(EvalError::Unsupported(_))));
    }

    #[test]
    fn read_file_refuses_excel_and_unknown_types() {
        let ctx = BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3));
        let excel = read_file(
            &ctx,
            &[
                Value::Str("book.xlsx".to_string()),
                Value::Str("Excel".to_string()),
            ],
        );
        assert!(matches!(excel, Err(EvalError::Unsupported(_))));
        let other = read_file(
            &ctx,
            &[
                Value::Str("a.txt".to_string()),
                Value::Str("binary".to_string()),
            ],
        );
        assert!(matches!(other, Err(EvalError::Unsupported(_))));
    }
}

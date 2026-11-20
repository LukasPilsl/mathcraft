//! The file functions that read and write delimited text: `READCSV`, `WRITECSV`, `READTEXT` and
//! `WRITETEXT` (help: "CSV Data Files", "Text Files").
//!
//! They reach the disk only through the worksheet's [`FileOptions`]. With no access set (the
//! default), each one fails with "file access is disabled for this worksheet". Every line parsed
//! or written is charged to the statement's work budget (see `fileio::parse_delimited`), so a
//! huge file runs out of budget like any other expensive expression.
//!
//! Values: a read gives a real matrix, unit-free. A write stores each element's SI value (Mathcad
//! stores SI, see MCDX_FORMAT.md section 3) and fails for a complex element. A write returns the
//! matrix it wrote, so `WRITECSV("a.csv", M) = M` shows the matrix, as the help example does.
//!
//! Not built (their formats need their own parsers): READFILE/WRITEFILE (fixed-width and Excel
//! forms), READPRN/WRITEPRN/APPENDPRN, READEXCEL/WRITEEXCEL, READBIN/WRITEBIN, the image readers
//! and writers (READBMP, READRGB, READ_IMAGE, READ_HLS, READ_HSV and their writers), and
//! READWAV/WRITEWAV/GETWAVINFO. The optional arguments of WRITECSV and WRITETEXT (rows, cols,
//! decimal symbol) and the column, row and options arguments of READTEXT are not built either.

use super::fileio::{format_delimited, parse_delimited, FileError, FileResult, MAX_FILE_BYTES};
use super::matrix::expect_matrix;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Most cells one read may produce. A 64 MiB file of one-digit numbers holds about 3e7 cells, so
/// the work budget normally runs out first; this cap bounds the memory on top of that. Reads of
/// one evaluation share a further quota (see `FileOptions::reserve`).
const MAX_CELLS: usize = 1_000_000;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("READCSV", Arity::Exact(1), read_csv);
    r.register_ctx("WRITECSV", Arity::Exact(2), write_csv);
    r.register_ctx("READTEXT", Arity::Exact(1), read_text);
    r.register_ctx("WRITETEXT", Arity::Exact(2), write_text);
}

/// `READCSV("file")`: the comma-separated numbers of `file`.
fn read_csv(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "READCSV")?;
    read_matrix(ctx, "READCSV", file, |_| ',')
}

/// `READTEXT("file")`: the numbers of a text file, with the delimiter detected from the first
/// non-blank line (tab, then comma, then whitespace).
fn read_text(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "READTEXT")?;
    read_matrix(ctx, "READTEXT", file, detect_delimiter)
}

/// `WRITECSV("file", M)`: writes `M` as comma-separated values, one row per line.
fn write_csv(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "WRITECSV")?;
    write_matrix(ctx, "WRITECSV", file, &args[1], ',')
}

/// `WRITETEXT("file", M)`: writes `M` as tab-delimited text.
fn write_text(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let file = file_name(&args[0], "WRITETEXT")?;
    write_matrix(ctx, "WRITETEXT", file, &args[1], '\t')
}

/// The file name argument: a text value.
pub(super) fn file_name<'a>(v: &'a Value, name: &str) -> Result<&'a str, EvalError> {
    match v {
        Value::Str(s) => Ok(s),
        _ => Err(EvalError::Unsupported(format!(
            "{name} needs the file name as text, in quotes"
        ))),
    }
}

/// The access of this evaluation for a read (`write` false) or a write, or the error that says why
/// it is not available: no base directory, or the switch for that kind of access is off.
pub(super) fn access_for<'a>(
    ctx: &'a BuiltinCtx<'_>,
    name: &str,
    write: bool,
) -> Result<
    (
        &'a (dyn super::fileio::FileAccess + Send + Sync),
        super::fileio::FilePolicy,
    ),
    EvalError,
> {
    let files = ctx.files();
    let Some(access) = files.access() else {
        return Err(EvalError::FileAccess(format!(
            "{name}: file access is disabled for this worksheet (no base directory is set)"
        )));
    };
    let switches = files.switches();
    if write && !switches.write {
        return Err(EvalError::FileAccess(format!(
            "{name}: file writing is disabled. Switch on Allow writing files for this session; \
             in Automatic calculation, press F9 to write"
        )));
    }
    if !write && !switches.read {
        return Err(EvalError::FileAccess(format!(
            "{name}: file reading is disabled. Switch on Allow reading files for this session"
        )));
    }
    Ok((access, files.policy()))
}

/// Takes bytes and cells from the evaluation's file quota (see `FileOptions::reserve`).
fn reserve(ctx: &BuiltinCtx<'_>, bytes: u64, cells: u64) -> FileResult<()> {
    ctx.files().reserve(bytes, cells)
}

/// Reads `file` and parses it with the delimiter `delimiter_for` picks from the bytes. The result is
/// built straight into the matrix's scalars, without nested rows.
pub(super) fn read_matrix(
    ctx: &BuiltinCtx<'_>,
    name: &str,
    file: &str,
    delimiter_for: fn(&[u8]) -> char,
) -> Result<Value, EvalError> {
    let (access, policy) = access_for(ctx, name, false)?;
    let grid = with_charge(ctx, |charge| {
        let bytes = access.read(file, policy, MAX_FILE_BYTES)?;
        reserve(ctx, bytes.len() as u64, 0)?;
        let grid = parse_delimited(&bytes, delimiter_for(&bytes), MAX_CELLS, charge)?;
        reserve(ctx, 0, grid.data.len() as u64)?;
        Ok(grid)
    })?;
    let data = grid
        .data
        .into_iter()
        .map(|x| Scalar::real(x, Dimension::dimensionless()))
        .collect();
    Ok(Value::Matrix(Matrix::new(grid.rows, grid.cols, data)?))
}

/// Writes matrix `m` to `file` with `delimiter`, and returns `m`.
pub(super) fn write_matrix(
    ctx: &BuiltinCtx<'_>,
    name: &str,
    file: &str,
    m: &Value,
    delimiter: char,
) -> Result<Value, EvalError> {
    let matrix = expect_matrix(m, name)?;
    let (access, policy) = access_for(ctx, name, true)?;
    let cols = matrix.cols().max(1);
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(matrix.rows());
    for chunk in matrix.data().chunks(cols) {
        let mut row = Vec::with_capacity(chunk.len());
        for s in chunk {
            if s.value.im != 0.0 {
                return Err(EvalError::Unsupported(format!(
                    "{name} writes real values only"
                )));
            }
            row.push(s.value.re);
        }
        rows.push(row);
    }
    with_charge(ctx, |charge| {
        let bytes = format_delimited(&rows, delimiter, charge)?;
        access.write(file, policy, &bytes)
    })?;
    Ok(m.clone())
}

/// The delimiter of a text file, from its first line that is not blank.
pub(super) fn detect_delimiter(bytes: &[u8]) -> char {
    let line = bytes
        .split(|b| *b == b'\n')
        .find(|l| !l.iter().all(u8::is_ascii_whitespace))
        .unwrap_or(&[]);
    if line.contains(&b'\t') {
        '\t'
    } else if line.contains(&b',') {
        ','
    } else {
        ' '
    }
}

/// Runs a file operation with its loops charged to the statement's budget, then turns its error
/// into an `EvalError`. A budget failure keeps the budget's own error (`WorkLimit`).
pub(super) fn with_charge<T>(
    ctx: &BuiltinCtx<'_>,
    run: impl FnOnce(&mut dyn FnMut(u64) -> FileResult<()>) -> FileResult<T>,
) -> Result<T, EvalError> {
    let mut failure: Option<EvalError> = None;
    let outcome = {
        let mut charge = |units: u64| -> FileResult<()> {
            ctx.charge(units).map_err(|e| {
                failure = Some(e);
                FileError::Budget
            })
        };
        run(&mut charge)
    };
    match outcome {
        Ok(value) => Ok(value),
        Err(err) => Err(failure.unwrap_or_else(|| to_eval_error(err))),
    }
}

/// The text of a file error, as the user sees it.
fn to_eval_error(err: FileError) -> EvalError {
    let text = match err {
        FileError::Refused(m) => format!("file access refused: {m}"),
        FileError::NotFound(name) => format!("file not found: {name}"),
        FileError::TooLarge { size, limit } => {
            format!("file too large: {size} exceeds the limit of {limit}")
        }
        FileError::Format(m) => m,
        FileError::Io(m) => format!("file error: {m}"),
        FileError::Budget => "work limit exceeded while reading or writing a file".to_string(),
        FileError::Quota => {
            "the file quota of this worksheet calculation is used up (4 million cells or 256 MiB)"
                .to_string()
        }
    };
    EvalError::FileAccess(text)
}

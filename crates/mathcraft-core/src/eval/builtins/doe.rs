//! Design-of-experiments matrices: `fullfact`, `boxwilson` and `doelabel` (help: "Factorial Designs",
//! "Box-Behnken and Box-Wilson Designs", "Example: Second-Order Response Surface").
//!
//! A design matrix has the columns `Run`, `Block` and then one column per factor, in coded levels.
//! Runs are numbered from 1 and every run is in block 1. Two-level factors are coded -1 and +1,
//! the first factor changing fastest (NIST order).
//!
//! Built:
//! - `fullfact(n)`: the 2^n two-level full factorial. A `levels` argument is accepted only as 2.
//! - `boxwilson(n, [nc, alpha])`: the central composite design for 2 to 4 factors, in the order of
//!   the help example (factorial runs, then the centre points, then the axial points +alpha/-alpha
//!   for each factor in turn). Default `alpha` is nf^(1/4) and default `nc` is
//!   round(4 sqrt(nf + 1) - 2n), where nf = 2^n. The example pins n = 2 only.
//! - `doelabel(X, Vals)`: converts coded factor columns to real values. Row k of `Vals` holds the
//!   real levels of factor k: two values for coded -1 and +1, or an odd count with the middle value
//!   at coded 0 and the slope set by the two neighbours of the middle (the help example: five
//!   values 77.93..92.07 for coded -1.414..1.414).
//!
//! Not built: `boxbehnken` (its run order is a table in NIST's documentation, not in the help), the
//! fractional factorials `fractfact`, `plackettburman` and `taguchi` (tables and generator
//! algorithms), `fractruns`, `fractresol`, `fractalias`, `block`, `foldover`, `randomize`,
//! `quickscreen`, `effects` and `anova` (factor screening), the Monte Carlo functions, and the
//! polynomial regressions `polyfitc` and `polyfitstat` (`polyfit` is in `doe_fit`).

use super::matrix::expect_matrix;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::env::integer_of;
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Largest design, in cells (runs times columns). Charged to the work budget as well.
const MAX_DESIGN_CELLS: usize = 1_000_000;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("fullfact", Arity::Range(1, 2), fullfact);
    r.register_ctx("boxwilson", Arity::Range(1, 3), boxwilson);
    r.register_ctx("doelabel", Arity::Exact(2), doelabel);
}

/// `fullfact(n, [levels])`: the two-level full factorial with `n` factors.
fn fullfact(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let factors = integer_arg(&args[0], "fullfact")?;
    if let Some(levels) = args.get(1) {
        if integer_arg(levels, "fullfact")? != 2 {
            return Err(EvalError::Unsupported(
                "fullfact: only two levels per factor are built".to_string(),
            ));
        }
    }
    let factors = check_range(factors, 1, 16, "fullfact")?;
    design(ctx, factorial_rows(factors))
}

/// `boxwilson(n, [nc, alpha])`: the central composite design for 2 to 4 factors.
fn boxwilson(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let factors = check_range(integer_arg(&args[0], "boxwilson")?, 2, 4, "boxwilson")?;
    let factorial = factorial_rows(factors);
    let nf = factorial.len();
    let centres = match args.get(1) {
        Some(v) => {
            let nc = integer_arg(v, "boxwilson")?;
            usize::try_from(nc).map_err(|_| {
                EvalError::Unsupported(
                    "boxwilson needs a count of centre points, 0 or more".to_string(),
                )
            })?
        }
        None => {
            let nc = 4.0 * ((nf + 1) as f64).sqrt() - 2.0 * factors as f64;
            nc.round().max(0.0) as usize
        }
    };
    let alpha = match args.get(2) {
        Some(v) => real_arg(v, "boxwilson")?,
        None => (nf as f64).powf(0.25),
    };
    // Check the size before the centre points are built: `nc` is the caller's number.
    let cells = (nf + centres + 2 * factors).saturating_mul(factors + 2);
    if cells > MAX_DESIGN_CELLS {
        return Err(EvalError::TooLarge(cells));
    }
    let mut rows = factorial;
    rows.extend((0..centres).map(|_| vec![0.0; factors]));
    for j in 0..factors {
        for sign in [alpha, -alpha] {
            let mut point = vec![0.0; factors];
            point[j] = sign;
            rows.push(point);
        }
    }
    design(ctx, rows)
}

/// `doelabel(X, Vals)`: the design `X` with its factor columns converted from coded to real values.
fn doelabel(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let design = expect_matrix(&args[0], "doelabel")?;
    let levels = expect_matrix(&args[1], "doelabel")?;
    if design.cols() < 3 {
        return Err(EvalError::Unsupported(
            "doelabel needs the Run and Block columns and at least one factor".to_string(),
        ));
    }
    let factors = design.cols() - 2;
    if levels.rows() != factors {
        return Err(EvalError::Unsupported(format!(
            "doelabel needs one row of levels for each of the {factors} factors"
        )));
    }
    let mut mappings = Vec::with_capacity(factors);
    for k in 0..factors {
        let mut row = Vec::with_capacity(levels.cols());
        for c in 0..levels.cols() {
            row.push(real_arg(&Value::Scalar(*levels.get(k, c)?), "doelabel")?);
        }
        mappings.push(level_mapping(&row)?);
    }
    ctx.charge((design.rows() * design.cols()) as u64)?;
    let mut data = Vec::with_capacity(design.rows() * design.cols());
    for i in 0..design.rows() {
        for j in 0..design.cols() {
            let coded = real_arg(&Value::Scalar(*design.get(i, j)?), "doelabel")?;
            let real = if j < 2 {
                coded
            } else {
                let (centre, slope) = mappings[j - 2];
                centre + slope * coded
            };
            data.push(Scalar::real(real, Dimension::dimensionless()));
        }
    }
    Ok(Value::Matrix(Matrix::new(
        design.rows(),
        design.cols(),
        data,
    )?))
}

/// The centre and slope that map coded values to a factor's real levels (see the module docs).
fn level_mapping(levels: &[f64]) -> Result<(f64, f64), EvalError> {
    match levels.len() {
        2 => Ok(((levels[0] + levels[1]) / 2.0, (levels[1] - levels[0]) / 2.0)),
        n if n >= 3 && n % 2 == 1 => {
            let mid = n / 2;
            Ok((levels[mid], (levels[mid + 1] - levels[mid - 1]) / 2.0))
        }
        _ => Err(EvalError::Unsupported(
            "doelabel needs two levels, or an odd number of levels with the middle one at coded 0"
                .to_string(),
        )),
    }
}

/// The coded rows of the two-level full factorial with `factors` factors, in NIST order: factor 1
/// changes fastest.
fn factorial_rows(factors: usize) -> Vec<Vec<f64>> {
    (0..1usize << factors)
        .map(|run| {
            (0..factors)
                .map(|j| if (run >> j) & 1 == 1 { 1.0 } else { -1.0 })
                .collect()
        })
        .collect()
}

/// The design matrix of coded `rows`: a `Run` column, a `Block` column of ones, then the factors.
fn design(ctx: &BuiltinCtx<'_>, rows: Vec<Vec<f64>>) -> Result<Value, EvalError> {
    let factors = rows.first().map_or(0, Vec::len);
    let cols = factors + 2;
    let cells = rows.len().saturating_mul(cols);
    if cells > MAX_DESIGN_CELLS {
        return Err(EvalError::TooLarge(cells));
    }
    ctx.charge(cells as u64)?;
    let mut data = Vec::with_capacity(cells);
    for (run, row) in rows.iter().enumerate() {
        data.push(Scalar::real((run + 1) as f64, Dimension::dimensionless()));
        data.push(Scalar::real(1.0, Dimension::dimensionless()));
        for &x in row {
            data.push(Scalar::real(x, Dimension::dimensionless()));
        }
    }
    Ok(Value::Matrix(Matrix::new(rows.len(), cols, data)?))
}

/// An integer argument: a dimensionless real integer.
fn integer_arg(v: &Value, name: &str) -> Result<i64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            integer_of(s).ok_or_else(|| EvalError::Unsupported(format!("{name} needs an integer")))
        }
        _ => Err(EvalError::Unsupported(format!("{name} needs an integer"))),
    }
}

/// A real argument: a dimensionless real number.
fn real_arg(v: &Value, name: &str) -> Result<f64, EvalError> {
    match v {
        Value::Scalar(s) => {
            ops::require_dimensionless(s.dim)?;
            if s.value.im != 0.0 || !s.value.re.is_finite() {
                return Err(EvalError::Unsupported(format!(
                    "{name} needs a real number"
                )));
            }
            Ok(s.value.re)
        }
        _ => Err(EvalError::Unsupported(format!(
            "{name} needs a real number"
        ))),
    }
}

fn check_range(value: i64, lo: i64, hi: i64, name: &str) -> Result<usize, EvalError> {
    if (lo..=hi).contains(&value) {
        Ok(value as usize)
    } else {
        Err(EvalError::Unsupported(format!(
            "{name} is built for {lo} to {hi} factors, found {value}"
        )))
    }
}

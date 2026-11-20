//! Factor screening of two-level designs: `quickscreen` and `effects` (help: "Effects", "Example:
//! Calculating Effects").
//!
//! The design `K` is a matrix with the columns `Run`, `Block` and then one column per factor, coded
//! +1 and -1 (`fullfact`, `boxwilson` and `block` give this shape); `Y` has one row per run and one
//! column per replicate. Every run must have the same number of replicates.
//!
//! - `quickscreen(K, Y, [terms])`: a list of two items, the labels (a list of strings) and a matrix
//!   with one row per label and the columns of the help table: the low average (-), the high average
//!   (+) and the effect (+) - (-). For `Blocks` the two averages are those of the first and second
//!   block. Without `terms` it reports every factor and, when the design has two blocks, `Blocks`.
//! - `effects(K, Y, [terms])`: a list of two items, the labels and a list of matrices, one per label.
//!   A factor gives a 2 x 2 matrix (level, effect of that level about the overall mean), `Blocks` the
//!   same with the block numbers, and a two-letter interaction such as `"AB"` a 4 x 3 matrix (level
//!   of A, level of B, effect). Without `terms` it reports the factors only.
//!
//! `terms` is a string of factor letters (`A` is the first factor column), interactions written as
//! the letters together (`AB`, the product of the coded columns) and `Blocks`; the separators are a
//! space, a comma, a colon or a semicolon. The results follow the order of the string.
//!
//! The averages are the means of the run means, and the overall mean is the mean of the runs, as in
//! the help example. The help's text on the table layout (a text column in one matrix) cannot be
//! written as a matrix, so the labels are returned as a separate list.
//!
//! Refused rather than guessed: NaN padding (unequal replicates), more than two blocks, a factor
//! with one level, a coded value other than +1 or -1, an integer `order` (the help does not show its
//! output), and an effect of three or more letters in `effects`.

use super::matrix::expect_matrix;
use super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Most factors a design may have (one letter each).
const MAX_FACTORS: usize = 26;
/// Most terms reported in one call.
const MAX_TERMS: usize = 64;
/// Tolerance for a coded level being exactly +1 or -1.
const CODE_TOL: f64 = 1e-9;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("quickscreen", Arity::Range(2, 3), quickscreen);
    r.register_ctx("effects", Arity::Range(2, 3), effects);
}

/// `quickscreen(K, Y, [terms])`: the low average, high average and effect of each term.
fn quickscreen(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let screen = Screen::new(ctx, args, "quickscreen")?;
    let terms = screen.terms(args.get(2), "quickscreen", true)?;
    ctx.charge((terms.len() * screen.means.len()) as u64)?;
    let mut labels = Vec::with_capacity(terms.len());
    let mut data = Vec::with_capacity(terms.len() * 3);
    for (label, term) in &terms {
        let (low, high) = screen.split(term)?;
        labels.push(Value::Str(label.clone()));
        data.push(Scalar::real(low, screen.dim));
        data.push(Scalar::real(high, screen.dim));
        data.push(Scalar::real(high - low, screen.dim));
    }
    Ok(Value::List(vec![
        Value::List(labels),
        Value::Matrix(Matrix::new(terms.len(), 3, data)?),
    ]))
}

/// `effects(K, Y, [terms])`: the level effects of each term about the overall mean.
fn effects(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let screen = Screen::new(ctx, args, "effects")?;
    let terms = screen.terms(args.get(2), "effects", false)?;
    ctx.charge((terms.len() * screen.means.len()) as u64)?;
    let mut labels = Vec::with_capacity(terms.len());
    let mut tables = Vec::with_capacity(terms.len());
    for (label, term) in &terms {
        labels.push(Value::Str(label.clone()));
        tables.push(Value::Matrix(screen.effect_table(term)?));
    }
    Ok(Value::List(vec![Value::List(labels), Value::List(tables)]))
}

/// A term of the screen: the product of the coded columns of some factors, or the blocks.
enum Term {
    Letters(Vec<usize>),
    Blocks,
}

/// The checked design: coded factor columns, the block of each run, and the mean of each run.
struct Screen {
    factors: Vec<Vec<f64>>,
    blocks: Vec<f64>,
    block_pair: Option<(f64, f64)>,
    means: Vec<f64>,
    overall: f64,
    dim: Dimension,
}

impl Screen {
    fn new(ctx: &BuiltinCtx<'_>, args: &[Value], name: &str) -> Result<Self, EvalError> {
        let k = expect_matrix(&args[0], name)?;
        let y = expect_matrix(&args[1], name)?;
        if k.cols() < 3 {
            return Err(EvalError::Unsupported(format!(
                "{name} needs the Run and Block columns and at least one factor"
            )));
        }
        if k.rows() != y.rows() {
            return Err(EvalError::ShapeMismatch {
                left: k.shape(),
                right: y.shape(),
            });
        }
        let factor_count = k.cols() - 2;
        if factor_count > MAX_FACTORS {
            return Err(EvalError::TooLarge(factor_count));
        }
        ctx.charge((k.rows() * k.cols() + y.rows() * y.cols()) as u64)?;

        let mut blocks = Vec::with_capacity(k.rows());
        for i in 0..k.rows() {
            let s = k.get(i, 1)?;
            ops::require_dimensionless(s.dim)?;
            blocks.push(real_value(s, name)?);
        }
        let mut distinct = blocks.clone();
        distinct.sort_by(f64::total_cmp);
        distinct.dedup();
        if distinct.len() > 2 {
            return Err(EvalError::Unsupported(format!(
                "{name} is built for designs with two blocks at most"
            )));
        }
        let block_pair = (distinct.len() == 2).then(|| (distinct[0], distinct[1]));

        let mut factors = Vec::with_capacity(factor_count);
        for j in 2..k.cols() {
            let mut column = Vec::with_capacity(k.rows());
            for i in 0..k.rows() {
                column.push(coded_level(k.get(i, j)?, name)?);
            }
            if !column.contains(&1.0) || !column.contains(&-1.0) {
                return Err(EvalError::Unsupported(format!(
                    "{name} needs both levels of every factor"
                )));
            }
            factors.push(column);
        }

        let dim = y.get(0, 0)?.dim;
        let mut means = Vec::with_capacity(y.rows());
        for i in 0..y.rows() {
            let mut sum = 0.0;
            for j in 0..y.cols() {
                let s = y.get(i, j)?;
                if s.dim != dim {
                    return Err(EvalError::DimensionMismatch {
                        left: dim,
                        right: s.dim,
                    });
                }
                sum += real_value(s, name)?;
            }
            means.push(sum / y.cols() as f64);
        }
        let overall = means.iter().sum::<f64>() / means.len() as f64;
        Ok(Self {
            factors,
            blocks,
            block_pair,
            means,
            overall,
            dim,
        })
    }

    /// The terms in the order asked for. Without a string, every factor (and `Blocks` when
    /// `default_blocks` and the design has two blocks).
    fn terms(
        &self,
        spec: Option<&Value>,
        name: &str,
        default_blocks: bool,
    ) -> Result<Vec<(String, Term)>, EvalError> {
        let mut out: Vec<(String, Term)> = Vec::new();
        match spec {
            None => {
                for j in 0..self.factors.len() {
                    out.push((letter(j).to_string(), Term::Letters(vec![j])));
                }
                if default_blocks && self.block_pair.is_some() {
                    out.push(("Blocks".to_string(), Term::Blocks));
                }
            }
            Some(Value::Str(text)) => {
                for token in text
                    .split([' ', ',', ':', ';'])
                    .filter(|t| !t.is_empty())
                {
                    if token.eq_ignore_ascii_case("blocks") {
                        if self.block_pair.is_none() {
                            return Err(EvalError::Unsupported(format!(
                                "{name}: Blocks needs a design with two blocks"
                            )));
                        }
                        out.push(("Blocks".to_string(), Term::Blocks));
                        continue;
                    }
                    let mut js: Vec<usize> = Vec::new();
                    for ch in token.chars() {
                        if !ch.is_ascii_uppercase() {
                            return Err(EvalError::Unsupported(format!(
                                "{name}: `{token}` is not factor letters or Blocks"
                            )));
                        }
                        let j = (ch as u8 - b'A') as usize;
                        if j >= self.factors.len() || js.contains(&j) {
                            return Err(EvalError::Unsupported(format!(
                                "{name}: `{token}` names a factor that is not in the design, or twice"
                            )));
                        }
                        js.push(j);
                    }
                    js.sort_unstable();
                    out.push((token.to_string(), Term::Letters(js)));
                    if out.len() > MAX_TERMS {
                        return Err(EvalError::TooLarge(out.len()));
                    }
                }
                if out.is_empty() {
                    return Err(EvalError::Unsupported(format!(
                        "{name} needs at least one term"
                    )));
                }
            }
            Some(_) => {
                return Err(EvalError::Unsupported(format!(
                    "{name} takes an optional string of terms such as \"A, AB, Blocks\"; an integer order is not built"
                )))
            }
        }
        Ok(out)
    }

    /// The low and high averages of a term: the (-) and (+) level means, or the first and second
    /// block means.
    fn split(&self, term: &Term) -> Result<(f64, f64), EvalError> {
        match term {
            Term::Blocks => {
                let (b1, b2) = self.block_pair.ok_or_else(|| {
                    EvalError::Unsupported("Blocks needs a design with two blocks".to_string())
                })?;
                Ok((self.block_mean(b1)?, self.block_mean(b2)?))
            }
            Term::Letters(js) => {
                let low = self.group_mean(|r| self.product(js, r) < 0.0);
                let high = self.group_mean(|r| self.product(js, r) > 0.0);
                match (low, high) {
                    (Some(lo), Some(hi)) => Ok((lo, hi)),
                    _ => Err(EvalError::Unsupported(
                        "a term needs both of its levels in the design".to_string(),
                    )),
                }
            }
        }
    }

    /// The effect table of a term: `(level, effect)` rows for a factor or the blocks, and
    /// `(level of A, level of B, effect)` rows for a two-letter interaction.
    fn effect_table(&self, term: &Term) -> Result<Matrix, EvalError> {
        let dimless = Dimension::dimensionless();
        match term {
            Term::Blocks => {
                let (b1, b2) = self.block_pair.ok_or_else(|| {
                    EvalError::Unsupported("Blocks needs a design with two blocks".to_string())
                })?;
                let (lo, hi) = self.split(term)?;
                let data = vec![
                    Scalar::real(b1, dimless),
                    Scalar::real(lo - self.overall, self.dim),
                    Scalar::real(b2, dimless),
                    Scalar::real(hi - self.overall, self.dim),
                ];
                Matrix::new(2, 2, data)
            }
            Term::Letters(js) if js.len() == 1 => {
                let (lo, hi) = self.split(term)?;
                let data = vec![
                    Scalar::real(-1.0, dimless),
                    Scalar::real(lo - self.overall, self.dim),
                    Scalar::real(1.0, dimless),
                    Scalar::real(hi - self.overall, self.dim),
                ];
                Matrix::new(2, 2, data)
            }
            Term::Letters(js) if js.len() == 2 => {
                let (a, b) = (js[0], js[1]);
                let mut data = Vec::with_capacity(12);
                for sa in [-1.0, 1.0] {
                    for sb in [-1.0, 1.0] {
                        let cell = self
                            .group_mean(|r| self.factors[a][r] == sa && self.factors[b][r] == sb)
                            .ok_or_else(|| {
                                EvalError::Unsupported(
                                    "an interaction needs every combination of its factors"
                                        .to_string(),
                                )
                            })?;
                        let level_a =
                            self.group_mean(|r| self.factors[a][r] == sa)
                                .ok_or_else(|| {
                                    EvalError::Unsupported("a factor has no level".to_string())
                                })?;
                        let level_b =
                            self.group_mean(|r| self.factors[b][r] == sb)
                                .ok_or_else(|| {
                                    EvalError::Unsupported("a factor has no level".to_string())
                                })?;
                        let effect = cell - level_a - level_b + self.overall;
                        data.push(Scalar::real(sa, dimless));
                        data.push(Scalar::real(sb, dimless));
                        data.push(Scalar::real(effect, self.dim));
                    }
                }
                Matrix::new(4, 3, data)
            }
            Term::Letters(_) => Err(EvalError::Unsupported(
                "effects of an interaction of three or more factors are not built".to_string(),
            )),
        }
    }

    fn product(&self, js: &[usize], run: usize) -> f64 {
        js.iter().map(|&j| self.factors[j][run]).product()
    }

    fn block_mean(&self, block: f64) -> Result<f64, EvalError> {
        self.group_mean(|r| self.blocks[r] == block)
            .ok_or_else(|| EvalError::Unsupported("a block has no runs".to_string()))
    }

    /// The mean of the run means over the runs that `keep` selects.
    fn group_mean(&self, keep: impl Fn(usize) -> bool) -> Option<f64> {
        let mut sum = 0.0;
        let mut count = 0usize;
        for (r, m) in self.means.iter().enumerate() {
            if keep(r) {
                sum += m;
                count += 1;
            }
        }
        (count > 0).then(|| sum / count as f64)
    }
}

/// A factor letter for a zero-based index: 0 is `A`.
fn letter(j: usize) -> char {
    (b'A' + j as u8) as char
}

/// A real value in SI units. `NaN` (padding) is refused: every run needs the same replicates.
fn real_value(s: &Scalar, name: &str) -> Result<f64, EvalError> {
    if s.value.re.is_nan() || s.value.im.is_nan() {
        return Err(EvalError::Unsupported(format!(
            "{name} is built for equal replicates per run; NaN padding is not built"
        )));
    }
    if s.value.im != 0.0 || !s.value.re.is_finite() {
        return Err(EvalError::Unsupported(format!(
            "{name} needs real, finite values"
        )));
    }
    Ok(s.value.re)
}

/// A coded factor level: exactly +1 or -1, dimensionless. Returns the sign as `1.0` or `-1.0`.
fn coded_level(s: &Scalar, name: &str) -> Result<f64, EvalError> {
    ops::require_dimensionless(s.dim)?;
    let v = real_value(s, name)?;
    if (v - 1.0).abs() < CODE_TOL {
        Ok(1.0)
    } else if (v + 1.0).abs() < CODE_TOL {
        Ok(-1.0)
    } else {
        Err(EvalError::Unsupported(format!(
            "{name} needs a two-level design coded +1 and -1"
        )))
    }
}

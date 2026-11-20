//! Calculation: evaluates a document under the engine's safety limits and reports each region's
//! result as plain text. The limits are the engine's own: the work budget of a sheet
//! (`WORKSHEET_MAX_WORK`), a wall-clock deadline (60 s by default), a cancel flag, and the region
//! cap (`MAX_REGIONS`, checked when regions are inserted). Regions reached after a limit is hit are
//! reported as skipped with the reason. File functions have no access.
//!
//! Results are English text for a reader that is not a window: numbers in the worksheet's result
//! format and unit system, matrices as `[[1, 2], [3, 4]]`. The GUI formats results itself.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use mathcraft_core::eval::builtins::fileio::FileOptions;
use mathcraft_core::eval::{format_complex_with, NumberSeparators, Outcome, ResultFormat, Value};
use mathcraft_core::expr::Expr;
use mathcraft_core::units::{self, display_unit, CompoundUnit, Dimension, UnitSystem};
use mathcraft_import::evaluate::{
    evaluate_worksheet_limited, is_stopped_reason, on_big_stack, panic_message, EvalLimits,
    EvalOutcome, RegionEvaluation, WorksheetEvaluation, EVALUATION_DEADLINE,
};
use mathcraft_import::regions::{RegionKind, RegionModel, Worksheet};
use mathcraft_import::results::ResultTable;

use crate::document::Document;

/// How to run a calculation.
#[derive(Debug, Clone)]
pub struct CalcOptions {
    /// The regions whose results are reported; `None` reports every region. The whole worksheet
    /// is always evaluated, because a region needs the definitions above it.
    pub regions: Option<Vec<u32>>,
    /// Stops the evaluation between regions when set.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Wall-clock limit of the evaluation.
    pub deadline: Duration,
}

impl Default for CalcOptions {
    fn default() -> Self {
        Self {
            regions: None,
            cancel: None,
            deadline: EVALUATION_DEADLINE,
        }
    }
}

/// What became of one region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultStatus {
    /// A value, shown after `=`.
    Value,
    /// A definition (`x := ...`), or a constraint of a solve block: nothing to show.
    Defined,
    /// A plot with its traces evaluated.
    Plot,
    /// The engine reported an error, or a plot trace failed.
    Error,
    /// The region was not run (a limit was reached, or it is not evaluated).
    Skipped,
}

/// The result of one region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionResult {
    pub region_id: u32,
    /// The expression as text.
    pub expr: String,
    pub status: ResultStatus,
    /// The value as text, empty when there is none.
    pub value: String,
    /// The error or the reason the region was skipped.
    pub error: Option<String>,
    /// For a definition: the name it defines.
    pub defines: Option<String>,
}

/// The outcome of one calculation.
#[derive(Debug, Clone, PartialEq)]
pub struct Calculation {
    /// The document revision the results belong to. They are stale once the revision moves on.
    pub revision: u64,
    pub results: Vec<RegionResult>,
    /// Problems that did not stop the run.
    pub notes: Vec<String>,
    /// Why the evaluation stopped early (limit, deadline or cancel), when it did.
    pub stopped: Option<String>,
}

impl Calculation {
    /// The result of region `id`, if it has one.
    pub fn result(&self, id: u32) -> Option<&RegionResult> {
        self.results.iter().find(|r| r.region_id == id)
    }
}

/// Evaluates `doc` on a thread with the engine's stack size.
pub fn calculate(doc: &Document, revision: u64, opts: &CalcOptions) -> Calculation {
    let format = doc.settings().result_format.clone().unwrap_or_default();
    calculate_in_format(doc, revision, opts, &format)
}

/// Like [`calculate`], but the value texts are written in `format` instead of the worksheet's.
/// Display only: values and errors do not change. Used for a region with its own result format.
pub fn calculate_in_format(
    doc: &Document,
    revision: u64,
    opts: &CalcOptions,
    format: &ResultFormat,
) -> Calculation {
    let ws = doc.eval_worksheet();
    let table = doc.results().clone();
    let limits = limits_of(opts);
    let wanted = opts.regions.clone();
    let system = unit_system_of(doc);
    let format = format.clone();
    let run = on_big_stack(move || run_worksheet(&ws, &table, &limits, system, &format));
    let (mut results, notes, stopped) = run;
    if let Some(ids) = wanted {
        results.retain(|r| ids.contains(&r.region_id));
    }
    Calculation {
        revision,
        results,
        notes,
        stopped,
    }
}

/// Evaluates the expression `text` in the scope of `doc` (all its regions) without adding a
/// region. `doc` may be empty.
///
/// # Errors
/// A message when `text` does not read as an expression.
pub fn evaluate_expression(
    doc: &Document,
    text: &str,
    opts: &CalcOptions,
) -> Result<RegionResult, String> {
    let ctx = crate::context::EditContext::for_regions(doc.regions());
    let expr = crate::context::parse_math_in(text, &ctx)
        .map_err(|e| crate::locale::message("edit.cannot_read", &[&e.to_string()]))?;
    evaluate_scratch(doc, vec![expr], opts)
        .pop()
        .ok_or_else(|| "the expression was not evaluated".to_string())
}

/// Evaluates `exprs` after the whole document, as temporary regions that are not kept. One result
/// per expression, in order (fewer when the engine panicked).
pub fn evaluate_scratch(doc: &Document, exprs: Vec<Expr>, opts: &CalcOptions) -> Vec<RegionResult> {
    let mut ws = doc.worksheet();
    let first = doc
        .regions()
        .iter()
        .map(|r| r.id)
        .max()
        .map_or(1, |m| m.saturating_add(1));
    let ids: Vec<u32> = (0..exprs.len())
        .map(|i| first.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    for (id, expr) in ids.iter().zip(exprs) {
        // No position: the region is evaluated after every placed region, in this order.
        ws.regions.push(RegionModel {
            id: *id,
            pos: None,
            size: None,
            result_ref: None,
            kind: RegionKind::Math(expr),
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        });
    }
    let table = doc.results().clone();
    let limits = limits_of(opts);
    let system = unit_system_of(doc);
    let format = doc.settings().result_format.clone().unwrap_or_default();
    let (results, _, _) =
        on_big_stack(move || run_worksheet(&ws, &table, &limits, system, &format));
    ids.iter()
        .filter_map(|id| results.iter().find(|r| r.region_id == *id).cloned())
        .collect()
}

fn limits_of(opts: &CalcOptions) -> EvalLimits {
    let limits = EvalLimits::new(opts.deadline);
    match &opts.cancel {
        Some(flag) => limits.with_cancel(Arc::clone(flag)),
        None => limits,
    }
}

fn unit_system_of(doc: &Document) -> UnitSystem {
    doc.settings()
        .unit_system
        .as_deref()
        .and_then(UnitSystem::from_attr)
        .unwrap_or_default()
}

type Run = (Vec<RegionResult>, Vec<String>, Option<String>);

fn run_worksheet(
    ws: &Worksheet,
    table: &ResultTable,
    limits: &EvalLimits,
    system: UnitSystem,
    format: &ResultFormat,
) -> Run {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate_worksheet_limited(ws, table, FileOptions::disabled(), limits)
    }));
    match outcome {
        Ok(eval) => summarize(&eval, system, format),
        Err(payload) => (
            Vec::new(),
            vec![format!(
                "the engine failed: {}",
                panic_message(payload.as_ref())
            )],
            None,
        ),
    }
}

fn summarize(eval: &WorksheetEvaluation, system: UnitSystem, format: &ResultFormat) -> Run {
    let mut stopped = None;
    let results = eval
        .regions
        .iter()
        .map(|r| {
            let result = region_result(r, system, format);
            if result.status == ResultStatus::Skipped {
                if let Some(reason) = result.error.as_deref().filter(|e| is_stopped_reason(e)) {
                    stopped.get_or_insert_with(|| reason.to_string());
                }
            }
            result
        })
        .collect();
    (results, eval.notes.clone(), stopped)
}

fn region_result(r: &RegionEvaluation, system: UnitSystem, format: &ResultFormat) -> RegionResult {
    let mut out = RegionResult {
        region_id: r.region_id,
        expr: r.expr_text.clone(),
        status: ResultStatus::Defined,
        value: String::new(),
        error: None,
        defines: None,
    };
    match &r.outcome {
        EvalOutcome::Engine(outcome) => fill_outcome(&mut out, outcome, system, format),
        EvalOutcome::Plot(plot) => {
            let failed = plot.traces.iter().find_map(|t| t.error.as_ref());
            match failed {
                Some(e) => {
                    out.status = ResultStatus::Error;
                    out.error = Some(e.message.clone());
                }
                None => {
                    out.status = ResultStatus::Plot;
                    out.value = format!(
                        "{} traces, {} points",
                        plot.traces.len(),
                        plot.point_count()
                    );
                }
            }
        }
        EvalOutcome::Plot3d(plot) => {
            out.status = ResultStatus::Plot;
            out.value = format!("{} traces", plot.traces.len());
        }
        EvalOutcome::Panic(message) => {
            out.status = ResultStatus::Error;
            out.error = Some(format!("the engine failed: {message}"));
        }
        EvalOutcome::Skipped(reason) => {
            out.status = ResultStatus::Skipped;
            out.error = Some(reason.clone());
        }
    }
    out
}

fn fill_outcome(
    out: &mut RegionResult,
    outcome: &Outcome,
    system: UnitSystem,
    format: &ResultFormat,
) {
    match outcome {
        Outcome::Defined { name, display } => {
            out.defines = Some(name.clone());
            out.status = ResultStatus::Defined;
            if let Some(inner) = display {
                fill_outcome(out, inner, system, format);
                out.defines = Some(name.clone());
            }
        }
        Outcome::Value(v) => {
            out.status = ResultStatus::Value;
            out.value = value_text(v, format, system);
        }
        Outcome::Converted { unit, shown, .. } => {
            out.status = ResultStatus::Value;
            out.value = match shown {
                Value::Scalar(s) => with_unit(
                    &format_complex_with(
                        s.value.re,
                        s.value.im,
                        format,
                        &NumberSeparators::INVARIANT,
                    ),
                    &unit_text(unit),
                ),
                other => value_text(other, format, system),
            };
        }
        Outcome::Accepted => out.status = ResultStatus::Defined,
        Outcome::Error(e) => {
            out.status = ResultStatus::Error;
            out.error = Some(e.to_string());
        }
    }
}

/// The text of a value in the worksheet's unit system.
pub fn value_text(v: &Value, format: &ResultFormat, system: UnitSystem) -> String {
    match v {
        Value::Scalar(s) => scalar_text(s.value.re, s.value.im, &s.dim, format, system),
        Value::Matrix(m) => {
            let (rows, cols) = m.shape();
            let lines: Vec<String> = (0..rows)
                .map(|r| {
                    let cells: Vec<String> = (0..cols)
                        .map(|c| match m.get(r, c) {
                            Ok(s) => scalar_text(s.value.re, s.value.im, &s.dim, format, system),
                            Err(_) => String::new(),
                        })
                        .collect();
                    format!("[{}]", cells.join(", "))
                })
                .collect();
            format!("[{}]", lines.join(", "))
        }
        Value::Str(s) => format!("\"{s}\""),
        Value::List(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(|i| value_text(i, format, system))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Value::Symbolic(s) => s.shown.to_string(),
        Value::Func(_) => "function".to_string(),
    }
}

fn scalar_text(
    re: f64,
    im: f64,
    dim: &Dimension,
    format: &ResultFormat,
    system: UnitSystem,
) -> String {
    let (re, im, unit) = if dim.is_dimensionless() {
        (re, im, String::new())
    } else {
        match display_unit(dim, system) {
            Some(u) => (
                re / u.factor_to_si,
                im / u.factor_to_si,
                u.symbol.to_string(),
            ),
            None => (re, im, dimension_text(dim)),
        }
    };
    with_unit(
        &format_complex_with(re, im, format, &NumberSeparators::INVARIANT),
        &unit,
    )
}

fn with_unit(number: &str, unit: &str) -> String {
    if unit.is_empty() {
        number.to_string()
    } else {
        format!("{number} {unit}")
    }
}

/// Coherent SI derived units with their own names (see the GUI's result text).
const NAMED_DERIVED: [&str; 12] = ["N", "Pa", "J", "W", "C", "V", "Ω", "F", "S", "T", "Wb", "H"];

fn dimension_text(dim: &Dimension) -> String {
    let named = units::units().iter().find(|u| {
        u.factor_to_si == 1.0
            && u.offset_to_si.is_none()
            && NAMED_DERIVED.contains(&u.symbol)
            && u.dimension == *dim
    });
    match named {
        Some(unit) => unit.symbol.to_string(),
        None => dim.to_string().replace('*', "·"),
    }
}

fn unit_text(unit: &CompoundUnit) -> String {
    unit.display_text()
}

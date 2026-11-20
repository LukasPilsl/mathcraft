//! Golden run: loads each worksheet, evaluates it with `mathcraft_import::evaluate` and classifies
//! each region with `mathcraft_import::compare::classify`. This file only collects the rows the
//! report prints.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use mathcraft_import::compare::{classify, Category};
use mathcraft_import::describe::describe_stored;
use mathcraft_import::evaluate::{
    evaluate_worksheet, load_results, on_big_stack, panic_message, EvalOutcome, RegionEvaluation,
};
use mathcraft_import::package::Package;
use mathcraft_import::regions::Worksheet;

use crate::common::file_name;
use crate::render::describe_outcome;

/// One classified region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionRow {
    pub region_id: u32,
    /// The region is a 2D plot. Its category comes from the comparison of its traces with the
    /// stored `<Trace2dResult>` data (see `mathcraft_import::compare::classify`).
    pub plot: bool,
    /// The expression text, or a short description of a region that is not run.
    pub expr: String,
    pub category: Category,
    /// The stored result, in full precision, when there is one.
    pub expected: Option<String>,
    /// The computed outcome, as [`describe_outcome`] prints it.
    pub actual: Option<String>,
}

/// The golden result of one worksheet.
#[derive(Debug, Clone, PartialEq)]
pub struct FileReport {
    pub name: String,
    /// Set when the file could not be loaded or its stored results could not be read. Its
    /// regions are then absent.
    pub file_error: Option<String>,
    /// Problems that did not stop the run, such as a setting that could not be applied.
    pub notes: Vec<String>,
    pub rows: Vec<RegionRow>,
}

impl FileReport {
    pub fn tally(&self) -> Tally {
        let mut tally = Tally::default();
        for row in &self.rows {
            tally.add_row(row);
        }
        tally
    }
}

/// The golden result over several worksheets.
#[derive(Debug, Clone, PartialEq)]
pub struct GoldenRun {
    pub files: Vec<FileReport>,
}

impl GoldenRun {
    /// Total of all files.
    pub fn tally(&self) -> Tally {
        let mut total = Tally::default();
        for file in &self.files {
            for row in &file.rows {
                total.add_row(row);
            }
        }
        total
    }

    /// Mismatches, engine errors (with and without a reference) and file errors. `--strict`
    /// fails when this is not zero.
    pub fn failure_count(&self) -> usize {
        let t = self.tally();
        t.mismatch
            + t.engine_error
            + t.engine_error_no_ref
            + self.files.iter().filter(|f| f.file_error.is_some()).count()
    }
}

/// Counts of categories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub pass: usize,
    pub mismatch: usize,
    /// Passes that accept any engine error for a stored error code other than `bad%_variable`.
    /// Also part of `pass`.
    pub weak_pass: usize,
    /// Count per unsupported reason.
    pub unsupported: BTreeMap<String, usize>,
    pub engine_error: usize,
    /// Engine errors on regions without a stored reference. Never part of `no_reference`.
    pub engine_error_no_ref: usize,
    pub no_reference: usize,
    /// 2D plots that evaluated completely but have no stored data to compare (`PlotOk`).
    pub plot_ok: usize,
    /// Plot regions among `pass` (every trace compared with Prime's stored series).
    pub plot_pass: usize,
    /// Plot regions among `mismatch`: a stored trace differs from the computed series.
    pub plot_mismatch: usize,
}

impl Tally {
    /// Adds one row, counting its plot split as well.
    pub fn add_row(&mut self, row: &RegionRow) {
        self.add(&row.category);
        if row.plot {
            match row.category {
                Category::Pass => self.plot_pass += 1,
                Category::Mismatch(_) => self.plot_mismatch += 1,
                _ => {}
            }
        }
    }

    pub fn add(&mut self, category: &Category) {
        match category {
            Category::Pass => self.pass += 1,
            Category::WeakPass(_) => {
                self.pass += 1;
                self.weak_pass += 1;
            }
            Category::Mismatch(_) => self.mismatch += 1,
            Category::Unsupported(reason) => {
                *self.unsupported.entry(reason.clone()).or_insert(0) += 1;
            }
            Category::EngineError(_) => self.engine_error += 1,
            Category::EngineErrorNoRef(_) => self.engine_error_no_ref += 1,
            Category::NoReference => self.no_reference += 1,
            Category::PlotOk(_) => self.plot_ok += 1,
        }
    }

    /// Total of all unsupported reasons.
    pub fn unsupported_total(&self) -> usize {
        self.unsupported.values().sum()
    }
}

/// Runs the golden check over `files`. The whole run is on a thread with a big stack (see
/// [`mathcraft_import::evaluate::STACK_SIZE`]). A panic inside one file is reported as that
/// file's error and does not stop the run.
pub fn golden(files: &[PathBuf]) -> GoldenRun {
    let files = files.to_vec();
    on_big_stack(move || GoldenRun {
        files: files.iter().map(|p| run_file(p)).collect(),
    })
}

fn run_file(path: &Path) -> FileReport {
    let name = file_name(path);
    match catch_unwind(AssertUnwindSafe(|| run_worksheet(path))) {
        Ok(Ok((notes, rows))) => FileReport {
            name,
            file_error: None,
            notes,
            rows,
        },
        Ok(Err(message)) => FileReport {
            name,
            file_error: Some(message),
            notes: Vec::new(),
            rows: Vec::new(),
        },
        Err(payload) => FileReport {
            name,
            file_error: Some(format!("Panic while loading: {}", panic_message(&*payload))),
            notes: Vec::new(),
            rows: Vec::new(),
        },
    }
}

fn run_worksheet(path: &Path) -> Result<(Vec<String>, Vec<RegionRow>), String> {
    let pkg = Package::open(path).map_err(|e| format!("open: {e}"))?;
    let ws = Worksheet::from_package(&pkg).map_err(|e| format!("import: {e}"))?;
    let table = load_results(&pkg)?;
    let evaluation = evaluate_worksheet(&ws, &table);
    let rows = evaluation.regions.iter().map(row_of).collect();
    Ok((evaluation.notes, rows))
}

fn row_of(eval: &RegionEvaluation) -> RegionRow {
    RegionRow {
        region_id: eval.region_id,
        plot: matches!(eval.outcome, EvalOutcome::Plot(_) | EvalOutcome::Plot3d(_)),
        expr: eval.expr_text.clone(),
        category: classify(eval),
        expected: eval.stored.as_ref().map(|d| describe_stored(&d.value)),
        actual: actual_text(&eval.outcome),
    }
}

/// The actual column: the engine's outcome, the panic message, or nothing for a skipped region.
fn actual_text(outcome: &EvalOutcome) -> Option<String> {
    match outcome {
        EvalOutcome::Engine(outcome) => Some(describe_outcome(outcome)),
        EvalOutcome::Panic(message) => Some(format!("Panic: {message}")),
        EvalOutcome::Plot(plot) => Some(format!(
            "plot: {} traces, {} points",
            plot.traces.len(),
            plot.point_count()
        )),
        EvalOutcome::Plot3d(plot) => Some(format!(
            "3D plot: {} traces, {} grid points",
            plot.traces.len(),
            plot.traces.iter().map(|t| t.rows * t.cols).sum::<usize>()
        )),
        EvalOutcome::Skipped(_) => None,
    }
}

#[cfg(test)]
#[path = "tests/golden.rs"]
mod tests;

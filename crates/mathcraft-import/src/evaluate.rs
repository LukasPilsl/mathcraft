//! Worksheet evaluation, shared by the CLI golden harness and the GUI.
//!
//! [`evaluate_worksheet`] runs every region of a sheet, in evaluation order, through ONE
//! [`Session`]. Solve blocks run as a unit. Panics are caught per region. It classifies nothing:
//! each [`RegionEvaluation`] carries the engine's outcome and the stored result.
//!
//! GUI use, three steps:
//! 1. Load with `Package::open`, `Worksheet::from_package` and [`load_results`].
//! 2. Call [`evaluate_worksheet_on_big_stack`] (or [`evaluate_worksheet`] on a thread you already
//!    run with a large stack).
//! 3. Colour each region with `compare::classify(&row)`, and show `expr_text`, `stored` and
//!    `outcome`.

use std::collections::{BTreeMap, HashMap};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mathcraft_core::eval::builtins::fileio::FileOptions;
use mathcraft_core::eval::{BlockMember, MemberKind, Outcome, Session, WORKSHEET_MAX_WORK};
use mathcraft_core::expr::{Expr, Id, IdKind};

use crate::evaluate_plots::{evaluate_plot, PlotData};
use crate::evaluate_plots3d::{evaluate_plot3d, Plot3dData};
use crate::package::Package;
use crate::regions::{
    Plot2d, Plot3d, PlotInfo, RegionKind, RegionModel, Settings, Step, Worksheet,
};
use crate::results::{parse_results, trace3d_of, ResultData, ResultTable, ResultValue};

/// Stack size of the thread that runs the engine. Matches the main thread's default on macOS
/// and keeps deep recursion in the evaluator from overflowing.
pub const STACK_SIZE: usize = 8 * 1024 * 1024;

/// Wall-clock time that one worksheet evaluation may take. Each top-level statement is bounded by
/// its own work budget (`MAX_WORK`); this bounds the whole sheet, so that many expensive regions
/// cannot add up to an unresponsive window or a hung golden run.
pub const EVALUATION_DEADLINE: Duration = Duration::from_secs(60);

/// Start of the reason text of every region that is not run because a limit was hit or the
/// evaluation was cancelled. The window shows "evaluation stopped" for these (see
/// [`is_stopped_reason`]).
pub const STOPPED_REASON: &str = "evaluation stopped";

/// Whether a `Skipped` reason says the region was stopped by a limit or a cancel, rather than
/// being unsupported.
pub fn is_stopped_reason(reason: &str) -> bool {
    reason.starts_with(STOPPED_REASON)
}

/// The limits of one worksheet evaluation: a deadline, a cap on the work the sheet spends, and an
/// optional cancel flag. All are checked between regions (and between solve blocks). A region that
/// has started runs to its own budget. Regions reached after a limit is hit are not run; each
/// reports [`EvalOutcome::Skipped`] with the reason.
#[derive(Debug, Clone)]
pub struct EvalLimits {
    deadline: Instant,
    cancel: Option<Arc<AtomicBool>>,
}

impl EvalLimits {
    /// Limits that stop the evaluation `duration` from now.
    pub fn new(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            cancel: None,
        }
    }

    /// The same limits, also stopped when `cancel` is set (with `Ordering::Relaxed` loads).
    #[must_use]
    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Why no further region may run, or `None` while the evaluation may go on. `work_spent` is
    /// the work the session has charged so far (see `Session::work_spent`).
    fn stop_reason(&self, work_spent: u64) -> Option<&'static str> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Some("evaluation stopped: cancelled");
        }
        if Instant::now() >= self.deadline {
            return Some("evaluation stopped: the worksheet took too long (limit reached)");
        }
        if work_spent >= WORKSHEET_MAX_WORK {
            return Some("evaluation stopped: the worksheet used too much work (limit reached)");
        }
        None
    }
}

/// What happened to one region.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalOutcome {
    /// The engine's outcome: `Defined`, `Value`, `Converted`, `Accepted` or `Error`.
    Engine(Box<Outcome>),
    /// A 2D plot, evaluated (see [`crate::evaluate_plots`]). Its traces may each carry an error.
    Plot(Box<PlotData>),
    /// A 3D plot, evaluated (see [`crate::evaluate_plots3d`]).
    Plot3d(Box<Plot3dData>),
    /// The engine panicked on this region. The text is the panic message.
    Panic(String),
    /// The region is not run. The text is the reason.
    Skipped(String),
}

/// How a region took part in evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Normal,
    /// A guess (`solve-block-category` `guess-value`).
    SolveGuess,
    /// A constraint (`solve-block-category` `constraint`, or any other solve category).
    SolveConstraint,
    /// The solver call (`solve-block-category` `solver`).
    SolveSolver,
}

/// One region after evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionEvaluation {
    pub region_id: u32,
    /// The expression text, or a short description of a region that is not run.
    pub expr_text: String,
    pub outcome: EvalOutcome,
    /// Prime's stored result for this region, when there is one.
    pub stored: Option<ResultData>,
    pub step_kind: StepKind,
}

/// The evaluation of one worksheet.
#[derive(Debug, Clone, PartialEq)]
pub struct WorksheetEvaluation {
    /// One entry per evaluated region, in evaluation order. Text regions and solve-block
    /// containers are not listed.
    pub regions: Vec<RegionEvaluation>,
    /// Problems that did not stop the run, such as a setting that could not be applied.
    pub notes: Vec<String>,
}

/// Reads the sheet's `result.xml`. A package without one has no stored results.
pub fn load_results(pkg: &Package) -> Result<ResultTable, String> {
    match pkg.result.as_deref() {
        Some(xml) => parse_results(xml).map_err(|e| format!("result.xml: {e}")),
        None => Ok(ResultTable {
            by_id: BTreeMap::new(),
        }),
    }
}

/// Evaluates every region of `ws`, joining each with its stored result from `table`. The file
/// functions have no access (see [`evaluate_worksheet_in`]).
pub fn evaluate_worksheet(ws: &Worksheet, table: &ResultTable) -> WorksheetEvaluation {
    evaluate_worksheet_in(ws, table, None)
}

/// [`evaluate_worksheet`] with the file functions given `base`, the directory of the worksheet
/// (relative names only; absolute names and symbolic links are refused). Access is only available
/// under `base`, and both switches are off, so the file functions refuse to read or write: a caller
/// that wants them passes explicit options to [`evaluate_worksheet_with`]. `None` leaves file access
/// disabled, and so does a `base` that is not a directory.
pub fn evaluate_worksheet_in(
    ws: &Worksheet,
    table: &ResultTable,
    base: Option<&Path>,
) -> WorksheetEvaluation {
    evaluate_worksheet_in_limited(ws, table, base, &EvalLimits::new(EVALUATION_DEADLINE))
}

/// [`evaluate_worksheet_in`] under explicit `limits` (see [`evaluate_worksheet_limited`]).
pub fn evaluate_worksheet_in_limited(
    ws: &Worksheet,
    table: &ResultTable,
    base: Option<&Path>,
    limits: &EvalLimits,
) -> WorksheetEvaluation {
    let files = base
        .and_then(|dir| FileOptions::real_under(dir).ok())
        .unwrap_or_else(FileOptions::disabled);
    evaluate_worksheet_limited(ws, table, files, limits)
}

/// [`evaluate_worksheet`] with explicit file options: the access, the policy, the switches and a
/// fresh per-evaluation quota. Use one `FileOptions` per evaluation. The sheet is limited to
/// [`EVALUATION_DEADLINE`] in wall-clock time (see [`evaluate_worksheet_limited`]).
pub fn evaluate_worksheet_with(
    ws: &Worksheet,
    table: &ResultTable,
    files: FileOptions,
) -> WorksheetEvaluation {
    evaluate_worksheet_limited(ws, table, files, &EvalLimits::new(EVALUATION_DEADLINE))
}

/// [`evaluate_worksheet_with`] under explicit `limits`. Regions reached after the deadline, after
/// the cancel flag is set, or after the sheet has spent [`WORKSHEET_MAX_WORK`] units in all, are not
/// run: each reports `Skipped` with the reason (see [`is_stopped_reason`]).
pub fn evaluate_worksheet_limited(
    ws: &Worksheet,
    table: &ResultTable,
    files: FileOptions,
    limits: &EvalLimits,
) -> WorksheetEvaluation {
    let stored: HashMap<u32, &ResultData> = ws.attach_stored_results(table).into_iter().collect();
    let mut session = Session::with_file_options(files);
    let notes = configure(&mut session, &ws.settings);
    let mut regions = Vec::new();
    for step in ws.evaluation_steps() {
        if let Some(reason) = limits.stop_reason(session.work_spent()) {
            regions.extend(stopped_evaluations(&step, table, &stored, reason));
            continue;
        }
        match step {
            Step::Region(region) => {
                if let RegionKind::Table(model) = &region.kind {
                    regions.extend(table_evaluations(region, model, &mut session, table));
                    continue;
                }
                let stored = stored.get(&region.id).copied();
                if let Some(eval) = region_evaluation(region, &mut session, stored, table) {
                    regions.push(eval);
                }
            }
            Step::SolveBlock { members, .. } => {
                regions.extend(solve_block_evaluations(&members, &mut session, &stored));
            }
        }
    }
    WorksheetEvaluation { regions, notes }
}

/// [`evaluate_worksheet`] on a thread with a [`STACK_SIZE`] stack. A panic that escapes the
/// evaluation is resumed on the caller.
pub fn evaluate_worksheet_on_big_stack(ws: &Worksheet, table: &ResultTable) -> WorksheetEvaluation {
    evaluate_worksheet_on_big_stack_in(ws, table, None)
}

/// [`evaluate_worksheet_in`] on a thread with a [`STACK_SIZE`] stack.
pub fn evaluate_worksheet_on_big_stack_in(
    ws: &Worksheet,
    table: &ResultTable,
    base: Option<&Path>,
) -> WorksheetEvaluation {
    let (ws, table) = (ws.clone(), table.clone());
    let base = base.map(Path::to_path_buf);
    on_big_stack(move || evaluate_worksheet_in(&ws, &table, base.as_deref()))
}

/// Runs `f` on a thread with a [`STACK_SIZE`] stack and returns its result. A panic in `f` is
/// resumed on the caller.
pub fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let handle = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(f)
        .expect("spawn evaluation thread");
    match handle.join() {
        Ok(value) => value,
        Err(payload) => resume_unwind(payload),
    }
}

/// The message of a panic payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

/// Evaluates one region, or returns `None` for regions that are not part of the report (text,
/// and the solve block containers).
fn region_evaluation(
    region: &RegionModel,
    session: &mut Session,
    stored: Option<&ResultData>,
    table: &ResultTable,
) -> Option<RegionEvaluation> {
    let step_kind = step_kind_for(region);
    let skipped = |reason: &str| {
        Some(evaluation(
            region,
            EvalOutcome::Skipped(reason.to_string()),
            stored,
            step_kind,
        ))
    };
    match &region.kind {
        // A table is evaluated by `table_evaluations`, one entry per member.
        RegionKind::Text(_)
        | RegionKind::SolveBlock(_)
        | RegionKind::Table(_)
        | RegionKind::Area(_)
        | RegionKind::Image(_)
        | RegionKind::PageBreak => None,
        _ if region.solve_category.is_some() => skipped("solve block member"),
        RegionKind::Plot(info) => {
            let outcome = plot_outcome(info, session, table);
            Some(evaluation(region, outcome, stored, step_kind))
        }
        RegionKind::Unsupported { reason } => skipped(reason),
        RegionKind::Math(expr) => {
            let outcome = match catch_unwind(AssertUnwindSafe(|| session.run(expr))) {
                Ok(outcome) => EvalOutcome::Engine(Box::new(outcome)),
                Err(payload) => EvalOutcome::Panic(panic_message(&*payload)),
            };
            Some(evaluation(region, outcome, stored, step_kind))
        }
    }
}

/// One evaluation per member of a table region. A table is only a presentation of its defines, so
/// each member runs exactly like a define region, in document order, and is joined with its own
/// stored result (its `resultRef`). A member that could not be converted is skipped with its
/// reason. Every entry carries the table's region ID.
fn table_evaluations(
    region: &RegionModel,
    model: &crate::regions::TableModel,
    session: &mut Session,
    table: &ResultTable,
) -> Vec<RegionEvaluation> {
    model
        .members
        .iter()
        .map(|member| {
            let stored = member
                .result_ref
                .and_then(|id| table.by_id.get(&id))
                .cloned();
            let (expr_text, outcome) = match &member.expr {
                Ok(expr) => {
                    let outcome = match catch_unwind(AssertUnwindSafe(|| session.run(expr))) {
                        Ok(outcome) => EvalOutcome::Engine(Box::new(outcome)),
                        Err(payload) => EvalOutcome::Panic(panic_message(&*payload)),
                    };
                    (expr.to_string(), outcome)
                }
                Err(reason) => (
                    "unconvertible table member".to_string(),
                    EvalOutcome::Skipped(reason.clone()),
                ),
            };
            RegionEvaluation {
                region_id: region.id,
                expr_text,
                outcome,
                stored,
                step_kind: StepKind::Normal,
            }
        })
        .collect()
}

/// The outcome of one plot region: its evaluation in the session's environment, or the reason it
/// is not supported (other plot types have no model).
fn plot_outcome(info: &PlotInfo, session: &Session, table: &ResultTable) -> EvalOutcome {
    if let Some(model) = &info.plot3d {
        return match catch_unwind(AssertUnwindSafe(|| evaluate_plot3d(model, session.env()))) {
            Ok(mut data) => {
                attach_stored_surfaces(&mut data, model, table);
                EvalOutcome::Plot3d(Box::new(data))
            }
            Err(payload) => EvalOutcome::Panic(panic_message(&*payload)),
        };
    }
    let Some(model) = &info.plot else {
        return EvalOutcome::Skipped(format!("plot type <{}> is not supported yet", info.kind));
    };
    match catch_unwind(AssertUnwindSafe(|| evaluate_plot(model, session.env()))) {
        Ok(mut data) => {
            attach_stored_traces(&mut data, model, table);
            EvalOutcome::Plot(Box::new(data))
        }
        Err(payload) => EvalOutcome::Panic(panic_message(&*payload)),
    }
}

/// Gives each trace of an evaluated plot the `<Trace2dResult>` that Prime stored for it (the
/// trace's `resultRef`). A trace whose entry is missing or is not plot data keeps `None`.
fn attach_stored_traces(data: &mut PlotData, model: &Plot2d, table: &ResultTable) {
    for (trace, model_trace) in data.traces.iter_mut().zip(&model.traces) {
        let Some(id) = model_trace.result_ref else {
            continue;
        };
        if let Some(ResultData {
            value: ResultValue::Trace2d(stored),
            ..
        }) = table.by_id.get(&id)
        {
            trace.stored = Some(stored.points());
        }
    }
}

/// Gives each trace of an evaluated 3D plot the stored 3D data of its `resultRef`, when the entry
/// has that shape (see `results::trace3d_of`). Other entries keep `None`.
fn attach_stored_surfaces(data: &mut Plot3dData, model: &Plot3d, table: &ResultTable) {
    for (trace, model_trace) in data.traces.iter_mut().zip(&model.traces) {
        let Some(id) = model_trace.result_ref else {
            continue;
        };
        if let Some(ResultData { value, .. }) = table.by_id.get(&id) {
            if let Some(stored) = trace3d_of(value) {
                trace.stored = Some(Ok(stored));
            }
        }
    }
}

/// Runs one solve block as a unit and returns one evaluation per member, in order.
fn solve_block_evaluations(
    members: &[&RegionModel],
    session: &mut Session,
    stored: &HashMap<u32, &ResultData>,
) -> Vec<RegionEvaluation> {
    let block: Vec<BlockMember> = members.iter().map(|m| block_member(m)).collect();
    let outcomes: Vec<EvalOutcome> =
        match catch_unwind(AssertUnwindSafe(|| session.run_solve_block(&block))) {
            Ok(outcomes) => outcomes
                .into_iter()
                .map(|o| EvalOutcome::Engine(Box::new(o)))
                .collect(),
            Err(payload) => {
                let message = panic_message(&*payload);
                members
                    .iter()
                    .map(|_| EvalOutcome::Panic(message.clone()))
                    .collect()
            }
        };
    members
        .iter()
        .zip(outcomes)
        .map(|(member, outcome)| {
            let step_kind = step_kind_of(member_kind(member));
            evaluation(member, outcome, stored.get(&member.id).copied(), step_kind)
        })
        .collect()
}

fn evaluation(
    region: &RegionModel,
    outcome: EvalOutcome,
    stored: Option<&ResultData>,
    step_kind: StepKind,
) -> RegionEvaluation {
    RegionEvaluation {
        region_id: region.id,
        expr_text: region_text(region),
        outcome,
        stored: stored.cloned(),
        step_kind,
    }
}

/// The step kind of a region: a solve-block member's kind, or `Normal` for a region run on its own.
fn step_kind_for(region: &RegionModel) -> StepKind {
    match region.solve_category {
        Some(_) => step_kind_of(member_kind(region)),
        None => StepKind::Normal,
    }
}

/// One `Skipped` evaluation per region of `step` that would have been reported, with `reason`. The
/// regions keep their stored results, so the window still shows what Prime stored.
fn stopped_evaluations(
    step: &Step,
    table: &ResultTable,
    stored: &HashMap<u32, &ResultData>,
    reason: &str,
) -> Vec<RegionEvaluation> {
    let skipped = || EvalOutcome::Skipped(reason.to_string());
    match step {
        Step::Region(region) => match &region.kind {
            RegionKind::Text(_)
            | RegionKind::SolveBlock(_)
            | RegionKind::Area(_)
            | RegionKind::Image(_) => Vec::new(),
            RegionKind::Table(model) => model
                .members
                .iter()
                .map(|member| RegionEvaluation {
                    region_id: region.id,
                    expr_text: match &member.expr {
                        Ok(expr) => expr.to_string(),
                        Err(_) => "unconvertible table member".to_string(),
                    },
                    outcome: skipped(),
                    stored: member
                        .result_ref
                        .and_then(|id| table.by_id.get(&id))
                        .cloned(),
                    step_kind: StepKind::Normal,
                })
                .collect(),
            _ => vec![evaluation(
                region,
                skipped(),
                stored.get(&region.id).copied(),
                step_kind_for(region),
            )],
        },
        Step::SolveBlock { members, .. } => members
            .iter()
            .map(|member| {
                evaluation(
                    member,
                    skipped(),
                    stored.get(&member.id).copied(),
                    step_kind_for(member),
                )
            })
            .collect(),
    }
}

fn step_kind_of(kind: MemberKind) -> StepKind {
    match kind {
        MemberKind::Guess => StepKind::SolveGuess,
        MemberKind::Constraint => StepKind::SolveConstraint,
        MemberKind::Solver => StepKind::SolveSolver,
    }
}

/// The solve-block kind of a region: from `solve-block-category`, and a constraint by default.
fn member_kind(region: &RegionModel) -> MemberKind {
    match region.solve_category.as_deref() {
        Some("guess-value") => MemberKind::Guess,
        Some("solver") => MemberKind::Solver,
        _ => MemberKind::Constraint,
    }
}

/// The solve-block member of a region: its kind, and its statement or the reason it cannot be
/// run.
fn block_member(region: &RegionModel) -> BlockMember {
    let body = match (&region.kind, region.solve_category.as_deref()) {
        (RegionKind::Math(expr), Some("guess-value" | "constraint" | "solver")) => Ok(expr.clone()),
        (RegionKind::Unsupported { reason }, _) => Err(reason.clone()),
        (_, Some("guess-value" | "constraint" | "solver")) => {
            Err(format!("{} in a solve block", region_text(region)))
        }
        (_, other) => Err(format!("solve block member of category {other:?}")),
    };
    BlockMember {
        kind: member_kind(region),
        body,
    }
}

/// Sets `ORIGIN`, `TOL` and `CTOL` from the sheet's settings, by running ordinary definitions in
/// the session. Returns a note for each one that failed.
fn configure(session: &mut Session, settings: &Settings) -> Vec<String> {
    let mut notes = Vec::new();
    let mut define = |name: &str, value: f64| {
        let stmt = Expr::Define {
            target: Box::new(Expr::Id(Id::new(name, IdKind::Variable))),
            expr: Box::new(Expr::num(value)),
        };
        if let Outcome::Error(e) = session.run(&stmt) {
            notes.push(format!("setting {name} not applied: {e}"));
        }
    };
    if let Some(origin) = settings.array_origin {
        define("ORIGIN", f64::from(origin));
    }
    if let Some(tol) = settings.convergence_tolerance {
        define("TOL", tol);
    }
    if let Some(tol) = settings.constraint_tolerance {
        define("CTOL", tol);
    }
    notes
}

/// Short description of a region that is not run, for the expression text.
fn region_text(region: &RegionModel) -> String {
    match &region.kind {
        RegionKind::Math(expr) => expr.to_string(),
        RegionKind::Plot(plot) => format!("plot ({})", plot.kind),
        RegionKind::Unsupported { .. } => "unsupported element".to_string(),
        RegionKind::Text(_) => "text".to_string(),
        RegionKind::SolveBlock(_) => "solve block".to_string(),
        RegionKind::Table(model) => format!("table ({} members)", model.members.len()),
        RegionKind::Area(_) => "area".to_string(),
        RegionKind::Image(_) => "image".to_string(),
        RegionKind::PageBreak => "page break".to_string(),
    }
}

#[cfg(test)]
mod limit_tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::Duration;

    use mathcraft_core::eval::MAX_WORK;

    use super::*;
    use crate::regions::{ImportReport, Settings, Worksheet};

    /// A region that is never run: its outcome is its own `Skipped` reason.
    fn unsupported(id: u32) -> RegionModel {
        RegionModel {
            id,
            pos: Some((0.0, 0.0)),
            size: None,
            result_ref: None,
            kind: RegionKind::Unsupported {
                reason: "test reason".to_string(),
            },
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        }
    }

    fn sheet() -> Worksheet {
        Worksheet {
            regions: vec![unsupported(1), unsupported(2)],
            settings: Settings::default(),
            page: Default::default(),
            report: ImportReport::default(),
            header: Vec::new(),
            footer: Vec::new(),
        }
    }

    fn empty_table() -> ResultTable {
        ResultTable {
            by_id: BTreeMap::new(),
        }
    }

    fn run(limits: &EvalLimits) -> Vec<String> {
        let eval =
            evaluate_worksheet_limited(&sheet(), &empty_table(), FileOptions::disabled(), limits);
        eval.regions
            .iter()
            .map(|r| match &r.outcome {
                EvalOutcome::Skipped(reason) => reason.clone(),
                other => format!("not skipped: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_sheet_within_its_limits_runs_every_region() {
        let limits = EvalLimits::new(Duration::from_secs(60));
        assert_eq!(run(&limits), ["test reason", "test reason"]);
    }

    #[test]
    fn an_expired_deadline_stops_the_remaining_regions() {
        let reasons = run(&EvalLimits::new(Duration::ZERO));
        assert_eq!(reasons.len(), 2, "every region is still reported");
        assert!(reasons.iter().all(|r| is_stopped_reason(r)));
    }

    #[test]
    fn a_set_cancel_flag_stops_the_remaining_regions() {
        let cancel = Arc::new(AtomicBool::new(true));
        let limits = EvalLimits::new(Duration::from_secs(60)).with_cancel(cancel);
        assert_eq!(
            run(&limits),
            [
                "evaluation stopped: cancelled",
                "evaluation stopped: cancelled"
            ]
        );
    }

    #[test]
    fn a_clear_cancel_flag_does_not_stop_the_evaluation() {
        let cancel = Arc::new(AtomicBool::new(false));
        let limits = EvalLimits::new(Duration::from_secs(60)).with_cancel(cancel);
        assert_eq!(run(&limits), ["test reason", "test reason"]);
    }

    /// A statement `x<k> := cheby(1000000, 0.5)`. Its window is charged `5 (1 + 1e6)^2` units, over
    /// the per-statement budget, so the charge is refused before the window is built: each
    /// statement spends its whole budget at once, and runs in microseconds.
    fn hostile_statement(k: usize) -> Expr {
        Expr::Define {
            target: Box::new(Expr::Id(Id::new(&format!("x{k}"), IdKind::Variable))),
            expr: Box::new(Expr::Call {
                func: Id::new("cheby", IdKind::Function),
                args: vec![Expr::num(1_000_000.0), Expr::num(0.5)],
            }),
        }
    }

    fn math_region(id: u32, expr: Expr) -> RegionModel {
        RegionModel {
            kind: RegionKind::Math(expr),
            ..unsupported(id)
        }
    }

    #[test]
    fn a_hostile_sheet_is_stopped_after_its_cumulative_work_cap() {
        const STATEMENTS: usize = 100;
        let regions = (0..STATEMENTS)
            .map(|k| math_region(k as u32 + 1, hostile_statement(k)))
            .collect();
        let ws = Worksheet {
            regions,
            settings: Settings::default(),
            page: Default::default(),
            header: Vec::new(),
            footer: Vec::new(),
            report: ImportReport::default(),
        };
        let started = std::time::Instant::now();
        let eval = evaluate_worksheet_limited(
            &ws,
            &empty_table(),
            FileOptions::disabled(),
            &EvalLimits::new(Duration::from_secs(60)),
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the sheet stops early"
        );
        assert_eq!(
            eval.regions.len(),
            STATEMENTS,
            "every region is still reported"
        );
        // The cap is 50 x MAX_WORK: the first 50 statements each spend one budget.
        let cap_statements = (WORKSHEET_MAX_WORK / MAX_WORK) as usize;
        for (k, r) in eval.regions.iter().enumerate() {
            if k < cap_statements {
                assert!(
                    matches!(&r.outcome, EvalOutcome::Engine(o) if matches!(**o, Outcome::Error(_))),
                    "statement {k} runs and fails on its work budget: {:?}",
                    r.outcome
                );
            } else {
                let EvalOutcome::Skipped(reason) = &r.outcome else {
                    panic!("statement {k} must be skipped: {:?}", r.outcome);
                };
                assert!(is_stopped_reason(reason), "{reason}");
                assert!(reason.contains("too much work"), "{reason}");
            }
        }
    }

    #[test]
    fn a_sheet_under_the_work_cap_runs_every_statement() {
        let ws = Worksheet {
            regions: vec![math_region(1, hostile_statement(0))],
            settings: Settings::default(),
            page: Default::default(),
            header: Vec::new(),
            footer: Vec::new(),
            report: ImportReport::default(),
        };
        let eval = evaluate_worksheet_limited(
            &ws,
            &empty_table(),
            FileOptions::disabled(),
            &EvalLimits::new(Duration::from_secs(60)),
        );
        assert!(matches!(eval.regions[0].outcome, EvalOutcome::Engine(_)));
    }
}

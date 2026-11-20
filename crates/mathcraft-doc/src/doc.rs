//! [`Doc`]: a worksheet open for editing, with the undo history, the file it is stored in, and the
//! last calculation. The GUI's `EditSession` and a headless caller (the MCP server) use the same
//! methods, so they make the same edits.
//!
//! Region ids are handed out by the document and never reused in a session. An id stays with its
//! region through edits, moves, deletes and undo or redo (undoing a delete brings the region back
//! under the same id), so a caller can hold ids across calls.
//!
//! Every edit is one undo step. Edits that are not commands (page settings, calculation settings,
//! a region's result format, the header and footer) mark the document changed but are not in the
//! undo history, as in the GUI.

use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use mathcraft_core::eval::{MatrixDisplay, ResultFormat};
use mathcraft_core::regular_file::{read_regular_file, RegularFileError};
use mathcraft_import::evaluate::{load_results, on_big_stack};
use mathcraft_import::package::Package;
use mathcraft_import::page::PageSettings;
use mathcraft_import::regions::{
    AreaModel, RegionKind, RegionModel, Settings, Worksheet, MAX_REGIONS,
};
use mathcraft_import::rich_text::TextBody;
use mathcraft_import::writer::write_package_with_report;

use crate::calc::{self, CalcOptions, Calculation, RegionResult};
use crate::command::{
    moved_position, Batch, Command, EditError, History, InsertRegion, MoveRegion, RemoveRegion,
    ReplaceRegion,
};
use crate::context::{parse_math_in, EditContext};
use crate::document::Document;
use crate::extent::{lowest_bottom, region_height};
use crate::locale::message;
use crate::plot::{self, PlotOp, PlotSpec};
use crate::read::{self, Problem, RegionDetail, RegionSummary, Variable};

mod blocks;
pub use blocks::{normalise_equation, SolveBlockIds, SolveBlockSpec, SOLVER_NAMES};

/// Largest `.mcdx` file opened, in bytes.
pub const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;

/// Vertical space left between a new region and the one above it, in page points.
const GAP_PT: f64 = 12.0;
/// Size of a new empty area, in page points.
const AREA_SIZE: (f64, f64) = (360.0, 120.0);

/// Why a call did not do what it was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum DocError {
    /// No top-level region has this id.
    NotFound(u32),
    /// The region is not of the kind the call works on (math, text, 2D plot).
    NotEditable(u32),
    /// The text does not read, or a plot value is not valid. The text says why.
    Invalid(String),
    /// The worksheet already has [`MAX_REGIONS`] regions.
    TooManyRegions,
    /// The region is in a locked area.
    Locked(u32),
    /// A file could not be read or written, or is not a worksheet.
    File(String),
    /// The document is not in the state a command expects.
    Edit(EditError),
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocError::NotFound(id) => {
                write!(f, "{}", message("edit.not_found", &[&id.to_string()]))
            }
            DocError::NotEditable(_) => write!(f, "{}", message("edit.not_editable", &[])),
            DocError::Invalid(text) | DocError::File(text) => write!(f, "{text}"),
            DocError::TooManyRegions => write!(f, "{}", message("edit.too_many", &[])),
            DocError::Locked(id) => write!(f, "region {id} is in a locked area"),
            DocError::Edit(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DocError {}

impl From<EditError> for DocError {
    fn from(e: EditError) -> Self {
        match e {
            EditError::NotFound(id) => DocError::NotFound(id),
            other => DocError::Edit(other),
        }
    }
}

/// Where a new region goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Below the last region, at the left margin.
    Auto,
    /// At this position (page points), last in the list.
    At([f64; 2]),
    /// Directly below this region, next to it in the list.
    After(u32),
}

/// What [`Doc::delete_regions`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Deleted {
    /// Regions removed.
    pub removed: usize,
    /// True when a region in a locked area was left alone.
    pub locked_skipped: bool,
}

/// A worksheet open for editing. Derefs to the [`Document`] for reading.
#[derive(Debug)]
pub struct Doc {
    document: Document,
    history: History,
    revision: u64,
    path: Option<PathBuf>,
    saved_at: u64,
    calculation: Option<Calculation>,
}

impl Deref for Doc {
    type Target = Document;
    fn deref(&self) -> &Document {
        &self.document
    }
}

impl Doc {
    /// A new, empty worksheet.
    pub fn new(title: &str) -> Self {
        Self::from_document(Document::untitled(title))
    }

    /// Takes an existing document (for example one the GUI imported and evaluated).
    pub fn from_document(document: Document) -> Self {
        Self {
            document,
            history: History::default(),
            revision: 0,
            path: None,
            saved_at: 0,
            calculation: None,
        }
    }

    /// Opens a `.mcdx` file. Nothing is calculated; call [`Doc::calculate`].
    ///
    /// # Errors
    /// [`DocError::File`] when the file cannot be read, is too large, or is not a worksheet.
    pub fn open(path: &Path) -> Result<Self, DocError> {
        let title = file_title(path);
        let expected = std::fs::canonicalize(path)
            .map_err(|e| DocError::File(format!("cannot open {title}: {e}")))?;
        Self::open_verified(path, &expected)
    }

    /// Opens `path` as [`Doc::open`] does, but only if it still resolves to `expected` (the
    /// canonical path the caller checked, for example against its roots). A FIFO, device or
    /// directory is refused without blocking.
    ///
    /// # Errors
    /// [`DocError::File`] when the file cannot be read, is not a regular file, changed since
    /// `expected` was checked, is too large, or is not a worksheet.
    pub fn open_verified(path: &Path, expected: &Path) -> Result<Self, DocError> {
        let title = file_title(path);
        let bytes = read_regular_file(path, expected, MAX_FILE_BYTES).map_err(|e| {
            DocError::File(match e {
                RegularFileError::NotRegular => format!("{title} is not a regular file"),
                RegularFileError::TooLarge { limit } => {
                    format!("{title} is too large (the limit is {limit} bytes)")
                }
                RegularFileError::Changed => format!("{title} changed while it was being opened"),
                RegularFileError::Io(e) => format!("cannot open {title}: {e}"),
            })
        })?;
        let mut doc = Self::from_bytes(&title, bytes)?;
        doc.path = Some(path.to_path_buf());
        Ok(doc)
    }

    /// Reads a worksheet from the bytes of a `.mcdx` package.
    ///
    /// # Errors
    /// [`DocError::File`] when the bytes are not a readable worksheet.
    pub fn from_bytes(title: &str, bytes: Vec<u8>) -> Result<Self, DocError> {
        let title = title.to_string();
        let document = on_big_stack(move || -> Result<Document, DocError> {
            let pkg = Package::from_bytes(&bytes)
                .map_err(|e| DocError::File(format!("cannot open {title}: {e}")))?;
            let ws = Worksheet::from_package(&pkg)
                .map_err(|e| DocError::File(format!("cannot read {title}: {e}")))?;
            let table = load_results(&pkg)
                .map_err(|e| DocError::File(format!("cannot read results of {title}: {e}")))?;
            Ok(Document::from_worksheet(&title, ws, table).with_source(bytes.into()))
        })?;
        Ok(Self::from_document(document))
    }

    /// The `.mcdx` bytes of the document as it is now.
    ///
    /// # Errors
    /// [`DocError::File`] when the writer refuses the document.
    pub fn to_bytes(&self) -> Result<Vec<u8>, DocError> {
        self.to_bytes_with_report().map(|(bytes, _)| bytes)
    }

    /// The `.mcdx` bytes as [`Doc::to_bytes`], with the warnings of the writer: one for each region
    /// written with a file format that is not verified against Prime. Empty when there is none.
    ///
    /// # Errors
    /// [`DocError::File`] when the writer refuses the document.
    pub fn to_bytes_with_report(&self) -> Result<(Vec<u8>, Vec<String>), DocError> {
        let (bytes, mut warnings) =
            write_package_with_report(&self.document.worksheet(), self.document.source()).map_err(
                |e| DocError::File(format!("cannot write {}: {e}", self.document.title)),
            )?;
        // UNVERIFIED: Prime's file format of a disabled region is not known, so the flag is lost.
        let off = self.document.disabled_ids().count();
        if off > 0 {
            warnings.push(format!(
                "{off} disabled region(s) are saved as enabled: the file format of a disabled region is not verified."
            ));
        }
        Ok((bytes, warnings))
    }

    /// Saves to `path`, or to the file the document came from when `path` is `None`. The bytes are
    /// built first and go to a temporary file that is renamed over the target, so a failure leaves
    /// the old file as it was. Marks the document saved.
    ///
    /// # Errors
    /// [`DocError::File`] when there is no path, or the file cannot be written.
    pub fn save(&mut self, path: Option<&Path>) -> Result<(), DocError> {
        self.save_with_report(path).map(drop)
    }

    /// Saves as [`Doc::save`], and returns the writer's warnings (see
    /// [`Doc::to_bytes_with_report`]). The file is written whatever the warnings say.
    ///
    /// # Errors
    /// As [`Doc::save`].
    pub fn save_with_report(&mut self, path: Option<&Path>) -> Result<Vec<String>, DocError> {
        let target = match (path, &self.path) {
            (Some(p), _) => p.to_path_buf(),
            (None, Some(p)) => p.clone(),
            (None, None) => return Err(DocError::File("the document has no file name".into())),
        };
        let (bytes, warnings) = self.to_bytes_with_report()?;
        write_atomic(&target, &bytes)?;
        self.path = Some(target);
        self.saved_at = self.revision;
        Ok(warnings)
    }

    /// Checks that the worksheet can be written, without writing a file. It runs the same writer
    /// as [`Doc::save`] and discards the bytes, so every refusal (a programming region, for
    /// example) is reported before any file is touched. The message names the region.
    ///
    /// # Errors
    /// [`DocError::File`] with the writer's message when the worksheet cannot be written.
    pub fn check_saveable(&self) -> Result<(), DocError> {
        self.to_bytes().map(drop)
    }

    /// The file the document is stored in, if it has one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The document for reading.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Counts every change, undo and redo. Equal revisions mean equal documents.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// True when the document has changes that are not in its file.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_at
    }

    fn changed(&mut self) {
        self.revision += 1;
    }

    /// A new region id (see [`Document::allocate_id`]).
    pub fn allocate_id(&mut self) -> u32 {
        self.document.allocate_id()
    }

    // ---- Undo ----

    /// Runs `cmd` as one undo step.
    ///
    /// # Errors
    /// The command's error; the document is then unchanged.
    pub fn execute(&mut self, cmd: Box<dyn Command>) -> Result<(), EditError> {
        self.history.execute(&mut self.document, cmd)?;
        self.changed();
        Ok(())
    }

    /// Undoes the last step. `Ok(false)` when there is nothing to undo.
    ///
    /// # Errors
    /// [`EditError`] when the document is not in the state the step expects.
    pub fn undo(&mut self) -> Result<bool, EditError> {
        let done = self.history.undo(&mut self.document)?;
        if done {
            self.changed();
        }
        Ok(done)
    }

    /// Redoes the last undone step. `Ok(false)` when there is nothing to redo.
    ///
    /// # Errors
    /// [`EditError`] when the document is not in the state the step expects.
    pub fn redo(&mut self) -> Result<bool, EditError> {
        let done = self.history.redo(&mut self.document)?;
        if done {
            self.changed();
        }
        Ok(done)
    }

    /// Number of undo steps executed so far (rolled-back ones not counted). The difference
    /// between two readings is the number of steps taken in between.
    pub fn steps_executed(&self) -> u64 {
        self.history.executed()
    }

    /// Merges the last `n` undo steps into one step named `label`.
    pub fn collapse_steps(&mut self, n: u64, label: &'static str) {
        self.history.collapse_last(n, label);
    }

    /// Reverts the last `n` undo steps and forgets them (they cannot be redone).
    ///
    /// # Errors
    /// [`EditError`] when the document is not in the state a step expects.
    pub fn rollback_steps(&mut self, n: u64) -> Result<(), EditError> {
        let result = self.history.rollback_last(&mut self.document, n);
        self.changed();
        result
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The label of the step `undo` would revert.
    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo_label()
    }

    /// The label of the step `redo` would apply.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo_label()
    }

    // ---- Settings that are not undoable ----

    /// Sets the page settings.
    pub fn set_page(&mut self, page: PageSettings) {
        self.document.set_page(page);
        self.changed();
    }

    /// Switches the top-level regions `ids` off (`true`) or on. Not undoable, and not saved
    /// (UNVERIFIED file format). False when nothing changed.
    pub fn set_disabled(&mut self, ids: &[u32], off: bool) -> bool {
        let changed = self.document.set_disabled(ids, off);
        if changed {
            self.changed();
        }
        changed
    }

    /// Replaces the calculation and display settings.
    pub fn set_settings(&mut self, settings: Settings) {
        self.document.set_settings(settings);
        self.changed();
    }

    /// Sets the result format of region `id`. False when there is no such region.
    pub fn set_region_format(&mut self, id: u32, format: Option<ResultFormat>) -> bool {
        let found = self.document.set_region_format(id, format);
        if found {
            self.changed();
        }
        found
    }

    /// Sets the matrix display of region `id`. False when there is no such region.
    pub fn set_region_matrix(&mut self, id: u32, matrix: MatrixDisplay) -> bool {
        let found = self.document.set_region_matrix(id, matrix);
        if found {
            self.changed();
        }
        found
    }

    /// Sets the text of the header or footer.
    pub fn set_furniture(&mut self, footer: bool, text: TextBody) {
        self.document.set_furniture(footer, text);
        self.changed();
    }

    // ---- Reading ----

    /// Every top-level region in document order, with a one-line summary.
    pub fn outline(&self) -> Vec<RegionSummary> {
        read::outline(&self.document)
    }

    /// The content of region `id`. The result is included when the last calculation is current.
    ///
    /// # Errors
    /// [`DocError::NotFound`] when there is no such region.
    pub fn read_region(&self, id: u32) -> Result<RegionDetail, DocError> {
        let region = self.document.region(id).ok_or(DocError::NotFound(id))?;
        Ok(read::detail(region, self.current_calculation()))
    }

    /// The last calculation, when nothing has changed since.
    pub fn current_calculation(&self) -> Option<&Calculation> {
        self.calculation
            .as_ref()
            .filter(|c| c.revision == self.revision)
    }

    // ---- Calculating ----

    /// Calculates the worksheet and keeps the result. `opts.regions` limits what is reported (the
    /// whole sheet is always evaluated). The engine's limits apply: work budget, deadline and the
    /// cancel flag; regions reached after a limit are reported as skipped.
    pub fn calculate(&mut self, opts: &CalcOptions) -> Calculation {
        let result = calc::calculate(&self.document, self.revision, opts);
        // Keep only a complete calculation for later reads.
        if opts.regions.is_none() {
            self.calculation = Some(result.clone());
        }
        result
    }

    fn ensure_calculated(&mut self) -> Calculation {
        match self.current_calculation() {
            Some(c) => c.clone(),
            None => self.calculate(&CalcOptions::default()),
        }
    }

    /// Evaluates `text` in the scope of the worksheet without adding a region.
    ///
    /// # Errors
    /// [`DocError::Invalid`] when `text` does not read as an expression.
    pub fn evaluate_expression(
        &self,
        text: &str,
        opts: &CalcOptions,
    ) -> Result<RegionResult, DocError> {
        calc::evaluate_expression(&self.document, text, opts).map_err(DocError::Invalid)
    }

    /// The names the worksheet defines, with their values (the worksheet is calculated first
    /// when it has not been).
    pub fn variables(&mut self, opts: &CalcOptions) -> Vec<Variable> {
        let mut vars = read::variables(&self.document);
        let exprs: Vec<_> = vars
            .iter()
            .filter(|v| v.kind == read::VariableKind::Variable)
            .map(|v| {
                // The defining region's target keeps a literal subscript (`g_acc`).
                use mathcraft_core::expr::Expr;
                match self.document.region(v.region_id).map(|r| &r.kind) {
                    Some(mathcraft_import::regions::RegionKind::Math(
                        Expr::Define { target, .. } | Expr::GlobalDefine { target, .. },
                    )) if matches!(target.as_ref(), Expr::Id(_)) => target.as_ref().clone(),
                    _ => Expr::Id(mathcraft_core::expr::Id::new(
                        &v.name,
                        mathcraft_core::expr::IdKind::Variable,
                    )),
                }
            })
            .collect();
        let results = calc::evaluate_scratch(&self.document, exprs, opts);
        let mut next = results.into_iter();
        for v in vars
            .iter_mut()
            .filter(|v| v.kind == read::VariableKind::Variable)
        {
            if let Some(r) = next.next() {
                if r.status == crate::calc::ResultStatus::Value {
                    v.value = Some(r.value);
                }
            }
        }
        vars
    }

    /// The errors, skipped regions and notes of the worksheet (calculated first when needed).
    pub fn errors(&mut self) -> Vec<Problem> {
        let calc = self.ensure_calculated();
        read::problems(&self.document, &calc)
    }

    /// The whole sheet as plain text with results (calculated first when needed).
    pub fn transcript(&mut self) -> String {
        let calc = self.ensure_calculated();
        read::transcript(&self.document, Some(&calc))
    }

    // ---- Inserting ----

    /// Inserts a math region from Mathcraft's text notation, such as `v:=d/t`. Returns its id.
    ///
    /// # Errors
    /// [`DocError::Invalid`] when the text does not read, [`DocError::TooManyRegions`], or
    /// [`DocError::NotFound`] for an `After` id that does not exist.
    pub fn insert_math(&mut self, text: &str, at: Placement) -> Result<u32, DocError> {
        let expr = self.parse(text)?;
        self.insert_region(RegionKind::Math(expr), None, at)
    }

    /// Inserts a text region. Returns its id.
    ///
    /// # Errors
    /// As [`Doc::insert_math`], without the parse error.
    pub fn insert_text(&mut self, text: &str, at: Placement) -> Result<u32, DocError> {
        self.insert_text_body(TextBody::from_plain(text), at)
    }

    /// Inserts a text region with styled text.
    ///
    /// # Errors
    /// As [`Doc::insert_text`].
    pub fn insert_text_body(&mut self, body: TextBody, at: Placement) -> Result<u32, DocError> {
        self.insert_region(RegionKind::Text(body), None, at)
    }

    /// Inserts a 2D plot built from `spec` (`spec.polar` makes a polar plot). Returns its id.
    ///
    /// # Errors
    /// [`DocError::Invalid`] when an expression or limit of `spec` is not valid, and the errors of
    /// [`Doc::insert_math`].
    pub fn insert_plot(&mut self, spec: &PlotSpec, at: Placement) -> Result<u32, DocError> {
        let ctx = EditContext::for_regions(self.document.regions());
        let info = plot::build(spec, &ctx).map_err(DocError::Invalid)?;
        self.insert_region(RegionKind::Plot(info), Some(plot::PLOT_DEFAULT_SIZE), at)
    }

    /// Inserts an empty plot (`polar` false: x-y). Returns its id.
    ///
    /// # Errors
    /// As [`Doc::insert_math`], without the parse error.
    pub fn insert_empty_plot(&mut self, polar: bool, at: Placement) -> Result<u32, DocError> {
        let (kind, size) = plot::new_plot_kind(polar);
        self.insert_region(kind, Some(size), at)
    }

    /// Inserts an empty area. Returns its id.
    ///
    /// # Errors
    /// As [`Doc::insert_math`], without the parse error.
    pub fn insert_area(&mut self, at: Placement) -> Result<u32, DocError> {
        self.insert_region(RegionKind::Area(AreaModel::default()), Some(AREA_SIZE), at)
    }

    /// Inserts a region of any kind. `size` is its frame when it has one. One undo step.
    ///
    /// # Errors
    /// See [`Doc::insert_math`].
    pub fn insert_region(
        &mut self,
        kind: RegionKind,
        size: Option<(f64, f64)>,
        at: Placement,
    ) -> Result<u32, DocError> {
        if self.document.regions().len() >= MAX_REGIONS {
            return Err(DocError::TooManyRegions);
        }
        let (pos, index) = self.place(at)?;
        let id = self.document.allocate_id();
        let region = RegionModel {
            id,
            pos: Some((pos[0], pos[1])),
            size,
            result_ref: None,
            kind,
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        };
        self.execute(Box::new(InsertRegion { index, region }))?;
        Ok(id)
    }

    /// The position and list index of a new region.
    fn place(&self, at: Placement) -> Result<([f64; 2], usize), DocError> {
        let regions = self.document.regions();
        match at {
            Placement::At(p) => Ok((p, regions.len())),
            Placement::After(id) => {
                let index = self.document.index_of(id).ok_or(DocError::NotFound(id))?;
                let r = &regions[index];
                let (x, y) = r.pos.unwrap_or((self.left_margin(), 0.0));
                Ok(([x, y + region_height(r, regions) + GAP_PT], index + 1))
            }
            Placement::Auto => {
                let margins = self.document.page().margins();
                let y = lowest_bottom(regions).map_or(margins.top, |b| b + GAP_PT);
                Ok(([margins.left, y], regions.len()))
            }
        }
    }

    fn left_margin(&self) -> f64 {
        self.document.page().margins().left
    }

    fn parse(&self, text: &str) -> Result<mathcraft_core::expr::Expr, DocError> {
        let ctx = EditContext::for_regions(self.document.regions());
        parse_math_in(text, &ctx)
            .map_err(|e| DocError::Invalid(message("edit.cannot_read", &[&e.to_string()])))
    }

    // ---- Editing ----

    /// Replaces the expression of math region `id`. One undo step; nothing changes when the text
    /// reads as the expression the region already has.
    ///
    /// # Errors
    /// [`DocError::NotFound`], [`DocError::NotEditable`] for a region that is not math, or
    /// [`DocError::Invalid`] when the text does not read.
    pub fn edit_math(&mut self, id: u32, text: &str) -> Result<(), DocError> {
        let expr = self.parse(text)?;
        let before = self
            .document
            .region(id)
            .cloned()
            .ok_or(DocError::NotFound(id))?;
        if !matches!(before.kind, RegionKind::Math(_)) {
            return Err(DocError::NotEditable(id));
        }
        self.replace_kind(id, RegionKind::Math(expr))
    }

    /// Replaces the text of text region `id`, keeping its style (see [`TextBody::edited`]).
    ///
    /// # Errors
    /// [`DocError::NotFound`] or [`DocError::NotEditable`] for a region that is not text.
    pub fn edit_text(&mut self, id: u32, text: &str) -> Result<(), DocError> {
        let region = self.document.region(id).ok_or(DocError::NotFound(id))?;
        let RegionKind::Text(body) = &region.kind else {
            return Err(DocError::NotEditable(id));
        };
        let new = body.edited(text);
        self.replace_kind(id, RegionKind::Text(new))
    }

    /// Replaces the character ranges of text region `id` by `replacement`, keeping the styles
    /// between them (see [`mathcraft_import::rich_text::RichText::replace_ranges`]).
    ///
    /// # Errors
    /// As [`Doc::edit_text`].
    pub fn replace_text_ranges(
        &mut self,
        id: u32,
        ranges: &[std::ops::Range<usize>],
        replacement: &str,
    ) -> Result<(), DocError> {
        let region = self.document.region(id).ok_or(DocError::NotFound(id))?;
        let RegionKind::Text(body) = &region.kind else {
            return Err(DocError::NotEditable(id));
        };
        let new = body.replaced_ranges(ranges, replacement);
        self.replace_kind(id, RegionKind::Text(new))
    }

    /// Replaces the body of text region `id` (text with styles).
    ///
    /// # Errors
    /// As [`Doc::edit_text`].
    pub fn set_text_body(&mut self, id: u32, body: TextBody) -> Result<(), DocError> {
        let region = self.document.region(id).ok_or(DocError::NotFound(id))?;
        if !matches!(region.kind, RegionKind::Text(_)) {
            return Err(DocError::NotEditable(id));
        }
        self.replace_kind(id, RegionKind::Text(body))
    }

    /// Replaces the body of text region `id` when its formatting (not only its plain text)
    /// changed. One undo step. Kept for callers that want the explicit name; [`Doc::set_text_body`]
    /// and [`Doc::replace_kind`] compare the rich text too.
    ///
    /// # Errors
    /// As [`Doc::edit_text`].
    pub fn set_text_format(&mut self, id: u32, body: TextBody) -> Result<(), DocError> {
        let before = self
            .document
            .region(id)
            .cloned()
            .ok_or(DocError::NotFound(id))?;
        let RegionKind::Text(old) = &before.kind else {
            return Err(DocError::NotEditable(id));
        };
        if old.rich() == body.rich() {
            return Ok(());
        }
        let mut after = before.clone();
        after.kind = RegionKind::Text(body);
        after.result_ref = None;
        self.execute(Box::new(ReplaceRegion { before, after }))?;
        Ok(())
    }

    /// Replaces the kind of region `id` as one undo step. The stored result of the old content is
    /// dropped. Nothing changes when the kind is equal to the old one; for text, equal means the
    /// same rich text (runs, styles and paragraph styles), not only the same plain text.
    ///
    /// # Errors
    /// [`DocError::NotFound`].
    pub fn replace_kind(&mut self, id: u32, kind: RegionKind) -> Result<(), DocError> {
        let before = self
            .document
            .region(id)
            .cloned()
            .ok_or(DocError::NotFound(id))?;
        if same_content(&before.kind, &kind) {
            return Ok(());
        }
        let mut after = before.clone();
        after.kind = kind;
        // The stored result belonged to the old content, so it is no longer compared.
        after.result_ref = None;
        self.execute(Box::new(ReplaceRegion { before, after }))?;
        Ok(())
    }

    /// Replaces the traces and axes of plot `id` with those of `spec` as one undo step. The style
    /// of a trace that already existed is kept, as are the legend, the point counts and the grid
    /// frequency. Nothing changes when an expression does not read.
    ///
    /// # Errors
    /// [`DocError::NotFound`], [`DocError::NotEditable`] for a region that is not a 2D plot, or
    /// [`DocError::Invalid`] for a bad expression or limit.
    pub fn set_plot_spec(&mut self, id: u32, spec: &PlotSpec) -> Result<(), DocError> {
        let before = self
            .document
            .region(id)
            .cloned()
            .ok_or(DocError::NotFound(id))?;
        let RegionKind::Plot(old) = &before.kind else {
            return Err(DocError::NotEditable(id));
        };
        let Some(old_plot) = old.plot.as_deref() else {
            return Err(DocError::NotEditable(id));
        };
        let ctx = EditContext::for_regions(self.document.regions());
        let mut spec = spec.clone();
        spec.polar = old_plot.polar;
        let mut info = plot::build(&spec, &ctx).map_err(DocError::Invalid)?;
        if let Some(new) = info.plot.as_deref_mut() {
            for (i, trace) in new.traces.iter_mut().enumerate() {
                if let Some(old_trace) = old_plot.traces.get(i) {
                    trace.style = old_trace.style.clone();
                    trace.name = old_trace.name.clone();
                    trace.num_of_points = old_trace.num_of_points;
                }
            }
            new.legend = old_plot.legend;
            new.x_axis.log = old_plot.x_axis.log;
            new.y_axis.log = old_plot.y_axis.log;
            new.x_axis.grid_frequency =
                old_plot.x_axis.grid_frequency.or(new.x_axis.grid_frequency);
            new.y_axis.grid_frequency =
                old_plot.y_axis.grid_frequency.or(new.y_axis.grid_frequency);
            new.y_axis.domain_start = old_plot.y_axis.domain_start.clone();
            new.y_axis.domain_end = old_plot.y_axis.domain_end.clone();
            // An axis whose limits the caller did not change keeps its stored limits and domain
            // (a plot read from a file has its own, which the text form does not hold exactly).
            if let Some(b) = plot::spec_of(&before) {
                if b.x_min == spec.x_min && b.x_max == spec.x_max {
                    new.x_axis.start = old_plot.x_axis.start;
                    new.x_axis.end = old_plot.x_axis.end;
                    new.x_axis.domain_start = old_plot.x_axis.domain_start.clone();
                    new.x_axis.domain_end = old_plot.x_axis.domain_end.clone();
                }
                if b.y_min == spec.y_min && b.y_max == spec.y_max {
                    new.y_axis.start = old_plot.y_axis.start;
                    new.y_axis.end = old_plot.y_axis.end;
                }
            }
        }
        if let Some(new) = info.plot.as_deref() {
            if !new.polar {
                info.x_range = new.x_axis.start.zip(new.x_axis.end);
                info.y_range = new.y_axis.start.zip(new.y_axis.end);
            }
        }
        if info == *old {
            return Ok(());
        }
        let mut after = before.clone();
        after.kind = RegionKind::Plot(info);
        // The stored traces belonged to the old expressions.
        after.result_ref = None;
        self.execute(Box::new(ReplaceRegion { before, after }))?;
        Ok(())
    }

    /// Sets one style property of trace `trace` of plot `id` as one undo step. Returns false when
    /// the plot has no such trace or the change does nothing.
    ///
    /// # Errors
    /// [`DocError::NotFound`] or [`DocError::NotEditable`] for a region that is not a 2D plot.
    pub fn plot_trace_style(
        &mut self,
        id: u32,
        trace: usize,
        change: plot::StyleChange,
    ) -> Result<bool, DocError> {
        let before = self
            .document
            .region(id)
            .cloned()
            .ok_or(DocError::NotFound(id))?;
        let mut after = before.clone();
        let RegionKind::Plot(info) = &mut after.kind else {
            return Err(DocError::NotEditable(id));
        };
        let Some(model) = info.plot.as_deref_mut() else {
            return Err(DocError::NotEditable(id));
        };
        let Some(t) = model.traces.get_mut(trace) else {
            return Ok(false);
        };
        plot::apply_style(&mut t.style, &change);
        if before == after {
            return Ok(false);
        }
        after.result_ref = None;
        self.execute(Box::new(ReplaceRegion { before, after }))?;
        Ok(true)
    }

    /// Applies a plot change (trace, marker, scale, grid, style) to plot `id` as one undo step.
    /// Returns false when the change does nothing.
    ///
    /// # Errors
    /// [`DocError::NotFound`] or [`DocError::NotEditable`] for a region that is not a 2D plot.
    pub fn plot_op(&mut self, id: u32, op: PlotOp) -> Result<bool, DocError> {
        let before = self
            .document
            .region(id)
            .cloned()
            .ok_or(DocError::NotFound(id))?;
        let mut after = before.clone();
        let RegionKind::Plot(info) = &mut after.kind else {
            return Err(DocError::NotEditable(id));
        };
        let Some(model) = info.plot.as_deref_mut() else {
            return Err(DocError::NotEditable(id));
        };
        if !plot::apply(model, op) {
            return Ok(false);
        }
        if model.polar {
            info.x_range = None;
            info.y_range = None;
        } else {
            info.x_range = model.x_axis.start.zip(model.x_axis.end);
            info.y_range = model.y_axis.start.zip(model.y_axis.end);
        }
        after.result_ref = None;
        self.execute(Box::new(ReplaceRegion { before, after }))?;
        Ok(true)
    }

    // ---- Moving, resizing, deleting ----

    /// Moves region `id` to `to` (page points). One undo step.
    ///
    /// # Errors
    /// [`DocError::NotFound`].
    pub fn move_region(&mut self, id: u32, to: [f64; 2]) -> Result<(), DocError> {
        let region = self.document.region(id).ok_or(DocError::NotFound(id))?;
        let from = region.pos.unwrap_or((0.0, 0.0));
        let to = (to[0].max(0.0), to[1].max(0.0));
        if region.pos == Some(to) {
            return Ok(());
        }
        self.execute(Box::new(MoveRegion { id, from, to }))?;
        Ok(())
    }

    /// Moves the regions `ids` by `delta` (page points) as one undo step. Regions without a stored
    /// position stay. Returns false when nothing moved.
    ///
    /// # Errors
    /// [`EditError`] when the document is not in the state the step expects.
    pub fn move_regions(&mut self, ids: &[u32], delta: [f64; 2]) -> Result<bool, DocError> {
        let steps: Vec<Box<dyn Command>> = ids
            .iter()
            .filter_map(|&id| {
                let from = self.document.region(id)?.pos?;
                let to = moved_position(from, delta);
                (to != from).then(|| Box::new(MoveRegion { id, from, to }) as Box<dyn Command>)
            })
            .collect();
        if steps.is_empty() {
            return Ok(false);
        }
        self.execute(Box::new(Batch::new("Move region", steps)))?;
        Ok(true)
    }

    /// Sets the frame of region `id` (position and size, page points) as one undo step.
    ///
    /// # Errors
    /// [`DocError::NotFound`], or [`DocError::Invalid`] for a size that is not positive.
    pub fn set_frame(
        &mut self,
        id: u32,
        pos: (f64, f64),
        size: (f64, f64),
    ) -> Result<(), DocError> {
        if !(size.0 > 0.0 && size.1 > 0.0 && size.0.is_finite() && size.1.is_finite()) {
            return Err(DocError::Invalid("the size must be positive".into()));
        }
        let before = self
            .document
            .region(id)
            .cloned()
            .ok_or(DocError::NotFound(id))?;
        if before.pos == Some(pos) && before.size == Some(size) {
            return Ok(());
        }
        let mut after = before.clone();
        after.pos = Some(pos);
        after.size = Some(size);
        self.execute(Box::new(ReplaceRegion { before, after }))?;
        Ok(())
    }

    /// Resizes region `id`, keeping its position, as one undo step.
    ///
    /// # Errors
    /// As [`Doc::set_frame`].
    pub fn resize_region(&mut self, id: u32, size: (f64, f64)) -> Result<(), DocError> {
        let pos = self
            .document
            .region(id)
            .ok_or(DocError::NotFound(id))?
            .pos
            .unwrap_or((0.0, 0.0));
        self.set_frame(id, pos, size)
    }

    /// Deletes region `id`. One undo step.
    ///
    /// # Errors
    /// [`DocError::NotFound`].
    pub fn delete_region(&mut self, id: u32) -> Result<(), DocError> {
        if self.document.region(id).is_none() {
            return Err(DocError::NotFound(id));
        }
        self.delete_regions(&[id]).map(|_| ())
    }

    /// Deletes the regions `ids` as one undo step. Ids that do not exist are ignored and a locked
    /// area is left alone (see [`Deleted`]).
    ///
    /// # Errors
    /// [`EditError`] when the document is not in the state the step expects.
    pub fn delete_regions(&mut self, ids: &[u32]) -> Result<Deleted, DocError> {
        // Highest position first: each removal then leaves the lower positions valid, and the
        // batch reverts in the opposite order, which puts every region back where it was.
        let mut locked_skipped = false;
        let mut found: Vec<(usize, RegionModel)> = ids
            .iter()
            .filter_map(|&id| {
                let index = self.document.index_of(id)?;
                let region = &self.document.regions()[index];
                if matches!(&region.kind, RegionKind::Area(a) if a.locked) {
                    locked_skipped = true;
                    return None;
                }
                Some((index, region.clone()))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0));
        found.dedup_by(|a, b| a.0 == b.0);
        if found.is_empty() {
            return Ok(Deleted {
                removed: 0,
                locked_skipped,
            });
        }
        let removed = found.len();
        let label = if removed == 1 {
            "Delete region"
        } else {
            "Delete regions"
        };
        let steps: Vec<Box<dyn Command>> = found
            .into_iter()
            .map(|(index, region)| Box::new(RemoveRegion { index, region }) as Box<dyn Command>)
            .collect();
        self.execute(Box::new(Batch::new(label, steps)))?;
        Ok(Deleted {
            removed,
            locked_skipped,
        })
    }
}

/// Replaces the file at `path` with `bytes` through a temporary file in the same folder.
/// The file name of `path`, for error messages.
fn file_title(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DocError> {
    use std::io::Write;
    let name = path
        .file_name()
        .ok_or_else(|| DocError::File(format!("{} is not a file path", path.display())))?
        .to_string_lossy()
        .into_owned();
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let fail = |e: std::io::Error| DocError::File(format!("cannot save {}: {e}", path.display()));
    let pid = std::process::id();
    for seq in 0..100u32 {
        let temp = dir.join(format!(".{name}.{pid}.{seq}.tmp"));
        // `create_new` never writes through an existing name or a planted link.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp);
        let mut file = match file {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(fail(e)),
        };
        let result = (|| -> std::io::Result<()> {
            file.write_all(bytes)?;
            if let Ok(meta) = std::fs::metadata(path) {
                file.set_permissions(meta.permissions())?;
            }
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, path)
        })();
        return result.map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            fail(e)
        });
    }
    Err(DocError::File(format!(
        "cannot save {}: no free temporary name",
        path.display()
    )))
}

/// True when two region kinds have the same content. `TextBody` equality compares the plain text
/// only, so text regions are compared by their rich text (runs, styles, sizes, paragraph styles),
/// which a style-only change differs in.
fn same_content(a: &RegionKind, b: &RegionKind) -> bool {
    match (a, b) {
        (RegionKind::Text(x), RegionKind::Text(y)) => x.rich() == y.rich(),
        _ => a == b,
    }
}

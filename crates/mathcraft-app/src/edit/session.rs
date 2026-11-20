//! The editing session: the document, its history, the selection, the insertion point and the
//! draft being typed. Every key and click the window gives is turned into one of these calls.
//! No egui types here, so the whole behaviour is tested without a window.
//!
//! Model of an edit:
//! - A click on a region selects it. A click on empty page sets the insertion point (a crosshair).
//! - Typing with an insertion point starts a new math region there. Typing with a selected math
//!   or text region starts editing it. The typed text is a draft; nothing changes in the document
//!   until the draft is committed (Enter for math, Escape for text, or a click elsewhere, or
//!   undo, or save).
//! - Committing a math draft parses it (see `math.rs`). A text that does not parse keeps the
//!   draft open with a message. A committed change is one undo step.
//! - After every committed change the window re-evaluates the document. `revision` counts the
//!   changes, so the window knows when its evaluated view is out of date.
//!
//! Limitations of this stage:
//! - Only top-level regions are edited. Solve-block members are read-only.
//! - Unsupported and solve-block regions can be selected, moved and deleted, not edited. A 2D plot
//!   is edited through its text form (`plot.rs`, `set_plot_spec`) and resized like an image.
//! - Only one draft at a time. Several regions can be selected (Ctrl+A, or several moved together
//!   by a drag); Delete and the arrow keys then act on all of them as one undo step.
//! - Arrow keys with a selected region and no draft move the region; there is no snapping.
//! - A drag moves the selected regions (or the one under the press, which becomes the selection).
//!   The move is one undo step, made when the mouse is released. A drag from empty page is a
//!   rubber band: on release it selects the regions it touches. Ctrl+drag copies instead.
//! - Shift+click adds a region to the selection or takes it out. Align, distribute and insert
//!   lines work on the selection as one undo step each.
//! - Copy, cut and paste use the system clipboard with the fragment text of `clipboard.rs`.

use crate::i18n::{tr, trf};
use mathcraft_import::regions::{AreaModel, ImageModel, RegionKind, RegionModel, MAX_REGIONS};
use mathcraft_import::rich_text::{Align, RichText, RunStyle, StyleChange, TextBody};

use super::clipboard::{self, ClipError, Clipped};
use super::command::{
    moved_position, Batch, Command, EditError, InsertRegion, MoveRegion, RemoveRegion,
    ReplaceRegion,
};
use super::document::Document;
use super::layout::{self, Arrange, Box2};
use super::math::{
    parse_math_in, EditContext, MathEditor, MatrixOp, Template, MAX_PASTE_CHARS, PLACEHOLDER,
};
use super::plot::{self, PlotSpec};
use super::resize::{self, Corner};
use super::text::TextEditor;
use mathcraft_doc::search::{self, Match as SearchMatch, ReplaceReport, SearchOptions};
use mathcraft_doc::{Doc, Placement};
use std::ops::Range;

/// Points moved by one arrow key press on a selected region.
pub const NUDGE: f64 = 6.0;

/// Offset of a paste from the regions it copies, when no insertion point is set (points).
const PASTE_OFFSET: f64 = 12.0;

/// Distance between the lines of plain text pasted as math regions (points).
const LINE_STEP: f64 = 24.0;

/// Largest space Insert lines adds at once (points).
const MAX_INSERT_LINES: f64 = 2000.0;

/// Space left between regions that Separate regions pushes apart (points).
const SEPARATE_GAP_PT: f64 = 6.0;

/// Size of a new area, in points.
const AREA_DEFAULT_SIZE: (f64, f64) = (360.0, 120.0);
/// Largest image file that can be inserted, in bytes (the package reader's cap for a part).
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Widest an inserted image is at first, in points. A wider one is scaled down to this.
pub const IMAGE_MAX_WIDTH_PT: f64 = 400.0;
/// Points per image pixel, for the first size of an inserted image (96 dpi on a 72-point inch).
const PT_PER_PX: f64 = 0.75;
/// How far from the insertion point a page break can be to be deleted, in points.
const PAGE_BREAK_REACH_PT: f64 = 12.0;
/// Space between the members of a new area and its border, in page points.
const AREA_MARGIN_PT: f64 = 8.0;

/// A key the window gives to the session. Letters and other text come as `Char`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Tab: the next empty slot (in a matrix, the next cell; from its last cell, a new row).
    Tab,
    /// Shift+Tab: the previous empty slot.
    BackTab,
    /// Shift+Left: extends the selection one item to the left.
    SelectLeft,
    /// Shift+Right: extends the selection one item to the right.
    SelectRight,
    /// Ctrl+Left: the caret moves to the start of the term before it (see `MathEditor::term_left`).
    TermLeft,
    /// Ctrl+Right: the caret moves to the end of the term after it.
    TermRight,
    /// Shift+Enter: a new matrix row under the caret's row. Outside a matrix it is Enter.
    ShiftEnter,
}

/// What is being typed.
#[derive(Debug, Clone, PartialEq)]
pub enum DraftKind {
    Math(MathEditor),
    /// The editor, and the text the draft edits (its style is kept by the edit).
    Text(TextEditor, TextBody),
}

/// A region being typed. `target` is the region it replaces, or `None` for a new region placed
/// at `at` (page points).
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub target: Option<u32>,
    pub at: [f64; 2],
    pub kind: DraftKind,
    /// `Some(footer)` when the draft edits the page header (false) or footer (true). Such a draft
    /// has no `target`: it writes the document's header or footer text on commit.
    pub furniture: Option<bool>,
}

/// A pointer action in the draft: a press puts the caret, a drag extends the selection from the
/// press, a double click selects the word. Each carries a character index of the draft's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftPointer {
    Press(usize),
    Drag(usize),
    Word(usize),
}

/// What the canvas draws for the draft.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftView {
    pub target: Option<u32>,
    /// True when the draft edits the page header or footer (see [`Draft::furniture`]).
    pub furniture: bool,
    pub at: [f64; 2],
    /// The text to show, with the caret not in it.
    pub text: String,
    /// The caret as a character index into `text`.
    pub caret: usize,
    /// The selected character range `(start, end)` of `text`, when one is made.
    pub selection: Option<(usize, usize)>,
    /// True for a math draft (shown as plain text, see `math.rs`), false for a text region.
    pub math: bool,
    /// The item tree of a math draft that holds structure (fractions, roots, operators): the
    /// canvas draws it typeset instead of `text`.
    pub tree: Option<crate::draftlayout::DraftTree>,
}

/// A move in progress: the regions that move (those with a stored position) and the total
/// displacement so far, in page points.
#[derive(Debug, Clone, PartialEq)]
struct Drag {
    ids: Vec<u32>,
    delta: [f64; 2],
    /// Ctrl+drag: the drop copies the regions instead of moving them.
    copy: bool,
}

/// A rubber-band selection in progress: a press on empty page, and where the mouse is now.
/// A resize of the selected image by one of its corner handles, in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resize {
    id: u32,
    corner: Corner,
    pos: (f64, f64),
    size: (f64, f64),
    delta: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Marquee {
    start: [f64; 2],
    end: [f64; 2],
}

/// A document open for editing.
#[derive(Debug)]
pub struct EditSession {
    doc: Doc,
    /// The selected top-level regions, in the order they were selected.
    selection: Vec<u32>,
    crosshair: Option<[f64; 2]>,
    draft: Option<Draft>,
    message: Option<String>,
    drag: Option<Drag>,
    marquee: Option<Marquee>,
    resize: Option<Resize>,
    /// The page point (stored frame) of a double click that started editing a region, until the
    /// canvas has selected the word under it (see [`EditSession::edit_region_word`]).
    word_point: Option<[f64; 2]>,
    /// The number of the live (MCP) document id of this tab, for its whole life.
    live_number: u64,
}

impl EditSession {
    pub fn new(doc: Document) -> Self {
        Self::from_doc(
            Doc::from_document(doc),
            mathcraft_mcp::live::next_doc_number(),
        )
    }

    /// A session around an existing `Doc` (a document an MCP tool made), with its live number.
    pub fn from_doc(doc: Doc, live_number: u64) -> Self {
        Self {
            live_number,
            doc,
            selection: Vec::new(),
            crosshair: None,
            draft: None,
            message: None,
            drag: None,
            marquee: None,
            resize: None,
            word_point: None,
        }
    }

    pub fn document(&self) -> &Document {
        self.doc.document()
    }

    /// The number in this tab's live document id (`live<N>`).
    pub fn live_number(&self) -> u64 {
        self.live_number
    }

    /// Takes the `Doc` out for one MCP call, leaving an empty placeholder. Pair with
    /// [`EditSession::put_doc`] in the same frame; nothing reads the session in between.
    pub fn take_doc(&mut self) -> Doc {
        std::mem::replace(&mut self.doc, Doc::new(""))
    }

    /// Puts the `Doc` back. `changed` is true when the call edited it: the selection is then
    /// trimmed to regions that still exist. An unchanged document keeps a drag in progress.
    pub fn put_doc(&mut self, doc: Doc, changed: bool) {
        self.doc = doc;
        if changed {
            self.changed();
        }
    }

    /// Sets the page settings. This is an edit that marks the document dirty, but it is not in the
    /// undo history (the page model is not a command).
    pub fn set_page(&mut self, page: mathcraft_import::page::PageSettings) {
        self.doc.set_page(page);
        self.changed();
    }

    /// Replaces the calculation and display settings. An edit that marks the document dirty and
    /// is not in the undo history, like the page settings.
    pub fn set_settings(&mut self, settings: mathcraft_import::regions::Settings) {
        self.doc.set_settings(settings);
        self.changed();
    }

    /// Sets the result format of the top-level region `id`. False when there is no such region.
    pub fn set_region_format(
        &mut self,
        id: u32,
        format: Option<mathcraft_core::eval::ResultFormat>,
    ) -> bool {
        let found = self.doc.set_region_format(id, format);
        if found {
            self.changed();
        }
        found
    }

    /// Sets the matrix display of a top-level region (see [`Document::set_region_matrix`]).
    pub fn set_region_matrix(
        &mut self,
        id: u32,
        matrix: mathcraft_core::eval::MatrixDisplay,
    ) -> bool {
        let found = self.doc.set_region_matrix(id, matrix);
        if found {
            self.changed();
        }
        found
    }

    /// Inserts an empty area at `at` (page points) and selects it. One undo step.
    pub fn insert_area(&mut self, at: [f64; 2]) {
        self.insert_region(
            at,
            RegionKind::Area(AreaModel::default()),
            AREA_DEFAULT_SIZE,
        );
    }

    /// Inserts an image at `at` (page points) and selects it. The bytes must be a PNG or JPEG file
    /// of at most [`MAX_IMAGE_BYTES`]. The image is sized from its pixels (96 dpi, shown at 72
    /// points to the inch), and scaled down to fit [`IMAGE_MAX_WIDTH_PT`]. One undo step.
    pub fn insert_image(&mut self, bytes: Vec<u8>, at: [f64; 2]) -> Result<(), String> {
        let image = ImageModel {
            item: String::new(),
            bytes: bytes.into(),
        };
        if image.media_type().is_none() {
            return Err(tr("edit.image_format").to_string());
        }
        if image.bytes.len() > MAX_IMAGE_BYTES {
            return Err(tr("edit.image_too_large").to_string());
        }
        let (px_w, px_h) = crate::images::dimensions(&image.bytes)
            .ok_or_else(|| tr("edit.image_format").to_string())?;
        let mut w = f64::from(px_w) * PT_PER_PX;
        let mut h = f64::from(px_h) * PT_PER_PX;
        if w > IMAGE_MAX_WIDTH_PT {
            h *= IMAGE_MAX_WIDTH_PT / w;
            w = IMAGE_MAX_WIDTH_PT;
        }
        self.insert_region(at, RegionKind::Image(image), (w, h));
        Ok(())
    }

    /// Inserts an empty x-y plot (`polar` false) or polar plot at `at` (page points) and selects
    /// it. One undo step. The plot has one trace with a placeholder on each axis.
    pub fn insert_plot(&mut self, at: [f64; 2], polar: bool) {
        let (kind, size) = plot::new_plot_kind(polar);
        self.insert_region(at, kind, size);
    }

    /// Inserts an empty plot at the insertion point. Without one it gives a message and returns
    /// false.
    pub fn insert_plot_at_crosshair(&mut self, polar: bool) -> bool {
        match self.crosshair {
            Some(at) => {
                self.insert_plot(at, polar);
                true
            }
            None => {
                self.message = Some(tr("edit.click_first").to_string());
                false
            }
        }
    }

    /// The text form of the plot `id`, for the plot dialog. `None` when `id` is not a 2D plot.
    pub fn plot_spec(&self, id: u32) -> Option<PlotSpec> {
        plot::spec_of(self.doc.region(id)?)
    }

    /// The id of the selected 2D plot, when exactly one region is selected and it is one.
    pub fn selected_plot(&self) -> Option<u32> {
        let id = self.selection()?;
        self.plot_spec(id).map(|_| id)
    }

    /// Replaces the traces and axes of the plot `id` with those of `spec`, as one undo step. The
    /// style of a trace that already existed is kept, as are the legend, the point counts and the
    /// grid frequency. Nothing changes when an expression does not parse; the error says which.
    ///
    /// # Errors
    /// The message to show: a parse error, a bad limit, or a region that is not a 2D plot.
    pub fn set_plot_spec(&mut self, id: u32, spec: &PlotSpec) -> Result<(), String> {
        if self.draft.is_some() && self.commit().is_err() {
            return Err(self.message.clone().unwrap_or_default());
        }
        let result = self.doc.set_plot_spec(id, spec).map_err(|e| e.to_string());
        self.changed();
        result
    }

    /// Applies a Diagramme-tab change to the selected 2D plot as one undo step. Returns false (with
    /// a message) when no plot is selected or the change does nothing.
    pub fn plot_op(&mut self, op: plot::PlotOp) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let Some(id) = self.selected_plot() else {
            return false;
        };
        match self.doc.plot_op(id, op) {
            Ok(true) => {
                self.changed();
                true
            }
            Ok(false) => {
                self.message = Some(tr("edit.plot_op_nothing").to_string());
                false
            }
            Err(_) => false,
        }
    }

    /// Selects the first 2D plot (`plot` true) or math region (false) of the sheet. For the
    /// screenshot harness. Returns false when there is none.
    pub fn select_first(&mut self, plot: bool) -> bool {
        let found = self.doc.regions().iter().find(|r| match &r.kind {
            RegionKind::Plot(info) => plot && info.plot.is_some(),
            RegionKind::Math(_) => !plot,
            _ => false,
        });
        match found.map(|r| r.id) {
            Some(id) => {
                self.selection = vec![id];
                true
            }
            None => false,
        }
    }

    /// The selected 2D plot's model, for the ribbon (marker state, trace count).
    pub fn selected_plot_model(&self) -> Option<&mathcraft_import::regions::Plot2d> {
        let id = self.selected_plot()?;
        match &self.doc.region(id)?.kind {
            RegionKind::Plot(info) => info.plot.as_deref(),
            _ => None,
        }
    }

    /// The math region the matrix result flags apply to: the selected one, else the one being
    /// edited.
    pub fn matrix_target(&self) -> Option<u32> {
        let id = self
            .selection()
            .or_else(|| self.draft_view().and_then(|d| d.target))?;
        matches!(self.doc.region(id)?.kind, RegionKind::Math(_)).then_some(id)
    }

    /// Flips the row and column indices (`indices` true) or the nested-matrix collapse of the
    /// matrix result of the target math region, as one undo step.
    pub fn toggle_matrix_flag(&mut self, indices: bool) -> bool {
        let Some(id) = self.matrix_target() else {
            return false;
        };
        let Some(before) = self.doc.region(id).cloned() else {
            return false;
        };
        let mut after = before.clone();
        if indices {
            after.matrix.show_indices = !after.matrix.show_indices;
        } else {
            after.matrix.expand_nested = !after.matrix.expand_nested;
        }
        self.execute(Box::new(ReplaceRegion { before, after }))
            .is_ok()
    }

    /// Expands or collapses an area (one undo step). A locked area is not changed; its message says
    /// so. Returns false when `id` is not a top-level area.
    pub fn toggle_area(&mut self, id: u32) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let Some(before) = self.doc.region(id).cloned() else {
            return false;
        };
        let RegionKind::Area(area) = before.kind.clone() else {
            return false;
        };
        if area.locked {
            self.message = Some(tr("edit.area_locked").to_string());
            return true;
        }
        let mut after = before.clone();
        after.kind = RegionKind::Area(AreaModel {
            collapsed: !area.collapsed,
            ..area
        });
        if let Err(e) = self.execute(Box::new(ReplaceRegion { before, after })) {
            self.message = Some(e.to_string());
        }
        true
    }

    /// Inserts an empty area at the insertion point and selects it. Without an insertion point
    /// it gives a message and returns false.
    pub fn insert_area_at_crosshair(&mut self) -> bool {
        match self.crosshair {
            Some(at) => {
                self.insert_area(at);
                true
            }
            None => {
                self.message = Some(tr("edit.click_first").to_string());
                false
            }
        }
    }

    /// Inserts a solve block skeleton (a guess, a constraint and a `find` call, which the user
    /// edits) at the insertion point and selects it. Without an insertion point it gives a message
    /// and returns false.
    pub fn insert_solve_block_at_crosshair(&mut self) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let Some(at) = self.crosshair else {
            self.message = Some(tr("edit.click_first").to_string());
            return false;
        };
        let spec = mathcraft_doc::SolveBlockSpec {
            guesses: vec!["x:=1".into()],
            constraints: vec!["x^2==4".into()],
            solver: "find(x)".into(),
            target: None,
        };
        match self.doc.insert_solve_block(&spec, Placement::At(at)) {
            Ok(ids) => {
                self.changed();
                self.selection = vec![ids.block];
                self.crosshair = None;
                self.message = None;
                true
            }
            Err(e) => {
                self.message = Some(e.to_string());
                false
            }
        }
    }

    /// Switches the selected regions off, or on when all of them are off already (Bereich
    /// deaktivieren). Disabled regions are not evaluated. Returns the new state, `None` when
    /// nothing is selected.
    pub fn toggle_disabled(&mut self) -> Option<bool> {
        if self.draft.is_some() && self.commit().is_err() {
            return None;
        }
        if self.selection.is_empty() {
            return None;
        }
        let doc = self.doc.document();
        let off = !self.selection.iter().all(|id| doc.is_disabled(*id));
        let ids = self.selection.clone();
        self.doc.set_disabled(&ids, off);
        Some(off)
    }

    /// True when every selected region is disabled.
    pub fn selection_disabled(&self) -> bool {
        !self.selection.is_empty()
            && self
                .selection
                .iter()
                .all(|id| self.doc.document().is_disabled(*id))
    }

    /// Inserts the image at the insertion point (see [`EditSession::insert_image`]). Without an
    /// insertion point the bytes are not read into the document and the error says so.
    pub fn insert_image_at_crosshair(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let Some(at) = self.crosshair else {
            return Err(tr("edit.click_first").to_string());
        };
        self.insert_image(bytes, at)
    }

    /// Puts the selected regions into a new area that surrounds them, as one undo step. The members
    /// keep their positions. The area takes their bounding box, with a margin, and the header bar
    /// above it. The area is selected afterwards. Returns false (with a message) when nothing can
    /// be put in an area: no selection, a region without a position, or a locked area.
    pub fn area_around_selection(&mut self) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let mut members: Vec<(usize, RegionModel)> = self
            .selection
            .iter()
            .filter_map(|&id| {
                let index = self.doc.index_of(id)?;
                Some((index, self.doc.regions()[index].clone()))
            })
            .collect();
        if members.is_empty() {
            self.message = Some(tr("edit.area_needs_selection").to_string());
            return false;
        }
        if members
            .iter()
            .any(|(_, r)| r.pos.is_none() || is_locked_area(r))
        {
            self.message = Some(tr("edit.area_member_refused").to_string());
            return false;
        }
        members.sort_by_key(|(index, _)| *index);
        let (mut left, mut top) = (f64::INFINITY, f64::INFINITY);
        let (mut right, mut bottom) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (_, region) in &members {
            let (x, y) = region.pos.unwrap_or((0.0, 0.0));
            let (w, h) = region.size.unwrap_or((0.0, 0.0));
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + w);
            bottom = bottom.max(y + h);
        }
        let header = f64::from(crate::viewmodel::AREA_HEADER_PT);
        let x0 = (left - AREA_MARGIN_PT).max(0.0);
        let y0 = (top - AREA_MARGIN_PT - header).max(0.0);
        let size = (right + AREA_MARGIN_PT - x0, bottom + AREA_MARGIN_PT - y0);
        let id = self.doc.allocate_id();
        let area = RegionModel {
            id,
            pos: Some((x0, y0)),
            size: Some(size),
            result_ref: None,
            kind: RegionKind::Area(AreaModel::default()),
            solve_category: None,
            children: members.iter().map(|(_, r)| r.clone()).collect(),
            result_format: None,
            matrix: Default::default(),
        };
        // Remove the highest index first, so the lower indices stay valid; the area takes the
        // place of the first member.
        let first = members[0].0;
        let mut steps: Vec<Box<dyn Command>> = members
            .iter()
            .rev()
            .map(|(index, region)| {
                Box::new(RemoveRegion {
                    index: *index,
                    region: region.clone(),
                }) as Box<dyn Command>
            })
            .collect();
        steps.push(Box::new(InsertRegion {
            index: first,
            region: area,
        }));
        match self.execute(Box::new(Batch::new(tr("edit.cmd_area"), steps))) {
            Ok(()) => {
                self.selection = vec![id];
                self.crosshair = None;
                self.message = None;
                true
            }
            Err(e) => {
                self.message = Some(e.to_string());
                false
            }
        }
    }

    /// Locks or unlocks an area (one undo step). A locked area is not moved, deleted or edited, and
    /// its display state cannot be changed. Returns false when `id` is not a top-level area.
    pub fn toggle_area_lock(&mut self, id: u32) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let Some(before) = self.doc.region(id).cloned() else {
            return false;
        };
        let RegionKind::Area(area) = before.kind.clone() else {
            return false;
        };
        let mut after = before.clone();
        after.kind = RegionKind::Area(AreaModel {
            locked: !area.locked,
            ..area
        });
        if let Err(e) = self.execute(Box::new(ReplaceRegion { before, after })) {
            self.message = Some(e.to_string());
        }
        true
    }

    /// Starts resizing the selected image by its corner `corner`. Needs exactly one selected image
    /// with a position and a size. The drag then moves the handle (see `drag_to`), and the release
    /// makes the new size one undo step (see `end_drag`).
    pub fn begin_resize(&mut self, corner: Corner) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let Some(id) = self.selection() else {
            return false;
        };
        let Some(region) = self.doc.region(id) else {
            return false;
        };
        let (Some(pos), Some(size)) = (region.pos, region.size) else {
            return false;
        };
        if !matches!(region.kind, RegionKind::Image(_) | RegionKind::Plot(_))
            || size.0 <= 0.0
            || size.1 <= 0.0
        {
            return false;
        }
        self.resize = Some(Resize {
            id,
            corner,
            pos,
            size,
            delta: [0.0, 0.0],
        });
        true
    }

    /// The selected image's frame `(id, [left, top, width, height])` in page points, while it is
    /// the only selection. During a resize it is the frame the image will have on release.
    pub fn image_frame(&self) -> Option<(u32, [f64; 4])> {
        if let Some(resize) = self.resize {
            let (pos, size) = resize_target(&resize);
            return Some((resize.id, [pos.0, pos.1, size.0, size.1]));
        }
        let id = self.selection()?;
        let region = self.doc.region(id)?;
        if !matches!(region.kind, RegionKind::Image(_) | RegionKind::Plot(_)) {
            return None;
        }
        let (x, y) = region.pos?;
        let (w, h) = region.size?;
        Some((id, [x, y, w, h]))
    }

    /// Writes the resized frame of an image as one undo step. A frame that did not change does
    /// nothing.
    fn resize_region(&mut self, resize: Resize) -> Result<(), String> {
        let (pos, size) = resize_target(&resize);
        if pos == resize.pos && size == resize.size {
            return Ok(());
        }
        let result = self
            .doc
            .set_frame(resize.id, pos, size)
            .map_err(|e| e.to_string());
        self.changed();
        result
    }

    /// Adds a top-level region of `kind` at `at`, with `size`, and selects it.
    fn insert_region(&mut self, at: [f64; 2], kind: RegionKind, size: (f64, f64)) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        match self.doc.insert_region(kind, Some(size), Placement::At(at)) {
            Ok(id) => {
                self.changed();
                self.selection = vec![id];
                self.crosshair = None;
            }
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    /// The selected top-level region, when exactly one region is selected.
    pub fn selection(&self) -> Option<u32> {
        match self.selection.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    /// All selected top-level regions.
    pub fn selected(&self) -> &[u32] {
        &self.selection
    }

    /// True while a draft is being typed (its text is not in the document yet).
    pub fn has_draft(&self) -> bool {
        self.draft.is_some()
    }

    /// The insertion point in page points, if one is set.
    pub fn crosshair(&self) -> Option<[f64; 2]> {
        self.crosshair
    }

    /// A short note for the user about the last action, if there is one.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Counts committed changes. Changes when the document changes, so a view can be refreshed.
    pub fn revision(&self) -> u64 {
        self.doc.revision()
    }

    pub fn can_undo(&self) -> bool {
        self.doc.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.doc.can_redo()
    }

    /// The label of the change Undo would revert.
    pub fn undo_label(&self) -> Option<&'static str> {
        self.doc.undo_label()
    }

    /// The draft as the canvas shows it, if there is one.
    pub fn draft_view(&self) -> Option<DraftView> {
        let draft = self.draft.as_ref()?;
        let mut tree = None;
        let (text, caret, selection, math) = match &draft.kind {
            DraftKind::Math(ed) => {
                // The placeholder is drawn as `_`: one character for one, so the indices hold.
                let (text, caret, selection) = ed.display_selected();
                tree = Some(ed.tree()).filter(crate::draftlayout::DraftTree::is_structured);
                (text.replace(PLACEHOLDER, "_"), caret, selection, true)
            }
            DraftKind::Text(t, _) => (t.text(), t.caret(), t.selection(), false),
        };
        Some(DraftView {
            target: draft.target,
            furniture: draft.furniture.is_some(),
            at: self.draft_origin(draft),
            text,
            caret,
            selection,
            math,
            tree,
        })
    }

    /// The top-left corner of a draft's box. A new region's `at` is the snapped click point, the
    /// box's bottom-left corner: the box sits in the grid rows above it (one row per started
    /// grid pitch of height), never above the margin box. Other drafts start at `at`.
    fn draft_origin(&self, draft: &Draft) -> [f64; 2] {
        if draft.target.is_some() || draft.furniture.is_some() {
            return draft.at;
        }
        let grid = f64::from(crate::canvas::grid_points(self.doc.page()));
        let height = match &draft.kind {
            DraftKind::Math(ed) => {
                let tree = ed.tree();
                if tree.is_structured() {
                    f64::from(tree.layout(crate::viewmodel::MATH_SIZE).height())
                } else {
                    0.0
                }
            }
            DraftKind::Text(t, _) => t.text().matches('\n').count() as f64 * grid,
        };
        [draft.at[0], box_top(draft.at[1], height, grid)]
    }

    /// A pointer action inside the draft being typed. `index` is a character index of
    /// `DraftView::text`, as the canvas works it out from the mouse. Does nothing without a draft.
    pub fn draft_pointer(&mut self, pointer: DraftPointer) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        match (&mut draft.kind, pointer) {
            (DraftKind::Math(ed), DraftPointer::Press(i)) => ed.place_caret(i),
            (DraftKind::Math(ed), DraftPointer::Drag(i)) => ed.extend_to(i),
            (DraftKind::Math(ed), DraftPointer::Word(i)) => ed.select_word_at(i),
            (DraftKind::Text(t, _), DraftPointer::Press(i)) => t.place_caret(i),
            (DraftKind::Text(t, _), DraftPointer::Drag(i)) => t.extend_to(i),
            (DraftKind::Text(t, _), DraftPointer::Word(i)) => t.select_word_at(i),
        }
    }

    /// A click on the page at `point` (page points). `hit` is the top-level or member region
    /// under the point, if any (see `ViewModel::hit_test`). A draft is committed first; if that
    /// fails the click does nothing, so the draft is not lost.
    pub fn click(&mut self, point: [f64; 2], hit: Option<u32>) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        self.crosshair = None;
        match hit {
            Some(id) if self.doc.region(id).is_some() => {
                self.selection = vec![id];
                self.message = None;
            }
            Some(_) => {
                self.selection.clear();
                self.message = Some(tr("edit.solve_readonly").to_string());
            }
            None => {
                self.selection.clear();
                self.crosshair = Some(point);
                self.message = None;
            }
        }
    }

    /// Selects every top-level region (Ctrl+A).
    pub fn select_all(&mut self) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        self.selection = self.doc.regions().iter().map(|r| r.id).collect();
        self.crosshair = None;
        self.message = None;
    }

    /// Starts a drag on the page. `hit` is the region under the press. A region that is not
    /// selected becomes the only selection; a selected one keeps the whole selection, so the
    /// group moves. Returns false when nothing movable is under the press.
    pub fn begin_drag(&mut self, hit: Option<u32>) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let Some(id) = hit.filter(|id| self.movable(*id)) else {
            return false;
        };
        if !self.selection.contains(&id) {
            self.selection = vec![id];
        }
        self.crosshair = None;
        let ids: Vec<u32> = self
            .selection
            .iter()
            .copied()
            .filter(|id| self.movable(*id))
            .collect();
        self.drag = Some(Drag {
            ids,
            delta: [0.0, 0.0],
            copy: false,
        });
        true
    }

    /// A press on the page that starts a drag. On a movable region it starts a move (see
    /// `begin_drag`); elsewhere it starts a rubber band that selects the regions it covers when the
    /// mouse is released.
    pub fn begin_press(&mut self, point: [f64; 2], hit: Option<u32>) {
        if self.begin_drag(hit) {
            return;
        }
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        self.marquee = Some(Marquee {
            start: point,
            end: point,
        });
    }

    /// The rubber band in progress as `[left, top, right, bottom]` in page points.
    pub fn marquee_rect(&self) -> Option<[f64; 4]> {
        self.marquee.map(band_rect)
    }

    /// Sets the total displacement of the drag (or the rubber band's corner) in progress, in page
    /// points.
    pub fn drag_to(&mut self, delta: [f64; 2]) {
        if let Some(drag) = self.drag.as_mut() {
            drag.delta = delta;
        }
        if let Some(resize) = self.resize.as_mut() {
            resize.delta = delta;
        }
        if let Some(m) = self.marquee.as_mut() {
            m.end = [m.start[0] + delta[0], m.start[1] + delta[1]];
        }
    }

    /// The regions being dragged and their displacement, while a drag is in progress.
    pub fn drag_preview(&self) -> Option<(&[u32], [f64; 2])> {
        self.drag
            .as_ref()
            .map(|d| (d.ids.as_slice(), self.snapped_delta(&d.ids, d.delta)))
    }

    /// The displacement the drag of `ids` really makes: the first region's corner is snapped to
    /// the grid, and the others keep their spacing to it. A drag moves regions on the grid.
    fn snapped_delta(&self, ids: &[u32], delta: [f64; 2]) -> [f64; 2] {
        // A drag that moved nothing moves nothing, even a region that is off the grid.
        if delta == [0.0, 0.0] {
            return delta;
        }
        let Some((x, y)) = ids
            .first()
            .and_then(|&id| self.doc.region(id))
            .and_then(|r| r.pos)
        else {
            return delta;
        };
        let to = crate::canvas::snap_to_grid(
            [x + delta[0], y + delta[1]],
            crate::canvas::grid_points(self.doc.page()),
        );
        [to[0] - x, to[1] - y]
    }

    /// Ends the drag and makes the move one undo step. A drag that moved nothing (or moved
    /// regions onto their own position at the page edge) changes nothing.
    pub fn end_drag(&mut self) {
        if let Some(resize) = self.resize.take() {
            if let Err(message) = self.resize_region(resize) {
                self.message = Some(message);
            }
            return;
        }
        if let Some(m) = self.marquee.take() {
            // A band smaller than 2 points is a plain press: it clears the selection.
            let rect = band_rect(m);
            if (rect[2] - rect[0]) < 2.0 && (rect[3] - rect[1]) < 2.0 {
                self.selection.clear();
            } else {
                self.select_within(rect);
            }
            return;
        }
        let Some(drag) = self.drag.take() else {
            return;
        };
        let delta = self.snapped_delta(&drag.ids, drag.delta);
        let result = if drag.copy {
            self.duplicate_regions(&drag.ids, delta)
        } else {
            self.move_regions(&drag.ids, delta)
        };
        if let Err(message) = result {
            self.message = Some(message);
        }
    }

    /// Selects the top-level regions whose box meets `rect` (`[left, top, right, bottom]`). A
    /// rectangle that covers nothing clears the selection.
    fn select_within(&mut self, rect: [f64; 4]) {
        let [x0, y0, x1, y1] = rect;
        self.selection = self
            .doc
            .regions()
            .iter()
            .filter(|r| {
                let Some((x, y)) = r.pos else {
                    return false;
                };
                let (w, h) = r.size.unwrap_or((0.0, 0.0));
                x < x1 && x + w > x0 && y < y1 && y + h > y0
            })
            .map(|r| r.id)
            .collect();
        self.crosshair = None;
        self.message = None;
    }

    /// True when the top-level region `id` exists and has a stored position, so it can move.
    fn movable(&self, id: u32) -> bool {
        self.doc
            .region(id)
            .is_some_and(|r| r.pos.is_some() && !is_locked_area(r))
    }

    /// Moves the given regions by `delta` as one undo step. Returns whether anything moved.
    fn move_regions(&mut self, ids: &[u32], delta: [f64; 2]) -> Result<bool, String> {
        let moved = self.doc.move_regions(ids, delta).map_err(|e| e.to_string());
        self.changed();
        moved
    }

    /// Starts a math region at the insertion point, even when a region is selected. Returns false
    /// (with a message) when there is no insertion point.
    pub fn start_math(&mut self) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        match self.crosshair {
            Some(at) => {
                self.draft = Some(Draft {
                    target: None,
                    at,
                    kind: DraftKind::Math(
                        MathEditor::new().with_decimal_comma(crate::i18n::decimal_comma()),
                    ),
                    furniture: None,
                });
                self.selection.clear();
                true
            }
            None => {
                self.message = Some(tr("edit.click_first").to_string());
                false
            }
        }
    }

    /// A double click on the region `hit` (a press outside the draft): starts editing it, and
    /// remembers the point so that the word under it is selected once the draft is drawn (see
    /// [`EditSession::take_word_point`]). Only math and text regions can be edited.
    pub fn edit_region_word(&mut self, point: [f64; 2], hit: Option<u32>) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let Some(id) = hit else {
            return;
        };
        if self.begin_edit(id) {
            self.selection.clear();
            self.word_point = Some(point);
        }
    }

    /// The page point (stored frame) of a double click that started editing, once; the canvas
    /// selects the word under it in the draft.
    pub fn take_word_point(&mut self) -> Option<[f64; 2]> {
        self.word_point.take()
    }

    /// Inserts a template into the math draft (starting one at the insertion point or on the
    /// selected math region). A text draft or no insertion point gives a message.
    pub fn insert_template(&mut self, template: Template) {
        if !self.ensure_draft() {
            return;
        }
        let evaluate = template == Template::Evaluate;
        match self.draft.as_mut().map(|d| &mut d.kind) {
            Some(DraftKind::Math(ed)) => ed.apply_template(template),
            _ => self.message = Some("A template goes into a math region.".to_string()),
        }
        if evaluate {
            let _ = self.commit();
        } else {
            self.refresh_math_message();
        }
    }

    /// Inserts `text` (a function name with its bracket, a unit, a symbol, a matrix) into the math
    /// draft, starting one as `insert_template` does. A text draft gives a message instead.
    pub fn insert_text(&mut self, text: &str) {
        if !self.ensure_draft() {
            return;
        }
        match self.draft.as_mut().map(|d| &mut d.kind) {
            Some(DraftKind::Math(ed)) => ed.insert_text(text),
            _ => self.message = Some(tr("edit.text_in_math").to_string()),
        }
        self.refresh_math_message();
    }

    /// The styles of the selected text of the text being typed, one per run, for the toggles of
    /// the Textformatierung tab. Empty when the draft is not a text or nothing is selected.
    pub fn selected_text_styles(&self) -> Vec<RunStyle> {
        let Some(Draft {
            kind: DraftKind::Text(t, base),
            ..
        }) = self.draft.as_ref()
        else {
            return Vec::new();
        };
        let Some((start, end)) = t.selection() else {
            return Vec::new();
        };
        base.edited(&t.text()).rich().styles_in(start, end)
    }

    /// Applies a character format to the selected text of the text being typed (Prime's
    /// Textformatierung buttons). Returns false when no text is selected in a text draft. The
    /// change is part of the region's edit: one undo step, with the text.
    pub fn format_text_selection(&mut self, change: &StyleChange) -> bool {
        self.edit_text_base(true, |rich, start, end| rich.styled(start, end, change))
    }

    /// Sets the alignment of the paragraphs of the selected text (or of the caret's paragraph).
    /// Returns false when the draft is not a text.
    pub fn align_text(&mut self, align: Align) -> bool {
        self.edit_text_base(false, |rich, start, end| rich.aligned(start, end, align))
    }

    /// Replaces the selection (or types at the caret) `text` as a hyperlink to `target`. The
    /// link is display only: nothing opens it. An empty `target` types plain text. Returns false
    /// when the draft is not a text or `text` has no printable characters.
    pub fn insert_link(&mut self, text: &str, target: &str) -> bool {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        let target = target.trim();
        if text.trim().is_empty() {
            return false;
        }
        let Some(draft) = self.draft.as_mut() else {
            return false;
        };
        let DraftKind::Text(t, base) = &mut draft.kind else {
            return false;
        };
        let start = t.selection().map_or(t.caret(), |(s, _)| s);
        for c in text.chars() {
            t.insert(c);
        }
        let n = text.chars().count();
        let edited = base.edited(&t.text());
        let rich = if target.is_empty() {
            edited.rich().clone()
        } else {
            edited.rich().styled(
                start,
                start + n,
                &StyleChange::Link(Some(target.to_owned())),
            )
        };
        *base = TextBody::from_rich(rich);
        true
    }

    /// Shared body of the text-draft formats: `f` gets the rich text of the draft (synced with the
    /// editor's text) and the selected range (the caret's empty range when `select_only` is false
    /// and nothing is selected), and returns the new rich text.
    fn edit_text_base(
        &mut self,
        select_only: bool,
        f: impl FnOnce(&RichText, usize, usize) -> RichText,
    ) -> bool {
        let Some(draft) = self.draft.as_mut() else {
            return false;
        };
        let DraftKind::Text(t, base) = &mut draft.kind else {
            return false;
        };
        let range = match t.selection() {
            Some(range) => range,
            None if select_only => return false,
            None => (t.caret(), t.caret()),
        };
        let (start, end) = range;
        let synced = base.edited(&t.text());
        *base = TextBody::from_rich(f(synced.rich(), start, end));
        true
    }

    /// Types one character: into the draft, or starts a draft at the insertion point or on the
    /// selected region.
    pub fn type_char(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        // Mathcad convention: a quote at an insertion point starts a text region.
        if c == '"' && self.draft.is_none() && self.selection.is_empty() && self.crosshair.is_some()
        {
            self.start_text();
            return;
        }
        // Prime's shortcut: `@` at an insertion point inserts an x-y plot.
        if c == '@' && self.draft.is_none() && self.selection.is_empty() && self.crosshair.is_some()
        {
            self.insert_plot_at_crosshair(false);
            return;
        }
        if !self.ensure_draft() {
            return;
        }
        // Typing `=` in a math region evaluates it at once (Prime's evaluation operator): the
        // region is committed with the `=`, and its result shows. A `=` that changes nothing (the
        // second `=` of a `:=`) does not.
        let mut evaluate = false;
        if let Some(draft) = self.draft.as_mut() {
            let before = match &draft.kind {
                DraftKind::Math(ed) => Some(ed.to_text()),
                DraftKind::Text(..) => None,
            };
            apply_key(&mut draft.kind, EditKey::Char(c));
            if let (Some(before), DraftKind::Math(ed)) = (before, &draft.kind) {
                evaluate = c == '=' && ed.to_text() != before;
            }
        }
        if evaluate {
            let _ = self.commit();
        } else {
            self.refresh_math_message();
        }
    }

    /// Clears the parse error of a math draft once its text reads as an expression again. The
    /// error stays while the text does not parse.
    fn refresh_math_message(&mut self) {
        if self.message.is_none() {
            return;
        }
        let valid = match self.draft.as_ref().map(|d| &d.kind) {
            Some(DraftKind::Math(ed)) => {
                let ctx = EditContext::for_regions(self.doc.regions());
                parse_math_in(&ed.to_text(), &ctx).is_ok()
            }
            _ => false,
        };
        if valid {
            self.message = None;
        }
    }

    /// A key that is not text: editing keys go to the draft; with no draft, Delete removes the
    /// selected region and arrow keys move it.
    pub fn key(&mut self, key: EditKey) {
        let Some(draft) = self.draft.as_ref() else {
            match key {
                // Delete and Backspace remove the selected regions (Prime: both do).
                EditKey::Delete | EditKey::Backspace if !self.selection.is_empty() => {
                    self.delete_selected();
                }
                EditKey::Left | EditKey::TermLeft => self.nudge(-NUDGE, 0.0),
                EditKey::Right | EditKey::TermRight => self.nudge(NUDGE, 0.0),
                EditKey::Up => self.nudge(0.0, -NUDGE),
                EditKey::Down => self.nudge(0.0, NUDGE),
                // Escape with nothing being typed clears the selection and the insertion point.
                EditKey::Escape => {
                    self.selection.clear();
                    self.crosshair = None;
                    self.message = None;
                }
                _ => {}
            }
            return;
        };
        let is_math = matches!(draft.kind, DraftKind::Math(_));
        match key {
            EditKey::Enter if is_math => {
                self.enter_math();
            }
            EditKey::ShiftEnter if is_math => {
                let inserted = match self.draft.as_mut().map(|d| &mut d.kind) {
                    Some(DraftKind::Math(ed)) => ed.matrix_op(MatrixOp::InsertRow),
                    _ => false,
                };
                if inserted {
                    self.refresh_math_message();
                } else {
                    let _ = self.commit();
                }
            }
            // Escape leaves the region as it was typed: the content is kept.
            EditKey::Escape => {
                let _ = self.commit();
            }
            _ => {
                if let Some(draft) = self.draft.as_mut() {
                    apply_key(&mut draft.kind, key);
                }
                self.refresh_math_message();
            }
        }
    }

    /// Enter in a math draft (Prime): commits the region and puts the insertion point one grid
    /// row below it, with a fresh empty draft, so the next typing makes a new region. A draft
    /// that fails to read stays open, and an empty new draft is only closed.
    fn enter_math(&mut self) {
        let Some(draft) = self.draft.clone() else {
            return;
        };
        let grid = f64::from(crate::canvas::grid_points(self.doc.page()));
        let x = match draft.target.and_then(|id| self.doc.region(id)) {
            Some(r) => r.pos.map_or(draft.at[0], |p| p.0),
            None => draft.at[0],
        };
        // The bottom edge of the region being left: a new region's anchor is its bottom-left
        // corner; an edited one is measured from its stored top and size.
        let bottom = match draft.target.and_then(|id| self.doc.region(id)) {
            Some(r) => {
                let (top, h) = (r.pos.map_or(0.0, |p| p.1), r.size.map_or(grid, |s| s.1));
                ((top + h) / grid - 1e-9).ceil().max(1.0) * grid
            }
            None => draft.at[1],
        };
        if !matches!(self.commit(), Ok(true)) {
            return;
        }
        self.selection.clear();
        self.crosshair = Some([x, bottom + grid]);
        self.start_math();
    }

    /// Starts a text region at the insertion point. Returns false, with a message, when there is
    /// no insertion point.
    pub fn start_text(&mut self) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        match self.crosshair {
            Some(at) => {
                self.draft = Some(Draft {
                    target: None,
                    at,
                    kind: DraftKind::Text(TextEditor::new(""), TextBody::from_plain("")),
                    furniture: None,
                });
                self.selection.clear();
                true
            }
            None => {
                self.message = Some(tr("edit.click_text_first").to_string());
                false
            }
        }
    }

    /// Starts editing the page header (`footer` false) or the footer. A part with no text yet
    /// starts empty, and its region is made when the draft commits. Returns false when the draft
    /// before it could not be committed.
    pub fn start_furniture(&mut self, footer: bool) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        let base = self.doc.furniture_text(footer);
        let (origin, _) = self.doc.furniture_frame(footer);
        let editor = TextEditor::new(base.as_str());
        self.draft = Some(Draft {
            target: None,
            at: origin,
            kind: DraftKind::Text(editor, base),
            furniture: Some(footer),
        });
        self.selection.clear();
        self.crosshair = None;
        self.message = None;
        true
    }

    /// Types a header or footer field, such as `{page}`, into the header or footer being edited.
    /// Outside one it gives a message.
    pub fn insert_field(&mut self, field: &str) {
        match self.draft.as_mut() {
            Some(draft) if draft.furniture.is_some() => {
                for c in field.chars() {
                    apply_key(&mut draft.kind, EditKey::Char(c));
                }
            }
            _ => self.message = Some(tr("edit.field_needs_header").to_string()),
        }
    }

    /// Removes the fields whose name starts with `prefix` (`page`, `date`) from the header and the
    /// footer. The header or footer being edited is committed first. A message when there was
    /// none.
    pub fn remove_field(&mut self, prefix: &str) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let mut removed = false;
        for footer in [false, true] {
            let text = self.doc.furniture_text(footer);
            let cleaned = crate::furniture::remove_tokens(text.as_str(), prefix);
            if cleaned != text.as_str() {
                self.doc.set_furniture(footer, text.edited(&cleaned));
                removed = true;
            }
        }
        if removed {
            self.changed();
        } else {
            self.message = Some(tr("edit.field_none").to_string());
        }
    }

    /// Inserts a page break at the insertion point (a stored sheet position) and selects it. The
    /// next page then starts at the break. One undo step. Without an insertion point it gives a
    /// message.
    pub fn insert_page_break(&mut self) {
        match self.crosshair {
            Some(at) => self.insert_region(at, RegionKind::PageBreak, (0.0, 0.0)),
            None => self.message = Some(tr("edit.page_break_needs_point").to_string()),
        }
    }

    /// Deletes a page break, as one undo step. The break is the selected one, else the one nearest
    /// the insertion point within [`PAGE_BREAK_REACH_PT`]. A message when there is none.
    pub fn delete_page_break(&mut self) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let is_break = |doc: &Document, id: u32| {
            doc.region(id)
                .is_some_and(|r| matches!(r.kind, RegionKind::PageBreak))
        };
        let id = match (self.selection(), self.crosshair) {
            (Some(id), _) if is_break(&self.doc, id) => Some(id),
            (_, Some(at)) => self
                .doc
                .regions()
                .iter()
                .filter(|r| matches!(r.kind, RegionKind::PageBreak))
                .filter_map(|r| r.pos.map(|(_, y)| ((y - at[1]).abs(), r.id)))
                .filter(|(distance, _)| *distance <= PAGE_BREAK_REACH_PT)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, id)| id),
            _ => None,
        };
        match id {
            Some(id) => {
                self.selection = vec![id];
                self.delete_selected();
            }
            None => self.message = Some(tr("edit.page_break_none").to_string()),
        }
    }

    /// Commits the draft. `Ok(false)` when there was no draft or nothing was typed into a new
    /// region. An `Err` keeps the draft open, with the reason in `message`.
    pub fn commit(&mut self) -> Result<bool, String> {
        let Some(draft) = self.draft.clone() else {
            return Ok(false);
        };
        let kind = match &draft.kind {
            DraftKind::Math(ed) => {
                if draft.target.is_none() && ed.is_blank() {
                    self.draft = None;
                    return Ok(false);
                }
                let text = ed.to_text();
                let ctx = EditContext::for_regions(self.doc.regions());
                let parsed = parse_math_in(&text, &ctx);
                // An expression whose text was not changed keeps its stored tree. When the text
                // does not read back to that tree (the printer and parser disagree on it), the
                // region is left as it is, rather than replaced by a different expression.
                let original =
                    draft
                        .target
                        .and_then(|id| self.doc.region(id))
                        .and_then(|r| match &r.kind {
                            RegionKind::Math(e) => Some(e.clone()),
                            _ => None,
                        });
                if let Some(orig) = original {
                    if MathEditor::from_text(&orig.to_string()).to_text() == text {
                        if matches!(&parsed, Ok(p) if *p == orig) {
                            RegionKind::Math(orig)
                        } else {
                            self.draft = None;
                            self.message = Some(unread_notice().to_string());
                            return Ok(false);
                        }
                    } else {
                        match parsed {
                            Ok(expr) => RegionKind::Math(expr),
                            Err(e) => {
                                return self.fail_math(trf("edit.cannot_read", &[&e.to_string()]))
                            }
                        }
                    }
                } else {
                    match parsed {
                        Ok(expr) => RegionKind::Math(expr),
                        Err(e) => {
                            return self.fail_math(trf("edit.cannot_read", &[&e.to_string()]))
                        }
                    }
                }
            }
            DraftKind::Text(t, base) => {
                if let Some(footer) = draft.furniture {
                    let body = base.edited(&t.text());
                    self.draft = None;
                    self.message = None;
                    // An empty part opened and left empty makes no region.
                    if body.as_str().is_empty()
                        && self.doc.furniture_text(footer).as_str().is_empty()
                    {
                        return Ok(false);
                    }
                    // A header or footer keeps its text even when it is emptied: the region stays.
                    self.doc.set_furniture(footer, body);
                    self.changed();
                    return Ok(true);
                }
                if draft.target.is_none() && t.is_empty() {
                    self.draft = None;
                    return Ok(false);
                }
                RegionKind::Text(base.edited(&t.text()))
            }
        };
        match self.store(draft.target, self.draft_origin(&draft), kind) {
            Ok(()) => {
                self.draft = None;
                self.message = None;
                Ok(true)
            }
            Err(message) => {
                self.message = Some(message.clone());
                Err(message)
            }
        }
    }

    fn fail_math(&mut self, message: String) -> Result<bool, String> {
        self.message = Some(message.clone());
        Err(message)
    }

    /// Drops the draft without changing the document.
    pub fn cancel(&mut self) {
        self.draft = None;
        self.message = None;
    }

    /// Undoes the last committed change. A draft is committed first. Returns true when the
    /// document changed.
    pub fn undo(&mut self) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        match self.doc.undo() {
            Ok(true) => {
                self.changed();
                true
            }
            Ok(false) => false,
            Err(e) => {
                self.message = Some(e.to_string());
                false
            }
        }
    }

    /// Redoes the last undone change. Returns true when the document changed.
    pub fn redo(&mut self) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        match self.doc.redo() {
            Ok(true) => {
                self.changed();
                true
            }
            Ok(false) => false,
            Err(e) => {
                self.message = Some(e.to_string());
                false
            }
        }
    }

    /// Removes the selected regions (one undo step).
    pub fn delete_selected(&mut self) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        if self.selection.is_empty() {
            self.message = Some(tr("edit.select_first").to_string());
            return;
        }
        let ids = std::mem::take(&mut self.selection);
        match self.doc.delete_regions(&ids) {
            Ok(deleted) => {
                if deleted.locked_skipped {
                    self.message = Some(tr("edit.area_locked").to_string());
                }
            }
            Err(e) => self.message = Some(e.to_string()),
        }
        self.changed();
    }

    /// Moves the selected regions by `(dx, dy)` points, as one undo step. Regions without a stored
    /// position cannot be moved.
    pub fn nudge(&mut self, dx: f64, dy: f64) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        if self.selection.is_empty() {
            return;
        }
        let ids = self.selection.clone();
        if !ids.iter().any(|id| self.movable(*id)) {
            self.message = Some(tr("edit.no_position").to_string());
            return;
        }
        if let Err(message) = self.move_regions(&ids, [dx, dy]) {
            self.message = Some(message);
        }
    }

    /// Starts a draft for typing. Returns false (with a message) when there is nothing to type
    /// into, or when several regions are selected.
    fn ensure_draft(&mut self) -> bool {
        if self.draft.is_some() {
            return true;
        }
        if self.selection.len() > 1 {
            self.message = Some(tr("edit.select_one").to_string());
            return false;
        }
        if let Some(id) = self.selection() {
            return self.begin_edit(id);
        }
        if let Some(at) = self.crosshair {
            self.draft = Some(Draft {
                target: None,
                at,
                kind: DraftKind::Math(
                    MathEditor::new().with_decimal_comma(crate::i18n::decimal_comma()),
                ),
                furniture: None,
            });
            return true;
        }
        self.message = Some(tr("edit.click_insert").to_string());
        false
    }

    /// Starts a draft that edits the region `id`. Math regions are opened as their printed text.
    fn begin_edit(&mut self, id: u32) -> bool {
        let Some(region) = self.doc.region(id) else {
            return false;
        };
        let kind = match &region.kind {
            RegionKind::Math(expr) => DraftKind::Math(
                MathEditor::from_text(&expr.to_string())
                    .with_decimal_comma(crate::i18n::decimal_comma()),
            ),
            RegionKind::Text(text) => DraftKind::Text(TextEditor::new(text), text.clone()),
            _ => {
                self.message = Some(tr("edit.not_editable").to_string());
                return false;
            }
        };
        let at = region.pos.map_or([0.0, 0.0], |(x, y)| [x, y]);
        self.draft = Some(Draft {
            target: Some(id),
            at,
            kind,
            furniture: None,
        });
        self.crosshair = None;
        true
    }

    /// Writes a committed region: a replacement of `target`, or a new region at `at`.
    fn store(&mut self, target: Option<u32>, at: [f64; 2], kind: RegionKind) -> Result<(), String> {
        match target {
            Some(id) => {
                let result = self.doc.replace_kind(id, kind).map_err(|e| e.to_string());
                self.changed();
                result
            }
            None => {
                let id = self
                    .doc
                    .insert_region(kind, None, Placement::At(at))
                    .map_err(|e| e.to_string())?;
                self.changed();
                self.selection = vec![id];
                self.crosshair = None;
                Ok(())
            }
        }
    }

    /// Inserts a matrix of `rows` by `cols` cells into the math draft (starting one as
    /// `insert_template` does).
    pub fn insert_matrix(&mut self, rows: usize, cols: usize) {
        if !self.ensure_draft() {
            return;
        }
        match self.draft.as_mut().map(|d| &mut d.kind) {
            Some(DraftKind::Math(ed)) => ed.insert_matrix(rows, cols),
            _ => self.message = Some("A matrix goes into a math region.".to_string()),
        }
    }

    /// Adds or deletes a matrix row or column at the caret of the math draft.
    pub fn matrix_op(&mut self, op: MatrixOp) {
        let done = match self.draft.as_mut().map(|d| &mut d.kind) {
            Some(DraftKind::Math(ed)) => ed.matrix_op(op),
            _ => false,
        };
        if !done {
            self.message = Some(
                "Put the insertion point in a matrix. A matrix keeps at least one row and one \
                 column and stays within its size limit."
                    .to_string(),
            );
        }
    }

    /// Shift+click: adds the region under the press to the selection, or takes it out when it is
    /// already selected. A press on empty page does nothing.
    pub fn toggle_selection(&mut self, hit: Option<u32>) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let Some(id) = hit.filter(|id| self.doc.region(*id).is_some()) else {
            return;
        };
        match self.selection.iter().position(|s| *s == id) {
            Some(index) => {
                self.selection.remove(index);
            }
            None => self.selection.push(id),
        }
        self.crosshair = None;
        self.message = None;
    }

    /// Starts a drag that copies the selected regions (Ctrl+drag). The copies appear where the
    /// drag ends and are one undo step. Returns false when nothing movable is under the press.
    pub fn begin_copy_drag(&mut self, hit: Option<u32>) -> bool {
        let started = self.begin_drag(hit);
        if let Some(drag) = self.drag.as_mut() {
            drag.copy = true;
        }
        started
    }

    /// Arranges the selected regions (Format tab): aligns or distributes them as one undo step.
    pub fn arrange(&mut self, how: Arrange) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let boxes = self.boxes_of(&self.selection);
        if boxes.len() < how.min_regions() {
            self.message = Some(trf("edit.arrange_min", &[&how.min_regions().to_string()]));
            return;
        }
        let targets = layout::arrange(&boxes, how);
        if let Err(message) = self.move_to(tr("edit.cmd_arrange"), &targets) {
            self.message = Some(message);
        }
    }

    /// Insert lines: moves every region at or below the insertion point down by `points` (one undo
    /// step). Needs an insertion point.
    pub fn insert_lines(&mut self, points: f64) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let Some(at) = self.crosshair else {
            self.message = Some(tr("edit.click_first").to_string());
            return;
        };
        if !(points.is_finite() && points > 0.0 && points <= MAX_INSERT_LINES) {
            return;
        }
        let ids: Vec<u32> = self.doc.regions().iter().map(|r| r.id).collect();
        let boxes = self.boxes_of(&ids);
        let targets = layout::insert_lines(&boxes, at[1], points);
        if let Err(message) = self.move_to(tr("edit.cmd_insert_lines"), &targets) {
            self.message = Some(message);
        }
    }

    /// Inserts one blank page: the regions at or below the insertion point move down by the
    /// height of a page's content area (paper minus margins). One undo step.
    pub fn insert_blank_page(&mut self) {
        let page = self.doc.page();
        let height = page.paper_points()[1];
        let margins = page.margins();
        let content = height - margins.top - margins.bottom;
        self.insert_lines(content.clamp(1.0, MAX_INSERT_LINES));
    }

    /// Remove space: closes up to `points` of the empty space below the insertion point (all of
    /// it for `f64::INFINITY`). One undo step. Needs an insertion point and a region below it.
    pub fn remove_lines(&mut self, points: f64) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let Some(at) = self.crosshair else {
            self.message = Some(tr("edit.click_first").to_string());
            return;
        };
        let ids: Vec<u32> = self.doc.regions().iter().map(|r| r.id).collect();
        let boxes = self.boxes_of(&ids);
        let targets = layout::remove_lines(&boxes, at[1], points);
        if targets.is_empty() {
            self.message = Some(tr("edit.no_space_below").to_string());
            return;
        }
        if let Err(message) = self.move_to(tr("edit.cmd_remove_lines"), &targets) {
            self.message = Some(message);
        }
    }

    /// Separate regions: moves overlapping selected regions apart, vertically or horizontally.
    /// One undo step. Needs at least two selected regions.
    pub fn separate_selected(&mut self, vertical: bool) {
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let ids = self.selection.clone();
        let boxes = self.boxes_of(&ids);
        if boxes.len() < 2 {
            self.message = Some(tr("edit.separate_needs_two").to_string());
            return;
        }
        let targets = layout::separate(&boxes, vertical, SEPARATE_GAP_PT);
        if targets.is_empty() {
            self.message = Some(tr("edit.nothing_overlaps").to_string());
            return;
        }
        if let Err(message) = self.move_to(tr("edit.cmd_separate"), &targets) {
            self.message = Some(message);
        }
    }

    /// The matches of a search over the stored worksheet (see `mathcraft_doc::search`). A draft
    /// being typed is not part of them until it is committed.
    pub fn find_matches(&self, query: &str, opts: SearchOptions) -> Vec<SearchMatch> {
        search::find(self.doc.document(), query, opts)
    }

    /// Brings a search hit into view: a text region opens in the text editor with the characters
    /// `range` selected. Any other region is selected. A draft is committed first; False when that
    /// fails or the region is gone.
    pub fn reveal_match(&mut self, id: u32, range: Option<Range<usize>>) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        if self.doc.region(id).is_none() {
            return false;
        }
        match range {
            Some(r) if self.begin_edit(id) => {
                self.selection.clear();
                self.draft_pointer(DraftPointer::Press(r.start));
                self.draft_pointer(DraftPointer::Drag(r.end));
                true
            }
            _ => self.select_region(id),
        }
    }

    /// Replaces one search hit as one undo step. A draft is committed first. A replacement that
    /// would not read is refused: the message says why, and nothing changes.
    pub fn replace_match(&mut self, m: &SearchMatch, replacement: &str) -> Result<(), String> {
        if self.draft.is_some() {
            self.commit()?;
        }
        match search::replace_one(&mut self.doc, m, replacement) {
            Ok(()) => {
                self.message = None;
                self.changed();
                Ok(())
            }
            Err(e) => {
                let text = e.to_string();
                self.message = Some(text.clone());
                Err(text)
            }
        }
    }

    /// Replaces every match of a search as one undo step. A draft is committed first. Regions whose
    /// result would not read are left as they were and listed in the report.
    pub fn replace_all_matches(
        &mut self,
        query: &str,
        replacement: &str,
        opts: SearchOptions,
        label: &'static str,
    ) -> Result<ReplaceReport, String> {
        if self.draft.is_some() {
            self.commit()?;
        }
        let report = search::replace_all(&mut self.doc, query, replacement, opts, label)
            .map_err(|e| e.to_string())?;
        self.message = None;
        self.changed();
        Ok(report)
    }

    /// Selects one top-level region (the error trace). False when there is no such region.
    pub fn select_region(&mut self, id: u32) -> bool {
        if self.draft.is_some() && self.commit().is_err() {
            return false;
        }
        if self.doc.region(id).is_none() {
            return false;
        }
        self.crosshair = None;
        self.selection = vec![id];
        self.message = None;
        true
    }

    /// The movable regions among `ids` (those with a stored position), as geometry.
    fn boxes_of(&self, ids: &[u32]) -> Vec<Box2> {
        ids.iter()
            .filter_map(|id| {
                let region = self.doc.region(*id)?;
                Some(Box2 {
                    id: *id,
                    pos: region.pos?,
                    size: region.size.unwrap_or((0.0, 0.0)),
                })
            })
            .collect()
    }

    /// Moves regions to the given positions as one undo step. Regions already there are not
    /// touched. Returns whether anything moved.
    fn move_to(
        &mut self,
        label: &'static str,
        targets: &[(u32, (f64, f64))],
    ) -> Result<bool, String> {
        let steps: Vec<Box<dyn Command>> = targets
            .iter()
            .filter_map(|&(id, to)| {
                let from = self.doc.region(id)?.pos?;
                (to != from).then(|| Box::new(MoveRegion { id, from, to }) as Box<dyn Command>)
            })
            .collect();
        if steps.is_empty() {
            return Ok(false);
        }
        self.execute(Box::new(Batch::new(label, steps)))
            .map(|()| true)
            .map_err(|e| e.to_string())
    }

    /// Copies the selected regions (Ctrl+C). While a math region is typed, copies the selected part
    /// of its expression instead. Returns the text for the system clipboard, or `None` with a
    /// message when there is nothing to copy.
    pub fn copy_selection(&mut self) -> Option<String> {
        if let Some(draft) = self.draft.as_ref() {
            return match &draft.kind {
                DraftKind::Math(ed) => match ed.selected_text() {
                    Some(text) => Some(text),
                    None => {
                        self.message = Some(tr("edit.select_part_copy").to_string());
                        None
                    }
                },
                DraftKind::Text(..) => {
                    self.message = Some(tr("edit.finish_text_copy").to_string());
                    None
                }
            };
        }
        let regions: Vec<&RegionModel> = self
            .selection
            .iter()
            .filter_map(|id| self.doc.region(*id))
            .collect();
        if regions.is_empty() {
            self.message = Some(tr("edit.select_copy").to_string());
            return None;
        }
        match clipboard::encode(&regions) {
            Ok(text) => Some(text),
            Err(e) => {
                self.message = Some(e.to_string());
                None
            }
        }
    }

    /// Cuts (Ctrl+X): copies as `copy_selection` does, then removes what was copied (one undo step).
    pub fn cut_selection(&mut self) -> Option<String> {
        if let Some(DraftKind::Math(ed)) = self.draft.as_mut().map(|d| &mut d.kind) {
            let cut = ed.cut_selection();
            if cut.is_none() {
                self.message = Some(tr("edit.select_part_cut").to_string());
            }
            return cut;
        }
        let text = self.copy_selection()?;
        self.delete_selected();
        Some(text)
    }

    /// Pastes (Ctrl+V) the text of the system clipboard. A fragment copied from Mathcraft comes
    /// back as regions at the insertion point (or next to the originals). Other text is typed into
    /// the math or text draft being edited; with no draft, each line becomes a math region at the
    /// insertion point. Refused text leaves the document unchanged.
    pub fn paste(&mut self, text: &str) {
        if text.len() > clipboard::MAX_CLIP_BYTES {
            self.message = Some(ClipError::TooLarge.to_string());
            return;
        }
        let fragment = text.lines().next() == Some(clipboard::HEADER);
        if !fragment && self.draft.is_some() {
            self.paste_into_draft(text);
            return;
        }
        if self.draft.is_some() && self.commit().is_err() {
            return;
        }
        let ctx = EditContext::for_regions(self.doc.regions());
        match clipboard::decode(text, &ctx) {
            Ok(Some(regions)) => {
                let anchor = self.crosshair;
                self.paste_regions(regions, anchor);
            }
            Ok(None) => self.paste_lines(text, &ctx),
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    /// Types pasted text into the draft that is open.
    fn paste_into_draft(&mut self, text: &str) {
        let typed: Vec<char> = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        if typed.len() > MAX_PASTE_CHARS {
            self.message = Some(trf("edit.paste_max", &[&MAX_PASTE_CHARS.to_string()]));
            return;
        }
        if let Some(draft) = self.draft.as_mut() {
            if let DraftKind::Math(ed) = &mut draft.kind {
                ed.insert_text(text);
                return;
            }
        }
        for c in typed {
            let key = if c == '\n' {
                EditKey::Enter
            } else {
                EditKey::Char(c)
            };
            if let Some(draft) = self.draft.as_mut() {
                apply_key(&mut draft.kind, key);
            }
        }
    }

    /// Plain text pasted with no draft open: one math region per non-empty line, stacked from the
    /// insertion point. Needs an insertion point; a line that does not read refuses the paste.
    fn paste_lines(&mut self, text: &str, ctx: &EditContext) {
        let Some(origin) = self.crosshair else {
            self.message = Some(tr("edit.click_paste").to_string());
            return;
        };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            return;
        }
        if lines.len() > clipboard::MAX_CLIP_REGIONS {
            self.message = Some(ClipError::TooManyRegions.to_string());
            return;
        }
        let mut regions = Vec::with_capacity(lines.len());
        for (k, line) in lines.iter().enumerate() {
            if line.chars().count() > MAX_PASTE_CHARS {
                self.message = Some(trf("edit.paste_max", &[&MAX_PASTE_CHARS.to_string()]));
                return;
            }
            match parse_math_in(line, ctx) {
                Ok(expr) => regions.push(Clipped {
                    pos: Some((0.0, k as f64 * LINE_STEP)),
                    size: None,
                    kind: RegionKind::Math(expr),
                }),
                Err(e) => {
                    self.message = Some(trf(
                        "edit.paste_line",
                        &[&(k + 1).to_string(), &e.to_string()],
                    ));
                    return;
                }
            }
        }
        self.paste_regions(regions, Some(origin));
    }

    /// Adds pasted regions as one undo step and selects them. With `anchor`, the top left of the
    /// group goes there; without, the group is offset from the originals.
    fn paste_regions(&mut self, regions: Vec<Clipped>, anchor: Option<[f64; 2]>) {
        if regions.is_empty() {
            return;
        }
        if self.doc.regions().len() + regions.len() > MAX_REGIONS {
            self.message = Some(tr("edit.too_many").to_string());
            return;
        }
        let corner = regions
            .iter()
            .filter_map(|c| c.pos)
            .fold(None, |acc: Option<(f64, f64)>, p| match acc {
                None => Some(p),
                Some(a) => Some((a.0.min(p.0), a.1.min(p.1))),
            })
            .unwrap_or((0.0, 0.0));
        let delta = match anchor {
            Some(at) => [at[0] - corner.0, at[1] - corner.1],
            None => [PASTE_OFFSET, PASTE_OFFSET],
        };
        let fallback = anchor.unwrap_or([PASTE_OFFSET, PASTE_OFFSET]);
        let base = self.doc.regions().len();
        let mut steps: Vec<Box<dyn Command>> = Vec::with_capacity(regions.len());
        let mut ids = Vec::with_capacity(regions.len());
        for (k, clipped) in regions.into_iter().enumerate() {
            let id = self.doc.allocate_id();
            ids.push(id);
            let pos = match clipped.pos {
                Some(p) => moved_position(p, delta),
                None => (fallback[0], fallback[1]),
            };
            steps.push(Box::new(InsertRegion {
                index: base + k,
                region: RegionModel {
                    id,
                    pos: Some(pos),
                    size: clipped.size,
                    result_ref: None,
                    kind: clipped.kind,
                    solve_category: None,
                    children: Vec::new(),
                    result_format: None,
                    matrix: Default::default(),
                },
            }));
        }
        match self.execute(Box::new(Batch::new(tr("edit.cmd_paste"), steps))) {
            Ok(()) => {
                self.selection = ids;
                self.crosshair = None;
                self.message = None;
            }
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    /// Copies the regions `ids` to new regions moved by `delta` (one undo step). Only math and text
    /// regions can be copied; any other kind refuses the copy.
    fn duplicate_regions(&mut self, ids: &[u32], delta: [f64; 2]) -> Result<bool, String> {
        let originals: Vec<RegionModel> = ids
            .iter()
            .filter_map(|id| self.doc.region(*id).cloned())
            .collect();
        if let Some(kind) = originals.iter().find_map(|r| clipboard::kind_name(&r.kind)) {
            return Err(ClipError::NotCopyable(kind).to_string());
        }
        if originals.is_empty() {
            return Ok(false);
        }
        if self.doc.regions().len() + originals.len() > MAX_REGIONS {
            return Err(tr("edit.too_many").to_string());
        }
        let base = self.doc.regions().len();
        let mut steps: Vec<Box<dyn Command>> = Vec::with_capacity(originals.len());
        let mut new_ids = Vec::with_capacity(originals.len());
        for (k, original) in originals.into_iter().enumerate() {
            let id = self.doc.allocate_id();
            new_ids.push(id);
            steps.push(Box::new(InsertRegion {
                index: base + k,
                region: RegionModel {
                    id,
                    pos: original.pos.map(|p| moved_position(p, delta)),
                    size: original.size,
                    result_ref: None,
                    kind: original.kind,
                    solve_category: None,
                    children: Vec::new(),
                    result_format: None,
                    matrix: Default::default(),
                },
            }));
        }
        self.execute(Box::new(Batch::new(tr("edit.cmd_duplicate"), steps)))
            .map_err(|e| e.to_string())?;
        self.selection = new_ids;
        Ok(true)
    }

    fn execute(&mut self, cmd: Box<dyn Command>) -> Result<(), EditError> {
        self.doc.execute(cmd)?;
        self.changed();
        Ok(())
    }

    /// Bookkeeping after the document changed by an undo, redo or new command.
    fn changed(&mut self) {
        let doc = &self.doc;
        self.selection.retain(|id| doc.region(*id).is_some());
        // A drag belongs to the state it started in; an undo or redo during one ends it.
        self.drag = None;
        self.marquee = None;
        self.resize = None;
    }
}

/// The position and size an image resize gives, from the drag in progress.
fn resize_target(resize: &Resize) -> ((f64, f64), (f64, f64)) {
    resize::resized(resize.pos, resize.size, resize.corner, resize.delta)
}

/// True for a locked area. Its regions and its position are not changed by the editor.
fn is_locked_area(region: &RegionModel) -> bool {
    matches!(&region.kind, RegionKind::Area(area) if area.locked)
}

/// The rectangle a rubber band covers, as `[left, top, right, bottom]`.
fn band_rect(m: Marquee) -> [f64; 4] {
    [
        m.start[0].min(m.end[0]),
        m.start[1].min(m.end[1]),
        m.start[0].max(m.end[0]),
        m.start[1].max(m.end[1]),
    ]
}

/// Shown when an unedited expression does not read back to its stored tree.
pub fn unread_notice() -> &'static str {
    tr("edit.structural")
}

/// Applies an editing key to a draft. Keys that do not apply to the kind are ignored.
fn apply_key(kind: &mut DraftKind, key: EditKey) {
    match kind {
        DraftKind::Math(ed) => match key {
            EditKey::Char(c) => ed.key(c),
            EditKey::Left => ed.left(),
            EditKey::Right => ed.right(),
            EditKey::TermLeft => ed.term_left(),
            EditKey::TermRight => ed.term_right(),
            EditKey::SelectLeft => ed.select_left(),
            EditKey::SelectRight => ed.select_right(),
            EditKey::Up => ed.vertical(false),
            EditKey::Down => ed.vertical(true),
            EditKey::Home => ed.home(),
            EditKey::End => ed.end(),
            EditKey::Tab => ed.tab(true),
            EditKey::BackTab => ed.tab(false),
            EditKey::Backspace => ed.backspace(),
            EditKey::Delete => ed.delete(),
            _ => {}
        },
        DraftKind::Text(t, _) => match key {
            EditKey::Char(c) => t.insert(c),
            EditKey::Enter => t.insert('\n'),
            EditKey::Left | EditKey::TermLeft => t.left(),
            EditKey::Right | EditKey::TermRight => t.right(),
            EditKey::ShiftEnter => t.insert('\n'),
            EditKey::SelectLeft => t.select_left(),
            EditKey::SelectRight => t.select_right(),
            EditKey::Backspace => t.backspace(),
            EditKey::Delete => t.delete(),
            EditKey::Home => t.home(),
            EditKey::End => t.end(),
            _ => {}
        },
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

/// How many grid rows a box of `height` points takes: one per started grid pitch, but a box that
/// overhangs a row by less than a quarter of it still counts as that row (a one-line formula is
/// a little taller than the 5 mm pitch).
pub fn grid_rows(height: f64, grid: f64) -> f64 {
    (height / grid - 0.25 - 1e-9).ceil().max(1.0)
}

/// The height of a one-line formula, in points (the layout of `x:=5` at the math size): what a plain
/// draft's box is taken to be before it is laid out.
const ONE_LINE: f64 = 12.24;

/// The top of a box whose bottom-left corner is at `bottom` (a snapped grid row): the box sits in
/// the grid rows above that row (see [`grid_rows`]) and is centred in them, so a formula stands in
/// the middle of its grid cell. It never starts above the margin box.
pub fn box_top(bottom: f64, height: f64, grid: f64) -> f64 {
    let slot = grid_rows(height, grid) * grid;
    let height = if height > 0.0 { height } else { ONE_LINE };
    (bottom - slot + (slot - height) / 2.0).max(0.0)
}

//! Headless view model: turns a worksheet and its evaluation into draw items, and counts the
//! golden classification. No egui types here; the canvas draws what this module decides.
//!
//! The evaluation and the classification come from `mathcraft-import` (`evaluate_worksheet` and
//! `compare::classify`), the same code the golden harness runs, so the counts here equal the
//! per-file rows of `mathcraft-cli golden`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use mathcraft_core::eval::{MatrixDisplay, Outcome, ResultFormat, Value};
use mathcraft_core::expr::Expr;
use mathcraft_core::units::UnitSystem;
use mathcraft_import::compare::{classify, Category};
use mathcraft_import::evaluate::{
    is_stopped_reason, EvalOutcome, RegionEvaluation, WorksheetEvaluation,
};
use mathcraft_import::regions::{RegionKind, RegionModel, Worksheet};
use mathcraft_import::results::ResultData;
use mathcraft_import::rich_text::{RichText, TextBody};

use crate::fonts::Face;
use crate::format::{matrix_text, shown_outcome, stored_text, Shown, TextContext};
use crate::glyphs::{printable, MAX_RESULT_CHARS};
use crate::i18n::{tr, trf};
use crate::images::PixelBudget;
use crate::mathlayout::{layout, matrix_result, text_width, MathBox};
use crate::messages::localize_error;
use crate::pages::{break_layout, shift_at, BreakLayout, PageFrame};
use crate::plot::{self, PlotView};
use crate::plot3d::{self, Plot3dView};
use crate::table::TableView;

/// Font size of math regions, in points.
pub const MATH_SIZE: f32 = 12.0;
/// Font size of text regions, in points.
pub const TEXT_SIZE: f32 = 11.0;
/// Width of a text region without a stated width.
const DEFAULT_TEXT_WIDTH: f32 = 360.0;
/// Size of a plot or unsupported placeholder without a stated size.
const DEFAULT_PLACEHOLDER: [f32; 2] = [320.0, 180.0];
/// Page size used when the sheet is smaller (US letter, portrait).
const PAGE: [f32; 2] = [612.0, 792.0];
/// Space between regions that follow each other without a position, and around results.
const GAP: f32 = 8.0;
/// Space between a math expression and its result, in points.
pub const RESULT_GAP: f32 = GAP;
/// Margin added after the last region on the page.
const MARGIN: f32 = 60.0;
/// Largest matrix result (in cells) drawn as a grid. Larger ones stay a shape text.
const MAX_RESULT_GRID_CELLS: usize = 400;

/// Everything the canvas needs for one worksheet.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewModel {
    pub title: String,
    /// The worksheet's file path, for the `{path}` header and footer field. Empty until the window
    /// sets it (a worksheet that is not saved has none).
    pub path: String,
    pub items: Vec<Item>,
    /// Page size in points, width then height.
    pub page: [f32; 2],
    /// Problems that did not stop the load: import notes, settings that were not applied.
    pub notes: Vec<String>,
    pub counts: Counts,
    /// Header and footer text, drawn on every page in page layout and in the PDF.
    pub furniture: Vec<Furniture>,
    /// Sheet heights of the page-break markers, in points, sorted.
    pub page_breaks: Vec<f32>,
    /// How each page break moves the content below it (see [`crate::pages::break_layout`]).
    pub breaks: Vec<BreakLayout>,
    /// The content frame: stored positions are measured from the margin box (see [`PageFrame`]). The
    /// items, page breaks and sheet size are in frame points.
    pub frame: PageFrame,
}

/// One header or footer text region. `rect` is `[x, y, w, h]` in points, measured from the top-left
/// corner of each page. The text still holds its field tokens (see [`crate::furniture`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Furniture {
    /// True for a footer, false for a header.
    pub footer: bool,
    pub rect: [f32; 4],
    pub text: TextBody,
}

/// One region on the page. `rect` is `[x, y, w, h]` in points, origin at the page's top-left.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub region_id: u32,
    pub rect: [f32; 4],
    pub body: Body,
}

/// How far outside its frame, in points, a click still hits an area.
const AREA_EDGE_SLOP: f64 = 4.0;

/// What an item draws.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Text region: wrapped inside the item's width. The text is the display copy (glyphs made
    /// printable), with its runs and their styles.
    Text(TextBody),
    /// Math region. `result` is the value after `=` for evaluations, or an error of the region.
    Math {
        math: MathBox,
        result: Option<ResultLine>,
    },
    /// A 2D plot, evaluated: drawn inside the item's rectangle.
    Plot(PlotView),
    /// A 3D plot, evaluated: drawn inside the item's rectangle, turned by dragging.
    Plot3d(Plot3dView),
    /// A table region: its columns, drawn with their units and values (see [`crate::table`]).
    Table(TableView),
    /// A box with a label: unsupported elements, plots that were not evaluated.
    Placeholder { label: String, detail: String },
    /// An area: a framed group with a header bar. Its regions are separate items. When it is
    /// collapsed, its regions are not items at all (see [`apply_collapse`]).
    Area(AreaView),
    /// An image, drawn inside the item's rectangle.
    Image(ImageView),
}

/// The display state of an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaView {
    pub collapsed: bool,
    pub locked: bool,
    pub hide_border: bool,
}

/// The encoded bytes of an image and a key that identifies them, for the canvas's texture cache.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageView {
    /// Hash of `bytes`, computed once when the view model is built.
    pub key: u64,
    pub bytes: Arc<[u8]>,
    /// The document's pixel budget, shared by all its images (see [`PixelBudget`]).
    pub budget: PixelBudget,
}

/// Height of an area's header bar, in points. A collapsed area is this tall.
pub const AREA_HEADER_PT: f32 = 18.0;

/// The result shown after a math region's `=`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultLine {
    /// The computed value, or the error or reason when the region did not compute.
    pub computed: String,
    /// Prime's stored result, as text, when one is stored.
    pub stored: Option<String>,
    /// A symbolic result, laid out as math (`computed` is then empty).
    pub math: Option<MathBox>,
    pub tone: Tone,
    /// Free width to the right of the expression, in points, up to the next region on the same
    /// rows (infinite when there is none). A text result wider than this goes below the
    /// expression instead.
    pub room: f32,
    /// Tooltip text: the reason for a problem or unsupported region, or the match status.
    pub tip: String,
}

/// How a result is coloured. Follows the golden categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Nothing to compare with (`NoReference`).
    Plain,
    /// Matches the stored result (`Pass`, `WeakPass`).
    Match,
    /// Mismatch, or the engine failed (`Mismatch`, `EngineError`, `EngineErrorNoRef`).
    Problem,
    /// The engine does not handle the region (`Unsupported`).
    Unsupported,
}

/// Counts of the golden classification, as `mathcraft-cli golden` tallies them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub pass: usize,
    /// Passes that accept any engine error for a stored error code other than `bad%_variable`.
    /// Also part of `pass`.
    pub weak_pass: usize,
    pub mismatch: usize,
    /// Count per unsupported reason.
    pub unsupported: BTreeMap<String, usize>,
    pub engine_error: usize,
    pub engine_error_no_ref: usize,
    pub no_reference: usize,
    /// 2D plots that evaluated completely (`PlotOk`).
    pub plot_ok: usize,
}

impl Counts {
    fn add(&mut self, category: &Category) {
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
}

/// Builds the view model of `ws` from its evaluation. Counts come from every evaluated region,
/// exactly as the golden harness counts them.
pub fn build(title: &str, ws: &Worksheet, evaluation: &WorksheetEvaluation) -> ViewModel {
    let categories: Vec<Category> = evaluation.regions.iter().map(classify).collect();
    let mut counts = Counts::default();
    for category in &categories {
        counts.add(category);
    }
    let by_id: HashMap<u32, (&RegionEvaluation, &Category)> = evaluation
        .regions
        .iter()
        .zip(&categories)
        .map(|(eval, category)| (eval.region_id, (eval, category)))
        .collect();
    let positions = absolute_positions(&ws.regions);
    // The worksheet's result format, which a region's own format overrides.
    let global = ws.settings.result_format.clone().unwrap_or_default();
    // The display language's decimal and digit-group separators (see `i18n`).
    let ctx = TextContext {
        seps: crate::i18n::number_separators(),
        units: ws
            .settings
            .unit_system
            .as_deref()
            .and_then(UnitSystem::from_attr)
            .unwrap_or_default(),
        origin: ws.settings.array_origin.unwrap_or(0),
    };
    // The decoded pixels of the document's images are counted against one budget (see `images`).
    let budget = PixelBudget::new();
    let mut items: Vec<Item> = Vec::new();
    let mut page_breaks: Vec<f32> = Vec::new();
    for region in ws.evaluation_order() {
        // A page break has no body. It is drawn as a marker at its height.
        if matches!(region.kind, RegionKind::PageBreak) {
            if let Some(Some((_, top))) = positions.get(&region.id) {
                page_breaks.push(*top as f32);
            }
            continue;
        }
        let pos = match positions.get(&region.id).copied().flatten() {
            Some((left, top)) => [left as f32, top as f32],
            None => items
                .last()
                .map_or([0.0, 0.0], |p| [p.rect[0], p.rect[1] + p.rect[3] + GAP]),
        };
        // A text region is at most as wide as the room left to the right of it on the page, so it
        // does not widen the sheet.
        let room = (PAGE[0] - MARGIN - pos[0]).max(40.0);
        let (body, size) = item_body(
            region,
            by_id.get(&region.id).copied(),
            &global,
            &ctx,
            &budget,
            room,
        );
        items.push(Item {
            region_id: region.id,
            rect: [pos[0], pos[1], size[0], size[1]],
            body,
        });
    }
    // A page break starts the next page: the content below it moves to the top of that page.
    // Only the display changes, as for a collapsed area, so the stored positions stay as they are.
    // Pages are cut at the printable height, so the breaks are laid out on that height.
    let frame = PageFrame::new(&ws.page);
    let breaks = break_layout(&page_breaks, frame.printable_h());
    if !breaks.is_empty() {
        for item in &mut items {
            item.rect[1] += shift_at(&breaks, item.rect[1]);
        }
        page_breaks = breaks.iter().map(|b| b.marker).collect();
    }
    apply_collapse(&mut items, &ws.regions, &mut page_breaks);
    // The laid-out sheet becomes the frame: each page's gap of margins is added between pages.
    for item in &mut items {
        item.rect[1] = frame.to_frame(item.rect[1]);
    }
    for y in &mut page_breaks {
        *y = frame.to_frame(*y);
    }
    page_breaks.sort_by(f32::total_cmp);
    let furniture: Vec<Furniture> = [(false, &ws.header), (true, &ws.footer)]
        .into_iter()
        .flat_map(|(footer, regions)| regions.iter().map(move |region| (footer, region)))
        .filter_map(|(footer, region)| furniture_of(region, footer))
        .collect();
    let rects: Vec<[f32; 4]> = items.iter().map(|item| item.rect).collect();
    for item in &mut items {
        if let Body::Math {
            math,
            result: Some(line),
        } = &mut item.body
        {
            line.room = result_room(item.rect, math.width, &rects);
        }
    }
    let (page_w, page_h) = items.iter().fold((PAGE[0], PAGE[1]), |(w, h), it| {
        (
            w.max(it.rect[0] + it.rect[2] + MARGIN),
            h.max(it.rect[1] + it.rect[3] + MARGIN),
        )
    });
    let mut notes = evaluation.notes.clone();
    notes.extend(ws.report.notes.iter().map(|n| {
        trf(
            "res.region",
            &[&n.region_id.to_string(), &localize_error(&n.reason)],
        )
    }));
    ViewModel {
        title: title.to_string(),
        path: String::new(),
        items,
        page: [page_w, page_h],
        notes,
        counts,
        furniture,
        page_breaks,
        breaks,
        frame,
    }
}

/// Hides what each collapsed area holds, and moves the items below the area up by the height it
/// no longer shows, so the page closes the gap. Only the display changes: the stored positions
/// stay as they are. An item below a collapsed area is one whose top is at or below the area's
/// bottom edge.
fn apply_collapse(items: &mut Vec<Item>, regions: &[RegionModel], page_breaks: &mut [f32]) {
    let mut collapsed: HashSet<u32> = HashSet::new();
    let mut hidden: HashSet<u32> = HashSet::new();
    collect_collapsed(regions, false, &mut collapsed, &mut hidden);
    if collapsed.is_empty() {
        return;
    }
    // (bottom edge before collapsing, height removed) of each collapsed area that is drawn.
    let mut gaps: Vec<(f32, f32)> = Vec::new();
    for item in items.iter_mut() {
        if !collapsed.contains(&item.region_id) {
            continue;
        }
        let [_, top, _, height] = item.rect;
        if height > AREA_HEADER_PT {
            gaps.push((top + height, height - AREA_HEADER_PT));
        }
        item.rect[3] = height.min(AREA_HEADER_PT);
    }
    items.retain(|item| !hidden.contains(&item.region_id));
    for item in items.iter_mut() {
        let top = item.rect[1];
        let shift: f32 = gaps
            .iter()
            .filter(|(bottom, _)| top >= *bottom)
            .map(|(_, removed)| removed)
            .sum();
        item.rect[1] -= shift;
    }
    // The page-break markers below a collapsed area move up with the content.
    for y in page_breaks.iter_mut() {
        let at = *y;
        let shift: f32 = gaps
            .iter()
            .filter(|(bottom, _)| at >= *bottom)
            .map(|(_, removed)| removed)
            .sum();
        *y = at - shift;
    }
}

/// Finds the collapsed areas, and every region inside any collapsed area (`inside` is true for
/// the regions below one).
fn collect_collapsed(
    list: &[RegionModel],
    inside: bool,
    collapsed: &mut HashSet<u32>,
    hidden: &mut HashSet<u32>,
) {
    for region in list {
        let mut below = inside;
        if let RegionKind::Area(area) = &region.kind {
            if area.collapsed {
                below = true;
                collapsed.insert(region.id);
            }
        }
        if inside {
            hidden.insert(region.id);
        }
        collect_collapsed(&region.children, below, collapsed, hidden);
    }
}

/// The header or footer text of one region, placed where the region says. A region with no
/// position, or with content other than text, is not shown.
fn furniture_of(region: &RegionModel, footer: bool) -> Option<Furniture> {
    let RegionKind::Text(text) = &region.kind else {
        return None;
    };
    let (left, top) = region.pos?;
    let width = region
        .size
        .map_or(DEFAULT_TEXT_WIDTH, |(w, _)| w as f32)
        .max(40.0);
    let shown = TextBody::from_rich(printable_rich(text.rich()));
    let height = rich_height(shown.rich(), width).max(region.size.map_or(0.0, |(_, h)| h as f32));
    Some(Furniture {
        footer,
        rect: [left as f32, top as f32, width, height],
        text: shown,
    })
}

/// Where the stored positions of a list of regions are measured from.
#[derive(Clone, Copy)]
enum Frame {
    /// The page: positions are absolute.
    Page,
    /// The members of a solve block: positions are relative to the block. `None` when the
    /// block has no absolute position, so the members have none either.
    Block(Option<(f64, f64)>),
}

/// Absolute `(left, top)` of every region, `None` when it has no position it can be drawn at.
/// Such a region is placed after the previous item instead (see [`build`]). A solve-block
/// member whose block has no position is not placed at its relative coordinates, which would
/// put it at the page origin.
pub(crate) fn absolute_positions(list: &[RegionModel]) -> HashMap<u32, Option<(f64, f64)>> {
    let mut out = HashMap::new();
    collect_positions(list, Frame::Page, &mut out);
    out
}

fn collect_positions(
    list: &[RegionModel],
    frame: Frame,
    out: &mut HashMap<u32, Option<(f64, f64)>>,
) {
    for region in list {
        let absolute = match (frame, region.pos) {
            (Frame::Page, own) => own,
            (Frame::Block(Some((ox, oy))), Some((x, y))) => Some((ox + x, oy + y)),
            (Frame::Block(_), _) => None,
        };
        out.insert(region.id, absolute);
        let child_frame = match region.kind {
            RegionKind::SolveBlock(_) => Frame::Block(absolute),
            _ => Frame::Page,
        };
        collect_positions(&region.children, child_frame, out);
    }
}

/// The body and size of one region. `evaluated` is its evaluation and category, when it was run.
/// `room` is the width a text region may take (the page's right margin is kept).
fn item_body(
    region: &RegionModel,
    evaluated: Option<(&RegionEvaluation, &Category)>,
    global: &ResultFormat,
    ctx: &TextContext,
    budget: &PixelBudget,
    room: f32,
) -> (Body, [f32; 2]) {
    match &region.kind {
        RegionKind::Text(text) => {
            let width = region
                .size
                .map_or(DEFAULT_TEXT_WIDTH, |(w, _)| w as f32)
                .max(40.0)
                .min(room);
            let shown = TextBody::from_rich(printable_rich(text.rich()));
            let height =
                rich_height(shown.rich(), width).max(region.size.map_or(0.0, |(_, h)| h as f32));
            (Body::Text(shown), [width, height])
        }
        RegionKind::Math(expr) => {
            let math = layout(expr, MATH_SIZE);
            let format = region.result_format.as_ref().unwrap_or(global);
            let result = evaluated.and_then(|(eval, category)| {
                result_line(expr, category, eval, format, &region.matrix, ctx)
            });
            let result_w = result.as_ref().map_or(0.0, |r| match &r.math {
                Some(shown) => shown.width + 3.0 * GAP,
                None => {
                    let alt = r.stored.as_deref().unwrap_or("");
                    text_width(&r.computed, MATH_SIZE).max(text_width(alt, MATH_SIZE)) + 3.0 * GAP
                }
            });
            let result_h = result
                .as_ref()
                .and_then(|r| r.math.as_ref())
                .map_or(0.0, MathBox::height);
            let size = [math.width + result_w, math.height().max(result_h)];
            (Body::Math { math, result }, size)
        }
        RegionKind::Plot(info) => match evaluated.map(|(eval, _)| &eval.outcome) {
            Some(EvalOutcome::Plot(data)) => (Body::Plot(plot::view_of(data)), region_size(region)),
            Some(EvalOutcome::Plot3d(data)) => {
                (Body::Plot3d(plot3d::view_of(data)), region_size(region))
            }
            Some(EvalOutcome::Skipped(reason)) => (
                Body::Placeholder {
                    label: tr("res.plot_not_drawn").to_string(),
                    detail: printable(reason, MAX_RESULT_CHARS, Face::Roman),
                },
                region_size(region),
            ),
            _ => (
                Body::Placeholder {
                    label: tr("res.plot_not_evaluated").to_string(),
                    detail: info.kind.clone(),
                },
                region_size(region),
            ),
        },
        RegionKind::Table(model) => (Body::Table(TableView::of(model)), region_size(region)),
        RegionKind::Unsupported { reason } => (
            Body::Placeholder {
                label: tr("res.unsupported").to_string(),
                detail: printable(reason, MAX_RESULT_CHARS, Face::Roman),
            },
            region_size(region),
        ),
        RegionKind::SolveBlock(_) => (
            Body::Placeholder {
                label: tr("res.solve_block").to_string(),
                detail: String::new(),
            },
            DEFAULT_PLACEHOLDER,
        ),
        RegionKind::Area(area) => (
            Body::Area(AreaView {
                collapsed: area.collapsed,
                locked: area.locked,
                hide_border: area.hide_border,
            }),
            region_size(region),
        ),
        RegionKind::Image(image) => {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            image.bytes.hash(&mut hasher);
            (
                Body::Image(ImageView {
                    key: hasher.finish(),
                    bytes: image.bytes.clone(),
                    budget: budget.clone(),
                }),
                region_size(region),
            )
        }
        // Handled by `build`, which draws it as a marker. Kept for the match to be complete.
        RegionKind::PageBreak => (
            Body::Placeholder {
                label: tr("res.unsupported").to_string(),
                detail: String::new(),
            },
            DEFAULT_PLACEHOLDER,
        ),
    }
}

fn region_size(region: &RegionModel) -> [f32; 2] {
    match region.size {
        Some((w, h)) if w > 0.0 && h > 0.0 => [w as f32, h as f32],
        _ => DEFAULT_PLACEHOLDER,
    }
}

/// The rich text with every run's glyphs made printable. The styles are kept.
fn printable_rich(rich: &RichText) -> RichText {
    let mut shown = rich.clone();
    for line in &mut shown.lines {
        for run in &mut line.runs {
            let face = Face::of(run.style.bold, run.style.italic);
            run.text = printable(&run.text, usize::MAX, face);
        }
    }
    shown
}

/// Height of a rich text wrapped to `width`, estimated with the same advance table as the
/// layout. Each line is as tall as its largest run, so a larger font takes more room.
fn rich_height(rich: &RichText, width: f32) -> f32 {
    let mut total = 6.0;
    for line in &rich.lines {
        let size = line
            .runs
            .iter()
            .map(|run| run.style.size)
            .fold(TEXT_SIZE, f32::max);
        let text: String = line.runs.iter().map(|run| run.text.as_str()).collect();
        let wrapped = (text_width(&text, size) / width).ceil().max(1.0);
        total += wrapped * size * 1.35;
    }
    total
}

/// Whether the region shows a result after `=`: an evaluation, or a definition whose right side
/// is an evaluation.
fn shows_result(expr: &Expr) -> bool {
    match expr {
        Expr::Eval { .. } | Expr::SymEval { .. } => true,
        Expr::Define { expr, .. } => {
            matches!(expr.as_ref(), Expr::Eval { .. } | Expr::SymEval { .. })
        }
        _ => false,
    }
}

/// The result line of a math region. `None` when there is nothing to show.
fn result_line(
    expr: &Expr,
    category: &Category,
    eval: &RegionEvaluation,
    format: &ResultFormat,
    matrix: &MatrixDisplay,
    ctx: &TextContext,
) -> Option<ResultLine> {
    let outcome: Option<&Outcome> = match &eval.outcome {
        EvalOutcome::Engine(outcome) => Some(outcome.as_ref()),
        EvalOutcome::Panic(_)
        | EvalOutcome::Skipped(_)
        | EvalOutcome::Plot(_)
        | EvalOutcome::Plot3d(_) => None,
    };
    let stored_text_value = eval
        .stored
        .as_ref()
        .map(|data| stored_text_of(data, format, ctx));
    let computed_shown = match outcome.map(|o| shown_outcome(o, format, ctx)) {
        Some(Shown::Value(v)) => v,
        Some(Shown::Error(e)) => e,
        Some(Shown::Nothing) | None => String::new(),
    };
    let (computed, tone, tip) = match category {
        Category::Pass => (
            computed_shown,
            Tone::Match,
            tr("res.matches_prime").to_string(),
        ),
        Category::WeakPass(code) => (
            computed_shown,
            Tone::Match,
            trf("res.engine_error_accepted", &[&code.to_string()]),
        ),
        Category::Mismatch(reason) => (computed_shown, Tone::Problem, reason.clone()),
        Category::EngineError(e) => (e.clone(), Tone::Problem, e.clone()),
        // Prime stored no result for this region, so it shows none. A crash inside the engine (a
        // panic) is an engine gap and grey. Any other error is the region's own error, such as a
        // dimension mismatch, and is red. A definition without `=` shows nothing (see the rule below).
        Category::EngineErrorNoRef(e) => {
            let tone = if e.starts_with("Panic: ") {
                Tone::Unsupported
            } else {
                Tone::Problem
            };
            (e.clone(), tone, e.clone())
        }
        Category::Unsupported(reason) => {
            let label = if is_stopped_reason(reason) {
                tr("res.evaluation_stopped")
            } else {
                tr("res.not_computed")
            };
            (label.to_string(), Tone::Unsupported, reason.clone())
        }
        Category::NoReference => (computed_shown, Tone::Plain, tr("res.no_stored").to_string()),
        Category::PlotOk(summary) => (
            summary.clone(),
            Tone::Plain,
            tr("res.plot_no_numbers").to_string(),
        ),
    };
    let math = match outcome {
        Some(Outcome::Value(Value::Symbolic(s))) if tone != Tone::Problem => {
            Some(layout(&s.shown, MATH_SIZE))
        }
        Some(Outcome::Value(Value::Matrix(m)))
            if tone != Tone::Problem && m.shape().0 * m.shape().1 <= MAX_RESULT_GRID_CELLS =>
        {
            let grid = matrix_text(m, matrix, format, ctx);
            Some(matrix_result(
                grid.rows,
                grid.cols,
                &grid.cells,
                &grid.unit,
                MATH_SIZE,
            ))
        }
        _ => None,
    };
    // A symbolic result shows only after the region's own `=` or `->`. A bare expression (a
    // limit, a sum) that the engine happens to evaluate shows nothing, as Prime does.
    let evaluation = shows_result(expr);
    // A non-evaluation shows only a problem, so failures are not hidden.
    if !evaluation && tone != Tone::Problem {
        return None;
    }
    if computed.is_empty() && math.is_none() && stored_text_value.is_none() {
        return None;
    }
    let mut tip = printable(&tip, MAX_RESULT_CHARS, Face::Roman);
    if let Some(stored) = &stored_text_value {
        tip.push_str(&format!(
            "\n{}",
            trf("res.prime_stored", &[&stored.to_string()])
        ));
    }
    Some(ResultLine {
        computed: printable(&computed, MAX_RESULT_CHARS, Face::Roman),
        stored: stored_text_value,
        math,
        tone,
        room: f32::INFINITY,
        tip,
    })
}

fn stored_text_of(data: &ResultData, format: &ResultFormat, ctx: &TextContext) -> String {
    printable(
        &stored_text(&data.value, format, ctx),
        MAX_RESULT_CHARS,
        Face::Roman,
    )
}

/// Free width after a math item's expression, in points: up to the nearest region that starts to
/// the right of the result on the same rows. Infinite when nothing is there. `me` is the item's
/// `[x, y, w, h]`, and `others` the rectangles of all items (the item itself is never to its right).
pub fn result_room(me: [f32; 4], math_width: f32, others: &[[f32; 4]]) -> f32 {
    let x0 = me[0] + math_width + RESULT_GAP;
    let (top, bottom) = (me[1], me[1] + me[3]);
    others
        .iter()
        .filter(|r| r[0] >= x0 && r[1] < bottom && r[1] + r[3] > top)
        .map(|r| r[0] - x0)
        .fold(f32::INFINITY, f32::min)
}

impl ViewModel {
    /// The stored sheet y that a frame y stands for: the inverse of the page-break layout and of
    /// the page gaps (see [`PageFrame`]). A point on a page-break marker gives the stored y of that
    /// break. Without breaks it is the laid-out y.
    pub fn to_stored(&self, frame_y: f32) -> f32 {
        let y = self.frame.to_laid(frame_y);
        let mut shift = 0.0_f32;
        for b in &self.breaks {
            if b.at + b.shift <= y {
                shift = b.shift;
            } else {
                break;
            }
        }
        y - shift
    }

    /// The frame y of a stored sheet y (see [`ViewModel::to_stored`]).
    pub fn to_display(&self, y: f32) -> f32 {
        self.frame.to_frame(y + shift_at(&self.breaks, y))
    }

    /// The region under the page point `(x, y)`, if any. When items overlap, the one drawn last
    /// (on top) wins.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<u32> {
        self.items
            .iter()
            .rev()
            .find(|item| {
                let [left, top, w, h] = item.rect.map(f64::from);
                // The frame of an area is a thin line: a click near it still takes the area.
                let slop = if matches!(item.body, Body::Area(_)) {
                    AREA_EDGE_SLOP
                } else {
                    0.0
                };
                x >= left - slop && x <= left + w + slop && y >= top - slop && y <= top + h + slop
            })
            .map(|item| item.region_id)
    }
}

#[cfg(test)]
#[path = "viewmodel_tests.rs"]
mod tests;

//! Page export: turns the view model into PDF pages with the same layout as the window.
//!
//! Math and result boxes come from the same [`MathBox`] primitives the window draws. Text regions
//! are wrapped with the bundled font advances. 2D plots are polylines with the window's axes and
//! ticks. 3D plots and tables are not exported: they are counted and listed in
//! [`Export::skipped`]. Gridlines and window chrome are not printed.

use crate::i18n::tr;
use crate::temp_file::{create_new_in, unique_suffix};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use eframe::egui::Color32;
use mathcraft_import::page::PageSettings;
use mathcraft_import::rich_text::{Align, RichText};

use crate::fonts::{self, Face};
use crate::furniture::{self, Fields};
use crate::mathlayout::{MathBox, Prim};
use crate::pages::Geometry;
use crate::pdf::{self, Glyph, Op, Page, Rgb, Run};
use crate::plot::{self, PlotView};
use crate::theme;
use crate::viewmodel::{Body, Item, ResultLine, Tone, ViewModel, MATH_SIZE};

/// Gap between a math region and its result, in points (as in the window).
const RESULT_GAP: f32 = 8.0;
/// Line height of text regions, as a multiple of the largest size on the line.
const LINE_HEIGHT: f32 = 1.25;
/// Baseline position below the top of a line, as a fraction of its size.
const ASCENT_FRACTION: f32 = 0.8;

/// The result of an export.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    /// The PDF file.
    pub pdf: Vec<u8>,
    /// Number of pages written.
    pub pages: usize,
    /// Items that are not exported, with how many of each kind, for the user to read.
    pub skipped: Vec<String>,
    /// True when the content ran past the page limit.
    pub capped: bool,
}

/// Writes the worksheet as a PDF. `show_prime` exports Prime's stored results instead of the
/// computed ones, as the window does.
///
/// # Errors
/// The writer's error text (see [`pdf::PdfError`]).
pub fn export(
    vm: &ViewModel,
    page: &PageSettings,
    show_prime: bool,
    title: &str,
) -> Result<Export, String> {
    let (geometry, pages, skipped) = paginate(vm, page, show_prime);
    let bytes = pdf::write(&pages, title, fonts::file_data).map_err(|e| e.to_string())?;
    let skipped = skipped
        .into_iter()
        .map(|(kind, n)| format!("{n} {kind}(s) not exported"))
        .collect();
    Ok(Export {
        pdf: bytes,
        pages: geometry.count,
        skipped,
        capped: geometry.capped,
    })
}

/// The pages of the export, with their operations in page-local points, and the kinds of items
/// that are not exported with how many of each.
pub(crate) fn paginate(
    vm: &ViewModel,
    page: &PageSettings,
    show_prime: bool,
) -> (Geometry, Vec<Page>, BTreeMap<&'static str, usize>) {
    let content_bottom = vm
        .items
        .iter()
        .map(|item| item.rect[1] + item.rect[3])
        .fold(0.0_f32, f32::max);
    let geometry = Geometry::new(page, content_bottom);
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut sheet: Vec<(f32, f32, Vec<Op>)> = Vec::new();
    // The export decodes the images again, with a budget of its own.
    let budget = crate::images::PixelBudget::new();
    for item in &vm.items {
        match item_ops(item, show_prime, &budget) {
            Ok(ops) => sheet.push((item.rect[1], item.rect[1] + item.rect[3], ops)),
            Err(kind) => *skipped.entry(kind).or_default() += 1,
        }
    }
    let [w, h] = geometry.paper;
    // Items are in frame points, whose origin is the margin box corner. On a page, the paper
    // origin is the frame origin less the margins, so an item moves by the left margin across and
    // by the top margin less the page's top in the frame down.
    let [left, top_margin] = geometry.frame().origin();
    let today = furniture::today();
    let mut pages = Vec::with_capacity(geometry.count);
    for index in 0..geometry.count {
        let top = index as f32 * geometry.frame().paper_h;
        let bottom = top + h;
        let mut ops = Vec::new();
        for (item_top, item_bottom, item_ops) in &sheet {
            if *item_top < bottom && *item_bottom > top {
                ops.extend(item_ops.iter().map(|op| shift(op, left, top_margin - top)));
            }
        }
        // Header and footer text, at their places on every page, in page-local points.
        let fields = Fields {
            page: index + 1,
            pages: geometry.count,
            filename: &vm.title,
            path: if vm.path.is_empty() {
                &vm.title
            } else {
                &vm.path
            },
            date: &today,
        };
        for f in &vm.furniture {
            let shown = furniture::expand(&f.text, &fields);
            ops.extend(text_ops(shown.rich(), f.rect[0], f.rect[1], f.rect[2]));
        }
        for &y in &vm.page_breaks {
            if y >= top && y < bottom {
                ops.extend(dashed_ops(y - top + top_margin, w));
            }
        }
        pages.push(Page {
            width: w,
            height: h,
            ops,
        });
    }
    (geometry, pages, skipped)
}

/// Longest file stem kept by [`safe_stem`], in characters.
const MAX_STEM: usize = 64;

/// Files written for printing. They are removed by [`remove_print_files`] when the app exits.
static PRINT_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A file name stem made safe for a temporary file name: only ASCII letters, digits, `-` and `_`
/// are kept (at most [`MAX_STEM`] characters). An empty result becomes `worksheet`.
pub fn safe_stem(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(MAX_STEM)
        .collect();
    if kept.is_empty() {
        "worksheet".to_string()
    } else {
        kept
    }
}

/// Writes `pdf` to a new file in the temporary folder, named `mathcraft-{stem}-{unique}.pdf`, and
/// records it for removal at exit. `stem` must already be [`safe_stem`]-cleaned.
///
/// # Errors
/// The text of the I/O error.
pub fn write_print_file(stem: &str, pdf: &[u8]) -> Result<PathBuf, String> {
    let (path, mut file) = create_new_in(&std::env::temp_dir(), |_| {
        format!("mathcraft-{stem}-{}.pdf", unique_suffix())
    })
    .map_err(|e| e.to_string())?;
    let written = file.write_all(pdf).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&path);
        return Err(e.to_string());
    }
    if let Ok(mut files) = PRINT_FILES.lock() {
        files.push(path.clone());
    }
    Ok(path)
}

/// Removes the files written by [`write_print_file`]. Best effort: a file that a viewer still holds
/// may stay in the temporary folder.
pub fn remove_print_files() {
    let files = match PRINT_FILES.lock() {
        Ok(mut files) => std::mem::take(&mut *files),
        Err(_) => return,
    };
    for path in files {
        let _ = std::fs::remove_file(path);
    }
}

/// Opens a file with the system's default viewer. Printing goes through that viewer, so no
/// printer API is needed. The path is always one argument, with no shell in between.
///
/// # Errors
/// The text of the spawn error, or that no launcher exists on this platform.
pub fn open_with_system_viewer(path: &std::path::Path) -> Result<(), String> {
    use std::process::Command;
    let spawned = if cfg!(target_os = "macos") {
        Command::new("open").arg(path).spawn()
    } else if cfg!(target_os = "windows") {
        // `cmd /C start` would read `&`, `^` and `%` in the path as operators.
        Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler"])
            .arg(path)
            .spawn()
    } else {
        Command::new("xdg-open").arg(path).spawn()
    };
    spawned.map(|_| ()).map_err(|e| e.to_string())
}

/// The operations of one item in sheet points, or the kind of item when it is not exported.
fn item_ops(
    item: &Item,
    show_prime: bool,
    budget: &crate::images::PixelBudget,
) -> Result<Vec<Op>, &'static str> {
    let [x, y, w, h] = item.rect;
    match &item.body {
        Body::Text(text) => Ok(text_ops(text.rich(), x, y, w)),
        Body::Math { math, result } => {
            let mut ops = prim_ops(math, x, y);
            if let Some(line) = result {
                ops.extend(result_ops(line, math, x, y, show_prime));
            }
            Ok(ops)
        }
        Body::Plot(view) => Ok(plot_ops(view, x, y, w, h)),
        Body::Plot3d(_) => Err("3D plot"),
        Body::Table(_) => Err("table"),
        Body::Placeholder { label, detail } => Ok(placeholder_ops(label, detail, x, y, w, h)),
        Body::Area(view) => Ok(area_ops(view, x, y, w, h)),
        Body::Image(view) => image_ops(view, budget, x, y, w, h),
    }
}

/// An area: its header bar, its frame unless the border is hidden, and its collapse icon (the
/// icon is not drawn for a locked area, which shows a solid square as in the window).
fn area_ops(view: &crate::viewmodel::AreaView, x: f32, y: f32, w: f32, h: f32) -> Vec<Op> {
    let edge = rgb(theme::AREA_EDGE);
    let mut ops = vec![Op::Rect {
        x,
        y,
        w,
        h: crate::viewmodel::AREA_HEADER_PT.min(h),
        colour: rgb(theme::AREA_HEADER),
    }];
    if !view.hide_border {
        ops.push(frame_ops(x, y, w, h, edge));
    }
    let side = 10.0_f32;
    let (ix, iy) = (x + 4.0, y + 4.0);
    if view.locked {
        ops.push(Op::Rect {
            x: ix,
            y: iy,
            w: side,
            h: side,
            colour: edge,
        });
        return ops;
    }
    ops.push(frame_ops(ix, iy, side, side, edge));
    let (mx, my) = (ix + side / 2.0, iy + side / 2.0);
    ops.push(line(vec![[mx - 3.0, my], [mx + 3.0, my]], 1.5, edge));
    if view.collapsed {
        ops.push(line(vec![[mx, my - 3.0], [mx, my + 3.0]], 1.5, edge));
    }
    ops
}

/// A dashed horizontal rule across a page at `y` points from its top: a page-break marker, drawn
/// as the window draws it.
fn dashed_ops(y: f32, width: f32) -> Vec<Op> {
    const DASH: f32 = 6.0;
    const GAP: f32 = 4.0;
    let mut ops = Vec::new();
    let mut x = 0.0;
    while x < width {
        let end = (x + DASH).min(width);
        ops.push(Op::Line {
            points: vec![[x, y], [end, y]],
            width: 1.0,
            colour: rgb(theme::UNSUPPORTED),
        });
        x += DASH + GAP;
    }
    ops
}

/// Largest image, in decoded bytes (width x height x 3), written into a PDF.
const MAX_IMAGE_RGB_BYTES: u64 = 16 * 1024 * 1024;

/// An image: its decoded pixels, flattened onto white, drawn into its rectangle. An image that
/// does not decode, or is too large for the PDF, is not exported and the item's kind is named.
fn image_ops(
    view: &crate::viewmodel::ImageView,
    budget: &crate::images::PixelBudget,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> Result<Vec<Op>, &'static str> {
    // The size is read from the header first, so an image too large for the PDF is never decoded.
    let (width, height) = crate::images::dimensions(&view.bytes).ok_or("image")?;
    if u64::from(width) * u64::from(height) * 3 > MAX_IMAGE_RGB_BYTES {
        return Err("image");
    }
    // The PDF takes the image at its full size, so no texture limit applies.
    let pixels = crate::images::decode(&view.bytes, u32::MAX, budget).ok_or("image")?;
    let [width, height] = pixels.size;
    let mut rgb_bytes = Vec::with_capacity(width * height * 3);
    for pixel in &pixels.pixels {
        // The colours are premultiplied by alpha. Over white, a colour c with alpha a comes out
        // as c + (255 - a).
        let [r, g, b, a] = pixel.to_array();
        let over_white = 255 - a;
        rgb_bytes.push(r.saturating_add(over_white));
        rgb_bytes.push(g.saturating_add(over_white));
        rgb_bytes.push(b.saturating_add(over_white));
    }
    Ok(vec![Op::Image {
        x,
        y,
        w,
        h,
        width: width as u32,
        height: height as u32,
        rgb: rgb_bytes,
    }])
}

/// Moves an operation by `dx` across and `dy` down, in points.
fn shift(op: &Op, dx: f32, dy: f32) -> Op {
    match op {
        Op::Text {
            x,
            baseline,
            size,
            colour,
            runs,
        } => Op::Text {
            x: x + dx,
            baseline: baseline + dy,
            size: *size,
            colour: *colour,
            runs: runs.clone(),
        },
        Op::Rect { x, y, w, h, colour } => Op::Rect {
            x: x + dx,
            y: y + dy,
            w: *w,
            h: *h,
            colour: *colour,
        },
        Op::Line {
            points,
            width,
            colour,
        } => Op::Line {
            points: points.iter().map(|[px, py]| [px + dx, py + dy]).collect(),
            width: *width,
            colour: *colour,
        },
        Op::Image {
            x,
            y,
            w,
            h,
            width,
            height,
            rgb,
        } => Op::Image {
            x: x + dx,
            y: y + dy,
            w: *w,
            h: *h,
            width: *width,
            height: *height,
            rgb: rgb.clone(),
        },
    }
}

/// Converts a math box drawn with its origin at `(x, y)` (its top-left), as the window does: its
/// baseline is `ascent` below the origin.
fn prim_ops(math: &MathBox, x: f32, y: f32) -> Vec<Op> {
    let base_y = y + math.ascent;
    math.prims
        .iter()
        .filter_map(|prim| match prim {
            Prim::Text {
                x: px,
                y: py,
                text,
                size,
                face,
            } => {
                let runs = runs_for(text, *face, false);
                (!runs.is_empty()).then(|| Op::Text {
                    x: x + px,
                    baseline: base_y + py,
                    size: *size,
                    colour: rgb(theme::INK),
                    runs,
                })
            }
            Prim::Rect { x: px, y: py, w, h } => Some(Op::Rect {
                x: x + px,
                y: base_y + py,
                w: *w,
                h: *h,
                colour: rgb(theme::INK),
            }),
            Prim::Line { points, width } => Some(Op::Line {
                points: points
                    .iter()
                    .map(|[px, py]| [x + px, base_y + py])
                    .collect(),
                width: *width,
                colour: rgb(theme::INK),
            }),
        })
        .collect()
}

/// The result after a math region's `=`, placed to the right of the math as the window places it.
fn result_ops(line: &ResultLine, region: &MathBox, x: f32, y: f32, show_prime: bool) -> Vec<Op> {
    let left = x + region.width + RESULT_GAP;
    if let (Some(math), false) = (&line.math, show_prime) {
        return prim_ops(math, left, y + region.ascent - math.ascent);
    }
    let (text, colour) = if show_prime {
        match &line.stored {
            Some(stored) => (stored.clone(), theme::STORED),
            None => (tr("res.no_stored_short").to_owned(), theme::STORED),
        }
    } else {
        let colour = match line.tone {
            Tone::Plain => theme::INK,
            Tone::Match => theme::MATCH,
            Tone::Problem => theme::PROBLEM,
            Tone::Unsupported => theme::UNSUPPORTED,
        };
        (line.computed.clone(), colour)
    };
    let runs = runs_for(&text, Face::Roman, false);
    if runs.is_empty() {
        return Vec::new();
    }
    vec![Op::Text {
        x: left,
        baseline: y + region.ascent,
        size: MATH_SIZE,
        colour: rgb(colour),
        runs,
    }]
}

/// Text regions: each line is wrapped to the item's width, word by word, with each run's style.
fn text_ops(rich: &RichText, x: f32, y: f32, width: f32) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut top = y;
    for line in &rich.lines {
        // Tokens: words and the spaces between them, each with its style.
        let mut tokens: Vec<Token> = Vec::new();
        for run in &line.runs {
            let face = Face::of(run.style.bold, run.style.italic);
            let size = run.style.size.max(1.0);
            let colour = Rgb(
                run.style.colour[0],
                run.style.colour[1],
                run.style.colour[2],
            );
            let mut word = String::new();
            for ch in run.text.chars() {
                if ch.is_whitespace() {
                    if !word.is_empty() {
                        tokens.push(Token::new(std::mem::take(&mut word), face, size, colour));
                    }
                    tokens.push(Token::new(" ".to_owned(), face, size, colour));
                } else {
                    word.push(ch);
                }
            }
            if !word.is_empty() {
                tokens.push(Token::new(word, face, size, colour));
            }
        }
        let line_size = line
            .runs
            .iter()
            .map(|r| r.style.size)
            .fold(rich.base.size, f32::max)
            .max(1.0);
        for wrapped in wrap(&tokens, width) {
            let total: f32 = wrapped.iter().map(|t| t.width).sum();
            let mut cursor = match line.align {
                Align::Center => x + (width - total) / 2.0,
                Align::Right => x + width - total,
                _ => x,
            };
            let baseline = top + line_size * ASCENT_FRACTION;
            for token in &wrapped {
                if !token.runs.is_empty() {
                    ops.push(Op::Text {
                        x: cursor,
                        baseline,
                        size: token.size,
                        colour: token.colour,
                        runs: token.runs.clone(),
                    });
                }
                cursor += token.width;
            }
            top += line_size * LINE_HEIGHT;
        }
        if line.runs.is_empty() {
            top += rich.base.size.max(1.0) * LINE_HEIGHT;
        }
    }
    ops
}

/// A word or a space, measured.
struct Token {
    runs: Vec<Run>,
    width: f32,
    size: f32,
    colour: Rgb,
}

impl Token {
    /// A token of a text region (Noto Sans), measured and drawn in its face.
    fn new(text: String, face: Face, size: f32, colour: Rgb) -> Self {
        let width = text
            .chars()
            .map(|c| fonts::text_advance_em(face, c).unwrap_or(0.0) * size)
            .sum();
        Self {
            runs: runs_for(&text, face, true),
            width,
            size,
            colour,
        }
    }
}

/// Greedy word wrap: lines of tokens whose width fits `width`. A space at a line end is dropped.
fn wrap(tokens: &[Token], width: f32) -> Vec<Vec<&Token>> {
    let mut lines: Vec<Vec<&Token>> = vec![Vec::new()];
    let mut used = 0.0_f32;
    for token in tokens {
        let is_space = token
            .runs
            .iter()
            .all(|r| r.glyphs.iter().all(|g| g.ch == ' '));
        if !is_space && used + token.width > width && used > 0.0 {
            while lines
                .last()
                .and_then(|l| l.last())
                .is_some_and(|t| is_blank(t))
            {
                lines.last_mut().map(Vec::pop);
            }
            lines.push(Vec::new());
            used = 0.0;
            if is_space {
                continue;
            }
        }
        if let Some(line) = lines.last_mut() {
            line.push(token);
        }
        used += token.width;
    }
    lines
}

fn is_blank(token: &Token) -> bool {
    token
        .runs
        .iter()
        .all(|r| r.glyphs.iter().all(|g| g.ch == ' '))
}

/// A 2D plot: frame, grid, ticks, traces, drawn inside `(x, y, w, h)` as the window draws it.
fn plot_ops(view: &PlotView, x: f32, y: f32, w: f32, h: f32) -> Vec<Op> {
    let grid = Rgb(222, 225, 230);
    let edge = rgb(theme::PLACEHOLDER_EDGE);
    let ink = rgb(theme::INK);
    let mut ops = vec![frame_ops(x, y, w, h, edge)];
    let (left, top, right, bottom) = (x + 30.0, y + 8.0, x + w - 8.0, y + h - 18.0);
    if right - left < 8.0 || bottom - top < 8.0 {
        return ops;
    }
    let sx = plot::scale_of(&view.x);
    let sy = plot::scale_of(&view.y);
    let frame = [
        f64::from(left),
        f64::from(top),
        f64::from(right - left),
        f64::from(bottom - top),
    ];
    let font = 9.0;
    for &tick in &view.x.ticks {
        let Some(f) = sx.fraction(tick) else { continue };
        let px = left + f as f32 * (right - left);
        if view.grid {
            ops.push(line(vec![[px, top], [px, bottom]], 0.5, grid));
        }
        let label = plot::format_tick(tick, view.x.decimals);
        let width = text_width(&label, Face::Roman, font);
        ops.extend(text_at(
            &label,
            px - width / 2.0,
            bottom + 2.0 + font * ASCENT_FRACTION,
            font,
            ink,
        ));
    }
    for &tick in &view.y.ticks {
        let Some(f) = sy.fraction(tick) else { continue };
        let py = bottom - f as f32 * (bottom - top);
        if view.grid {
            ops.push(line(vec![[left, py], [right, py]], 0.5, grid));
        }
        let label = plot::format_tick(tick, view.y.decimals);
        let width = text_width(&label, Face::Roman, font);
        ops.extend(text_at(
            &label,
            left - 3.0 - width,
            py + font * ASCENT_FRACTION / 2.0,
            font,
            ink,
        ));
    }
    for trace in &view.traces {
        let colour = Rgb(trace.color[0], trace.color[1], trace.color[2]);
        for segment in &trace.segments {
            let points: Vec<[f32; 2]> = segment
                .iter()
                .filter_map(|p| plot::screen_point(sx, sy, frame, *p))
                .filter(|[px, py]| px.abs() < 1.0e6 && py.abs() < 1.0e6)
                .map(|[px, py]| [px as f32, py as f32])
                .collect();
            if trace.points_only || points.len() == 1 {
                for [px, py] in points {
                    let r = trace.width.max(2.0);
                    ops.push(Op::Rect {
                        x: px - r / 2.0,
                        y: py - r / 2.0,
                        w: r,
                        h: r,
                        colour,
                    });
                }
            } else if points.len() >= 2 {
                ops.push(line(points, trace.width, colour));
            }
        }
    }
    ops.push(frame_ops(left, top, right - left, bottom - top, ink));
    if let Some(note) = &view.note {
        ops.extend(text_at(
            note,
            x + 4.0,
            y + 2.0 + font * ASCENT_FRACTION,
            font,
            rgb(theme::UNSUPPORTED),
        ));
    }
    ops
}

/// A box with a label and detail text, centred, as the window draws an unsupported element.
fn placeholder_ops(label: &str, detail: &str, x: f32, y: f32, w: f32, h: f32) -> Vec<Op> {
    let mut ops = vec![Op::Rect {
        x,
        y,
        w,
        h,
        colour: rgb(theme::PLACEHOLDER_FILL),
    }];
    ops.push(frame_ops(x, y, w, h, rgb(theme::PLACEHOLDER_EDGE)));
    let colour = rgb(theme::UNSUPPORTED);
    let size = 13.0;
    let width = text_width(label, Face::Roman, size);
    ops.extend(text_at(
        label,
        x + (w - width) / 2.0,
        y + h / 2.0,
        size,
        colour,
    ));
    if !detail.is_empty() {
        let size = 10.0;
        let width = text_width(detail, Face::Roman, size);
        ops.extend(text_at(
            detail,
            x + (w - width) / 2.0,
            y + h / 2.0 + 14.0,
            size,
            colour,
        ));
    }
    ops
}

/// A closed rectangle outline.
fn frame_ops(x: f32, y: f32, w: f32, h: f32, colour: Rgb) -> Op {
    line(
        vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h], [x, y]],
        1.0,
        colour,
    )
}

fn line(points: Vec<[f32; 2]>, width: f32, colour: Rgb) -> Op {
    Op::Line {
        points,
        width,
        colour,
    }
}

/// A single text operation whose left end is at `x` on baseline `baseline`.
fn text_at(text: &str, x: f32, baseline: f32, size: f32, colour: Rgb) -> Vec<Op> {
    let runs = runs_for(text, Face::Roman, false);
    if runs.is_empty() {
        return Vec::new();
    }
    vec![Op::Text {
        x,
        baseline,
        size,
        colour,
        runs,
    }]
}

/// The width of `text` in points at `size`, with the bundled advances.
fn text_width(text: &str, face: Face, size: f32) -> f32 {
    text.chars()
        .map(|c| fonts::advance_em(face, c).unwrap_or(0.0) * size)
        .sum()
}

/// Splits text into runs of one embedded font file each. Characters no bundled face has are
/// left out, as the window draws them as boxes that a PDF cannot reproduce.
/// The runs that draw `text` in `face`: the sheet's STIX faces, or Noto Sans for text regions
/// (`sans`).
fn runs_for(text: &str, face: Face, sans: bool) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for ch in text.chars() {
        let slot = if sans {
            fonts::text_slot(face, ch)
        } else {
            fonts::slot(face, ch)
        };
        let Some((file, gid)) = slot else {
            continue;
        };
        let glyph = Glyph { gid, ch };
        match runs.last_mut() {
            Some(run) if run.file == file => run.glyphs.push(glyph),
            _ => runs.push(Run {
                file,
                glyphs: vec![glyph],
            }),
        }
    }
    runs
}

fn rgb(c: Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}

#[cfg(test)]
#[path = "export_print_tests.rs"]
mod print_file_tests;

//! Read-only views of a document: the outline, the content of a region, the variables, the errors
//! and the plain-text transcript. All text is English and meant to be read by a program or an AI
//! as well as by a person.

use mathcraft_core::expr::Expr;
use mathcraft_import::regions::{RegionKind, RegionModel};
use mathcraft_import::rich_text::{Align, TextBody};

use crate::calc::{Calculation, RegionResult, ResultStatus};
use crate::document::Document;
use crate::plot::{spec_of, PlotSpec};

/// Longest one-line summary, in characters.
const SUMMARY_CHARS: usize = 80;

/// One line of the outline.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionSummary {
    pub id: u32,
    /// `math`, `text`, `plot`, `plot3d`, `solve_block`, `area`, `image`, `table`, `page_break` or
    /// `unsupported`.
    pub kind: &'static str,
    /// `(left, top)` in page points.
    pub pos: Option<(f64, f64)>,
    /// `(width, height)` in page points.
    pub size: Option<(f64, f64)>,
    /// The text of a math or text region, or a short description. At most 80 characters.
    pub summary: String,
}

/// A piece of text with one style.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub colour: [u8; 4],
    pub link: Option<String>,
}

/// One line of a text region.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    /// `left`, `center`, `right` or `justify`.
    pub align: &'static str,
    pub runs: Vec<TextRun>,
}

/// What a region holds.
#[derive(Debug, Clone, PartialEq)]
pub enum RegionContent {
    /// Math in Mathcraft's text notation, for example `v:=d/t`.
    Math {
        text: String,
    },
    /// Text with its runs.
    Text {
        text: String,
        lines: Vec<TextLine>,
    },
    /// A plot: its type, and the text form of a 2D plot.
    Plot {
        kind: String,
        spec: Option<PlotSpec>,
    },
    Image {
        media_type: Option<&'static str>,
        bytes: usize,
    },
    Area {
        locked: bool,
        regions: usize,
    },
    /// A solve block with the text of its members.
    SolveBlock {
        members: Vec<String>,
    },
    Table,
    PageBreak,
    Unsupported {
        reason: String,
    },
}

/// Everything about one region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionDetail {
    pub id: u32,
    pub kind: &'static str,
    pub pos: Option<(f64, f64)>,
    pub size: Option<(f64, f64)>,
    pub content: RegionContent,
    /// The result of the last calculation, when it is not stale.
    pub result: Option<RegionResult>,
}

/// Whether a name is a variable or a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Variable,
    Function,
}

/// A name the worksheet defines.
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub kind: VariableKind,
    /// The region that defines it (the last one when several do).
    pub region_id: u32,
    /// The definition as text, for example `v:=d/t`.
    pub definition: String,
    /// The value as text, for a variable that evaluates.
    pub value: Option<String>,
}

/// An error or a note about the worksheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// The region the problem is in, `None` for the worksheet.
    pub region_id: Option<u32>,
    /// `error`, `skipped`, `unsupported` or `note`.
    pub severity: &'static str,
    pub message: String,
}

/// The kind name of a region.
pub fn kind_name(kind: &RegionKind) -> &'static str {
    match kind {
        RegionKind::Math(_) => "math",
        RegionKind::Text(_) => "text",
        RegionKind::SolveBlock(_) => "solve_block",
        RegionKind::Plot(info) if info.plot3d.is_some() => "plot3d",
        RegionKind::Plot(_) => "plot",
        RegionKind::Area(_) => "area",
        RegionKind::Image(_) => "image",
        RegionKind::Table(_) => "table",
        RegionKind::PageBreak => "page_break",
        RegionKind::Unsupported { .. } => "unsupported",
    }
}

fn one_line(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    let mut out: String = flat.chars().take(SUMMARY_CHARS).collect();
    if flat.chars().count() > SUMMARY_CHARS {
        out.push('…');
    }
    out
}

/// The text of a region for the outline and the transcript.
pub fn region_text(region: &RegionModel) -> String {
    match &region.kind {
        RegionKind::Math(expr) => expr.to_string(),
        RegionKind::Text(body) => body.as_str().to_string(),
        RegionKind::Plot(info) => match spec_of(region) {
            Some(spec) => plot_text(&info.kind, &spec),
            None => format!("{} plot", info.kind),
        },
        RegionKind::SolveBlock(_) => "solve block".to_string(),
        RegionKind::Area(_) => "area".to_string(),
        RegionKind::Image(_) => "image".to_string(),
        RegionKind::Table(_) => "table".to_string(),
        RegionKind::PageBreak => "page break".to_string(),
        RegionKind::Unsupported { reason } => format!("unsupported: {reason}"),
    }
}

/// One line for a plot, each trace as `y1 = sin(x); y2 = cos(x) over x`; a parametric trace reads
/// `(x1, y1) = (...)`. The x axis is named only when it is not the plain `x`.
fn plot_text(kind: &str, spec: &PlotSpec) -> String {
    let xs = crate::plot::split_expressions(&spec.x);
    let ys = crate::plot::split_expressions(&spec.y);
    let (label, radius) = if kind == "polarPlot" {
        ("polar plot", "r")
    } else {
        ("xy plot", "y")
    };
    let mut text = if xs.len() == ys.len() && !ys.is_empty() {
        let numbered = ys.len() > 1;
        let traces: Vec<String> = xs
            .iter()
            .zip(&ys)
            .enumerate()
            .map(|(i, (x, y))| {
                let n = if numbered {
                    (i + 1).to_string()
                } else {
                    String::new()
                };
                if x == "x" {
                    format!("{radius}{n} = {y}")
                } else {
                    format!("(x{n}, {radius}{n}) = ({x}, {y})")
                }
            })
            .collect();
        let over = if xs.iter().all(|x| x == "x") {
            " over x"
        } else {
            ""
        };
        format!("{label}: {}{over}", traces.join("; "))
    } else {
        format!("{label}: x = {}, y = {}", spec.x, spec.y)
    };
    if !spec.title.is_empty() {
        text.push_str(&format!(", title \"{}\"", spec.title));
    }
    text
}

/// The outline: every top-level region in document order.
pub fn outline(doc: &Document) -> Vec<RegionSummary> {
    doc.regions().iter().map(summary).collect()
}

fn summary(region: &RegionModel) -> RegionSummary {
    RegionSummary {
        id: region.id,
        kind: kind_name(&region.kind),
        pos: region.pos,
        size: region.size,
        summary: one_line(&region_text(region)),
    }
}

fn align_name(align: Align) -> &'static str {
    match align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
    }
}

fn text_lines(body: &TextBody) -> Vec<TextLine> {
    body.rich()
        .lines
        .iter()
        .map(|line| TextLine {
            align: align_name(line.align),
            runs: line
                .runs
                .iter()
                .map(|run| TextRun {
                    text: run.text.clone(),
                    family: run.style.family.clone(),
                    size: run.style.size,
                    bold: run.style.bold,
                    italic: run.style.italic,
                    underline: run.style.underline,
                    colour: run.style.colour,
                    link: run.style.link.clone(),
                })
                .collect(),
        })
        .collect()
}

fn content(region: &RegionModel) -> RegionContent {
    match &region.kind {
        RegionKind::Math(expr) => RegionContent::Math {
            text: expr.to_string(),
        },
        RegionKind::Text(body) => RegionContent::Text {
            text: body.as_str().to_string(),
            lines: text_lines(body),
        },
        RegionKind::Plot(info) => RegionContent::Plot {
            kind: info.kind.clone(),
            spec: spec_of(region),
        },
        RegionKind::Image(image) => RegionContent::Image {
            media_type: image.media_type(),
            bytes: image.bytes.len(),
        },
        RegionKind::Area(area) => RegionContent::Area {
            locked: area.locked,
            regions: region.children.len(),
        },
        RegionKind::SolveBlock(_) => RegionContent::SolveBlock {
            members: region.children.iter().map(region_text).collect(),
        },
        RegionKind::Table(_) => RegionContent::Table,
        RegionKind::PageBreak => RegionContent::PageBreak,
        RegionKind::Unsupported { reason } => RegionContent::Unsupported {
            reason: reason.clone(),
        },
    }
}

/// The content of region `id` with its result from `calc` (pass only a current calculation).
pub fn detail(region: &RegionModel, calc: Option<&Calculation>) -> RegionDetail {
    RegionDetail {
        id: region.id,
        kind: kind_name(&region.kind),
        pos: region.pos,
        size: region.size,
        content: content(region),
        result: calc.and_then(|c| c.result(region.id)).cloned(),
    }
}

/// The names the worksheet defines, from the top-level math regions and the ones nested in them,
/// in document order. A name defined twice is listed once, at its last definition.
pub fn variables(doc: &Document) -> Vec<Variable> {
    let mut found: Vec<Variable> = Vec::new();
    let mut ordered: Vec<&RegionModel> = Vec::new();
    collect(doc.regions(), &mut ordered);
    // Evaluation order is by position, so list in that order.
    ordered.sort_by(|a, b| position_key(a).partial_cmp(&position_key(b)).unwrap());
    for region in ordered {
        let RegionKind::Math(expr) = &region.kind else {
            continue;
        };
        let (Expr::Define { target, .. } | Expr::GlobalDefine { target, .. }) = expr else {
            continue;
        };
        let (name, kind) = match target.as_ref() {
            Expr::Id(_) => (target.to_string(), VariableKind::Variable),
            Expr::Call { func, .. } => (Expr::Id(func.clone()).to_string(), VariableKind::Function),
            _ => continue,
        };
        found.retain(|v| v.name != name);
        found.push(Variable {
            name,
            kind,
            region_id: region.id,
            definition: expr.to_string(),
            value: None,
        });
    }
    found
}

fn collect<'a>(list: &'a [RegionModel], out: &mut Vec<&'a RegionModel>) {
    for region in list {
        out.push(region);
        collect(&region.children, out);
    }
}

fn position_key(region: &RegionModel) -> (u8, f64, f64) {
    match region.pos {
        Some((x, y)) => (0, (y * 10.0).round(), (x * 10.0).round()),
        None => (1, 0.0, 0.0),
    }
}

/// The errors and notes of a calculation, and the regions the importer could not read.
pub fn problems(doc: &Document, calc: &Calculation) -> Vec<Problem> {
    let mut out = Vec::new();
    for r in &calc.results {
        match r.status {
            ResultStatus::Error => out.push(Problem {
                region_id: Some(r.region_id),
                severity: "error",
                message: format!("{}: {}", r.expr, r.error.as_deref().unwrap_or("error")),
            }),
            ResultStatus::Skipped => out.push(Problem {
                region_id: Some(r.region_id),
                severity: "skipped",
                message: r.error.clone().unwrap_or_default(),
            }),
            _ => {}
        }
    }
    for region in doc.regions() {
        if let RegionKind::Unsupported { reason } = &region.kind {
            out.push(Problem {
                region_id: Some(region.id),
                severity: "unsupported",
                message: reason.clone(),
            });
        }
    }
    for note in &calc.notes {
        out.push(Problem {
            region_id: None,
            severity: "note",
            message: note.clone(),
        });
    }
    out
}

/// The whole sheet as plain text: one line per region, top to bottom and left to right, with the
/// id, the text and the result of the calculation (when there is one).
pub fn transcript(doc: &Document, calc: Option<&Calculation>) -> String {
    let mut ordered: Vec<&RegionModel> = doc.regions().iter().collect();
    ordered.sort_by(|a, b| position_key(a).partial_cmp(&position_key(b)).unwrap());
    let mut out = String::new();
    for region in ordered {
        let kind = kind_name(&region.kind);
        let text = region_text(region);
        out.push_str(&format!("[{}] {kind}: {text}", region.id));
        if let Some(r) = calc.and_then(|c| c.result(region.id)) {
            match r.status {
                ResultStatus::Value | ResultStatus::Plot if !r.value.is_empty() => {
                    out.push_str(&format!("  => {}", r.value));
                }
                ResultStatus::Error => {
                    out.push_str(&format!("  !! {}", r.error.as_deref().unwrap_or("error")));
                }
                ResultStatus::Skipped => {
                    out.push_str(&format!(
                        "  (not run: {})",
                        r.error.as_deref().unwrap_or("")
                    ));
                }
                _ => {}
            }
        }
        out.push('\n');
    }
    out
}

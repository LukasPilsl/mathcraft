//! Write tools: insert_math, insert_text, edit_region, move_region, resize_region, delete_region
//! and the atomic `batch`. Every call answers with the regions it touched, recalculated, and the
//! errors that appeared, so the client sees the effect at once.
//!
//! One op parser serves the single tools and `batch`, so both accept the same fields.

use std::collections::HashSet;

use mathcraft_doc::{CalcOptions, Doc, DocError, Placement, RegionResult, ResultStatus};
use mathcraft_import::regions::RegionKind;
use mathcraft_import::rich_text::{Align, Line, RichText, Run, RunStyle, TextBody};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::areas::{self, Layout};
use crate::tools::blocks::{self, BlockWrite};
use crate::tools::plots::{self, PlotWrite};
use crate::tools::view::status_name;
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

/// Longest text of one region, in bytes.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Most ops in one batch.
pub const MAX_BATCH_OPS: usize = 200;
/// Most runs in one text region.
const MAX_RUNS: usize = 500;
/// Largest coordinate or size accepted, in page points.
const MAX_COORD: f64 = 1.0e6;
/// Most regions whose results one response lists.
const MAX_AFFECTED: usize = 100;
/// Most new errors one response lists.
const MAX_NEW_ERRORS: usize = 50;
/// Longest value or expression text in a response, in characters.
const MAX_VALUE_CHARS: usize = 1000;

const BATCH_LABEL: &str = "AI: batch";

const OP_NAMES: [&str; 14] = [
    "insert_math",
    "insert_text",
    "edit_region",
    "move_region",
    "resize_region",
    "delete_region",
    "insert_plot",
    "set_plot_options",
    "insert_symbolic",
    "insert_solve_block",
    "insert_program",
    "insert_area",
    "insert_page_break",
    "insert_image",
];

/// The undo label of a single write tool's step, the same name live mode gives it.
fn single_label(name: &str) -> &'static str {
    match name {
        "insert_math" => "AI: insert_math",
        "insert_text" => "AI: insert_text",
        "edit_region" => "AI: edit_region",
        "move_region" => "AI: move_region",
        "resize_region" => "AI: resize_region",
        "delete_region" => "AI: delete_region",
        "insert_area" => "AI: insert_area",
        "insert_page_break" => "AI: insert_page_break",
        "insert_image" => "AI: insert_image",
        _ => "AI: edit",
    }
}

/// `{"type":"number"}` (the shared schema helpers only have integers).
pub fn number_schema(description: &str) -> Value {
    json!({"type": "number", "description": description})
}

pub fn register(t: &mut ToolTable) {
    let at_props = || {
        vec![
            ("x", number_schema("Left edge in page points (needs y)")),
            ("y", number_schema("Top edge in page points (needs x)")),
            (
                "after_region",
                schema::integer("Place directly below this region"),
            ),
        ]
    };
    let mut props = vec![
        schema::document_id(),
        (
            "text",
            schema::string("Math in Mathcraft linear text syntax, for example `v:=len/dur`"),
        ),
    ];
    props.extend(at_props());
    t.add(Tool {
        name: "insert_math",
        description: "Inserts a math region from Mathcraft linear text syntax (`v:=len/dur`, `A:=[1 2;3 4]`). Without x/y/after_region it goes below the last region. Returns region_id with its calculated result or error, and any new errors elsewhere.".into(),
        input_schema: schema::object(&props, &["document_id", "text"]),
        annotations: write_annotations("Insert math", false, false),
        handler: |ctx, args| single(ctx, args, "insert_math"),
    });
    let mut props = vec![
        schema::document_id(),
        (
            "text",
            schema::string("Plain text; newlines start new lines"),
        ),
        ("runs", runs_schema()),
    ];
    props.extend(at_props());
    t.add(Tool {
        name: "insert_text",
        description: "Inserts a text region. Give `text` (plain) or `runs` (styled pieces: text, bold, italic, underline, size, font, link), not both. A run's `link` makes it a hyperlink (http://, https:// or mailto:). Placement as insert_math.".into(),
        input_schema: schema::object(&props, &["document_id"]),
        annotations: write_annotations("Insert text", false, false),
        handler: |ctx, args| single(ctx, args, "insert_text"),
    });
    t.add(Tool {
        name: "edit_region",
        description: "Replaces the content of a math or text region (kind is kept). Overwrites the old content (undo restores it). Returns the new result or error.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region to edit")),
                ("text", schema::string("New content: linear syntax for math, plain text for text")),
            ],
            &["document_id", "region_id", "text"],
        ),
        annotations: write_annotations("Edit region", true, true),
        handler: |ctx, args| single(ctx, args, "edit_region"),
    });
    t.add(Tool {
        name: "move_region",
        description: "Moves a region to x, y (page points). Position matters: Mathcraft calculates top to bottom, left to right.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region to move")),
                ("x", number_schema("Left edge in page points")),
                ("y", number_schema("Top edge in page points")),
            ],
            &["document_id", "region_id", "x", "y"],
        ),
        annotations: write_annotations("Move region", false, true),
        handler: |ctx, args| single(ctx, args, "move_region"),
    });
    t.add(Tool {
        name: "resize_region",
        description: "Sets the size of a region in page points, keeping its position.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region to resize")),
                ("width", number_schema("Width in page points, above 0")),
                ("height", number_schema("Height in page points, above 0")),
            ],
            &["document_id", "region_id", "width", "height"],
        ),
        annotations: write_annotations("Resize region", false, true),
        handler: |ctx, args| single(ctx, args, "resize_region"),
    });
    t.add(Tool {
        name: "delete_region",
        description: "Deletes a region (undo restores it). Definitions it made disappear, so regions that use them may now show errors; the response lists them.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region to delete")),
            ],
            &["document_id", "region_id"],
        ),
        annotations: write_annotations("Delete region", true, false),
        handler: |ctx, args| single(ctx, args, "delete_region"),
    });
    t.add(Tool {
        name: "batch",
        description: format!("Applies up to {MAX_BATCH_OPS} ops atomically: all succeed or none is applied. One undo step, labelled \"{BATCH_LABEL}\". Each op is an object with `op` (insert_math, insert_text, edit_region, move_region, resize_region, delete_region, insert_plot, set_plot_options, insert_symbolic, insert_solve_block, insert_program) and the fields of that tool without document_id. Layout ops: insert_area, insert_page_break, insert_image. Strongly preferred for building a sheet, plots included. Returns per-op results and the final results of all touched regions."),
        input_schema: schema::object(
            &[
                schema::document_id(),
                (
                    "ops",
                    json!({
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_BATCH_OPS,
                        "items": {
                            "type": "object",
                            "properties": {"op": {"type": "string", "enum": OP_NAMES}},
                            "required": ["op"],
                        },
                        "description": "Ops, applied in order",
                    }),
                ),
            ],
            &["document_id", "ops"],
        ),
        annotations: write_annotations("Batch edit", true, false),
        handler: batch,
    });
}

fn write_annotations(title: &'static str, destructive: bool, idempotent: bool) -> Annotations {
    Annotations {
        title,
        read_only: false,
        destructive,
        idempotent,
    }
}

fn runs_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAX_RUNS,
        "items": {
            "type": "object",
            "properties": {
                "text": {"type": "string"},
                "bold": {"type": "boolean"},
                "italic": {"type": "boolean"},
                "underline": {"type": "boolean"},
                "size": {"type": "number"},
                "font": {"type": "string"},
                "link": {"type": "string", "description": "Target of a hyperlink on this run: http://, https:// or mailto:"},
            },
            "required": ["text"],
            "additionalProperties": false,
        },
        "description": "Styled pieces of one text region; a newline starts a new line",
    })
}

/// One edit.
enum Op {
    InsertMath { text: String, at: Placement },
    InsertText { body: TextBody, at: Placement },
    Edit { id: u32, text: String },
    Move { id: u32, to: [f64; 2] },
    Resize { id: u32, size: (f64, f64) },
    Delete { id: u32 },
    Plot(Box<PlotWrite>),
    Block(Box<BlockWrite>),
    Layout(Box<Layout>),
}

/// What an op did.
struct Done {
    /// The region it created or changed.
    region: u32,
    deleted: bool,
    /// More regions whose results belong in the answer (the members of a solve block).
    also: Vec<u32>,
}

impl Done {
    fn of(region: u32) -> Self {
        Self {
            region,
            deleted: false,
            also: Vec::new(),
        }
    }
}

pub(crate) fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::InvalidParams(msg.into())
}

pub(crate) fn doc_err(e: DocError) -> ToolError {
    match e {
        DocError::TooManyRegions => ToolError::LimitReached(
            "the worksheet is at its region cap; delete regions or start another sheet".into(),
        ),
        DocError::Invalid(m) => ToolError::InvalidParams(m),
        other => ToolError::from(other),
    }
}

pub(crate) fn number(args: &Args<'_>, key: &str) -> Result<Option<f64>, ToolError> {
    match args.0.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(v) if v.is_finite() => Ok(Some(v)),
            _ => Err(invalid(format!("`{key}` must be a finite number"))),
        },
        Some(_) => Err(invalid(format!("`{key}` must be a number"))),
    }
}

pub(crate) fn coordinate(args: &Args<'_>, key: &str) -> Result<Option<f64>, ToolError> {
    match number(args, key)? {
        Some(v) if !(0.0..=MAX_COORD).contains(&v) => Err(invalid(format!(
            "`{key}` must be between 0 and {MAX_COORD} page points"
        ))),
        other => Ok(other),
    }
}

pub(crate) fn dimension(args: &Args<'_>, key: &str) -> Result<f64, ToolError> {
    match number(args, key)? {
        None => Err(invalid(format!("missing argument `{key}`"))),
        Some(v) if v <= 0.0 || v > MAX_COORD => Err(invalid(format!(
            "`{key}` must be above 0 and at most {MAX_COORD} page points"
        ))),
        Some(v) => Ok(v),
    }
}

/// A string argument of at most [`MAX_TEXT_BYTES`].
pub fn limited_text<'a>(args: &'a Args<'_>, key: &str) -> Result<&'a str, ToolError> {
    let s = args.str(key)?;
    if s.len() > MAX_TEXT_BYTES {
        return Err(ToolError::LimitReached(format!(
            "`{key}` is {} bytes, over the {MAX_TEXT_BYTES} byte limit; split it into several regions",
            s.len()
        )));
    }
    Ok(s)
}

pub(crate) fn placement(args: &Args<'_>) -> Result<Placement, ToolError> {
    let x = coordinate(args, "x")?;
    let y = coordinate(args, "y")?;
    let after = args.opt_u64("after_region")?;
    match (x, y, after) {
        (None, None, None) => Ok(Placement::Auto),
        (Some(x), Some(y), None) => Ok(Placement::At([x, y])),
        (None, None, Some(_)) => Ok(Placement::After(args.region_id("after_region")?)),
        (Some(_), Some(_), Some(_)) => Err(invalid("give x and y, or after_region, not both")),
        _ => Err(invalid("x and y go together")),
    }
}

fn empty_line() -> Line {
    Line {
        align: Align::Left,
        broken: false,
        runs: Vec::new(),
    }
}

/// The target of a hyperlink: `http`, `https` or `mailto`, at most 2048 characters, without spaces
/// or control characters. Anything else (`javascript:`, file paths) is refused.
pub(crate) fn hyperlink(url: &str) -> Result<String, ToolError> {
    const MAX_LINK: usize = 2048;
    if url.len() > MAX_LINK {
        return Err(invalid(format!("`link` is over {MAX_LINK} characters")));
    }
    let lower = url.to_ascii_lowercase();
    let scheme_ok = ["http://", "https://", "mailto:"]
        .iter()
        .any(|s| lower.starts_with(s) && url.len() > s.len());
    if !scheme_ok {
        return Err(invalid(
            "`link` must start with http://, https:// or mailto: and name a target",
        ));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid(
            "`link` must not contain spaces or control characters",
        ));
    }
    Ok(url.to_owned())
}

fn runs_body(runs: &Value) -> Result<TextBody, ToolError> {
    let Some(items) = runs.as_array() else {
        return Err(invalid("`runs` must be an array"));
    };
    if items.len() > MAX_RUNS {
        return Err(ToolError::LimitReached(format!(
            "`runs` has {} items, over the limit of {MAX_RUNS}",
            items.len()
        )));
    }
    let mut total = 0usize;
    let mut lines = vec![empty_line()];
    for item in items {
        let Some(map) = item.as_object() else {
            return Err(invalid("each run must be an object"));
        };
        let a = Args(map);
        a.only(&[
            "text",
            "bold",
            "italic",
            "underline",
            "size",
            "font",
            "link",
        ])?;
        let text = a.str("text")?;
        total += text.len();
        if total > MAX_TEXT_BYTES {
            return Err(ToolError::LimitReached(format!(
                "the runs hold over {MAX_TEXT_BYTES} bytes of text"
            )));
        }
        let mut style = RunStyle {
            bold: a.opt_bool("bold")?,
            italic: a.opt_bool("italic")?,
            underline: a.opt_bool("underline")?,
            ..RunStyle::default()
        };
        if let Some(size) = number(&a, "size")? {
            if !(1.0..=500.0).contains(&size) {
                return Err(invalid("`size` must be between 1 and 500 points"));
            }
            style.size = size as f32;
        }
        if let Some(font) = a.opt_str("font")? {
            if font.len() > 100 {
                return Err(invalid("`font` is too long"));
            }
            style.family = font.to_owned();
        }
        let link = match a.opt_str("link")? {
            None => None,
            Some(url) => Some(hyperlink(url)?),
        };
        style.link = link;
        for (i, piece) in text.split('\n').enumerate() {
            if i > 0 {
                lines.push(empty_line());
            }
            if !piece.is_empty() {
                if let Some(line) = lines.last_mut() {
                    line.runs.push(Run {
                        text: piece.to_owned(),
                        style: style.clone(),
                    });
                }
            }
        }
    }
    Ok(TextBody::from_rich(RichText {
        lines,
        base: RunStyle::default(),
        section_attrs: None,
    }))
}

/// Reads one op. Standalone ops carry `document_id`, ops in a batch carry `op` instead.
fn parse_op(
    ctx: &ToolCtx<'_>,
    name: &str,
    args: &Args<'_>,
    standalone: bool,
) -> Result<Op, ToolError> {
    let head: &[&str] = if standalone {
        &["document_id"]
    } else {
        &["op"]
    };
    let allowed = |extra: &[&str]| -> Result<(), ToolError> {
        let all: Vec<&str> = head.iter().chain(extra).copied().collect();
        args.only(&all)
    };
    match name {
        "insert_math" => {
            allowed(&["text", "x", "y", "after_region"])?;
            Ok(Op::InsertMath {
                text: limited_text(args, "text")?.to_owned(),
                at: placement(args)?,
            })
        }
        "insert_text" => {
            allowed(&["text", "runs", "x", "y", "after_region"])?;
            let runs = args.0.get("runs").filter(|r| !r.is_null());
            let body = match (args.opt_str("text")?, runs) {
                (Some(_), Some(_)) => return Err(invalid("give `text` or `runs`, not both")),
                (Some(_), None) => TextBody::from_plain(limited_text(args, "text")?),
                (None, Some(r)) => runs_body(r)?,
                (None, None) => return Err(invalid("missing argument `text` (or `runs`)")),
            };
            Ok(Op::InsertText {
                body,
                at: placement(args)?,
            })
        }
        "edit_region" => {
            allowed(&["region_id", "text"])?;
            Ok(Op::Edit {
                id: args.region_id("region_id")?,
                text: limited_text(args, "text")?.to_owned(),
            })
        }
        "move_region" => {
            allowed(&["region_id", "x", "y"])?;
            let (Some(x), Some(y)) = (coordinate(args, "x")?, coordinate(args, "y")?) else {
                return Err(invalid("move_region needs x and y"));
            };
            Ok(Op::Move {
                id: args.region_id("region_id")?,
                to: [x, y],
            })
        }
        "resize_region" => {
            allowed(&["region_id", "width", "height"])?;
            Ok(Op::Resize {
                id: args.region_id("region_id")?,
                size: (dimension(args, "width")?, dimension(args, "height")?),
            })
        }
        "delete_region" => {
            allowed(&["region_id"])?;
            Ok(Op::Delete {
                id: args.region_id("region_id")?,
            })
        }
        "insert_plot" | "set_plot_options" => Ok(Op::Plot(Box::new(plots::parse_plot(
            name, args, standalone,
        )?))),
        "insert_area" | "insert_page_break" | "insert_image" => Ok(Op::Layout(Box::new(
            areas::parse(ctx, name, args, standalone)?,
        ))),
        "insert_symbolic" | "insert_solve_block" | "insert_program" => Ok(Op::Block(Box::new(
            blocks::parse_block(name, args, standalone)?,
        ))),
        other => Err(invalid(format!(
            "unknown op `{other}`; use one of {}",
            OP_NAMES.join(", ")
        ))),
    }
}

fn apply(doc: &mut Doc, op: Op) -> Result<Done, ToolError> {
    match op {
        Op::Plot(w) => plots::apply_plot(doc, &w).map(Done::of),
        Op::Layout(l) => areas::apply(doc, *l).map(Done::of),
        Op::Block(w) => blocks::apply_block(doc, &w).map(|a| Done {
            also: a.touched,
            ..Done::of(a.region)
        }),
        other => apply_doc(doc, other),
    }
}

/// The ops the document layer applies directly (everything but plots).
fn apply_doc(doc: &mut Doc, op: Op) -> Result<Done, ToolError> {
    let done = Done::of;
    match op {
        Op::Plot(_) | Op::Block(_) | Op::Layout(_) => {
            Err(DocError::Invalid("op outside apply".into()))
        }
        Op::InsertMath { text, at } => doc.insert_math(&text, at).map(done),
        Op::InsertText { body, at } => doc.insert_text_body(body, at).map(done),
        Op::Edit { id, text } => {
            let is_text = match doc.region(id).map(|r| &r.kind) {
                None => return Err(ToolError::from(DocError::NotFound(id))),
                Some(RegionKind::Math(_)) => false,
                Some(RegionKind::Text(_)) => true,
                Some(_) => {
                    return Err(ToolError::Doc(format!(
                        "region {id} is neither math nor text and cannot be edited this way"
                    )))
                }
            };
            if is_text {
                doc.edit_text(id, &text)
            } else {
                doc.edit_math(id, &text)
            }
            .map(|()| done(id))
        }
        Op::Move { id, to } => doc.move_region(id, to).map(|()| done(id)),
        Op::Resize { id, size } => doc.resize_region(id, size).map(|()| done(id)),
        Op::Delete { id } => doc.delete_region(id).map(|()| Done {
            deleted: true,
            ..Done::of(id)
        }),
    }
    .map_err(doc_err)
}

fn clip(s: &str) -> Value {
    let n = s.chars().count();
    if n > MAX_VALUE_CHARS {
        let head: String = s.chars().take(MAX_VALUE_CHARS).collect();
        json!(format!("{head}... (cut, {n} characters in all)"))
    } else {
        json!(s)
    }
}

/// A region result as JSON with long texts cut, so a response stays small.
pub fn result_view(r: &RegionResult) -> Value {
    json!({
        "region_id": r.region_id,
        "status": status_name(r.status),
        "expression": clip(&r.expr),
        "value": clip(&r.value),
        "error": r.error.as_deref().map(clip),
        "defines": r.defines,
    })
}

type ErrorKey = (u32, String);

fn error_key(r: &RegionResult) -> ErrorKey {
    (r.region_id, r.error.clone().unwrap_or_default())
}

/// The errors the document has now (calculating first when that is not yet done), to tell later
/// which errors an edit added.
pub(crate) fn error_keys(doc: &mut Doc) -> HashSet<ErrorKey> {
    if doc.current_calculation().is_none() && !doc.regions().is_empty() {
        doc.calculate(&CalcOptions::default());
    }
    doc.current_calculation()
        .map(|c| {
            c.results
                .iter()
                .filter(|r| r.status == ResultStatus::Error)
                .map(error_key)
                .collect()
        })
        .unwrap_or_default()
}

/// Recalculates and describes the effect of an edit: results of the touched regions, errors that
/// were not there before, and why the run stopped early (if it did).
pub(crate) fn feedback(
    doc: &mut Doc,
    before: &HashSet<ErrorKey>,
    touched: &[u32],
    deleted: &[u32],
) -> Value {
    let raw = doc.calculate(&CalcOptions::default());
    let calc = crate::tools::format::display_calc(doc, &raw);
    let mut seen = HashSet::new();
    let ids: Vec<u32> = touched
        .iter()
        .copied()
        .filter(|id| !deleted.contains(id) && seen.insert(*id))
        .collect();
    let results: Vec<Value> = ids
        .iter()
        .filter_map(|id| calc.result(*id))
        .take(MAX_AFFECTED)
        .map(result_view)
        .collect();
    let errors: Vec<&RegionResult> = calc
        .results
        .iter()
        .filter(|r| r.status == ResultStatus::Error)
        .collect();
    let new_errors: Vec<Value> = errors
        .iter()
        .filter(|r| !before.contains(&error_key(r)))
        .take(MAX_NEW_ERRORS)
        .map(|r| result_view(r))
        .collect();
    json!({
        "results": results,
        "results_truncated": ids.len() > MAX_AFFECTED,
        "deleted": deleted,
        "new_errors": new_errors,
        "error_count": errors.len(),
        "stopped": calc.stopped,
        "revision": doc.revision(),
    })
}

pub(crate) fn merge(mut base: Value, extra: Value) -> Value {
    if let (Some(b), Value::Object(e)) = (base.as_object_mut(), extra) {
        b.extend(e);
    }
    base
}

pub(crate) fn single(
    ctx: &ToolCtx<'_>,
    args: &Args<'_>,
    name: &'static str,
) -> Result<ToolOutput, ToolError> {
    let mut op = Some(parse_op(ctx, name, args, true)?);
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let op = op
            .take()
            .ok_or_else(|| ToolError::Internal("write op ran twice".into()))?;
        let before = error_keys(doc);
        let start = doc.steps_executed();
        let done = apply(doc, op)?;
        let taken = doc.steps_executed() - start;
        if taken > 0 {
            doc.collapse_steps(taken, single_label(name));
        }
        let deleted: Vec<u32> = if done.deleted {
            vec![done.region]
        } else {
            vec![]
        };
        let mut touched = vec![done.region];
        touched.extend(&done.also);
        Ok(merge(
            json!({"ok": true, "region_id": done.region}),
            feedback(doc, &before, &touched, &deleted),
        ))
    })?;
    Ok(ToolOutput::json(v))
}

fn batch(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "ops"])?;
    let Some(items) = args.0.get("ops").and_then(Value::as_array) else {
        return Err(invalid("`ops` must be an array of op objects"));
    };
    if items.is_empty() {
        return Err(invalid("`ops` is empty"));
    }
    if items.len() > MAX_BATCH_OPS {
        return Err(ToolError::LimitReached(format!(
            "{} ops in one batch, over the limit of {MAX_BATCH_OPS}; split it",
            items.len()
        )));
    }
    // Everything is checked before the document is touched, so a typo costs no rollback.
    let mut ops = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let at = |e: ToolError| match e {
            ToolError::InvalidParams(m) => invalid(format!("op {i}: {m}")),
            ToolError::LimitReached(m) => ToolError::LimitReached(format!("op {i}: {m}")),
            other => other,
        };
        let Some(map) = item.as_object() else {
            return Err(invalid(format!("op {i} must be an object")));
        };
        let a = Args(map);
        let name = a.str("op").map_err(at)?;
        ops.push((name.to_owned(), parse_op(ctx, name, &a, false).map_err(at)?));
    }
    let mut ops = Some(ops);
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let ops = ops
            .take()
            .ok_or_else(|| ToolError::Internal("batch ran twice".into()))?;
        let before = error_keys(doc);
        let start = doc.steps_executed();
        let mut per_op = Vec::with_capacity(ops.len());
        let mut touched = Vec::new();
        let mut deleted = Vec::new();
        for (i, (name, op)) in ops.into_iter().enumerate() {
            match apply(doc, op) {
                Ok(done) => {
                    touched.push(done.region);
                    touched.extend(&done.also);
                    if done.deleted {
                        deleted.push(done.region);
                    }
                    per_op.push(
                        json!({"index": i, "op": name, "ok": true, "region_id": done.region}),
                    );
                }
                Err(e) => {
                    let taken = doc.steps_executed() - start;
                    doc.rollback_steps(taken).map_err(|r| {
                        ToolError::Internal(format!("batch failed and could not be undone: {r}"))
                    })?;
                    return Err(batch_error(&e, i, &name));
                }
            }
        }
        let taken = doc.steps_executed() - start;
        doc.collapse_steps(taken, BATCH_LABEL);
        Ok(merge(
            json!({"ok": true, "ops": per_op}),
            feedback(doc, &before, &touched, &deleted),
        ))
    })?;
    Ok(ToolOutput::json(v))
}

/// The error of op `index`, with the same code and the position in the message.
fn batch_error(e: &ToolError, index: usize, name: &str) -> ToolError {
    let m = format!(
        "op {index} ({name}) failed, nothing was applied: {}",
        e.message()
    );
    match e {
        ToolError::InvalidParams(_) => ToolError::InvalidParams(m),
        ToolError::NotFound(_) => ToolError::NotFound(m),
        ToolError::LimitReached(_) => ToolError::LimitReached(m),
        ToolError::Internal(_) => ToolError::Internal(m),
        _ => ToolError::Doc(m),
    }
}

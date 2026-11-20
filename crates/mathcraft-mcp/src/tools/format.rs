//! Formatting tools: format_result (result display of a math region), format_text (styles and
//! paragraph alignment of a text region), set_page_setup and set_header_footer. Display settings
//! change what a result or page looks like, never what it calculates.
//!
//! Undo: format_text is one undo step. format_result, set_page_setup and set_header_footer are not
//! in the undo history (the Doc API sets them directly, as the GUI does); the response says so.

use mathcraft_core::eval::{MatrixDisplay, NumberFormat, Radix, ResultFormat};
use mathcraft_doc::read::kind_name;
use mathcraft_doc::Doc;
use mathcraft_import::page::PageSettings;
use mathcraft_import::regions::RegionKind;
use mathcraft_import::rich_text::{Align, StyleChange as TextChange, TextBody};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::plots::flag;
use crate::tools::view::detail_json;
use crate::tools::write::{doc_err, error_keys, feedback, invalid, merge, number};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

/// Longest header or footer, in characters.
const MAX_FURNITURE_CHARS: usize = 500;
/// Most visible rows or columns of a matrix result.
const MAX_MATRIX_VISIBLE: u32 = 1000;
/// Largest first row or column shown.
const MAX_MATRIX_OFFSET: u32 = 1_000_000;

const PAPER_CODES: [&str; 6] = ["A4", "Letter", "Legal", "A3", "Tabloid", "A5"];
const NOTATIONS: [&str; 5] = ["general", "decimal", "scientific", "engineering", "percent"];
const RADIXES: [&str; 4] = ["decimal", "binary", "octal", "hexadecimal"];
const FORMAT_KEYS: [&str; 9] = [
    "precision",
    "notation",
    "trailing_zeros",
    "exponential_threshold",
    "zero_threshold",
    "complex_threshold",
    "radix",
    "digit_grouping",
    "imaginary_j",
];
const MATRIX_KEYS: [&str; 6] = [
    "rows",
    "cols",
    "offset_row",
    "offset_col",
    "show_indices",
    "expand_nested",
];

pub fn register(t: &mut ToolTable) {
    t.add(Tool {
        name: "format_result",
        description: "Sets how a math region's result is displayed: precision (0 to 16 digits), notation (general, decimal, scientific, engineering, percent), trailing zeros, the exponential and zero thresholds, radix, digit grouping, imaginary unit j, and for matrix results the visible window (`matrix`: rows, cols, offset_row, offset_col, show_indices, expand_nested). `reset: true` makes the region follow the worksheet format again. Display only: values do not change. Not in the undo history. Returns the effective format and the recalculated result.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Math region whose result is shown")),
                ("precision", schema::integer("Digits right of the decimal point, 0 to 16")),
                ("notation", json!({"type": "string", "enum": NOTATIONS})),
                ("trailing_zeros", schema::boolean("Fill the precision with trailing zeros")),
                ("exponential_threshold", schema::integer("General format: exponent size (0 to 15) from which numbers show exponential")),
                ("zero_threshold", schema::integer("Numbers below 10^-n show as zero (0 to 307)")),
                ("complex_threshold", schema::integer("A complex part is dropped above 10^n times the other (0 to 307)")),
                ("radix", json!({"type": "string", "enum": RADIXES})),
                ("digit_grouping", schema::boolean("Group integer digits in threes")),
                ("imaginary_j", schema::boolean("Write the imaginary unit as j instead of i")),
                ("reset", schema::boolean("Follow the worksheet format again (no other format field)")),
                (
                    "matrix",
                    json!({
                        "type": "object",
                        "properties": {
                            "rows": {"type": "integer", "minimum": 1, "maximum": MAX_MATRIX_VISIBLE},
                            "cols": {"type": "integer", "minimum": 1, "maximum": MAX_MATRIX_VISIBLE},
                            "offset_row": {"type": "integer", "minimum": 0, "maximum": MAX_MATRIX_OFFSET},
                            "offset_col": {"type": "integer", "minimum": 0, "maximum": MAX_MATRIX_OFFSET},
                            "show_indices": {"type": "boolean"},
                            "expand_nested": {"type": "boolean"},
                        },
                        "additionalProperties": false,
                        "description": "Matrix display of the result",
                    }),
                ),
            ],
            &["document_id", "region_id"],
        ),
        annotations: Annotations {
            title: "Format result",
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        handler: format_result,
    });
    t.add(Tool {
        name: "format_text",
        description: "Formats a text region: bold, italic, underline, size (1 to 500 points), colour ([r, g, b] or with alpha) and paragraph alignment (left, center, right, justify), for the characters `range` {start, end} (0-based, end exclusive, in characters) or the whole text. Overwrites the formatting of that range. One undo step. Font family and hyperlinks are not supported yet.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Text region to format")),
                (
                    "range",
                    json!({
                        "type": "object",
                        "properties": {
                            "start": {"type": "integer", "minimum": 0},
                            "end": {"type": "integer", "minimum": 1},
                        },
                        "required": ["start", "end"],
                        "additionalProperties": false,
                        "description": "Characters to format; start below end; default the whole text",
                    }),
                ),
                ("bold", schema::boolean("Bold on or off")),
                ("italic", schema::boolean("Italic on or off")),
                ("underline", schema::boolean("Underline on or off")),
                ("size", crate::tools::write::number_schema("Font size in points, 1 to 500")),
                (
                    "colour",
                    json!({
                        "type": "array",
                        "items": {"type": "integer", "minimum": 0, "maximum": 255},
                        "minItems": 3,
                        "maxItems": 4,
                        "description": "[red, green, blue] or [red, green, blue, alpha], each 0 to 255",
                    }),
                ),
                (
                    "align",
                    json!({"type": "string", "enum": ["left", "center", "right", "justify"], "description": "Alignment of the paragraphs in the range"}),
                ),
            ],
            &["document_id", "region_id"],
        ),
        annotations: Annotations {
            title: "Format text",
            read_only: false,
            destructive: true,
            idempotent: true,
        },
        handler: format_text,
    });
    t.add(Tool {
        name: "set_page_setup",
        description: "Sets the page: paper size (A4, Letter, Legal, A3, Tabloid, A5), orientation, margin preset (Standard 25 mm, Narrow 12.5 mm, Wide 50 mm, Legacy 30.48 mm) and grid size. Custom margin values are not supported yet. Not in the undo history. Returns the page setup in points.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("size", json!({"type": "string", "enum": PAPER_CODES})),
                ("orientation", json!({"type": "string", "enum": ["Portrait", "Landscape"]})),
                ("margins", json!({"type": "string", "enum": ["Standard", "Narrow", "Wide", "Legacy"]})),
                ("grid_size", json!({"type": "string", "enum": ["Standard", "Fine"]})),
            ],
            &["document_id"],
        ),
        annotations: Annotations {
            title: "Set page setup",
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        handler: set_page_setup,
    });
    t.add(Tool {
        name: "set_header_footer",
        description: "Sets the page header and/or footer as plain text (up to 500 characters, newlines allowed). Omit a field to keep it; an empty string clears it. Overwrites the old text. Not in the undo history. Returns both texts.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("header", json!({"type": ["string", "null"], "description": "Header text"})),
                ("footer", json!({"type": ["string", "null"], "description": "Footer text"})),
            ],
            &["document_id"],
        ),
        annotations: Annotations {
            title: "Set header and footer",
            read_only: false,
            destructive: true,
            idempotent: true,
        },
        handler: set_header_footer,
    });
}

/// A whole number argument in `T`'s range; out of range is `invalid_params`.
fn bounded<T: TryFrom<u64>>(args: &Args<'_>, key: &str) -> Result<Option<T>, ToolError> {
    match args.opt_u64(key)? {
        None => Ok(None),
        Some(v) => T::try_from(v)
            .map(Some)
            .map_err(|_| invalid(format!("`{key}` is out of range"))),
    }
}

fn notation_of(name: &str) -> Result<NumberFormat, ToolError> {
    Ok(match pick_name(&NOTATIONS, name, "notation")? {
        "general" => NumberFormat::General,
        "decimal" => NumberFormat::Decimal,
        "scientific" => NumberFormat::Scientific,
        "engineering" => NumberFormat::Engineering,
        _ => NumberFormat::Percent,
    })
}

fn notation_name(n: NumberFormat) -> &'static str {
    match n {
        NumberFormat::General => "general",
        NumberFormat::Decimal => "decimal",
        NumberFormat::Scientific => "scientific",
        NumberFormat::Engineering => "engineering",
        NumberFormat::Percent => "percent",
    }
}

fn radix_of(name: &str) -> Result<Radix, ToolError> {
    Ok(match pick_name(&RADIXES, name, "radix")? {
        "decimal" => Radix::Decimal,
        "binary" => Radix::Binary,
        "octal" => Radix::Octal,
        _ => Radix::Hexadecimal,
    })
}

fn radix_name(r: Radix) -> &'static str {
    match r {
        Radix::Decimal => "decimal",
        Radix::Binary => "binary",
        Radix::Octal => "octal",
        Radix::Hexadecimal => "hexadecimal",
    }
}

fn pick_name(options: &[&'static str], name: &str, what: &str) -> Result<&'static str, ToolError> {
    crate::tools::plots::pick(options, name, what)
}

fn format_json(f: &ResultFormat) -> Value {
    json!({
        "notation": notation_name(f.number),
        "precision": f.precision,
        "trailing_zeros": f.trailing_zeros,
        "exponential_threshold": f.exponential_threshold,
        "zero_threshold": f.zero_threshold,
        "complex_threshold": f.complex_threshold,
        "radix": radix_name(f.radix),
        "digit_grouping": f.digit_grouping,
        "imaginary_j": f.imaginary_j,
    })
}

/// The calculation with the value text of every region that has its own result format written in
/// that format (format_result). Only the value texts change; a region that follows the worksheet
/// keeps its value. The document is not changed.
pub(crate) fn display_calc(
    doc: &Doc,
    calc: &mathcraft_doc::Calculation,
) -> mathcraft_doc::Calculation {
    let mut out = calc.clone();
    let mut groups: Vec<(ResultFormat, Vec<u32>)> = Vec::new();
    for r in &calc.results {
        let Some(format) = doc
            .region(r.region_id)
            .and_then(|region| region.result_format.clone())
        else {
            continue;
        };
        match groups.iter_mut().find(|(f, _)| *f == format) {
            Some((_, ids)) => ids.push(r.region_id),
            None => groups.push((format, vec![r.region_id])),
        }
    }
    for (format, ids) in groups {
        let options = mathcraft_doc::CalcOptions {
            regions: Some(ids),
            ..mathcraft_doc::CalcOptions::default()
        };
        let shown = mathcraft_doc::calc::calculate_in_format(doc, calc.revision, &options, &format);
        for r in shown.results {
            if let Some(o) = out.results.iter_mut().find(|o| o.region_id == r.region_id) {
                o.value = r.value;
            }
        }
    }
    out
}

/// The display settings of region `id` as read_region reports them: the region's own format (or
/// the worksheet's when it follows it) and its matrix display. None for an unknown region.
pub(crate) fn display_json(doc: &Doc, id: u32) -> Option<Value> {
    let region = doc.region(id)?;
    let worksheet = doc.settings().result_format.clone().unwrap_or_default();
    let effective = region.result_format.clone().unwrap_or(worksheet);
    Some(json!({
        "follows_worksheet": region.result_format.is_none(),
        "result_format": format_json(&effective),
        "matrix": matrix_json(&region.matrix),
    }))
}

fn matrix_json(m: &MatrixDisplay) -> Value {
    json!({
        "rows": m.rows,
        "cols": m.cols,
        "offset_row": m.offset_row,
        "offset_col": m.offset_col,
        "show_indices": m.show_indices,
        "expand_nested": m.expand_nested,
    })
}

/// The format with the given fields changed. None when no format field is given.
fn format_changed(args: &Args<'_>, base: &ResultFormat) -> Result<Option<ResultFormat>, ToolError> {
    if !FORMAT_KEYS
        .iter()
        .any(|k| args.0.get(*k).is_some_and(|v| !v.is_null()))
    {
        return Ok(None);
    }
    let mut f = base.clone();
    if let Some(p) = bounded::<u8>(args, "precision")? {
        f.precision = p;
    }
    if let Some(n) = args.opt_str("notation")? {
        f.number = notation_of(n)?;
    }
    if let Some(b) = flag(args, "trailing_zeros")? {
        f.trailing_zeros = b;
    }
    if let Some(e) = bounded::<u8>(args, "exponential_threshold")? {
        f.exponential_threshold = e;
    }
    if let Some(z) = bounded::<u16>(args, "zero_threshold")? {
        f.zero_threshold = z;
    }
    if let Some(c) = bounded::<u16>(args, "complex_threshold")? {
        f.complex_threshold = c;
    }
    if let Some(r) = args.opt_str("radix")? {
        f.radix = radix_of(r)?;
    }
    if let Some(g) = flag(args, "digit_grouping")? {
        f.digit_grouping = g;
    }
    if let Some(j) = flag(args, "imaginary_j")? {
        f.imaginary_j = j;
    }
    f.checked().map(Some).map_err(|e| invalid(e.to_string()))
}

/// The matrix display with the given fields changed. None when `matrix` is not given.
fn matrix_changed(
    args: &Args<'_>,
    base: MatrixDisplay,
) -> Result<Option<MatrixDisplay>, ToolError> {
    let Some(v) = args.0.get("matrix").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let map = v
        .as_object()
        .ok_or_else(|| invalid("`matrix` must be an object"))?;
    let a = Args(map);
    a.only(&MATRIX_KEYS)?;
    let mut m = base;
    if let Some(r) = bounded::<u32>(&a, "rows")? {
        if !(1..=MAX_MATRIX_VISIBLE).contains(&r) {
            return Err(invalid(format!("`rows` must be 1 to {MAX_MATRIX_VISIBLE}")));
        }
        m.rows = r;
    }
    if let Some(c) = bounded::<u32>(&a, "cols")? {
        if !(1..=MAX_MATRIX_VISIBLE).contains(&c) {
            return Err(invalid(format!("`cols` must be 1 to {MAX_MATRIX_VISIBLE}")));
        }
        m.cols = c;
    }
    if let Some(r) = bounded::<u32>(&a, "offset_row")? {
        if r > MAX_MATRIX_OFFSET {
            return Err(invalid(format!(
                "`offset_row` must be at most {MAX_MATRIX_OFFSET}"
            )));
        }
        m.offset_row = r;
    }
    if let Some(c) = bounded::<u32>(&a, "offset_col")? {
        if c > MAX_MATRIX_OFFSET {
            return Err(invalid(format!(
                "`offset_col` must be at most {MAX_MATRIX_OFFSET}"
            )));
        }
        m.offset_col = c;
    }
    if let Some(b) = flag(&a, "show_indices")? {
        m.show_indices = b;
    }
    if let Some(b) = flag(&a, "expand_nested")? {
        m.expand_nested = b;
    }
    Ok(Some(m))
}

fn format_result(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[
        "document_id",
        "region_id",
        "matrix",
        "reset",
        "precision",
        "notation",
        "trailing_zeros",
        "exponential_threshold",
        "zero_threshold",
        "complex_threshold",
        "radix",
        "digit_grouping",
        "imaginary_j",
    ])?;
    let region_id = args.region_id("region_id")?;
    let reset = flag(args, "reset")?.unwrap_or(false);
    let has_format = FORMAT_KEYS
        .iter()
        .any(|k| args.0.get(*k).is_some_and(|v| !v.is_null()));
    let has_matrix = args.0.get("matrix").is_some_and(|v| !v.is_null());
    if reset && has_format {
        return Err(invalid("`reset` takes no other format field"));
    }
    if !reset && !has_format && !has_matrix {
        return Err(invalid(
            "nothing to change: give a format field, matrix or reset",
        ));
    }
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let region = doc
            .region(region_id)
            .ok_or_else(|| crate::error::no_region(region_id))?;
        if !matches!(region.kind, RegionKind::Math(_)) {
            return Err(invalid(format!(
                "format_result works on math regions; region {region_id} is {}",
                kind_name(&region.kind)
            )));
        }
        let current_format = region.result_format.clone();
        let current_matrix = region.matrix;
        let worksheet = doc.settings().result_format.clone().unwrap_or_default();
        let base = current_format.clone().unwrap_or_else(|| worksheet.clone());
        let new_format = if reset {
            None
        } else {
            match format_changed(args, &base)? {
                Some(f) => Some(f),
                None => current_format,
            }
        };
        let new_matrix = matrix_changed(args, current_matrix)?.unwrap_or(current_matrix);
        let effective = new_format.clone().unwrap_or(worksheet);
        let before = error_keys(doc);
        doc.set_region_format(region_id, new_format);
        doc.set_region_matrix(region_id, new_matrix);
        let fb = feedback(doc, &before, &[region_id], &[]);
        Ok(merge(
            json!({
                "ok": true,
                "region_id": region_id,
                "format": format_json(&effective),
                "follows_worksheet": reset,
                "matrix": matrix_json(&new_matrix),
                "undoable": false,
                "note": "The stored display is read back by read_region (display). The calculated value text follows the worksheet format; the region format is what the Mathcraft window shows.",
            }),
            fb,
        ))
    })?;
    Ok(ToolOutput::json(v))
}

fn align_of(name: &str) -> Result<Align, ToolError> {
    Ok(
        match pick_name(&["left", "center", "right", "justify"], name, "alignment")? {
            "left" => Align::Left,
            "center" => Align::Center,
            "right" => Align::Right,
            _ => Align::Justify,
        },
    )
}

fn colour_of(v: &Value) -> Result<[u8; 4], ToolError> {
    let items = v
        .as_array()
        .filter(|a| a.len() == 3 || a.len() == 4)
        .ok_or_else(|| invalid("`colour` is [r, g, b] or [r, g, b, a], each 0 to 255"))?;
    let mut out = [0u8, 0, 0, 255];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = item
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| invalid("`colour` is [r, g, b] or [r, g, b, a], each 0 to 255"))?;
    }
    Ok(out)
}

/// `range` {start, end}: start below end; the bounds against the text are checked later.
fn span_of(v: &Value) -> Result<(usize, usize), ToolError> {
    let map = v
        .as_object()
        .ok_or_else(|| invalid("`range` must be an object with start and end"))?;
    let a = Args(map);
    a.only(&["start", "end"])?;
    let start = bounded::<usize>(&a, "start")?.ok_or_else(|| invalid("`range` needs start"))?;
    let end = bounded::<usize>(&a, "end")?.ok_or_else(|| invalid("`range` needs end"))?;
    if start >= end {
        return Err(invalid(
            "`range` is empty or reversed: start must be below end",
        ));
    }
    Ok((start, end))
}

fn format_text(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[
        "document_id",
        "region_id",
        "range",
        "bold",
        "italic",
        "underline",
        "size",
        "colour",
        "align",
    ])?;
    let region_id = args.region_id("region_id")?;
    let mut changes = Vec::new();
    if let Some(b) = flag(args, "bold")? {
        changes.push(TextChange::Bold(b));
    }
    if let Some(i) = flag(args, "italic")? {
        changes.push(TextChange::Italic(i));
    }
    if let Some(u) = flag(args, "underline")? {
        changes.push(TextChange::Underline(u));
    }
    if let Some(s) = number(args, "size")? {
        if !(1.0..=500.0).contains(&s) {
            return Err(invalid("`size` must be between 1 and 500 points"));
        }
        changes.push(TextChange::Size(s as f32));
    }
    if let Some(c) = args.0.get("colour").filter(|v| !v.is_null()) {
        changes.push(TextChange::Colour(colour_of(c)?));
    }
    let align = args.opt_str("align")?.map(align_of).transpose()?;
    if changes.is_empty() && align.is_none() {
        return Err(invalid("nothing to change: give a style or `align`"));
    }
    let span = args
        .0
        .get("range")
        .filter(|v| !v.is_null())
        .map(span_of)
        .transpose()?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let body = match &doc
            .region(region_id)
            .ok_or_else(|| crate::error::no_region(region_id))?
            .kind
        {
            RegionKind::Text(body) => body.clone(),
            other => {
                return Err(invalid(format!(
                    "format_text works on text regions; region {region_id} is {}",
                    kind_name(other)
                )))
            }
        };
        let len = body.as_str().chars().count();
        let (start, end) = match span {
            None => (0, len),
            Some((s, e)) => {
                if e > len {
                    return Err(invalid(format!(
                        "`range` ends at {e}, past the {len} characters of the text"
                    )));
                }
                (s, e)
            }
        };
        let mut rich = body.rich().clone();
        if start < end {
            for change in &changes {
                rich = rich.styled(start, end, change);
            }
        }
        if let Some(a) = align {
            rich = rich.aligned(start, end, a);
        }
        let before = error_keys(doc);
        let start = doc.steps_executed();
        doc.set_text_format(region_id, TextBody::from_rich(rich))
            .map_err(doc_err)?;
        let taken = doc.steps_executed() - start;
        if taken > 0 {
            doc.collapse_steps(taken, "AI: format_text");
        }
        let fb = feedback(doc, &before, &[region_id], &[]);
        let detail = doc.read_region(region_id).map_err(doc_err)?;
        Ok(merge(
            json!({"ok": true, "region_id": region_id, "text": detail_json(&detail)}),
            fb,
        ))
    })?;
    Ok(ToolOutput::json(v))
}

fn page_json(p: &PageSettings) -> Value {
    let [w, h] = p.paper_points();
    let m = p.margins();
    json!({
        "paper": p.paper_code,
        "orientation": p.orientation,
        "margin_type": p.margin_type,
        "grid_size": p.grid_size,
        "paper_pt": {"width": w, "height": h},
        "margins_pt": {"top": m.top, "right": m.right, "bottom": m.bottom, "left": m.left},
    })
}

fn set_page_setup(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "size", "orientation", "margins", "grid_size"])?;
    let size = args
        .opt_str("size")?
        .map(|s| pick_name(&PAPER_CODES, s, "paper size"))
        .transpose()?;
    let orientation = args
        .opt_str("orientation")?
        .map(|s| pick_name(&["Portrait", "Landscape"], s, "orientation"))
        .transpose()?;
    let margins = args
        .opt_str("margins")?
        .map(|s| {
            pick_name(
                &["Standard", "Narrow", "Wide", "Legacy"],
                s,
                "margin preset",
            )
        })
        .transpose()?;
    let grid_size = args
        .opt_str("grid_size")?
        .map(|s| pick_name(&["Standard", "Fine"], s, "grid size"))
        .transpose()?;
    if size.is_none() && orientation.is_none() && margins.is_none() && grid_size.is_none() {
        return Err(invalid(
            "nothing to change: give size, orientation, margins or grid_size",
        ));
    }
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc: &mut Doc| {
        let mut page = doc.page().clone();
        let mut changed = false;
        if let Some(s) = size {
            changed |= std::mem::replace(&mut page.paper_code, s.to_owned()) != s;
        }
        if let Some(o) = orientation {
            changed |= std::mem::replace(&mut page.orientation, o.to_owned()) != o;
        }
        if let Some(m) = margins {
            changed |= std::mem::replace(&mut page.margin_type, m.to_owned()) != m;
        }
        if let Some(g) = grid_size {
            changed |= std::mem::replace(&mut page.grid_size, g.to_owned()) != g;
        }
        if changed {
            doc.set_page(page.clone());
        }
        Ok(json!({"ok": true, "changed": changed, "undoable": false, "page_setup": page_json(&page)}))
    })?;
    Ok(ToolOutput::json(v))
}

/// A header or footer argument: None when absent or null (keep), otherwise checked plain text.
fn furniture_arg(args: &Args<'_>, key: &str) -> Result<Option<String>, ToolError> {
    let Some(text) = args.opt_str(key)? else {
        return Ok(None);
    };
    if text.chars().count() > MAX_FURNITURE_CHARS {
        return Err(ToolError::LimitReached(format!(
            "`{key}` is over {MAX_FURNITURE_CHARS} characters"
        )));
    }
    if text.chars().any(|c| c.is_control() && c != '\n') {
        return Err(invalid(format!("`{key}` has a control character")));
    }
    Ok(Some(text.to_owned()))
}

fn set_header_footer(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "header", "footer"])?;
    let header = furniture_arg(args, "header")?;
    let footer = furniture_arg(args, "footer")?;
    if header.is_none() && footer.is_none() {
        return Err(invalid("nothing to change: give `header` or `footer`"));
    }
    let v = ctx
        .backend
        .with_doc(args.str("document_id")?, &mut |doc: &mut Doc| {
            if let Some(h) = &header {
                doc.set_furniture(false, TextBody::from_plain(h));
            }
            if let Some(f) = &footer {
                doc.set_furniture(true, TextBody::from_plain(f));
            }
            let h = doc.furniture_text(false);
            let f = doc.furniture_text(true);
            Ok(json!({
                "ok": true,
                "undoable": false,
                "header": h.as_str(),
                "footer": f.as_str(),
            }))
        })?;
    Ok(ToolOutput::json(v))
}

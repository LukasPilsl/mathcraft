//! Read tools: outline, region, worksheet text, variables, errors. None of them changes the
//! document; results come from the engine's calculation, which is cached per revision.

use mathcraft_doc::CalcOptions;
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::format::display_calc;
use crate::tools::view::{
    detail_json, paged, paging_properties, problem_json, summary_json, variable_json,
};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

pub fn register(t: &mut ToolTable) {
    let mut outline_props = vec![
        schema::document_id(),
        (
            "page",
            schema::integer("Only regions whose top edge is on this page (1-based)"),
        ),
    ];
    outline_props.extend(paging_properties());
    t.add(Tool {
        name: "read_outline",
        description: "Ordered list of the regions of a document: region_id, kind, position, size and a one-line summary. Paged.".into(),
        input_schema: schema::object(&outline_props, &["document_id"]),
        annotations: Annotations::read_only("Read outline"),
        handler: read_outline,
    });
    t.add(Tool {
        name: "read_region",
        description: "Full content of one region: math in Mathcraft text notation, text runs with styles, plot spec, and the current result (value, error). `display` holds the region's result format; when it has its own, `display.result_text` and `result.value` are the value in that format.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region id from read_outline")),
            ],
            &["document_id", "region_id"],
        ),
        annotations: Annotations::read_only("Read region"),
        handler: read_region,
    });
    t.add(Tool {
        name: "read_worksheet_text",
        description: "The whole worksheet as a compact plain-text transcript, top to bottom with results (each result in its region's own format when format_result set one): the cheapest way to understand a sheet. Long sheets come in chunks; pass next_cursor to continue.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("cursor", schema::string("`next_cursor` of the previous chunk; omit for the start")),
                ("max_chars", schema::integer("Chunk size in characters (server cap applies)")),
            ],
            &["document_id"],
        ),
        annotations: Annotations::read_only("Read worksheet text"),
        handler: read_worksheet_text,
    });
    let mut paged_props = vec![schema::document_id()];
    paged_props.extend(paging_properties());
    t.add(Tool {
        name: "read_variables",
        description: "Names the worksheet defines: kind (variable or function), defining region, definition and current value. Paged.".into(),
        input_schema: schema::object(&paged_props, &["document_id"]),
        annotations: Annotations::read_only("Read variables"),
        handler: read_variables,
    });
    t.add(Tool {
        name: "read_errors",
        description:
            "All current errors, skipped regions and notes with region id and message. Paged."
                .into(),
        input_schema: schema::object(&paged_props, &["document_id"]),
        annotations: Annotations::read_only("Read errors"),
        handler: read_errors,
    });
}

fn read_outline(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "page", "cursor", "limit"])?;
    let wanted_page = args.opt_u64("page")?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let page = doc.page();
        let margins = page.margins();
        let printable = (page.paper_points()[1] - margins.top - margins.bottom).max(1.0);
        let items: Vec<Value> = doc
            .outline()
            .iter()
            .filter(|r| match wanted_page {
                None => true,
                Some(p) => {
                    let top = r.pos.map_or(0.0, |(_, y)| y);
                    ((top / printable).floor().max(0.0) as u64) + 1 == p
                }
            })
            .map(summary_json)
            .collect();
        paged(args, ctx.limits.page_items, "regions", &items)
    })?;
    Ok(ToolOutput::json(v))
}

fn read_region(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "region_id"])?;
    let id = args.region_id("region_id")?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        if doc.current_calculation().is_none() {
            doc.calculate(&CalcOptions::default());
        }
        let mut detail = detail_json(&doc.read_region(id)?);
        if let Some(mut display) = crate::tools::format::display_json(doc, id) {
            // A region with its own format shows its value in that format (display.result_text).
            if display["follows_worksheet"] == json!(false) {
                if let Some(calc) = doc.current_calculation() {
                    let shown = display_calc(doc, calc);
                    if let Some(text) = shown.result(id).map(|r| r.value.clone()) {
                        if detail["result"].is_object() {
                            detail["result"]["value"] = json!(text);
                        }
                        display["result_text"] = json!(text);
                    }
                }
            }
            detail["display"] = display;
        }
        Ok(detail)
    })?;
    Ok(ToolOutput::json(v))
}

fn read_worksheet_text(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "cursor", "max_chars"])?;
    let chunk = match args.opt_u64("max_chars")? {
        None => ctx.limits.chunk_chars,
        Some(n) => usize::try_from(n)
            .unwrap_or(ctx.limits.chunk_chars)
            .clamp(1, ctx.limits.chunk_chars),
    };
    let cursor = args.opt_str("cursor")?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        // Results in each region's own result format, as read_region shows them.
        let raw = match doc.current_calculation() {
            Some(calc) => calc.clone(),
            None => doc.calculate(&CalcOptions::default()),
        };
        let shown = display_calc(doc, &raw);
        let text = mathcraft_doc::read::transcript(doc, Some(&shown));
        let revision = doc.revision();
        let start = match cursor {
            None => 0,
            Some(c) => parse_cursor(c, revision)?,
        };
        let (slice, end, total) = chunk_of(&text, start, chunk);
        Ok(json!({
            "text": slice,
            "total_chars": total,
            "truncated": end < total,
            "next_cursor": (end < total).then(|| format!("{revision}:{end}")),
        }))
    })?;
    Ok(ToolOutput::json(v))
}

/// `revision:offset`. A cursor from an older revision is refused: the text moved under it.
fn parse_cursor(cursor: &str, revision: u64) -> Result<usize, ToolError> {
    let bad = || ToolError::InvalidParams("`cursor` is not a cursor from this tool".into());
    let (rev, offset) = cursor.split_once(':').ok_or_else(bad)?;
    let rev: u64 = rev.parse().map_err(|_| bad())?;
    let offset: usize = offset.parse().map_err(|_| bad())?;
    if rev != revision {
        return Err(ToolError::InvalidParams(
            "the document changed since this cursor was issued; start again without a cursor"
                .into(),
        ));
    }
    Ok(offset)
}

/// Up to `max` characters of `text` from character offset `start`, cut after a line end when one
/// falls in the second half of the chunk. Returns the slice, the end offset and the total length.
fn chunk_of(text: &str, start: usize, max: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let start = start.min(total);
    let mut end = start.saturating_add(max).min(total);
    if end < total {
        let half = start + max / 2;
        if let Some(nl) = chars[half.min(end)..end].iter().rposition(|c| *c == '\n') {
            end = half.min(end) + nl + 1;
        }
    }
    (chars[start..end].iter().collect(), end, total)
}

fn read_variables(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "cursor", "limit"])?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let items: Vec<Value> = doc
            .variables(&CalcOptions::default())
            .iter()
            .map(variable_json)
            .collect();
        paged(args, ctx.limits.page_items, "variables", &items)
    })?;
    Ok(ToolOutput::json(v))
}

fn read_errors(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "cursor", "limit"])?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let items: Vec<Value> = doc.errors().iter().map(problem_json).collect();
        let mut out = paged(args, ctx.limits.page_items, "problems", &items)?;
        out["ok"] = json!(items.iter().all(|p| p["severity"] != "error"));
        Ok(out)
    })?;
    Ok(ToolOutput::json(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::view::page_of;

    #[test]
    fn chunks_cover_the_text_without_gaps() {
        let text = "line one\nline two\nline three\nline four\n";
        let mut start = 0;
        let mut joined = String::new();
        loop {
            let (slice, end, total) = chunk_of(text, start, 12);
            joined.push_str(&slice);
            if end >= total {
                break;
            }
            assert!(end > start);
            start = end;
        }
        assert_eq!(joined, text);
    }

    #[test]
    fn cursor_of_another_revision_is_refused() {
        assert_eq!(parse_cursor("3:10", 3), Ok(10));
        assert!(parse_cursor("2:10", 3).is_err());
        assert!(parse_cursor("garbage", 3).is_err());
    }

    #[test]
    fn page_cursor_parses() {
        let map = serde_json::Map::new();
        let page = page_of(&Args(&map), 5, 2).unwrap();
        assert_eq!((page.start, page.end), (0, 2));
        assert_eq!(page.next_cursor.as_deref(), Some("2"));
    }
}

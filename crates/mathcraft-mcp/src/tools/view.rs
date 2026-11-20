//! JSON views of the document layer's types, shared by every tool group that returns regions,
//! results or problems, and the paging helper behind every list tool.

use mathcraft_doc::plot::PlotSpec;
use mathcraft_doc::read::{TextLine, TextRun};
use mathcraft_doc::{
    Problem, RegionContent, RegionDetail, RegionResult, RegionSummary, ResultStatus, Variable,
    VariableKind,
};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::Args;

/// A region id is `region_id` in every response.
pub fn summary_json(s: &RegionSummary) -> Value {
    json!({
        "region_id": s.id,
        "kind": s.kind,
        "position": s.pos.map(|(x, y)| json!({"x": x, "y": y})),
        "size": s.size.map(|(w, h)| json!({"width": w, "height": h})),
        "summary": s.summary,
    })
}

pub fn status_name(status: ResultStatus) -> &'static str {
    match status {
        ResultStatus::Value => "value",
        ResultStatus::Defined => "defined",
        ResultStatus::Plot => "plot",
        ResultStatus::Error => "error",
        ResultStatus::Skipped => "skipped",
    }
}

pub fn result_json(r: &RegionResult) -> Value {
    json!({
        "status": status_name(r.status),
        "expression": r.expr,
        "value": r.value,
        "error": r.error,
        "defines": r.defines,
    })
}

fn run_json(r: &TextRun) -> Value {
    json!({
        "text": r.text,
        "font": r.family,
        "size": r.size,
        "bold": r.bold,
        "italic": r.italic,
        "underline": r.underline,
        "colour_rgba": r.colour,
        "link": r.link,
    })
}

fn line_json(l: &TextLine) -> Value {
    json!({"align": l.align, "runs": l.runs.iter().map(run_json).collect::<Vec<_>>()})
}

fn plot_spec_json(p: &PlotSpec) -> Value {
    json!({
        "x": p.x, "y": p.y,
        "x_min": p.x_min, "x_max": p.x_max,
        "y_min": p.y_min, "y_max": p.y_max,
        "grid": p.grid, "title": p.title, "polar": p.polar,
    })
}

fn content_json(c: &RegionContent) -> Value {
    match c {
        RegionContent::Math { text } => json!({"type": "math", "text": text}),
        RegionContent::Text { text, lines } => json!({
            "type": "text",
            "text": text,
            "lines": lines.iter().map(line_json).collect::<Vec<_>>(),
        }),
        RegionContent::Plot { kind, spec } => json!({
            "type": "plot",
            "plot_kind": kind,
            "spec": spec.as_ref().map(plot_spec_json),
        }),
        RegionContent::Image { media_type, bytes } => {
            json!({"type": "image", "media_type": media_type, "bytes": bytes})
        }
        RegionContent::Area { locked, regions } => {
            json!({"type": "area", "locked": locked, "regions": regions})
        }
        RegionContent::SolveBlock { members } => json!({"type": "solve_block", "members": members}),
        RegionContent::Table => json!({"type": "table"}),
        RegionContent::PageBreak => json!({"type": "page_break"}),
        RegionContent::Unsupported { reason } => json!({"type": "unsupported", "reason": reason}),
    }
}

pub fn detail_json(d: &RegionDetail) -> Value {
    json!({
        "region_id": d.id,
        "kind": d.kind,
        "position": d.pos.map(|(x, y)| json!({"x": x, "y": y})),
        "size": d.size.map(|(w, h)| json!({"width": w, "height": h})),
        "content": content_json(&d.content),
        "result": d.result.as_ref().map(result_json),
    })
}

pub fn variable_json(v: &Variable) -> Value {
    json!({
        "name": v.name,
        "kind": match v.kind {
            VariableKind::Variable => "variable",
            VariableKind::Function => "function",
        },
        "region_id": v.region_id,
        "definition": v.definition,
        "value": v.value,
    })
}

pub fn problem_json(p: &Problem) -> Value {
    json!({"region_id": p.region_id, "severity": p.severity, "message": p.message})
}

/// One page of a list: which items, and the cursor of the next page.
pub struct PageOf {
    pub start: usize,
    pub end: usize,
    pub next_cursor: Option<String>,
}

/// Reads `cursor` and `limit` from the arguments and cuts `total` items into a page. The cursor
/// is the offset of the first item as text; `limit` is capped at `max_items`.
pub fn page_of(args: &Args<'_>, total: usize, max_items: usize) -> Result<PageOf, ToolError> {
    let start = match args.opt_str("cursor")? {
        None => 0,
        Some(c) => c.parse::<usize>().map_err(|_| {
            ToolError::InvalidParams("`cursor` is not a cursor from this tool".into())
        })?,
    };
    let limit = match args.opt_u64("limit")? {
        None => max_items,
        Some(l) => usize::try_from(l).unwrap_or(max_items).clamp(1, max_items),
    };
    let start = start.min(total);
    let end = start.saturating_add(limit).min(total);
    Ok(PageOf {
        start,
        end,
        next_cursor: (end < total).then(|| end.to_string()),
    })
}

/// The schema properties every paged list tool takes.
pub fn paging_properties() -> [(&'static str, Value); 2] {
    [
        (
            "cursor",
            crate::schema::string("`next_cursor` of the previous page; omit for the first page"),
        ),
        (
            "limit",
            crate::schema::integer("Most items to return (server cap applies)"),
        ),
    ]
}

/// A paged JSON list: `{ "<key>": [...], "total": n, "next_cursor": ... }`.
pub fn paged(
    args: &Args<'_>,
    max_items: usize,
    key: &str,
    items: &[Value],
) -> Result<Value, ToolError> {
    let page = page_of(args, items.len(), max_items)?;
    Ok(json!({
        key: &items[page.start..page.end],
        "total": items.len(),
        "next_cursor": page.next_cursor,
    }))
}

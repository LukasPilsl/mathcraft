//! `app_*` tools: only meaningful when the running Mathcraft window hosts the server (live mode).
//! Without a window they answer `live_unavailable`, so a client can tell "no app" from a failure.
//!
//! * `app_status`: the connection, the documents in the window and the one in front.
//! * `app_focus_document`: brings a document's tab to the front.
//! * `app_select_region`: selects a region in the window, so the user sees what the AI means.
//! * `app_screenshot`: the page of a live document as the window's renderer draws it.

use serde_json::{json, Map, Value};

use crate::backend::LiveControl;
use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::render::render_page;
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

pub fn register(t: &mut ToolTable) {
    t.add(Tool {
        name: "app_status",
        description: "Live mode only: whether the Mathcraft window is connected, whether the user paused the AI, the documents open in the window (with `busy` while the user is typing in one) and the document in front. Answers live_unavailable when no app is running.".into(),
        input_schema: schema::empty(),
        annotations: Annotations::read_only("App status"),
        handler: app_status,
    });
    t.add(Tool {
        name: "app_focus_document",
        description: "Live mode only: brings the tab of a document to the front of the Mathcraft window so the user sees it.".into(),
        input_schema: schema::object(&[schema::document_id()], &["document_id"]),
        annotations: Annotations {
            title: "Focus document",
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        handler: app_focus_document,
    });
    t.add(Tool {
        name: "app_select_region",
        description: "Live mode only: selects a region in the Mathcraft window (and brings its tab to the front), to show the user what you mean. Ids come from read_outline.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("region_id", schema::integer("Region to select")),
            ],
            &["document_id", "region_id"],
        ),
        annotations: Annotations {
            title: "Select region",
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        handler: app_select_region,
    });
    t.add(Tool {
        name: "app_screenshot",
        description: "Live mode only: one page of a live document as a PNG, drawn by the same renderer as the window (paper, margins, header, footer, regions). Without document_id it takes the document in front. Same size caps as render_page. This is the page, not a capture of the operating system window.".into(),
        input_schema: schema::object(
            &[
                ("document_id", schema::string("Live document id; default: the document in front")),
                ("page", schema::integer("Page number, counted from 1 (default 1)")),
                ("dpi", json!({"type": "number", "description": "Resolution in dpi (default 96). Not with width_px."})),
                ("width_px", schema::integer("Image width in pixels instead of dpi")),
                ("show_grid", schema::boolean("Draw the grid (default false)")),
                ("max_side", schema::integer("Longest side in pixels (default 1600, at most 4096)")),
            ],
            &[],
        ),
        annotations: Annotations::read_only("App screenshot"),
        handler: app_screenshot,
    });
}

fn live<'a>(ctx: &'a ToolCtx<'_>) -> Result<&'a dyn LiveControl, ToolError> {
    ctx.backend.live().ok_or_else(|| {
        ToolError::LiveUnavailable(
            "no running Mathcraft window is connected; start the app (live mode), or use the headless tools".into(),
        )
    })
}

fn app_status(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[])?;
    Ok(ToolOutput::json(live(ctx)?.status()))
}

fn app_focus_document(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id"])?;
    let id = args.str("document_id")?;
    live(ctx)?.focus(id)?;
    Ok(ToolOutput::json(json!({"focused": id})))
}

fn app_select_region(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "region_id"])?;
    let id = args.str("document_id")?;
    let region = args.region_id("region_id")?;
    live(ctx)?.select_region(id, region)?;
    Ok(ToolOutput::json(
        json!({"selected": region, "document_id": id}),
    ))
}

fn app_screenshot(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[
        "document_id",
        "page",
        "dpi",
        "width_px",
        "show_grid",
        "max_side",
    ])?;
    let control = live(ctx)?;
    let mut map: Map<String, Value> = args.0.clone();
    if args.opt_str("document_id")?.is_none() {
        let front = control
            .status()
            .get("front_document")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ToolError::NotFound("no document is open in the window".into()))?;
        map.insert("document_id".into(), Value::String(front));
    }
    render_page(ctx, &Args(&map))
}

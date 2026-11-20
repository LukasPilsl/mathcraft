//! Document tools: capabilities, new, open, save, close, list, info, undo, redo.

use std::collections::BTreeMap;

use mathcraft_doc::Doc;
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

pub fn register(t: &mut ToolTable) {
    t.add(Tool {
        name: "capabilities",
        description: "Server and engine version, mode (headless or live), limits, allowed file roots and which tools change documents. Call first."
            .into(),
        input_schema: schema::empty(),
        annotations: Annotations::read_only("Capabilities"),
        handler: capabilities,
    });
    t.add(Tool {
        name: "document_new",
        description: "Creates an empty worksheet in memory and returns its document_id.".into(),
        input_schema: schema::object(
            &[
                ("title", schema::string("Title of the new worksheet")),
                target_schema(),
            ],
            &[],
        ),
        annotations: Annotations {
            title: "New document",
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        handler: document_new,
    });
    t.add(Tool {
        name: "document_open",
        description: "Opens a .mcdx worksheet from an allowed root and returns its document_id. Nothing is calculated until a tool needs results.".into(),
        input_schema: schema::object(
            &[
                ("path", schema::string("Path of the .mcdx file; relative paths start at the first root")),
                target_schema(),
            ],
            &["path"],
        ),
        annotations: Annotations {
            title: "Open document",
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        handler: document_open,
    });
    t.add(Tool {
        name: "document_save",
        description: "Saves a document as .mcdx inside an allowed root. Replacing an existing file needs overwrite: true; the first overwrite of a file in a session keeps a .bak copy.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("path", schema::string("Target path; omit to save to the file the document was opened from")),
                ("overwrite", schema::boolean("Allow replacing an existing file")),
            ],
            &["document_id"],
        ),
        annotations: Annotations {
            title: "Save document",
            read_only: false,
            destructive: true,
            idempotent: true,
        },
        handler: document_save,
    });
    t.add(Tool {
        name: "document_close",
        description: "Closes a document and frees its slot. A document with unsaved changes needs discard: true.".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("discard", schema::boolean("Close even if there are unsaved changes")),
            ],
            &["document_id"],
        ),
        annotations: Annotations {
            title: "Close document",
            read_only: false,
            destructive: true,
            idempotent: false,
        },
        handler: document_close,
    });
    t.add(Tool {
        name: "document_list",
        description: "Lists the open documents with id, title, path, dirty flag and region count."
            .into(),
        input_schema: schema::empty(),
        annotations: Annotations::read_only("List documents"),
        handler: document_list,
    });
    t.add(Tool {
        name: "document_info",
        description: "Page count, page setup, header and footer text (empty when none), unit system, region count, dirty flag and undo state of a document.".into(),
        input_schema: schema::object(&[schema::document_id()], &["document_id"]),
        annotations: Annotations::read_only("Document info"),
        handler: document_info,
    });
    t.add(Tool {
        name: "document_undo",
        description: "Undoes the last edit of the document (the undo step is labelled, for example `AI: insert_area`). Reports whether anything was undone, the label of the step and the ids of the regions it changed (added, removed or changed)."
            .into(),
        input_schema: schema::object(&[schema::document_id()], &["document_id"]),
        annotations: Annotations {
            title: "Undo",
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        handler: document_undo,
    });
    t.add(Tool {
        name: "document_redo",
        description:
            "Redoes the last undone edit of the document. Reports whether anything was redone and the ids of the regions it changed."
                .into(),
        input_schema: schema::object(&[schema::document_id()], &["document_id"]),
        annotations: Annotations {
            title: "Redo",
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        handler: document_redo,
    });
}

fn capabilities(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[])?;
    let tools: Vec<Value> = ctx
        .catalog
        .iter()
        .map(|m| json!({"name": m.name, "mutating": !m.read_only}))
        .collect();
    Ok(ToolOutput::json(json!({
        "server": "mathcraft-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": ctx.backend.mode(),
        "roots": ctx.backend.roots(),
        "limits": {
            "max_open_documents": ctx.backend.max_documents(),
            "max_response_bytes": ctx.limits.max_response_bytes,
            "transcript_chunk_chars": ctx.limits.chunk_chars,
            "list_page_items": ctx.limits.page_items,
            "max_regions": mathcraft_import_max_regions(),
        },
        "features": {
            "read": true,
            "write": true,
            "calculate": true,
            "render": true,
            "live": ctx.backend.mode() == "live",
        },
        "tools": tools,
    })))
}

/// The worksheet region cap of the document layer, as a number for `capabilities`.
fn mathcraft_import_max_regions() -> usize {
    mathcraft_import::regions::MAX_REGIONS
}

/// The `target` property of `document_new` and `document_open`.
fn target_schema() -> (&'static str, Value) {
    (
        "target",
        json!({
            "type": "string",
            "enum": ["live", "headless"],
            "description": "Where the document lives. Default: a file that is open in the running Mathcraft window is attached to (live); everything else is headless. \"live\" opens or creates it in the window (live_unavailable when no app runs), \"headless\" keeps it in the server, even if the file is open in the window."
        }),
    )
}

/// Checks `target` against the backend: `live` needs the live backend. The router in front of a
/// headless server has already sent `live` calls to the app; `headless` reaching the live backend
/// cannot happen through it and is refused.
fn check_target(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<(), ToolError> {
    match args.opt_str("target")? {
        None => Ok(()),
        Some("live") if ctx.backend.mode() != "live" => Err(ToolError::LiveUnavailable(
            "no running Mathcraft window is connected; start the app, or use target: \"headless\"".into(),
        )),
        Some("headless") if ctx.backend.mode() == "live" => Err(ToolError::InvalidParams(
            "this call reached the running app; headless documents are made by the standalone server".into(),
        )),
        Some("live" | "headless") => Ok(()),
        Some(other) => Err(ToolError::InvalidParams(format!(
            "`target` must be \"live\" or \"headless\", not \"{other}\""
        ))),
    }
}

fn document_new(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["title", "target"])?;
    check_target(ctx, args)?;
    let doc = ctx.backend.new_document(args.opt_str("title")?)?;
    Ok(ToolOutput::json(doc.to_json()))
}

fn document_open(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["path", "target"])?;
    check_target(ctx, args)?;
    let doc = ctx.backend.open(args.str("path")?).map_err(path_hint)?;
    Ok(ToolOutput::json(doc.to_json()))
}

/// A refused path says what to do first: use a folder given with `--root`. The reason follows.
fn path_hint(e: ToolError) -> ToolError {
    match e {
        ToolError::PathDenied(m) => ToolError::PathDenied(format!(
            "use a .mcdx path inside a folder given with --root (capabilities lists the folders); reason: {m}"
        )),
        other => other,
    }
}

fn document_save(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "path", "overwrite"])?;
    let out = ctx
        .backend
        .save(
            args.str("document_id")?,
            args.opt_str("path")?,
            args.opt_bool("overwrite")?,
        )
        .map_err(path_hint)?;
    Ok(ToolOutput::json(json!({
        "path": out.path,
        "overwrote": out.overwrote,
        "backup": out.backup,
        "warnings": out.warnings,
    })))
}

fn document_close(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "discard"])?;
    let id = args.str("document_id")?;
    ctx.backend.close(id, args.opt_bool("discard")?)?;
    Ok(ToolOutput::json(json!({"closed": id})))
}

fn document_list(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[])?;
    let docs: Vec<Value> = ctx.backend.list().iter().map(|d| d.to_json()).collect();
    Ok(ToolOutput::json(json!({"documents": docs})))
}

fn document_info(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id"])?;
    let id = args.str("document_id")?;
    let summary = ctx.backend.summary(id)?;
    let info = ctx.backend.with_doc(id, &mut |doc| {
        let page = doc.page();
        let [paper_w, paper_h] = page.paper_points();
        let margins = page.margins();
        let printable = (paper_h - margins.top - margins.bottom).max(1.0);
        // The page of a region is the one holding its top edge; the page count is the page of the
        // lowest bottom edge. Explicit page breaks are not applied here.
        let bottom = doc
            .outline()
            .iter()
            .filter_map(|r| r.pos.map(|(_, y)| y + r.size.map_or(0.0, |(_, h)| h)))
            .fold(0.0_f64, f64::max);
        let pages = ((bottom / printable).ceil() as u64).max(1);
        Ok(json!({
            "page_count": pages,
            "page_setup": {
                "paper": page.paper_code,
                "orientation": page.orientation,
                "margin_type": page.margin_type,
                "grid_size": page.grid_size,
                "paper_width_pt": paper_w,
                "paper_height_pt": paper_h,
                "margins_pt": {
                    "top": margins.top, "right": margins.right,
                    "bottom": margins.bottom, "left": margins.left,
                },
            },
            "header": doc.furniture_text(false).as_str(),
            "footer": doc.furniture_text(true).as_str(),
            "unit_system": doc.settings().unit_system.clone().unwrap_or_else(|| "si".into()),
            "can_undo": doc.can_undo(),
            "can_redo": doc.can_redo(),
            "undo_label": doc.undo_label(),
            "revision": doc.revision(),
        }))
    })?;
    let mut out = summary.to_json();
    if let (Some(o), Some(i)) = (out.as_object_mut(), info.as_object()) {
        o.extend(i.clone());
    }
    Ok(ToolOutput::json(out))
}

fn document_undo(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id"])?;
    history_step(ctx, args, true)
}

fn document_redo(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id"])?;
    history_step(ctx, args, false)
}

/// Most changed region ids one undo or redo reply lists.
const MAX_CHANGED: usize = 100;

fn history_step(ctx: &ToolCtx<'_>, args: &Args<'_>, undo: bool) -> Result<ToolOutput, ToolError> {
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let label = if undo {
            doc.undo_label()
        } else {
            doc.redo_label()
        };
        let before = region_states(doc);
        let done = if undo { doc.undo() } else { doc.redo() }
            .map_err(|e| ToolError::Doc(e.to_string()))?;
        let after = region_states(doc);
        let changed: Vec<u32> = changed_regions(&before, &after);
        Ok(json!({
            "done": done,
            "step": if done { label } else { None },
            "changed_regions": changed.iter().take(MAX_CHANGED).collect::<Vec<_>>(),
            "changed_count": changed.len(),
            "can_undo": doc.can_undo(),
            "can_redo": doc.can_redo(),
            "region_count": doc.regions().len(),
        }))
    })?;
    Ok(ToolOutput::json(v))
}

/// A text per top-level region that changes when the region does: kind, position, size and
/// summary. A region compared this way is changed when any of them differ.
fn region_states(doc: &Doc) -> BTreeMap<u32, String> {
    doc.outline()
        .into_iter()
        .map(|r| {
            (
                r.id,
                format!("{}|{:?}|{:?}|{}", r.kind, r.pos, r.size, r.summary),
            )
        })
        .collect()
}

/// The ids of the regions that were added, removed or changed between two states, in order.
fn changed_regions(before: &BTreeMap<u32, String>, after: &BTreeMap<u32, String>) -> Vec<u32> {
    let mut ids: Vec<u32> = before
        .iter()
        .filter(|(id, state)| after.get(id) != Some(state))
        .map(|(id, _)| *id)
        .collect();
    ids.extend(after.keys().filter(|id| !before.contains_key(id)).copied());
    ids.sort_unstable();
    ids
}

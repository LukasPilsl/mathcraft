//! Calculation tools: calculate, evaluate_expression, set_calculation_options. The engine's own
//! limits (work budget, 60 s deadline) apply to all of them.

use mathcraft_doc::{CalcOptions, Doc, ResultStatus};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::format::display_calc;
use crate::tools::view::{page_of, paging_properties};
use crate::tools::write::{limited_text, number_schema, result_view};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

/// Most region ids one `calculate` call may name.
const MAX_REGION_IDS: usize = 1000;

pub fn register(t: &mut ToolTable) {
    let mut props = vec![
        schema::document_id(),
        (
            "region_ids",
            json!({
                "type": "array",
                "items": {"type": "integer", "minimum": 0},
                "maxItems": MAX_REGION_IDS,
                "description": "Only report these regions (the whole sheet is still evaluated: a region needs the definitions above it). Omit for all.",
            }),
        ),
    ];
    props.extend(paging_properties());
    t.add(Tool {
        name: "calculate",
        description: "Calculates the worksheet and returns value or error per region, plus run notes and why a run stopped early (work budget, 60 s deadline). A region with its own result format (format_result) reports its value in that format, so `value` is what read_region and read_worksheet_text show. Paged.".into(),
        input_schema: schema::object(&props, &["document_id"]),
        annotations: Annotations::read_only("Calculate"),
        handler: calculate,
    });
    t.add(Tool {
        name: "evaluate_expression",
        description: "Evaluates one expression (linear text syntax) without adding a region. Returns `scratch: true` and no region_id: nothing is stored. With document_id it sees the names the sheet defines; without, it is a scratch calculation.".into(),
        input_schema: schema::object(
            &[
                (
                    "expression",
                    schema::string("Expression such as `sin(30 deg)` or `v*2`"),
                ),
                (
                    "document_id",
                    schema::string("Evaluate in the scope of this document"),
                ),
            ],
            &["expression"],
        ),
        annotations: Annotations::read_only("Evaluate expression"),
        handler: evaluate_expression,
    });
    t.add(Tool {
        name: "set_calculation_options",
        description: "Sets calculation options of the document: array_origin (0 or 1), unit_system (si, us or cgs), convergence_tolerance, constraint_tolerance, prn_precision. Not undoable. Changes results, so the sheet is recalculated and the new error count returned. Automatic/manual calculation mode is not supported (the headless server always calculates on demand).".into(),
        input_schema: schema::object(
            &[
                schema::document_id(),
                ("array_origin", json!({"type": "integer", "enum": [0, 1]})),
                ("unit_system", json!({"type": "string", "enum": ["si", "us", "cgs"]})),
                ("convergence_tolerance", number_schema("TOL, above 0")),
                ("constraint_tolerance", number_schema("CTOL, above 0")),
                ("prn_precision", json!({"type": "integer", "minimum": 1, "maximum": 17})),
            ],
            &["document_id"],
        ),
        annotations: Annotations {
            title: "Set calculation options",
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        handler: set_calculation_options,
    });
}

fn calculate(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "region_ids", "cursor", "limit"])?;
    let wanted: Option<Vec<u32>> = match args.0.get("region_ids") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => {
            if items.len() > MAX_REGION_IDS {
                return Err(ToolError::LimitReached(format!(
                    "{} region ids, over the limit of {MAX_REGION_IDS}",
                    items.len()
                )));
            }
            let mut ids = Vec::with_capacity(items.len());
            for v in items {
                let id = v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| {
                        ToolError::InvalidParams("`region_ids` must hold region ids".into())
                    })?;
                ids.push(id);
            }
            Some(ids)
        }
        Some(_) => {
            return Err(ToolError::InvalidParams(
                "`region_ids` must be an array".into(),
            ))
        }
    };
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        if let Some(ids) = &wanted {
            if let Some(missing) = ids.iter().find(|id| doc.region(**id).is_none()) {
                return Err(crate::error::no_region(*missing));
            }
        }
        // The full result is cached by the document; a subset is only a filter on it. A region
        // with its own result format reports its value in that format.
        let raw = doc.calculate(&CalcOptions::default());
        let calc = display_calc(doc, &raw);
        let rows: Vec<&mathcraft_doc::RegionResult> = calc
            .results
            .iter()
            .filter(|r| wanted.as_ref().is_none_or(|w| w.contains(&r.region_id)))
            .collect();
        let page = page_of(args, rows.len(), ctx.limits.page_items)?;
        let results: Vec<Value> = rows[page.start..page.end]
            .iter()
            .map(|r| result_view(r))
            .collect();
        let error_count = calc
            .results
            .iter()
            .filter(|r| r.status == ResultStatus::Error)
            .count();
        Ok(json!({
            "results": results,
            "total": rows.len(),
            "next_cursor": page.next_cursor,
            "error_count": error_count,
            "notes": calc.notes.iter().take(50).collect::<Vec<_>>(),
            "stopped": calc.stopped,
            "revision": calc.revision,
        }))
    })?;
    Ok(ToolOutput::json(v))
}

fn evaluate_expression(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["expression", "document_id"])?;
    let expression = limited_text(args, "expression")?;
    let eval = |doc: &Doc| -> Result<Value, ToolError> {
        let r = doc
            .evaluate_expression(expression, &CalcOptions::default())
            .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
        // Not a region: no region_id, only the flag that says so.
        let mut v = result_view(&r);
        if let Some(map) = v.as_object_mut() {
            map.remove("region_id");
            map.insert("scratch".into(), json!(true));
        }
        Ok(v)
    };
    let v = match args.opt_str("document_id")? {
        Some(id) => ctx.backend.with_doc(id, &mut |doc| eval(doc))?,
        None => eval(&Doc::new("scratch"))?,
    };
    Ok(ToolOutput::json(v))
}

fn positive(args: &Args<'_>, key: &str) -> Result<Option<f64>, ToolError> {
    match args.0.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() && n > 0.0 => Ok(Some(n)),
            _ => Err(ToolError::InvalidParams(format!(
                "`{key}` must be a number above 0"
            ))),
        },
    }
}

type SettingsEdit = Box<dyn Fn(&mut mathcraft_import::regions::Settings)>;

fn set_calculation_options(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[
        "document_id",
        "array_origin",
        "unit_system",
        "convergence_tolerance",
        "constraint_tolerance",
        "prn_precision",
    ])?;
    let mut settings_edit: Vec<SettingsEdit> = Vec::new();
    if let Some(n) = args.opt_u64("array_origin")? {
        if n > 1 {
            return Err(ToolError::InvalidParams("`array_origin` is 0 or 1".into()));
        }
        settings_edit.push(Box::new(move |s| s.array_origin = Some(n as u32)));
    }
    if let Some(u) = args.opt_str("unit_system")? {
        if !["si", "us", "cgs"].contains(&u) {
            return Err(ToolError::InvalidParams(
                "`unit_system` is si, us or cgs".into(),
            ));
        }
        let u = u.to_owned();
        settings_edit.push(Box::new(move |s| s.unit_system = Some(u.clone())));
    }
    if let Some(t) = positive(args, "convergence_tolerance")? {
        settings_edit.push(Box::new(move |s| s.convergence_tolerance = Some(t)));
    }
    if let Some(t) = positive(args, "constraint_tolerance")? {
        settings_edit.push(Box::new(move |s| s.constraint_tolerance = Some(t)));
    }
    if let Some(p) = args.opt_u64("prn_precision")? {
        if !(1..=17).contains(&p) {
            return Err(ToolError::InvalidParams(
                "`prn_precision` is between 1 and 17".into(),
            ));
        }
        settings_edit.push(Box::new(move |s| s.prn_precision = Some(p as u32)));
    }
    if settings_edit.is_empty() {
        return Err(ToolError::InvalidParams(
            "give at least one option to set".into(),
        ));
    }
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let mut settings = doc.settings().clone();
        for edit in &settings_edit {
            edit(&mut settings);
        }
        doc.set_settings(settings);
        let calc = doc.calculate(&CalcOptions::default());
        let error_count = calc
            .results
            .iter()
            .filter(|r| r.status == ResultStatus::Error)
            .count();
        Ok(json!({"ok": true, "error_count": error_count, "stopped": calc.stopped}))
    })?;
    Ok(ToolOutput::json(v))
}

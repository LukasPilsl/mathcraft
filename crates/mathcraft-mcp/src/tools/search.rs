//! Search tools: `search_document` finds text in the worksheet and changes nothing;
//! `replace_in_document` replaces every match (one undo step). Both use the worksheet search of
//! `mathcraft_doc::search`, the same one the status bar's Suchen and Ersetzen fields use.
//!
//! Inputs are bounded: the query and the replacement are at most [`MAX_ARG_CHARS`] characters,
//! a search lists at most [`MAX_LISTED`] matches (the count is complete up to the search's own
//! cap), and one replace-all changes at most [`MAX_REGIONS`] regions.

use mathcraft_doc::search::{self, SearchOptions, MAX_MATCHES};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::schema::{self, Args};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

/// Most matches one search response lists. The count is always complete.
const MAX_LISTED: usize = 500;

/// Most characters of a query or a replacement.
pub const MAX_ARG_CHARS: usize = 1_000;

/// Most regions one replace-all changes.
pub const MAX_REGIONS: usize = 5_000;

/// Undo label of a replace-all step, the same way the other AI edits are labelled.
const REPLACE_LABEL: &str = "AI: replace_in_document";

pub fn register(t: &mut ToolTable) {
    let options = || {
        vec![
            (
                "match_case",
                schema::boolean("Upper and lower case differ (default false)"),
            ),
            (
                "whole_word",
                schema::boolean(
                    "A match must be a whole word: not touching a letter, digit or `_` (default TRUE here, unlike the GUI, so that x does not change max or 1e-3; set false for parts of words)",
                ),
            ),
        ]
    };
    let mut props = vec![
        schema::document_id(),
        (
            "query",
            schema::string(
                "Text to find (at most 1000 characters). Searched in math (linear text syntax), text and plot titles",
            ),
        ),
    ];
    props.extend(options());
    t.add(Tool {
        name: "search_document",
        description: "Finds the query in the worksheet, as the Suchen field of the status bar does. Changes nothing. Whole words only by default (whole_word=false for parts of words). Returns `count` (at most 1000000), `truncated` and the first 500 matches in document order: region_id, kind (math, text or plot_title), start and end (character offsets into the region's text), and the matched text. Matches do not overlap.".into(),
        input_schema: schema::object(&props, &["document_id", "query"]),
        annotations: Annotations::read_only("Search document"),
        handler: search_document,
    });
    let mut props = vec![
        schema::document_id(),
        (
            "query",
            schema::string("Text to find (at most 1000 characters)"),
        ),
        (
            "replacement",
            schema::string(
                "Text put in place of each match; may be empty (at most 1000 characters)",
            ),
        ),
    ];
    props.extend(options());
    t.add(Tool {
        name: "replace_in_document",
        description: "Replaces every match of the query by the replacement, as the Ersetzen durch fields do with Replace all. Whole words only by default (whole_word=false replaces inside names and numbers too, which can break formulas: x in max, e in 1e-3; a warning is returned when math was changed that way). Math is replaced only where the result still reads: a region whose result would not read is left as it was and listed in `refused`. At most 5000 regions per call (`truncated`). All replacements are one undo step (document_undo); any other failure leaves the worksheet unchanged. Returns `replaced`, `refused` and the matches that remain.".into(),
        input_schema: schema::object(&props, &["document_id", "query", "replacement"]),
        annotations: Annotations {
            title: "Replace in document",
            read_only: false,
            destructive: true,
            idempotent: false,
        },
        handler: replace_in_document,
    });
}

/// The search options of the arguments: any case, equations and text, whole words unless
/// `whole_word` says false.
fn options(args: &Args<'_>) -> Result<SearchOptions, ToolError> {
    let whole_word = match args.0.get("whole_word") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            return Err(ToolError::InvalidParams(
                "`whole_word` must be a boolean".into(),
            ))
        }
    };
    Ok(SearchOptions {
        match_case: args.opt_bool("match_case")?,
        whole_word,
        ..SearchOptions::default()
    })
}

/// A string argument of at most [`MAX_ARG_CHARS`] characters.
fn bounded(args: &Args<'_>, key: &str) -> Result<String, ToolError> {
    let s = args.str(key)?;
    // Bytes first, so that a huge string is not walked: a character is at most 4 bytes.
    if s.len() > 4 * MAX_ARG_CHARS || s.chars().count() > MAX_ARG_CHARS {
        return Err(ToolError::InvalidParams(format!(
            "`{key}` is longer than {MAX_ARG_CHARS} characters"
        )));
    }
    Ok(s.to_owned())
}

fn search_document(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["document_id", "query", "match_case", "whole_word"])?;
    let query = bounded(args, "query")?;
    let opts = options(args)?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let found = search::scan(doc.document(), &query, opts, MAX_MATCHES, MAX_LISTED);
        let listed: Vec<Value> = found
            .matches
            .iter()
            .zip(&found.texts)
            .map(|(m, text)| {
                json!({
                    "region_id": m.region_id,
                    "kind": m.kind.name(),
                    "start": m.range.start,
                    "end": m.range.end,
                    "text": text,
                })
            })
            .collect();
        Ok(json!({
            "count": found.matches.len(),
            "matches": listed,
            "truncated": found.truncated || found.matches.len() > MAX_LISTED,
        }))
    })?;
    Ok(ToolOutput::json(v))
}

fn replace_in_document(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[
        "document_id",
        "query",
        "replacement",
        "match_case",
        "whole_word",
    ])?;
    let query = bounded(args, "query")?;
    let replacement = bounded(args, "replacement")?;
    let opts = options(args)?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let report = search::replace_all_limited(
            doc,
            &query,
            &replacement,
            opts,
            REPLACE_LABEL,
            MAX_REGIONS,
        )?;
        let remaining = search::find(doc.document(), &query, opts).len();
        let mut out = json!({
            "ok": report.refused.is_empty() && !report.truncated,
            "replaced": report.replaced,
            "refused": report.refused,
            "truncated": report.truncated,
            "remaining": remaining,
        });
        if !opts.whole_word && report.math_regions > 0 {
            out["warning"] = json!(
                "whole_word is false: the replacement was made inside names and numbers of math regions too (x in max, e in 1e-3). Check the result, or undo with document_undo and repeat with whole_word=true."
            );
        }
        Ok(out)
    })?;
    Ok(ToolOutput::json(v))
}

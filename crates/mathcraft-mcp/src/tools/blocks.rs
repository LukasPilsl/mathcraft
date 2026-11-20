//! Block tools: insert_symbolic, insert_solve_block and insert_program. They make the regions the
//! GUI has no button for yet: a symbolic evaluation (`expr -> solve, x`), a solve block (guesses,
//! constraints and a `find`/`minerr`/... call as one region) and a multi-line program.
//!
//! Each is also an op of `batch` (same parser, same apply). Every call answers with the results of
//! the regions it made, so the client sees the solution (or the error) at once.

use mathcraft_doc::{normalise_equation, Doc, Placement, SolveBlockSpec, SOLVER_NAMES};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::keywords::{self, Group};
use crate::schema::{self, Args};
use crate::tools::write::{doc_err, error_keys, feedback, invalid, merge, placement};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

/// Longest expression of a symbolic line, in characters.
const MAX_EXPR_CHARS: usize = 4096;
/// Longest single constraint, guess, name or argument, in characters.
const MAX_ITEM_CHARS: usize = 1000;
/// Most constraints, and most guesses, of one solve block.
const MAX_MEMBERS: usize = 100;
/// Most unknowns (arguments) of the solver call.
const MAX_VARS: usize = 50;
/// Most parameters of a program.
const MAX_PARAMS: usize = 32;

const LABEL_SYMBOLIC: &str = "AI: insert_symbolic";
const LABEL_SOLVE: &str = "AI: insert_solve_block";
const LABEL_PROGRAM: &str = "AI: insert_program";

/// Printed with every program: the file form of a program is not known yet.
const PROGRAM_SAVE_NOTE: &str = "A sheet that holds a program cannot be saved to .mcdx yet (the file form of programs is unknown): it works in memory only. document_save fails and names the region; delete it or keep the sheet open.";

/// A block edit, checked and owned.
pub(crate) enum BlockWrite {
    Symbolic {
        text: String,
        at: Placement,
    },
    Solve {
        spec: SolveBlockSpec,
        /// The unknowns of a `find`/`minerr` call that have no guess in the call.
        unguessed: Vec<String>,
        at: Placement,
    },
    Program {
        name: String,
        params: Vec<String>,
        lines: Vec<String>,
        at: Placement,
    },
}

/// What an applied block edit made.
pub(crate) struct Applied {
    /// The region the call is about (the block, for a solve block).
    pub region: u32,
    /// Every region whose result belongs in the answer.
    pub touched: Vec<u32>,
    /// Extra fields for the single tool's answer.
    pub extra: Value,
}

pub fn register(t: &mut ToolTable) {
    let at_props = || {
        vec![
            (
                "x",
                json!({"type": "number", "description": "Left edge in page points (needs y)"}),
            ),
            (
                "y",
                json!({"type": "number", "description": "Top edge in page points (needs x)"}),
            ),
            (
                "after_region",
                schema::integer("Place directly below this region"),
            ),
        ]
    };

    let mut props = vec![
        schema::document_id(),
        (
            "expression",
            schema::string("The expression, for example `x^2 - 5*x + 6 == 0`; use `==` for an equation"),
        ),
        (
            "keyword",
            schema::string("What to do with it, after the arrow: `solve, x`, `simplify`, `expand`, `factor`, `series, x = 0, 4`, `float, 5`, ... See mathcraft://reference/keywords. Omitted: a bare `->` (simplify)."),
        ),
    ];
    props.extend(at_props());
    t.add(Tool {
        name: "insert_symbolic",
        description: "Inserts a symbolic evaluation `expression -> keyword` and returns its result (an expression, a list of solutions, ...). `solve` and the other words are keywords, not functions: list them with list_functions or read mathcraft://reference/keywords. Also an op of `batch`.".into(),
        input_schema: schema::object(&props, &["document_id", "expression"]),
        annotations: annotations("Insert symbolic evaluation"),
        handler: |ctx, args| single(ctx, args, "insert_symbolic"),
    });

    let find = json!({
        "type": "object",
        "properties": {
            "kind": {"type": "string", "enum": SOLVER_KINDS, "description": "The solver call"},
            "vars": {
                "type": "array", "minItems": 1, "maxItems": MAX_VARS,
                "items": {"type": "string", "maxLength": MAX_ITEM_CHARS},
                "description": "The unknowns, in order. For Minimize/Maximize the function name comes first (`[\"f\", \"x\"]`); for odesolve/pdesolve these are the call's arguments (`[\"y(t)\", \"2\", \"200\"]`)",
            },
            "guesses": {
                "type": "object",
                "additionalProperties": {"type": ["string", "number"]},
                "description": "Starting values by name, for example {\"x\": \"1 m\", \"y\": 2}. Every unknown of Find/Minerr needs one (or a definition above the block).",
            },
            "target": {
                "type": "string", "maxLength": MAX_ITEM_CHARS,
                "description": "Name (or vector of names such as `[a, b]`) that receives the result. Required for odesolve and pdesolve, which define functions.",
            },
        },
        "required": ["kind", "vars"],
        "additionalProperties": false,
    });
    let mut props = vec![
        schema::document_id(),
        (
            "given",
            json!({
                "type": "array", "maxItems": MAX_MEMBERS,
                "items": {"type": "string", "maxLength": MAX_ITEM_CHARS},
                "description": "Constraints: equations with `==` (a single `=` is read as `==`) and inequalities `<`, `>`, `<=`, `>=`. May be empty for Minimize/Maximize.",
            }),
        ),
        ("find", find),
    ];
    props.extend(at_props());
    t.add(Tool {
        name: "insert_solve_block",
        description: "Inserts a solve block as one region: guess values, the constraints (`given`) and the solver call (`find`: Find, Minerr, Minimize, Maximize, odesolve or pdesolve). One undo step. Returns the ids of the members and the solution (values with units) or the error. Also an op of `batch`. See mathcraft://reference/keywords.".into(),
        input_schema: schema::object(&props, &["document_id", "given", "find"]),
        annotations: annotations("Insert solve block"),
        handler: |ctx, args| single(ctx, args, "insert_solve_block"),
    });

    let mut props = vec![
        schema::document_id(),
        ("name", schema::string("Function name, for example `fact`")),
        (
            "params",
            json!({
                "type": "array", "maxItems": MAX_PARAMS,
                "items": {"type": "string", "maxLength": 100},
                "description": "Parameter names. Empty: the program defines a value, not a function.",
            }),
        ),
        (
            "body",
            json!({
                "oneOf": [
                    {"type": "array", "items": {"type": "string"}, "maxItems": mathcraft_doc::program_text::MAX_PROGRAM_LINES},
                    {"type": "string"},
                ],
                "description": "The program's lines: an array, or one text with newlines. Nesting is by indentation (two spaces per level): `if`/`else if`/`else`/`for k ∈ 1..n`/`while`/`try`/`on error` own the deeper lines below them. `x ← expr` is a local assignment, `return expr` ends with a value. See mathcraft://reference/keywords.",
            }),
        ),
    ];
    props.extend(at_props());
    t.add(Tool {
        name: "insert_program",
        description: format!("Inserts the function definition `name(params) := <program>` with a multi-line program body (local assignments, if/else, for, while, break, continue, return, try/on error). Call it afterwards with insert_math (`fact(5) =`). One undo step. Also an op of `batch`. NOTE: {PROGRAM_SAVE_NOTE}"),
        input_schema: schema::object(&props, &["document_id", "name", "body"]),
        annotations: annotations("Insert program"),
        handler: |ctx, args| single(ctx, args, "insert_program"),
    });
}

/// The solver kinds as the schema offers them.
const SOLVER_KINDS: [&str; 6] = [
    "Find", "Minerr", "Minimize", "Maximize", "Odesolve", "Pdesolve",
];

fn annotations(title: &'static str) -> Annotations {
    Annotations {
        title,
        read_only: false,
        destructive: false,
        idempotent: false,
    }
}

fn label(name: &str) -> &'static str {
    match name {
        "insert_symbolic" => LABEL_SYMBOLIC,
        "insert_solve_block" => LABEL_SOLVE,
        _ => LABEL_PROGRAM,
    }
}

fn check_len(what: &str, s: &str, max: usize) -> Result<(), ToolError> {
    if s.chars().count() > max {
        return Err(ToolError::LimitReached(format!(
            "{what} is longer than {max} characters"
        )));
    }
    Ok(())
}

fn string_items(
    args: &Args<'_>,
    key: &str,
    max_items: usize,
    max_chars: usize,
) -> Result<Vec<String>, ToolError> {
    let Some(v) = args.0.get(key).filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let Some(items) = v.as_array() else {
        return Err(invalid(format!("`{key}` must be an array of strings")));
    };
    if items.len() > max_items {
        return Err(ToolError::LimitReached(format!(
            "`{key}` has {} items, over the limit of {max_items}",
            items.len()
        )));
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let s = item
                .as_str()
                .ok_or_else(|| invalid(format!("`{key}`[{i}] must be a string")))?;
            check_len(&format!("`{key}`[{i}]"), s, max_chars)?;
            Ok(s.to_owned())
        })
        .collect()
}

/// A name usable as a variable or function: letters, digits, `_`, starting with a letter.
fn plain_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(char::is_alphabetic) && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn parse_symbolic(args: &Args<'_>, allowed: &[&str]) -> Result<BlockWrite, ToolError> {
    args.only(allowed)?;
    let expr = args.str("expression")?.trim();
    if expr.is_empty() {
        return Err(invalid("`expression` is empty"));
    }
    check_len("`expression`", expr, MAX_EXPR_CHARS)?;
    if expr.contains("->") {
        return Err(invalid(
            "`expression` already holds an arrow: give the expression alone and the word after the arrow as `keyword`",
        ));
    }
    let keyword = args
        .opt_str("keyword")?
        .map(str::trim)
        .filter(|k| !k.is_empty());
    let mut expression = expr.to_owned();
    let text = match keyword {
        None => format!("{expression} ->"),
        Some(k) => {
            check_len("`keyword`", k, MAX_ITEM_CHARS)?;
            let head = k.trim_start_matches("->").trim();
            let word = head.split(',').next().unwrap_or(head).trim();
            let known = keywords::find(word).filter(|e| e.group == Group::Symbolic);
            if known.is_none() {
                let close = keywords::close_matches(word);
                let hint = if close.is_empty() {
                    String::new()
                } else {
                    format!("; did you mean {}?", close.join(", "))
                };
                let why = if keywords::find(word).is_some() {
                    "belongs to a solve block or a program, not after `->` (see insert_solve_block and insert_program)"
                } else {
                    "is not a symbolic keyword (see mathcraft://reference/keywords)"
                };
                return Err(invalid(format!("`{word}` {why}{hint}")));
            }
            if word.eq_ignore_ascii_case("solve") {
                expression = normalise_equation(&expression);
            }
            format!("{expression} -> {head}")
        }
    };
    Ok(BlockWrite::Symbolic {
        text,
        at: placement(args)?,
    })
}

fn guess_text(name: &str, value: &Value) -> Result<String, ToolError> {
    if !plain_name(name) {
        return Err(invalid(format!(
            "guess name `{name}` must be a plain name such as `x` or `x_1`"
        )));
    }
    let v = match value {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => {
            return Err(invalid(format!(
                "guess `{name}` must be a number or text such as \"2 m\""
            )))
        }
    };
    if v.is_empty() {
        return Err(invalid(format!("guess `{name}` is empty")));
    }
    check_len(&format!("guess `{name}`"), &v, MAX_ITEM_CHARS)?;
    Ok(format!("{name}:={v}"))
}

fn parse_solve(args: &Args<'_>, allowed: &[&str]) -> Result<BlockWrite, ToolError> {
    args.only(allowed)?;
    let constraints = string_items(args, "given", MAX_MEMBERS, MAX_ITEM_CHARS)?;
    let Some(find) = args.0.get("find").and_then(Value::as_object) else {
        return Err(invalid(
            "`find` must be an object like {\"kind\": \"Find\", \"vars\": [\"x\", \"y\"]}",
        ));
    };
    let f = Args(find);
    f.only(&["kind", "vars", "guesses", "target"])?;
    let kind = f.str("kind")?.trim().to_lowercase();
    if !SOLVER_NAMES.contains(&kind.as_str()) {
        let close = keywords::close_matches(&kind);
        let hint = if close.is_empty() {
            String::new()
        } else {
            format!("; did you mean {}?", close.join(", "))
        };
        return Err(invalid(format!(
            "`find.kind` is one of Find, Minerr, Minimize, Maximize, Odesolve, Pdesolve{hint}"
        )));
    }
    let vars = string_items(&f, "vars", MAX_VARS, MAX_ITEM_CHARS)?;
    if vars.is_empty() {
        return Err(invalid("`find.vars` needs at least one name"));
    }
    if vars.iter().any(|v| v.trim().is_empty() || v.contains('\n')) {
        return Err(invalid(
            "`find.vars` entries must be non-empty single lines",
        ));
    }
    let mut guesses = Vec::new();
    let mut guessed: Vec<&str> = Vec::new();
    if let Some(g) = f.0.get("guesses").filter(|v| !v.is_null()) {
        let Some(map) = g.as_object() else {
            return Err(invalid("`find.guesses` must be an object {name: value}"));
        };
        if map.len() > MAX_MEMBERS {
            return Err(ToolError::LimitReached(format!(
                "`find.guesses` has {} entries, over the limit of {MAX_MEMBERS}",
                map.len()
            )));
        }
        for (name, value) in map {
            guesses.push(guess_text(name, value)?);
            guessed.push(name);
        }
    }
    let unguessed = if matches!(kind.as_str(), "find" | "minerr") {
        vars.iter()
            .filter(|v| plain_name(v) && !guessed.contains(&v.as_str()))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let target = f
        .opt_str("target")?
        .map(str::trim)
        .filter(|t| !t.is_empty());
    if let Some(t) = target {
        check_len("`find.target`", t, MAX_ITEM_CHARS)?;
    }
    Ok(BlockWrite::Solve {
        spec: SolveBlockSpec {
            guesses,
            constraints,
            solver: format!("{kind}({})", vars.join(", ")),
            target: target.map(str::to_owned),
        },
        unguessed,
        at: placement(args)?,
    })
}

fn parse_program(args: &Args<'_>, allowed: &[&str]) -> Result<BlockWrite, ToolError> {
    args.only(allowed)?;
    let name = args.str("name")?.trim();
    if !plain_name(name) {
        return Err(invalid(
            "`name` must be a plain name such as `fact` (letters, digits, `_`)",
        ));
    }
    check_len("`name`", name, 100)?;
    let params = string_items(args, "params", MAX_PARAMS, 100)?;
    if let Some(p) = params.iter().find(|p| !plain_name(p)) {
        return Err(invalid(format!("parameter `{p}` must be a plain name")));
    }
    let lines: Vec<String> = match args.0.get("body") {
        Some(Value::String(s)) => s.lines().map(str::to_owned).collect(),
        Some(Value::Array(_)) => string_items(
            args,
            "body",
            mathcraft_doc::program_text::MAX_PROGRAM_LINES,
            mathcraft_doc::program_text::MAX_LINE_CHARS,
        )?,
        _ => {
            return Err(invalid(
                "`body` must be an array of lines or one text with newlines",
            ))
        }
    };
    if lines.len() > mathcraft_doc::program_text::MAX_PROGRAM_LINES {
        return Err(ToolError::LimitReached(format!(
            "`body` has {} lines, over the limit of {}",
            lines.len(),
            mathcraft_doc::program_text::MAX_PROGRAM_LINES
        )));
    }
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Err(invalid("`body` has no lines"));
    }
    Ok(BlockWrite::Program {
        name: name.to_owned(),
        params,
        lines,
        at: placement(args)?,
    })
}

/// Reads one block op. Standalone calls carry `document_id`, ops in a batch carry `op`.
pub(crate) fn parse_block(
    name: &str,
    args: &Args<'_>,
    standalone: bool,
) -> Result<BlockWrite, ToolError> {
    let head = if standalone { "document_id" } else { "op" };
    let own: &[&str] = match name {
        "insert_symbolic" => &["expression", "keyword"],
        "insert_solve_block" => &["given", "find"],
        "insert_program" => &["name", "params", "body"],
        other => return Err(invalid(format!("`{other}` is not a block op"))),
    };
    let mut allowed = vec![head, "x", "y", "after_region"];
    allowed.extend(own);
    match name {
        "insert_symbolic" => parse_symbolic(args, &allowed),
        "insert_solve_block" => parse_solve(args, &allowed),
        _ => parse_program(args, &allowed),
    }
}

/// Applies one checked block edit.
pub(crate) fn apply_block(doc: &mut Doc, w: &BlockWrite) -> Result<Applied, ToolError> {
    match w {
        BlockWrite::Symbolic { text, at } => {
            let id = doc.insert_math(text, *at).map_err(doc_err)?;
            Ok(Applied {
                region: id,
                touched: vec![id],
                extra: json!({}),
            })
        }
        BlockWrite::Solve {
            spec,
            unguessed,
            at,
        } => {
            let ids = doc.insert_solve_block(spec, *at).map_err(doc_err)?;
            let mut touched = ids.guesses.clone();
            touched.extend(&ids.constraints);
            touched.push(ids.solver);
            let mut notes = Vec::new();
            if !unguessed.is_empty() {
                notes.push(format!(
                    "no guess given for {}: the solver starts from a definition above the block, or fails",
                    unguessed.join(", ")
                ));
            }
            Ok(Applied {
                region: ids.block,
                touched,
                extra: json!({
                    "members": {
                        "guesses": ids.guesses,
                        "constraints": ids.constraints,
                        "solver": ids.solver,
                    },
                    "solver_region": ids.solver,
                    "notes": notes,
                }),
            })
        }
        BlockWrite::Program {
            name,
            params,
            lines,
            at,
        } => {
            let id = doc
                .insert_program(name, params, lines, *at)
                .map_err(doc_err)?;
            Ok(Applied {
                region: id,
                touched: vec![id],
                extra: json!({"saveable": false, "warning": PROGRAM_SAVE_NOTE}),
            })
        }
    }
}

fn single(ctx: &ToolCtx<'_>, args: &Args<'_>, name: &'static str) -> Result<ToolOutput, ToolError> {
    let plan = parse_block(name, args, true)?;
    let v = ctx.backend.with_doc(args.str("document_id")?, &mut |doc| {
        let before = error_keys(doc);
        let start = doc.steps_executed();
        match apply_block(doc, &plan) {
            Ok(done) => {
                let taken = doc.steps_executed() - start;
                if taken > 0 {
                    doc.collapse_steps(taken, label(name));
                }
                let fb = feedback(doc, &before, &done.touched, &[]);
                let mut out = merge(json!({"ok": true, "region_id": done.region}), done.extra);
                if let Some(solver) = out.get("solver_region").and_then(Value::as_u64) {
                    // The solution is the result of the solver call.
                    let solution = fb["results"]
                        .as_array()
                        .and_then(|r| r.iter().find(|x| x["region_id"] == solver))
                        .cloned();
                    if let Some(map) = out.as_object_mut() {
                        map.insert("solution".into(), solution.unwrap_or(Value::Null));
                    }
                }
                Ok(merge(out, fb))
            }
            Err(e) => {
                let taken = doc.steps_executed() - start;
                doc.rollback_steps(taken).map_err(|r| {
                    ToolError::Internal(format!("insert failed and could not be undone: {r}"))
                })?;
                Err(e)
            }
        }
    })?;
    Ok(ToolOutput::json(v))
}

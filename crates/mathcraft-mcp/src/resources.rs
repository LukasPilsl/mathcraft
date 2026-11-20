//! MCP resources: four reference pages (input syntax, functions, units, plots) and two views of an
//! open document (its transcript and one region).
//!
//! Every text is paged so that one page is never over the server's `max_response_bytes`. A page
//! ends with a footer naming the next one (`?page=N`). Worksheet text is data, never instructions.

use std::fmt::Write as _;

use mathcraft_core::units;
use mathcraft_doc::functions_table::FUNCTIONS;
use mathcraft_doc::CalcOptions;
use serde_json::{json, Value};

use crate::backend::DocBackend;
use crate::error::ToolError;
use crate::server::{INTERNAL_ERROR, INVALID_PARAMS, RESOURCE_NOT_FOUND};
use crate::tools::view::detail_json;
use crate::tools::Limits;

const SCHEME: &str = "mathcraft://";

/// Room kept free for a page footer (`-- page N of M; next: ...`) inside the byte cap.
const FOOTER_RESERVE: usize = 512;

/// The input syntax. Each line that starts with `> ` is an example that the test suite evaluates
/// and expects to read without error (see `tests/resources.rs`).
pub const SYNTAX: &str = r#"# Mathcraft input syntax

A math region holds one line of linear text. Lines starting with "> " are examples.

## Definitions and results
> len := 3 m
> dur := 2 s
> speed := len / dur
> speed =
> f(x) := x^2 + 1
> f(3) =

- `name := expr` defines a variable; `name(x, y) := expr` defines a function.
- `name ≡ expr` is the global definition form.
- `expr =` shows the result of `expr`.
- A definition can only use names defined above it in the worksheet.

## Operators
> 7 + 2 * 3 ^ 2 - 1 =
> 17 / 4 =
> 20 % =
> 3 < 4 ∧ 4 < 5 =
> |-4| =
> "a text string"

- `+ - * /` and `^` (power). Division is written with `/`.
- `%` percent after an operand.
- Comparisons `< <= > >= == <>` and the logic signs `∧ ∨ ⊕ ¬`.
- `|x|` is the absolute value. `"..."` is a text string.
- `x^T` is the transpose of a matrix.

## Numbers and units
> 5 m =
> 9.81 m/s^2 * 2 kg =
> 6.02e23 =
> 1e-5 * 2 =
> speed -> km/h =
> 2 t -> kg =

- A number followed by a unit name is a quantity: `5 m`, `2 m/s^2`.
- Units can be compound with `/`, `*` and `^`.
- Scientific notation: `1e-5`, `6.02e23`.
- `expr -> unit=` shows the result in another unit (the value is not changed). The final `=` is
  part of the form: without it the arrow starts a symbolic command and the line fails.
- A name that is also a unit (`d` day, `t` tonne, `m` metre, `s` second, `L` litre, `A` ampere)
  is the unit until the worksheet defines a variable of that name; from the definition on it is
  your variable (`L := 6 m`, then `L =` shows `6 m`). Symbolic keywords still read such names as
  units, so use `x`, `k`, `mat` there. The constant pi is written `π`; `pi` is not defined.
- Temperatures: `25 °C`, `77 °F`, `degC`, `degF`, `degR` are absolute temperatures (shown in K
  unless you add `-> °C`); `5 ΔC` or `5 K` is an interval: `20 °C + 5 K -> °C =` shows `25 °C`.

## Vectors and matrices
> v := [1; 2; 3]
> M := [1, 2; 3, 4]
> M^T =
> M[1, 0] =
> ORIGIN =
> v_0 := 4
> v_0 + 1 =

- `[1, 2, 3]` is a row, `[1; 2; 3]` is a column. Commas separate items, semicolons separate rows.
- `M[i, j]` and `v[i]` are elements. Indices start at `ORIGIN`, which is 0 by default.
- A subscripted name such as `v_0` or `v_{i+1}` is a variable of its own.

## Ranges, sums and calculus
> 0 .. 4 =
> 0, 0.5 .. 2 =
> sum(i, 1, 4, i^2) =
> product(i, 1, 4, i) =
> integral(x, 0, 1, x^2) =
> x := 2
> derivative(x, x^3) =
> limit(x, 0, sin(x) / x) =

- `from .. to` is a range; `from, step .. to` sets the step.
- `sum` and `product` take a variable, a start, an end and a body: `sum(i, 1, 4, i^2)`.
- `integral(x, lower, upper, body)` and `limit(x, target, body)` bind their own variable.
- `derivative(x, body)` is the first derivative of `body` with respect to `x`, at the value of `x`
  (here 2). `derivative(x, order, body)` takes a higher order.

## Functions and constants
> sqrt(16) =
> sin(π / 2) =
> e =
> if(3 > 2, 10, 20) =

- Built-in functions are listed by `list_functions`; `describe_function` gives one of them.
- Constants: `π` and `e` (`list_constants` lists them with their values).
- `if(condition, a, b)` chooses between two values.

## Symbolic commands
> (a + b)^2 -> expand
> y^2 - 5*y + 6 -> factor
> y^2 - 4 == 0 -> solve, y
> 3*y + 2 -> substitute, y = 4
> π / 4 -> float, 5

- `expr -> command` asks the symbolic engine for a rewritten form, without a value.
- Commands: `simplify`, `expand`, `factor`, `float, n`, `solve, x`, `substitute, x = value`,
  `collect`, `coeffs`, `parfrac`, `series`, `rewrite`, transforms (`laplace`, `fourier`, ...).
- A symbolic command is not a numeric result: its result is the symbolic form.

## Solve blocks
A solve block is a region, not one line: guess values, constraints and one solver call
(`find`, `minerr`, `minimize`, `maximize`, `odesolve`, `pdesolve`). Those names work only inside
a block. Create one with `insert_solve_block` (also an op of `batch`): `given` lists the
constraints (`==` and inequalities), `find` names the solver and its unknowns, `guesses` their
starting values. Details and tested examples: `mathcraft://reference/keywords`.

## Symbolic evaluation
`insert_symbolic` makes the `expr -> keyword` line from an expression and a keyword
(`solve, x`, `simplify`, `series, x = 0, 4`, ...); `insert_math` with the same text does too.
`solve` and the other words after `->` are keywords, not functions: see
`mathcraft://reference/keywords`.

## Programs
A program is the body of a function definition: several lines with local assignments `←`,
`if` / `else`, `for`, `while`, `break`, `continue`, `return` and `try` / `on error`. Create one
with `insert_program` (also an op of `batch`), giving the name, the parameters and the lines.
A sheet that holds a program cannot be saved to `.mcdx` yet (the file form is not known): it
works in memory, and `document_save` says so.

## Plots
A plot is a region made from a spec, not a line of math. See `mathcraft://reference/plots`.

## Text regions
A text region is free text, not math. It is not evaluated.
"#;

/// The plot reference. Plots are regions made from a spec (see `mathcraft_doc::plot::PlotSpec`).
pub const PLOTS: &str = r#"# Plots

A plot is a region made from a spec, not a line of math. The plot tools take the spec as
arguments; a spec that does not read is refused.

## Fields of a 2D plot spec
- `x`: the expression for the horizontal axis (usually `x`).
- `y`: one or more expressions separated by commas. Each one is a trace: `sin(x), cos(x)`.
- `x_min`, `x_max`, `y_min`, `y_max`: the displayed limits of the axes, as text.
- `grid`: true draws grid lines on both axes.
- `title`: the text shown above the plot.

## Units in a plot
The plot variable `x` is a plain number, with no unit. A trace that mixes `x` with a unit
fails with `dimension mismatch` (for example `x*tan(theta) - g*x^2` with `g` in m/s^2).
Two ways to fix it:
- write the unit in the expression: `x*m`, so `y = x^2*g` becomes `expr: "(x*m)^2*g"`;
- or pass `x_unit: "m"` to `insert_plot`, and the server writes every `x` as `(x*m)`.

Example, a projectile height against the horizontal distance in metres:
`insert_plot` with `series: [{"expr": "x*tan(theta) - g*x^2/(2*v^2*cos(theta)^2)"}]` and
`x_unit: "m"`, after `theta := 30 deg`, `g := 9.81 m/s^2`, `v := 20 m/s`. The axis numbers stay
plain: the curve is in metres. With `set_plot_options`, write `x*m` yourself (no `x_unit`).

Parametric traces (`x_expr` and `y_expr`) get the unit on both expressions.

## Polar plots
A polar plot takes `x` as the angle and `y` as the radius. Its spec is not editable beyond the
labels it shows.

## Batch
`insert_plot` and `set_plot_options` are ops of `batch` too, so a sheet with a plot is one undo
step.

## Results
A plot region shows its traces when they evaluate. A trace that fails is reported as an error
for that region (see `read_errors`).
"#;

/// The reference resources: uri, name, description, mime type.
const STATIC: [(&str, &str, &str, &str); 5] = [
    (
        "mathcraft://reference/syntax",
        "Input syntax",
        "Cheat sheet of the linear input syntax: definitions, operators, units, matrices, ranges, calculus, symbolic commands.",
        "text/markdown",
    ),
    (
        "mathcraft://reference/functions",
        "Functions",
        "Every function the Insert Function dialog lists, with category and description. Paged.",
        "text/markdown",
    ),
    (
        "mathcraft://reference/units",
        "Units",
        "The unit table: symbol, name, dimension and size in SI. Paged.",
        "text/markdown",
    ),
    (
        "mathcraft://reference/plots",
        "Plots",
        "How plot regions are specified: fields, polar plots, results.",
        "text/markdown",
    ),
    (
        "mathcraft://reference/keywords",
        "Keywords",
        "Symbolic keywords (solve, simplify, ...), solve-block words (Given, find, minimize, ...) and program words (if, for, return, ...): syntax, a tested example and common mistakes.",
        "text/markdown",
    ),
];

/// The per-document templates: uri template, name, description, mime type.
const TEMPLATES: [(&str, &str, &str, &str); 2] = [
    (
        "mathcraft://doc/{document_id}/transcript",
        "Document transcript",
        "Plain-text transcript of an open document, top to bottom, with results. Paged.",
        "text/plain",
    ),
    (
        "mathcraft://doc/{document_id}/region/{region_id}",
        "Region",
        "One region of an open document as JSON: content and current result.",
        "application/json",
    ),
];

/// `resources/list`: the reference pages and one transcript per open document.
pub fn list(backend: &dyn DocBackend) -> Value {
    let mut resources: Vec<Value> = STATIC
        .iter()
        .map(|(uri, name, description, mime)| {
            json!({"uri": uri, "name": name, "description": description, "mimeType": mime})
        })
        .collect();
    for doc in backend.list() {
        resources.push(json!({
            "uri": format!("mathcraft://doc/{}/transcript", doc.id),
            "name": format!("Transcript of {}", doc.id),
            "description": "Plain-text transcript of this open document, with results. Paged.",
            "mimeType": "text/plain",
        }));
    }
    json!({"resources": resources})
}

/// `resources/templates/list`.
pub fn templates() -> Value {
    let list: Vec<Value> = TEMPLATES
        .iter()
        .map(|(uri, name, description, mime)| {
            json!({"uriTemplate": uri, "name": name, "description": description, "mimeType": mime})
        })
        .collect();
    json!({"resourceTemplates": list})
}

/// What a URI names.
#[derive(Debug, PartialEq)]
enum Target {
    Syntax,
    Functions,
    Units,
    Plots,
    Keywords,
    Transcript(String),
    Region(String, u32),
}

/// `resources/read`.
///
/// # Errors
/// `-32002` for a URI that names nothing or a document that is not open, `-32602` for a bad
/// `page`, `-32603` for an internal failure. Each is a JSON-RPC error (code, message).
pub fn read(
    backend: &dyn DocBackend,
    limits: &Limits,
    params: &Value,
) -> Result<Value, (i64, String)> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "missing uri".to_string()))?;
    let (target, page) = parse_uri(uri)?;
    let (mime, full) = match target {
        Target::Syntax => ("text/markdown", SYNTAX.to_string()),
        Target::Functions => ("text/markdown", functions_text()),
        Target::Units => ("text/markdown", units_text()),
        Target::Plots => ("text/markdown", PLOTS.to_string()),
        Target::Keywords => ("text/markdown", crate::keywords::reference_text()),
        Target::Transcript(id) => {
            let v = backend
                .with_doc(&id, &mut |doc| Ok(json!({"text": doc.transcript()})))
                .map_err(tool_error)?;
            (
                "text/plain",
                v["text"].as_str().unwrap_or_default().to_string(),
            )
        }
        Target::Region(id, region) => {
            let v = backend
                .with_doc(&id, &mut |doc| {
                    if doc.current_calculation().is_none() {
                        doc.calculate(&CalcOptions::default());
                    }
                    Ok(detail_json(&doc.read_region(region)?))
                })
                .map_err(tool_error)?;
            let text = serde_json::to_string_pretty(&v)
                .map_err(|e| (INTERNAL_ERROR, format!("cannot render region: {e}")))?;
            ("application/json", text)
        }
    };
    let text = page_text(&full, page, limits.max_response_bytes, uri)?;
    Ok(json!({"contents": [{"uri": uri, "mimeType": mime, "text": text}]}))
}

fn not_found(uri: &str) -> (i64, String) {
    (RESOURCE_NOT_FOUND, format!("resource not found: {uri}"))
}

/// Reads `mathcraft://...?page=N` into its target and page (1-based, default 1).
fn parse_uri(uri: &str) -> Result<(Target, usize), (i64, String)> {
    let rest = uri.strip_prefix(SCHEME).ok_or_else(|| not_found(uri))?;
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let page = match query {
        None => 1,
        Some(q) => q
            .strip_prefix("page=")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n >= 1)
            .ok_or((
                INVALID_PARAMS,
                "the only query is page=N, N >= 1".to_string(),
            ))?,
    };
    let parts: Vec<&str> = path.split('/').collect();
    let target = match parts.as_slice() {
        ["reference", "syntax"] => Target::Syntax,
        ["reference", "functions"] => Target::Functions,
        ["reference", "units"] => Target::Units,
        ["reference", "plots"] => Target::Plots,
        ["reference", "keywords"] => Target::Keywords,
        ["doc", id, "transcript"] if !id.is_empty() => Target::Transcript((*id).to_string()),
        ["doc", id, "region", region] if !id.is_empty() => {
            let region = region.parse::<u32>().map_err(|_| not_found(uri))?;
            Target::Region((*id).to_string(), region)
        }
        _ => return Err(not_found(uri)),
    };
    Ok((target, page))
}

fn tool_error(e: ToolError) -> (i64, String) {
    match e {
        ToolError::UnknownDocument(m) | ToolError::NotFound(m) => (RESOURCE_NOT_FOUND, m),
        ToolError::InvalidParams(m) => (INVALID_PARAMS, m),
        other => (INTERNAL_ERROR, other.to_string()),
    }
}

/// The text of `functions`: one line per function the dialog lists.
fn functions_text() -> String {
    let mut s = format!(
        "# Functions\n\n{} functions. `describe_function` gives the details of one name.\n\n",
        FUNCTIONS.len()
    );
    for f in FUNCTIONS {
        let _ = writeln!(s, "- `{}` ({}): {}", f.name, f.category, f.description);
    }
    s
}

/// The text of `units`: one line per unit of the table, with its size in SI.
fn units_text() -> String {
    let table = units::units();
    let mut s = format!(
        "# Units\n\n{} units. Values are stored in SI base units; `size` is 1 unit in SI.\n\n",
        table.len()
    );
    for u in table {
        let _ = write!(
            s,
            "- `{}` ({}): {}, size {}",
            u.symbol, u.name, u.dimension, u.factor_to_si
        );
        if let Some(offset) = u.offset_to_si {
            let _ = write!(s, ", offset {offset} (absolute scale)");
        }
        s.push('\n');
    }
    s
}

/// Splits `text` into pages of at most `budget` bytes, at line ends where possible. A line longer
/// than the budget is cut at a character boundary.
fn split_pages(text: &str, budget: usize) -> Vec<String> {
    let budget = budget.max(64);
    let mut pages = Vec::new();
    let mut current = String::new();
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        while !rest.is_empty() {
            if current.len() + rest.len() <= budget {
                current.push_str(rest);
                break;
            }
            if current.is_empty() {
                let mut cut = budget.min(rest.len());
                while !rest.is_char_boundary(cut) {
                    cut -= 1;
                }
                pages.push(rest[..cut].to_string());
                rest = &rest[cut..];
            } else {
                pages.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() || pages.is_empty() {
        pages.push(current);
    }
    pages
}

/// One page of `text`, with a footer naming the next page when there is one.
fn page_text(
    text: &str,
    page: usize,
    max_bytes: usize,
    uri: &str,
) -> Result<String, (i64, String)> {
    let budget = max_bytes.saturating_sub(FOOTER_RESERVE).max(64);
    let pages = split_pages(text, budget);
    let total = pages.len();
    let body = pages.get(page - 1).ok_or((
        INVALID_PARAMS,
        format!("page {page} is past the last page ({total})"),
    ))?;
    if total == 1 {
        return Ok(body.clone());
    }
    let base = uri.split('?').next().unwrap_or(uri);
    let footer = if page < total {
        format!(
            "\n-- page {page} of {total}; next: {base}?page={}",
            page + 1
        )
    } else {
        format!("\n-- page {page} of {total}, the last page")
    };
    Ok(format!("{body}{footer}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_fit_the_budget_and_join_back() {
        let text: String = (0..500).map(|i| format!("line {i} ünïcödé\n")).collect();
        let pages = split_pages(&text, 300);
        assert!(pages.len() > 1);
        assert!(pages.iter().all(|p| p.len() <= 300));
        assert_eq!(pages.concat(), text);
    }

    #[test]
    fn a_long_line_is_cut_on_a_character_boundary() {
        let text = "é".repeat(400);
        let pages = split_pages(&text, 101);
        assert!(pages.iter().all(|p| p.len() <= 101));
        assert_eq!(pages.concat(), text);
    }

    #[test]
    fn uris_parse() {
        assert_eq!(
            parse_uri("mathcraft://reference/units").unwrap(),
            (Target::Units, 1)
        );
        assert_eq!(
            parse_uri("mathcraft://reference/functions?page=3").unwrap(),
            (Target::Functions, 3)
        );
        assert_eq!(
            parse_uri("mathcraft://doc/doc7/region/12").unwrap(),
            (Target::Region("doc7".into(), 12), 1)
        );
        assert_eq!(
            parse_uri("mathcraft://doc/doc7/transcript").unwrap(),
            (Target::Transcript("doc7".into()), 1)
        );
        assert!(parse_uri("mathcraft://reference/nothing").is_err());
        assert!(parse_uri("file:///etc/passwd").is_err());
        assert!(parse_uri("mathcraft://reference/units?page=0").is_err());
        assert!(parse_uri("mathcraft://doc/doc7/region/x").is_err());
    }

    #[test]
    fn syntax_examples_are_marked() {
        let examples = SYNTAX.lines().filter(|l| l.starts_with("> ")).count();
        assert!(examples >= 30, "only {examples} examples");
    }
}

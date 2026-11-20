//! Discovery tools: functions, units, constants. Descriptions come from Mathcraft's own function
//! table (`mathcraft_doc::functions_table`), never from PTC help text.

use mathcraft_core::eval::{all_constants, Arity, Env};
use mathcraft_core::units::{self, Dimension};
use mathcraft_doc::functions_table::{FunctionEntry, FUNCTIONS, HIDDEN};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::keywords;
use crate::schema::{self, Args};
use crate::tools::view::{paged, paging_properties};
use crate::tools::{Annotations, Tool, ToolCtx, ToolOutput, ToolTable};

pub fn register(t: &mut ToolTable) {
    let mut fn_props = vec![
        (
            "query",
            schema::string("Case-insensitive text to find in the name or description"),
        ),
        (
            "category",
            schema::string("Exact category name, as listed in `categories`"),
        ),
    ];
    fn_props.extend(paging_properties());
    t.add(Tool {
        name: "list_functions",
        description: "Lists the built-in functions and the keywords (symbolic, solve-block and program words, marked `keyword: true`) with category and a one-line description; filter by query or category. Paged.".into(),
        input_schema: schema::object(&fn_props, &[]),
        annotations: Annotations::read_only("List functions"),
        handler: list_functions,
    });
    t.add(Tool {
        name: "describe_function",
        description: "Describes one built-in function (category, description, number of arguments) or one keyword such as `solve`, `find`, `for` (syntax, a tested example, common mistakes). Unknown names get close-match suggestions.".into(),
        input_schema: schema::object(
            &[("name", schema::string("Function or keyword name, for example `linfit` or `solve`"))],
            &["name"],
        ),
        annotations: Annotations::read_only("Describe function"),
        handler: describe_function,
    });
    let mut unit_props = vec![(
        "query",
        schema::string("Case-insensitive text to find in the unit name or symbol"),
    )];
    unit_props.extend(paging_properties());
    t.add(Tool {
        name: "list_units",
        description: "Lists the units the engine knows: name, symbol, dimension and factor to SI base units. Paged.".into(),
        input_schema: schema::object(&unit_props, &[]),
        annotations: Annotations::read_only("List units"),
        handler: list_units,
    });
    t.add(Tool {
        name: "list_constants",
        description:
            "Lists the built-in constants with their value and dimension in SI base units.".into(),
        input_schema: schema::empty(),
        annotations: Annotations::read_only("List constants"),
        handler: list_constants,
    });
}

fn function_json(f: &FunctionEntry) -> Value {
    json!({"name": f.name, "category": f.category, "description": f.description})
}

fn contains_ci(haystack: &str, needle_lower: &str) -> bool {
    haystack.to_lowercase().contains(needle_lower)
}

fn list_functions(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["query", "category", "cursor", "limit"])?;
    let query = args.opt_str("query")?.map(str::to_lowercase);
    let category = args.opt_str("category")?;
    let matches = |name: &str, description: &str| {
        query
            .as_deref()
            .is_none_or(|q| contains_ci(name, q) || contains_ci(description, q))
    };
    let mut items: Vec<Value> = FUNCTIONS
        .iter()
        .filter(|f| category.is_none_or(|c| f.category == c))
        .filter(|f| matches(f.name, f.description))
        .map(function_json)
        .collect();
    items.extend(
        keywords::KEYWORDS
            .iter()
            .filter(|k| category.is_none_or(|c| k.group.category() == c))
            .filter(|k| matches(k.name, k.description) || k.aliases.iter().any(|a| matches(a, "")))
            .map(keywords::list_json),
    );
    let mut categories: Vec<&str> = FUNCTIONS.iter().map(|f| f.category).collect();
    categories.extend(keywords::KEYWORDS.iter().map(|k| k.group.category()));
    categories.sort_unstable();
    categories.dedup();
    let mut out = paged(args, ctx.limits.page_items, "functions", &items)?;
    out["categories"] = json!(categories);
    Ok(ToolOutput::json(out))
}

fn arity_json(a: Arity) -> Value {
    match a {
        Arity::Exact(n) => json!({"min": n, "max": n}),
        Arity::Range(lo, hi) => json!({"min": lo, "max": hi}),
        Arity::AtLeast(n) => json!({"min": n, "max": null}),
    }
}

fn describe_function(_ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["name"])?;
    let name = args.str("name")?;
    let entry = FUNCTIONS
        .iter()
        .find(|f| f.name == name)
        .or_else(|| FUNCTIONS.iter().find(|f| f.name.eq_ignore_ascii_case(name)));
    let Some(entry) = entry else {
        if let Some(k) = keywords::find(name) {
            return Ok(ToolOutput::json(keywords::describe_json(k)));
        }
        if keywords::is_unsupported(name) {
            return Err(ToolError::NotFound(format!(
                "`{name}` is a keyword, not a function: see mathcraft://reference/keywords (this build does not implement it)"
            )));
        }
        let hidden = HIDDEN.iter().any(|(n, _)| *n == name);
        let close = keywords::close_matches(name);
        let hint = if close.is_empty() {
            String::new()
        } else {
            format!("; did you mean {}?", close.join(", "))
        };
        return Err(ToolError::NotFound(if hidden {
            format!("`{name}` is not available as a user function")
        } else {
            format!("no function or keyword `{name}`; use list_functions to search{hint}")
        }));
    };
    let env = Env::new();
    let arguments = env.builtins().get(entry.name).map(|b| arity_json(b.arity));
    let mut out = function_json(entry);
    out["arguments"] = arguments.unwrap_or(Value::Null);
    Ok(ToolOutput::json(out))
}

fn list_units(ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&["query", "cursor", "limit"])?;
    let query = args.opt_str("query")?.map(str::to_lowercase);
    let items: Vec<Value> = units::units()
        .iter()
        .filter(|u| {
            query
                .as_deref()
                .is_none_or(|q| contains_ci(u.name, q) || contains_ci(u.symbol, q))
        })
        .map(|u| {
            json!({
                "name": u.name,
                "symbol": u.symbol,
                "dimension": u.dimension.to_string(),
                "factor_to_si": u.factor_to_si,
                "offset_to_si": u.offset_to_si,
            })
        })
        .collect();
    Ok(ToolOutput::json(paged(
        args,
        ctx.limits.page_items,
        "units",
        &items,
    )?))
}

fn list_constants(_ctx: &ToolCtx<'_>, args: &Args<'_>) -> Result<ToolOutput, ToolError> {
    args.only(&[])?;
    let items: Vec<Value> = all_constants()
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "value_si": c.value,
                "dimension": Dimension::from_ints(c.dim).to_string(),
            })
        })
        .collect();
    Ok(ToolOutput::json(json!({"constants": items})))
}

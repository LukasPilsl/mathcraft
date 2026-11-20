//! A text form for programming regions, for callers without a mouse (the MCP server).
//!
//! The engine's expression printer shows a program one statement per line, indented by nesting,
//! but the expression parser has no program syntax. This module reads that line form back into
//! [`Stmt`]s, so a program can be written as plain lines:
//!
//! ```text
//! total ← 0
//! for k ∈ 1..n
//!   total ← total + k
//! return total
//! ```
//!
//! - Nesting is by indentation: a header line (`if`, `else if`, `also if`, `else`, `for`, `while`,
//!   `try`, `on error`) owns the lines indented deeper than itself directly below it.
//! - `←` (or `<-`) is the local assignment. `otherwise` is a synonym of `else`, `in` of `∈`.
//! - Every other line is an expression (`x`, `f(x)`, `y = `). A program's value is its `return`,
//!   or the last line that has a value.

use mathcraft_core::expr::{BranchKind, Expr, Id, IdKind, Stmt};
use mathcraft_core::parser;

use crate::context::EditContext;

/// Most lines of one program.
pub const MAX_PROGRAM_LINES: usize = 500;
/// Longest line, in characters.
pub const MAX_LINE_CHARS: usize = 2000;
/// Deepest nesting of blocks.
pub const MAX_NESTING: usize = 16;

struct Row {
    /// 1-based number among the lines given (blank lines count).
    no: usize,
    indent: usize,
    text: String,
}

/// Reads `lines` as the statements of one program.
///
/// Names that the program assigns locally or loops over are declared as variables in `ctx`, so a
/// local called `L` or `m` is not taken for a unit.
///
/// # Errors
/// A message naming the line that does not read, or the limit that was passed.
pub fn parse_program(lines: &[String], ctx: &mut EditContext) -> Result<Vec<Stmt>, String> {
    if lines.len() > MAX_PROGRAM_LINES {
        return Err(format!(
            "a program has at most {MAX_PROGRAM_LINES} lines, got {}",
            lines.len()
        ));
    }
    let mut rows = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        if raw.chars().count() > MAX_LINE_CHARS {
            return Err(format!(
                "line {}: longer than {MAX_LINE_CHARS} characters",
                i + 1
            ));
        }
        if raw.trim().is_empty() {
            continue;
        }
        let indent = raw
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum();
        rows.push(Row {
            no: i + 1,
            indent,
            text: raw.trim().to_owned(),
        });
    }
    if rows.is_empty() {
        return Err("a program needs at least one line".into());
    }
    // Names the program introduces are variables, whatever else they spell.
    for row in &rows {
        if let Some((target, _)) = split_assign(&row.text) {
            if let Some(name) = base_name(target) {
                ctx.declare_variable(name);
            }
        } else if let Some(rest) = row.text.strip_prefix("for ") {
            if let Some((var, _)) = split_for(rest) {
                ctx.declare_variable(var);
            }
        }
    }
    let mut pos = 0;
    let stmts = block(&rows, &mut pos, rows[0].indent, 0, ctx)?;
    if pos < rows.len() {
        return Err(format!(
            "line {}: indentation is less than the first line's",
            rows[pos].no
        ));
    }
    Ok(stmts)
}

fn base_name(target: &str) -> Option<&str> {
    let t = target.trim();
    let end = t.find('[').unwrap_or(t.len());
    let name = t[..end].trim();
    (!name.is_empty()).then_some(name)
}

/// `(target, expression)` of a local assignment line, `None` when the line is not one.
fn split_assign(text: &str) -> Option<(&str, &str)> {
    let (at, len) = match text.find('←') {
        Some(i) => (i, '←'.len_utf8()),
        None => (text.find("<-")?, 2),
    };
    let target = text[..at].trim();
    let target_like = !target.is_empty()
        && target
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        && target
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '[' | ']' | ',' | ' '));
    target_like.then(|| (target, text[at + len..].trim()))
}

/// `(variable, range)` of the text after `for `.
fn split_for(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.trim();
    let at = rest
        .find('∈')
        .map(|i| (i, '∈'.len_utf8()))
        .or_else(|| rest.find(" in ").map(|i| (i, 4)))?;
    let var = rest[..at.0].trim();
    let range = rest[at.0 + at.1..].trim();
    (!var.is_empty() && !range.is_empty()).then_some((var, range))
}

fn expr(text: &str, no: usize, ctx: &EditContext) -> Result<Expr, String> {
    parser::parse(text, ctx).map_err(|e| format!("line {no}: cannot read `{text}`: {e}"))
}

fn block(
    rows: &[Row],
    pos: &mut usize,
    indent: usize,
    depth: usize,
    ctx: &EditContext,
) -> Result<Vec<Stmt>, String> {
    if depth > MAX_NESTING {
        return Err(format!("blocks nested deeper than {MAX_NESTING} levels"));
    }
    let mut out: Vec<Stmt> = Vec::new();
    while *pos < rows.len() && rows[*pos].indent >= indent {
        let row = &rows[*pos];
        if row.indent > indent {
            return Err(format!(
                "line {}: indented without a header line (if, for, while, try, ...) above it",
                row.no
            ));
        }
        let text = row.text.as_str();
        let no = row.no;
        *pos += 1;
        // The lines indented deeper than this header, when there are any.
        let body = |pos: &mut usize| -> Result<Vec<Stmt>, String> {
            match rows.get(*pos) {
                Some(next) if next.indent > indent => block(rows, pos, next.indent, depth + 1, ctx),
                _ => Err(format!("line {no}: `{text}` needs indented lines below it")),
            }
        };
        let has_body = rows.get(*pos).is_some_and(|n| n.indent > indent);
        let stmt = if text == "break" {
            Stmt::Break
        } else if text == "continue" {
            Stmt::Continue
        } else if text == "return" {
            return Err(format!(
                "line {no}: `return` needs a value, as in `return x`"
            ));
        } else if let Some(rest) = text.strip_prefix("return ") {
            Stmt::Return(expr(rest.trim(), no, ctx)?)
        } else if text == "try" {
            let try_body = body(pos)?;
            match rows.get(*pos) {
                Some(next) if next.indent == indent && next.text == "on error" => {
                    *pos += 1;
                    let on_error = body(pos)?;
                    Stmt::Try {
                        body: try_body,
                        on_error,
                    }
                }
                _ => {
                    return Err(format!(
                        "line {no}: `try` needs an `on error` line after its body"
                    ))
                }
            }
        } else if text == "else" || text == "otherwise" {
            require_branch(&out, no, text)?;
            Stmt::Branch {
                kind: BranchKind::Else,
                cond: None,
                body: body(pos)?,
            }
        } else if let Some(rest) = text.strip_prefix("else if ") {
            require_branch(&out, no, "else if")?;
            Stmt::Branch {
                kind: BranchKind::ElseIf,
                cond: Some(expr(rest.trim(), no, ctx)?),
                body: body(pos)?,
            }
        } else if let Some(rest) = text.strip_prefix("also if ") {
            require_branch(&out, no, "also if")?;
            Stmt::Branch {
                kind: BranchKind::AlsoIf,
                cond: Some(expr(rest.trim(), no, ctx)?),
                body: body(pos)?,
            }
        } else if let (Some(rest), true) = (text.strip_prefix("if "), has_body) {
            Stmt::Branch {
                kind: BranchKind::If,
                cond: Some(expr(rest.trim(), no, ctx)?),
                body: body(pos)?,
            }
        } else if let Some(rest) = text.strip_prefix("while ") {
            Stmt::While {
                cond: expr(rest.trim(), no, ctx)?,
                body: body(pos)?,
            }
        } else if let Some(rest) = text.strip_prefix("for ") {
            let Some((var, range)) = split_for(rest) else {
                return Err(format!(
                    "line {no}: a loop reads `for k ∈ 1..n` (or `for k in 1..n`)"
                ));
            };
            Stmt::For {
                var: Id::new(var, IdKind::Variable),
                range: expr(range, no, ctx)?,
                body: body(pos)?,
            }
        } else if let Some((target, value)) = split_assign(text) {
            let t = expr(target, no, ctx)?;
            if !matches!(t, Expr::Id(_) | Expr::Index { .. }) {
                return Err(format!(
                    "line {no}: the left of `←` must be a name or an element like `v[i]`"
                ));
            }
            Stmt::Local {
                target: t,
                expr: expr(value, no, ctx)?,
            }
        } else if text == "on error" {
            return Err(format!("line {no}: `on error` without a `try` above it"));
        } else {
            Stmt::Line(expr(text, no, ctx)?)
        };
        out.push(stmt);
    }
    Ok(out)
}

fn require_branch(done: &[Stmt], no: usize, word: &str) -> Result<(), String> {
    match done.last() {
        Some(Stmt::Branch { .. }) => Ok(()),
        _ => Err(format!(
            "line {no}: `{word}` must come directly after an `if` block at the same indentation"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn reads_loops_branches_and_locals() {
        let mut ctx = EditContext::default();
        let stmts = parse_program(
            &lines("s ← 0\nfor k ∈ 1..5\n  if k > 3\n    break\n  s ← s + k\nreturn s"),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(stmts.len(), 3);
        assert!(matches!(&stmts[1], Stmt::For { body, .. } if body.len() == 2));
        assert!(matches!(&stmts[2], Stmt::Return(_)));
    }

    #[test]
    fn rejects_stray_else_and_bad_indent() {
        let mut ctx = EditContext::default();
        let e = parse_program(&lines("x ← 1\nelse\n  x ← 2"), &mut ctx).unwrap_err();
        assert!(e.contains("line 2"), "{e}");
        let e = parse_program(&lines("x ← 1\n  y ← 2"), &mut ctx).unwrap_err();
        assert!(e.contains("line 2"), "{e}");
        let e = parse_program(&lines("for k in 1..3"), &mut ctx).unwrap_err();
        assert!(e.contains("indented"), "{e}");
    }

    #[test]
    fn try_needs_on_error() {
        let mut ctx = EditContext::default();
        let ok = parse_program(&lines("try\n  1/0\non error\n  0"), &mut ctx).unwrap();
        assert!(matches!(&ok[0], Stmt::Try { .. }));
        assert!(parse_program(&lines("try\n  1/0"), &mut ctx).is_err());
    }

    #[test]
    fn local_named_like_a_unit_is_a_variable() {
        let mut ctx = EditContext::default();
        let stmts = parse_program(&lines("L ← 2\nreturn L*3"), &mut ctx).unwrap();
        assert!(
            matches!(&stmts[0], Stmt::Local { target: Expr::Id(id), .. } if id.kind == IdKind::Variable)
        );
    }
}

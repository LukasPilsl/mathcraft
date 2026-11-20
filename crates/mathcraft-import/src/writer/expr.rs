//! [`Expr`] to `ml:` XML: the inverse of [`crate::expr::convert_math`].
//!
//! [`write_math`] returns the XML of one expression. Reading that XML back with `convert_math`
//! gives the same [`Expr`]. Every shape the reader cannot give back unchanged is an error instead
//! of a silent change:
//!
//! * an exact rational, a global definition (`≡`), and the operators the reader never produces
//!   (`!=`, `<=`, `>=`, `and`, `or`, `xor`, `not`, factorial, percent) are unsupported;
//! * a `Placeholder` as a degree or unit override (it reads back as `None`);
//! * a plain number as a `Scale` unit (the reader rejects it);
//! * a `Command` holding only a placeholder (it reads back as no command);
//! * a number whose text does not parse to its value, or a subscript the reader would not read.
//!
//! A solve block's keyword (`find`, `minerr`) is stored as a `KEYWORD` identifier, which [`Expr`]
//! does not tell apart from a `FUNCTION` identifier. [`write_math_keyword`] writes the first
//! function identifier with the keyword's name as `KEYWORD`, and reports whether it did.

use mathcraft_core::expr::{Accumulation, BinaryOp, Command, Expr, Id, IdKind, Number, UnaryOp};

use super::xml::{check_chars, push_text};
use super::WriteError;
use crate::expr::MAX_DEPTH;

/// What the writer carries through one expression.
struct Cx<'k> {
    /// The keyword still to place: the first function identifier with this name is written as a
    /// `KEYWORD`. Cleared once it is placed.
    keyword: Option<&'k str>,
    /// Whether a `KEYWORD` was written.
    placed: bool,
}

/// The XML of one expression, without a surrounding `<math>` element.
///
/// # Errors
/// [`WriteError::Unsupported`] for an expression the writer cannot represent, and
/// [`WriteError::Invalid`] for one that would not read back as it is.
pub fn write_math(expr: &Expr) -> Result<String, WriteError> {
    write_math_keyword(expr, None).map(|(xml, _)| xml)
}

/// Like [`write_math`], and writes the first function identifier named `keyword` as a `KEYWORD`.
/// The second value is whether one was written.
pub(super) fn write_math_keyword(
    expr: &Expr,
    keyword: Option<&str>,
) -> Result<(String, bool), WriteError> {
    let mut out = String::new();
    let mut cx = Cx {
        keyword,
        placed: false,
    };
    write_expr(&mut out, expr, &mut cx, 0)?;
    Ok((out, cx.placed))
}

/// Writes `expr`, which sits `depth` expressions below the root. Past [`MAX_DEPTH`] the write is
/// refused, as the reader refuses such input, so the recursion is bounded.
fn write_expr(
    out: &mut String,
    expr: &Expr,
    cx: &mut Cx<'_>,
    depth: usize,
) -> Result<(), WriteError> {
    if depth > MAX_DEPTH {
        return Err(WriteError::Invalid(format!(
            "expression nested deeper than {MAX_DEPTH} levels"
        )));
    }
    match expr {
        Expr::Number(number) => write_number(out, number),
        Expr::Id(id) => write_id(out, id, cx),
        Expr::Unary { op, operand } => {
            let name = match op {
                UnaryOp::Neg => "neg",
                UnaryOp::Abs => "absval",
                UnaryOp::Transpose => "transpose",
                other => {
                    return Err(WriteError::Unsupported(format!(
                        "unary operator {other:?} is not written yet"
                    )))
                }
            };
            apply(out, name, &[operand], cx, depth)
        }
        Expr::Binary { op, lhs, rhs } => {
            let name = match op {
                BinaryOp::Add => "plus",
                BinaryOp::Sub => "minus",
                BinaryOp::Mul => "mult",
                BinaryOp::Div => "div",
                BinaryOp::Pow => "pow",
                BinaryOp::Eq => "equal",
                BinaryOp::Gt => "greaterThan",
                BinaryOp::Lt => "lessThan",
                other => {
                    return Err(WriteError::Unsupported(format!(
                        "binary operator {other:?} is not written yet"
                    )))
                }
            };
            apply(out, name, &[lhs, rhs], cx, depth)
        }
        Expr::Call { func, args } => {
            out.push_str("<ml:apply>");
            write_id(out, func, cx)?;
            for arg in args {
                write_expr(out, arg, cx, depth + 1)?;
            }
            out.push_str("</ml:apply>");
            Ok(())
        }
        Expr::Range { from, step, to } => {
            out.push_str("<ml:range>");
            match step {
                Some(step) => {
                    out.push_str("<ml:sequence>");
                    write_expr(out, from, cx, depth + 1)?;
                    write_expr(out, step, cx, depth + 1)?;
                    out.push_str("</ml:sequence>");
                }
                None => write_expr(out, from, cx, depth + 1)?,
            }
            write_expr(out, to, cx, depth + 1)?;
            out.push_str("</ml:range>");
            Ok(())
        }
        Expr::Matrix {
            rows,
            cols,
            elements,
        } => {
            if rows.checked_mul(*cols) != Some(elements.len()) {
                return Err(WriteError::Invalid(format!(
                    "matrix is {rows}x{cols} but holds {} elements",
                    elements.len()
                )));
            }
            out.push_str(&format!("<ml:matrix rows=\"{rows}\" cols=\"{cols}\">"));
            for element in elements {
                write_expr(out, element, cx, depth + 1)?;
            }
            out.push_str("</ml:matrix>");
            Ok(())
        }
        Expr::Index { base, indices } => match indices.as_slice() {
            [] => Err(WriteError::Invalid("indexing with no index".to_owned())),
            [single] => apply(out, "indexer", &[base, single], cx, depth),
            many => {
                out.push_str("<ml:apply><ml:indexer/>");
                write_expr(out, base, cx, depth + 1)?;
                out.push_str("<ml:sequence>");
                for index in many {
                    write_expr(out, index, cx, depth + 1)?;
                }
                out.push_str("</ml:sequence></ml:apply>");
                Ok(())
            }
        },
        Expr::NthRoot { degree, radicand } => {
            out.push_str("<ml:apply><ml:nthRoot/>");
            match degree.as_deref() {
                None => out.push_str("<ml:placeholder/>"),
                Some(Expr::Placeholder) => {
                    return Err(WriteError::Invalid(
                        "a placeholder degree reads back as no degree; use None".to_owned(),
                    ))
                }
                Some(degree) => write_expr(out, degree, cx, depth + 1)?,
            }
            write_expr(out, radicand, cx, depth + 1)?;
            out.push_str("</ml:apply>");
            Ok(())
        }
        Expr::Parens(inner) => {
            out.push_str("<ml:parens>");
            write_expr(out, inner, cx, depth + 1)?;
            out.push_str("</ml:parens>");
            Ok(())
        }
        Expr::Define { target, expr } => {
            out.push_str("<ml:define>");
            write_expr(out, target, cx, depth + 1)?;
            write_expr(out, expr, cx, depth + 1)?;
            out.push_str("</ml:define>");
            Ok(())
        }
        Expr::GlobalDefine { .. } => Err(WriteError::Unsupported(
            "global definition (≡) is not written yet".to_owned(),
        )),
        Expr::Eval {
            expr,
            unit_override,
        } => {
            out.push_str("<ml:eval>");
            write_expr(out, expr, cx, depth + 1)?;
            if let Some(unit) = unit_override {
                if matches!(unit.as_ref(), Expr::Placeholder) {
                    return Err(WriteError::Invalid(
                        "a placeholder unit override reads back as no override; use None"
                            .to_owned(),
                    ));
                }
                out.push_str("<ml:unitOverride>");
                write_expr(out, unit, cx, depth + 1)?;
                out.push_str("</ml:unitOverride>");
            }
            out.push_str("</ml:eval>");
            Ok(())
        }
        Expr::SymEval {
            expr,
            command,
            result,
        } => write_sym_eval(out, expr, command.as_ref(), result.as_deref(), cx, depth),
        Expr::Scale { value, unit } => {
            if matches!(unit.as_ref(), Expr::Number(_)) {
                return Err(WriteError::Invalid(
                    "a plain number as the unit of a scale reads back as an error".to_owned(),
                ));
            }
            apply(out, "scale", &[value, unit], cx, depth)
        }
        Expr::Placeholder => {
            out.push_str("<ml:placeholder/>");
            Ok(())
        }
        // No sample worksheet holds a string literal, so the Prime XML element for it is unknown
        // and the writer refuses it rather than guess one.
        Expr::Str(_) => Err(WriteError::Unsupported(
            "string literals are not written yet".to_owned(),
        )),
        // The importer reads no programming region yet (no sample contains one), so the writer
        // has no element to write it as.
        Expr::Program(_) => Err(WriteError::Unsupported(
            "programming regions are not written yet".to_owned(),
        )),
        Expr::Accumulate {
            op,
            var,
            from,
            to,
            body,
        } => {
            let name = match op {
                Accumulation::Sum => "summation",
                // No sample holds a product, so its element name is not known: refused.
                Accumulation::Product => {
                    return Err(WriteError::Unsupported(
                        "product operator: its element name is not in any sample".to_owned(),
                    ))
                }
            };
            out.push_str(&format!("<ml:apply><ml:{name} />"));
            write_lambda(out, var, body, cx, depth)?;
            write_bound(out, "lowerBound", from, cx, depth)?;
            write_bound(out, "upperBound", to, cx, depth)?;
            out.push_str("</ml:apply>");
            Ok(())
        }
        Expr::Integral {
            var,
            lower,
            upper,
            body,
        } => {
            out.push_str("<ml:apply><ml:integral />");
            write_lambda(out, var, body, cx, depth)?;
            write_bound(out, "lowerBound", lower, cx, depth)?;
            write_bound(out, "upperBound", upper, cx, depth)?;
            out.push_str("</ml:apply>");
            Ok(())
        }
        // UNVERIFIED (documentation/UNVERIFIED_FORMATS.md): no sample stores a partial derivative,
        // a gradient, a cross product or a norm, so they are refused, never written as a guess.
        Expr::Derivative { partial: true, .. } => Err(WriteError::Unsupported(
            "the partial derivative operator is not written: its stored form is unknown".to_owned(),
        )),
        Expr::Gradient { .. } => Err(WriteError::Unsupported(
            "the gradient operator is not written: its stored form is unknown".to_owned(),
        )),
        Expr::Derivative {
            var, order, body, ..
        } => {
            out.push_str("<ml:apply><ml:derivative />");
            write_lambda(out, var, body, cx, depth)?;
            // The first derivative stores an empty `degree`, a placeholder (sample `Unbenannt`).
            out.push_str("<ml:degree>");
            match order.as_deref() {
                None => out.push_str("<ml:placeholder />"),
                Some(Expr::Placeholder) => {
                    return Err(WriteError::Invalid(
                        "a placeholder degree reads back as the first derivative; use None"
                            .to_owned(),
                    ))
                }
                Some(n) => write_expr(out, n, cx, depth + 1)?,
            }
            out.push_str("</ml:degree></ml:apply>");
            Ok(())
        }
        Expr::Limit { var, target, body } => {
            out.push_str("<ml:apply><ml:limit />");
            write_lambda(out, var, body, cx, depth)?;
            write_expr(out, target, cx, depth + 1)?;
            out.push_str("</ml:apply>");
            Ok(())
        }
        // The vector form `sum(v)` has no element in any sample, so it is refused.
        Expr::Fold { .. } => Err(WriteError::Unsupported(
            "vector sum or product: its element is not in any sample".to_owned(),
        )),
    }
}

/// `<ml:lambda><ml:boundVars><ml:id>var</ml:id></ml:boundVars> body </ml:lambda>`.
fn write_lambda(
    out: &mut String,
    var: &Id,
    body: &Expr,
    cx: &mut Cx<'_>,
    depth: usize,
) -> Result<(), WriteError> {
    out.push_str("<ml:lambda><ml:boundVars>");
    write_id(out, var, cx)?;
    out.push_str("</ml:boundVars>");
    write_expr(out, body, cx, depth + 1)?;
    out.push_str("</ml:lambda>");
    Ok(())
}

/// A wrapper element holding a bound. An empty bound (a placeholder) is no element at all, which is
/// how the reader finds it.
fn write_bound(
    out: &mut String,
    wrapper: &str,
    bound: &Expr,
    cx: &mut Cx<'_>,
    depth: usize,
) -> Result<(), WriteError> {
    if matches!(bound, Expr::Placeholder) {
        return Ok(());
    }
    out.push_str(&format!("<ml:{wrapper}>"));
    write_expr(out, bound, cx, depth + 1)?;
    out.push_str(&format!("</ml:{wrapper}>"));
    Ok(())
}

/// `<ml:apply><ml:op/> operands... </ml:apply>`.
fn apply(
    out: &mut String,
    op: &str,
    operands: &[&Expr],
    cx: &mut Cx<'_>,
    depth: usize,
) -> Result<(), WriteError> {
    out.push_str(&format!("<ml:apply><ml:{op}/>"));
    for operand in operands {
        write_expr(out, operand, cx, depth + 1)?;
    }
    out.push_str("</ml:apply>");
    Ok(())
}

fn write_number(out: &mut String, number: &Number) -> Result<(), WriteError> {
    match number {
        Number::Real { value, text } => {
            if text.is_empty() || text.trim() != text {
                return Err(WriteError::Invalid(format!(
                    "number text `{text}` must be non-empty and have no surrounding spaces"
                )));
            }
            check_chars(text)?;
            match text.parse::<f64>() {
                Ok(parsed) if parsed.is_finite() && parsed == *value => {}
                _ => {
                    return Err(WriteError::Invalid(format!(
                        "number text `{text}` does not read back as {value}"
                    )))
                }
            }
            out.push_str("<ml:real>");
            push_text(out, text);
            out.push_str("</ml:real>");
            Ok(())
        }
        Number::Rational(_) => Err(WriteError::Unsupported(
            "exact rational numbers are not written yet".to_owned(),
        )),
    }
}

/// `<ml:id labels="...">name</ml:id>`, with a XAML subscript when the name has one.
fn write_id(out: &mut String, id: &Id, cx: &mut Cx<'_>) -> Result<(), WriteError> {
    if id.name.is_empty() || id.name.trim() != id.name {
        return Err(WriteError::Invalid(format!(
            "name `{}` must be non-empty and have no surrounding spaces",
            id.name
        )));
    }
    check_chars(&id.name)?;
    let labels = if id.kind == IdKind::Function && cx.keyword == Some(id.name.as_str()) {
        cx.keyword = None;
        cx.placed = true;
        "KEYWORD"
    } else {
        match id.kind {
            IdKind::Variable => "VARIABLE",
            IdKind::Function => "FUNCTION",
            IdKind::Unit => "UNIT",
            IdKind::Constant => "CONSTANT",
        }
    };
    out.push_str(&format!(
        "<ml:id labels=\"{labels}\" xml:space=\"preserve\">"
    ));
    match &id.subscript {
        None => push_text(out, &id.name),
        Some(subscript) => {
            let text = subscript_text(subscript)?;
            out.push_str(concat!(
                r#"<Span xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" "#,
                r#"xmlns:pw="clr-namespace:Ptc.Wpf;assembly=Ptc.Core">"#
            ));
            push_text(out, &id.name);
            out.push_str("<pw:Subscript>");
            push_text(out, &text);
            out.push_str("</pw:Subscript></Span>");
        }
    }
    out.push_str("</ml:id>");
    Ok(())
}

/// The text of a subscript the reader reads back: digits kept as written, or an alphanumeric
/// variable name.
fn subscript_text(subscript: &Expr) -> Result<String, WriteError> {
    match subscript {
        Expr::Number(Number::Real { text, .. })
            if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) =>
        {
            Ok(text.clone())
        }
        Expr::Id(Id {
            name,
            subscript: None,
            ..
        }) if !name.is_empty() && name.chars().all(char::is_alphanumeric) => Ok(name.clone()),
        other => Err(WriteError::Invalid(format!(
            "subscript `{other}` cannot be written; the reader reads only digits or a name"
        ))),
    }
}

/// `expr ->` with the commands in order, as the reader builds nested `SymEval`s: the innermost
/// command first, the symbolic result on the outermost node only.
fn write_sym_eval(
    out: &mut String,
    expr: &Expr,
    command: Option<&Command>,
    result: Option<&Expr>,
    cx: &mut Cx<'_>,
    depth: usize,
) -> Result<(), WriteError> {
    // Walk down the chain of commanded SymEvals to the base expression. The top node is the
    // outermost command. A top node without a command is written as it is, with no commands.
    let mut commands: Vec<&Command> = Vec::new();
    let mut base = expr;
    if let Some(command) = command {
        commands.push(command);
        while let Expr::SymEval {
            expr: inner,
            command: Some(inner_command),
            result: inner_result,
        } = base
        {
            if inner_result.is_some() {
                return Err(WriteError::Invalid(
                    "a symbolic result is kept on the outermost command only".to_owned(),
                ));
            }
            commands.push(inner_command);
            base = inner;
        }
    }
    out.push_str("<ml:symEval>");
    write_expr(out, base, cx, depth + 1)?;
    for command in commands.iter().rev() {
        write_command(out, command, cx, depth + 1)?;
    }
    if let Some(result) = result {
        out.push_str("<ml:symResult>");
        write_expr(out, result, cx, depth + 1)?;
        out.push_str("</ml:symResult>");
    }
    out.push_str("</ml:symEval>");
    Ok(())
}

/// `<ml:command>`: a single name directly, otherwise a `sequence` of words.
fn write_command(
    out: &mut String,
    command: &Command,
    cx: &mut Cx<'_>,
    depth: usize,
) -> Result<(), WriteError> {
    out.push_str("<ml:command>");
    match command.0.as_slice() {
        [Expr::Placeholder] => {
            return Err(WriteError::Invalid(
                "a command holding only a placeholder reads back as no command".to_owned(),
            ))
        }
        [Expr::Id(id)] => write_id(out, id, cx)?,
        words => {
            out.push_str("<ml:sequence>");
            for word in words {
                write_expr(out, word, cx, depth + 1)?;
            }
            out.push_str("</ml:sequence>");
        }
    }
    out.push_str("</ml:command>");
    Ok(())
}

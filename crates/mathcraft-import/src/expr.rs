//! Conversion of `<math>` trees in `worksheet.xml` to the engine AST ([`Expr`]).
//!
//! The entry point is [`convert_math`]. It takes a `<math>` element, or any single math element
//! (`ml:apply`, `ml:id`, ...), and returns one expression. Nothing is dropped silently: an element
//! that is not handled, or a known element with the wrong shape, is a [`ConvertError`] that
//! names the element.
//!
//! # Element mapping
//!
//! | XML (`ml:` unless noted)                 | `Expr`                                             |
//! |------------------------------------------|----------------------------------------------------|
//! | `id` (labels `VARIABLE`, absent, `*`, `SYSTEM`) | `Id` with `IdKind::Variable`               |
//! | `id` labels `FUNCTION`, `KEYWORD`        | `Id` with `IdKind::Function` (`find`, `solve`...)  |
//! | `id` labels `UNIT` / `CONSTANT`          | `Id` with `IdKind::Unit` / `IdKind::Constant`     |
//! | `real`                                   | `Number::Real`, text kept as written               |
//! | `placeholder`                            | `Expr::Placeholder` (empty `nthRoot` degree: `None`) |
//! | `apply` + `plus/minus/mult/div/pow`      | `Binary` `Add/Sub/Mul/Div/Pow`                     |
//! | `apply` + `equal/greaterThan/lessThan`   | `Binary` `Eq/Gt/Lt`                                |
//! | `apply` + `neg/absval/transpose`         | `Unary` `Neg/Abs/Transpose`                        |
//! | `apply` + `nthRoot` (degree, radicand)   | `NthRoot`                                          |
//! | `apply` + `scale` (value, unit)          | `Scale`                                            |
//! | `apply` + `indexer` (base, index or `sequence`) | `Index`                                     |
//! | `apply` + `id` (name, args or `sequence`) | `Call`                                            |
//! | `parens`                                 | `Parens`                                           |
//! | `range` (`from` or `sequence(from, second)`, `to`) | `Range`                           |
//! | `matrix` (`rows`, `cols`, elements)      | `Matrix`                                           |
//! | `define` (target, expr)                  | `Define`; a `function` target becomes a `Call`     |
//! | `function` (`id`, `boundVars`)           | `Call` with the bound variables as arguments       |
//! | `eval` (expr, `unitOverride`)            | `Eval`; an empty `unitOverride` placeholder is `None` |
//! | `symEval` (expr, `command`*, `symResult`?) | `SymEval`, see below                             |
//! | `apply` + `derivative` / `integral` / `summation` / `product` / `limit` | `Derivative`, `Integral`, `Accumulate`, `Limit`; see [Calculus operators] |
//! | `math` (`resultFormat` is skipped)       | the one expression inside                          |
//!
//! # Calculus operators
//!
//! Each operator is `<ml:apply>` with the operator element, then an `<ml:lambda>` holding
//! `<ml:boundVars>` (one `ml:id`, the bound variable) followed by the body. The other operands
//! are wrapper elements: `ml:degree` (derivative), `ml:lowerBound` and `ml:upperBound` (integral,
//! summation). A limit has no wrapper: its target is the bare expression after the lambda.
//!
//! An absent bound is the empty placeholder of the operator and becomes `Expr::Placeholder`. A
//! derivative's `ml:degree` holding a placeholder is the first derivative, `order: None`. `ml:product`
//! is not in any sample: it is read on the assumption it is the product of `ml:summation`.
//! A limit's side is not in any sample either, so a limit carrying any attribute is an error.
//!
//! `resultFormat` is display metadata, not math, so it is skipped at the top level of `<math>`.
//!
//! # Symbolic evaluation
//!
//! A `symEval` takes its `command` children in order. A `command` holding only a placeholder
//! means no command and becomes `command: None`. Words inside a command `sequence` are flattened:
//! `solve, x` nested with `y` becomes the command `solve, x, y`. A symEval with several
//! non-placeholder commands becomes nested `SymEval`s, one per command, the first innermost.
//! The `symResult` is kept as `result` on the outermost node.
//!
//! # Ranges
//!
//! Mathcad writes `a, b .. c` where `b` is the second value of the range, not the increment.
//! `Range::step` holds that second value, so the printed form matches the source.
//!
//! # XAML subscripts
//!
//! A subscripted name is stored as an `ml:id` whose only child is an XAML `Span`, with a
//! `Subscript` element (namespace `clr-namespace:Ptc.Wpf;assembly=Ptc.Core`) holding the subscript
//! text. Samples of this shape (296 in 18 files) look like this:
//!
//! ```xml
//! <ml:id labels="VARIABLE" xml:space="preserve">
//!   <Span xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
//!         xmlns:pw="clr-namespace:Ptc.Wpf;assembly=Ptc.Core">g<pw:Subscript>1</pw:Subscript></Span>
//! </ml:id>
//! ```
//!
//! The text before `Subscript` is the name (`g`), and the subscript becomes `Id.subscript`:
//! digits are a number kept as written (`02` stays `02`), a letter or digit run is a variable
//! (`max`, `G1`). Any other subscript text is an error.

use mathcraft_core::expr::{Accumulation, BinaryOp, Command, Expr, Id, IdKind, Number, UnaryOp};
use roxmltree::Node;
use thiserror::Error;

use crate::dom::{self, ML, WS};

/// Why a math tree could not be converted. Every variant names the element concerned.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// An element this converter does not handle, in a position where it can appear.
    #[error("unsupported element <{element}> inside <{parent}>")]
    UnknownElement {
        /// Qualified name of the element, such as `ml:command`.
        element: String,
        /// Qualified name of its parent element.
        parent: String,
    },
    /// A known element with the wrong shape: operand count, bad number, unknown label, or a
    /// subscript this converter cannot read.
    #[error("<{element}>: {reason}")]
    Malformed {
        /// Qualified name of the element.
        element: String,
        /// What is wrong with it.
        reason: String,
    },
    /// Expressions are nested deeper than [`MAX_DEPTH`].
    #[error("<{element}>: expressions nested deeper than {max} levels")]
    DepthExceeded {
        /// Qualified name of the element where the limit was passed.
        element: String,
        /// The limit, [`MAX_DEPTH`].
        max: usize,
    },
}

/// Deepest expression nesting the converter accepts. A debug build spends well over 1 KiB of
/// stack per level, so 100 levels stay inside the default 2 MiB thread stack (200 overflowed
/// it), while every real Mathcad worksheet is nested only a few levels deep.
pub const MAX_DEPTH: usize = 100;

/// Converts a `<math>` element, or one math element inside it, to an expression.
///
/// A `<math>` element must hold exactly one expression, ignoring `resultFormat`. Any other
/// element is converted as an expression itself.
///
/// # Errors
/// Returns [`ConvertError::UnknownElement`] for an element this converter does not handle and
/// [`ConvertError::Malformed`] for a known element with the wrong shape.
pub fn convert_math(node: Node<'_, '_>) -> Result<Expr, ConvertError> {
    if !is_math(node) {
        return convert_expr(node, 0);
    }
    let parts: Vec<Node<'_, '_>> = elements(node)
        .into_iter()
        .filter(|n| n.tag_name().name() != "resultFormat")
        .collect();
    match parts.as_slice() {
        [one] => convert_expr(*one, 0),
        [] => Err(malformed(node, "no expression inside <math>")),
        _ => Err(malformed(
            node,
            format!("{} expressions inside <math>, expected one", parts.len()),
        )),
    }
}

fn is_math(node: Node<'_, '_>) -> bool {
    node.tag_name().namespace() == Some(WS) && node.tag_name().name() == "math"
}

/// Local name of `node` when it is in the math namespace, otherwise `None`.
fn ml_name<'i>(node: Node<'_, 'i>) -> Option<&'i str> {
    (node.tag_name().namespace() == Some(ML)).then(|| node.tag_name().name())
}

fn elements<'a, 'i>(node: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    node.children().filter(|c| c.is_element()).collect()
}

fn qname(node: Node<'_, '_>) -> String {
    let name = node.tag_name();
    match name.namespace() {
        Some(ML) => format!("ml:{}", name.name()),
        Some(WS) => format!("ws:{}", name.name()),
        Some(other) => format!("{{{other}}}{}", name.name()),
        None => name.name().to_owned(),
    }
}

fn unknown(node: Node<'_, '_>) -> ConvertError {
    ConvertError::UnknownElement {
        element: qname(node),
        parent: node
            .parent_element()
            .map_or_else(|| "document".to_owned(), qname),
    }
}

fn malformed(node: Node<'_, '_>, reason: impl Into<String>) -> ConvertError {
    ConvertError::Malformed {
        element: qname(node),
        reason: reason.into(),
    }
}

/// The single element child of `node`, or an error naming how many there are.
fn only_child<'a, 'i>(node: Node<'a, 'i>) -> Result<Node<'a, 'i>, ConvertError> {
    match elements(node).as_slice() {
        [one] => Ok(*one),
        other => Err(malformed(
            node,
            format!("expected one child element, found {}", other.len()),
        )),
    }
}

/// Converts one expression. `depth` is how many expressions enclose `node`; past
/// [`MAX_DEPTH`] the conversion stops with [`ConvertError::DepthExceeded`] instead of
/// recursing further, so hostile input cannot overflow the stack.
fn convert_expr(node: Node<'_, '_>, depth: usize) -> Result<Expr, ConvertError> {
    if depth > MAX_DEPTH {
        return Err(ConvertError::DepthExceeded {
            element: qname(node),
            max: MAX_DEPTH,
        });
    }
    let Some(name) = ml_name(node) else {
        return Err(unknown(node));
    };
    match name {
        "id" => Ok(Expr::Id(convert_id(node)?)),
        "real" => convert_real(node),
        "apply" => convert_apply(node, depth),
        "define" => convert_define(node, depth),
        "eval" => convert_eval(node, depth),
        "symEval" => convert_sym_eval(node, depth),
        "matrix" => convert_matrix(node, depth),
        "range" => convert_range(node, depth),
        "function" => convert_function(node),
        "placeholder" => Ok(Expr::Placeholder),
        "parens" => Ok(Expr::Parens(Box::new(convert_expr(
            only_child(node)?,
            depth + 1,
        )?))),
        _ => Err(unknown(node)),
    }
}

fn convert_real(node: Node<'_, '_>) -> Result<Expr, ConvertError> {
    let text = dom::text_of(node);
    let text = text.trim();
    let value = text
        .parse::<f64>()
        .map_err(|_| malformed(node, format!("not a number: `{text}`")))?;
    if !value.is_finite() {
        return Err(malformed(node, format!("not a finite number: `{text}`")));
    }
    Ok(Expr::Number(Number::Real {
        value,
        text: text.to_owned(),
    }))
}

/// An `ml:id`: name, label, and optional XAML subscript (see the module docs).
fn convert_id(node: Node<'_, '_>) -> Result<Id, ConvertError> {
    if ml_name(node) != Some("id") {
        return Err(unknown(node));
    }
    let kind = match node.attribute("labels") {
        None | Some("VARIABLE" | "*" | "SYSTEM") => IdKind::Variable,
        Some("FUNCTION" | "KEYWORD") => IdKind::Function,
        Some("UNIT") => IdKind::Unit,
        Some("CONSTANT") => IdKind::Constant,
        Some(other) => return Err(malformed(node, format!("unknown label `{other}`"))),
    };
    let mut name = String::new();
    let mut subscript = None;
    for child in node.children() {
        if child.is_text() {
            let text = child.text().unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }
            if subscript.is_some() {
                return Err(malformed(node, "text after a subscript"));
            }
            name.push_str(text);
        } else if child.is_element() && child.tag_name().name() == "Span" {
            let (base, sub) = convert_span(child)?;
            name.push_str(&base);
            subscript = sub.map(Box::new);
        } else {
            return Err(unknown(child));
        }
    }
    if name.is_empty() {
        return Err(malformed(node, "empty name"));
    }
    Ok(Id {
        name,
        kind,
        subscript,
    })
}

/// The XAML `Span` of a subscripted name: base text, then one `Subscript`.
fn convert_span(span: Node<'_, '_>) -> Result<(String, Option<Expr>), ConvertError> {
    let mut base = String::new();
    let mut sub = None;
    for child in span.children() {
        if child.is_text() {
            let text = child.text().unwrap_or("").trim();
            if sub.is_some() && !text.is_empty() {
                return Err(malformed(span, "text after <Subscript>"));
            }
            base.push_str(text);
        } else if child.is_element() && child.tag_name().name() == "Subscript" {
            if sub.is_some() {
                return Err(malformed(span, "more than one <Subscript>"));
            }
            sub = Some(subscript_expr(child)?);
        } else {
            return Err(unknown(child));
        }
    }
    Ok((base, sub))
}

fn subscript_expr(node: Node<'_, '_>) -> Result<Expr, ConvertError> {
    let text = dom::text_of(node);
    let text = text.trim();
    if text.is_empty() {
        return Err(malformed(node, "empty subscript"));
    }
    if text.chars().all(|c| c.is_ascii_digit()) {
        let value = text
            .parse::<f64>()
            .map_err(|_| malformed(node, format!("bad subscript `{text}`")))?;
        if !value.is_finite() {
            return Err(malformed(node, format!("subscript too large: `{text}`")));
        }
        Ok(Expr::Number(Number::Real {
            value,
            text: text.to_owned(),
        }))
    } else if text.chars().all(char::is_alphanumeric) {
        Ok(Expr::var(text))
    } else {
        Err(malformed(node, format!("unsupported subscript `{text}`")))
    }
}

fn convert_apply(node: Node<'_, '_>, depth: usize) -> Result<Expr, ConvertError> {
    let kids = elements(node);
    let Some((op, operands)) = kids.split_first() else {
        return Err(malformed(node, "empty apply"));
    };
    let op = *op;
    let arity = |n: usize| -> Result<(), ConvertError> {
        if operands.len() == n {
            Ok(())
        } else {
            Err(malformed(
                op,
                format!("takes {n} operand(s), found {}", operands.len()),
            ))
        }
    };
    let Some(op_name) = ml_name(op) else {
        return Err(unknown(op));
    };
    match op_name {
        "plus" | "minus" | "mult" | "div" | "pow" | "equal" | "greaterThan" | "lessThan" => {
            arity(2)?;
            let bin = match op_name {
                "plus" => BinaryOp::Add,
                "minus" => BinaryOp::Sub,
                "mult" => BinaryOp::Mul,
                "div" => BinaryOp::Div,
                "pow" => BinaryOp::Pow,
                "equal" => BinaryOp::Eq,
                "greaterThan" => BinaryOp::Gt,
                _ => BinaryOp::Lt,
            };
            Ok(Expr::binary(
                bin,
                convert_expr(operands[0], depth + 1)?,
                convert_expr(operands[1], depth + 1)?,
            ))
        }
        "neg" | "absval" | "transpose" => {
            arity(1)?;
            let un = match op_name {
                "neg" => UnaryOp::Neg,
                "absval" => UnaryOp::Abs,
                _ => UnaryOp::Transpose,
            };
            Ok(Expr::unary(un, convert_expr(operands[0], depth + 1)?))
        }
        "nthRoot" => {
            arity(2)?;
            // Operand order is degree, then radicand. An empty degree is a placeholder.
            let degree = match convert_expr(operands[0], depth + 1)? {
                Expr::Placeholder => None,
                other => Some(Box::new(other)),
            };
            Ok(Expr::NthRoot {
                degree,
                radicand: Box::new(convert_expr(operands[1], depth + 1)?),
            })
        }
        "scale" => {
            arity(2)?;
            let value = convert_expr(operands[0], depth + 1)?;
            let unit = convert_expr(operands[1], depth + 1)?;
            if matches!(unit, Expr::Number(_)) {
                return Err(malformed(
                    op,
                    "the unit operand is a plain number; `6 2` would not read back",
                ));
            }
            Ok(Expr::Scale {
                value: Box::new(value),
                unit: Box::new(unit),
            })
        }
        "derivative" | "integral" | "summation" | "product" | "limit" => {
            convert_calculus(op, op_name, operands, depth)
        }
        "indexer" => {
            arity(2)?;
            let base = convert_expr(operands[0], depth + 1)?;
            let indices = match operands[1] {
                seq if ml_name(seq) == Some("sequence") => sequence_items(seq, depth)?,
                single => vec![convert_expr(single, depth + 1)?],
            };
            Ok(Expr::index(base, indices))
        }
        "id" => {
            let func = convert_id(op)?;
            let args = match operands {
                [seq] if ml_name(*seq) == Some("sequence") => sequence_items(*seq, depth)?,
                _ => operands
                    .iter()
                    .map(|n| convert_expr(*n, depth + 1))
                    .collect::<Result<_, _>>()?,
            };
            Ok(Expr::Call { func, args })
        }
        _ => Err(unknown(op)),
    }
}

/// A calculus operator (see the module docs, *Calculus operators*). `operands` are the elements
/// after the operator element: the lambda, then the wrappers (or the bare target of a limit).
fn convert_calculus(
    op: Node<'_, '_>,
    name: &str,
    operands: &[Node<'_, '_>],
    depth: usize,
) -> Result<Expr, ConvertError> {
    if op.attributes().next().is_some() {
        return Err(malformed(
            op,
            "attributes on a calculus operator are not in any sample; refused",
        ));
    }
    let Some((lambda, rest)) = operands.split_first() else {
        return Err(malformed(op, "needs a lambda with its bound variable"));
    };
    if ml_name(*lambda) != Some("lambda") {
        return Err(unknown(*lambda));
    }
    let (var, body) = calculus_lambda(*lambda, depth)?;
    let var = Box::new(var);
    let body = Box::new(body);
    if name == "limit" {
        let [target] = rest else {
            return Err(malformed(
                op,
                format!(
                    "takes a lambda and one target, found {} more operands",
                    rest.len()
                ),
            ));
        };
        return Ok(Expr::Limit {
            var,
            target: Box::new(convert_expr(*target, depth + 1)?),
            body,
        });
    }
    // The wrappers allowed for each operator. Anything else is an error.
    let allowed: &[&str] = match name {
        "derivative" => &["degree"],
        "integral" => &["lowerBound", "upperBound"],
        _ => &["lowerBound", "upperBound"],
    };
    // `first` is the degree or the lower bound; `upper` is the upper bound.
    let mut first: Option<Expr> = None;
    let mut upper: Option<Expr> = None;
    for kid in rest {
        let wrapper = match ml_name(*kid) {
            Some(w) if allowed.contains(&w) => w,
            _ => return Err(unknown(*kid)),
        };
        let value = convert_expr(only_child(*kid)?, depth + 2)?;
        let slot = if wrapper == "upperBound" {
            &mut upper
        } else {
            &mut first
        };
        if slot.is_some() {
            return Err(malformed(*kid, "given twice"));
        }
        *slot = Some(value);
    }
    // An absent bound is the operator's empty placeholder.
    let absent = || Expr::Placeholder;
    Ok(match name {
        "derivative" => Expr::Derivative {
            partial: false,
            var,
            order: match first {
                None | Some(Expr::Placeholder) => None,
                Some(n) => Some(Box::new(n)),
            },
            body,
        },
        "integral" => Expr::Integral {
            var,
            lower: Box::new(first.unwrap_or_else(absent)),
            upper: Box::new(upper.unwrap_or_else(absent)),
            body,
        },
        _ => Expr::Accumulate {
            op: if name == "summation" {
                Accumulation::Sum
            } else {
                Accumulation::Product
            },
            var,
            from: Box::new(first.unwrap_or_else(absent)),
            to: Box::new(upper.unwrap_or_else(absent)),
            body,
        },
    })
}

/// The bound variable and body of a `ml:lambda`: `<ml:boundVars>` with one `ml:id`, then the body.
fn calculus_lambda(lambda: Node<'_, '_>, depth: usize) -> Result<(Id, Expr), ConvertError> {
    let kids = elements(lambda);
    let [bound, body] = kids.as_slice() else {
        return Err(malformed(
            lambda,
            format!("needs boundVars and a body, found {} children", kids.len()),
        ));
    };
    if ml_name(*bound) != Some("boundVars") {
        return Err(unknown(*bound));
    }
    let vars = elements(*bound);
    let [var] = vars.as_slice() else {
        return Err(malformed(
            *bound,
            format!("needs one bound variable, found {}", vars.len()),
        ));
    };
    Ok((convert_id(*var)?, convert_expr(*body, depth + 1)?))
}

/// The items of an `ml:sequence`, each converted. Used for arguments, indices and range bounds.
fn sequence_items(node: Node<'_, '_>, depth: usize) -> Result<Vec<Expr>, ConvertError> {
    elements(node)
        .into_iter()
        .map(|kid| convert_expr(kid, depth + 1))
        .collect::<Result<Vec<_>, _>>()
        .and_then(|items| {
            if items.is_empty() {
                Err(malformed(node, "empty sequence"))
            } else {
                Ok(items)
            }
        })
}

fn convert_define(node: Node<'_, '_>, depth: usize) -> Result<Expr, ConvertError> {
    match elements(node).as_slice() {
        [target, expr] => Ok(Expr::Define {
            target: Box::new(convert_expr(*target, depth + 1)?),
            expr: Box::new(convert_expr(*expr, depth + 1)?),
        }),
        other => Err(malformed(
            node,
            format!(
                "needs a target and an expression, found {} children",
                other.len()
            ),
        )),
    }
}

fn convert_eval(node: Node<'_, '_>, depth: usize) -> Result<Expr, ConvertError> {
    let kids = elements(node);
    let (expr, unit_override) = match kids.as_slice() {
        [expr] if ml_name(*expr) != Some("unitOverride") => (*expr, None),
        [expr, ovr] if ml_name(*ovr) == Some("unitOverride") => (*expr, Some(*ovr)),
        _ => {
            return Err(malformed(
                node,
                format!(
                    "needs an expression and an optional <unitOverride>, found {} children",
                    kids.len()
                ),
            ))
        }
    };
    let unit_override = match unit_override {
        None => None,
        Some(ovr) => match convert_expr(only_child(ovr)?, depth + 1)? {
            Expr::Placeholder => None,
            unit => Some(Box::new(unit)),
        },
    };
    Ok(Expr::Eval {
        expr: Box::new(convert_expr(expr, depth + 1)?),
        unit_override,
    })
}

fn convert_sym_eval(node: Node<'_, '_>, depth: usize) -> Result<Expr, ConvertError> {
    let mut expr = None;
    let mut commands = Vec::new();
    let mut result = None;
    for kid in elements(node) {
        match ml_name(kid) {
            Some("command") => commands.push(convert_command(kid, depth)?),
            Some("symResult") => {
                if result.is_some() {
                    return Err(malformed(node, "more than one <symResult>"));
                }
                result = Some(Box::new(convert_expr(only_child(kid)?, depth + 1)?));
            }
            _ => {
                if expr.is_some() {
                    return Err(malformed(node, "more than one expression"));
                }
                expr = Some(convert_expr(kid, depth + 1)?);
            }
        }
    }
    let expr = expr.ok_or_else(|| malformed(node, "no expression to evaluate"))?;
    let commands: Vec<Command> = commands.into_iter().flatten().collect();
    if commands.is_empty() {
        return Ok(Expr::SymEval {
            expr: Box::new(expr),
            command: None,
            result,
        });
    }
    let last = commands.len() - 1;
    let mut acc = expr;
    for (i, command) in commands.into_iter().enumerate() {
        let here = if i == last { result.take() } else { None };
        acc = Expr::SymEval {
            expr: Box::new(acc),
            command: Some(command),
            result: here,
        };
    }
    Ok(acc)
}

/// A `command` element: `None` for a placeholder, otherwise its words.
fn convert_command(node: Node<'_, '_>, depth: usize) -> Result<Option<Command>, ConvertError> {
    let word = only_child(node)?;
    match ml_name(word) {
        Some("placeholder") => Ok(None),
        Some("id") => Ok(Some(Command(vec![Expr::Id(convert_id(word)?)]))),
        Some("sequence") => Ok(Some(Command(flatten_words(word, depth)?))),
        _ => Err(unknown(word)),
    }
}

/// Flattens nested `sequence` elements into command words. Each nested sequence is one level
/// deeper, so past [`MAX_DEPTH`] it fails with [`ConvertError::DepthExceeded`].
fn flatten_words(node: Node<'_, '_>, depth: usize) -> Result<Vec<Expr>, ConvertError> {
    if depth > MAX_DEPTH {
        return Err(ConvertError::DepthExceeded {
            element: qname(node),
            max: MAX_DEPTH,
        });
    }
    let mut words = Vec::new();
    for kid in elements(node) {
        if ml_name(kid) == Some("sequence") {
            words.extend(flatten_words(kid, depth + 1)?);
        } else {
            words.push(convert_expr(kid, depth + 1)?);
        }
    }
    Ok(words)
}

fn convert_matrix(node: Node<'_, '_>, depth: usize) -> Result<Expr, ConvertError> {
    let rows = count_attr(node, "rows")?;
    let cols = count_attr(node, "cols")?;
    let elements: Vec<Expr> = elements(node)
        .into_iter()
        .map(|kid| convert_expr(kid, depth + 1))
        .collect::<Result<_, _>>()?;
    if rows.checked_mul(cols) != Some(elements.len()) {
        return Err(malformed(
            node,
            format!("rows={rows} cols={cols} but {} elements", elements.len()),
        ));
    }
    Ok(Expr::Matrix {
        rows,
        cols,
        elements,
    })
}

fn count_attr(node: Node<'_, '_>, attr: &str) -> Result<usize, ConvertError> {
    node.attribute(attr)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .ok_or_else(|| malformed(node, format!("missing or bad `{attr}` attribute")))
}

fn convert_range(node: Node<'_, '_>, depth: usize) -> Result<Expr, ConvertError> {
    let (start, to) = match elements(node).as_slice() {
        [start, to] => (*start, *to),
        other => {
            return Err(malformed(
                node,
                format!("needs a start and an end, found {} children", other.len()),
            ))
        }
    };
    let (from, step) = if ml_name(start) == Some("sequence") {
        match elements(start).as_slice() {
            [from, second] => (
                convert_expr(*from, depth + 1)?,
                Some(convert_expr(*second, depth + 1)?),
            ),
            other => {
                return Err(malformed(
                    start,
                    format!(
                        "range start needs (from, second), found {} values",
                        other.len()
                    ),
                ))
            }
        }
    } else {
        (convert_expr(start, depth + 1)?, None)
    };
    Ok(Expr::range(from, step, convert_expr(to, depth + 1)?))
}

/// A `function` element: the name and its bound variables. Only seen as a `define` target.
fn convert_function(node: Node<'_, '_>) -> Result<Expr, ConvertError> {
    let kids = elements(node);
    let (name, bound) = match kids.as_slice() {
        [name] => (*name, None),
        [name, bound] => (*name, Some(*bound)),
        other => {
            return Err(malformed(
                node,
                format!(
                    "needs a name and bound variables, found {} children",
                    other.len()
                ),
            ))
        }
    };
    let func = convert_id(name)?;
    let args = match bound {
        None => Vec::new(),
        Some(b) if ml_name(b) == Some("boundVars") => elements(b)
            .into_iter()
            .map(|v| convert_id(v).map(Expr::Id))
            .collect::<Result<_, _>>()?,
        Some(other) => return Err(unknown(other)),
    };
    Ok(Expr::Call { func, args })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom;

    /// Wraps `inner` in a `<math>` element with the worksheet and math namespaces, converts it.
    fn conv(inner: &str) -> Result<Expr, ConvertError> {
        let xml = format!(
            r#"<math xmlns="http://schemas.mathsoft.com/worksheet50" xmlns:ml="http://schemas.mathsoft.com/math50">{inner}</math>"#
        );
        let doc = dom::parse(&xml).expect("test XML is well-formed");
        convert_math(doc.root_element())
    }

    fn show(inner: &str) -> String {
        conv(inner).expect("converts").to_string()
    }

    fn err(inner: &str) -> ConvertError {
        conv(inner).expect_err("should not convert")
    }

    const X: &str = r#"<ml:id labels="VARIABLE">x</ml:id>"#;
    const Y: &str = r#"<ml:id labels="VARIABLE">y</ml:id>"#;
    const TWO: &str = "<ml:real>2</ml:real>";

    fn apply(op: &str, operands: &[&str]) -> String {
        format!("<ml:apply><ml:{op}/>{}</ml:apply>", operands.concat())
    }

    #[test]
    fn real_keeps_text() {
        assert_eq!(show("<ml:real>0.50</ml:real>"), "0.50");
        assert_eq!(show("<ml:real>-3</ml:real>"), "-3");
    }

    #[test]
    fn bad_real_is_error() {
        let e = err("<ml:real>1,5</ml:real>");
        assert!(matches!(e, ConvertError::Malformed { ref element, .. } if element == "ml:real"));
    }

    #[test]
    fn id_labels_map_to_kinds() {
        let kind = |label: &str| {
            let id = conv(&format!(r#"<ml:id labels="{label}">n</ml:id>"#)).expect("converts");
            match id {
                Expr::Id(id) => id.kind,
                other => panic!("not an id: {other:?}"),
            }
        };
        assert_eq!(kind("VARIABLE"), IdKind::Variable);
        assert_eq!(kind("FUNCTION"), IdKind::Function);
        assert_eq!(kind("KEYWORD"), IdKind::Function);
        assert_eq!(kind("UNIT"), IdKind::Unit);
        assert_eq!(kind("CONSTANT"), IdKind::Constant);
        assert_eq!(kind("*"), IdKind::Variable);
        assert_eq!(kind("SYSTEM"), IdKind::Variable);
        let plain = conv("<ml:id>n</ml:id>").expect("converts");
        assert!(matches!(
            plain,
            Expr::Id(Id {
                kind: IdKind::Variable,
                ..
            })
        ));
    }

    #[test]
    fn unknown_label_is_error() {
        let e = err(r#"<ml:id labels="BOGUS">n</ml:id>"#);
        assert!(matches!(e, ConvertError::Malformed { .. }));
    }

    #[test]
    fn xaml_subscript_numeric_and_named() {
        let span = |base: &str, sub: &str| {
            format!(
                r#"<ml:id labels="VARIABLE"><Span xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:pw="clr-namespace:Ptc.Wpf;assembly=Ptc.Core">{base}<pw:Subscript>{sub}</pw:Subscript></Span></ml:id>"#
            )
        };
        assert_eq!(show(&span("v", "0")), "v_0");
        assert_eq!(show(&span("v", "02")), "v_02");
        assert_eq!(show(&span("v", "max")), "v_max");
        assert_eq!(show(&span("v", "G1")), "v_G1");
        // Subscripted name inside an operator:
        assert_eq!(show(&apply("pow", &[&span("v", "0"), TWO])), "v_0^2");
    }

    #[test]
    fn xaml_subscript_bad_text_is_error() {
        let xml = r#"<ml:id labels="VARIABLE"><Span xmlns="urn:x">v<pw:Subscript xmlns:pw="urn:y">i,j</pw:Subscript></Span></ml:id>"#;
        let e = err(xml);
        assert!(matches!(e, ConvertError::Malformed { .. }));
    }

    #[test]
    fn plus() {
        assert_eq!(show(&apply("plus", &[X, Y])), "x+y");
    }
    #[test]
    fn minus() {
        assert_eq!(show(&apply("minus", &[X, Y])), "x-y");
    }
    #[test]
    fn mult() {
        assert_eq!(show(&apply("mult", &[X, Y])), "x*y");
    }
    #[test]
    fn div() {
        assert_eq!(show(&apply("div", &[X, Y])), "x/y");
    }
    #[test]
    fn pow() {
        assert_eq!(show(&apply("pow", &[X, TWO])), "x^2");
    }
    #[test]
    fn neg() {
        assert_eq!(show(&apply("neg", &[X])), "-x");
    }
    #[test]
    fn equal() {
        assert_eq!(show(&apply("equal", &[X, Y])), "x==y");
    }
    #[test]
    fn greater_than() {
        assert_eq!(show(&apply("greaterThan", &[X, Y])), "x>y");
    }
    #[test]
    fn less_than() {
        assert_eq!(show(&apply("lessThan", &[X, Y])), "x<y");
    }
    #[test]
    fn absval() {
        assert_eq!(show(&apply("absval", &[X])), "|x|");
    }
    #[test]
    fn transpose() {
        assert_eq!(show(&apply("transpose", &[X])), "x^T");
    }
    #[test]
    fn parens_wraps_child() {
        assert_eq!(
            show(&format!(
                "<ml:parens>{}</ml:parens>",
                apply("plus", &[X, Y])
            )),
            "(x+y)"
        );
    }

    #[test]
    fn nth_root_placeholder_degree_is_square_root() {
        assert_eq!(
            show(&apply("nthRoot", &["<ml:placeholder/>", X])),
            "sqrt(x)"
        );
        let e = conv(&apply("nthRoot", &["<ml:placeholder/>", X])).expect("converts");
        assert!(matches!(e, Expr::NthRoot { degree: None, .. }));
    }

    #[test]
    fn nth_root_with_degree() {
        assert_eq!(
            show(&apply("nthRoot", &["<ml:real>3</ml:real>", X])),
            "root(x, 3)"
        );
    }

    #[test]
    fn range_from_and_to() {
        let r = format!("<ml:range>{TWO}<ml:real>5</ml:real></ml:range>");
        assert_eq!(show(&r), "2 .. 5");
    }

    #[test]
    fn range_with_second_value() {
        let r = format!("<ml:range><ml:sequence>{TWO}<ml:real>2.5</ml:real></ml:sequence><ml:real>5</ml:real></ml:range>");
        assert_eq!(show(&r), "2, 2.5 .. 5");
    }

    #[test]
    fn range_sequence_with_three_values_is_error() {
        let r = format!(
            "<ml:range><ml:sequence>{TWO}{TWO}{TWO}</ml:sequence><ml:real>5</ml:real></ml:range>"
        );
        assert!(matches!(err(&r), ConvertError::Malformed { .. }));
    }

    #[test]
    fn indexer_single_and_multi() {
        assert_eq!(show(&apply("indexer", &[X, TWO])), "x[2]");
        let multi =
            format!("<ml:apply><ml:indexer/>{X}<ml:sequence>{X}{TWO}</ml:sequence></ml:apply>");
        assert_eq!(show(&multi), "x[x,2]");
    }

    #[test]
    fn scale_value_and_unit() {
        let s = format!("<ml:apply><ml:scale/>{TWO}<ml:id labels=\"UNIT\">m</ml:id></ml:apply>");
        assert_eq!(show(&s), "2 m");
        let c =
            format!("<ml:apply><ml:scale/>{TWO}<ml:id labels=\"CONSTANT\">π</ml:id></ml:apply>");
        // A constant is not a unit kind, so the printer parenthesizes it.
        assert_eq!(show(&c), "2 (π)");
    }

    #[test]
    fn scale_with_number_unit_is_error() {
        let s = format!("<ml:apply><ml:scale/>{TWO}<ml:real>3</ml:real></ml:apply>");
        assert!(matches!(err(&s), ConvertError::Malformed { .. }));
    }

    #[test]
    fn call_with_sequence_arguments() {
        let c = format!("<ml:apply><ml:id labels=\"FUNCTION\">g</ml:id><ml:sequence>{X}{Y}</ml:sequence></ml:apply>");
        assert_eq!(show(&c), "g(x, y)");
        let single = format!("<ml:apply><ml:id labels=\"FUNCTION\">sin</ml:id>{X}</ml:apply>");
        assert_eq!(show(&single), "sin(x)");
    }

    #[test]
    fn define_scalar_and_matrix_target() {
        assert_eq!(show(&format!("<ml:define>{X}{TWO}</ml:define>")), "x:=2");
        let m = format!(
            "<ml:define><ml:matrix rows=\"2\" cols=\"1\">{X}{Y}</ml:matrix>{TWO}</ml:define>"
        );
        assert_eq!(show(&m), "[x; y]:=2");
    }

    #[test]
    fn define_function_with_bound_vars() {
        let f = format!(
            "<ml:define><ml:function><ml:id labels=\"VARIABLE\">g</ml:id><ml:boundVars>{X}{Y}</ml:boundVars></ml:function>{}</ml:define>",
            apply("plus", &[X, Y])
        );
        assert_eq!(show(&f), "g(x, y):=x+y");
    }

    #[test]
    fn define_with_subscripted_function_name() {
        let f = "<ml:define><ml:function><ml:id labels=\"VARIABLE\"><Span xmlns=\"urn:x\">g<pw:Subscript xmlns:pw=\"urn:y\">1</pw:Subscript></Span></ml:id><ml:boundVars>".to_owned()
            + X + "</ml:boundVars></ml:function>" + TWO + "</ml:define>";
        assert_eq!(show(&f), "g_1(x):=2");
    }

    #[test]
    fn eval_with_and_without_unit_override() {
        assert_eq!(
            show(&format!("<ml:eval>{}</ml:eval>", apply("plus", &[X, TWO]))),
            "x+2="
        );
        let e = format!(
            "<ml:eval>{}<ml:unitOverride><ml:id labels=\"UNIT\">m</ml:id></ml:unitOverride></ml:eval>",
            X
        );
        assert_eq!(show(&e), "x->m=");
        let blank =
            format!("<ml:eval>{X}<ml:unitOverride><ml:placeholder/></ml:unitOverride></ml:eval>");
        let e = conv(&blank).expect("converts");
        assert!(matches!(
            e,
            Expr::Eval {
                unit_override: None,
                ..
            }
        ));
    }

    #[test]
    fn matrix_row_major_in_document_order() {
        let m = "<ml:matrix rows=\"2\" cols=\"2\"><ml:real>1</ml:real><ml:real>2</ml:real><ml:real>3</ml:real><ml:real>4</ml:real></ml:matrix>";
        assert_eq!(show(m), "[1, 2; 3, 4]");
    }

    #[test]
    fn matrix_count_mismatch_is_error() {
        let m = "<ml:matrix rows=\"2\" cols=\"2\"><ml:real>1</ml:real></ml:matrix>";
        assert!(matches!(err(m), ConvertError::Malformed { .. }));
    }

    #[test]
    fn placeholder_is_placeholder() {
        assert_eq!(
            conv("<ml:placeholder/>").expect("converts"),
            Expr::Placeholder
        );
    }

    #[test]
    fn symeval_without_command() {
        let s = format!(
            "<ml:symEval>{}<ml:command><ml:placeholder/></ml:command></ml:symEval>",
            apply("plus", &[X, Y])
        );
        assert_eq!(show(&s), "x+y ->");
    }

    #[test]
    fn symeval_with_command_words_and_result() {
        let s = format!(
            "<ml:symEval>{}<ml:command><ml:sequence><ml:id>solve</ml:id>{X}</ml:sequence></ml:command><ml:symResult>{TWO}</ml:symResult></ml:symEval>",
            apply("pow", &[X, TWO])
        );
        let e = conv(&s).expect("converts");
        assert_eq!(e.to_string(), "x^2 -> solve, x");
        match e {
            Expr::SymEval {
                result: Some(r), ..
            } => assert_eq!(*r, Expr::num(2.0)),
            other => panic!("expected SymEval with result, got {other:?}"),
        }
    }

    #[test]
    fn symeval_nested_sequence_flattens_command() {
        let s = format!(
            "<ml:symEval>{X}<ml:command><ml:sequence><ml:sequence><ml:id>solve</ml:id>{X}</ml:sequence>{Y}</ml:sequence></ml:command></ml:symEval>"
        );
        assert_eq!(show(&s), "x -> solve, x, y");
    }

    #[test]
    fn symeval_two_commands_nest() {
        let s = format!(
            "<ml:symEval>{X}<ml:command><ml:id>expand</ml:id></ml:command><ml:command><ml:sequence><ml:id>float</ml:id>{TWO}</ml:sequence></ml:command></ml:symEval>"
        );
        assert_eq!(show(&s), "(x -> expand) -> float, 2");
    }

    #[test]
    fn eval_of_symeval_drops_equals() {
        let s = format!("<ml:eval><ml:symEval>{X}<ml:command><ml:placeholder/></ml:command></ml:symEval><ml:unitOverride><ml:placeholder/></ml:unitOverride></ml:eval>");
        assert_eq!(show(&s), "x ->");
    }

    #[test]
    fn math_skips_result_format() {
        let s = format!("{X}<resultFormat><general precision=\"10\"/></resultFormat>");
        assert_eq!(show(&s), "x");
    }

    #[test]
    fn math_without_expression_is_error() {
        assert!(matches!(
            err("<resultFormat/>"),
            ConvertError::Malformed { .. }
        ));
    }

    #[test]
    fn bare_ml_element_converts_directly() {
        let xml = r#"<ml:apply xmlns:ml="http://schemas.mathsoft.com/math50"><ml:mult/><ml:real>2</ml:real><ml:id labels="UNIT">m</ml:id></ml:apply>"#;
        let doc = dom::parse(xml).expect("well-formed");
        assert_eq!(
            convert_math(doc.root_element())
                .expect("converts")
                .to_string(),
            "2*m"
        );
    }

    #[test]
    fn unknown_element_is_named_error() {
        let e = err("<ml:bogus/>");
        assert_eq!(
            e,
            ConvertError::UnknownElement {
                element: "ml:bogus".to_owned(),
                parent: "ws:math".to_owned(),
            }
        );
        assert!(e.to_string().contains("ml:bogus"));
    }

    #[test]
    fn unknown_apply_operator_is_named_error() {
        let e = err("<ml:apply><ml:sqrt/><ml:real>2</ml:real></ml:apply>");
        assert!(
            matches!(e, ConvertError::UnknownElement { ref element, .. } if element == "ml:sqrt")
        );
    }

    #[test]
    fn operand_count_is_checked() {
        let e = err("<ml:apply><ml:plus/><ml:real>1</ml:real></ml:apply>");
        assert!(matches!(e, ConvertError::Malformed { ref element, .. } if element == "ml:plus"));
    }

    #[test]
    fn command_in_expression_position_is_error() {
        let e = err("<ml:command><ml:placeholder/></ml:command>");
        assert!(matches!(e, ConvertError::UnknownElement { .. }));
    }

    #[test]
    fn matrix_dimensions_that_overflow_are_error() {
        for m in [
            r#"<ml:matrix rows="4294967296" cols="4294967296"/>"#,
            r#"<ml:matrix rows="18446744073709551615" cols="2"/>"#,
        ] {
            let e = err(m);
            assert!(
                matches!(e, ConvertError::Malformed { ref element, .. } if element == "ml:matrix")
            );
        }
    }

    #[test]
    fn non_finite_real_is_error() {
        for text in [
            "NaN",
            "nan",
            "inf",
            "-inf",
            "+inf",
            "Infinity",
            "-Infinity",
            "1e400",
        ] {
            let e = err(&format!("<ml:real>{text}</ml:real>"));
            assert!(
                matches!(e, ConvertError::Malformed { ref element, .. } if element == "ml:real"),
                "{text}: {e:?}"
            );
        }
    }

    /// Every construct that can nest an expression or a command word.
    #[derive(Clone, Copy, Debug)]
    enum Kind {
        ApplyPlus,
        ApplyNeg,
        NthRoot,
        Scale,
        IndexerBase,
        IdSequenceArgs,
        IdDirectArgs,
        CommandSequences,
        Parens,
        MatrixElement,
        Define,
        EvalExpr,
        EvalUnitOverride,
        SymEval,
        Range,
    }

    const KINDS: [Kind; 15] = [
        Kind::ApplyPlus,
        Kind::ApplyNeg,
        Kind::NthRoot,
        Kind::Scale,
        Kind::IndexerBase,
        Kind::IdSequenceArgs,
        Kind::IdDirectArgs,
        Kind::CommandSequences,
        Kind::Parens,
        Kind::MatrixElement,
        Kind::Define,
        Kind::EvalExpr,
        Kind::EvalUnitOverride,
        Kind::SymEval,
        Kind::Range,
    ];

    /// `depth` nested wraps of `kind` around `x`. Each wrap is a constant prefix and suffix
    /// (plus a sibling `x` where the construct takes two operands), so the nest is one
    /// `repeat` per side.
    fn nest(kind: Kind, depth: usize) -> String {
        let (pre, post) = match kind {
            Kind::ApplyPlus => ("<ml:apply><ml:plus/>".to_owned(), format!("{X}</ml:apply>")),
            Kind::ApplyNeg => ("<ml:apply><ml:neg/>".to_owned(), "</ml:apply>".to_owned()),
            Kind::NthRoot => (
                format!("<ml:apply><ml:nthRoot/>{TWO}"),
                "</ml:apply>".to_owned(),
            ),
            Kind::Scale => (
                "<ml:apply><ml:scale/>".to_owned(),
                format!("{X}</ml:apply>"),
            ),
            Kind::IndexerBase => (
                "<ml:apply><ml:indexer/>".to_owned(),
                format!("{X}</ml:apply>"),
            ),
            Kind::IdSequenceArgs => (
                r#"<ml:apply><ml:id labels="FUNCTION">f</ml:id><ml:sequence>"#.to_owned(),
                format!("{X}</ml:sequence></ml:apply>"),
            ),
            Kind::IdDirectArgs => (
                r#"<ml:apply><ml:id labels="FUNCTION">f</ml:id>"#.to_owned(),
                format!("{X}</ml:apply>"),
            ),
            Kind::CommandSequences => {
                return format!(
                    "<ml:symEval>{X}<ml:command>{}{X}{}</ml:command></ml:symEval>",
                    "<ml:sequence>".repeat(depth),
                    "</ml:sequence>".repeat(depth)
                );
            }
            Kind::Parens => ("<ml:parens>".to_owned(), "</ml:parens>".to_owned()),
            Kind::MatrixElement => (
                r#"<ml:matrix rows="1" cols="1">"#.to_owned(),
                "</ml:matrix>".to_owned(),
            ),
            Kind::Define => (format!("<ml:define>{X}"), "</ml:define>".to_owned()),
            Kind::EvalExpr => ("<ml:eval>".to_owned(), "</ml:eval>".to_owned()),
            Kind::EvalUnitOverride => (
                format!("<ml:eval>{X}<ml:unitOverride>"),
                "</ml:unitOverride></ml:eval>".to_owned(),
            ),
            Kind::SymEval => (
                "<ml:symEval>".to_owned(),
                "<ml:command><ml:placeholder/></ml:command></ml:symEval>".to_owned(),
            ),
            Kind::Range => ("<ml:range>".to_owned(), format!("{TWO}</ml:range>")),
        };
        format!("{}{X}{}", pre.repeat(depth), post.repeat(depth))
    }

    /// Runs `f` on a thread with the 2 MiB stack that default test threads get.
    fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f)
            .expect("spawn")
            .join()
            .unwrap_or_else(|p| std::panic::resume_unwind(p))
    }

    #[test]
    fn nesting_past_max_depth_is_error() {
        assert!(conv(&nest(Kind::Parens, MAX_DEPTH)).is_ok());
        let e = err(&nest(Kind::Parens, MAX_DEPTH + 1));
        assert!(matches!(e, ConvertError::DepthExceeded { max, .. } if max == MAX_DEPTH));
    }

    #[test]
    fn nests_of_90_levels_convert() {
        for kind in [Kind::ApplyPlus, Kind::CommandSequences] {
            let ok = on_small_stack(move || conv(&nest(kind, 90)).is_ok());
            assert!(ok, "{kind:?}: 90 levels should convert");
        }
    }

    #[test]
    fn deep_nests_of_every_kind_are_depth_errors_not_overflow() {
        for kind in KINDS {
            let e =
                on_small_stack(move || conv(&nest(kind, 10_000)).expect_err("should not convert"));
            assert!(
                matches!(e, ConvertError::DepthExceeded { max, .. } if max == MAX_DEPTH),
                "{kind:?}: {e:?}"
            );
        }
    }

    /// A lambda with one bound variable and a body, as the calculus operators write it.
    fn lambda(var: &str, body: &str) -> String {
        format!(
            r#"<ml:lambda><ml:boundVars><ml:id xml:space="preserve">{var}</ml:id></ml:boundVars>{body}</ml:lambda>"#
        )
    }

    fn id_xml(name: &str) -> String {
        format!(r#"<ml:id xml:space="preserve">{name}</ml:id>"#)
    }

    #[test]
    fn product_operator_reads_as_product() {
        let xml = format!(
            "<ml:apply><ml:product />{}<ml:upperBound>{TWO}</ml:upperBound></ml:apply>",
            lambda("f", &id_xml("y"))
        );
        let expr = conv(&xml).expect("converts");
        assert!(matches!(
            expr,
            Expr::Accumulate {
                op: Accumulation::Product,
                ..
            }
        ));
    }

    #[test]
    fn absent_bounds_are_placeholders() {
        let xml = format!(
            "<ml:apply><ml:integral />{}</ml:apply>",
            lambda("p", &id_xml("x"))
        );
        let Expr::Integral { lower, upper, .. } = conv(&xml).expect("converts") else {
            panic!("not an integral");
        };
        assert_eq!(*lower, Expr::Placeholder);
        assert_eq!(*upper, Expr::Placeholder);
    }

    #[test]
    fn derivative_degree_placeholder_is_first_order() {
        let xml = format!(
            "<ml:apply><ml:derivative />{}<ml:degree><ml:placeholder /></ml:degree></ml:apply>",
            lambda("x", &id_xml("r"))
        );
        let Expr::Derivative { order, .. } = conv(&xml).expect("converts") else {
            panic!("not a derivative");
        };
        assert!(order.is_none());
    }

    #[test]
    fn derivative_degree_number_is_the_order() {
        let xml = format!(
            "<ml:apply><ml:derivative />{}<ml:degree>{TWO}</ml:degree></ml:apply>",
            lambda("x", &id_xml("r"))
        );
        let Expr::Derivative { order, .. } = conv(&xml).expect("converts") else {
            panic!("not a derivative");
        };
        assert_eq!(
            order.as_deref().map(ToString::to_string).as_deref(),
            Some("2")
        );
    }

    #[test]
    fn limit_with_an_attribute_is_refused() {
        let xml = format!(
            r#"<ml:apply><ml:limit side="right" />{}<ml:real>9</ml:real></ml:apply>"#,
            lambda("x", &id_xml("o"))
        );
        assert!(matches!(
            err(&xml),
            ConvertError::Malformed { ref element, .. } if element == "ml:limit"
        ));
    }

    #[test]
    fn limit_reads_target_as_bare_expression() {
        let xml = format!(
            "<ml:apply><ml:limit />{}<ml:real>9</ml:real></ml:apply>",
            lambda("x", &id_xml("o"))
        );
        assert_eq!(show(&xml), "limit(x, 9, o)");
    }

    #[test]
    fn wrapper_not_allowed_for_the_operator_is_refused() {
        // A derivative has no bounds: `lowerBound` there is not in any sample.
        let xml = format!(
            "<ml:apply><ml:derivative />{}<ml:lowerBound>{TWO}</ml:lowerBound></ml:apply>",
            lambda("x", &id_xml("r"))
        );
        assert!(matches!(
            err(&xml),
            ConvertError::UnknownElement { ref element, .. } if element == "ml:lowerBound"
        ));
    }

    #[test]
    fn lambda_without_bound_variable_is_refused() {
        let xml = "<ml:apply><ml:integral /><ml:lambda><ml:id>x</ml:id></ml:lambda></ml:apply>"
            .to_string();
        assert!(matches!(err(&xml), ConvertError::Malformed { .. }));
    }
}

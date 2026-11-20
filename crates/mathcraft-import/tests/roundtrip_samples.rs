//! Round trip of every `<math>` element in the 24 sample packages: convert, print with
//! `Display`, parse the text back, and compare the two trees.
//!
//! Each element is parsed with a context built from its own original tree (see
//! [`SampleContext`]). Trees are compared after [`normalize`], which removes only the
//! differences listed there. Any other difference, or any parse error, fails the test.

#[macro_use]
mod support;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mathcraft_core::expr::{BinaryOp, Command, Expr, Id, IdKind, Number, UnaryOp};
use mathcraft_core::parser::{parse, DefaultIdContext, IdContext};
use mathcraft_import::dom::{self, WS};
use mathcraft_import::expr::convert_math;
use mathcraft_import::package::Package;

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

/// Identifier kinds for the parser, taken from the original tree. Bare names, subscripted names
/// and called names each keep the kind of their first occurrence, because the converter can label
/// `v` a unit in `find(v, t)` and a variable in `v_0`. Names not in the tree fall back to the
/// parser's defaults.
///
/// A juxtaposed name reads as a `Scale` unit only when the parser classifies it as a unit. The
/// printed `1.1 sec` and `6 x^3` must read back as `Scale`s, even when the converter labelled the
/// unit a variable. So the leading name of every `Scale` unit counts as a unit to the parser.
#[derive(Default)]
struct SampleContext {
    bare: HashMap<String, IdKind>,
    subscripted: HashMap<String, IdKind>,
    called: HashMap<String, IdKind>,
    scale_units: HashSet<String>,
}

impl SampleContext {
    fn of(e: &Expr) -> Self {
        let mut ctx = SampleContext::default();
        ctx.visit(e);
        ctx
    }

    fn visit(&mut self, e: &Expr) {
        match e {
            Expr::Id(id) if id.subscript.is_some() => {
                self.subscripted.entry(id.name.clone()).or_insert(id.kind);
            }
            Expr::Id(id) => {
                self.bare.entry(id.name.clone()).or_insert(id.kind);
            }
            Expr::Call { func, .. } => {
                self.called.entry(func.name.clone()).or_insert(func.kind);
            }
            Expr::Scale { unit, .. } => {
                // Every name written in a unit reads back as a unit, not only the first.
                let mut stack = vec![unit.as_ref()];
                while let Some(part) = stack.pop() {
                    if let Expr::Id(id) = part {
                        self.scale_units.insert(id.name.clone());
                    }
                    stack.extend(children(part));
                }
            }
            _ => {}
        }
        for child in children(e) {
            self.visit(child);
        }
    }
}

impl IdContext for SampleContext {
    fn classify(&self, name: &str, called: bool) -> IdKind {
        if called {
            if let Some(kind) = self.called.get(name) {
                return *kind;
            }
            return self.bare.get(name).copied().unwrap_or(IdKind::Function);
        }
        self.bare.get(name).copied().unwrap_or(IdKind::Variable)
    }

    fn classify_subscripted(&self, name: &str) -> IdKind {
        match self.subscripted.get(name) {
            Some(kind) => *kind,
            None => self.classify(name, false),
        }
    }

    fn is_scale_unit(&self, name: &str) -> bool {
        self.scale_units.contains(name) || self.classify(name, false) == IdKind::Unit
    }
}

/// Direct sub-expressions, including subscripts, command items and `result`.
fn children(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Number(_) | Expr::Str(_) | Expr::Placeholder => vec![],
        Expr::Id(id) => id.subscript.as_deref().into_iter().collect(),
        Expr::Call { func, args } => func
            .subscript
            .as_deref()
            .into_iter()
            .chain(args.iter())
            .collect(),
        Expr::Unary { operand, .. } => vec![operand],
        Expr::Binary { lhs, rhs, .. } => vec![lhs, rhs],
        Expr::Range { from, step, to } => std::iter::once(&**from)
            .chain(step.as_deref())
            .chain(std::iter::once(&**to))
            .collect(),
        Expr::Matrix { elements, .. } => elements.iter().collect(),
        Expr::Index { base, indices } => std::iter::once(&**base).chain(indices).collect(),
        Expr::NthRoot { degree, radicand } => degree
            .as_deref()
            .into_iter()
            .chain(std::iter::once(&**radicand))
            .collect(),
        Expr::Parens(inner) => vec![inner],
        Expr::Define { target, expr } | Expr::GlobalDefine { target, expr } => vec![target, expr],
        Expr::Eval {
            expr,
            unit_override,
        } => std::iter::once(&**expr)
            .chain(unit_override.as_deref())
            .collect(),
        Expr::SymEval {
            expr,
            command,
            result,
        } => std::iter::once(&**expr)
            .chain(command.iter().flat_map(|c| c.0.iter()))
            .chain(result.as_deref())
            .collect(),
        Expr::Scale { value, unit } => vec![value, unit],
        // Programming regions have no text syntax, so the round trip never builds one.
        Expr::Program(_) => vec![],
        Expr::Accumulate { from, to, body, .. } => vec![from, to, body],
        Expr::Fold { operand, .. } => vec![operand],
        Expr::Integral {
            lower, upper, body, ..
        } => vec![lower, upper, body],
        Expr::Derivative { order, body, .. } => {
            order.as_deref().into_iter().chain([&**body]).collect()
        }
        Expr::Gradient { body, .. } => vec![body],
        Expr::Limit { target, body, .. } => vec![target, body],
    }
}

/// Removes the differences the printer and parser are documented to have. It runs on both
/// trees, so each side reaches the same canonical form:
///
/// - Parentheses are dropped (`Parens`). The parser keeps them only around atoms.
/// - `Neg` of a number is the negative number (`-2`), and `Neg` of a rational is the negative
///   rational. The parser folds a leading `-` into a literal.
/// - A division of two integers with a fractional result is the exact `Rational`, which prints
///   as `p/q`, and the parser reads `p/q` back as `Rational`.
/// - `Eval` around a `SymEval` with no unit override is the bare `SymEval`. The printer drops
///   the `=` there (documented on `Expr::SymEval`), so the text reads back as `SymEval`.
/// - `SymEval::result` is Mathcad's cached answer. It is never printed (documented on
///   `Expr::SymEval`), so it is dropped from both sides.
fn normalize(e: &Expr) -> Expr {
    let boxed = |x: &Expr| Box::new(normalize(x));
    match e {
        Expr::Parens(inner) => normalize(inner),
        Expr::Number(_) | Expr::Str(_) | Expr::Placeholder | Expr::Program(_) => e.clone(),
        Expr::Id(id) => Expr::Id(Id {
            name: id.name.clone(),
            kind: id.kind,
            subscript: id.subscript.as_deref().map(boxed),
        }),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => match normalize(operand) {
            Expr::Number(Number::Real { value, text }) if !text.starts_with('-') => {
                Expr::Number(Number::Real {
                    value: -value,
                    text: format!("-{text}"),
                })
            }
            Expr::Number(Number::Rational(r)) => Expr::Number(Number::Rational(-r)),
            other => Expr::unary(UnaryOp::Neg, other),
        },
        Expr::Unary { op, operand } => Expr::Unary {
            op: *op,
            operand: boxed(operand),
        },
        Expr::Binary {
            op: BinaryOp::Div,
            lhs,
            rhs,
        } => {
            let (l, r) = (normalize(lhs), normalize(rhs));
            match as_rational(&l, &r) {
                Some(q) => q,
                None => Expr::binary(BinaryOp::Div, l, r),
            }
        }
        Expr::Binary { op, lhs, rhs } => Expr::Binary {
            op: *op,
            lhs: boxed(lhs),
            rhs: boxed(rhs),
        },
        Expr::Call { func, args } => Expr::Call {
            func: Id {
                name: func.name.clone(),
                kind: func.kind,
                subscript: func.subscript.as_deref().map(boxed),
            },
            args: args.iter().map(normalize).collect(),
        },
        Expr::Range { from, step, to } => Expr::Range {
            from: boxed(from),
            step: step.as_deref().map(boxed),
            to: boxed(to),
        },
        Expr::Accumulate {
            op,
            var,
            from,
            to,
            body,
        } => Expr::Accumulate {
            op: *op,
            var: var.clone(),
            from: boxed(from),
            to: boxed(to),
            body: boxed(body),
        },
        Expr::Fold { op, operand } => Expr::Fold {
            op: *op,
            operand: boxed(operand),
        },
        Expr::Gradient { vars, body } => Expr::Gradient {
            vars: vars.clone(),
            body: boxed(body),
        },
        Expr::Limit { var, target, body } => Expr::Limit {
            var: var.clone(),
            target: boxed(target),
            body: boxed(body),
        },
        Expr::Integral {
            var,
            lower,
            upper,
            body,
        } => Expr::Integral {
            var: var.clone(),
            lower: boxed(lower),
            upper: boxed(upper),
            body: boxed(body),
        },
        Expr::Derivative {
            var,
            order,
            body,
            partial,
        } => Expr::Derivative {
            var: var.clone(),
            order: order.as_deref().map(boxed),
            body: boxed(body),
            partial: *partial,
        },
        Expr::Matrix {
            rows,
            cols,
            elements,
        } => Expr::Matrix {
            rows: *rows,
            cols: *cols,
            elements: elements.iter().map(normalize).collect(),
        },
        Expr::Index { base, indices } => Expr::Index {
            base: boxed(base),
            indices: indices.iter().map(normalize).collect(),
        },
        Expr::NthRoot { degree, radicand } => Expr::NthRoot {
            degree: degree.as_deref().map(boxed),
            radicand: boxed(radicand),
        },
        Expr::Define { target, expr } => Expr::Define {
            target: boxed(target),
            expr: boxed(expr),
        },
        Expr::GlobalDefine { target, expr } => Expr::GlobalDefine {
            target: boxed(target),
            expr: boxed(expr),
        },
        Expr::Eval {
            expr,
            unit_override: None,
        } if matches!(**expr, Expr::SymEval { .. }) => normalize(expr),
        Expr::Eval {
            expr,
            unit_override,
        } => Expr::Eval {
            expr: boxed(expr),
            unit_override: unit_override.as_deref().map(boxed),
        },
        Expr::SymEval { expr, command, .. } => Expr::SymEval {
            expr: boxed(expr),
            command: command
                .as_ref()
                .map(|c| Command(c.0.iter().map(normalize).collect())),
            result: None,
        },
        Expr::Scale { value, unit } => Expr::Scale {
            value: boxed(value),
            unit: boxed(unit),
        },
    }
}

/// The exact rational `l / r` when both are plain integer literals and the quotient is not an
/// integer. The rational is built by the parser itself (`p/q` reads back as `Rational`), so the
/// test needs no rational crate of its own.
fn as_rational(l: &Expr, r: &Expr) -> Option<Expr> {
    let (Expr::Number(Number::Real { text: a, .. }), Expr::Number(Number::Real { text: b, .. })) =
        (l, r)
    else {
        return None;
    };
    let is_int = |t: &str| {
        let digits = t.strip_prefix('-').unwrap_or(t);
        !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
    };
    if !is_int(a) || !is_int(b) || b.starts_with('-') || b.bytes().all(|c| c == b'0') {
        return None;
    }
    match parse(&format!("{a}/{b}"), &DefaultIdContext) {
        Ok(q @ Expr::Number(Number::Rational(_))) => Some(q),
        _ => None,
    }
}

#[derive(Default)]
struct Tally {
    exact: usize,
    harmless: usize,
    parse_errors: Vec<String>,
    mismatches: Vec<String>,
}

fn sample_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .expect("read examples dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "mcdx"))
        .collect();
    paths.sort();
    paths
}

/// Round-trips every `<math>` element of one package into `tally`. Returns the element count.
fn round_trip_file(path: &Path, tally: &mut Tally) -> usize {
    let name = path
        .file_name()
        .expect("file name")
        .to_string_lossy()
        .into_owned();
    let pkg = Package::open(path).expect("open sample");
    let doc = dom::parse(&pkg.worksheet).expect("parse worksheet");
    let math: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((WS, "math")))
        .collect();
    for (index, node) in math.iter().enumerate() {
        let label = format!("{name}#{index}");
        let orig = convert_math(*node).unwrap_or_else(|e| panic!("{label}: {e}"));
        let text = orig.to_string();
        let ctx = SampleContext::of(&orig);
        let back = match parse(&text, &ctx) {
            Ok(back) => back,
            Err(e) => {
                tally
                    .parse_errors
                    .push(format!("{label} text={text:?}: {e}"));
                continue;
            }
        };
        if orig == back {
            tally.exact += 1;
        } else if normalize(&orig) == normalize(&back) {
            tally.harmless += 1;
        } else {
            tally.mismatches.push(format!(
                "{label} text={text:?}\n  orig={orig:?}\n  back={back:?}"
            ));
        }
    }
    math.len()
}

/// Expected totals. A change here means the round trip changed, so review it before updating.
const MATH_ELEMENTS: usize = 704;
// Was 589 exact and 115 harmless. Now: `20 (m/sec)`, `7 (x)` and other number-then-group products
// read back as `Scale` (exact), and `p/q` reads back as the stored `Div` (exact).
const EXACT: usize = 591;
const HARMLESS: usize = 113;

#[test]
fn every_sample_expression_prints_parses_and_round_trips() {
    require_samples!();
    let paths = sample_paths();
    assert_eq!(paths.len(), 24, "expected 24 sample packages");

    println!(
        "{:<48} {:>6} {:>8} {:>9} {:>8}",
        "file", "math", "exact", "harmless", "failed"
    );
    let mut total = 0;
    let mut all = Tally::default();
    for path in &paths {
        let mut tally = Tally::default();
        let math = round_trip_file(path, &mut tally);
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        println!(
            "{:<48} {:>6} {:>8} {:>9} {:>8}",
            name,
            math,
            tally.exact,
            tally.harmless,
            tally.parse_errors.len() + tally.mismatches.len()
        );
        total += math;
        all.exact += tally.exact;
        all.harmless += tally.harmless;
        all.parse_errors.extend(tally.parse_errors);
        all.mismatches.extend(tally.mismatches);
    }
    println!(
        "total math={total} exact={} harmless={} parse_errors={} other_mismatches={}",
        all.exact,
        all.harmless,
        all.parse_errors.len(),
        all.mismatches.len()
    );

    assert_eq!(
        total, MATH_ELEMENTS,
        "math element count across the 24 samples"
    );
    assert!(
        all.parse_errors.is_empty(),
        "parse errors:\n{}",
        all.parse_errors.join("\n")
    );
    assert!(
        all.mismatches.is_empty(),
        "round-trip mismatches:\n{}",
        all.mismatches.join("\n")
    );
    assert_eq!(all.exact, EXACT, "exact round trips");
    assert_eq!(all.harmless, HARMLESS, "harmless differences");
}

//! The identifier context of a worksheet, and reading Mathcad text into an expression. No GUI.

use std::collections::HashMap;

use mathcraft_core::expr::{Expr, Id, IdKind};
use mathcraft_core::parser::{self, IdContext, ParseError};
use mathcraft_core::units;
use mathcraft_import::regions::{RegionKind, RegionModel};

/// The identifier classes the parser needs. A name the document already uses keeps the kind its
/// expressions gave it, so a variable called `A` or `m` stays a variable. Other names are units
/// when they are unit names, and variables otherwise. Called names are functions. Names with a
/// literal subscript (`v_0`) are variables.
#[derive(Debug, Clone, Default)]
pub struct EditContext {
    kinds: HashMap<String, IdKind>,
    /// Kinds of call heads (`g` in `g(x)`), kept apart from bare names.
    calls: HashMap<String, IdKind>,
}

impl EditContext {
    /// The context for the expressions of a document's regions, including solve-block members.
    pub fn for_regions(regions: &[RegionModel]) -> Self {
        let mut ctx = Self::default();
        for region in regions {
            if let RegionKind::Math(expr) = &region.kind {
                ctx.record(expr);
            }
            ctx.record_children(&region.children);
        }
        ctx
    }

    /// Makes `name` a variable even when it is also a unit name (`L`, `m`): used for the names a
    /// solve block or a program introduces, which are variables by construction.
    pub fn declare_variable(&mut self, name: &str) {
        self.kinds.insert(name.to_owned(), IdKind::Variable);
    }

    fn record_children(&mut self, regions: &[RegionModel]) {
        for region in regions {
            if let RegionKind::Math(expr) = &region.kind {
                self.record(expr);
            }
            self.record_children(&region.children);
        }
    }

    /// Notes the kind of every bare name in `expr`. A variable use wins over a unit use.
    fn record(&mut self, expr: &Expr) {
        let mut note = |id: &Id, called: bool| {
            // A call head keeps its kind even with a literal subscript (`S_b(x)`).
            if called {
                // A function use wins over a variable one (built-in `cos` is written both ways).
                let entry = self.calls.entry(id.name.clone()).or_insert(id.kind);
                if id.kind == IdKind::Function {
                    *entry = IdKind::Function;
                }
            } else if id.subscript.is_none() && id.kind != IdKind::Function {
                let entry = self.kinds.entry(id.name.clone()).or_insert(id.kind);
                if id.kind == IdKind::Variable {
                    *entry = IdKind::Variable;
                }
            }
        };
        visit_ids(expr, &mut note);
    }
}

/// Calls `f` on every `Id` in `expr`, including those in subscripts and call heads.
fn visit_ids(expr: &Expr, f: &mut dyn FnMut(&Id, bool)) {
    match expr {
        Expr::Number(_) | Expr::Str(_) | Expr::Placeholder => {}
        Expr::Id(id) => {
            f(id, false);
            if let Some(sub) = &id.subscript {
                visit_ids(sub, f);
            }
        }
        Expr::Unary { operand, .. } => visit_ids(operand, f),
        Expr::Binary { lhs, rhs, .. } => {
            visit_ids(lhs, f);
            visit_ids(rhs, f);
        }
        Expr::Call { func, args } => {
            f(func, true);
            for a in args {
                visit_ids(a, f);
            }
        }
        Expr::Range { from, step, to } => {
            visit_ids(from, f);
            if let Some(step) = step {
                visit_ids(step, f);
            }
            visit_ids(to, f);
        }
        Expr::Matrix { elements, .. } => {
            for x in elements {
                visit_ids(x, f)
            }
        }
        Expr::Index { base, indices } => {
            visit_ids(base, f);
            for x in indices {
                visit_ids(x, f)
            }
        }
        Expr::NthRoot { degree, radicand } => {
            if let Some(d) = degree {
                visit_ids(d, f);
            }
            visit_ids(radicand, f);
        }
        Expr::Parens(inner) => visit_ids(inner, f),
        Expr::Define { target, expr } | Expr::GlobalDefine { target, expr } => {
            visit_ids(target, f);
            visit_ids(expr, f);
        }
        Expr::Eval {
            expr,
            unit_override,
        } => {
            visit_ids(expr, f);
            if let Some(u) = unit_override {
                visit_ids(u, f);
            }
        }
        Expr::SymEval {
            expr,
            command,
            result,
        } => {
            visit_ids(expr, f);
            if let Some(c) = command {
                for e in &c.0 {
                    visit_ids(e, f);
                }
            }
            if let Some(r) = result {
                visit_ids(r, f);
            }
        }
        Expr::Scale { value, unit } => {
            visit_ids(value, f);
            visit_ids(unit, f);
        }
        // The editor cannot type a programming region yet, so none is visited.
        Expr::Program(_) => {}
        Expr::Accumulate { from, to, body, .. } => {
            visit_ids(from, f);
            visit_ids(to, f);
            visit_ids(body, f);
        }
        Expr::Fold { operand, .. } => visit_ids(operand, f),
        Expr::Integral {
            lower, upper, body, ..
        } => {
            visit_ids(lower, f);
            visit_ids(upper, f);
            visit_ids(body, f);
        }
        Expr::Derivative { order, body, .. } => {
            if let Some(n) = order {
                visit_ids(n, f);
            }
            visit_ids(body, f);
        }
        Expr::Gradient { vars, body } => {
            for v in vars {
                f(v, false);
            }
            visit_ids(body, f);
        }
        Expr::Limit { var, target, body } => {
            f(var, false);
            visit_ids(target, f);
            visit_ids(body, f);
        }
    }
}

impl IdContext for EditContext {
    fn classify(&self, name: &str, called: bool) -> IdKind {
        if called {
            self.calls.get(name).copied().unwrap_or(IdKind::Function)
        } else if let Some(kind) = self.kinds.get(name) {
            *kind
        } else if units::unit(name).is_some() {
            IdKind::Unit
        } else {
            IdKind::Variable
        }
    }

    fn classify_subscripted(&self, _name: &str) -> IdKind {
        IdKind::Variable
    }
}

/// Reads Mathcad text into an expression, with the identifier kinds of a document.
///
/// # Errors
/// The parser's error, with the byte position in `text`.
pub fn parse_math_in(text: &str, ctx: &EditContext) -> Result<Expr, ParseError> {
    parser::parse(text, ctx)
}

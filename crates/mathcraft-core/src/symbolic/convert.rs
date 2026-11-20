//! Conversion between [`Expr`] and the canonical [`Sym`] tree.
//!
//! [`from_expr`] accepts the node kinds the Prime samples use and returns
//! `SymError::Unsupported` for everything else (never a panic). Decimal literals are converted
//! from their text, so `0.5` is exactly `1/2`. Nesting deeper than [`MAX_DEPTH`] is refused.

use crate::expr::{BinaryOp, Expr, Id, IdKind, Number, UnaryOp};

use super::linear::Equation;
use num_traits::{Signed, Zero};

use super::num::{self, half, int, parse_decimal, zero};
use super::sym::Sym;
use super::{SymError, MAX_DEPTH};

/// Converts an expression to its canonical symbolic value.
pub fn from_expr(e: &Expr) -> Result<Sym, SymError> {
    let _scope = super::budget::Scope::enter();
    conv(e, 0)
}

/// Converts `lhs == rhs` to an [`Equation`]; parentheses around it are accepted.
pub fn equation_from_expr(e: &Expr) -> Result<Equation, SymError> {
    let _scope = super::budget::Scope::enter();
    eq_at(e, 0)
}

/// Converts a column (or any matrix) of equations to a system. A single equation is a
/// system of one.
pub fn system_from_expr(e: &Expr) -> Result<Vec<Equation>, SymError> {
    match e {
        Expr::Matrix { elements, .. } => elements.iter().map(equation_from_expr).collect(),
        other => Ok(vec![equation_from_expr(other)?]),
    }
}

/// Prints a solution vector as a column matrix, in the given order.
pub fn solution_to_expr(values: &[Sym]) -> Expr {
    let elements: Vec<Expr> = values.iter().map(to_expr).collect();
    Expr::matrix(elements.len(), 1, elements)
}

/// Converts a canonical value back to an expression (for printing).
pub fn to_expr(s: &Sym) -> Expr {
    match s {
        Sym::Num(q) => Expr::Number(Number::Rational(q.clone())),
        Sym::Symbol(n) => Expr::Id(Id::new(n, IdKind::Variable)),
        Sym::Unit(n) => Expr::Id(Id::new(n, IdKind::Unit)),
        Sym::Func(n, args) if n == "exp" && args.len() == 1 => Expr::binary(
            BinaryOp::Pow,
            Expr::Id(Id::new("e", IdKind::Constant)),
            to_expr(&args[0]),
        ),
        Sym::Func(n, args) => Expr::call(n, args.iter().map(to_expr).collect()),
        Sym::Add(ts) => {
            let mut acc: Option<Expr> = None;
            for t in ts {
                acc = Some(match (acc.take(), t.negated_term()) {
                    (None, _) => to_expr(t),
                    (Some(a), Some(pos)) => Expr::binary(BinaryOp::Sub, a, to_expr(&pos)),
                    (Some(a), None) => Expr::binary(BinaryOp::Add, a, to_expr(t)),
                });
            }
            acc.unwrap_or_else(|| Expr::Number(Number::Rational(zero())))
        }
        Sym::Mul(fs) => {
            if let Some(Sym::Num(q)) = fs.first() {
                if *q == int(-1) && fs.len() > 1 {
                    let rest = Sym::Mul(fs[1..].to_vec());
                    let inner = if fs.len() == 2 { fs[1].clone() } else { rest };
                    return Expr::unary(UnaryOp::Neg, to_expr(&inner));
                }
            }
            let mut it = fs.iter();
            let first = match it.next() {
                Some(f) => to_expr(f),
                None => return Expr::Number(Number::Rational(int(1))),
            };
            it.fold(first, |acc, f| Expr::binary(BinaryOp::Mul, acc, to_expr(f)))
        }
        Sym::Pow(b, e) => {
            if *e == half() {
                return Expr::sqrt(to_expr(b));
            }
            if e.is_negative() {
                let pos = Sym::Pow(b.clone(), -e.clone());
                return Expr::binary(
                    BinaryOp::Div,
                    Expr::Number(Number::Rational(int(1))),
                    to_expr(&pos),
                );
            }
            Expr::binary(
                BinaryOp::Pow,
                to_expr(b),
                Expr::Number(Number::Rational(e.clone())),
            )
        }
    }
}

fn kind_label(e: &Expr) -> &'static str {
    match e {
        Expr::Number(_) => "number",
        Expr::Id(_) => "identifier",
        Expr::Unary { .. } => "unary operator",
        Expr::Binary { .. } => "binary operator",
        Expr::Call { .. } => "call",
        Expr::Range { .. } => "range",
        Expr::Matrix { .. } => "matrix",
        Expr::Index { .. } => "index",
        Expr::NthRoot { .. } => "root",
        Expr::Parens(_) => "parentheses",
        Expr::Define { .. } => "definition",
        Expr::GlobalDefine { .. } => "global definition",
        Expr::Eval { .. } => "numeric evaluation with unit",
        Expr::SymEval { .. } => "symbolic evaluation",
        Expr::Scale { .. } => "scale",
        Expr::Program(_) => "programming region",
        Expr::Placeholder => "placeholder",
        Expr::Str(_) => "string",
        Expr::Accumulate {
            op: crate::expr::Accumulation::Sum,
            ..
        } => "summation",
        Expr::Accumulate { .. } => "product",
        Expr::Fold { .. } => "vector summation or product",
        Expr::Integral { .. } => "integral",
        Expr::Derivative { .. } => "derivative",
        Expr::Gradient { .. } => "gradient",
        Expr::Limit { .. } => "limit",
    }
}

fn conv(e: &Expr, depth: usize) -> Result<Sym, SymError> {
    if depth > MAX_DEPTH {
        return Err(SymError::TooDeep);
    }
    let d = depth + 1;
    match e {
        Expr::Number(Number::Rational(q)) => {
            num::check_bits(q)?;
            Ok(Sym::Num(q.clone()))
        }
        Expr::Number(Number::Real { text, .. }) => parse_decimal(text)
            .map(Sym::Num)
            .ok_or_else(|| SymError::Unsupported(format!("number text {text}"))),
        Expr::Id(id) => id_sym(id),
        Expr::Parens(inner) => conv(inner, d),
        Expr::Unary { op, operand } => match op {
            UnaryOp::Neg => Sym::product(vec![Sym::int(-1), conv(operand, d)?]),
            UnaryOp::Percent => Sym::product(vec![conv(operand, d)?, Sym::rational(1, 100)?]),
            UnaryOp::Abs => Sym::func("abs", vec![conv(operand, d)?]),
            other => Err(SymError::Unsupported(format!("unary {other:?}"))),
        },
        Expr::Binary { op, lhs, rhs } => binary(*op, lhs, rhs, d),
        Expr::Call { func, args } => {
            if func.subscript.is_some() {
                return Err(SymError::Unsupported("subscripted function".into()));
            }
            let parsed = args
                .iter()
                .map(|a| conv(a, d))
                .collect::<Result<Vec<_>, _>>()?;
            if func.name == "sqrt" && parsed.len() == 1 {
                let mut it = parsed.into_iter();
                let r = it.next().unwrap_or(Sym::int(0));
                return Sym::pow(r, half());
            }
            Sym::func(&func.name, parsed)
        }
        Expr::NthRoot { degree, radicand } => {
            let r = conv(radicand, d)?;
            match degree.as_deref() {
                None | Some(Expr::Placeholder) => Sym::pow(r, half()),
                Some(deg) => match conv(deg, d)? {
                    Sym::Num(q) if !q.is_zero() => Sym::pow(r, int(1) / q),
                    _ => Err(SymError::Unsupported("root with a symbolic degree".into())),
                },
            }
        }
        Expr::Scale { value, unit } => Sym::product(vec![conv(value, d)?, conv(unit, d)?]),
        Expr::Eval {
            expr,
            unit_override: None,
        } => conv(expr, d),
        other => Err(SymError::Unsupported(kind_label(other).to_string())),
    }
}

fn binary(op: BinaryOp, lhs: &Expr, rhs: &Expr, d: usize) -> Result<Sym, SymError> {
    match op {
        BinaryOp::Add => Sym::sum(vec![conv(lhs, d)?, conv(rhs, d)?]),
        BinaryOp::Sub => conv(lhs, d)?.minus(conv(rhs, d)?),
        BinaryOp::Mul => conv(lhs, d)?.times(conv(rhs, d)?),
        BinaryOp::Div => conv(lhs, d)?.over(conv(rhs, d)?),
        BinaryOp::Pow => {
            // The constant e is exp(1) (see `id_sym`): e^u is exp(u) for any exponent. A variable
            // named e (a differentiation or solve unknown) keeps its power for a number exponent,
            // and e^u with a symbolic exponent is exp(u) as well.
            let base_is_e = matches!(lhs, Expr::Id(id)
                if id.name == "e" && id.subscript.is_none() && id.kind != IdKind::Unit);
            let base_is_constant_e = matches!(lhs, Expr::Id(id)
                if id.name == "e" && id.subscript.is_none() && id.kind == IdKind::Constant);
            match conv(rhs, d)? {
                Sym::Num(q) if base_is_constant_e => Ok(Sym::func("exp", vec![Sym::Num(q)])?),
                Sym::Num(q) => Sym::pow(conv(lhs, d)?, q),
                // e^u with a symbolic exponent is exp(u); other bases stay unsupported.
                exponent if base_is_e => Sym::func("exp", vec![exponent]),
                _ => Err(SymError::Unsupported("symbolic exponent".into())),
            }
        }
        other => Err(SymError::Unsupported(format!("operator {other:?}"))),
    }
}

fn eq_at(e: &Expr, depth: usize) -> Result<Equation, SymError> {
    if depth > MAX_DEPTH {
        return Err(SymError::TooDeep);
    }
    match e {
        Expr::Parens(inner) => eq_at(inner, depth + 1),
        Expr::Binary {
            op: BinaryOp::Eq,
            lhs,
            rhs,
        } => Ok(Equation {
            lhs: conv(lhs, depth + 1)?,
            rhs: conv(rhs, depth + 1)?,
        }),
        _ => Err(SymError::Unsupported("not an equation".into())),
    }
}

fn id_sym(id: &Id) -> Result<Sym, SymError> {
    let name = match &id.subscript {
        None => id.name.clone(),
        Some(sub) => format!("{}_{}", id.name, sub),
    };
    match id.kind {
        IdKind::Unit => Ok(Sym::Unit(name)),
        // The constant e is exp(1), so that a result which holds it is a number, not a free name.
        IdKind::Constant if name == "e" => Ok(Sym::func("exp", vec![Sym::int(1)])?),
        IdKind::Variable | IdKind::Constant => Ok(Sym::Symbol(name)),
        IdKind::Function => Err(SymError::Unsupported("bare function name".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_decimal_from_text() {
        let e = Expr::Number(Number::Real {
            value: 0.5,
            text: "0.5".into(),
        });
        assert_eq!(from_expr(&e), Ok(Sym::Num(int(1) / int(2))));
    }
}

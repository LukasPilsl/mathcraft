//! The `rectangular` keyword (help: `about_keywords`, "Returns results involving complex numbers
//! separated into real and imaginary parts"): an expression built from numbers, the imaginary unit
//! `i` or `j`, `pi`, worksheet values, `+ - * /`, integer and rational powers, roots, `|z|`, and
//! `e^(z)` with `z = i q pi`, is evaluated exactly as `re + im*i` by [`crate::symbolic::Cplx`].
//!
//! The imaginary unit is written `i` in the answer (the worksheet's `j` setting is not read yet).
//! Anything else is refused: a constant other than `pi`, a real power of `e`, a complex exponent
//! that is not a rational number, or a principal value outside the exact table of the core.

use super::*;
use crate::symbolic::{euler, Cplx};

/// The answer of `expr` as `re + im*i`, exact. A value with no imaginary part is its real value.
pub(super) fn answer(expr: &Expr, scope: &Scope<'_>, budget: &Budget) -> Result<Answer, EvalError> {
    let z = complex_of(expr, scope, budget, scope.nodes)?;
    let re = core(simplify(&z.re))?;
    let im = core(simplify(&z.im))?;
    if im.is_zero() {
        return Ok(Answer::One(re));
    }
    let imaginary = core(Sym::product(vec![im, Sym::symbol("i")]))?;
    let total = if re.is_zero() {
        imaginary
    } else {
        core(Sym::sum(vec![re, imaginary]))?
    };
    Ok(Answer::One(core(simplify(&total))?))
}

/// Whether `e` names the imaginary unit: `i` or `j` that the worksheet does not define.
fn is_imaginary_unit(e: &Expr, scope: &Scope<'_>) -> bool {
    match e {
        Expr::Id(id) => {
            id.subscript.is_none()
                && id.kind != IdKind::Unit
                && (id.name == "i" || id.name == "j")
                && scope.env.variable(&id.name).is_none()
        }
        _ => false,
    }
}

/// Whether `e` is the constant `e` (not a worksheet value).
fn is_constant_e(e: &Expr, scope: &Scope<'_>) -> bool {
    match e {
        Expr::Id(id) => {
            id.subscript.is_none()
                && id.kind != IdKind::Unit
                && id.name == "e"
                && scope.env.variable("e").is_none()
        }
        _ => false,
    }
}

/// The exact complex value of `e`.
fn complex_of(
    e: &Expr,
    scope: &Scope<'_>,
    budget: &Budget,
    nodes: usize,
) -> Result<Cplx, EvalError> {
    if nodes > MAX_NODE_DEPTH {
        return Err(EvalError::DepthLimit(MAX_NODE_DEPTH));
    }
    budget.charge(NODE_UNITS)?;
    let next = nodes + 1;
    if is_imaginary_unit(e, scope) {
        return Ok(Cplx::i());
    }
    match e {
        Expr::Parens(inner) => complex_of(inner, scope, budget, next),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => {
            let z = complex_of(operand, scope, budget, next)?;
            core(Cplx::real(Sym::int(0)).sub(&z))
        }
        Expr::Unary {
            op: UnaryOp::Abs,
            operand,
        } => {
            let z = complex_of(operand, scope, budget, next)?;
            Ok(Cplx::real(core(z.modulus())?))
        }
        Expr::Binary { op, lhs, rhs } => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                let a = complex_of(lhs, scope, budget, next)?;
                let b = complex_of(rhs, scope, budget, next)?;
                core(match op {
                    BinaryOp::Add => a.add(&b),
                    BinaryOp::Sub => a.sub(&b),
                    BinaryOp::Mul => a.mul(&b),
                    _ => a.div(&b),
                })
            }
            BinaryOp::Pow => power_of(lhs, rhs, scope, budget, next),
            _ => Err(unsupported("operator in a rectangular result")),
        },
        Expr::NthRoot { degree, radicand } => {
            let n = match degree.as_deref() {
                None => 2,
                Some(d) => integer_literal(d)
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| unsupported("root of this degree"))?,
            };
            let base = complex_of(radicand, scope, budget, next)?;
            core(base.pow_rational(&rational(1, n)?))
        }
        Expr::Id(id)
            if id.name == "pi" && id.subscript.is_none() && scope.env.variable("pi").is_none() =>
        {
            Ok(Cplx::real(Sym::symbol("pi")))
        }
        Expr::Number(_) => Ok(Cplx::real(real_leaf(e, scope, budget, nodes)?)),
        Expr::Id(_) => Ok(Cplx::real(real_leaf(e, scope, budget, nodes)?)),
        _ => Err(unsupported("expression in a rectangular result")),
    }
}

/// `base ^ exponent` for the forms the core reads exactly.
fn power_of(
    lhs: &Expr,
    rhs: &Expr,
    scope: &Scope<'_>,
    budget: &Budget,
    nodes: usize,
) -> Result<Cplx, EvalError> {
    if is_constant_e(lhs, scope) {
        // e^z is exact only when z = i q pi (Euler's form).
        let z = complex_of(rhs, scope, budget, nodes)?;
        let q = euler_coefficient(&z)?;
        return core(euler(&q));
    }
    let base = complex_of(lhs, scope, budget, nodes)?;
    let exponent = complex_of(rhs, scope, budget, nodes)?;
    if !exponent.im.is_zero() {
        return Err(unsupported("complex exponent in a rectangular result"));
    }
    match &exponent.re {
        Sym::Num(q) if q.is_integer() => {
            let n = i64::try_from(q.to_integer()).map_err(|_| unsupported("power out of range"))?;
            core(base.pow_int(n))
        }
        Sym::Num(q) => core(base.pow_rational(q)),
        _ => Err(unsupported("symbolic exponent in a rectangular result")),
    }
}

/// `q` when `z = i q pi` exactly (`z` has no real part and an imaginary part that is a rational
/// multiple of `pi`).
fn euler_coefficient(z: &Cplx) -> Result<crate::symbolic::Q, EvalError> {
    if !z.re.is_zero() {
        return Err(unsupported("e to a power with a real part"));
    }
    let im = core(simplify(&z.im))?;
    let one_pi = Sym::symbol("pi");
    if im.is_zero() {
        return rational(0, 1);
    }
    if im == one_pi {
        return rational(1, 1);
    }
    match &im {
        Sym::Mul(fs) => match fs.as_slice() {
            [Sym::Num(q), s] if *s == one_pi => Ok(q.clone()),
            _ => Err(unsupported("e to a power that is not i q pi")),
        },
        _ => Err(unsupported("e to a power that is not i q pi")),
    }
}

/// A real leaf: a number, a worksheet value, or a name left free (which the result keeps as a
/// symbol). A constant other than `pi` is inexact here and refused.
fn real_leaf(e: &Expr, scope: &Scope<'_>, budget: &Budget, nodes: usize) -> Result<Sym, EvalError> {
    if let Expr::Id(id) = e {
        let worksheet = scope.env.variable(&id.name).is_some();
        if !worksheet && constant(&id.name).is_some() {
            return Err(unsupported("inexact constant in a rectangular result"));
        }
    }
    let prepared = prepare(e, scope, &[], budget, nodes)?;
    core(from_expr(&prepared))
}

/// `p/q` as a rational.
fn rational(p: i64, q: i64) -> Result<crate::symbolic::Q, EvalError> {
    format!("{p}/{q}")
        .parse()
        .map_err(|_| unsupported("rational exponent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{Outcome, Session};
    use crate::symbolic::{from_expr, to_f64};

    fn text(src: &str) -> Expr {
        crate::parser::parse(src, &crate::parser::DefaultIdContext).expect("parses")
    }

    fn rectangular(src: &str) -> Outcome {
        Session::new().run(&Expr::SymEval {
            expr: Box::new(text(src)),
            command: Some(crate::expr::Command(vec![Expr::var("rectangular")])),
            result: None,
        })
    }

    fn s_of(src: &str) -> crate::symbolic::Sym {
        from_expr(&text(src)).expect("converts")
    }

    /// The symbolic value of a rectangular result, or the number it evaluates to.
    fn symbolic_entry(src: &str) -> crate::symbolic::Sym {
        match rectangular(src) {
            Outcome::Value(Value::Symbolic(s)) => s.entries[0].clone(),
            other => panic!("{src}: {other:?}"),
        }
    }

    fn real(src: &str) -> f64 {
        match rectangular(src) {
            Outcome::Value(Value::Scalar(s)) => {
                assert!(s.value.im.abs() < 1e-15, "{src}: {s:?}");
                s.value.re
            }
            other => panic!("{src}: {other:?}"),
        }
    }

    #[test]
    fn square_of_one_plus_i_is_two_i() {
        assert_eq!(symbolic_entry("(1+i)^2"), s_of("2*i"));
    }

    #[test]
    fn square_root_of_minus_four_is_two_i() {
        assert_eq!(symbolic_entry("sqrt(-4)"), s_of("2*i"));
    }

    #[test]
    fn euler_form_of_e_to_i_pi_is_minus_one() {
        assert_eq!(real("e^(i*pi)"), -1.0);
    }

    #[test]
    fn quotient_and_products_are_exact() {
        // (1+i)/(1-i) = i and (1+2i)(3-i) = 5 + 5i.
        assert_eq!(symbolic_entry("(1+i)/(1-i)"), s_of("i"));
        assert_eq!(symbolic_entry("(1+2*i)*(3-i)"), s_of("5+5*i"));
    }

    #[test]
    fn principal_power_of_minus_one_matches_the_help() {
        // help (complex_results): (-1)^(1/3) is 1/2 + sqrt(3)/2 i.
        assert_eq!(symbolic_entry("(-1)^(1/3)"), s_of("1/2+sqrt(3)/2*i"));
    }

    #[test]
    fn unsupported_forms_are_refused() {
        // e to a real power is not exact here; a constant other than pi is inexact.
        assert!(matches!(
            rectangular("e^2"),
            Outcome::Error(EvalError::Unsupported(_))
        ));
        assert!(matches!(
            rectangular("e^(i*0.5)"),
            Outcome::Error(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn complex_is_the_same_command_as_rectangular() {
        let out = Session::new().run(&Expr::SymEval {
            expr: Box::new(text("(1+i)^2")),
            command: Some(crate::expr::Command(vec![Expr::var("complex")])),
            result: None,
        });
        match out {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("2*i")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn complex_roots_of_a_quadratic_are_i_and_minus_i() {
        // the complex roots of a numeric quadratic are returned with the imaginary unit `i`
        let out = Session::new().run(&Expr::SymEval {
            expr: Box::new(Expr::binary(
                crate::expr::BinaryOp::Eq,
                text("x^2+1"),
                Expr::num(0.0),
            )),
            command: Some(crate::expr::Command(vec![
                Expr::var("solve"),
                Expr::var("x"),
            ])),
            result: None,
        });
        match out {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries.len(), 2);
                assert_eq!(s.entries[0], Sym::symbol("i"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// The identities hold exactly for random small Gaussian integers, and numerically
    /// (|z|^2 = z conj(z)) to 1e-9.
    #[test]
    fn complex_identities_hold_on_random_gaussian_integers() {
        use crate::symbolic::Cplx;
        let mut seed: i64 = 11;
        for _ in 0..40 {
            seed = (seed * 1103 + 12345) % 1000;
            let a = seed % 11 - 5;
            seed = (seed * 1103 + 12345) % 1000;
            let b = seed % 11 - 5;
            let z = Cplx::new(Sym::int(a), Sym::int(b)).expect("z");
            let w = Cplx::new(Sym::int(b + 7), Sym::int(a - 3)).expect("w");
            if (w.re.is_zero() && w.im.is_zero()) || (z.re.is_zero() && z.im.is_zero()) {
                continue;
            }
            // (z w) / w = z exactly.
            let back = z.mul(&w).expect("mul").div(&w).expect("div");
            assert_eq!(back, z, "z={a}+{b}i");
            // z^2 z^-2 = 1 exactly.
            let one = z
                .pow_int(2)
                .expect("p")
                .mul(&z.pow_int(-2).expect("p"))
                .expect("m");
            assert_eq!(one, Cplx::real(Sym::int(1)), "z={a}+{b}i");
            // |z|^2 = z conj(z) numerically.
            let n2 = to_f64(z.norm_sq().expect("n").as_num().expect("num"));
            let prod = z.mul(&z.conj().expect("c")).expect("m");
            let p = to_f64(prod.re.as_num().expect("num"));
            assert!((n2 - p).abs() < 1e-9, "z={a}+{b}i");
        }
    }
}

//! Equations in one unknown that are polynomial in an exponential or a logarithm of it.
//!
//! - `exp`: every occurrence of the unknown is inside `exp(a x)` with a rational `a`, and every
//!   `a` is an integer multiple of the smallest one, `a0`. With `u = exp(a0 x)` the equation is a
//!   Laurent polynomial in `u` with rational coefficients. Each root `r` must be real and positive
//!   (a root `r <= 0` has only complex solutions, which are not computed here, so the whole solve
//!   is refused), and gives `x = ln(r)/a0`.
//! - `ln`: one `ln(a x + b)` with rational `a`, `b`; with `w = ln(a x + b)` the equation is a
//!   polynomial in `w`; each real root gives `x = (exp(w) - b)/a`.
//!
//! `sin`, `cos`, `tan` of `a x + b` equal to a rational: the principal solution (see
//! [`solve_trig`]); the help says a periodic solution set is answered by a single value.
//!
//! Every root is substituted back into the equation and must give zero numerically (to 1e-7)
//! before it is returned; one that does not makes the whole solve `Unsupported`.

use num_complex::Complex64;
use num_traits::{Signed, ToPrimitive};

use super::linear::Equation;
use super::num::{int, Q};
use super::numcheck::eval;
use super::poly::{expand, Poly};
use super::poly_solve::solve_polynomial;
use super::sym::Sym;
use super::{budget, SymError};

/// The placeholder for the exponential or logarithm (a name no worksheet uses).
const PLACEHOLDER: &str = "u__trans";

fn unsupported(text: &str) -> SymError {
    SymError::Unsupported(text.into())
}

/// Solutions of `eq` in `var` when it is polynomial in an exponential or a logarithm of `var`.
pub fn solve_transcendental(eq: &Equation, var: &str) -> Result<Vec<Sym>, SymError> {
    let _scope = budget::Scope::enter();
    let diff = expand(&eq.lhs.clone().minus(eq.rhs.clone())?)?;
    let mut found: Vec<(String, Sym)> = Vec::new();
    discover(&diff, var, &mut found)?;
    let Some((name, _)) = found.first() else {
        return Err(unsupported("no function of the unknown"));
    };
    if found.iter().any(|(n, _)| n != name) {
        return Err(unsupported("mixed functions of the unknown"));
    }
    let roots = match name.as_str() {
        "exp" => solve_exp(&diff, var, &found)?,
        "ln" => solve_ln(&diff, var, &found)?,
        "sin" | "cos" | "tan" => solve_trig(&diff, var, &found)?,
        _ => return Err(unsupported("equation in this function of the unknown")),
    };
    for r in &roots {
        let at = super::poly::substitute(&diff, &[(var, r.clone())])?;
        let pi = |n: &str| (n == "pi").then(|| Complex64::new(std::f64::consts::PI, 0.0));
        let value = eval(&at, &pi).ok_or_else(|| unsupported("root check not evaluable"))?;
        if !value.is_finite() || value.norm() > 1e-7 {
            return Err(unsupported("a root failed the numeric check"));
        }
    }
    Ok(roots)
}

/// Collects the distinct `(function, argument)` nodes that mention `var`; any occurrence of `var`
/// outside such a node, or a node inside another, is refused.
fn discover(s: &Sym, var: &str, out: &mut Vec<(String, Sym)>) -> Result<(), SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Unit(_) => Ok(()),
        Sym::Symbol(n) => {
            if n == var {
                Err(unsupported("unknown outside an exponential or logarithm"))
            } else {
                Ok(())
            }
        }
        Sym::Func(name, args) => {
            if !s.mentions_any(&[var.to_string()]) {
                return Ok(());
            }
            if args.len() != 1 || has_func_of(&args[0], var) {
                return Err(unsupported("nested function of the unknown"));
            }
            let entry = (name.clone(), args[0].clone());
            if !out.contains(&entry) {
                out.push(entry);
            }
            Ok(())
        }
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().try_for_each(|t| discover(t, var, out)),
        Sym::Pow(b, _) => discover(b, var, out),
    }
}

fn has_func_of(s: &Sym, var: &str) -> bool {
    match s {
        Sym::Func(..) => s.mentions_any(&[var.to_string()]),
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().any(|t| has_func_of(t, var)),
        Sym::Pow(b, _) => has_func_of(b, var),
        _ => false,
    }
}

/// `(a, b)` with `arg = a*var + b`, `a` a nonzero rational and `b` a rational.
fn linear_rational(arg: &Sym, var: &str) -> Result<(Q, Q), SymError> {
    let x = Sym::symbol(var);
    let poly = Poly::from_sym(arg)?;
    let cs = poly.coeffs_in(&x)?;
    if cs.len() != 2 {
        return Err(unsupported("argument is not linear in the unknown"));
    }
    let number = |p: &Poly| -> Result<Q, SymError> {
        match p.to_sym()? {
            Sym::Num(q) => Ok(q),
            _ => Err(unsupported("argument with a symbolic coefficient")),
        }
    };
    Ok((number(&cs[1])?, number(&cs[0])?))
}

/// Replaces every function node that mentions `var` using `make`, rebuilding canonically.
fn replace(
    s: &Sym,
    var: &str,
    make: &dyn Fn(&str, &Sym) -> Result<Sym, SymError>,
) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            if s.mentions_any(&[var.to_string()]) {
                make(name, &args[0])
            } else {
                Ok(s.clone())
            }
        }
        Sym::Add(ts) => Sym::sum(
            ts.iter()
                .map(|t| replace(t, var, make))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Mul(ts) => Sym::product(
            ts.iter()
                .map(|t| replace(t, var, make))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Pow(b, e) => Sym::pow(replace(b, var, make)?, e.clone()),
    }
}

/// The real value of a closed expression, or `None`.
fn real_value(s: &Sym) -> Option<f64> {
    let v: Complex64 = eval(s, &|_| None)?;
    if v.is_finite() && v.im.abs() <= 1e-12 * (1.0 + v.re.abs()) {
        Some(v.re)
    } else {
        None
    }
}

/// Roots of the polynomial `p(u)` (a Sym in the placeholder with rational coefficients).
fn roots_of(p: &Sym) -> Result<Vec<Sym>, SymError> {
    let poly = Poly::from_sym(p)?;
    let u = Sym::symbol(PLACEHOLDER);
    for c in poly.coeffs_in(&u)? {
        if !matches!(c.to_sym()?, Sym::Num(_)) {
            return Err(unsupported("equation with symbolic coefficients"));
        }
    }
    solve_polynomial(
        &Equation {
            lhs: p.clone(),
            rhs: Sym::int(0),
        },
        PLACEHOLDER,
    )
}

fn solve_exp(diff: &Sym, var: &str, found: &[(String, Sym)]) -> Result<Vec<Sym>, SymError> {
    let mut rates: Vec<Q> = Vec::with_capacity(found.len());
    for (_, arg) in found {
        let (a, b) = linear_rational(arg, var)?;
        if b.numer().to_i64().is_none_or(|n| n != 0) {
            return Err(unsupported("exponential with an offset in its argument"));
        }
        rates.push(a);
    }
    let a0 = rates
        .iter()
        .map(|a| a.abs())
        .min()
        .ok_or_else(|| unsupported("no exponential"))?;
    let mut shift: i64 = 0;
    let mut ratios: Vec<(Q, i64)> = Vec::new();
    for a in &rates {
        let m = a / &a0;
        if !m.is_integer() {
            return Err(unsupported("exponentials with incommensurable rates"));
        }
        let m = m.to_integer().to_i64().filter(|m| m.abs() <= 12);
        let m = m.ok_or_else(|| unsupported("exponential power too large"))?;
        shift = shift.max(-m);
        ratios.push((a.clone(), m));
    }
    let u = Sym::symbol(PLACEHOLDER);
    let lookup = ratios.clone();
    let swapped = replace(diff, var, &|_, arg| {
        let (a, _) = linear_rational(arg, var)?;
        let m = lookup
            .iter()
            .find(|(r, _)| *r == a)
            .map(|(_, m)| *m)
            .ok_or_else(|| unsupported("exponential not listed"))?;
        Sym::pow(u.clone(), int(m))
    })?;
    let cleared = expand(&Sym::product(vec![
        swapped,
        Sym::pow(u.clone(), int(shift))?,
    ])?)?;
    let mut out: Vec<(f64, Sym)> = Vec::new();
    for r in roots_of(&cleared)? {
        let value = real_value(&r).ok_or_else(|| unsupported("complex exponential root"))?;
        if value <= 0.0 {
            return Err(unsupported(
                "exponential equation with a non-positive root (complex solutions)",
            ));
        }
        let ln = if value == 1.0 && r == Sym::int(1) {
            Sym::int(0)
        } else {
            Sym::func("ln", vec![r])?
        };
        let x = ln.over(Sym::Num(a0.clone()))?;
        out.push((real_value(&x).unwrap_or(f64::NAN), x));
    }
    sorted(out)
}

/// `f(a x + b) = c` for `f` one of `sin`, `cos`, `tan`, with rational `a`, `b`, `c`: the
/// principal solution only. The help states that for a periodic solution set `solve` returns a
/// single value of the set (`sin(x) = 0` gives `0`), so this returns `(f^-1(c) - b)/a`, with the
/// inverse function's principal value (`asin`, `acos` in `[0, pi]`, `atan`). The exact angle is
/// given for the rational values that have one; any other `c` keeps the inverse function. `sin`
/// and `cos` with `|c| > 1` have only complex solutions and are refused.
fn solve_trig(diff: &Sym, var: &str, found: &[(String, Sym)]) -> Result<Vec<Sym>, SymError> {
    if found.len() != 1 {
        return Err(unsupported("several trigonometric terms of the unknown"));
    }
    let (name, arg) = &found[0];
    let (a, b) = linear_rational(arg, var)?;
    let w = Sym::symbol(PLACEHOLDER);
    let swapped = expand(&replace(diff, var, &|_, _| Ok(w.clone()))?)?;
    let cs = Poly::from_sym(&swapped)?.coeffs_in(&w)?;
    if cs.len() != 2 {
        return Err(unsupported(
            "trigonometric equation that is not linear in the function",
        ));
    }
    let number = |p: &Poly| -> Result<Q, SymError> {
        match p.to_sym()? {
            Sym::Num(q) => Ok(q),
            _ => Err(unsupported(
                "trigonometric equation with a symbolic coefficient",
            )),
        }
    };
    let (k, m) = (number(&cs[1])?, number(&cs[0])?);
    let c = -m / k;
    let pi = Sym::symbol("pi");
    let angle = |p: i64, q: i64| -> Result<Sym, SymError> {
        if p == 0 {
            Ok(Sym::int(0))
        } else {
            Sym::product(vec![Sym::Num(int(p) / int(q)), pi.clone()])
        }
    };
    let half = int(1) / int(2);
    let principal = match name.as_str() {
        "sin" | "cos" if c.abs() > int(1) => {
            return Err(unsupported(
                "trigonometric equation with only complex solutions",
            ))
        }
        "sin" => match () {
            _ if c == int(0) => angle(0, 1)?,
            _ if c == half => angle(1, 6)?,
            _ if c == -half.clone() => angle(-1, 6)?,
            _ if c == int(1) => angle(1, 2)?,
            _ if c == int(-1) => angle(-1, 2)?,
            _ => Sym::func("asin", vec![Sym::Num(c.clone())])?,
        },
        "cos" => match () {
            _ if c == int(1) => angle(0, 1)?,
            _ if c == half => angle(1, 3)?,
            _ if c == int(0) => angle(1, 2)?,
            _ if c == -half.clone() => angle(2, 3)?,
            _ if c == int(-1) => angle(1, 1)?,
            _ => Sym::func("acos", vec![Sym::Num(c.clone())])?,
        },
        "tan" => match () {
            _ if c == int(0) => angle(0, 1)?,
            _ if c == int(1) => angle(1, 4)?,
            _ if c == int(-1) => angle(-1, 4)?,
            _ => Sym::func("atan", vec![Sym::Num(c.clone())])?,
        },
        _ => return Err(unsupported("equation in this function of the unknown")),
    };
    let x = principal.minus(Sym::Num(b))?.over(Sym::Num(a))?;
    Ok(vec![x])
}

fn solve_ln(diff: &Sym, var: &str, found: &[(String, Sym)]) -> Result<Vec<Sym>, SymError> {
    if found.len() != 1 {
        return Err(unsupported("several logarithms of the unknown"));
    }
    let (a, b) = linear_rational(&found[0].1, var)?;
    let w = Sym::symbol(PLACEHOLDER);
    let swapped = replace(diff, var, &|_, _| Ok(w.clone()))?;
    let cleared = expand(&swapped)?;
    let mut out: Vec<(f64, Sym)> = Vec::new();
    for r in roots_of(&cleared)? {
        real_value(&r).ok_or_else(|| unsupported("complex logarithm root"))?;
        let e = if r.is_zero() {
            Sym::int(1)
        } else {
            Sym::func("exp", vec![r])?
        };
        let x = e.minus(Sym::Num(b.clone()))?.over(Sym::Num(a.clone()))?;
        out.push((real_value(&x).unwrap_or(f64::NAN), x));
    }
    sorted(out)
}

fn sorted(mut v: Vec<(f64, Sym)>) -> Result<Vec<Sym>, SymError> {
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    v.dedup_by(|a, b| a.1 == b.1);
    Ok(v.into_iter().map(|(_, s)| s).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::equation_from_expr;

    fn eq(src: &str) -> Equation {
        let (l, r) = src.split_once('=').expect("an equation");
        let parse =
            |t: &str| crate::parser::parse(t, &crate::parser::DefaultIdContext).expect("parses");
        equation_from_expr(&crate::expr::Expr::binary(
            crate::expr::BinaryOp::Eq,
            parse(l),
            parse(r),
        ))
        .expect("equation")
    }

    fn s(src: &str) -> Sym {
        crate::symbolic::from_expr(
            &crate::parser::parse(src, &crate::parser::DefaultIdContext).expect("parses"),
        )
        .expect("converts")
    }

    #[test]
    fn exponential_and_logarithmic_equations() {
        assert_eq!(
            solve_transcendental(&eq("exp(x)=5"), "x").expect("e^x=5"),
            vec![s("ln(5)")]
        );
        // e^(2x) - 3 e^x + 2 = 0: x = 0 and ln 2.
        assert_eq!(
            solve_transcendental(&eq("exp(2*x)-3*exp(x)+2=0"), "x").expect("quadratic in e^x"),
            vec![Sym::int(0), s("ln(2)")]
        );
        assert_eq!(
            solve_transcendental(&eq("exp(2*x)=8"), "x").expect("rate 2"),
            vec![s("ln(8)/2")]
        );
        // ln(2 x + 1) = 2: x = (e^2 - 1)/2.
        assert_eq!(
            solve_transcendental(&eq("ln(2*x+1)=2"), "x").expect("ln"),
            vec![s("(exp(2)-1)/2")]
        );
    }

    #[test]
    fn trigonometric_equations_give_the_principal_value() {
        assert_eq!(
            solve_transcendental(&eq("sin(x)=0"), "x").expect("sin"),
            vec![Sym::int(0)]
        );
        assert_eq!(
            solve_transcendental(&eq("sin(x)=1/2"), "x").expect("sin"),
            vec![s("pi/6")]
        );
        assert_eq!(
            solve_transcendental(&eq("cos(x)=1/2"), "x").expect("cos"),
            vec![s("pi/3")]
        );
        assert_eq!(
            solve_transcendental(&eq("cos(x)=-1/2"), "x").expect("cos"),
            vec![s("2*pi/3")]
        );
        assert_eq!(
            solve_transcendental(&eq("tan(x)=1"), "x").expect("tan"),
            vec![s("pi/4")]
        );
        // an argument a x + b and a scaled function: 2 sin(2x + 1) = 1 -> (pi/6 - 1)/2
        assert_eq!(
            solve_transcendental(&eq("2*sin(2*x+1)=1"), "x").expect("scaled"),
            vec![s("(pi/6-1)/2")]
        );
        // no exact angle: the inverse function stays
        assert_eq!(
            solve_transcendental(&eq("sin(x)=1/3"), "x").expect("asin"),
            vec![s("asin(1/3)")]
        );
        assert_eq!(
            solve_transcendental(&eq("tan(x)=3"), "x").expect("atan"),
            vec![s("atan(3)")]
        );
    }

    #[test]
    fn refusals() {
        // Complex solutions only: e^x = -1; x outside the exponential; mixed or nonlinear trig.
        for src in [
            "exp(x)=-1",
            "x*exp(x)=1",
            "exp(x)=sin(x)",
            "sin(x)=2",
            "cos(x)=-3/2",
            "sin(x)^2=1/4",
            "sin(x)=cos(x)",
            "sin(x)+x=0",
        ] {
            assert!(solve_transcendental(&eq(src), "x").is_err(), "{src}");
        }
    }
}

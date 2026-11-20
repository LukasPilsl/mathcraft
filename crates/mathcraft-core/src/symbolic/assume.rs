//! Assumptions on variables (`assume`) and the rewrites they license.
//!
//! Help (`making_assumptions_about_variables`): `x>a`, `x>=a`, `x<b`, `x<=b`, `x=real`,
//! `x=complex`, `x=integer`, `n=even`, `n=odd`, and `ALL` for every variable. By default every
//! variable is real. An [`Assumptions`] value belongs to one symbolic command: it is built from
//! that command's words and dropped with it, so nothing leaks into the next region.
//!
//! Only bounds at zero are supported (`x>0`, `x>=0`, `x<0`, `x<=0`); other bounds are refused by
//! the caller. `integer`, `even` and `odd` are accepted and used by no rule yet: a weaker
//! assumption gives a result that is equal but less simplified, never a wrong one.
//!
//! Rules (all exact):
//! - `(x^n)^p` with integer `n`, rational non-integer `p`:
//!   - `x > 0` or `x >= 0`: `x^(n p)`.
//!   - real `x` of unknown sign, `p = 1/2`, `n = 2k` even: `|x|^k`, printed `x^k` when `k` is
//!     even and `abs(x)^k` when `k` is odd (`sqrt(x^2) -> abs(x)`).
//!   - `x < 0` or `x <= 0`, `p = 1/2`, `n = 2k`: `(-x)^k`.
//! - `abs(x)`: `x` when `x >= 0`, `-x` when `x <= 0`.
//! - `ln(f)` where every factor of `f` is positive: the sum of the logs of the factors, and
//!   `ln(b^q)` becomes `q ln(b)` for a positive base `b`.
//!
//! Complex variables (`x=complex`) never get the real-only rules (`abs`, the square-root rule).

use std::collections::BTreeMap;

use num_traits::Signed;

use super::canon::to_i64;
use super::num::{int, one, Q};
use super::sym::Sym;
use super::{budget, SymError};

/// What is known about one variable: its sign and whether it may be complex.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Knowledge {
    /// `x >= 0` (also set by `x > 0`).
    pub ge0: bool,
    /// `x > 0`.
    pub gt0: bool,
    /// `x <= 0` (also set by `x < 0`).
    pub le0: bool,
    /// `x < 0`.
    pub lt0: bool,
    /// `x=real` was given explicitly.
    pub real: bool,
    /// `x=complex` was given.
    pub complex: bool,
}

impl Knowledge {
    /// Both facts hold: the conjunction of two assumptions on one variable. A contradiction
    /// (`x > 0` with `x < 0`, or `real` with `complex`) is refused.
    fn and(self, other: Knowledge) -> Result<Knowledge, SymError> {
        let k = Knowledge {
            ge0: self.ge0 || other.ge0,
            gt0: self.gt0 || other.gt0,
            le0: self.le0 || other.le0,
            lt0: self.lt0 || other.lt0,
            real: self.real || other.real,
            complex: self.complex || other.complex,
        };
        let positive = k.gt0 || k.ge0;
        let negative = k.lt0 || k.le0;
        if (positive && negative) || (k.real && k.complex) {
            return Err(SymError::Invalid("contradictory assumptions".into()));
        }
        if k.complex && (positive || negative) {
            return Err(SymError::Unsupported(
                "an inequality on a complex variable".into(),
            ));
        }
        Ok(k)
    }

    /// `x >= 0`.
    pub fn nonnegative(&self) -> bool {
        self.gt0 || self.ge0
    }

    /// `x <= 0`.
    pub fn nonpositive(&self) -> bool {
        self.lt0 || self.le0
    }

    /// `x` is known to be real (the default unless `complex` was given).
    pub fn is_real(&self) -> bool {
        !self.complex
    }
}

/// One assumption as written in the command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fact {
    Gt0,
    Ge0,
    Lt0,
    Le0,
    Real,
    Complex,
    /// Accepted; used by no rule (see the module docs).
    Integer,
}

impl Fact {
    fn knowledge(self) -> Knowledge {
        let mut k = Knowledge::default();
        match self {
            Fact::Gt0 => {
                k.gt0 = true;
                k.ge0 = true;
            }
            Fact::Ge0 => k.ge0 = true,
            Fact::Lt0 => {
                k.lt0 = true;
                k.le0 = true;
            }
            Fact::Le0 => k.le0 = true,
            Fact::Real | Fact::Integer => k.real = true,
            Fact::Complex => k.complex = true,
        }
        k
    }
}

/// The assumptions of one command: facts on named variables and facts on `ALL`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assumptions {
    all: Knowledge,
    names: BTreeMap<String, Knowledge>,
}

impl Assumptions {
    /// Adds `fact` for `name`, or for every variable when `name` is `None` (`ALL`).
    pub fn add(&mut self, name: Option<&str>, fact: Fact) -> Result<(), SymError> {
        let k = fact.knowledge();
        match name {
            None => self.all = self.all.and(k)?,
            Some(n) => {
                let current = self.names.get(n).copied().unwrap_or_default();
                self.names.insert(n.to_string(), current.and(k)?);
            }
        }
        // A contradiction between `ALL` and a name (or between two facts on a name) is refused
        // now, not when a rewrite happens to read the name.
        match name {
            None => {
                for n in self.names.keys() {
                    self.knowledge(n)?;
                }
            }
            Some(n) => {
                self.knowledge(n)?;
            }
        }
        Ok(())
    }

    /// What is known about `name`: the facts on `ALL` and on the name, together.
    pub fn knowledge(&self, name: &str) -> Result<Knowledge, SymError> {
        let named = self.names.get(name).copied().unwrap_or_default();
        self.all.and(named)
    }
}

/// Applies the assumptions to a canonical value (one bottom-up pass, no simplification).
pub fn apply(s: &Sym, asm: &Assumptions) -> Result<Sym, SymError> {
    let _scope = budget::Scope::enter();
    rewrite(s, asm)
}

fn rewrite(s: &Sym, asm: &Assumptions) -> Result<Sym, SymError> {
    budget::spend(1)?;
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(name, args) => {
            let args = args
                .iter()
                .map(|a| rewrite(a, asm))
                .collect::<Result<Vec<_>, _>>()?;
            func_rule(name, args, asm)
        }
        Sym::Add(ts) => Sym::sum(
            ts.iter()
                .map(|t| rewrite(t, asm))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Mul(fs) => Sym::product(
            fs.iter()
                .map(|f| rewrite(f, asm))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Sym::Pow(b, e) => {
            let base = rewrite(b, asm)?;
            power_rule(base, e, asm)
        }
    }
}

fn power_rule(base: Sym, e: &Q, asm: &Assumptions) -> Result<Sym, SymError> {
    if let Sym::Pow(inner, n) = &base {
        if let Sym::Symbol(x) = inner.as_ref() {
            if n.is_integer() && !e.is_integer() {
                let k = asm.knowledge(x)?;
                if k.nonnegative() {
                    return Sym::pow(Sym::Symbol(x.clone()), n.clone() * e.clone());
                }
                let half = *e == int(1) / int(2);
                let n_even = to_i64(n).map(|v| v % 2 == 0).unwrap_or(false);
                if half && n_even && k.is_real() {
                    let power = to_i64(n)? / 2;
                    let x_sym = Sym::Symbol(x.clone());
                    if k.nonpositive() {
                        let minus_x = Sym::product(vec![Sym::int(-1), x_sym])?;
                        return Sym::pow(minus_x, int(power));
                    }
                    let magnitude = if power % 2 == 0 {
                        x_sym
                    } else {
                        Sym::func("abs", vec![x_sym])?
                    };
                    return Sym::pow(magnitude, int(power));
                }
            }
        }
    }
    Sym::pow(base, e.clone())
}

fn func_rule(name: &str, args: Vec<Sym>, asm: &Assumptions) -> Result<Sym, SymError> {
    match (name, args.as_slice()) {
        ("abs", [Sym::Symbol(x)]) => {
            let k = asm.knowledge(x)?;
            let arg = Sym::Symbol(x.clone());
            if k.nonnegative() {
                Ok(arg)
            } else if k.nonpositive() {
                Sym::product(vec![Sym::int(-1), arg])
            } else {
                Sym::func(name, args)
            }
        }
        ("ln", [arg]) if all_positive(arg, asm)? && splits(arg) => {
            let parts = match arg {
                Sym::Mul(fs) => fs.clone(),
                other => vec![other.clone()],
            };
            let logs = parts.iter().map(log_of).collect::<Result<Vec<_>, _>>()?;
            Sym::sum(logs)
        }
        _ => Sym::func(name, args),
    }
}

/// `ln` splits only a product of several factors, or a single power of a positive base.
fn splits(arg: &Sym) -> bool {
    match arg {
        Sym::Mul(_) => true,
        Sym::Pow(_, e) => !e.is_integer() || *e != one(),
        _ => false,
    }
}

/// `ln(f)` for a positive factor `f`: powers come out front, other factors stay as `ln(f)`.
fn log_of(f: &Sym) -> Result<Sym, SymError> {
    match f {
        Sym::Pow(b, q) => Sym::product(vec![
            Sym::Num(q.clone()),
            Sym::func("ln", vec![(**b).clone()])?,
        ]),
        other => Sym::func("ln", vec![other.clone()]),
    }
}

/// Whether `s` is positive for every value the assumptions allow. Only symbols with `x > 0`,
/// positive rationals, and powers of those qualify. A product is positive when all factors are.
fn all_positive(s: &Sym, asm: &Assumptions) -> Result<bool, SymError> {
    Ok(match s {
        Sym::Num(q) => q.is_positive(),
        Sym::Symbol(x) => asm.knowledge(x)?.gt0,
        Sym::Mul(fs) => {
            for f in fs {
                if !all_positive(f, asm)? {
                    return Ok(false);
                }
            }
            true
        }
        Sym::Pow(b, _) => all_positive(b, asm)?,
        Sym::Unit(_) | Sym::Func(..) | Sym::Add(_) => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolic::{from_expr, simplify};

    fn sqrt_of_square(var: &str) -> Sym {
        let x2 = Sym::pow(Sym::symbol(var), int(2)).expect("pow");
        Sym::pow(x2, int(1) / int(2)).expect("sqrt")
    }

    #[test]
    fn contradictions_are_refused_when_added() {
        // ALL=complex then x>0: the name's fact contradicts ALL, refused without any rewrite.
        let mut asm = Assumptions::default();
        asm.add(None, Fact::Complex).expect("ALL=complex");
        assert!(asm.add(Some("x"), Fact::Gt0).is_err());
        // Two facts on a name: x=real then x=complex.
        let mut asm = Assumptions::default();
        asm.add(Some("y"), Fact::Real).expect("real");
        assert!(asm.add(Some("y"), Fact::Complex).is_err());
        // x>0 then ALL=complex: the new ALL fact is checked against every name.
        let mut asm = Assumptions::default();
        asm.add(Some("x"), Fact::Gt0).expect("x>0");
        assert!(asm.add(None, Fact::Complex).is_err());
    }

    fn with(facts: &[(Option<&str>, Fact)]) -> Assumptions {
        let mut a = Assumptions::default();
        for (name, fact) in facts {
            a.add(*name, *fact).expect("add");
        }
        a
    }

    #[test]
    fn default_real_square_root_is_abs() {
        let got = apply(&sqrt_of_square("x"), &Assumptions::default()).expect("apply");
        assert_eq!(got, Sym::func("abs", vec![Sym::symbol("x")]).expect("abs"));
    }

    #[test]
    fn positive_square_root_is_the_variable() {
        let asm = with(&[(Some("x"), Fact::Gt0)]);
        assert_eq!(
            apply(&sqrt_of_square("x"), &asm).expect("apply"),
            Sym::symbol("x")
        );
    }

    #[test]
    fn nonnegative_square_root_is_the_variable() {
        let asm = with(&[(Some("x"), Fact::Ge0)]);
        assert_eq!(
            apply(&sqrt_of_square("x"), &asm).expect("apply"),
            Sym::symbol("x")
        );
    }

    #[test]
    fn negative_square_root_is_minus_the_variable() {
        let asm = with(&[(Some("x"), Fact::Lt0)]);
        let got = apply(&sqrt_of_square("x"), &asm).expect("apply");
        assert_eq!(
            got,
            Sym::product(vec![Sym::int(-1), Sym::symbol("x")]).expect("neg")
        );
    }

    #[test]
    fn complex_variable_keeps_the_root() {
        let asm = with(&[(Some("x"), Fact::Complex)]);
        assert_eq!(
            apply(&sqrt_of_square("x"), &asm).expect("apply"),
            sqrt_of_square("x")
        );
    }

    #[test]
    fn all_positive_applies_to_every_name() {
        let asm = with(&[(None, Fact::Gt0)]);
        let got = apply(&sqrt_of_square("a"), &asm).expect("apply");
        assert_eq!(got, Sym::symbol("a"));
    }

    #[test]
    fn contradictory_facts_are_refused() {
        let mut asm = Assumptions::default();
        asm.add(Some("x"), Fact::Gt0).expect("first");
        assert!(asm.add(Some("x"), Fact::Lt0).is_err());
    }

    #[test]
    fn abs_follows_the_sign() {
        let abs_x = Sym::func("abs", vec![Sym::symbol("x")]).expect("abs");
        assert_eq!(
            apply(&abs_x, &with(&[(Some("x"), Fact::Ge0)])).expect("apply"),
            Sym::symbol("x")
        );
        assert_eq!(
            apply(&abs_x, &with(&[(Some("x"), Fact::Le0)])).expect("apply"),
            Sym::product(vec![Sym::int(-1), Sym::symbol("x")]).expect("neg")
        );
        assert_eq!(
            apply(&abs_x, &Assumptions::default()).expect("apply"),
            abs_x
        );
    }

    #[test]
    fn log_of_a_positive_product_splits_under_the_assumption() {
        let ab = Sym::product(vec![Sym::symbol("a"), Sym::symbol("b")]).expect("ab");
        let ln_ab = Sym::func("ln", vec![ab]).expect("ln");
        let asm = with(&[(None, Fact::Gt0)]);
        let got = apply(&ln_ab, &asm).expect("apply");
        let want = Sym::sum(vec![
            Sym::func("ln", vec![Sym::symbol("a")]).expect("ln a"),
            Sym::func("ln", vec![Sym::symbol("b")]).expect("ln b"),
        ])
        .expect("sum");
        assert_eq!(got, want);
        // Without the assumption the product stays inside the logarithm.
        assert_eq!(
            apply(&ln_ab, &Assumptions::default()).expect("apply"),
            ln_ab
        );
    }

    #[test]
    fn log_of_a_power_comes_out_front_only_when_positive() {
        let a2 = Sym::pow(Sym::symbol("a"), int(2)).expect("a2");
        let ln_a2 = Sym::func("ln", vec![a2]).expect("ln");
        let asm = with(&[(Some("a"), Fact::Gt0)]);
        let want = Sym::product(vec![
            Sym::int(2),
            Sym::func("ln", vec![Sym::symbol("a")]).expect("ln a"),
        ])
        .expect("2 ln a");
        assert_eq!(apply(&ln_a2, &asm).expect("apply"), want);
        assert_eq!(
            apply(&ln_a2, &Assumptions::default()).expect("apply"),
            ln_a2
        );
    }

    #[test]
    fn help_sample_sum_of_roots_under_all_positive() {
        // sqrt(a^2) + sqrt(b^2) with ALL>0 is b + a (2_rata_2 r4).
        let asm = with(&[(None, Fact::Gt0)]);
        let s = Sym::sum(vec![sqrt_of_square("a"), sqrt_of_square("b")]).expect("sum");
        let got = simplify(&apply(&s, &asm).expect("apply")).expect("simplify");
        assert_eq!(
            got,
            Sym::sum(vec![Sym::symbol("a"), Sym::symbol("b")]).expect("a+b")
        );
    }

    #[test]
    fn help_sample_converts_through_from_expr() {
        let e = crate::parser::parse("sqrt(x^2)", &crate::parser::DefaultIdContext).expect("parse");
        let s = from_expr(&e).expect("from_expr");
        assert_eq!(s, sqrt_of_square("x"));
    }
}

#[cfg(test)]
mod bridge_tests {
    use crate::eval::{Outcome, Session, Value};

    /// The printed form of a symbolic region (spaces removed), or a panic with the outcome.
    fn shown(session: &mut Session, src: &str) -> String {
        let expr = crate::parser::parse(src, &crate::parser::DefaultIdContext)
            .unwrap_or_else(|e| panic!("{src}: {e:?}"));
        match session.run(&expr) {
            Outcome::Value(Value::Symbolic(s)) => {
                s.shown.to_string().chars().filter(|c| *c != ' ').collect()
            }
            other => panic!("{src}: {other:?}"),
        }
    }

    #[test]
    fn help_pythagorean_identity_is_one() {
        // No name is left after the identity, so the region is a plain number (real part 1).
        let mut s = Session::new();
        let expr = crate::parser::parse("sin(x)^2+cos(x)^2 ->", &crate::parser::DefaultIdContext)
            .expect("parse");
        let out = format!("{:?}", s.run(&expr));
        assert!(out.contains("re: 1.0, im: 0.0"), "{out}");
    }

    #[test]
    fn help_double_angle_folds() {
        let mut s = Session::new();
        assert_eq!(shown(&mut s, "2*sin(x)*cos(x) ->"), "sin(2*x)");
    }

    #[test]
    fn help_exponentials_combine_and_print_as_e_powers() {
        let mut s = Session::new();
        let got = shown(&mut s, "e^(2*x)*e^(5*y) ->");
        // Canonical order puts the sum in reverse alphabetical order (help shows e^(7y+2x)).
        assert!(got == "e^(5*y+2*x)" || got == "e^(2*x+5*y)", "{got}");
    }

    #[test]
    fn default_real_square_root_is_abs() {
        let mut s = Session::new();
        assert_eq!(shown(&mut s, "sqrt(x^2) ->"), "abs(x)");
    }

    #[test]
    fn assumption_on_a_command_removes_the_abs() {
        let mut s = Session::new();
        assert_eq!(shown(&mut s, "sqrt(x^2) -> assume, x>0"), "x");
    }

    #[test]
    fn assumptions_do_not_leak_into_the_next_region() {
        let mut s = Session::new();
        assert_eq!(shown(&mut s, "sqrt(x^2) -> assume, x>0"), "x");
        assert_eq!(shown(&mut s, "sqrt(x^2) ->"), "abs(x)");
    }

    #[test]
    fn help_all_positive_sum_of_roots() {
        let mut s = Session::new();
        assert_eq!(shown(&mut s, "sqrt(a^2)+sqrt(b^2) -> assume, ALL>0"), "b+a");
    }

    #[test]
    fn complex_variable_keeps_the_root() {
        let mut s = Session::new();
        assert_eq!(shown(&mut s, "sqrt(x^2) -> assume, x=complex"), "sqrt(x^2)");
    }

    /// `[eq1; eq2] -> solve, x, y` as the sheet builds it.
    fn system_region(eqs: Vec<crate::expr::Expr>, unknowns: &[&str]) -> crate::expr::Expr {
        use crate::expr::{Command, Expr};
        let rows = eqs.len();
        let matrix = Expr::matrix(rows, 1, eqs);
        let mut words = vec![Expr::var("solve")];
        words.extend(unknowns.iter().map(|u| Expr::var(u)));
        Expr::SymEval {
            expr: Box::new(matrix),
            command: Some(Command(words)),
            result: None,
        }
    }

    #[test]
    fn nonlinear_system_with_one_solution_gives_a_column() {
        use crate::expr::{BinaryOp, Expr};
        let eq = |l: Expr, r: Expr| Expr::binary(BinaryOp::Eq, l, r);
        // x^2 + y^2 = 0 and x = y: the only solution is (0, 0).
        let circle = eq(
            Expr::binary(
                BinaryOp::Add,
                Expr::binary(BinaryOp::Pow, Expr::var("x"), Expr::num(2.0)),
                Expr::binary(BinaryOp::Pow, Expr::var("y"), Expr::num(2.0)),
            ),
            Expr::num(0.0),
        );
        let line = eq(Expr::var("x"), Expr::var("y"));
        let region = system_region(vec![circle, line], &["x", "y"]);
        let out = Session::new().run(&region);
        assert!(matches!(out, Outcome::Value(_)), "{out:?}");
    }

    #[test]
    fn nonlinear_system_with_several_solutions_is_refused() {
        use crate::expr::{BinaryOp, Expr};
        let eq = |l: Expr, r: Expr| Expr::binary(BinaryOp::Eq, l, r);
        // x + y = 3 and x y = 2: two solutions, whose print format is not implemented.
        let sum = eq(
            Expr::binary(BinaryOp::Add, Expr::var("x"), Expr::var("y")),
            Expr::num(3.0),
        );
        let prod = eq(
            Expr::binary(BinaryOp::Mul, Expr::var("x"), Expr::var("y")),
            Expr::num(2.0),
        );
        let region = system_region(vec![sum, prod], &["x", "y"]);
        let out = Session::new().run(&region);
        assert!(
            !matches!(out, Outcome::Value(Value::Symbolic(_))),
            "{out:?}"
        );
        assert!(!matches!(out, Outcome::Value(_)), "{out:?}");
    }

    #[test]
    fn unsupported_assumption_is_refused() {
        let mut s = Session::new();
        let expr = crate::parser::parse("x -> assume, x>a", &crate::parser::DefaultIdContext)
            .expect("parse");
        assert!(!matches!(s.run(&expr), Outcome::Value(Value::Symbolic(_))));
    }
}

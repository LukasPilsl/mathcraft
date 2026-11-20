//! Tests for the symbolic core. Sample values come from the Prime results in
//! `documentation/examples/*.mcdx` (see `documentation/SYMBOLIC_OPTIONS.md`, section 8).

use crate::expr::Expr;
use crate::parser::{parse, DefaultIdContext};

use super::{equation_from_expr, from_expr, Equation, Sym};

mod budget;
mod calculus;
mod canonical;
mod complex;
mod factor;
mod factor_integers;
mod float;
mod fuzz;
mod hostile;
mod limit;
mod matrix;
mod normal;
mod parametric;
mod partial;
mod props;
mod root_search;
mod samples;
mod show;
mod solve;
mod stress;
mod surd;
mod taylor;

/// Whether two canonical values are the same rational function (exact, both sides cancelled).
pub(crate) fn same_function(a: &Sym, b: &Sym) -> bool {
    use crate::symbolic::ratfunc::RatFunc;
    match (RatFunc::from_sym(a), RatFunc::from_sym(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Parses source text with the default identifier rules.
pub(crate) fn ex(src: &str) -> Expr {
    parse(src, &DefaultIdContext).expect("test input parses")
}

/// Canonical value of source text.
pub(crate) fn sy(src: &str) -> Sym {
    from_expr(&ex(src)).expect("test input converts")
}

/// Equation of source text such as `x + y == 4`.
pub(crate) fn eqn(src: &str) -> Equation {
    equation_from_expr(&ex(src)).expect("test equation converts")
}

/// Approximate f64 value of a canonical value built from numbers, surds, `pi` and sums and
/// products of them. Other shapes give NaN (the caller checks finiteness).
pub(crate) fn approx(s: &Sym) -> f64 {
    use crate::symbolic::to_f64;
    match s {
        Sym::Num(q) => to_f64(q),
        Sym::Symbol(n) if n == "pi" => std::f64::consts::PI,
        Sym::Add(ts) => ts.iter().map(approx).sum(),
        Sym::Mul(fs) => fs.iter().map(approx).product(),
        Sym::Pow(b, e) => approx(b).powf(to_f64(e)),
        _ => f64::NAN,
    }
}

/// Owned unknown names.
pub(crate) fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Deterministic linear congruential generator (no external crates).
pub(crate) struct Lcg(pub u64);

impl Lcg {
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Integer in `lo..=hi`.
    pub(crate) fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo + 1) as u64;
        lo + (self.next_u64() % span) as i64
    }
}

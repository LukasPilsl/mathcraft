//! Exact symbolic core (Stage 1 of Mathcraft's own CAS). A pure library: no worksheet,
//! session or evaluator wiring yet.
//!
//! # What is here
//! - [`Q`]: exact rationals (`num-rational`). Decimal text is converted exactly, `0.5 -> 1/2`.
//! - [`Sym`]: canonical symbolic tree: `Num`, `Symbol`, `Unit`, `Func`, `Add`, `Mul`, `Pow`
//!   with rational exponents. Every value is built through constructors that keep it canonical
//!   (flattened, like terms combined, zero and one removed, deterministic order).
//! - Polynomials and rational functions (`expand`, `degree`, `substitute`, `simplify`).
//!   `simplify` cancels gcds of univariate factors over Q and removes common monomial and
//!   rational content otherwise (no multivariate gcd yet).
//! - Exact solving: [`solve_linear_system`] (Gauss-Jordan over rational functions, so symbolic
//!   coefficients work) and [`solve_polynomial`] (one unknown: linear, quadratic, rational
//!   roots of higher degree).
//! - Conversion to and from [`crate::expr::Expr`]: [`from_expr`], [`to_expr`],
//!   [`equation_from_expr`], [`system_from_expr`].
//!
//! # Output order (derived from the Prime samples)
//! Sums are ordered by lexicographic comparison of their monomials, with the variables taken
//! in reverse alphabetical order and exponents compared largest first; a constant term comes
//! last. Products list their factors alphabetically, with the numeric coefficient first. The
//! samples that fix this rule:
//! - `(a+b)^2` expanded: `b^2 + 2ab + a^2` (`1_rata_1` r10); `a^2+2ab+b^2` factored keeps the
//!   same order (`1_rata_1` r11).
//! - cube difference: `75 c^2 d^4 - 45 c^4 d^2 + 7 c^6` (`1_rata_1` r12).
//! - `sqrt(a^2)+sqrt(b^2)` under `assume ALL>0`: `b + a` (`2_rata_2` r4).
//! - `a^2-b^2` factored: `-((b-a)(b+a))` (`2_rata_2` r3): the sum `b - a` is reverse
//!   alphabetical.
//! - `a x^2 + b x + c` (`3_rata_3` r3); `3/2 x^2 + 16 - 21/2 x` (`3_rata_3` r4): descending
//!   degree, constant last.
//! - `(9C+160)/5` (`2_rata_2` r14) is a fraction; the canonical form is `9/5 C + 32`, equal in
//!   value but not in printed shape (printing is out of scope here).
//!
//! Prime's manual says its output order is engine specific, so the rule above is a
//! *reproduction* of the samples, not a proof. Pure lexicographic and graded orders agree on
//! every sample checked; the samples cannot tell them apart.
//!
//! # Limits
//! Every operation that can grow stops with [`SymError::TooLarge`]: at most 10 000 terms
//! ([`MAX_TERMS`]), integer exponents and polynomial degrees up to 1 000 ([`MAX_DEGREE`]),
//! coefficients up to 100 000 bits ([`MAX_COEFF_BITS`], checked at every coefficient product,
//! sum and power), at most 60 unknowns ([`MAX_UNKNOWNS`]). Each top-level call also has a work
//! budget of 100 000 charged operations (see `budget.rs`), so a dense numeric system of about
//! 25 unknowns is the practical size for elimination. Nesting is limited to 100 levels
//! ([`MAX_DEPTH`]): the constructors refuse deeper values, so no recursive function and no
//! `Drop` sees a deeper tree. The rational root search refuses constants above 64 bits, and
//! tries at most 2 000 candidates. Errors are returned, never panics.
//!
//! # Known limits (not done in this stage)
//! - Stage 3 (see `factor.rs` and `calc.rs`): `factor` over Q (rational roots of univariate
//!   polynomials, content and common monomial, difference of squares, square trinomials; other
//!   multivariate sums are refused), `differentiate` (polynomials, powers, sin/cos/exp/ln),
//!   `integrate_polynomial` (polynomials only), `coefficients`, and `assume_positive` (`ALL>0`).
//! - Stage 4 (see `calc.rs`): `integrate` (sums, constant factors, linear-argument powers, `exp`,
//!   `sin`, `cos`, `sinh`, `cosh`, `ln`; every result verified by differentiation) and
//!   `definite_integral` (refused across a singularity or at a divergent bound); derivatives of
//!   `tan`, `asin`, `acos`, `atan`, `sinh`, `cosh`, `tanh`.
//! - Stage 5 (see `limit.rs`, `taylor.rs`, `partial.rs`): `limit` (rational functions at finite
//!   points and at infinity, L'Hopital for analytic 0/0, one-sided only where the value is the
//!   same from both sides), `series` (Taylor at a finite point, up to 12 terms), `parfrac` over Q
//!   (rational roots, one irreducible leftover of degree 2 or 3).
//! - Not done: integration by parts and arctangent-type integrals, Laurent and Puiseux series,
//!   `standard` (O terms), `parfrac` with symbolic coefficients or a complex domain, complex and
//!   polar keywords on the sheet, trigonometric rewriting, transforms, units arithmetic (units
//!   are opaque symbols only), `float n` on non-numeric results.
//! - Core-only (no sheet wiring yet): [`SymMatrix`] (det, inverse, products, powers, transpose,
//!   characteristic polynomial, eigenvalues for 2x2/3x3, rank, null space) and [`Cplx`]
//!   (exact complex arithmetic, principal powers, Euler forms); see `matrix.rs`, `complex.rs`.
//!   Symbolic results with free names are still refused by the bridge (the engine stores values).
//! - Systems of two or more nonlinear equations: not supported.
//! - Polynomials of degree 3 or more: only rational roots (plus quadratic remainders).
//!   Complex roots and irrational cubic roots are `Unsupported`.
//! - Linear systems with symbolic coefficients are solved assuming every pivot is nonzero;
//!   the solution is the generic one. [`solve_linear_system_with_conditions`] and
//!   [`solve_polynomial_with_conditions`] return the assumed nonzero values with the answer.
//!   A leftover row `0 = b` with symbolic `b` is `Unsupported` (consistency depends on the
//!   parameters); a nonzero constant leftover is `NoSolution`.
//! - Surds: square roots of positive integers in one product merge (`sqrt(2)*sqrt(3)` is
//!   `6^(1/2)`, `sqrt(8)*sqrt(3)` is `2*6^(1/2)`), and `b^(p/q)` with an integer part splits
//!   (`2^(3/2) = 2*2^(1/2)`). Not merged: other root degrees (`2^(1/3)*3^(1/3)` stays two
//!   factors). Sums of surds collect only like radicands (`sqrt(2)+sqrt(8)` is `3*2^(1/2)`,
//!   `sqrt(2)+sqrt(3)` stays a sum).
//! - Multivariate gcd: `simplify` cancels only univariate common factors and monomial content.
//! - Worksheet names: a name the worksheet defines is a `Symbol` here. The caller must
//!   substitute worksheet definitions before conversion (sample `Mechanik_3` region 9 shows
//!   the wrong answer otherwise). Worksheet function calls convert as opaque `Func` values.
//!
//! # How the evaluator will use this
//! The evaluator will see a `SymEval { expr, command, result }` node (see
//! [`crate::expr::Expr`]). The plan for the wiring task:
//! - Plain `expr ->` (no command): `from_expr(expr)` then [`simplify`]; the result becomes
//!   `to_expr`. If `from_expr` returns `Unsupported`, the region stays unevaluated, as today.
//! - `expand`: [`expand`] on the converted tree.
//! - `solve, x` (one unknown, `expr` is an equation `lhs == rhs`): [`equation_from_expr`] then
//!   [`solve_polynomial`]; a single value is the printed result, several values are printed as
//!   a comma-separated list.
//! - Vector of names, `[[solve, x], y]` on a matrix of equations: [`system_from_expr`] then
//!   [`solve_linear_system`]; `Unique` gives a column matrix in the unknown order, as Prime
//!   prints `matrix{...}`. `Infinite` and `NoSolution` must not be printed as numbers.
//! - Define with solve (`y(x) := ...` plus a solve on the same worksheet): the evaluator
//!   substitutes the solved values into the defined expression with [`substitute`], then
//!   simplifies. Prime keeps unknown names symbolic: a name that is not defined and not a unit
//!   stays a `Symbol`, so `a*x+b` with `x` free stays an expression, never an error.
//! - `y(x) ->` forms: a worksheet function `y` with a free `x` is resolved by the caller
//!   before conversion; the symbolic core only sees the expanded expression with `x` as a
//!   `Symbol`.
//! - `float n`: not in this stage. The core keeps exact rationals and never rounds.

mod assume;
mod budget;
mod calc;
mod canon;
mod complex;
mod convert;
mod factor;
mod factor_multi;
mod fourier;
mod limit;
mod linear;
mod matrix;
mod num;
mod numcheck;
mod order;
mod partial;
mod poly;
mod poly_solve;
mod polysys;
mod ratfunc;
mod show;
mod sym;
mod taylor;
#[cfg(test)]
mod tests;
mod trans_solve;
mod transform;
mod trig;
mod trigexp;
mod ztrans;

pub use assume::{Assumptions, Fact, Knowledge};
pub use calc::{
    assume_positive, coefficients, collect, definite_integral, differentiate, integrate,
    integrate_polynomial, symbols,
};
pub use complex::{euler, Cplx};
pub use convert::{equation_from_expr, from_expr, solution_to_expr, system_from_expr, to_expr};
pub use factor::factor;
pub use fourier::{fourier, inverse_fourier};
pub use limit::{limit_of, Point, Side, MAX_LHOPITAL};
pub use linear::{
    solve_linear_system, solve_linear_system_with_conditions, Equation, SolveOutcome,
};
pub use matrix::{Inverse, Rank, SymMatrix, MAX_MATRIX_DIM, MAX_MATRIX_POWER};
pub use num::{from_f64_exact, parse_decimal, round_significant, to_f64, Q};
pub use partial::parfrac;
pub use poly::{degree, eval_rational, expand, substitute};
pub use poly_solve::{
    approximate, solve_polynomial, solve_polynomial_complex, solve_polynomial_with_conditions,
};
pub use polysys::{solve_triangular, MAX_SOLUTIONS};
pub use ratfunc::simplify;
pub use show::shown_expr;
pub use sym::Sym;
pub use taylor::{series_of, MAX_SERIES_TERMS};
pub use trans_solve::solve_transcendental;
pub use transform::{inverse_laplace, laplace};
pub use trig::simplify_identities;
pub use trigexp::{expand_trig, rewrite_as, rewrite_modifier_known, MAX_MULTIPLE};
pub use ztrans::{inverse_ztrans, ztrans};

/// Largest number of terms in a sum, product, polynomial or expansion.
pub const MAX_TERMS: usize = 10_000;
/// Largest integer exponent and polynomial degree.
pub const MAX_DEGREE: u32 = 1_000;
/// Largest coefficient size in bits (numerator or denominator).
pub const MAX_COEFF_BITS: u64 = 100_000;
/// Largest number of unknowns in one linear system.
pub const MAX_UNKNOWNS: usize = 60;
/// Largest nesting depth accepted by [`from_expr`] and [`equation_from_expr`].
pub const MAX_DEPTH: usize = 100;

/// Errors of the symbolic core. Never a panic: every bad input maps to one of these.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SymError {
    /// The input uses a construct this stage does not implement.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// A work budget was exceeded.
    #[error("too large: {0}")]
    TooLarge(&'static str),
    /// Expression nesting exceeded [`MAX_DEPTH`].
    #[error("nesting deeper than 100 levels")]
    TooDeep,
    /// The input is well formed but not valid for the operation (e.g. duplicate unknowns).
    #[error("invalid: {0}")]
    Invalid(String),
    /// Division by zero and similar.
    #[error("undefined: {0}")]
    Undefined(&'static str),
}

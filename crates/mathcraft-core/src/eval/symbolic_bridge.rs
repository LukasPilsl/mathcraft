//! Symbolic evaluation (`expr -> command`, Mathcad's `ml:symEval`) through the exact core in
//! [`crate::symbolic`].
//!
//! Three steps:
//! 1. **Prepare** rewrites the request into an `Expr` the core can read. A worksheet variable
//!    becomes its value: a real number, written as the shortest decimal that reads back to the same
//!    `f64`, times one base-unit power per dimension exponent (`F_G` = 833.56525 N becomes
//!    `833.56525 kg m s^-2`, so the answer keeps its unit). A call to a worksheet function is
//!    expanded at the call, with the body's captured names (see `closure`). A vector-valued part
//!    such as `(A+B)/2` is evaluated numerically, and `A[1]` of a literal vector is picked out.
//! 2. **Compute** runs the core: `simplify` for a plain `->`, `expand`, or `solve` (one equation in
//!    one unknown, or a system into a column of unknowns, in the order given).
//! 3. **Finish** converts the canonical result back to an `Expr` and evaluates it with the ordinary
//!    evaluator, so the answer is a `Value` with its dimension.
//!
//! Anything the core cannot answer is `EvalError::Unsupported`: an unknown command, a name left free
//! (one the worksheet does not define), several solutions, a symbolic exponent or index. Nothing is
//! guessed. Work and depth are charged to the caller's budget, and the core has its own budget.
//!
//! Inside a function body (see `body_value`) each call is one evaluation over the body's parameters
//! (by value) and the names captured at definition. A plain or expanded answer with no names left
//! is its number. `float` rounds the coefficients of the answer in the parameters, then binds them
//! (see `float_answer`). A recursive body and a result that keeps a name are refused.

use num_rational::Ratio;

use crate::expr::{BinaryOp, Command, Expr, Id, IdKind, Number, UnaryOp};
use num_traits::{Signed, ToPrimitive};

use crate::symbolic::{
    approximate, coefficients, collect, definite_integral, differentiate, equation_from_expr,
    expand, expand_trig, factor, fourier, from_expr, from_f64_exact, inverse_fourier,
    inverse_laplace, inverse_ztrans, laplace, limit_of, parfrac, rewrite_as,
    rewrite_modifier_known, round_significant, series_of, shown_expr, simplify,
    simplify_identities, solve_linear_system_with_conditions, solve_polynomial_complex,
    solve_transcendental, solve_triangular, substitute, symbols, system_from_expr, to_expr, ztrans,
    Assumptions, Equation, Fact, Point, Side, SolveOutcome, Sym, SymError, MAX_SERIES_TERMS, Q,
};
use crate::units::{Dimension, BASE_SYMBOLS};

mod sym_complex;
mod sym_matrix;

use super::constants::constant;
use super::env::{Env, UserFunction};
use super::error::EvalError;
use super::expr_eval::{
    evaluate_budget, number_value, range_in_expression, var_key, Ctx, Frame, MAX_CALL_DEPTH,
    MAX_NODE_DEPTH,
};
use super::indexing::zero_based;
use super::value::{Scalar, Symbolic, Value};
use super::work::Budget;

/// Budget units charged per node of a prepared expression (about 10 µs in a release build).
const NODE_UNITS: u64 = 1_000;

/// Evaluates `expr -> command` (no command for the plain form) against the worksheet.
///
/// The answer is a number (with its unit) when it has no names left. A result with a free name
/// (`x^2 - 1` with `x` free, `solve, x` of `a + x = 2`) is a [`Value::Symbolic`]. `factor` of an
/// integer is symbolic too when it shows a product of prime powers (`factor(12)` is `2^2*3`).
pub(super) fn evaluate(
    expr: &Expr,
    command: Option<&Command>,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    // A programming region's local scope is not a function body: it stays refused.
    if ctx.scope.is_some() {
        return Err(unsupported(
            "symbolic evaluation inside a programming region",
        ));
    }
    let request = request_of(command)?;
    let factored = matches!(request, Request::Factor);
    let value = match ctx.frame {
        None => {
            if !ctx.locals.is_empty() {
                return Err(unsupported("symbolic evaluation with local names"));
            }
            let scope = Scope {
                env: ctx.env,
                body: None,
                depth: 0,
                nodes: ctx.nodes,
            };
            // The exact matrix and imaginary-unit paths come first: each returns `None` when the
            // ordinary path applies (no names in a matrix), and refuses what it cannot make exact.
            let exact = match &request {
                Request::Rectangular => Some(sym_complex::answer(expr, &scope, ctx.budget)?),
                Request::Simplify | Request::Expand | Request::Float(_)
                    if matches!(strip(expr), Expr::Gradient { .. }) =>
                {
                    Some(gradient_answer(strip(expr), &request, &scope, ctx.budget)?)
                }
                Request::Simplify | Request::Expand | Request::Float(_)
                    if sym_matrix::contains_matrix(expr, &scope) =>
                {
                    sym_matrix::answer(expr, &request, &scope, ctx.budget)?
                }
                _ => None,
            };
            let answer = match (exact, request) {
                (Some(answer), Request::Float(digits)) => round_answer(answer, digits)?,
                (Some(answer), _) => answer,
                (None, Request::Float(digits)) => {
                    round_answer(answer_of(expr, &scope, ctx.budget)?, digits)?
                }
                // A matrix with no names is a number: the ordinary evaluator gives it exactly.
                (None, Request::Simplify) if sym_matrix::contains_matrix(expr, &scope) => {
                    return evaluate_budget(expr, ctx.env, ctx.budget).map_err(finish_error);
                }
                (None, other) => run(&other, expr, &scope, ctx.budget)?,
            };
            finish(answer, factored, ctx)?
        }
        Some(frame) => {
            // Each call of a body is one symbolic evaluation, charged to the budget (the core
            // charges its own operations to its counter).
            ctx.budget.charge(SYMBOLIC_EVAL_UNITS)?;
            body_value(expr, &request, frame, factored, ctx)?
        }
    };
    // Inside a function body the result feeds a numeric context, so it must be a number.
    if ctx.frame.is_some() && matches!(value, Value::Symbolic(_)) {
        return Err(unsupported(
            "symbolic result inside a function body is not a number",
        ));
    }
    Ok(value)
}

/// Budget units charged per symbolic evaluation inside a function body (each call): the set-up of
/// one evaluation, before its nodes. A plot evaluates its bodies once per sample.
const SYMBOLIC_EVAL_UNITS: u64 = 5 * NODE_UNITS;

/// The value of a symbolic evaluation inside a function body. The arguments are the body's
/// parameters (by value, as the evaluator binds them). A plain or expanded answer with no names
/// left is its number, evaluated directly. `float` rounds the symbolic answer with the parameters
/// still symbols, then binds them: Prime rounds the coefficients of the body's expression in its
/// parameter (sample 15, `wsa(x) := f(x, A, A+c_0+b_0) -> float, 3`: the stored values are the
/// line with its slope and intercept rounded to 3 figures, not the rounded value).
fn body_value(
    expr: &Expr,
    request: &Request,
    frame: Frame<'_>,
    factored: bool,
    ctx: Ctx<'_>,
) -> Result<Value, EvalError> {
    // The parameters as expressions, for the symbolic paths only (a value with no expression, such
    // as text, can still be evaluated numerically).
    let params = || -> Result<Vec<(String, Expr)>, EvalError> {
        ctx.locals
            .iter()
            .map(|(name, value)| Ok((name.clone(), value_expr(value)?)))
            .collect()
    };
    let scope_with = |params: Vec<(String, Expr)>| Scope {
        env: ctx.env,
        body: Some(Body {
            name: frame.name.to_string(),
            func: frame.func,
            params,
        }),
        depth: ctx.calls,
        nodes: ctx.nodes,
    };
    match request {
        Request::Simplify | Request::Expand => {
            // With numeric arguments the body has no name left, and its simplified form is its
            // number: the ordinary evaluator, in the body's own scope (parameters, captures and
            // depth), gives it without the core. A recursive body is not run this way (it is
            // refused below, as the symbolic path does), and a budget or depth error is final;
            // any other failure goes the symbolic way.
            if !frame.func.closure.recursive {
                match super::expr_eval::eval(expr, ctx) {
                    Ok(value) => return Ok(value),
                    Err(
                        e @ (EvalError::WorkLimit(_)
                        | EvalError::CallLimit(_)
                        | EvalError::DepthLimit(_)),
                    ) => return Err(e),
                    Err(_) => {}
                }
            }
            let scope = scope_with(params()?);
            let prepared = prepare(expr, &scope, &[], ctx.budget, ctx.nodes)?;
            finish(compute(request, &prepared)?, factored, ctx)
        }
        Request::Float(digits) => {
            let answer = float_answer(expr, *digits, &scope_with(params()?), ctx.budget)?;
            finish(answer, factored, ctx)
        }
        other => {
            let answer = run(other, expr, &scope_with(params()?), ctx.budget)?;
            finish(answer, factored, ctx)
        }
    }
}

/// `float, n` of `inner` in `scope`. At top level it rounds the answer. In a function body the
/// parameters stay symbols while the answer is rounded, then they take their values (see
/// `body_value`): the rounding is the one of the body's expression in its parameters, whatever
/// the arguments are (a number or a name left by an enclosing solve).
fn float_answer(
    inner: &Expr,
    digits: u32,
    scope: &Scope<'_>,
    budget: &Budget,
) -> Result<Answer, EvalError> {
    let Some(body) = &scope.body else {
        return round_answer(answer_of(inner, scope, budget)?, digits);
    };
    let symbolic = body
        .params
        .iter()
        .map(|(name, _)| (name.clone(), Expr::Id(Id::new(name, IdKind::Variable))))
        .collect();
    let inside = Scope {
        env: scope.env,
        body: Some(Body {
            name: body.name.clone(),
            func: body.func,
            params: symbolic,
        }),
        depth: scope.depth,
        nodes: scope.nodes,
    };
    let answer = round_answer(answer_of(inner, &inside, budget)?, digits)?;
    bind_parameters(answer, &body.params)
}

/// Substitutes the parameters of a body (`params`, by value) into a symbolic answer and
/// simplifies it: the numbers of the answer are the ones `float` left.
fn bind_parameters(answer: Answer, params: &[(String, Expr)]) -> Result<Answer, EvalError> {
    let values: Vec<(&str, Sym)> = params
        .iter()
        .map(|(name, arg)| Ok((name.as_str(), core(from_expr(arg))?)))
        .collect::<Result<_, EvalError>>()?;
    let bind = |s: Sym| -> Result<Sym, EvalError> {
        if values.is_empty() {
            return Ok(s);
        }
        core(simplify(&core(substitute(&s, &values))?))
    };
    Ok(match answer {
        Answer::One(s) => Answer::One(bind(s)?),
        Answer::Column(v) => Answer::Column(v.into_iter().map(bind).collect::<Result<_, _>>()?),
        Answer::Row(v) => Answer::Row(v.into_iter().map(bind).collect::<Result<_, _>>()?),
        Answer::Grid(rows, cols, v) => Answer::Grid(
            rows,
            cols,
            v.into_iter().map(bind).collect::<Result<_, _>>()?,
        ),
    })
}

/// One `substitute` pair: `name = rhs`, or `name(p1, p2) = rhs` (a function body over its
/// parameters) when `params` is set.
struct SubPair {
    name: String,
    params: Option<Vec<String>>,
    rhs: Expr,
}

/// What the command asks for.
enum Request {
    Simplify,
    /// `assume, x>0, ALL>0, ...`: the assumptions of this command only, then simplify.
    Assumed(Assumptions),
    Expand,
    Factor,
    /// `float, n`: every number of the result is rounded to `n` significant figures.
    Float(u32),
    /// `solve, x` or `solve, x, y`: the unknowns in the order given.
    Solve(Vec<String>),
    /// `collect` or `collect, x`: the expanded polynomial grouped by powers of `x`. Without a name
    /// the expression must have one symbol.
    Collect(Option<String>),
    /// `coeffs` or `coeffs, x`: the column of coefficients, lowest power first.
    Coeffs(Option<String>),
    /// `substitute, x = e, ...`: each name is replaced by its expression, then simplified.
    Substitute(Vec<SubPair>),
    /// `parfrac` or `parfrac, x`: the partial fraction decomposition over the rationals.
    Parfrac(Option<String>),
    /// `series`, `series, n`, `series, x = a` or `series, x = a, n`: the Taylor series in `var` (the
    /// only variable when none is named) around `point` (0 when none is given), `terms` terms.
    Series {
        var: Option<String>,
        point: Option<Expr>,
        terms: usize,
    },
    /// `laplace` (default variable `t`, new variable `s`) or `invlaplace` (`s` to `t`), with the
    /// optional names `from, to`. The `simplify` modifier is accepted: results are always canonical.
    Transform {
        inverse: bool,
        from: Option<String>,
        to: Option<String>,
        /// `ztrans` / `invztrans` (defaults `n` and `z`) instead of Laplace.
        z: bool,
        /// `fourier` / `invfourier` (defaults `t` and `ω`) instead of Laplace.
        fourier: bool,
    },
    /// `rewrite, m`: the expression rewritten in terms of the elementary function `m` (see
    /// [`crate::symbolic::rewrite_as`]).
    Rewrite(String),
    /// `rectangular` (or `complex`): the result as `re + im*i`, exactly (see `sym_complex`).
    Rectangular,
}

/// Largest `n` of `float, n`; beyond it the decimal text is not exact enough to trust.
const MAX_FLOAT_DIGITS: u32 = 15;

fn request_of(command: Option<&Command>) -> Result<Request, EvalError> {
    let words: &[Expr] = command.map_or(&[][..], |c| c.0.as_slice());
    let keyword = match words.first() {
        None => return Ok(Request::Simplify),
        // Words are plain names, or names with a literal subscript (`M_1`), keyed as variables are.
        Some(Expr::Id(id)) => var_key(id)?,
        Some(other) => return Err(unsupported(format!("symbolic command word `{other}`"))),
    };
    let rest = &words[1..];
    match keyword.as_str() {
        "expand" if rest.is_empty() => Ok(Request::Expand),
        "factor" if rest.is_empty() => Ok(Request::Factor),
        "float" => float_digits(rest).map(Request::Float),
        "assume" => assumptions_of(rest).map(Request::Assumed),
        "simplify" if rest.is_empty() => Ok(Request::Simplify),
        "collect" => Ok(Request::Collect(optional_name(rest)?)),
        "coeffs" => Ok(Request::Coeffs(optional_name(rest)?)),
        "substitute" if !rest.is_empty() => substitutions(rest).map(Request::Substitute),
        "parfrac" => Ok(Request::Parfrac(optional_name(rest)?)),
        "series" => series_request(rest),
        "laplace" => transform_request(false, Kind::Laplace, rest),
        "invlaplace" => transform_request(true, Kind::Laplace, rest),
        "ztrans" => transform_request(false, Kind::Z, rest),
        "invztrans" => transform_request(true, Kind::Z, rest),
        "fourier" => transform_request(false, Kind::Fourier, rest),
        "invfourier" => transform_request(true, Kind::Fourier, rest),
        "rectangular" | "complex" if rest.is_empty() => Ok(Request::Rectangular),
        "rewrite" => rewrite_request(rest),
        "solve" if !rest.is_empty() => {
            let mut names = Vec::with_capacity(rest.len());
            for word in rest {
                match word {
                    Expr::Id(id) => names.push(var_key(id)?),
                    other => return Err(unsupported(format!("symbolic command word `{other}`"))),
                }
            }
            Ok(Request::Solve(names))
        }
        other => Err(unsupported(format!(
            "symbolic command `{other}` with these words"
        ))),
    }
}

/// `rewrite, m` with exactly one modifier from the supported set (a bare `rewrite` is an error in
/// Prime too, and `raw` is not reproduced).
fn rewrite_request(rest: &[Expr]) -> Result<Request, EvalError> {
    match rest {
        [Expr::Id(id)] => {
            let name = var_key(id)?;
            if rewrite_modifier_known(&name) {
                Ok(Request::Rewrite(name))
            } else {
                Err(unsupported(format!("rewrite, {name}")))
            }
        }
        [] => Err(unsupported("rewrite needs a modifier")),
        _ => Err(unsupported("rewrite with these words")),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Laplace,
    Z,
    Fourier,
}

/// `laplace`, `laplace, t`, `laplace, t, s` (and the same for `invlaplace`), with the optional
/// `simplify` modifier.
fn transform_request(inverse: bool, kind: Kind, rest: &[Expr]) -> Result<Request, EvalError> {
    let (z, fourier) = (kind == Kind::Z, kind == Kind::Fourier);
    let mut names = Vec::with_capacity(rest.len());
    for word in rest {
        match word {
            Expr::Id(id) if id.name == "simplify" && id.subscript.is_none() => {}
            Expr::Id(id) => names.push(var_key(id)?),
            other => return Err(unsupported(format!("symbolic command word `{other}`"))),
        }
    }
    match names.as_slice() {
        [] => Ok(Request::Transform {
            inverse,
            from: None,
            to: None,
            z,
            fourier,
        }),
        [from] => Ok(Request::Transform {
            inverse,
            from: Some(from.clone()),
            to: None,
            z,
            fourier,
        }),
        [from, to] => Ok(Request::Transform {
            inverse,
            from: Some(from.clone()),
            to: Some(to.clone()),
            z,
            fourier,
        }),
        _ => Err(unsupported("transform with these words")),
    }
}

/// The variable of `collect, x` or `coeffs, x`; no word means the expression's only variable.
fn optional_name(rest: &[Expr]) -> Result<Option<String>, EvalError> {
    match rest {
        [] => Ok(None),
        [Expr::Id(id)] => var_key(id).map(Some),
        _ => Err(unsupported("symbolic command with these words")),
    }
}

/// The `x = e` pairs of `substitute, x = e, y = f`. A function or an expression on the left
/// (`cos = sin`) and modifiers (`raw`) are refused.
fn substitutions(rest: &[Expr]) -> Result<Vec<SubPair>, EvalError> {
    rest.iter()
        .map(|word| match word {
            Expr::Binary {
                op: BinaryOp::Eq,
                lhs,
                rhs,
            } => match &**lhs {
                Expr::Id(id) => Ok(SubPair {
                    name: var_key(id)?,
                    params: None,
                    rhs: (**rhs).clone(),
                }),
                // `f(x, y) = body`: the parameters must be distinct plain names.
                Expr::Call { func, args } if !args.is_empty() => {
                    let mut params = Vec::with_capacity(args.len());
                    for a in args {
                        match a {
                            Expr::Id(p) => params.push(var_key(p)?),
                            _ => return Err(unsupported("substitute of an expression")),
                        }
                    }
                    let mut seen = params.clone();
                    seen.sort();
                    seen.dedup();
                    if seen.len() != params.len() {
                        return Err(unsupported("substitute with repeated parameters"));
                    }
                    Ok(SubPair {
                        name: var_key(func)?,
                        params: Some(params),
                        rhs: (**rhs).clone(),
                    })
                }
                _ => Err(unsupported("substitute of an expression")),
            },
            other => Err(unsupported(format!("symbolic command word `{other}`"))),
        })
        .collect()
}

/// The words of `series`: at most one variable (bare, or with its point as `x = a`) and at most one
/// literal number of terms (default 6, from 1 to [`MAX_SERIES_TERMS`]). Several variables are refused.
fn series_request(rest: &[Expr]) -> Result<Request, EvalError> {
    let mut var = None;
    let mut point = None;
    let mut terms = None;
    for word in rest {
        match word {
            Expr::Binary {
                op: BinaryOp::Eq,
                lhs,
                rhs,
            } => match &**lhs {
                Expr::Id(id) if var.is_none() && id.kind != IdKind::Function => {
                    var = Some(var_key(id)?);
                    point = Some((**rhs).clone());
                }
                _ => return Err(unsupported("series with this point")),
            },
            Expr::Id(id) if var.is_none() && id.kind != IdKind::Function => {
                var = Some(var_key(id)?);
            }
            Expr::Number(n) if terms.is_none() => {
                let k = whole_number(n)
                    .and_then(|k| usize::try_from(k).ok())
                    .filter(|k| (1..=MAX_SERIES_TERMS).contains(k))
                    .ok_or_else(|| unsupported("series with this number of terms"))?;
                terms = Some(k);
            }
            other => return Err(unsupported(format!("symbolic command word `{other}`"))),
        }
    }
    Ok(Request::Series {
        var,
        point,
        terms: terms.unwrap_or(6),
    })
}

/// The digits of `float, n` (a literal integer from 1 to [`MAX_FLOAT_DIGITS`]).
fn float_digits(rest: &[Expr]) -> Result<u32, EvalError> {
    if let [Expr::Number(number)] = rest {
        if let Some(n) = whole_number(number).and_then(|n| u32::try_from(n).ok()) {
            if (1..=MAX_FLOAT_DIGITS).contains(&n) {
                return Ok(n);
            }
        }
    }
    Err(unsupported("symbolic command `float` with these words"))
}

/// The value of a literal that is a whole number (`3`, `3.0`), if it is one.
fn whole_number(number: &Number) -> Option<i64> {
    match number {
        Number::Rational(q) if q.is_integer() => q.to_integer().to_i64(),
        Number::Real { value, .. } if value.fract() == 0.0 && value.abs() < 1e15 => {
            Some(*value as i64)
        }
        _ => None,
    }
}

/// The assumptions of `assume, x>0, ALL>0, n=odd, ...`. Each word is a comparison with zero
/// (`x>0`, `x>=0`, `x<0`, `x<=0`) or an equality with a keyword (`x=real`, `x=complex`,
/// `x=integer`, `n=even`, `n=odd`). `ALL` stands for every variable. Anything else is refused,
/// so the region stays unevaluated rather than silently ignoring a constraint.
fn assumptions_of(rest: &[Expr]) -> Result<Assumptions, EvalError> {
    if rest.is_empty() {
        return Err(unsupported("assume without a constraint"));
    }
    let mut out = Assumptions::default();
    for word in rest {
        let (name, fact) = assumption_word(word)?;
        out.add(name.as_deref(), fact).map_err(core_error)?;
    }
    Ok(out)
}

/// One assumption word: the variable (`None` for `ALL`) and the fact it states.
fn assumption_word(word: &Expr) -> Result<(Option<String>, Fact), EvalError> {
    let refused = || unsupported(format!("assumption `{word}`"));
    let Expr::Binary { op, lhs, rhs } = word else {
        return Err(refused());
    };
    let Expr::Id(id) = lhs.as_ref() else {
        return Err(refused());
    };
    if id.subscript.is_some() {
        return Err(refused());
    }
    let name = if id.name == "ALL" {
        None
    } else {
        Some(var_key(id)?)
    };
    let fact = match (op, rhs.as_ref()) {
        (BinaryOp::Gt | BinaryOp::Ge | BinaryOp::Lt | BinaryOp::Le, Expr::Number(n))
            if whole_number(n) == Some(0) =>
        {
            match op {
                BinaryOp::Gt => Fact::Gt0,
                BinaryOp::Ge => Fact::Ge0,
                BinaryOp::Lt => Fact::Lt0,
                _ => Fact::Le0,
            }
        }
        (BinaryOp::Eq, Expr::Id(word)) => match word.name.as_str() {
            "real" => Fact::Real,
            "complex" => Fact::Complex,
            "integer" | "even" | "odd" => Fact::Integer,
            _ => return Err(refused()),
        },
        _ => return Err(refused()),
    };
    Ok((name, fact))
}

fn core_error(e: SymError) -> EvalError {
    unsupported(e.to_string())
}

/// The answer of `expr` as the input of `float`: a nested command (`(... -> solve, x) -> float, 4`)
/// is computed first, and a plain expression is simplified.
fn answer_of(expr: &Expr, scope: &Scope<'_>, budget: &Budget) -> Result<Answer, EvalError> {
    match expr {
        Expr::Parens(inner) => answer_of(inner, scope, budget),
        Expr::SymEval {
            expr: inner,
            command: Some(command),
            ..
        } => match request_of(Some(command))? {
            Request::Float(digits) => float_answer(inner, digits, scope, budget),
            other => run(&other, inner, scope, budget),
        },
        other => run(&Request::Simplify, other, scope, budget),
    }
}

/// Prepares `expr` for the request (unknowns stay symbolic) and computes the answer.
fn run(
    request: &Request,
    expr: &Expr,
    scope: &Scope<'_>,
    budget: &Budget,
) -> Result<Answer, EvalError> {
    if let Request::Substitute(pairs) = request {
        return substitute_answer(pairs, expr, scope, budget);
    }
    if let Request::Series { var, point, terms } = request {
        return series_answer(var.as_deref(), point.as_ref(), *terms, expr, scope, budget);
    }
    let unknowns: &[String] = match request {
        Request::Solve(names) => names.as_slice(),
        _ => &[],
    };
    let prepared = prepare(expr, scope, unknowns, budget, scope.nodes)?;
    compute(request, &prepared)
}

/// `substitute, x = e, ...`: the expression and each replacement are prepared, the names are
/// replaced, and the result is simplified. The replacements are not expanded into the result
/// beyond what the canonical form does.
fn substitute_answer(
    pairs: &[SubPair],
    expr: &Expr,
    scope: &Scope<'_>,
    budget: &Budget,
) -> Result<Answer, EvalError> {
    let mut main = core(from_expr(&prepare(expr, scope, &[], budget, scope.nodes)?))?;
    // `f(x) = body`: every application `f(a)` becomes the body with `x` replaced by `a`. The
    // parameters stay symbolic in the body whatever the worksheet defines them as. A body may apply
    // another replaced function (`f(x) = g(x) + 1`), so the replacements repeat until none applies;
    // one that is still applied after as many rounds as there are pairs is recursive, and refused.
    let fn_pairs: Vec<&SubPair> = pairs.iter().filter(|p| p.params.is_some()).collect();
    let mut bodies = Vec::with_capacity(fn_pairs.len());
    for pair in &fn_pairs {
        let prepared = prepare(
            &pair.rhs,
            scope,
            pair.params.as_deref().unwrap_or(&[]),
            budget,
            scope.nodes,
        )?;
        bodies.push(core(from_expr(&prepared))?);
    }
    let mut applied_once = vec![false; fn_pairs.len()];
    for _round in 0..=fn_pairs.len() {
        let mut changed = false;
        for (i, pair) in fn_pairs.iter().enumerate() {
            if !applies_any(&main, &[pair.name.as_str()]) {
                continue;
            }
            let params = pair.params.as_deref().unwrap_or(&[]);
            main = core(replace_function(&main, &pair.name, params, &bodies[i]))?;
            applied_once[i] = true;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    if applied_once.iter().any(|done| !done) {
        return Err(unsupported("substitute of a function that is not applied"));
    }
    if fn_pairs
        .iter()
        .any(|p| applies_any(&main, &[p.name.as_str()]))
    {
        return Err(unsupported("substitute of a recursive function"));
    }
    let mut values: Vec<(String, Sym)> = Vec::with_capacity(pairs.len());
    for pair in pairs.iter().filter(|p| p.params.is_none()) {
        let prepared = prepare(&pair.rhs, scope, &[], budget, scope.nodes)?;
        values.push((pair.name.clone(), core(from_expr(&prepared))?));
    }
    // A name applied as a function (`cos(x)` with `cos = sin`) is not a variable. It is renamed
    // when both sides are elementary one-argument functions (help: `cos(x) -> substitute, cos =
    // sin` is `sin(x)`); anything else is refused, never left in place.
    let mut renames: Vec<(String, String)> = Vec::new();
    values.retain(|(name, value)| {
        let applied = applies_any(&main, &[name.as_str()]);
        match value {
            Sym::Symbol(target) if applied => {
                renames.push((name.clone(), target.clone()));
                false
            }
            _ => true,
        }
    });
    for (from, to) in &renames {
        if !is_elementary(from) || !is_elementary(to) {
            return Err(unsupported("substitute of a function is not supported"));
        }
    }
    if !renames.is_empty() {
        main = core(rename_functions(&main, &renames))?;
    }
    let names: Vec<&str> = values.iter().map(|(n, _)| n.as_str()).collect();
    if applies_any(&main, &names) {
        return Err(unsupported("substitute of a function is not supported"));
    }
    let refs: Vec<(&str, Sym)> = values
        .iter()
        .map(|(n, v)| (n.as_str(), v.clone()))
        .collect();
    let out = core(substitute(&main, &refs))?;
    Ok(Answer::One(core(simplify(&out))?))
}

/// `s` with every application `name(a1, ..)` replaced by `body` with `params` replaced by the
/// arguments at once. An application with another number of arguments, or an argument that
/// mentions one of the parameters (when there are several), is refused.
fn replace_function(s: &Sym, name: &str, params: &[String], body: &Sym) -> Result<Sym, SymError> {
    let rec = |t: &Sym| replace_function(t, name, params, body);
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(n, args) => {
            let args = args.iter().map(rec).collect::<Result<Vec<_>, _>>()?;
            if n != name {
                return Sym::func(n, args);
            }
            if args.len() != params.len() {
                return Err(SymError::Unsupported("substitute of a function".into()));
            }
            // `substitute` replaces every parameter in one pass, so an argument that names a
            // parameter (`f(y, x)` for `f(x, y)`) is taken as written, not substituted again.
            let refs: Vec<(&str, Sym)> = params
                .iter()
                .zip(args)
                .map(|(p, a)| (p.as_str(), a))
                .collect();
            substitute(body, &refs)
        }
        Sym::Add(ts) => Sym::sum(ts.iter().map(rec).collect::<Result<Vec<_>, _>>()?),
        Sym::Mul(ts) => Sym::product(ts.iter().map(rec).collect::<Result<Vec<_>, _>>()?),
        Sym::Pow(b, e) => Sym::pow(rec(b)?, e.clone()),
    }
}

/// The one-argument elementary functions `substitute` may rename into each other.
fn is_elementary(name: &str) -> bool {
    matches!(
        name,
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "ln"
    )
}

/// `s` with every application of a name in `renames` renamed to its partner, all at once: each
/// application is looked up once, so `sin = cos, cos = sin` swaps the two. Rebuilt canonically.
/// An applied name with other than one argument is refused.
fn rename_functions(s: &Sym, renames: &[(String, String)]) -> Result<Sym, SymError> {
    let rec = |t: &Sym| rename_functions(t, renames);
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => Ok(s.clone()),
        Sym::Func(n, args) => {
            let args = args.iter().map(rec).collect::<Result<Vec<_>, _>>()?;
            match renames.iter().find(|(from, _)| from == n) {
                Some((_, to)) => {
                    if args.len() != 1 {
                        return Err(SymError::Unsupported("substitute of a function".into()));
                    }
                    Sym::func(to, args)
                }
                None => Sym::func(n, args),
            }
        }
        Sym::Add(ts) => Sym::sum(ts.iter().map(rec).collect::<Result<Vec<_>, _>>()?),
        Sym::Mul(ts) => Sym::product(ts.iter().map(rec).collect::<Result<Vec<_>, _>>()?),
        Sym::Pow(b, e) => Sym::pow(rec(b)?, e.clone()),
    }
}

/// Whether a function applied in `s` has one of `names`.
fn applies_any(s: &Sym, names: &[&str]) -> bool {
    match s {
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => false,
        Sym::Func(n, args) => {
            names.contains(&n.as_str()) || args.iter().any(|a| applies_any(a, names))
        }
        Sym::Add(ts) | Sym::Mul(ts) => ts.iter().any(|t| applies_any(t, names)),
        Sym::Pow(b, _) => applies_any(b, names),
    }
}

/// Rounds every number of an answer to `digits` significant figures (Prime's `float n`).
///
/// A rational is rounded exactly. A surd or other exact expression (`sqrt(5)`, the roots of
/// `x^2+x-1`) is evaluated in double precision and the binary value of that double is rounded
/// exactly, so only the first 15 or so digits are meaningful (`digits` is at most 15, see
/// [`MAX_FLOAT_DIGITS`]). A unit, a symbol or a function is refused.
fn round_answer(answer: Answer, digits: u32) -> Result<Answer, EvalError> {
    let round = |s: Sym| -> Result<Sym, EvalError> {
        if symbols(&s).is_empty() {
            round_number(&s, digits)
        } else {
            round_coefficients(&s, digits)
        }
    };
    // Prime's `float` on a vector of unknowns gives a row: the samples store `SP^T` (a column)
    // for `SP:=(... -> solve, x, y) -> float, 3` (11_rata_AA r8, 8_rata_18 r7), and the sheets
    // transpose `I` and `SP` before using them as columns.
    match answer {
        Answer::One(s) => Ok(Answer::One(round(s)?)),
        Answer::Column(values) => Ok(Answer::Row(
            values.into_iter().map(round).collect::<Result<_, _>>()?,
        )),
        Answer::Row(values) => Ok(Answer::Row(
            values.into_iter().map(round).collect::<Result<_, _>>()?,
        )),
        Answer::Grid(rows, cols, values) => Ok(Answer::Grid(
            rows,
            cols,
            values.into_iter().map(round).collect::<Result<_, _>>()?,
        )),
    }
}

/// Rounds a number-valued answer (no names) to `digits` significant figures: a rational exactly,
/// anything else through its double.
fn round_number(s: &Sym, digits: u32) -> Result<Sym, EvalError> {
    match s {
        Sym::Num(q) => Ok(Sym::Num(core(round_significant(q, digits))?)),
        other => {
            let x = approximate(other).ok_or_else(|| unsupported("float of a symbolic result"))?;
            let exact = from_f64_exact(x).ok_or_else(|| unsupported("float: not finite"))?;
            Ok(Sym::Num(core(round_significant(&exact, digits))?))
        }
    }
}

/// Rounds the numbers of an answer that still has names (`a*x + b` with `x` free): every number
/// (a coefficient) is rounded to `digits` significant figures, and a subtree without names is
/// rounded as a whole ([`round_number`]). The names and the structure stay, so the answer can be
/// bound to values later (see `body_value`).
fn round_coefficients(s: &Sym, digits: u32) -> Result<Sym, EvalError> {
    if symbols(s).is_empty() {
        return round_number(s, digits);
    }
    let each = |ts: &[Sym]| -> Result<Vec<Sym>, EvalError> {
        ts.iter().map(|t| round_coefficients(t, digits)).collect()
    };
    Ok(match s {
        Sym::Add(ts) => Sym::Add(each(ts)?),
        Sym::Mul(ts) => Sym::Mul(each(ts)?),
        Sym::Func(name, args) => Sym::Func(name.clone(), each(args)?),
        Sym::Pow(base, exp) => Sym::Pow(Box::new(round_coefficients(base, digits)?), exp.clone()),
        Sym::Num(_) | Sym::Symbol(_) | Sym::Unit(_) => s.clone(),
    })
}

/// The core's answer: one value, or a column of values in unknown order (`Row` after `float`), or
/// a matrix of `rows` x `cols` values in row-major order (a symbolic matrix command).
enum Answer {
    One(Sym),
    Column(Vec<Sym>),
    Row(Vec<Sym>),
    Grid(usize, usize, Vec<Sym>),
}

fn compute(request: &Request, prepared: &Expr) -> Result<Answer, EvalError> {
    match request {
        Request::Simplify => {
            let s = core(simplify(&core(from_expr(prepared))?))?;
            Ok(Answer::One(core(simplify_identities(
                &s,
                &Assumptions::default(),
            ))?))
        }
        Request::Assumed(asm) => {
            let s = core(simplify(&core(from_expr(prepared))?))?;
            Ok(Answer::One(core(simplify_identities(&s, asm))?))
        }
        Request::Expand => Ok(Answer::One(core(expand_trig(&core(from_expr(prepared))?))?)),
        Request::Rewrite(modifier) => {
            let s = core(simplify(&core(from_expr(prepared))?))?;
            Ok(Answer::One(core(rewrite_as(&s, modifier))?))
        }
        Request::Factor => {
            let s = core(simplify(&core(from_expr(prepared))?))?;
            Ok(Answer::One(core(factor(&s))?))
        }
        Request::Float(_) => Err(unsupported("nested float command")),
        Request::Substitute(_) => Err(unsupported("substitute inside another command")),
        Request::Series { .. } => Err(unsupported("series inside another command")),
        Request::Rectangular => Err(unsupported("rectangular inside another command")),
        Request::Transform {
            inverse,
            from,
            to,
            z,
            fourier: is_fourier,
        } => {
            let s = core(from_expr(prepared))?;
            let result = if *is_fourier {
                if *inverse {
                    let from = from.as_deref().unwrap_or("ω");
                    let to = to.as_deref().unwrap_or("t");
                    core(inverse_fourier(&s, from, to))?
                } else {
                    let from = from.as_deref().unwrap_or("t");
                    let to = to.as_deref().unwrap_or("ω");
                    core(fourier(&s, from, to))?
                }
            } else if *z {
                let s = core(simplify(&s))?;
                if *inverse {
                    let from = from.as_deref().unwrap_or("z");
                    let to = to.as_deref().unwrap_or("n");
                    core(inverse_ztrans(&s, from, to))?
                } else {
                    let from = from.as_deref().unwrap_or("n");
                    let to = to.as_deref().unwrap_or("z");
                    core(ztrans(&s, from, to))?
                }
            } else if *inverse {
                let s = core(simplify(&s))?;
                let from = from.as_deref().unwrap_or("s");
                let to = to.as_deref().unwrap_or("t");
                core(inverse_laplace(&s, from, to))?
            } else {
                let s = core(simplify(&s))?;
                let from = from.as_deref().unwrap_or("t");
                let to = to.as_deref().unwrap_or("s");
                core(laplace(&s, from, to))?
            };
            Ok(Answer::One(core(simplify(&result))?))
        }
        Request::Parfrac(name) => {
            let s = core(simplify(&core(from_expr(prepared))?))?;
            let var = variable_of(&s, name.as_deref())?;
            Ok(Answer::One(core(parfrac(&s, &var))?))
        }
        Request::Collect(name) => {
            let expanded = core(expand(&core(from_expr(prepared))?))?;
            let var = variable_of(&expanded, name.as_deref())?;
            Ok(Answer::One(core(collect(&expanded, &var))?))
        }
        Request::Coeffs(name) => {
            let expanded = core(expand(&core(from_expr(prepared))?))?;
            let var = variable_of(&expanded, name.as_deref())?;
            Ok(Answer::Column(core(coefficients(&expanded, &var))?))
        }
        Request::Solve(unknowns) => {
            if unknowns.len() > 1 || matches!(prepared, Expr::Matrix { .. }) {
                solve_system(prepared, unknowns)
            } else {
                let eq = core(equation_from_expr(prepared))?;
                let (mut roots, conditions) = match solve_polynomial_complex(&eq, &unknowns[0]) {
                    Ok(found) => found,
                    // Not polynomial in the unknown: an exponential or logarithm of it may
                    // be (every root is checked numerically), else the first error stands.
                    Err(first @ SymError::Unsupported(_)) => {
                        match solve_transcendental(&eq, &unknowns[0]) {
                            Ok(roots) => (roots, Vec::new()),
                            Err(_) => return Err(core_error(first)),
                        }
                    }
                    Err(other) => return Err(core_error(other)),
                };
                refuse_conditions(&conditions)?;
                match roots.len() {
                    0 => Err(unsupported("symbolic solve without a solution")),
                    1 => Ok(Answer::One(roots.remove(0))),
                    _ => Ok(Answer::Column(roots)),
                }
            }
        }
    }
}

fn solve_system(prepared: &Expr, unknowns: &[String]) -> Result<Answer, EvalError> {
    let eqs = core(system_from_expr(prepared))?;
    let (outcome, conditions) = match solve_linear_system_with_conditions(&eqs, unknowns) {
        Ok(found) => found,
        Err(_) => return solve_nonlinear_system(&eqs, unknowns),
    };
    match outcome {
        SolveOutcome::Unique(pairs) => {
            refuse_conditions(&conditions)?;
            let mut column = Vec::with_capacity(unknowns.len());
            for name in unknowns {
                let value = pairs
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| unsupported(format!("no solution for `{name}`")))?;
                column.push(value);
            }
            Ok(Answer::Column(column))
        }
        SolveOutcome::NotLinear => solve_nonlinear_system(&eqs, unknowns),
        other => Err(unsupported(format!("symbolic system gives {other:?}"))),
    }
}

/// A system that is not linear, by triangular elimination (see `polysys.rs`). One solution is a
/// column in the order of the unknowns. Several solutions need a multi-row answer format that is
/// not implemented, so they are refused rather than printed in a guessed shape.
fn solve_nonlinear_system(eqs: &[Equation], unknowns: &[String]) -> Result<Answer, EvalError> {
    let mut rows = core(solve_triangular(eqs, unknowns))?;
    match rows.len() {
        0 => Err(unsupported("symbolic system without a solution")),
        1 => Ok(Answer::Column(rows.remove(0))),
        _ => Err(unsupported("symbolic system with several solutions")),
    }
}

/// The answer as a value. Without a free name it is evaluated (a number with its unit). With one,
/// or with a `factor` that shows prime powers (`factored`), it is a [`Symbolic`] value.
fn finish(answer: Answer, factored: bool, ctx: Ctx<'_>) -> Result<Value, EvalError> {
    let (shape, values) = match answer {
        Answer::One(s) => (None, vec![s]),
        Answer::Column(values) => (Some((values.len(), 1)), values),
        Answer::Row(values) => (Some((1, values.len())), values),
        Answer::Grid(rows, cols, values) => (Some((rows, cols)), values),
    };
    if values.iter().any(|s| free_name(s).is_some()) || (factored && shows_factors(&values)?) {
        return symbolic(shape, values, factored).map(|s| Value::Symbolic(Box::new(s)));
    }
    let expr = match shape {
        None => to_expr(&values[0]),
        Some((rows, cols)) => Expr::matrix(rows, cols, values.iter().map(to_expr).collect()),
    };
    evaluate_budget(&expr, ctx.env, ctx.budget).map_err(finish_error)
}

/// Whether the factored print of any value differs from its plain print (`12` is `2^2*3`).
fn shows_factors(values: &[Sym]) -> Result<bool, EvalError> {
    for s in values {
        let plain = core(shown_expr(s, false))?;
        if core(shown_expr(s, true))?.to_string() != plain.to_string() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The symbolic value of `values` in `shape` (`None` for a scalar).
fn symbolic(
    shape: Option<(usize, usize)>,
    values: Vec<Sym>,
    factored: bool,
) -> Result<Symbolic, EvalError> {
    let shown = |s: &Sym| core(shown_expr(s, factored));
    let (rows, cols, expr) = match shape {
        None => (1, 1, shown(&values[0])?),
        Some((rows, cols)) => {
            let elements = values.iter().map(shown).collect::<Result<Vec<_>, _>>()?;
            (rows, cols, Expr::matrix(rows, cols, elements))
        }
    };
    Ok(Symbolic {
        rows,
        cols,
        entries: values,
        shown: expr,
    })
}

/// The variable a `collect` or `coeffs` acts on: the one named, or the only symbol of `e`. An
/// expression without symbols gives the empty name, which no symbol has.
fn variable_of(e: &Sym, name: Option<&str>) -> Result<String, EvalError> {
    if let Some(name) = name {
        return Ok(name.to_string());
    }
    match symbols(e).as_slice() {
        [] => Ok(String::new()),
        [only] => Ok(only.clone()),
        _ => Err(unsupported(
            "symbolic command needs a variable when the expression has several",
        )),
    }
}

/// Budget and depth errors are kept; anything else from the bridge is `Unsupported`.
fn finish_error(e: EvalError) -> EvalError {
    match e {
        EvalError::WorkLimit(_)
        | EvalError::CallLimit(_)
        | EvalError::DepthLimit(_)
        | EvalError::TooLarge(_) => e,
        other => unsupported(format!("symbolic result: {other}")),
    }
}

/// A generic answer that assumes a symbolic pivot or leading coefficient is nonzero is not the
/// answer for every value, so it is refused rather than returned without its condition.
fn refuse_conditions(conditions: &[Sym]) -> Result<(), EvalError> {
    if conditions.is_empty() {
        Ok(())
    } else {
        Err(unsupported(
            "symbolic solve assumes a nonzero symbolic coefficient",
        ))
    }
}

fn core<T>(r: Result<T, SymError>) -> Result<T, EvalError> {
    r.map_err(|e| unsupported(e.to_string()))
}

fn unsupported(text: impl Into<String>) -> EvalError {
    EvalError::Unsupported(text.into())
}

fn free_name(s: &Sym) -> Option<&str> {
    match s {
        Sym::Symbol(name) => Some(name),
        Sym::Num(_) | Sym::Unit(_) => None,
        Sym::Func(_, args) | Sym::Add(args) | Sym::Mul(args) => args.iter().find_map(free_name),
        Sym::Pow(base, _) => free_name(base),
    }
}

/// A function body being expanded: its definition (whose closure holds the names it sees), its
/// name as called, and its parameters bound to the prepared arguments.
struct Body<'a> {
    name: String,
    func: &'a UserFunction,
    params: Vec<(String, Expr)>,
}

/// What a name refers to while preparing: the worksheet, or the body of a function being expanded.
struct Scope<'a> {
    env: &'a Env,
    body: Option<Body<'a>>,
    /// Function bodies expanded on the way to this node (starts at the evaluator's call depth).
    depth: usize,
    /// Expression nodes already on the native stack when the evaluation starts (the evaluator's
    /// node depth), so nested evaluations share the one bound.
    nodes: usize,
}

/// Rewrites `e` into an expression the core can convert (see the module docs). `unknowns` are the
/// names a solve command solves for: they are never replaced, even when the worksheet defines them.
/// `nodes` is the recursion depth, bounded by [`MAX_NODE_DEPTH`].
fn prepare(
    e: &Expr,
    scope: &Scope<'_>,
    unknowns: &[String],
    budget: &Budget,
    nodes: usize,
) -> Result<Expr, EvalError> {
    if nodes > MAX_NODE_DEPTH {
        return Err(EvalError::DepthLimit(MAX_NODE_DEPTH));
    }
    budget.charge(NODE_UNITS)?;
    let next = nodes + 1;
    let rec = |x: &Expr| prepare(x, scope, unknowns, budget, next);
    match e {
        Expr::Number(_) | Expr::Str(_) | Expr::Placeholder => Ok(e.clone()),
        Expr::Id(id) => identifier(id, scope, unknowns, budget),
        Expr::Unary { op, operand } => {
            let child = rec(operand)?;
            let vector = is_vector(&child);
            settle(Expr::unary(*op, child), vector, scope, budget)
        }
        // `e^u` with a symbolic exponent keeps the name `e`, so the core writes it as exp(u)
        // exactly. A numeric exponent takes the ordinary path (the constant's value).
        Expr::Binary {
            op: BinaryOp::Pow,
            lhs,
            rhs,
        } if is_free_constant_e(lhs, scope, unknowns) => {
            let r = rec(rhs)?;
            if is_numeric(&r) {
                let l = rec(lhs)?;
                let vector = is_vector(&l) || is_vector(&r);
                return settle(Expr::binary(BinaryOp::Pow, l, r), vector, scope, budget);
            }
            let vector = is_vector(&r);
            settle(
                Expr::binary(BinaryOp::Pow, (**lhs).clone(), r),
                vector,
                scope,
                budget,
            )
        }
        Expr::Binary { op, lhs, rhs } => {
            let (l, r) = (rec(lhs)?, rec(rhs)?);
            let vector = is_vector(&l) || is_vector(&r);
            settle(Expr::binary(*op, l, r), vector, scope, budget)
        }
        Expr::Parens(inner) => Ok(Expr::Parens(Box::new(rec(inner)?))),
        Expr::Scale { value, unit } => Ok(Expr::Scale {
            value: Box::new(rec(value)?),
            unit: Box::new(rec(unit)?),
        }),
        Expr::NthRoot { degree, radicand } => Ok(Expr::NthRoot {
            degree: match degree.as_deref() {
                Some(d) => Some(Box::new(rec(d)?)),
                None => None,
            },
            radicand: Box::new(rec(radicand)?),
        }),
        Expr::Matrix {
            rows,
            cols,
            elements,
        } => Ok(Expr::Matrix {
            rows: *rows,
            cols: *cols,
            elements: elements.iter().map(rec).collect::<Result<Vec<_>, _>>()?,
        }),
        Expr::Index { base, indices } => {
            let base = rec(base)?;
            let indices = indices.iter().map(rec).collect::<Result<Vec<_>, _>>()?;
            select(base, indices, scope)
        }
        Expr::Eval {
            expr,
            unit_override: None,
        } => rec(expr),
        // A plain `expr ->` only simplifies, so inside a function body (`f(x) := g(x) ->`) it is
        // just its expression. A command inside an expression is not supported.
        Expr::SymEval {
            expr,
            command: None,
            ..
        } => rec(expr),
        Expr::Call { func, args } => {
            let key = var_key(func)?;
            match resolve_function(&key, scope)? {
                Some((name, f)) => expand_call(name, f, args, scope, unknowns, budget, next),
                None => Ok(Expr::Call {
                    func: func.clone(),
                    args: args.iter().map(rec).collect::<Result<Vec<_>, _>>()?,
                }),
            }
        }
        Expr::Eval { .. } => Err(unsupported("unit override inside a symbolic evaluation")),
        // A command applied to a command's answer (`(M -> solve, x) -> expand`): the inner answer
        // is computed, then becomes the input of the outer request as its expression.
        Expr::SymEval {
            command: Some(_), ..
        } => answer_expr(answer_of(e, scope, budget)?),
        Expr::Range { .. } => Err(unsupported("range inside a symbolic evaluation")),
        Expr::Define { .. } | Expr::GlobalDefine { .. } => {
            Err(unsupported("definition inside a symbolic evaluation"))
        }
        Expr::Program(_) => Err(unsupported(
            "programming region inside a symbolic evaluation",
        )),
        Expr::Derivative {
            var, order, body, ..
        } => derivative_expr(var, order.as_deref(), body, scope, unknowns, budget, next),
        // `∇ f` is a whole answer (a column), read at the top of the request; inside a larger
        // expression it has no symbolic form here.
        Expr::Gradient { .. } => Err(unsupported("gradient inside a symbolic expression")),
        Expr::Integral {
            var,
            lower,
            upper,
            body,
        } => integral_expr(var, lower, upper, body, scope, unknowns, budget, next),
        Expr::Accumulate { .. } | Expr::Fold { .. } => Err(unsupported(
            "calculus operator inside a symbolic evaluation",
        )),
        Expr::Limit { var, target, body } => {
            limit_expr(var, target, body, scope, unknowns, budget, next)
        }
    }
}

/// The sign of an infinite target: `∞` or `inf` is `+1`, `-∞` is `-1`; anything else is `None`.
fn infinity_sign(e: &Expr) -> Option<f64> {
    fn is_infinity(name: &str) -> bool {
        name == "∞" || name == "inf"
    }
    match e {
        Expr::Id(id) if is_infinity(&id.name) => Some(1.0),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => match &**operand {
            Expr::Id(id) if is_infinity(&id.name) => Some(-1.0),
            _ => None,
        },
        Expr::Parens(inner) => infinity_sign(inner),
        _ => None,
    }
}

/// `limit(x, a, f)`: the target is an infinity or a value (prepared in the caller's scope, without
/// the variable); the body is prepared with `var` as an unknown. The limit is exact or refused
/// (see the `limit` module of the core).
fn limit_expr(
    var: &Id,
    target: &Expr,
    body: &Expr,
    scope: &Scope<'_>,
    unknowns: &[String],
    budget: &Budget,
    nodes: usize,
) -> Result<Expr, EvalError> {
    let key = var_key(var)?;
    let point = match infinity_sign(target) {
        Some(sign) if sign > 0.0 => Point::PosInfinity,
        Some(_) => Point::NegInfinity,
        None => Point::Finite(core(from_expr(&prepare(
            target, scope, unknowns, budget, nodes,
        )?))?),
    };
    let mut names = unknowns.to_vec();
    names.push(key.clone());
    if let Expr::Binary {
        op: BinaryOp::Pow,
        lhs,
        rhs,
    } = strip(body)
    {
        let g = core(from_expr(&prepare(rhs, scope, &names, budget, nodes)?))?;
        if g.mentions_any(std::slice::from_ref(&key)) {
            let f = core(from_expr(&prepare(lhs, scope, &names, budget, nodes)?))?;
            return Ok(to_expr(&core(power_limit(&f, &g, &key, &point))?));
        }
    }
    let f = core(from_expr(&prepare(body, scope, &names, budget, nodes)?))?;
    Ok(to_expr(&core(limit_of(&f, &key, &point, Side::Both))?))
}

/// The limit of `f^g` where the exponent `g` depends on the variable (the core has no power with
/// a symbolic exponent). Two cases are answered, anything else is refused:
/// - `f -> 1`: `f^g = exp(g ln f)` and `ln f ~ f - 1`, so the limit is `exp(lim g (f - 1))`
///   (this covers `(1 + 1/x)^x -> e` and the `1^infinity` family; a finite `g` gives 1).
/// - `f -> L` a positive rational and `g -> c` a rational: the value `L^c`.
fn power_limit(f: &Sym, g: &Sym, var: &str, point: &Point) -> Result<Sym, SymError> {
    let base = limit_of(f, var, point, Side::Both)?;
    if base == Sym::int(1) {
        let h = g.clone().times(f.clone().minus(Sym::int(1))?)?;
        let rate = limit_of(&h, var, point, Side::Both)?;
        return if rate.is_zero() {
            Ok(Sym::int(1))
        } else {
            simplify(&Sym::func("exp", vec![rate])?)
        };
    }
    match (base.as_num(), limit_of(g, var, point, Side::Both)?.as_num()) {
        (Some(l), Some(c))
            if l.is_positive() && c.is_integer() && c.abs() <= Q::from_integer(64.into()) =>
        {
            simplify(&Sym::pow(Sym::Num(l.clone()), c.clone())?)
        }
        _ => Err(SymError::Unsupported(
            "limit of a power with a symbolic exponent that is not of the form 1^infinity".into(),
        )),
    }
}

/// `series`: the expression and the point are prepared; the variable is the named one (kept as an
/// unknown) or the only variable of the expression.
fn series_answer(
    var: Option<&str>,
    point: Option<&Expr>,
    terms: usize,
    expr: &Expr,
    scope: &Scope<'_>,
    budget: &Budget,
) -> Result<Answer, EvalError> {
    let unknowns: Vec<String> = var.map(|v| vec![v.to_string()]).unwrap_or_default();
    let main = core(from_expr(&prepare(
        expr,
        scope,
        &unknowns,
        budget,
        scope.nodes,
    )?))?;
    let var = match var {
        Some(v) => v.to_string(),
        None => variable_of(&main, None)?,
    };
    let at = match point {
        Some(p) => core(from_expr(&prepare(p, scope, &[], budget, scope.nodes)?))?,
        None => Sym::int(0),
    };
    Ok(Answer::One(core(series_of(&main, &var, &at, terms))?))
}

/// The largest order of a symbolic derivative (`derivative(x, n, f)`).
const MAX_DERIVATIVE_ORDER: usize = 8;

/// `∇ f`: the column of the exact partial derivatives of the body by each variable. Every variable
/// is an unknown in all the components, so `∇ f(a, b)` keeps `b` symbolic while differentiating by
/// `a`. The numbers of the worksheet are not substituted for the variables.
fn gradient_answer(
    gradient: &Expr,
    request: &Request,
    scope: &Scope<'_>,
    budget: &Budget,
) -> Result<Answer, EvalError> {
    let Expr::Gradient { vars, body } = gradient else {
        return Err(unsupported("gradient"));
    };
    let vars = super::calculus::gradient_variables(vars, body)?;
    let mut names = Vec::with_capacity(vars.len());
    for v in &vars {
        names.push(var_key(v)?);
    }
    let mut column = Vec::with_capacity(vars.len());
    for v in &vars {
        let f = derivative_expr(v, None, body, scope, &names, budget, scope.nodes)?;
        let sym = core(from_expr(&f))?;
        column.push(match request {
            Request::Expand => core(expand(&core(simplify(&sym))?))?,
            _ => {
                let s = core(simplify(&sym))?;
                core(simplify_identities(&s, &Assumptions::default()))?
            }
        });
    }
    Ok(Answer::Column(column))
}

/// `d/dx f` or the `n`th derivative: the body is prepared with `var` as an unknown (a symbol, never
/// a worksheet value), then differentiated `n` times exactly.
fn derivative_expr(
    var: &Id,
    order: Option<&Expr>,
    body: &Expr,
    scope: &Scope<'_>,
    unknowns: &[String],
    budget: &Budget,
    nodes: usize,
) -> Result<Expr, EvalError> {
    let times = match order {
        None => 1,
        Some(n) => integer_literal(n)
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= MAX_DERIVATIVE_ORDER)
            .ok_or_else(|| unsupported("symbolic derivative of this order"))?,
    };
    let key = var_key(var)?;
    let mut names = unknowns.to_vec();
    names.push(key.clone());
    let mut f = core(from_expr(&prepare(body, scope, &names, budget, nodes)?))?;
    for _ in 0..times {
        f = core(differentiate(&f, &key))?;
    }
    Ok(to_expr(&f))
}

/// `integral(x, a, b, f)`: the bounds are prepared in the caller's scope, the body with `var` as an
/// unknown, and the definite integral is exact or refused (see [`definite_integral`]).
#[allow(clippy::too_many_arguments)]
fn integral_expr(
    var: &Id,
    lower: &Expr,
    upper: &Expr,
    body: &Expr,
    scope: &Scope<'_>,
    unknowns: &[String],
    budget: &Budget,
    nodes: usize,
) -> Result<Expr, EvalError> {
    let key = var_key(var)?;
    let lo = core(from_expr(&prepare(lower, scope, unknowns, budget, nodes)?))?;
    let hi = core(from_expr(&prepare(upper, scope, unknowns, budget, nodes)?))?;
    let mut names = unknowns.to_vec();
    names.push(key.clone());
    let f = core(from_expr(&prepare(body, scope, &names, budget, nodes)?))?;
    Ok(to_expr(&core(definite_integral(&f, &key, &lo, &hi))?))
}

/// A nested command's answer as the expression that the outer request reads.
fn answer_expr(answer: Answer) -> Result<Expr, EvalError> {
    Ok(match answer {
        Answer::One(s) => to_expr(&s),
        Answer::Column(values) => {
            let n = values.len();
            Expr::matrix(n, 1, values.iter().map(to_expr).collect())
        }
        Answer::Row(values) => {
            let n = values.len();
            Expr::matrix(1, n, values.iter().map(to_expr).collect())
        }
        Answer::Grid(rows, cols, values) => {
            Expr::matrix(rows, cols, values.iter().map(to_expr).collect())
        }
    })
}

/// A name's prepared value: a worksheet value or a parameter, a captured value, or the name itself
/// (a constant, a unit, or a free name the core will refuse).
fn identifier(
    id: &Id,
    scope: &Scope<'_>,
    unknowns: &[String],
    budget: &Budget,
) -> Result<Expr, EvalError> {
    if id.kind == IdKind::Unit {
        return Ok(Expr::Id(id.clone()));
    }
    let key = var_key(id)?;
    // An unknown (a solve's, or a derivative's or integral's variable) is never replaced, even by a
    // parameter or a captured value of the same name.
    if unknowns.contains(&key) {
        // An unknown is a variable whatever kind the worksheet gave the name (`e` as a variable).
        return Ok(Expr::Id(Id {
            kind: IdKind::Variable,
            ..id.clone()
        }));
    }
    match &scope.body {
        Some(body) => {
            if let Some((_, arg)) = body.params.iter().find(|(p, _)| *p == key) {
                // Each use copies the prepared argument, so the copy is charged as work.
                budget.charge(NODE_UNITS * node_count(arg))?;
                return Ok(arg.clone());
            }
            let closure = &body.func.closure;
            if closure.range_variables.contains(&key) {
                return Err(range_in_expression());
            }
            if let Some(value) = closure.variables.get(&key) {
                return value_expr(value);
            }
            // A solve's unknown is never a constant (`c` in `[a,b,c] := ... -> solve, a, b, c`).
            if !closure.unbound_variables.contains(&key) && !unknowns.contains(&key) {
                if let Some(value) = constant(&key) {
                    return value_expr(&value?);
                }
            }
            // A name that was undefined when the body was defined stays a free name here, so a
            // solve can take it as an unknown (`y(x) := a*x^2+..` then `[a,b,c] := [...] -> solve`).
            // The core refuses a free name that is not solved for.
            Ok(Expr::Id(id.clone()))
        }
        None => {
            if unknowns.contains(&key) {
                return Ok(Expr::Id(id.clone()));
            }
            if scope.env.is_range(&key) {
                return Err(range_in_expression());
            }
            match scope.env.variable(&key) {
                Some(value) => value_expr(value),
                None => match constant(&key) {
                    Some(value) if !scope.env.is_failed(&key) => value_expr(&value?),
                    _ => Ok(Expr::Id(id.clone())),
                },
            }
        }
    }
}

fn strip(e: &Expr) -> &Expr {
    match e {
        Expr::Parens(inner) => strip(inner),
        other => other,
    }
}

/// Whether `e` is the constant `e` on the worksheet: not a variable the worksheet defines, not a
/// name the command solves for or differentiates (`derivative(e, e^x)`), and not inside a function
/// body.
fn is_free_constant_e(e: &Expr, scope: &Scope<'_>, unknowns: &[String]) -> bool {
    match e {
        Expr::Id(id) => {
            id.name == "e"
                && id.subscript.is_none()
                && id.kind != IdKind::Unit
                && scope.body.is_none()
                && scope.env.variable("e").is_none()
                && !unknowns.iter().any(|u| u == "e")
        }
        _ => false,
    }
}

/// The number of nodes in a prepared expression: the cost of copying it.
fn node_count(e: &Expr) -> u64 {
    1 + match e {
        Expr::Unary { operand, .. } => node_count(operand),
        Expr::Binary { lhs, rhs, .. } => node_count(lhs) + node_count(rhs),
        Expr::Parens(inner) => node_count(inner),
        Expr::Scale { value, unit } => node_count(value) + node_count(unit),
        Expr::Matrix { elements, .. } => elements.iter().map(node_count).sum(),
        Expr::Call { args, .. } => args.iter().map(node_count).sum(),
        Expr::Index { base, indices } => {
            node_count(base) + indices.iter().map(node_count).sum::<u64>()
        }
        Expr::NthRoot { degree, radicand } => {
            degree.as_deref().map_or(0, node_count) + node_count(radicand)
        }
        _ => 0,
    }
}

/// The function a call names: a user function (with its name as called), or `None` for a builtin.
/// Inside a body the callee comes from the body's closure, as the evaluator resolves it.
fn resolve_function<'a>(
    key: &str,
    scope: &Scope<'a>,
) -> Result<Option<(String, &'a UserFunction)>, EvalError> {
    let Some(body) = &scope.body else {
        return Ok(scope.env.function(key).map(|f| (key.to_string(), f)));
    };
    let func: &'a UserFunction = body.func;
    let closure = &func.closure;
    if let Some(g) = closure.functions.get(key) {
        return Ok(Some((key.to_string(), g.as_ref())));
    }
    if closure.recursive && key == body.name {
        return Err(unsupported(format!(
            "recursive function `{key}` in a symbolic evaluation"
        )));
    }
    if closure.unbound_functions.contains(key) {
        return Err(EvalError::UndefinedFunction(key.to_string()));
    }
    Ok(None)
}

/// Expands a call of user function `f`: the arguments are prepared in the caller's scope, then the
/// body is prepared with its parameters bound to them. Counts as one call in the budget.
fn expand_call(
    name: String,
    f: &UserFunction,
    args: &[Expr],
    scope: &Scope<'_>,
    unknowns: &[String],
    budget: &Budget,
    nodes: usize,
) -> Result<Expr, EvalError> {
    if args.len() != f.params.len() {
        return Err(unsupported(format!(
            "`{name}` takes {} arguments",
            f.params.len()
        )));
    }
    if scope.depth >= MAX_CALL_DEPTH {
        return Err(unsupported("symbolic function calls nested too deeply"));
    }
    budget.take_call()?;
    let mut params = Vec::with_capacity(args.len());
    for (param, arg) in f.params.iter().zip(args) {
        params.push((param.clone(), prepare(arg, scope, unknowns, budget, nodes)?));
    }
    let inner = Scope {
        env: scope.env,
        body: Some(Body {
            name,
            func: f,
            params,
        }),
        depth: scope.depth + 1,
        nodes,
    };
    prepare(&f.body, &inner, unknowns, budget, nodes)
}

/// `base[i]` or `base[i, j]`. Resolved when the base is a literal vector or matrix and the indices
/// are integer literals; otherwise the index is kept and the core refuses it.
fn select(base: Expr, indices: Vec<Expr>, scope: &Scope<'_>) -> Result<Expr, EvalError> {
    let literal: Option<Vec<i64>> = indices.iter().map(integer_literal).collect();
    let (
        Expr::Matrix {
            rows,
            cols,
            elements,
        },
        Some(positions),
    ) = (&base, literal)
    else {
        return Ok(Expr::Index {
            base: Box::new(base),
            indices,
        });
    };
    let origin = origin(scope)?;
    let picked = match positions.as_slice() {
        [k] if *rows == 1 || *cols == 1 => elements.get(zero_based(*k, origin)?),
        [i, j] => {
            let (i, j) = (zero_based(*i, origin)?, zero_based(*j, origin)?);
            if i < *rows && j < *cols {
                elements.get(i * cols + j)
            } else {
                None
            }
        }
        _ => None,
    };
    picked
        .cloned()
        .ok_or_else(|| unsupported("index outside a literal vector or matrix"))
}

/// `ORIGIN` as the current scope sees it.
fn origin(scope: &Scope<'_>) -> Result<i64, EvalError> {
    match &scope.body {
        None => scope.env.origin(),
        Some(body) => match body.func.closure.variables.get("ORIGIN").map(|v| &**v) {
            Some(Value::Scalar(s)) if s.value.im == 0.0 && s.value.re.fract() == 0.0 => {
                Ok(s.value.re as i64)
            }
            _ => Err(unsupported("ORIGIN inside a function body")),
        },
    }
}

/// The value of an integer literal, if `e` is one.
fn integer_literal(e: &Expr) -> Option<i64> {
    match e {
        Expr::Number(n) => {
            let x = number_value(n).ok()?;
            (x.fract() == 0.0 && x.abs() < 1e15).then_some(x as i64)
        }
        _ => None,
    }
}

fn is_vector(e: &Expr) -> bool {
    matches!(e, Expr::Matrix { .. })
}

/// Settles a node with a vector operand: it is evaluated numerically (the operands must hold no
/// names), so `(A+B)/2` becomes a literal vector. Other nodes are returned unchanged.
fn settle(node: Expr, vector: bool, scope: &Scope<'_>, budget: &Budget) -> Result<Expr, EvalError> {
    if !vector {
        return Ok(node);
    }
    if !is_numeric(&node) {
        return Err(unsupported("vector arithmetic with a symbolic part"));
    }
    evaluate_budget(&node, scope.env, budget)
        .map_err(finish_error)
        .and_then(|v| value_expr(&v))
}

/// Whether `e` holds numbers and units only (no names, no calls).
fn is_numeric(e: &Expr) -> bool {
    match e {
        Expr::Number(_) => true,
        Expr::Id(id) => id.kind == IdKind::Unit,
        Expr::Unary { operand, .. } => is_numeric(operand),
        Expr::Binary { lhs, rhs, .. } => is_numeric(lhs) && is_numeric(rhs),
        Expr::Parens(inner) => is_numeric(inner),
        Expr::Scale { value, unit } => is_numeric(value) && is_numeric(unit),
        Expr::Matrix { elements, .. } => elements.iter().all(is_numeric),
        _ => false,
    }
}

/// A value as an expression: a real number times its unit monomial, or a literal matrix of them.
fn value_expr(v: &Value) -> Result<Expr, EvalError> {
    match v {
        Value::Scalar(s) => scalar_expr(s),
        Value::Matrix(m) => {
            let (rows, cols) = m.shape();
            let elements = m
                .data()
                .iter()
                .map(scalar_expr)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Expr::matrix(rows, cols, elements))
        }
        Value::Str(_) => Err(unsupported("text inside a symbolic evaluation")),
        Value::List(_) => Err(unsupported("a nested array inside a symbolic evaluation")),
        Value::Func(_) => Err(unsupported("a function value inside a symbolic evaluation")),
        // A symbolic result read back into a later evaluation is its canonical value.
        Value::Symbolic(s) => {
            if s.rows * s.cols == 1 {
                Ok(to_expr(&s.entries[0]))
            } else {
                let elements = s.entries.iter().map(to_expr).collect();
                Ok(Expr::matrix(s.rows, s.cols, elements))
            }
        }
    }
}

fn scalar_expr(s: &Scalar) -> Result<Expr, EvalError> {
    let x = s.value.re;
    if s.value.im != 0.0 || !x.is_finite() {
        return Err(unsupported(
            "complex or non-finite value inside a symbolic evaluation",
        ));
    }
    // `{}` on an f64 is the shortest decimal that reads back to the same value (no exponent).
    let magnitude = Expr::Number(Number::Real {
        value: x.abs(),
        text: format!("{}", x.abs()),
    });
    let number = match unit_expr(s.dim) {
        Some(units) => Expr::binary(BinaryOp::Mul, magnitude, units),
        None => magnitude,
    };
    Ok(if x < 0.0 {
        Expr::unary(UnaryOp::Neg, number)
    } else {
        number
    })
}

/// The base-unit monomial of a dimension (`kg m s^-2` for a force), or `None` when dimensionless.
fn unit_expr(dim: Dimension) -> Option<Expr> {
    let mut acc: Option<Expr> = None;
    for (symbol, exp) in BASE_SYMBOLS.iter().zip(dim.exponents()) {
        if *exp.numer() == 0 {
            continue;
        }
        let unit = Expr::Id(Id::new(symbol, IdKind::Unit));
        let term = if *exp == Ratio::from_integer(1) {
            unit
        } else {
            // Parsed from `n/d` text: the core's `Q` has its own `num-bigint`, not this one.
            let power: Q = format!("{}/{}", exp.numer(), exp.denom()).parse().ok()?;
            Expr::binary(BinaryOp::Pow, unit, Expr::Number(Number::Rational(power)))
        };
        acc = Some(match acc {
            None => term,
            Some(a) => Expr::binary(BinaryOp::Mul, a, term),
        });
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{Outcome, Session};

    fn sym(expr: Expr, words: Option<&[&str]>) -> Expr {
        Expr::SymEval {
            expr: Box::new(expr),
            command: words.map(|w| Command(w.iter().map(|s| Expr::var(s)).collect())),
            result: None,
        }
    }

    fn eq(lhs: Expr, rhs: Expr) -> Expr {
        Expr::binary(BinaryOp::Eq, lhs, rhs)
    }

    fn define(name: &str, expr: Expr) -> Expr {
        Expr::Define {
            target: Box::new(Expr::var(name)),
            expr: Box::new(expr),
        }
    }

    fn real_of(out: &Outcome) -> f64 {
        match out {
            Outcome::Value(Value::Scalar(s)) => s.value.re,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn plain_arrow_computes_an_exact_rational() {
        let e = Expr::binary(
            BinaryOp::Div,
            Expr::binary(BinaryOp::Add, Expr::num(3.0), Expr::num(4.0)),
            Expr::binary(
                BinaryOp::Sub,
                Expr::binary(BinaryOp::Pow, Expr::num(5.0), Expr::num(2.0)),
                Expr::num(1.0),
            ),
        );
        let out = Session::new().run(&sym(e, None));
        assert!((real_of(&out) - 7.0 / 24.0).abs() < 1e-15, "{out:?}");
    }

    #[test]
    fn solve_substitutes_a_worksheet_value() {
        // k := 2, then k*x + 1 = 5 solved for x gives 2.
        let mut s = Session::new();
        s.run(&define("k", Expr::num(2.0)));
        let lhs = Expr::binary(
            BinaryOp::Add,
            Expr::binary(BinaryOp::Mul, Expr::var("k"), Expr::var("x")),
            Expr::num(1.0),
        );
        let out = s.run(&sym(eq(lhs, Expr::num(5.0)), Some(&["solve", "x"])));
        assert!((real_of(&out) - 2.0).abs() < 1e-15, "{out:?}");
    }

    fn float_of(e: Expr, digits: f64) -> Outcome {
        let float = Expr::SymEval {
            expr: Box::new(e),
            command: Some(Command(vec![Expr::var("float"), Expr::num(digits)])),
            result: None,
        };
        Session::new().run(&float)
    }

    #[test]
    fn float_rounds_a_rational_half_away_from_zero() {
        // help: 7.5675 float,4 -> 7.568 and -7.5675 -> -7.568
        let out = float_of(Expr::num(7.5675), 4.0);
        assert!((real_of(&out) - 7.568).abs() < 1e-12, "{out:?}");
        let neg = Expr::unary(UnaryOp::Neg, Expr::num(7.5675));
        let out = float_of(neg, 4.0);
        assert!((real_of(&out) + 7.568).abs() < 1e-12, "{out:?}");
    }

    #[test]
    fn float_of_a_surd_rounds_its_double_value() {
        // help: sqrt(5) float,6 -> 2.23607 (the double value is exact to 15 digits)
        let root5 = Expr::NthRoot {
            degree: None,
            radicand: Box::new(Expr::num(5.0)),
        };
        let out = float_of(root5, 6.0);
        assert!((real_of(&out) - 2.23607).abs() < 1e-12, "{out:?}");
    }

    #[test]
    fn solve_into_a_vector_defines_a_column_in_unknown_order() {
        // [x + y = 3; x - y = 1] solved for x, y gives the column (2; 1).
        let rows = Expr::matrix(
            2,
            1,
            vec![
                eq(
                    Expr::binary(BinaryOp::Add, Expr::var("x"), Expr::var("y")),
                    Expr::num(3.0),
                ),
                eq(
                    Expr::binary(BinaryOp::Sub, Expr::var("x"), Expr::var("y")),
                    Expr::num(1.0),
                ),
            ],
        );
        let mut s = Session::new();
        let out = s.run(&define("U", sym(rows, Some(&["solve", "x", "y"]))));
        assert!(matches!(out, Outcome::Defined { .. }), "{out:?}");
        match s.env().variable("U") {
            Some(Value::Matrix(m)) => {
                assert_eq!(m.shape(), (2, 1));
                assert!((m.data()[0].value.re - 2.0).abs() < 1e-15);
                assert!((m.data()[1].value.re - 1.0).abs() < 1e-15);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn solved_value_keeps_its_units() {
        // 0 = R2 - F with F := 4 N gives R2 = 4 N: a force of 4.
        let mut s = Session::new();
        let newton = Expr::Id(Id::new("N", IdKind::Unit));
        s.run(&define(
            "F",
            Expr::Scale {
                value: Box::new(Expr::num(4.0)),
                unit: Box::new(newton),
            },
        ));
        let rhs = Expr::binary(BinaryOp::Sub, Expr::var("R2"), Expr::var("F"));
        let out = s.run(&sym(eq(Expr::num(0.0), rhs), Some(&["solve", "R2"])));
        match out {
            Outcome::Value(Value::Scalar(v)) => {
                assert!((v.value.re - 4.0).abs() < 1e-12, "{v:?}");
                assert_eq!(v.dim, Dimension::from_ints([1, 1, -2, 0, 0, 0, 0]));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_free_name_gives_a_symbolic_result() {
        // `a` is not defined and is not solved for, so x = 2 - a is a symbolic value.
        let lhs = Expr::binary(BinaryOp::Add, Expr::var("a"), Expr::var("x"));
        let out = Session::new().run(&sym(eq(lhs, Expr::num(2.0)), Some(&["solve", "x"])));
        match out {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries, vec![s_of("2 - a")]);
                assert_eq!((s.rows, s.cols), (1, 1));
            }
            other => panic!("{other:?}"),
        }
    }

    /// The outcome of a symbolic command written as text (`x^2 -> expand`), in a new worksheet.
    fn run_text(src: &str) -> Outcome {
        let expr = crate::parser::parse(src, &crate::parser::DefaultIdContext)
            .unwrap_or_else(|e| panic!("{src}: {e:?}"));
        Session::new().run(&expr)
    }

    /// The printed form of a symbolic result without spaces (the help's `2*x + 1` and our
    /// `2*x+1` are the same text; the sheet draws the math).
    fn shown(src: &str) -> String {
        match run_text(src) {
            Outcome::Value(Value::Symbolic(s)) => compact(&s.shown.to_string()),
            other => panic!("{src}: {other:?}"),
        }
    }

    fn compact(text: &str) -> String {
        text.chars().filter(|c| *c != ' ').collect()
    }

    /// `expr -> words`, with the words built directly (the text parser does not read `=` in a
    /// command: the importer builds `x = 13` from the Boolean equal sign).
    fn with_words(src: &str, words: Vec<Expr>) -> Outcome {
        let expr = crate::parser::parse(src, &crate::parser::DefaultIdContext)
            .unwrap_or_else(|e| panic!("{src}: {e:?}"));
        Session::new().run(&Expr::SymEval {
            expr: Box::new(expr),
            command: Some(Command(words)),
            result: None,
        })
    }

    fn assign(name: &str, value: &str) -> Expr {
        Expr::binary(
            BinaryOp::Eq,
            Expr::var(name),
            crate::parser::parse(value, &crate::parser::DefaultIdContext).expect("parses"),
        )
    }

    /// The canonical value of a source expression (for entries that are not printed).
    fn s_of(src: &str) -> Sym {
        crate::symbolic::from_expr(
            &crate::parser::parse(src, &crate::parser::DefaultIdContext).expect("parses"),
        )
        .expect("converts")
    }

    #[test]
    fn help_collect_single_variable_is_descending() {
        // help: x^2 (5 x^2 + 10 x + 1) collect -> 5 x^4 + 10 x^3 + x^2
        assert_eq!(shown("x^2*(5*x^2+10*x+1) -> collect"), "5*x^4+10*x^3+x^2");
    }

    #[test]
    fn help_collect_by_y_matches_the_help_and_collect_by_x_keeps_the_value() {
        // help: 3 x^2 y + 5 x^2 y^2 + 7 x y^2 + x y, collect,y -> (5 x^2 + 7 x) y^2 + (3 x^2 + x) y.
        // The terms are the same, but the help puts the coefficient before the power and we print
        // the power first (not reproduced: the help's factor order is not one rule we can derive).
        let p = "3*x^2*y+5*x^2*y^2+7*x*y^2+x*y";
        assert_eq!(
            shown(&format!("{p} -> collect, y")),
            "y^2*(5*x^2+7*x)+y*(3*x^2+x)"
        );
        // help: collect,x gives x (7 y^2 + y) + x^2 (5 y^2 + 3 y). The terms are the same, but our
        // canonical order puts the higher power of x first (the help's order is not reproduced).
        assert_eq!(
            shown(&format!("{p} -> collect, x")),
            "x^2*(5*y^2+3*y)+x*(7*y^2+y)"
        );
    }

    #[test]
    fn help_coeffs_is_a_column_lowest_power_first() {
        // help: x^3 + 3 x^2 + 7 x coeffs -> [0; 7; 3; 1]
        assert!(matches!(
            run_text("x^3+3*x^2+7*x -> coeffs"),
            Outcome::Value(Value::Matrix(m)) if m.shape() == (4, 1)
                && m.data().iter().map(|s| s.value.re).collect::<Vec<_>>() == [0.0, 7.0, 3.0, 1.0]
        ));
    }

    #[test]
    fn help_coeffs_of_a_two_variable_polynomial_stays_symbolic() {
        // help: 3 x^2 y + 5 x^2 y^2 + 7 x y^2 + x y coeffs,x -> [0; 7 y^2 + y; 5 y^2 + 3 y]
        match run_text("3*x^2*y+5*x^2*y^2+7*x*y^2+x*y -> coeffs, x") {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!((s.rows, s.cols), (3, 1));
                assert_eq!(compact(&s.shown.to_string()), "[0;7*y^2+y;5*y^2+3*y]");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn help_substitute_numbers_and_names() {
        // help: x + y -> substitute, x = 13 gives y + 13.
        match with_words("x+y", vec![Expr::var("substitute"), assign("x", "13")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(compact(&s.shown.to_string()), "y+13"),
            other => panic!("{other:?}"),
        }
        // help: x + y with x = 13, y = 12 gives 25.
        assert!(matches!(
            with_words(
                "x+y",
                vec![
                    Expr::var("substitute"),
                    assign("x", "13"),
                    assign("y", "12"),
                ]
            ),
            Outcome::Value(Value::Scalar(s)) if (s.value.re - 25.0).abs() < 1e-12
        ));
        // help: 5 x^2 + 3 x + 9 with x = 2 gives 35.
        assert!(matches!(
            with_words("5*x^2+3*x+9", vec![Expr::var("substitute"), assign("x", "2")]),
            Outcome::Value(Value::Scalar(s)) if (s.value.re - 35.0).abs() < 1e-12
        ));
        // help: x = s + t gives 5 t^2 + (10 s + 3) t + 5 s^2 + 3 s + 9: the same value, in the
        // canonical form (the help's grouping is not reproduced).
        let expected = crate::symbolic::from_expr(
            &crate::parser::parse(
                "5*t^2+10*s*t+3*t+5*s^2+3*s+9",
                &crate::parser::DefaultIdContext,
            )
            .expect("parses"),
        )
        .expect("converts");
        match with_words(
            "5*x^2+3*x+9",
            vec![Expr::var("substitute"), assign("x", "s+t")],
        ) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], expected),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn substitute_of_a_function_renames_elementary_functions_only() {
        // help: cos(x) -> substitute, cos = sin is sin(x).
        match with_words(
            "cos(x)",
            vec![Expr::var("substitute"), assign("cos", "sin")],
        ) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("sin(x)")),
            other => panic!("{other:?}"),
        }
        // Inside a larger expression, and with a variable in the same command.
        match with_words(
            "x*cos(x)+cos(2*y)",
            vec![
                Expr::var("substitute"),
                assign("cos", "tan"),
                assign("y", "3"),
            ],
        ) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("x*tan(x)+tan(6)"))
            }
            other => panic!("{other:?}"),
        }
        // A function renamed to an unknown name, or a function name not in the expression that is
        // applied nowhere, is not silently dropped.
        for (src, from, to) in [("cos(x)", "cos", "foo"), ("foo(x)", "foo", "sin")] {
            let out = with_words(src, vec![Expr::var("substitute"), assign(from, to)]);
            assert!(
                matches!(out, Outcome::Error(EvalError::Unsupported(_))),
                "{src}: {out:?}"
            );
        }
    }

    #[test]
    fn fourier_pairs_without_distributions() {
        // exp(-t^2) has a Gaussian transform; the Dirac delta pairs of the help are refused.
        let out = with_words("exp(-t^2)", vec![Expr::var("fourier")]);
        assert!(matches!(out, Outcome::Value(Value::Symbolic(_))), "{out:?}");
        let out = with_words("1/(t^2+1)", vec![Expr::var("fourier"), Expr::var("t")]);
        assert!(matches!(out, Outcome::Value(Value::Symbolic(_))), "{out:?}");
        for src in ["1", "sin(t)"] {
            let out = with_words(src, vec![Expr::var("fourier")]);
            assert!(
                matches!(out, Outcome::Error(EvalError::Unsupported(_))),
                "{src}: {out:?}"
            );
        }
    }

    #[test]
    fn substitute_of_an_applied_function_by_a_body() {
        let call = |name: &str, params: &[&str]| {
            Expr::binary(
                BinaryOp::Eq,
                Expr::call(name, params.iter().map(|p| Expr::var(p)).collect()),
                crate::parser::parse("x^2+1", &crate::parser::DefaultIdContext).expect("parses"),
            )
        };
        // f(a+1) + f(2) with f(x) = x^2+1.
        match with_words(
            "f(a+1)+f(2)",
            vec![Expr::var("substitute"), call("f", &["x"])],
        ) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("a^2+2*a+7"));
            }
            other => panic!("{other:?}"),
        }
        // Wrong arity, and a function that is not applied, are refused.
        for (src, words) in [("f(1,2)", call("f", &["x"])), ("g(1)", call("f", &["x"]))] {
            let out = with_words(src, vec![Expr::var("substitute"), words]);
            assert!(
                matches!(out, Outcome::Error(EvalError::Unsupported(_))),
                "{src}: {out:?}"
            );
        }
    }

    /// `name(params) := rhs` as a `substitute` word.
    fn def(name: &str, params: &[&str], rhs: &str) -> Expr {
        Expr::binary(
            BinaryOp::Eq,
            Expr::call(name, params.iter().map(|p| Expr::var(p)).collect()),
            crate::parser::parse(rhs, &crate::parser::DefaultIdContext).expect("parses"),
        )
    }

    #[test]
    fn substitute_swaps_two_function_names_in_one_pass() {
        // sin = cos and cos = sin at once: sin(x) + cos(x) becomes cos(x) + sin(x), not 2 sin(x).
        match with_words(
            "sin(x)+cos(x)",
            vec![
                Expr::var("substitute"),
                assign("sin", "cos"),
                assign("cos", "sin"),
            ],
        ) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("cos(x)+sin(x)"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn substitute_of_several_parameters_takes_each_argument_as_written() {
        // f(x, y) = x - y accepts arguments that name the parameters themselves.
        let body = "x-y";
        let cases = [
            ("f(y,x)", "y-x"),
            ("f(2,x)", "2-x"),
            ("f(x,y)", "x-y"),
            ("f(y,2)", "y-2"),
        ];
        for (src, expected) in cases {
            match with_words(
                src,
                vec![Expr::var("substitute"), def("f", &["x", "y"], body)],
            ) {
                Outcome::Value(Value::Symbolic(s)) => {
                    assert_eq!(s.entries[0], s_of(expected), "{src}")
                }
                other => panic!("{src}: {other:?}"),
            }
        }
    }

    #[test]
    fn substitute_of_functions_applies_a_body_that_applies_another() {
        // f(x) = g(x) + 1 and g(x) = 2 x give f(a) = 2 a + 1, whichever order the pairs come in.
        for order in [[0usize, 1], [1, 0]] {
            let pairs = [def("f", &["x"], "g(x)+1"), def("g", &["x"], "2*x")];
            let mut words = vec![Expr::var("substitute")];
            words.extend(order.iter().map(|&i| pairs[i].clone()));
            match with_words("f(a)", words) {
                Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("2*a+1")),
                other => panic!("{order:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn substitute_of_a_recursive_function_is_refused() {
        let out = with_words(
            "f(1)",
            vec![Expr::var("substitute"), def("f", &["x"], "f(x)+1")],
        );
        assert!(
            matches!(out, Outcome::Error(EvalError::Unsupported(_))),
            "{out:?}"
        );
    }

    #[test]
    fn factor_of_an_integer_shows_prime_powers() {
        // help: factor(12) -> 2^2*3 (the value stays 12; only the printed form factors it).
        match run_text("12 -> factor") {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.shown.to_string(), "2^2*3");
                assert_eq!(s.entries, vec![s_of("12")]);
            }
            other => panic!("{other:?}"),
        }
        // A prime is shown as it is, as a number.
        assert!(matches!(
            run_text("7 -> factor"),
            Outcome::Value(Value::Scalar(_))
        ));
    }

    #[test]
    fn factor_of_a_polynomial_shows_its_factors() {
        // help: x^2 - 1 factors as (x - 1)(x + 1).
        assert_eq!(shown("x^2-1 -> factor"), "(x-1)*(x+1)");
        // 2 x^2 + 4 x = 2 x (x + 2).
        assert_eq!(shown("2*x^2+4*x -> factor"), "2*x*(x+2)");
        // help: (1/(x+2)) (2/(x+3)) -> factor is 2/((x+2)(x+3)).
        assert_eq!(shown("(1/(x+2))*(2/(x+3)) -> factor"), "2/((x+2)*(x+3))");
    }

    #[test]
    fn expand_and_plain_arrow_keep_names() {
        // (a+b)^2 expands to the three terms of 1_rata_1 r10 (order by the core's rule).
        assert_eq!(shown("(a+b)^2 -> expand"), "b^2+2*a*b+a^2");
        // 1_rata_1 r3: x^5 * 6 x^3 / (x^(1/2) x^(5/3)) simplifies to 6 x^(35/6).
        assert_eq!(shown("x^5*(6*x^3)/(x^(1/2)*x^(5/3)) ->"), "6*x^(35/6)");
    }

    #[test]
    fn simplify_keyword_is_the_plain_arrow() {
        assert_eq!(shown("(x^2-x)/x -> simplify"), shown("(x^2-x)/x ->"));
    }

    #[test]
    fn a_result_with_a_name_refuses_arithmetic() {
        // A symbolic result has no number: `+ 1` on it is refused, not computed.
        let mut s = Session::new();
        s.run(&define(
            "y",
            crate::parser::parse("x^2 -> expand", &crate::parser::DefaultIdContext)
                .expect("parses"),
        ));
        let out = s.run(&crate::parser::parse("y+1", &crate::parser::DefaultIdContext).unwrap());
        assert!(
            matches!(out, Outcome::Error(EvalError::Unsupported(_))),
            "{out:?}"
        );
    }

    #[test]
    fn self_referencing_symbolic_define_shows_but_stays_undefined() {
        // Ax := 0 == Ax -> solve, Ax shows 0 (as Prime does) and leaves Ax undefined.
        let mut s = Session::new();
        let out = s.run(&define(
            "Ax",
            sym(eq(Expr::num(0.0), Expr::var("Ax")), Some(&["solve", "Ax"])),
        ));
        match out {
            Outcome::Defined {
                display: Some(shown),
                ..
            } => assert!(matches!(*shown, Outcome::Value(_)), "{shown:?}"),
            other => panic!("{other:?}"),
        }
        assert!(s.env().variable("Ax").is_none());
    }

    #[test]
    fn unit_monomial_of_a_force_evaluates_to_a_force() {
        let force = Dimension::from_ints([1, 1, -2, 0, 0, 0, 0]);
        let units = unit_expr(force).expect("a force has base units");
        match evaluate_budget(&units, &Env::new(), &Budget::new()) {
            Ok(Value::Scalar(s)) => assert_eq!(s.dim, force),
            other => panic!("{other:?}"),
        }
        assert!(unit_expr(Dimension::dimensionless()).is_none());
    }

    #[test]
    fn constant_is_substituted_not_refused() {
        // `π*2 ->` (the constant's key in eval/constants.rs) simplifies with its value, as plain
        // evaluation does.
        let e = Expr::binary(BinaryOp::Mul, Expr::var("π"), Expr::num(2.0));
        let out = Session::new().run(&sym(e, None));
        assert!(
            (real_of(&out) - 2.0 * std::f64::consts::PI).abs() < 1e-12,
            "{out:?}"
        );
    }

    #[test]
    fn nested_calls_are_charged_for_copies() {
        // p(x) := sin(x)*x uses x twice, so each call doubles its argument. Depth 18 would copy
        // about 2^18 nodes per use: the work budget refuses it, quickly.
        let mut s = Session::new();
        s.run(&Expr::Define {
            target: Box::new(Expr::call("p", vec![Expr::var("x")])),
            expr: Box::new(Expr::binary(
                BinaryOp::Mul,
                Expr::call("sin", vec![Expr::var("x")]),
                Expr::var("x"),
            )),
        });
        let mut arg = Expr::num(0.5);
        for _ in 0..18 {
            arg = Expr::call("p", vec![arg]);
        }
        let start = std::time::Instant::now();
        let out = s.run(&sym(arg, None));
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
        assert!(matches!(out, Outcome::Error(_)), "{out:?}");
    }

    /// `src -> float, digits` as an expression (the command words built directly).
    fn float_expr(src: &str, digits: f64) -> Expr {
        Expr::SymEval {
            expr: Box::new(text(src)),
            command: Some(Command(vec![Expr::var("float"), Expr::num(digits)])),
            result: None,
        }
    }

    fn define_call(name: &str, params: &[&str], body: Expr) -> Expr {
        Expr::Define {
            target: Box::new(Expr::call(
                name,
                params.iter().map(|p| Expr::var(p)).collect(),
            )),
            expr: Box::new(body),
        }
    }

    #[test]
    fn float_in_a_body_rounds_the_coefficients_of_its_parameter() {
        // h(x) := (1.23456 x + 2.34567) -> float, 3 is the line 1.23 x + 2.35 (sample 15, wsa:
        // the stored values are the coefficients rounded to 3 figures). At x = 10 that is 14.65;
        // rounding the value instead would give 14.7.
        let mut s = Session::new();
        s.run(&define_call(
            "h",
            &["x"],
            float_expr("1.23456*x+2.34567", 3.0),
        ));
        let out = s.run(&Expr::call("h", vec![Expr::num(10.0)]));
        assert!((real_of(&out) - 14.65).abs() < 1e-12, "{out:?}");
    }

    #[test]
    fn a_plain_arrow_body_with_no_names_left_is_its_number() {
        // k(x) := (x^2 + 1) -> is numeric once x is a value, so it is evaluated as a number.
        let mut s = Session::new();
        s.run(&define_call("k", &["x"], sym(text("x^2+1"), None)));
        let out = s.run(&Expr::call("k", vec![Expr::num(3.0)]));
        assert!((real_of(&out) - 10.0).abs() < 1e-12, "{out:?}");
    }

    #[test]
    fn a_body_whose_answer_keeps_a_free_name_is_refused() {
        // m(x) := (x + y) -> with y undefined is a symbolic answer, not a number: refused.
        let mut s = Session::new();
        s.run(&define_call("m", &["x"], sym(text("x+y"), None)));
        let out = s.run(&Expr::call("m", vec![Expr::num(1.0)]));
        assert!(matches!(out, Outcome::Error(_)), "{out:?}");
    }

    #[test]
    fn a_recursive_body_in_a_symbolic_evaluation_is_refused() {
        // r(x) := (r(x-1) + 1) -> refers to itself: refused, not expanded forever.
        let mut s = Session::new();
        let body = sym(
            Expr::binary(
                BinaryOp::Add,
                Expr::call(
                    "r",
                    vec![Expr::binary(BinaryOp::Sub, Expr::var("x"), Expr::num(1.0))],
                ),
                Expr::num(1.0),
            ),
            None,
        );
        s.run(&define_call("r", &["x"], body));
        let out = s.run(&Expr::call("r", vec![Expr::num(3.0)]));
        assert!(matches!(out, Outcome::Error(_)), "{out:?}");
    }

    #[test]
    fn solve_that_assumes_a_nonzero_pivot_is_refused() {
        // a*x + b = 0 for x is -b/a only if a is not 0: the condition is not dropped.
        let lhs = Expr::binary(
            BinaryOp::Add,
            Expr::binary(BinaryOp::Mul, Expr::var("a"), Expr::var("x")),
            Expr::var("b"),
        );
        let out = Session::new().run(&sym(eq(lhs, Expr::num(0.0)), Some(&["solve", "x"])));
        assert!(
            matches!(&out, Outcome::Error(EvalError::Unsupported(t)) if t.contains("nonzero")),
            "{out:?}"
        );
    }

    /// A parsed expression (the test helper `ex` of the core is not visible from here).
    fn text(src: &str) -> Expr {
        crate::parser::parse(src, &crate::parser::DefaultIdContext).expect("parses")
    }

    fn x_id() -> Id {
        Id::new("x", IdKind::Variable)
    }

    #[test]
    fn derivative_operator_is_differentiated_symbolically() {
        // d/dx (x^3 + sin x) = 3 x^2 + cos x, and the second derivative of x^3 is 6 x.
        let d = Expr::Derivative {
            var: Box::new(x_id()),
            order: None,
            partial: false,
            body: Box::new(text("x^3+sin(x)")),
        };
        match Session::new().run(&Expr::SymEval {
            expr: Box::new(d),
            command: None,
            result: None,
        }) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("3*x^2+cos(x)"));
            }
            other => panic!("{other:?}"),
        }
        let d2 = Expr::Derivative {
            var: Box::new(x_id()),
            order: Some(Box::new(Expr::num(2.0))),
            partial: false,
            body: Box::new(text("x^3")),
        };
        match Session::new().run(&Expr::SymEval {
            expr: Box::new(d2),
            command: None,
            result: None,
        }) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("6*x")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn integral_operator_gives_an_exact_definite_value() {
        // int_0^1 exp(x) dx = e - 1
        let i = Expr::Integral {
            var: Box::new(x_id()),
            lower: Box::new(Expr::num(0.0)),
            upper: Box::new(Expr::num(1.0)),
            body: Box::new(text("exp(x)")),
        };
        let out = Session::new().run(&Expr::SymEval {
            expr: Box::new(i),
            command: None,
            result: None,
        });
        assert!(
            (real_of(&out) - (std::f64::consts::E - 1.0)).abs() < 1e-12,
            "{out:?}"
        );
        // An integral the core cannot verify is refused, not evaluated.
        let refused = Expr::Integral {
            var: Box::new(x_id()),
            lower: Box::new(Expr::num(0.0)),
            upper: Box::new(Expr::num(1.0)),
            body: Box::new(text("x*sin(x)")),
        };
        let out = Session::new().run(&Expr::SymEval {
            expr: Box::new(refused),
            command: None,
            result: None,
        });
        assert!(
            matches!(out, Outcome::Error(EvalError::Unsupported(_))),
            "{out:?}"
        );
    }

    #[test]
    fn a_command_on_a_command_answer_is_chained() {
        // ((x+1)^2 -> expand) -> expand gives x^2 + 2 x + 1.
        let inner = Expr::SymEval {
            expr: Box::new(text("(x+1)^2")),
            command: Some(Command(vec![Expr::var("expand")])),
            result: None,
        };
        let chained = Expr::SymEval {
            expr: Box::new(inner),
            command: Some(Command(vec![Expr::var("expand")])),
            result: None,
        };
        match Session::new().run(&chained) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("x^2+2*x+1")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn request_words_are_checked() {
        assert!(matches!(request_of(None), Ok(Request::Simplify)));
        let solve = Command(vec![Expr::var("solve"), Expr::var("x"), Expr::var("y")]);
        assert!(matches!(request_of(Some(&solve)), Ok(Request::Solve(n)) if n == ["x", "y"]));
        let float = Command(vec![Expr::var("float"), Expr::num(3.0)]);
        assert!(matches!(request_of(Some(&float)), Ok(Request::Float(3))));
        let no_digits = Command(vec![Expr::var("float"), Expr::num(0.0)]);
        assert!(matches!(
            request_of(Some(&no_digits)),
            Err(EvalError::Unsupported(_))
        ));
        let factor = Command(vec![Expr::var("factor")]);
        assert!(matches!(request_of(Some(&factor)), Ok(Request::Factor)));
        let series = Command(vec![Expr::var("series"), Expr::var("x"), Expr::num(3.0)]);
        assert!(matches!(
            request_of(Some(&series)),
            Ok(Request::Series { terms: 3, .. })
        ));
        let two_names = Command(vec![Expr::var("series"), Expr::var("x"), Expr::var("y")]);
        assert!(matches!(
            request_of(Some(&two_names)),
            Err(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn limit_operator_is_answered_exactly_or_refused() {
        let limit = |target: Expr, body: &str| Expr::Limit {
            var: Box::new(x_id()),
            target: Box::new(target),
            body: Box::new(text(body)),
        };
        // limit(x, 0, sin(x)/x) = 1
        let out = Session::new().run(&limit(Expr::num(0.0), "sin(x)/x"));
        assert!((real_of(&out) - 1.0).abs() < 1e-15, "{out:?}");
        // The sample region limit(x, 9, o): no x in the body, so the limit is o (a free name).
        match Session::new().run(&limit(Expr::num(9.0), "o")) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("o")),
            other => panic!("{other:?}"),
        }
        // At infinity: (3x^2+1)/(2x^2-5) -> 3/2.
        let out = Session::new().run(&limit(text("inf"), "(3*x^2+1)/(2*x^2-5)"));
        assert!((real_of(&out) - 1.5).abs() < 1e-15, "{out:?}");
        // A pole is refused, never a value.
        assert!(matches!(
            Session::new().run(&limit(Expr::num(0.0), "1/x")),
            Outcome::Error(EvalError::Unsupported(_))
        ));
    }

    #[test]
    fn series_command_follows_the_help() {
        // e^x series -> six terms; sin(x) series,8 -> four nonzero terms.
        match with_words("exp(x)", vec![Expr::var("series")]) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("1+x+x^2/2+x^3/6+x^4/24+x^5/120"));
            }
            other => panic!("{other:?}"),
        }
        match with_words("sin(x)", vec![Expr::var("series"), Expr::num(8.0)]) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("x-x^3/6+x^5/120-x^7/5040"));
            }
            other => panic!("{other:?}"),
        }
        // e^x series,x=4 is the expansion about 4.
        match with_words("exp(x)", vec![Expr::var("series"), assign("x", "4")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(
                s.entries[0],
                s_of("exp(4)+exp(4)*(x-4)+exp(4)/2*(x-4)^2+exp(4)/6*(x-4)^3+exp(4)/24*(x-4)^4+exp(4)/120*(x-4)^5")
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parfrac_command_splits_over_the_rationals() {
        match with_words("1/(x^2-3*x+2)", vec![Expr::var("parfrac")]) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("1/(x-2) - 1/(x-1)"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn laplace_and_invlaplace_follow_the_help_words() {
        // exp(t) laplace -> 1/(s-1); exp(x) laplace, x, y -> 1/(y-1) (help: transforms).
        match with_words("exp(t)", vec![Expr::var("laplace")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("1/(s-1)")),
            other => panic!("{other:?}"),
        }
        match with_words(
            "exp(x)",
            vec![Expr::var("laplace"), Expr::var("x"), Expr::var("y")],
        ) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("1/(y-1)")),
            other => panic!("{other:?}"),
        }
        // 1/(s+2) invlaplace -> exp(-2 t).
        match with_words("1/(s+2)", vec![Expr::var("invlaplace")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("exp(-2*t)")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ztrans_and_invztrans_follow_the_help_words() {
        // help: example_symbolic_transforms. n ztrans -> z/(z-1)^2; z/(z-1)^2 invztrans -> n.
        match with_words("n", vec![Expr::var("ztrans")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("z/(z^2-2*z+1)")),
            other => panic!("{other:?}"),
        }
        match with_words("z/(z-1)^2", vec![Expr::var("invztrans")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("n")),
            other => panic!("{other:?}"),
        }
        // A pole elsewhere would need r^n: refused, not guessed.
        assert!(!matches!(
            with_words("z/(z-2)", vec![Expr::var("invztrans")]),
            Outcome::Value(_)
        ));
    }

    #[test]
    fn derivative_with_respect_to_e_keeps_the_base_varying() {
        // help-style: d/de e^x = x e^(x-1); d/de (e^x x) = x^2 e^(x-1); d/de e^2 = 2 e (symbolic,
        // not the number 2 e evaluated).
        // The canonical order prints `e^(x-1)*x` (the help writes `x*e^(x-1)`): same value, same
        // canonical tree.
        let of = |src: &str| match run_text(src) {
            Outcome::Value(Value::Symbolic(s)) => s.entries[0].clone(),
            other => panic!("{src}: {other:?}"),
        };
        assert_eq!(of("derivative(e, e^x) ->"), s_of("x*exp(x-1)"));
        assert_eq!(of("derivative(e, e^x*x) ->"), s_of("x^2*exp(x-1)"));
        assert_eq!(shown("derivative(e, e^2) ->"), "2*e");
    }

    #[test]
    fn assume_contradiction_is_refused_when_it_is_added() {
        // ALL=complex with x>0 contradicts itself for x at once, whether or not a rewrite reads x.
        let out = with_words(
            "x",
            vec![
                Expr::var("assume"),
                assign("ALL", "complex"),
                Expr::binary(BinaryOp::Gt, Expr::var("x"), Expr::num(0.0)),
            ],
        );
        assert!(
            matches!(&out, Outcome::Error(EvalError::Unsupported(_))),
            "{out:?}"
        );
    }

    #[test]
    fn substitute_takes_an_equation_in_the_command_words() {
        // (x + y) -> substitute, x = 13 is y + 13 (the `=` of a command word is an equation).
        match run_text("(x+y) -> substitute, x = 13") {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("y+13")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn expand_of_trigonometric_sums_and_multiples() {
        // help (to_expand_expressions): sin(2 x) expand -> 2 cos(x) sin(x).
        match with_words("sin(2*x)", vec![Expr::var("expand")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("2*cos(x)*sin(x)")),
            other => panic!("{other:?}"),
        }
        match with_words("cos(a+b)", vec![Expr::var("expand")]) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("cos(a)*cos(b)-sin(a)*sin(b)"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rewrite_follows_the_help_examples() {
        // help (example_simplify_rewrite_expressions): sin(x)^2 rewrite, cos -> -cos(x)^2 + 1;
        // asinh(x) rewrite, ln -> ln(sqrt(x^2 + 1) + x).
        match with_words("sin(x)^2", vec![Expr::var("rewrite"), Expr::var("cos")]) {
            Outcome::Value(Value::Symbolic(s)) => assert_eq!(s.entries[0], s_of("1-cos(x)^2")),
            other => panic!("{other:?}"),
        }
        match with_words("asinh(x)", vec![Expr::var("rewrite"), Expr::var("ln")]) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert_eq!(s.entries[0], s_of("ln(sqrt(x^2+1)+x)"))
            }
            other => panic!("{other:?}"),
        }
        // sin(x) rewrite, exp has no sin left.
        match with_words("sin(x)", vec![Expr::var("rewrite"), Expr::var("exp")]) {
            Outcome::Value(Value::Symbolic(s)) => {
                assert!(!format!("{:?}", s.entries[0]).contains("sin"))
            }
            other => panic!("{other:?}"),
        }
        // A missing or unknown modifier is an error (help: "rewrite requires a modifier").
        for words in [
            vec![Expr::var("rewrite")],
            vec![Expr::var("rewrite"), Expr::var("zzz")],
        ] {
            let out = with_words("sin(x)", words);
            assert!(
                matches!(out, Outcome::Error(EvalError::Unsupported(_))),
                "{out:?}"
            );
        }
    }

    #[test]
    fn solve_of_an_exponential_equation_is_a_logarithm() {
        let parse =
            |t: &str| crate::parser::parse(t, &crate::parser::DefaultIdContext).expect("parses");
        // e^x = 5: x = ln(5) (about 1.6094).
        let out = Session::new().run(&sym(eq(parse("exp(x)"), parse("5")), Some(&["solve", "x"])));
        assert!((real_of(&out) - 5f64.ln()).abs() < 1e-12, "{out:?}");
        // A negative right side has only complex solutions: refused, not a wrong real value.
        let out = Session::new().run(&sym(
            eq(parse("exp(x)"), parse("-5")),
            Some(&["solve", "x"]),
        ));
        assert!(
            matches!(out, Outcome::Error(EvalError::Unsupported(_))),
            "{out:?}"
        );
    }
}

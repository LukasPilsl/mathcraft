//! Exact symbolic matrices in a symbolic command (`expr -> command`): the determinant, the
//! inverse, transpose, products, integer powers, `|M|` (the norm of a column, the determinant of a square matrix), `tr`, `rank`, and
//! `eigenvals` of 2x2 and 3x3 matrices. Entries are canonical [`Sym`]s (see [`SymMatrix`]).
//!
//! Only a matrix with a name in it is symbolic: a numeric matrix returns `None` from [`answer`] and
//! takes the ordinary path. An inverse whose determinant is not a nonzero constant is refused (its
//! condition is not shown), and so is a rank that assumes a symbolic pivot is nonzero. Anything
//! else the walker cannot read is refused, and the caller falls back to the ordinary path, which
//! refuses it with its own message.

use super::*;
use crate::symbolic::{Assumptions, SymMatrix};

/// The variable of the characteristic polynomial behind `eigenvals`. A matrix that uses this name
/// is refused rather than mixed with it.
const EIGEN_VAR: &str = "lambda";

/// The functions that take a matrix (`det(M)`, `tr(M)`, `rank(M)`, `eigenvals(M)`).
const MATRIX_FUNCTIONS: [&str; 4] = ["det", "tr", "rank", "eigenvals"];

/// A value of the walk: a scalar, a column of values (`eigenvals`), or a matrix.
enum Operand {
    Scalar(Sym),
    Vector(Vec<Sym>),
    Grid(SymMatrix),
}

/// Whether `e` has a matrix operand: a matrix literal, a worksheet matrix, or a matrix function.
pub(super) fn contains_matrix(e: &Expr, scope: &Scope<'_>) -> bool {
    match e {
        Expr::Matrix { .. } => true,
        Expr::Id(id) => {
            scope.body.is_none()
                && matches!(var_key(id), Ok(key)
                    if matches!(scope.env.variable(&key), Some(Value::Matrix(_))))
        }
        Expr::Parens(inner) | Expr::Unary { operand: inner, .. } => contains_matrix(inner, scope),
        Expr::Binary { lhs, rhs, .. } => contains_matrix(lhs, scope) || contains_matrix(rhs, scope),
        Expr::Call { func, args } => {
            let matrix_function = matches!(var_key(func), Ok(key)
                if MATRIX_FUNCTIONS.contains(&key.as_str()) && scope.env.function(&key).is_none());
            matrix_function || args.iter().any(|a| contains_matrix(a, scope))
        }
        _ => false,
    }
}

/// The answer of `expr` (which has a matrix operand, see [`contains_matrix`]) for a `->`, `float`
/// or `expand` request. `None` means the expression has no names, or is not read here: the caller
/// then takes the ordinary path. Budget and depth errors are returned as they are.
pub(super) fn answer(
    expr: &Expr,
    request: &Request,
    scope: &Scope<'_>,
    budget: &Budget,
) -> Result<Option<Answer>, EvalError> {
    let mut walk = Walk {
        scope,
        budget,
        symbolic: false,
    };
    let operand = match walk.value(expr, scope.nodes) {
        Ok(operand) => operand,
        Err(e @ (EvalError::WorkLimit(_) | EvalError::CallLimit(_) | EvalError::DepthLimit(_))) => {
            return Err(e)
        }
        Err(_) => return Ok(None),
    };
    if !walk.symbolic {
        return Ok(None);
    }
    let answer = match operand {
        Operand::Scalar(s) => Answer::One(entry(s, request)?),
        Operand::Vector(values) => Answer::Column(
            values
                .into_iter()
                .map(|s| entry(s, request))
                .collect::<Result<_, _>>()?,
        ),
        Operand::Grid(m) => {
            let (rows, cols) = (m.rows(), m.cols());
            let values = m
                .to_rows()
                .into_iter()
                .flatten()
                .map(|s| entry(s, request))
                .collect::<Result<Vec<_>, _>>()?;
            Answer::Grid(rows, cols, values)
        }
    };
    Ok(Some(answer))
}

/// One entry of an answer, in the requested form.
fn entry(s: Sym, request: &Request) -> Result<Sym, EvalError> {
    match request {
        Request::Simplify | Request::Float(_) => {
            let simple = core(simplify(&s))?;
            core(simplify_identities(&simple, &Assumptions::default()))
        }
        Request::Expand => core(expand(&core(simplify(&s))?)),
        _ => Err(unsupported("symbolic matrix with this command")),
    }
}

/// The walk over an expression with matrices: `symbolic` records whether a name was read.
struct Walk<'a, 'b> {
    scope: &'a Scope<'b>,
    budget: &'a Budget,
    symbolic: bool,
}

impl Walk<'_, '_> {
    /// A scalar subexpression, prepared and converted by the ordinary rules.
    fn scalar(&mut self, e: &Expr, nodes: usize) -> Result<Sym, EvalError> {
        let prepared = prepare(e, self.scope, &[], self.budget, nodes)?;
        let s = core(from_expr(&prepared))?;
        self.symbolic |= !symbols(&s).is_empty();
        Ok(s)
    }

    fn value(&mut self, e: &Expr, nodes: usize) -> Result<Operand, EvalError> {
        if nodes > MAX_NODE_DEPTH {
            return Err(EvalError::DepthLimit(MAX_NODE_DEPTH));
        }
        self.budget.charge(NODE_UNITS)?;
        if !contains_matrix(e, self.scope) {
            return Ok(Operand::Scalar(self.scalar(e, nodes)?));
        }
        let next = nodes + 1;
        match e {
            Expr::Parens(inner) => self.value(inner, next),
            Expr::Matrix {
                rows,
                cols,
                elements,
            } => {
                if *rows == 0 || *cols == 0 || elements.len() != rows * cols {
                    return Err(unsupported("matrix with this shape"));
                }
                let mut data = Vec::with_capacity(elements.len());
                for element in elements {
                    data.push(self.scalar(element, next)?);
                }
                let grid: Vec<Vec<Sym>> = data.chunks(*cols).map(<[Sym]>::to_vec).collect();
                Ok(Operand::Grid(core(SymMatrix::new(grid))?))
            }
            // A worksheet matrix is read as its literal of numbers.
            Expr::Id(id) => {
                let key = var_key(id)?;
                match self.scope.env.variable(&key) {
                    Some(value) => {
                        let literal = value_expr(value)?;
                        self.value(&literal, next)
                    }
                    None => Err(unsupported("matrix name")),
                }
            }
            Expr::Unary { op, operand } => {
                let v = self.value(operand, next)?;
                match (*op, v) {
                    (UnaryOp::Neg, Operand::Scalar(s)) => Ok(Operand::Scalar(negate(s)?)),
                    (UnaryOp::Neg, Operand::Grid(m)) => {
                        Ok(Operand::Grid(map_grid(&m, |s| negate(s.clone()))?))
                    }
                    (UnaryOp::Transpose, Operand::Grid(m)) => Ok(Operand::Grid(m.transpose())),
                    // `|M|` follows the numeric rule (`ops::abs`): a column (a 1x1 included) has
                    // its norm, any other square matrix its determinant, a row or a non-square
                    // matrix is refused.
                    (UnaryOp::Abs, Operand::Grid(m)) => Ok(Operand::Scalar(bars(&m)?)),
                    _ => Err(unsupported("symbolic matrix operator")),
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                let l = self.value(lhs, next)?;
                let r = self.value(rhs, next)?;
                binary(*op, l, r)
            }
            Expr::Call { func, args } => {
                let name = var_key(func)?;
                let values = args
                    .iter()
                    .map(|a| self.value(a, next))
                    .collect::<Result<Vec<_>, _>>()?;
                match (name.as_str(), values.as_slice()) {
                    ("det", [Operand::Grid(m)]) => Ok(Operand::Scalar(core(m.determinant())?)),
                    ("tr", [Operand::Grid(m)]) => Ok(Operand::Grid(m.transpose())),
                    ("rank", [Operand::Grid(m)]) => {
                        let rank = core(m.rank())?;
                        if !rank.conditions.is_empty() {
                            return Err(unsupported("symbolic rank assumes a nonzero pivot"));
                        }
                        Ok(Operand::Scalar(Sym::int(rank.rank as i64)))
                    }
                    ("eigenvals", [Operand::Grid(m)]) => {
                        let uses_name = m
                            .to_rows()
                            .iter()
                            .flatten()
                            .any(|s| symbols(s).iter().any(|n| n == EIGEN_VAR));
                        if uses_name {
                            return Err(unsupported("eigenvals of a matrix that uses lambda"));
                        }
                        Ok(Operand::Vector(core(m.eigenvalues(EIGEN_VAR))?))
                    }
                    _ => Err(unsupported("symbolic matrix function")),
                }
            }
            _ => Err(unsupported("symbolic matrix expression")),
        }
    }
}

/// `|M|` as the numeric evaluator reads it (`ops::abs`): the norm of a column (a 1x1 matrix is a
/// column, so `|[[x]]|` is `|x|`), the determinant of any other square matrix, and a refusal for
/// a row or a non-square matrix.
fn bars(m: &SymMatrix) -> Result<Sym, EvalError> {
    if m.cols() != 1 {
        if m.rows() == m.cols() {
            return core(m.determinant());
        }
        return Err(unsupported("|M| needs a column vector or a square matrix"));
    }
    let entries: Vec<Sym> = m.to_rows().into_iter().flatten().collect();
    if let [only] = entries.as_slice() {
        if !matches!(only, Sym::Num(_)) {
            return core(Sym::func("abs", vec![only.clone()]));
        }
    }
    // sqrt(sum |e|^2), with |e|^2 = e*e for an exact rational and abs(e)*abs(e) otherwise.
    let mut squares = Vec::with_capacity(entries.len());
    for e in &entries {
        let a = match e {
            Sym::Num(_) => e.clone(),
            other => core(Sym::func("abs", vec![other.clone()]))?,
        };
        squares.push(core(Sym::product(vec![a.clone(), a]))?);
    }
    if let [Sym::Num(_)] = entries.as_slice() {
        // A single rational: its magnitude.
        let half = Ratio::new(1.into(), 2.into());
        return core(Sym::pow(squares.remove(0), half));
    }
    let total = core(Sym::sum(squares))?;
    core(Sym::pow(total, Ratio::new(1.into(), 2.into())))
}

/// `-s`.
fn negate(s: Sym) -> Result<Sym, EvalError> {
    core(Sym::product(vec![Sym::int(-1), s]))
}

/// Applies `f` to every entry of `m`.
fn map_grid(
    m: &SymMatrix,
    f: impl Fn(&Sym) -> Result<Sym, EvalError>,
) -> Result<SymMatrix, EvalError> {
    let rows = m
        .to_rows()
        .iter()
        .map(|row| row.iter().map(&f).collect::<Result<Vec<_>, _>>())
        .collect::<Result<Vec<_>, _>>()?;
    core(SymMatrix::new(rows))
}

/// A binary operator on scalars and matrices: sums of the same shape, products (a scalar times a
/// matrix, or a matrix times a matrix of fitting shape), division by a scalar, and `M^k` for an
/// integer `k` (a negative `k` needs an inverse with no condition).
fn binary(op: BinaryOp, l: Operand, r: Operand) -> Result<Operand, EvalError> {
    let subtract = matches!(op, BinaryOp::Sub);
    match (op, l, r) {
        (BinaryOp::Add | BinaryOp::Sub, Operand::Scalar(a), Operand::Scalar(b)) => {
            let b = if subtract { negate(b)? } else { b };
            Ok(Operand::Scalar(core(Sym::sum(vec![a, b]))?))
        }
        (BinaryOp::Add | BinaryOp::Sub, Operand::Grid(a), Operand::Grid(b)) => {
            if a.rows() != b.rows() || a.cols() != b.cols() {
                return Err(unsupported("matrix sum of different shapes"));
            }
            let (ra, rb) = (a.to_rows(), b.to_rows());
            let mut rows = Vec::with_capacity(ra.len());
            for (x_row, y_row) in ra.iter().zip(&rb) {
                let mut row = Vec::with_capacity(x_row.len());
                for (x, y) in x_row.iter().zip(y_row) {
                    let y = if subtract {
                        negate(y.clone())?
                    } else {
                        y.clone()
                    };
                    row.push(core(Sym::sum(vec![x.clone(), y]))?);
                }
                rows.push(row);
            }
            Ok(Operand::Grid(core(SymMatrix::new(rows))?))
        }
        (BinaryOp::Mul, Operand::Scalar(a), Operand::Scalar(b)) => {
            Ok(Operand::Scalar(core(Sym::product(vec![a, b]))?))
        }
        (BinaryOp::Mul, Operand::Grid(a), Operand::Grid(b)) => {
            Ok(Operand::Grid(core(a.multiply(&b))?))
        }
        (BinaryOp::Mul, Operand::Scalar(s), Operand::Grid(m))
        | (BinaryOp::Mul, Operand::Grid(m), Operand::Scalar(s)) => {
            Ok(Operand::Grid(map_grid(&m, |x| {
                core(Sym::product(vec![s.clone(), x.clone()]))
            })?))
        }
        (BinaryOp::Div, Operand::Scalar(a), Operand::Scalar(b)) => {
            Ok(Operand::Scalar(core(a.over(b))?))
        }
        (BinaryOp::Div, Operand::Grid(m), Operand::Scalar(s)) => {
            Ok(Operand::Grid(map_grid(&m, |x| {
                core(x.clone().over(s.clone()))
            })?))
        }
        (BinaryOp::Pow, Operand::Scalar(base), Operand::Scalar(Sym::Num(q))) => {
            Ok(Operand::Scalar(core(Sym::pow(base, q))?))
        }
        (BinaryOp::Pow, Operand::Grid(m), Operand::Scalar(Sym::Num(q))) if q.is_integer() => {
            let k = i64::try_from(q.to_integer())
                .map_err(|_| unsupported("matrix power out of range"))?;
            if k < 0 {
                let inverse = core(m.inverse())?;
                if inverse.condition.is_some() {
                    return Err(unsupported(
                        "symbolic inverse assumes a nonzero determinant",
                    ));
                }
                Ok(Operand::Grid(core(inverse.matrix.power(-k))?))
            } else {
                Ok(Operand::Grid(core(m.power(k))?))
            }
        }
        _ => Err(unsupported("symbolic matrix operator with these operands")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{Outcome, Session};
    use crate::expr::UnaryOp as U;
    use crate::symbolic::{from_expr, substitute};

    fn text(src: &str) -> Expr {
        crate::parser::parse(src, &crate::parser::DefaultIdContext).expect("parses")
    }

    fn s_of(src: &str) -> Sym {
        from_expr(&text(src)).expect("converts")
    }

    /// `expr -> command` (a plain `->` when `command` is empty), in a new worksheet.
    fn run_expr(expr: Expr, command: Option<&str>) -> Outcome {
        Session::new().run(&Expr::SymEval {
            expr: Box::new(expr),
            command: command.map(|c| crate::expr::Command(vec![Expr::var(c)])),
            result: None,
        })
    }

    fn run(src: &str, command: Option<&str>) -> Outcome {
        run_expr(text(src), command)
    }

    fn symbolic_of(out: Outcome) -> (usize, usize, Vec<Sym>) {
        match out {
            Outcome::Value(Value::Symbolic(s)) => (s.rows, s.cols, s.entries),
            other => panic!("{other:?}"),
        }
    }

    /// The entry at `x = at` of a symbolic entry, simplified.
    fn at(entry: &Sym, x: i64) -> Sym {
        let v = substitute(entry, &[("x", Sym::int(x))]).expect("substitute");
        simplify(&v).expect("simplify")
    }

    #[test]
    fn determinant_of_a_symbolic_matrix() {
        // det [[a, b], [c, d]] = a d - b c.
        let out = run_expr(Expr::call("det", vec![text("[a, b; p, d]")]), None);
        let (r, c, v) = symbolic_of(out);
        assert_eq!((r, c), (1, 1));
        assert_eq!(v[0], s_of("a*d-b*p"));
    }

    #[test]
    fn bars_follow_the_numeric_rule() {
        // Square: the determinant. Column (1x1 included): the norm. Row or non-square: refused.
        let (_, _, v) = symbolic_of(run("|[a, b; p, d]| ", None));
        assert_eq!(v[0], s_of("a*d-b*p"));
        let (_, _, v) = symbolic_of(run("|[3; 4; a]|", None));
        assert_eq!(v[0], s_of("(25+abs(a)^2)^(1/2)"));
        let (_, _, v) = symbolic_of(run("|[a]|", None));
        assert_eq!(v[0], s_of("abs(a)"));
        for src in ["|[a, b]|", "|[a, b, c; p, d, e]|"] {
            assert!(
                matches!(run(src, None), Outcome::Error(EvalError::Unsupported(_))),
                "{src}"
            );
        }
        // The same numeric inputs agree with the numeric evaluator.
        for src in ["|[3; -4]|", "|[-2]|", "|[1, 2; 3, 4]|"] {
            let exact = match run(src, None) {
                Outcome::Value(Value::Scalar(s)) => s.value.re,
                other => panic!("{src}: {other:?}"),
            };
            let numeric = match Session::new().run(&text(src)) {
                Outcome::Value(Value::Scalar(s)) => s.value.re,
                other => panic!("{src}: {other:?}"),
            };
            assert!((exact - numeric).abs() < 1e-12, "{src}");
        }
    }

    #[test]
    fn a_symbolic_matrix_is_an_answer_matrix() {
        let (r, c, v) = symbolic_of(run("[a, b; p, d]", None));
        assert_eq!((r, c), (2, 2));
        assert_eq!(v[0], s_of("a"));
        assert_eq!(v[3], s_of("d"));
    }

    #[test]
    fn transpose_power_and_product() {
        // Transpose of a diagonal matrix, then its square.
        let diag = text("[a, 0; 0, b]");
        let (_, _, v) = symbolic_of(run_expr(Expr::unary(U::Transpose, diag.clone()), None));
        assert_eq!(v[0], s_of("a"));
        assert_eq!(v[3], s_of("b"));
        let (_, _, v) = symbolic_of(run_expr(
            Expr::binary(crate::expr::BinaryOp::Pow, diag, Expr::num(2.0)),
            None,
        ));
        assert_eq!(v[0], s_of("a^2"));
        assert_eq!(v[3], s_of("b^2"));
        // [[a,1],[0,b]] * [[1,0],[2,1]] = [[a+2, 1], [2b, b]].
        let (_, _, v) = symbolic_of(run("[a, 1; 0, b]*[1, 0; 2, 1]", None));
        assert_eq!(v[0], s_of("a+2"));
        assert_eq!(v[1], s_of("1"));
        assert_eq!(v[2], s_of("2*b"));
        assert_eq!(v[3], s_of("b"));
    }

    #[test]
    fn inverse_with_a_condition_is_refused() {
        // The inverse of [[a,b],[c,d]] needs a d - b c != 0: refused, not printed bare.
        let out = run_expr(
            Expr::binary(
                crate::expr::BinaryOp::Pow,
                text("[a, b; p, d]"),
                Expr::num(-1.0),
            ),
            None,
        );
        assert!(
            matches!(out, Outcome::Error(EvalError::Unsupported(_))),
            "{out:?}"
        );
    }

    #[test]
    fn eigenvalues_of_a_triangular_symbolic_matrix() {
        // [x, 1; 0, 2] has eigenvalues x and 2.
        let out = run_expr(Expr::call("eigenvals", vec![text("[x, 1; 0, 2]")]), None);
        let (r, c, v) = symbolic_of(out);
        assert_eq!((r, c), (2, 1));
        // The roots are printed by the quadratic formula (not simplified to x and 2), so each is
        // checked at x = 5 and x = -3, where it must be 5 or 2 (and -3 or 2).
        for x in [5_i64, -3] {
            let vals: Vec<Sym> = v.iter().map(|e| at(e, x)).collect();
            assert!(
                vals.contains(&Sym::int(x)) && vals.contains(&Sym::int(2)),
                "{vals:?}"
            );
        }
    }

    #[test]
    fn numeric_matrix_takes_the_ordinary_path() {
        // No names: the ordinary path gives the exact number (det of the help's example).
        let out = run_expr(
            Expr::call("det", vec![text("[0, 3, 5; 1, 0, 3; 2, 2, 1]")]),
            None,
        );
        assert!(matches!(out, Outcome::Value(Value::Scalar(_))), "{out:?}");
    }

    #[test]
    fn float_rounds_the_numbers_of_a_symbolic_matrix() {
        // [1.23456 a, 2; 0, 1/3] -> float, 3 is [1.23 a, 2; 0, 0.333]: the names stay.
        let out = Session::new().run(&Expr::SymEval {
            expr: Box::new(text("[1.23456*a, 2; 0, 1/3]")),
            command: Some(crate::expr::Command(vec![
                Expr::var("float"),
                Expr::num(3.0),
            ])),
            result: None,
        });
        let (r, c, v) = symbolic_of(out);
        assert_eq!((r, c), (2, 2));
        assert_eq!(v[0], s_of("1.23*a"));
        assert_eq!(v[1], s_of("2"));
        assert_eq!(v[3], s_of("0.333"));
    }

    /// The determinant of a symbolic matrix at integer points equals the value of the expansion
    /// computed by hand: det [[x,1,2],[0,x^2,1],[x+1,3,x]].
    #[test]
    fn determinant_agrees_with_the_numeric_value_at_points() {
        let m = SymMatrix::new(vec![
            vec![s_of("x"), s_of("1"), s_of("2")],
            vec![s_of("0"), s_of("x^2"), s_of("1")],
            vec![s_of("x+1"), s_of("3"), s_of("x")],
        ])
        .expect("matrix");
        let det = m.determinant().expect("det");
        for x in -500_i64..=500 {
            if x % 37 != 0 {
                continue;
            }
            let (x1, x2) = (x as i128, (x * x) as i128);
            // a(ei - fh) - b(di - fg) + c(dh - eg) with a=x, b=1, c=2, d=0, e=x^2, f=1, g=x+1, h=3, i=x.
            let by_hand = x1 * (x2 * x1 - 3) - (0 - (x1 + 1)) + 2 * (0 - x2 * (x1 + 1));
            assert_eq!(at(&det, x), Sym::int(by_hand as i64), "x = {x}");
        }
    }

    /// A times its inverse is the identity wherever the determinant is nonzero.
    #[test]
    fn inverse_times_matrix_is_the_identity() {
        let m = SymMatrix::new(vec![
            vec![s_of("x"), s_of("1")],
            vec![s_of("0"), s_of("x+2")],
        ])
        .expect("matrix");
        let inv = m.inverse().expect("inverse");
        assert!(inv.condition.is_some());
        let product = m.multiply(&inv.matrix).expect("product");
        for x in [3_i64, -5, 7] {
            for (i, row) in product.to_rows().iter().enumerate() {
                for (j, entry) in row.iter().enumerate() {
                    let want = if i == j { Sym::int(1) } else { Sym::int(0) };
                    assert_eq!(at(entry, x), want, "x = {x}, entry ({i},{j})");
                }
            }
        }
    }

    /// Each eigenvalue l of [[a,2],[3,a]] satisfies det(A - l I) = 0.
    #[test]
    fn eigenvalues_are_roots_of_the_characteristic_determinant() {
        let a = SymMatrix::new(vec![vec![s_of("a"), s_of("2")], vec![s_of("3"), s_of("a")]])
            .expect("matrix");
        let roots = a.eigenvalues(EIGEN_VAR).expect("eigenvalues");
        assert_eq!(roots.len(), 2);
        for root in roots {
            let diagonal =
                Sym::sum(vec![s_of("a"), negate(root.clone()).expect("negate")]).expect("sum");
            let shifted = SymMatrix::new(vec![
                vec![diagonal.clone(), s_of("2")],
                vec![s_of("3"), diagonal],
            ])
            .expect("shifted");
            let det = simplify(&shifted.determinant().expect("det")).expect("simplify");
            assert_eq!(det, Sym::int(0), "root {root:?}");
        }
    }
}

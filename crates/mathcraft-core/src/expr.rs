//! Expression model: AST and Mathcad-style text printer.
//!
//! Precedence follows the Prime help operator pages (`about_operators` and the operator pages
//! under "Operators"). The help gives no full precedence table, so the levels in [`Prec`] are
//! the conventional Mathcad order; see the open questions in the commit report.
//!
//! # Text forms of names, indices and matrices
//!
//! - `v_0`, `v_i`, `v_{i+1}`: a name with one literal subscript. The subscript is part of the
//!   name, so it is an [`Id`] with `subscript` set. Braces are used unless the subscript is a
//!   plain number or name. `M_{i,j}` is not a valid name: a literal subscript has one item.
//! - `v[i]`, `M[i,j]`, `M[i][j]`: array indexing, an [`Expr::Index`]. Brackets always mean an
//!   index, so `v_i` and `v[i]` are different expressions.
//! - `[1, 2; 3, 4]`: a matrix literal, rows separated by `;`.
//! - `g_1(x)`: a call of a subscripted name. The function is an [`Id`] with `subscript` set, so
//!   the converter's `Call` shape prints and reads back unchanged.
//! - `C'`, `B''`: primes are name characters. `F_A` is an [`Id`] named `F_A` only when it is
//!   written in backticks, `` `F_A` ``. A name the parser would not read plainly (one with `_`,
//!   for example) prints in backticks, so it cannot be mistaken for `F` with subscript `A`.
//! - `` `a``b` ``: a name that is not plain is quoted in backticks, with each backtick inside
//!   doubled, so the name ``a`b`` prints as `` `a``b` ``. An empty name is invalid.
//! - `6 m`: a bare unit identifier after a value is a `Scale`. Other units are parenthesized,
//!   as in `42 (m/s)`, and a matrix unit is parenthesized too, as in `text ([1, 2])`. The
//!   parser reads a parenthesized group after a number as a unit only when it is a unit
//!   expression, so the printer's `Scale` forms are the ones that read back.
//!
//! # Lossy text round trip
//!
//! Text is the printer's output and the parser's input. It does not carry every field of an
//! [`Expr`]:
//!
//! - The `result` of `Expr::SymEval` is never printed. Any text serialization of an expression loses the
//!   stored symbolic result of a `->` evaluation, so `y -> solve, x` reads back with no result.
//!   The sample round trip's normalization drops `result` too, so it counts these as harmless.
//! - The worksheet model keeps the full tree, including `result`. Only text export and import
//!   lose it, so a worksheet is not rebuilt from its text.
//! - Several other shapes read back differently but equal in meaning; see the parser's module
//!   docs for the list.
//! - Disambiguation of `[`: directly after an operand (no whitespace between) it is an index,
//!   as in `v[1]`, `(a+b)[1]`, `f(x)[2]`, `[1, 2][1]`. At the start of an expression or after
//!   an operator, `(`, or `,` it is a matrix literal. A space between an operand and `[` is an
//!   error, because neither reading applies.

use std::fmt;

use num_rational::BigRational;
use num_traits::Signed;

/// Binding strength of an operator or form; higher binds tighter.
///
/// `ASSIGN` `:= ≡ =` < `RANGE` `..` < `OR` `∨` < `XOR` `⊕` < `AND` `∧` < `NOT` `¬` (prefix)
/// < `CMP` `== <> < > <= >=` < `ADD` `+ -` < `MUL` `* /` < `NEG` `-x` (prefix)
/// < `POW` `^` (right-assoc) < `POSTFIX` `x! x% x^T x[i]` < `ATOM` (names, numbers, calls,
/// `sqrt`, `|x|`, `(x)`, `[..]`). Binary operators are left-associative except `^`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Prec(u8);

impl Prec {
    pub const ASSIGN: Prec = Prec(0);
    pub const RANGE: Prec = Prec(1);
    pub const OR: Prec = Prec(2);
    pub const XOR: Prec = Prec(3);
    pub const AND: Prec = Prec(4);
    pub const NOT: Prec = Prec(5);
    pub const CMP: Prec = Prec(6);
    pub const ADD: Prec = Prec(7);
    pub const MUL: Prec = Prec(8);
    pub const NEG: Prec = Prec(9);
    pub const POW: Prec = Prec(10);
    pub const POSTFIX: Prec = Prec(11);
    pub const ATOM: Prec = Prec(12);

    fn above(self) -> Prec {
        Prec(self.0 + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKind {
    Variable,
    Function,
    Unit,
    Constant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Number {
    /// A real number; `text` is the literal as written and is printed verbatim.
    Real { value: f64, text: String },
    /// An exact rational, printed `n/d` (or `n` when integral).
    Rational(BigRational),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    Transpose,
    Abs,
    /// `‖x‖`: the norm of a scalar, vector or matrix (the Euclidean norm of all elements).
    Norm,
    Factorial,
    Percent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    /// `u × v`: the cross product of two three-element column vectors.
    Cross,
    Pow,
    /// Prints `==`. The Mathcad `equal` element (boolean / equation equality) maps here.
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Xor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Id {
    pub name: String,
    pub kind: IdKind,
    pub subscript: Option<Box<Expr>>,
}

/// A command keyword sequence such as `solve, x` or `simplify`, printed comma-separated.
#[derive(Debug, Clone, PartialEq)]
pub struct Command(pub Vec<Expr>);

/// Which branch of a conditional group a [`Stmt::Branch`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    /// `if x`: opens a conditional group.
    If,
    /// `else if x`: runs only when no earlier branch of the group ran.
    ElseIf,
    /// `also if x`: runs whatever the earlier branches did; it keeps a later `else` from running.
    AlsoIf,
    /// `else`: runs when no earlier branch of the group ran. It has no condition.
    Else,
}

/// One line of a programming region (Mathcad's program operators, see [`Expr::Program`]).
///
/// A body is a block of statements, one nesting level deeper than the line that owns it.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// A line that is an expression or an evaluation. Its value is the value of the line.
    Line(Expr),
    /// `target ← expr`: a local assignment. Its value is the assigned value.
    Local { target: Expr, expr: Expr },
    /// A conditional branch. `cond` is `None` only for `else`.
    Branch {
        kind: BranchKind,
        cond: Option<Expr>,
        body: Vec<Stmt>,
    },
    /// `for var ∈ range`: the body runs once per value of `range`, the elements of a vector in
    /// order.
    For {
        var: Id,
        range: Expr,
        body: Vec<Stmt>,
    },
    /// `while cond`: the body runs while `cond` is nonzero, tested before each pass.
    While { cond: Expr, body: Vec<Stmt> },
    /// `break`: leaves the innermost loop.
    Break,
    /// `continue`: skips to the next pass of the innermost loop.
    Continue,
    /// `return x`: ends the program with the value `x`.
    Return(Expr),
    /// `try` `body` `on error` `on_error`: when `body` fails, `on_error` runs instead.
    Try {
        body: Vec<Stmt>,
        on_error: Vec<Stmt>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(Number),
    Id(Id),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        func: Id,
        args: Vec<Expr>,
    },
    Range {
        from: Box<Expr>,
        step: Option<Box<Expr>>,
        to: Box<Expr>,
    },
    /// A programming region: the statements run in order. Its value is the value of the last
    /// line that gives one, or the value of the `return` that ends it. It has no text syntax yet:
    /// the printer shows one statement per line, indented by nesting.
    Program(Vec<Stmt>),
    /// Row-major; `elements.len() == rows * cols`.
    Matrix {
        rows: usize,
        cols: usize,
        elements: Vec<Expr>,
    },
    /// Array indexing, printed `base[i]` or `base[i,j]` (nested: `M[i][j]`). Distinct from a
    /// name's literal subscript (`Id` with `subscript`, printed `v_i`).
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
    },
    /// `None` or a placeholder degree prints as `sqrt(x)`.
    NthRoot {
        degree: Option<Box<Expr>>,
        radicand: Box<Expr>,
    },
    Parens(Box<Expr>),
    /// `target := expr`
    Define {
        target: Box<Expr>,
        expr: Box<Expr>,
    },
    /// `target ≡ expr`
    GlobalDefine {
        target: Box<Expr>,
        expr: Box<Expr>,
    },
    /// Mathcad's `expr =` (numeric) or `expr->unit=` with a unit override.
    /// A symbolic `SymEval` as `expr` drops the trailing `=` (see the Display impl).
    /// An empty unit-override placeholder is represented as `None` by the importer.
    Eval {
        expr: Box<Expr>,
        unit_override: Option<Box<Expr>>,
    },
    /// Symbolic evaluation, Mathcad's `ml:symEval`: `expr -> command`, or `expr ->` when no
    /// command is given. `result` is Mathcad's cached symbolic output; it is kept for round
    /// trips but never printed, because the text form is the request, not the answer.
    ///
    /// Importer contract: an `ml:command` containing only a placeholder must map to
    /// `command: None`, not `Command(vec![Placeholder])`, which would print `x -> ■`.
    ///
    /// Known lossy case: `Eval { expr: SymEval, unit_override: None }` prints as the bare
    /// `SymEval` text (`x -> solve, x`, with no trailing `=`), so it reads back as `SymEval`.
    /// The `result` is never printed, so it never reads back either. The round-trip test in
    /// `mathcraft-import` tolerates exactly these two differences.
    SymEval {
        expr: Box<Expr>,
        command: Option<Command>,
        result: Option<Box<Expr>>,
    },
    /// A number or expression juxtaposed with a unit or expression, Mathcad's `ml:scale`
    /// (implicit multiplication, e.g. `0.75 m`, `45 °`, `6 x^3`). Printed `value unit`
    /// with a space; binds like `*`, so `a*(6 m)` keeps its parentheses.
    ///
    /// Importer contract: `unit` must not be a plain number. `6 2` would print as a
    /// number juxtaposed with a number, which reads back as something else.
    Scale {
        value: Box<Expr>,
        unit: Box<Expr>,
    },
    /// Summation or product over a range variable (Mathcad's Σ and Π with `i = m .. n`), text
    /// `sum(i, m, n, X)` or `product(i, m, n, X)`. The terms are `i = m, m+1, ...` while `i <= n`;
    /// the limits may be non-integer reals (help: summation operator). An empty range gives 0
    /// (sum) or 1 (product).
    Accumulate {
        op: Accumulation,
        var: Box<Id>,
        from: Box<Expr>,
        to: Box<Expr>,
        body: Box<Expr>,
    },
    /// The vector form of Σ and Π, text `sum(v)` or `product(v)`: the sum or product of all the
    /// elements of `v`.
    Fold {
        op: Accumulation,
        operand: Box<Expr>,
    },
    /// Definite integral of `body` with respect to `var` from `lower` to `upper`, text
    /// `integral(x, a, b, f)`. A bound of `∞` or `-∞` is an infinite limit (the bound names
    /// the infinity; it is not a value).
    Integral {
        var: Box<Id>,
        lower: Box<Expr>,
        upper: Box<Expr>,
        body: Box<Expr>,
    },
    /// The `order`-th derivative of `body` with respect to `var`, evaluated at the current value of
    /// `var`. Text `derivative(x, f)` (first order, `order` is `None`) or `derivative(x, n, f)`.
    ///
    /// `partial` is Prime's partial-derivative operator `∂/∂x`: the same numbers, but it prints and
    /// reads as `partial(x, f)` or `partial(x, n, f)` and is drawn with `∂`.
    Derivative {
        var: Box<Id>,
        order: Option<Box<Expr>>,
        body: Box<Expr>,
        partial: bool,
    },
    /// The gradient `∇ f` of a scalar function: the column vector of its partial derivatives with
    /// respect to `vars`. Text `gradient(f(a, b), a, b)`: the body first, then the variables. An
    /// empty variable list takes the arguments of a call body (`∇ f(a, b, c)`).
    Gradient {
        vars: Vec<Id>,
        body: Box<Expr>,
    },
    /// The limit of `body` as `var` approaches `target`, Mathcad's symbolic limit operator, text
    /// `limit(x, 9, o)`. It is two-sided: the one-sided forms (`+`, `-` on the arrow) are not
    /// represented, because no sample shows how Prime stores them.
    ///
    /// Symbolic in Prime ("you cannot evaluate limits using the numerical equal sign"), so the
    /// numeric evaluator refuses it as [`crate::eval::EvalError::Unsupported`].
    Limit {
        var: Box<Id>,
        target: Box<Expr>,
        body: Box<Expr>,
    },
    /// A string literal, text `"S"` (double quotes). The text has no escapes: Mathcad's string
    /// entry has none either, so a `"` cannot be written inside a literal.
    Str(String),
    Placeholder,
}

/// Which accumulation an [`Expr::Accumulate`] or [`Expr::Fold`] computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accumulation {
    Sum,
    Product,
}

impl Accumulation {
    /// The function name of the text form.
    pub fn name(self) -> &'static str {
        match self {
            Accumulation::Sum => "sum",
            Accumulation::Product => "product",
        }
    }
}

impl UnaryOp {
    pub fn prec(self) -> Prec {
        match self {
            UnaryOp::Neg => Prec::NEG,
            UnaryOp::Not => Prec::NOT,
            UnaryOp::Abs | UnaryOp::Norm => Prec::ATOM,
            UnaryOp::Transpose | UnaryOp::Factorial | UnaryOp::Percent => Prec::POSTFIX,
        }
    }
}

impl BinaryOp {
    pub fn prec(self) -> Prec {
        use BinaryOp::*;
        match self {
            Or => Prec::OR,
            Xor => Prec::XOR,
            And => Prec::AND,
            Eq | Ne | Lt | Gt | Le | Ge => Prec::CMP,
            Add | Sub => Prec::ADD,
            Mul | Div | Cross => Prec::MUL,
            Pow => Prec::POW,
        }
    }

    /// Minimum precedence of the (lhs, rhs) operands that print without parentheses.
    fn operand_mins(self) -> (Prec, Prec) {
        let p = self.prec();
        match self {
            // Postfix forms (`x!`, `v^T`, `x_i`) need parentheses as a power base: `(v^T)^2`.
            BinaryOp::Pow => (Prec::ATOM, Prec::POW),
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge => (p.above(), p.above()),
            _ => (p, p.above()),
        }
    }

    fn symbol(self) -> &'static str {
        use BinaryOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Cross => "×",
            Pow => "^",
            Eq => "==",
            Ne => "<>",
            Lt => "<",
            Gt => ">",
            Le => "<=",
            Ge => ">=",
            And => "∧",
            Or => "∨",
            Xor => "⊕",
        }
    }
}

impl Number {
    fn prec(&self) -> Prec {
        match self {
            Number::Real { text, .. } if text.starts_with('-') => Prec::NEG,
            Number::Real { .. } => Prec::ATOM,
            Number::Rational(r) if r.is_negative() => Prec::NEG,
            Number::Rational(r) if r.is_integer() => Prec::ATOM,
            Number::Rational(_) => Prec::MUL,
        }
    }
}

impl Id {
    pub fn new(name: &str, kind: IdKind) -> Self {
        Id {
            name: name.to_string(),
            kind,
            subscript: None,
        }
    }
}

impl Expr {
    /// Precedence of the outermost form of this node.
    pub fn prec(&self) -> Prec {
        match self {
            Expr::Number(n) => n.prec(),
            Expr::Unary { op, .. } => op.prec(),
            Expr::Binary { op, .. } => op.prec(),
            Expr::Index { .. } => Prec::POSTFIX,
            Expr::Range { .. } | Expr::SymEval { .. } => Prec::RANGE,
            Expr::Scale { .. } => Prec::MUL,
            Expr::Define { .. } | Expr::GlobalDefine { .. } | Expr::Eval { .. } => Prec::ASSIGN,
            _ => Prec::ATOM,
        }
    }

    /// A real number from an `f64`, printed with Rust's `to_string` (so `2` and `0.5`).
    ///
    /// `NaN` and infinities are accepted and print as `NaN` / `inf`; neither is valid Mathcad
    /// input, so callers must not pass them into worksheets.
    pub fn num(value: f64) -> Self {
        Expr::Number(Number::Real {
            value,
            text: value.to_string(),
        })
    }

    pub fn var(name: &str) -> Self {
        Expr::Id(Id::new(name, IdKind::Variable))
    }

    /// A variable with a literal subscript, e.g. `v_0`.
    pub fn subscripted(name: &str, subscript: Expr) -> Self {
        Expr::Id(Id {
            subscript: Some(Box::new(subscript)),
            ..Id::new(name, IdKind::Variable)
        })
    }

    pub fn unary(op: UnaryOp, operand: Expr) -> Self {
        Expr::Unary {
            op,
            operand: Box::new(operand),
        }
    }

    pub fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Self {
        Expr::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    pub fn call(name: &str, args: Vec<Expr>) -> Self {
        Expr::Call {
            func: Id::new(name, IdKind::Function),
            args,
        }
    }

    pub fn sqrt(radicand: Expr) -> Self {
        Expr::NthRoot {
            degree: None,
            radicand: Box::new(radicand),
        }
    }

    pub fn nth_root(degree: Expr, radicand: Expr) -> Self {
        Expr::NthRoot {
            degree: Some(Box::new(degree)),
            radicand: Box::new(radicand),
        }
    }

    /// A row-major matrix literal.
    ///
    /// # Panics
    ///
    /// Panics if `elements.len() != rows * cols`.
    pub fn matrix(rows: usize, cols: usize, elements: Vec<Expr>) -> Self {
        assert_eq!(
            rows * cols,
            elements.len(),
            "matrix element count must be rows * cols"
        );
        Expr::Matrix {
            rows,
            cols,
            elements,
        }
    }

    pub fn range(from: Expr, step: Option<Expr>, to: Expr) -> Self {
        Expr::Range {
            from: Box::new(from),
            step: step.map(Box::new),
            to: Box::new(to),
        }
    }

    pub fn index(base: Expr, indices: Vec<Expr>) -> Self {
        Expr::Index {
            base: Box::new(base),
            indices,
        }
    }
}

/// Prints `e`, parenthesized when it binds more loosely than `min`.
fn child(f: &mut fmt::Formatter<'_>, e: &Expr, min: Prec) -> fmt::Result {
    if e.prec() < min {
        write!(f, "({e})")
    } else {
        write!(f, "{e}")
    }
}

fn join(f: &mut fmt::Formatter<'_>, items: &[Expr], sep: &str) -> fmt::Result {
    for (i, e) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(sep)?;
        }
        write!(f, "{e}")?;
    }
    Ok(())
}

/// `_0` or `_i` for a plain number or name, `_{i+1}` otherwise.
fn write_subscript(f: &mut fmt::Formatter<'_>, s: &Expr) -> fmt::Result {
    let plain = matches!(
        s,
        Expr::Number(_)
            | Expr::Id(Id {
                subscript: None,
                ..
            })
    );
    if plain && s.prec() == Prec::ATOM {
        write!(f, "_{s}")
    } else {
        write!(f, "_{{{s}}}")
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Number::Real { text, .. } => f.write_str(text),
            Number::Rational(r) if r.is_integer() => write!(f, "{}", r.numer()),
            Number::Rational(r) => write!(f, "{}/{}", r.numer(), r.denom()),
        }
    }
}

/// Whether the parser reads `name` back as one identifier with no quotes: a letter, then letters,
/// digits and primes (`C'`), or the degree sign alone. Other names print in backticks.
fn plain_name(name: &str) -> bool {
    matches!(name, "°" | "°C" | "°F" | "°R")
        || (name.starts_with(char::is_alphabetic)
            && name.chars().all(|c| c.is_alphanumeric() || c == '\''))
}

/// A `Scale` unit that prints without parentheses: a unit name, or one raised to a number
/// (`m`, `m^2`). Anything else, such as a variable or a product, is parenthesized.
fn plain_unit(e: &Expr) -> bool {
    match e {
        Expr::Id(id) => id.kind == IdKind::Unit,
        Expr::Binary {
            op: BinaryOp::Pow,
            lhs,
            rhs,
        } => matches!(**rhs, Expr::Number(_)) && plain_unit(lhs),
        _ => false,
    }
}

impl fmt::Display for Id {
    /// A name that is not plain prints in backticks, with each backtick inside doubled. An empty
    /// name prints as ``` `` ```, which the parser rejects: an empty `Id` is invalid, and the
    /// converter never builds one.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if plain_name(&self.name) {
            f.write_str(&self.name)?;
        } else {
            write!(f, "`{}`", self.name.replace('`', "``"))?;
        }
        match &self.subscript {
            Some(s) => write_subscript(f, s),
            None => Ok(()),
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        join(f, &self.0, ", ")
    }
}

/// Appends one line per statement, indented four spaces per nesting level.
fn push_statements(stmts: &[Stmt], depth: usize, out: &mut Vec<String>) {
    let pad = "    ".repeat(depth);
    for stmt in stmts {
        match stmt {
            Stmt::Line(e) => out.push(format!("{pad}{e}")),
            Stmt::Local { target, expr } => out.push(format!("{pad}{target} ← {expr}")),
            Stmt::Branch { kind, cond, body } => {
                let head = match (kind, cond) {
                    (BranchKind::If, Some(c)) => format!("if {c}"),
                    (BranchKind::ElseIf, Some(c)) => format!("else if {c}"),
                    (BranchKind::AlsoIf, Some(c)) => format!("also if {c}"),
                    _ => "else".to_string(),
                };
                out.push(format!("{pad}{head}"));
                push_statements(body, depth + 1, out);
            }
            Stmt::For { var, range, body } => {
                out.push(format!("{pad}for {var} ∈ {range}"));
                push_statements(body, depth + 1, out);
            }
            Stmt::While { cond, body } => {
                out.push(format!("{pad}while {cond}"));
                push_statements(body, depth + 1, out);
            }
            Stmt::Break => out.push(format!("{pad}break")),
            Stmt::Continue => out.push(format!("{pad}continue")),
            Stmt::Return(e) => out.push(format!("{pad}return {e}")),
            Stmt::Try { body, on_error } => {
                out.push(format!("{pad}try"));
                push_statements(body, depth + 1, out);
                out.push(format!("{pad}on error"));
                push_statements(on_error, depth + 1, out);
            }
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Number(n) => write!(f, "{n}"),
            Expr::Id(id) => write!(f, "{id}"),
            Expr::Unary { op, operand } => {
                // Postfix operands must be atoms: `(v^T)!`, `(x!)^T`, never `v^T!`.
                let (pre, min, post) = match op {
                    UnaryOp::Neg => ("-", Prec::POW, ""),
                    UnaryOp::Not => ("¬", Prec::NOT, ""),
                    UnaryOp::Abs => return write!(f, "|{operand}|"),
                    UnaryOp::Norm => return write!(f, "‖{operand}‖"),
                    UnaryOp::Factorial => ("", Prec::ATOM, "!"),
                    UnaryOp::Percent => ("", Prec::ATOM, "%"),
                    UnaryOp::Transpose => ("", Prec::ATOM, "^T"),
                };
                f.write_str(pre)?;
                child(f, operand, min)?;
                f.write_str(post)
            }
            Expr::Binary { op, lhs, rhs } => {
                let (lmin, mut rmin) = op.operand_mins();
                // A negative right operand is parenthesized: `a-(-b)`, `a+(-b)`, `a*(-b)`,
                // `a/(-b)`. Without this `--`, `+-`, `*-` and `/-` runs would be ambiguous.
                let arithmetic = matches!(
                    op,
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
                );
                if arithmetic && rhs.prec() == Prec::NEG {
                    rmin = Prec::ATOM;
                }
                child(f, lhs, lmin)?;
                f.write_str(op.symbol())?;
                child(f, rhs, rmin)
            }
            Expr::Call { func, args } => {
                write!(f, "{func}(")?;
                join(f, args, ", ")?;
                f.write_str(")")
            }
            Expr::Range { from, step, to } => {
                child(f, from, Prec::ADD)?;
                if let Some(s) = step {
                    f.write_str(", ")?;
                    child(f, s, Prec::ADD)?;
                }
                f.write_str(" .. ")?;
                child(f, to, Prec::ADD)
            }
            Expr::Accumulate {
                op,
                var,
                from,
                to,
                body,
            } => write!(f, "{}({var}, {from}, {to}, {body})", op.name()),
            Expr::Fold { op, operand } => write!(f, "{}({operand})", op.name()),
            Expr::Integral {
                var,
                lower,
                upper,
                body,
            } => write!(f, "integral({var}, {lower}, {upper}, {body})"),
            Expr::Derivative {
                var,
                order,
                body,
                partial,
            } => {
                let name = if *partial { "partial" } else { "derivative" };
                match order {
                    Some(n) => write!(f, "{name}({var}, {n}, {body})"),
                    None => write!(f, "{name}({var}, {body})"),
                }
            }
            Expr::Gradient { vars, body } => {
                write!(f, "gradient({body}")?;
                for v in vars {
                    write!(f, ", {v}")?;
                }
                f.write_str(")")
            }
            Expr::Limit { var, target, body } => write!(f, "limit({var}, {target}, {body})"),
            Expr::Matrix { cols, elements, .. } => {
                f.write_str("[")?;
                for (i, row) in elements.chunks((*cols).max(1)).enumerate() {
                    if i > 0 {
                        f.write_str("; ")?;
                    }
                    join(f, row, ", ")?;
                }
                f.write_str("]")
            }
            Expr::Index { base, indices } => {
                // Postfix operator bases keep their parentheses: `(v^T)[1]`, `(x!)[1]`,
                // `(x%)[1]`. Nested indexes and name subscripts need none: `M[i][j]`, `v_0[1]`.
                match &**base {
                    Expr::Unary {
                        op: UnaryOp::Transpose | UnaryOp::Factorial | UnaryOp::Percent,
                        ..
                    } => write!(f, "({base})")?,
                    _ => child(f, base, Prec::POSTFIX)?,
                }
                f.write_str("[")?;
                join(f, indices, ",")?;
                f.write_str("]")
            }
            Expr::NthRoot { degree, radicand } => match degree.as_deref() {
                None | Some(Expr::Placeholder) => write!(f, "sqrt({radicand})"),
                Some(n) => write!(f, "root({radicand}, {n})"),
            },
            Expr::Parens(e) => write!(f, "({e})"),
            Expr::Define { target, expr } | Expr::GlobalDefine { target, expr } => {
                let op = if matches!(self, Expr::Define { .. }) {
                    ":="
                } else {
                    "≡"
                };
                child(f, target, Prec::POSTFIX)?;
                f.write_str(op)?;
                child(f, expr, Prec::RANGE)
            }
            Expr::Eval {
                expr,
                unit_override,
            } => {
                // A symbolic evaluation already ends in `->`; adding `=` would make it numeric.
                if let (Expr::SymEval { .. }, None) = (&**expr, unit_override) {
                    return write!(f, "{expr}");
                }
                // A unit override needs `(x ->)->m=` and `(0 .. 3)->m=`, so a range or
                // symbolic evaluation is parenthesized there. Without one, `0 .. 3=` stays.
                let min = if unit_override.is_some() {
                    Prec::OR
                } else {
                    Prec::RANGE
                };
                child(f, expr, min)?;
                if let Some(u) = unit_override {
                    f.write_str("->")?;
                    child(f, u, Prec::POSTFIX)?;
                }
                f.write_str("=")
            }
            Expr::SymEval { expr, command, .. } => {
                // Nested evaluations and ranges need parentheses: `(x ->) -> solve`.
                child(f, expr, Prec::OR)?;
                match command {
                    // An empty command prints like `None`, without a trailing space.
                    Some(c) if !c.0.is_empty() => write!(f, " -> {c}"),
                    _ => f.write_str(" ->"),
                }
            }
            Expr::Scale { value, unit } => {
                // A matrix unit is parenthesized: `text ([1, 2])`. Bare `text [1, 2]` would be
                // an index with a space before the bracket, which the parser rejects.
                let matrix = matches!(&**unit, Expr::Matrix { .. });
                // A name value keeps its bare form only when the parser reads the unit back after
                // it: `x m`, `x ([1, 2])`. Otherwise it is parenthesized, as `(x) (y)`. A number
                // takes any group after it (`6 (x+1)`, `7 (x)`, `6 (m/s)`), as the parser reads it.
                let bare = match &**value {
                    Expr::Id(_) => matrix || plain_unit(unit),
                    _ => true,
                };
                if bare {
                    child(f, value, Prec::ATOM)?;
                } else {
                    write!(f, "({value})")?;
                }
                f.write_str(" ")?;
                if matrix {
                    write!(f, "({unit})")
                } else if plain_unit(unit) {
                    child(f, unit, Prec::POW)
                } else if matches!(&**unit, Expr::Parens(_)) {
                    write!(f, "{unit}")
                } else {
                    write!(f, "({unit})")
                }
            }
            Expr::Program(stmts) => {
                let mut lines = Vec::new();
                push_statements(stmts, 0, &mut lines);
                f.write_str(&lines.join("\n"))
            }
            Expr::Str(s) => write!(f, "\"{s}\""),
            Expr::Placeholder => f.write_str("■"),
        }
    }
}

// Table-style tests: one test per line so the list stays scannable.
#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use BinaryOp as B;
    use UnaryOp as U;

    fn s(e: &Expr) -> String { e.to_string() }
    fn v(name: &str) -> Expr { Expr::var(name) }
    fn n(x: f64) -> Expr { Expr::num(x) }
    fn b(op: BinaryOp, l: Expr, r: Expr) -> Expr { Expr::binary(op, l, r) }
    fn u(op: UnaryOp, e: Expr) -> Expr { Expr::unary(op, e) }
    fn rat(p: i32, q: i32) -> Expr { Expr::Number(Number::Rational(BigRational::new(p.into(), q.into()))) }
    fn assign(t: Expr, e: Expr) -> Expr { Expr::Define { target: Box::new(t), expr: Box::new(e) } }
    fn range(a: Expr, step: Option<Expr>, c: Expr) -> Expr { Expr::range(a, step, c) }
    fn sym(e: Expr, cmd: Option<Command>) -> Expr { Expr::SymEval { expr: Box::new(e), command: cmd, result: None } }
    fn cmd(words: &[&str]) -> Command { Command(words.iter().map(|w| v(w)).collect()) }
    fn scale(value: Expr, unit: Expr) -> Expr { Expr::Scale { value: Box::new(value), unit: Box::new(unit) } }
    fn unit(name: &str) -> Expr { Expr::Id(Id { name: name.to_string(), kind: IdKind::Unit, subscript: None }) }

    #[test] fn integer_number() { assert_eq!(s(&n(3.0)), "3"); }
    #[test] fn original_text_kept() { assert_eq!(s(&Expr::Number(Number::Real { value: 2.5, text: "2.50".into() })), "2.50"); }
    #[test] fn rational_number() { assert_eq!(s(&rat(1, 3)), "1/3"); assert_eq!(s(&rat(4, 2)), "2"); }
    #[test] fn rational_as_power_base() { assert_eq!(s(&b(B::Pow, rat(1, 3), n(2.0))), "(1/3)^2"); }
    #[test] fn negative_number_as_power_base() { assert_eq!(s(&b(B::Pow, n(-1.0), n(2.0))), "(-1)^2"); }
    #[test] fn square() { assert_eq!(s(&b(B::Pow, v("x"), n(2.0))), "x^2"); }
    #[test] fn power_right_associative() { assert_eq!(s(&b(B::Pow, v("x"), b(B::Pow, v("y"), n(2.0)))), "x^y^2"); }
    #[test] fn power_nested_on_left() { assert_eq!(s(&b(B::Pow, b(B::Pow, v("x"), n(2.0)), n(3.0))), "(x^2)^3"); }
    #[test] fn product_with_sum_right() { assert_eq!(s(&b(B::Mul, v("a"), b(B::Add, v("b"), v("c")))), "a*(b+c)"); }
    #[test] fn product_with_sum_left() { assert_eq!(s(&b(B::Mul, b(B::Add, v("a"), v("b")), v("c"))), "(a+b)*c"); }
    #[test] fn addition_left_nested() { assert_eq!(s(&b(B::Add, b(B::Add, v("a"), v("b")), v("c"))), "a+b+c"); }
    #[test] fn addition_right_nested() { assert_eq!(s(&b(B::Add, v("a"), b(B::Add, v("b"), v("c")))), "a+(b+c)"); }
    #[test] fn subtraction_chain() { assert_eq!(s(&b(B::Sub, b(B::Sub, v("a"), v("b")), v("c"))), "a-b-c"); }
    #[test] fn subtraction_right_nested() { assert_eq!(s(&b(B::Sub, v("a"), b(B::Sub, v("b"), v("c")))), "a-(b-c)"); }
    #[test] fn division_by_product() { assert_eq!(s(&b(B::Div, v("a"), b(B::Mul, v("b"), v("c")))), "a/(b*c)"); }
    #[test] fn product_with_quotient() { assert_eq!(s(&b(B::Mul, v("a"), b(B::Div, v("b"), v("c")))), "a*(b/c)"); }
    #[test] fn negation_below_power() { assert_eq!(s(&u(U::Neg, b(B::Pow, v("x"), n(2.0)))), "-x^2"); }
    #[test] fn negated_power_base() { assert_eq!(s(&b(B::Pow, u(U::Neg, v("x")), n(2.0))), "(-x)^2"); }
    #[test] fn double_negation() { assert_eq!(s(&u(U::Neg, u(U::Neg, v("x")))), "-(-x)"); }
    #[test] fn negated_sum() { assert_eq!(s(&u(U::Neg, b(B::Add, v("a"), v("b")))), "-(a+b)"); }
    #[test] fn subtract_negation() { assert_eq!(s(&b(B::Sub, v("a"), u(U::Neg, v("b")))), "a-(-b)"); }
    #[test] fn add_negation() { assert_eq!(s(&b(B::Add, v("a"), u(U::Neg, v("b")))), "a+(-b)"); }
    #[test] fn multiply_negation() { assert_eq!(s(&b(B::Mul, v("a"), u(U::Neg, v("b")))), "a*(-b)"); }
    #[test] fn divide_negation() { assert_eq!(s(&b(B::Div, v("a"), u(U::Neg, v("b")))), "a/(-b)"); }
    #[test] fn multiply_negative_number() { assert_eq!(s(&b(B::Mul, v("a"), n(-2.0))), "a*(-2)"); }
    #[test] fn division_by_quotient() { assert_eq!(s(&b(B::Div, v("a"), b(B::Div, v("b"), v("c")))), "a/(b/c)"); }
    #[test] fn quotient_left_nested() { assert_eq!(s(&b(B::Div, b(B::Div, v("a"), v("b")), v("c"))), "a/b/c"); }
    #[test] fn power_of_half() { assert_eq!(s(&b(B::Pow, v("x"), rat(1, 2))), "x^(1/2)"); }
    #[test] fn non_finite_numbers_print_verbatim() { assert_eq!(s(&n(f64::NAN)), "NaN"); assert_eq!(s(&n(f64::INFINITY)), "inf"); }
    #[test] #[should_panic(expected = "rows * cols")] fn matrix_count_mismatch_panics() { Expr::matrix(2, 2, vec![n(1.0)]); }
    #[test] fn factorial_of_name() { assert_eq!(s(&u(U::Factorial, v("x"))), "x!"); }
    #[test] fn factorial_of_power() { assert_eq!(s(&u(U::Factorial, b(B::Pow, v("x"), n(2.0)))), "(x^2)!"); }
    #[test] fn power_of_factorial() { assert_eq!(s(&b(B::Pow, u(U::Factorial, v("x")), n(2.0))), "(x!)^2"); }
    #[test] fn power_of_transpose() { assert_eq!(s(&b(B::Pow, u(U::Transpose, v("v")), n(2.0))), "(v^T)^2"); }
    #[test] fn transpose_of_transpose() { assert_eq!(s(&u(U::Transpose, u(U::Transpose, v("v")))), "(v^T)^T"); }
    #[test] fn transpose_of_factorial() { assert_eq!(s(&u(U::Transpose, u(U::Factorial, v("x")))), "(x!)^T"); }
    #[test] fn factorial_of_transpose() { assert_eq!(s(&u(U::Factorial, u(U::Transpose, v("v")))), "(v^T)!"); }
    #[test] fn index_of_subscripted_base() { assert_eq!(s(&Expr::index(Expr::subscripted("v", n(0.0)), vec![n(1.0)])), "v_0[1]"); }
    #[test] fn index_of_index_base() { assert_eq!(s(&Expr::index(Expr::index(v("M"), vec![v("i")]), vec![v("j")])), "M[i][j]"); }
    #[test] fn index_of_postfix_base() { assert_eq!(s(&Expr::index(u(U::Transpose, v("v")), vec![n(1.0)])), "(v^T)[1]"); assert_eq!(s(&Expr::index(u(U::Factorial, v("x")), vec![n(1.0)])), "(x!)[1]"); assert_eq!(s(&Expr::index(u(U::Percent, v("x")), vec![n(1.0)])), "(x%)[1]"); }
    #[test] fn index_and_subscript_print_differently() { assert_eq!(s(&Expr::index(v("v"), vec![v("i")])), "v[i]"); assert_eq!(s(&Expr::subscripted("v", v("i"))), "v_i"); }
    #[test] fn sym_eval_without_command() { assert_eq!(s(&sym(b(B::Add, v("a"), v("b")), None)), "a+b ->"); }
    #[test] fn sym_eval_with_command() { assert_eq!(s(&sym(b(B::Pow, v("x"), n(2.0)), Some(cmd(&["simplify"])))), "x^2 -> simplify"); }
    #[test] fn sym_eval_result_not_printed() { let e = Expr::SymEval { expr: Box::new(v("y")), command: Some(cmd(&["solve", "x"])), result: Some(Box::new(n(1.0))) }; assert_eq!(s(&e), "y -> solve, x"); }
    #[test] fn sym_eval_inside_define() { assert_eq!(s(&assign(Expr::matrix(1, 2, vec![v("a"), v("b")]), sym(v("M"), Some(cmd(&["solve", "a", "b"]))))), "[a, b]:=M -> solve, a, b"); }
    #[test] fn sym_eval_inside_sum_parenthesized() { assert_eq!(s(&b(B::Add, sym(v("x"), None), n(1.0))), "(x ->)+1"); }
    #[test] fn symbolic_eval_drops_equals() { assert_eq!(s(&Expr::Eval { expr: Box::new(sym(v("x"), Some(cmd(&["solve", "x"])))), unit_override: None }), "x -> solve, x"); }
    #[test] fn sym_eval_of_sym_eval_parenthesized() { assert_eq!(s(&sym(sym(v("x"), None), Some(cmd(&["solve"])))), "(x ->) -> solve"); }
    #[test] fn sym_eval_of_range_parenthesized() { assert_eq!(s(&sym(range(n(1.0), None, n(3.0)), Some(cmd(&["solve"])))), "(1 .. 3) -> solve"); }
    #[test] fn sym_eval_empty_command_prints_like_none() { assert_eq!(s(&sym(v("x"), Some(Command(vec![])))), "x ->"); assert_eq!(s(&sym(v("x"), None)), "x ->"); }
    #[test] fn eval_unit_of_sym_eval_parenthesized() { assert_eq!(s(&Expr::Eval { expr: Box::new(sym(v("x"), None)), unit_override: Some(Box::new(v("m"))) }), "(x ->)->m="); assert_eq!(s(&Expr::Eval { expr: Box::new(sym(v("x"), Some(cmd(&["solve", "x"])))), unit_override: Some(Box::new(v("m"))) }), "(x -> solve, x)->m="); }
    #[test] fn eval_unit_of_range_parenthesized() { assert_eq!(s(&Expr::Eval { expr: Box::new(range(n(0.0), None, n(3.0))), unit_override: Some(Box::new(v("m"))) }), "(0 .. 3)->m="); }
    #[test] fn command_keywords() { assert_eq!(cmd(&["solve", "a", "b", "c"]).to_string(), "solve, a, b, c"); }
    #[test] fn scale_decimal_with_unit() { assert_eq!(s(&scale(n(0.75), unit("m"))), "0.75 m"); }
    #[test] fn scale_matrix_unit_parenthesized() { assert_eq!(s(&scale(v("text"), Expr::matrix(1, 2, vec![n(1.0), n(2.0)]))), "text ([1, 2])"); }
    #[test] fn primes_print_plainly() { assert_eq!(s(&v("C'")), "C'"); assert_eq!(s(&Expr::subscripted("B'", n(1.0))), "B'_1"); }
    #[test] fn name_with_underscore_is_quoted() { assert_eq!(s(&v("F_A")), "`F_A`"); assert_eq!(s(&Expr::subscripted("F", v("A"))), "F_A"); }
    #[test] fn name_with_backtick_doubles_it() { assert_eq!(s(&v("a`b")), "`a``b`"); assert_eq!(s(&v("`")), "````"); }
    #[test] fn name_with_non_letter_is_quoted() { assert_eq!(s(&v("μ_02")), "`μ_02`"); assert_eq!(s(&v("°")), "°"); }
    #[test] fn scale_degree_symbol() { assert_eq!(s(&scale(n(45.0), unit("°"))), "45 °"); }
    #[test] fn scale_power_unit() { assert_eq!(s(&scale(n(6.0), b(B::Pow, unit("x"), n(3.0)))), "6 x^3"); }
    #[test] fn scale_fraction_value_parenthesized() { assert_eq!(s(&scale(rat(1, 2), unit("m"))), "(1/2) m"); }
    #[test] fn scale_inside_product_parenthesized() { assert_eq!(s(&b(B::Mul, v("a"), scale(n(6.0), unit("m")))), "a*(6 m)"); }
    #[test] fn scale_divided_by_unit() { assert_eq!(s(&b(B::Div, scale(n(6.0), unit("m")), unit("s"))), "6 m/s"); }
    #[test] fn function_definition_with_bound_vars() { assert_eq!(s(&assign(Expr::call("g", vec![v("x"), v("P"), v("Q")]), b(B::Add, v("x"), v("P")))), "g(x, P, Q):=x+P"); }
    #[test] fn function_call_from_sequence_args() { assert_eq!(s(&Expr::call("g", vec![v("x"), v("A"), v("B")])), "g(x, A, B)"); }
    #[test] fn percent_and_transpose() { assert_eq!(s(&u(U::Percent, n(5.0))), "5%"); assert_eq!(s(&u(U::Transpose, v("v"))), "v^T"); }
    #[test] fn square_root_forms() { assert_eq!(s(&Expr::sqrt(b(B::Add, v("x"), n(1.0)))), "sqrt(x+1)"); assert_eq!(s(&Expr::NthRoot { degree: Some(Box::new(Expr::Placeholder)), radicand: Box::new(v("x")) }), "sqrt(x)"); }
    #[test] fn nth_root_with_degree() { assert_eq!(s(&Expr::nth_root(n(3.0), v("x"))), "root(x, 3)"); }
    #[test] fn subscript_plain_and_power() { assert_eq!(s(&Expr::subscripted("v", n(0.0))), "v_0"); assert_eq!(s(&b(B::Pow, Expr::subscripted("v", n(0.0)), n(2.0))), "v_0^2"); }
    #[test] fn subscript_compound_braced() { assert_eq!(s(&Expr::subscripted("v", b(B::Add, v("i"), n(1.0)))), "v_{i+1}"); assert_eq!(s(&Expr::subscripted("v", Expr::subscripted("w", n(0.0)))), "v_{w_0}"); }
    #[test] fn index_single_and_multi() { assert_eq!(s(&Expr::index(v("v"), vec![v("i")])), "v[i]"); assert_eq!(s(&Expr::index(v("M"), vec![v("i"), v("j")])), "M[i,j]"); }
    #[test] fn index_of_compound_base() { assert_eq!(s(&Expr::index(b(B::Add, v("A"), v("B")), vec![n(1.0)])), "(A+B)[1]"); }
    #[test] fn index_of_call_and_matrix_bases() { assert_eq!(s(&Expr::index(Expr::call("f", vec![v("x")]), vec![n(2.0)])), "f(x)[2]"); assert_eq!(s(&Expr::index(Expr::matrix(1, 2, vec![n(1.0), n(2.0)]), vec![n(1.0)])), "[1, 2][1]"); }
    #[test] fn matrix_literal() { assert_eq!(s(&Expr::matrix(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)])), "[1, 2; 3, 4]"); }
    #[test] fn range_default_step() { assert_eq!(s(&range(n(0.0), None, b(B::Sub, v("n"), n(1.0)))), "0 .. n-1"); }
    #[test] fn range_explicit_step() { assert_eq!(s(&range(n(0.0), Some(n(0.5)), n(3.0))), "0, 0.5 .. 3"); }
    #[test] fn range_inside_sum() { assert_eq!(s(&b(B::Add, range(n(0.0), None, n(3.0)), n(1.0))), "(0 .. 3)+1"); }
    #[test] fn function_calls() { assert_eq!(s(&Expr::call("sin", vec![v("x")])), "sin(x)"); assert_eq!(s(&Expr::call("atan2", vec![v("y"), v("x")])), "atan2(y, x)"); }
    #[test] fn define_and_global_define() { assert_eq!(s(&assign(v("a"), v("b"))), "a:=b"); assert_eq!(s(&Expr::GlobalDefine { target: Box::new(v("a")), expr: Box::new(v("b")) }), "a≡b"); }
    #[test] fn define_range_and_indexed_target() { assert_eq!(s(&assign(v("w"), range(n(0.0), None, n(3.0)))), "w:=0 .. 3"); assert_eq!(s(&assign(Expr::index(v("v"), vec![v("i")]), n(0.0))), "v[i]:=0"); }
    #[test] fn evaluation_plain_and_unit() { assert_eq!(s(&Expr::Eval { expr: Box::new(b(B::Add, v("a"), v("b"))), unit_override: None }), "a+b="); assert_eq!(s(&Expr::Eval { expr: Box::new(v("x")), unit_override: Some(Box::new(v("m"))) }), "x->m="); }
    #[test] fn boolean_precedence() { assert_eq!(s(&u(U::Not, b(B::Lt, v("x"), v("y")))), "¬x<y"); assert_eq!(s(&b(B::Or, b(B::And, v("a"), v("b")), v("c"))), "a∧b∨c"); assert_eq!(s(&b(B::And, b(B::Or, v("a"), v("b")), v("c"))), "(a∨b)∧c"); }
    #[test] fn comparison_operators() { assert_eq!(s(&b(B::Eq, v("a"), v("b"))), "a==b"); assert_eq!(s(&b(B::Ne, v("a"), v("b"))), "a<>b"); assert_eq!(s(&b(B::Le, v("a"), v("b"))), "a<=b"); }
    #[test] fn comparison_chain_parenthesized() { assert_eq!(s(&b(B::Lt, b(B::Lt, v("a"), v("b")), v("c"))), "(a<b)<c"); }
    #[test] fn absolute_value() { assert_eq!(s(&u(U::Abs, b(B::Add, v("a"), v("b")))), "|a+b|"); }
    #[test] fn explicit_parens_kept() { assert_eq!(s(&Expr::Parens(Box::new(v("x")))), "(x)"); assert_eq!(s(&b(B::Mul, v("a"), Expr::Parens(Box::new(b(B::Add, v("b"), v("c")))))), "a*(b+c)"); }
    #[test] fn placeholder() { assert_eq!(s(&Expr::Placeholder), "■"); }
    #[test] fn string_literal() { assert_eq!(s(&Expr::Str("yes".into())), "\"yes\""); }
}

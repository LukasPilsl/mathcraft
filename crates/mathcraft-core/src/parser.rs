//! Text parser: Mathcad-style text back into [`Expr`], the inverse of the printer in `expr.rs`.
//!
//! Binding strength comes from the printer's [`Prec`] table, so the two cannot drift apart.
//! Several trees print to the same text, so the parser picks one reading for each. These
//! shapes do not round-trip exactly:
//!
//! - `v_i`, `v_{i+1}` (a literal subscript on a name) is always `Id` with a subscript. `v[i]`
//!   is `Index`, and `v[i][j]` is nested `Index`. `M_{i,j}` is an error: a literal subscript
//!   has one item, and several indices are written `M[i,j]`. `(v_0)[1]` reads as `v_0[1]`.
//! - `[` directly after an operand (no whitespace before it) is an index. Elsewhere `[` starts a
//!   matrix literal, so `[1, 2][1]` is an `Index` of a matrix. A space before `[` after an
//!   operand is an error.
//! - Parentheses around an operator expression are not kept: `(a+b)` is `Add`. Parentheses
//!   around an atom are kept as `Parens`, so `(x)` is `Parens(x)`.
//! - `-2` is a negative number literal, not `Neg(2)`. `1/3` is a `Div`, as stored by the importer;
//!   the parser never builds a `Rational` (only the symbolic core does).
//! - `x^T` is always `Transpose`. `sqrt(x)` has no degree.
//! - `x ->` and `x -> ■` both read as `command: None`.
//! - `x -> m=` with one item before `=` is a unit override (`Eval`), not a symbolic command.
//! - `x -> solve, x` with no `=` is `SymEval`; the printer drops the `=`, so it is not `Eval`.
//! - A bare identifier after a value is a `Scale` unit only when the context says it is a unit
//!   (`is_scale_unit`, by default `IdKind::Unit`). `6 x` with `x` a variable reads as `Mul`.
//! - A parenthesized group after a value is a `Scale` unit in three cases (see
//!   `paren_unit_after`). Otherwise it is not consumed, so it is an `unexpected '('` error.
//!   1. After an operand that ends with `)` or `]` (a parenthesized value, a call, an index),
//!      any group is the unit: `(a) (b)`, `(1/(x+2)) (2/(x+3))`, `f(x) (y)`, `v[1] (m)`.
//!      A value that ends with a number or a name, such as `(a+b)^2`, does not qualify.
//!   2. After a number, any group: `42 (m/s)`, `7 (x)`, `2 (x+1)`, `0.05 .. 4 (π)` are a `Scale`
//!      (implicit multiplication, which the printer writes this way). `2 ()`, `2 (2)` (a number
//!      is not a unit) and `2 (m, s)` are errors. A unit expression is still the usual unit.
//!   3. After any operand, a matrix literal in parentheses with any elements: `text ([1, 2])`
//!      and `text ([sqrt(a^2)])`, as the printer writes them. `x (y)` stays an error, because a
//!      name before a plain group may be a call. A bare `[` after a space is never a unit.
//!
//!   Redundant parentheses around the unit are dropped, so `2 (((m)))` is `2 m`.
//! - `g_1(x)` and `S_a(x)` are a `Call` whose function keeps the subscript. A space before `(`
//!   ends the name, so `g_1 (x)` is a `Scale`.
//! - `'` directly after a name is part of the name (`C'`, `B''`, `C'_1`). After a number or a
//!   closing bracket it is an error.
//! - A name in backticks is one identifier of any text: `` `F_A` `` is the name `F_A`, not `F`
//!   with subscript `A`. The printer uses this form for names it cannot print plainly. A
//!   backtick inside the name is doubled: `` `a``b` `` is the name ``a`b``. An empty name
//!   (`` `` ``) is an error; the converter never makes one.
//! - A subscripted name takes its kind from `IdContext::classify_subscripted`, so `v` (a unit)
//!   and `v_0` (a variable) can both occur in one expression.
//! - `1e-5`, `2E+3` and `6.02e23` are one number literal, kept as written. An exponent needs
//!   digits after `e` and no identifier character after them: `2e` is `2 * e`, `2e3x` is
//!   `2 * e3x`. A space ends the number, so `2 e3` is `2 * e3` as well.
//! - Nesting deeper than [`MAX_DEPTH`] and inputs over [`MAX_TOKENS`] tokens are `ParseError`s.

use std::borrow::Cow;

use crate::expr::{Accumulation, BinaryOp, Command, Expr, Id, IdKind, Number, Prec, UnaryOp};

/// A parse failure: the byte offset in the source and a short message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("parse error at byte {pos}: {message}")]
pub struct ParseError {
    pub pos: usize,
    pub message: String,
}

type PResult<T> = Result<T, ParseError>;

/// Classifies identifiers while parsing. Unit and constant names come from the caller.
pub trait IdContext {
    /// `called` is true when the name is immediately followed by `(`.
    fn classify(&self, _name: &str, called: bool) -> IdKind {
        if called {
            IdKind::Function
        } else {
            IdKind::Variable
        }
    }

    /// Whether a value followed by this bare name is a `Scale` (`6 m`), rather than a product
    /// (`6 x`). By default only names classified as units are. A caller that knows which names
    /// were written as units, whatever their kind, may override this.
    fn is_scale_unit(&self, name: &str) -> bool {
        self.classify(name, false) == IdKind::Unit
    }

    /// Kind of a name written with a literal subscript (`v_0`). Defaults to the kind of the bare
    /// name, but a caller may give a subscripted name a different kind from the bare one.
    fn classify_subscripted(&self, name: &str) -> IdKind {
        self.classify(name, false)
    }
}

/// Every called name is a `Function` and every other name a `Variable`.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultIdContext;

impl IdContext for DefaultIdContext {}

/// Parses one statement (the printer's output for any [`Expr`]) and rejects trailing input.
pub fn parse(src: &str, ctx: &dyn IdContext) -> PResult<Expr> {
    let toks = lex(src)?;
    let mut p = Parser {
        toks,
        i: 0,
        ctx,
        depth: 0,
    };
    let e = p.statement(true)?;
    if !matches!(p.peek().tok, Tok::Eof) {
        return err(p.peek().start, format!("unexpected {}", p.describe()));
    }
    Ok(e)
}

fn err<T>(pos: usize, message: impl Into<String>) -> PResult<T> {
    Err(ParseError {
        pos,
        message: message.into(),
    })
}

/// Drops redundant parentheses around a unit, so `2 (((m)))` reads as `2 m`.
fn strip_parens(mut e: Expr) -> Expr {
    while let Expr::Parens(inner) = e {
        e = *inner;
    }
    e
}

// ---------------------------------------------------------------------------------------------
// Lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok<'a> {
    Num(&'a str),
    /// A name. Borrowed from the source, except a quoted name with doubled backticks, which
    /// is unescaped into an owned string.
    Ident(Cow<'a, str>),
    /// A string literal's text, without the quotes. Borrowed from the source.
    Str(&'a str),
    Punct(&'static str),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
struct Token<'a> {
    tok: Tok<'a>,
    start: usize,
    end: usize,
}

const TWO_CHAR: [&str; 7] = ["->", ":=", "==", "<>", "<=", ">=", ".."];

fn single_char(c: char) -> Option<&'static str> {
    Some(match c {
        '(' => "(",
        ')' => ")",
        '[' => "[",
        ']' => "]",
        '{' => "{",
        '}' => "}",
        '|' => "|",
        ',' => ",",
        ';' => ";",
        '_' => "_",
        '!' => "!",
        '%' => "%",
        '^' => "^",
        '*' => "*",
        '×' => "×",
        '‖' => "‖",
        '/' => "/",
        '+' => "+",
        '-' => "-",
        '=' => "=",
        '<' => "<",
        '>' => ">",
        '≡' => "≡",
        '∧' => "∧",
        '∨' => "∨",
        '⊕' => "⊕",
        '¬' => "¬",
        '■' => "■",
        _ => return None,
    })
}

fn scan_while(src: &str, from: usize, pred: impl Fn(char) -> bool) -> usize {
    let mut i = from;
    while let Some(c) = src[i..].chars().next() {
        if !pred(c) {
            break;
        }
        i += c.len_utf8();
    }
    i
}

/// End of an exponent (`e`, `E`, optional sign, digits) that starts at `from`, or `from` when
/// there is none. An identifier character right after the digits cancels it, so `2e3x` stays
/// the number `2` followed by the identifier `e3x`.
fn scan_exponent(src: &str, from: usize) -> usize {
    if !matches!(src[from..].chars().next(), Some('e' | 'E')) {
        return from;
    }
    let mut j = from + 1;
    if matches!(src[j..].chars().next(), Some('+' | '-')) {
        j += 1;
    }
    let end = scan_while(src, j, |c| c.is_ascii_digit());
    if end == j || src[end..].chars().next().is_some_and(char::is_alphanumeric) {
        return from;
    }
    end
}

/// Byte index of the backtick that closes the quoted name opened at `open`. Doubled backticks
/// inside the name are skipped.
fn quoted_name_end(src: &str, open: usize) -> PResult<usize> {
    let mut j = open + 1;
    loop {
        match src[j..].find('`') {
            None => return err(open, "unterminated quoted name"),
            Some(n) => {
                let k = j + n;
                if src[k + 1..].starts_with('`') {
                    j = k + 2;
                } else {
                    return Ok(k);
                }
            }
        }
    }
}

/// Most tokens (not counting the end marker) one input may have. Bounds the work and the size
/// of the tree, which is dropped recursively.
const MAX_TOKENS: usize = 4096;

/// Longest string literal the parser reads, in bytes. Matches the size cap of the string functions.
pub const MAX_STRING_BYTES: usize = 1 << 20;

fn lex(src: &str) -> PResult<Vec<Token<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(c) = src[i..].chars().next() {
        let start = i;
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let tok = if c.is_ascii_digit() {
            i = scan_while(src, i, |c| c.is_ascii_digit());
            // A fraction needs a digit after a single dot, so `0..5` stays a range.
            if src[i..].starts_with('.') && src[i + 1..].starts_with(|d: char| d.is_ascii_digit()) {
                i = scan_while(src, i + 1, |c| c.is_ascii_digit());
            }
            i = scan_exponent(src, i);
            Tok::Num(&src[start..i])
        } else if c == '°' || c == '∞' {
            // A degree sign, or the infinite limit of an integral (a single-symbol name). The
            // degree sign followed by C, F or R (not part of a longer name) is a temperature
            // unit: `25 °C`, while `30°` and `30° C` stay an angle.
            i += c.len_utf8();
            if c == '°' {
                if let Some(t) = src[i..]
                    .chars()
                    .next()
                    .filter(|t| matches!(t, 'C' | 'F' | 'R'))
                {
                    let after = i + t.len_utf8();
                    if !src[after..].starts_with(|n: char| n.is_alphanumeric() || n == '\'') {
                        i = after;
                    }
                }
            }
            Tok::Ident(Cow::Borrowed(&src[start..i]))
        } else if c.is_alphabetic() {
            // A prime right after a name is part of it (`C'`, `B''`). After a number or a
            // closing bracket it is not a name character, so it stays unexpected.
            i = scan_while(src, i, |c| c.is_alphanumeric() || c == '\'');
            Tok::Ident(Cow::Borrowed(&src[start..i]))
        } else if c == '`' {
            // A quoted name runs to the next unpaired backtick and may hold any text, such as
            // `F_A`. A doubled backtick inside it is one backtick of the name.
            let close = quoted_name_end(src, start)?;
            let raw = &src[start + 1..close];
            if raw.is_empty() {
                return err(start, "empty quoted name");
            }
            i = close + 1;
            Tok::Ident(if raw.contains("``") {
                Cow::Owned(raw.replace("``", "`"))
            } else {
                Cow::Borrowed(raw)
            })
        } else if c == '"' {
            // A string literal runs to the next double quote. It has no escapes (see `Expr::Str`).
            let Some(n) = src[i + 1..].find('"') else {
                return err(start, "unterminated string");
            };
            let raw = &src[i + 1..i + 1 + n];
            if raw.len() > MAX_STRING_BYTES {
                return err(
                    start,
                    format!("string is longer than {MAX_STRING_BYTES} bytes"),
                );
            }
            i = i + 1 + n + 1;
            Tok::Str(raw)
        } else if let Some(p) = TWO_CHAR.iter().copied().find(|p| src[i..].starts_with(*p)) {
            i += p.len();
            Tok::Punct(p)
        } else if let Some(p) = single_char(c) {
            i += c.len_utf8();
            Tok::Punct(p)
        } else {
            return err(i, format!("unexpected character '{c}'"));
        };
        if out.len() >= MAX_TOKENS {
            return err(start, format!("input has more than {MAX_TOKENS} tokens"));
        }
        out.push(Token { tok, start, end: i });
    }
    out.push(Token {
        tok: Tok::Eof,
        start: src.len(),
        end: src.len(),
    });
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Parser

/// The level directly above `p` in the printer's table, so right operands can bind tighter.
const LEVELS: [Prec; 13] = [
    Prec::ASSIGN,
    Prec::RANGE,
    Prec::OR,
    Prec::XOR,
    Prec::AND,
    Prec::NOT,
    Prec::CMP,
    Prec::ADD,
    Prec::MUL,
    Prec::NEG,
    Prec::POW,
    Prec::POSTFIX,
    Prec::ATOM,
];

fn above(p: Prec) -> Prec {
    LEVELS
        .iter()
        .copied()
        .find(|&q| q > p)
        .unwrap_or(Prec::ATOM)
}

/// Bounds parser recursion and the depth of every returned tree. The depth counts nested
/// `statement` and `binary` calls, plus each operator a loop wraps around the tree so far
/// (binary operators, postfix operators, `->` and juxtaposed scales), while those wrappers are
/// held. A left chain of operators therefore counts one level per operator, and so does the
/// tree it builds. Parentheses and similar wrappers use two levels each (a `statement` plus the
/// `binary` under it), so about 50 can nest.
const MAX_DEPTH: usize = 100;

struct Parser<'a> {
    toks: Vec<Token<'a>>,
    i: usize,
    ctx: &'a dyn IdContext,
    /// Current nesting, at most [`MAX_DEPTH`]: see the comment on that constant.
    depth: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Token<'a> {
        self.toks.get(self.i).cloned().unwrap_or(Token {
            tok: Tok::Eof,
            start: 0,
            end: 0,
        })
    }

    fn next_tok(&self, offset: usize) -> Option<Token<'a>> {
        self.toks.get(self.i + offset).cloned()
    }

    fn bump(&mut self) {
        // The last token is always `Eof`, which is never consumed.
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
    }

    fn at(&self, p: &str) -> bool {
        matches!(self.peek().tok, Tok::Punct(q) if q == p)
    }

    fn eat(&mut self, p: &str) -> bool {
        let found = self.at(p);
        if found {
            self.bump();
        }
        found
    }

    fn expect(&mut self, p: &str) -> PResult<()> {
        if self.eat(p) {
            Ok(())
        } else {
            err(
                self.peek().start,
                format!("expected '{p}', found {}", self.describe()),
            )
        }
    }

    fn describe(&self) -> String {
        match self.peek().tok {
            Tok::Num(t) => format!("number '{t}'"),
            Tok::Ident(t) => format!("identifier '{t}'"),
            Tok::Str(t) => format!("string \"{t}\""),
            Tok::Punct(p) => format!("'{p}'"),
            Tok::Eof => "end of input".to_string(),
        }
    }

    /// True when the current identifier is immediately followed by `(`.
    fn is_called(&self) -> bool {
        let cur = self.peek();
        matches!(
            self.next_tok(1),
            Some(Token { tok: Tok::Punct("("), start, .. }) if start == cur.end
        )
    }

    /// True when the current token is `=` and an expression starts right after it.
    fn eq_word_follows(&self) -> bool {
        matches!(self.peek().tok, Tok::Punct("="))
            && matches!(
                self.next_tok(1),
                Some(Token {
                    tok: Tok::Num(_)
                        | Tok::Ident(_)
                        | Tok::Str(_)
                        | Tok::Punct("(" | "[" | "|" | "‖" | "-" | "¬" | "■"),
                    ..
                })
            )
    }

    fn starts_expr(&self) -> bool {
        matches!(
            self.peek().tok,
            Tok::Num(_)
                | Tok::Ident(_)
                | Tok::Str(_)
                | Tok::Punct("(" | "[" | "|" | "‖" | "-" | "¬" | "■")
        )
    }

    /// Increments the nesting depth, or fails when it would exceed [`MAX_DEPTH`]. Each caller
    /// that succeeds must decrement `depth` on every exit path.
    fn enter(&mut self) -> PResult<()> {
        if self.depth >= MAX_DEPTH {
            return err(self.peek().start, "expression nested too deeply");
        }
        self.depth += 1;
        Ok(())
    }

    /// Top level and inside parentheses: definitions, evaluations, and (if `step_ok`) range steps.
    fn statement(&mut self, step_ok: bool) -> PResult<Expr> {
        self.enter()?;
        let result = self.statement_body(step_ok);
        self.depth -= 1;
        result
    }

    fn statement_body(&mut self, step_ok: bool) -> PResult<Expr> {
        let start = self.peek().start;
        let lhs = self.rel(step_ok)?;
        if self.eat(":=") {
            reject_operator_definition(&lhs, start)?;
            let expr = self.rel(step_ok)?;
            return Ok(Expr::Define {
                target: Box::new(lhs),
                expr: Box::new(expr),
            });
        }
        if self.eat("≡") {
            reject_operator_definition(&lhs, start)?;
            let expr = self.rel(step_ok)?;
            return Ok(Expr::GlobalDefine {
                target: Box::new(lhs),
                expr: Box::new(expr),
            });
        }
        let eq = self.peek().start;
        if self.eat("=") {
            return match lhs {
                Expr::SymEval {
                    expr,
                    command: Some(Command(mut items)),
                    ..
                } if items.len() == 1 => Ok(Expr::Eval {
                    expr,
                    unit_override: items.pop().map(Box::new),
                }),
                Expr::SymEval { .. } => err(eq, "'=' after '->' needs exactly one unit"),
                other => Ok(Expr::Eval {
                    expr: Box::new(other),
                    unit_override: None,
                }),
            };
        }
        Ok(lhs)
    }

    /// A range, followed by any number of symbolic evaluations `-> command`.
    fn rel(&mut self, step_ok: bool) -> PResult<Expr> {
        let e = self.range(step_ok)?;
        let mut applied = 0;
        let result = self.rel_chain(e, &mut applied);
        self.depth -= applied;
        result
    }

    /// The `->` loop of `rel`. Each evaluation wraps the tree so far, so it is held in `applied`.
    fn rel_chain(&mut self, mut e: Expr, applied: &mut usize) -> PResult<Expr> {
        while self.eat("->") {
            let items = self.command_items()?;
            self.hold(applied)?;
            e = Expr::SymEval {
                expr: Box::new(e),
                command: command_of(items),
                result: None,
            };
        }
        Ok(e)
    }

    /// Comma-separated items after `->`. Empty when nothing can start an expression.
    fn command_items(&mut self) -> PResult<Vec<Expr>> {
        let mut items = Vec::new();
        if !self.starts_expr() {
            return Ok(items);
        }
        loop {
            let item = self.range(false)?;
            // A command word may be an assignment-like equation (`substitute, x = 13`): the
            // Boolean equal sign, as the importer builds it.
            // Only after the keyword and when an expression follows: a unit override (`expr -> m=`,
            // `expr -> m =`) keeps its `=` for the statement.
            let item = if !items.is_empty() && self.eq_word_follows() {
                self.eat("=");
                Expr::binary(BinaryOp::Eq, item, self.range(false)?)
            } else {
                item
            };
            items.push(item);
            if !self.eat(",") {
                return Ok(items);
            }
        }
    }

    fn range(&mut self, step_ok: bool) -> PResult<Expr> {
        let from = self.binary(Prec::OR)?;
        if step_ok && self.eat(",") {
            let step = self.binary(Prec::ADD)?;
            self.expect("..")?;
            let to = self.binary(Prec::ADD)?;
            return Ok(Expr::range(from, Some(step), to));
        }
        if self.eat("..") {
            let to = self.binary(Prec::ADD)?;
            return Ok(Expr::range(from, None, to));
        }
        Ok(from)
    }

    /// Precedence climbing over the printer's binary operators.
    fn binary(&mut self, min: Prec) -> PResult<Expr> {
        self.enter()?;
        let result = self.binary_body(min);
        self.depth -= 1;
        result
    }

    fn binary_body(&mut self, min: Prec) -> PResult<Expr> {
        let lhs = self.unary()?;
        let mut applied = 0;
        let result = self.binary_chain(lhs, min, &mut applied);
        self.depth -= applied;
        result
    }

    /// The operator loop of `binary_body`. Each operator applied is held in `applied` until the
    /// caller releases it, so the depth covers the tree this loop builds.
    fn binary_chain(&mut self, mut lhs: Expr, min: Prec, applied: &mut usize) -> PResult<Expr> {
        while let Some((op, prec)) = self.binop() {
            if prec < min {
                break;
            }
            self.bump();
            let rhs_min = if op == BinaryOp::Pow {
                prec
            } else {
                above(prec)
            };
            let rhs = self.binary(rhs_min)?;
            self.hold(applied)?;
            lhs = make_binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    /// Counts one operator a loop is about to wrap around the tree. It is released by the loop's
    /// caller on every exit path, so `applied` must be subtracted from `depth` there.
    fn hold(&mut self, applied: &mut usize) -> PResult<()> {
        self.enter()?;
        *applied += 1;
        Ok(())
    }

    fn binop(&self) -> Option<(BinaryOp, Prec)> {
        let Tok::Punct(p) = self.peek().tok else {
            return None;
        };
        let op = match p {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            "*" => BinaryOp::Mul,
            "/" => BinaryOp::Div,
            "×" => BinaryOp::Cross,
            "^" => BinaryOp::Pow,
            "==" => BinaryOp::Eq,
            "<>" => BinaryOp::Ne,
            "<" => BinaryOp::Lt,
            ">" => BinaryOp::Gt,
            "<=" => BinaryOp::Le,
            ">=" => BinaryOp::Ge,
            "∧" => BinaryOp::And,
            "∨" => BinaryOp::Or,
            "⊕" => BinaryOp::Xor,
            _ => return None,
        };
        Some((op, op.prec()))
    }

    /// Prefix operators. `-` takes a power-level operand, `¬` a level-NOT operand.
    fn unary(&mut self) -> PResult<Expr> {
        if self.eat("-") {
            let operand = self.binary(Prec::POW)?;
            return Ok(negate(operand));
        }
        if self.eat("¬") {
            let operand = self.binary(Prec::NOT)?;
            return Ok(Expr::unary(UnaryOp::Not, operand));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let atom = self.atom()?;
        let mut applied = 0;
        let result = self.postfix_chain(atom, &mut applied);
        self.depth -= applied;
        result
    }

    /// The postfix loop and the juxtaposition after it. Each postfix operator applied is held in
    /// `applied`, which the caller releases on every exit path.
    fn postfix_chain(&mut self, mut e: Expr, applied: &mut usize) -> PResult<Expr> {
        let mut after_factorial = false;
        loop {
            let was_factorial = std::mem::take(&mut after_factorial);
            if self.at("!") {
                if was_factorial {
                    return err(
                        self.peek().start,
                        "there is no double factorial operator `!!`; use fact2(n) or write (n!)!",
                    );
                }
                self.bump();
                self.hold(applied)?;
                e = Expr::unary(UnaryOp::Factorial, e);
                after_factorial = true;
            } else if self.eat("%") {
                self.hold(applied)?;
                e = Expr::unary(UnaryOp::Percent, e);
            } else if self.at("^")
                && matches!(
                    self.next_tok(1),
                    Some(Token {
                        tok: Tok::Ident(ref name),
                        ..
                    }) if name == "T"
                )
            {
                self.bump();
                self.bump();
                self.hold(applied)?;
                e = Expr::unary(UnaryOp::Transpose, e);
            } else if self.bracket_follows_directly() {
                let indices = self.index_list()?;
                self.hold(applied)?;
                e = Expr::index(e, indices);
            } else {
                break;
            }
        }
        self.juxtapose(e)
    }

    /// Implicit product after an operand: `6 m` is a `Scale` when `m` is a unit, and `6 x` is a
    /// `Mul` when `x` is not. Any other juxtaposition is an error.
    fn juxtapose(&mut self, base: Expr) -> PResult<Expr> {
        if self.at("(") {
            return self.paren_unit_after(base);
        }
        let Tok::Ident(name) = self.peek().tok else {
            return Ok(base);
        };
        let start = self.peek().start;
        let called = self.is_called();
        if !called && self.ctx.is_scale_unit(&name) {
            let unit = self.binary(Prec::POW)?;
            return Ok(Expr::Scale {
                value: Box::new(base),
                unit: Box::new(unit),
            });
        }
        if matches!(base, Expr::Number(_)) {
            let rhs = self.binary(Prec::POW)?;
            return Ok(Expr::binary(BinaryOp::Mul, base, rhs));
        }
        err(
            start,
            format!("unexpected identifier '{name}', expected an operator"),
        )
    }

    /// `value (unit)`: a parenthesized group after a value is the unit of a `Scale` in three
    /// cases, and otherwise is left unconsumed, so the caller reports `unexpected '('`:
    /// - after a closing `)` or `]` (see [`Self::ends_closed`]), any group: `(a) (b)`,
    ///   `(1/(x+2)) (2/(x+3))`, `v[1] (m)`;
    /// - after a number, any group: `20 (m/sec)`, `7 (x)`, `4 (π)`, the implicit products the
    ///   printer writes as `Scale`. `2 ()` and `2 (2)` are still errors (empty and a number unit);
    /// - after any other operand, only a matrix literal `([...])`, as the printer writes
    ///   `text ([1, 2])`. A name followed by a plain group may be a call, so `x (y)` stays an error.
    fn paren_unit_after(&mut self, value: Expr) -> PResult<Expr> {
        let start = self.peek().start;
        let accepted = self.ends_closed()
            || (matches!(value, Expr::Number(_)) && !self.number_is_exponent())
            || self.matrix_group_follows();
        if !accepted {
            return Ok(value);
        }
        let unit = strip_parens(self.binary(Prec::POW)?);
        if matches!(unit, Expr::Number(_)) {
            return err(
                start,
                "a number is not a unit; expected an expression in parentheses",
            );
        }
        Ok(Expr::Scale {
            value: Box::new(value),
            unit: Box::new(unit),
        })
    }

    /// True when the token before the current one closes a group or index, so the operand just
    /// parsed ends with `)` or `]`: a parenthesized value, a call, or an index. Only such an
    /// operand takes any parenthesized group as its unit. Other operands (names, numbers,
    /// `(a+b)^2`) take only the forms in [`Self::paren_unit_after`].
    /// True when the number just parsed is the exponent of a `^` (`x^2 (c)`), so the group after
    /// it is not a `Scale` of that number. The printer parenthesizes a `Scale` in an exponent.
    fn number_is_exponent(&self) -> bool {
        self.i >= 2 && self.punct_at(self.i - 2, "^")
    }

    fn ends_closed(&self) -> bool {
        self.i > 0 && matches!(self.toks[self.i - 1].tok, Tok::Punct(")" | "]"))
    }

    /// True when `(` is followed by `[` and the `(` closes right after that matrix's `]`, so the
    /// group is a matrix literal with any elements: `([sqrt(a^2)])`. Scans tokens only.
    fn matrix_group_follows(&self) -> bool {
        if !self.punct_at(self.i, "(") || !self.punct_at(self.i + 1, "[") {
            return false;
        }
        let mut depth = 0usize;
        for j in self.i..self.toks.len() {
            match self.toks[j].tok {
                Tok::Punct("(" | "[") => depth += 1,
                Tok::Punct(")" | "]") => {
                    depth -= 1;
                    if depth == 0 {
                        return self.punct_at(j, ")") && self.punct_at(j - 1, "]");
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn punct_at(&self, j: usize, p: &str) -> bool {
        matches!(self.toks.get(j), Some(Token { tok: Tok::Punct(q), .. }) if *q == p)
    }

    fn atom(&mut self) -> PResult<Expr> {
        let t = self.peek();
        match t.tok {
            Tok::Num(text) => {
                self.bump();
                Ok(number(text))
            }
            Tok::Ident(name) => self.ident(name),
            Tok::Str(text) => {
                self.bump();
                Ok(Expr::Str(text.to_string()))
            }
            Tok::Punct("(") => self.paren(),
            Tok::Punct("[") => self.matrix(),
            Tok::Punct("|") => self.abs(),
            Tok::Punct("‖") => self.norm(),
            Tok::Punct("■") => {
                self.bump();
                Ok(Expr::Placeholder)
            }
            _ => err(
                t.start,
                format!("expected an expression, found {}", self.describe()),
            ),
        }
    }

    fn ident(&mut self, name: Cow<'a, str>) -> PResult<Expr> {
        let start = self.peek().start;
        let called = self.is_called();
        self.bump();
        if called {
            self.bump();
            let args = self.args()?;
            return call(&name, args, start, self.ctx);
        }
        let subscript = if self.eat("_") {
            Some(Box::new(self.name_subscript()?))
        } else {
            None
        };
        let kind = if subscript.is_some() {
            self.ctx.classify_subscripted(&name)
        } else {
            self.ctx.classify(&name, false)
        };
        // A subscripted function name, `g_1(x)`, is a Call whose function keeps the subscript.
        if subscript.is_some() && self.paren_follows_directly() {
            self.bump();
            let args = self.args()?;
            let func = Id {
                kind: self.ctx.classify(&name, true),
                name: name.into_owned(),
                subscript,
            };
            return Ok(Expr::Call { func, args });
        }
        Ok(Expr::Id(Id {
            name: name.into_owned(),
            kind,
            subscript,
        }))
    }

    /// True when the current token is `(` with no whitespace before it.
    fn paren_follows_directly(&self) -> bool {
        let cur = self.peek();
        self.i > 0 && cur.tok == Tok::Punct("(") && self.toks[self.i - 1].end == cur.start
    }

    fn args(&mut self) -> PResult<Vec<Expr>> {
        let mut args = Vec::new();
        if self.eat(")") {
            return Ok(args);
        }
        loop {
            args.push(self.statement(false)?);
            if self.eat(")") {
                return Ok(args);
            }
            self.expect(",")?;
        }
    }

    /// The literal subscript of a name after `_`: a number, a name, or one item in braces.
    /// Braces with a comma are an error, because a name takes one subscript.
    fn name_subscript(&mut self) -> PResult<Expr> {
        let t = self.peek();
        match t.tok {
            Tok::Punct("{") => {
                self.bump();
                let item = self.binary(Prec::OR)?;
                if self.at(",") {
                    return err(
                        self.peek().start,
                        "a name takes one subscript; write several indices as `M[i,j]`",
                    );
                }
                self.expect("}")?;
                Ok(item)
            }
            Tok::Num(text) => {
                self.bump();
                Ok(number(text))
            }
            Tok::Ident(name) => {
                self.bump();
                let kind = self.ctx.classify(&name, false);
                Ok(Expr::Id(Id::new(&name, kind)))
            }
            _ => err(
                t.start,
                format!("expected a subscript, found {}", self.describe()),
            ),
        }
    }

    /// `[i]` or `[i,j]`: the indices of an `Index`. Each is parsed like a braced subscript.
    fn index_list(&mut self) -> PResult<Vec<Expr>> {
        self.expect("[")?;
        let mut items = Vec::new();
        loop {
            items.push(self.binary(Prec::OR)?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect("]")?;
        Ok(items)
    }

    /// True when the next token is `[` and no whitespace separates it from the token before.
    /// Only then is it an index; see the module docs for the matrix reading.
    fn bracket_follows_directly(&self) -> bool {
        let cur = self.peek();
        self.i > 0 && cur.tok == Tok::Punct("[") && self.toks[self.i - 1].end == cur.start
    }

    /// `( ... )`. Atoms keep their parentheses as `Parens`; operator forms do not, because the
    /// printer adds those parentheses itself. A subscripted name before an index is not wrapped,
    /// so `(v_0)[1]` reads the same as `v_0[1]`.
    fn paren(&mut self) -> PResult<Expr> {
        self.bump();
        let inner = self.statement(true)?;
        self.expect(")")?;
        let bare = inner.prec() < Prec::ATOM
            || (matches!(
                inner,
                Expr::Id(Id {
                    subscript: Some(_),
                    ..
                })
            ) && self.bracket_follows_directly());
        Ok(if bare {
            inner
        } else {
            Expr::Parens(Box::new(inner))
        })
    }

    fn matrix(&mut self) -> PResult<Expr> {
        let open = self.peek().start;
        self.bump();
        if self.eat("]") {
            return Ok(Expr::Matrix {
                rows: 0,
                cols: 0,
                elements: Vec::new(),
            });
        }
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        loop {
            let mut row = Vec::new();
            loop {
                row.push(self.statement(false)?);
                if !self.eat(",") {
                    break;
                }
            }
            rows.push(row);
            if !self.eat(";") {
                break;
            }
        }
        self.expect("]")?;
        let cols = rows.first().map_or(0, Vec::len);
        if rows.iter().any(|r| r.len() != cols) {
            return err(open, "matrix rows must all have the same length");
        }
        let nrows = rows.len();
        Ok(Expr::Matrix {
            rows: nrows,
            cols,
            elements: rows.into_iter().flatten().collect(),
        })
    }

    fn abs(&mut self) -> PResult<Expr> {
        self.bump();
        let inner = self.statement(false)?;
        self.expect("|")?;
        Ok(Expr::unary(UnaryOp::Abs, inner))
    }

    fn norm(&mut self) -> PResult<Expr> {
        self.bump();
        let inner = self.statement(false)?;
        self.expect("‖")?;
        Ok(Expr::unary(UnaryOp::Norm, inner))
    }
}

fn number(text: &str) -> Expr {
    Expr::Number(Number::Real {
        value: text.parse().unwrap_or(f64::NAN),
        text: text.to_string(),
    })
}

/// `-` before a bare literal is part of the literal, so `-2` reads as `Number(-2)`.
/// `-2^2` keeps its power, since the operand is parsed at power level. A literal that already
/// starts with `-` (as in `-(-2)`) is not folded again, so its text stays a single sign.
fn negate(e: Expr) -> Expr {
    match e {
        Expr::Number(Number::Real { value, text }) if !text.starts_with('-') => {
            Expr::Number(Number::Real {
                value: -value,
                text: format!("-{text}"),
            })
        }
        // `-5 °C` is the number -5 in degC, not the negation of 5 °C (the offset applies to the
        // number, so a sign applied afterwards would give -278.15 K instead of 268.15 K).
        Expr::Scale { value, unit } => Expr::Scale {
            value: Box::new(negate(*value)),
            unit,
        },
        other => Expr::unary(UnaryOp::Neg, other),
    }
}

fn command_of(items: Vec<Expr>) -> Option<Command> {
    if items.is_empty() || items == [Expr::Placeholder] {
        None
    } else {
        Some(Command(items))
    }
}

/// A definition whose left side is a call of a calculus operator (`sum(a) := 2*a`) is refused: the
/// name is reserved for the operator, and a one-argument `sum(a)` is always the vector form.
fn reject_operator_definition(lhs: &Expr, pos: usize) -> PResult<()> {
    let name = match lhs {
        Expr::Fold { op, .. } | Expr::Accumulate { op, .. } => match op {
            Accumulation::Sum => "sum",
            Accumulation::Product => "product",
        },
        Expr::Integral { .. } => "integral",
        Expr::Derivative { partial: false, .. } => "derivative",
        Expr::Derivative { partial: true, .. } => "partial",
        Expr::Gradient { .. } => "gradient",
        Expr::Limit { .. } => "limit",
        _ => return Ok(()),
    };
    err(
        pos,
        format!("'{name}' is reserved for the {name} operator and cannot be defined as a function"),
    )
}

/// `reserved` names `sqrt` and `root` are never plain calls.
fn call(name: &str, args: Vec<Expr>, pos: usize, ctx: &dyn IdContext) -> PResult<Expr> {
    match name {
        "sqrt" => match <[Expr; 1]>::try_from(args) {
            Ok([radicand]) => Ok(Expr::sqrt(radicand)),
            Err(_) => err(pos, "sqrt takes one argument"),
        },
        "root" => match <[Expr; 2]>::try_from(args) {
            // `root(f(x), x)`, with a plain name as the second argument that the first one uses, is
            // the root-finding form with the guess `x` (see `root.rs`). Any other two arguments are
            // the nth root, so `root(27, n)` with a variable `n` stays a degree.
            Ok([radicand, degree]) => {
                let guess_form = matches!(
                    &degree,
                    Expr::Id(id) if id.subscript.is_none()
                        && crate::eval::uses_variable(&radicand, &id.name)
                );
                if guess_form {
                    Ok(Expr::Call {
                        func: Id::new(name, ctx.classify(name, true)),
                        args: vec![radicand, degree],
                    })
                } else {
                    Ok(Expr::nth_root(degree, radicand))
                }
            }
            // `root(f(x), x, [a, b])` is the root-finding function (see `special_forms`).
            Err(args) if args.len() == 3 => Ok(Expr::Call {
                func: Id::new(name, ctx.classify(name, true)),
                args,
            }),
            Err(_) => err(pos, "root takes two arguments, or three for root finding"),
        },
        // Calculus operators (see `Expr::Accumulate` and friends). Their names are reserved, so
        // the forms below are the only reading of these names.
        "sum" | "product" => {
            let op = if name == "sum" {
                Accumulation::Sum
            } else {
                Accumulation::Product
            };
            match <[Expr; 1]>::try_from(args) {
                Ok([operand]) => Ok(Expr::Fold {
                    op,
                    operand: Box::new(operand),
                }),
                Err(args) => match <[Expr; 4]>::try_from(args) {
                    Ok([var, from, to, body]) => Ok(Expr::Accumulate {
                        op,
                        var: Box::new(bound_variable(var, pos)?),
                        from: Box::new(from),
                        to: Box::new(to),
                        body: Box::new(body),
                    }),
                    Err(_) => err(pos, format!("{name} takes one argument or four")),
                },
            }
        }
        "integral" => match <[Expr; 4]>::try_from(args) {
            Ok([var, lower, upper, body]) => Ok(Expr::Integral {
                var: Box::new(bound_variable(var, pos)?),
                lower: Box::new(lower),
                upper: Box::new(upper),
                body: Box::new(body),
            }),
            Err(_) => err(
                pos,
                "integral takes four arguments: variable, lower, upper, integrand",
            ),
        },
        "limit" => match <[Expr; 3]>::try_from(args) {
            Ok([var, target, body]) => Ok(Expr::Limit {
                var: Box::new(bound_variable(var, pos)?),
                target: Box::new(target),
                body: Box::new(body),
            }),
            Err(_) => err(
                pos,
                "limit takes three arguments: variable, target, expression",
            ),
        },
        "derivative" | "partial" => {
            let partial = name == "partial";
            match <[Expr; 2]>::try_from(args) {
                Ok([var, body]) => Ok(Expr::Derivative {
                    var: Box::new(bound_variable(var, pos)?),
                    order: None,
                    body: Box::new(body),
                    partial,
                }),
                Err(args) => match <[Expr; 3]>::try_from(args) {
                    Ok([var, order, body]) => Ok(Expr::Derivative {
                        var: Box::new(bound_variable(var, pos)?),
                        order: Some(Box::new(order)),
                        body: Box::new(body),
                        partial,
                    }),
                    Err(_) => err(
                        pos,
                        format!("{name} takes two arguments or three: variable, order, expression"),
                    ),
                },
            }
        }
        "gradient" => {
            let mut args = args.into_iter();
            let Some(body) = args.next() else {
                return err(pos, "gradient takes an expression and optional variables");
            };
            let vars = args
                .map(|a| bound_variable(a, pos))
                .collect::<PResult<Vec<Id>>>()?;
            Ok(Expr::Gradient {
                vars,
                body: Box::new(body),
            })
        }
        _ => Ok(Expr::Call {
            func: Id::new(name, ctx.classify(name, true)),
            args,
        }),
    }
}

/// The variable of a calculus operator: a plain name, not an indexed or subscripted one.
fn bound_variable(e: Expr, pos: usize) -> PResult<Id> {
    match e {
        Expr::Id(id) if id.subscript.is_none() => Ok(id),
        _ => err(
            pos,
            "the variable of a calculus operator must be a plain name",
        ),
    }
}

/// `p/q` is a `Div`, as the importer stores every typed fraction. The printer writes a `Div` and
/// a `Rational` the same way, so the parser cannot tell them apart; it returns the `Div`, which is
/// the stored form.
fn make_binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::binary(op, lhs, rhs)
}

// ---------------------------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{BinaryOp as B, Id, IdKind, Number, UnaryOp as U};

    /// Units `m`, `s`, `°`; constant `pi`; everything else a variable (or a function when called).
    struct Units;

    impl IdContext for Units {
        fn classify(&self, name: &str, called: bool) -> IdKind {
            if called {
                return IdKind::Function;
            }
            match name {
                "m" | "s" | "°" => IdKind::Unit,
                "pi" => IdKind::Constant,
                _ => IdKind::Variable,
            }
        }
    }

    /// Units with the longer name `sec` and several SI names, for the unit-group tests.
    struct Si;

    impl IdContext for Si {
        fn classify(&self, name: &str, called: bool) -> IdKind {
            if called {
                return IdKind::Function;
            }
            match name {
                "m" | "s" | "sec" | "kg" => IdKind::Unit,
                _ => IdKind::Variable,
            }
        }
    }

    fn p(src: &str) -> Expr {
        parse(src, &Units).unwrap_or_else(|e| panic!("{src:?}: {e}"))
    }

    fn perr(src: &str) -> ParseError {
        match parse(src, &Units) {
            Ok(e) => panic!("{src:?} parsed as {e:?}"),
            Err(e) => e,
        }
    }

    fn round_trip(e: Expr) {
        let text = e.to_string();
        let back = parse(&text, &Units).unwrap_or_else(|err| panic!("{text:?}: {err}"));
        assert_eq!(back, e, "text: {text:?}");
    }

    fn n(x: f64) -> Expr {
        Expr::num(x)
    }
    fn v(name: &str) -> Expr {
        Expr::var(name)
    }
    fn unit(name: &str) -> Expr {
        Expr::Id(Id::new(name, IdKind::Unit))
    }
    fn b(op: B, l: Expr, r: Expr) -> Expr {
        Expr::binary(op, l, r)
    }
    fn u(op: U, e: Expr) -> Expr {
        Expr::unary(op, e)
    }
    fn cmd(words: &[&str]) -> Command {
        Command(words.iter().map(|w| v(w)).collect())
    }
    fn sym(e: Expr, c: Option<Command>) -> Expr {
        Expr::SymEval {
            expr: Box::new(e),
            command: c,
            result: None,
        }
    }
    fn eval(e: Expr, unit_override: Option<Expr>) -> Expr {
        Expr::Eval {
            expr: Box::new(e),
            unit_override: unit_override.map(Box::new),
        }
    }
    fn assign(t: Expr, e: Expr) -> Expr {
        Expr::Define {
            target: Box::new(t),
            expr: Box::new(e),
        }
    }
    fn range(a: Expr, step: Option<Expr>, c: Expr) -> Expr {
        Expr::range(a, step, c)
    }
    fn scale(value: Expr, unit: Expr) -> Expr {
        Expr::Scale {
            value: Box::new(value),
            unit: Box::new(unit),
        }
    }
    fn call_of(name: &str, args: Vec<Expr>) -> Expr {
        Expr::call(name, args)
    }

    macro_rules! round_trips {
        ($($name:ident => $expr:expr;)*) => {
            $( #[test] fn $name() { round_trip($expr); } )*
        };
    }

    round_trips! {
        rt_integer => n(3.0);
        rt_decimal_keeps_text => Expr::Number(Number::Real { value: 2.5, text: "2.50".into() });
        rt_fraction => b(B::Div, n(1.0), n(3.0));
        rt_fraction_power_base => b(B::Pow, b(B::Div, n(1.0), n(3.0)), n(2.0));
        rt_negative_power_base => b(B::Pow, n(-1.0), n(2.0));
        rt_square => b(B::Pow, v("x"), n(2.0));
        rt_power_right_assoc => b(B::Pow, v("x"), b(B::Pow, v("y"), n(2.0)));
        rt_power_nested_left => b(B::Pow, b(B::Pow, v("x"), n(2.0)), n(3.0));
        rt_power_of_half => b(B::Pow, v("x"), b(B::Div, n(1.0), n(2.0)));
        rt_product_with_sum_right => b(B::Mul, v("a"), b(B::Add, v("b"), v("c")));
        rt_product_with_sum_left => b(B::Mul, b(B::Add, v("a"), v("b")), v("c"));
        rt_addition_left => b(B::Add, b(B::Add, v("a"), v("b")), v("c"));
        rt_addition_right => b(B::Add, v("a"), b(B::Add, v("b"), v("c")));
        rt_subtraction_chain => b(B::Sub, b(B::Sub, v("a"), v("b")), v("c"));
        rt_subtraction_right_nested => b(B::Sub, v("a"), b(B::Sub, v("b"), v("c")));
        rt_division_by_product => b(B::Div, v("a"), b(B::Mul, v("b"), v("c")));
        rt_product_with_quotient => b(B::Mul, v("a"), b(B::Div, v("b"), v("c")));
        rt_quotient_left_nested => b(B::Div, b(B::Div, v("a"), v("b")), v("c"));
        rt_division_by_quotient => b(B::Div, v("a"), b(B::Div, v("b"), v("c")));
        rt_negation_below_power => u(U::Neg, b(B::Pow, v("x"), n(2.0)));
        rt_negated_power_base => b(B::Pow, u(U::Neg, v("x")), n(2.0));
        rt_double_negation => u(U::Neg, u(U::Neg, v("x")));
        rt_negated_sum => u(U::Neg, b(B::Add, v("a"), v("b")));
        rt_subtract_negation => b(B::Sub, v("a"), u(U::Neg, v("b")));
        rt_add_negation => b(B::Add, v("a"), u(U::Neg, v("b")));
        rt_multiply_negation => b(B::Mul, v("a"), u(U::Neg, v("b")));
        rt_divide_negation => b(B::Div, v("a"), u(U::Neg, v("b")));
        rt_multiply_negative_number => b(B::Mul, v("a"), n(-2.0));
        rt_factorial_of_name => u(U::Factorial, v("x"));
        rt_factorial_of_power => u(U::Factorial, b(B::Pow, v("x"), n(2.0)));
        rt_power_of_factorial => b(B::Pow, u(U::Factorial, v("x")), n(2.0));
        rt_power_of_transpose => b(B::Pow, u(U::Transpose, v("v")), n(2.0));
        rt_transpose_of_transpose => u(U::Transpose, u(U::Transpose, v("v")));
        rt_transpose_of_factorial => u(U::Transpose, u(U::Factorial, v("x")));
        rt_factorial_of_transpose => u(U::Factorial, u(U::Transpose, v("v")));
        rt_percent => u(U::Percent, n(5.0));
        rt_transpose => u(U::Transpose, v("v"));
        rt_index_of_subscripted_base => Expr::index(Expr::subscripted("v", n(0.0)), vec![n(1.0)]);
        rt_index_of_transpose_base => Expr::index(u(U::Transpose, v("v")), vec![n(1.0)]);
        rt_index_of_factorial_base => Expr::index(u(U::Factorial, v("x")), vec![n(1.0)]);
        rt_index_of_percent_base => Expr::index(u(U::Percent, v("x")), vec![n(1.0)]);
        rt_index_of_compound_base => Expr::index(b(B::Add, v("A"), v("B")), vec![n(1.0)]);
        rt_index_of_call => Expr::index(call_of("f", vec![v("x")]), vec![n(1.0)]);
        rt_index_of_add => Expr::index(b(B::Add, v("a"), v("b")), vec![n(1.0)]);
        rt_index_of_number => Expr::index(n(2.0), vec![n(1.0)]);
        rt_index_of_negative_number => Expr::index(n(-2.0), vec![n(1.0)]);
        rt_index_of_fraction => Expr::index(b(B::Div, n(1.0), n(3.0)), vec![n(1.0)]);
        rt_index_of_matrix => Expr::index(Expr::matrix(1, 2, vec![n(1.0), n(2.0)]), vec![n(1.0)]);
        rt_index_of_name_subscript => Expr::index(Expr::subscripted("v", v("i")), vec![v("j")]);
        rt_index_nested => Expr::index(Expr::index(v("M"), vec![v("i")]), vec![v("j")]);
        rt_index_of_unit => Expr::index(Expr::Id(Id::new("m", IdKind::Unit)), vec![n(1.0)]);
        rt_index_of_percent_name => Expr::index(u(U::Percent, v("x")), vec![n(1.0)]);
        rt_index_of_sqrt => Expr::index(Expr::sqrt(v("x")), vec![n(1.0)]);
        rt_index_of_abs => Expr::index(u(U::Abs, v("x")), vec![n(1.0)]);
        rt_index_in_matrix => Expr::matrix(1, 2, vec![Expr::index(v("v"), vec![n(1.0)]), n(2.0)]);
        rt_index_with_matrix_index => Expr::index(v("v"), vec![Expr::matrix(1, 2, vec![n(1.0), n(2.0)])]);
        rt_index_in_power => b(B::Pow, Expr::index(v("v"), vec![n(1.0)]), n(2.0));
        rt_index_of_name_subscript_nested => Expr::index(Expr::subscripted("v", n(0.0)), vec![n(1.0), n(2.0)]);
        rt_subscript_power => b(B::Pow, Expr::subscripted("v", n(0.0)), n(2.0));
        rt_subscript_compound => Expr::subscripted("v", b(B::Add, v("i"), n(1.0)));
        rt_subscript_nested => Expr::subscripted("v", Expr::subscripted("w", n(0.0)));
        rt_index_multi => Expr::index(v("M"), vec![v("i"), v("j")]);
        rt_index_multi_expr => Expr::index(v("M"), vec![b(B::Add, v("i"), n(1.0)), v("j")]);
        rt_define_indexed_multi => assign(Expr::index(v("M"), vec![v("i"), v("j")]), n(0.0));
        rt_sym_eval_without_command => sym(b(B::Add, v("a"), v("b")), None);
        rt_sym_eval_with_command => sym(b(B::Pow, v("x"), n(2.0)), Some(cmd(&["simplify"])));
        rt_sym_eval_inside_define => assign(
            Expr::matrix(1, 2, vec![v("a"), v("b")]),
            sym(v("M"), Some(cmd(&["solve", "a", "b"]))),
        );
        rt_sym_eval_inside_sum => b(B::Add, sym(v("x"), None), n(1.0));
        rt_sym_eval_of_sym_eval => sym(sym(v("x"), None), Some(cmd(&["solve"])));
        rt_sym_eval_of_range => sym(range(n(1.0), None, n(3.0)), Some(cmd(&["solve"])));
        rt_eval_plain => eval(b(B::Add, v("a"), v("b")), None);
        rt_eval_unit => eval(v("x"), Some(unit("m")));
        rt_eval_unit_power => eval(v("x"), Some(b(B::Pow, unit("m"), n(2.0))));
        rt_eval_unit_of_sym_eval => eval(sym(v("x"), None), Some(unit("m")));
        rt_eval_unit_of_sym_eval_with_command => eval(sym(v("x"), Some(cmd(&["solve", "x"]))), Some(unit("m")));
        rt_eval_unit_of_range => eval(range(n(0.0), None, n(3.0)), Some(unit("m")));
        rt_eval_of_range => eval(range(n(0.0), None, n(3.0)), None);
        rt_scale_decimal => scale(n(0.75), unit("m"));
        rt_scale_degree_symbol => scale(n(45.0), unit("°"));
        rt_scale_power_unit => scale(n(6.0), b(B::Pow, unit("m"), n(3.0)));
        rt_scale_fraction_value => scale(b(B::Div, n(1.0), n(2.0)), unit("m"));
        rt_scale_inside_product => b(B::Mul, v("a"), scale(n(6.0), unit("m")));
        rt_scale_divided_by_unit => b(B::Div, scale(n(6.0), unit("m")), unit("s"));
        rt_cross_product => b(B::Cross, v("u"), v("v"));
        rt_cross_in_sum => b(B::Add, b(B::Cross, v("u"), v("v")), v("w"));
        rt_cross_chain_is_left_associative => b(B::Cross, b(B::Cross, v("u"), v("v")), v("w"));
        rt_norm => u(U::Norm, v("x"));
        rt_norm_of_sum => u(U::Norm, b(B::Add, v("a"), v("b")));
        rt_norm_in_product => b(B::Mul, u(U::Norm, v("a")), u(U::Norm, v("b")));
        rt_partial_derivative => Expr::Derivative { var: Box::new(Id::new("x", IdKind::Variable)), order: None, body: Box::new(v("f")), partial: true };
        rt_nth_partial_derivative => Expr::Derivative { var: Box::new(Id::new("x", IdKind::Variable)), order: Some(Box::new(n(2.0))), body: Box::new(v("f")), partial: true };
        rt_gradient => Expr::Gradient { vars: vec![Id::new("a", IdKind::Variable), Id::new("b", IdKind::Variable)], body: Box::new(call_of("f", vec![v("a"), v("b")])) };
        rt_gradient_without_variables => Expr::Gradient { vars: vec![], body: Box::new(call_of("f", vec![v("a"), v("b")])) };
        rt_define_function => assign(call_of("g", vec![v("x"), v("P"), v("Q")]), b(B::Add, v("x"), v("P")));
        rt_call_args => call_of("g", vec![v("x"), v("A"), v("B")]);
        rt_call_two_args => call_of("atan2", vec![v("y"), v("x")]);
        rt_call_no_args => call_of("f", vec![]);
        rt_call_in_product => b(B::Mul, n(2.0), call_of("sin", vec![v("x")]));
        rt_sqrt => Expr::sqrt(b(B::Add, v("x"), n(1.0)));
        rt_nth_root => Expr::nth_root(n(3.0), v("x"));
        rt_matrix => Expr::matrix(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
        rt_matrix_expressions => Expr::matrix(2, 2, vec![b(B::Add, v("x"), n(1.0)), v("y"), n(2.0), b(B::Pow, v("z"), n(2.0))]);
        rt_matrix_empty => Expr::matrix(0, 0, vec![]);
        rt_range_default_step => range(n(0.0), None, b(B::Sub, v("n"), n(1.0)));
        rt_range_explicit_step => range(n(0.0), Some(n(0.5)), n(3.0));
        rt_range_inside_sum => b(B::Add, range(n(0.0), None, n(3.0)), n(1.0));
        rt_define => assign(v("a"), v("b"));
        rt_global_define => Expr::GlobalDefine { target: Box::new(v("a")), expr: Box::new(v("b")) };
        rt_define_range => assign(v("w"), range(n(0.0), None, n(3.0)));
        rt_define_matrix_target => assign(Expr::matrix(1, 2, vec![v("a"), v("b")]), v("M"));
        rt_not => u(U::Not, b(B::Lt, v("x"), v("y")));
        rt_not_not => u(U::Not, u(U::Not, v("x")));
        rt_boolean_precedence => b(B::Or, b(B::And, v("a"), v("b")), v("c"));
        rt_boolean_parens => b(B::And, b(B::Or, v("a"), v("b")), v("c"));
        rt_xor => b(B::Xor, v("a"), v("b"));
        rt_comparison_eq => b(B::Eq, v("a"), v("b"));
        rt_comparison_ne => b(B::Ne, v("a"), v("b"));
        rt_comparison_le => b(B::Le, v("a"), v("b"));
        rt_comparison_chain => b(B::Lt, b(B::Lt, v("a"), v("b")), v("c"));
        rt_absolute_value => u(U::Abs, b(B::Add, v("a"), v("b")));
        rt_absolute_of_negation => u(U::Abs, u(U::Neg, v("x")));
        rt_explicit_parens => Expr::Parens(Box::new(v("x")));
        rt_explicit_parens_subscripted => Expr::Parens(Box::new(Expr::subscripted("v", n(0.0))));
        rt_explicit_parens_power => b(B::Pow, Expr::Parens(Box::new(v("x"))), n(2.0));
        rt_placeholder => Expr::Placeholder;
        rt_constant_in_product => b(B::Mul, n(2.0), Expr::Id(Id::new("pi", IdKind::Constant)));
        rt_exponent_negative => real(1e-5, "1e-5");
        rt_exponent_positive => real(2e3, "2e3");
        rt_exponent_capital_plus => real(1.5e10, "1.5E+10");
        rt_exponent_avogadro => real(6.02e23, "6.02e23");
        rt_negated_negative_literal => u(U::Neg, n(-2.0));
        rt_negative_fraction => b(B::Div, n(-1.0), n(3.0));
        rt_negative_fraction_power_base => b(B::Pow, b(B::Div, n(-1.0), n(3.0)), n(2.0));
        rt_sym_eval_with_range_command => sym(v("x"), Some(Command(vec![v("solve"), range(n(0.0), None, n(3.0))])));
    }

    fn real(value: f64, text: &str) -> Expr {
        Expr::Number(Number::Real {
            value,
            text: text.to_string(),
        })
    }

    /// Runs `test` on a thread with a 2 MB stack, the size of a default test thread's.
    fn on_small_stack(test: fn()) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(test)
            .unwrap()
            .join()
            .unwrap();
    }

    fn nested_too_deeply(src: &str) -> bool {
        matches!(parse(src, &Units), Err(e) if e.message == "expression nested too deeply")
    }

    /// Each of these is far over the token cap as well as the depth cap, so the error may come
    /// from either; the test only requires that parsing fails without overflowing the stack.
    #[test]
    fn deep_nesting_inputs_are_errors_not_stack_overflows() {
        on_small_stack(|| {
            assert!(parse(&format!("{}x", "-".repeat(10_000)), &Units).is_err());
            assert!(parse(&format!("{}x", "x^".repeat(50_000)), &Units).is_err());
            assert!(parse(&format!("{}x", "[".repeat(10_000)), &Units).is_err());
            assert!(parse(&format!("{}x", "sqrt(".repeat(10_000)), &Units).is_err());
            assert!(parse(&format!("{}x", "|".repeat(10_000)), &Units).is_err());
        });
    }

    /// Short enough to pass the token cap, so the depth cap itself is what rejects them.
    #[test]
    fn depth_cap_rejects_nesting_under_the_token_cap() {
        on_small_stack(|| {
            assert!(nested_too_deeply(&format!("{}x", "-".repeat(200))));
            assert!(nested_too_deeply(&format!("{}x", "x^".repeat(200))));
            assert!(nested_too_deeply(&format!("{}x", "[".repeat(200))));
            assert!(nested_too_deeply(&format!("{}x", "sqrt(".repeat(200))));
            assert!(nested_too_deeply(&format!("{}x", "|".repeat(200))));
        });
    }

    #[test]
    fn nesting_of_90_still_parses() {
        on_small_stack(|| {
            assert!(parse(&format!("{}x", "-".repeat(90)), &Units).is_ok());
            assert!(parse(&format!("{}x", "x^".repeat(90)), &Units).is_ok());
        });
    }

    /// Loops that wrap the tree once per iteration count each wrap against the depth cap, so
    /// these long chains are depth errors rather than trees that overflow the stack when printed.
    #[test]
    fn long_chains_that_wrap_the_tree_are_depth_errors() {
        on_small_stack(|| {
            let chains = [
                format!("x{}", "!%".repeat(100)),
                format!("x{}", "%".repeat(200)),
                format!("x{}", "^T".repeat(200)),
                format!("x{}", "+x".repeat(200)),
                format!("a{}", " ->".repeat(200)),
                format!("x{}", "[1]".repeat(200)),
                format!("2{}", " m".repeat(200)),
                format!("{}x", "-".repeat(200)),
            ];
            for src in &chains {
                assert!(nested_too_deeply(src), "{src:?}");
            }
        });
    }

    /// A chain of exactly 90 operators parses, prints and drops without overflowing. The printed
    /// `+` and `[1]` chains parse back to the same tree. The printer wraps every factorial level
    /// in parentheses, so 90 `!` prints as 90 nested parentheses, which the parser rejects; that
    /// one only checks parse, print and drop.
    #[test]
    fn chain_of_90_operators_parses_prints_and_round_trips() {
        on_small_stack(|| {
            for src in [
                format!("x{}", "+x".repeat(90)),
                format!("x{}", "[1]".repeat(90)),
            ] {
                let e = parse(&src, &Units).unwrap_or_else(|err| panic!("{src:?}: {err}"));
                let text = format!("{e}");
                let back = parse(&text, &Units).unwrap_or_else(|err| panic!("{text:?}: {err}"));
                assert_eq!(back, e, "text: {text:?}");
            }

            let e = parse(&format!("x{}", "!%".repeat(45)), &Units).unwrap();
            drop(format!("{e}"));
        });
    }

    /// Index chains of 150, 1000 and 4000 repetitions: `x[` opened and never closed, `x[1]`
    /// repeated, and `x[` nested and closed. Each is over the depth cap or the token cap, so each
    /// must fail as an error rather than overflow the stack.
    #[test]
    fn index_chains_of_150_1000_and_4000_are_errors() {
        on_small_stack(|| {
            for n in [150, 1000, 4000] {
                let unclosed = "x[".repeat(n);
                let flat = format!("x{}", "[1]".repeat(n));
                let nested = format!("{}1{}", "x[".repeat(n), "]".repeat(n));
                for src in [&unclosed, &flat, &nested] {
                    assert!(parse(src, &Units).is_err(), "{n} reps parsed: {src:.20}...");
                }
            }
        });
    }

    /// Postfix `[1]` chains that stay under the caps parse and print without overflowing; a
    /// chain past the depth cap is a `ParseError`.
    #[test]
    fn index_chains_under_the_cap_round_trip() {
        on_small_stack(|| {
            let e = parse(&format!("x{}", "[1]".repeat(90)), &Units).unwrap();
            assert_eq!(parse(&format!("{e}"), &Units).unwrap(), e);
            assert!(nested_too_deeply(&format!("x{}", "[1]".repeat(150))));
        });
    }

    #[test]
    fn index_brackets_are_not_name_subscripts() {
        assert_eq!(p("v[i]"), Expr::index(v("v"), vec![v("i")]));
        assert_eq!(p("v_i"), Expr::subscripted("v", v("i")));
        assert_eq!(
            p("v[i+1]"),
            Expr::index(v("v"), vec![b(B::Add, v("i"), n(1.0))])
        );
        assert_eq!(
            p("v_{i+1}"),
            Expr::subscripted("v", b(B::Add, v("i"), n(1.0)))
        );
        assert_eq!(p("M[i,j]"), Expr::index(v("M"), vec![v("i"), v("j")]));
        assert_eq!(
            p("M[i][j]"),
            Expr::index(Expr::index(v("M"), vec![v("i")]), vec![v("j")])
        );
        assert_eq!(p("v[0]"), Expr::index(v("v"), vec![n(0.0)]));
    }

    #[test]
    fn index_bases_parse_as_written() {
        assert_eq!(
            p("(a+b)[1]"),
            Expr::index(b(B::Add, v("a"), v("b")), vec![n(1.0)])
        );
        assert_eq!(
            p("f(x)[2]"),
            Expr::index(call_of("f", vec![v("x")]), vec![n(2.0)])
        );
        assert_eq!(
            p("v_0[1]"),
            Expr::index(Expr::subscripted("v", n(0.0)), vec![n(1.0)])
        );
        assert_eq!(
            p("(v_0)[1]"),
            Expr::index(Expr::subscripted("v", n(0.0)), vec![n(1.0)])
        );
        assert_eq!(
            p("v_{i}[j]"),
            Expr::index(Expr::subscripted("v", v("i")), vec![v("j")])
        );
        assert_eq!(
            p("x^T[1]"),
            Expr::index(u(U::Transpose, v("x")), vec![n(1.0)])
        );
        assert_eq!(
            p("(x!)[1]"),
            Expr::index(u(U::Factorial, v("x")), vec![n(1.0)])
        );
    }

    /// `[` after an operand with no space is an index; at the start or after an operator or a
    /// comma it is a matrix literal.
    #[test]
    fn bracket_is_index_after_operand_and_matrix_elsewhere() {
        assert_eq!(
            p("[1, 2][1]"),
            Expr::index(Expr::matrix(1, 2, vec![n(1.0), n(2.0)]), vec![n(1.0)])
        );
        assert_eq!(p("[1, 2]"), Expr::matrix(1, 2, vec![n(1.0), n(2.0)]));
        assert_eq!(
            p("a+[1]"),
            b(B::Add, v("a"), Expr::matrix(1, 1, vec![n(1.0)]))
        );
        assert_eq!(
            p("f([1], 2)"),
            call_of("f", vec![Expr::matrix(1, 1, vec![n(1.0)]), n(2.0)])
        );
        assert_eq!(
            p("[1]:=2"),
            assign(Expr::matrix(1, 1, vec![n(1.0)]), n(2.0))
        );
    }

    #[test]
    fn bracket_after_space_or_braced_comma_subscript_is_error() {
        assert!(perr("x [1]").message.contains("'['"), "{}", perr("x [1]"));
        assert!(perr("M_{i,j}").message.contains("one subscript"));
        assert!(perr("(M_i)_j").message.contains("'_'"));
        assert!(perr("v_1_2").message.contains("'_'"));
    }

    /// Name subscripts and index brackets print differently and read back to different trees.
    #[test]
    fn subscript_and_index_with_same_letters_differ() {
        let sub = Expr::subscripted("v", v("i"));
        let idx = Expr::index(v("v"), vec![v("i")]);
        assert_ne!(sub, idx);
        assert_eq!(sub.to_string(), "v_i");
        assert_eq!(idx.to_string(), "v[i]");
    }

    /// Postfix chains inside parentheses inside binary chains, at many sizes around the cap.
    /// Whatever parses must print and drop without overflowing the 2 MB stack.
    #[test]
    fn mixed_chains_up_to_the_depth_cap_print_without_overflow() {
        on_small_stack(|| {
            let mut parsed = 0;
            for ops in (0..=100).step_by(5) {
                for parens in (0..=50).step_by(5) {
                    for posts in (0..=100).step_by(10) {
                        let src = format!(
                            "x{}+({}x{}{})",
                            "+x".repeat(ops),
                            "(".repeat(parens),
                            "!".repeat(posts),
                            ")".repeat(parens)
                        );
                        if let Ok(e) = parse(&src, &Units) {
                            parsed += 1;
                            drop(format!("{e}"));
                        }
                    }
                }
            }
            assert!(parsed > 0);
        });
    }

    #[test]
    fn long_left_chain_is_capped_by_token_count() {
        on_small_stack(|| {
            // A flat row of 2047 elements is 4095 tokens, within the cap. It is flat because a
            // 2047-operator chain is now a depth error (see `long_chains_that_wrap_the_tree`).
            let ok = format!("[{}1]", "1,".repeat(2046));
            drop(parse(&ok, &Units).unwrap());
            // 2048 `a+` terms make 4097 tokens.
            let too_long = format!("{}a", "a+".repeat(2048));
            let e = perr(&too_long);
            assert!(e.message.contains("more than 4096 tokens"), "{e}");
            assert!(perr(&"a+".repeat(50_000)).message.contains("tokens"));
        });
    }

    #[test]
    fn exponent_is_part_of_the_number() {
        assert_eq!(p("1e-5"), real(1e-5, "1e-5"));
        assert_eq!(p("2e3"), real(2e3, "2e3"));
        assert_eq!(p("1.5E+10"), real(1.5e10, "1.5E+10"));
        assert_eq!(p("6.02e23"), real(6.02e23, "6.02e23"));
    }

    #[test]
    fn e_without_digits_is_still_a_variable() {
        // `2e` is `2 * e`; an identifier character after the digits cancels the exponent.
        assert_eq!(p("2e"), b(B::Mul, n(2.0), v("e")));
        assert_eq!(p("2e-x"), b(B::Sub, b(B::Mul, n(2.0), v("e")), v("x")));
        assert_eq!(p("2e3x"), b(B::Mul, n(2.0), v("e3x")));
        // A space ends the number, so `2 e3` multiplies.
        assert_eq!(p("2 e3"), b(B::Mul, n(2.0), v("e3")));
    }

    #[test]
    fn double_negative_literal_keeps_one_sign_in_text() {
        assert_eq!(p("-(-2)"), u(U::Neg, n(-2.0)));
        assert_eq!(p("--2"), u(U::Neg, n(-2.0)));
    }

    #[test]
    fn negative_fraction_reads_back_as_a_division() {
        assert_eq!(p("-1/3"), b(B::Div, n(-1.0), n(3.0)));
        assert_eq!(p("-4/2"), b(B::Div, n(-4.0), n(2.0)));
    }

    #[test]
    fn range_is_accepted_as_a_command_item() {
        assert_eq!(
            p("x -> 0 .. 3"),
            sym(v("x"), Some(Command(vec![range(n(0.0), None, n(3.0))])))
        );
        assert_eq!(
            p("x -> solve, 0 .. 3"),
            sym(
                v("x"),
                Some(Command(vec![v("solve"), range(n(0.0), None, n(3.0))]))
            )
        );
    }

    // Shapes that the printer maps to text the parser reads differently, or that need care.

    #[test]
    fn integer_division_stays_a_division() {
        assert_eq!(p("1/3"), b(B::Div, n(1.0), n(3.0)));
        assert_eq!(p("4/2"), b(B::Div, n(4.0), n(2.0)));
    }

    #[test]
    fn negative_literal_folds_into_number() {
        assert_eq!(p("-2"), n(-2.0));
        assert_eq!(p("-2^2"), u(U::Neg, b(B::Pow, n(2.0), n(2.0))));
    }

    #[test]
    fn parens_around_operator_are_dropped() {
        assert_eq!(p("(a+b)"), b(B::Add, v("a"), v("b")));
        assert_eq!(p("(x)"), Expr::Parens(Box::new(v("x"))));
    }

    #[test]
    fn single_subscript_on_name_is_id() {
        assert_eq!(p("v_i"), Expr::subscripted("v", v("i")));
        assert_eq!(p("v_0.5"), Expr::subscripted("v", n(0.5)));
    }

    #[test]
    fn symbolic_eval_drops_equals_on_print_so_reads_as_sym_eval() {
        assert_eq!(p("x -> solve, x"), sym(v("x"), Some(cmd(&["solve", "x"]))));
        assert_eq!(p("x -> ■"), sym(v("x"), None));
        assert_eq!(p("x ->"), sym(v("x"), None));
    }

    #[test]
    fn transpose_and_power_text() {
        assert_eq!(p("v^T"), u(U::Transpose, v("v")));
        assert_eq!(p("v^2"), b(B::Pow, v("v"), n(2.0)));
    }

    #[test]
    fn sqrt_is_nth_root_without_degree() {
        assert_eq!(p("sqrt(x)"), Expr::sqrt(v("x")));
        assert_eq!(p("root(x, 3)"), Expr::nth_root(n(3.0), v("x")));
    }

    #[test]
    fn scale_needs_unit_kind_otherwise_multiplies() {
        assert_eq!(p("6 x"), b(B::Mul, n(6.0), v("x")));
        assert_eq!(p("6 m"), scale(n(6.0), unit("m")));
    }

    #[test]
    fn comparisons_chain_left_associative() {
        assert_eq!(p("a<b<c"), b(B::Lt, b(B::Lt, v("a"), v("b")), v("c")));
    }

    #[test]
    fn empty_input_is_an_error() {
        assert_eq!(perr("").pos, 0);
        assert_eq!(perr("   ").pos, 3);
    }

    #[test]
    fn error_positions() {
        assert_eq!(perr("1 +").pos, 3);
        assert_eq!(perr("a $ b").pos, 2);
        assert_eq!(perr("(a+b").pos, 4);
        assert_eq!(perr("a b").pos, 2);
        assert_eq!(perr("[1, 2; 3]").pos, 0);
        assert_eq!(perr("x := ").pos, 5);
        assert_eq!(perr("sqrt(x, y)").pos, 0);
        assert_eq!(perr("1..").pos, 3);
        assert_eq!(perr("é $").pos, 3);
        assert_eq!(perr("a . b").pos, 2);
        assert_eq!(perr("x = y").pos, 4);
        assert_eq!(perr("x -> m=y").pos, 7);
    }

    #[test]
    fn error_messages_name_the_problem() {
        assert!(perr("a $ b").message.contains("unexpected character '$'"));
        assert!(perr("(a+b").message.contains("expected ')'"));
        assert!(perr("a b").message.contains("identifier 'b'"));
    }

    #[test]
    fn double_factorial_is_rejected_with_a_hint() {
        let e = perr("5!!");
        assert_eq!(e.pos, 2);
        assert!(e.message.contains("fact2"), "{}", e.message);
        assert_eq!(perr("x!!!").pos, 2);
        // explicit nesting and a factorial after another operator stay valid
        assert!(parse("(5!)!", &DefaultIdContext).is_ok());
        assert!(parse("5!%!", &DefaultIdContext).is_ok());
        assert!(parse("5! !", &DefaultIdContext).is_err());
    }

    #[test]
    fn default_context_classifies_called_names_as_functions() {
        let e = parse("sin(x) + y", &DefaultIdContext).unwrap();
        assert_eq!(e, b(B::Add, call_of("sin", vec![v("x")]), v("y")));
    }

    #[test]
    fn display_of_parsed_error() {
        assert_eq!(
            perr("a $ b").to_string(),
            "parse error at byte 2: unexpected character '$'"
        );
    }

    /// `name_sub(args)`: a call of a subscripted function name, as the converter builds it.
    fn sub_call(name: &str, sub: Expr, args: Vec<Expr>) -> Expr {
        Expr::Call {
            func: Id {
                name: name.into(),
                kind: IdKind::Function,
                subscript: Some(Box::new(sub)),
            },
            args,
        }
    }

    /// A call of a name with no subscript, for names the parser would otherwise not read as one.
    fn call_of_name(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            func: Id::new(name, IdKind::Function),
            args,
        }
    }

    #[test]
    fn subscripted_call_is_a_call_of_the_subscripted_name() {
        assert_eq!(p("g_1(x)"), sub_call("g", n(1.0), vec![v("x")]));
        assert_eq!(p("S_a(x)"), sub_call("S", v("a"), vec![v("x")]));
        assert_eq!(
            p("f_1(x, r, UMP)"),
            sub_call("f", n(1.0), vec![v("x"), v("r"), v("UMP")])
        );
        assert_eq!(
            p("g_{i+1}(x)"),
            sub_call("g", b(B::Add, v("i"), n(1.0)), vec![v("x")])
        );
        assert_eq!(p("g_1()"), sub_call("g", n(1.0), vec![]));
        // A space before `(` ends the name, so the group is not an argument list. A name is not
        // a number, so the group after it is not a Scale unit either.
        assert_eq!(perr("g_1 (x)").message, "unexpected '('");
    }

    #[test]
    fn subscripted_call_round_trips_and_takes_an_index() {
        round_trip(sub_call("g", n(1.0), vec![v("x")]));
        round_trip(sub_call(
            "h",
            v("A"),
            vec![sub_call("g", n(2.0), vec![v("x")])],
        ));
        round_trip(Expr::index(
            sub_call("g", n(1.0), vec![v("x")]),
            vec![n(2.0)],
        ));
        assert_eq!(
            p("g_1(x)[2]"),
            Expr::index(sub_call("g", n(1.0), vec![v("x")]), vec![n(2.0)])
        );
    }

    /// A subscripted call nests like any call, about two levels per argument list.
    #[test]
    fn subscripted_call_nesting_is_capped_not_overflowing() {
        on_small_stack(|| {
            let ok = format!("{}x{}", "g_1(".repeat(40), ")".repeat(40));
            assert!(parse(&ok, &Units).is_ok());
            let deep = format!("{}x{}", "g_1(".repeat(60), ")".repeat(60));
            assert!(nested_too_deeply(&deep));
            assert!(parse(&format!("{}x", "g_1(".repeat(10_000)), &Units).is_err());
        });
    }

    /// `'` is a name character right after a name, so `C'`, `B''` and `C'_1` are names.
    #[test]
    fn string_literals_parse_and_round_trip() {
        assert_eq!(p("\"yes\""), Expr::Str("yes".to_string()));
        assert_eq!(p("\"\""), Expr::Str(String::new()));
        assert_eq!(
            p("strlen(\"ab c\")"),
            call_of("strlen", vec![Expr::Str("ab c".to_string())])
        );
        round_trip(Expr::Str("x y".to_string()));
        assert!(perr("\"open").message.contains("unterminated string"));
    }

    #[test]
    fn primes_are_part_of_the_name() {
        assert_eq!(p("C'"), v("C'"));
        assert_eq!(p("B''"), v("B''"));
        assert_eq!(p("C'^T"), u(U::Transpose, v("C'")));
        assert_eq!(p("C'_1"), Expr::subscripted("C'", n(1.0)));
        assert_eq!(p("C'(x)"), call_of_name("C'", vec![v("x")]));
        assert_eq!(
            p("|C'-C|/2"),
            b(B::Div, u(U::Abs, b(B::Sub, v("C'"), v("C"))), n(2.0))
        );
        round_trip(v("C'"));
        round_trip(u(U::Transpose, v("C'")));
    }

    #[test]
    fn primes_after_numbers_or_brackets_are_errors() {
        assert!(perr("2'").message.contains("unexpected character '''"));
        assert!(perr("(x)'").message.contains("unexpected character '''"));
        assert!(perr("x '").message.contains("unexpected character '''"));
        assert!(perr("'x").message.contains("unexpected character '''"));
    }

    /// A quoted name is one identifier whatever its text, so names the parser would split (such
    /// as `F_A`, a name with a literal underscore) print and read back unchanged.
    #[test]
    fn quoted_names_read_back_as_one_identifier() {
        assert_eq!(p("`F_A`"), v("F_A"));
        assert_eq!(p("`tan_α`"), v("tan_α"));
        assert_eq!(p("`d_b1`_2"), Expr::subscripted("d_b1", n(2.0)));
        assert_eq!(p("`g_x`(y)"), call_of_name("g_x", vec![v("y")]));
        assert_ne!(p("F_A"), v("F_A"));
        round_trip(v("F_A"));
        round_trip(v("tan_α"));
        round_trip(Expr::subscripted("F", v("A")));
    }

    #[test]
    fn quoted_name_errors() {
        assert!(perr("`abc").message.contains("unterminated"));
        assert!(perr("`a``").message.contains("unterminated"));
        // An empty name is invalid. The converter rejects an empty name, so it never prints one.
        assert!(perr("``").message.contains("empty"));
    }

    /// A backtick inside a quoted name is doubled when printed and read back as one backtick.
    #[test]
    fn backtick_inside_a_quoted_name_is_doubled() {
        assert_eq!(p("`a``b`"), v("a`b"));
        assert_eq!(p("````"), v("`"));
        assert_eq!(p("`a``b`_1"), Expr::subscripted("a`b", n(1.0)));
        round_trip(v("a`b"));
        round_trip(v("`"));
        round_trip(Expr::subscripted("a`b", n(1.0)));
    }

    #[test]
    fn scale_with_parenthesized_unit_expression() {
        assert_eq!(
            p("42 (m/s)"),
            scale(n(42.0), b(B::Div, unit("m"), unit("s")))
        );
        assert_eq!(p("6 (m^2)"), scale(n(6.0), b(B::Pow, unit("m"), n(2.0))));
        assert_eq!(p("6 (m)"), scale(n(6.0), unit("m")));
        round_trip(scale(n(42.0), b(B::Div, unit("m"), unit("s"))));
        round_trip(b(B::Mul, scale(n(6.0), unit("m")), n(2.0)));
    }

    fn assert_stray_paren(src: &str) {
        let e = match parse(src, &DefaultIdContext) {
            Ok(e) => panic!("{src:?} parsed as {e:?}"),
            Err(e) => e,
        };
        assert_eq!(e.message, "unexpected '('", "{src:?}");
    }

    /// Any group after a number is a `Scale` (implicit multiplication), so it reads back as the
    /// printer writes it. A group after a name is still not a `Scale`: under the default context
    /// these are the same parse errors as before.
    #[test]
    fn number_before_a_group_is_a_scale() {
        assert_eq!(
            parse("2 (x+1)", &DefaultIdContext).unwrap(),
            scale(n(2.0), b(B::Add, v("x"), n(1.0)))
        );
        assert_eq!(
            parse("7 (x)", &DefaultIdContext).unwrap(),
            scale(n(7.0), v("x"))
        );
        assert_eq!(
            parse("0 .. 4 (pi)", &DefaultIdContext).unwrap(),
            range(n(0.0), None, scale(n(4.0), v("pi")))
        );
    }

    /// Implicit products that the printer writes and the sample worksheets store: each text reads
    /// as the tree, and the tree prints back to text that reads as the same tree. The sample
    /// round-trip test covers real worksheets; this table names the tricky shapes.
    #[test]
    fn implicit_multiplication_table_round_trips() {
        let x = || v("x");
        let cases: Vec<(&str, Expr)> = vec![
            ("7 (x)", scale(n(7.0), x())),
            (
                "7 (x)+3 (x)",
                b(B::Add, scale(n(7.0), x()), scale(n(3.0), x())),
            ),
            (
                "a^2+2 (ab)",
                b(B::Add, b(B::Pow, v("a"), n(2.0)), scale(n(2.0), v("ab"))),
            ),
            (
                "0 .. 4 (pi)",
                range(
                    n(0.0),
                    None,
                    scale(
                        n(4.0),
                        Expr::Id(Id {
                            name: "pi".into(),
                            kind: IdKind::Constant,
                            subscript: None,
                        }),
                    ),
                ),
            ),
            ("20 (m/s)", scale(n(20.0), b(B::Div, unit("m"), unit("s")))),
            (
                "2 (x+1)*3",
                b(B::Mul, scale(n(2.0), b(B::Add, x(), n(1.0))), n(3.0)),
            ),
            ("(a+b) (c)", scale(b(B::Add, v("a"), v("b")), v("c"))),
            (
                "(2 (c^2))^2",
                b(B::Pow, scale(n(2.0), b(B::Pow, v("c"), n(2.0))), n(2.0)),
            ),
            (
                "6 (x^3)/(x)",
                b(
                    B::Div,
                    scale(n(6.0), b(B::Pow, x(), n(3.0))),
                    Expr::Parens(Box::new(x())),
                ),
            ),
            ("6 m^2", scale(n(6.0), b(B::Pow, unit("m"), n(2.0)))),
            ("0.75 m", scale(n(0.75), unit("m"))),
            ("-2 (x)", negate(scale(n(2.0), x()))),
            ("(1/3) (x)", scale(b(B::Div, n(1.0), n(3.0)), x())),
        ];
        for (text, expected) in cases {
            let read = parse(text, &Units).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!(read, expected, "reading {text:?}");
            let printed = read.to_string();
            let back = parse(&printed, &Units).unwrap_or_else(|e| panic!("{printed:?}: {e}"));
            assert_eq!(back, read, "printed {printed:?} from {text:?}");
        }
    }

    #[test]
    fn groups_after_a_name_are_stray_parens() {
        assert_stray_paren("x (y)");
        assert_stray_paren("x (y^2)");
        assert_stray_paren("(a+b)^2 (c)");
        assert_eq!(parse("x (y)", &DefaultIdContext).unwrap_err().pos, 2);
    }

    /// A group after a closing `)` or `]` is a `Scale` unit, whatever it holds, so the printer's
    /// `(a) (b)` reads back. The unit has its outer parentheses dropped.
    #[test]
    fn group_after_a_closing_bracket_is_a_scale() {
        let ctx = &DefaultIdContext;
        let e = parse("(a)(b)", ctx).unwrap();
        assert_eq!(
            e,
            Expr::Scale {
                value: Box::new(Expr::Parens(Box::new(v("a")))),
                unit: Box::new(v("b")),
            }
        );
        assert_eq!(e.to_string(), "(a) (b)");
        assert_eq!(parse(&e.to_string(), ctx).unwrap(), e);
        assert_eq!(parse("(a) (b)", ctx).unwrap(), e);
        // A call result and an index end with a closing bracket too.
        assert!(matches!(parse("f(x) (y)", ctx), Ok(Expr::Scale { .. })));
        assert!(matches!(parse("v[1] (y)", ctx), Ok(Expr::Scale { .. })));
        // A compound value that ends with a number or name is not closed, so it stays an error.
        assert_stray_paren("(a+b)^2 (c)");
    }

    #[test]
    fn parenthesized_fractions_and_nested_parens_are_scales() {
        let ctx = &DefaultIdContext;
        let e = parse("(1/(x+2)) (2/(x+3)) -> factor", ctx).unwrap();
        let Expr::SymEval { expr, .. } = &e else {
            panic!("expected a symbolic evaluation, got {e:?}");
        };
        assert!(matches!(**expr, Expr::Scale { .. }));
        assert_eq!(parse(&e.to_string(), ctx).unwrap(), e, "{e}");
        let nested = parse("(((a))) (((b)))", ctx).unwrap();
        assert_eq!(nested.to_string(), "(((a))) (b)");
        assert_eq!(parse(&nested.to_string(), ctx).unwrap(), nested);
    }

    #[test]
    fn matrix_group_after_a_name_or_number_is_a_scale() {
        let ctx = &DefaultIdContext;
        let e = parse("text ([sqrt(a^2)])", ctx).unwrap();
        assert!(matches!(e, Expr::Scale { .. }), "{e:?}");
        assert_eq!(e.to_string(), "text ([sqrt(a^2)])");
        assert_eq!(parse(&e.to_string(), ctx).unwrap(), e);
        assert!(matches!(
            parse("2 ([sqrt(a)])", ctx),
            Ok(Expr::Scale { .. })
        ));
        // A bare bracket after a space is still an error: the printer never writes one.
        assert!(parse("text [1, 2]", ctx).is_err());
        assert!(parse("x [sqrt(a)]", ctx).is_err());
    }

    /// The new groups nest with the same depth limit: nested and chained, 150, 1000 and 4000
    /// deep, are errors without overflowing the stack.
    #[test]
    fn closing_bracket_groups_count_depth() {
        on_small_stack(|| {
            for n in [150, 1000, 4000] {
                let nested = format!(
                    "{}a{} {}b{}",
                    "(".repeat(n),
                    ")".repeat(n),
                    "(".repeat(n),
                    ")".repeat(n)
                );
                assert!(parse(&nested, &DefaultIdContext).is_err());
                let chain = "(a) ".repeat(n);
                assert!(parse(&chain, &DefaultIdContext).is_err());
            }
        });
    }

    /// Only a unit context makes a group after a number a unit, and every shape around it:
    /// names, products, quotients, powers, rational powers, matrices, and what is rejected.
    #[test]
    fn unit_groups_after_a_number_are_scales() {
        let si = |src: &str| parse(src, &Si).unwrap_or_else(|e| panic!("{src:?}: {e}"));
        assert_eq!(
            si("42 (m/sec)"),
            scale(n(42.0), b(B::Div, unit("m"), unit("sec")))
        );
        assert_eq!(
            si("5 (kg*m^2/s^2)"),
            scale(
                n(5.0),
                b(
                    B::Div,
                    b(B::Mul, unit("kg"), b(B::Pow, unit("m"), n(2.0))),
                    b(B::Pow, unit("s"), n(2.0))
                )
            )
        );
        // The printer writes these without the group, as `3 m^(1/2)`, which reads back the same.
        assert_eq!(si("3 (m^(1/2))"), si("3 m^(1/2)"));
        assert!(matches!(si("3 (m^(1/2))"), Expr::Scale { .. }));
        assert_eq!(si("2 (m^(-1))"), si("2 m^(-1)"));
        assert_eq!(si("2 (((m)))"), scale(n(2.0), unit("m")));
        assert_eq!(si("2 ((m))"), scale(n(2.0), unit("m")));
        assert_eq!(
            si("2 (m/sec)*3"),
            b(
                B::Mul,
                scale(n(2.0), b(B::Div, unit("m"), unit("sec"))),
                n(3.0)
            )
        );
        assert_eq!(si("2 (m*m)").to_string(), "2 (m*m)");
        assert_eq!(
            p("text ([1, 2])"),
            scale(v("text"), Expr::matrix(1, 2, vec![n(1.0), n(2.0)]))
        );
        assert_eq!(
            p("text ([1, -2; 3, 1/2])").to_string(),
            "text ([1, -2; 3, 1/2])"
        );
    }

    #[test]
    fn non_unit_groups_are_rejected_even_in_a_unit_context() {
        // A number before an empty group, a number unit, or a group that is not an expression.
        for src in ["2 ()", "2 (2)", "2 (m, s)", "x^2 (c)"] {
            assert!(parse(src, &Units).is_err(), "{src:?}");
        }
        // A name after a value is not a group, so `x (m)` is an error too: the value is a name.
        assert_eq!(perr("x (m)").message, "unexpected '('");
    }

    #[test]
    fn bare_names_still_multiply() {
        assert_eq!(p("6 x"), b(B::Mul, n(6.0), v("x")));
    }

    #[test]
    fn unit_group_nesting_counts_depth() {
        on_small_stack(|| {
            // Groups nested in a unit group are scanned, and the scan has its own depth limit.
            let deeper = format!("2 ({}m{})", "(".repeat(10_000), ")".repeat(10_000));
            assert!(parse(&deeper, &Units).is_err());
            let deep_matrix = format!("2 ({}m{})", "[".repeat(200), "]".repeat(200));
            assert!(nested_too_deeply(&deep_matrix));
            // A plain parenthesized unit deep enough to exceed the parser's limit is a depth
            // error from the parser itself, not from the scanner.
            let deep_unit = format!("2 ({}m{})", "(".repeat(55), ")".repeat(55));
            assert!(nested_too_deeply(&deep_unit));
        });
    }

    #[test]
    fn scale_with_matrix_unit_is_parenthesized() {
        let m = Expr::matrix(1, 2, vec![n(1.0), n(2.0)]);
        assert_eq!(p("text ([1, 2])"), scale(v("text"), m.clone()));
        round_trip(scale(v("text"), m));
        assert!(perr("text [1, 2]").message.contains("'['"));
    }

    /// Parenthesized scale units nest with the same cap as other groups, and a deep chain of
    /// them is an error rather than a stack overflow.
    #[test]
    fn scale_group_nesting_is_capped_not_overflowing() {
        on_small_stack(|| {
            let deep = format!("2 ({}m{})", "(".repeat(60), ")".repeat(60));
            assert!(nested_too_deeply(&deep));
            let very_deep = format!("2 ({}m", "(".repeat(10_000));
            assert!(parse(&very_deep, &Units).is_err());
            let ok = format!("2 ({}m{})", "(".repeat(20), ")".repeat(20));
            assert_eq!(parse(&ok, &Units).unwrap(), scale(n(2.0), unit("m")));
        });
    }

    /// `classify_subscripted` lets a name be a unit bare and a variable with a subscript.
    #[test]
    fn subscripted_names_may_have_a_different_kind_from_the_bare_name() {
        struct Split;
        impl IdContext for Split {
            fn classify(&self, _name: &str, _called: bool) -> IdKind {
                IdKind::Unit
            }
            fn classify_subscripted(&self, _name: &str) -> IdKind {
                IdKind::Variable
            }
        }
        assert_eq!(parse("v", &Split).unwrap(), unit("v"));
        assert_eq!(
            parse("v_0", &Split).unwrap(),
            Expr::subscripted("v", n(0.0))
        );
    }

    /// 50k pseudo-random strings from a fixed-seed LCG. Parsing must never panic, and every
    /// error position must lie on a character boundary within the input.
    #[test]
    fn fuzz_random_strings_never_panic() {
        const ALPHABET: &[char] = &[
            '0', '1', '2', '5', '.', 'a', 'm', 'x', 'T', 'v', '_', '{', '}', '(', ')', '[', ']',
            '|', ',', ';', '!', '%', '^', '*', '/', '+', '-', '=', '<', '>', ':', '≡', '∧', '∨',
            '⊕', '¬', '■', '°', 'é', '→', ' ', '\t', '$', '#',
        ];
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as usize
        };
        for _ in 0..50_000 {
            let len = next() % 24;
            let src: String = (0..len)
                .map(|_| ALPHABET[next() % ALPHABET.len()])
                .collect();
            if let Err(e) = parse(&src, &Units) {
                assert!(
                    e.pos <= src.len() && src.is_char_boundary(e.pos),
                    "{src:?}: {e}"
                );
            }
        }
    }
}

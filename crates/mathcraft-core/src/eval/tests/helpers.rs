//! Expression and assertion helpers shared by the evaluation tests.

use num_complex::Complex64;

use crate::eval::{EvalError, Matrix, Outcome, Scalar, Session, Value};
use crate::expr::{BinaryOp, Expr, Id, IdKind, UnaryOp};
use crate::units::{self, Dimension};

pub fn n(x: f64) -> Expr {
    Expr::num(x)
}

pub fn v(name: &str) -> Expr {
    Expr::var(name)
}

/// A unit name as an expression, as the parser produces it.
pub fn u(name: &str) -> Expr {
    Expr::Id(Id::new(name, IdKind::Unit))
}

/// `x unit`, Mathcad's implicit scaling such as `5 m`.
pub fn scale(x: f64, unit: Expr) -> Expr {
    Expr::Scale {
        value: Box::new(n(x)),
        unit: Box::new(unit),
    }
}

pub fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
    Expr::binary(op, a, b)
}

pub fn add(a: Expr, b: Expr) -> Expr {
    bin(BinaryOp::Add, a, b)
}

pub fn mul(a: Expr, b: Expr) -> Expr {
    bin(BinaryOp::Mul, a, b)
}

pub fn div(a: Expr, b: Expr) -> Expr {
    bin(BinaryOp::Div, a, b)
}

pub fn pow(a: Expr, b: Expr) -> Expr {
    bin(BinaryOp::Pow, a, b)
}

pub fn neg(a: Expr) -> Expr {
    Expr::unary(UnaryOp::Neg, a)
}

pub fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::call(name, args)
}

pub fn index(base: Expr, idx: Vec<Expr>) -> Expr {
    Expr::index(base, idx)
}

/// `name := rhs`
pub fn def(name: &str, rhs: Expr) -> Expr {
    Expr::Define {
        target: Box::new(v(name)),
        expr: Box::new(rhs),
    }
}

/// `name(params) := body`
pub fn def_fn(name: &str, params: &[&str], body: Expr) -> Expr {
    Expr::Define {
        target: Box::new(Expr::call(name, params.iter().map(|p| v(p)).collect())),
        expr: Box::new(body),
    }
}

/// `expr =`
pub fn evaluate_stmt(expr: Expr) -> Expr {
    Expr::Eval {
        expr: Box::new(expr),
        unit_override: None,
    }
}

/// `expr ->unit=`
pub fn evaluate_in(expr: Expr, unit: Expr) -> Expr {
    Expr::Eval {
        expr: Box::new(expr),
        unit_override: Some(Box::new(unit)),
    }
}

/// A matrix literal of scalars, row-major.
pub fn mat(rows: usize, cols: usize, elements: Vec<Expr>) -> Expr {
    Expr::matrix(rows, cols, elements)
}

/// Runs `f` on a thread with a 2 MiB stack and returns its result. Recursion tests use it: a
/// stack overflow would abort the test process, and a 2 MiB stack is the smallest one we run on.
pub fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("no panic, no stack overflow")
}

/// Runs statements in order on a fresh session and returns each outcome.
pub fn run_all(stmts: &[Expr]) -> Vec<Outcome> {
    let mut s = Session::new();
    stmts.iter().map(|st| s.run(st)).collect()
}

/// Runs statements and returns the outcome of the last one.
pub fn run_last(stmts: &[Expr]) -> Outcome {
    run_all(stmts).pop().expect("at least one statement")
}

/// The value of an `Outcome::Value`, or a panic naming what came back.
pub fn expect_value(out: Outcome) -> Value {
    match out {
        Outcome::Value(v) => v,
        other => panic!("expected a value, got {other:?}"),
    }
}

pub fn expect_error(out: Outcome) -> EvalError {
    match out {
        Outcome::Error(e) => e,
        other => panic!("expected an error, got {other:?}"),
    }
}

pub fn scalar(v: &Value) -> Scalar {
    match v {
        Value::Scalar(s) => *s,
        other => panic!("expected a scalar, got {other:?}"),
    }
}

pub fn matrix(v: &Value) -> &Matrix {
    match v {
        Value::Matrix(m) => m,
        other => panic!("expected a matrix, got {other:?}"),
    }
}

/// The real part of a scalar value, checking it is real and has dimension `dim`.
pub fn real_in(v: &Value, dim: Dimension) -> f64 {
    let s = scalar(v);
    assert_eq!(s.dim, dim, "dimension of {v:?}");
    assert!(s.value.im.abs() < 1e-300, "expected real, got {}", s.value);
    s.value.re
}

pub fn unit_dim(name: &str) -> Dimension {
    units::unit(name)
        .unwrap_or_else(|| panic!("unit {name} missing"))
        .dimension
}

pub fn close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "{actual} is not {expected} (tolerance {tol})"
    );
}

pub fn complex_close(actual: Complex64, expected: Complex64, tol: f64) {
    assert!(
        (actual - expected).norm() <= tol,
        "{actual} is not {expected} (tolerance {tol})"
    );
}

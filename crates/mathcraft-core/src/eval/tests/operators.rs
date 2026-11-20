//! The operators of the ribbon's Operators palette that read as text: factorial, cross product,
//! norm, partial derivative and gradient. Known values from the Prime help pages.

use super::helpers::*;
use crate::eval::{EvalError, Outcome, Session, Value};
use crate::expr::{BinaryOp, Expr, UnaryOp};
use crate::parser::{parse, DefaultIdContext};

fn text(src: &str) -> Expr {
    parse(src, &DefaultIdContext).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

fn run(stmts: &[&str]) -> Outcome {
    let mut s = Session::new();
    let mut last = None;
    for src in stmts {
        last = Some(s.run(&text(src)));
    }
    last.expect("a statement")
}

fn value(stmts: &[&str]) -> Value {
    expect_value(run(stmts))
}

fn column(v: &Value) -> Vec<f64> {
    matrix(v).data().iter().map(|s| s.value.re).collect()
}

#[test]
fn factorial_in_text() {
    close(scalar(&value(&["5!="])).value.re, 120.0, 0.0);
    close(
        scalar(&value(&["(0.5)!="])).value.re,
        0.886_226_925_452_758,
        1e-12,
    );
    close(scalar(&value(&["3!+2="])).value.re, 8.0, 0.0);
    assert!(matches!(expect_error(run(&["171!="])), EvalError::Overflow));
}

#[test]
fn cross_product_of_unit_vectors() {
    let v = value(&["[1; 0; 0] × [0; 1; 0]="]);
    assert_eq!(column(&v), vec![0.0, 0.0, 1.0]);
    let v = value(&["[1; 2; 3] × [4; 5; 6]="]);
    assert_eq!(column(&v), vec![-3.0, 6.0, -3.0]);
}

#[test]
fn cross_product_units_multiply() {
    let r = mat(
        3,
        1,
        vec![scale(2.0, u("m")), scale(0.0, u("m")), scale(0.0, u("m"))],
    );
    let f = mat(
        3,
        1,
        vec![scale(0.0, u("N")), scale(3.0, u("N")), scale(0.0, u("N"))],
    );
    let out = expect_value(run_last(&[evaluate_stmt(bin(BinaryOp::Cross, r, f))]));
    let m = matrix(&out);
    let want = unit_dim("m").checked_mul(unit_dim("N")).expect("N*m");
    assert!(m.data().iter().all(|s| s.dim == want));
    close(m.data()[2].value.re, 6.0, 1e-12);
}

#[test]
fn cross_product_shape_errors() {
    assert!(matches!(
        expect_error(run(&["[1; 2] × [3; 4]="])),
        EvalError::ShapeMismatch { .. }
    ));
    assert!(matches!(
        expect_error(run(&["2 × [3; 4; 5]="])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn norm_of_vector_scalar_and_matrix() {
    close(scalar(&value(&["‖[3; 4]‖="])).value.re, 5.0, 1e-12);
    close(scalar(&value(&["‖-7‖="])).value.re, 7.0, 0.0);
    close(
        scalar(&value(&["‖[1, 2; 3, 4]‖="])).value.re,
        30.0_f64.sqrt(),
        1e-12,
    );
    // The norm keeps the elements' unit.
    let v = mat(2, 1, vec![scale(3.0, u("m")), scale(4.0, u("m"))]);
    let out = expect_value(run_last(&[evaluate_stmt(Expr::unary(UnaryOp::Norm, v))]));
    assert_eq!(scalar(&out).dim, unit_dim("m"));
    close(scalar(&out).value.re, 5.0, 1e-12);
}

#[test]
fn partial_derivative_matches_the_derivative() {
    let a = scalar(&value(&["x:=2", "partial(x, x^3)="])).value.re;
    close(a, 12.0, 1e-5);
    let b = scalar(&value(&["x:=2", "partial(x, 2, x^3)="])).value.re;
    close(b, 12.0, 1e-4);
    // The other variable is held fixed.
    let c = scalar(&value(&["x:=2", "y:=5", "partial(x, x^2*y)="]))
        .value
        .re;
    close(c, 20.0, 1e-5);
}

#[test]
fn gradient_numeric() {
    let defs = ["f(a,b,c):=a^2*b+b*c^2", "a:=1", "b:=2", "c:=3"];
    let mut all = defs.to_vec();
    all.push("gradient(f(a,b,c))=");
    let v = column(&value(&all));
    close(v[0], 4.0, 1e-5);
    close(v[1], 10.0, 1e-5);
    close(v[2], 12.0, 1e-5);
    // A subset of the variables.
    let mut subset = defs.to_vec();
    subset.push("gradient(f(a,b,c), a, c)=");
    let v = column(&value(&subset));
    assert_eq!(v.len(), 2);
    close(v[1], 12.0, 1e-5);
}

#[test]
fn gradient_symbolic() {
    let out = value(&["f(a,b,c):=a^2*b+b*c^2", "gradient(f(a,b,c)) -> "]);
    let Value::Symbolic(s) = out else {
        panic!("{out:?}")
    };
    assert_eq!((s.rows, s.cols), (3, 1));
    let shown: Vec<String> = s.entries.iter().map(|e| e.to_string()).collect();
    assert_eq!(shown, ["2*a*b", "c^2 + a^2", "2*b*c"]);
}

#[test]
fn prime_operator_is_the_derivative_function() {
    close(
        scalar(&value(&["f(x):=x^3", "f'(2)="])).value.re,
        12.0,
        1e-5,
    );
    close(
        scalar(&value(&["f(x):=x^3", "f''(2)="])).value.re,
        12.0,
        1e-3,
    );
    // The derivative function can be named and called.
    let g = scalar(&value(&["f(x):=sin(x)", "g(t):=f'(t)", "g(0)="]))
        .value
        .re;
    close(g, 1.0, 1e-6);
    // A primed name that is no function stays undefined.
    assert!(matches!(
        expect_error(run(&["h'(2)="])),
        EvalError::UndefinedFunction(_)
    ));
}

#[test]
fn gradient_needs_a_scalar_function_of_named_variables() {
    assert!(matches!(
        expect_error(run(&["gradient(2)="])),
        EvalError::Unsupported(_)
    ));
}

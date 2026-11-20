//! Expression evaluation against an environment (no statements).

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::expr::{BinaryOp, Expr, Id, IdKind, Number, UnaryOp};
use crate::units::Dimension;
use num_complex::Complex64;
use num_rational::BigRational;
use std::f64::consts::{E, PI};

fn eval_in_env(e: &Expr, env: &Env) -> Result<Value, EvalError> {
    evaluate(e, env)
}

fn eval_ok(e: &Expr) -> Value {
    evaluate(e, &Env::new()).unwrap_or_else(|err| panic!("evaluate failed: {err}"))
}

fn eval_err(e: &Expr) -> EvalError {
    evaluate(e, &Env::new()).expect_err("expected an error")
}

fn dimensionless() -> Dimension {
    Dimension::dimensionless()
}

#[test]
fn number_literal_parses_its_text() {
    let e = Expr::Number(Number::Real {
        value: 2.5,
        text: "2.50".to_string(),
    });
    close(real_in(&eval_ok(&e), dimensionless()), 2.5, 0.0);
}

#[test]
fn number_literal_parses_text_exactly() {
    let text = "0.1";
    let e = Expr::Number(Number::Real {
        value: 0.0,
        text: text.to_string(),
    });
    assert_eq!(real_in(&eval_ok(&e), dimensionless()), 0.1);
}

#[test]
fn rational_number_converts_its_fraction() {
    let e = Expr::Number(Number::Rational(BigRational::new(1.into(), 3.into())));
    close(real_in(&eval_ok(&e), dimensionless()), 1.0 / 3.0, 1e-15);
}

#[test]
fn unit_name_is_its_si_factor() {
    let km = eval_ok(&u("km"));
    assert_eq!(real_in(&km, unit_dim("m")), 1000.0);
}

#[test]
fn degree_sign_is_pi_over_180() {
    let angle = eval_ok(&scale(30.0, u("°")));
    close(real_in(&angle, dimensionless()), PI / 6.0, 1e-15);
}

#[test]
fn scale_multiplies_value_by_unit() {
    let len = eval_ok(&scale(5.0, u("m")));
    assert_eq!(real_in(&len, unit_dim("m")), 5.0);
}

#[test]
fn scale_with_power_unit() {
    let area = eval_ok(&scale(6.0, pow(u("m"), n(3.0))));
    assert_eq!(real_in(&area, unit_dim("m").pow(3.into()).unwrap()), 6.0);
}

#[test]
fn addition_needs_one_dimension() {
    let e = add(scale(1.0, u("m")), scale(2.0, u("s")));
    assert!(matches!(eval_err(&e), EvalError::DimensionMismatch { .. }));
}

#[test]
fn addition_of_units_converts_to_si() {
    let e = add(scale(1.0, u("km")), scale(500.0, u("m")));
    assert_eq!(real_in(&eval_ok(&e), unit_dim("m")), 1500.0);
}

#[test]
fn multiplication_combines_dimensions() {
    let e = mul(scale(2.0, u("m")), scale(3.0, u("kg")));
    let want = unit_dim("kg").checked_mul(unit_dim("m")).unwrap();
    assert_eq!(real_in(&eval_ok(&e), want), 6.0);
}

#[test]
fn division_by_zero_is_an_error() {
    assert_eq!(eval_err(&div(n(1.0), n(0.0))), EvalError::DivisionByZero);
}

#[test]
fn negation_keeps_dimension() {
    let e = neg(scale(2.0, u("m")));
    assert_eq!(real_in(&eval_ok(&e), unit_dim("m")), -2.0);
}

#[test]
fn parentheses_are_transparent() {
    let e = Expr::Parens(Box::new(add(n(1.0), n(2.0))));
    assert_eq!(real_in(&eval_ok(&e), dimensionless()), 3.0);
}

#[test]
fn undefined_name_reports_its_name() {
    assert_eq!(
        eval_err(&v("nowhere")),
        EvalError::Undefined("nowhere".to_string())
    );
}

#[test]
fn literal_subscript_is_a_variable_name_not_an_index() {
    let mut env = Env::new();
    env.set_variable(
        "v_0".to_string(),
        Value::Scalar(crate::eval::Scalar::real(5.0, dimensionless())),
    );
    let e = Expr::subscripted("v", n(0.0));
    assert_eq!(
        real_in(&eval_in_env(&e, &env).unwrap(), dimensionless()),
        5.0
    );
}

#[test]
fn compound_subscript_is_unsupported() {
    let e = Expr::subscripted("v", add(v("i"), n(1.0)));
    assert!(matches!(eval_err(&e), EvalError::Unsupported(_)));
}

#[test]
fn constants_pi_e_and_imaginary_unit() {
    close(real_in(&eval_ok(&v("π")), dimensionless()), PI, 0.0);
    close(real_in(&eval_ok(&v("e")), dimensionless()), E, 0.0);
    complex_close(
        scalar(&eval_ok(&v("i"))).value,
        Complex64::new(0.0, 1.0),
        0.0,
    );
    complex_close(
        scalar(&eval_ok(&v("j"))).value,
        Complex64::new(0.0, 1.0),
        0.0,
    );
}

#[test]
fn tolerances_default_to_one_thousandth() {
    let env = Env::new();
    assert_eq!(
        real_in(&eval_in_env(&v("TOL"), &env).unwrap(), dimensionless()),
        1e-3
    );
    assert_eq!(
        real_in(&eval_in_env(&v("CTOL"), &env).unwrap(), dimensionless()),
        1e-3
    );
}

#[test]
fn origin_defaults_to_zero() {
    assert_eq!(real_in(&eval_ok(&v("ORIGIN")), dimensionless()), 0.0);
}

#[test]
fn infinity_is_unsupported() {
    assert!(matches!(eval_err(&v("∞")), EvalError::Unsupported(_)));
}

#[test]
fn celsius_is_refused_but_delta_celsius_is_a_difference() {
    assert!(matches!(eval_err(&u("°C")), EvalError::Unsupported(_)));
    let delta = eval_ok(&scale(3.0, u("Δ°C")));
    assert_eq!(real_in(&delta, unit_dim("K")), 3.0);
}

#[test]
fn vector_literal_and_indexing_from_origin_zero() {
    let vec = mat(3, 1, vec![n(1.0), n(2.0), n(3.0)]);
    let e = index(vec, vec![n(2.0)]);
    assert_eq!(real_in(&eval_ok(&e), dimensionless()), 3.0);
}

#[test]
fn matrix_element_is_indexed_by_row_and_column() {
    let m = mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    let e = index(m, vec![n(1.0), n(0.0)]);
    assert_eq!(real_in(&eval_ok(&e), dimensionless()), 3.0);
}

#[test]
fn indexing_is_relative_to_origin() {
    let mut env = Env::new();
    env.set_variable(
        "ORIGIN".to_string(),
        Value::Scalar(crate::eval::Scalar::real(1.0, dimensionless())),
    );
    let vec = mat(3, 1, vec![n(10.0), n(20.0), n(30.0)]);
    let e = index(vec, vec![n(1.0)]);
    assert_eq!(
        real_in(&eval_in_env(&e, &env).unwrap(), dimensionless()),
        10.0
    );
}

#[test]
fn matrix_elements_keep_their_units() {
    let m = mat(1, 2, vec![scale(1.0, u("m")), scale(2.0, u("m"))]);
    let e = index(m, vec![n(1.0)]);
    assert_eq!(real_in(&eval_ok(&e), unit_dim("m")), 2.0);
}

#[test]
fn index_past_the_end_is_out_of_range() {
    let vec = mat(2, 1, vec![n(1.0), n(2.0)]);
    let e = index(vec, vec![n(2.0)]);
    assert!(matches!(eval_err(&e), EvalError::IndexOutOfRange { .. }));
}

#[test]
fn fractional_index_is_refused() {
    let vec = mat(2, 1, vec![n(1.0), n(2.0)]);
    let e = index(vec, vec![n(0.5)]);
    assert!(matches!(eval_err(&e), EvalError::Unsupported(_)));
}

#[test]
fn index_below_origin_is_refused() {
    let vec = mat(2, 1, vec![n(1.0), n(2.0)]);
    let e = index(vec, vec![n(0.0)]);
    let mut env = Env::new();
    env.set_variable(
        "ORIGIN".to_string(),
        Value::Scalar(crate::eval::Scalar::real(1.0, dimensionless())),
    );
    assert!(matches!(
        eval_in_env(&e, &env),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn indexing_a_scalar_is_refused() {
    assert!(matches!(
        eval_err(&index(n(1.0), vec![n(0.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn single_index_on_a_true_matrix_is_refused() {
    let m = mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    assert!(matches!(
        eval_err(&index(m, vec![n(0.0)])),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn matrix_literal_with_wrong_element_count_is_refused() {
    let bad = Expr::Matrix {
        rows: 1 << 40,
        cols: 1 << 40,
        elements: vec![n(1.0)],
    };
    assert!(matches!(eval_err(&bad), EvalError::Unsupported(_)));
}

#[test]
fn matrix_element_that_is_a_matrix_is_refused() {
    let inner = mat(1, 1, vec![n(1.0)]);
    let outer = mat(1, 1, vec![inner]);
    assert!(matches!(eval_err(&outer), EvalError::Unsupported(_)));
}

#[test]
fn transpose_turns_row_into_column() {
    let row = mat(1, 3, vec![n(1.0), n(2.0), n(3.0)]);
    let e = index(
        Expr::Unary {
            op: UnaryOp::Transpose,
            operand: Box::new(row),
        },
        vec![n(2.0)],
    );
    assert_eq!(real_in(&eval_ok(&e), dimensionless()), 3.0);
}

#[test]
fn matrix_product_of_two_square_matrices() {
    let a = mat(2, 2, vec![n(1.0), n(2.0), n(3.0), n(4.0)]);
    let b = mat(2, 2, vec![n(0.0), n(1.0), n(1.0), n(0.0)]);
    let p = mul(a, b);
    let got = eval_ok(&index(p, vec![n(0.0), n(0.0)]));
    assert_eq!(real_in(&got, dimensionless()), 2.0);
}

#[test]
fn abs_of_scalar_and_of_vector_norm() {
    let neg_three = Expr::Unary {
        op: UnaryOp::Abs,
        operand: Box::new(n(-3.0)),
    };
    assert_eq!(real_in(&eval_ok(&neg_three), dimensionless()), 3.0);
    let vec = mat(2, 1, vec![n(3.0), n(4.0)]);
    let norm = Expr::Unary {
        op: UnaryOp::Abs,
        operand: Box::new(vec),
    };
    assert_eq!(real_in(&eval_ok(&norm), dimensionless()), 5.0);
}

#[test]
fn comparisons_give_one_or_zero() {
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Lt, n(1.0), n(2.0))),
            dimensionless()
        ),
        1.0
    );
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Le, n(2.0), n(1.0))),
            dimensionless()
        ),
        0.0
    );
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Eq, n(2.0), n(2.0))),
            dimensionless()
        ),
        1.0
    );
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Ne, n(2.0), n(2.0))),
            dimensionless()
        ),
        0.0
    );
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Gt, n(3.0), n(2.0))),
            dimensionless()
        ),
        1.0
    );
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Ge, n(1.0), n(2.0))),
            dimensionless()
        ),
        0.0
    );
}

#[test]
fn comparing_different_dimensions_is_an_error() {
    let e = bin(BinaryOp::Lt, scale(1.0, u("m")), scale(1.0, u("s")));
    assert!(matches!(eval_err(&e), EvalError::DimensionMismatch { .. }));
}

#[test]
fn boolean_operators_on_flags() {
    let t = bin(BinaryOp::Lt, n(1.0), n(2.0));
    let f = bin(BinaryOp::Gt, n(1.0), n(2.0));
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::And, t.clone(), f.clone())),
            dimensionless()
        ),
        0.0
    );
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Or, t.clone(), f.clone())),
            dimensionless()
        ),
        1.0
    );
    assert_eq!(
        real_in(
            &eval_ok(&bin(BinaryOp::Xor, t.clone(), t.clone())),
            dimensionless()
        ),
        0.0
    );
    assert_eq!(
        real_in(&eval_ok(&Expr::unary(UnaryOp::Not, f)), dimensionless()),
        1.0
    );
}

#[test]
fn boolean_operator_refuses_numbers_other_than_flags() {
    let e = bin(BinaryOp::And, n(2.0), n(1.0));
    assert!(matches!(eval_err(&e), EvalError::Unsupported(_)));
}

#[test]
fn percent_divides_by_one_hundred() {
    let e = Expr::unary(UnaryOp::Percent, n(5.0));
    close(real_in(&eval_ok(&e), dimensionless()), 0.05, 1e-17);
}

#[test]
fn nth_root_and_square_root_of_negative_is_complex() {
    let cube = Expr::nth_root(n(3.0), n(8.0));
    close(real_in(&eval_ok(&cube), dimensionless()), 2.0, 1e-12);
    let root = eval_ok(&Expr::sqrt(n(-4.0)));
    complex_close(scalar(&root).value, Complex64::new(0.0, 2.0), 1e-12);
}

#[test]
fn zero_degree_root_is_division_by_zero() {
    assert_eq!(
        eval_err(&Expr::nth_root(n(0.0), n(8.0))),
        EvalError::DivisionByZero
    );
}

#[test]
fn range_evaluates_to_a_column_vector() {
    // Help (range_operators.md): `-1 .. 2` is [-1; 0; 1; 2].
    let e = Expr::range(Expr::num(-1.0), None, n(2.0));
    match evaluate(&e, &Env::new()).unwrap() {
        Value::Matrix(m) => {
            assert_eq!(m.shape(), (4, 1));
            let xs: Vec<f64> = m.data().iter().map(|s| s.value.re).collect();
            assert_eq!(xs, vec![-1.0, 0.0, 1.0, 2.0]);
        }
        other => panic!("expected a column vector, got {other:?}"),
    }
}

#[test]
fn symbolic_evaluation_with_a_name_is_a_symbolic_value() {
    let e = Expr::SymEval {
        expr: Box::new(v("x")),
        command: None,
        result: None,
    };
    assert!(matches!(evaluate(&e, &Env::new()), Ok(Value::Symbolic(_))));
}

#[test]
fn definitions_are_statements_not_expressions() {
    assert!(matches!(
        eval_err(&def("x", n(1.0))),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn placeholder_is_unsupported() {
    assert!(matches!(
        eval_err(&Expr::Placeholder),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn factorial_known_values() {
    let f = |x: f64| {
        real_in(
            &eval_ok(&Expr::unary(UnaryOp::Factorial, n(x))),
            dimensionless(),
        )
    };
    close(f(0.0), 1.0, 0.0);
    close(f(5.0), 120.0, 0.0);
    close(f(0.5), 0.886_226_925_452_758, 1e-12);
    close(f(170.0), 7.257_415_615_307_994e306, 1e294);
    close(f(-0.5), 1.772_453_850_905_516, 1e-12);
}

#[test]
fn factorial_errors() {
    for x in [171.0, 1e9, -1.0, -3.0] {
        let e = Expr::unary(UnaryOp::Factorial, n(x));
        assert!(evaluate(&e, &Env::new()).is_err(), "{x}! must fail");
    }
}

#[test]
fn unit_identifier_gives_way_to_a_variable_of_the_same_name() {
    // An Id of kind Unit is the worksheet's variable when one of that name is defined.
    let mut env = Env::new();
    env.set_variable(
        "m".to_string(),
        Value::Scalar(crate::eval::Scalar::real(7.0, dimensionless())),
    );
    let e = Expr::Id(Id::new("m", IdKind::Unit));
    let got = scalar(&eval_in_env(&e, &env).unwrap());
    assert_eq!(got.dim, dimensionless());
    assert_eq!(got.value.re, 7.0);
    // Without the variable it is the unit.
    let got = scalar(&eval_in_env(&e, &Env::new()).unwrap());
    assert_eq!(got.dim, unit_dim("m"));
    assert_eq!(got.value.re, 1.0);
}

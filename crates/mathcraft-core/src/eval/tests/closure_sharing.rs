//! Closure capture shares values and functions (`Arc`) instead of copying them. A chain of
//! functions over one large matrix must hold one matrix, and redefining a capturing function must
//! not copy the matrix again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::helpers::*;
use crate::eval::closure::capture;
use crate::eval::{Env, Matrix, Scalar, UserFunction, Value};
use crate::expr::Expr;
use crate::units::Dimension;

const SIDE: usize = 1000;

/// An environment with `M`, a `SIDE x SIDE` matrix of ones.
fn env_with_big_matrix() -> Env {
    let mut env = Env::new();
    let data = vec![Scalar::real(1.0, Dimension::dimensionless()); SIDE * SIDE];
    let m = Matrix::new(SIDE, SIDE, data).expect("matrix");
    env.set_variable("M".to_string(), Value::Matrix(m));
    env
}

/// Defines `name(x) := body` on `env`, the way `Session` does.
fn define(env: &mut Env, name: &str, body: Expr) {
    let params = vec!["x".to_string()];
    let closure = capture(name, &params, &body, env);
    env.set_function(
        name.to_string(),
        UserFunction {
            params,
            body,
            closure,
        },
    );
}

#[test]
fn chain_of_twenty_closures_over_one_matrix_holds_one_matrix() {
    let mut env = env_with_big_matrix();
    let start = Instant::now();
    define(&mut env, "c1", add(v("x"), v("M")));
    for k in 2..=20 {
        let body = add(call(&format!("c{}", k - 1), vec![v("x")]), v("M"));
        define(&mut env, &format!("c{k}"), body);
    }
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "capturing took {:?}",
        start.elapsed()
    );
    // The worksheet's handle plus one per closure that names M: no copies of the matrix.
    let m = env.variable_shared("M").expect("M");
    assert_eq!(Arc::strong_count(m), 21);
    // Each function is shared by the next closure in the chain, not copied.
    let c19 = env.function_shared("c19").expect("c19");
    assert_eq!(Arc::strong_count(c19), 2);
}

#[test]
fn redefining_a_capturing_function_a_thousand_times_is_fast() {
    let mut env = env_with_big_matrix();
    let start = Instant::now();
    for _ in 0..1000 {
        define(&mut env, "f", add(v("x"), v("M")));
    }
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "1000 redefinitions took {:?}",
        start.elapsed()
    );
    // The worksheet's handle plus the one live definition of f.
    assert_eq!(Arc::strong_count(env.variable_shared("M").expect("M")), 2);
}

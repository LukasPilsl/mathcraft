//! Linear solves: `lsolve` for square matrices (LU), tall matrices (least squares) and matrices of
//! right-hand sides. Expected values are exact or from hand computation; see each test's comment.

use super::helpers::*;
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::expr::Expr;
use crate::units::Dimension;

fn run(e: &Expr) -> Result<Value, EvalError> {
    evaluate(e, &Env::new())
}

fn ok(e: &Expr) -> Value {
    run(e).unwrap_or_else(|err| panic!("failed: {err}"))
}

fn err(e: &Expr) -> EvalError {
    run(e).expect_err("expected an error")
}

/// A matrix literal of plain numbers, row-major.
fn grid(rows: usize, cols: usize, vals: &[f64]) -> Expr {
    mat(rows, cols, vals.iter().map(|x| n(*x)).collect())
}

/// The real parts of a matrix value, asserting every element is real.
fn reals(v: &Value) -> Vec<f64> {
    matrix(v)
        .data()
        .iter()
        .map(|s| {
            assert!(s.value.im.abs() < 1e-12, "expected real, got {}", s.value);
            s.value.re
        })
        .collect()
}

fn dims(v: &Value) -> Vec<Dimension> {
    matrix(v).data().iter().map(|s| s.dim).collect()
}

fn close_all(actual: &[f64], expected: &[f64], tol: f64) {
    assert_eq!(actual.len(), expected.len(), "length of {actual:?}");
    for (a, e) in actual.iter().zip(expected) {
        close(*a, *e, tol);
    }
}

fn m_dim(name: &str) -> Dimension {
    unit_dim(name)
}

// ---- lsolve ---------------------------------------------------------------------------------

#[test]
fn lsolve_of_a_two_by_two_system() {
    // 2x + y = 3, x + 3y = 5: x = 0.8, y = 1.4.
    let got = ok(&call(
        "lsolve",
        vec![grid(2, 2, &[2.0, 1.0, 1.0, 3.0]), grid(2, 1, &[3.0, 5.0])],
    ));
    close_all(&reals(&got), &[0.8, 1.4], 1e-12);
}

#[test]
fn lsolve_gives_negative_solutions() {
    // diag(1, -1) x = (2, 3): x = (2, -3).
    let got = ok(&call(
        "lsolve",
        vec![grid(2, 2, &[1.0, 0.0, 0.0, -1.0]), grid(2, 1, &[2.0, 3.0])],
    ));
    close_all(&reals(&got), &[2.0, -3.0], 1e-12);
}

#[test]
fn lsolve_dimension_is_b_over_a() {
    // A in m, b in N: x in N/m. Same system as above, with units.
    let a = mat(
        2,
        2,
        vec![
            scale(2.0, u("m")),
            scale(1.0, u("m")),
            scale(1.0, u("m")),
            scale(3.0, u("m")),
        ],
    );
    let b = mat(2, 1, vec![scale(3.0, u("N")), scale(5.0, u("N"))]);
    let got = ok(&call("lsolve", vec![a, b]));
    close_all(&reals(&got), &[0.8, 1.4], 1e-12);
    let expected = unit_dim("N").checked_div(m_dim("m")).unwrap();
    assert!(dims(&got).iter().all(|d| *d == expected));
}

#[test]
fn lsolve_of_a_singular_matrix_is_singular() {
    let e = call(
        "lsolve",
        vec![grid(2, 2, &[1.0, 2.0, 2.0, 4.0]), grid(2, 1, &[1.0, 1.0])],
    );
    assert_eq!(err(&e), EvalError::Singular);
}

#[test]
fn lsolve_with_mismatched_vector_is_a_shape_error() {
    let e = call(
        "lsolve",
        vec![
            grid(2, 2, &[1.0, 0.0, 0.0, 1.0]),
            grid(3, 1, &[1.0, 2.0, 3.0]),
        ],
    );
    assert_eq!(
        err(&e),
        EvalError::ShapeMismatch {
            left: (2, 2),
            right: (3, 1)
        }
    );
}

#[test]
fn lsolve_solves_each_column_of_a_matrix_of_right_hand_sides() {
    // A = [2 1; 1 3]: b = (3, 5) gives (0.8, 1.4); b = (1, 2) gives (0.2, 0.6).
    let a = grid(2, 2, &[2.0, 1.0, 1.0, 3.0]);
    let b = grid(2, 2, &[3.0, 1.0, 5.0, 2.0]);
    let got = ok(&call("lsolve", vec![a, b]));
    assert_eq!(matrix(&got).shape(), (2, 2));
    close_all(&reals(&got), &[0.8, 0.2, 1.4, 0.6], 1e-12);
}

#[test]
fn lsolve_of_a_tall_system_is_the_least_squares_solution() {
    // A = [1 0; 0 1; 1 1], b = (1, 2, 4). The normal equations [2 1; 1 2] x = (5, 6) give
    // x = (4/3, 7/3); the residual b - A x = (-1, -1, 1)/3 is orthogonal to the columns of A.
    let a = grid(3, 2, &[1.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let got = ok(&call("lsolve", vec![a, grid(3, 1, &[1.0, 2.0, 4.0])]));
    assert_eq!(matrix(&got).shape(), (2, 1));
    close_all(&reals(&got), &[4.0 / 3.0, 7.0 / 3.0], 1e-12);
}

#[test]
fn lsolve_of_a_tall_system_with_several_right_hand_sides() {
    // Same A. b = (0, 0, 1) gives (1/3, 1/3); b = (1, 2, 4) gives (4/3, 7/3). Result rows are
    // x-components, columns are the solutions: [4/3 1/3; 7/3 1/3].
    let a = grid(3, 2, &[1.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let b = grid(3, 2, &[1.0, 0.0, 2.0, 0.0, 4.0, 1.0]);
    let got = ok(&call("lsolve", vec![a, b]));
    assert_eq!(matrix(&got).shape(), (2, 2));
    close_all(
        &reals(&got),
        &[4.0 / 3.0, 1.0 / 3.0, 7.0 / 3.0, 1.0 / 3.0],
        1e-12,
    );
}

#[test]
fn lsolve_of_a_wide_matrix_is_unsupported() {
    // The help gives the least-squares rule for more rows than columns only.
    let e = call(
        "lsolve",
        vec![grid(2, 3, &[1.0; 6]), grid(2, 1, &[1.0, 1.0])],
    );
    assert!(matches!(err(&e), EvalError::Unsupported(_)));
}

#[test]
fn lsolve_of_a_rank_deficient_tall_matrix_is_singular() {
    // The second column is twice the first, so the least-squares problem has no unique answer.
    let a = grid(3, 2, &[1.0, 2.0, 1.0, 2.0, 1.0, 2.0]);
    let e = call("lsolve", vec![a, grid(3, 1, &[1.0, 2.0, 3.0])]);
    assert_eq!(err(&e), EvalError::Singular);
}

/// Deterministic pseudo-random values in [-1, 1), from a linear congruential generator.
struct Lcg(u64);

impl Lcg {
    fn next_value(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }
}

#[test]
fn lsolve_of_random_well_conditioned_systems_has_a_small_residual() {
    // Diagonally dominant A (diagonal += size, entries in [-1, 1)): residual below 1e-12.
    for (seed, size) in [(1u64, 5usize), (2, 20), (3, 60)] {
        let mut rng = Lcg(seed);
        let mut a = vec![0.0; size * size];
        for i in 0..size {
            for j in 0..size {
                let diag = if i == j { size as f64 } else { 0.0 };
                a[i * size + j] = rng.next_value() + diag;
            }
        }
        let b: Vec<f64> = (0..size).map(|_| rng.next_value()).collect();
        let got = ok(&call(
            "lsolve",
            vec![grid(size, size, &a), grid(size, 1, &b)],
        ));
        let x = reals(&got);
        for i in 0..size {
            let ax: f64 = (0..size).map(|j| a[i * size + j] * x[j]).sum();
            let residual = (ax - b[i]).abs();
            assert!(
                residual < 1e-12,
                "residual {residual} at row {i}, size {size}"
            );
        }
    }
}

#[test]
fn lsolve_of_the_hilbert_five_matrix_recovers_ones() {
    // H_ij = 1 / (i + j + 1), condition number about 5e5. With b = H (1, 1, 1, 1, 1) the exact
    // solution is all ones; the error is about cond * machine epsilon, far below 1e-6.
    let size = 5;
    let h: Vec<f64> = (0..size)
        .flat_map(|i| (0..size).map(move |j| 1.0 / ((i + j + 1) as f64)))
        .collect();
    let b: Vec<f64> = (0..size)
        .map(|i| (0..size).map(|j| h[i * size + j]).sum())
        .collect();
    let got = ok(&call(
        "lsolve",
        vec![grid(size, size, &h), grid(size, 1, &b)],
    ));
    close_all(&reals(&got), &[1.0; 5], 1e-6);
}

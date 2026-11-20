//! Evaluator-level checks for `quickscreen` and `effects`: the help example "Example: Calculating
//! Effects" (its design K and results Y), a linear model whose effects are known by construction,
//! and the refusals.

use super::helpers::*;
use crate::eval::value::Value;
use crate::expr::Expr;

/// The numeric table of a `[labels; table]` result, element `(i, j)`.
fn table(v: &Value, i: usize, j: usize) -> f64 {
    let Value::List(items) = v else {
        panic!("expected a list");
    };
    let Value::Matrix(m) = &items[1] else {
        panic!("expected the numeric table");
    };
    m.get(i, j).expect("element").value.re
}

fn value_of(e: Expr) -> Value {
    expect_value(run_last(&[evaluate_stmt(e)]))
}

fn error_of(e: Expr) -> crate::eval::EvalError {
    expect_error(run_last(&[evaluate_stmt(e)]))
}

/// Rows of a design as `(run, block, a, b)` numbers, as a matrix expression.
fn design(rows: &[[f64; 4]]) -> Expr {
    let mut elements = Vec::new();
    for r in rows {
        for x in r {
            elements.push(n(*x));
        }
    }
    mat(rows.len(), 4, elements)
}

/// A column of replicated results, one row per run, as a matrix expression.
fn results(rows: &[Vec<f64>]) -> Expr {
    let cols = rows[0].len();
    let mut elements = Vec::new();
    for r in rows {
        for x in r {
            elements.push(n(*x));
        }
    }
    mat(rows.len(), cols, elements)
}

/// The help design: run 1 and 2 in block 1, runs 3 and 4 in block 2; A = (+,-,-,+), B = (-,+,-,+).
fn help_design() -> Expr {
    design(&[
        [1.0, 1.0, 1.0, -1.0],
        [2.0, 1.0, -1.0, 1.0],
        [3.0, 2.0, -1.0, -1.0],
        [4.0, 2.0, 1.0, 1.0],
    ])
}

fn help_results() -> Expr {
    results(&[
        vec![116.1, 116.9, 112.6, 118.7, 114.9],
        vec![116.5, 115.5, 119.2, 114.7, 118.3],
        vec![106.7, 107.5, 105.9, 107.1, 106.5],
        vec![123.2, 125.1, 124.5, 124.0, 124.7],
    ])
}

/// The labels and the matrices of a `[labels; tables]` result.
fn parts(v: &Value) -> (Vec<String>, Vec<Value>) {
    let Value::List(items) = v else {
        panic!("expected a list, found {v:?}");
    };
    let Value::List(labels) = &items[0] else {
        panic!("expected a list of labels");
    };
    let names = labels
        .iter()
        .map(|l| match l {
            Value::Str(s) => s.clone(),
            other => panic!("expected a string label, found {other:?}"),
        })
        .collect();
    let tables = match &items[1] {
        Value::List(t) => t.clone(),
        Value::Matrix(_) => vec![items[1].clone()],
        other => panic!("expected the tables, found {other:?}"),
    };
    (names, tables)
}

fn quick(extra: Option<&str>) -> Value {
    let mut args = vec![help_design(), help_results()];
    if let Some(s) = extra {
        args.push(Expr::Str(s.to_string()));
    }
    value_of(call("quickscreen", args))
}

fn effects_of(extra: Option<&str>) -> Value {
    let mut args = vec![help_design(), help_results()];
    if let Some(s) = extra {
        args.push(Expr::Str(s.to_string()));
    }
    value_of(call("effects", args))
}

#[test]
fn quickscreen_matches_the_help_table() {
    // Help: quickscreen(K, Y) = "A" 111.79 120.07 8.28; "B" 111.29 120.57 9.28;
    // "Blocks" 116.34 115.52 -0.82.
    let v = quick(None);
    let (names, _) = parts(&v);
    assert_eq!(names, ["A", "B", "Blocks"]);
    let Value::List(items) = &v else {
        unreachable!()
    };
    let Value::Matrix(m) = &items[1] else {
        panic!("expected the numeric table");
    };
    assert_eq!(m.shape(), (3, 3));
    let expected = [
        [111.79, 120.07, 8.28],
        [111.29, 120.57, 9.28],
        [116.34, 115.52, -0.82],
    ];
    for (i, row) in expected.iter().enumerate() {
        for (j, want) in row.iter().enumerate() {
            close(table(&v, i, j), *want, 1e-9);
        }
    }
}

#[test]
fn quickscreen_interaction_matches_the_help() {
    // Help: quickscreen(K, Y, "AB") = "AB" 116.34 115.52 -0.82.
    let v = quick(Some("AB"));
    let (names, _) = parts(&v);
    assert_eq!(names, ["AB"]);
    close(table(&v, 0, 0), 116.34, 1e-9);
    close(table(&v, 0, 1), 115.52, 1e-9);
    close(table(&v, 0, 2), -0.82, 1e-9);
}

#[test]
fn effects_of_factors_match_the_help() {
    // Help: effects(K, Y) gives A at -1 and 1 as -4.14 and 4.14, and B as -4.64 and 4.64.
    let v = effects_of(None);
    let (names, tables) = parts(&v);
    assert_eq!(names, ["A", "B"]);
    let Value::Matrix(a) = &tables[0] else {
        panic!("expected a matrix for A");
    };
    close(a.get(0, 1).unwrap().value.re, -4.14, 1e-9);
    close(a.get(1, 1).unwrap().value.re, 4.14, 1e-9);
    assert_eq!(a.get(0, 0).unwrap().value.re, -1.0);
    assert_eq!(a.get(1, 0).unwrap().value.re, 1.0);
    let Value::Matrix(b) = &tables[1] else {
        panic!("expected a matrix for B");
    };
    close(b.get(0, 1).unwrap().value.re, -4.64, 1e-9);
    close(b.get(1, 1).unwrap().value.re, 4.64, 1e-9);
}

#[test]
fn effects_of_blocks_and_interaction_match_the_help() {
    // Help: effects(K, Y, "Blocks") = (1 0.41 ; 2 -0.41).
    let (_, tables) = parts(&effects_of(Some("Blocks")));
    let Value::Matrix(b) = &tables[0] else {
        panic!("expected the block matrix");
    };
    close(b.get(0, 1).unwrap().value.re, 0.41, 1e-9);
    close(b.get(1, 1).unwrap().value.re, -0.41, 1e-9);

    // Help: effects(K, Y, "AB") rows (-1,-1,-0.41), (-1,1,0.41), (1,-1,0.41), (1,1,-0.41).
    let (names, tables) = parts(&effects_of(Some("AB")));
    assert_eq!(names, ["AB"]);
    let Value::Matrix(ab) = &tables[0] else {
        panic!("expected the interaction matrix");
    };
    assert_eq!(ab.shape(), (4, 3));
    let expected = [-0.41, 0.41, 0.41, -0.41];
    for (i, want) in expected.iter().enumerate() {
        close(ab.get(i, 2).unwrap().value.re, *want, 1e-9);
    }
}

#[test]
fn effects_recover_a_known_linear_model() {
    // y = 10 + 2A - 3B on the 2^2 design: the effect of A is 4, of B is -6, of AB is 0, and the
    // level effects of A are -2 and 2 about the overall mean of 10.
    let rows = [
        [1.0, 1.0, -1.0, -1.0],
        [2.0, 1.0, 1.0, -1.0],
        [3.0, 1.0, -1.0, 1.0],
        [4.0, 1.0, 1.0, 1.0],
    ];
    let y: Vec<Vec<f64>> = rows
        .iter()
        .map(|r| vec![10.0 + 2.0 * r[2] - 3.0 * r[3]])
        .collect();
    let q = value_of(call("quickscreen", vec![design(&rows), results(&y)]));
    close(table(&q, 0, 2), 4.0, 1e-9);
    close(table(&q, 1, 2), -6.0, 1e-9);
    let (_, tables) = parts(&value_of(call(
        "effects",
        vec![design(&rows), results(&y), Expr::Str("A, AB".to_string())],
    )));
    let Value::Matrix(a) = &tables[0] else {
        panic!("expected the matrix for A");
    };
    close(a.get(0, 1).unwrap().value.re, -2.0, 1e-9);
    close(a.get(1, 1).unwrap().value.re, 2.0, 1e-9);
    let Value::Matrix(ab) = &tables[1] else {
        panic!("expected the matrix for AB");
    };
    for i in 0..4 {
        close(ab.get(i, 2).unwrap().value.re, 0.0, 1e-9);
    }
}

#[test]
fn screening_refuses_what_it_cannot_reproduce() {
    // An integer order is not built.
    let err = error_of(call(
        "quickscreen",
        vec![help_design(), help_results(), n(2.0)],
    ));
    assert!(
        matches!(err, crate::eval::EvalError::Unsupported(_)),
        "{err:?}"
    );

    // A term naming a factor the design does not have.
    let err = error_of(call(
        "effects",
        vec![help_design(), help_results(), Expr::Str("C".to_string())],
    ));
    assert!(
        matches!(err, crate::eval::EvalError::Unsupported(_)),
        "{err:?}"
    );

    // Blocks asked for on a design with one block.
    let one_block = design(&[
        [1.0, 1.0, 1.0, -1.0],
        [2.0, 1.0, -1.0, 1.0],
        [3.0, 1.0, -1.0, -1.0],
        [4.0, 1.0, 1.0, 1.0],
    ]);
    let err = error_of(call(
        "quickscreen",
        vec![one_block, help_results(), Expr::Str("Blocks".to_string())],
    ));
    assert!(
        matches!(err, crate::eval::EvalError::Unsupported(_)),
        "{err:?}"
    );

    // A coded value other than +1 or -1.
    let uncoded = design(&[
        [1.0, 1.0, 0.5, -1.0],
        [2.0, 1.0, -1.0, 1.0],
        [3.0, 2.0, -1.0, -1.0],
        [4.0, 2.0, 1.0, 1.0],
    ]);
    let err = error_of(call("quickscreen", vec![uncoded, help_results()]));
    assert!(
        matches!(err, crate::eval::EvalError::Unsupported(_)),
        "{err:?}"
    );

    // Rows of the design and of the results do not match.
    let short = results(&[vec![1.0], vec![2.0]]);
    let err = error_of(call("quickscreen", vec![help_design(), short]));
    assert!(
        matches!(err, crate::eval::EvalError::ShapeMismatch { .. }),
        "{err:?}"
    );
}

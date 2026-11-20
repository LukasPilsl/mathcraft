//! Sort builtins `csort`, `rsort` and `sort`: heapsort tie order, the help's visible examples,
//! properties on random matrices, units, key errors and the work charge at large sizes. Expected
//! values come from the help "Example: Sorting Functions" and "Sorting Arrays" (heapsort). The
//! tie orders are those of the Numerical Recipes heapsort, computed by a python3 -I run of it.

use super::builtins_env::{def_origin, dims, error_of, grid, reals, then_eval, value_of};
use super::helpers::*;
use crate::eval::work::builtin_units;
use crate::eval::{EvalError, Matrix, Scalar, Value};
use crate::units::Dimension;

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

// ---- heapsort tie order ----------------------------------------------------------------------
// Ties come out in the order of the Numerical Recipes heapsort that the help names (sorting_arrays
// page). The expected permutations were computed by a python3 -I run of the same hpsort on the keys.

#[test]
fn csort_ties_come_out_in_heapsort_order() {
    // Keys [1, 0, 1, 0] give the permutation [1, 3, 2, 0]: the 0s are rows 1 and 3 (not stable).
    let a = grid(4, 2, &[1.0, 10.0, 0.0, 20.0, 1.0, 30.0, 0.0, 40.0]);
    let got = value_of(&[evaluate_stmt(call("csort", vec![a, n(0.0)]))]);
    assert_eq!(
        reals(&got),
        vec![0.0, 20.0, 0.0, 40.0, 1.0, 30.0, 1.0, 10.0]
    );
}

#[test]
fn rsort_ties_come_out_in_heapsort_order() {
    // Key row [2, 1, 2] gives the permutation [1, 2, 0]: column 2 comes before column 0.
    let a = grid(2, 3, &[2.0, 1.0, 2.0, 5.0, 6.0, 7.0]);
    let got = value_of(&[evaluate_stmt(call("rsort", vec![a, n(0.0)]))]);
    assert_eq!(reals(&got), vec![1.0, 2.0, 2.0, 6.0, 7.0, 5.0]);
}

#[test]
fn csort_tie_order_on_a_help_shaped_key_column() {
    // Keys 0 0 0 1 1 2 2 (the shape of the help's csort(M, 1) output, whose M is truncated there)
    // give the permutation [2, 0, 1, 4, 3, 6, 5]. The rows are (year, key) with years 1710 + i,
    // so the year 1712 comes before 1710 and 1711, as 1712 comes before 1711 in the help.
    let mut elements = Vec::new();
    for (i, key) in [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0].iter().enumerate() {
        elements.push(n(1710.0 + i as f64));
        elements.push(n(*key));
    }
    let a = mat(7, 2, elements);
    let got = value_of(&[evaluate_stmt(call("csort", vec![a, n(1.0)]))]);
    assert_eq!(
        reals(&got),
        vec![
            1712.0, 0.0, 1710.0, 0.0, 1711.0, 0.0, 1714.0, 1.0, 1713.0, 1.0, 1716.0, 2.0, 1715.0,
            2.0,
        ]
    );
}

#[test]
fn rsort_help_example_rows_match_the_visible_output() {
    // R (help step 8) and rsort(R, 0), rsort(R, 5), rsort(R, 10) (help step 9). Only the first
    // seven rows are visible in the help. Rows 7 to 9 are not shown: they do not affect the result,
    // since rsort orders each column by the key row alone and the visible rows only move columns.
    // Row 10 is (6, 3, 1710) (help step 9's text). The keys are unique, so no tie order arises.
    let visible: [[f64; 3]; 7] = [
        [1.0, 1700.0, 5.0],
        [0.0, 1701.0, 11.0],
        [2.0, 1702.0, 16.0],
        [1.0, 1703.0, 23.0],
        [3.0, 1704.0, 36.0],
        [2.0, 1705.0, 58.0],
        [4.0, 1706.0, 29.0],
    ];
    let mut rows: Vec<[f64; 3]> = visible.to_vec();
    rows.extend([[0.0, 0.0, 0.0]; 3]);
    rows.push([6.0, 1710.0, 3.0]);
    let flat: Vec<f64> = rows.iter().flatten().copied().collect();
    let r = grid(11, 3, &flat);
    let expected_r0 = [
        [1.0, 5.0, 1700.0],
        [0.0, 11.0, 1701.0],
        [2.0, 16.0, 1702.0],
        [1.0, 23.0, 1703.0],
        [3.0, 36.0, 1704.0],
        [2.0, 58.0, 1705.0],
        [4.0, 29.0, 1706.0],
    ];
    let expected_r10 = [
        [5.0, 1.0, 1700.0],
        [11.0, 0.0, 1701.0],
        [16.0, 2.0, 1702.0],
        [23.0, 1.0, 1703.0],
        [36.0, 3.0, 1704.0],
        [58.0, 2.0, 1705.0],
        [29.0, 4.0, 1706.0],
    ];
    for (key, expected) in [(0.0, expected_r0), (5.0, expected_r0), (10.0, expected_r10)] {
        let got = reals(&value_of(&[evaluate_stmt(call(
            "rsort",
            vec![r.clone(), n(key)],
        ))]));
        let shown: Vec<f64> = expected.iter().flatten().copied().collect();
        assert_eq!(&got[..21], &shown[..], "rsort(R, {key})");
    }
}

/// A tiny deterministic generator for the property tests (no external crate).
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, m: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % m
    }

    /// `count` small integers, so that ties are common.
    fn values(&mut self, count: usize) -> Vec<f64> {
        (0..count).map(|_| self.below(4) as f64).collect()
    }
}

fn rows_of(flat: &[f64], cols: usize) -> Vec<Vec<f64>> {
    flat.chunks(cols).map(<[f64]>::to_vec).collect()
}

fn columns_of(flat: &[f64], rows: usize, cols: usize) -> Vec<Vec<f64>> {
    (0..cols)
        .map(|j| (0..rows).map(|i| flat[i * cols + j]).collect())
        .collect()
}

fn sorted_copy(mut v: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    v
}

#[test]
fn csort_properties_hold_on_random_matrices() {
    let mut rng = Lcg(7);
    for case in 0..200 {
        let rows = 1 + rng.below(9) as usize;
        let cols = 1 + rng.below(4) as usize;
        let vals = rng.values(rows * cols);
        let key = rng.below(cols as u64) as usize;
        let a = grid(rows, cols, &vals);
        let once = reals(&value_of(&[evaluate_stmt(call(
            "csort",
            vec![a.clone(), n(key as f64)],
        ))]));
        // Sorted: the key column never decreases.
        let key_col: Vec<f64> = rows_of(&once, cols).iter().map(|r| r[key]).collect();
        assert!(key_col.windows(2).all(|w| w[0] <= w[1]), "case {case}");
        // A permutation of the rows.
        assert_eq!(
            sorted_copy(rows_of(&once, cols)),
            sorted_copy(rows_of(&vals, cols)),
            "case {case}"
        );
        // Sorting again leaves the key order unchanged.
        let twice = reals(&value_of(&[evaluate_stmt(call(
            "csort",
            vec![grid(rows, cols, &once), n(key as f64)],
        ))]));
        let key_twice: Vec<f64> = rows_of(&twice, cols).iter().map(|r| r[key]).collect();
        assert_eq!(key_twice, key_col, "case {case}");
        // ORIGIN 1 with the column one higher gives the same matrix as ORIGIN 0.
        let shifted = then_eval(
            vec![def_origin(1.0)],
            call("csort", vec![a, n((key + 1) as f64)]),
        );
        assert_eq!(reals(&value_of(&shifted)), once, "case {case}");
    }
}

#[test]
fn rsort_properties_hold_on_random_matrices() {
    let mut rng = Lcg(11);
    for case in 0..200 {
        let rows = 1 + rng.below(4) as usize;
        let cols = 1 + rng.below(9) as usize;
        let vals = rng.values(rows * cols);
        let key = rng.below(rows as u64) as usize;
        let a = grid(rows, cols, &vals);
        let once = reals(&value_of(&[evaluate_stmt(call(
            "rsort",
            vec![a.clone(), n(key as f64)],
        ))]));
        let key_row: Vec<f64> = columns_of(&once, rows, cols)
            .iter()
            .map(|c| c[key])
            .collect();
        assert!(key_row.windows(2).all(|w| w[0] <= w[1]), "case {case}");
        // A permutation of the columns.
        assert_eq!(
            sorted_copy(columns_of(&once, rows, cols)),
            sorted_copy(columns_of(&vals, rows, cols)),
            "case {case}"
        );
        let twice = reals(&value_of(&[evaluate_stmt(call(
            "rsort",
            vec![grid(rows, cols, &once), n(key as f64)],
        ))]));
        let key_twice: Vec<f64> = columns_of(&twice, rows, cols)
            .iter()
            .map(|c| c[key])
            .collect();
        assert_eq!(key_twice, key_row, "case {case}");
        let shifted = then_eval(
            vec![def_origin(1.0)],
            call("rsort", vec![a, n((key + 1) as f64)]),
        );
        assert_eq!(reals(&value_of(&shifted)), once, "case {case}");
    }
}

#[test]
fn sort_of_a_vector_is_ascending_with_ties_in_place() {
    // Tied values are equal, so their order cannot show in the output; the result is ascending.
    let v = mat(7, 1, [2.0, 1.0, 2.0, 0.0, 1.0, 0.0, 2.0].map(n).to_vec());
    let got = reals(&value_of(&[evaluate_stmt(call("sort", vec![v]))]));
    assert_eq!(got, vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0]);
}

#[test]
fn rsort_keeps_units_and_refuses_mixed_key_dimensions() {
    let good = mat(
        2,
        2,
        vec![
            scale(2.0, u("m")),
            scale(1.0, u("m")),
            scale(5.0, u("s")),
            scale(6.0, u("s")),
        ],
    );
    // Row 0 is in metres: columns 1 then 0 (values 1 m and 2 m); the seconds follow their columns.
    let got = value_of(&[evaluate_stmt(call("rsort", vec![good, n(0.0)]))]);
    assert_eq!(reals(&got), vec![1.0, 2.0, 6.0, 5.0]);
    assert_eq!(
        dims(&got),
        vec![unit_dim("m"), unit_dim("m"), unit_dim("s"), unit_dim("s")]
    );
    // Row 0 mixes metres and a dimensionless number: the key cannot be ordered.
    let mixed = mat(
        2,
        2,
        vec![scale(1.0, u("m")), n(1.0), scale(2.0, u("s")), n(2.0)],
    );
    let e = error_of(&[evaluate_stmt(call("rsort", vec![mixed, n(0.0)]))]);
    assert!(matches!(e, EvalError::DimensionMismatch { .. }), "{e:?}");
}

// ---- hostile sizes: the work charge per element and pass ---------------------------------------

#[test]
fn a_large_column_is_charged_per_element_and_pass_and_sorts() {
    // 1000 rows: ceil(log2 1000) = 10 passes, 3 units per element and pass.
    let values: Vec<Scalar> = (0..1000)
        .map(|i| Scalar::real(f64::from(i * 7 % 13), dimless()))
        .collect();
    let column = Value::Matrix(Matrix::new(1000, 1, values).expect("matrix"));
    assert_eq!(
        builtin_units("csort", std::slice::from_ref(&column)),
        3 * 1000 * 10
    );
    let got = reals(&value_of(&[evaluate_stmt(call(
        "sort",
        vec![mat(
            1000,
            1,
            (0..1000).map(|i| n(f64::from(i * 7 % 13))).collect(),
        )],
    ))]));
    assert!(got.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(got.len(), 1000);
}

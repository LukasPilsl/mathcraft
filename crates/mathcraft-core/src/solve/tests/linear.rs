//! Linear systems: solved by Newton in one step, to round-off.

use super::{assert_vec_close, solve};
use crate::solve::{find, Problem};

#[test]
fn two_by_two_linear_system() {
    // 2x + 7y = 7 and 3x - 4y = 0  =>  x = 28/29, y = 21/29 (Prime help: 0.966, 0.724).
    let p = Problem::new(vec![1.0, 1.0]).equations(2, |v: &[f64]| {
        Ok(vec![2.0 * v[0] + 7.0 * v[1] - 7.0, 3.0 * v[0] - 4.0 * v[1]])
    });
    let sol = solve(&p);
    assert_vec_close(&sol.x, &[28.0 / 29.0, 21.0 / 29.0], 1e-12);
    assert!(sol.residual < 1e-12);
}

#[test]
fn three_by_three_linear_system() {
    // A = [[2,1,-1],[-3,-1,2],[-2,1,2]], b = [8,-11,-3]; solution (2, 3, -1) checked by hand.
    let a = [[2.0, 1.0, -1.0], [-3.0, -1.0, 2.0], [-2.0, 1.0, 2.0]];
    let b = [8.0, -11.0, -3.0];
    let p = Problem::new(vec![0.0, 0.0, 0.0]).equations(3, move |v: &[f64]| {
        Ok((0..3)
            .map(|i| a[i].iter().zip(v).map(|(c, x)| c * x).sum::<f64>() - b[i])
            .collect())
    });
    let sol = solve(&p);
    assert_vec_close(&sol.x, &[2.0, 3.0, -1.0], 1e-12);
}

#[test]
fn linear_result_does_not_depend_on_guess() {
    let f = |v: &[f64]| Ok(vec![2.0 * v[0] + 7.0 * v[1] - 7.0, 3.0 * v[0] - 4.0 * v[1]]);
    let a = find(&Problem::new(vec![1.0, 1.0]).equations(2, f)).unwrap();
    let b = find(&Problem::new(vec![-50.0, 80.0]).equations(2, f)).unwrap();
    assert_vec_close(&a.x, &b.x, 1e-12);
}

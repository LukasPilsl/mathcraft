//! Kernel tests for `relax` and `multigrid` (see `pde_grid.rs`). The expected values are closed
//! forms or the discrete equations themselves.

use super::*;

fn free(_: u64) -> Result<(), EvalError> {
    Ok(())
}

/// The largest residual of `neighbours - 4 u = b` at the interior points.
fn stencil_residual(n: usize, u: &[f64], b: &[f64]) -> f64 {
    let mut worst = 0.0f64;
    for i in 1..n - 1 {
        for j in 1..n - 1 {
            let p = i * n + j;
            let lap = u[p - n] + u[p + n] + u[p - 1] + u[p + 1] - 4.0 * u[p];
            worst = worst.max((lap - b[p]).abs());
        }
    }
    worst
}

#[test]
fn relax_keeps_a_constant_harmonic_field() {
    // Laplace's equation with all edges at 1: the solution is 1 everywhere.
    let n = 9;
    let ones = vec![1.0; n * n];
    let zero = vec![0.0; n * n];
    let mut u = vec![0.0; n * n];
    for (k, value) in u.iter_mut().enumerate() {
        let (i, j) = (k / n, k % n);
        if i == 0 || j == 0 || i == n - 1 || j == n - 1 {
            *value = 1.0;
        }
    }
    let e = vec![-4.0; n * n];
    let st = Stencil {
        a: &ones,
        b: &ones,
        c: &ones,
        d: &ones,
        e: &e,
    };
    let out = relax(n, &st, &zero, u, 0.9, &free).expect("converges");
    for v in out {
        assert!((v - 1.0).abs() < 1e-8, "value {v}");
    }
}

#[test]
fn relax_solves_the_discrete_equation_with_a_source() {
    let n = 17;
    let ones = vec![1.0; n * n];
    let e = vec![-4.0; n * n];
    let s: Vec<f64> = (0..n * n).map(|k| 0.05 * ((k % 7) as f64 - 3.0)).collect();
    let r = 1.0 - 2.0 * std::f64::consts::PI / n as f64;
    let st = Stencil {
        a: &ones,
        b: &ones,
        c: &ones,
        d: &ones,
        e: &e,
    };
    let u = relax(n, &st, &s, vec![0.0; n * n], r, &free).expect("converges");
    assert!(stencil_residual(n, &u, &s) < 1e-6);
}

#[test]
fn relax_refuses_too_many_iterations_with_the_help_message() {
    let n = 33;
    let ones = vec![1.0; n * n];
    let e = vec![-4.0; n * n];
    let s = vec![1.0; n * n];
    let st = Stencil {
        a: &ones,
        b: &ones,
        c: &ones,
        d: &ones,
        e: &e,
    };
    // A spectral radius this close to 1 needs far more sweeps than the limit.
    let err = relax(n, &st, &s, vec![0.0; n * n], 0.999_999_99, &free).unwrap_err();
    assert!(matches!(err, EvalError::NoConvergence(m) if m.contains("too many iterations")));
}

#[test]
fn multigrid_reproduces_a_discrete_eigenfunction() {
    // u = sin(pi i h) sin(pi j h) satisfies the discrete Poisson equation with the source
    // -lambda u, lambda = 4 (1 - cos(pi h)) / h^2, and zero edges.
    let n = 33;
    let h = 1.0 / (n - 1) as f64;
    let lambda = 4.0 * (1.0 - (std::f64::consts::PI * h).cos()) / (h * h);
    let exact: Vec<f64> = (0..n * n)
        .map(|k| {
            let (i, j) = (k / n, k % n);
            (std::f64::consts::PI * i as f64 * h).sin()
                * (std::f64::consts::PI * j as f64 * h).sin()
        })
        .collect();
    let source: Vec<f64> = exact.iter().map(|u| -lambda * u).collect();
    let out = multigrid(n, &source, 2, &free).expect("solves");
    let worst = out
        .iter()
        .zip(&exact)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f64, f64::max);
    assert!(worst < 1e-4, "worst error {worst}");
}

#[test]
fn multigrid_two_cycles_are_close_to_the_converged_solution() {
    // The help's heat-flow example: a point source of 1545 on a 33 x 33 grid. The peak is about
    // -1 (the help's colour bar starts at -0.98); two cycles per level agree with twelve to 1e-3
    // of the peak, and the twelve-cycle result satisfies the discrete equation.
    let n = 33;
    let mut source = vec![0.0; n * n];
    source[14 * n + 8] = 1545.0;
    let two = multigrid(n, &source, 2, &free).expect("solves");
    let twelve = multigrid(n, &source, 12, &free).expect("solves");
    let peak = twelve.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    assert!((0.8..1.2).contains(&peak), "peak {peak}");
    let worst = two
        .iter()
        .zip(&twelve)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f64, f64::max);
    assert!(worst < 1e-3 * peak, "two cycles differ by {worst}");
    let h = 1.0 / (n - 1) as f64;
    let scaled: Vec<f64> = source.iter().map(|s| s * h * h).collect();
    assert!(stencil_residual(n, &twelve, &scaled) < 1e-6 * peak);
}

#[test]
fn restriction_and_prolongation_preserve_constants_in_the_interior() {
    let n = 9;
    let ones = vec![1.0; n * n];
    let coarse = restrict(n, &ones, 1.0);
    let m = n / 2 + 1;
    for i in 1..m - 1 {
        for j in 1..m - 1 {
            assert!((coarse[i * m + j] - 1.0).abs() < 1e-12);
        }
    }
    let fine = prolong(m, &coarse);
    assert_eq!(fine.len(), n * n);
    assert!((fine[4 * n + 4] - 1.0).abs() < 1e-12);
}

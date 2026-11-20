//! Function-argument builtins (see `special_forms`): root finding, polynomial roots, the fits that
//! take a function, the fixed-form fits, and the single-line ODE solvers.

use crate::eval::{EvalError, Outcome, Session, Value};
use crate::expr::Expr;
use crate::parser::{parse, DefaultIdContext};

fn stmt(src: &str) -> Expr {
    parse(src, &DefaultIdContext).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

/// Runs the statements in order on a fresh worksheet; returns the outcome of the last.
fn run_text(lines: &[&str]) -> Outcome {
    let mut s = Session::new();
    let mut last = None;
    for line in lines {
        last = Some(s.run(&stmt(line)));
    }
    last.expect("at least one statement")
}

fn real_of(out: Outcome) -> f64 {
    match out {
        Outcome::Value(Value::Scalar(s)) => {
            assert!(s.value.im.abs() < 1e-12, "complex {:?}", s.value);
            s.value.re
        }
        other => panic!("expected a scalar, got {other:?}"),
    }
}

/// The real parts of a column (or of a matrix in row-major order).
fn reals_of(out: Outcome) -> Vec<f64> {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.data().iter().map(|s| s.value.re).collect(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn shape_of(out: Outcome) -> (usize, usize) {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.shape(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn assert_close(got: &[f64], want: &[f64], tol: f64) {
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= tol, "{got:?} != {want:?} (tol {tol})");
    }
}

fn is_error(out: &Outcome) -> bool {
    matches!(out, Outcome::Error(_))
}

// ----- root -------------------------------------------------------------------------------------

#[test]
fn root_of_an_expression_in_a_bracket() {
    // Help: root(f(x), x, [a, b]) finds the root of f in [a, b].
    let got = real_of(run_text(&["root(x^2 - 2, x, [0, 2])="]));
    assert!((got - 2f64.sqrt()).abs() < 1e-12, "{got}");
}

#[test]
fn root_of_a_defined_function() {
    let got = real_of(run_text(&["f(x) := cos(x) - x", "root(f(x), x, [0, 1])="]));
    assert!((got - 0.739_085_133_215_160_6).abs() < 1e-12, "{got}");
}

#[test]
fn root_refuses_an_interval_without_a_sign_change() {
    let out = run_text(&["root(x^2 + 1, x, [-1, 1])="]);
    assert!(
        matches!(out, Outcome::Error(EvalError::NoSignChange(_))),
        "{out:?}"
    );
}

#[test]
fn root_refuses_a_pole_as_a_root() {
    // 1/x changes sign at 0, but there is no root there. Brent may probe 0 itself, where the
    // function is undefined (an error), or converge to the pole (refused): either is an error,
    // never a value.
    let out = run_text(&["root(1/x, x, [-1, 1])="]);
    assert!(is_error(&out), "{out:?}");
}

#[test]
fn root_refuses_a_jump_as_a_root() {
    // The step has a sign change at 0 and no zero: refused.
    let out = run_text(&["root(if(x < 0, -1, 1), x, [-1, 1])="]);
    assert!(is_error(&out), "{out:?}");
}

#[test]
fn root_uses_the_worksheet_value_of_other_names() {
    let got = real_of(run_text(&["a := 2", "root(x^2 - a, x, [0, 5])="]));
    assert!((got - 2f64.sqrt()).abs() < 1e-12, "{got}");
}

#[test]
fn nested_roots_inside_a_function_argument() {
    // g(x) = sqrt(x) (the root in y), and the outer root solves sqrt(x) = 3.
    let got = real_of(run_text(&[
        "g(x) := root(y^2 - x, y, [0, 10])",
        "root(g(x) - 3, x, [0, 10])=",
    ]));
    assert!((got - 9.0).abs() < 1e-9, "{got}");
}

#[test]
fn a_function_that_recurses_through_root_is_refused_not_a_panic() {
    let out = run_text(&["h(x) := root(h(x) - x, x, [0, 1])", "h(0.5)="]);
    assert!(is_error(&out), "{out:?}");
}

#[test]
fn two_argument_root_is_still_the_nth_root() {
    assert!((real_of(run_text(&["root(4, 2)="])) - 2.0).abs() < 1e-15);
}

#[test]
fn two_argument_root_with_a_variable_degree_is_the_nth_root() {
    // The first argument does not use n, so root(27, n) is the cube root.
    assert!((real_of(run_text(&["n := 3", "root(27, n)="])) - 3.0).abs() < 1e-12);
}

#[test]
fn root_guess_form_finds_the_root_near_the_guess() {
    // Help: root(f(x), x) starts from the guess value of x.
    let got = real_of(run_text(&["x := 1", "root(x^2 - 2, x)="]));
    assert!((got - 2f64.sqrt()).abs() < 1e-12, "{got}");
}

#[test]
fn root_guess_form_with_a_transcendental_function() {
    let got = real_of(run_text(&["x := 0.5", "root(cos(x) - x, x)="]));
    assert!((got - 0.739_085_133_215_160_6).abs() < 1e-10, "{got}");
}

#[test]
fn root_guess_form_on_a_defined_function() {
    let got = real_of(run_text(&[
        "f(x) := x^3 - 2*x - 5",
        "x := 2",
        "root(f(x), x)=",
    ]));
    assert!((got - 2.094_551_481_542_326_6).abs() < 1e-10, "{got}");
}

#[test]
fn root_guess_form_refuses_a_guess_with_no_root_in_reach() {
    // 1/x has no root: the secant walks away from the guess and is refused, never returned.
    let out = run_text(&["x := 0.5", "root(1/x, x)="]);
    assert!(
        matches!(out, Outcome::Error(_)),
        "expected a refusal, got {out:?}"
    );
}

#[test]
fn root_guess_form_needs_a_guess_value() {
    let out = run_text(&["root(x^2 - 2, x)="]);
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
}

// ----- polyroots --------------------------------------------------------------------------------

#[test]
fn polyroots_of_a_quadratic_are_sorted() {
    // Coefficients are constant term first: x^2 - 2.
    let got = reals_of(run_text(&["v := [-2; 0; 1]", "polyroots(v)="]));
    assert_close(&got, &[-2f64.sqrt(), 2f64.sqrt()], 1e-12);
}

#[test]
fn polyroots_of_a_cubic_with_three_real_roots() {
    // (x - 1)(x - 2)(x - 3) = x^3 - 6x^2 + 11x - 6.
    let got = reals_of(run_text(&["v := [-6; 11; -6; 1]", "polyroots(v)="]));
    assert_close(&got, &[1.0, 2.0, 3.0], 1e-10);
}

#[test]
fn polyroots_of_a_complex_pair() {
    // x^2 + 1: roots +-i; the real parts are zero.
    let out = run_text(&["v := [1; 0; 1]", "polyroots(v)="]);
    match out {
        Outcome::Value(Value::Matrix(m)) => {
            assert_eq!(m.rows(), 2);
            let imags: Vec<f64> = m.data().iter().map(|s| s.value.im.abs()).collect();
            assert_close(&imags, &[1.0, 1.0], 1e-12);
        }
        other => panic!("{other:?}"),
    }
}

// ----- linfit -----------------------------------------------------------------------------------

#[test]
fn linfit_recovers_an_exact_line() {
    // y = 2x + 1 on five points; basis [1; x].
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4]",
        "vy := 2*vx + 1",
        "F(x) := [1; x]",
        "linfit(vx, vy, F)=",
    ]));
    assert_close(&got, &[1.0, 2.0], 1e-10);
}

#[test]
fn linfit_recovers_an_exact_quadratic() {
    // y = x^2 - x + 3 with basis [1; x; x^2].
    let got = reals_of(run_text(&[
        "vx := [-2; -1; 0; 1; 2; 3]",
        "vy := vx^2 - vx + 3",
        "F(x) := [1; x; x^2]",
        "linfit(vx, vy, F)=",
    ]));
    assert_close(&got, &[3.0, -1.0, 1.0], 1e-9);
}

#[test]
fn linfit_refuses_a_basis_with_the_wrong_arity() {
    let out = run_text(&[
        "vx := [0; 1; 2]",
        "vy := vx",
        "F(x, y) := [1; x]",
        "linfit(vx, vy, F)=",
    ]);
    assert!(is_error(&out), "{out:?}");
}

// ----- genfit -----------------------------------------------------------------------------------

#[test]
fn genfit_vector_form_recovers_an_exponential() {
    // y = 2 e^(-0.5 x), F(x, u) := u_0 e^(u_1 x) with the parameters as the vector u.
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4; 5]",
        "vy := 2*exp(-0.5*vx)",
        "g := [1; -0.3]",
        "F(x, u) := u_0*exp(u_1*x)",
        "genfit(vx, vy, g, F)=",
    ]));
    assert_close(&got, &[2.0, -0.5], 1e-9);
}

#[test]
fn genfit_named_parameter_form_recovers_a_power_law() {
    // y = 2 x^1.5 with F(x, A, c) := A x^c (one name per parameter).
    let got = reals_of(run_text(&[
        "vx := [1; 2; 3; 4; 5; 6]",
        "vy := 2*vx^1.5",
        "g := [1; 1]",
        "F(x, A, c) := A*x^c",
        "genfit(vx, vy, g, F)=",
    ]));
    assert_close(&got, &[2.0, 1.5], 1e-9);
}

#[test]
fn genfit_refuses_a_model_with_the_wrong_parameter_count() {
    let out = run_text(&[
        "vx := [1; 2; 3]",
        "vy := vx",
        "g := [1; 1]",
        "F(x, A) := A*x",
        "genfit(vx, vy, g, F)=",
    ]);
    // The vector form takes F(x, u) with a 2-vector: a scalar F(x, A) with a 2-vector guess is
    // not a model of two parameters, so it is refused.
    assert!(is_error(&out), "{out:?}");
}

// ----- fixed-form fits --------------------------------------------------------------------------

#[test]
fn expfit_recovers_an_exponential_without_a_guess() {
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4; 5]",
        "vy := 3*exp(0.4*vx) + 1",
        "expfit(vx, vy)=",
    ]));
    assert_close(&got, &[3.0, 0.4, 1.0], 1e-9);
}

#[test]
fn lnfit_is_exact_on_a_logarithm() {
    let got = reals_of(run_text(&[
        "vx := [1; 2; 3; 4; 5]",
        "vy := 2*ln(vx) + 5",
        "lnfit(vx, vy)=",
    ]));
    assert_close(&got, &[2.0, 5.0], 1e-10);
}

#[test]
fn logfit_recovers_a_shifted_logarithm() {
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4; 5]",
        "vy := 2*ln(vx + 1) + 3",
        "g := [1; 1; 1]",
        "logfit(vx, vy, g)=",
    ]));
    assert_close(&got, &[2.0, 1.0, 3.0], 1e-9);
}

#[test]
fn pwrfit_recovers_a_power_curve() {
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4; 5]",
        "vy := 2*vx^1.5 + 1",
        "g := [1; 1; 0]",
        "pwrfit(vx, vy, g)=",
    ]));
    assert_close(&got, &[2.0, 1.5, 1.0], 1e-9);
}

#[test]
fn sinfit_recovers_a_sine_near_its_guess() {
    let got = reals_of(run_text(&[
        "vx := [0; 0.5; 1; 1.5; 2; 2.5; 3; 3.5; 4; 4.5]",
        "vy := 2*sin(vx + 0.5) + 1",
        "g := [1.5; 0.4; 0.8]",
        "sinfit(vx, vy, g)=",
    ]));
    assert_close(&got, &[2.0, 0.5, 1.0], 1e-9);
}

#[test]
fn lgsfit_recovers_a_logistic_curve() {
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4; 5; 6]",
        "vy := 4/(1 + exp(-0.8*vx))",
        "g := [3; 0.5; 0.5]",
        "lgsfit(vx, vy, g)=",
    ]));
    assert_close(&got, &[4.0, 1.0, 0.8], 1e-9);
}

#[test]
fn a_fit_that_cannot_converge_is_an_error() {
    // Noise-free data that no sine fits: a constant-valued set with a sine guess must not give a
    // plausible value; it is refused or it returns the constant fit, never a non-finite one.
    let out = run_text(&[
        "vx := [0; 1; 2; 3]",
        "vy := [1; 1; 1; 1]",
        "g := [0; 0; 0]",
        "pwrfit(vx, vy, g)=",
    ]);
    match out {
        Outcome::Value(Value::Matrix(m)) => {
            assert!(m.data().iter().all(|s| s.value.re.is_finite()));
        }
        Outcome::Error(_) => {}
        other => panic!("{other:?}"),
    }
}

// ----- rkfixed and Rkadapt ----------------------------------------------------------------------

#[test]
fn rkfixed_returns_the_grid_and_the_solution() {
    // y' = y, y(0) = 1: the last row is (1, e).
    let out = run_text(&["D(x, y) := y", "rkfixed(1, 0, 1, 10, D)="]);
    assert_eq!(shape_of(out.clone()), (11, 2));
    let data = reals_of(out);
    assert!((data[0] - 0.0).abs() < 1e-15 && (data[1] - 1.0).abs() < 1e-15);
    let last = data[data.len() - 1];
    // One RK4 step per interval (Mathcad's rkfixed, see `ode.rs`): the error here is about 2e-6, where
    // the old eight substeps per interval gave 1e-8.
    assert!((last - std::f64::consts::E).abs() < 1e-5, "{last}");
    let x_last = data[data.len() - 2];
    assert!((x_last - 1.0).abs() < 1e-15, "{x_last}");
}

#[test]
fn rkfixed_solves_a_system_with_components() {
    // y0' = y1, y1' = -y0 with y(0) = (0, 1): y0 = sin x.
    let out = run_text(&["D(x, y) := [y_1; -y_0]", "rkfixed([0; 1], 0, 1, 10, D)="]);
    assert_eq!(shape_of(out.clone()), (11, 3));
    let data = reals_of(out);
    let row_at_one = &data[10 * 3..11 * 3];
    // One RK4 step per interval: the error is about 1e-6 (eight substeps gave about 1e-9).
    assert!((row_at_one[1] - 1f64.sin()).abs() < 1e-5, "{row_at_one:?}");
    assert!((row_at_one[2] - 1f64.cos()).abs() < 1e-5, "{row_at_one:?}");
}

#[test]
fn rkadapt_meets_the_tolerance_set_by_tol() {
    let out = run_text(&[
        "TOL := 1e-9",
        "D(x, y) := [y_1; -y_0]",
        "Rkadapt([0; 1], 0, 3, 6, D)=",
    ]);
    assert_eq!(shape_of(out.clone()), (7, 3));
    let data = reals_of(out);
    // Rows are [x, y0, y1].
    let last = &data[6 * 3..7 * 3];
    assert!((last[0] - 3.0).abs() < 1e-15, "{last:?}");
    assert!((last[1] - 3f64.sin()).abs() < 1e-6, "{last:?}");
    assert!((last[2] - 3f64.cos()).abs() < 1e-6, "{last:?}");
}

#[test]
fn stiffr_solves_linear_decay_with_its_augmented_jacobian() {
    // y' = -y, y(0) = 1; AJ = (df/dx, df/dy) = (0, -1).
    let out = run_text(&[
        "TOL := 1e-8",
        "D(x, y) := -y",
        "AJ(x, y) := [0, -1]",
        "Stiffr(1, 0, 1, 10, D, AJ)=",
    ]);
    assert_eq!(shape_of(out.clone()), (11, 2));
    let data = reals_of(out);
    let last = data[data.len() - 1];
    // ROS2 is second order: the global error is not bounded by TOL (see the module docs of
    // `ode_stiff.rs`); the bound below is loose.
    assert!((last - (-1f64).exp()).abs() < 1e-5, "{last}");
}

#[test]
fn stiffb_gives_the_stiffr_solution() {
    // The help's Stiffb and Stiffr example shows identical solutions, so Stiffb runs the same
    // integrator: the two outputs match exactly and are close to exp(-x).
    let out_b = run_text(&[
        "TOL := 1e-8",
        "D(x, y) := -y",
        "AJ(x, y) := [0, -1]",
        "Stiffb(1, 0, 1, 10, D, AJ)=",
    ]);
    let out_r = run_text(&[
        "TOL := 1e-8",
        "D(x, y) := -y",
        "AJ(x, y) := [0, -1]",
        "Stiffr(1, 0, 1, 10, D, AJ)=",
    ]);
    assert_eq!(shape_of(out_b.clone()), (11, 2));
    let (b, r) = (reals_of(out_b), reals_of(out_r));
    assert_eq!(b, r);
    let last = b[b.len() - 1];
    assert!((last - (-1f64).exp()).abs() < 1e-5, "{last}");
}

#[test]
fn stiffr_global_error_accumulates_over_steps() {
    // y' = -y, y(0) = 1 at x = 1 with TOL 1e-6: TOL bounds each step's error, not the global error,
    // which accumulates over the steps to about 20 TOL here (2e-5).
    let out = run_text(&[
        "TOL := 1e-6",
        "D(x, y) := -y",
        "AJ(x, y) := [0, -1]",
        "Stiffr(1, 0, 1, 10, D, AJ)=",
    ]);
    let data = reals_of(out);
    let last = data[data.len() - 1];
    let err = (last - (-1f64).exp()).abs();
    assert!(err < 100.0 * 1e-6, "global error {err}");
}

#[test]
fn stiffr_oscillator_global_error_is_measured() {
    // y0' = y1, y1' = -y0 on [0, 10] with 50 intervals from (0, 1): the max error is 3.1e-4 at
    // TOL 1e-6 and 1.4e-5 at TOL 1e-8, about TOL^(2/3) and far above TOL (see the module docs).
    for (tol, lo, hi) in [("1e-6", 1e-4, 1e-3), ("1e-8", 1e-6, 1e-4)] {
        let tol_line = format!("TOL := {tol}");
        let out = run_text(&[
            tol_line.as_str(),
            "D(x, y) := [y_1; -y_0]",
            "AJ(x, y) := [0, 0, 1; 0, -1, 0]",
            "Stiffr([0; 1], 0, 10, 50, D, AJ)=",
        ]);
        let data = reals_of(out);
        let n = data.len();
        let err = (data[n - 2] - 10f64.sin())
            .abs()
            .max((data[n - 1] - 10f64.cos()).abs());
        assert!(err > lo && err < hi, "TOL {tol}: {err}");
    }
}

#[test]
fn stiffr_solves_robertson_with_the_default_tol() {
    // Robertson at the built-in TOL (0.001): the solve must finish, near the help's values at x = 40.
    // (Before the error scale floor, see `ode_stiff.rs`, the steps diverged and the solve was refused.)
    let out = run_text(&[
        "D(x, y) := [-0.04*y_0 + 1e4*y_1*y_2; 0.04*y_0 - 1e4*y_1*y_2 - 3e7*y_1^2; 3e7*y_1^2]",
        "AJ(x, y) := [0, -0.04, 1e4*y_2, 1e4*y_1; 0, 0.04, -1e4*y_2 - 6e7*y_1, -1e4*y_1; 0, 0, 6e7*y_1, 0]",
        "Stiffr([1; 0; 0], 0, 40, 4, D, AJ)=",
    ]);
    let data = reals_of(out);
    let last = &data[4 * 4..5 * 4];
    assert!((last[1] - 0.7158).abs() < 2e-3, "{last:?}");
    assert!((last[2] - 9.185e-6).abs() < 2e-6, "{last:?}");
    assert!((last[3] - 0.2842).abs() < 2e-3, "{last:?}");
}

#[test]
fn stiffr_solves_robertson_within_its_budget() {
    // Robertson's stiff kinetics, y(0) = (1, 0, 0). Reference at x = 40 (Hairer & Wanner):
    // y0 = 0.7158, y1 = 9.185e-6, y2 = 0.2842. The sum y0 + y1 + y2 is 1 exactly.
    let out = run_text(&[
        "TOL := 1e-6",
        "D(x, y) := [-0.04*y_0 + 1e4*y_1*y_2; 0.04*y_0 - 1e4*y_1*y_2 - 3e7*y_1^2; 3e7*y_1^2]",
        "AJ(x, y) := [0, -0.04, 1e4*y_2, 1e4*y_1; 0, 0.04, -1e4*y_2 - 6e7*y_1, -1e4*y_1; 0, 0, 6e7*y_1, 0]",
        "Stiffr([1; 0; 0], 0, 40, 4, D, AJ)=",
    ]);
    assert_eq!(shape_of(out.clone()), (5, 4));
    let data = reals_of(out);
    let last = &data[4 * 4..5 * 4];
    assert!((last[1] - 0.7158).abs() < 2e-3, "{last:?}");
    assert!((last[2] - 9.185e-6).abs() < 2e-6, "{last:?}");
    assert!((last[3] - 0.2842).abs() < 2e-3, "{last:?}");
}

#[test]
fn stiffr_van_der_pol_mu_1000_is_solved_or_refused_cleanly() {
    // Stiff (mu = 1000): either the solve finishes or the budget or step cap refuses it. Never a
    // panic or a hang.
    let out = run_text(&[
        "TOL := 1e-4",
        "mu := 1000",
        "D(x, y) := [y_1; mu*(1 - y_0^2)*y_1 - y_0]",
        "AJ(x, y) := [0, 0, 1; 0, -2*mu*y_0*y_1 - 1, mu*(1 - y_0^2)]",
        "Stiffr([2; 0], 0, 3000, 10, D, AJ)=",
    ]);
    assert!(
        matches!(out, Outcome::Value(_))
            || matches!(
                out,
                Outcome::Error(EvalError::WorkLimit(_) | EvalError::NoConvergence(_))
            ),
        "{out:?}"
    );
}

#[test]
fn a_huge_solve_is_refused_by_the_budget() {
    // A million output intervals of RK4 is far past the budget: an error, not a hang.
    let out = run_text(&["D(x, y) := y", "rkfixed(1, 0, 1, 1000000, D)="]);
    assert!(
        matches!(out, Outcome::Error(EvalError::WorkLimit(_))),
        "{out:?}"
    );
}

#[test]
fn a_solver_arity_mistake_is_an_error() {
    let out = run_text(&["D(x, y) := y", "rkfixed(1, 0, 1, D)="]);
    assert!(is_error(&out), "{out:?}");
}

#[test]
fn a_form_inside_a_range_statement_is_evaluated_per_value() {
    // t runs over 0, 1, 2: the root of x^2 - t - 1 is sqrt(1 + t) for each.
    let got = reals_of(run_text(&["t := 0 .. 2", "root(x^2 - t - 1, x, [0, 5])="]));
    assert_close(&got, &[1.0, 2f64.sqrt(), 3f64.sqrt()], 1e-12);
}

// ----- review fixes -----------------------------------------------------------------------------

/// Runs owned statement lines in order; returns the outcome of the last.
fn run_owned(lines: &[String]) -> Outcome {
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    run_text(&refs)
}

/// The coefficients (constant term first) of `prod (x - r)` over `roots`, as worksheet literal text.
/// `{:e}` prints the shortest exact form, so no digit of a coefficient is lost.
fn poly_literal(roots: &[f64]) -> String {
    let mut c = vec![1.0_f64];
    for r in roots {
        let mut next = vec![0.0; c.len() + 1];
        for (k, ck) in c.iter().enumerate() {
            next[k + 1] += ck;
            next[k] -= r * ck;
        }
        c = next;
    }
    let items: Vec<String> = c.iter().map(|v| format!("{v:e}")).collect();
    format!("[{}]", items.join("; "))
}

/// The real parts of the roots `polyroots` returns for the polynomial with these roots.
fn polyroots_of(roots: &[f64]) -> Vec<f64> {
    reals_of(run_owned(&[
        format!("v := {}", poly_literal(roots)),
        "polyroots(v)=".to_string(),
    ]))
}

// Fit accuracy: exact data comes back to 1e-9 (the tolerances above).

#[test]
fn logfit_is_exact_on_exact_data_from_a_rough_guess() {
    // Regression: Levenberg-Marquardt used to stop early on a small gain (about 2e-6 off).
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4; 5]",
        "vy := 3 + 2*ln(vx + 1)",
        "g := [1; 1; 1]",
        "logfit(vx, vy, g)=",
    ]));
    assert_close(&got, &[2.0, 1.0, 3.0], 1e-9);
}

#[test]
fn expfit_recovers_negative_data_without_a_guess() {
    // y = -3 e^(0.4 x): the logarithms exist only for -y, which the fit starts from.
    let got = reals_of(run_text(&[
        "vx := [0; 1; 2; 3; 4; 5]",
        "vy := -3*exp(0.4*vx)",
        "expfit(vx, vy)=",
    ]));
    assert_close(&got, &[-3.0, 0.4, 0.0], 1e-9);
}

// polyroots

#[test]
fn polyroots_of_roots_k_over_10_up_to_degree_8() {
    let roots: Vec<f64> = (1..=8).map(|k| k as f64 / 10.0).collect();
    assert_close(&polyroots_of(&roots), &roots, 1e-9);
}

#[test]
fn polyroots_of_roots_k_over_10_up_to_degree_20() {
    // The tolerance is the conditioning of the f64 coefficients, which grows with the degree:
    // about 1e-7 at degree 15, about 5e-4 at 20. The roots are returned, not refused.
    for n in (10..=20).step_by(2) {
        let roots: Vec<f64> = (1..=n).map(|k| k as f64 / 10.0).collect();
        let tol = if n <= 14 { 1e-6 } else { 1e-3 };
        assert_close(&polyroots_of(&roots), &roots, tol);
    }
}

#[test]
fn polyroots_of_wilkinson_like_roots_one_to_twelve() {
    let roots: Vec<f64> = (1..=12).map(f64::from).collect();
    assert_close(&polyroots_of(&roots), &roots, 1e-6);
}

#[test]
fn polyroots_of_a_double_root_is_returned() {
    // (x - 1)^2 = x^2 - 2x + 1: the pair at 1.
    let got = reals_of(run_text(&["v := [1; -2; 1]", "polyroots(v)="]));
    assert_close(&got, &[1.0, 1.0], 1e-6);
}

#[test]
fn polyroots_refuses_a_triple_root_instead_of_noisy_values() {
    // (x - 1)^3: the iterates cluster around 1; the cluster is refused.
    let out = run_text(&["v := [-1; 3; -3; 1]", "polyroots(v)="]);
    assert!(is_error(&out), "{out:?}");
}

// root

#[test]
fn root_refuses_a_reversed_interval() {
    // The help requires a < b.
    let out = run_text(&["root(x^2 - 2, x, [2, 0])="]);
    assert!(
        matches!(out, Outcome::Error(EvalError::Unsupported(_))),
        "{out:?}"
    );
}

#[test]
fn root_reports_a_tangent_pole_as_a_pole() {
    // tan changes sign at pi/2 with no root there: the pole message, not "not finite".
    let out = run_text(&["root(tan(x), x, [1, 2])="]);
    assert!(
        matches!(out, Outcome::Error(EvalError::NoSignChange(_))),
        "{out:?}"
    );
}

// linfit with range data

#[test]
fn linfit_accepts_range_variables_as_vectors() {
    // vx is a range (0..4), read as the vector vec(vx) by the fit.
    let got = reals_of(run_text(&[
        "vx := 0..4",
        "vy := 2*vec(vx) + 1",
        "F(x) := [1; x]",
        "linfit(vx, vy, F)=",
    ]));
    assert_close(&got, &[1.0, 2.0], 1e-10);
}

// user definitions shadow the forms

#[test]
fn a_user_function_named_like_a_form_is_the_one_called() {
    // The worksheet's own definition wins over the builtin form of the same name.
    let got = real_of(run_text(&["linfit(a, b, c) := 42", "linfit(1, 2, 3)="]));
    assert_eq!(got, 42.0);
}

// rkfixed budget

#[test]
fn rkfixed_accepts_fifty_thousand_intervals() {
    // Each derivative evaluation is charged as work, not as a user call: the call count no longer
    // caps the steps (1e6 calls used to allow only about 31k intervals).
    let out = run_text(&["D(x, y) := y", "rkfixed(1, 0, 1, 50000, D)="]);
    assert_eq!(shape_of(out), (50001, 2));
}

#[test]
fn a_recursive_derivative_through_a_solver_is_refused_not_a_crash() {
    // D calls rkfixed, which calls D: refused as an error on the test thread's stack, never a
    // stack overflow.
    let out = run_text(&[
        "D(x, y) := rkfixed(y, 0, 1, 2, D)",
        "rkfixed(1, 0, 1, 2, D)=",
    ]);
    assert!(is_error(&out), "{out:?}");
}

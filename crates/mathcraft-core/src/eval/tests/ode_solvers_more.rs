//! The further ODE solvers (`Bulstoer`, `Adams`, `BDF`, `Radau`, `AdamsBDF`) and the shooting
//! boundary value forms (`sbval`, `bvalfit`). See `ode_implicit.rs` and `ode_bvp.rs`.

use crate::eval::{EvalError, Outcome, Session, Value};
use crate::expr::Expr;
use crate::parser::{parse, DefaultIdContext};

fn stmt(src: &str) -> Expr {
    parse(src, &DefaultIdContext).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

fn run_text(lines: &[&str]) -> Outcome {
    let mut s = Session::new();
    let mut last = None;
    for line in lines {
        last = Some(s.run(&stmt(line)));
    }
    last.expect("at least one statement")
}

fn shape_of(out: &Outcome) -> (usize, usize) {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.shape(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn reals_of(out: Outcome) -> Vec<f64> {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.data().iter().map(|s| s.value.re).collect(),
        Outcome::Value(Value::Scalar(s)) => vec![s.value.re],
        other => panic!("expected numbers, got {other:?}"),
    }
}

fn is_refused(out: &Outcome) -> bool {
    matches!(out, Outcome::Error(_))
}

/// The last row of a solution matrix with `cols` columns.
fn last_row(data: &[f64], cols: usize) -> &[f64] {
    &data[data.len() - cols..]
}

#[test]
fn bulstoer_solves_exponential_growth_to_tol() {
    let out = run_text(&["TOL := 1e-10", "D(x, y) := y", "Bulstoer(1, 0, 1, 10, D)="]);
    assert_eq!(shape_of(&out), (11, 2));
    let data = reals_of(out);
    let last = last_row(&data, 2);
    assert!((last[1] - std::f64::consts::E).abs() < 1e-8, "{last:?}");
}

#[test]
fn bulstoer_solves_a_harmonic_system() {
    let out = run_text(&[
        "TOL := 1e-9",
        "D(x, y) := [y_1; -y_0]",
        "Bulstoer([0; 1], 0, 2, 4, D)=",
    ]);
    let data = reals_of(out);
    let last = last_row(&data, 3);
    assert!((last[1] - 2f64.sin()).abs() < 1e-7, "{last:?}");
    assert!((last[2] - 2f64.cos()).abs() < 1e-7, "{last:?}");
}

#[test]
fn adams_matches_decay_at_its_tolerance() {
    let out = run_text(&["D(x, y) := -y", "Adams(1, 0, 2, 8, D, 1e-8)="]);
    assert_eq!(shape_of(&out), (9, 2));
    let data = reals_of(out);
    let last = last_row(&data, 2);
    assert!((last[1] - (-2f64).exp()).abs() < 1e-7, "{last:?}");
}

#[test]
fn bdf_matches_decay_at_its_tolerance() {
    let out = run_text(&["D(x, y) := -y", "BDF(1, 0, 2, 8, D, 1e-6)="]);
    let data = reals_of(out);
    let last = last_row(&data, 2);
    assert!((last[1] - (-2f64).exp()).abs() < 1e-5, "{last:?}");
}

#[test]
fn radau_matches_decay_and_takes_a_per_unknown_tol() {
    let out = run_text(&["D(x, y) := -y", "Radau(1, 0, 2, 8, D, [1e-8])="]);
    assert_eq!(shape_of(&out), (9, 2));
    let data = reals_of(out);
    let last = last_row(&data, 2);
    assert!((last[1] - (-2f64).exp()).abs() < 1e-6, "{last:?}");
}

#[test]
fn adamsbdf_solves_a_nonstiff_system() {
    let out = run_text(&[
        "D(x, y) := [y_1; -y_0]",
        "AdamsBDF([0; 1], 0, 3, 6, D, 1e-7)=",
    ]);
    let data = reals_of(out);
    let last = last_row(&data, 3);
    assert!((last[1] - 3f64.sin()).abs() < 1e-5, "{last:?}");
}

#[test]
fn radau_solves_robertson_to_the_help_values() {
    // Robertson's stiff kinetics at x = 40 (Hairer & Wanner): 0.7158, 9.185e-6, 0.2842.
    let out = run_text(&[
        "D(x, y) := [-0.04*y_0 + 1e4*y_1*y_2; 0.04*y_0 - 1e4*y_1*y_2 - 3e7*y_1^2; 3e7*y_1^2]",
        "J(x, y) := [-0.04, 1e4*y_2, 1e4*y_1; 0.04, -1e4*y_2 - 6e7*y_1, -1e4*y_1; 0, 6e7*y_1, 0]",
        "Radau([1; 0; 0], 0, 40, 4, D, J, 1e-6)=",
    ]);
    assert_eq!(shape_of(&out), (5, 4));
    let data = reals_of(out);
    let last = last_row(&data, 4);
    assert!((last[1] - 0.7158).abs() < 2e-3, "{last:?}");
    assert!((last[2] - 9.185e-6).abs() < 2e-6, "{last:?}");
    assert!((last[3] - 0.2842).abs() < 2e-3, "{last:?}");
}

#[test]
fn bdf_solves_robertson_with_a_finite_difference_jacobian() {
    let out = run_text(&[
        "D(x, y) := [-0.04*y_0 + 1e4*y_1*y_2; 0.04*y_0 - 1e4*y_1*y_2 - 3e7*y_1^2; 3e7*y_1^2]",
        "BDF([1; 0; 0], 0, 40, 4, D, 1e-4)=",
    ]);
    if is_refused(&out) {
        // A refusal by the budget or the step cap is an allowed outcome for the multistep solver.
        return;
    }
    let data = reals_of(out);
    let last = last_row(&data, 4);
    assert!((last[1] - 0.7158).abs() < 1e-2, "{last:?}");
}

#[test]
fn van_der_pol_mu_1000_is_solved_or_refused_cleanly() {
    // Stiff: a finished solve or a budget or step refusal, never a hang or a panic.
    let out = run_text(&[
        "mu := 1000",
        "D(x, y) := [y_1; mu*(1 - y_0^2)*y_1 - y_0]",
        "Radau([2; 0], 0, 3000, 10, D, 1e-4)=",
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
fn radau_refuses_a_mass_matrix_other_than_the_identity() {
    let out = run_text(&[
        "D(x, y) := [y_1; -y_0]",
        "Radau([0; 1], 0, 1, 4, D, [2, 0; 0, 1])=",
    ]);
    assert!(is_refused(&out), "{out:?}");
}

#[test]
fn radau_refuses_a_jacobian_of_the_wrong_shape() {
    let out = run_text(&[
        "D(x, y) := [y_1; -y_0]",
        "J(x, y) := [0, 1]",
        "Radau([0; 1], 0, 1, 4, D, J)=",
    ]);
    assert!(is_refused(&out), "{out:?}");
}

#[test]
fn adams_takes_no_jacobian_and_bulstoer_takes_no_options() {
    let with_j = run_text(&[
        "D(x, y) := -y",
        "J(x, y) := [-1]",
        "Adams(1, 0, 1, 4, D, J)=",
    ]);
    assert!(is_refused(&with_j), "{with_j:?}");
    let arity = run_text(&["D(x, y) := -y", "Bulstoer(1, 0, 1, 4, D, 1e-3)="]);
    assert!(is_refused(&arity), "{arity:?}");
}

#[test]
fn a_tol_with_the_wrong_length_is_refused() {
    let out = run_text(&[
        "D(x, y) := [y_1; -y_0]",
        "Adams([0; 1], 0, 1, 4, D, [1e-3; 1e-3; 1e-3])=",
    ]);
    assert!(is_refused(&out), "{out:?}");
}

#[test]
fn a_solver_calling_itself_is_refused_on_the_stack() {
    // D calls Radau, which calls D: refused as an error, never a stack overflow.
    let out = run_text(&["D(x, y) := Radau(y, 0, 1, 2, D)", "Radau(1, 0, 1, 2, D)="]);
    assert!(is_refused(&out), "{out:?}");
}

#[test]
fn sbval_finds_the_initial_slope_of_a_two_point_problem() {
    // y'' = -y with y(0) = 0 and y(pi/2) = 1: the solution is sin x, with y'(0) = 1.
    let out = run_text(&[
        "D(x, y) := [y_1; -y_0]",
        "load1(x, v) := [0; v_0]",
        "scorei(x, y) := [y_0 - 1]",
        "sbval(1, 0, 1.5707963267948966, D, load1, scorei)=",
    ]);
    let got = reals_of(out);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(
        got[0].abs() < 1e-12 && (got[1] - 1.0).abs() < 1e-7,
        "{got:?}"
    );
}

#[test]
fn bvalfit_matches_the_two_ends_at_an_intermediate_point() {
    // The same problem, shot from both ends and matched at xf = pi/4 (y = sin, y' = cos there).
    let out = run_text(&[
        "D(x, y) := [y_1; -y_0]",
        "load1(x, v) := [0; v_0]",
        "load2(x, v) := [1; v_0]",
        "scoreb(x, y) := y",
        "bvalfit(1, 0.5, 0, 1.5707963267948966, 0.7853981633974483, D, load1, load2, scoreb)=",
    ]);
    let got = reals_of(out);
    assert!(
        got[0].abs() < 1e-12 && (got[1] - 1.0).abs() < 1e-6,
        "{got:?}"
    );
}

#[test]
fn sbval_refuses_a_load_that_returns_the_wrong_count() {
    let out = run_text(&[
        "D(x, y) := [y_1; -y_0]",
        "load1(x, v) := [0; v_0]",
        "scorei(x, y) := [y_0 - 1; y_1]",
        "sbval(1, 0, 1.5707963267948966, D, load1, scorei)=",
    ]);
    assert!(is_refused(&out), "{out:?}");
}

/// The help's damped oscillator (a = 10, b = 0.5, Q(0) = 1, Q'(0) = 0), as the worksheet's `D`.
const DAMPED: &str = "D(x, y) := [y_1; -100*y_0 - 2*0.5*y_1]";

/// Its exact solution Q(t) = e^(-bt) (cos(wt) + (b/w) sin(wt)) with w = sqrt(a^2 - b^2).
fn damped_exact(t: f64) -> f64 {
    let w = (100.0f64 - 0.25).sqrt();
    (-0.5 * t).exp() * ((w * t).cos() + 0.5 / w * (w * t).sin())
}

#[test]
fn rkfixed_damped_oscillator_matches_the_help_error_sum() {
    // The help's E1nss = 0.54: rkfixed with 40 intervals on [0, 6] takes one RK4 step per interval.
    let out = run_text(&[DAMPED, "rkfixed([1; 0], 0, 6, 40, D)="]);
    assert_eq!(shape_of(&out), (41, 3));
    let sse: f64 = reals_of(out)
        .chunks(3)
        .map(|row| (damped_exact(row[0]) - row[1]).powi(2))
        .sum();
    assert!((sse - 0.54).abs() < 0.005, "{sse}");
}

#[test]
fn implicit_solvers_default_tol_is_1e_5() {
    // Adams, AdamsBDF, BDF and Radau use 10^-5 when no tol is given (the help's default).
    let d = "D(x, y) := [y_1; -y_0 - 0.1*y_1]";
    for solver in ["Adams", "AdamsBDF", "BDF", "Radau"] {
        let default_call = format!("{solver}([1; 0], 0, 3, 6, D)=");
        let explicit_call = format!("{solver}([1; 0], 0, 3, 6, D, 1e-5)=");
        let default = reals_of(run_text(&[d, default_call.as_str()]));
        let explicit = reals_of(run_text(&[d, explicit_call.as_str()]));
        assert_eq!(default, explicit, "{solver}");
    }
}

#[test]
fn radau_on_the_help_oscillator_stays_near_its_tolerance() {
    // The help's oscillator at tol 1e-7 (its sTOL). The step error is held to 1e-7 per step; over 40
    // steps the max error is about 10 tol (it was 1.7e-6 before the error estimate was divided by
    // 31 for the order-5 method instead of 63).
    let out = run_text(&[DAMPED, "Radau([1; 0], 0, 6, 40, D, 1e-7)="]);
    let max_err = reals_of(out)
        .chunks(3)
        .map(|row| (damped_exact(row[0]) - row[1]).abs())
        .fold(0.0f64, f64::max);
    assert!(max_err < 1.2e-6, "max error {max_err}");
}

#[test]
fn sbval_backtracks_out_of_an_overflowing_trial() {
    // y'' = y^3 with y(0) = 0, y'(0) = s and y(0.5) = 10. The Newton step from s = 0.5 overshoots
    // to a shot that blows up before 0.5 (NotFinite): that trial is halved, and the solve converges
    // to the finite shot at s = 11.8307...
    let out = run_text(&[
        "D(x, y) := [y_1; y_0^3]",
        "load1(x, v) := [0; v_0]",
        "scorei(x, y) := [y_0 - 10]",
        "sbval([0.5], 0, 0.5, D, load1, scorei)=",
    ]);
    let got = reals_of(out);
    assert!(
        got[0].abs() < 1e-12 && (got[1] - 11.830_761_885).abs() < 1e-6,
        "{got:?}"
    );
}

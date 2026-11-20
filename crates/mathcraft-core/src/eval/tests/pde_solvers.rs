//! The state-space solver (`statespace`) and the method-of-lines solver (`numol`), checked against
//! closed-form solutions. See `ode_state.rs` and `pde_mol.rs`.

use crate::eval::{Outcome, Session, Value};
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

/// The shape and the row-major numbers of a matrix outcome.
fn matrix_of(out: Outcome) -> ((usize, usize), Vec<f64>) {
    match out {
        Outcome::Value(Value::Matrix(m)) => {
            (m.shape(), m.data().iter().map(|s| s.value.re).collect())
        }
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn is_refused(out: &Outcome) -> bool {
    matches!(out, Outcome::Error(_))
}

// ----- statespace ----------------------------------------------------------------------------

#[test]
fn statespace_overdamped_oscillator_matches_the_closed_form() {
    // The help's overdamped case: x0' = -w^2 x1, x1' = x0 - (b/m) x1 with w = 1/6, b/m = 1/2.
    // x1 satisfies x1'' + x1' / 2 + w^2 x1 = 0, with x1(0) = 1 and x1'(0) = -1/2.
    let out = run_text(&[
        "A(t) := [0, -1/36; 1, -1/2]",
        "statespace([0; 1], 0, 5, 50, A)=",
    ]);
    let ((rows, cols), data) = matrix_of(out);
    assert_eq!((rows, cols), (51, 3));
    let w2 = 1.0 / 36.0;
    let disc = (0.25f64 - 4.0 * w2).sqrt();
    let (r1, r2) = ((-0.5 + disc) / 2.0, (-0.5 - disc) / 2.0);
    // C1 + C2 = 1 and r1 C1 + r2 C2 = -1/2.
    let c1 = (-0.5 - r2) / (r1 - r2);
    let c2 = 1.0 - c1;
    let t = 5.0f64;
    let exact = c1 * (r1 * t).exp() + c2 * (r2 * t).exp();
    let last = data[50 * 3 + 2];
    assert!(
        (last - exact).abs() < 1e-6,
        "x1(5) = {last}, expected {exact}"
    );
    // The first column is the time grid, and its last entry is t2.
    assert!((data[50 * 3] - 5.0).abs() < 1e-12);
    assert!(data[0].abs() < 1e-12);
}

#[test]
fn statespace_with_an_input_follows_the_forced_oscillator() {
    // x0'' + x0 = 1 with x0(0) = x0'(0) = 0 gives x0 = 1 - cos t, so x0(pi) = 2.
    let out = run_text(&[
        "A(t) := [0, 1; -1, 0]",
        "B(t) := [0; 1]",
        "u(t) := 1",
        "statespace([0; 0], 0, 3.141592653589793, 100, A, B, u)=",
    ]);
    let ((rows, cols), data) = matrix_of(out);
    assert_eq!((rows, cols), (101, 3));
    assert!(
        (data[100 * 3 + 1] - 2.0).abs() < 1e-6,
        "x0(pi) = {}",
        data[100 * 3 + 1]
    );
}

#[test]
fn statespace_refuses_an_input_without_its_partner() {
    let out = run_text(&[
        "A(t) := [0, 1; -1, 0]",
        "B(t) := [0; 1]",
        "statespace([0; 0], 0, 1, 10, A, B)=",
    ]);
    assert!(is_refused(&out));
}

#[test]
fn statespace_refuses_a_matrix_of_the_wrong_size() {
    let out = run_text(&[
        "A(t) := [0, 1; -1, 0]",
        "statespace([0; 0; 0], 0, 1, 10, A)=",
    ]);
    assert!(is_refused(&out));
}

#[test]
fn statespace_refuses_a_non_integer_interval_count() {
    let out = run_text(&[
        "A(t) := [0, 1; -1, 0]",
        "statespace([0; 0], 0, 1, 10.5, A)=",
    ]);
    assert!(is_refused(&out));
}

// ----- numol ---------------------------------------------------------------------------------

/// Heat equation `u_t = u_xx` on [0, pi] with zero edges and `u(x, 0) = sin x`: the exact
/// solution is `exp(-t) sin x`.
fn heat_lines() -> Vec<&'static str> {
    vec![
        "F(x, z, u, ux, uxx) := [uxx[0]]",
        "pinit(x) := [sin(x)]",
        "bc(z) := [0, 0, \"D\"]",
    ]
}

#[test]
fn numol_heat_equation_matches_the_exact_decay() {
    let mut lines = heat_lines();
    lines.push("numol([0; 3.141592653589793], 21, [0; 1], 11, 1, 0, F, pinit, bc)=");
    let ((rows, cols), data) = matrix_of(run_text(&lines));
    assert_eq!((rows, cols), (21, 11));
    // Row 10 is x = pi / 2, column 10 is t = 1 (the single function's block of 11 columns).
    let value = data[10 * cols + 10];
    let exact = (-1.0f64).exp();
    assert!(
        (value - exact).abs() < 2e-3,
        "u(pi/2, 1) = {value}, expected {exact}"
    );
    // The start column is the initial condition, sin(x_i).
    let x = std::f64::consts::PI / 2.0;
    assert!((data[10 * cols] - x.sin()).abs() < 1e-9);
}

#[test]
fn numol_wave_system_matches_the_help_example() {
    // The help's PDE example: w_z = v and v_z = a^2 w_xx on [0, L], with w = sin(pi x / L) at
    // z = 0, v = 0, and w = 0 at both ends. The exact solution is w = sin(pi x / L) cos(a pi z / L).
    let out = run_text(&[
        "a := 3",
        "L := 6.283185307179586",
        "F(x, z, u, ux, uxx) := [u[1]; a^2 * uxx[0]]",
        "init(x) := [sin(3.141592653589793 * x / L); 0]",
        "bc(z) := [0, 0, \"D\"; \"NA\", \"NA\", \"D\"]",
        "numol([0; L], 21, [0; 6.283185307179586], 21, 2, 0, F, init, bc)=",
    ]);
    let ((rows, cols), data) = matrix_of(out);
    assert_eq!((rows, cols), (21, 42));
    let l = 2.0 * std::f64::consts::PI;
    let t_end = 2.0 * std::f64::consts::PI;
    for i in [3usize, 10, 17] {
        let x = i as f64 * l / 20.0;
        let exact =
            (std::f64::consts::PI * x / l).sin() * (3.0 * std::f64::consts::PI * t_end / l).cos();
        let value = data[i * cols + 20];
        assert!(
            (value - exact).abs() < 0.05,
            "w({x}, 2 pi) = {value}, expected {exact}"
        );
    }
}

#[test]
fn numol_refuses_algebraic_equations() {
    let mut lines = heat_lines();
    lines.push("numol([0; 3.141592653589793], 21, [0; 1], 11, 1, 1, F, pinit, bc)=");
    assert!(is_refused(&run_text(&lines)));
}

#[test]
fn numol_refuses_a_first_order_spatial_derivative() {
    let out = run_text(&[
        "F(x, z, u, ux, uxx) := [-ux[0]]",
        "pinit(x) := [sin(x)]",
        "bc(z) := [0, 0, \"D\"]",
        "numol([0; 3.141592653589793], 21, [0; 1], 11, 1, 0, F, pinit, bc)=",
    ]);
    assert!(is_refused(&out));
}

#[test]
fn numol_refuses_a_point_count_below_five() {
    let mut lines = heat_lines();
    lines.push("numol([0; pi], 4, [0; 1], 11, 1, 0, F, pinit, bc)=");
    assert!(is_refused(&run_text(&lines)));
}

#[test]
fn numol_refuses_a_boundary_row_without_a_type() {
    let out = run_text(&[
        "F(x, z, u, ux, uxx) := [uxx[0]]",
        "pinit(x) := [sin(x)]",
        "bc(z) := [0, 0, \"X\"]",
        "numol([0; pi], 21, [0; 1], 11, 1, 0, F, pinit, bc)=",
    ]);
    assert!(is_refused(&out));
}

#[test]
fn numol_uxx_with_a_coefficient_that_vanishes_at_the_midpoint_is_used() {
    // u_t = x^2 u_xx on [-1, 1] (the coefficient is zero at the midpoint, where the old probe was),
    // u(x, 0) = 1 - x^2, zero at both ends. The reference is an explicit RK4 method of lines on the
    // same 21 nodes: u(-0.5, 0.1) = 0.7007124564. The old probe froze the solution at 0.75.
    let out = run_text(&[
        "F(x, z, u, ux, uxx) := [x^2 * uxx[0]]",
        "pinit(x) := [1 - x^2]",
        "bc(z) := [0, 0, \"D\"]",
        "numol([-1; 1], 21, [0; 0.1], 5, 1, 0, F, pinit, bc)=",
    ]);
    let ((rows, cols), data) = matrix_of(out);
    assert_eq!((rows, cols), (21, 5));
    let value = data[5 * cols + 4];
    assert!(
        (value - 0.700_712_456_4).abs() < 1e-4,
        "u(-0.5, 0.1) = {value}, expected 0.7007124564"
    );
}

#[test]
fn numol_failing_review_input_is_not_frozen() {
    // u_t = x u_xx on [-1, 1] as reported. For x < 0 the equation is backward parabolic, so the
    // solution grows without a physical bound and no finite reference applies. The regression
    // checked here is the reported symptom: the interior no longer stays at its initial 0.75.
    let out = run_text(&[
        "G(x, z, u, ux, uxx) := [x * uxx[0]]",
        "pinit(x) := [1 - x^2]",
        "bc(z) := [0, 0, \"D\"]",
        "numol([-1; 1], 21, [0; 0.1], 5, 1, 0, G, pinit, bc)=",
    ]);
    let ((_, cols), data) = matrix_of(out);
    let value = data[5 * cols + 4];
    assert!(
        (value - 0.75).abs() > 1e-3,
        "u(-0.5, 0.1) = {value} stayed at its initial value"
    );
}

#[test]
fn numol_uxx_is_probed_at_the_end_time() {
    // u_t = t u_xx: the coefficient is zero at t0 = 0, so only the probe at t1 sees u_xx. The exact
    // solution is exp(-t^2 / 2) sin x, so u(pi/2, 1) = exp(-1/2).
    let out = run_text(&[
        "F(x, z, u, ux, uxx) := [z * uxx[0]]",
        "pinit(x) := [sin(x)]",
        "bc(z) := [0, 0, \"D\"]",
        "numol([0; 3.141592653589793], 21, [0; 1], 11, 1, 0, F, pinit, bc)=",
    ]);
    let ((rows, cols), data) = matrix_of(out);
    assert_eq!((rows, cols), (21, 11));
    let value = data[10 * cols + 10];
    let exact = (-0.5f64).exp();
    assert!(
        (value - exact).abs() < 3e-3,
        "u(pi/2, 1) = {value}, expected {exact}"
    );
}

#[test]
fn numol_boundary_row_other_than_na_na_d_makes_the_function_second_order() {
    // F uses no spatial derivative, but the row [0, 0, "D"] gives both edges a value: the function
    // is second order, so its edge stays at 0 while the interior decays as exp(-t).
    let out = run_text(&[
        "F(x, z, u, ux, uxx) := [-u[0]]",
        "pinit(x) := [1]",
        "bc(z) := [0, 0, \"D\"]",
        "numol([0; 3.141592653589793], 21, [0; 1], 11, 1, 0, F, pinit, bc)=",
    ]);
    let ((rows, cols), data) = matrix_of(out);
    assert_eq!((rows, cols), (21, 11));
    // Row 0 is the left edge: 0 at every time. Row 10 is x = pi / 2: exp(-t), so exp(-1) at t = 1.
    assert!(data[0].abs() < 1e-12, "left edge at t=0 is {}", data[0]);
    assert!(data[10].abs() < 1e-9, "left edge at t=1 is {}", data[10]);
    let interior = data[10 * cols + 10];
    assert!(
        (interior - (-1.0f64).exp()).abs() < 2e-3,
        "u(pi/2, 1) = {interior}"
    );
}

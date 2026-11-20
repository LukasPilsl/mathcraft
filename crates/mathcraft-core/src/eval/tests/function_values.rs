//! Function values (`Value::Func`) and the functions built on them: `polyfit`, `IsFunction`,
//! `funmap`, `funconv` and `funcdeconv`. The numbers are the help's worked examples ("Example:
//! Second-Order Response Surface", "Example: Quality of Fit", "Example: Function and Level
//! Mapping", "Example: Filtering in the Fourier Transform Domain").

use crate::eval::{EvalError, Outcome, Session, Value};
use crate::expr::IdKind;
use crate::parser::{parse, DefaultIdContext, IdContext};

/// `m` and `s` are units in these sheets, as the worksheet classifies them.
struct Units;

impl IdContext for Units {
    fn classify(&self, name: &str, called: bool) -> IdKind {
        if !called && (name == "m" || name == "s") {
            IdKind::Unit
        } else if called {
            IdKind::Function
        } else {
            IdKind::Variable
        }
    }
}

/// Runs the statements in order on a fresh worksheet; returns the outcome of the last.
fn run_text(lines: &[&str]) -> Outcome {
    let mut s = Session::new();
    let mut last = None;
    for line in lines {
        let e = parse(line, &Units).unwrap_or_else(|e| panic!("{line}: {e:?}"));
        last = Some(s.run(&e));
    }
    last.expect("at least one statement")
}

fn real_of(out: Outcome) -> f64 {
    match out {
        Outcome::Value(Value::Scalar(s)) => s.value.re,
        other => panic!("expected a scalar, got {other:?}"),
    }
}

fn reals_of(out: Outcome) -> Vec<f64> {
    match out {
        Outcome::Value(Value::Matrix(m)) => m.data().iter().map(|s| s.value.re).collect(),
        other => panic!("expected a matrix, got {other:?}"),
    }
}

fn error_of(out: Outcome) -> EvalError {
    match out {
        Outcome::Error(e) => e,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn close(got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() <= tol, "{got} is not {want} (tol {tol})");
}

const SURFACE_Y: &str =
    "Y := [76.5; 77; 78; 79.5; 79.9; 80.3; 80; 79.7; 79.8; 78.4; 75.6; 78.5; 77]";
const SURFACE_VALS: &str = "Vals := [77.93, 80, 85, 90, 92.07; 167.93, 170, 175, 180, 182.07]";

const VISCOSITY: [&str; 3] = [
    "vx := [80, 8; 93, 9; 100, 10; 82, 12; 90, 11; 99, 8; 81, 8; 96, 10; 94, 12; 93, 11; 97, 13; 95, 11; 100, 8; 85, 12; 86, 9; 87, 12]",
    "vy := [2256; 2340; 2426; 2293; 2330; 2368; 2250; 2409; 2364; 2379; 2440; 2364; 2404; 2317; 2309; 2328]",
    "p := polyfit(vx, vy, 1)",
];

// ----- polyfit ------------------------------------------------------------------------------------

#[test]
fn polyfit_of_the_coded_design_predicts_the_help_value() {
    // The design matrix keeps its Run and Block columns; the fit reads A and B only.
    let got = real_of(run_text(&[
        "X := boxwilson(2)",
        SURFACE_Y,
        "yield := polyfit(X, Y, 2)",
        "yield([-1; -1])=",
    ]));
    close(got, 76.302, 5e-3);
}

#[test]
fn polyfit_of_the_real_levels_predicts_the_help_value() {
    let got = real_of(run_text(&[
        "X := boxwilson(2)",
        SURFACE_VALS,
        "D := doelabel(X, Vals)",
        SURFACE_Y,
        "Yield := polyfit(D, Y, 2)",
        "Yield([80; 170])=",
    ]));
    close(got, 76.302, 5e-3);
}

#[test]
fn polyfit_function_value_is_usable_by_maximize() {
    // The help finds the maximum yield 80.216 at t = 86.544, T = 177.103.
    let lines = [
        "X := boxwilson(2)",
        SURFACE_VALS,
        "D := doelabel(X, Vals)",
        SURFACE_Y,
        "Yield := polyfit(D, Y, 2)",
        "a := 85",
        "b := 175",
        "f(a, b) := Yield([a; b])",
    ];
    let mut s = Session::new();
    for l in lines {
        let e = parse(l, &DefaultIdContext).unwrap();
        let out = s.run(&e);
        assert!(!matches!(out, Outcome::Error(_)), "{l}: {out:?}");
    }
    let eval = |s: &mut Session, src: &str| s.run(&parse(src, &DefaultIdContext).unwrap());
    let best = real_of(eval(&mut s, "f(86.544, 177.103)="));
    close(best, 80.216, 5e-3);
    // A stationary point: a step away on either side is lower.
    assert!(best > real_of(eval(&mut s, "f(87.5, 177.103)=")));
    assert!(best > real_of(eval(&mut s, "f(86.544, 176)=")));
}

#[test]
fn polyfit_linear_regression_has_the_help_error_sum_of_squares() {
    // "Example: Quality of Fit": the residuals of the linear fit have SSE = 3478.851.
    let mut lines: Vec<&str> = VISCOSITY.to_vec();
    lines.push("SSE := (p([80; 8]) - 2256)^2 + (p([93; 9]) - 2340)^2 + (p([100; 10]) - 2426)^2 + (p([82; 12]) - 2293)^2 + (p([90; 11]) - 2330)^2 + (p([99; 8]) - 2368)^2 + (p([81; 8]) - 2250)^2 + (p([96; 10]) - 2409)^2 + (p([94; 12]) - 2364)^2 + (p([93; 11]) - 2379)^2 + (p([97; 13]) - 2440)^2 + (p([95; 11]) - 2364)^2 + (p([100; 8]) - 2404)^2 + (p([85; 12]) - 2317)^2 + (p([86; 9]) - 2309)^2 + (p([87; 12]) - 2328)^2");
    lines.push("SSE=");
    close(real_of(run_text(&lines)), 3478.851, 2e-2);
}

#[test]
fn polyfit_evaluates_over_a_range_of_runs() {
    // `pred_i := p([t_i; fr_i])` of the help example, one value for each run.
    let mut lines: Vec<&str> = VISCOSITY.to_vec();
    lines.push("t := [80; 93; 100]");
    lines.push("fr := [8; 9; 10]");
    lines.push("i := 0 .. 2");
    lines.push("pred_i := p([t_i; fr_i])");
    lines.push("pred=");
    let got = reals_of(run_text(&lines));
    assert_eq!(got.len(), 3);
    close(got[0], 2256.0, 25.0);
}

#[test]
fn polyfit_terms_string_and_matrix_agree() {
    // "A B AB AA BB" is the order-2 model; M gives the same terms by exponents.
    let base = ["X := boxwilson(2)", SURFACE_Y];
    let at = |fit: &str| {
        let mut lines: Vec<&str> = base.to_vec();
        lines.push(fit);
        lines.push("q([0.5; -0.7])=");
        real_of(run_text(&lines))
    };
    let order = at("q := polyfit(X, Y, 2)");
    let terms = at("q := polyfit(X, Y, \"A B AB AA BB\")");
    let matrix = at("q := polyfit(X, Y, [0, 0, 0; 0, 1, 0; 0, 0, 1; 0, 1, 1; 0, 2, 0; 0, 0, 2])");
    close(terms, order, 1e-9);
    close(matrix, order, 1e-9);
    // Separators may be commas, colons or semicolons.
    close(at("q := polyfit(X, Y, \"A,B;AB:AA BB\")"), order, 1e-9);
}

#[test]
fn polyfit_exact_line_through_data() {
    let got = real_of(run_text(&[
        "x := [1; 2; 3; 4]",
        "y := [3; 5; 7; 9]",
        "p := polyfit(x, y, 1)",
        "p(10)=",
    ]));
    close(got, 21.0, 1e-9);
}

#[test]
fn polyfit_matrix_without_intercept_passes_through_the_origin() {
    // Terms from M only: y = c x with no constant.
    let got = real_of(run_text(&[
        "x := [1; 2; 3]",
        "y := [2.1; 3.9; 6.1]",
        "p := polyfit(x, y, [0, 1])",
        "p(0)=",
    ]));
    assert_eq!(got, 0.0);
}

#[test]
fn polyfit_fits_every_replicate_of_a_run() {
    // Two replicates for each run: the line through the pairs (x, y1) and (x, y2).
    let got = real_of(run_text(&[
        "x := [1; 2; 3]",
        "y := [1.9, 2.1; 3.9, 4.1; 5.9, 6.1]",
        "p := polyfit(x, y, 1)",
        "p(5)=",
    ]));
    close(got, 10.0, 1e-9);
}

#[test]
fn polyfit_keeps_the_units() {
    // x in metres, y in seconds: the result is in seconds and the argument must be in metres.
    let ok = run_text(&[
        "x := [1 m; 2 m; 3 m]",
        "y := [2 s; 4 s; 6 s]",
        "p := polyfit(x, y, 1)",
        "p([5 m])=",
    ]);
    match ok {
        Outcome::Value(Value::Scalar(s)) => {
            close(s.value.re, 10.0, 1e-9);
            assert!(!s.dim.is_dimensionless());
        }
        other => panic!("{other:?}"),
    }
    let bad = run_text(&[
        "x := [1 m; 2 m; 3 m]",
        "y := [2 s; 4 s; 6 s]",
        "p := polyfit(x, y, 1)",
        "p([5 s])=",
    ]);
    assert!(matches!(error_of(bad), EvalError::DimensionMismatch { .. }));
}

#[test]
fn polyfit_refuses_what_it_cannot_fit() {
    // More terms than points, an order that is too large, dependent columns, a wrong term.
    for lines in [
        vec!["x := [1; 2]", "y := [1; 2]", "polyfit(x, y, 2)="],
        vec!["x := [1; 2; 3]", "y := [1; 2; 3]", "polyfit(x, y, 0)="],
        vec![
            "x := [1; 2; 3]",
            "y := [1; 2; 3]",
            "polyfit(x, y, \"A C\")=",
        ],
        vec!["x := [1; 2; 3]", "y := [1; 2; 3]", "polyfit(x, y, \"a\")="],
        vec!["x := [1; 2; 3]", "y := [1; 2]", "polyfit(x, y, 1)="],
        vec![
            "x := [1; 2; 3]",
            "y := [1; 2; 3]",
            "polyfit(x, y, [1, 0, 0])=",
        ],
        vec!["x := [1; 1; 1]", "y := [1; 2; 3]", "polyfit(x, y, 1)="],
        vec!["x := [1; 2; 3]", "y := [1; 2; 3]", "polyfit(x, y, 1.5)="],
    ] {
        let out = run_text(&lines);
        assert!(matches!(out, Outcome::Error(_)), "{lines:?}: {out:?}");
    }
}

#[test]
fn polyfit_function_wants_one_vector_of_the_right_length() {
    let base = ["x := [1; 2; 3]", "y := [1; 2; 4]", "p := polyfit(x, y, 1)"];
    for call in ["p(1, 2)=", "p([1; 2])=", "p(\"a\")="] {
        let mut lines = base.to_vec();
        lines.push(call);
        assert!(
            matches!(run_text(&lines), Outcome::Error(_)),
            "{call} should be refused"
        );
    }
}

#[test]
fn a_function_value_refuses_arithmetic() {
    for expr in ["p + 1=", "p * 2=", "-p=", "p^2=", "sin(p)=", "p[0]="] {
        let out = run_text(&["p := polyfit([1; 2; 3], [1; 2; 4], 1)", expr]);
        assert!(matches!(out, Outcome::Error(_)), "{expr}: {out:?}");
    }
}

#[test]
fn polyfit_work_is_charged() {
    // A large order on many variables is refused as too large, not computed.
    let out = run_text(&[
        "x := identity(40)",
        "y := identity(40)",
        "polyfit(x, y, 5)=",
    ]);
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
}

// ----- function values in calls ------------------------------------------------------------------

#[test]
fn a_function_value_can_be_a_parameter_and_be_captured() {
    // A parameter that holds a function value is called like a function, and a function defined
    // after the variable captures it.
    let lines_param = [
        "p := polyfit([1; 2; 3], [2; 4; 6], 1)",
        "g(f, x) := f(x) + 1",
        "g(p, 4)=",
    ];
    close(real_of(run_text(&lines_param)), 9.0, 1e-9);
    let lines_capture = [
        "p := polyfit([1; 2; 3], [2; 4; 6], 1)",
        "h(x) := p(x) * 2",
        "p := 0",
        "h(4)=",
    ];
    close(real_of(run_text(&lines_capture)), 16.0, 1e-9);
}

// ----- IsFunction ---------------------------------------------------------------------------------

#[test]
fn is_function_tells_functions_from_values() {
    let defs = [
        "f(x) := x^2",
        "p := polyfit([1; 2; 3], [2; 4; 6], 1)",
        "v := 3",
    ];
    let check = |arg: &str| {
        let mut lines = defs.to_vec();
        let call = format!("IsFunction({arg})=");
        lines.push(&call);
        real_of(run_text(&lines))
    };
    assert_eq!(check("f"), 1.0);
    assert_eq!(check("p"), 1.0);
    assert_eq!(check("sin"), 1.0);
    assert_eq!(check("v"), 0.0);
    assert_eq!(check("3"), 0.0);
    assert_eq!(check("f(2)"), 0.0);
    assert_eq!(check("[1, 2]"), 0.0);
}

#[test]
fn is_function_of_an_undefined_name_is_an_error() {
    assert!(matches!(
        run_text(&["IsFunction(nothing)="]),
        Outcome::Error(_)
    ));
}

// ----- funmap -------------------------------------------------------------------------------------

#[test]
fn funmap_applies_the_function_to_each_element() {
    // Help: M = i j 6 for i = 0..3, j = 0..4; f(x) = x^2.
    let got = reals_of(run_text(&[
        "M := [0, 0, 0, 0, 0; 0, 6, 12, 18, 24; 0, 12, 24, 36, 48; 0, 18, 36, 54, 72]",
        "f(x) := x^2",
        "funmap(M, f)=",
    ]));
    let want = [
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 36.0, 144.0, 324.0, 576.0, 0.0, 144.0, 576.0, 1296.0, 2304.0,
        0.0, 324.0, 1296.0, 2916.0, 5184.0,
    ];
    assert_eq!(got, want);
}

#[test]
fn funmap_takes_a_function_value_and_keeps_the_shape() {
    let got = real_of(run_text(&[
        "p := polyfit([1; 2; 3], [2; 4; 6], 1)",
        "r := funmap([1, 2; 3, 4], p)",
        "r[1, 1]=",
    ]));
    close(got, 8.0, 1e-9);
}

#[test]
fn funmap_refuses_a_non_function() {
    for lines in [
        vec!["funmap([1, 2], 3)="],
        vec!["funmap([1, 2], nothing)="],
        vec!["f(x) := [x, x]", "funmap([1, 2], f)="],
    ] {
        assert!(
            matches!(run_text(&lines), Outcome::Error(_)),
            "{lines:?} should be refused"
        );
    }
}

// ----- funconv and funcdeconv ----------------------------------------------------------------------

const FILTER_M: &str =
    "M := [0, 0, 0, 0, 0, 0; 0, 1, 2, 3, 4, 5; 0, 2, 4, 6, 8, 10; 0, 3, 6, 9, 12, 15; 0, 4, 8, 12, 16, 20]";

#[test]
fn funconv_matches_the_help_example() {
    let got = reals_of(run_text(&[
        FILTER_M,
        "f(x) := x^2 + x + 1",
        "funconv(M, f)=",
    ]));
    let want = [
        -1.221, -1.656, -2.58, -3.503, -4.428, -4.863, -2.321, 0.818, 1.607, 3.192, 3.981, 7.12,
        -3.958, 2.107, 3.492, 6.508, 7.893, 13.958, -5.594, 3.397, 5.377, 9.825, 11.805, 20.795,
        -6.695, 5.87, 9.564, 16.519, 20.213, 32.778,
    ];
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        close(*g, w, 5e-4);
    }
}

#[test]
fn funcdeconv_undoes_funconv() {
    // Help: funcdeconv(F, f, 0) returns the original matrix.
    let got = reals_of(run_text(&[
        FILTER_M,
        "f(x) := x^2 + x + 1",
        "F := funconv(M, f)",
        "funcdeconv(F, f, 0)=",
    ]));
    let want = reals_of(run_text(&[FILTER_M, "M="]));
    for (g, w) in got.iter().zip(want) {
        close(*g, w, 1e-9);
    }
}

#[test]
fn funcdeconv_with_a_zero_divisor_is_refused_and_e_lifts_it() {
    let zero = run_text(&[FILTER_M, "f(x) := x", "funcdeconv(M, f, 0)="]);
    assert!(matches!(error_of(zero), EvalError::DivisionByZero));
    let lifted = run_text(&[FILTER_M, "f(x) := x", "funcdeconv(M, f, 0.5)="]);
    assert!(matches!(lifted, Outcome::Value(Value::Matrix(_))));
}

#[test]
fn funconv_refuses_what_it_cannot_filter() {
    for lines in [
        vec!["funconv([1], f)="],
        vec!["f(x) := x", "funconv([1], f)="],
        vec!["f(x) := x", "funconv(3, f)="],
        vec!["f(x) := [x, x]", "funconv([1, 2; 3, 4], f)="],
        vec!["f(x) := x", "funconv([1 m, 2 m], f)="],
    ] {
        assert!(
            matches!(run_text(&lines), Outcome::Error(_)),
            "{lines:?} should be refused"
        );
    }
}

#[test]
fn defining_a_function_with_a_function_parameter_is_not_an_error() {
    let mut s = Session::new();
    for line in ["p := polyfit([1;2;3],[2;4;6],1)", "g(f,x):=f(x)+1"] {
        let e = parse(line, &Units).unwrap_or_else(|e| panic!("{line}: {e:?}"));
        let out = s.run(&e);
        assert!(!matches!(out, Outcome::Error(_)), "{line}: {out:?}");
    }
}

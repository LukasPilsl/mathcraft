//! Tests for the QA round 1 engine findings (F1 temperatures, F4 programs, F7 names that are also
//! units, F8 unit display, F10 subscripted parameters, F11 descending ranges, F14 trig poles).
//! Reference numbers come from python3.

use crate::calc::CalcOptions;
use crate::doc::{Doc, Placement};

fn values(doc: &mut Doc, ids: &[u32]) -> Vec<String> {
    let calc = doc.calculate(&CalcOptions::default());
    ids.iter()
        .map(|id| match calc.result(*id) {
            Some(r) => match &r.error {
                Some(e) => format!("ERR: {e}"),
                None => r.value.clone(),
            },
            None => "NONE".into(),
        })
        .collect()
}

/// Inserts every line as a math region and returns the value (or `ERR: message`) of each.
fn run(lines: &[&str]) -> Vec<String> {
    let mut doc = Doc::new("qa");
    let ids: Vec<u32> = lines
        .iter()
        .map(|l| doc.insert_math(l, Placement::Auto).unwrap())
        .collect();
    values(&mut doc, &ids)
}

/// The value of the last line.
fn last(lines: &[&str]) -> String {
    run(lines).pop().unwrap()
}

/// A program `pp(params)` made of `body`, then each call in `calls`.
fn run_prog(params: &[&str], body: &str, calls: &[&str]) -> Vec<String> {
    let mut doc = Doc::new("qa");
    let params: Vec<String> = params.iter().map(|p| (*p).to_owned()).collect();
    let lines: Vec<String> = body.lines().map(str::to_owned).collect();
    doc.insert_program("pp", &params, &lines, Placement::Auto)
        .unwrap();
    let ids: Vec<u32> = calls
        .iter()
        .map(|l| doc.insert_math(l, Placement::Auto).unwrap())
        .collect();
    values(&mut doc, &ids)
}

// ---- F1: temperatures --------------------------------------------------------------------

#[test]
fn celsius_and_fahrenheit_are_absolute_temperatures_in_kelvin() {
    // python: 25 + 273.15 = 298.15; (1 + 459.67) * 5 / 9 = 255.92777...
    assert_eq!(last(&["25 °C ="]), "298.15 K");
    assert_eq!(last(&["25 degC ="]), "298.15 K");
    assert_eq!(last(&["1 °F ="]), "255.928 K");
    assert_eq!(last(&["1 degF ="]), "255.928 K");
    assert_eq!(last(&["100 °R ="]), "55.556 K");
    assert_eq!(last(&["0 °C ="]), "273.15 K");
}

#[test]
fn temperature_values_convert_with_the_offset() {
    assert_eq!(last(&["77 °F -> °C ="]), "25 °C");
    assert_eq!(last(&["25 °C -> degF ="]), "77 °F");
    assert_eq!(last(&["x:=25 °C", "x -> K ="]), "298.15 K");
    assert_eq!(last(&["(-40) °C -> °F ="]), "-40 °F");
}

#[test]
fn temperature_intervals_use_difference_semantics() {
    // 20 °C = 293.15 K, plus an interval of 5 K.
    assert_eq!(last(&["20 °C + 5 K ="]), "298.15 K");
    assert_eq!(last(&["20 °C + 5 K -> °C ="]), "25 °C");
    assert_eq!(last(&["20 °C + 5 ΔC -> °C ="]), "25 °C");
    assert_eq!(last(&["5 ΔC ="]), "5 K");
    assert_eq!(last(&["9 ΔF ="]), "5 K");
    assert_eq!(last(&["25 °C - 20 °C ="]), "5 K");
}

#[test]
fn degree_sign_is_an_angle_unless_followed_by_c_f_or_r() {
    // 30 degrees = 0.5235987755982988 rad
    assert_eq!(last(&["30° ="]), "0.524");
    assert_eq!(last(&["30 ° ="]), "0.524");
    assert_eq!(last(&["sin(30°) ="]), "0.5");
}

// ---- F7: a variable wins over a unit of the same name -----------------------------------

#[test]
fn user_variables_win_over_unit_names() {
    assert_eq!(last(&["L:=6 m", "L ="]), "6 m");
    assert_eq!(last(&["m:=3", "m ="]), "3");
    assert_eq!(last(&["s:=5", "s ="]), "5");
    assert_eq!(last(&["g:=2", "g ="]), "2");
    assert_eq!(last(&["A:=7", "A ="]), "7");
    assert_eq!(last(&["h:=10", "h ="]), "10");
    assert_eq!(last(&["t:=7", "t ="]), "7");
    assert_eq!(last(&["d:=3 m", "tt:=2 s", "v:=d/tt", "v ="]), "1.5 m/s");
    assert_eq!(last(&["F:=2 N", "F ="]), "2 N");
    assert_eq!(last(&["H:=4", "W:=5", "H*W ="]), "20");
    assert_eq!(last(&["V:=2 m^3", "V ="]), "2 m^3");
    assert_eq!(last(&["N:=3", "N ="]), "3");
    assert_eq!(last(&["R:=8.314", "R ="]), "8.314");
}

#[test]
fn a_number_before_a_redefined_unit_name_multiplies_the_variable() {
    // Before the definition `L` is litres, after it the variable.
    assert_eq!(
        run(&["5 L =", "L:=3 m", "5 L ="]),
        ["5×10^-3 m^3", "", "15 m"]
    );
}

#[test]
fn a_function_sees_the_variable_that_was_a_unit_name() {
    assert_eq!(last(&["L:=6 m", "f(x):=x*L", "f(2) ="]), "12 m");
    // With no variable of that name the unit stays a unit inside a function.
    assert_eq!(last(&["f(x):=x*L", "f(2) ="]), "2×10^-3 m^3");
}

#[test]
fn units_still_read_as_units_when_no_variable_has_the_name() {
    assert_eq!(last(&["5 m ="]), "5 m");
    assert_eq!(last(&["2 t -> kg ="]), "2×10^3 kg");
}

// ---- F8: unit display ----------------------------------------------------------------------

#[test]
fn a_ratio_of_units_is_shown_with_a_slash() {
    assert_eq!(last(&["spd:=3 m/s", "spd -> m/s ="]), "3 m/s");
    assert_eq!(last(&["a:=3 m/s^2", "a -> m/s^2 ="]), "3 m/s^2");
    assert_eq!(last(&["a:=3 m/s^2", "a -> km/(h*s) ="]), "10.8 km/(h·s)");
    assert_eq!(last(&["f:=3 N*m", "f -> N*m ="]), "3 N·m");
    assert_eq!(last(&["r:=4/s", "r -> 1/s ="]), "4 1/s");
}

#[test]
#[ignore = "writes a sample sheet for the screenshot harness"]
fn write_screenshot_sheet() {
    let mut doc = Doc::new("qa");
    for l in [
        "spd:=3 m/s",
        "spd -> m/s =",
        "integral(x, 0, inf, exp(-x))",
        "integral(x, 0, inf, exp(-x)) =",
        "T:=25 °C",
        "T -> degF =",
    ] {
        doc.insert_math(l, Placement::Auto).unwrap();
    }
    let out = "/Users/lukaspilsl/development/mathcraft/screenshots/mathcraft/qa_units.mcdx";
    std::fs::create_dir_all("/Users/lukaspilsl/development/mathcraft/screenshots/mathcraft")
        .unwrap();
    std::fs::write(out, doc.to_bytes().unwrap()).unwrap();
}

// ---- F14: poles of the trig functions --------------------------------------------------------

#[test]
fn trig_functions_are_finite_at_the_double_nearest_a_pole() {
    // python: math.tan(math.pi/2) = 1.633123935319537e16
    assert_eq!(last(&["tan(π/2) ="]), "1.633×10^16");
    assert_eq!(last(&["sec(π/2) ="]), "1.633×10^16");
    assert_eq!(last(&["csc(π) ="]), "8.166×10^15");
    // python: 1/math.tan(math.pi) = -8.165619676597685e15
    assert_eq!(last(&["cot(π) ="]), "-8.166×10^15");
    assert_eq!(last(&["tan(90°) ="]), "1.633×10^16");
    assert_eq!(last(&["tan(π/4) ="]), "1");
}

// ---- F11: ranges that count down --------------------------------------------------------------

#[test]
fn a_range_with_the_end_below_the_start_counts_down() {
    assert_eq!(last(&["r:=1..-1", "r ="]), "[[1], [0], [-1]]");
    assert_eq!(
        run_prog(
            &["n"],
            "acc ← 0\nfor k ∈ 1..n\n  acc ← acc*3+k\nreturn acc",
            &["pp(-1)=", "pp(4)="]
        ),
        ["8", "58"]
    );
}

// ---- F10: subscripted function parameters --------------------------------------------------------

#[test]
fn function_parameters_may_be_subscripted() {
    assert_eq!(last(&["ff(t_1):=t_1^2+1", "ff(3) ="]), "10");
    assert_eq!(last(&["ff(t_1, t_2):=t_1-t_2", "ff(5, 3) ="]), "2");
}

// ---- F4: a collatz program ------------------------------------------------------------------------

#[test]
fn collatz_of_27_takes_111_steps() {
    // python: the Collatz sequence of 27 has 111 steps.
    let body = "c ← 0\nx ← n\nwhile x<>1\n  if mod(x,2)==0\n    x ← x/2\n  else\n    x ← 3*x+1\n  c ← c+1\nreturn c";
    assert_eq!(run_prog(&["n"], body, &["pp(27)=", "pp(6)="]), ["111", "8"]);
    let with_break = "c ← 0\nx ← n\nfor k ∈ 1..500\n  if x==1\n    break\n  if mod(x,2)==0\n    x ← x/2\n  else\n    x ← 3*x+1\n  c ← c+1\nreturn c";
    assert_eq!(run_prog(&["n"], with_break, &["pp(27)="]), ["111"]);
    assert_eq!(
        run(&[
            "cz(n):=if(n==1,0,1+cz(if(mod(n,2)==0,n/2,3*n+1)))",
            "cz(27)="
        ])[1],
        "111"
    );
}

#[test]
fn collatz_is_cheap_next_to_a_big_worksheet() {
    let mut doc = Doc::new("qa");
    doc.insert_math("M:=matrix(300,300,0)", Placement::Auto)
        .unwrap();
    let body = "c ← 0\nx ← n\nwhile x<>1\n  if mod(x,2)==0\n    x ← x/2\n  else\n    x ← 3*x+1\n  c ← c+1\nreturn c";
    let lines: Vec<String> = body.lines().map(str::to_owned).collect();
    doc.insert_program("pp", &["n".to_owned()], &lines, Placement::Auto)
        .unwrap();
    let id = doc.insert_math("pp(27)=", Placement::Auto).unwrap();
    assert_eq!(values(&mut doc, &[id]), ["111"]);
}

#[test]
fn a_runaway_loop_still_hits_the_work_limit() {
    let out = run_prog(&["n"], "c ← 0\nwhile 1\n  c ← c+1\nreturn c", &["pp(1)="]);
    assert!(out[0].starts_with("ERR: work limit"), "{out:?}");
}

#[test]
fn a_negative_celsius_literal_keeps_the_sign_on_the_number() {
    // python: -5 + 273.15 = 268.15; -40 degC = -40 degF; (-5 degC) - (25 degC) is a 30 K difference.
    assert_eq!(last(&["-5 °C ="]), "268.15 K");
    assert_eq!(last(&["(-5) °C ="]), "268.15 K");
    assert_eq!(last(&["x:=-5 °C", "x ="]), "268.15 K");
    assert_eq!(last(&["-40 °C -> °F ="]), "-40 °F");
    assert_eq!(last(&["T:=25 °C", "T2:=-5 °C", "T-T2 ="]), "30 K");
}

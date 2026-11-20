//! Engine gaps found by QA (F21, F22) and the float noise of offset temperatures.
//!
//! Reference values come from explicit closed forms in plain Python (`math` only): the
//! explicit-sum formulas of the Legendre, Hermite (physicists'), Laguerre and Jacobi polynomials,
//! `cos(n acos x)` and `sin((n+1) acos x) / sin(acos x)` for the Chebyshev ones, at x = 0.3.
//! Prime's names are `Leg`, `Her`, `Lag`, `Tcheb`, `Ucheb`, `Jac`, `LU`, `QR` and `Cholesky`;
//! the legacy names `Pn`, `Hn`, `Tn`, `regress`, lowercase `lu` and `qr` do not exist in Prime and
//! stay undefined here.

use crate::eval::{EvalError, Outcome, Session, Value};
use crate::expr::IdKind;
use crate::parser::{parse, IdContext};
use crate::symbolic::Sym;

struct Units;

impl IdContext for Units {
    fn classify(&self, name: &str, called: bool) -> IdKind {
        if name.starts_with('°') || name == "K" {
            IdKind::Unit
        } else if called {
            IdKind::Function
        } else {
            IdKind::Variable
        }
    }
}

fn run(lines: &[&str]) -> Outcome {
    let mut s = Session::new();
    let mut last = None;
    for line in lines {
        let e = parse(line, &Units).unwrap_or_else(|e| panic!("{line}: {e:?}"));
        last = Some(s.run(&e));
    }
    last.expect("a statement")
}

fn real(line: &str) -> f64 {
    match run(&[line]) {
        Outcome::Value(Value::Scalar(s)) => s.value.re,
        other => panic!("{line}: {other:?}"),
    }
}

fn entries(lines: &[&str]) -> Vec<Sym> {
    match run(lines) {
        Outcome::Value(Value::Symbolic(s)) => s.entries,
        other => panic!("{lines:?}: {other:?}"),
    }
}

fn sym(src: &str) -> Sym {
    crate::symbolic::from_expr(&parse(src, &Units).expect("parses")).expect("converts")
}

fn close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= 1e-12 * (1.0 + want.abs()),
        "{what}: {got} vs {want}"
    );
}

#[test]
fn orthogonal_polynomials_match_their_closed_forms_up_to_degree_ten() {
    let leg = [
        1.0,
        0.3,
        -0.365,
        -0.38249999999999995,
        0.07293750000000003,
        0.34538625000000006,
        0.12918118749999996,
        -0.22407298125000005,
        -0.23907459101562495,
        0.06370038175781256,
        0.2514763495160155,
    ];
    let her = [
        1.0,
        0.6,
        -1.6400000000000001,
        -3.3839999999999995,
        7.8096000000000005,
        31.757759999999998,
        -59.041343999999995,
        -416.51792639999996,
        576.66806016,
        7010.2876584959995,
        -6173.852487782401,
    ];
    let lag = [
        1.0,
        0.7,
        0.445,
        0.2305000000000001,
        0.052337500000000065,
        -0.09333275000000003,
        -0.21005798749999988,
        -0.30110570589285723,
        -0.36948099551562485,
        -0.4179429975520087,
        -0.4490205094581942,
    ];
    let tcheb = [
        1.0,
        0.3,
        -0.82,
        -0.792,
        0.3448,
        0.99888,
        0.254528,
        -0.8461632,
        -0.76222592,
        0.388827648,
        0.9955225088,
    ];
    let ucheb = [
        1.0,
        0.6,
        -0.64,
        -0.984,
        0.0496,
        1.01376,
        0.558656,
        -0.6785664,
        -0.96579584,
        0.099088896,
        1.0252491776,
    ];
    let jac = [
        1.0,
        0.10000000000000042,
        -0.6625000000000001,
        -0.3972499999999988,
        0.33862500000000006,
        0.5180175000000051,
        -0.011308171874992331,
        -0.46438021093749016,
        -0.2501329969531071,
        0.27951184777347904,
        0.3876877225102869,
    ];
    for n in 0..=10 {
        close(real(&format!("Leg({n}, 0.3)")), leg[n], &format!("Leg {n}"));
        close(real(&format!("Her({n}, 0.3)")), her[n], &format!("Her {n}"));
        close(real(&format!("Lag({n}, 0.3)")), lag[n], &format!("Lag {n}"));
        close(
            real(&format!("Tcheb({n}, 0.3)")),
            tcheb[n],
            &format!("Tcheb {n}"),
        );
        close(
            real(&format!("Ucheb({n}, 0.3)")),
            ucheb[n],
            &format!("Ucheb {n}"),
        );
        close(
            real(&format!("Jac({n}, 0.5, 1.5, 0.3)")),
            jac[n],
            &format!("Jac {n}"),
        );
    }
    // Legendre P2(x) = (3 x^2 - 1)/2 on a vector argument
    match run(&["Leg(2, [0.3, 0.5, 1])"]) {
        Outcome::Value(Value::Matrix(m)) => {
            let v: Vec<f64> = m.data().iter().map(|s| s.value.re).collect();
            close(v[0], -0.365, "P2(0.3)");
            close(v[1], -0.125, "P2(0.5)");
            close(v[2], 1.0, "P2(1)");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn legacy_names_that_prime_lacks_stay_undefined() {
    for call in [
        "Pn(2, 0.3)",
        "Hn(2, 0.3)",
        "Tn(2, 0.3)",
        "regress([1, 2, 3], [1, 4, 9], 2)",
        "lu([2, 1; 4, 3])",
        "qr([2, 1; 4, 3])",
    ] {
        assert!(
            matches!(
                run(&[call]),
                Outcome::Error(EvalError::UndefinedFunction(_))
            ),
            "{call}"
        );
    }
}

#[test]
fn solve_returns_complex_roots_of_a_quadratic() {
    // a^2 + 1 = 0: [i; -i], the plus root first
    let i = Sym::symbol("i");
    let minus_i = Sym::product(vec![Sym::int(-1), i.clone()]).expect("product");
    assert_eq!(entries(&["a^2+1==0 -> solve, a"]), vec![i, minus_i]);
    // 2 a^2 + 2 a + 1 = 0: (-1 +- i)/2
    let want = |sign: i64| {
        Sym::sum(vec![
            sym("-1/2"),
            Sym::product(vec![Sym::int(sign), sym("1/2"), Sym::symbol("i")]).expect("product"),
        ])
        .expect("sum")
    };
    assert_eq!(
        entries(&["2*a^2+2*a+1==0 -> solve, a"]),
        vec![want(1), want(-1)]
    );
}

#[test]
fn solve_gives_the_principal_value_of_a_periodic_equation() {
    // the help: a periodic solution set is answered by a single value (sin(x) = 0 gives 0)
    assert_eq!(entries(&["sin(a)==0.5 -> solve, a"]), vec![sym("pi/6")]);
    assert_eq!(entries(&["cos(a)==0.5 -> solve, a"]), vec![sym("pi/3")]);
    assert_eq!(entries(&["tan(a)==1 -> solve, a"]), vec![sym("pi/4")]);
    // no complex solutions are invented for |c| > 1
    assert!(matches!(
        run(&["sin(a)==2 -> solve, a"]),
        Outcome::Error(EvalError::Unsupported(_))
    ));
}

#[test]
fn solve_cubic_and_quartic_binomials_and_biquadratics() {
    // a^3 - 2 = 0: the real cube root and the complex pair
    let got = entries(&["a^3-2==0 -> solve, a"]);
    assert_eq!(got.len(), 3);
    assert_eq!(got[0], sym("2^(1/3)"));
    // a^4 - 5 a^2 + 6 = 0: +-sqrt(3), +-sqrt(2); the exact surds fold to numbers on a plain
    // answer, so compare the values
    let want = [3f64.sqrt(), -(3f64.sqrt()), 2f64.sqrt(), -(2f64.sqrt())];
    match run(&["a^4-5*a^2+6==0 -> solve, a"]) {
        Outcome::Value(Value::Matrix(m)) => {
            let v: Vec<f64> = m.data().iter().map(|s| s.value.re).collect();
            for (g, w) in v.iter().zip(want) {
                close(*g, w, "biquadratic root");
            }
            assert_eq!(v.len(), 4);
        }
        other => panic!("{other:?}"),
    }
    // a general cubic with no rational root is refused rather than answered wrongly
    assert!(matches!(
        run(&["a^3+a+1==0 -> solve, a"]),
        Outcome::Error(EvalError::Unsupported(_))
    ));
}

#[test]
fn limit_with_a_symbolic_exponent() {
    close(
        real("limit(xx, inf, (1+1/xx)^xx)"),
        std::f64::consts::E,
        "(1+1/x)^x",
    );
    // (1 + 2/x)^x -> e^2 and (1 + 1/x)^(2x) -> e^2
    close(
        real("limit(xx, inf, (1+2/xx)^xx)"),
        std::f64::consts::E.powi(2),
        "e^2",
    );
    close(
        real("limit(xx, inf, (1+1/xx)^(2*xx))"),
        std::f64::consts::E.powi(2),
        "e^2 b",
    );
    // 1^infinity with a vanishing rate is 1
    close(real("limit(xx, inf, (1+1/xx^2)^xx)"), 1.0, "rate 0");
    // a base that does not tend to 1 with a divergent exponent is refused
    assert!(matches!(
        run(&["limit(xx, inf, (2+1/xx)^xx)"]),
        Outcome::Error(EvalError::Unsupported(_))
    ));
}

#[test]
fn offset_temperature_conversion_snaps_float_noise_to_zero() {
    // 491.67 degR is 273.15 K, which is exactly 0 degC; the binary scale change leaves ~5.7e-14
    let shown = |line: &str| match run(&["t := 491.67 °R", line]) {
        Outcome::Converted { shown, .. } => match shown {
            Value::Scalar(s) => s.value.re,
            other => panic!("{other:?}"),
        },
        other => panic!("{line}: {other:?}"),
    };
    assert_eq!(shown("t ->°C="), 0.0);
    // other values are not snapped
    close(shown("t ->°F="), 32.0, "degF");
    close(shown("t ->K="), 273.15, "K");
}

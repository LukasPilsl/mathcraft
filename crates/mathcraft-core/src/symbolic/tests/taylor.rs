//! Taylor series: the help's examples (`e^x series`, `sin(x) series,8`, `e^x series,x=4`), a
//! property check against the functions near the point, and refusals.

use super::Lcg;
use crate::symbolic::taylor::{series_with, Method};
use crate::symbolic::{approximate, series_of, simplify, substitute, Sym, SymError};

use super::sy;

fn series(src: &str, point: i64, terms: usize) -> Result<Sym, SymError> {
    series_of(&sy(src), "x", &Sym::int(point), terms)
}

#[test]
fn help_examples_of_series() {
    // e^x series -> 1 + x + x^2/2 + x^3/6 + x^4/24 + x^5/120 (six terms by default).
    assert_eq!(
        series("exp(x)", 0, 6),
        Ok(sy("1 + x + x^2/2 + x^3/6 + x^4/24 + x^5/120"))
    );
    // sin(x) series,8 -> x - x^3/6 + x^5/120 - x^7/5040 (zero terms are not shown).
    assert_eq!(
        series("sin(x)", 0, 8),
        Ok(sy("x - x^3/6 + x^5/120 - x^7/5040"))
    );
}

#[test]
fn expansion_about_a_point_other_than_zero() {
    // e^x series,x=4: e^4 + e^4 (x-4) + e^4/2 (x-4)^2 + ... (six terms).
    let got = series("exp(x)", 4, 6).expect("series about 4");
    let want = sy("exp(4) + exp(4)*(x-4) + exp(4)/2*(x-4)^2 + exp(4)/6*(x-4)^3 + exp(4)/24*(x-4)^4 + exp(4)/120*(x-4)^5");
    assert_eq!(got, want);
}

#[test]
fn series_agrees_with_the_function_near_zero() {
    // Twelve terms, at x = 1/1000: the truncation error is far below 1e-12.
    let h = sy("1/1000");
    for (src, f) in [
        ("exp(x)", (|x: f64| x.exp()) as fn(f64) -> f64),
        ("sin(x)", |x: f64| x.sin()),
        ("cos(x)", |x: f64| x.cos()),
        ("1/(1-x)", |x: f64| 1.0 / (1.0 - x)),
    ] {
        let s = series(src, 0, 12).expect(src);
        let at = simplify(&substitute(&s, &[("x", h.clone())]).unwrap()).unwrap();
        let v = approximate(&at).expect("exact value at 1/1000");
        let want = f(0.001);
        assert!((v - want).abs() < 1e-12, "{src}: {v} vs {want}");
    }
}

#[test]
fn a_point_that_is_not_analytic_is_refused() {
    // A pole, and a branch point: no exact Taylor coefficient.
    assert!(matches!(series("1/x", 0, 3), Err(SymError::Unsupported(_))));
    assert!(matches!(
        series("sqrt(x)", 0, 3),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn bad_term_counts_are_refused() {
    assert!(matches!(series("exp(x)", 0, 0), Err(SymError::Invalid(_))));
    assert!(matches!(series("exp(x)", 0, 13), Err(SymError::Invalid(_))));
}

/// Series with the given method (`Method::Arithmetic` is what `series()` uses).
fn series_by(src: &str, point: i64, terms: usize, method: Method) -> Result<Sym, SymError> {
    series_with(&sy(src), "x", &Sym::int(point), terms, method)
}

#[test]
fn reviewer_slow_examples_finish_quickly_and_match_the_function() {
    // Each of these ran for minutes (or 30 s) by repeated differentiation. Checked against the
    // function at a point a little away from the expansion point.
    let cases: [(&str, i64, usize); 3] = [
        ("cos(x)/(sin(x)^2+3)", 0, 6),
        ("1/(2+x^2)+1/(1-x)", 12, 12),
        ("((1+x)^(-1/2))*(exp(x)/(1-x))", 2, 6),
    ];
    for (src, point, terms) in cases {
        let start = std::time::Instant::now();
        let got = series(src, point, terms);
        let took = start.elapsed();
        assert!(
            took < std::time::Duration::from_secs(10),
            "{src}: took {took:?}"
        );
        let got = got.unwrap_or_else(|e| panic!("{src}: {e:?}"));
        let f = |x: f64| match src {
            "cos(x)/(sin(x)^2+3)" => x.cos() / (x.sin().powi(2) + 3.0),
            "1/(2+x^2)+1/(1-x)" => 1.0 / (2.0 + x * x) + 1.0 / (1.0 - x),
            _ => (1.0 + x).powf(-0.5) * x.exp() / (1.0 - x),
        };
        let at = point as f64 + 1e-3;
        let v = crate::symbolic::calc::approx_at(&got, "x", at).expect("real value");
        let want = f(at);
        assert!(
            (v - want).abs() < 1e-6 * (1.0 + want.abs()),
            "{src}: {v} vs {want}"
        );
    }
}

#[test]
fn unknown_function_swell_is_charged_to_the_budget() {
    // tan and asin have no arithmetic: the differentiation fallback runs, charged by derivative
    // size, so the call ends (answer or refusal) within the time limit.
    let start = std::time::Instant::now();
    let out = series("tan(x)*asin(x)*atan(x)*tan(x)*asin(x)", 0, 12);
    let took = start.elapsed();
    assert!(took < std::time::Duration::from_secs(10), "took {took:?}");
    assert!(matches!(
        out,
        Ok(_) | Err(SymError::TooLarge(_)) | Err(SymError::Unsupported(_))
    ));
}

#[test]
fn polynomials_are_exact_whatever_the_order() {
    // Degree 20 at 0: the window 20..25 runs past the order cap, but the polynomial terminates.
    assert_eq!(series("x^20", 0, 6), Ok(sy("x^20")));
    assert_eq!(series("x^30 + 2*x", 0, 6), Ok(sy("2*x")));
    assert_eq!(series("(x+1)^3", 0, 6), Ok(sy("1 + 3*x + 3*x^2 + x^3")));
}

#[test]
fn non_terminating_series_stays_capped() {
    // A non-polynomial whose window runs past order 24 is refused, not truncated: sin(x)^30
    // starts at order 30.
    assert!(matches!(
        series("sin(x)^30", 0, 2),
        Err(SymError::Unsupported(_))
    ));
    // The same window inside the cap is fine: sin(x)^2 starts at order 2, 12 terms reach 13.
    assert!(series("sin(x)^2", 0, 12).is_ok());
    // (x-1)^(1/2) at 1 is not analytic: refused.
    assert!(matches!(
        series("(x-1)^(1/2)", 1, 3),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn arithmetic_and_differentiation_agree_on_random_functions() {
    // Random elementary expressions in x at integer points. Where the reference method
    // (differentiation) answers, the arithmetic method must give the same series, exactly.
    fn gen(rng: &mut Lcg, depth: usize) -> String {
        if depth == 0 || rng.range(0, 3) == 0 {
            return match rng.range(0, 2) {
                0 => "x".to_string(),
                _ => format!("({})", rng.range(-3, 3)),
            };
        }
        let a = gen(rng, depth - 1);
        let b = gen(rng, depth - 1);
        match rng.range(0, 9) {
            0 => format!("({a} + {b})"),
            1 => format!("({a} * {b})"),
            2 => format!("exp({a})"),
            3 => format!("sin({a})"),
            4 => format!("cos({a})"),
            5 => format!("(1/(3+({a})^2))"),
            6 => format!("((2+({a})^2)^(1/2))"),
            7 => format!("sinh({a})"),
            8 => format!("cosh({a})"),
            _ => format!("ln(3+({a})^2)"),
        }
    }
    let mut rng = Lcg(2024);
    let (mut compared, mut refused_by_reference, mut budget_refusals, mut other_form) =
        (0, 0, 0, 0);
    for _ in 0..300 {
        let src = gen(&mut rng, 2);
        let point = rng.range(-2, 2);
        let terms = rng.range(1, 6) as usize;
        let reference = series_by(&src, point, terms, Method::Differentiation);
        let fast = series_by(&src, point, terms, Method::Arithmetic);
        match (reference, fast) {
            (Ok(want), Ok(got)) => {
                if got != want {
                    // Same value in another form: symbolic constants (sin(cos(-2))) can leave a
                    // rational function uncancelled. The values must agree at a sample point.
                    let x = point as f64 + 0.01;
                    let (a, b) = (
                        crate::symbolic::calc::approx_at(&got, "x", x),
                        crate::symbolic::calc::approx_at(&want, "x", x),
                    );
                    match (a, b) {
                        (Some(a), Some(b)) if (a - b).abs() <= 1e-9 * (1.0 + b.abs()) => {
                            other_form += 1;
                        }
                        _ => panic!("{src} at {point}, {terms} terms\nGOT  {got}\nWANT {want}"),
                    }
                }
                compared += 1;
            }
            (Err(_), _) => refused_by_reference += 1,
            // Nested transcendental constants (sin(exp(-1)) and the like) can swell the
            // arithmetic past the work budget: a refusal, never a different answer.
            (Ok(_), Err(SymError::TooLarge(_))) => budget_refusals += 1,
            (Ok(want), Err(e)) => {
                panic!("{src} at {point}: arithmetic refused ({e:?}), reference gave {want}")
            }
        }
    }
    assert!(
        compared > 100,
        "{compared} compared, {refused_by_reference} refused by reference"
    );
    assert!(budget_refusals <= 5, "{budget_refusals} budget refusals");
    assert!(
        other_form <= 30,
        "{other_form} same-value answers in another form"
    );
}

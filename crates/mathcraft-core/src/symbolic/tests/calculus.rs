//! `differentiate`, `integrate_polynomial`, `coefficients`, `assume_positive`: exact values and
//! property tests (the derivative of the integral is the integrand).

use super::{sy, Lcg};
use crate::symbolic::{
    assume_positive, coefficients, definite_integral, differentiate, expand, integrate,
    integrate_polynomial, simplify, Sym, SymError,
};

fn d(src: &str, var: &str) -> Sym {
    expand(&differentiate(&sy(src), var).expect("differentiate")).expect("expand")
}

#[test]
fn derivative_of_polynomials_and_powers() {
    assert_eq!(d("3*x^2+2*x+5", "x"), sy("6*x+2"));
    assert_eq!(d("x^(1/2)", "x"), sy("1/2*x^(-1/2)"));
    // d/dx (a x^2 + b x + c) = 2 a x + b
    assert_eq!(d("a*x^2+b*x+c", "x"), sy("2*a*x+b"));
    // Symbolic constant: derivative 0.
    assert_eq!(d("a^2", "x"), sy("0"));
}

#[test]
fn derivative_of_products_and_quotients_by_powers() {
    // d/dx (x+1)^3 = 3 (x+1)^2
    assert_eq!(d("(x+1)^3", "x"), sy("3*x^2+6*x+3"));
    // d/dx (x^2 (x+1)) = 3 x^2 + 2 x
    assert_eq!(d("x^2*(x+1)", "x"), sy("3*x^2+2*x"));
}

#[test]
fn derivative_of_elementary_functions_with_chain_rule() {
    // d/dx sin(2x) = 2 cos(2x)
    let s = differentiate(&sy("sin(2*x)"), "x").unwrap();
    assert_eq!(s, simplify(&sy("2*cos(2*x)")).unwrap());
    // d/dx exp(x^2) = 2 x exp(x^2)
    let e = differentiate(&sy("exp(x^2)"), "x").unwrap();
    assert_eq!(e, simplify(&sy("2*x*exp(x^2)")).unwrap());
    // d/dx ln(x) = 1/x
    assert_eq!(differentiate(&sy("ln(x)"), "x").unwrap(), sy("x^(-1)"));
}

#[test]
fn unknown_function_derivative_is_refused() {
    let r = differentiate(&sy("gamma(x)"), "x");
    assert!(matches!(r, Err(SymError::Unsupported(_))));
}

#[test]
fn integral_of_polynomials() {
    // int (3 x^2 + 2 x + 5) dx = x^3 + x^2 + 5 x
    assert_eq!(
        integrate_polynomial(&sy("3*x^2+2*x+5"), "x").unwrap(),
        sy("x^3+x^2+5*x")
    );
    // int a x dx = a x^2 / 2 (symbolic coefficient)
    assert_eq!(
        integrate_polynomial(&sy("a*x"), "x").unwrap(),
        sy("1/2*a*x^2")
    );
}

#[test]
fn integral_of_non_polynomial_is_refused() {
    // 1/x integrates to ln(x): not a polynomial, refused.
    assert!(matches!(
        integrate_polynomial(&sy("x^(-1)"), "x"),
        Err(SymError::Unsupported(_))
    ));
    assert!(matches!(
        integrate_polynomial(&sy("sin(x)"), "x"),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn derivative_of_integral_is_integrand_random() {
    let mut rng = Lcg(3);
    for _ in 0..80 {
        let terms = rng.range(1, 4);
        let mut src = String::new();
        for k in 0..terms {
            let c = rng.range(-7, 7);
            let e = rng.range(0, 5);
            if k > 0 {
                src.push('+');
            }
            src.push_str(&format!("({c})*a*x^{e}"));
        }
        let p = sy(&src);
        let i = integrate_polynomial(&p, "x").unwrap_or_else(|e| panic!("integral of {src}: {e}"));
        let back = expand(&differentiate(&i, "x").unwrap()).unwrap();
        assert_eq!(back, expand(&p).unwrap(), "d/dx of the integral of {src}");
    }
}

#[test]
fn coefficients_of_a_polynomial() {
    // x^2 - 3x + 2 has coefficients 2, -3, 1 (lowest power first)
    let c = coefficients(&sy("x^2-3*x+2"), "x").unwrap();
    assert_eq!(c, vec![sy("2"), sy("-3"), sy("1")]);
    // a symbol that is not the variable stays inside the coefficients
    let k = coefficients(&sy("a*x+b"), "x").unwrap();
    assert_eq!(k, vec![sy("b"), sy("a")]);
}

#[test]
fn assume_positive_makes_sqrt_of_a_square_the_base() {
    // sample 2_rata_2 r4: sqrt(a^2)+sqrt(b^2) under ALL>0 is b + a
    let s = simplify(&sy("(a^2)^(1/2)+(b^2)^(1/2)")).unwrap();
    let p = simplify(&assume_positive(&s).unwrap()).unwrap();
    assert_eq!(p, sy("a+b"));
    // Without the assumption the square root stays.
    assert_ne!(s, sy("a+b"));
}

/// Independent numeric oracle for a canonical value in the single variable `x` (f64 arithmetic).
fn num_at(s: &Sym, x: f64) -> f64 {
    match s {
        Sym::Num(q) => crate::symbolic::to_f64(q),
        Sym::Symbol(n) => {
            assert_eq!(n, "x", "only x is a free name in these tests");
            x
        }
        Sym::Unit(_) => f64::NAN,
        Sym::Add(ts) => ts.iter().map(|t| num_at(t, x)).sum(),
        Sym::Mul(fs) => fs.iter().map(|t| num_at(t, x)).product(),
        Sym::Pow(b, e) => num_at(b, x).powf(crate::symbolic::to_f64(e)),
        Sym::Func(name, args) => {
            let u = num_at(&args[0], x);
            match name.as_str() {
                "sin" => u.sin(),
                "cos" => u.cos(),
                "tan" => u.tan(),
                "exp" => u.exp(),
                "ln" => u.ln(),
                "sinh" => u.sinh(),
                "cosh" => u.cosh(),
                "tanh" => u.tanh(),
                "asin" => u.asin(),
                "acos" => u.acos(),
                "atan" => u.atan(),
                other => panic!("no numeric rule for {other}"),
            }
        }
    }
}

/// Central difference of `f` at `x`.
fn slope_at(f: &dyn Fn(f64) -> f64, x: f64) -> f64 {
    let h = 1e-5;
    (f(x + h) - f(x - h)) / (2.0 * h)
}

#[test]
fn derivatives_of_the_inverse_and_hyperbolic_functions_match_finite_differences() {
    for src in [
        "tan(2*x)",
        "asin(x/2)",
        "acos(x/2)",
        "atan(x^2)",
        "sinh(x)",
        "cosh(3*x)",
        "tanh(x)",
        "x^(1/2)",
        "ln(1+x^2)",
    ] {
        let f = sy(src);
        let df = differentiate(&f, "x").unwrap_or_else(|e| panic!("{src}: {e}"));
        for x in [0.3, 0.6] {
            let want = slope_at(&|t: f64| num_at(&f, t), x);
            let got = num_at(&df, x);
            assert!(
                (got - want).abs() < 1e-6 * (1.0 + want.abs()),
                "d/dx {src} at {x}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn integrals_of_elementary_forms_differentiate_back_to_the_integrand() {
    // The antiderivative is compared by its derivative, so any constant offset would show up
    // only here: 1/(2x+1) integrates to ln(2x+1)/2 (exact form checked below).
    for integrand in [
        "exp(2*x+1)",
        "sin(3*x)",
        "1/x",
        "1/(2*x+1)",
        "(x+1)^(1/2)",
        "ln(x)",
    ] {
        let f = sy(integrand);
        let big_f = integrate(&f, "x").unwrap_or_else(|e| panic!("{integrand}: {e}"));
        let back = expand(&differentiate(&big_f, "x").unwrap()).unwrap();
        let want = expand(&simplify(&f).unwrap()).unwrap();
        if back == want {
            continue;
        }
        // Different canonical forms of the same value (1/(2x+1) against (2x+1)^-1 with the
        // factor 2 kept): compared at sample points.
        for x in [0.4, 0.9] {
            assert!(
                (num_at(&back, x) - num_at(&want, x)).abs() < 1e-12,
                "d/dx of the integral of {integrand} at {x}"
            );
        }
    }
    // The exact textbook form for the exponential and the 1/x case.
    assert_eq!(
        integrate(&sy("exp(2*x+1)"), "x").unwrap(),
        simplify(&sy("exp(2*x+1)/2")).unwrap()
    );
    assert_eq!(
        integrate(&sy("1/x"), "x").unwrap(),
        simplify(&sy("ln(x)")).unwrap()
    );
}

#[test]
fn integrals_outside_the_table_are_refused() {
    // Parts, arctangent and a product of two x-dependent factors are not in this stage.
    for src in ["x*sin(x)", "1/(x^2+1)", "sin(x^2)", "tan(x)", "x*exp(x^2)"] {
        assert!(
            matches!(integrate(&sy(src), "x"), Err(SymError::Unsupported(_))),
            "{src} must be refused"
        );
    }
}

#[test]
fn definite_integrals_match_the_numeric_value() {
    // int_0^1 (3x^2 + exp(x)) dx = 1 + (e - 1) = e
    let v = definite_integral(&sy("3*x^2+exp(x)"), "x", &sy("0"), &sy("1")).unwrap();
    assert!((num_at(&v, 0.0) - std::f64::consts::E).abs() < 1e-12, "{v}");
    // int_1^2 1/x dx = ln 2
    let v = definite_integral(&sy("1/x"), "x", &sy("1"), &sy("2")).unwrap();
    assert!((num_at(&v, 0.0) - 2f64.ln()).abs() < 1e-12, "{v}");
    // int_0^2 (3x^2+1) dx = 10 exactly
    assert_eq!(
        definite_integral(&sy("3*x^2+1"), "x", &sy("0"), &sy("2")).unwrap(),
        sy("10")
    );
}

#[test]
fn definite_integrals_that_diverge_or_are_complex_are_refused() {
    // The logarithm at 0 (diverges) and at -1 (complex) is refused, never a number.
    assert!(definite_integral(&sy("1/x"), "x", &sy("0"), &sy("1")).is_err());
    assert!(definite_integral(&sy("1/x"), "x", &sy("-1"), &sy("1")).is_err());
    // A bound with the variable is invalid.
    assert!(definite_integral(&sy("x"), "x", &sy("x"), &sy("1")).is_err());
    // 1/x^2 over -1..1 diverges at 0 (F = -1/x would give -2): refused, not returned.
    assert!(definite_integral(&sy("1/x^2"), "x", &sy("-1"), &sy("1")).is_err());
    // The same integrand where it converges, and an integrable singularity at the bound.
    let v = definite_integral(&sy("1/x^2"), "x", &sy("1"), &sy("2")).unwrap();
    assert!((num_at(&v, 0.0) - 0.5).abs() < 1e-12, "{v}");
    let v = definite_integral(&sy("1/x^2"), "x", &sy("-2"), &sy("-1")).unwrap();
    assert!((num_at(&v, 0.0) - 0.5).abs() < 1e-12, "{v}");
    let v = definite_integral(&sy("x^(-1/2)"), "x", &sy("0"), &sy("1")).unwrap();
    assert!((num_at(&v, 0.0) - 2.0).abs() < 1e-12, "{v}");
    // A divergent power at a symbolic bound cannot be decided: refused.
    assert!(definite_integral(&sy("1/x^2"), "x", &sy("a"), &sy("1")).is_err());
}

#[test]
fn integral_then_derivative_gives_the_integrand_on_random_linear_arguments() {
    // Property: whenever an integral is returned, its derivative is the integrand (numerically at
    // two points), and most of the table forms are returned (not refused).
    let mut rng = Lcg(29);
    let mut returned = 0;
    for _ in 0..240 {
        let p = rng.range(1, 4);
        let q = rng.range(1, 4);
        let c = rng.range(1, 3);
        let u = format!("({p})*x+({q})");
        let forms = [
            format!("({c})*exp({u})"),
            format!("({c})*sin({u})"),
            format!("({c})*cosh({u})"),
            format!("({c})/({u})"),
            format!("({c})*({u})^(1/2)"),
            format!("({c})*({u})^(-2)"),
        ];
        for src in forms {
            let f = sy(&src);
            let Ok(big_f) = integrate(&f, "x") else {
                continue;
            };
            returned += 1;
            for x in [0.4, 0.9] {
                let want = num_at(&f, x);
                let got = slope_at(&|t: f64| num_at(&big_f, t), x);
                assert!(
                    (got - want).abs() < 1e-5 * (1.0 + want.abs()),
                    "d/dx of the integral of {src} at {x}: {got} vs {want}"
                );
            }
        }
    }
    assert!(returned > 1000, "only {returned} integrals returned");
}

#[test]
fn definite_integrals_with_complex_values_at_a_bound_are_refused() {
    // The antiderivative has a fractional power of a negative bound: the value is complex.
    for (src, lo, hi) in [
        ("x^(-1/2)", "-1", "1"),
        ("x^(1/2)", "-1", "1"),
        ("(x-3)^(1/2)", "0", "1"),
        ("(x+1)^(3/2)", "-2", "-1"),
    ] {
        assert!(
            definite_integral(&sy(src), "x", &sy(lo), &sy(hi)).is_err(),
            "{src} over {lo}..{hi} must be refused"
        );
    }
}

#[test]
fn definite_integrals_that_stay_real_keep_their_values() {
    // int_0^1 x^(1/2) dx = 2/3
    assert_eq!(
        definite_integral(&sy("x^(1/2)"), "x", &sy("0"), &sy("1")).unwrap(),
        simplify(&sy("2/3")).unwrap()
    );
    // int_4^7 (x-3)^(1/2) dx = 2/3 (8 - 1) = 14/3: the base is positive on the interval
    assert_eq!(
        definite_integral(&sy("(x-3)^(1/2)"), "x", &sy("4"), &sy("7")).unwrap(),
        simplify(&sy("14/3")).unwrap()
    );
    // int_-1^0 (x+1)^(3/2) dx = 2/5: the lower bound gives 0
    assert_eq!(
        definite_integral(&sy("(x+1)^(3/2)"), "x", &sy("-1"), &sy("0")).unwrap(),
        simplify(&sy("2/5")).unwrap()
    );
}

#[test]
fn linear_reciprocal_integrates_with_the_denominator_as_written() {
    // int 1/(2x+1) dx = ln(2x+1)/2, int 1/(1-2x) dx = -ln(1-2x)/2 (Mathcad's linear form)
    for (src, want) in [
        ("1/(2*x+1)", "1/2*ln(2*x+1)"),
        ("1/(1-2*x)", "-1/2*ln(1-2*x)"),
        ("1/x", "ln(x)"),
    ] {
        let big_f = integrate(&sy(src), "x").unwrap();
        assert_eq!(big_f, simplify(&sy(want)).unwrap(), "integral of {src}");
    }
}

#[test]
fn logarithm_of_a_linear_argument_is_integrated_and_verified() {
    // int ln(2x-1) dx = (2x-1)(ln(2x-1) - 1)/2: refused before the verification was fixed.
    let big_f = integrate(&sy("ln(2*x-1)"), "x").unwrap();
    for x in [1.2, 2.5] {
        let want = num_at(&sy("ln(2*x-1)"), x);
        let got = slope_at(&|t: f64| num_at(&big_f, t), x);
        assert!((got - want).abs() < 1e-6 * (1.0 + want.abs()), "at {x}");
    }
}

#[test]
fn definite_results_fold_exact_constants() {
    // int_2^1 1/x dx = ln(1) - ln(2) = -ln(2)
    assert_eq!(
        definite_integral(&sy("1/x"), "x", &sy("2"), &sy("1")).unwrap(),
        simplify(&sy("-ln(2)")).unwrap()
    );
    // int_0^pi sin(x) dx = 2 and int_0^(pi/2) cos(x) dx = 1
    assert_eq!(
        definite_integral(&sy("sin(x)"), "x", &sy("0"), &sy("pi")).unwrap(),
        simplify(&sy("2")).unwrap()
    );
    assert_eq!(
        definite_integral(&sy("cos(x)"), "x", &sy("0"), &sy("pi/2")).unwrap(),
        simplify(&sy("1")).unwrap()
    );
    // int_0^(pi/6) sin(x) dx = 1 - cos(pi/6) = 1 - 3^(1/2)/2
    assert_eq!(
        definite_integral(&sy("sin(x)"), "x", &sy("0"), &sy("pi/6")).unwrap(),
        simplify(&sy("1-3^(1/2)/2")).unwrap()
    );
    // int_0^pi sin(x) dx is also exact through the bound 0: sin(0) = 0 folds too
    assert_eq!(
        definite_integral(&sy("cos(x)"), "x", &sy("0"), &sy("0")).unwrap(),
        simplify(&sy("0")).unwrap()
    );
}

/// Simpson's rule for `f` over `[a, b]` with `n` (even) panels.
fn simpson(f: &dyn Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let h = (b - a) / n as f64;
    let mut sum = f(a) + f(b);
    for i in 1..n {
        let w = if i % 2 == 1 { 4.0 } else { 2.0 };
        sum += w * f(a + h * i as f64);
    }
    sum * h / 3.0
}

#[test]
fn definite_integrals_match_simpson_quadrature_on_random_cases() {
    let mut rng = Lcg(41);
    let mut compared = 0;
    let mut draws = 0;
    // Draws until 100 integrals are compared; refused and singular draws are skipped.
    while compared < 100 && draws < 2000 {
        draws += 1;
        // A linear base u = p x + q, with its root -q/p; the integrand is one of a few forms.
        let p = rng.range(1, 2) * if rng.range(0, 1) == 0 { 1 } else { -1 };
        let q = rng.range(-3, 3);
        let (src, root) = match rng.range(0, 4) {
            0 => {
                let n = [-2, -1, 1, 2, 3][rng.range(0, 4) as usize];
                (format!("({p})*x^({n})"), 0.0)
            }
            1 => {
                let r = ["-2", "-1", "1", "2", "1/2", "3/2", "-1/2"][rng.range(0, 6) as usize];
                (format!("(({p})*x+({q}))^({r})"), -q as f64 / p as f64)
            }
            2 => (format!("exp(({p})*x+({q}))"), f64::NAN),
            3 => (format!("sin(({p})*x+({q}))"), f64::NAN),
            _ => (format!("1/(({p})*x+({q}))"), -q as f64 / p as f64),
        };
        let (lo, hi) = (rng.range(-3, 3), rng.range(-3, 3));
        if lo == hi {
            continue;
        }
        let (lo, hi) = (lo.min(hi) as f64, lo.max(hi) as f64);
        // A singular point inside or at an end of the interval is refused, not compared.
        if !root.is_nan() && lo <= root && root <= hi {
            continue;
        }
        let f = sy(&src);
        let Ok(v) = definite_integral(&f, "x", &sy(&format!("{lo}")), &sy(&format!("{hi}"))) else {
            continue;
        };
        let want = simpson(&|t: f64| num_at(&f, t), lo, hi, 4000);
        if !want.is_finite() {
            continue;
        }
        let got = num_at(&v, 0.0);
        assert!(
            (got - want).abs() < 1e-6 * (1.0 + want.abs()),
            "integral of {src} over {lo}..{hi}: symbolic {got}, Simpson {want}"
        );
        compared += 1;
    }
    assert_eq!(
        compared, 100,
        "definite integrals compared in {draws} draws"
    );
}

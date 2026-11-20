//! Special functions: kernels against reference values and identities, and the builtins' unit
//! rules and error paths. Reference values come from CPython's `math` module (`erf`, `erfc`,
//! `gamma`, `lgamma`), from closed forms, or from published tables for Bessel and Airy.

use std::f64::consts::{FRAC_PI_2, PI};

use super::helpers::*;
use crate::eval::builtins::special::{
    special_bessel as bes, special_gamma as gam, special_misc as misc,
};
use crate::eval::{evaluate, Env, EvalError, Value};
use crate::units::Dimension;

fn dimless() -> Dimension {
    Dimension::dimensionless()
}

fn val(name: &str, args: Vec<f64>) -> Result<Value, EvalError> {
    evaluate(&call(name, args.into_iter().map(n).collect()), &Env::new())
}

/// Calls a builtin on real scalar arguments and returns the real dimensionless result.
fn num(name: &str, args: Vec<f64>) -> f64 {
    let v = val(name, args).unwrap_or_else(|e| panic!("{name} failed: {e}"));
    real_in(&v, dimless())
}

fn err(name: &str, args: Vec<f64>) -> EvalError {
    val(name, args).expect_err("expected an error")
}

fn rel(actual: f64, expected: f64) -> f64 {
    ((actual - expected) / expected).abs()
}

// ----- gamma family ----------------------------------------------------------------------------

/// `(x, Gamma(x))` from CPython's `math.gamma`.
const GAMMA_REF: &[(f64, f64)] = &[
    (0.1, 9.51350769866873),
    (1.8, 0.9313837709802427),
    (3.5, 3.323350970447842),
    (5.2, 32.578096050331354),
    (7.75, 3057.822671192606),
    (10.3, 716430.6890623765),
    (12.0, 39916800.0),
    (14.55, 26352222923.752953),
    (17.95, 308279539934923.44),
    (23.05, 1.3134139073797698e21),
    (30.7, 9.528117499079476e31),
    (40.05, 2.451485046012692e46),
    (52.8, 3.654103712427105e67),
    (70.65, 2.7034323269900387e99),
    (96.15, 2.0471579561842782e148),
    (120.8, 2.566068882179458e198),
    (150.55, 5.988169596067935e261),
    (168.4, 1.166676710207928e301),
];

#[test]
fn gamma_matches_cpython_on_reference_points() {
    for &(x, expected) in GAMMA_REF {
        let got = gam::gamma(x).unwrap();
        assert!(
            rel(got, expected) < 1e-13,
            "gamma({x}) = {got}, want {expected}"
        );
    }
}

#[test]
fn gamma_recurrence_holds_on_a_200_point_grid() {
    // Gamma(x + 1) = x Gamma(x), checked on 199 points with 0.1 <= x <= 169.
    for i in 0..199 {
        let x = 0.1 + i as f64 * 0.85;
        let lhs = gam::gamma(x + 1.0).unwrap();
        let rhs = x * gam::gamma(x).unwrap();
        assert!(rel(lhs, rhs) < 1e-13, "recurrence at {x}: {lhs} vs {rhs}");
    }
}

#[test]
fn gamma_reflection_at_negative_half_integers() {
    close(gam::gamma(-0.5).unwrap(), -3.544_907_701_811_032, 1e-13);
    close(gam::gamma(-2.5).unwrap(), -0.945_308_720_482_941_8, 1e-14);
    close(gam::gamma(-3.5).unwrap(), 0.270_088_205_852_269_2, 1e-14);
}

#[test]
fn gamma_builtin_poles_overflow_and_arity() {
    assert!(matches!(err("Γ", vec![0.0]), EvalError::Unsupported(_)));
    assert!(matches!(err("Γ", vec![-3.0]), EvalError::Unsupported(_)));
    assert_eq!(err("Γ", vec![200.0]), EvalError::Overflow);
    assert_eq!(err("Γ", vec![1e300]), EvalError::Overflow);
    close(num("Γ", vec![0.5]), PI.sqrt(), 1e-15);
    assert!(matches!(
        val("Γ", vec![1.0, 2.0, 3.0]),
        Err(EvalError::ArgCount { .. })
    ));
}

#[test]
fn gamma_refuses_dimensioned_and_complex_arguments() {
    let dimensioned = call("Γ", vec![scale(2.0, u("m"))]);
    assert!(matches!(
        evaluate(&dimensioned, &Env::new()),
        Err(EvalError::DimensionNotDimensionless { .. })
    ));
    let complex = call(
        "Γ",
        vec![bin(crate::expr::BinaryOp::Add, n(1.0), mul(n(2.0), v("i")))],
    );
    assert!(matches!(
        evaluate(&complex, &Env::new()),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn ln_gamma_matches_cpython_lgamma() {
    close(num("lnΓ", vec![0.5]), 0.572_364_942_924_700_4, 1e-14);
    close(num("lnΓ", vec![0.1]), 2.252_712_651_734_205_5, 1e-14);
    close(num("lnΓ", vec![100.0]), 359.134_205_369_575_4, 1e-12);
    close(num("lnΓ", vec![1.0]), 0.0, 1e-13);
    close(num("lnΓ", vec![2.0]), 0.0, 1e-13);
    assert!(matches!(err("lnΓ", vec![-1.5]), EvalError::Unsupported(_)));
}

#[test]
fn psi_at_known_points() {
    close(num("Psi", vec![1.0]), -0.577_215_664_901_532_9, 1e-14);
    close(num("Psi", vec![0.5]), -1.963_510_026_021_423_5, 1e-14);
    close(num("Psi", vec![-0.5]), 0.036_489_973_978_576_5, 1e-13);
    assert!(matches!(err("Psi", vec![0.0]), EvalError::Unsupported(_)));
}

#[test]
fn incomplete_gamma_closed_forms() {
    // Gamma(1, x) = e^(-x); Gamma(1/2, x) = sqrt(pi) erfc(sqrt(x)); Gamma(a, 0) = Gamma(a).
    close(num("Γ", vec![1.0, 2.0]), (-2.0f64).exp(), 1e-15);
    close(
        num("Γ", vec![0.5, 2.0]),
        PI.sqrt() * gam::erfc(2f64.sqrt()),
        1e-14,
    );
    close(num("Γ", vec![3.5, 0.0]), gam::gamma(3.5).unwrap(), 1e-14);
    // P + Q = 1 and P(1, x) = 1 - e^(-x) on both sides of the series/continued-fraction switch.
    for &x in &[0.3, 1.0, 2.5, 7.0] {
        let p = gam::gamma_p(2.5, x).unwrap();
        let q = gam::gamma_q(2.5, x).unwrap();
        close(p + q, 1.0, 1e-14);
        close(gam::gamma_p(1.0, x).unwrap(), 1.0 - (-x).exp(), 1e-14);
    }
}

#[test]
fn incomplete_beta_closed_forms() {
    close(num("ibeta", vec![0.5, 1.0, 1.0]), 0.5, 1e-15);
    close(num("ibeta", vec![0.3, 1.0, 1.0]), 0.3, 1e-15);
    close(num("ibeta", vec![0.5, 2.0, 2.0]), 0.5, 1e-14);
    // I_0.3(2, 5) = P(Binomial(6, 0.3) >= 2)
    close(num("ibeta", vec![0.3, 2.0, 5.0]), 0.579_825, 1e-12);
    close(num("ibeta", vec![1.0, 2.0, 5.0]), 1.0, 0.0);
    assert!(matches!(
        err("ibeta", vec![1.5, 2.0, 5.0]),
        EvalError::Unsupported(_)
    ));
}

// ----- error function ----------------------------------------------------------------------------

/// `(x, erf(x), erfc(x))` from CPython's `math`.
const ERF_REF: &[(f64, f64, f64)] = &[
    (0.25, 0.2763263901682369, 0.7236736098317631),
    (0.5, 0.5204998778130465, 0.4795001221869535),
    (1.0, 0.8427007929497148, 0.15729920705028516),
    (1.5, 0.9661051464753108, 0.033894853524689274),
    (1.9, 0.9927904292352574, 0.007209570764742534),
    (2.0, 0.9953222650189527, 0.0046777349810472645),
    (2.5, 0.999593047982555, 0.000406952017444959),
    (3.0, 0.9999779095030015, 2.2090496998585438e-05),
    (4.0, 0.9999999845827421, 1.541725790028002e-08),
    (5.0, 0.9999999999984626, 1.537459794428035e-12),
    (6.0, 1.0, 2.1519736712498913e-17),
    (-1.0, -0.8427007929497148, 1.8427007929497148),
    (-3.0, -0.9999779095030015, 1.9999779095030015),
];

#[test]
fn erf_and_erfc_match_cpython() {
    for &(x, erf_ref, erfc_ref) in ERF_REF {
        assert!(
            rel(gam::erf(x), erf_ref) < 1e-14 || (gam::erf(x) - erf_ref).abs() < 1e-16,
            "erf({x}) = {}, want {erf_ref}",
            gam::erf(x)
        );
        assert!(
            rel(gam::erfc(x), erfc_ref) < 1e-13,
            "erfc({x}) = {}, want {erfc_ref}",
            gam::erfc(x)
        );
    }
}

#[test]
fn erfc_tail_keeps_relative_accuracy() {
    assert!(rel(gam::erfc(10.0), 2.088_487_583_762_545e-45) < 1e-13);
    assert_eq!(gam::erfc(40.0), 0.0);
}

#[test]
fn erf_identities_on_a_200_point_grid() {
    for i in 0..200 {
        let x = -6.0 + i as f64 * 0.06;
        close(gam::erf(x) + gam::erfc(x), 1.0, 2e-15);
        close(gam::erf(-x), -gam::erf(x), 0.0);
    }
}

#[test]
fn erf_builtin_units_and_domain() {
    close(num("erf", vec![0.5]), 0.5204998778130465, 1e-14);
    close(num("erfc", vec![10.0]), 2.088_487_583_762_545e-45, 1e-57);
    let dimensioned = call("erf", vec![scale(1.0, u("m"))]);
    assert!(matches!(
        evaluate(&dimensioned, &Env::new()),
        Err(EvalError::DimensionNotDimensionless { .. })
    ));
    // erf of a vector is element-wise.
    let m = mat(1, 2, vec![n(0.0), n(1.0)]);
    let out = evaluate(&call("erf", vec![m]), &Env::new()).unwrap();
    assert_eq!(matrix(&out).shape(), (1, 2));
}

// ----- Bessel, Airy, Fresnel ---------------------------------------------------------------------

#[test]
fn bessel_values_at_one() {
    close(num("J0", vec![1.0]), 0.765_197_686_557_966_6, 1e-14);
    close(num("J1", vec![1.0]), 0.440_050_585_744_933_55, 1e-14);
    close(num("Y0", vec![1.0]), 0.088_256_964_215_676_97, 1e-13);
    close(num("I0", vec![1.0]), 1.266_065_877_752_008_2, 1e-14);
    close(num("K0", vec![1.0]), 0.421_024_438_240_708_34, 1e-14);
    close(num("K1", vec![1.0]), 0.601_907_230_197_234_6, 1e-13);
}

#[test]
fn bessel_known_values_at_ten() {
    close(num("J0", vec![10.0]), -0.245_935_764_451_348_3, 1e-13);
    close(num("J1", vec![10.0]), 0.043_472_746_168_861_44, 1e-13);
}

#[test]
fn bessel_parity_and_order_zero_at_origin() {
    close(num("J1", vec![-1.0]), -num("J1", vec![1.0]), 1e-15);
    close(num("I0", vec![-1.0]), num("I0", vec![1.0]), 1e-15);
    close(num("Jn", vec![2.0, -2.0]), num("Jn", vec![2.0, 2.0]), 1e-15);
    close(num("J0", vec![0.0]), 1.0, 0.0);
    close(num("Jn", vec![3.0, 0.0]), 0.0, 0.0);
}

#[test]
fn bessel_recurrence_on_a_grid() {
    // J_(n-1)(x) + J_(n+1)(x) = (2n/x) J_n(x), and the same for I with the sign of the recurrence.
    for i in 1..60 {
        let x = 0.5 * i as f64;
        for n in 1..4 {
            let nf = n as f64;
            let lhs = bes::bessel_j(nf - 1.0, x).unwrap() + bes::bessel_j(nf + 1.0, x).unwrap();
            let rhs = 2.0 * nf / x * bes::bessel_j(nf, x).unwrap();
            assert!(
                (lhs - rhs).abs() < 1e-12,
                "J recurrence n={n} x={x}: {lhs} vs {rhs}"
            );
            let ilhs = bes::bessel_i(nf - 1.0, x).unwrap() - bes::bessel_i(nf + 1.0, x).unwrap();
            let irhs = 2.0 * nf / x * bes::bessel_i(nf, x).unwrap();
            assert!(
                (ilhs - irhs).abs() < 1e-12 * irhs.abs().max(1.0),
                "I recurrence n={n} x={x}"
            );
        }
    }
}

#[test]
fn bessel_wronskians() {
    // J1 Y0 - J0 Y1 = 2/(pi x);  I0 K1 + I1 K0 = 1/x.
    for &x in &[0.5, 1.0, 2.0, 5.0, 12.0] {
        let j0 = bes::bessel_j(0.0, x).unwrap();
        let j1 = bes::bessel_j(1.0, x).unwrap();
        let y0 = bes::bessel_y_int(0, x).unwrap();
        let y1 = bes::bessel_y_int(1, x).unwrap();
        close(j1 * y0 - j0 * y1, 2.0 / (PI * x), 1e-12);
        let i0 = bes::bessel_i(0.0, x).unwrap();
        let i1 = bes::bessel_i(1.0, x).unwrap();
        let k0 = bes::bessel_k(0.0, x).unwrap();
        let k1 = bes::bessel_k(1.0, x).unwrap();
        close((i0 * k1 + i1 * k0) * x, 1.0, 1e-12);
    }
}

#[test]
fn half_integer_orders_have_elementary_forms() {
    // J_(1/2)(x) = sqrt(2/(pi x)) sin x, I_(1/2)(x) = sqrt(2/(pi x)) sinh x, K_(1/2)(x) = sqrt(pi/(2x)) e^-x
    for &x in &[0.7, 1.0, 3.0, 9.5] {
        let s = (2.0 / (PI * x)).sqrt();
        close(bes::bessel_j(0.5, x).unwrap(), s * x.sin(), 1e-13);
        close(
            bes::bessel_i(0.5, x).unwrap(),
            s * x.sinh(),
            1e-13 * (s * x.sinh()).abs().max(1.0),
        );
        close(
            bes::bessel_k(0.5, x).unwrap(),
            (PI / (2.0 * x)).sqrt() * (-x).exp(),
            1e-14,
        );
    }
}

#[test]
fn bessel_domain_limits_and_overflow_are_errors() {
    assert!(matches!(err("J0", vec![1e300]), EvalError::Unsupported(_)));
    assert!(matches!(err("Y0", vec![0.0]), EvalError::Unsupported(_)));
    assert!(matches!(err("Y0", vec![-1.0]), EvalError::Unsupported(_)));
    assert_eq!(err("K0", vec![0.0]), EvalError::NotFinite);
    assert_eq!(err("I0", vec![800.0]), EvalError::Overflow);
    assert!(matches!(
        err("Jn", vec![0.5, -1.0]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("Yn", vec![0.5, 1.0]),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn k0_at_tiny_argument_matches_the_log_asymptote() {
    // K0(x) ~ -ln(x/2) - gamma as x -> 0.
    let x = 1e-300;
    close(
        num("K0", vec![x]),
        -(x / 2.0).ln() - 0.577_215_664_901_532_9,
        1e-10,
    );
}

#[test]
fn hankel_functions_combine_j_and_y() {
    let h1 = val("H1", vec![0.0, 1.0]).unwrap();
    let s = scalar(&h1);
    close(s.value.re, 0.765_197_686_557_966_6, 1e-14);
    close(s.value.im, 0.088_256_964_215_676_97, 1e-13);
}

#[test]
fn airy_values_at_known_points() {
    close(num("Ai", vec![0.0]), 0.355_028_053_887_817_2, 1e-14);
    close(num("Bi", vec![0.0]), 0.614_926_627_446_000_7, 1e-14);
    close(num("Ai", vec![1.0]), 0.135_292_416_312_881_4, 1e-14);
    close(num("Bi", vec![1.0]), 1.207_423_594_952_871_3, 1e-13);
    close(num("Ai", vec![-1.0]), 0.535_560_883_292_352_1, 1e-13);
    close(num("Bi", vec![-1.0]), 0.103_997_389_496_944_6, 1e-13);
    close(num("Ai", vec![2.0]), 0.034_924_130_423_274_37, 1e-14);
    close(num("Ai", vec![-2.0]), 0.227_407_428_201_685_57, 1e-13);
}

#[test]
fn airy_derivatives_are_refused() {
    assert!(matches!(err("DAi", vec![1.0]), EvalError::Unsupported(_)));
    assert!(matches!(err("DBi", vec![1.0]), EvalError::Unsupported(_)));
}

#[test]
fn fresnel_values_and_symmetry() {
    let (c, s) = bes::fresnel(1.0).unwrap();
    close(c, 0.779_893_400_376_822_8, 1e-12);
    close(s, 0.438_259_147_390_354_7, 1e-12);
    assert_eq!(bes::fresnel(0.0).unwrap(), (0.0, 0.0));
    let (c_neg, s_neg) = bes::fresnel(-1.0).unwrap();
    close(c_neg, -c, 1e-15);
    close(s_neg, -s, 1e-15);
    assert!(matches!(
        err("FresnelC", vec![5000.0]),
        EvalError::Unsupported(_)
    ));
}

// ----- Lambert W, delta, Heaviside -------------------------------------------------------------

#[test]
fn lambert_w_known_points_and_identity() {
    close(num("LambertW", vec![1.0]), 0.567_143_290_409_783_8, 1e-15);
    close(num("LambertW", vec![0.0]), 0.0, 0.0);
    close(num("LambertW", vec![std::f64::consts::E]), 1.0, 1e-15);
    close(
        num("LambertW", vec![-1.0 / std::f64::consts::E]),
        -1.0,
        1e-15,
    );
    close(
        num("LambertW", vec![0.0, 1.0]),
        0.567_143_290_409_783_8,
        1e-15,
    );
    close(
        num("LambertW", vec![-1.0, -0.2]),
        -2.542_641_357_773_526_5,
        1e-13,
    );
    for i in 0..50 {
        let x = -0.36 + i as f64 * 0.5;
        let w = misc::lambert_w(0, x).unwrap();
        close(w * w.exp(), x, 1e-13 * x.abs().max(1e-3));
    }
}

#[test]
fn lambert_w_domain_and_branches() {
    assert!(matches!(
        err("LambertW", vec![-0.5]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("LambertW", vec![2.0, 1.0]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("LambertW", vec![-1.0, 1.0]),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn kronecker_delta_and_heaviside() {
    close(num("δ", vec![2.0, 2.0]), 1.0, 0.0);
    close(num("δ", vec![2.0, 3.0]), 0.0, 0.0);
    close(num("Φ", vec![-1.0]), 0.0, 0.0);
    close(num("Φ", vec![0.0]), 0.5, 0.0);
    close(num("Φ", vec![2.0]), 1.0, 0.0);
    assert!(matches!(
        err("δ", vec![1.5, 2.0]),
        EvalError::Unsupported(_)
    ));
}

// ----- orthogonal polynomials --------------------------------------------------------------------

#[test]
fn polynomials_match_their_closed_forms() {
    for &x in &[-1.7, -0.3, 0.0, 0.6, 2.2] {
        close(
            misc::polynomial(misc::Poly::Legendre, 2, x).unwrap(),
            (3.0 * x * x - 1.0) / 2.0,
            1e-14,
        );
        close(
            misc::polynomial(misc::Poly::Hermite, 2, x).unwrap(),
            4.0 * x * x - 2.0,
            1e-13,
        );
        close(
            misc::polynomial(misc::Poly::Hermite, 3, x).unwrap(),
            8.0 * x.powi(3) - 12.0 * x,
            1e-12,
        );
        close(
            misc::polynomial(misc::Poly::Laguerre, 2, x).unwrap(),
            (x * x - 4.0 * x + 2.0) / 2.0,
            1e-13,
        );
        close(
            misc::polynomial(misc::Poly::ChebyshevT, 3, x).unwrap(),
            4.0 * x.powi(3) - 3.0 * x,
            1e-13,
        );
        close(
            misc::polynomial(misc::Poly::ChebyshevU, 2, x).unwrap(),
            4.0 * x * x - 1.0,
            1e-13,
        );
        for n in 0..6 {
            close(
                misc::jacobi(n, 0.0, 0.0, x).unwrap(),
                misc::polynomial(misc::Poly::Legendre, n, x).unwrap(),
                1e-12,
            );
        }
    }
}

#[test]
fn polynomial_builtins_check_the_degree() {
    close(num("Leg", vec![3.0, 0.5]), -0.4375, 1e-15);
    assert!(matches!(
        err("Leg", vec![-1.0, 0.5]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("Leg", vec![1.5, 0.5]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("Leg", vec![1e7, 0.5]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("Jac", vec![2.0, -1.0, 0.0, 0.5]),
        EvalError::Unsupported(_)
    ));
}

// ----- elliptic integrals --------------------------------------------------------------------------

/// Midpoint-rule reference for `K(m)` and `E(m)` on `[0, pi/2]` with a large panel count.
fn elliptic_reference(m: f64) -> (f64, f64) {
    let steps = 200_000;
    let h = FRAC_PI_2 / steps as f64;
    let (mut k, mut e) = (0.0, 0.0);
    for i in 0..steps {
        let t = (i as f64 + 0.5) * h;
        let s = (m * t.sin().powi(2)).min(1.0);
        k += 1.0 / (1.0 - s).sqrt();
        e += (1.0 - s).sqrt();
    }
    (k * h, e * h)
}

#[test]
fn complete_elliptic_integrals() {
    let (k0, e0) = misc::elliptic_ke(0.0).unwrap();
    close(k0, PI / 2.0, 1e-15);
    close(e0, PI / 2.0, 1e-15);
    close(num("ellipticK", vec![0.5]), 1.854_074_677_301_371_9, 1e-14);
    close(num("ellipticE", vec![0.5]), 1.350_643_881_047_675_5, 1e-14);
    for &m in &[-1.0, 0.3, 0.9] {
        let (k, e) = misc::elliptic_ke(m).unwrap();
        let (kr, er) = elliptic_reference(m);
        close(k, kr, 1e-9);
        close(e, er, 1e-9);
    }
    assert!(matches!(
        err("ellipticK", vec![1.0]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("ellipticK", vec![2.0]),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn incomplete_elliptic_integrals_reduce_to_complete_ones() {
    let (k, e) = misc::elliptic_ke(0.5).unwrap();
    let (f1, e1) = misc::elliptic_incomplete(1.0, 0.5).unwrap();
    close(f1, k, 1e-10);
    close(e1, e, 1e-10);
    close(num("ellipticF", vec![0.5, 0.0]), PI / 6.0, 1e-14);
}

// ----- trigonometric and hyperbolic names --------------------------------------------------------

#[test]
fn reciprocal_trig_and_inverses() {
    close(num("csc", vec![FRAC_PI_2]), 1.0, 1e-15);
    close(num("sec", vec![0.0]), 1.0, 0.0);
    close(num("cot", vec![PI / 4.0]), 1.0, 1e-15);
    close(num("acsc", vec![2.0]), PI / 6.0, 1e-15);
    close(num("asec", vec![2.0]), PI / 3.0, 1e-15);
    close(num("acot", vec![1.0]), PI / 4.0, 1e-15);
    close(num("acot", vec![-1.0]), 3.0 * PI / 4.0, 1e-15);
    close(num("sinc", vec![0.0]), 1.0, 0.0);
    close(num("sinc", vec![PI]), 0.0, 1e-15);
    assert_eq!(err("csc", vec![0.0]), EvalError::NotFinite);
}

#[test]
fn hyperbolic_reciprocals_and_inverses() {
    close(num("csch", vec![1.0]), 1.0 / 1f64.sinh(), 1e-15);
    close(num("sech", vec![0.0]), 1.0, 0.0);
    close(num("coth", vec![1.0]), 1.0 / 1f64.tanh(), 1e-15);
    close(num("asinh", vec![1.0]), 0.881_373_587_019_543, 1e-15);
    close(num("acosh", vec![2.0]), 1.316_957_896_924_816_6, 1e-15);
    close(num("atanh", vec![0.5]), 0.549_306_144_334_054_8, 1e-15);
    close(num("acsch", vec![1.0]), 1f64.recip().asinh(), 1e-15);
    close(num("asech", vec![0.5]), 2f64.acosh(), 1e-15);
    close(num("acoth", vec![2.0]), 0.5 * 3f64.ln(), 1e-15);
}

// ----- wiring and hostile inputs ---------------------------------------------------------------

#[test]
fn every_special_name_is_registered() {
    let names = [
        "Γ",
        "lnΓ",
        "Psi",
        "ibeta",
        "erf",
        "erfc",
        "J0",
        "J1",
        "Jn",
        "Y0",
        "Y1",
        "Yn",
        "I0",
        "I1",
        "In",
        "K0",
        "K1",
        "Kn",
        "H1",
        "H2",
        "Ai",
        "Bi",
        "DAi",
        "DBi",
        "FresnelC",
        "FresnelS",
        "LambertW",
        "δ",
        "Φ",
        "Her",
        "Lag",
        "Leg",
        "Tcheb",
        "Ucheb",
        "Jac",
        "ellipticK",
        "ellipticE",
        "ellipticF",
        "ellipticPi",
        "csc",
        "sec",
        "cot",
        "csch",
        "sech",
        "coth",
        "asec",
        "acsc",
        "acot",
        "asinh",
        "acosh",
        "atanh",
        "acsch",
        "asech",
        "acoth",
        "sinc",
    ];
    for name in names {
        let got = val(name, vec![]);
        assert!(
            !matches!(got, Err(EvalError::UndefinedFunction(_))),
            "{name} is not registered"
        );
    }
}

#[test]
fn unimplemented_names_say_so() {
    assert!(matches!(
        err("js", vec![1.0, 1.0]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("bei", vec![0.0, 1.0]),
        EvalError::Unsupported(_)
    ));
    assert!(matches!(
        err("ellipticPi", vec![0.5, 0.5]),
        EvalError::Unsupported(_)
    ));
}

#[test]
fn large_arguments_finish_quickly() {
    assert!(matches!(err("Y1", vec![1e300]), EvalError::Unsupported(_)));
    assert!(matches!(err("Ai", vec![1e300]), EvalError::Unsupported(_)));
    assert_eq!(num("erf", vec![1e10]), 1.0);
}

//! Exact complex numbers: arithmetic, powers, modulus, argument, principal powers and Euler
//! forms. Identities are exact where the value is exact; numeric checks use 1e-9.

use super::{approx, sy, Lcg};
use crate::symbolic::num::{int, Q};
use crate::symbolic::{euler, expand, Cplx, Sym, SymError};

fn z(re: &str, im: &str) -> Cplx {
    Cplx::new(sy(re), sy(im)).expect("complex builds")
}

fn same(a: &Sym, b: &Sym) -> bool {
    crate::symbolic::ratfunc::simplify(&a.clone().minus(b.clone()).unwrap())
        .unwrap()
        .is_zero()
}

fn same_c(a: &Cplx, b: &Cplx) -> bool {
    same(&a.re, &b.re) && same(&a.im, &b.im)
}

fn close(a: &Cplx, re: f64, im: f64) -> bool {
    (approx(&a.re) - re).abs() < 1e-9 && (approx(&a.im) - im).abs() < 1e-9
}

#[test]
fn powers_of_the_imaginary_unit() {
    let i = Cplx::i();
    assert!(same_c(&i.pow_int(2).unwrap(), &z("-1", "0")));
    assert!(same_c(&i.pow_int(3).unwrap(), &z("0", "-1")));
    assert!(same_c(&i.pow_int(4).unwrap(), &z("1", "0")));
    assert!(same_c(&i.pow_int(-1).unwrap(), &z("0", "-1")));
}

#[test]
fn small_products_and_powers() {
    let one_i = z("1", "1");
    assert!(same_c(&one_i.pow_int(2).unwrap(), &z("0", "2")));
    assert!(same_c(&one_i.mul(&z("1", "-1")).unwrap(), &z("2", "0")));
    assert!(same_c(&z("3", "4").conj().unwrap(), &z("3", "-4")));
    let q = z("3", "4").div(&z("1", "2")).unwrap();
    assert!(same_c(&q, &z("11/5", "-2/5")));
    assert!(same_c(
        &z("1", "1").div(&z("1", "-1")).unwrap(),
        &z("0", "1")
    ));
}

#[test]
fn modulus_and_argument_when_exact() {
    assert!(same(&z("3", "4").modulus().unwrap(), &sy("5")));
    assert!(same(&z("1", "2").modulus().unwrap(), &sy("5^(1/2)")));
    assert!(same(&z("0", "1").arg().unwrap(), &sy("pi/2")));
    assert!(same(&z("-1", "0").arg().unwrap(), &sy("pi")));
    assert!(same(&z("0", "-1").arg().unwrap(), &sy("-pi/2")));
    assert!(same(&z("1", "1").arg().unwrap(), &sy("pi/4")));
    assert!(same(&z("-1", "-1").arg().unwrap(), &sy("-3*pi/4")));
    assert!(matches!(z("1", "2").arg(), Err(SymError::Unsupported(_))));
}

#[test]
fn principal_powers_follow_the_help() {
    // sqrt(-4) = 2i
    let r = Cplx::real(sy("-4"))
        .pow_rational(&Q::new(1.into(), 2.into()))
        .unwrap();
    assert!(same_c(&r, &z("0", "2")));
    // (-1)^(1/2) = i
    let r = Cplx::real(sy("-1"))
        .pow_rational(&Q::new(1.into(), 2.into()))
        .unwrap();
    assert!(same_c(&r, &z("0", "1")));
    // (-1)^(1/3) = 0.5 + 0.866i (principal value, help "Complex Results")
    let r = Cplx::real(sy("-1"))
        .pow_rational(&Q::new(1.into(), 3.into()))
        .unwrap();
    assert!(close(&r, 0.5, 3f64.sqrt() / 2.0), "got {r:?}");
    // (1+i)^(1/2) has an argument outside the table: refused.
    assert!(matches!(
        z("1", "1").pow_rational(&Q::new(1.into(), 2.into())),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn euler_forms() {
    assert!(same_c(&euler(&int(1)).unwrap(), &z("-1", "0")));
    assert!(same_c(
        &euler(&Q::new(1.into(), 2.into())).unwrap(),
        &z("0", "1")
    ));
    assert!(same_c(&euler(&int(0)).unwrap(), &z("1", "0")));
    let t = euler(&Q::new(1.into(), 3.into())).unwrap();
    assert!(close(&t, 0.5, 3f64.sqrt() / 2.0));
    let s = euler(&Q::new(1.into(), 4.into())).unwrap();
    assert!(close(&s, 0.5f64.sqrt(), 0.5f64.sqrt()));
    assert!(matches!(
        euler(&Q::new(1.into(), 7.into())),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn symbolic_square_expands_to_real_and_imaginary_parts() {
    let a = Cplx::new(sy("a"), sy("b")).unwrap();
    let sq = a.pow_int(2).unwrap();
    assert!(same(&expand(&sq.re).unwrap(), &sy("a^2-b^2")));
    assert!(same(&expand(&sq.im).unwrap(), &sy("2*a*b")));
    assert!(same(&a.norm_sq().unwrap(), &sy("a^2+b^2")));
}

#[test]
fn symbolic_division_and_modulus_are_refused() {
    assert!(matches!(
        z("1", "1").div(&Cplx::new(sy("a"), sy("1")).unwrap()),
        Err(SymError::Unsupported(_))
    ));
    assert!(matches!(
        Cplx::new(sy("a"), sy("1")).unwrap().modulus(),
        Err(SymError::Unsupported(_))
    ));
    assert!(matches!(
        z("1", "0").div(&z("0", "0")),
        Err(SymError::Undefined(_))
    ));
}

#[test]
fn random_gaussian_identities_hold_numerically() {
    let mut rng = Lcg(9);
    for _ in 0..200 {
        let a = (rng.range(-5, 5), rng.range(-5, 5));
        let b = (rng.range(-5, 5), rng.range(-5, 5));
        if b == (0, 0) {
            continue;
        }
        let za = z(&format!("{}", a.0), &format!("{}", a.1));
        let zb = z(&format!("{}", b.0), &format!("{}", b.1));
        let prod = za.mul(&zb).unwrap();
        let (ar, ai, br, bi) = (a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64);
        assert!(close(&prod, ar * br - ai * bi, ar * bi + ai * br));
        let quot = za.div(&zb).unwrap();
        let back = quot.mul(&zb).unwrap();
        assert!(same_c(&back, &za), "(a/b)*b = a for {a:?} {b:?}");
        let n = za.norm_sq().unwrap();
        assert!((approx(&n) - (ar * ar + ai * ai)).abs() < 1e-9);
        // |z|^2 = z * conj(z), exactly
        assert!(same_c(
            &za.mul(&za.conj().unwrap()).unwrap(),
            &Cplx::real(n)
        ));
    }
}

#[test]
fn powers_with_surd_parts() {
    let w = Cplx::new(sy("1"), sy("2^(1/2)")).unwrap();
    let sq = w.pow_int(2).unwrap();
    assert!(same(&sq.re, &sy("-1")));
    assert!(same(&sq.im, &sy("2*2^(1/2)")));
}

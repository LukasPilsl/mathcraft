//! `float, n` rounding: exact on the rational, half away from zero (`round_significant`), and
//! the exact binary value of a double (`from_f64_exact`).

use crate::symbolic::num::int;
use crate::symbolic::{from_f64_exact, parse_decimal, round_significant, to_f64, SymError, Q};

fn dec(s: &str) -> Q {
    parse_decimal(s).expect("decimal literal")
}

#[test]
fn help_examples_round_half_away_from_zero() {
    // help: 7.5675 with float,4 -> 7.568 and -7.5675 -> -7.568
    assert_eq!(round_significant(&dec("7.5675"), 4).unwrap(), dec("7.568"));
    assert_eq!(
        round_significant(&dec("-7.5675"), 4).unwrap(),
        dec("-7.568")
    );
    // Ties go away from zero, not to even: 2.5 -> 3 and 0.125 -> 0.13.
    assert_eq!(round_significant(&dec("2.5"), 1).unwrap(), int(3));
    assert_eq!(round_significant(&dec("-2.5"), 1).unwrap(), int(-3));
    assert_eq!(round_significant(&dec("0.125"), 2).unwrap(), dec("0.13"));
    // 0.5 already has one significant figure: it stays 0.5.
    assert_eq!(round_significant(&dec("0.5"), 1).unwrap(), dec("0.5"));
}

#[test]
fn large_and_small_magnitudes() {
    assert_eq!(
        round_significant(&dec("123456789"), 3).unwrap(),
        dec("123000000")
    );
    assert_eq!(
        round_significant(&dec("0.000123456"), 3).unwrap(),
        dec("0.000123")
    );
    // A carry into the next decade: 9.9999 to 2 figures is 10.
    assert_eq!(round_significant(&dec("9.9999"), 2).unwrap(), int(10));
    assert_eq!(
        round_significant(&dec("1.23456e-20"), 3).unwrap(),
        dec("1.23e-20")
    );
    assert_eq!(
        round_significant(&dec("1.235e30"), 3).unwrap(),
        dec("1.24e30")
    );
    assert_eq!(round_significant(&dec("0"), 3).unwrap(), int(0));
}

#[test]
fn magnitudes_outside_the_bounds_are_refused() {
    assert!(matches!(
        round_significant(&dec("1e-70"), 3),
        Err(SymError::Unsupported(_))
    ));
    assert!(matches!(
        round_significant(&dec("1e301"), 1),
        Err(SymError::Unsupported(_))
    ));
}

#[test]
fn double_values_are_exact_binary_fractions() {
    assert_eq!(from_f64_exact(0.5).unwrap(), dec("0.5"));
    assert_eq!(from_f64_exact(-3.0).unwrap(), int(-3));
    let tenth = from_f64_exact(0.1).unwrap();
    assert_eq!(to_f64(&tenth), 0.1);
    assert!(from_f64_exact(f64::NAN).is_none());
    assert!(from_f64_exact(f64::INFINITY).is_none());
}

#[test]
fn sqrt5_to_six_figures_through_the_double_value() {
    // sqrt(5) = 2.2360679... -> 2.23607 (help: float of a surd), by way of the exact double.
    let exact = from_f64_exact(5f64.sqrt()).unwrap();
    assert_eq!(round_significant(&exact, 6).unwrap(), dec("2.23607"));
}

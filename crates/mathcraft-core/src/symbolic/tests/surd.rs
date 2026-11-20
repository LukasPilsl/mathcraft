//! Square roots of fractions and their squarefree split (regression for `sqrt_split`).

use super::{eqn, sy, Lcg};
use crate::symbolic::num::{from_i128, half, int, sqrt_split, Q};
use crate::symbolic::{solve_polynomial, Sym};
use num_traits::{Signed, ToPrimitive};

fn root2_times(coef: Q) -> Sym {
    Sym::Mul(vec![
        Sym::Num(coef),
        Sym::Pow(Box::new(Sym::int(2)), half()),
    ])
}

#[test]
fn sqrt_of_one_eighth_is_a_quarter_times_root_two() {
    // sqrt(1/8) = sqrt(8)/8 = 2*sqrt(2)/8 = sqrt(2)/4 (the old code gave 1/8).
    assert_eq!(sy("sqrt(1/8)"), root2_times(int(1) / int(4)));
}

#[test]
fn sqrt_of_two_ninths_is_a_third_times_root_two() {
    // sqrt(2/9) = sqrt(2)/3 (the old code gave 1/9*2^(1/2)).
    assert_eq!(sy("sqrt(2/9)"), root2_times(int(1) / int(3)));
}

#[test]
fn quadratic_with_fractional_constant_has_correct_surd_roots() {
    // x^2 + x + 1/8 = 0 -> x = -1/2 +- sqrt(2)/4, i.e. -0.854 and -0.146.
    // The old code gave -3/4 and -1/4 (wrong values).
    let roots = solve_polynomial(&eqn("x^2+x+1/8==0"), "x").expect("solve");
    let shown: Vec<String> = roots.iter().map(|r| r.to_string()).collect();
    assert_eq!(shown, vec!["1/4*2^(1/2) - 1/2", "-1/4*2^(1/2) - 1/2"]);
}

/// Whether `d^2` divides `n` for some `2 <= d <= 10 000`, or `n` is a perfect square
/// (for `n < 10^12` the only square factor left after trial division is such a square).
fn has_square_factor(n: i128) -> bool {
    for d in 2..=10_000i128 {
        if n % (d * d) == 0 {
            return true;
        }
    }
    let r = (n as f64).sqrt().round() as i128;
    (r - 1..=r + 1).any(|c| c >= 0 && c * c == n)
}

#[test]
fn sqrt_split_is_exact_and_squarefree_on_random_rationals() {
    let mut rng = Lcg(20_261_116);
    for _ in 0..300 {
        let p = rng.range(1, 1_000_000);
        let q = rng.range(1, 1_000_000);
        let value = from_i128(i128::from(p)) / from_i128(i128::from(q));
        let (outside, inside) = sqrt_split(&value);
        assert!(inside.is_integer() && inside.is_positive(), "p={p} q={q}");
        assert_eq!(
            outside.clone() * outside.clone() * inside.clone(),
            value,
            "p={p} q={q}"
        );
        let n = inside.numer().to_i128().expect("small radicand");
        assert!(!has_square_factor(n), "radicand {n} for p={p} q={q}");
    }
}

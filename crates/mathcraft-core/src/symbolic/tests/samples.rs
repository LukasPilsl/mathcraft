//! The Prime sample values (verified against Prime's stored results) and the systems behind
//! them. Systems are written in the form the evaluator will produce after resolving the
//! worksheet's `id(...)` calls, e.g. `f(2) = 1` becomes `a*2^2 + b*2 + c == 1`.

use super::{eqn, names, sy};
use crate::symbolic::{solve_linear_system, SolveOutcome, Sym};

fn q(n: i64, d: i64) -> Sym {
    Sym::rational(n, d).expect("rational")
}

fn unique(system: &[&str], unknowns: &[&str]) -> Vec<(String, Sym)> {
    let eqs: Vec<_> = system.iter().map(|s| eqn(s)).collect();
    match solve_linear_system(&eqs, &names(unknowns)).expect("solve") {
        SolveOutcome::Unique(v) => v,
        other => panic!("expected a unique solution, got {other:?}"),
    }
}

#[test]
fn sample_1_rata_1_constant() {
    assert_eq!(sy("(3+4)/(5^2-1)"), q(7, 24));
}

#[test]
fn sample_2_rata_2_r0_single_unknown() {
    // 7x + 3x = 5, solve x -> 1/2
    let got = unique(&["7*x+3*x==5"], &["x"]);
    assert_eq!(got, vec![("x".to_string(), q(1, 2))]);
}

#[test]
fn sample_2_rata_2_r15_three_unknowns() {
    // x + 2y - z = 4; x + z = -3; y - 2z = 5 -> x = -3/2, y = 2, z = -3/2
    let got = unique(&["x+2*y-z==4", "x+z==-3", "y-2*z==5"], &["x", "y", "z"]);
    assert_eq!(
        got.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
        vec![q(-3, 2), Sym::int(2), q(-3, 2)]
    );
}

#[test]
fn sample_2_rata_2_r16_two_unknowns() {
    // a + 2b = 7; 2a - b = 2 -> a = 11/5, b = 12/5
    let got = unique(&["a+2*b==7", "2*a-b==2"], &["a", "b"]);
    assert_eq!(got[0].1, q(11, 5));
    assert_eq!(got[1].1, q(12, 5));
}

#[test]
fn sample_2_rata_2_r19_two_unknowns() {
    // c + 2d = 7; c - d = 2 -> c = 11/3, d = 5/3
    let got = unique(&["c+2*d==7", "c-d==2"], &["c", "d"]);
    assert_eq!(got[0].1, q(11, 3));
    assert_eq!(got[1].1, q(5, 3));
}

#[test]
fn sample_3_rata_3_parabola_through_three_points() {
    // f(2) = 1, f(4) = -2, f(5) = 1 with f = a x^2 + b x + c -> a = 3/2, b = -21/2, c = 16
    let got = unique(
        &["a*2^2+b*2+c==1", "a*4^2+b*4+c==-2", "a*5^2+b*5+c==1"],
        &["a", "b", "c"],
    );
    let vals: Vec<Sym> = got.into_iter().map(|(_, v)| v).collect();
    assert_eq!(vals, vec![q(3, 2), q(-21, 2), Sym::int(16)]);
}

#[test]
fn sample_2_rata_2_r14_solve_for_f() {
    // C = 5/9 (F - 32), solve F -> (9C + 160)/5, which is 9/5 C + 32
    let got = unique(&["C==5/9*(F-32)"], &["F"]);
    assert_eq!(got[0].1, sy("9/5*C+32"));
    assert_eq!(got[0].1.to_string(), "9/5*C + 32");
}

#[test]
fn sample_1_rata_1_power_combination_value() {
    assert_eq!(
        sy("x^5*(6*x^3)/(x^(1/2)*x^(5/3))").to_string(),
        "6*x^(35/6)"
    );
}

#[test]
fn sample_orderings_are_reverse_alphabetical() {
    // 1_rata_1 r10 expand (a+b)^2 -> b^2 + 2ab + a^2
    let expanded = crate::symbolic::expand(&sy("(a+b)^2")).expect("expand");
    assert_eq!(expanded.to_string(), "b^2 + 2*a*b + a^2");
    // 1_rata_1 r12: cube difference expands to 75 c^2 d^4 - 45 c^4 d^2 + 7 c^6
    let cube = crate::symbolic::expand(&sy("(2*c^2-5*d^2)^3-(c^2-5*d^2)^3")).expect("expand");
    assert_eq!(cube.to_string(), "75*c^2*d^4 - 45*c^4*d^2 + 7*c^6");
}

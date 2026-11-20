//! Coefficient size and work budgets: hostile products and large systems stop quickly.

use std::time::{Duration, Instant};

use super::{eqn, names};
use crate::symbolic::num::{int, Q};
use crate::symbolic::{solve_linear_system, Equation, SolveOutcome, Sym, SymError};

#[test]
fn product_of_huge_powers_of_two_is_refused_at_the_first_overflow() {
    // 3000 copies of 2^99990: the second multiplication already exceeds 100 000 bits.
    let big: Q = int(2).pow(99_990);
    let factors: Vec<Sym> = (0..3000).map(|_| Sym::Num(big.clone())).collect();
    let start = Instant::now();
    let out = Sym::product(factors);
    let took = start.elapsed();
    assert!(matches!(out, Err(SymError::TooLarge(_))), "{out:?}");
    assert!(took < Duration::from_secs(4), "took {took:?}");
}

#[test]
fn symbolic_six_by_six_system_is_refused_quickly() {
    // Every coefficient is a new symbol: the elimination grows past the work budget.
    let n = 6;
    let eqs: Vec<Equation> = (0..n)
        .map(|i| {
            let text: Vec<String> = (0..n).map(|j| format!("a{i}{j}*x{j}")).collect();
            eqn(&format!("{}==1", text.join("+")))
        })
        .collect();
    let unknowns: Vec<String> = (0..n).map(|j| format!("x{j}")).collect();
    let start = Instant::now();
    let out = solve_linear_system(&eqs, &unknowns);
    let took = start.elapsed();
    assert!(
        matches!(
            out,
            Err(SymError::TooLarge(_)) | Err(SymError::Unsupported(_))
        ),
        "{out:?}"
    );
    assert!(took < Duration::from_secs(12), "took {took:?}");
}

/// Diagonally dominant integer system of size `n` with a known unique solution family.
fn numeric_system(n: usize) -> (Vec<Equation>, Vec<String>) {
    let eqs = (0..n)
        .map(|i| {
            let mut terms = Vec::new();
            for j in 0..n {
                let mut c = ((i * 7 + j * 3) % 11) as i64;
                if i == j {
                    c += 100;
                }
                terms.push(format!("({c})*x{j}"));
            }
            eqn(&format!("{}==({})", terms.join("+"), i + 1))
        })
        .collect();
    (
        eqs,
        names(
            &(0..n)
                .map(|j| format!("x{j}"))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ),
    )
}

#[test]
fn numeric_twenty_by_twenty_system_still_solves() {
    let (eqs, unknowns) = numeric_system(20);
    let start = Instant::now();
    let out = solve_linear_system(&eqs, &unknowns);
    let took = start.elapsed();
    assert!(matches!(out, Ok(SolveOutcome::Unique(_))), "{out:?}");
    assert!(took < Duration::from_secs(20), "took {took:?}");
}

#[test]
fn numeric_sixty_by_sixty_system_succeeds_or_fails_fast() {
    // About 1 million units: refused by the work budget (not run to completion).
    let (eqs, unknowns) = numeric_system(60);
    let start = Instant::now();
    let out = solve_linear_system(&eqs, &unknowns);
    let took = start.elapsed();
    assert!(matches!(out, Err(SymError::TooLarge(_))), "{out:?}");
    assert!(took < Duration::from_secs(40), "took {took:?}");
}

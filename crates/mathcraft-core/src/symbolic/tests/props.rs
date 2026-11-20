//! Property tests with a deterministic generator: normal-form idempotence, expansion
//! identities, and exact random systems verified by substitution.

use super::{names, sy, Lcg};
use crate::symbolic::num::int;
use crate::symbolic::{
    expand, from_expr, solve_linear_system, substitute, to_expr, Equation, SolveOutcome, Sym,
};

/// Random polynomial text in `x`, `y` with small integer coefficients.
fn random_poly_text(rng: &mut Lcg, terms: usize) -> String {
    let vars = ["x", "y", "a"];
    let mut out = String::new();
    for i in 0..terms {
        let c = rng.range(-5, 5);
        let v1 = vars[rng.range(0, 2) as usize];
        let e1 = rng.range(0, 3);
        let v2 = vars[rng.range(0, 2) as usize];
        let e2 = rng.range(0, 2);
        let term = format!("({c})*{v1}^{e1}*{v2}^{e2}");
        if i > 0 {
            out.push_str(" + ");
        }
        out.push_str(&term);
    }
    out
}

#[test]
fn expansion_is_idempotent_and_round_trips() {
    let mut rng = Lcg(7);
    for _ in 0..60 {
        let text = random_poly_text(&mut rng, 4);
        let p = expand(&sy(&text)).expect("expand");
        assert_eq!(expand(&p).expect("again"), p, "idempotent: {text}");
        let back = from_expr(&to_expr(&p)).expect("round trip");
        assert_eq!(back, p, "round trip: {text}");
        assert_eq!(p.rebuild().expect("rebuild"), p, "fixed point: {text}");
    }
}

#[test]
fn sum_and_product_are_commutative_after_expansion() {
    let mut rng = Lcg(11);
    for _ in 0..40 {
        let a = random_poly_text(&mut rng, 3);
        let b = random_poly_text(&mut rng, 3);
        let ab = expand(&sy(&format!("({a})*({b})"))).expect("ab");
        let ba = expand(&sy(&format!("({b})*({a})"))).expect("ba");
        assert_eq!(ab, ba);
        let diff = expand(&sy(&format!("({a})+({b})-({b})-({a})"))).expect("diff");
        assert!(diff.is_zero(), "a+b-b-a = 0 for {a} / {b}");
    }
}

#[test]
fn square_of_a_sum_expands_to_the_three_terms() {
    assert_eq!(expand(&sy("(a+b)^2")).expect("e"), sy("a^2+2*a*b+b^2"));
    assert_eq!(expand(&sy("(x-3*y)^2")).expect("e"), sy("x^2-6*x*y+9*y^2"));
}

/// Random integer matrix (rows x cols) with entries in -4..=4.
fn random_matrix(rng: &mut Lcg, n: usize) -> Vec<Vec<i64>> {
    (0..n)
        .map(|_| (0..n).map(|_| rng.range(-4, 4)).collect())
        .collect()
}

fn residual_is_zero(eq: &Equation, subs: &[(&str, Sym)]) -> bool {
    let diff = eq.lhs.clone().minus(eq.rhs.clone()).expect("diff");
    let v = substitute(&diff, subs).expect("subst");
    expand(&v).expect("expand").is_zero()
}

#[test]
fn random_rational_systems_are_solved_exactly() {
    let mut rng = Lcg(2026);
    let mut unique_seen = 0;
    for n in [3usize, 4] {
        let vars: Vec<String> = (0..n).map(|i| format!("v{i}")).collect();
        let var_refs: Vec<&str> = vars.iter().map(String::as_str).collect();
        for _ in 0..40 {
            let a = random_matrix(&mut rng, n);
            let truth: Vec<i64> = (0..n).map(|_| rng.range(-6, 6)).collect();
            // Right-hand sides from the matrix: the system is consistent by construction.
            let eqs: Vec<Equation> = (0..n)
                .map(|i| {
                    let mut lhs = Sym::int(0);
                    let mut rhs_val = 0i64;
                    for j in 0..n {
                        rhs_val += a[i][j] * truth[j];
                        let term = Sym::product(vec![Sym::int(a[i][j]), Sym::symbol(&vars[j])])
                            .expect("term");
                        lhs = lhs.plus(term).expect("sum");
                    }
                    Equation {
                        lhs,
                        rhs: Sym::int(rhs_val),
                    }
                })
                .collect();
            match solve_linear_system(&eqs, &names(&var_refs)).expect("solve") {
                SolveOutcome::Unique(sol) => {
                    unique_seen += 1;
                    for (j, (_, v)) in sol.iter().enumerate() {
                        assert_eq!(*v, Sym::int(truth[j]), "unknown {j} of a {n}x{n} system");
                    }
                }
                SolveOutcome::Infinite { solution, .. } => {
                    // Singular matrix: every equation still vanishes for the parametric form.
                    let subs: Vec<(&str, Sym)> = var_refs
                        .iter()
                        .zip(solution.iter())
                        .map(|(k, (_, v))| (*k, v.clone()))
                        .collect();
                    for eq in &eqs {
                        assert!(residual_is_zero(eq, &subs), "parametric residual");
                    }
                }
                other => panic!("consistent system gave {other:?}"),
            }
        }
    }
    assert!(unique_seen > 30, "most random matrices are nonsingular");
}

#[test]
fn solution_substituted_back_is_zero_for_samples() {
    let eqs = [
        super::eqn("x+2*y-z==4"),
        super::eqn("x+z==-3"),
        super::eqn("y-2*z==5"),
    ];
    let sol = solve_linear_system(&eqs, &names(&["x", "y", "z"])).expect("solve");
    let SolveOutcome::Unique(v) = sol else {
        panic!("unique expected");
    };
    let subs: Vec<(&str, Sym)> = ["x", "y", "z"]
        .iter()
        .zip(v.iter())
        .map(|(k, (_, val))| (*k, val.clone()))
        .collect();
    for eq in &eqs {
        assert!(residual_is_zero(eq, &subs));
    }
}

#[test]
fn integer_power_of_a_number_is_exact() {
    assert_eq!(sy("3^4"), Sym::int(81));
    assert_eq!(sy("(2/3)^-2"), Sym::Num(int(9) / int(4)));
}

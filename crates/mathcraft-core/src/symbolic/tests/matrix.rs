//! Symbolic matrices: determinant, inverse, products, powers, characteristic polynomial,
//! eigenvalues, rank and null space. Checks are exact where the result is exact, and numeric
//! (exact rational point substitution) for the general identities.

use super::{approx, sy, Lcg};
use crate::symbolic::num::{int, Q};
use crate::symbolic::poly::eval_rational;
use crate::symbolic::ratfunc::simplify;
use crate::symbolic::{expand, substitute, Sym, SymError, SymMatrix};
use num_traits::Zero;

fn mat(rows: &[&[&str]]) -> SymMatrix {
    let rs: Vec<Vec<Sym>> = rows
        .iter()
        .map(|r| r.iter().map(|t| sy(t)).collect())
        .collect();
    SymMatrix::new(rs).expect("matrix builds")
}

fn same(a: &Sym, b: &Sym) -> bool {
    simplify(&a.clone().minus(b.clone()).expect("minus"))
        .expect("simplify")
        .is_zero()
}

fn assert_same_matrix(a: &SymMatrix, b: &SymMatrix) {
    assert_eq!((a.rows(), a.cols()), (b.rows(), b.cols()));
    for i in 0..a.rows() {
        for j in 0..a.cols() {
            assert!(
                same(a.get(i, j).unwrap(), b.get(i, j).unwrap()),
                "entry ({i},{j}): {:?} vs {:?}",
                a.get(i, j),
                b.get(i, j)
            );
        }
    }
}

/// Exact value of a matrix at rational points (all symbols given).
fn at(m: &SymMatrix, env: &[(&str, Q)]) -> Option<Vec<Vec<Q>>> {
    m.to_rows()
        .iter()
        .map(|row| {
            row.iter()
                .map(|e| eval_rational(e, env).expect("eval"))
                .collect::<Option<Vec<Q>>>()
        })
        .collect()
}

/// Leibniz determinant of a numeric rational matrix (independent of the symbolic code).
fn leibniz(m: &[Vec<Q>]) -> Q {
    fn perms(n: usize) -> Vec<(Vec<usize>, i64)> {
        if n == 1 {
            return vec![(vec![0], 1)];
        }
        let mut out = Vec::new();
        for (p, s) in perms(n - 1) {
            for pos in 0..n {
                let mut q = p.clone();
                q.insert(pos, n - 1);
                let swaps = (n - 1 - pos) as i64;
                let sign = if swaps % 2 == 0 { s } else { -s };
                out.push((q, sign));
            }
        }
        out
    }
    let n = m.len();
    let mut total = int(0);
    for (p, s) in perms(n) {
        let mut t = int(s);
        for (i, &j) in p.iter().enumerate() {
            t *= &m[i][j];
        }
        total += t;
    }
    total
}

#[test]
fn determinant_of_numbers() {
    assert_eq!(
        mat(&[&["1", "2"], &["3", "4"]]).determinant().unwrap(),
        sy("-2")
    );
    assert_eq!(
        mat(&[&["2", "0", "1"], &["1", "3", "2"], &["1", "1", "4"]])
            .determinant()
            .unwrap(),
        sy("18")
    );
    assert!(mat(&[&["1", "2"], &["2", "4"]])
        .determinant()
        .unwrap()
        .is_zero());
}

#[test]
fn determinant_of_two_by_two_symbolic() {
    let m = mat(&[&["a", "b"], &["c", "d"]]);
    assert!(same(&m.determinant().unwrap(), &sy("a*d-b*c")));
}

#[test]
fn determinant_matches_cofactor_and_numeric_on_random_polynomial_matrices() {
    let mut rng = Lcg(2024);
    for case in 0..24 {
        let n = if case % 2 == 0 { 2 } else { 3 };
        let mut rows: Vec<Vec<String>> = Vec::new();
        for _ in 0..n {
            let row = (0..n)
                .map(|_| {
                    let c1 = rng.range(-3, 3);
                    let c2 = rng.range(-3, 3);
                    format!("({c1})*x+({c2})*y^2+{}", rng.range(-2, 2))
                })
                .collect();
            rows.push(row);
        }
        let text: Vec<Vec<&str>> = rows
            .iter()
            .map(|r| r.iter().map(String::as_str).collect())
            .collect();
        let m = SymMatrix::new(
            text.iter()
                .map(|r| r.iter().map(|t| sy(t)).collect())
                .collect(),
        )
        .expect("build");
        let det = m.determinant().expect("det");

        // Cofactor expansion, expanded to a polynomial.
        let cof = if n == 2 {
            m.get(0, 0)
                .unwrap()
                .clone()
                .times(m.get(1, 1).unwrap().clone())
                .unwrap()
                .minus(
                    m.get(0, 1)
                        .unwrap()
                        .clone()
                        .times(m.get(1, 0).unwrap().clone())
                        .unwrap(),
                )
                .unwrap()
        } else {
            let g = |i, j| m.get(i, j).unwrap().clone();
            let minor = |a: Sym, b: Sym, c: Sym, d: Sym| {
                a.times(d).unwrap().minus(b.times(c).unwrap()).unwrap()
            };
            let c0 = g(0, 0)
                .times(minor(g(1, 1), g(1, 2), g(2, 1), g(2, 2)))
                .unwrap();
            let c1 = g(0, 1)
                .times(minor(g(1, 0), g(1, 2), g(2, 0), g(2, 2)))
                .unwrap();
            let c2 = g(0, 2)
                .times(minor(g(1, 0), g(1, 1), g(2, 0), g(2, 1)))
                .unwrap();
            c0.minus(c1).unwrap().plus(c2).unwrap()
        };
        assert!(
            same(&expand(&det).unwrap(), &expand(&cof).unwrap()),
            "cofactor mismatch for {rows:?}"
        );

        for (x, y) in [(2, 3), (-1, 5), (7, -4)] {
            let env = [("x", int(x)), ("y", int(y))];
            let vals = at(&m, &env).expect("numeric");
            let want = leibniz(&vals);
            let got = eval_rational(&det, &env).unwrap().expect("det numeric");
            assert_eq!(got, want, "numeric det at ({x},{y}) for {rows:?}");
        }
    }
}

#[test]
fn inverse_of_numbers_and_identity_product() {
    let a = mat(&[&["2", "1"], &["1", "1"]]);
    let inv = a.inverse().expect("inverse");
    assert!(inv.condition.is_none());
    assert_same_matrix(&inv.matrix, &mat(&[&["1", "-1"], &["-1", "2"]]));
    let id = a.multiply(&inv.matrix).expect("product");
    assert_same_matrix(&id, &SymMatrix::identity(2).unwrap());
}

#[test]
fn inverse_of_symbolic_triangular_reports_its_condition() {
    let a = mat(&[&["a", "1"], &["0", "b"]]);
    let inv = a.inverse().expect("inverse");
    assert!(same(&inv.condition.clone().expect("condition"), &sy("a*b")));
    assert_same_matrix(&inv.matrix, &mat(&[&["1/a", "-1/(a*b)"], &["0", "1/b"]]));
    let id = a.multiply(&inv.matrix).expect("product");
    assert_same_matrix(&id, &SymMatrix::identity(2).unwrap());
}

#[test]
fn inverse_times_matrix_is_identity_at_random_points() {
    let mut rng = Lcg(77);
    for _ in 0..16 {
        let rows: Vec<Vec<String>> = (0..3)
            .map(|_| {
                (0..3)
                    .map(|_| format!("{}*x+{}", rng.range(-3, 3), rng.range(-3, 3)))
                    .collect()
            })
            .collect();
        let m = SymMatrix::new(
            rows.iter()
                .map(|r| r.iter().map(|t| sy(t)).collect())
                .collect(),
        )
        .unwrap();
        let det = m.determinant().unwrap();
        if det.is_zero() {
            assert!(matches!(m.inverse(), Err(SymError::Undefined(_))));
            continue;
        }
        let inv = m.inverse().expect("inverse").matrix;
        let prod = m.multiply(&inv).expect("product");
        for x in [-3, 2, 5] {
            let env = [("x", int(x))];
            let d = eval_rational(&det, &env).unwrap().expect("det");
            if d.is_zero() {
                continue;
            }
            let p = at(&prod, &env).expect("numeric");
            for (i, row) in p.iter().enumerate() {
                for (j, v) in row.iter().enumerate() {
                    assert_eq!(*v, int(i64::from(i == j)), "A*inv at x={x} for {rows:?}");
                }
            }
        }
    }
}

#[test]
fn singular_matrix_has_no_inverse() {
    assert!(matches!(
        mat(&[&["1", "2"], &["2", "4"]]).inverse(),
        Err(SymError::Undefined(_))
    ));
}

#[test]
fn powers_including_zero_and_negative() {
    let a = mat(&[&["1", "1"], &["0", "1"]]);
    assert_same_matrix(&a.power(5).unwrap(), &mat(&[&["1", "5"], &["0", "1"]]));
    assert_same_matrix(&a.power(-1).unwrap(), &mat(&[&["1", "-1"], &["0", "1"]]));
    assert_same_matrix(&a.power(0).unwrap(), &SymMatrix::identity(2).unwrap());
    assert!(matches!(a.power(65), Err(SymError::TooLarge(_))));
}

#[test]
fn transpose_and_product_rules() {
    let a = mat(&[&["a", "2", "x"], &["0", "b", "1"]]);
    let b = mat(&[&["1", "x"], &["y", "0"], &["2", "b"]]);
    let lhs = a.multiply(&b).unwrap().transpose();
    let rhs = b.transpose().multiply(&a.transpose()).unwrap();
    assert_same_matrix(&lhs, &rhs);
    assert!(matches!(a.multiply(&a), Err(SymError::Invalid(_))));
}

#[test]
fn characteristic_polynomial_and_eigenvalues() {
    let a = mat(&[&["2", "1"], &["1", "2"]]);
    let cp = a.characteristic_polynomial("l").unwrap();
    assert!(same(&cp, &sy("l^2-4*l+3")));
    let mut roots = a.eigenvalues("l").unwrap();
    roots.sort_by(|x, y| x.partial_cmp(y).unwrap());
    assert_eq!(roots, vec![sy("1"), sy("3")]);

    let d = mat(&[&["1", "0", "0"], &["0", "2", "0"], &["0", "0", "3"]]);
    let mut r = d.eigenvalues("l").unwrap();
    r.sort_by(|x, y| x.partial_cmp(y).unwrap());
    assert_eq!(r, vec![sy("1"), sy("2"), sy("3")]);
}

#[test]
fn eigenvalues_satisfy_the_characteristic_equation() {
    // Surd roots (1 +- sqrt(5))/2 of l^2 - l - 1.
    let a = mat(&[&["1", "1"], &["1", "0"]]);
    let roots = a.eigenvalues("l").unwrap();
    assert_eq!(roots.len(), 2);
    let cp = a.characteristic_polynomial("l").unwrap();
    for r in &roots {
        let v = substitute(&cp, &[("l", r.clone())]).unwrap();
        let x = approx(r);
        assert!(x.is_finite());
        assert!(approx(&v).abs() < 1e-9, "root {x} leaves {}", approx(&v));
        assert!((x * x - x - 1.0).abs() < 1e-9);
    }
}

#[test]
fn eigenvalues_refused_for_other_sizes_and_cubic_irrationals() {
    assert!(matches!(
        mat(&[
            &["1", "2", "3", "4"],
            &["1", "0", "0", "0"],
            &["0", "1", "0", "0"],
            &["0", "0", "1", "0"]
        ])
        .eigenvalues("l"),
        Err(SymError::Unsupported(_))
    ));
    // Cubic with no rational root: refused, never a wrong value.
    let c = mat(&[&["0", "0", "2"], &["1", "0", "0"], &["0", "1", "0"]]);
    assert!(matches!(c.eigenvalues("l"), Err(SymError::Unsupported(_))));
}

#[test]
fn rank_of_numeric_and_symbolic_matrices() {
    let r = mat(&[&["1", "2", "3"], &["4", "5", "6"], &["7", "8", "9"]])
        .rank()
        .unwrap();
    assert_eq!(r.rank, 2);
    assert!(r.conditions.is_empty());
    let s = mat(&[&["a", "1"], &["1", "a"]]).rank().unwrap();
    assert_eq!(s.rank, 2);
    assert!(!s.conditions.is_empty());
}

#[test]
fn nullspace_basis_is_in_the_kernel() {
    let a = mat(&[&["1", "2"], &["2", "4"]]);
    let (basis, conditions) = a.nullspace().unwrap();
    assert!(conditions.is_empty());
    assert_eq!(basis.len(), 1);
    let col = SymMatrix::new(vec![vec![basis[0][0].clone()], vec![basis[0][1].clone()]]).unwrap();
    let prod = a.multiply(&col).unwrap();
    assert!(prod.get(0, 0).unwrap().is_zero());
    assert!(prod.get(1, 0).unwrap().is_zero());
}

#[test]
fn size_and_shape_errors() {
    assert!(matches!(
        SymMatrix::new(vec![vec![sy("1")], vec![sy("1"), sy("2")]]),
        Err(SymError::Invalid(_))
    ));
    let six = (0..6).map(|_| (0..6).map(|_| sy("1")).collect()).collect();
    assert!(matches!(SymMatrix::new(six), Err(SymError::TooLarge(_))));
    assert!(matches!(
        mat(&[&["1", "2", "3"], &["4", "5", "6"]]).determinant(),
        Err(SymError::Invalid(_))
    ));
}

#[test]
fn five_by_five_determinant_is_exact() {
    let m = SymMatrix::new(
        (0..5)
            .map(|i| {
                (0..5)
                    .map(|j| Sym::int(if i == j { 2 } else { i64::from(i + j == 4) }))
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    let env: [(&str, Q); 0] = [];
    let det = m.determinant().unwrap();
    let vals = at(&m, &env).unwrap();
    assert_eq!(eval_rational(&det, &env).unwrap().unwrap(), leibniz(&vals));
}

#[test]
fn dense_symbolic_five_by_five_finishes_or_refuses() {
    let names = ["a", "b", "c", "d", "e"];
    let rows: Vec<Vec<Sym>> = (0..5)
        .map(|i| {
            (0..5)
                .map(|j| sy(&format!("{}*{}+{}", names[i], names[j], names[(i + j) % 5])))
                .collect()
        })
        .collect();
    let m = SymMatrix::new(rows).expect("build");
    let start = std::time::Instant::now();
    match m.determinant() {
        Ok(d) => assert!(!d.is_zero()),
        Err(SymError::TooLarge(_)) => {}
        Err(e) => panic!("unexpected refusal {e:?}"),
    }
    assert!(start.elapsed().as_secs() < 30, "determinant too slow");
}

#[test]
fn random_entries_never_panic_and_refusals_are_typed() {
    // Polynomial entries only: inverses of rational entries can be large in debug builds.
    let mut rng = Lcg(31337);
    let atoms = ["x", "y", "2", "(1/3)", "x^2", "(x+1)", "0", "sqrt(2)"];
    for _ in 0..40 {
        let n = rng.range(1, 3) as usize;
        let rows: Vec<Vec<Sym>> = (0..n)
            .map(|_| {
                (0..n)
                    .map(|_| {
                        let a = atoms[rng.range(0, atoms.len() as i64 - 1) as usize];
                        let b = atoms[rng.range(0, atoms.len() as i64 - 1) as usize];
                        sy(&format!("{a}+{b}*x"))
                    })
                    .collect()
            })
            .collect();
        let m = SymMatrix::new(rows).expect("build");
        let _ = m.determinant();
        let _ = m.rank();
        let _ = m.nullspace();
        let _ = m.characteristic_polynomial("l");
        match m.inverse() {
            Ok(inv) => match m.multiply(&inv.matrix) {
                Ok(id) => assert_eq!(id.rows(), n),
                Err(SymError::TooLarge(_)) => {}
                Err(e) => panic!("unexpected product refusal {e:?}"),
            },
            Err(SymError::Undefined(_))
            | Err(SymError::Unsupported(_))
            | Err(SymError::TooLarge(_)) => {}
            Err(e) => panic!("unexpected {e:?}"),
        }
        match m.power(2) {
            Ok(_) | Err(SymError::TooLarge(_)) => {}
            Err(e) => panic!("unexpected power refusal {e:?}"),
        }
    }
}

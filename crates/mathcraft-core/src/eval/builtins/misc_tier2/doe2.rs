//! Two-level fractional factorial designs from generator strings (help: "Factorial Designs",
//! "Fractional Factorial Utility Functions", "DOE Utility Functions").
//!
//! Built:
//! - `fractfact("gen")`: the design of the generator string, such as `"A, B, C=AB, D=AC"`. Base
//!   factors (the letters given alone) take the full factorial columns in NIST order (the first base
//!   factor changes fastest); a defined factor is the product of the coded columns of the letters on
//!   its right. Columns are the factors in alphabetical order, after the `Run` and `Block` columns
//!   of `fullfact`.
//! - `fractruns("gen")`: 2^k, the runs of that design (k = number of base factors).
//! - `fractresol("gen")`: the resolution, the length of the shortest defining word (every product
//!   of the defining words `X·R` taken together).
//! - `foldover(A, ["foldfac"])`: the design A (coded +1/-1) followed by its mirror image. The
//!   mirror reverses every factor's level, or only those named in `foldfac`. Block 1 holds the
//!   original runs, block 2 the mirror runs.
//!
//! - `fractalias("gen", "fac", [order])`: the aliases of each term in `fac` (factor letters such as
//!   `B` or `AB`), from the defining relation: every nonempty product of the defining words, times
//!   the term, with the term itself left out. Aliases are sorted by length, then alphabetically, and
//!   `order` keeps those of at most that many letters. The help's table has text cells, which a
//!   matrix cannot hold, so the result is a list of rows `["term", "alias, alias, ..."]`.
//!
//! Not built: `fractfact(n, r)` (the default generators come from a table in the cited book, not in
//! the help), `block` (blocked designs), `randomize` (needs a random seed contract),
//! `plackettburman`, `taguchi` and `mtaguchi` (tables not in the help).

use super::super::matrix::expect_matrix;
use super::super::{Arity, BuiltinCtx, Registry};
use crate::eval::error::EvalError;
use crate::eval::ops;
use crate::eval::value::{Matrix, Scalar, Value};
use crate::units::Dimension;

/// Most factors a fraction may have (the help limits `fractfact` to 26).
const MAX_FACTORS: usize = 26;
/// Most base factors (2^16 runs).
const MAX_BASE: usize = 16;
/// Most defining words whose subsets are enumerated (2^20 products).
const MAX_WORDS: usize = 20;
/// Largest design, in cells (runs times columns).
const MAX_CELLS: usize = 1_000_000;
/// Most (defining word, term) pairs `fractalias` turns into alias strings.
const MAX_ALIAS_PAIRS: usize = 4_000_000;

pub(super) fn register(r: &mut Registry) {
    r.register_ctx("fractfact", Arity::Range(1, 2), fractfact);
    r.register("fractruns", Arity::Exact(1), fractruns);
    r.register("fractresol", Arity::Exact(1), fractresol);
    r.register_ctx("foldover", Arity::Range(1, 2), foldover);
    r.register_ctx("fractalias", Arity::Range(2, 3), fractalias);
}

/// A parsed generator string: the factors as bit masks over the letters A..Z, the base factors in
/// order, and the definitions of the others.
struct Generators {
    /// Every factor letter (0..26), in alphabetical order.
    factors: Vec<u8>,
    /// Base letters, alphabetical.
    base: Vec<u8>,
    /// For each defined letter, the letters on its right.
    defined: Vec<(u8, Vec<u8>)>,
}

fn parse_generators(text: &str) -> Result<Generators, EvalError> {
    let bad = |m: &str| EvalError::Unsupported(format!("fractfact generators: {m} in {text:?}"));
    let mut base: Vec<u8> = Vec::new();
    let mut defined: Vec<(u8, Vec<u8>)> = Vec::new();
    for token in text.split([' ', ',', ':', ';']).filter(|t| !t.is_empty()) {
        match token.split_once('=') {
            None => {
                let letter = single_letter(token)
                    .ok_or_else(|| bad("factors are single capital letters"))?;
                if base.contains(&letter) || defined.iter().any(|(l, _)| *l == letter) {
                    return Err(bad("a factor is listed twice"));
                }
                base.push(letter);
            }
            Some((lhs, rhs)) => {
                let letter = single_letter(lhs)
                    .ok_or_else(|| bad("a defined factor is one capital letter"))?;
                let right: Vec<u8> = rhs
                    .bytes()
                    .map(letter_of)
                    .collect::<Option<_>>()
                    .ok_or_else(|| bad("the generator must be capital letters"))?;
                let distinct = right
                    .iter()
                    .enumerate()
                    .all(|(k, l)| !right[..k].contains(l));
                if right.is_empty() || !distinct || right.contains(&letter) {
                    return Err(bad(
                        "a generator needs distinct letters other than the defined factor",
                    ));
                }
                if defined.iter().any(|(l, _)| *l == letter) || base.contains(&letter) {
                    return Err(bad("a factor is defined twice"));
                }
                defined.push((letter, right));
            }
        }
    }
    if base.is_empty() {
        return Err(bad("no base factor"));
    }
    if base.len() > MAX_BASE {
        return Err(EvalError::TooLarge(1usize << base.len()));
    }
    base.sort_unstable();
    let mut factors: Vec<u8> = base
        .iter()
        .copied()
        .chain(defined.iter().map(|(l, _)| *l))
        .collect();
    factors.sort_unstable();
    if factors.len() > MAX_FACTORS {
        return Err(EvalError::TooLarge(factors.len()));
    }
    // Every letter on a right-hand side must be a factor of the design.
    for (_, right) in &defined {
        if right.iter().any(|l| !factors.contains(l)) {
            return Err(bad("a generator uses a letter that is not a factor"));
        }
    }
    Ok(Generators {
        factors,
        base,
        defined,
    })
}

fn letter_of(b: u8) -> Option<u8> {
    b.is_ascii_uppercase().then_some(b - b'A')
}

fn single_letter(token: &str) -> Option<u8> {
    match token.as_bytes() {
        [b] => letter_of(*b),
        _ => None,
    }
}

/// `fractfact("gen")`: the fractional factorial design of the generator string.
fn fractfact(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    if args.len() == 2 {
        return Err(EvalError::Unsupported(
            "fractfact(n, r) needs the default generators, which are not built; give the generators as a string"
                .to_string(),
        ));
    }
    let text = match &args[0] {
        Value::Str(s) => s.clone(),
        _ => {
            return Err(EvalError::Unsupported(
                "fractfact needs the generator string, such as \"A, B, C=AB\"".to_string(),
            ))
        }
    };
    let gens = parse_generators(&text)?;
    let k = gens.base.len();
    let runs = 1usize << k;
    let cells = runs.saturating_mul(gens.factors.len() + 2);
    if cells > MAX_CELLS {
        return Err(EvalError::TooLarge(cells));
    }
    ctx.charge(cells as u64)?;
    // Coded columns of the base factors, then of the defined factors (in dependency order).
    let mut cols: Vec<Option<Vec<f64>>> = vec![None; 26];
    for (j, &letter) in gens.base.iter().enumerate() {
        cols[letter as usize] = Some(
            (0..runs)
                .map(|run| if (run >> j) & 1 == 1 { 1.0 } else { -1.0 })
                .collect(),
        );
    }
    let mut pending = gens.defined.clone();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|(letter, right)| {
            if right.iter().all(|l| cols[*l as usize].is_some()) {
                let column: Vec<f64> = (0..runs)
                    .map(|run| {
                        right
                            .iter()
                            .map(|l| cols[*l as usize].as_ref().map_or(1.0, |c| c[run]))
                            .product()
                    })
                    .collect();
                cols[*letter as usize] = Some(column);
                false
            } else {
                true
            }
        });
        if pending.len() == before {
            return Err(EvalError::Unsupported(
                "fractfact generators are circular".to_string(),
            ));
        }
    }
    let width = gens.factors.len() + 2;
    let mut data = Vec::with_capacity(runs * width);
    for run in 0..runs {
        data.push(real((run + 1) as f64));
        data.push(real(1.0));
        for &letter in &gens.factors {
            let column = cols[letter as usize].as_ref().ok_or_else(|| {
                EvalError::Unsupported("fractfact: a factor has no column".to_string())
            })?;
            data.push(real(column[run]));
        }
    }
    Ok(Value::Matrix(Matrix::new(runs, width, data)?))
}

/// `fractruns("gen")`: the number of runs, 2^k for k base factors.
fn fractruns(args: &[Value]) -> Result<Value, EvalError> {
    let gens = parse_generators(string_arg(&args[0], "fractruns")?)?;
    Ok(Value::Scalar(real_scalar((1u64 << gens.base.len()) as f64)))
}

/// `fractresol("gen")`: the resolution, the shortest defining word.
fn fractresol(args: &[Value]) -> Result<Value, EvalError> {
    let gens = parse_generators(string_arg(&args[0], "fractresol")?)?;
    let words: Vec<u32> = gens
        .defined
        .iter()
        .map(|(letter, right)| {
            right
                .iter()
                .fold(1u32 << letter, |acc, l| acc ^ (1u32 << l))
        })
        .collect();
    if words.is_empty() {
        return Err(EvalError::Unsupported(
            "a full factorial has no defining word, so no resolution".to_string(),
        ));
    }
    if words.len() > MAX_WORDS {
        return Err(EvalError::TooLarge(1usize << words.len()));
    }
    // Every nonempty product of the defining words (Gray-code walk over the subsets).
    let mut current = 0u32;
    let mut best = u32::MAX;
    for step in 1u64..(1u64 << words.len()) {
        let bit = step.trailing_zeros() as usize;
        current ^= words[bit];
        best = best.min(current.count_ones());
    }
    Ok(Value::Scalar(real_scalar(f64::from(best))))
}

/// `fractalias("gen", "fac", [order])`: the aliases of each term, as rows `["term", "aliases"]`.
fn fractalias(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let gens = parse_generators(string_arg(&args[0], "fractalias")?)?;
    let max_len = match args.get(2) {
        None => usize::MAX,
        Some(Value::Scalar(s)) => {
            ops::require_dimensionless(s.dim)?;
            let v = s.value.re;
            if s.value.im != 0.0 || !v.is_finite() || v < 1.0 || v.fract() != 0.0 {
                return Err(EvalError::Unsupported(
                    "fractalias order is an integer of at least 1".to_string(),
                ));
            }
            v as usize
        }
        Some(_) => {
            return Err(EvalError::Unsupported(
                "fractalias order is an integer of at least 1".to_string(),
            ))
        }
    };
    // The defining words: each defined letter times the letters on its right.
    let words: Vec<u32> = gens
        .defined
        .iter()
        .map(|(letter, right)| {
            right
                .iter()
                .fold(1u32 << letter, |acc, l| acc ^ (1u32 << l))
        })
        .collect();
    if words.len() > MAX_WORDS {
        return Err(EvalError::TooLarge(1usize << words.len()));
    }
    // The defining relation: every product of the words (a set, so dependent words cost nothing).
    let mut group: Vec<u32> = vec![0];
    for w in &words {
        let extra: Vec<u32> = group.iter().map(|g| g ^ w).collect();
        group.extend(extra);
    }
    group.sort_unstable();
    group.dedup();

    let text = string_arg(&args[1], "fractalias")?;
    let mut terms: Vec<(String, u32)> = Vec::new();
    for token in text.split([' ', ',', ':', ';']).filter(|t| !t.is_empty()) {
        let mut mask = 0u32;
        for ch in token.bytes() {
            let l = letter_of(ch).ok_or_else(|| {
                EvalError::Unsupported(format!("fractalias: `{token}` is not factor letters"))
            })?;
            if !gens.factors.contains(&l) || mask & (1u32 << l) != 0 {
                return Err(EvalError::Unsupported(format!(
                    "fractalias: `{token}` names a factor that is not in the design, or twice"
                )));
            }
            mask |= 1u32 << l;
        }
        terms.push((token.to_string(), mask));
    }
    if terms.is_empty() {
        return Err(EvalError::Unsupported(
            "fractalias needs at least one factor or interaction, such as \"A, B, AB\"".to_string(),
        ));
    }
    ctx.charge((group.len() * terms.len()) as u64)?;
    // One alias string per (term, defining-relation word); each is built and joined below.
    let pairs = group.len().saturating_mul(terms.len());
    if pairs > MAX_ALIAS_PAIRS {
        return Err(EvalError::TooLarge(pairs));
    }

    let mut rows = Vec::with_capacity(terms.len());
    for (label, mask) in &terms {
        // Every product with the defining relation, except the term itself (the identity).
        let mut aliases: Vec<String> = group
            .iter()
            .filter(|g| **g != 0)
            .map(|g| g ^ mask)
            .filter(|m| *m != 0 && (m.count_ones() as usize) <= max_len)
            .map(letters_of)
            .collect();
        aliases.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        aliases.dedup();
        rows.push(Value::List(vec![
            Value::Str(label.clone()),
            Value::Str(aliases.join(", ")),
        ]));
    }
    Ok(Value::List(rows))
}

/// The factor letters of a bit mask, alphabetically (bit 0 is `A`).
fn letters_of(mask: u32) -> String {
    (0..26u8)
        .filter(|l| mask & (1u32 << l) != 0)
        .map(|l| (b'A' + l) as char)
        .collect()
}

fn string_arg<'a>(v: &'a Value, name: &str) -> Result<&'a str, EvalError> {
    match v {
        Value::Str(s) => Ok(s),
        _ => Err(EvalError::Unsupported(format!(
            "{name} needs the generator string, such as \"A, B, C=AB\""
        ))),
    }
}

/// `foldover(A, ["foldfac"])`: the coded design `A` in block 1, then its mirror image in block 2.
fn foldover(ctx: &BuiltinCtx<'_>, args: &[Value]) -> Result<Value, EvalError> {
    let design = expect_matrix(&args[0], "foldover")?;
    if design.cols() < 3 {
        return Err(EvalError::Unsupported(
            "foldover needs the Run and Block columns and at least one factor".to_string(),
        ));
    }
    let factors = design.cols() - 2;
    let mut flip = vec![true; factors];
    if let Some(v) = args.get(1) {
        let text = string_arg(v, "foldover")?;
        flip = vec![false; factors];
        for token in text.split([' ', ',', ':', ';']).filter(|t| !t.is_empty()) {
            let letter = single_letter(token).ok_or_else(|| {
                EvalError::Unsupported("foldover: the fold factors are capital letters".to_string())
            })?;
            let k = letter as usize;
            if k >= factors {
                return Err(EvalError::Unsupported(format!(
                    "foldover: factor {token} is not in the design"
                )));
            }
            flip[k] = true;
        }
    }
    let n = design.rows();
    let cells = n.saturating_mul(2).saturating_mul(design.cols());
    if cells > MAX_CELLS {
        return Err(EvalError::TooLarge(cells));
    }
    ctx.charge(cells as u64)?;
    let mut data = Vec::with_capacity(cells);
    for block in 0..2usize {
        for i in 0..n {
            data.push(real((block * n + i + 1) as f64));
            data.push(real((block + 1) as f64));
            for (j, &flipped) in flip.iter().enumerate() {
                let x = coded(design.get(i, j + 2)?)?;
                let mirrored = block == 1 && flipped;
                data.push(real(if mirrored { -x } else { x }));
            }
        }
    }
    Ok(Value::Matrix(Matrix::new(2 * n, design.cols(), data)?))
}

/// A coded level: +1 or -1 exactly.
fn coded(s: &Scalar) -> Result<f64, EvalError> {
    ops::require_dimensionless(s.dim)?;
    if s.value.im == 0.0 && (s.value.re == 1.0 || s.value.re == -1.0) {
        Ok(s.value.re)
    } else {
        Err(EvalError::Unsupported(
            "foldover needs a coded design (levels +1 and -1)".to_string(),
        ))
    }
}

fn real(x: f64) -> Scalar {
    Scalar::real(x, Dimension::dimensionless())
}

fn real_scalar(x: f64) -> Scalar {
    real(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::builtins::BuiltinCtx;

    fn ctx() -> BuiltinCtx<'static> {
        BuiltinCtx::new(Ok(0), Ok(1e-3), Ok(1e-3))
    }

    fn design_of(gen: &str) -> Matrix {
        match fractfact(&ctx(), &[Value::Str(gen.to_string())]) {
            Ok(Value::Matrix(m)) => m,
            other => panic!("expected a matrix, got {other:?}"),
        }
    }

    fn value(m: &Matrix, i: usize, j: usize) -> f64 {
        m.get(i, j).unwrap().value.re
    }

    #[test]
    fn half_fraction_has_the_defined_column_as_a_product() {
        // A, B, C=AB: 4 runs, C is the product of A and B.
        let m = design_of("A, B, C=AB");
        assert_eq!((m.rows(), m.cols()), (4, 5));
        for i in 0..4 {
            assert_eq!(value(&m, i, 4), value(&m, i, 2) * value(&m, i, 3));
        }
        assert_eq!(fractresol_of("A, B, C=AB"), 3);
    }

    #[test]
    fn columns_are_balanced_and_orthogonal() {
        let m = design_of("A, B, C, D=ABC");
        let n = m.rows() as f64;
        for j in 2..m.cols() {
            let sum: f64 = (0..m.rows()).map(|i| value(&m, i, j)).sum();
            assert_eq!(sum, 0.0, "column {j} is balanced");
            for k in (j + 1)..m.cols() {
                let dot: f64 = (0..m.rows())
                    .map(|i| value(&m, i, j) * value(&m, i, k))
                    .sum();
                assert_eq!(dot, 0.0, "columns {j} and {k} are orthogonal ({n} runs)");
            }
        }
    }

    fn fractresol_of(gen: &str) -> i64 {
        match fractresol(&[Value::Str(gen.to_string())]) {
            Ok(Value::Scalar(s)) => s.value.re as i64,
            other => panic!("expected a scalar, got {other:?}"),
        }
    }

    #[test]
    fn resolution_and_runs_of_the_standard_fractions() {
        // 2^(4-1) with D=ABC: defining word ABCD, resolution IV.
        assert_eq!(fractresol_of("A, B, C, D=ABC"), 4);
        // 2^(5-2) with D=AB, E=AC: words ABD, ACE, BCDE, resolution III.
        assert_eq!(fractresol_of("A, B, C, D=AB, E=AC"), 3);
        // E=AC and D=BC give the word ABDE as well: the shortest is BCD, resolution III.
        assert_eq!(fractresol_of("A, B, C, D=BC, E=AC"), 3);
        // 2^(6-2) with E=ABC, F=BCD: words ABCE, BCDF, ADEF, resolution IV.
        assert_eq!(fractresol_of("A, B, C, D, E=ABC, F=BCD"), 4);
        match fractruns(&[Value::Str("A, B, C, D=ABC".to_string())]) {
            Ok(Value::Scalar(s)) => assert_eq!(s.value.re, 8.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bad_generators_are_refused() {
        for gen in [
            "", "A, A", "A, B=AB", "A, B=Q", "A, B=AA", "A=BC", "ab, c=ab",
        ] {
            assert!(
                fractfact(&ctx(), &[Value::Str(gen.to_string())]).is_err(),
                "{gen:?} should be refused"
            );
        }
    }

    #[test]
    fn foldover_doubles_the_runs_and_breaks_the_aliasing() {
        let base = design_of("A, B, C=AB");
        let folded = match foldover(&ctx(), &[Value::Matrix(base.clone())]) {
            Ok(Value::Matrix(m)) => m,
            other => panic!("{other:?}"),
        };
        assert_eq!(folded.rows(), 8);
        assert_eq!(value(&folded, 4, 1), 2.0, "mirror runs are block 2");
        for j in 2..folded.cols() {
            assert_eq!(value(&folded, 4, j), -value(&folded, 0, j));
        }
    }

    #[test]
    fn foldover_refuses_uncoded_input() {
        let m = Matrix::new(1, 3, vec![real(1.0), real(1.0), real(0.5)]).unwrap();
        assert!(foldover(&ctx(), &[Value::Matrix(m)]).is_err());
    }

    fn alias_rows(gen: &str, text: &str, order: Option<f64>) -> Vec<(String, String)> {
        let mut args = vec![Value::Str(gen.to_string()), Value::Str(text.to_string())];
        if let Some(o) = order {
            args.push(Value::Scalar(real_scalar(o)));
        }
        match fractalias(&ctx(), &args) {
            Ok(Value::List(rows)) => rows
                .into_iter()
                .map(|row| match row {
                    Value::List(cells) => match (&cells[0], &cells[1]) {
                        (Value::Str(a), Value::Str(b)) => (a.clone(), b.clone()),
                        other => panic!("bad cells {other:?}"),
                    },
                    other => panic!("bad row {other:?}"),
                })
                .collect(),
            other => panic!("expected rows, got {other:?}"),
        }
    }

    #[test]
    fn fractalias_matches_the_help_example() {
        // Help: fractalias(gen, "B, C, F", 2) = ("B" "AD, CF, EG"; "C" "AE, BF, DG"; "F" "AG, BC, DE").
        let rows = alias_rows("A, B, C, D=AB, E=AC, F=BC, G=ABC", "B, C, F", Some(2.0));
        assert_eq!(
            rows,
            vec![
                ("B".to_string(), "AD, CF, EG".to_string()),
                ("C".to_string(), "AE, BF, DG".to_string()),
                ("F".to_string(), "AG, BC, DE".to_string()),
            ]
        );
    }

    #[test]
    fn fractalias_without_order_lists_longer_aliases_too() {
        // The defining words have 3 and 4 letters, so B is also aliased with 3- and 4-letter terms.
        let rows = alias_rows("A, B, C, D=AB, E=AC, F=BC, G=ABC", "B", None);
        assert_eq!(rows.len(), 1);
        let aliases: Vec<&str> = rows[0].1.split(", ").collect();
        assert_eq!(aliases.len(), 15);
        assert!(aliases.starts_with(&["AD", "CF", "EG"]));
        assert!(aliases.iter().any(|a| a.len() == 4));
    }

    #[test]
    fn fractalias_refuses_a_19_generator_design_with_many_terms() {
        // 7 base factors and 19 defined ones: 2^19 defining words, so 14 terms is 7.3 million
        // alias strings, over the cap. Refused before any string is built.
        let gen = "A, B, C, D, E, F, G, H=A, I=B, J=C, K=D, L=E, M=F, N=G, O=A, P=B, Q=C, R=D, \
                   S=E, T=F, U=G, V=A, W=B, X=C, Y=D, Z=E";
        let terms = "A, B, C, D, E, F, G, AB, AC, AD, AE, AF, AG, BC";
        let args = vec![Value::Str(gen.to_string()), Value::Str(terms.to_string())];
        let started = std::time::Instant::now();
        assert!(matches!(
            fractalias(&ctx(), &args),
            Err(EvalError::TooLarge(_))
        ));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn fractalias_refuses_bad_input() {
        let gen = "A, B, C=AB";
        for args in [
            vec![Value::Str(gen.to_string()), Value::Str("Z".to_string())],
            vec![Value::Str(gen.to_string()), Value::Str("".to_string())],
            vec![
                Value::Str(gen.to_string()),
                Value::Str("A".to_string()),
                Value::Scalar(real_scalar(0.0)),
            ],
            vec![
                Value::Str(gen.to_string()),
                Value::Str("A".to_string()),
                Value::Scalar(real_scalar(1.5)),
            ],
        ] {
            assert!(fractalias(&ctx(), &args).is_err(), "{args:?}");
        }
    }
}

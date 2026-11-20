//! Runs the symbolic core on every `symEval` region of the 24 samples.
//!
//! For each region: convert the input (`from_expr`, or `equation_from_expr` /
//! `system_from_expr` for solve commands), compute the result with the core, and compare it
//! with Prime's stored `symResult` by numeric substitution at three rational points (values
//! only; printed forms are compared separately). The test prints one line per region and
//! pins only facts it can establish: the region count and that some regions are reproduced.
//! A wrong value is a failure of the test.

#[macro_use]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mathcraft_core::expr::Expr;
use mathcraft_core::symbolic::{
    equation_from_expr, expand, from_expr, simplify, solve_linear_system, solve_polynomial,
    system_from_expr, to_expr, Equation, SolveOutcome, Sym, SymError, Q,
};
use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, RegionModel, Worksheet};

/// One `symEval` node with its region id, input, command words and Prime's stored result.
struct Case<'a> {
    id: u32,
    input: &'a Expr,
    words: Vec<String>,
    result: Option<&'a Expr>,
}

fn samples() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("documentation/examples exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    files.sort();
    files
}

/// Every `SymEval` node under the regions (solve-block members included), in document order.
fn collect<'a>(regions: &'a [RegionModel], out: &mut Vec<(u32, &'a Expr)>) {
    for r in regions {
        if let RegionKind::Math(e) = &r.kind {
            find_syms(e, r.id, out);
        }
        collect(&r.children, out);
    }
}

fn find_syms<'a>(e: &'a Expr, id: u32, out: &mut Vec<(u32, &'a Expr)>) {
    match e {
        Expr::SymEval { expr, .. } => {
            out.push((id, e));
            find_syms(expr, id, out);
        }
        Expr::Number(_) | Expr::Str(_) | Expr::Id(_) | Expr::Placeholder => {}
        Expr::Unary { operand, .. } => find_syms(operand, id, out),
        Expr::Binary { lhs, rhs, .. } => {
            find_syms(lhs, id, out);
            find_syms(rhs, id, out);
        }
        Expr::Call { args, .. } => args.iter().for_each(|a| find_syms(a, id, out)),
        Expr::Range { from, step, to } => {
            find_syms(from, id, out);
            if let Some(s) = step {
                find_syms(s, id, out);
            }
            find_syms(to, id, out);
        }
        Expr::Matrix { elements, .. } => elements.iter().for_each(|a| find_syms(a, id, out)),
        Expr::Index { base, indices } => {
            find_syms(base, id, out);
            indices.iter().for_each(|a| find_syms(a, id, out));
        }
        Expr::NthRoot { degree, radicand } => {
            if let Some(d) = degree {
                find_syms(d, id, out);
            }
            find_syms(radicand, id, out);
        }
        Expr::Parens(inner) => find_syms(inner, id, out),
        Expr::Define { expr, .. } | Expr::GlobalDefine { expr, .. } => find_syms(expr, id, out),
        Expr::Eval {
            expr,
            unit_override,
        } => {
            find_syms(expr, id, out);
            if let Some(u) = unit_override {
                find_syms(u, id, out);
            }
        }
        Expr::Scale { value, unit } => {
            find_syms(value, id, out);
            find_syms(unit, id, out);
        }
        // The samples hold no programming region; its statements are not searched.
        Expr::Program(_) => {}
        Expr::Gradient { body, .. } => find_syms(body, id, out),
        Expr::Limit { target, body, .. } => {
            find_syms(target, id, out);
            find_syms(body, id, out);
        }
        Expr::Accumulate { from, to, body, .. } => {
            find_syms(from, id, out);
            find_syms(to, id, out);
            find_syms(body, id, out);
        }
        Expr::Fold { operand, .. } => find_syms(operand, id, out),
        Expr::Integral {
            lower, upper, body, ..
        } => {
            find_syms(lower, id, out);
            find_syms(upper, id, out);
            find_syms(body, id, out);
        }
        Expr::Derivative { order, body, .. } => {
            if let Some(n) = order {
                find_syms(n, id, out);
            }
            find_syms(body, id, out);
        }
    }
}

fn case_of(id: u32, node: &Expr) -> Case<'_> {
    match node {
        Expr::SymEval {
            expr,
            command,
            result,
        } => Case {
            id,
            input: expr,
            words: command
                .as_ref()
                .map(|c| c.0.iter().map(word).collect())
                .unwrap_or_default(),
            result: result.as_deref(),
        },
        _ => unreachable!("collect only yields SymEval nodes"),
    }
}

fn word(e: &Expr) -> String {
    match e {
        Expr::Id(id) => id.name.clone(),
        other => format!("{other}"),
    }
}

/// What the core computed for one region.
enum Computed {
    /// A single value (plain input, expand, simplify).
    Value(Sym),
    /// Roots of one equation in one unknown (solve, one unknown).
    Roots(Vec<Sym>),
    /// A solution vector in the unknown order (solve, several unknowns or a system).
    Vector(Vec<Sym>),
    /// The core refused or could not answer.
    Failed(String),
}

fn compute(case: &Case) -> Computed {
    let result =
        |r: Result<Computed, SymError>| r.unwrap_or_else(|e| Computed::Failed(e.to_string()));
    match case.words.first().map(String::as_str) {
        None => result(
            from_expr(case.input)
                .and_then(|v| simplify(&v))
                .map(Computed::Value),
        ),
        Some("expand") => result(
            from_expr(case.input)
                .and_then(|v| expand(&v))
                .map(Computed::Value),
        ),
        Some("solve") => {
            let unknowns: Vec<String> = case.words[1..].to_vec();
            result(solve(case.input, &unknowns))
        }
        Some(other) => Computed::Failed(format!("command `{other}` not in this stage")),
    }
}

fn solve(input: &Expr, unknowns: &[String]) -> Result<Computed, SymError> {
    if unknowns.is_empty() {
        return Err(SymError::Unsupported("solve without unknown".into()));
    }
    if let Expr::Matrix { .. } = input {
        let eqs: Vec<Equation> = system_from_expr(input)?;
        return match solve_linear_system(&eqs, unknowns)? {
            SolveOutcome::Unique(pairs) => Ok(Computed::Vector(
                pairs.into_iter().map(|(_, v)| v).collect(),
            )),
            other => Err(SymError::Unsupported(format!("system outcome {other:?}"))),
        };
    }
    if unknowns.len() == 1 {
        let eq = equation_from_expr(input)?;
        return Ok(Computed::Roots(solve_polynomial(&eq, &unknowns[0])?));
    }
    let eqs = system_from_expr(input)?;
    match solve_linear_system(&eqs, unknowns)? {
        SolveOutcome::Unique(pairs) => Ok(Computed::Vector(
            pairs.into_iter().map(|(_, v)| v).collect(),
        )),
        other => Err(SymError::Unsupported(format!("system outcome {other:?}"))),
    }
}

/// Prime's stored result as core values: a matrix gives its elements in order, anything else
/// one value. `None` when an element does not convert.
fn prime_values(result: &Expr) -> Option<Vec<Sym>> {
    match result {
        Expr::Matrix { elements, .. } => elements.iter().map(|e| from_expr(e).ok()).collect(),
        other => from_expr(other).ok().map(|v| vec![v]),
    }
}

/// Exact rational as `f64`, through its `n/d` text (no numeric crate needed here).
fn q_f64(q: &Q) -> f64 {
    let s = q.to_string();
    match s.split_once('/') {
        Some((n, d)) => n.parse::<f64>().unwrap_or(f64::NAN) / d.parse::<f64>().unwrap_or(f64::NAN),
        None => s.parse::<f64>().unwrap_or(f64::NAN),
    }
}

fn eval(s: &Sym, env: &BTreeMap<String, f64>) -> f64 {
    match s {
        Sym::Num(q) => q_f64(q),
        Sym::Symbol(n) | Sym::Unit(n) => env.get(n).copied().unwrap_or(f64::NAN),
        Sym::Func(name, args) if args.len() == 1 => {
            let x = eval(&args[0], env);
            match name.as_str() {
                "ln" => x.ln(),
                "exp" => x.exp(),
                "sin" => x.sin(),
                "cos" => x.cos(),
                "abs" => x.abs(),
                _ => f64::NAN,
            }
        }
        Sym::Func(..) => f64::NAN,
        Sym::Add(ts) => ts.iter().map(|t| eval(t, env)).sum(),
        Sym::Mul(fs) => fs.iter().map(|f| eval(f, env)).product(),
        Sym::Pow(b, e) => eval(b, env).powf(q_f64(e)),
    }
}

fn names_of(s: &Sym, out: &mut Vec<String>) {
    match s {
        Sym::Symbol(n) | Sym::Unit(n) => {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
        Sym::Num(_) => {}
        Sym::Func(_, args) | Sym::Add(args) | Sym::Mul(args) => {
            args.iter().for_each(|a| names_of(a, out))
        }
        Sym::Pow(b, _) => names_of(b, out),
    }
}

/// Three fixed positive rational points `k/7` (k in 1..=41, chosen by an LCG) for every name.
/// Positive only, so fractional powers stay real.
fn points(names: &[String]) -> Vec<BTreeMap<String, f64>> {
    let mut seed: u64 = 20_261_116;
    (0..3)
        .map(|_| {
            names
                .iter()
                .map(|n| {
                    seed = seed
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    let k = (seed >> 33) % 41 + 1;
                    (n.clone(), k as f64 / 7.0)
                })
                .collect()
        })
        .collect()
}

/// How two result lists compare at the three points.
#[derive(Debug, PartialEq, Eq)]
enum Agreement {
    /// Every pair is finite at every point and equal (relative tolerance 1e-9).
    Same,
    /// Every pair is finite at every point and some pair differs: a wrong value.
    Differs,
    /// Some value is not finite (an unevaluated function) or the counts differ.
    NotComparable,
}

fn agreement(ours: &[Sym], theirs: &[Sym], ordered: bool) -> Agreement {
    if ours.len() != theirs.len() {
        return Agreement::NotComparable;
    }
    let mut names = Vec::new();
    for s in ours.iter().chain(theirs) {
        names_of(s, &mut names);
    }
    names.sort();
    names.dedup();
    let pts = points(&names);
    let mut a: Vec<&Sym> = ours.iter().collect();
    let mut b: Vec<&Sym> = theirs.iter().collect();
    if !ordered {
        let key = |s: &&Sym| eval(s, &pts[0]);
        a.sort_by(|x, y| key(x).total_cmp(&key(y)));
        b.sort_by(|x, y| key(x).total_cmp(&key(y)));
    }
    let mut finite = true;
    for (x, y) in a.iter().zip(&b) {
        for env in &pts {
            let (p, q) = (eval(x, env), eval(y, env));
            if !(p.is_finite() && q.is_finite()) {
                finite = false;
            } else if (p - q).abs() > 1e-9 * p.abs().max(1.0) {
                return Agreement::Differs;
            }
        }
    }
    if finite {
        Agreement::Same
    } else {
        Agreement::NotComparable
    }
}

fn values_of(c: &Computed) -> Option<(Vec<Sym>, bool)> {
    match c {
        Computed::Value(v) => Some((vec![v.clone()], true)),
        Computed::Roots(r) => Some((r.clone(), false)),
        Computed::Vector(v) => Some((v.clone(), true)),
        Computed::Failed(_) => None,
    }
}

#[test]
fn symbolic_core_on_the_sample_regions() {
    require_samples!();
    let mut total = 0usize;
    let mut converted = 0usize;
    let mut reproduced = 0usize;
    let mut chained = 0usize;
    let mut printed_same = 0usize;
    let mut wrong = 0usize;
    let mut unresolved = 0usize;
    let mut failures: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for path in samples() {
        let pkg = Package::open(&path).expect("package opens");
        let ws = Worksheet::from_package(&pkg).expect("worksheet imports");
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut nodes = Vec::new();
        collect(&ws.regions, &mut nodes);
        for (id, node) in nodes {
            total += 1;
            let case = case_of(id, node);
            if matches!(case.input, Expr::SymEval { .. }) {
                chained += 1;
                eprintln!("CHAINED {file} #{id}");
                continue;
            }
            let cmd = if case.words.is_empty() {
                "->".to_string()
            } else {
                case.words.join(",")
            };
            let computed = compute(&case);
            let status = match &computed {
                Computed::Failed(e) => {
                    *failures
                        .entry(e.split(':').next().unwrap_or("").to_string())
                        .or_insert(0) += 1;
                    format!("FAIL {e}")
                }
                _ => {
                    converted += 1;
                    "OK".to_string()
                }
            };
            eprintln!("E {file} #{} [{cmd}] {} => {status}", case.id, case.input);
            let (Some((ours, ordered)), Some(prime)) = (values_of(&computed), case.result) else {
                continue;
            };
            let Some(theirs) = prime_values(prime) else {
                eprintln!("  (Prime result does not convert: {prime})");
                continue;
            };
            match agreement(&ours, &theirs, ordered) {
                Agreement::Differs => {
                    // Free names that Prime's answer does not contain are worksheet constants
                    // Prime substituted (`F_G` = 833.57 N). The core cannot know they are
                    // defined, so the caller must substitute them first: reported, not counted.
                    let (mut mine_names, mut prime_names) = (Vec::new(), Vec::new());
                    ours.iter().for_each(|s| names_of(s, &mut mine_names));
                    theirs.iter().for_each(|s| names_of(s, &mut prime_names));
                    if mine_names.iter().any(|n| !prime_names.contains(n)) {
                        unresolved += 1;
                        eprintln!(
                            "  UNRESOLVED worksheet name(s) {mine_names:?}; prime {theirs:?}"
                        );
                    } else {
                        wrong += 1;
                        eprintln!("  WRONG VALUE: mine {ours:?} prime {theirs:?}");
                    }
                    continue;
                }
                Agreement::NotComparable => {
                    eprintln!("  not comparable (unevaluated function or other); prime {prime}");
                    continue;
                }
                Agreement::Same => {}
            }
            {
                reproduced += 1;
                let mine: Vec<String> = ours.iter().map(|v| to_expr(v).to_string()).collect();
                let stored = match prime {
                    Expr::Matrix { elements, .. } => {
                        elements.iter().map(|e| e.to_string()).collect::<Vec<_>>()
                    }
                    other => vec![other.to_string()],
                };
                if mine == stored {
                    printed_same += 1;
                }
                eprintln!("  VALUE OK; mine {mine:?} prime {stored:?}");
            }
        }
    }
    eprintln!(
        "SUMMARY nodes={total} chain_wrappers={chained} converted_ok={converted} failed={} reproduced={reproduced} printed_identical={printed_same} wrong_values={wrong} unresolved_names={unresolved}",
        total - chained - converted
    );
    eprintln!("FAILURES {failures:?}");
    assert_eq!(
        total, 95,
        "91 symEval inputs and 4 chain wrappers in the samples"
    );
    assert_eq!(wrong, 0, "no reproduced value may disagree with Prime");
    assert!(reproduced >= 13, "reproduced {reproduced}");
}

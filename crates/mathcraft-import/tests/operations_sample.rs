//! The calculus-and-table sample `documentation/examples/operations/Unbenannt.mcdx`.
//!
//! It holds one derivative, one definite integral, one summation with an empty lower bound, one
//! limit and a `spec-table` of three defines. None of the 24 top-level samples has them, so this
//! file pins what the reader, the printer, the writer and the evaluator do with them. The schema
//! is written up in `documentation/MCDX_FORMAT.md`, section "Calculus operators and tables".
//!
//! The sample is kept out of the pinned golden baseline (which lists the top-level samples only).
//! Its counts are pinned here instead.

#[macro_use]
mod support;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use mathcraft_core::expr::{Accumulation, Expr, Id, IdKind, Number};
use mathcraft_core::parser::{parse, IdContext};
use mathcraft_import::compare::{classify, Category};
use mathcraft_import::evaluate::{evaluate_worksheet_on_big_stack, load_results};
use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, RegionModel, Worksheet};
use mathcraft_import::writer::write_package;
use zip::ZipArchive;

fn sample_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../documentation/examples/operations/Unbenannt.mcdx")
}

fn load() -> (Vec<u8>, Worksheet) {
    let bytes = std::fs::read(sample_path()).expect("sample reads");
    let pkg = Package::from_bytes(&bytes).expect("package opens");
    let ws = Worksheet::from_package(&pkg).expect("worksheet imports");
    (bytes, ws)
}

fn math_of(region: &RegionModel) -> &Expr {
    match &region.kind {
        RegionKind::Math(expr) => expr,
        other => panic!("region {} is not math: {other:?}", region.id),
    }
}

fn id(name: &str, kind: IdKind) -> Id {
    Id::new(name, kind)
}

fn is_real(expr: &Expr, text: &str) -> bool {
    matches!(expr, Expr::Number(Number::Real { text: t, .. }) if t == text)
}

/// Units `m` and `s` are units; every other bare name is a variable (or a function when called).
struct SampleIds;

impl IdContext for SampleIds {
    fn classify(&self, name: &str, called: bool) -> IdKind {
        match name {
            "m" | "s" => IdKind::Unit,
            _ if called => IdKind::Function,
            _ => IdKind::Variable,
        }
    }
}

#[test]
fn sample_reads_with_five_regions_and_no_unsupported_content() {
    require_samples!();
    let (_, ws) = load();
    assert_eq!(ws.regions.len(), 5);
    assert_eq!(ws.report.region_count, 5);
    assert!(
        ws.report.unsupported.is_empty(),
        "no unsupported regions: {:?}",
        ws.report.unsupported
    );
    assert!(
        ws.report.notes.is_empty(),
        "no notes: {:?}",
        ws.report.notes
    );
}

#[test]
fn derivative_has_no_order_and_bound_variable_x() {
    require_samples!();
    let (_, ws) = load();
    let Expr::Derivative {
        var, order, body, ..
    } = math_of(&ws.regions[0])
    else {
        panic!("region 0 is not a derivative");
    };
    assert_eq!(**var, id("x", IdKind::Variable));
    assert!(order.is_none(), "an empty degree is the first derivative");
    assert_eq!(**body, Expr::Id(id("r", IdKind::Variable)));
}

#[test]
fn integral_takes_p_from_i_to_f_of_x() {
    require_samples!();
    let (_, ws) = load();
    let Expr::Integral {
        var,
        lower,
        upper,
        body,
    } = math_of(&ws.regions[1])
    else {
        panic!("region 1 is not an integral");
    };
    assert_eq!(**var, id("p", IdKind::Variable));
    assert_eq!(**lower, Expr::Id(id("i", IdKind::Variable)));
    assert_eq!(**upper, Expr::Id(id("f", IdKind::Variable)));
    assert_eq!(**body, Expr::Id(id("x", IdKind::Variable)));
}

#[test]
fn summation_has_empty_lower_bound_as_placeholder() {
    require_samples!();
    let (_, ws) = load();
    let Expr::Accumulate {
        op,
        var,
        from,
        to,
        body,
    } = math_of(&ws.regions[2])
    else {
        panic!("region 2 is not a summation");
    };
    assert_eq!(*op, Accumulation::Sum);
    assert_eq!(**var, id("f", IdKind::Variable));
    assert_eq!(**from, Expr::Placeholder, "no lowerBound element");
    assert!(is_real(to, "2"));
    assert_eq!(**body, Expr::Id(id("y", IdKind::Variable)));
}

#[test]
fn limit_is_two_sided_with_target_nine() {
    require_samples!();
    let (_, ws) = load();
    let Expr::Limit { var, target, body } = math_of(&ws.regions[3]) else {
        panic!("region 3 is not a limit");
    };
    assert_eq!(**var, id("x", IdKind::Variable));
    assert!(is_real(target, "9"));
    assert_eq!(**body, Expr::Id(id("o", IdKind::Variable)));
}

#[test]
fn table_is_one_region_with_three_member_defines() {
    require_samples!();
    let (_, ws) = load();
    let RegionKind::Table(table) = &ws.regions[4].kind else {
        panic!("region 4 is not a table");
    };
    let refs: Vec<Option<u32>> = table.members.iter().map(|m| m.result_ref).collect();
    assert_eq!(refs, [Some(4), Some(5), Some(6)]);
    let names: Vec<String> = table
        .members
        .iter()
        .map(|m| match m.expr.as_ref().expect("member converts") {
            Expr::Define { target, expr } => {
                assert!(
                    matches!(expr.as_ref(), Expr::Scale { .. }),
                    "member is a scale"
                );
                target.to_string()
            }
            other => panic!("member is not a define: {other}"),
        })
        .collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert!(ws.regions[4].children.is_empty(), "members are not regions");
    assert_eq!(ws.regions[4].pos.map(|p| p.0.round() as i64), Some(189));
}

#[test]
fn every_math_region_prints_and_parses_back_to_the_same_tree() {
    require_samples!();
    let (_, ws) = load();
    let mut exprs: Vec<Expr> = ws.regions[..4].iter().map(|r| math_of(r).clone()).collect();
    if let RegionKind::Table(table) = &ws.regions[4].kind {
        exprs.extend(
            table
                .members
                .iter()
                .map(|m| m.expr.clone().expect("converts")),
        );
    }
    for expr in exprs {
        let text = expr.to_string();
        let back =
            parse(&text, &SampleIds).unwrap_or_else(|e| panic!("`{text}` does not parse: {e:?}"));
        assert_eq!(back, expr, "`{text}` reads back as a different tree");
    }
}

fn entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip opens");
    let mut out = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("entry");
        let name = entry.name().expect("name").into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("entry reads");
        out.insert(name, data);
    }
    out
}

#[test]
fn writer_round_trip_keeps_the_model_and_every_other_part_byte_for_byte() {
    require_samples!();
    let (source, ws) = load();
    let out = write_package(&ws, Some(&source)).expect("writes");
    let again = write_package(&ws, Some(&source)).expect("writes again");
    assert_eq!(out, again, "output is deterministic");

    let written = Package::from_bytes(&out).expect("written package reads");
    let back = Worksheet::from_package(&written).expect("written worksheet models");
    assert_eq!(back, ws, "model after the round trip");

    let before = entries(&source);
    let after = entries(&out);
    for (part, bytes) in &before {
        if part == "mathcad/worksheet.xml" {
            continue;
        }
        assert_eq!(
            after.get(part),
            Some(bytes),
            "part `{part}` is copied unchanged"
        );
    }
}

#[test]
fn written_worksheet_has_the_same_calculus_and_table_elements() {
    require_samples!();
    let (source, ws) = load();
    let out = write_package(&ws, Some(&source)).expect("writes");
    let written = Package::from_bytes(&out).expect("written package reads");
    let source_pkg = Package::from_bytes(&source).expect("source reads");
    let shape = |xml: &str| -> Vec<String> {
        let doc = roxmltree::Document::parse(xml).expect("well-formed");
        doc.descendants()
            .filter(|n| n.is_element())
            .filter(|n| {
                matches!(
                    n.tag_name().name(),
                    "derivative"
                        | "integral"
                        | "summation"
                        | "limit"
                        | "lowerBound"
                        | "upperBound"
                        | "degree"
                        | "boundVars"
                        | "spec-table"
                        | "define"
                        | "scale"
                        | "placeholder"
                        | "matrix"
                )
            })
            .map(|n| {
                let text: String = n
                    .children()
                    .filter(|c| c.is_text())
                    .filter_map(|c| c.text())
                    .collect();
                format!("{}:{}", n.tag_name().name(), text.trim())
            })
            .collect()
    };
    let before = shape(&source_pkg.worksheet);
    let after = shape(&written.worksheet);
    assert_eq!(after, before);
}

#[test]
fn evaluation_counts_are_pinned_and_nothing_panics() {
    require_samples!();
    let (_, ws) = load();
    let pkg = Package::from_bytes(&std::fs::read(sample_path()).unwrap()).unwrap();
    let table = load_results(&pkg).expect("result.xml parses");
    let run = evaluate_worksheet_on_big_stack(&ws, &table);
    assert_eq!(
        run.regions.len(),
        7,
        "four operators plus three table members"
    );
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for row in &run.regions {
        let key = match classify(row) {
            Category::Pass | Category::WeakPass(_) => "Pass",
            Category::Mismatch(_) => "Mismatch",
            Category::Unsupported(_) => "Unsupported",
            Category::EngineError(_) => "EngineError",
            Category::EngineErrorNoRef(_) => "EngineErrorNoRef",
            Category::NoReference => "NoReference",
            _ => "other",
        };
        *counts.entry(key).or_default() += 1;
    }
    // The limit region `limit(x, 9, o)` now evaluates (to the free name `o`, exact core), so it
    // moves from Unsupported to NoReference: no stored result exists to compare with.
    let expected: BTreeMap<&str, usize> = [
        ("EngineErrorNoRef", 1),
        ("NoReference", 4),
        ("Unsupported", 2),
    ]
    .into_iter()
    .collect();
    assert_eq!(counts, expected);
}

#[test]
fn integral_bound_i_is_the_imaginary_unit_and_is_refused() {
    require_samples!();
    let (source, _) = load();
    let pkg = Package::from_bytes(&source).unwrap();
    let ws = Worksheet::from_package(&pkg).unwrap();
    let table = load_results(&pkg).unwrap();
    let run = evaluate_worksheet_on_big_stack(&ws, &table);
    let integral = run
        .regions
        .iter()
        .find(|r| r.region_id == 1)
        .expect("region 1");
    match classify(integral) {
        Category::Unsupported(reason) => assert!(reason.contains("complex limit"), "{reason}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn derivative_with_free_names_reports_an_undefined_name() {
    require_samples!();
    let (source, _) = load();
    let pkg = Package::from_bytes(&source).unwrap();
    let ws = Worksheet::from_package(&pkg).unwrap();
    let table = load_results(&pkg).unwrap();
    let run = evaluate_worksheet_on_big_stack(&ws, &table);
    let derivative = run
        .regions
        .iter()
        .find(|r| r.region_id == 0)
        .expect("region 0");
    match classify(derivative) {
        Category::EngineErrorNoRef(reason) => assert!(reason.contains("undefined"), "{reason}"),
        other => panic!("expected EngineErrorNoRef, got {other:?}"),
    }
}

#[test]
fn table_members_are_evaluated_as_defines_in_order() {
    require_samples!();
    let (source, _) = load();
    let pkg = Package::from_bytes(&source).unwrap();
    let ws = Worksheet::from_package(&pkg).unwrap();
    let table = load_results(&pkg).unwrap();
    let run = evaluate_worksheet_on_big_stack(&ws, &table);
    let members: Vec<&str> = run
        .regions
        .iter()
        .filter(|r| r.region_id == 4)
        .map(|r| r.expr_text.as_str())
        .collect();
    assert_eq!(members.len(), 3);
    assert!(members[0].starts_with("a:="), "{members:?}");
    assert!(members[2].starts_with("c:="), "{members:?}");
    // Each member goes to the engine (it is not skipped), so a failure would be an engine error.
    for row in run.regions.iter().filter(|r| r.region_id == 4) {
        assert!(
            matches!(
                row.outcome,
                mathcraft_import::evaluate::EvalOutcome::Engine(_)
            ),
            "member is run by the engine"
        );
    }
}

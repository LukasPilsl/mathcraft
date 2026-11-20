//! Reads `mathcad/result.xml` from every sample `.mcdx` and checks the parse against the samples.

#[macro_use]
mod support;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use mathcraft_import::results::{parse_results, ResultTable, ResultValue, Status, UnitRef};

const GERMAN_UNDEFINED: &str =
    "Diese Variable ist nicht definiert. Prüfen Sie, ob die Beschriftung richtig festgelegt ist.";

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

fn sample_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(examples_dir())
        .expect("read documentation/examples")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    paths.sort();
    paths
}

fn read_result_xml(path: &Path) -> String {
    let file = File::open(path).expect("open .mcdx");
    let mut archive = zip::ZipArchive::new(file).expect("open .mcdx as zip");
    let mut entry = archive
        .by_name("mathcad/result.xml")
        .expect("mathcad/result.xml in package");
    let mut text = String::new();
    entry.read_to_string(&mut text).expect("read result.xml");
    text
}

fn load(name: &str) -> ResultTable {
    let path = examples_dir().join(name);
    parse_results(&read_result_xml(&path)).expect("parse result.xml")
}

fn kind(v: &ResultValue) -> &'static str {
    match v {
        ResultValue::Empty => "Empty",
        ResultValue::Real(_) => "Real",
        ResultValue::United { .. } => "United",
        ResultValue::Matrix { .. } => "Matrix",
        ResultValue::Error { .. } => "Error",
        ResultValue::Trace2d(_) => "Trace2d",
        ResultValue::Symbolic(_) => "Symbolic",
        ResultValue::Unknown(_) => "Unknown",
    }
}

const KINDS: [&str; 8] = [
    "Empty", "Real", "United", "Matrix", "Error", "Trace2d", "Symbolic", "Unknown",
];

/// Number of `<resultData>` elements in the raw XML, counted without the reader under test.
fn raw_result_data_count(xml: &str) -> usize {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).expect("xml");
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "resultData")
        .count()
}

#[test]
fn every_sample_parses_every_result_data() {
    require_samples!();
    let paths = sample_paths();
    assert_eq!(paths.len(), 24, "expected the 24 sample packages");

    let mut header = format!("{:<46}{:>6}", "sample", "count");
    for k in KINDS {
        header.push_str(&format!("{k:>9}"));
    }
    println!("{header}");

    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unknowns = Vec::new();
    for path in &paths {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let xml = read_result_xml(path);
        let table = parse_results(&xml).expect("parse result.xml");

        assert_eq!(
            table.by_id.len(),
            raw_result_data_count(&xml),
            "{name}: every resultData must be parsed"
        );

        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for (id, data) in &table.by_id {
            let k = kind(&data.value);
            *counts.entry(k).or_default() += 1;
            *totals.entry(k).or_default() += 1;
            if let ResultValue::Unknown(what) = &data.value {
                unknowns.push(format!("{name} result-id {id}: {what}"));
            }
        }

        let mut row = format!("{:<46}{:>6}", name, table.by_id.len());
        for k in KINDS {
            row.push_str(&format!("{:>9}", counts.get(k).copied().unwrap_or(0)));
        }
        println!("{row}");
    }

    let mut total_row = format!("{:<46}{:>6}", "TOTAL", totals.values().sum::<usize>());
    for k in KINDS {
        total_row.push_str(&format!("{:>9}", totals.get(k).copied().unwrap_or(0)));
    }
    println!("{total_row}");

    assert!(
        unknowns.is_empty(),
        "unknown result shapes (none are documented):\n{}",
        unknowns.join("\n")
    );
}

#[test]
fn engine_errors_parse_with_german_message() {
    require_samples!();
    for (sample, expected) in [
        ("3_rata_3_Kurve_durch_3_Punkte.mcdx", 1),
        ("Mechanik_3_Spannungsberechnung.mcdx", 3),
    ] {
        let table = load(sample);
        let errors: Vec<&ResultValue> = table
            .by_id
            .values()
            .map(|d| &d.value)
            .filter(|v| matches!(v, ResultValue::Error { .. }))
            .collect();
        assert_eq!(errors.len(), expected, "{sample}: error entries");
        for v in errors {
            match v {
                ResultValue::Error { code, message } => {
                    assert!(code.starts_with("bad%_variable"), "{sample}: code {code:?}");
                    assert_eq!(message, GERMAN_UNDEFINED, "{sample}: message");
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn united_value_from_mechanik_1_is_exact() {
    require_samples!();
    let table = load("Mechanik_1_massen_ziehen.mcdx");
    let data = &table.by_id[&19];
    assert_eq!(data.status, Status::Synchronized);
    assert_eq!(
        data.value,
        ResultValue::United {
            value: 1015.1544629760358,
            units: vec![UnitRef {
                unit: "newton".to_string(),
                num: 1,
                den: 1,
            }],
        }
    );
}

#[test]
fn matrix_of_united_values_from_golfball_parses() {
    require_samples!();
    let table = load("17_rata_golfball_donau_lsg.mcdx");
    match &table.by_id[&7].value {
        ResultValue::Matrix {
            rows,
            cols,
            elements,
        } => {
            assert_eq!((*rows, *cols), (1, 2));
            assert_eq!(elements.len(), 2);
            assert!(matches!(&elements[1], ResultValue::United { units, .. }
                if units == &[UnitRef { unit: "second".into(), num: 1, den: 1 }]));
        }
        other => panic!("expected a matrix, got {other:?}"),
    }
}

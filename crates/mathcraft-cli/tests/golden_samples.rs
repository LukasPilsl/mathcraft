//! Golden baseline over the 24 samples in `documentation/examples`.
//!
//! The counts and the known-failure list are pinned on purpose. Any change in engine behaviour
//! (a new pass, a new failure, a region that becomes unsupported) shows up here as a diff. To
//! update after an intended change, rerun `cargo run -p mathcraft-cli -- golden --verbose`,
//! check every change by hand, and then edit the constants below.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mathcraft_cli::{golden, inspect_file, render_run, Category, GoldenRun};

#[macro_use]
mod common;

use common::baseline::{BASELINE, KNOWN_FAILURES, KNOWN_NO_REFERENCE_ERRORS};

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

fn sample(name: &str) -> PathBuf {
    samples_dir().join(name)
}

fn on_disk_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(samples_dir())
        .expect("documentation/examples exists")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".mcdx"))
        .collect();
    names.sort();
    names
}

fn run_all() -> GoldenRun {
    let files: Vec<PathBuf> = BASELINE.iter().map(|(name, _)| sample(name)).collect();
    golden(&files)
}

#[test]
fn baseline_lists_every_sample_on_disk() {
    require_samples!();
    let pinned: Vec<String> = BASELINE.iter().map(|(n, _)| (*n).to_string()).collect();
    assert_eq!(pinned, on_disk_names());
}

#[test]
fn golden_counts_match_the_pinned_baseline() {
    require_samples!();
    let run = run_all();
    println!("{}", render_run(&run, true));

    for file in &run.files {
        assert!(
            file.file_error.is_none(),
            "{}: {:?}",
            file.name,
            file.file_error
        );
    }
    assert_eq!(run.files.len(), BASELINE.len());

    for (file, (name, expected)) in run.files.iter().zip(BASELINE) {
        assert_eq!(&file.name, name);
        let tally = file.tally();
        let expected_unsupported: BTreeMap<String, usize> = expected
            .unsupported
            .iter()
            .map(|(reason, n)| ((*reason).to_string(), *n))
            .collect();
        assert_eq!(tally.pass, expected.pass, "{name}: Pass");
        assert_eq!(tally.mismatch, expected.mismatch, "{name}: Mismatch");
        assert_eq!(
            tally.engine_error, expected.engine_error,
            "{name}: EngineError"
        );
        assert_eq!(
            tally.engine_error_no_ref, expected.engine_error_no_ref,
            "{name}: EngineErrorNoRef"
        );
        assert_eq!(
            tally.no_reference, expected.no_reference,
            "{name}: NoReference"
        );
        assert_eq!(
            tally.unsupported, expected_unsupported,
            "{name}: Unsupported"
        );
    }
}

#[test]
fn every_mismatch_and_engine_error_is_a_known_failure() {
    require_samples!();
    let run = run_all();
    let mut actual: Vec<(String, u32, &str)> = Vec::new();
    for file in &run.files {
        for row in &file.rows {
            match &row.category {
                Category::Mismatch(_) => {
                    actual.push((file.name.clone(), row.region_id, "Mismatch"))
                }
                Category::EngineError(_) => {
                    actual.push((file.name.clone(), row.region_id, "EngineError"))
                }
                _ => {}
            }
        }
    }
    let mut expected: Vec<(String, u32, &str)> = KNOWN_FAILURES
        .iter()
        .map(|(f, id, kind, _)| ((*f).to_string(), *id, *kind))
        .collect();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    assert_eq!(
        run.failure_count(),
        KNOWN_FAILURES.len() + KNOWN_NO_REFERENCE_ERRORS.len()
    );
}

#[test]
fn every_engine_error_without_reference_is_a_known_gap() {
    require_samples!();
    let run = run_all();
    let mut actual: Vec<(String, u32)> = Vec::new();
    for file in &run.files {
        for row in &file.rows {
            if let Category::EngineErrorNoRef(_) = &row.category {
                actual.push((file.name.clone(), row.region_id));
            }
        }
    }
    let mut expected: Vec<(String, u32)> = KNOWN_NO_REFERENCE_ERRORS
        .iter()
        .map(|(f, id, _)| ((*f).to_string(), *id))
        .collect();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn inspect_shows_order_stored_results_and_settings() {
    require_samples!();
    let text = inspect_file(&sample("3_rata_3_Kurve_durch_3_Punkte.mcdx")).expect("inspect");
    println!("{text}");
    assert!(text.contains("settings: ORIGIN=0"), "{text}");
    assert!(text.contains("y(x):=a*x^2+b*x+c"), "{text}");
    assert!(text.contains("stored: error bad%_variable"), "{text}");
    assert!(text.contains("plot    xyPlot"), "{text}");
    let first = text.find("#1 ").expect("region 1 listed");
    let second = text.find("#2 ").expect("region 2 listed");
    assert!(first < second, "evaluation order: region 1 before region 2");
}

#[test]
fn inspect_marks_solve_block_members() {
    require_samples!();
    let text = inspect_file(&sample("17_rata_golfball_donau_lsg_2.mcdx")).expect("inspect");
    println!("{text}");
    assert!(text.contains("[solve find, guess-value]"), "{text}");
    assert!(
        text.contains("stored: 240.00000000000003 SI [meter]"),
        "{text}"
    );
}

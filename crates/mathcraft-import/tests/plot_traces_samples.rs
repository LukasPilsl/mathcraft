//! Every 2D plot trace of the 24 samples, compared with Prime's stored `<Trace2dResult>` (the
//! trace's `resultRef` in `result.xml`). Run with `-- --nocapture` for the per-trace table.

#[macro_use]
mod support;

use std::path::PathBuf;

use mathcraft_import::compare::compare_trace;
use mathcraft_import::evaluate::{
    evaluate_worksheet, load_results, on_big_stack, EvalOutcome, WorksheetEvaluation,
};
use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, Worksheet};
use mathcraft_import::results::{ResultTable, ResultValue, Trace2d};

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

/// One sample: its name, worksheet, stored results and evaluation.
struct Sample {
    name: String,
    worksheet: Worksheet,
    table: ResultTable,
    evaluation: WorksheetEvaluation,
}

fn all_samples() -> Vec<Sample> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .expect("samples dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    paths.sort();
    on_big_stack(move || {
        paths
            .into_iter()
            .map(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                let pkg = Package::open(&path).expect("open");
                let worksheet = Worksheet::from_package(&pkg).expect("worksheet");
                let table = load_results(&pkg).expect("results");
                let evaluation = evaluate_worksheet(&worksheet, &table);
                Sample {
                    name,
                    worksheet,
                    table,
                    evaluation,
                }
            })
            .collect()
    })
}

/// The stored `<Trace2dResult>` of a result id, when it is plot data.
fn trace_entry(table: &ResultTable, id: u32) -> Option<&Trace2d> {
    match &table.by_id.get(&id)?.value {
        ResultValue::Trace2d(t) => Some(t),
        _ => None,
    }
}

#[test]
fn every_sample_trace_result_is_read_with_consistent_lengths() {
    require_samples!();
    let samples = all_samples();
    let mut read = 0;
    for sample in &samples {
        for region in sample.worksheet.all_regions() {
            let RegionKind::Plot(info) = &region.kind else {
                continue;
            };
            for id in &info.traces {
                let Some(t) = trace_entry(&sample.table, *id) else {
                    continue;
                };
                read += 1;
                let points = t
                    .points()
                    .unwrap_or_else(|e| panic!("{} #{}: {e}", sample.name, region.id));
                assert!(!points.is_empty(), "{} #{}", sample.name, region.id);
                assert!(
                    matches!(t.trace_type.as_str(), "Range" | "Parametric" | "Vector"),
                    "{} #{}: {}",
                    sample.name,
                    region.id,
                    t.trace_type
                );
                if t.trace_type == "Vector" {
                    assert_eq!(points.len(), 1, "{} #{}", sample.name, region.id);
                }
            }
        }
    }
    assert_eq!(read, 85, "Trace2dResult entries referenced by plot traces");
}

#[test]
fn every_sample_trace_with_stored_data_matches_prime() {
    require_samples!();
    let samples = all_samples();
    let mut stored_total = 0;
    let mut compared = 0;
    let mut failures = Vec::new();
    let mut errors = Vec::new();
    for sample in &samples {
        for region in sample.worksheet.all_regions() {
            let RegionKind::Plot(info) = &region.kind else {
                continue;
            };
            let Some(row) = sample
                .evaluation
                .regions
                .iter()
                .find(|e| e.region_id == region.id)
            else {
                continue;
            };
            let EvalOutcome::Plot(plot) = &row.outcome else {
                continue;
            };
            for (index, (id, trace)) in info.traces.iter().zip(&plot.traces).enumerate() {
                let Some(stored) = trace_entry(&sample.table, *id) else {
                    continue;
                };
                stored_total += 1;
                let label = format!("{} #{} trace {}", sample.name, region.id, index + 1);
                if let Some(error) = &trace.error {
                    errors.push(format!("{label}: {}", error.message));
                    continue;
                }
                compared += 1;
                match compare_trace(stored, trace) {
                    Ok(()) => println!("{label}: {} points, Pass", stored.range_points.len()),
                    Err(e) => {
                        println!(
                            "{label}: {} points, Mismatch ({e})",
                            stored.range_points.len()
                        );
                        failures.push(format!("{label}: {e}"));
                    }
                }
            }
        }
    }
    for e in &errors {
        println!("not compared (evaluation error): {e}");
    }
    println!(
        "stored traces {stored_total}, compared {compared}, evaluation errors {}, mismatches {}",
        errors.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(compared + errors.len(), stored_total);
}

/// The two plots that found the plot defects: sample 6 region 7 (the sampled domain is the stored
/// one, -5 to 10/3, not the displayed -5.1 to 3.4) and sample 15 region 23 (a `float` body rounds
/// the coefficients of its line). Each trace with stored data must evaluate and match Prime.
#[test]
fn the_domain_and_float_plots_match_prime_trace_by_trace() {
    require_samples!();
    let samples = all_samples();
    let mut checked = 0;
    for (sample_name, region_id) in [
        ("6_rata_18_gerade_durch_2_punkte.mcdx", 7),
        ("15_rata_Inkreismittelpunkt.mcdx", 23),
    ] {
        let sample = samples
            .iter()
            .find(|s| s.name == sample_name)
            .expect("sample on disk");
        let region = sample
            .worksheet
            .all_regions()
            .into_iter()
            .find(|r| r.id == region_id)
            .expect("region");
        let RegionKind::Plot(info) = &region.kind else {
            panic!("{sample_name} #{region_id} is not a plot");
        };
        let row = sample
            .evaluation
            .regions
            .iter()
            .find(|e| e.region_id == region_id)
            .expect("evaluated");
        let EvalOutcome::Plot(plot) = &row.outcome else {
            panic!("{sample_name} #{region_id}: not evaluated as a plot");
        };
        for (index, (id, trace)) in info.traces.iter().zip(&plot.traces).enumerate() {
            let Some(stored) = trace_entry(&sample.table, *id) else {
                continue;
            };
            let label = format!("{sample_name} #{region_id} trace {}", index + 1);
            assert!(trace.error.is_none(), "{label}: {:?}", trace.error);
            compare_trace(stored, trace).unwrap_or_else(|e| panic!("{label}: {e}"));
            checked += 1;
        }
    }
    // Sample 6 region 7 has 6 traces with stored data, sample 15 region 23 has 12.
    assert_eq!(checked, 18, "every stored trace of the two plots");
}

#[test]
fn stored_trace_reader_keeps_the_nan_gaps() {
    require_samples!();
    let samples = all_samples();
    let gaps: usize = samples
        .iter()
        .flat_map(|s| s.table.by_id.values())
        .filter_map(|d| match &d.value {
            ResultValue::Trace2d(t) => Some(t),
            _ => None,
        })
        .map(|t| {
            t.points()
                .expect("lengths")
                .iter()
                .filter(|p| p.is_none())
                .count()
        })
        .sum();
    assert!(gaps > 0, "the samples have NaN gaps in stored traces");
}

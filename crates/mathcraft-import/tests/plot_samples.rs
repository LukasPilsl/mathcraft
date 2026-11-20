//! Every plot of the 24 samples: read into the model, evaluated, and classified. Prints one line
//! per plot and trace (`-- --nocapture`).

#[macro_use]
mod support;

use std::path::PathBuf;

use mathcraft_import::compare::{classify, Category};
use mathcraft_import::evaluate::{
    evaluate_worksheet, load_results, on_big_stack, EvalOutcome, WorksheetEvaluation,
};
use mathcraft_import::package::Package;
use mathcraft_import::regions::{Equation, Plot2d, RegionKind, Worksheet};

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

struct SamplePlots {
    name: String,
    /// `(region_id, kind, has 2D model, classification)` per plot region.
    plots: Vec<(u32, String, bool, Category)>,
    evaluation: WorksheetEvaluation,
    worksheet: Worksheet,
}

fn run_sample(path: PathBuf) -> SamplePlots {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let pkg = Package::open(&path).expect("open");
    let ws = Worksheet::from_package(&pkg).expect("read worksheet");
    let table = load_results(&pkg).expect("results");
    let evaluation = evaluate_worksheet(&ws, &table);
    let plots = ws
        .all_regions()
        .into_iter()
        .filter_map(|r| match &r.kind {
            RegionKind::Plot(info) => {
                let row = evaluation.regions.iter().find(|e| e.region_id == r.id)?;
                Some((r.id, info.kind.clone(), info.plot.is_some(), classify(row)))
            }
            _ => None,
        })
        .collect();
    SamplePlots {
        name,
        plots,
        evaluation,
        worksheet: ws,
    }
}

fn all_samples() -> Vec<SamplePlots> {
    let dir = samples_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("samples dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    paths.sort();
    on_big_stack(move || paths.into_iter().map(run_sample).collect())
}

/// The 2D model of one plot region of a sample.
fn plot_model(sample: &SamplePlots, id: u32) -> Plot2d {
    let region = sample
        .worksheet
        .all_regions()
        .into_iter()
        .find(|r| r.id == id)
        .expect("region");
    match &region.kind {
        RegionKind::Plot(info) => *info.plot.clone().expect("a 2D model"),
        other => panic!("region {id} is {other:?}"),
    }
}

#[test]
fn every_sample_plot_is_read_and_classified() {
    require_samples!();
    let samples = all_samples();
    let mut total = 0;
    let mut two_d = 0;
    for sample in &samples {
        for (id, kind, has_model, category) in &sample.plots {
            total += 1;
            if *has_model {
                two_d += 1;
            }
            println!(
                "{} region #{id} {kind} 2D={has_model} => {category:?}",
                sample.name
            );
        }
    }
    // 23 plots in the samples: 19 xyPlot, 2 polarPlot, 2 plot3D.
    assert_eq!(total, 23, "plot regions in the samples");
    assert_eq!(two_d, 21, "2D plots with a model");
    for sample in &samples {
        for (id, kind, has_model, category) in &sample.plots {
            if !has_model {
                // The two 3D plots: sheet 13 matches Prime's stored grids, sheet 14 does not (its
                // stored surface does not follow the sheet, see the golden baseline).
                assert!(
                    matches!(category, Category::Pass | Category::Mismatch(_)),
                    "{} #{id} {kind}: {category:?}",
                    sample.name
                );
            }
        }
    }
}

#[test]
fn every_sample_plot_trace_is_printed() {
    require_samples!();
    for sample in all_samples() {
        for region in sample.worksheet.all_regions() {
            let RegionKind::Plot(_) = &region.kind else {
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
            if let EvalOutcome::Plot(data) = &row.outcome {
                for (i, trace) in data.traces.iter().enumerate() {
                    println!(
                        "  {} #{} trace {}: segments {:?} gaps {} error {:?}",
                        sample.name,
                        region.id,
                        i + 1,
                        trace.segments.iter().map(Vec::len).collect::<Vec<_>>(),
                        trace.gaps,
                        trace.error
                    );
                }
            }
        }
    }
}

#[test]
fn sample_3_reads_its_traces_axes_and_styles() {
    require_samples!();
    let sample = run_sample(samples_dir().join("3_rata_3_Kurve_durch_3_Punkte.mcdx"));
    let plot = plot_model(&sample, 5);
    assert!(!plot.polar);
    assert_eq!(plot.traces.len(), 4);
    // Trace 1 is y(x) over x; traces 2 to 4 are single points (no num-of-points).
    assert_eq!(plot.traces[0].num_of_points, Some(500));
    assert!(plot.traces[1..].iter().all(|t| t.num_of_points.is_none()));
    assert_eq!(plot.x_axis.start, Some(0.0));
    assert_eq!(plot.x_axis.end, Some(6.0));
    assert_eq!(plot.y_axis.start, Some(-3.0));
    assert_eq!(plot.y_axis.end, Some(8.0));
    assert!(!plot.x_axis.log && !plot.y_axis.log);
    assert!(plot.x_axis.grid_lines);
    assert_eq!(plot.x_axis.grid_frequency, Some(11));
    let style = &plot.traces[0].style;
    assert_eq!(style.color, Some([0x00, 0x00, 0x8B, 0xFF]));
    assert_eq!(style.line_weight, Some(1.0));
    assert_eq!(style.line_style.as_deref(), Some("Solid"));
    assert_eq!(style.symbol.as_deref(), Some("none"));
    assert_eq!(style.mode.as_deref(), Some("lines"));
    assert_eq!(plot.traces[3].style.color, Some([0xFF, 0xFF, 0x00, 0xFF]));
    assert!(matches!(plot.traces[0].x, Equation::Expr(_)));
    assert!(matches!(plot.traces[0].y, Equation::Expr(_)));
}

#[test]
fn sample_8_shares_the_last_x_equation_across_traces() {
    require_samples!();
    // The x axis lists two equations (SP[0] and x) for six traces: traces 2 to 6 use x.
    let sample = run_sample(samples_dir().join("8_rata_18_Schwerpunkt_Dreieck.mcdx"));
    let plot = plot_model(&sample, 11);
    assert_eq!(plot.x_axis.equations.len(), 2);
    assert_eq!(plot.traces.len(), 6);
    assert_eq!(plot.y_axis.equations.len(), 6);
    for trace in &plot.traces {
        assert!(
            matches!(&trace.y, Equation::Expr(_)),
            "every trace has a y equation"
        );
    }
}

#[test]
fn sample_12_polar_plot_uses_angular_and_radial_axes() {
    require_samples!();
    let sample = run_sample(samples_dir().join("12_rata_Kreisdiagramme.mcdx"));
    let plot = plot_model(&sample, 5);
    assert!(plot.polar);
    assert_eq!(plot.x_axis.start, Some(0.0), "angular axis");
    assert_eq!(plot.x_axis.end, Some(360.0), "stated in degrees");
    assert_eq!(plot.y_axis.start, Some(1.0), "radial axis");
    assert_eq!(plot.y_axis.end, Some(2.0));
    assert_eq!(plot.traces.len(), 1);
}

#[test]
fn sample_13_3d_plot_has_no_2d_model_and_matches_its_stored_grids() {
    require_samples!();
    let sample = run_sample(samples_dir().join("13_rata_plot1.mcdx"));
    let (_, kind, has_model, category) = &sample.plots[0];
    assert_eq!(kind, "plot3D");
    assert!(!has_model);
    assert_eq!(category, &Category::Pass);
}

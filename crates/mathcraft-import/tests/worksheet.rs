//! Builds the worksheet model for every sample `.mcdx` under `documentation/examples` and checks
//! it against the raw files. Run with `-- --nocapture` to see the per-file kind table.

#[macro_use]
mod support;

use std::path::{Path, PathBuf};

use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, RegionModel, Worksheet};
use mathcraft_import::results::parse_results;

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

fn sample_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .expect("read examples dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "mcdx"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 24, "expected the 24 sample worksheets");
    paths
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned()
}

/// Counts `<region ` start tags in raw worksheet text, independently of the XML parser.
fn raw_region_count(xml: &str) -> usize {
    xml.matches("<region ").count()
}

fn load(path: &Path) -> (Package, Worksheet) {
    let pkg = Package::open(path).unwrap_or_else(|e| panic!("{}: {e}", file_name(path)));
    let ws = Worksheet::from_package(&pkg).unwrap_or_else(|e| panic!("{}: {e}", file_name(path)));
    (pkg, ws)
}

#[derive(Default, Clone, Copy)]
struct Tally {
    math: usize,
    text: usize,
    solve: usize,
    plot: usize,
    table: usize,
    unsupported: usize,
}

fn tally(regions: &[&RegionModel]) -> Tally {
    let mut t = Tally::default();
    for region in regions {
        match region.kind {
            RegionKind::Math(_) => t.math += 1,
            RegionKind::Text(_) => t.text += 1,
            RegionKind::SolveBlock(_) => t.solve += 1,
            RegionKind::Plot(_) => t.plot += 1,
            RegionKind::Table(_) => t.table += 1,
            RegionKind::Unsupported { .. } => t.unsupported += 1,
            RegionKind::Area(_) | RegionKind::Image(_) | RegionKind::PageBreak => {}
        }
    }
    t
}

#[test]
fn every_sample_loads_and_no_region_is_dropped() {
    require_samples!();
    println!(
        "{:<48} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5}",
        "file", "regions", "Math", "Text", "Solve", "Plot", "Unsup", "orig"
    );
    let mut total = Tally::default();
    let mut total_regions = 0;
    let mut reasons = Vec::new();
    for path in sample_paths() {
        let (pkg, ws) = load(&path);
        let raw = raw_region_count(&pkg.worksheet);
        let all = ws.all_regions();
        assert_eq!(all.len(), raw, "{}: regions in model", file_name(&path));
        assert_eq!(
            ws.report.region_count,
            raw,
            "{}: report count",
            file_name(&path)
        );
        let t = tally(&all);
        assert_eq!(
            t.math + t.text + t.solve + t.plot + t.table + t.unsupported,
            raw,
            "{}: every region has exactly one kind",
            file_name(&path)
        );
        println!(
            "{:<48} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5}",
            file_name(&path),
            raw,
            t.math,
            t.text,
            t.solve,
            t.plot,
            t.unsupported,
            ws.settings
                .array_origin
                .map_or("-".to_owned(), |o| o.to_string()),
        );
        for u in &ws.report.unsupported {
            reasons.push(format!(
                "{} region {}: {}",
                file_name(&path),
                u.region_id,
                u.reason
            ));
        }
        assert_eq!(ws.report.unsupported.len(), t.unsupported);
        total.math += t.math;
        total.text += t.text;
        total.solve += t.solve;
        total.plot += t.plot;
        total.table += t.table;
        total.unsupported += t.unsupported;
        total_regions += raw;
    }
    println!(
        "{:<48} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "TOTAL", total_regions, total.math, total.text, total.solve, total.plot, total.unsupported
    );
    println!("unsupported regions: {}", total.unsupported);
    for reason in &reasons {
        println!("  {reason}");
    }
    // Exact tallies for the 24 samples. Zero unsupported regions means every sample region is
    // handled. A change to these counts is a change to the model's coverage or to the samples.
    assert_eq!(
        (
            total_regions,
            total.math,
            total.text,
            total.solve,
            total.plot,
            total.unsupported
        ),
        (401, 366, 5, 7, 23, 0),
        "sample tallies"
    );
}

#[test]
fn evaluation_order_is_a_permutation_of_all_non_container_regions() {
    require_samples!();
    for path in sample_paths() {
        let (_, ws) = load(&path);
        let expected: Vec<&RegionModel> = ws
            .all_regions()
            .into_iter()
            .filter(|r| !matches!(r.kind, RegionKind::SolveBlock(_)))
            .collect();
        let order = ws.evaluation_order();
        assert_eq!(order.len(), expected.len(), "{}: length", file_name(&path));
        for region in &expected {
            let hits = order.iter().filter(|r| std::ptr::eq(**r, *region)).count();
            assert_eq!(
                hits,
                1,
                "{}: region {} appears once",
                file_name(&path),
                region.id
            );
        }
    }
}

fn order_ids(ws: &Worksheet) -> Vec<u32> {
    ws.evaluation_order().iter().map(|r| r.id).collect()
}

/// Mechanik_1: 24 top-level regions on 16 rows. Rows are ordered by top, and within a row by
/// left. Hand-checked from the XML: 37.8 (ids 0, 1, 2 at left 37.8, 170.1, 302.4), 94.5 (3, 4,
/// 5), 151.2 (6, 7), 188.9 (8, 9), 226.8 (10, 11), 264.6 (12, 13), 321.3 (14), 359.1 (15),
/// 415.7 (16), 453.5 (17), 491.3 (18), 510.2 (19), 548.0 (20), 585.8 (21), 642.5 (22), 699.2 (23).
/// The document order is the same, so the expected order is 0 to 23.
#[test]
fn mechanik_1_order_is_position_order() {
    require_samples!();
    let (_, ws) = load(&samples_dir().join("Mechanik_1_massen_ziehen.mcdx"));
    assert_eq!(order_ids(&ws), (0..24).collect::<Vec<u32>>());
}

/// 1_rata_1: 14 top-level regions, all on different rows with left 0. Tops (pt): id 0 18.9,
/// 1 75.6, 2 151.2, 3 226.8, 4 283.5, 5 321.3, 6 359.1, 7 415.7, 8 453.5, 9 472.4, 10 510.2,
/// 11 548.0, 12 585.8, 13 642.5. Ascending, so the order is 0 to 13.
#[test]
fn one_rata_1_order_is_position_order() {
    require_samples!();
    let (_, ws) = load(&samples_dir().join("1_rata_1.mcdx"));
    assert_eq!(order_ids(&ws), (0..14).collect::<Vec<u32>>());
}

/// 17_rata_schiefer_wurf: regions 0 to 3 (tops 37.8 to 151.2), then solve block 4 at top 189.0
/// with members 5 (rel. top 18.9), 6 (94.5) and 7 (207.9). The block itself is not returned.
/// Then 8 (472.4), 9 (510.2), and the plots 10 (top 529.1, left 0) and 11 (529.1, left 302.4).
#[test]
fn schiefer_wurf_order_puts_block_members_in_place() {
    require_samples!();
    let (_, ws) = load(&samples_dir().join("17_rata_schiefer_wurf.mcdx"));
    assert_eq!(order_ids(&ws), vec![0, 1, 2, 3, 5, 6, 7, 8, 9, 10, 11]);
}

#[test]
fn stored_results_join_to_every_region_with_a_result_ref() {
    require_samples!();
    for path in sample_paths() {
        let (pkg, ws) = load(&path);
        let xml = pkg
            .result
            .as_deref()
            .unwrap_or_else(|| panic!("{}: result.xml missing", file_name(&path)));
        let table = parse_results(xml).unwrap_or_else(|e| panic!("{}: {e}", file_name(&path)));
        let joined = ws.attach_stored_results(&table);
        let linked: Vec<u32> = ws
            .all_regions()
            .into_iter()
            .filter(|r| r.result_ref.is_some())
            .map(|r| r.id)
            .collect();
        let got: Vec<u32> = joined.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            got,
            linked,
            "{}: every region with a resultRef has a result",
            file_name(&path)
        );
    }
}

/// Plot 20 in 10_rata_18_UMP_Dreieck (xyPlot, six traces, x axis -5.1 to 3.4) checked against the XML.
#[test]
fn plot_info_reads_type_traces_and_axis() {
    require_samples!();
    let (_, ws) = load(&samples_dir().join("10_rata_18_UMP_Dreieck.mcdx"));
    let plot = ws
        .all_regions()
        .into_iter()
        .find(|r| r.id == 20)
        .expect("region 20");
    let RegionKind::Plot(info) = &plot.kind else {
        panic!("region 20 is not a plot: {:?}", plot.kind);
    };
    assert_eq!(info.kind, "xyPlot");
    assert_eq!(info.traces, vec![20, 21, 22, 23, 24, 25]);
    assert_eq!(info.x_range, Some((-5.1, 3.4)));
}

/// The five text regions of the 24 samples, as (file, region id, text). The text comes from the
/// `Xaml/Document.xaml` of each region's flow document, one line per paragraph.
#[test]
fn text_regions_hold_the_text_of_their_flow_documents() {
    require_samples!();
    let mut found = Vec::new();
    for path in sample_paths() {
        let (_, ws) = load(&path);
        for region in ws.all_regions() {
            if let RegionKind::Text(text) = &region.kind {
                found.push((file_name(&path), region.id, text.to_string()));
            }
        }
        assert!(
            ws.report.notes.is_empty(),
            "{}: no text note expected, got {:?}",
            file_name(&path),
            ws.report.notes
        );
    }
    let expected: Vec<(String, u32, String)> = [
        ("11_rata_AA_eulersche_Gerade.mcdx", 3, "Berechnung  SP"),
        ("11_rata_AA_eulersche_Gerade.mcdx", 9, "Berechnung HSP"),
        ("11_rata_AA_eulersche_Gerade.mcdx", 19, "Berechnung UMP"),
        ("2_rata_2.mcdx", 5, "Hallo strg+t = Text"),
        ("2_rata_2.mcdx", 6, " "),
    ]
    .into_iter()
    .map(|(file, id, text)| (file.to_owned(), id, text.to_owned()))
    .collect();
    assert_eq!(found, expected);
}

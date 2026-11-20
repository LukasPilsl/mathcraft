//! Evaluates every sample in `documentation/examples` with `evaluate_worksheet` and classifies
//! each region with `classify`. Pins the per-category totals, and checks that no region is
//! dropped: every Math, Plot and Unsupported region (solve-block members included) gets exactly
//! one evaluation.

#[macro_use]
mod support;

use std::path::{Path, PathBuf};

use mathcraft_import::compare::{classify, Category};
use mathcraft_import::evaluate::{evaluate_worksheet_on_big_stack, load_results};
use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, Worksheet};

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

/// Counts per category, with the golden report's rules: a weak pass is also a pass.
#[derive(Debug, Default, PartialEq, Eq)]
struct Totals {
    pass: usize,
    mismatch: usize,
    unsupported: usize,
    engine_error: usize,
    engine_error_no_ref: usize,
    no_reference: usize,
    plot_ok: usize,
}

impl Totals {
    fn add(&mut self, category: &Category) {
        match category {
            Category::Pass => self.pass += 1,
            Category::WeakPass(_) => self.pass += 1,
            Category::Mismatch(_) => self.mismatch += 1,
            Category::Unsupported(_) => self.unsupported += 1,
            Category::EngineError(_) => self.engine_error += 1,
            Category::EngineErrorNoRef(_) => self.engine_error_no_ref += 1,
            Category::NoReference => self.no_reference += 1,
            Category::PlotOk(_) => self.plot_ok += 1,
        }
    }
}

#[test]
fn samples_pin_the_category_totals_and_drop_no_region() {
    require_samples!();
    let files = samples();
    assert_eq!(files.len(), 24, "the 24 samples are on disk");

    let mut totals = Totals::default();
    for path in &files {
        let pkg = Package::open(path).expect("package opens");
        let ws = Worksheet::from_package(&pkg).expect("worksheet imports");
        let table = load_results(&pkg).expect("results load");
        let evaluation = evaluate_worksheet_on_big_stack(&ws, &table);

        let mut expected_ids: Vec<u32> = ws
            .all_regions()
            .into_iter()
            .filter(|r| {
                matches!(
                    r.kind,
                    RegionKind::Math(_) | RegionKind::Plot(_) | RegionKind::Unsupported { .. }
                )
            })
            .map(|r| r.id)
            .collect();
        let mut actual_ids: Vec<u32> = evaluation.regions.iter().map(|r| r.region_id).collect();
        expected_ids.sort_unstable();
        actual_ids.sort_unstable();
        assert_eq!(actual_ids, expected_ids, "{}: regions", path.display());

        for region in &evaluation.regions {
            totals.add(&classify(region));
        }
    }

    assert_eq!(
        totals,
        Totals {
            pass: 103,
            mismatch: 2,
            unsupported: 5,
            engine_error: 0,
            engine_error_no_ref: 12,
            no_reference: 267,
            plot_ok: 0,
        }
    );
}

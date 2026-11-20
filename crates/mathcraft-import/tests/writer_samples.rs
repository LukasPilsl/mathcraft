//! Writer round trip of the 24 sample packages in `documentation/examples`.
//!
//! For each sample, the worksheet is read, written back with the source, and read again. The
//! checks are:
//!
//! * the written worksheet reads to the same model (regions, expressions, text, positions,
//!   settings and the import report);
//! * every source part the model does not regenerate is byte-for-byte the same (headers,
//!   footers, properties, presentation, integration, XamlPackages, results, calculation);
//! * the output is deterministic;
//! * evaluating the written file gives the same evaluation as the source, and the golden totals
//!   stay at the pinned values (Pass 56, Mismatch 1, Unsupported 61, EngineError 22,
//!   EngineErrorNoRef 26, NoReference 223).

#[macro_use]
mod support;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use mathcraft_import::compare::{classify, Category};
use mathcraft_import::evaluate::{evaluate_worksheet_on_big_stack, load_results};
use mathcraft_import::package::Package;
use mathcraft_import::regions::Worksheet;
use mathcraft_import::writer::write_package;
use zip::ZipArchive;

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

/// Every entry of a ZIP as name to contents.
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
            Category::Pass | Category::WeakPass(_) => self.pass += 1,
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
fn every_sample_round_trips_through_the_writer() {
    require_samples!();
    let files = samples();
    assert_eq!(files.len(), 24, "the 24 samples are on disk");
    for path in &files {
        let source = std::fs::read(path).expect("sample reads");
        let name = path.display();
        let pkg = Package::from_bytes(&source).expect("package opens");
        let model = Worksheet::from_package(&pkg).expect("worksheet imports");

        let out = write_package(&model, Some(&source))
            .unwrap_or_else(|e| panic!("{name}: write failed: {e}"));
        let again = write_package(&model, Some(&source)).expect("second write");
        assert_eq!(out, again, "{name}: output is deterministic");

        let written = Package::from_bytes(&out).expect("written package reads");
        let back = Worksheet::from_package(&written).expect("written worksheet models");
        assert_eq!(back, model, "{name}: model after the round trip");

        let before = entries(&source);
        let after = entries(&out);
        for (part, bytes) in &before {
            if part == "mathcad/worksheet.xml" || part == "mathcad/_rels/worksheet.xml.rels" {
                continue;
            }
            assert_eq!(
                after.get(part),
                Some(bytes),
                "{name}: part `{part}` must be copied unchanged"
            );
        }
    }
}

#[test]
fn golden_evaluation_of_written_samples_is_unchanged() {
    require_samples!();
    let mut written_totals = Totals::default();
    let mut source_totals = Totals::default();
    for path in &samples() {
        let source = std::fs::read(path).expect("sample reads");
        let name = path.display();
        let pkg = Package::from_bytes(&source).expect("package opens");
        let model = Worksheet::from_package(&pkg).expect("worksheet imports");
        let table = load_results(&pkg).expect("results load");
        let expected = evaluate_worksheet_on_big_stack(&model, &table);

        let out = write_package(&model, Some(&source)).expect("writes");
        let written = Package::from_bytes(&out).expect("written package reads");
        let back = Worksheet::from_package(&written).expect("written worksheet models");
        let written_table = load_results(&written).expect("written results load");
        let actual = evaluate_worksheet_on_big_stack(&back, &written_table);

        assert_eq!(actual, expected, "{name}: evaluation of the written file");
        for region in &expected.regions {
            source_totals.add(&classify(region));
        }
        for region in &actual.regions {
            written_totals.add(&classify(region));
        }
    }
    let pinned = Totals {
        pass: 103,
        mismatch: 2,
        unsupported: 5,
        engine_error: 0,
        engine_error_no_ref: 12,
        no_reference: 267,
        plot_ok: 0,
    };
    assert_eq!(source_totals, pinned, "source golden totals");
    assert_eq!(written_totals, pinned, "golden totals of the written files");
}

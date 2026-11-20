//! Page header and footer regions, and page-break markers.
//!
//! No sample has header or footer content or a page break, so the synthetic cases edit a copy of
//! a sample in memory. The element shapes they use are guesses (see `regions.rs`).

#[macro_use]
mod support;

use std::path::PathBuf;

use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, Worksheet};

fn sample(name: &str) -> Package {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../documentation/examples")
        .join(name);
    Package::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

const WS: &str = "http://schemas.mathsoft.com/worksheet50";

#[test]
fn every_sample_has_an_empty_header_and_footer() {
    require_samples!();
    for entry in std::fs::read_dir(support::examples_dir()).expect("examples dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "mcdx") {
            continue;
        }
        let pkg = Package::open(&path).expect("open");
        let ws = Worksheet::from_package(&pkg).expect("import");
        assert!(ws.header.is_empty(), "{}", path.display());
        assert!(ws.footer.is_empty(), "{}", path.display());
    }
}

#[test]
fn header_region_is_read_apart_from_the_sheet() {
    require_samples!();
    let mut pkg = sample("1_rata_1.mcdx");
    let before = Worksheet::from_package(&pkg).expect("import").regions.len();
    pkg.header = Some(format!(
        r#"<header xmlns="{WS}"><regions><region region-id="500" top="10" left="20" actualWidth="200" actualHeight="20"><text /></region></regions></header>"#
    ));
    let ws = Worksheet::from_package(&pkg).expect("import");
    assert_eq!(
        ws.regions.len(),
        before,
        "header regions are not sheet regions"
    );
    assert_eq!(ws.header.len(), 1);
    let region = &ws.header[0];
    assert_eq!(region.id, 500);
    assert_eq!(region.pos, Some((20.0, 10.0)));
    assert_eq!(region.size, Some((200.0, 20.0)));
    assert!(matches!(&region.kind, RegionKind::Text(t) if t.as_str().is_empty()));
    assert!(ws.footer.is_empty());
}

#[test]
fn page_break_region_is_read_and_not_evaluated() {
    require_samples!();
    let mut pkg = sample("1_rata_1.mcdx");
    let at = pkg.worksheet.find("</regions>").expect("regions end");
    pkg.worksheet.insert_str(
        at,
        r#"<region region-id="900" top="300" left="0"><page-break /></region>"#,
    );
    let ws = Worksheet::from_package(&pkg).expect("import");
    let breaks: Vec<_> = ws
        .all_regions()
        .into_iter()
        .filter(|r| matches!(r.kind, RegionKind::PageBreak))
        .map(|r| (r.id, r.pos))
        .collect();
    assert_eq!(breaks, vec![(900, Some((0.0, 300.0)))]);
}

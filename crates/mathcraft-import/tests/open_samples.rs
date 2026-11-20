//! Opens every sample `.mcdx` under `documentation/examples`, parses its worksheet and result
//! parts, and checks that regions are found. Prints a per-file table (`-- --nocapture`).

#[macro_use]
mod support;

use std::path::{Path, PathBuf};

use mathcraft_import::dom;
use mathcraft_import::package::Package;

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples")
}

/// Counts `<region` start tags in raw worksheet text, independently of the XML parser.
/// `<regions` also contains `<region`, so container tags are subtracted.
fn raw_region_count(xml: &str) -> usize {
    xml.matches("<region").count() - xml.matches("<regions").count()
}

/// Returns (regions, regions with math, regions with a result ref) for one package.
fn inspect(path: &Path) -> Result<(usize, usize, usize), String> {
    let pkg = Package::open(path).map_err(|e| e.to_string())?;
    let worksheet = dom::parse(&pkg.worksheet).map_err(|e| e.to_string())?;
    let regions = dom::regions(&worksheet).map_err(|e| e.to_string())?;
    let result_xml = pkg.result.as_deref().ok_or("result.xml missing")?;
    dom::parse(result_xml).map_err(|e| e.to_string())?;
    if regions.is_empty() {
        return Err("no regions found".to_owned());
    }
    let expected = raw_region_count(&pkg.worksheet);
    if regions.len() != expected {
        return Err(format!(
            "regions() returned {} but worksheet has {expected} region elements",
            regions.len()
        ));
    }
    let with_math = regions.iter().filter(|r| r.math.is_some()).count();
    let with_ref = regions.iter().filter(|r| r.result_ref.is_some()).count();
    Ok((regions.len(), with_math, with_ref))
}

#[test]
fn sample_5_finds_nested_regions() {
    require_samples!();
    let path = samples_dir().join("5_rata_4_Gerade_schneidet_Kreis.mcdx");
    let pkg = Package::open(&path).expect("open sample 5");
    let worksheet = dom::parse(&pkg.worksheet).expect("parse worksheet");
    let regions = dom::regions(&worksheet).expect("regions parse");
    assert_eq!(regions.len(), 15, "all region elements, nested included");
    // Verified separately with python3 -I (xml.etree.ElementTree, not roxmltree): unzipped the
    // sample into an empty temp dir and counted `<region>` elements that have a direct `<math>`
    // child in mathcad/worksheet.xml. Result: 15 region elements, 12 with a direct `<math>`
    // child. The 3 without are region-ids 0, 7 (both contain nested regions) and 14.
    let with_math = regions.iter().filter(|r| r.math.is_some()).count();
    assert_eq!(with_math, 12, "regions with a direct math child");
}

#[test]
fn all_samples_open_and_yield_regions() {
    require_samples!();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(samples_dir())
        .expect("read examples dir")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "mcdx"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 24, "expected 24 sample packages");

    println!(
        "{:<48} {:>8} {:>10} {:>10}",
        "file", "regions", "with_math", "with_ref"
    );
    let mut failures = Vec::new();
    for path in &paths {
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        match inspect(path) {
            Ok((total, math, refs)) => {
                println!("{name:<48} {total:>8} {math:>10} {refs:>10}");
            }
            Err(err) => {
                println!("{name:<48} FAILED: {err}");
                failures.push(format!("{name}: {err}"));
            }
        }
    }
    assert!(failures.is_empty(), "samples failed: {failures:#?}");
}

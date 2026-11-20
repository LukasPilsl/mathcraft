//! Per-region result formats (`<math><resultFormat><general>`) of a sample: read, changed through
//! the writer, and read back. Other attributes of the element keep their bytes.

#[macro_use]
mod support;

use std::io::Read;
use std::path::Path;

use mathcraft_core::eval::{MatrixDisplay, NumberFormat, ResultFormat};
use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, RegionModel, Worksheet};
use mathcraft_import::writer::{write_package, write_package_with_report, WriteError};
use zip::ZipArchive;

/// Region 5 of `1_rata_1` states its own general format (`precision="10"`).
fn sample_1_rata_1() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples/1_rata_1.mcdx");
    std::fs::read(path).expect("1_rata_1.mcdx is readable")
}

fn worksheet_xml(bytes: &[u8]) -> String {
    let mut archive = ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip opens");
    let mut entry = archive
        .by_name("mathcad/worksheet.xml")
        .expect("worksheet part");
    let mut text = String::new();
    entry.read_to_string(&mut text).expect("utf-8");
    text
}

/// The raw `resultFormat` element of the math region `id` in `worksheet.xml`.
fn raw_result_format(bytes: &[u8], id: u32) -> String {
    let xml = worksheet_xml(bytes);
    let start = xml
        .find(&format!("<region region-id=\"{id}\""))
        .expect("region present");
    let rest = &xml[start..];
    let from = rest.find("<resultFormat>").expect("resultFormat present");
    let to = rest[from..].find("</resultFormat>").expect("closed") + from + "</resultFormat>".len();
    rest[from..to].to_owned()
}

fn set_region_format(regions: &mut [RegionModel], id: u32, format: Option<ResultFormat>) -> bool {
    for region in regions {
        if region.id == id {
            region.result_format = format;
            return true;
        }
        if set_region_format(&mut region.children, id, format.clone()) {
            return true;
        }
    }
    false
}

fn region_format(ws: &Worksheet, id: u32) -> Option<ResultFormat> {
    ws.all_regions()
        .into_iter()
        .find(|r| r.id == id)
        .and_then(|r| r.result_format.clone())
}

#[test]
fn a_region_format_is_read_and_unchanged_bytes_are_kept() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let own = region_format(&model, 5).expect("region 5 states its own format");
    assert_eq!(own.precision, 10);
    assert!(!own.trailing_zeros);

    let unchanged = write_package(&model, Some(&source)).expect("write succeeds");
    assert_eq!(
        raw_result_format(&unchanged, 5),
        raw_result_format(&source, 5)
    );
}

#[test]
fn a_changed_region_format_patches_only_its_attribute() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let changed = ResultFormat {
        precision: 4,
        ..region_format(&model, 5).expect("own format")
    };
    assert!(set_region_format(
        &mut model.regions,
        5,
        Some(changed.clone())
    ));
    let out = write_package(&model, Some(&source)).expect("write succeeds");
    assert_eq!(
        raw_result_format(&out, 5),
        raw_result_format(&source, 5).replace(r#"precision="10""#, r#"precision="4""#)
    );
    let back = Worksheet::from_package(&Package::from_bytes(&out).expect("reads")).expect("models");
    assert_eq!(region_format(&back, 5), Some(changed));
}

#[test]
fn a_region_without_a_format_gets_no_element_for_the_default() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let other = model
        .all_regions()
        .into_iter()
        .find(|r| r.id != 5 && r.result_format.is_none() && matches!(r.kind, RegionKind::Math(_)))
        .map(|r| r.id);
    let Some(other) = other else {
        panic!("the sample has a math region without its own format");
    };
    assert!(set_region_format(
        &mut model.regions,
        other,
        Some(ResultFormat::default())
    ));
    let out = write_package(&model, Some(&source)).expect("write succeeds");
    let back = Worksheet::from_package(&Package::from_bytes(&out).expect("reads")).expect("models");
    assert_eq!(region_format(&back, other), None);
}

fn set_region_matrix(regions: &mut [RegionModel], id: u32, matrix: MatrixDisplay) -> bool {
    for region in regions {
        if region.id == id {
            region.matrix = matrix;
            return true;
        }
        if set_region_matrix(&mut region.children, id, matrix) {
            return true;
        }
    }
    false
}

fn region_matrix(ws: &Worksheet, id: u32) -> Option<MatrixDisplay> {
    ws.all_regions()
        .into_iter()
        .find(|r| r.id == id)
        .map(|r| r.matrix)
}

#[test]
fn a_region_matrix_display_round_trips_through_the_writer() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let id = model
        .all_regions()
        .into_iter()
        .find(|r| matches!(r.kind, RegionKind::Math(_)))
        .map(|r| r.id)
        .expect("the sample has a math region");
    let before = region_matrix(&model, id).expect("region present");
    let changed = MatrixDisplay {
        rows: 4,
        show_indices: true,
        expand_nested: true,
        ..before
    };
    assert!(set_region_matrix(&mut model.regions, id, changed));
    let out = write_package(&model, Some(&source)).expect("write succeeds");
    let back = Worksheet::from_package(&Package::from_bytes(&out).expect("reads")).expect("models");
    assert_eq!(region_matrix(&back, id), Some(changed));
    // The region's general format is untouched by a matrix change.
    assert_eq!(region_format(&back, id), region_format(&model, id));
}

#[test]
fn the_unit_system_round_trips_through_the_writer() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    model.settings.unit_system = Some("cgs".to_owned());
    let out = write_package(&model, Some(&source)).expect("write succeeds");
    let back = Worksheet::from_package(&Package::from_bytes(&out).expect("reads")).expect("models");
    assert_eq!(back.settings.unit_system.as_deref(), Some("cgs"));
}

#[test]
fn a_region_complex_threshold_change_is_refused() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let changed = ResultFormat {
        complex_threshold: 3,
        ..region_format(&model, 5).expect("own format")
    };
    assert!(set_region_format(&mut model.regions, 5, Some(changed)));
    assert!(matches!(
        write_package(&model, Some(&source)),
        Err(WriteError::Unsupported(_))
    ));
}

const UNVERIFIED_WARNING: &str =
    "unverified file format written: result number format (region {}); Prime may not read it";

#[test]
fn a_region_number_format_is_written_with_an_unverified_warning() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let own = region_format(&model, 5).expect("own format");
    let changed = ResultFormat {
        number: NumberFormat::Scientific,
        ..own
    };
    assert!(set_region_format(&mut model.regions, 5, Some(changed)));
    let (out, warnings) = write_package_with_report(&model, Some(&source)).expect("write");
    assert_eq!(warnings, vec![UNVERIFIED_WARNING.replace("{}", "5")]);
    assert!(raw_result_format(&out, 5).contains(r#"display="scientific""#));
}

#[test]
fn a_new_region_digit_grouping_is_written_with_an_unverified_warning() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let other = model
        .all_regions()
        .into_iter()
        .find(|r| r.id != 5 && r.result_format.is_none() && matches!(r.kind, RegionKind::Math(_)))
        .map(|r| r.id)
        .expect("a math region without its own format");
    let grouped = ResultFormat {
        digit_grouping: true,
        ..ResultFormat::default()
    };
    assert!(set_region_format(&mut model.regions, other, Some(grouped)));
    let (_, warnings) = write_package_with_report(&model, Some(&source)).expect("write");
    assert_eq!(
        warnings,
        vec![UNVERIFIED_WARNING.replace("{}", &other.to_string())]
    );
}

#[test]
fn defaults_and_verified_changes_emit_no_warning() {
    require_samples!();
    let source = sample_1_rata_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let (unchanged, warnings) = write_package_with_report(&model, Some(&source)).expect("write");
    assert!(warnings.is_empty());
    assert_eq!(
        unchanged,
        write_package(&model, Some(&source)).expect("write")
    );
    let own = region_format(&model, 5).expect("own format");
    let precise = ResultFormat {
        precision: 4,
        ..own
    };
    assert!(set_region_format(&mut model.regions, 5, Some(precise)));
    let (_, warnings) = write_package_with_report(&model, Some(&source)).expect("write");
    assert!(warnings.is_empty());
}

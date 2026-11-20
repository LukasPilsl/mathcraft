//! Result display settings of the samples: read from `presentation.xml`, changed through the
//! writer, and read back. A display-only change keeps the stored results and the evaluation
//! settings.

#[macro_use]
mod support;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use mathcraft_core::eval::{NumberFormat, Radix, ResultFormat};
use mathcraft_import::package::Package;
use mathcraft_import::regions::Worksheet;
use mathcraft_import::writer::{write_package, WriteError};
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

fn mechanik_1() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../documentation/examples/Mechanik_1_massen_ziehen.mcdx");
    std::fs::read(path).expect("sample reads")
}

#[test]
fn every_sample_states_the_prime_default_display() {
    require_samples!();
    for path in samples() {
        let source = std::fs::read(&path).expect("sample reads");
        let pkg = Package::from_bytes(&source).expect("package opens");
        let model = Worksheet::from_package(&pkg).expect("worksheet imports");
        let name = path.display();
        assert_eq!(
            model.settings.result_format,
            Some(ResultFormat::default()),
            "{name}: presentation.xml general element"
        );
        assert_eq!(
            model.settings.prn_precision,
            Some(4),
            "{name}: PRNPRECISION"
        );
    }
}

#[test]
fn a_display_change_is_written_read_back_and_keeps_the_results() {
    require_samples!();
    let source = mechanik_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    model.settings.result_format = Some(ResultFormat {
        precision: 6,
        trailing_zeros: true,
        exponential_threshold: 5,
        zero_threshold: 12,
        radix: Radix::Hexadecimal,
        imaginary_j: true,
        ..ResultFormat::default()
    });
    model.settings.prn_precision = Some(6);

    let out = write_package(&model, Some(&source)).expect("write succeeds");
    let written = Package::from_bytes(&out).expect("written package reads");
    let back = Worksheet::from_package(&written).expect("written worksheet models");
    assert_eq!(
        back.settings, model.settings,
        "settings after the round trip"
    );
    assert_eq!(back.regions, model.regions, "regions are unchanged");

    let presentation = written.presentation.as_deref().expect("presentation part");
    for attr in [
        r#"precision="6""#,
        r#"show-trailing-zeros="true""#,
        r#"radix="hex""#,
        r#"zero-threshold="12""#,
        r#"exponential-threshold="5""#,
        r#"imaginary-value="j""#,
    ] {
        assert!(presentation.contains(attr), "presentation has {attr}");
    }

    let before = entries(&source);
    let after = entries(&out);
    assert_eq!(
        after.get("mathcad/result.xml"),
        before.get("mathcad/result.xml"),
        "stored results survive a display-only change"
    );
    let calc =
        String::from_utf8_lossy(after.get("mathcad/settings/calculation.xml").expect("calc"));
    assert!(
        calc.contains(r#"prn-precision="6""#),
        "PRNPRECISION is written"
    );
    assert!(
        calc.contains(r#"array-origin="0""#),
        "other calculation settings stay"
    );
}

#[test]
fn an_unchanged_write_keeps_the_presentation_bytes() {
    require_samples!();
    let source = mechanik_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let model = Worksheet::from_package(&pkg).expect("worksheet imports");
    let out = write_package(&model, Some(&source)).expect("write succeeds");
    assert_eq!(
        entries(&out).get("mathcad/settings/presentation.xml"),
        entries(&source).get("mathcad/settings/presentation.xml"),
    );
}

#[test]
fn a_number_format_or_digit_grouping_in_presentation_is_refused() {
    // No sample states these attributes in presentation.xml, whose schema is strict: a guessed
    // attribute could make Prime refuse the whole file, so the change is refused.
    require_samples!();
    let source = mechanik_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    for format in [
        ResultFormat {
            number: NumberFormat::Decimal,
            ..ResultFormat::default()
        },
        ResultFormat {
            number: NumberFormat::Percent,
            ..ResultFormat::default()
        },
        ResultFormat {
            digit_grouping: true,
            ..ResultFormat::default()
        },
    ] {
        model.settings.result_format = Some(format);
        match write_package(&model, Some(&source)) {
            Err(WriteError::Settings(msg)) => assert!(msg.contains("(unverified)"), "{msg}"),
            other => panic!("expected a settings refusal, got {other:?}"),
        }
    }
}

#[test]
fn a_polar_complex_form_is_refused() {
    require_samples!();
    let source = mechanik_1();
    let pkg = Package::from_bytes(&source).expect("package opens");
    let mut model = Worksheet::from_package(&pkg).expect("worksheet imports");
    model.settings.result_format = Some(ResultFormat {
        complex_form: mathcraft_core::eval::ComplexForm::Polar(
            mathcraft_core::eval::AngleUnit::Degrees,
        ),
        ..ResultFormat::default()
    });
    assert!(matches!(
        write_package(&model, Some(&source)),
        Err(WriteError::Settings(_))
    ));
}

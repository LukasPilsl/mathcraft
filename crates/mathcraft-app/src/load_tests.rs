//! Hostile and damaged input: every case must give an error text, never a panic. Archives are
//! built in memory; nothing is written to disk.

use std::io::{Cursor, Write};
use std::path::Path;

use mathcraft_import::evaluate::on_big_stack;
use mathcraft_import::package::{Package, WORKSHEET};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::load::{load_file, load_package};
use crate::testdata::{examples_dir, sample};
use crate::viewmodel::ViewModel;

/// A zip archive of stored (uncompressed) entries.
fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in parts {
        writer.start_file(*name, options).expect("start part");
        writer.write_all(body).expect("write part");
    }
    writer.finish().expect("finish zip").into_inner()
}

/// Opens bytes as a package and loads it, as `load_file` does for a file.
fn load_bytes(bytes: Vec<u8>) -> Result<ViewModel, String> {
    on_big_stack(move || {
        let pkg = Package::from_bytes(&bytes).map_err(|e| format!("cannot open: {e}"))?;
        load_package("hostile.mcdx", &pkg)
    })
}

fn assert_error(result: Result<ViewModel, String>, what: &str) {
    match result {
        Err(message) => assert!(!message.is_empty(), "{what}: empty message"),
        Ok(_) => panic!("{what}: expected an error"),
    }
}

#[test]
fn zero_bytes_give_an_error() {
    assert_error(load_bytes(Vec::new()), "0 bytes");
}

#[cfg(unix)]
#[test]
fn zero_byte_file_on_disk_gives_an_error() {
    assert_error(load_file(Path::new("/dev/null")), "0-byte file");
}

#[test]
fn truncated_archive_gives_an_error() {
    require_samples!();
    let bytes = std::fs::read(sample("Mechanik_1_massen_ziehen.mcdx")).expect("sample");
    assert_error(load_bytes(bytes[..bytes.len() / 2].to_vec()), "truncated");
}

#[test]
fn non_zip_gives_an_error() {
    assert_error(load_bytes(b"not a zip at all".to_vec()), "non-zip");
}

#[test]
fn directory_gives_an_error() {
    require_samples!();
    assert_error(load_file(&examples_dir()), "directory");
}

#[test]
fn missing_file_gives_an_error() {
    require_samples!();
    assert_error(
        load_file(&examples_dir().join("no-such-file.mcdx")),
        "missing file",
    );
}

#[test]
fn archive_without_a_worksheet_gives_an_error() {
    assert_error(
        load_bytes(zip_of(&[("docProps/app.xml", b"<x/>")])),
        "no worksheet part",
    );
}

#[test]
fn worksheet_with_the_wrong_root_gives_an_error() {
    let bytes = zip_of(&[(WORKSHEET, b"<notAWorksheet/>")]);
    assert_error(load_bytes(bytes), "wrong root");
}

#[test]
fn malformed_worksheet_xml_gives_an_error() {
    let bytes = zip_of(&[(WORKSHEET, b"<worksheet><region></worksheet>")]);
    assert_error(load_bytes(bytes), "malformed xml");
}

#[test]
fn worksheet_over_the_64_mib_cap_gives_an_error() {
    // One byte over the cap for a main part: the archive is refused, not read in full.
    let over = vec![b' '; 64 * 1024 * 1024 + 1];
    assert_error(
        load_bytes(zip_of(&[(WORKSHEET, over.as_slice())])),
        "64 MiB worksheet",
    );
}

#[test]
fn damaged_stored_results_give_an_error_with_a_message() {
    require_samples!();
    let ws = std::fs::read(sample("Mechanik_1_massen_ziehen.mcdx")).expect("sample");
    let pkg = Package::from_bytes(&ws).expect("sample opens");
    let worksheet = pkg.worksheet.clone();
    let bytes = zip_of(&[
        (WORKSHEET, worksheet.as_bytes()),
        (mathcraft_import::package::RESULT, b"<not-results"),
    ]);
    assert_error(load_bytes(bytes), "damaged result.xml");
}

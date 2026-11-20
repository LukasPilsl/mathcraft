//! Headless round trip: open a sample, edit it through the session, save it with the writer to a
//! temporary file, open that file again, and compare the regions and the re-evaluated results.
//! Nothing here opens a window.

use std::fs;
use std::path::PathBuf;

use mathcraft_import::evaluate::on_big_stack;

use crate::edit::document::Document;
use crate::edit::save::{DocumentWriter, McdxWriter};
use crate::edit::session::{EditKey, EditSession};
use crate::load::{evaluate_document, open_bytes, open_file};
use crate::testdata::sample;
use crate::viewmodel::{Body, ViewModel};

/// A path in the system temporary folder, unique to this process and test.
fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mathcraft-roundtrip-{}-{name}.mcdx",
        std::process::id()
    ))
}

/// Types `text` into the session and commits it with Enter.
fn type_and_commit(session: &mut EditSession, at: [f64; 2], text: &str) {
    session.click(at, None);
    for c in text.chars() {
        session.type_char(c);
    }
    session.key(EditKey::Enter);
}

/// The computed text after `=` of the region `id`, from a fresh evaluation of the document.
fn computed_of(doc: &Document, id: u32) -> Option<String> {
    let owned = doc.clone();
    let vm: ViewModel = on_big_stack(move || evaluate_document(&owned));
    vm.items.iter().find_map(|item| match &item.body {
        Body::Math {
            result: Some(line), ..
        } if item.region_id == id => Some(line.computed.clone()),
        _ => None,
    })
}

/// The id of the top-level region with the last id in the document.
fn last_id(doc: &Document) -> Option<u32> {
    doc.regions().last().map(|r| r.id)
}

#[test]
fn an_unedited_sample_saves_and_reopens_with_the_same_regions() {
    require_samples!();
    let loaded = open_file(&sample("Mechanik_1_massen_ziehen.mcdx"), None).expect("open sample");
    let out = temp_file("unedited");
    McdxWriter
        .write(&loaded.doc, &out)
        .expect("save unedited sample");
    let again = open_file(&out, None).expect("reopen");
    assert_eq!(again.doc.regions(), loaded.doc.regions());
    let _ = fs::remove_file(&out);
}

#[test]
fn edited_sample_saves_and_reopens_with_the_new_regions_and_results() {
    require_samples!();
    let loaded = open_file(&sample("Mechanik_1_massen_ziehen.mcdx"), None).expect("open sample");
    let before = loaded.doc.regions().len();
    let mut session = EditSession::new(loaded.doc);
    type_and_commit(&mut session, [20.0, 700.0], "zz:=3");
    type_and_commit(&mut session, [20.0, 740.0], "zz*2=");
    let doc = session.document();
    assert_eq!(doc.regions().len(), before + 2, "two regions were added");
    let result_id = last_id(doc).expect("the evaluation region");

    let out = temp_file("edited");
    McdxWriter.write(doc, &out).expect("save edited sample");
    let again = open_file(&out, None).expect("reopen");

    assert_eq!(again.doc.regions(), doc.regions());
    assert_eq!(computed_of(&again.doc, result_id).as_deref(), Some("6"));
    let _ = fs::remove_file(&out);
}

#[test]
fn a_deleted_region_stays_deleted_after_a_save() {
    require_samples!();
    let loaded = open_file(&sample("Mechanik_1_massen_ziehen.mcdx"), None).expect("open sample");
    let before = loaded.doc.regions().len();
    let first = loaded.doc.regions()[0].id;
    let mut session = EditSession::new(loaded.doc);
    session.click([0.0, 0.0], Some(first));
    session.key(EditKey::Delete);
    assert_eq!(session.document().regions().len(), before - 1);
    let out = temp_file("deleted");
    McdxWriter.write(session.document(), &out).expect("save");
    let again = open_file(&out, None).expect("reopen");
    assert_eq!(again.doc.regions().len(), before - 1);
    assert!(again.doc.region(first).is_none());
    let _ = fs::remove_file(&out);
}

#[test]
fn page_and_calculation_settings_survive_a_save() {
    let mut session = EditSession::new(Document::untitled("Settings.mcdx"));
    let mut page = session.document().page().clone();
    page.paper_code = "Letter".to_owned();
    page.orientation = "Landscape".to_owned();
    page.margin_type = "Narrow".to_owned();
    page.grid_size = "Fine".to_owned();
    session.set_page(page.clone());
    let mut settings = session.document().settings().clone();
    settings.array_origin = Some(1);
    settings.convergence_tolerance = Some(1e-5);
    settings.constraint_tolerance = Some(1e-2);
    settings.unit_system = Some("us".to_owned());
    session.set_settings(settings.clone());
    let out = temp_file("settings");
    McdxWriter
        .write(session.document(), &out)
        .expect("save the settings");
    let again = open_file(&out, None).expect("reopen");
    assert_eq!(again.doc.page(), &page);
    assert_eq!(again.doc.settings().array_origin, Some(1));
    assert_eq!(again.doc.settings().convergence_tolerance, Some(1e-5));
    assert_eq!(again.doc.settings().constraint_tolerance, Some(1e-2));
    assert_eq!(again.doc.settings().unit_system.as_deref(), Some("us"));
    let _ = fs::remove_file(&out);
}

#[test]
fn a_new_worksheet_saves_and_reopens() {
    let mut session = EditSession::new(Document::untitled("Untitled.mcdx"));
    type_and_commit(&mut session, [20.0, 40.0], "a:=2");
    // The `=` must leave the exponent first: the Right arrow moves the caret out of it.
    session.click([20.0, 80.0], None);
    for c in "a^2".chars() {
        session.type_char(c);
    }
    session.key(EditKey::Right);
    for c in "=".chars() {
        session.type_char(c);
    }
    session.key(EditKey::Enter);
    let doc = session.document();
    let result_id = last_id(doc).expect("evaluation region");
    let out = temp_file("untitled");
    McdxWriter.write(doc, &out).expect("save new worksheet");
    let again = open_file(&out, None).expect("reopen");
    assert_eq!(again.doc.regions(), doc.regions());
    assert_eq!(computed_of(&again.doc, result_id).as_deref(), Some("4"));
    let _ = fs::remove_file(&out);
}

#[test]
fn bytes_that_are_not_a_package_give_an_error_not_a_panic() {
    assert!(open_bytes("odd.mcdx", b"not a zip file".to_vec()).is_err());
    assert!(open_bytes("empty.mcdx", Vec::new()).is_err());
}

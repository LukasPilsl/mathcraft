//! Session tests for the page header and footer (typing, fields, the frame in the margin) and for
//! page breaks (inserting and deleting them, and undoing).

use std::collections::BTreeMap;

use mathcraft_import::regions::{ImportReport, RegionKind, RegionModel, Settings, Worksheet};
use mathcraft_import::results::ResultTable;
use mathcraft_import::rich_text::TextBody;

use super::document::Document;
use super::session::{EditKey, EditSession};

fn session_with_footer(footer: Vec<RegionModel>) -> EditSession {
    let ws = Worksheet {
        regions: Vec::new(),
        settings: Settings::default(),
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer,
    };
    let doc = Document::from_worksheet(
        "test",
        ws,
        ResultTable {
            by_id: BTreeMap::new(),
        },
    );
    EditSession::new(doc)
}

fn footer_region(id: u32, text: &str) -> RegionModel {
    RegionModel {
        id,
        pos: Some((10.0, 800.0)),
        size: Some((200.0, 16.0)),
        result_ref: None,
        kind: RegionKind::Text(TextBody::from_plain(text)),
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
}

#[test]
fn typing_in_the_header_makes_its_region_on_commit() {
    let mut s = session_with_footer(Vec::new());
    assert!(s.start_furniture(false));
    assert!(s.has_draft());
    for c in "Seite ".chars() {
        s.type_char(c);
    }
    s.insert_field("{page}");
    assert_eq!(s.commit(), Ok(true));
    assert!(!s.has_draft());
    let header = s.document().header();
    assert_eq!(header.len(), 1);
    assert_eq!(s.document().furniture_text(false).as_str(), "Seite {page}");
}

#[test]
fn an_empty_header_left_untouched_makes_no_region() {
    let mut s = session_with_footer(Vec::new());
    assert!(s.start_furniture(false));
    assert_eq!(s.commit(), Ok(false));
    assert!(s.document().header().is_empty());
}

#[test]
fn removing_the_page_number_field_keeps_the_other_text() {
    let mut s = session_with_footer(vec![footer_region(1, "Beam {page} of {pages} {date}")]);
    s.remove_field("page");
    assert_eq!(
        s.document().furniture_text(true).as_str(),
        "Beam  of  {date}"
    );
    s.remove_field("date");
    assert_eq!(s.document().furniture_text(true).as_str(), "Beam  of  ");
    s.remove_field("date");
    assert!(s.message().is_some(), "nothing left to remove");
}

#[test]
fn a_field_outside_a_header_or_footer_draft_is_refused() {
    let mut s = session_with_footer(Vec::new());
    s.insert_field("{page}");
    assert!(!s.has_draft());
    assert!(s.message().is_some());
    assert!(s.document().header().is_empty());
}

#[test]
fn editing_the_footer_keeps_its_region_and_position() {
    let mut s = session_with_footer(vec![footer_region(7, "old")]);
    assert!(s.start_furniture(true));
    for _ in 0..3 {
        s.key(EditKey::Backspace);
    }
    for c in "new".chars() {
        s.type_char(c);
    }
    assert_eq!(s.commit(), Ok(true));
    let footer = s.document().footer();
    assert_eq!(footer.len(), 1);
    assert_eq!(footer[0].id, 7);
    assert_eq!(footer[0].pos, Some((10.0, 800.0)));
    assert_eq!(s.document().furniture_text(true).as_str(), "new");
}

#[test]
fn a_page_break_is_inserted_at_the_insertion_point_and_selected() {
    let mut s = session_with_footer(Vec::new());
    s.click([10.0, 300.0], None);
    s.insert_page_break();
    let breaks: Vec<_> = s
        .document()
        .regions()
        .iter()
        .filter(|r| matches!(r.kind, RegionKind::PageBreak))
        .collect();
    assert_eq!(breaks.len(), 1);
    assert_eq!(breaks[0].pos, Some((10.0, 300.0)));
    assert_eq!(s.selection(), Some(breaks[0].id));
}

#[test]
fn a_page_break_is_removed_near_the_insertion_point_and_undo_restores_it() {
    let mut s = session_with_footer(Vec::new());
    s.click([10.0, 300.0], None);
    s.insert_page_break();
    s.click([12.0, 305.0], None);
    s.delete_page_break();
    assert!(s.document().regions().is_empty());
    assert!(s.undo());
    assert_eq!(s.document().regions().len(), 1);
}

#[test]
fn no_page_break_near_the_point_is_not_removed() {
    let mut s = session_with_footer(Vec::new());
    s.click([10.0, 300.0], None);
    s.insert_page_break();
    s.click([10.0, 600.0], None);
    s.delete_page_break();
    assert_eq!(s.document().regions().len(), 1);
    assert!(s.message().is_some());
}

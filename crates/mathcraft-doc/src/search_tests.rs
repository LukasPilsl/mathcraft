//! Tests of find and replace: the matching rules, the locations, replacement validity, undo, the
//! wrap-around of steps and searches over several pages.

use crate::doc::{Doc, DocError, Placement};
use crate::plot::PlotSpec;
use crate::search::{
    find, find_ranges, replace_all, replace_one, searchable_text, splice, step_index, MatchKind,
    SearchOptions,
};

fn all() -> SearchOptions {
    SearchOptions::default()
}

fn text_of(doc: &Doc, id: u32) -> String {
    let region = doc.document().region(id).expect("region");
    searchable_text(region).expect("searchable").1
}

#[test]
fn case_is_ignored_unless_asked() {
    assert_eq!(find_ranges("Speed SPEED speed", "speed", all()).len(), 3);
    let exact = SearchOptions {
        match_case: true,
        ..all()
    };
    assert_eq!(
        find_ranges("Speed SPEED speed", "speed", exact),
        vec![12..17]
    );
}

#[test]
fn whole_word_rejects_parts_of_words() {
    let whole = SearchOptions {
        whole_word: true,
        ..all()
    };
    assert_eq!(
        find_ranges("v vv v_1 v1 (v)", "v", whole),
        vec![0..1, 13..14]
    );
    assert_eq!(find_ranges("v vv", "v", all()).len(), 3);
}

#[test]
fn unicode_positions_are_character_indices() {
    // The umlaut and the sharp s are one character each, not their byte lengths.
    let text = "Größe x größe";
    assert_eq!(find_ranges(text, "größe", all()), vec![0..5, 8..13]);
    let upper = find_ranges("ÄPFEL äpfel", "äpfel", all());
    assert_eq!(upper, vec![0..5, 6..11]);
    let exact = SearchOptions {
        match_case: true,
        ..all()
    };
    assert_eq!(find_ranges("ÄPFEL äpfel", "äpfel", exact), vec![6..11]);
}

#[test]
fn matches_do_not_overlap() {
    assert_eq!(find_ranges("aaaa", "aa", all()), vec![0..2, 2..4]);
    assert_eq!(find_ranges("aaa", "aa", all()), vec![0..2]);
    assert!(find_ranges("ab", "", all()).is_empty());
    assert!(find_ranges("ab", "abc", all()).is_empty());
}

#[test]
fn splice_replaces_by_character() {
    assert_eq!(splice("v:=len/dur", 3..6, "d"), "v:=d/dur");
    assert_eq!(splice("größe", 0..1, "G"), "Größe");
}

#[test]
fn step_index_wraps_at_both_ends() {
    assert_eq!(step_index(0, None, true), None);
    assert_eq!(step_index(3, None, true), Some(0));
    assert_eq!(step_index(3, None, false), Some(2));
    assert_eq!(step_index(3, Some(2), true), Some(0));
    assert_eq!(step_index(3, Some(0), false), Some(2));
    assert_eq!(step_index(3, Some(1), true), Some(2));
}

/// A sheet with math, text and a plot title, several pages apart.
fn sheet() -> (Doc, [u32; 3]) {
    let mut doc = Doc::new("search");
    let m = doc
        .insert_math("v:=len/dur", Placement::At([60.0, 80.0]))
        .unwrap();
    let t = doc
        .insert_text("Speed v and V", Placement::At([60.0, 1200.0]))
        .unwrap();
    let spec = PlotSpec {
        x: "x".into(),
        y: "x^2".into(),
        x_min: "0".into(),
        x_max: "2".into(),
        title: "Velocity plot".into(),
        ..PlotSpec::default()
    };
    let p = doc
        .insert_plot(&spec, Placement::At([60.0, 2400.0]))
        .unwrap();
    (doc, [m, t, p])
}

#[test]
fn find_covers_math_text_and_plot_titles_in_document_order() {
    let (doc, [m, t, p]) = sheet();
    let found = find(doc.document(), "v", all());
    let ids: Vec<(u32, MatchKind)> = found.iter().map(|f| (f.region_id, f.kind)).collect();
    assert_eq!(
        ids,
        vec![
            (m, MatchKind::Math),
            (t, MatchKind::Text),
            (t, MatchKind::Text),
            (p, MatchKind::PlotTitle),
        ]
    );
    // Case-sensitive: the capital V of the text is left out.
    let exact = SearchOptions {
        match_case: true,
        ..all()
    };
    // The capital V of the text and the V of the plot title.
    assert_eq!(find(doc.document(), "V", exact).len(), 2);
}

#[test]
fn options_switch_off_math_or_text() {
    let (doc, _) = sheet();
    let math_only = SearchOptions {
        in_text: false,
        ..all()
    };
    let found = find(doc.document(), "v", math_only);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, MatchKind::Math);
    let text_only = SearchOptions {
        in_math: false,
        ..all()
    };
    assert_eq!(find(doc.document(), "v", text_only).len(), 3);
}

#[test]
fn empty_query_finds_nothing() {
    let (doc, _) = sheet();
    assert!(find(doc.document(), "", all()).is_empty());
}

#[test]
fn replace_one_math_keeps_the_expression_valid() {
    let (mut doc, [m, _, _]) = sheet();
    let hit = find(doc.document(), "len", all())[0].clone();
    assert_eq!(hit.region_id, m);
    replace_one(&mut doc, &hit, "d").unwrap();
    assert_eq!(text_of(&doc, m), "v:=d/dur");
}

#[test]
fn replace_one_refuses_text_that_does_not_read_and_keeps_it() {
    let (mut doc, [m, _, _]) = sheet();
    let before = text_of(&doc, m);
    let steps = doc.steps_executed();
    let hit = find(doc.document(), "len", all())[0].clone();
    let err = replace_one(&mut doc, &hit, "(").unwrap_err();
    assert!(matches!(err, DocError::Invalid(_)), "{err:?}");
    assert_eq!(text_of(&doc, m), before);
    assert_eq!(doc.steps_executed(), steps, "a refused replace is no step");
}

#[test]
fn replace_one_is_one_undo_step() {
    let (mut doc, [m, _, _]) = sheet();
    let hit = find(doc.document(), "dur", all())[0].clone();
    replace_one(&mut doc, &hit, "t").unwrap();
    assert_eq!(text_of(&doc, m), "v:=len/t");
    assert!(doc.undo().unwrap());
    assert_eq!(text_of(&doc, m), "v:=len/dur");
}

#[test]
fn replace_text_and_plot_title() {
    let (mut doc, [_, t, p]) = sheet();
    let hit = find(doc.document(), "Speed", all())[0].clone();
    replace_one(&mut doc, &hit, "Rate").unwrap();
    assert_eq!(text_of(&doc, t), "Rate v and V");
    let hit = find(doc.document(), "Velocity", all())[0].clone();
    assert_eq!(hit.kind, MatchKind::PlotTitle);
    replace_one(&mut doc, &hit, "Speed").unwrap();
    let title = searchable_text(doc.document().region(p).unwrap()).unwrap();
    assert_eq!(title, (MatchKind::PlotTitle, "Speed plot".to_string()));
}

#[test]
fn replace_all_is_one_undo_step() {
    let (mut doc, [m, t, _]) = sheet();
    let report = replace_all(&mut doc, "v", "w", all(), "AI: test").unwrap();
    assert_eq!(report.replaced, 4);
    assert!(report.refused.is_empty());
    assert_eq!(text_of(&doc, m), "w:=len/dur");
    assert_eq!(text_of(&doc, t), "Speed w and w");
    assert!(doc.undo().unwrap());
    assert_eq!(text_of(&doc, m), "v:=len/dur");
    assert_eq!(text_of(&doc, t), "Speed v and V");
}

#[test]
fn replace_all_reports_refused_math_and_keeps_it() {
    let mut doc = Doc::new("refuse");
    let a = doc.insert_math("x+x", Placement::Auto).unwrap();
    let b = doc.insert_text("x and y", Placement::Auto).unwrap();
    // Removing x from the math leaves "+", which does not read.
    let report = replace_all(&mut doc, "x", "", all(), "AI: test").unwrap();
    assert_eq!(report.refused, vec![a]);
    assert_eq!(report.replaced, 1);
    assert_eq!(text_of(&doc, a), "x+x");
    assert_eq!(text_of(&doc, b), " and y");
}

#[test]
fn replace_all_with_no_match_changes_nothing() {
    let (mut doc, _) = sheet();
    let steps = doc.steps_executed();
    let report = replace_all(&mut doc, "zzz", "q", all(), "AI: test").unwrap();
    assert_eq!(report.replaced, 0);
    assert_eq!(doc.steps_executed(), steps);
}

#[test]
fn matches_on_several_pages_are_all_found_and_stepped() {
    let mut doc = Doc::new("pages");
    let mut ids = Vec::new();
    for page in 0..4 {
        let at = [60.0, 800.0 * page as f64 + 100.0];
        ids.push(doc.insert_text("note", Placement::At(at)).unwrap());
    }
    let found = find(doc.document(), "note", all());
    let order: Vec<u32> = found.iter().map(|f| f.region_id).collect();
    assert_eq!(order, ids);
    // A forward walk from the last match wraps to the first.
    assert_eq!(step_index(found.len(), Some(3), true), Some(0));
}

#[test]
fn replace_all_of_500k_characters_is_linear() {
    let mut doc = Doc::new("big");
    let id = doc
        .insert_text(&"é".repeat(500_000), Placement::Auto)
        .unwrap();
    let start = std::time::Instant::now();
    let report = replace_all(&mut doc, "é", "ee", all(), "Replace all").unwrap();
    assert_eq!(report.replaced, 500_000);
    assert!(start.elapsed().as_secs() < 2, "{:?}", start.elapsed());
    assert_eq!(text_of(&doc, id).chars().count(), 1_000_000);
}

#[test]
fn find_stops_at_the_match_cap() {
    let mut doc = Doc::new("cap");
    doc.insert_text(
        &"a".repeat(crate::search::MAX_MATCHES + 50),
        Placement::Auto,
    )
    .unwrap();
    let scan = crate::search::scan(doc.document(), "a", all(), 100, 10);
    assert_eq!(
        (scan.matches.len(), scan.texts.len(), scan.truncated),
        (100, 10, true)
    );
    assert_eq!(
        find(doc.document(), "a", all()).len(),
        crate::search::MAX_MATCHES
    );
}

#[test]
fn replace_all_keeps_styles_between_matches() {
    use mathcraft_import::regions::RegionKind;
    let mut doc = Doc::new("rich");
    let id = doc.insert_text("a x b x c", Placement::Auto).unwrap();
    let mut body = match &doc.region(id).unwrap().kind {
        RegionKind::Text(b) => b.rich().clone(),
        _ => panic!("text"),
    };
    body.lines[0].runs.clear();
    let mut bold = body.base.clone();
    bold.bold = true;
    let plain = body.base.clone();
    for (t, st) in [("a ", &plain), ("x b x", &bold), (" c", &plain)] {
        body.lines[0].runs.push(mathcraft_import::rich_text::Run {
            text: t.into(),
            style: st.clone(),
        });
    }
    doc.set_text_body(id, mathcraft_import::rich_text::TextBody::from_rich(body))
        .unwrap();
    let r = replace_all(&mut doc, "x", "yy", all(), "Replace all").unwrap();
    assert_eq!(r.replaced, 2);
    let RegionKind::Text(b) = &doc.region(id).unwrap().kind else {
        panic!("text")
    };
    let runs: Vec<(&str, bool)> = b.rich().lines[0]
        .runs
        .iter()
        .map(|r| (r.text.as_str(), r.style.bold))
        .collect();
    assert_eq!(runs, vec![("a ", false), ("yy b yy", true), (" c", false)]);
}

#[test]
fn a_failing_region_rolls_the_earlier_ones_back() {
    let mut doc = Doc::new("atomic");
    let a = doc.insert_text("x", Placement::Auto).unwrap();
    let b = doc.insert_text("x", Placement::Auto).unwrap();
    // A match range past the end of the region's text is a non-Invalid error in `apply`; here
    // the region vanishes between find and apply, which `replace_one` reports the same way.
    let m = crate::search::Match {
        region_id: b,
        kind: MatchKind::Text,
        range: 5..6,
    };
    assert!(replace_one(&mut doc, &m, "y").is_err());
    assert_eq!(text_of(&doc, a), "x");
    assert_eq!(text_of(&doc, b), "x");
}

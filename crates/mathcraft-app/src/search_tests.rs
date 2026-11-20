//! Tests of the status bar search glue: the match cache, and the session calls behind a step and
//! a replace (a text hit is selected in its text, a refused replace changes nothing, undo).

use std::cell::Cell;

use mathcraft_doc::{Doc, MatchKind, Placement, SearchMatch, SearchOptions as DocSearchOptions};

use crate::edit::session::EditSession;
use crate::statusbar::SearchState;

fn hit(region_id: u32, start: usize, end: usize) -> SearchMatch {
    SearchMatch {
        region_id,
        kind: MatchKind::Text,
        range: start..end,
    }
}

#[test]
fn refresh_is_made_only_when_query_options_or_revision_change() {
    let mut state = SearchState::default();
    state.find = "v".into();
    let calls = Cell::new(0);
    let find = |_: &str, _: DocSearchOptions| {
        calls.set(calls.get() + 1);
        vec![hit(1, 0, 1), hit(2, 0, 1), hit(3, 0, 1)]
    };
    state.refresh_with(7, find);
    assert_eq!(state.matches.len(), 3);
    assert_eq!(state.current, None);
    state.current = Some(2);
    state.refresh_with(7, find);
    assert_eq!(
        calls.get(),
        1,
        "same query, options and revision: no new search"
    );
    // A change of the worksheet searches again, and the current match is kept (clamped).
    state.refresh_with(8, find);
    assert_eq!(calls.get(), 2);
    assert_eq!(state.current, Some(2));
    // A new query clears the current match.
    state.find = "w".into();
    state.refresh_with(8, find);
    assert_eq!(state.current, None);
    // Toggling an option searches again.
    state.options.match_case = true;
    state.refresh_with(8, find);
    assert_eq!(calls.get(), 4);
}

#[test]
fn an_empty_query_has_no_matches_and_no_search() {
    let mut state = SearchState::default();
    state.refresh_with(1, |_, _| panic!("no search for an empty query"));
    assert!(state.matches.is_empty());
}

/// A sheet with a math region and a text region holding a word.
fn session() -> (EditSession, u32, u32) {
    let mut doc = Doc::new("search");
    let m = doc.insert_math("v:=len/dur", Placement::Auto).unwrap();
    let t = doc.insert_text("speed v", Placement::Auto).unwrap();
    (EditSession::from_doc(doc, 0), m, t)
}

#[test]
fn a_text_hit_is_selected_in_its_text() {
    let (mut s, _, t) = session();
    let found = s.find_matches("v", Default::default());
    let text_hit = found
        .iter()
        .find(|m| m.kind == MatchKind::Text)
        .unwrap()
        .clone();
    assert!(s.reveal_match(t, Some(text_hit.range.clone())));
    let view = s.draft_view().expect("the text is open for typing");
    assert_eq!(view.target, Some(t));
    assert_eq!(view.selection, Some((6, 7)));
}

#[test]
fn a_math_hit_selects_the_region() {
    let (mut s, m, _) = session();
    assert!(s.reveal_match(m, None));
    assert_eq!(s.selection(), Some(m));
    assert!(s.draft_view().is_none());
}

#[test]
fn a_refused_replace_changes_nothing_and_says_why() {
    let (mut s, m, _) = session();
    let before = s.revision();
    let found = s.find_matches("len", Default::default());
    let hit = found[0].clone();
    assert_eq!(hit.region_id, m);
    let err = s.replace_match(&hit, "").unwrap_err();
    assert!(!err.is_empty());
    assert_eq!(s.revision(), before, "a refused replace is no change");
    assert!(s.message().is_some());
}

#[test]
fn a_replace_is_one_undo_step() {
    let (mut s, _, t) = session();
    let found = s.find_matches("speed", Default::default());
    s.replace_match(&found[0], "rate").unwrap();
    assert_eq!(
        s.find_matches("rate", Default::default()).len(),
        1,
        "the text changed"
    );
    assert!(s.undo());
    assert_eq!(s.find_matches("speed", Default::default()).len(), 1);
    assert!(s.find_matches("rate", Default::default()).is_empty());
    assert!(s.reveal_match(t, None));
}

//! Tests of the editing session on synthetic documents, and of the sample worksheets.

use std::collections::BTreeMap;

use mathcraft_core::expr::Expr;
use mathcraft_import::package::Package;
use mathcraft_import::regions::{ImportReport, RegionKind, RegionModel, Settings, Worksheet};
use mathcraft_import::results::ResultTable;

use super::*;
use crate::edit::math::{parse_math, parse_math_in, EditContext, MathEditor};
use crate::edit::session::unread_notice;
use crate::load::{evaluate_document, open_package};
use crate::testdata::{examples_dir, sample};
use crate::viewmodel::Body;

/// The form of a tree that its printed text can carry. Three differences are not printed and
/// are dropped from both sides before comparing:
/// - `SymEval::result`: the cached symbolic output, documented as never printed.
/// - `Parens`: the printer adds parentheses only where they are needed; the parser keeps the
///   ones it reads, so `(a-b)^2` reads back with a `Parens` node the stored tree may lack. The
///   two are equal in meaning.
/// - `Eval` over a `SymEval` with no unit override: prints as the bare `expr ->`, which reads
///   back as the `SymEval` (documented lossy case in `expr.rs`).
///   Literal forms are not normalized: `3/4` reads back as the `Div` it is stored as, and `0.75 m`
///   reads back as a `Scale` when `m` is a unit in the document (per-occurrence kind conflicts
///   are the commit fallback instead).
fn normalize_for_text(e: &mut Expr) {
    match e {
        Expr::Number(_) | Expr::Str(_) | Expr::Placeholder => {}
        Expr::Id(id) => {
            if let Some(sub) = &mut id.subscript {
                normalize_for_text(sub);
            }
        }
        Expr::Unary { operand, .. } => normalize_for_text(operand),
        Expr::Binary { lhs, rhs, .. } => {
            normalize_for_text(lhs);
            normalize_for_text(rhs);
        }
        Expr::Call { args, .. } => args.iter_mut().for_each(normalize_for_text),
        Expr::Range { from, step, to } => {
            normalize_for_text(from);
            if let Some(s) = step {
                normalize_for_text(s);
            }
            normalize_for_text(to);
        }
        Expr::Matrix { elements, .. } => elements.iter_mut().for_each(normalize_for_text),
        Expr::Index { base, indices } => {
            normalize_for_text(base);
            indices.iter_mut().for_each(normalize_for_text);
        }
        Expr::NthRoot { degree, radicand } => {
            if let Some(d) = degree {
                normalize_for_text(d);
            }
            normalize_for_text(radicand);
        }
        Expr::Parens(inner) => normalize_for_text(inner),
        Expr::Define { target, expr } | Expr::GlobalDefine { target, expr } => {
            normalize_for_text(target);
            normalize_for_text(expr);
        }
        Expr::Eval {
            expr,
            unit_override,
        } => {
            normalize_for_text(expr);
            if let Some(u) = unit_override {
                normalize_for_text(u);
            }
        }
        Expr::SymEval {
            expr,
            command,
            result,
        } => {
            normalize_for_text(expr);
            if let Some(c) = command {
                c.0.iter_mut().for_each(normalize_for_text);
            }
            *result = None;
        }
        Expr::Scale { value, unit } => {
            normalize_for_text(value);
            normalize_for_text(unit);
        }
        Expr::Program(_) => {}
        Expr::Accumulate { from, to, body, .. } => {
            normalize_for_text(from);
            normalize_for_text(to);
            normalize_for_text(body);
        }
        Expr::Fold { operand, .. } => normalize_for_text(operand),
        Expr::Integral {
            lower, upper, body, ..
        } => {
            normalize_for_text(lower);
            normalize_for_text(upper);
            normalize_for_text(body);
        }
        Expr::Derivative { order, body, .. } => {
            if let Some(n) = order {
                normalize_for_text(n);
            }
            normalize_for_text(body);
        }
        Expr::Gradient { body, .. } => normalize_for_text(body),
        Expr::Limit { target, body, .. } => {
            normalize_for_text(target);
            normalize_for_text(body);
        }
    }
    // Collapse the wrappers and literal forms after their contents are normalized.
    match e {
        Expr::Parens(inner) => {
            let taken = std::mem::replace(&mut **inner, Expr::Placeholder);
            *e = taken;
        }
        Expr::Eval {
            expr,
            unit_override: None,
        } if matches!(**expr, Expr::SymEval { .. }) => {
            let taken = std::mem::replace(&mut **expr, Expr::Placeholder);
            *e = taken;
        }
        _ => {}
    }
}

/// The value of an exact rational, as a float. `Display` prints `n/d`.
/// Editing a region without changing its text, then committing, must leave it unchanged when
/// its text does not read back. Used for every region the parser rejects.
fn commit_unedited(doc: &Document, id: u32) -> EditSession {
    let mut t = EditSession::new(doc.clone());
    t.click([0.0, 0.0], Some(id));
    type_text(&mut t, "x");
    t.key(EditKey::Backspace);
    t.key(EditKey::Enter);
    t
}

fn region(id: u32, pos: Option<(f64, f64)>, kind: RegionKind) -> RegionModel {
    RegionModel {
        id,
        pos,
        size: None,
        result_ref: None,
        kind,
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
}

fn math_region(id: u32, pos: (f64, f64), text: &str) -> RegionModel {
    region(
        id,
        Some(pos),
        RegionKind::Math(parse_math(text).expect("test text parses")),
    )
}

fn session_with(regions: Vec<RegionModel>) -> EditSession {
    let ws = Worksheet {
        regions,
        settings: Settings::default(),
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer: Vec::new(),
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

fn type_text(s: &mut EditSession, text: &str) {
    for c in text.chars() {
        s.type_char(c);
    }
}

fn ids(s: &EditSession) -> Vec<u32> {
    s.document().regions().iter().map(|r| r.id).collect()
}

#[test]
fn a_click_on_empty_page_places_the_insertion_point() {
    let mut s = session_with(vec![]);
    s.click([100.0, 120.0], None);
    assert_eq!(s.crosshair(), Some([100.0, 120.0]));
    assert_eq!(s.selection(), None);
}

#[test]
fn typing_at_the_insertion_point_creates_a_math_region_on_enter() {
    let mut s = session_with(vec![]);
    s.click([100.0, 120.0], None);
    type_text(&mut s, "2+3");
    assert!(s.draft_view().is_some(), "typing opens a draft");
    assert!(ids(&s).is_empty(), "nothing is committed while typing");
    s.key(EditKey::Enter);
    assert_eq!(ids(&s), vec![1]);
    let region = s.document().region(1).expect("region 1");
    assert_eq!(
        region.pos,
        Some((100.0, 120.0 - GRID + (GRID - 12.24) / 2.0))
    );
    assert_eq!(region.kind, RegionKind::Math(parse_math("2+3").unwrap()));
    assert_eq!(
        s.selection(),
        None,
        "Enter leaves the region: the next typing is a new one"
    );
    assert!(s.draft_view().is_some(), "a fresh draft is open below it");
    assert_eq!(s.revision(), 1);
}

#[test]
fn a_new_region_is_one_undo_step() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "x:=1/2");
    s.key(EditKey::Enter);
    assert!(s.undo());
    assert!(ids(&s).is_empty());
    assert!(s.redo());
    assert_eq!(ids(&s), vec![1]);
    assert_eq!(
        s.document().region(1).unwrap().kind,
        RegionKind::Math(parse_math("x:=1/2").unwrap())
    );
}

#[test]
fn an_empty_new_region_is_not_created() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    s.key(EditKey::Enter);
    assert!(ids(&s).is_empty());
    assert_eq!(s.revision(), 0);
}

#[test]
fn a_text_that_does_not_parse_keeps_the_draft_with_a_message() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "a$");
    s.key(EditKey::Enter);
    assert!(ids(&s).is_empty());
    assert!(s.draft_view().is_some(), "the draft is kept");
    assert!(s.message().unwrap_or("").contains("Cannot read"));
    // Correcting the draft lets it commit.
    s.key(EditKey::Backspace);
    s.key(EditKey::Enter);
    assert_eq!(ids(&s), vec![1]);
}

#[test]
fn escape_keeps_the_typed_math_region() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "2+3");
    s.key(EditKey::Escape);
    assert!(s.draft_view().is_none(), "Escape leaves the edit");
    assert_eq!(ids(&s), vec![1], "and keeps the content");
}

#[test]
fn escape_on_an_unreadable_draft_keeps_it_open_with_the_message() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "a$");
    s.key(EditKey::Escape);
    assert!(s.has_draft());
    assert!(ids(&s).is_empty());
}

#[test]
fn typing_equals_evaluates_the_region_at_once() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "2+3=");
    assert!(!s.has_draft(), "the = commits the region");
    assert_eq!(ids(&s), vec![1]);
    let RegionKind::Math(expr) = &s.document().region(1).unwrap().kind else {
        panic!("a math region");
    };
    assert!(expr.to_string().contains('='));
}

#[test]
fn equals_inside_a_definition_does_not_evaluate() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "a:=");
    assert!(
        s.has_draft(),
        "the := of a definition is typed, not evaluated"
    );
    assert!(ids(&s).is_empty());
}

#[test]
fn return_evaluates_the_region_too() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "2+3*4");
    s.key(EditKey::Enter);
    assert!(
        s.draft_view().is_none_or(|d| d.target.is_none()),
        "only the fresh empty draft of the next region is open"
    );
    assert_eq!(ids(&s), vec![1]);
}

#[test]
fn a_stale_parse_error_clears_once_the_text_reads_again() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "a$");
    s.key(EditKey::Enter);
    assert!(s.message().is_some());
    s.key(EditKey::Backspace);
    assert!(
        s.message().is_none(),
        "the text reads again, so the error goes"
    );
}

#[test]
fn a_double_click_selects_the_word_so_typing_replaces_it() {
    let mut s = session_with(vec![math_region(1, (108.0, 108.0), "12+34")]);
    s.edit_region_word([120.0, 115.0], Some(1));
    assert!(s.has_draft(), "the double click starts editing the region");
    let point = s.take_word_point().expect("the word point is pending");
    assert_eq!(point, [120.0, 115.0]);
    assert!(s.take_word_point().is_none(), "the point is taken once");
    // The canvas found the character under the point: the 2 of 12.
    s.draft_pointer(DraftPointer::Word(1));
    type_text(&mut s, "9");
    s.key(EditKey::Enter);
    let RegionKind::Math(expr) = &s.document().region(1).unwrap().kind else {
        panic!("a math region");
    };
    assert_eq!(expr.to_string(), "9+34");
}

#[test]
fn a_click_selects_a_region_and_nudge_moves_it_with_undo() {
    let mut s = session_with(vec![math_region(1, (10.0, 10.0), "x+1")]);
    s.click([12.0, 12.0], Some(1));
    assert_eq!(s.selection(), Some(1));
    s.nudge(5.0, 0.0);
    assert_eq!(s.document().region(1).unwrap().pos, Some((15.0, 10.0)));
    assert!(s.undo());
    assert_eq!(s.document().region(1).unwrap().pos, Some((10.0, 10.0)));
}

#[test]
fn arrow_keys_move_the_selected_region() {
    let mut s = session_with(vec![math_region(1, (10.0, 10.0), "x")]);
    s.click([12.0, 12.0], Some(1));
    s.key(EditKey::Down);
    assert_eq!(
        s.document().region(1).unwrap().pos,
        Some((10.0, 10.0 + NUDGE))
    );
}

#[test]
fn a_region_without_position_cannot_be_moved() {
    let mut r = math_region(1, (0.0, 0.0), "x");
    r.pos = None;
    let mut s = session_with(vec![r]);
    s.click([1.0, 1.0], Some(1));
    s.nudge(5.0, 0.0);
    assert_eq!(s.revision(), 0);
    assert!(s.message().is_some());
}

#[test]
fn delete_removes_the_selected_region_and_undo_brings_it_back_in_place() {
    let mut s = session_with(vec![
        math_region(1, (0.0, 0.0), "a"),
        math_region(2, (0.0, 50.0), "b"),
        math_region(3, (0.0, 100.0), "c"),
    ]);
    s.click([0.0, 52.0], Some(2));
    s.key(EditKey::Delete);
    assert_eq!(ids(&s), vec![1, 3]);
    assert_eq!(s.selection(), None);
    assert!(s.undo());
    assert_eq!(ids(&s), vec![1, 2, 3]);
}

#[test]
fn typing_on_a_selected_math_region_edits_it_from_its_end() {
    let mut r = math_region(1, (0.0, 0.0), "x+1");
    r.result_ref = Some(7);
    let original = r.clone();
    let mut s = session_with(vec![r]);
    s.click([1.0, 1.0], Some(1));
    type_text(&mut s, "*2");
    s.key(EditKey::Enter);
    let edited = s.document().region(1).unwrap().clone();
    assert_eq!(edited.kind, RegionKind::Math(parse_math("x+1*2").unwrap()));
    assert_eq!(
        edited.result_ref, None,
        "the old stored result no longer applies"
    );
    assert!(s.undo());
    assert_eq!(s.document().region(1), Some(&original));
}

#[test]
fn a_draft_on_a_selected_region_is_committed_by_a_click_elsewhere() {
    let mut s = session_with(vec![math_region(1, (0.0, 0.0), "x")]);
    s.click([1.0, 1.0], Some(1));
    type_text(&mut s, "+1");
    s.click([300.0, 300.0], None);
    assert_eq!(
        s.document().region(1).unwrap().kind,
        RegionKind::Math(parse_math("x+1").unwrap())
    );
    assert_eq!(s.crosshair(), Some([300.0, 300.0]));
}

#[test]
fn an_unchanged_edit_is_not_a_change() {
    let mut s = session_with(vec![math_region(1, (0.0, 0.0), "x")]);
    s.click([1.0, 1.0], Some(1));
    type_text(&mut s, "+1");
    s.key(EditKey::Backspace);
    s.key(EditKey::Backspace);
    s.key(EditKey::Enter);
    assert_eq!(s.revision(), 0);
}

#[test]
fn a_text_region_is_edited_as_text_and_escape_commits() {
    let mut s = session_with(vec![region(
        1,
        Some((0.0, 0.0)),
        RegionKind::Text("Hello".into()),
    )]);
    s.click([1.0, 1.0], Some(1));
    type_text(&mut s, "!");
    s.key(EditKey::Enter);
    type_text(&mut s, "2");
    s.key(EditKey::Escape);
    assert_eq!(
        s.document().region(1).unwrap().kind,
        RegionKind::Text("Hello!\n2".into())
    );
}

#[test]
fn a_new_text_region_starts_at_the_insertion_point() {
    let mut s = session_with(vec![]);
    s.click([40.0, 40.0], None);
    assert!(s.start_text());
    type_text(&mut s, "Hi");
    s.key(EditKey::Escape);
    let r = s.document().region(1).expect("text region");
    assert_eq!(r.pos, Some((40.0, 40.0 - GRID + (GRID - 12.24) / 2.0)));
    assert_eq!(r.kind, RegionKind::Text("Hi".into()));
}

#[test]
fn start_text_needs_an_insertion_point() {
    let mut s = session_with(vec![]);
    assert!(!s.start_text());
    assert!(s.message().is_some());
}

#[test]
fn solve_block_members_are_reported_read_only() {
    let mut s = session_with(vec![math_region(1, (0.0, 0.0), "x")]);
    s.click([1.0, 1.0], Some(999));
    assert_eq!(s.selection(), None);
    assert!(s.message().unwrap_or("").contains("read-only"));
    type_text(&mut s, "z");
    assert!(
        s.draft_view().is_none(),
        "typing with nothing selected starts no draft"
    );
}

#[test]
fn an_undone_step_is_dropped_when_a_new_edit_follows() {
    let mut s = session_with(vec![math_region(1, (0.0, 0.0), "x")]);
    s.click([1.0, 1.0], Some(1));
    s.nudge(3.0, 0.0);
    assert!(s.undo());
    assert!(s.can_redo());
    s.nudge(4.0, 0.0);
    assert!(!s.can_redo());
}

#[test]
fn the_draft_view_shows_the_caret_inside_a_fraction() {
    let mut s = session_with(vec![]);
    s.click([0.0, 0.0], None);
    type_text(&mut s, "1/");
    let view = s.draft_view().expect("draft");
    assert!(view.math);
    assert_eq!(view.text, "1/_");
    assert_eq!(view.caret, 2);
}

#[test]
fn the_draft_view_reports_a_selected_range_in_a_math_draft() {
    let mut s = session_with(vec![]);
    s.click([0.0, 0.0], None);
    type_text(&mut s, "ab+cd");
    s.key(EditKey::SelectLeft);
    s.key(EditKey::SelectLeft);
    let view = s.draft_view().expect("draft");
    assert_eq!(view.text, "ab+cd");
    assert_eq!(view.selection, Some((3, 5)));
    assert_eq!(view.caret, 3);
}

#[test]
fn the_draft_view_reports_a_selected_range_in_a_text_draft() {
    let mut s = session_with(vec![]);
    s.click([0.0, 0.0], None);
    assert!(s.start_text());
    type_text(&mut s, "Hello");
    s.key(EditKey::SelectLeft);
    s.key(EditKey::SelectLeft);
    let view = s.draft_view().expect("draft");
    assert!(!view.math);
    assert_eq!(view.selection, Some((3, 5)));
    s.key(EditKey::Left);
    assert_eq!(s.draft_view().expect("draft").selection, None);
}

#[test]
fn backspace_and_delete_with_nothing_typed_delete_the_selected_region() {
    let mut s = session_with(vec![math_region(1, (0.0, 0.0), "x")]);
    s.click([1.0, 1.0], Some(1));
    s.key(EditKey::Backspace);
    assert!(ids(&s).is_empty());
    assert!(s.undo());
    s.click([1.0, 1.0], Some(1));
    s.key(EditKey::Delete);
    assert!(ids(&s).is_empty());
}

#[test]
fn an_edited_worksheet_is_evaluated_with_its_new_result() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    type_text(&mut s, "2+3=");
    s.key(EditKey::Enter);
    let vm = on_big_stack_eval(s.document().clone());
    let item = vm.items.iter().find(|i| i.region_id == 1).expect("item 1");
    match &item.body {
        Body::Math {
            result: Some(line), ..
        } => assert!(line.computed.contains('5'), "computed {}", line.computed),
        other => panic!("expected a math body with a result, got {other:?}"),
    }
}

fn on_big_stack_eval(doc: Document) -> crate::viewmodel::ViewModel {
    mathcraft_import::evaluate::on_big_stack(move || evaluate_document(&doc))
}

/// Every sample opens as a document, and one move followed by undo leaves it exactly as it was.
#[test]
fn samples_open_for_editing_and_an_edit_undoes_exactly() {
    require_samples!();
    let dir = examples_dir();
    let mut failures: Vec<String> = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("examples directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    names.sort();
    assert!(names.len() >= 20, "found {} samples", names.len());
    for path in names {
        let pkg = Package::open(&path).expect("package");
        let loaded = open_package("sample", &pkg).expect("sample loads");
        let before = loaded.doc.clone();
        let mut s = EditSession::new(loaded.doc);
        let movable = s
            .document()
            .regions()
            .iter()
            .find(|r| r.pos.is_some())
            .map(|r| r.id);
        if let Some(first) = movable {
            s.click([0.0, 0.0], Some(first));
            s.nudge(1.0, 1.0);
            assert!(s.undo(), "{path:?}");
        }
        assert_eq!(
            s.document().regions(),
            before.regions(),
            "{path:?} differs after undo"
        );
        // Every math region survives opening as its printed text and reading that text back. A
        // region that does not read back (a name used as both unit and variable, or a call head
        // of both kinds) must instead stay unchanged when committed unedited.
        let ctx = EditContext::for_regions(before.regions());
        let mut fallback = 0usize;
        let mut regions = 0usize;
        for r in before.regions() {
            if let RegionKind::Math(e) = &r.kind {
                regions += 1;
                let text = MathEditor::from_text(&e.to_string()).to_text();
                let mut expected = e.clone();
                normalize_for_text(&mut expected);
                let round_trips = match parse_math_in(&text, &ctx) {
                    Ok(mut read) => {
                        normalize_for_text(&mut read);
                        read == expected
                    }
                    Err(_) => false,
                };
                if round_trips {
                    continue;
                }
                // Not carried by its text (the kinds of a name used as both unit and variable,
                // or of a call head, are per occurrence and the text has none). Such a region
                // must be kept, unchanged, when committed unedited.
                fallback += 1;
                let t = commit_unedited(&before, r.id);
                if t.document().region(r.id).map(|x| &x.kind) != Some(&r.kind)
                    || t.message() != Some(unread_notice())
                {
                    failures.push(format!(
                        "{path:?} region {} text {text:?}: not text-equal and not kept on commit",
                        r.id
                    ));
                }
            }
        }
        println!(
            "{}: {regions} math regions, {fallback} via the commit fallback",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
    }
    assert!(
        failures.is_empty(),
        "{} math regions do not read back:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // The sample path helper points at a real file.
    assert!(sample("Mechanik_1_massen_ziehen.mcdx").exists());
}

#[test]
fn an_exponent_region_edited_and_undone_to_the_same_text_is_not_changed() {
    let mut s = session_with(vec![math_region(1, (0.0, 0.0), "1e-5")]);
    s.click([1.0, 1.0], Some(1));
    type_text(&mut s, "x");
    s.key(EditKey::Backspace);
    s.key(EditKey::Enter);
    let region = s.document().region(1).expect("region 1");
    assert_eq!(
        region.kind,
        RegionKind::Math(parse_math("1e-5").expect("parses"))
    );
    assert_eq!(s.revision(), 0);
}

#[test]
fn an_unreadable_unedited_expression_is_kept_and_a_notice_is_shown() {
    // Printed as `v:=x (2)`: a name followed by a group is not a unit, so the parser rejects it.
    // No printed form of a well-formed tree does this, so the stored tree is a `Scale` whose unit
    // is a number (outside the importer contract). Committing it unedited must not replace it.
    let stored = Expr::Define {
        target: Box::new(Expr::var("v")),
        expr: Box::new(Expr::Scale {
            value: Box::new(Expr::var("x")),
            unit: Box::new(Expr::num(2.0)),
        }),
    };
    let ctx = EditContext::for_regions(&[region(1, None, RegionKind::Math(stored.clone()))]);
    assert!(
        parse_math_in("v:=x (2)", &ctx).is_err(),
        "the gap this test relies on"
    );
    let mut s = session_with(vec![region(
        1,
        Some((0.0, 0.0)),
        RegionKind::Math(stored.clone()),
    )]);
    s.click([1.0, 1.0], Some(1));
    type_text(&mut s, "x");
    s.key(EditKey::Backspace);
    s.key(EditKey::Enter);
    assert_eq!(
        s.document().region(1).expect("region 1").kind,
        RegionKind::Math(stored)
    );
    assert_eq!(s.revision(), 0);
    assert_eq!(s.message(), Some(unread_notice()));
}

#[test]
fn expressions_from_regions_print_and_read_back() {
    // An existing region is opened as its printed text; reading it back gives the same tree.
    let e = parse_math("y:=sqrt(x)+2*a").expect("parses");
    let text = e.to_string();
    assert_eq!(parse_math(&text).expect("reads back"), e);
}

/// A text region being typed: `text` typed at the insertion point, then `range` selected.
fn text_draft_with_selection(text: &str, range: (usize, usize)) -> EditSession {
    let mut s = session_with(vec![]);
    s.click([0.0, 0.0], None);
    assert!(s.start_text());
    type_text(&mut s, text);
    s.draft_pointer(DraftPointer::Press(range.0));
    s.draft_pointer(DraftPointer::Drag(range.1));
    s
}

#[test]
fn bold_on_a_selection_of_the_text_draft_is_one_undoable_edit() {
    let mut s = text_draft_with_selection("Hello world", (6, 11));
    assert_eq!(s.selected_text_styles().len(), 1);
    assert!(s.format_text_selection(&StyleChange::Bold(true)));
    assert!(s.selected_text_styles().iter().all(|st| st.bold));
    s.key(EditKey::Escape);
    let RegionKind::Text(body) = &s.document().region(1).expect("text region").kind else {
        panic!("a text region")
    };
    assert_eq!(body.as_str(), "Hello world");
    assert!(body.rich().lines[0]
        .runs
        .iter()
        .any(|run| run.style.bold && run.text == "world"));
    assert!(s.undo());
    assert!(s.document().region(1).is_none(), "the new region is undone");
}

#[test]
fn formats_need_a_selection_but_alignment_uses_the_caret() {
    let mut s = text_draft_with_selection("abc", (1, 1));
    assert!(!s.format_text_selection(&StyleChange::Italic(true)));
    assert!(s.align_text(Align::Center));
    assert!(s.draft_view().is_some());
}

#[test]
fn a_size_and_a_font_apply_to_the_selected_characters_only() {
    let mut s = text_draft_with_selection("abcd", (0, 2));
    assert!(s.format_text_selection(&StyleChange::Size(20.0)));
    assert!(s.format_text_selection(&StyleChange::Family("Arial")));
    let styles = s.selected_text_styles();
    assert!(styles
        .iter()
        .all(|st| st.size == 20.0 && st.family == "Arial"));
    s.key(EditKey::Escape);
    let RegionKind::Text(body) = &s.document().region(1).expect("text region").kind else {
        panic!("a text region")
    };
    let runs = &body.rich().lines[0].runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].style.size, RunStyle::default().size);
}

#[test]
fn a_link_replaces_the_selection_and_is_display_only() {
    let mut s = text_draft_with_selection("see here", (4, 8));
    assert!(s.insert_link("there", "https://example.org/x"));
    s.key(EditKey::Escape);
    let RegionKind::Text(body) = &s.document().region(1).expect("text region").kind else {
        panic!("a text region")
    };
    assert_eq!(body.as_str(), "see there");
    let linked: Vec<_> = body.rich().lines[0]
        .runs
        .iter()
        .filter(|run| run.style.link.is_some())
        .collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].text, "there");
    assert_eq!(
        linked[0].style.link.as_deref(),
        Some("https://example.org/x")
    );
}

#[test]
fn a_link_without_address_is_plain_text_and_blank_text_is_refused() {
    let mut s = text_draft_with_selection("ab", (0, 0));
    assert!(!s.insert_link("   ", "x"), "a blank text types nothing");
    assert!(s.insert_link("cd", ""));
    s.key(EditKey::Escape);
    let RegionKind::Text(body) = &s.document().region(1).expect("text region").kind else {
        panic!("a text region")
    };
    assert_eq!(body.as_str(), "cdab");
    assert!(body.rich().lines[0]
        .runs
        .iter()
        .all(|r| r.style.link.is_none()));
}

#[test]
fn bold_on_an_existing_text_region_survives_commit_without_text_change() {
    // An existing text region, opened for editing, with "world" selected and bolded. Nothing else
    // changes, so the plain text is equal and only the formatting differs.
    let text = region(
        1,
        Some((10.0, 10.0)),
        RegionKind::Text(mathcraft_import::rich_text::TextBody::from_plain(
            "Hello world",
        )),
    );
    let mut s = session_with(vec![text]);
    s.edit_region_word([0.0, 0.0], Some(1));
    s.draft_pointer(DraftPointer::Press(6));
    s.draft_pointer(DraftPointer::Drag(11));
    assert!(s.format_text_selection(&StyleChange::Bold(true)));
    assert_eq!(s.commit(), Ok(true));
    let RegionKind::Text(body) = &s.document().region(1).expect("text region").kind else {
        panic!("a text region")
    };
    assert_eq!(body.as_str(), "Hello world");
    assert!(body.rich().lines[0]
        .runs
        .iter()
        .any(|run| run.style.bold && run.text == "world"));
}

#[test]
fn an_empty_derivative_placeholder_is_discarded_on_escape() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    s.insert_template(crate::edit::math::Template::Derivative);
    assert!(s.has_draft());
    s.key(EditKey::Escape);
    assert!(!s.has_draft(), "Escape drops the empty placeholder");
    assert!(ids(&s).is_empty());
    assert!(s.message().is_none(), "and says nothing about it");
}

#[test]
fn a_derivative_with_a_typed_slot_is_kept_on_escape() {
    let mut s = session_with(vec![]);
    s.click([10.0, 10.0], None);
    s.insert_template(crate::edit::math::Template::Derivative);
    type_text(&mut s, "x");
    s.key(EditKey::Escape);
    assert!(!s.has_draft());
    assert_eq!(
        ids(&s),
        vec![1],
        "a draft with typed content is kept as before"
    );
}

/// One grid row, in points: a new region box sits this far above the click point.
const GRID: f64 = 14.173_f32 as f64;

#[test]
fn enter_commits_and_starts_a_new_region_one_row_below() {
    let mut s = session_with(vec![]);
    s.click([100.0, 120.0], None);
    type_text(&mut s, "x:=5");
    s.key(EditKey::Enter);
    type_text(&mut s, "y:=x*2");
    s.key(EditKey::Enter);
    type_text(&mut s, "y=");
    assert_eq!(ids(&s), vec![1, 2, 3], "three regions, not one");
    let texts: Vec<String> = s
        .document()
        .regions()
        .iter()
        .map(|r| match &r.kind {
            RegionKind::Math(e) => e.to_string(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(texts[0], "x:=5");
    assert!(texts[1].starts_with("y:="), "{texts:?}");
    let ys: Vec<f64> = s
        .document()
        .regions()
        .iter()
        .map(|r| r.pos.unwrap().1)
        .collect();
    assert!(
        ys[0] < ys[1] && ys[1] < ys[2],
        "each region is below the last: {ys:?}"
    );
}

#[test]
fn enter_on_an_unreadable_draft_keeps_it_open() {
    let mut s = session_with(vec![]);
    s.click([100.0, 120.0], None);
    type_text(&mut s, "2+*3");
    s.key(EditKey::Enter);
    assert!(s.draft_view().is_some(), "the draft stays");
    assert!(ids(&s).is_empty());
}

#[test]
fn enter_in_a_text_region_still_adds_a_line() {
    let mut s = session_with(vec![]);
    s.click([100.0, 120.0], None);
    assert!(s.start_text());
    type_text(&mut s, "a");
    s.key(EditKey::Enter);
    type_text(&mut s, "b");
    let draft = s.draft_view().expect("still typing");
    assert_eq!(draft.text, "a\nb");
}

#[test]
fn a_solve_block_skeleton_is_inserted_at_the_insertion_point() {
    let mut s = session_with(vec![]);
    assert!(
        !s.insert_solve_block_at_crosshair(),
        "needs an insertion point"
    );
    s.click([100.0, 200.0], None);
    assert!(s.insert_solve_block_at_crosshair());
    let id = s.selection().expect("block selected");
    let block = s.document().region(id).expect("block");
    assert!(matches!(block.kind, RegionKind::SolveBlock(_)));
    assert_eq!(block.children.len(), 3);
}

#[test]
fn a_disabled_region_is_not_evaluated() {
    let mut s = session_with(vec![
        math_region(1, (10.0, 20.0), "a:=1"),
        math_region(2, (10.0, 60.0), "b:=2"),
    ]);
    s.click([10.0, 60.0], Some(2));
    assert_eq!(s.toggle_disabled(), Some(true));
    assert!(s.selection_disabled());
    let ws = s.document().eval_worksheet();
    assert_eq!(ws.regions.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1]);
    let vm = evaluate_document(s.document());
    assert_eq!(vm.items.len(), 2, "the region is still drawn");
    assert_eq!(s.toggle_disabled(), Some(false));
    assert_eq!(s.document().eval_worksheet().regions.len(), 2);
}

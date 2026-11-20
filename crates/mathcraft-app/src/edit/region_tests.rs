//! Session tests for region management and the clipboard: arranging, shift-click selection,
//! copy-drag, copy/cut/paste round trips, refused pastes, and undo after random sequences.

use std::collections::BTreeMap;

use mathcraft_core::expr::Expr;
use mathcraft_import::regions::{ImportReport, RegionKind, RegionModel, Settings, Worksheet};
use mathcraft_import::results::ResultTable;

use super::clipboard::{HEADER, MAX_CLIP_BYTES};
use super::document::Document;
use super::layout::Arrange;
use super::math::{parse_math_in, EditContext};
use super::session::EditSession;

fn math_region(id: u32, pos: (f64, f64), size: (f64, f64), text: &str) -> RegionModel {
    let expr = parse_math_in(text, &EditContext::for_regions(&[])).expect("parse");
    RegionModel {
        id,
        pos: Some(pos),
        size: Some(size),
        result_ref: None,
        kind: RegionKind::Math(expr),
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
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

fn sample() -> Vec<RegionModel> {
    vec![
        math_region(1, (10.0, 20.0), (50.0, 15.0), "a := 1"),
        math_region(2, (80.0, 60.0), (30.0, 10.0), "b := 2*a"),
        math_region(3, (200.0, 30.0), (20.0, 40.0), "c := a+b"),
    ]
}

fn positions(s: &EditSession) -> Vec<Option<(f64, f64)>> {
    s.document().regions().iter().map(|r| r.pos).collect()
}

fn ids(s: &EditSession) -> Vec<u32> {
    s.document().regions().iter().map(|r| r.id).collect()
}

#[test]
fn shift_click_adds_and_removes_from_the_selection() {
    let mut s = session_with(sample());
    s.toggle_selection(Some(1));
    s.toggle_selection(Some(2));
    assert_eq!(s.selected(), &[1, 2]);
    s.toggle_selection(Some(1));
    assert_eq!(s.selected(), &[2]);
    // A press on empty page (no hit) changes nothing.
    s.toggle_selection(None);
    assert_eq!(s.selected(), &[2]);
}

#[test]
fn align_left_moves_the_regions_as_one_undo_step() {
    let mut s = session_with(sample());
    let before = s.document().regions().to_vec();
    s.select_all();
    s.arrange(Arrange::AlignLeft);
    assert_eq!(
        positions(&s),
        vec![Some((10.0, 20.0)), Some((10.0, 60.0)), Some((10.0, 30.0))]
    );
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn distribute_needs_three_movable_regions() {
    let mut s = session_with(sample());
    s.toggle_selection(Some(1));
    s.toggle_selection(Some(2));
    s.arrange(Arrange::DistributeHorizontal);
    assert!(s.message().is_some());
    assert!(!s.can_undo());
}

#[test]
fn distribute_spaces_three_regions_evenly() {
    let mut s = session_with(sample());
    s.select_all();
    s.arrange(Arrange::DistributeHorizontal);
    // Left edges 10, 80, 200 with widths 50, 30, 20: span 210, widths 100, so gaps of 55.
    let xs: Vec<f64> = positions(&s).iter().map(|p| p.expect("pos").0).collect();
    assert_eq!(xs, vec![10.0, 115.0, 200.0]);
}

#[test]
fn insert_lines_moves_regions_below_the_insertion_point() {
    let mut s = session_with(sample());
    s.click([0.0, 50.0], None);
    s.insert_lines(12.0);
    assert_eq!(
        positions(&s),
        vec![Some((10.0, 20.0)), Some((80.0, 72.0)), Some((200.0, 30.0))]
    );
    assert!(s.undo());
    assert_eq!(positions(&s)[1], Some((80.0, 60.0)));
}

#[test]
fn remove_lines_closes_the_gap_below_the_insertion_point_and_undo_restores_it() {
    let mut s = session_with(sample());
    // Region 1 ends at y 35, region 2 starts at 60: the point at y 40 has 20 points below it.
    s.click([0.0, 40.0], None);
    s.remove_lines(12.0);
    assert_eq!(positions(&s)[1], Some((80.0, 48.0)));
    s.remove_lines(f64::INFINITY);
    assert_eq!(positions(&s)[1], Some((80.0, 40.0)));
    s.remove_lines(f64::INFINITY);
    assert!(s.message().is_some(), "no gap is left");
    assert!(s.undo());
    assert!(s.undo());
    assert_eq!(positions(&s)[1], Some((80.0, 60.0)));
}

#[test]
fn a_blank_page_moves_the_regions_below_by_the_content_height() {
    let mut s = session_with(sample());
    s.click([0.0, 50.0], None);
    s.insert_blank_page();
    let page = s.document().page();
    let m = page.margins();
    let content = page.paper_points()[1] - m.top - m.bottom;
    assert_eq!(positions(&s)[1], Some((80.0, 60.0 + content)));
}

#[test]
fn separate_regions_moves_selected_overlapping_regions_apart() {
    let mut s = session_with(vec![
        math_region(1, (10.0, 20.0), (50.0, 30.0), "a := 1"),
        math_region(2, (30.0, 40.0), (50.0, 30.0), "b := 2"),
    ]);
    s.separate_selected(true);
    assert!(s.message().is_some(), "two regions must be selected");
    s.select_all();
    s.separate_selected(true);
    // Region 2 now starts below region 1 (20 + 30) plus the gap.
    assert_eq!(positions(&s)[1], Some((30.0, 56.0)));
    assert!(s.undo());
    assert_eq!(positions(&s)[1], Some((30.0, 40.0)));
    s.select_all();
    s.separate_selected(false);
    assert_eq!(positions(&s)[1], Some((66.0, 40.0)));
}

#[test]
fn the_error_trace_selects_a_region_by_id() {
    let mut s = session_with(sample());
    assert!(s.select_region(2));
    assert_eq!(s.selection(), Some(2));
    assert!(!s.select_region(99));
    assert_eq!(s.selection(), Some(2));
}

#[test]
fn insert_lines_needs_an_insertion_point() {
    let mut s = session_with(sample());
    s.insert_lines(12.0);
    assert!(s.message().is_some());
    assert!(!s.can_undo());
}

#[test]
fn ctrl_drag_copies_the_selected_regions_where_the_mouse_lands() {
    let mut s = session_with(sample());
    assert!(s.begin_copy_drag(Some(1)));
    s.drag_to([100.0, 0.0]);
    s.end_drag();
    assert_eq!(s.document().regions().len(), 4);
    // The mouse lands at (110, 20); the copy goes to the grid corner nearest to it (grid lines 8
    // and 1 of the Standard grid, counted from the margin box corner).
    let copy = &s.document().regions()[3];
    let line = |k: f64| k * f64::from(crate::canvas::grid_points(&Default::default()));
    assert_eq!(copy.pos, Some((line(8.0), line(1.0))));
    assert_eq!(copy.kind, s.document().regions()[0].kind);
    assert!(copy.id > 3, "the copy has a new id");
    assert!(s.undo());
    assert_eq!(s.document().regions().len(), 3);
    // The original did not move.
    assert_eq!(positions(&s)[0], Some((10.0, 20.0)));
}

#[test]
fn copy_and_paste_gives_equal_regions_at_the_insertion_point() {
    let mut s = session_with(sample());
    s.select_all();
    let text = s.copy_selection().expect("copied");
    assert!(text.starts_with(HEADER));
    s.click([300.0, 300.0], None);
    s.paste(&text);
    let doc = s.document();
    assert_eq!(doc.regions().len(), 6);
    for (original, pasted) in sample().iter().zip(&doc.regions()[3..]) {
        assert_eq!(pasted.kind, original.kind);
        assert_eq!(pasted.size, original.size);
    }
    // The group's top-left corner is at the insertion point (the first region was at 10,20).
    assert_eq!(doc.regions()[3].pos, Some((300.0, 300.0)));
    assert_eq!(doc.regions()[4].pos, Some((370.0, 340.0)));
    assert!(s.undo());
    assert_eq!(s.document().regions().len(), 3);
}

#[test]
fn a_fragment_pastes_into_another_session() {
    let mut first = session_with(sample());
    first.toggle_selection(Some(2));
    let text = first.copy_selection().expect("copied");
    let mut second = session_with(vec![]);
    second.click([5.0, 5.0], None);
    second.paste(&text);
    let pasted = second.document().regions();
    assert_eq!(pasted.len(), 1);
    assert_eq!(pasted[0].kind, sample()[1].kind);
}

#[test]
fn cut_removes_the_regions_and_undo_puts_them_back() {
    let mut s = session_with(sample());
    let before = s.document().regions().to_vec();
    s.toggle_selection(Some(2));
    let text = s.cut_selection().expect("cut");
    assert_eq!(ids(&s), vec![1, 3]);
    // The printer writes a define as `b:=...`.
    assert!(text.contains("b:="));
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn math_text_copies_and_cuts_only_the_selected_part() {
    let mut s = session_with(vec![]);
    s.click([0.0, 0.0], None);
    s.type_char('a');
    s.type_char('+');
    s.type_char('b');
    s.key(super::session::EditKey::SelectLeft);
    s.key(super::session::EditKey::SelectLeft);
    assert_eq!(s.copy_selection().as_deref(), Some("+b"));
    assert_eq!(s.cut_selection().as_deref(), Some("+b"));
    s.key(super::session::EditKey::Enter);
    assert_eq!(s.document().regions().len(), 1);
    match &s.document().regions()[0].kind {
        RegionKind::Math(e) => assert_eq!(e.to_string(), Expr::var("a").to_string()),
        other => panic!("expected a math region, got {other:?}"),
    }
}

#[test]
fn pasting_plain_math_text_makes_a_math_region_per_line() {
    let mut s = session_with(vec![]);
    s.click([50.0, 50.0], None);
    s.paste("x := 3\n\ny := x * 2\n");
    let regions = s.document().regions();
    assert_eq!(regions.len(), 2, "message: {:?}", s.message());
    assert_eq!(regions[1].pos, Some((50.0, 74.0)));
}

#[test]
fn pasting_plain_text_that_does_not_read_changes_nothing() {
    let mut s = session_with(vec![]);
    s.click([50.0, 50.0], None);
    s.paste("ok := 1\n(((\n");
    assert!(s.document().regions().is_empty());
    assert!(s.message().is_some());
}

#[test]
fn a_paste_with_no_insertion_point_is_refused_with_a_message() {
    let mut s = session_with(vec![]);
    s.paste("x := 3");
    assert!(s.document().regions().is_empty());
    assert!(s.message().is_some());
}

#[test]
fn a_huge_or_damaged_paste_changes_nothing() {
    let mut s = session_with(sample());
    s.click([1.0, 1.0], None);
    let huge = format!(
        "{HEADER}\n{}",
        "T\t0\t0\t-\t-\tx\n".repeat(MAX_CLIP_BYTES / 8)
    );
    s.paste(&huge);
    assert_eq!(s.document().regions().len(), 3);
    assert!(s.message().is_some());
    s.paste(&format!("{HEADER}\nM\t0\t0\t-\t-\t((((\n"));
    s.paste(&format!(
        "{HEADER}\nM\t0\t0\t-\t-\t{}1{}\n",
        "(".repeat(3000),
        ")".repeat(3000)
    ));
    s.paste(&format!("{HEADER}\nM\tnan\t0\t-\t-\tx\n"));
    assert_eq!(s.document().regions().len(), 3);
}

#[test]
fn a_rubber_band_selects_the_regions_it_covers() {
    let mut s = session_with(sample());
    s.begin_press([0.0, 0.0], None);
    s.drag_to([100.0, 70.0]);
    assert_eq!(s.marquee_rect(), Some([0.0, 0.0, 100.0, 70.0]));
    s.end_drag();
    // Regions 1 and 2 meet the band; region 3 (at x=200) does not.
    assert_eq!(s.selected(), &[1, 2]);
    assert_eq!(s.marquee_rect(), None);
    assert!(!s.can_undo(), "a selection is not an edit");
}

#[test]
fn a_band_dragged_up_and_left_selects_the_same_regions() {
    let mut s = session_with(sample());
    s.begin_press([250.0, 100.0], None);
    s.drag_to([-200.0, -60.0]);
    s.end_drag();
    // The band runs from (50, 40) to (250, 100). Region 1 (y 20..35) stays out; regions 2 (x
    // 80..110, y 60..70) and 3 (x 200..220, y 30..70) meet it.
    assert_eq!(s.selected(), &[2, 3]);
}

#[test]
fn a_tiny_band_is_a_plain_press_and_clears_the_selection() {
    let mut s = session_with(sample());
    s.select_all();
    s.begin_press([5.0, 5.0], None);
    s.drag_to([0.5, 0.5]);
    s.end_drag();
    assert!(s.selected().is_empty());
}

#[test]
fn a_fragment_with_a_solve_block_is_not_copied() {
    let mut s = session_with(vec![RegionModel {
        id: 1,
        pos: Some((0.0, 0.0)),
        size: None,
        result_ref: None,
        kind: RegionKind::Unsupported {
            reason: "plot".into(),
        },
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }]);
    s.toggle_selection(Some(1));
    assert_eq!(s.copy_selection(), None);
    assert!(s.message().is_some());
}

/// A small deterministic generator for the undo property.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

#[test]
fn undo_after_any_sequence_restores_the_document_exactly() {
    // Property: after any sequence of region operations, undoing each one that changed the
    // document brings back the original regions exactly.
    let mut rng = Lcg(2026);
    for _ in 0..60 {
        let mut s = session_with(sample());
        let original = s.document().regions().to_vec();
        for _ in 0..(5 + rng.below(20)) {
            let id = (1 + rng.below(3)) as u32;
            match rng.below(12) {
                0 => s.select_all(),
                1 => s.toggle_selection(Some(id)),
                2 => s.arrange(
                    [
                        Arrange::AlignLeft,
                        Arrange::AlignRight,
                        Arrange::AlignTop,
                        Arrange::AlignBottom,
                        Arrange::AlignCenterX,
                        Arrange::AlignCenterY,
                        Arrange::DistributeHorizontal,
                        Arrange::DistributeVertical,
                    ][rng.below(8)],
                ),
                3 => s.click([rng.below(300) as f64, rng.below(300) as f64], None),
                4 => s.insert_lines(12.0),
                5 => {
                    s.begin_copy_drag(Some(id));
                    s.drag_to([rng.below(50) as f64, rng.below(50) as f64]);
                    s.end_drag();
                }
                6 => {
                    s.begin_drag(Some(id));
                    s.drag_to([-(rng.below(50) as f64), 7.0]);
                    s.end_drag();
                }
                7 => {
                    if let Some(text) = s.copy_selection() {
                        s.click([rng.below(300) as f64, 400.0], None);
                        s.paste(&text);
                    }
                }
                8 => {
                    s.cut_selection();
                }
                9 => s.delete_selected(),
                10 => s.nudge(6.0, 0.0),
                _ => {
                    s.click([0.0, 0.0], None);
                    s.type_char('z');
                    s.commit().ok();
                }
            }
        }
        // Undo until nothing is left to undo.
        while s.undo() {}
        assert_eq!(s.document().regions(), original.as_slice());
    }
}

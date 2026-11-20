//! Tests of the pointer and keyboard behaviour of the session on synthetic documents: drags,
//! multi-selection, batch undo, templates. No window is needed.

use std::collections::BTreeMap;

use mathcraft_core::expr::Expr;
use mathcraft_import::regions::{ImportReport, RegionKind, RegionModel, Settings, Worksheet};
use mathcraft_import::results::ResultTable;

use crate::edit::command::{moved_position, Batch, Command, EditError, MoveRegion};
use crate::edit::document::Document;
use crate::edit::math::{parse_math, Template};
use crate::edit::session::{EditKey, EditSession, NUDGE};

fn math_region(id: u32, pos: (f64, f64)) -> RegionModel {
    RegionModel {
        id,
        pos: Some(pos),
        size: None,
        result_ref: None,
        kind: RegionKind::Math(Expr::var("a")),
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
    let results = ResultTable {
        by_id: BTreeMap::new(),
    };
    EditSession::new(Document::from_worksheet("t", ws, results))
}

fn pos_of(s: &EditSession, id: u32) -> Option<(f64, f64)> {
    s.document().region(id).and_then(|r| r.pos)
}

fn ids(s: &EditSession) -> Vec<u32> {
    s.document().regions().iter().map(|r| r.id).collect()
}

/// The grid line `k` of the content frame, measured from the margin box corner: `k * grid` for the
/// Standard grid.
fn grid_line(k: f64) -> f64 {
    k * f64::from(crate::canvas::grid_points(&Default::default()))
}

#[test]
fn a_drag_of_a_selected_group_moves_all_of_it_as_one_undo_step() {
    let mut s = session_with(vec![
        math_region(1, (grid_line(7.0), grid_line(7.0))),
        math_region(2, (grid_line(14.0), grid_line(14.0))),
    ]);
    s.select_all();
    assert!(s.begin_drag(Some(1)));
    assert_eq!(s.selected(), &[1, 2], "a selected region keeps the group");
    // The first region's corner lands on the grid line nearest to the drop point: x 117 -> line 9,
    // y 107 -> line 8.
    s.drag_to([30.0, 20.0]);
    s.end_drag();
    assert_eq!(pos_of(&s, 1), Some((grid_line(9.0), grid_line(8.0))));
    assert_eq!(
        pos_of(&s, 2),
        Some((grid_line(16.0), grid_line(15.0))),
        "the group keeps its spacing"
    );
    assert!(s.undo());
    assert_eq!(pos_of(&s, 1), Some((grid_line(7.0), grid_line(7.0))));
    assert_eq!(pos_of(&s, 2), Some((grid_line(14.0), grid_line(14.0))));
    assert!(!s.undo(), "the whole move was one step");
}

#[test]
fn dragging_an_unselected_region_selects_only_it_and_stops_at_the_page_edge() {
    let mut s = session_with(vec![
        math_region(1, (100.0, 100.0)),
        math_region(2, (200.0, 200.0)),
    ]);
    s.click([0.0, 0.0], Some(1));
    assert!(s.begin_drag(Some(2)));
    assert_eq!(s.selected(), &[2]);
    s.drag_to([-500.0, -500.0]);
    s.end_drag();
    // The margin box corner is grid line 0: a drag past it stops there, inside the printable area.
    assert_eq!(pos_of(&s, 2), Some((grid_line(0.0), grid_line(0.0))));
}

#[test]
fn a_drag_that_moves_nothing_adds_no_undo_step() {
    let mut s = session_with(vec![math_region(1, (100.0, 100.0))]);
    assert!(s.begin_drag(Some(1)));
    s.drag_to([0.0, 0.0]);
    s.end_drag();
    assert!(!s.undo());
}

#[test]
fn a_drag_on_empty_page_does_not_start() {
    let mut s = session_with(vec![math_region(1, (100.0, 100.0))]);
    assert!(!s.begin_drag(None));
    assert!(s.drag_preview().is_none());
}

#[test]
fn the_drag_preview_shows_the_regions_and_the_displacement() {
    // The region starts on grid line 7; the drag lands on y 107, which snaps to grid line 8: the
    // preview shows the snapped displacement of one grid cell.
    let mut s = session_with(vec![math_region(1, (grid_line(7.0), grid_line(7.0)))]);
    assert!(s.begin_drag(Some(1)));
    s.drag_to([20.0, 20.0]);
    let (ids, delta) = s.drag_preview().expect("a drag is in progress");
    assert_eq!(ids, &[1]);
    let cell = grid_line(8.0) - grid_line(7.0);
    assert_eq!(delta, [cell, cell]);
}

#[test]
fn delete_removes_every_selected_region_and_undo_puts_them_back_in_order() {
    let mut s = session_with(vec![
        math_region(1, (0.0, 0.0)),
        math_region(2, (0.0, 50.0)),
        math_region(3, (0.0, 100.0)),
    ]);
    s.click([0.0, 0.0], Some(1));
    s.select_all();
    s.key(EditKey::Delete);
    assert!(ids(&s).is_empty());
    assert!(s.undo());
    assert_eq!(ids(&s), vec![1, 2, 3]);
}

#[test]
fn delete_of_a_middle_and_a_last_region_keeps_the_order_on_undo() {
    let mut s = session_with(vec![
        math_region(1, (0.0, 0.0)),
        math_region(2, (0.0, 50.0)),
        math_region(3, (0.0, 100.0)),
    ]);
    s.click([0.0, 0.0], Some(2));
    s.key(EditKey::Delete);
    assert_eq!(ids(&s), vec![1, 3]);
    assert!(s.undo());
    assert_eq!(ids(&s), vec![1, 2, 3]);
}

#[test]
fn arrow_keys_move_the_whole_selection_and_undo_reverts_it() {
    let mut s = session_with(vec![
        math_region(1, (100.0, 100.0)),
        math_region(2, (50.0, 50.0)),
    ]);
    s.select_all();
    s.key(EditKey::Left);
    assert_eq!(pos_of(&s, 1), Some((100.0 - NUDGE, 100.0)));
    assert_eq!(pos_of(&s, 2), Some((50.0 - NUDGE, 50.0)));
    assert!(s.undo());
    assert_eq!(pos_of(&s, 2), Some((50.0, 50.0)));
}

#[test]
fn escape_with_nothing_typed_clears_the_selection_and_the_insertion_point() {
    let mut s = session_with(vec![math_region(1, (100.0, 100.0))]);
    s.select_all();
    s.key(EditKey::Escape);
    assert!(s.selected().is_empty());
    s.click([5.0, 5.0], None);
    s.key(EditKey::Escape);
    assert!(s.crosshair().is_none());
}

#[test]
fn typing_with_several_regions_selected_asks_for_one() {
    let mut s = session_with(vec![
        math_region(1, (0.0, 0.0)),
        math_region(2, (0.0, 50.0)),
    ]);
    s.select_all();
    s.type_char('x');
    assert!(!s.has_draft());
    assert!(s.message().is_some());
}

#[test]
fn typing_at_an_insertion_point_is_a_draft_until_committed() {
    let mut s = session_with(vec![]);
    s.click([20.0, 20.0], None);
    s.type_char('q');
    assert!(s.has_draft(), "a draft makes the document dirty");
    assert!(s.commit().is_ok());
    assert!(!s.has_draft());
    assert_eq!(ids(&s).len(), 1);
}

#[test]
fn the_fraction_template_wraps_the_operand_before_the_caret() {
    let mut s = session_with(vec![]);
    s.click([20.0, 20.0], None);
    s.type_char('x');
    s.insert_template(Template::Fraction);
    let view = s.draft_view().expect("the draft is shown");
    assert_eq!(view.text, "x/_", "the empty denominator is a placeholder");
    assert!(
        s.commit().is_ok(),
        "the template commits as a valid expression"
    );
}

#[test]
fn the_define_and_evaluate_templates_type_their_symbols() {
    let mut s = session_with(vec![]);
    s.click([20.0, 20.0], None);
    s.type_char('a');
    s.insert_template(Template::Define);
    assert_eq!(s.draft_view().expect("draft").text, "a:=");
    assert!(
        s.commit().is_err(),
        "an unfinished definition is not committed"
    );
    assert!(s.has_draft(), "and its draft stays open");
    s.type_char('2');
    s.commit().expect("the finished definition commits");

    let mut s = session_with(vec![]);
    s.click([20.0, 20.0], None);
    s.type_char('b');
    s.insert_template(Template::Evaluate);
    // Evaluate commits the region at once, with its `=`.
    assert!(!s.has_draft());
    assert_eq!(ids(&s).len(), 1);
}

#[test]
fn the_matrix_template_reads_as_a_matrix_with_placeholders() {
    assert!(parse_math("[■, ■; ■, ■]").is_ok());
    let mut s = session_with(vec![]);
    s.click([20.0, 20.0], None);
    s.insert_template(Template::Matrix2x2);
    assert!(s.commit().is_ok());
}

#[test]
fn the_math_button_starts_a_region_at_the_insertion_point_only() {
    let mut s = session_with(vec![math_region(1, (0.0, 0.0))]);
    assert!(!s.start_math(), "no insertion point, no region");
    assert!(s.message().is_some());
    s.click([10.0, 10.0], None);
    assert!(s.start_math());
    assert!(s.has_draft());
}

#[test]
fn a_failing_batch_reverts_the_steps_it_already_made() {
    let mut doc = Document::from_worksheet(
        "t",
        Worksheet {
            regions: vec![math_region(1, (1.0, 1.0))],
            settings: Settings::default(),
            page: Default::default(),
            report: ImportReport::default(),
            header: Vec::new(),
            footer: Vec::new(),
        },
        ResultTable {
            by_id: BTreeMap::new(),
        },
    );
    let batch = Batch::new(
        "Move region",
        vec![
            Box::new(MoveRegion {
                id: 1,
                from: (1.0, 1.0),
                to: (9.0, 9.0),
            }),
            Box::new(MoveRegion {
                id: 42,
                from: (0.0, 0.0),
                to: (1.0, 1.0),
            }),
        ],
    );
    assert_eq!(batch.apply(&mut doc), Err(EditError::NotFound(42)));
    assert_eq!(doc.region(1).and_then(|r| r.pos), Some((1.0, 1.0)));
}

#[test]
fn a_move_is_clamped_to_the_page_origin() {
    assert_eq!(moved_position((5.0, 5.0), [-10.0, 2.0]), (0.0, 7.0));
    assert_eq!(moved_position((5.0, 5.0), [0.5, -0.5]), (5.5, 4.5));
}

//! The commands behind the Matrizen/Tabellen and Diagramme tabs, through the editing session.

use std::collections::BTreeMap;

use mathcraft_import::regions::{ImportReport, RegionKind, Settings, Worksheet};
use mathcraft_import::results::ResultTable;

use super::document::Document;
use super::math::{MathEditor, MatrixOp};
use super::plot::{PlotOp, StyleChange, PALETTE};
use super::session::EditSession;

fn empty_session() -> EditSession {
    let ws = Worksheet {
        regions: Vec::new(),
        settings: Settings::default(),
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer: Vec::new(),
    };
    EditSession::new(Document::from_worksheet(
        "test",
        ws,
        ResultTable {
            by_id: BTreeMap::new(),
        },
    ))
}

fn with_plot() -> EditSession {
    let mut s = empty_session();
    s.click([48.0, 72.0], None);
    assert!(s.insert_plot_at_crosshair(false));
    s
}

fn plot(s: &EditSession) -> mathcraft_import::regions::Plot2d {
    s.selected_plot_model().expect("a plot is selected").clone()
}

#[test]
fn rows_and_columns_go_above_and_left_of_the_caret() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(2, 2);
    // The caret is in the first cell: a row above it and a column left of it.
    assert!(ed.matrix_op(MatrixOp::InsertRowAbove));
    assert_eq!(ed.to_text(), "[■, ■; ■, ■; ■, ■]");
    assert!(ed.matrix_op(MatrixOp::InsertColumnLeft));
    assert_eq!(ed.to_text(), "[■, ■, ■; ■, ■, ■; ■, ■, ■]");
    // The caret sits in the new cell: typing fills it.
    ed.insert_text("x");
    assert_eq!(ed.to_text(), "[x, ■, ■; ■, ■, ■; ■, ■, ■]");
}

#[test]
fn the_size_grid_inserts_a_matrix_into_the_draft() {
    let mut s = empty_session();
    s.click([48.0, 72.0], None);
    s.insert_matrix(3, 4);
    let view = s.draft_view().expect("a math draft");
    assert!(view.math);
    assert_eq!(
        view.text.matches(';').count(),
        2,
        "three rows: {}",
        view.text
    );
    assert_eq!(view.text.matches(',').count(), 9, "four columns");
}

#[test]
fn add_and_remove_trace_are_undoable() {
    let mut s = with_plot();
    assert_eq!(plot(&s).traces.len(), 1);
    assert!(s.plot_op(PlotOp::AddTrace));
    let p = plot(&s);
    assert_eq!(p.traces.len(), 2);
    assert_eq!(p.x_axis.equations.len(), 2);
    assert_eq!(p.traces[1].style.color, Some(PALETTE[1]));
    assert!(s.plot_op(PlotOp::RemoveTrace));
    assert_eq!(plot(&s).traces.len(), 1);
    assert!(!s.plot_op(PlotOp::RemoveTrace), "the last trace stays");
    assert!(s.undo());
    assert_eq!(plot(&s).traces.len(), 2);
    assert!(s.undo());
    assert_eq!(plot(&s).traces.len(), 1);
}

#[test]
fn markers_are_added_and_deleted() {
    let mut s = with_plot();
    assert!(!s.plot_op(PlotOp::DeleteMarker), "no marker yet");
    assert!(s.plot_op(PlotOp::AddMarker(true)));
    assert!(s.plot_op(PlotOp::AddMarker(false)));
    let p = plot(&s);
    assert_eq!(p.markers.len(), 2);
    assert!(p.markers[0].vertical && !p.markers[1].vertical);
    assert_eq!(p.markers[0].value, 10.0);
    assert!(s.plot_op(PlotOp::DeleteMarker));
    assert_eq!(plot(&s).markers.len(), 1);
    assert!(s.undo());
    assert_eq!(plot(&s).markers.len(), 2);
}

#[test]
fn trace_style_changes_the_last_trace() {
    let mut s = with_plot();
    assert!(s.plot_op(PlotOp::AddTrace));
    assert!(s.plot_op(PlotOp::Style(StyleChange::Color(2))));
    assert!(s.plot_op(PlotOp::Style(StyleChange::Weight(6))));
    assert!(s.plot_op(PlotOp::Style(StyleChange::Line("Dash"))));
    assert!(s.plot_op(PlotOp::Style(StyleChange::Symbol("circle"))));
    let p = plot(&s);
    assert_eq!(
        p.traces[0].style.color,
        Some(PALETTE[0]),
        "first trace untouched"
    );
    let style = &p.traces[1].style;
    assert_eq!(style.color, Some(PALETTE[2]));
    assert_eq!(style.line_weight, Some(6.0));
    assert_eq!(style.line_style.as_deref(), Some("Dash"));
    assert_eq!(style.symbol.as_deref(), Some("circle"));
}

#[test]
fn axes_and_chart_type() {
    let mut s = with_plot();
    assert!(s.plot_op(PlotOp::ToggleLog(false)));
    assert!(plot(&s).x_axis.log && !plot(&s).y_axis.log);
    assert!(s.plot_op(PlotOp::ToggleGrid));
    assert!(plot(&s).x_axis.grid_lines && plot(&s).y_axis.grid_lines);
    assert!(s.plot_op(PlotOp::SetPolar(true)));
    assert!(plot(&s).polar);
    assert!(!plot(&s).x_axis.log, "log is for x-y plots");
    assert!(!s.plot_op(PlotOp::SetPolar(true)), "already polar");
    assert!(
        !s.plot_op(PlotOp::ToggleLog(true)),
        "no log on a polar plot"
    );
    assert!(s.undo());
    assert!(!plot(&s).polar);
}

#[test]
fn plot_commands_need_a_selected_plot() {
    let mut s = empty_session();
    assert!(!s.plot_op(PlotOp::AddTrace));
    assert!(s.selected_plot_model().is_none());
}

#[test]
fn matrix_result_flags_toggle_on_the_math_region_and_undo() {
    let mut s = empty_session();
    s.click([48.0, 72.0], None);
    assert!(s.start_math());
    for c in "m:=1".chars() {
        s.type_char(c);
    }
    s.commit().expect("commit");
    let id = s.matrix_target().expect("the region is the target");
    let flags = |s: &EditSession| {
        let m = s.document().region(id).expect("region").matrix;
        (m.show_indices, m.expand_nested)
    };
    assert_eq!(flags(&s), (false, false));
    assert!(s.toggle_matrix_flag(true));
    assert!(s.toggle_matrix_flag(false));
    assert_eq!(flags(&s), (true, true));
    assert!(s.undo());
    assert_eq!(flags(&s), (true, false));
    assert!(matches!(
        s.document().region(id).expect("region").kind,
        RegionKind::Math(_)
    ));
}

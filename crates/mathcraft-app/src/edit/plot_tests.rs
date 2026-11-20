//! Plots through the editing session: insert, edit, undo, move, resize, delete, and the round
//! trip through the writer and the reader.

use std::collections::BTreeMap;

use mathcraft_import::evaluate::on_big_stack;
use mathcraft_import::regions::{Equation, ImportReport, RegionKind, Settings, Worksheet};
use mathcraft_import::results::ResultTable;

use super::document::Document;
use super::plot::{PlotSpec, PALETTE};
use super::resize::Corner;
use super::save::{DocumentWriter, McdxWriter};
use super::session::{EditKey, EditSession};
use crate::load::{evaluate_document, open_file};
use crate::viewmodel::Body;

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

fn spec(x: &str, y: &str) -> PlotSpec {
    PlotSpec {
        x: x.into(),
        y: y.into(),
        ..PlotSpec::default()
    }
}

fn plot_of(s: &EditSession, id: u32) -> mathcraft_import::regions::Plot2d {
    match &s.document().region(id).expect("region").kind {
        RegionKind::Plot(info) => *info.plot.clone().expect("2d plot"),
        other => panic!("not a plot: {other:?}"),
    }
}

#[test]
fn insert_gives_an_empty_xy_plot_at_the_insertion_point() {
    let mut s = empty_session();
    assert!(
        !s.insert_plot_at_crosshair(false),
        "needs an insertion point"
    );
    s.click([48.0, 72.0], None);
    assert!(s.insert_plot_at_crosshair(false));
    let region = &s.document().regions()[0];
    assert_eq!(region.pos, Some((48.0, 72.0)));
    assert!(region.size.is_some());
    assert_eq!(s.selection(), Some(region.id));
    let plot = plot_of(&s, region.id);
    assert!(!plot.polar);
    assert_eq!(plot.traces.len(), 1);
    assert_eq!(plot.traces[0].x, Equation::Empty);
    assert_eq!(plot.traces[0].style.color, Some(PALETTE[0]));
    assert!(s.undo());
    assert!(s.document().regions().is_empty());
    assert!(s.redo());
    assert_eq!(s.document().regions().len(), 1);
}

#[test]
fn at_sign_and_polar_insert() {
    let mut s = empty_session();
    s.click([24.0, 24.0], None);
    s.type_char('@');
    assert_eq!(s.document().regions().len(), 1);
    assert!(!s.has_draft());
    s.click([24.0, 400.0], None);
    s.insert_plot_at_crosshair(true);
    let id = s.document().regions()[1].id;
    assert!(plot_of(&s, id).polar);
}

#[test]
fn editing_sets_traces_and_undo_restores() {
    let mut s = empty_session();
    s.click([0.0, 0.0], None);
    s.insert_plot_at_crosshair(false);
    let id = s.selection().unwrap();
    let before = s.document().region(id).cloned().unwrap();
    s.set_plot_spec(id, &spec("x", "sin(x), cos(x)")).unwrap();
    let plot = plot_of(&s, id);
    assert_eq!(plot.traces.len(), 2);
    assert!(matches!(plot.traces[1].y, Equation::Expr(_)));
    // The shorter axis serves the rest of the traces.
    assert_eq!(plot.traces[0].x, plot.traces[1].x);
    assert_eq!(plot.traces[1].style.color, Some(PALETTE[1]));
    assert_eq!(s.plot_spec(id).unwrap().y, "sin(x), cos(x)");
    assert!(s.undo());
    assert_eq!(s.document().region(id), Some(&before));
    assert!(s.redo());
    assert_eq!(plot_of(&s, id).traces.len(), 2);
}

#[test]
fn a_bad_expression_or_limit_changes_nothing() {
    let mut s = empty_session();
    s.click([0.0, 0.0], None);
    s.insert_plot_at_crosshair(false);
    let id = s.selection().unwrap();
    let before = s.document().region(id).cloned();
    assert!(s.set_plot_spec(id, &spec("x", "sin(")).is_err());
    let mut bad = spec("x", "x");
    bad.x_min = "abc".into();
    assert!(s.set_plot_spec(id, &bad).is_err());
    bad.x_min = "5".into();
    bad.x_max = "1".into();
    assert!(s.set_plot_spec(id, &bad).is_err());
    assert_eq!(s.document().region(id).cloned(), before);
}

#[test]
fn move_resize_and_delete_use_the_normal_commands() {
    let mut s = empty_session();
    s.click([24.0, 24.0], None);
    s.insert_plot_at_crosshair(false);
    let id = s.selection().unwrap();
    s.key(EditKey::Right);
    assert_eq!(s.document().region(id).unwrap().pos.unwrap().0, 30.0);
    let size = s.document().region(id).unwrap().size.unwrap();
    assert!(s.begin_resize(Corner::BottomRight));
    s.drag_to([60.0, 60.0]);
    s.end_drag();
    let resized = s.document().region(id).unwrap().size.unwrap();
    assert!(resized.0 > size.0);
    assert!(s.undo());
    assert_eq!(s.document().region(id).unwrap().size.unwrap(), size);
    s.key(EditKey::Delete);
    assert!(s.document().regions().is_empty());
    assert!(s.undo());
    assert_eq!(s.document().regions().len(), 1);
}

#[test]
fn a_polar_plot_survives_save_and_reload() {
    let mut s = empty_session();
    s.click([48.0, 96.0], None);
    s.insert_plot_at_crosshair(true);
    let id = s.selection().unwrap();
    s.set_plot_spec(id, &spec("a", "1 + cos(a)")).unwrap();
    let path = std::env::temp_dir().join(format!("mathcraft-polar-{}.mcdx", std::process::id()));
    McdxWriter.write(s.document(), &path).expect("save");
    let again = open_file(&path, None).expect("reopen");
    let _ = std::fs::remove_file(&path);
    assert_eq!(s.document().regions()[0].kind, again.doc.regions()[0].kind);
}

#[test]
fn an_edited_sample_plot_is_written_and_read_back_equal() {
    require_samples!();
    let loaded = open_file(
        &crate::testdata::sample("3_rata_3_Kurve_durch_3_Punkte.mcdx"),
        None,
    )
    .expect("open sample");
    let mut s = EditSession::new(loaded.doc);
    let id = s
        .document()
        .regions()
        .iter()
        .find(|r| matches!(&r.kind, RegionKind::Plot(p) if p.plot.is_some()))
        .map(|r| r.id)
        .expect("a 2d plot");
    let mut sp = s.plot_spec(id).expect("spec");
    sp.title = "Edited".into();
    s.set_plot_spec(id, &sp).expect("edit");
    let path = std::env::temp_dir().join(format!("mathcraft-edited-{}.mcdx", std::process::id()));
    McdxWriter.write(s.document(), &path).expect("save");
    let again = open_file(&path, None).expect("reopen");
    let _ = std::fs::remove_file(&path);
    let (a, b) = (
        s.document().region(id).unwrap(),
        again.doc.region(id).expect("same id"),
    );
    assert_eq!(a.kind, b.kind);
    assert_eq!(a.pos, b.pos);
}

fn drawn_traces(doc: &Document, id: u32) -> Vec<usize> {
    let doc = doc.clone();
    let vm = on_big_stack(move || evaluate_document(&doc));
    vm.items
        .iter()
        .find(|i| i.region_id == id)
        .and_then(|i| match &i.body {
            Body::Plot(view) => Some(view.traces.iter().map(|t| t.segments.len()).collect()),
            _ => None,
        })
        .expect("a drawn plot")
}

#[test]
fn the_plot_draws_live_and_survives_save_and_reload() {
    let mut s = empty_session();
    s.click([48.0, 96.0], None);
    s.insert_plot_at_crosshair(false);
    let id = s.selection().unwrap();
    let mut sp = spec("x", "sin(x), x^2");
    sp.x_min = "-5".into();
    sp.x_max = "5".into();
    sp.grid = true;
    sp.title = "Test".into();
    s.set_plot_spec(id, &sp).unwrap();
    assert_eq!(drawn_traces(s.document(), id), vec![1, 1]);

    let path = std::env::temp_dir().join(format!("mathcraft-plot-{}.mcdx", std::process::id()));
    McdxWriter.write(s.document(), &path).expect("save");
    let again = open_file(&path, None).expect("reopen");
    let _ = std::fs::remove_file(&path);
    let (a, b) = (
        s.document().region(id).unwrap(),
        again.doc.regions().first().expect("region"),
    );
    assert_eq!(a.pos, b.pos);
    match (&a.kind, &b.kind) {
        (RegionKind::Plot(x), RegionKind::Plot(y)) => {
            assert_eq!(x.plot, y.plot, "traces, axes and styles read back equal");
            assert_eq!(x.x_range, y.x_range);
        }
        other => panic!("not plots: {other:?}"),
    }
    assert_eq!(drawn_traces(&again.doc, b.id), vec![1, 1]);
}

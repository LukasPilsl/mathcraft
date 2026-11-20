//! Tests of the document API: ids, edits, undo and redo, reading, calculation, files.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use mathcraft_import::regions::{AreaModel, RegionKind, MAX_REGIONS};

use crate::calc::{CalcOptions, ResultStatus};
use crate::doc::{Doc, DocError, Placement};
use crate::plot::{PlotOp, PlotSpec};
use crate::read::{RegionContent, VariableKind};

fn ids(doc: &Doc) -> Vec<u32> {
    doc.outline().iter().map(|r| r.id).collect()
}

fn distance_sheet() -> (Doc, [u32; 4]) {
    let mut doc = Doc::new("test");
    let d = doc.insert_math("len:=3 m", Placement::Auto).unwrap();
    let t = doc.insert_math("dur:=2 s", Placement::Auto).unwrap();
    let v = doc.insert_math("v:=len/dur", Placement::Auto).unwrap();
    let r = doc.insert_math("v=", Placement::Auto).unwrap();
    (doc, [d, t, v, r])
}

#[test]
fn new_document_is_empty_and_clean() {
    let doc = Doc::new("empty");
    assert!(doc.outline().is_empty());
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    assert_eq!(doc.path(), None);
}

#[test]
fn insert_gives_unique_ids_and_auto_places_below() {
    let (doc, all) = distance_sheet();
    let mut sorted = all.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 4);
    let outline = doc.outline();
    let ys: Vec<f64> = outline.iter().map(|r| r.pos.unwrap().1).collect();
    assert!(
        ys.windows(2).all(|w| w[0] < w[1]),
        "regions go downwards: {ys:?}"
    );
    assert_eq!(outline[0].kind, "math");
    assert_eq!(outline[2].summary, "v:=len/dur");
}

#[test]
fn insert_placement_at_and_after() {
    let mut doc = Doc::new("p");
    let a = doc
        .insert_math("a:=1", Placement::At([100.0, 50.0]))
        .unwrap();
    let c = doc.insert_math("c:=3", Placement::Auto).unwrap();
    let b = doc.insert_math("b:=2", Placement::After(a)).unwrap();
    assert_eq!(doc.region(a).unwrap().pos, Some((100.0, 50.0)));
    // `After` goes next to its region in the list, below it, at its x.
    assert_eq!(ids(&doc), vec![a, b, c]);
    let pos = doc.region(b).unwrap().pos.unwrap();
    assert_eq!(pos.0, 100.0);
    assert!(pos.1 > 50.0);
    assert_eq!(
        doc.insert_math("x:=1", Placement::After(999)),
        Err(DocError::NotFound(999))
    );
}

#[test]
fn insert_math_rejects_text_that_does_not_read() {
    let mut doc = Doc::new("bad");
    let err = doc.insert_math("1 +* 2", Placement::Auto).unwrap_err();
    assert!(matches!(err, DocError::Invalid(_)), "{err:?}");
    assert!(doc.outline().is_empty());
    assert!(!doc.can_undo());
}

#[test]
fn insert_text_and_read_runs() {
    let mut doc = Doc::new("t");
    let id = doc.insert_text("Hello\nworld", Placement::Auto).unwrap();
    let detail = doc.read_region(id).unwrap();
    assert_eq!(detail.kind, "text");
    let RegionContent::Text { text, lines } = detail.content else {
        panic!("not text");
    };
    assert_eq!(text, "Hello\nworld");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].runs[0].text, "Hello");
    assert!(!lines[0].runs[0].bold);
}

#[test]
fn ids_survive_edits_moves_and_deletes() {
    let (mut doc, [d, t, v, r]) = distance_sheet();
    doc.edit_math(t, "dur:=4 s").unwrap();
    doc.move_region(d, [200.0, 300.0]).unwrap();
    doc.delete_region(v).unwrap();
    assert_eq!(ids(&doc), vec![d, t, r]);
    // A new region never takes the id of the deleted one.
    let fresh = doc.insert_math("w:=1", Placement::Auto).unwrap();
    assert!(![d, t, v, r].contains(&fresh));
    assert_eq!(doc.region(t).unwrap().pos, doc.region(t).unwrap().pos);
    assert_eq!(doc.read_region(d).unwrap().pos, Some((200.0, 300.0)));
    let RegionContent::Math { text } = doc.read_region(t).unwrap().content else {
        panic!("not math");
    };
    assert_eq!(text, "dur:=4 s");
}

#[test]
fn undo_and_redo_keep_ids() {
    let (mut doc, [d, t, v, r]) = distance_sheet();
    let before = doc.outline();
    doc.delete_region(t).unwrap();
    assert_eq!(ids(&doc), vec![d, v, r]);
    assert!(doc.undo().unwrap());
    assert_eq!(
        doc.outline(),
        before,
        "undo restores the region under its id and place"
    );
    assert!(doc.redo().unwrap());
    assert_eq!(ids(&doc), vec![d, v, r]);
    assert!(doc.undo().unwrap());
    // Undo of an edit gives back the old text; undo of an insert removes the region.
    doc.edit_math(d, "len:=9 m").unwrap();
    assert!(doc.undo().unwrap());
    let RegionContent::Math { text } = doc.read_region(d).unwrap().content else {
        panic!("not math");
    };
    assert_eq!(text, "len:=3 m");
    assert!(doc.undo().unwrap());
    assert_eq!(ids(&doc), vec![d, t, v]);
    assert!(doc.redo().unwrap());
    assert_eq!(ids(&doc), vec![d, t, v, r]);
}

#[test]
fn undo_with_nothing_to_undo_is_false() {
    let mut doc = Doc::new("u");
    assert_eq!(doc.undo(), Ok(false));
    assert_eq!(doc.redo(), Ok(false));
}

#[test]
fn a_new_edit_clears_redo() {
    let mut doc = Doc::new("u");
    doc.insert_math("a:=1", Placement::Auto).unwrap();
    doc.undo().unwrap();
    assert!(doc.can_redo());
    doc.insert_math("b:=2", Placement::Auto).unwrap();
    assert!(!doc.can_redo());
}

#[test]
fn revision_and_dirty_follow_edits_undo_and_save() {
    let mut doc = Doc::new("r");
    let r0 = doc.revision();
    let id = doc.insert_math("a:=1", Placement::Auto).unwrap();
    assert!(doc.revision() > r0);
    assert!(doc.is_dirty());
    let r1 = doc.revision();
    doc.undo().unwrap();
    assert!(doc.revision() > r1, "undo is a change");
    // Edits that are not commands also count.
    let r2 = doc.revision();
    doc.set_page(doc.page().clone());
    assert!(doc.revision() > r2);
    let _ = id;
}

#[test]
fn edit_math_errors() {
    let mut doc = Doc::new("e");
    let text = doc.insert_text("hi", Placement::Auto).unwrap();
    let math = doc.insert_math("a:=1", Placement::Auto).unwrap();
    assert_eq!(
        doc.edit_math(text, "a:=2"),
        Err(DocError::NotEditable(text))
    );
    assert_eq!(doc.edit_math(77, "a:=2"), Err(DocError::NotFound(77)));
    assert!(matches!(
        doc.edit_math(math, "((("),
        Err(DocError::Invalid(_))
    ));
    assert_eq!(doc.edit_text(math, "x"), Err(DocError::NotEditable(math)));
    // An unchanged edit is no undo step.
    let rev = doc.revision();
    doc.edit_math(math, "a:=1").unwrap();
    assert_eq!(doc.revision(), rev);
}

#[test]
fn edit_text_keeps_the_region() {
    let mut doc = Doc::new("e");
    let id = doc.insert_text("old", Placement::Auto).unwrap();
    doc.edit_text(id, "new").unwrap();
    let RegionContent::Text { text, .. } = doc.read_region(id).unwrap().content else {
        panic!("not text");
    };
    assert_eq!(text, "new");
    assert!(doc.undo().unwrap());
    let RegionContent::Text { text, .. } = doc.read_region(id).unwrap().content else {
        panic!("not text");
    };
    assert_eq!(text, "old");
}

#[test]
fn move_and_resize() {
    let mut doc = Doc::new("m");
    let id = doc
        .insert_empty_plot(false, Placement::At([10.0, 20.0]))
        .unwrap();
    doc.move_region(id, [30.0, 40.0]).unwrap();
    assert_eq!(doc.region(id).unwrap().pos, Some((30.0, 40.0)));
    doc.resize_region(id, (200.0, 100.0)).unwrap();
    assert_eq!(doc.region(id).unwrap().size, Some((200.0, 100.0)));
    assert_eq!(doc.region(id).unwrap().pos, Some((30.0, 40.0)));
    assert!(matches!(
        doc.resize_region(id, (0.0, 10.0)),
        Err(DocError::Invalid(_))
    ));
    assert_eq!(doc.resize_region(9, (1.0, 1.0)), Err(DocError::NotFound(9)));
    doc.undo().unwrap();
    assert_ne!(doc.region(id).unwrap().size, Some((200.0, 100.0)));
    // Positions never go below 0.
    doc.move_region(id, [-5.0, -5.0]).unwrap();
    assert_eq!(doc.region(id).unwrap().pos, Some((0.0, 0.0)));
}

#[test]
fn move_regions_is_one_undo_step() {
    let mut doc = Doc::new("m");
    let a = doc
        .insert_math("a:=1", Placement::At([10.0, 10.0]))
        .unwrap();
    let b = doc
        .insert_math("b:=2", Placement::At([10.0, 50.0]))
        .unwrap();
    assert_eq!(doc.move_regions(&[a, b], [5.0, 6.0]), Ok(true));
    assert_eq!(doc.region(b).unwrap().pos, Some((15.0, 56.0)));
    doc.undo().unwrap();
    assert_eq!(doc.region(a).unwrap().pos, Some((10.0, 10.0)));
    assert_eq!(doc.region(b).unwrap().pos, Some((10.0, 50.0)));
    assert_eq!(doc.move_regions(&[a], [0.0, 0.0]), Ok(false));
}

#[test]
fn delete_regions_batch_and_locked_area() {
    let mut doc = Doc::new("d");
    let a = doc.insert_math("a:=1", Placement::Auto).unwrap();
    let b = doc.insert_math("b:=2", Placement::Auto).unwrap();
    let area = doc
        .insert_region(
            RegionKind::Area(AreaModel {
                locked: true,
                ..AreaModel::default()
            }),
            Some((100.0, 50.0)),
            Placement::Auto,
        )
        .unwrap();
    let deleted = doc.delete_regions(&[b, a, area, 12345]).unwrap();
    assert_eq!(deleted.removed, 2);
    assert!(deleted.locked_skipped);
    assert_eq!(ids(&doc), vec![area]);
    doc.undo().unwrap();
    assert_eq!(
        ids(&doc),
        vec![a, b, area],
        "one undo puts both back in place"
    );
    assert_eq!(doc.delete_region(500), Err(DocError::NotFound(500)));
}

#[test]
fn plot_insert_read_spec_and_edit() {
    let mut doc = Doc::new("p");
    let spec = PlotSpec {
        x: "x".into(),
        y: "sin(x), cos(x)".into(),
        x_min: "0".into(),
        x_max: "6".into(),
        title: "Waves".into(),
        grid: true,
        ..PlotSpec::default()
    };
    let id = doc.insert_plot(&spec, Placement::Auto).unwrap();
    let RegionContent::Plot { kind, spec: read } = doc.read_region(id).unwrap().content else {
        panic!("not a plot");
    };
    assert_eq!(kind, "xyPlot");
    let read = read.unwrap();
    assert_eq!(read.title, "Waves");
    assert_eq!(read.x_min, "0");
    assert!(read.y.contains("sin"));

    let mut changed = read.clone();
    changed.title = "Other".into();
    doc.set_plot_spec(id, &changed).unwrap();
    assert!(doc.undo().unwrap());
    let RegionContent::Plot { spec, .. } = doc.read_region(id).unwrap().content else {
        panic!("not a plot");
    };
    assert_eq!(spec.unwrap().title, "Waves");

    assert_eq!(doc.plot_op(id, PlotOp::ToggleGrid), Ok(true));
    assert_eq!(doc.plot_op(id, PlotOp::DeleteMarker), Ok(false));
    let text = doc.insert_text("t", Placement::Auto).unwrap();
    assert_eq!(
        doc.plot_op(text, PlotOp::AddTrace),
        Err(DocError::NotEditable(text))
    );
    let bad = PlotSpec {
        x_min: "abc".into(),
        ..spec_default()
    };
    assert!(matches!(
        doc.insert_plot(&bad, Placement::Auto),
        Err(DocError::Invalid(_))
    ));
}

fn spec_default() -> PlotSpec {
    PlotSpec::default()
}

#[test]
fn calculate_the_worksheet() {
    let (mut doc, [_, _, v, r]) = distance_sheet();
    let calc = doc.calculate(&CalcOptions::default());
    assert_eq!(calc.stopped, None);
    let result = calc.result(r).expect("the evaluation has a result");
    assert_eq!(result.status, ResultStatus::Value);
    assert_eq!(result.value, "1.5 m/s");
    assert_eq!(calc.result(v).unwrap().defines.as_deref(), Some("v"));
    assert_eq!(calc.result(v).unwrap().status, ResultStatus::Defined);
    // The result shows up when the region is read, until the next change.
    assert_eq!(doc.read_region(r).unwrap().result.unwrap().value, "1.5 m/s");
    doc.edit_math(r, "len=").unwrap();
    assert_eq!(
        doc.read_region(r).unwrap().result,
        None,
        "a change makes it stale"
    );
}

#[test]
fn calculate_a_subset_still_sees_definitions() {
    let (mut doc, [_, _, v, r]) = distance_sheet();
    let calc = doc.calculate(&CalcOptions {
        regions: Some(vec![r]),
        ..CalcOptions::default()
    });
    assert_eq!(calc.results.len(), 1);
    assert_eq!(calc.results[0].region_id, r);
    assert_eq!(calc.results[0].value, "1.5 m/s");
    assert!(calc.result(v).is_none());
}

#[test]
fn scratch_expression_adds_no_region() {
    let (doc, _) = distance_sheet();
    let before = doc.outline();
    let rev = doc.revision();
    let result = doc
        .evaluate_expression("len*dur", &CalcOptions::default())
        .unwrap();
    assert_eq!(result.status, ResultStatus::Value);
    assert_eq!(result.value, "6 m·s");
    assert_eq!(doc.outline(), before);
    assert_eq!(doc.revision(), rev);
    assert!(matches!(
        doc.evaluate_expression("1 +* 2", &CalcOptions::default()),
        Err(DocError::Invalid(_))
    ));
    // Also without a document.
    let empty = Doc::new("e");
    assert_eq!(
        empty
            .evaluate_expression("2+3", &CalcOptions::default())
            .unwrap()
            .value,
        "5"
    );
}

#[test]
fn calculation_reports_errors() {
    let mut doc = Doc::new("err");
    let id = doc.insert_math("nothere=", Placement::Auto).unwrap();
    let calc = doc.calculate(&CalcOptions::default());
    assert_eq!(calc.result(id).unwrap().status, ResultStatus::Error);
    let problems = doc.errors();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].region_id, Some(id));
    assert_eq!(problems[0].severity, "error");
}

#[test]
fn cancel_flag_stops_the_calculation() {
    let (mut doc, _) = distance_sheet();
    let cancel = Arc::new(AtomicBool::new(true));
    let calc = doc.calculate(&CalcOptions {
        cancel: Some(cancel),
        ..CalcOptions::default()
    });
    assert!(calc
        .stopped
        .as_deref()
        .is_some_and(|s| s.contains("cancelled")));
    assert!(calc
        .results
        .iter()
        .all(|r| r.status == ResultStatus::Skipped));
}

#[test]
fn deadline_of_zero_skips_every_region() {
    let (mut doc, _) = distance_sheet();
    let calc = doc.calculate(&CalcOptions {
        deadline: std::time::Duration::ZERO,
        ..CalcOptions::default()
    });
    assert!(calc.stopped.is_some());
    assert!(calc
        .results
        .iter()
        .all(|r| r.status == ResultStatus::Skipped));
}

#[test]
fn region_cap_is_enforced() {
    let mut doc = Doc::new("cap");
    for _ in 0..MAX_REGIONS {
        doc.insert_region(RegionKind::PageBreak, None, Placement::At([0.0, 0.0]))
            .unwrap();
    }
    assert_eq!(
        doc.insert_math("a:=1", Placement::Auto),
        Err(DocError::TooManyRegions)
    );
    assert_eq!(doc.outline().len(), MAX_REGIONS);
}

#[test]
fn variables_with_values() {
    let (mut doc, [d, _, v, _]) = distance_sheet();
    doc.insert_math("f(x):=x^2", Placement::Auto).unwrap();
    doc.insert_math("len:=5 m", Placement::Auto).unwrap();
    let vars = doc.variables(&CalcOptions::default());
    let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["dur", "v", "f", "len"],
        "a redefinition is listed once, at its last place"
    );
    let var = |n: &str| vars.iter().find(|v| v.name == n).unwrap();
    assert_eq!(var("len").value.as_deref(), Some("5 m"));
    assert_ne!(var("len").region_id, d);
    assert_eq!(var("v").region_id, v);
    assert_eq!(var("v").value.as_deref(), Some("1.5 m/s"));
    assert_eq!(var("f").kind, VariableKind::Function);
    assert_eq!(var("f").value, None);
    assert_eq!(var("dur").definition, "dur:=2 s");
}

#[test]
fn transcript_lists_regions_top_to_bottom_with_results() {
    let (mut doc, [_, _, _, r]) = distance_sheet();
    let first = doc.insert_text("Speed", Placement::At([0.0, 0.0])).unwrap();
    let text = doc.transcript();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], format!("[{first}] text: Speed"));
    assert_eq!(lines[4], format!("[{r}] math: v=  => 1.5 m/s"));
}

#[test]
fn save_open_round_trip_keeps_content() {
    let (mut doc, [_, _, _, r]) = distance_sheet();
    let text = doc
        .insert_text("Heading", Placement::At([0.0, 0.0]))
        .unwrap();
    doc.insert_empty_plot(false, Placement::Auto).unwrap();
    let dir = std::env::temp_dir().join(format!("mathcraft-doc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sheet.mcdx");
    assert!(doc.is_dirty());
    doc.save(Some(&path)).unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(doc.path(), Some(path.as_path()));
    doc.edit_text(text, "Other").unwrap();
    assert!(doc.is_dirty());
    doc.save(None).unwrap();

    let mut reopened = Doc::open(&path).unwrap();
    assert_eq!(reopened.outline().len(), 6);
    let RegionContent::Text { text: shown, .. } = reopened.read_region(text).unwrap().content
    else {
        panic!("not text");
    };
    assert_eq!(shown, "Other");
    let calc = reopened.calculate(&CalcOptions::default());
    assert_eq!(calc.result(r).unwrap().value, "1.5 m/s");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_without_a_path_and_open_errors() {
    let mut doc = Doc::new("x");
    assert!(matches!(doc.save(None), Err(DocError::File(_))));
    assert!(matches!(
        Doc::open(std::path::Path::new("/nonexistent/none.mcdx")),
        Err(DocError::File(_))
    ));
    assert!(matches!(
        Doc::from_bytes("junk", b"not a zip".to_vec()),
        Err(DocError::File(_))
    ));
}

/// The rich text of text region `id`, as stored.
fn stored_rich(doc: &Doc, id: u32) -> mathcraft_import::rich_text::RichText {
    let region = doc.document().region(id).expect("region");
    let RegionKind::Text(body) = &region.kind else {
        panic!("not text");
    };
    body.rich().clone()
}

/// The text "Hello world" with `start..end` bold.
fn bold_body(start: usize, end: usize) -> mathcraft_import::rich_text::TextBody {
    use mathcraft_import::rich_text::{RichText, StyleChange, TextBody};
    let rich = RichText::from_plain("Hello world").styled(start, end, &StyleChange::Bold(true));
    TextBody::from_rich(rich)
}

#[test]
fn style_only_change_through_replace_kind_persists_and_undoes() {
    let mut doc = Doc::new("style");
    let id = doc.insert_text("Hello world", Placement::Auto).unwrap();
    let plain = stored_rich(&doc, id);
    doc.replace_kind(id, RegionKind::Text(bold_body(6, 11)))
        .unwrap();
    let bold = stored_rich(&doc, id);
    assert_ne!(bold, plain);
    assert!(bold.lines[0]
        .runs
        .iter()
        .any(|r| r.style.bold && r.text == "world"));
    assert!(doc.undo().unwrap());
    assert_eq!(stored_rich(&doc, id), plain);
    assert!(doc.redo().unwrap());
    assert_eq!(stored_rich(&doc, id), bold);
}

#[test]
fn style_only_change_through_set_text_body_persists_and_undoes() {
    let mut doc = Doc::new("style");
    let id = doc.insert_text("Hello world", Placement::Auto).unwrap();
    let plain = stored_rich(&doc, id);
    doc.set_text_body(id, bold_body(0, 5)).unwrap();
    let bold = stored_rich(&doc, id);
    assert_ne!(bold, plain);
    assert!(bold.lines[0]
        .runs
        .iter()
        .any(|r| r.style.bold && r.text == "Hello"));
    assert!(doc.undo().unwrap());
    assert_eq!(stored_rich(&doc, id), plain);
}

#[test]
fn identical_text_body_is_still_a_no_op() {
    let mut doc = Doc::new("same");
    let id = doc.insert_text("Hello world", Placement::Auto).unwrap();
    doc.set_text_body(id, bold_body(6, 11)).unwrap();
    let revision = doc.revision();
    let bold = stored_rich(&doc, id);
    // The same body again: no change, no revision bump, no new undo step.
    doc.set_text_body(id, bold_body(6, 11)).unwrap();
    assert_eq!(doc.revision(), revision);
    assert_eq!(stored_rich(&doc, id), bold);
    // The undo step is the bold change itself, so one undo restores the plain text.
    assert!(doc.undo().unwrap());
    assert!(!stored_rich(&doc, id).lines[0]
        .runs
        .iter()
        .any(|r| r.style.bold));
}

#[test]
fn subscripted_names_save_reopen_and_list_in_full() {
    let mut doc = Doc::new("sub");
    for t in [
        "range_m:=2 m",
        "g_acc:=9.81 m/s^2",
        "x_1:=3",
        "v_max:=5 m/s",
        "A_b:=2",
        "t_s:=g_acc*range_m",
    ] {
        doc.insert_math(t, Placement::Auto).unwrap();
    }
    let opts = CalcOptions::default();
    let before = doc.variables(&opts);
    let names: Vec<_> = before.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["range_m", "g_acc", "x_1", "v_max", "A_b", "t_s"]);
    let bytes = doc.to_bytes().expect("subscripted names are writable");
    let mut back = Doc::from_bytes("sub", bytes).unwrap();
    let after = back.variables(&opts);
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert_eq!((&a.name, &a.value), (&b.name, &b.value));
    }
    assert!(after.iter().all(|v| v.value.is_some()));
}

fn value_of_region(doc: &mut Doc, id: u32) -> String {
    let calc = doc.calculate(&CalcOptions::default());
    let r = calc
        .result(id)
        .unwrap_or_else(|| panic!("no result for {id}"));
    assert_ne!(r.status, ResultStatus::Error, "{r:?}");
    r.value.clone()
}

#[test]
fn solve_block_finds_a_root_and_saves() {
    use crate::doc::SolveBlockSpec;
    let mut doc = Doc::new("blocks");
    let spec = SolveBlockSpec {
        guesses: vec!["x:=1".into(), "y:=1".into()],
        constraints: vec!["x+y=3".into(), "x-y=1".into()],
        solver: "find(x, y)".into(),
        target: None,
    };
    let ids = doc.insert_solve_block(&spec, Placement::Auto).unwrap();
    assert_eq!(doc.steps_executed(), 1, "one undo step");
    let v = value_of_region(&mut doc, ids.solver);
    assert_eq!(v, "[[2], [1]]");
    let bytes = doc.to_bytes().expect("a solve block is writable");
    let back = Doc::from_bytes("back", bytes).unwrap();
    assert_eq!(back.regions().len(), 1);
    assert_eq!(back.regions()[0].children.len(), 5);
}

#[test]
fn solve_block_rejects_bad_members() {
    use crate::doc::SolveBlockSpec;
    let mut doc = Doc::new("blocks");
    let mut spec = SolveBlockSpec {
        guesses: vec!["x".into()],
        constraints: vec![],
        solver: "find(x)".into(),
        target: None,
    };
    assert!(matches!(
        doc.insert_solve_block(&spec, Placement::Auto),
        Err(DocError::Invalid(m)) if m.contains("guess")
    ));
    spec.guesses = vec!["x:=1".into()];
    spec.solver = "sin(x)".into();
    assert!(matches!(
        doc.insert_solve_block(&spec, Placement::Auto),
        Err(DocError::Invalid(m)) if m.contains("find")
    ));
    assert!(doc.regions().is_empty(), "a refused block leaves nothing");
}

#[test]
fn program_runs_and_is_not_writable_yet() {
    let mut doc = Doc::new("prog");
    let lines: Vec<String> = "acc ← 1\nfor k ∈ 1..n\n  acc ← acc*k\nreturn acc"
        .lines()
        .map(str::to_owned)
        .collect();
    doc.insert_program("fact", &["n".to_owned()], &lines, Placement::Auto)
        .unwrap();
    let call = doc.insert_math("fact(5)=", Placement::Auto).unwrap();
    assert_eq!(value_of_region(&mut doc, call), "120");
    assert!(doc.to_bytes().is_err(), "programs have no file form yet");
}

#[test]
fn check_saveable_refuses_a_program_and_names_its_region() {
    let mut doc = Doc::new("plain");
    doc.insert_math("x := 2", Placement::Auto).unwrap();
    assert!(doc.check_saveable().is_ok(), "a plain sheet is saveable");

    let lines: Vec<String> = "return n".lines().map(str::to_owned).collect();
    let id = doc
        .insert_program("id1", &["n".to_owned()], &lines, Placement::Auto)
        .unwrap();
    match doc.check_saveable() {
        Err(DocError::File(m)) => {
            assert!(m.contains("programming regions are not written yet"), "{m}");
            assert!(
                m.contains(&format!("region {id}")),
                "{m} should name region {id}"
            );
        }
        other => panic!("expected the writer's refusal, got {other:?}"),
    }
}

#[test]
fn normalise_equation_leaves_string_contents_alone() {
    use crate::doc::normalise_equation;
    assert_eq!(normalise_equation("x=1"), "x==1");
    assert_eq!(normalise_equation("x:=1"), "x:=1");
    assert_eq!(normalise_equation("x<=1"), "x<=1");
    assert_eq!(normalise_equation("x==1"), "x==1");
    assert_eq!(normalise_equation(r#""a=b"=="a=b""#), r#""a=b"=="a=b""#);
    assert_eq!(normalise_equation(r#""a=b"="c""#), r#""a=b"=="c""#);
    // Call arguments are still converted: `if(a=b, ...)` means a comparison.
    assert_eq!(normalise_equation("f(x=1)"), "f(x==1)");
}

#[test]
fn auto_placement_leaves_room_for_tall_regions_and_results() {
    use crate::extent::region_height;
    let mut doc = Doc::new("tall");
    doc.insert_math("A:=[1,2,3;4,5,6;7,8,9]", Placement::Auto)
        .unwrap();
    doc.insert_math("A=", Placement::Auto).unwrap();
    doc.insert_math("s:=sum(k,k,1,10)", Placement::Auto)
        .unwrap();
    doc.insert_math("f:=(1+1/2)/(3+4/5)", Placement::Auto)
        .unwrap();
    doc.insert_math("z:=1", Placement::Auto).unwrap();
    let regions = doc.document().regions();
    for pair in regions.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let bottom = a.pos.unwrap().1 + region_height(a, regions);
        assert!(
            b.pos.unwrap().1 >= bottom,
            "region {} overlaps region {}",
            b.id,
            a.id
        );
    }
    assert!(region_height(&regions[0], regions) >= 72.0);
    assert!(region_height(&regions[1], regions) >= 72.0);
    assert!(regions[2].pos.unwrap().1 - regions[1].pos.unwrap().1 >= 72.0);
}

#[test]
fn auto_placement_of_thousands_of_evaluations_is_fast_and_does_not_overlap() {
    use crate::extent::region_height;
    let mut doc = Doc::new("bulk");
    // Each evaluation names a matrix defined far above it, the case whose definition lookup scans
    // every region. The text is read with an empty context, so the time is the placement's: the
    // identifier context of the whole sheet is a separate cost of every typed insert.
    let no_context = crate::context::EditContext::default();
    let start = std::time::Instant::now();
    for i in 0..1500 {
        for text in [format!("M{i}:=[1,2;3,4]"), format!("M{i}=")] {
            let expr = crate::context::parse_math_in(&text, &no_context).unwrap();
            doc.insert_region(RegionKind::Math(expr), None, Placement::Auto)
                .unwrap();
        }
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs_f64() < 3.0,
        "3000 Auto inserts took {elapsed:?}"
    );
    let regions = doc.document().regions();
    assert_eq!(regions.len(), 3000);
    for pair in regions.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let bottom = a.pos.unwrap().1 + region_height(a, regions);
        assert!(
            b.pos.unwrap().1 >= bottom,
            "region {} overlaps {}",
            b.id,
            a.id
        );
    }
    let brute = regions
        .iter()
        .filter_map(|r| r.pos.map(|(_, y)| y + region_height(r, regions)))
        .fold(None, |acc: Option<f64>, b| {
            Some(acc.map_or(b, |a| a.max(b)))
        });
    assert_eq!(crate::extent::lowest_bottom(regions), brute);
}

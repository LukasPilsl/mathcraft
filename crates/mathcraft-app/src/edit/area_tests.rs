//! Session tests for areas and images: inserting, expanding and collapsing, locked areas, inserting
//! images from bytes, putting regions in an area, locking, and resizing an image.

use std::collections::BTreeMap;

use mathcraft_import::regions::{
    AreaModel, ImageModel, ImportReport, RegionKind, RegionModel, Settings, Worksheet,
};
use mathcraft_import::results::ResultTable;

use super::document::Document;
use super::session::EditSession;

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

fn area(id: u32, pos: (f64, f64), area: AreaModel) -> RegionModel {
    RegionModel {
        id,
        pos: Some(pos),
        size: Some((300.0, 150.0)),
        result_ref: None,
        kind: RegionKind::Area(area),
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
}

fn area_of(s: &EditSession, id: u32) -> AreaModel {
    match &s.document().region(id).expect("region").kind {
        RegionKind::Area(a) => *a,
        other => panic!("not an area: {other:?}"),
    }
}

/// A small PNG made here, so the tests need no sample file.
fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(661, 748, image::Rgba([10, 20, 30, 255]));
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("encodes");
    out
}

#[test]
fn insert_area_adds_an_empty_expanded_area_and_selects_it() {
    let mut s = session_with(Vec::new());
    s.insert_area([40.0, 60.0]);
    assert_eq!(s.document().regions().len(), 1);
    let region = &s.document().regions()[0];
    assert_eq!(region.pos, Some((40.0, 60.0)));
    assert_eq!(region.size, Some((360.0, 120.0)));
    assert_eq!(area_of(&s, region.id), AreaModel::default());
    assert_eq!(s.selected(), &[region.id]);
}

#[test]
fn toggle_collapses_and_expands_and_undo_restores_it() {
    let mut s = session_with(vec![area(1, (0.0, 0.0), AreaModel::default())]);
    assert!(s.toggle_area(1));
    assert!(area_of(&s, 1).collapsed);
    assert!(s.toggle_area(1));
    assert!(!area_of(&s, 1).collapsed);
    assert!(s.undo());
    assert!(area_of(&s, 1).collapsed);
}

#[test]
fn a_locked_area_does_not_toggle_and_says_why() {
    let locked = AreaModel {
        locked: true,
        ..AreaModel::default()
    };
    let mut s = session_with(vec![area(1, (0.0, 0.0), locked)]);
    assert!(s.toggle_area(1));
    assert!(!area_of(&s, 1).collapsed);
    assert!(s.message().is_some());
    assert!(!s.can_undo());
}

#[test]
fn a_locked_area_is_not_moved_or_deleted() {
    let locked = AreaModel {
        locked: true,
        ..AreaModel::default()
    };
    let mut s = session_with(vec![area(1, (0.0, 0.0), locked)]);
    s.click([5.0, 5.0], Some(1));
    s.nudge(10.0, 0.0);
    assert_eq!(s.document().region(1).unwrap().pos, Some((0.0, 0.0)));
    s.delete_selected();
    assert!(s.document().region(1).is_some());
}

#[test]
fn toggle_of_a_missing_region_is_refused() {
    let mut s = session_with(Vec::new());
    s.insert_area([0.0, 0.0]);
    let id = s.document().regions()[0].id;
    assert!(!s.toggle_area(id + 100));
    assert!(s.toggle_area(id));
}

#[test]
fn insert_image_sizes_from_pixels_and_inserts_one_region() {
    let mut s = session_with(Vec::new());
    s.insert_image(png(), [10.0, 10.0]).expect("inserts");
    let region = &s.document().regions()[0];
    // The image is 661 x 748 pixels: 0.75 pt each, scaled to 400 pt wide.
    let (w, h) = region.size.expect("sized");
    assert!((w - 400.0).abs() < 1e-9);
    assert!((h - 748.0 * 0.75 * 400.0 / (661.0 * 0.75)).abs() < 1e-6);
    assert!(matches!(region.kind, RegionKind::Image(_)));
}

#[test]
fn insert_image_refuses_other_formats_and_large_files() {
    let mut s = session_with(Vec::new());
    assert!(s.insert_image(b"GIF89a....".to_vec(), [0.0, 0.0]).is_err());
    assert!(s
        .insert_image(b"not an image".to_vec(), [0.0, 0.0])
        .is_err());
    let big = {
        let mut bytes = png();
        bytes.resize(super::session::MAX_IMAGE_BYTES + 1, 0);
        bytes
    };
    assert!(s.insert_image(big, [0.0, 0.0]).is_err());
    assert!(s.document().regions().is_empty());
}

fn image_region(id: u32, pos: (f64, f64), size: (f64, f64)) -> RegionModel {
    RegionModel {
        id,
        pos: Some(pos),
        size: Some(size),
        result_ref: None,
        kind: RegionKind::Image(ImageModel {
            item: String::new(),
            bytes: std::sync::Arc::from(png()),
        }),
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
}

fn top_ids(s: &EditSession) -> Vec<u32> {
    s.document().regions().iter().map(|r| r.id).collect()
}

#[test]
fn insert_area_needs_an_insertion_point() {
    let mut s = session_with(Vec::new());
    assert!(!s.insert_area_at_crosshair());
    assert!(s.document().regions().is_empty());
    assert!(s.message().is_some());
    s.click([70.0, 80.0], None);
    assert!(s.insert_area_at_crosshair());
    assert_eq!(s.document().regions()[0].pos, Some((70.0, 80.0)));
}

#[test]
fn insert_image_at_the_insertion_point_and_refused_without_one() {
    let mut s = session_with(Vec::new());
    assert!(s.insert_image_at_crosshair(png()).is_err());
    assert!(s.document().regions().is_empty());
    s.click([30.0, 40.0], None);
    s.insert_image_at_crosshair(png()).expect("inserts");
    assert_eq!(s.document().regions()[0].pos, Some((30.0, 40.0)));
    assert!(s.undo());
    assert!(s.document().regions().is_empty());
}

#[test]
fn area_around_selection_wraps_the_members_and_undo_restores_them() {
    let mut s = session_with(vec![
        image_region(1, (100.0, 100.0), (50.0, 40.0)),
        image_region(2, (50.0, 300.0), (60.0, 20.0)),
        image_region(3, (500.0, 500.0), (10.0, 10.0)),
    ]);
    s.click([110.0, 110.0], Some(1));
    s.toggle_selection(Some(2));
    assert!(s.area_around_selection());
    assert_eq!(top_ids(&s).len(), 2);
    let area_id = s.document().regions()[0].id;
    assert_eq!(s.selected(), &[area_id]);
    let area = s.document().region(area_id).expect("area");
    assert!(matches!(area.kind, RegionKind::Area(_)));
    // The members keep their own positions.
    let kids: Vec<(u32, Option<(f64, f64)>)> =
        area.children.iter().map(|r| (r.id, r.pos)).collect();
    assert_eq!(
        kids,
        vec![(1, Some((100.0, 100.0))), (2, Some((50.0, 300.0)))]
    );
    // The box covers both members with the margin, and the header bar above.
    let (x, y) = area.pos.expect("pos");
    let (w, h) = area.size.expect("size");
    assert!((x - 42.0).abs() < 1e-9);
    assert!((y - (100.0 - 8.0 - 18.0)).abs() < 1e-9);
    assert!((x + w - 158.0).abs() < 1e-9);
    assert!((y + h - (320.0 + 8.0)).abs() < 1e-9);
    assert!(s.undo());
    assert_eq!(top_ids(&s), vec![1, 2, 3]);
    assert_eq!(s.document().region(2).unwrap().pos, Some((50.0, 300.0)));
}

#[test]
fn area_around_selection_needs_a_selection() {
    let mut s = session_with(vec![image_region(1, (0.0, 0.0), (10.0, 10.0))]);
    assert!(!s.area_around_selection());
    assert!(s.message().is_some());
    assert_eq!(top_ids(&s), vec![1]);
}

#[test]
fn a_locked_area_cannot_go_into_another_area() {
    let locked = AreaModel {
        locked: true,
        ..AreaModel::default()
    };
    let mut s = session_with(vec![area(1, (0.0, 0.0), locked)]);
    s.click([5.0, 5.0], Some(1));
    assert!(!s.area_around_selection());
    assert_eq!(top_ids(&s), vec![1]);
}

#[test]
fn lock_toggles_and_a_locked_area_refuses_collapse() {
    let mut s = session_with(vec![area(1, (0.0, 0.0), AreaModel::default())]);
    assert!(s.toggle_area_lock(1));
    assert!(area_of(&s, 1).locked);
    assert!(s.toggle_area(1));
    assert!(!area_of(&s, 1).collapsed);
    assert!(s.toggle_area_lock(1));
    assert!(!area_of(&s, 1).locked);
    assert!(s.undo());
    assert!(area_of(&s, 1).locked);
    assert!(!s.toggle_area_lock(9));
}

#[test]
fn dragging_a_corner_resizes_the_image_as_one_undo_step() {
    use super::resize::Corner;
    let mut s = session_with(vec![image_region(1, (100.0, 100.0), (200.0, 100.0))]);
    s.click([150.0, 150.0], Some(1));
    assert_eq!(s.image_frame(), Some((1, [100.0, 100.0, 200.0, 100.0])));
    assert!(s.begin_resize(Corner::BottomRight));
    s.drag_to([50.0, 0.0]);
    assert_eq!(s.image_frame(), Some((1, [100.0, 100.0, 250.0, 125.0])));
    s.end_drag();
    let region = s.document().region(1).expect("image");
    assert_eq!(region.pos, Some((100.0, 100.0)));
    assert_eq!(region.size, Some((250.0, 125.0)));
    assert!(s.undo());
    assert_eq!(s.document().region(1).unwrap().size, Some((200.0, 100.0)));
}

#[test]
fn dragging_the_top_left_corner_moves_the_position() {
    use super::resize::Corner;
    let mut s = session_with(vec![image_region(1, (100.0, 100.0), (200.0, 100.0))]);
    s.click([150.0, 150.0], Some(1));
    assert!(s.begin_resize(Corner::TopLeft));
    s.drag_to([-40.0, 0.0]);
    s.end_drag();
    let region = s.document().region(1).expect("image");
    assert_eq!(region.size, Some((240.0, 120.0)));
    let (x, y) = region.pos.expect("pos");
    // The bottom-right corner stays at (300, 200).
    assert!((x + 240.0 - 300.0).abs() < 1e-9);
    assert!((y + 120.0 - 200.0).abs() < 1e-9);
}

#[test]
fn resize_needs_one_selected_image() {
    use super::resize::Corner;
    let mut s = session_with(vec![
        area(1, (0.0, 0.0), AreaModel::default()),
        image_region(2, (100.0, 0.0), (50.0, 50.0)),
    ]);
    assert!(!s.begin_resize(Corner::TopLeft));
    s.click([5.0, 5.0], Some(1));
    assert!(!s.begin_resize(Corner::TopLeft));
    assert_eq!(s.image_frame(), None);
    s.click([110.0, 5.0], Some(2));
    assert!(s.begin_resize(Corner::TopLeft));
}

#[test]
fn delete_and_backspace_remove_a_selected_region_of_every_kind() {
    use super::session::EditKey;
    for key in [EditKey::Delete, EditKey::Backspace] {
        for kind in ["area", "plot", "image", "math", "text"] {
            let mut s = session_with(Vec::new());
            s.click([20.0, 20.0], None);
            match kind {
                "area" => assert!(s.insert_area_at_crosshair()),
                "plot" => assert!(s.insert_plot_at_crosshair(false)),
                "image" => s.insert_image_at_crosshair(png()).expect("image"),
                "math" => {
                    s.type_char('x');
                    s.key(EditKey::Enter);
                }
                _ => {
                    assert!(s.start_text());
                    s.type_char('a');
                    s.key(EditKey::Escape);
                }
            }
            let id = s.document().regions().last().expect("region").id;
            s.click([25.0, 25.0], Some(id));
            assert_eq!(s.selected(), &[id], "{kind} selected");
            s.key(key);
            assert!(
                s.document().region(id).is_none(),
                "{kind} deleted by {key:?}"
            );
            assert!(s.undo());
            assert!(s.document().region(id).is_some(), "{kind} restored");
        }
    }
}

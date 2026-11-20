//! Page export over the sample worksheets: every sample exports to a structurally valid PDF with
//! the page count the geometry gives, the output is deterministic, and the page limit holds.

use std::path::PathBuf;

use mathcraft_import::page::PageSettings;

use crate::export::export;
use crate::load::load_file;
use crate::pages::MAX_PAGES;
use crate::pdf::tests::check_xref;
use crate::testdata::{examples_dir, sample};
use crate::viewmodel::ViewModel;

/// Every `.mcdx` under the examples directory, and the operations sample.
fn all_samples() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(examples_dir())
        .expect("examples directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    paths.push(examples_dir().join("operations/Unbenannt.mcdx"));
    paths.sort();
    paths
}

fn load(path: &std::path::Path) -> ViewModel {
    load_file(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn every_sample_exports_a_valid_pdf() {
    require_samples!();
    let samples = all_samples();
    assert!(samples.len() >= 25, "24 samples and the operations sample");
    for path in &samples {
        let vm = load(path);
        let title = path.file_name().unwrap().to_string_lossy().to_string();
        let result = export(&vm, &PageSettings::default(), false, &title)
            .unwrap_or_else(|e| panic!("{title}: {e}"));
        let entries = check_xref(&result.pdf);
        assert!(
            entries >= 6,
            "{title}: objects in the cross-reference table"
        );
        let text = String::from_utf8_lossy(&result.pdf);
        assert!(text.starts_with("%PDF-1.4"), "{title}: header");
        assert!(
            text.contains(&format!("/Count {}", result.pages)),
            "{title}: page count {}",
            result.pages
        );
        assert!(result.pages >= 1 && result.pages <= MAX_PAGES, "{title}");
    }
}

#[test]
fn samples_with_text_embed_their_fonts() {
    require_samples!();
    let vm = load(&sample("Mechanik_1_massen_ziehen.mcdx"));
    let result = export(&vm, &PageSettings::default(), false, "M1").expect("exports");
    let text = String::from_utf8_lossy(&result.pdf);
    assert!(text.contains("/FontFile2 "), "font file embedded");
    assert!(text.contains("/ToUnicode "), "text map present");
}

#[test]
fn export_is_deterministic() {
    require_samples!();
    let vm = load(&sample("Mechanik_1_massen_ziehen.mcdx"));
    let a = export(&vm, &PageSettings::default(), false, "M1").expect("exports");
    let b = export(&vm, &PageSettings::default(), false, "M1").expect("exports");
    assert_eq!(a.pdf, b.pdf);
}

#[test]
fn landscape_letter_sets_the_media_box() {
    require_samples!();
    let vm = load(&sample("Mechanik_1_massen_ziehen.mcdx"));
    let page = PageSettings {
        paper_code: "Letter".to_owned(),
        orientation: "Landscape".to_owned(),
        ..PageSettings::default()
    };
    let result = export(&vm, &page, false, "M1").expect("exports");
    let text = String::from_utf8_lossy(&result.pdf);
    assert!(text.contains("/MediaBox [0 0 792 612]"), "landscape letter");
}

#[test]
fn a_huge_worksheet_is_capped_at_the_page_limit() {
    require_samples!();
    let mut vm = load(&sample("Mechanik_1_massen_ziehen.mcdx"));
    let mut far = vm.items[0].clone();
    far.rect[1] = 1.0e9;
    vm.items.push(far);
    let result = export(&vm, &PageSettings::default(), false, "huge").expect("exports");
    assert_eq!(result.pages, MAX_PAGES);
    assert!(result.capped);
    assert!(result.pdf.len() < crate::pdf::MAX_BYTES);
}

#[test]
fn a_crossing_region_appears_on_both_pages() {
    require_samples!();
    let mut vm = load(&sample("Mechanik_1_massen_ziehen.mcdx"));
    vm.items.truncate(1);
    let h = PageSettings::default().paper_points()[1] as f32;
    let mut item = vm.items[0].clone();
    item.rect[1] = h - 5.0;
    item.rect[3] = 40.0;
    vm.items[0] = item;
    let result = export(&vm, &PageSettings::default(), false, "cross").expect("exports");
    assert_eq!(result.pages, 2);
    let text = String::from_utf8_lossy(&result.pdf);
    assert_eq!(text.matches("/Type /Page /").count(), 2, "two page objects");
}

/// A worksheet with no regions but one tall placeholder, so it runs to two pages.
fn two_page_vm() -> ViewModel {
    use crate::viewmodel::{Body, Item};
    ViewModel {
        title: "Beam".to_owned(),
        path: String::new(),
        items: vec![Item {
            region_id: 1,
            rect: [0.0, 900.0, 10.0, 10.0],
            body: Body::Placeholder {
                label: "x".to_owned(),
                detail: String::new(),
            },
        }],
        page: [595.0, 842.0],
        notes: Vec::new(),
        counts: Default::default(),
        furniture: Vec::new(),
        page_breaks: Vec::new(),
        breaks: Vec::new(),
        frame: crate::pages::PageFrame::default(),
    }
}

fn text_ops(page: &crate::pdf::Page) -> usize {
    page.ops
        .iter()
        .filter(|op| matches!(op, crate::pdf::Op::Text { .. }))
        .count()
}

#[test]
fn header_and_footer_are_drawn_on_every_page() {
    use crate::viewmodel::Furniture;
    use mathcraft_import::rich_text::TextBody;
    let plain = crate::export::paginate(&two_page_vm(), &PageSettings::default(), false).1;
    let mut vm = two_page_vm();
    vm.furniture.push(Furniture {
        footer: false,
        rect: [50.0, 20.0, 300.0, 14.0],
        text: TextBody::from_plain("{filename}"),
    });
    vm.furniture.push(Furniture {
        footer: true,
        rect: [50.0, 800.0, 300.0, 14.0],
        text: TextBody::from_plain("{page}/{pages}"),
    });
    let (geometry, pages, _) = crate::export::paginate(&vm, &PageSettings::default(), false);
    assert_eq!(geometry.count, 2);
    for (with, without) in pages.iter().zip(&plain) {
        assert_eq!(text_ops(with) - text_ops(without), 2, "header and footer");
    }
}

#[test]
fn content_below_a_page_break_is_paginated_from_the_next_page() {
    use crate::pages::{break_layout, PageFrame};
    // The item sits at sheet y 100 and the break at y 50, above it. The break moves the item to
    // the top of page 2 (plus its 50 pt below the break), so the PDF has two pages, where a sheet
    // cut only at the printable height would have one. Pages are cut at the printable height.
    let page = PageSettings::default();
    let frame = PageFrame::new(&page);
    let breaks = break_layout(&[50.0], frame.printable_h());
    let mut vm = two_page_vm();
    vm.items[0].rect[1] = frame.to_frame(100.0 + breaks[0].shift);
    vm.page_breaks = vec![frame.to_frame(breaks[0].marker)];
    vm.breaks = breaks;
    vm.frame = frame;
    let (geometry, pages, _) = crate::export::paginate(&vm, &page, false);
    assert_eq!(geometry.count, 2);
    assert_eq!(pages.len(), 2);
    assert!(!pages[1].ops.is_empty(), "the item is drawn on page 2");
}

#[test]
fn stored_origin_is_drawn_inside_the_margins() {
    // An item at the frame origin is drawn at the paper point of the margin box corner, so no
    // operation of the page starts left of or above the left and top margins.
    let page = PageSettings::default();
    let frame = crate::pages::PageFrame::new(&page);
    let mut vm = two_page_vm();
    vm.items[0].rect = [0.0, 0.0, 10.0, 10.0];
    vm.frame = frame;
    let (_, pages, _) = crate::export::paginate(&vm, &page, false);
    let (left, top) = (frame.margins[3], frame.margins[0]);
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for op in &pages[0].ops {
        match op {
            crate::pdf::Op::Rect { x, y, .. } => {
                xs.push(*x);
                ys.push(*y);
            }
            crate::pdf::Op::Line { points, .. } => {
                xs.extend(points.iter().map(|p| p[0]));
                ys.extend(points.iter().map(|p| p[1]));
            }
            _ => {}
        }
    }
    assert!(!xs.is_empty(), "the placeholder is drawn");
    let min_x = xs.iter().copied().fold(f32::INFINITY, f32::min);
    let min_y = ys.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(
        min_x >= left - 1e-3,
        "x {min_x} is left of the margin {left}"
    );
    assert!(min_y >= top - 1e-3, "y {min_y} is above the margin {top}");
}

#[test]
fn page_break_marker_is_a_dashed_rule_on_its_page() {
    let mut vm = two_page_vm();
    vm.page_breaks.push(300.0);
    let (_, pages, _) = crate::export::paginate(&vm, &PageSettings::default(), false);
    let rules = |page: &crate::pdf::Page| -> Vec<f32> {
        page.ops
            .iter()
            .filter_map(|op| match op {
                crate::pdf::Op::Line { points, .. } => Some(points[0][1]),
                _ => None,
            })
            .collect()
    };
    // The marker is at frame y 300; on the page it is drawn at that plus the top margin.
    let rule_y = 300.0 + crate::pages::PageFrame::default().margins[0];
    // Other lines (the placeholder's outline) may be on the page too, so count the rule's own.
    let on_rule = |page: &crate::pdf::Page| {
        rules(page)
            .iter()
            .filter(|y| (**y - rule_y).abs() < 1e-3)
            .count()
    };
    assert!(on_rule(&pages[0]) > 1, "dashes are several segments");
    assert_eq!(on_rule(&pages[1]), 0, "the rule is on its own page only");
}

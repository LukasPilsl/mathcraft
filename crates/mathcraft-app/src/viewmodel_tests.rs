use eframe::egui::{self, FontId};

use mathcraft_import::compare::{classify, Category};
use mathcraft_import::evaluate::{evaluate_worksheet, load_results, on_big_stack};
use mathcraft_import::package::Package;
use mathcraft_import::regions::{RegionKind, RegionModel, SolveBlock, Worksheet};

use super::{absolute_positions, Body, Counts, Tone, PAGE};
use crate::fonts::{covers, Face};
use crate::load::load_file;
use crate::testdata::{examples_dir, sample};

/// Count per golden category, in the order of the CLI table: Pass (weak passes included),
/// Mismatch, Unsupported, EngineError, EngineErrorNoRef, NoReference, PlotOk.
pub(crate) type Row = [usize; 7];

/// The golden row of one sample, computed with the shared evaluation and classification, as
/// `mathcraft-cli golden` computes it.
fn golden_row(name: &str) -> Row {
    let path = sample(name);
    on_big_stack(move || {
        let pkg = Package::open(&path).expect("opens");
        let ws = Worksheet::from_package(&pkg).expect("imports");
        let table = load_results(&pkg).expect("results");
        let evaluation = evaluate_worksheet(&ws, &table);
        let mut row = [0; 7];
        for eval in &evaluation.regions {
            let slot = match classify(eval) {
                Category::Pass | Category::WeakPass(_) => 0,
                Category::Mismatch(_) => 1,
                Category::Unsupported(_) => 2,
                Category::EngineError(_) => 3,
                Category::EngineErrorNoRef(_) => 4,
                Category::NoReference => 5,
                Category::PlotOk(_) => 6,
            };
            row[slot] += 1;
        }
        row
    })
}

/// The row of a view model's counts, in the same order as [`golden_row`].
pub(crate) fn counts_row(c: &Counts) -> Row {
    [
        c.pass,
        c.mismatch,
        c.unsupported.values().sum(),
        c.engine_error,
        c.engine_error_no_ref,
        c.no_reference,
        c.plot_ok,
    ]
}

/// Checks that the shared classification and the view model both give `expected` for `name`.
fn assert_sample_row(name: &str, expected: Row) {
    assert_eq!(golden_row(name), expected, "{name}: shared classification");
    let model = load_file(&sample(name)).expect("loads");
    assert_eq!(counts_row(&model.counts), expected, "{name}: view model");
}

/// Golden table of Mechanik_1 from `mathcraft-cli golden --verbose`: Pass 4, Mismatch 0,
/// Unsupported 0, EngineError 0, EngineErrorNoRef 10, NoReference 10, PlotOk 0.
#[test]
fn mechanik_1_counts_match_the_golden_table() {
    require_samples!();
    assert_sample_row("Mechanik_1_massen_ziehen.mcdx", [4, 0, 0, 0, 10, 10, 0]);
    let model = load_file(&sample("Mechanik_1_massen_ziehen.mcdx")).expect("loads");
    assert_eq!(model.counts.weak_pass, 0);
    assert!(model.notes.is_empty(), "{:?}", model.notes);
}

/// Every evaluated 2D plot is drawn as a plot body at its stored position and size. The
/// 3D plots of 13 and 14 are drawn as 3D plot bodies.
#[test]
fn plots_are_drawn_at_their_stored_position_and_size() {
    require_samples!();
    let path = sample("12_rata_Kreisdiagramme.mcdx");
    let model = load_file(&path).expect("loads");
    let pkg = Package::open(&path).expect("opens");
    let ws = Worksheet::from_package(&pkg).expect("imports");
    let positions = absolute_positions(&ws.regions);
    let mut drawn = 0;
    for region in ws.all_regions() {
        if !matches!(region.kind, RegionKind::Plot(_)) {
            continue;
        }
        let item = model
            .items
            .iter()
            .find(|i| i.region_id == region.id)
            .expect("a plot has an item");
        assert!(matches!(item.body, Body::Plot(_)), "region {}", region.id);
        let (left, top) = positions[&region.id].expect("stored position");
        let (width, height) = region.size.expect("stored size");
        assert_eq!(
            item.rect,
            [left as f32, top as f32, width as f32, height as f32],
            "region {}",
            region.id
        );
        drawn += 1;
    }
    assert_eq!(drawn, 3, "the three plots of sheet 12");
    for path in [sample("13_rata_plot1.mcdx"), sample("14_rata_plot2.mcdx")] {
        let model = load_file(&path).expect("loads");
        let drawn = model
            .items
            .iter()
            .filter(|item| matches!(&item.body, Body::Plot3d(_)))
            .count();
        assert_eq!(drawn, 1, "{path:?}: one 3D plot per sheet");
    }
}

/// Golden row of 17_rata_schiefer_wurf.mcdx (a solve block): Pass 4, Mismatch 0, Unsupported 0,
/// EngineError 0, EngineErrorNoRef 0, NoReference 7, PlotOk 0 (its two plots match Prime's
/// stored traces).
#[test]
fn solve_block_sample_17_schiefer_wurf_counts_match_the_golden_table() {
    require_samples!();
    assert_sample_row("17_rata_schiefer_wurf.mcdx", [4, 0, 0, 0, 0, 7, 0]);
}

/// Golden row of 5_rata_4_Gerade_schneidet_Kreis.mcdx (a solve block): Pass 3, Mismatch 0,
/// Unsupported 0, EngineError 0, EngineErrorNoRef 0, NoReference 10, PlotOk 0. Its plot passes:
/// the complex values of its traces are gaps, as Prime stores NaN there.
#[test]
fn solve_block_sample_5_gerade_schneidet_kreis_counts_match_the_golden_table() {
    require_samples!();
    assert_sample_row(
        "5_rata_4_Gerade_schneidet_Kreis.mcdx",
        [3, 0, 0, 0, 0, 10, 0],
    );
}

/// Golden row of 17_rata_golfball_donau_lsg_2.mcdx, which has the sheet's only mismatch: Pass 4,
/// Mismatch 1, Unsupported 0, EngineError 0, EngineErrorNoRef 0, NoReference 6, PlotOk 0.
#[test]
fn mismatch_sample_17_golfball_counts_match_the_golden_table() {
    require_samples!();
    assert_sample_row("17_rata_golfball_donau_lsg_2.mcdx", [4, 1, 0, 0, 0, 6, 0]);
    let model = load_file(&sample("17_rata_golfball_donau_lsg_2.mcdx")).expect("loads");
    let problems = model
        .items
        .iter()
        .filter_map(|item| match &item.body {
            Body::Math {
                result: Some(line), ..
            } => Some(line.tone),
            _ => None,
        })
        .filter(|tone| *tone == Tone::Problem)
        .count();
    assert_eq!(problems, 1, "the mismatch is drawn as a problem");
}

#[test]
fn solve_block_members_are_drawn_like_regions() {
    require_samples!();
    let model = load_file(&sample("17_rata_schiefer_wurf.mcdx")).expect("loads");
    assert!(
        !model
            .counts
            .unsupported
            .keys()
            .any(|r| r.contains("solve block member")),
        "{:?}",
        model.counts.unsupported
    );
    let path = sample("17_rata_schiefer_wurf.mcdx");
    let ws = on_big_stack(move || {
        let pkg = Package::open(&path).expect("opens");
        Worksheet::from_package(&pkg).expect("imports")
    });
    let block = ws
        .all_regions()
        .into_iter()
        .find(|r| matches!(r.kind, RegionKind::SolveBlock(_)))
        .expect("the sample has a solve block");
    let block_pos = block.pos.expect("the block has a position");
    for member in &block.children {
        let Some(item) = model.items.iter().find(|i| i.region_id == member.id) else {
            panic!("member {} has no item", member.id);
        };
        if let Some((x, y)) = member.pos {
            let expected = [(block_pos.0 + x) as f32, (block_pos.1 + y) as f32];
            assert!(
                (item.rect[0] - expected[0]).abs() < 1e-3,
                "member {}",
                member.id
            );
            assert!(
                (item.rect[1] - expected[1]).abs() < 1e-3,
                "member {}",
                member.id
            );
        }
    }
}

#[test]
fn solve_block_member_of_a_block_without_position_has_no_absolute_position() {
    let member = RegionModel {
        id: 2,
        pos: Some((5.0, 5.0)),
        size: None,
        result_ref: None,
        kind: RegionKind::Math(mathcraft_core::expr::Expr::var("x")),
        solve_category: Some("guess-value".to_string()),
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    };
    let block = RegionModel {
        id: 1,
        pos: None,
        size: None,
        result_ref: None,
        kind: RegionKind::SolveBlock(SolveBlock { keyword: None }),
        solve_category: None,
        children: vec![member],
        result_format: None,
        matrix: Default::default(),
    };
    let positions = absolute_positions(&[block]);
    assert_eq!(positions[&1], None);
    // Not the relative (5, 5): that would draw the member at the page origin.
    assert_eq!(positions[&2], None);
}

#[test]
fn solve_block_member_is_offset_by_its_block() {
    let member = RegionModel {
        id: 2,
        pos: Some((5.0, 7.0)),
        size: None,
        result_ref: None,
        kind: RegionKind::Math(mathcraft_core::expr::Expr::var("x")),
        solve_category: Some("guess-value".to_string()),
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    };
    let block = RegionModel {
        id: 1,
        pos: Some((100.0, 200.0)),
        size: None,
        result_ref: None,
        kind: RegionKind::SolveBlock(SolveBlock { keyword: None }),
        solve_category: None,
        children: vec![member],
        result_format: None,
        matrix: Default::default(),
    };
    let positions = absolute_positions(&[block]);
    assert_eq!(positions[&2], Some((105.0, 207.0)));
}

/// Symbolic results (`x^2 - 1 -> factor`, `x*(7 y) -> expand`) are laid out as math next to their
/// region: the region is as wide as its math plus the result, and the result is not empty.
#[test]
fn symbolic_results_are_laid_out_as_math_beside_the_region() {
    require_samples!();
    let model = load_file(&sample("1_rata_1.mcdx")).expect("loads");
    let mut laid_out = 0;
    for item in &model.items {
        let Body::Math {
            math,
            result: Some(line),
        } = &item.body
        else {
            continue;
        };
        let Some(shown) = &line.math else {
            continue;
        };
        laid_out += 1;
        assert!(shown.width > 0.0 && shown.height() > 0.0, "{shown:?}");
        assert!(!shown.prims.is_empty());
        assert!(item.rect[2] >= math.width + shown.width, "{:?}", item.rect);
        assert!(item.rect[3] >= shown.height(), "{:?}", item.rect);
    }
    assert!(laid_out > 0, "1_rata_1 has symbolic results");
}

#[test]
fn mechanik_1_has_one_item_per_region_and_math_results() {
    require_samples!();
    let pkg = Package::open(sample("Mechanik_1_massen_ziehen.mcdx")).expect("opens");
    let ws = Worksheet::from_package(&pkg).expect("imports");
    let model = load_file(&sample("Mechanik_1_massen_ziehen.mcdx")).expect("loads");
    // 24 regions on the sheet (`mathcraft-cli inspect`), all math: one item each.
    assert_eq!(model.items.len(), 24);
    assert_eq!(model.items.len(), ws.evaluation_order().len());
    let math = model
        .items
        .iter()
        .filter(|i| matches!(i.body, Body::Math { .. }))
        .count();
    // Every counted region is a math region in this sheet; text regions are not counted.
    assert_eq!(
        math,
        model.counts.pass + model.counts.engine_error_no_ref + model.counts.no_reference
    );
    for item in &model.items {
        if let Body::Math {
            result: Some(line), ..
        } = &item.body
        {
            assert!(!line.tip.is_empty());
        }
    }
}

#[test]
fn all_24_samples_build_with_finite_sizes_and_sum_to_the_golden_totals() {
    require_samples!();
    let mut loaded = 0;
    let mut total = [0; 7];
    let mut unsupported_total = 0;
    for entry in std::fs::read_dir(examples_dir())
        .expect("examples")
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mcdx") {
            continue;
        }
        let model = load_file(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        loaded += 1;
        for item in &model.items {
            assert!(item.rect.iter().all(|v| v.is_finite()), "{path:?}");
            assert!(item.rect[2] >= 0.0 && item.rect[3] >= 0.0, "{path:?}");
            if let Body::Math { math, .. } = &item.body {
                assert!(math.width.is_finite() && math.width > 0.0, "{path:?}");
                assert!(math.height() > 0.0, "{path:?}");
            }
        }
        assert!(model.page[0] >= PAGE[0] && model.page[1] >= PAGE[1]);
        let row = counts_row(&model.counts);
        for (sum, value) in total.iter_mut().zip(row) {
            *sum += value;
        }
        unsupported_total += model.counts.unsupported.values().sum::<usize>();
    }
    assert_eq!(loaded, 24);
    // The golden totals of `mathcraft-cli golden`: Pass 103, Mismatch 2, Unsupported 5,
    // EngineError 0, EngineErrorNoRef 12, NoReference 267, PlotOk 0.
    assert_eq!(total, [103, 2, 5, 0, 12, 267, 0]);
    assert_eq!(unsupported_total, 5);
}

/// Symbols the layout and the result text can emit, beyond ASCII. Each one must be drawn with a
/// glyph of the bundled font, or be spelled in ASCII by `glyphs::FALLBACKS`.
const SYMBOLS: &str = "−·×≠≤≥→≡…°∞π ϕ αβγδεθλμσφω";

fn bundled_font_has(c: char) -> bool {
    crate::fonts::Face::ALL
        .iter()
        .any(|&face| crate::fonts::covers(face, c))
}

/// A headless egui context with the bundled fonts installed.
fn headless_context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

/// Every face family is bound in egui (an unbound family panics when laid out) and has the
/// glyphs of the symbols the sheet draws.
#[test]
fn every_face_family_is_bound_in_egui_and_has_the_symbols() {
    let ctx = headless_context();
    for face in Face::ALL {
        for c in "aα→≤".chars() {
            let font = FontId::new(12.0, crate::fonts::family(face));
            assert!(
                ctx.fonts_mut(|f| f.has_glyph(&font, c)),
                "{face:?} lacks {c}"
            );
        }
    }
}

#[test]
fn every_symbol_the_layout_emits_has_a_glyph_in_the_bundled_fonts() {
    for c in SYMBOLS.chars().filter(|c| !c.is_whitespace()) {
        assert!(bundled_font_has(c), "{c} has no glyph in the bundled fonts");
    }
}

/// Every character the samples draw (math primitives in their own face, text runs in their
/// weight and slant, results in roman) has a glyph in that face, so no fallback box is drawn.
#[test]
fn every_character_drawn_for_the_samples_has_a_glyph_in_its_face() {
    require_samples!();
    let mut missing = std::collections::BTreeSet::new();
    let mut check = |text: &str, face: Face| {
        for c in text.chars().filter(|c| !c.is_ascii()) {
            if !covers(face, c) {
                missing.insert((c, face));
            }
        }
    };
    for entry in std::fs::read_dir(examples_dir())
        .expect("examples")
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mcdx") {
            continue;
        }
        let model = load_file(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        for item in &model.items {
            match &item.body {
                Body::Text(text) => {
                    for run in text.rich().lines.iter().flat_map(|l| &l.runs) {
                        check(&run.text, Face::of(run.style.bold, run.style.italic));
                    }
                }
                Body::Math { math, result } => {
                    let mut boxes = vec![math];
                    if let Some(line) = result {
                        check(&line.computed, Face::Roman);
                        if let Some(stored) = &line.stored {
                            check(stored, Face::Roman);
                        }
                        boxes.extend(line.math.iter());
                    }
                    for b in boxes {
                        for prim in &b.prims {
                            if let crate::mathlayout::Prim::Text { text, face, .. } = prim {
                                check(text, *face);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}

/// Every text region of the 24 samples builds with its runs, a size and a height. Drawing needs
/// an egui context, so the view model is what is checked here.
#[test]
fn text_regions_of_all_samples_build_with_runs_and_heights() {
    require_samples!();
    let mut paths: Vec<_> = std::fs::read_dir(examples_dir())
        .expect("samples exist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 24);
    let mut texts = 0;
    for path in &paths {
        let model = load_file(path).expect("loads");
        for item in &model.items {
            let Body::Text(text) = &item.body else {
                continue;
            };
            texts += 1;
            let [_, _, w, h] = item.rect;
            assert!(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0);
            let rich = text.rich();
            assert!(!rich.lines.is_empty());
            assert_eq!(text.as_str(), rich.plain());
            for run in rich.lines.iter().flat_map(|line| &line.runs) {
                assert!(run.style.size > 0.0);
                assert!(!run.text.is_empty());
            }
        }
    }
    assert_eq!(texts, 5, "the samples hold five text regions");
}

#[test]
fn pages_are_at_least_the_default_page() {
    require_samples!();
    let model = load_file(&sample("Mechanik_2_Wellendurchbiegung.mcdx")).expect("loads");
    assert!(model.page[0] >= PAGE[0] && model.page[1] >= PAGE[1]);
}

#[test]
fn a_result_has_the_room_up_to_the_next_region_on_its_rows() {
    use super::result_room;
    // Expression 100 wide at x=0; the next region starts at x=200 on the same rows.
    let me = [0.0, 10.0, 100.0, 20.0];
    let next = [200.0, 12.0, 50.0, 20.0];
    // Result starts at 100 + gap (8): room is 200 - 108.
    assert!((result_room(me, 100.0, &[me, next]) - 92.0).abs() < 1e-4);
    // A region on another row does not limit it.
    let below = [200.0, 80.0, 50.0, 20.0];
    assert!(result_room(me, 100.0, &[me, below]).is_infinite());
    // A region that starts inside the expression is not counted (it cannot be beside the result).
    let inside = [50.0, 10.0, 30.0, 20.0];
    assert!(result_room(me, 100.0, &[me, inside]).is_infinite());
}

/// A top-level region at `pos` with `size`, holding `kind`, and `children`.
fn boxed(id: u32, pos: (f64, f64), size: (f64, f64), kind: RegionKind) -> RegionModel {
    RegionModel {
        id,
        pos: Some(pos),
        size: Some(size),
        result_ref: None,
        kind,
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
}

/// The view model of a worksheet built in code (no file, no stored results).
fn view_of(regions: Vec<RegionModel>) -> super::ViewModel {
    use mathcraft_import::evaluate::evaluate_worksheet;
    use mathcraft_import::regions::{ImportReport, Settings};
    use mathcraft_import::results::ResultTable;
    use std::collections::BTreeMap;
    let ws = Worksheet {
        regions,
        settings: Settings::default(),
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer: Vec::new(),
    };
    let table = ResultTable {
        by_id: BTreeMap::new(),
    };
    let evaluation = evaluate_worksheet(&ws, &table);
    super::build("test", &ws, &evaluation)
}

/// A text region placed near the right edge keeps its width inside the page: the sheet does not
/// grow to take it (it is narrowed instead).
#[test]
fn a_text_region_near_the_right_edge_does_not_widen_the_page() {
    let text = boxed(
        1,
        (480.0, 100.0),
        (360.0, 20.0),
        RegionKind::Text(mathcraft_import::rich_text::TextBody::from_plain(
            "Hallo Test",
        )),
    );
    let vm = view_of(vec![text]);
    let item = vm.items.iter().find(|i| i.region_id == 1).expect("text");
    assert!(item.rect[0] + item.rect[2] <= super::PAGE[0] - super::MARGIN + 0.5);
    assert_eq!(vm.page[0], super::PAGE[0], "the page keeps its width");
}

#[test]
fn collapsed_area_hides_its_regions_and_closes_the_gap() {
    use mathcraft_import::regions::AreaModel;
    use mathcraft_import::rich_text::TextBody;
    // The area spans y 100..300. Its text is inside; a text below starts at 320 and an item at
    // 400 starts well below.
    let mut area = boxed(
        1,
        (10.0, 100.0),
        (300.0, 200.0),
        RegionKind::Area(AreaModel {
            collapsed: true,
            ..AreaModel::default()
        }),
    );
    area.children = vec![boxed(
        2,
        (20.0, 130.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("inside")),
    )];
    let below = boxed(
        3,
        (10.0, 320.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("below")),
    );
    let far = boxed(
        4,
        (10.0, 400.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("far")),
    );
    let vm = view_of(vec![area, below, far]);
    let ids: Vec<u32> = vm.items.iter().map(|i| i.region_id).collect();
    assert!(!ids.contains(&2), "a collapsed area hides its regions");
    let area_item = vm.items.iter().find(|i| i.region_id == 1).expect("area");
    assert_eq!(area_item.rect[3], super::AREA_HEADER_PT);
    // The gap is 200 - 18 = 182 points.
    let y = |id: u32| vm.items.iter().find(|i| i.region_id == id).unwrap().rect[1];
    assert_eq!(y(3), 320.0 - 182.0);
    assert_eq!(y(4), 400.0 - 182.0);
    assert_eq!(y(1), 100.0, "the area keeps its top");
}

#[test]
fn expanded_area_keeps_every_item_where_it_is() {
    use mathcraft_import::regions::AreaModel;
    use mathcraft_import::rich_text::TextBody;
    let mut area = boxed(
        1,
        (10.0, 100.0),
        (300.0, 200.0),
        RegionKind::Area(AreaModel::default()),
    );
    area.children = vec![boxed(
        2,
        (20.0, 130.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("inside")),
    )];
    let below = boxed(
        3,
        (10.0, 320.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("below")),
    );
    let vm = view_of(vec![area, below]);
    let ids: Vec<u32> = vm.items.iter().map(|i| i.region_id).collect();
    assert!(ids.contains(&2));
    let below_item = vm.items.iter().find(|i| i.region_id == 3).unwrap();
    assert_eq!(below_item.rect[1], 320.0);
}

#[test]
fn page_break_moves_the_content_below_it_to_the_next_page() {
    use crate::pages::Geometry;
    use mathcraft_import::rich_text::TextBody;
    let h = Geometry::new(&Default::default(), 1.0).paper[1];
    let above = boxed(
        1,
        (10.0, 100.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("above")),
    );
    let marker = boxed(
        2,
        (10.0, f64::from(h) / 2.0),
        (0.0, 0.0),
        RegionKind::PageBreak,
    );
    let below = boxed(
        3,
        (10.0, f64::from(h) / 2.0 + 10.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("below")),
    );
    let vm = view_of(vec![above, marker, below]);
    let top = |id: u32| vm.items.iter().find(|i| i.region_id == id).unwrap().rect[1];
    assert_eq!(top(1), 100.0, "content above the break does not move");
    // The break sits at the middle of page 1; the content 10 pt below it starts 10 pt below the
    // top of page 2.
    assert_eq!(top(3), h + 10.0, "content below starts the next page");
    assert_eq!(vm.page_breaks, vec![h / 2.0]);
}

#[test]
fn stored_positions_are_measured_from_the_margin_box() {
    use mathcraft_import::rich_text::TextBody;
    // A region stored at (0, 0) is at the margin box corner: 25 mm in from the paper's edges, so
    // it lies inside the grid, and the frame origin is that corner.
    let at_origin = boxed(
        1,
        (0.0, 0.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("origin")),
    );
    let vm = view_of(vec![at_origin]);
    let item = vm.items.iter().find(|i| i.region_id == 1).unwrap();
    assert_eq!((item.rect[0], item.rect[1]), (0.0, 0.0));
    let origin = vm.frame.origin();
    assert!((origin[0] - 70.866).abs() < 0.01, "left margin of 25 mm");
    assert!((origin[1] - 70.866).abs() < 0.01, "top margin of 25 mm");
    assert_eq!(vm.to_display(0.0), 0.0);
    assert_eq!(vm.to_stored(0.0), 0.0);
}

#[test]
fn stored_and_frame_positions_round_trip_across_pages_and_breaks() {
    use crate::pages::PageFrame;
    use mathcraft_import::rich_text::TextBody;
    let frame = PageFrame::default();
    let p = frame.printable_h();
    // A break near the bottom of page 1 pushes the content below it onto page 2.
    let marker = boxed(
        2,
        (10.0, f64::from(p) * 1.5),
        (0.0, 0.0),
        RegionKind::PageBreak,
    );
    let below = boxed(
        3,
        (10.0, f64::from(p) * 1.5 + 30.0),
        (100.0, 20.0),
        RegionKind::Text(TextBody::from_plain("below")),
    );
    let vm = view_of(vec![marker, below]);
    let top = |id: u32| vm.items.iter().find(|i| i.region_id == id).unwrap().rect[1];
    // The break falls in the middle of page 1, so the content below starts page 2: its printable
    // top, 30 pt down, is at frame y 2 * paper_h + 30.
    assert!(
        (top(3) - (2.0 * frame.paper_h + 30.0)).abs() < 1e-2,
        "top {}",
        top(3)
    );
    for stored in [0.0_f32, 10.0, p * 0.5, p * 1.5 + 30.0, p * 3.0 + 1.0] {
        let back = vm.to_stored(vm.to_display(stored));
        assert!(
            (back - stored).abs() < 1e-2,
            "stored {stored} came back as {back}"
        );
    }
}

#[test]
fn image_item_carries_its_bytes_and_a_key() {
    use mathcraft_import::regions::ImageModel;
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(b"one");
    let image = boxed(
        1,
        (10.0, 10.0),
        (50.0, 40.0),
        RegionKind::Image(ImageModel {
            item: String::new(),
            bytes: bytes.clone().into(),
        }),
    );
    let vm = view_of(vec![image]);
    let item = &vm.items[0];
    assert_eq!(item.rect, [10.0, 10.0, 50.0, 40.0]);
    let Body::Image(view) = &item.body else {
        panic!("an image item");
    };
    assert_eq!(&*view.bytes, &bytes[..]);
    assert_ne!(view.key, 0);
}

//! Tests of the render library. The GPU tests skip (pass, with a note) when no wgpu adapter exists.

use super::*;
use crate::testdata::sample;

fn renderer() -> Option<Renderer> {
    match Renderer::new() {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

fn first_plot(sheet: &Sheet) -> u32 {
    sheet
        .vm
        .items
        .iter()
        .find(|i| matches!(i.body, Body::Plot(_)))
        .map(|i| i.region_id)
        .expect("a 2D plot")
}

/// Decodes PNG bytes and counts the pixels that are not (nearly) white.
fn ink(png: &Png) -> usize {
    let img = image::load_from_memory_with_format(&png.bytes, image::ImageFormat::Png)
        .expect("valid PNG")
        .to_rgba8();
    assert_eq!((img.width(), img.height()), (png.width, png.height));
    img.pixels()
        .filter(|p| p.0[..3].iter().any(|c| *c < 200))
        .count()
}

#[test]
fn plot_data_matches_the_projectile_formula() {
    require_samples!();
    let sheet = Sheet::open(&sample("17_rata_schiefer_wurf.mcdx")).expect("opens");
    let id = first_plot(&sheet);
    let samples = plot_data(&sheet.vm, RegionRef::Id(id)).expect("plot data");
    assert_eq!(samples.region_id, id);
    assert_eq!(samples.x_range, (0.0, 45.0));
    assert_eq!(samples.y_range, (-6.0, 6.0));
    let points: Vec<[f64; 2]> = samples
        .traces
        .iter()
        .flat_map(|t| t.segments.iter().flatten().copied())
        .collect();
    // tt = 0 s, 0.1 s .. 2.5 s: 26 samples of (x(tt), y(tt)) with v_0 = 20 m/s, beta = 30 degrees.
    assert_eq!(points.len(), 26);
    let (v0, beta, g) = (20.0_f64, 30.0_f64.to_radians(), 9.80665_f64);
    for (k, p) in points.iter().enumerate() {
        let t = 0.1 * k as f64;
        let (x, y) = (v0 * beta.cos() * t, v0 * beta.sin() * t - g / 2.0 * t * t);
        assert!((p[0] - x).abs() < 1e-6, "x at t={t}: {} vs {x}", p[0]);
        assert!((p[1] - y).abs() < 1e-6, "y at t={t}: {} vs {y}", p[1]);
    }
    // Index and id name the same region; a non-plot region is an error.
    let index = sheet
        .vm
        .items
        .iter()
        .position(|i| i.region_id == id)
        .unwrap();
    assert_eq!(
        plot_data(&sheet.vm, RegionRef::Index(index)).unwrap(),
        samples
    );
    assert!(plot_data(&sheet.vm, RegionRef::Index(0)).is_err());
    assert!(plot_data(&sheet.vm, RegionRef::Id(u32::MAX)).is_err());
}

#[test]
fn page_render_has_the_paper_size_and_ink() {
    require_samples!();
    let Some(r) = renderer() else { return };
    let sheet = Sheet::open(&sample("Mechanik_1_massen_ziehen.mcdx")).expect("opens");
    let [w, h] = crate::app::page_geometry_for(true, &sheet.page, &sheet.vm)
        .expect("layout")
        .paper;
    let options = PageOptions::default();
    let png = r.render_page(&sheet, &options).expect("renders");
    // 96 dpi: 4/3 pixels per point, under the 1600 px cap for letter or A4.
    assert_eq!(png.width, (w * 96.0 / 72.0).round() as u32);
    assert_eq!(png.height, (h * 96.0 / 72.0).round() as u32);
    assert!(ink(&png) > 500, "page is blank");
    assert!(r
        .render_page(
            &sheet,
            &PageOptions {
                page: 99,
                ..options
            }
        )
        .is_err());
}

#[test]
fn size_cap_lowers_the_scale() {
    require_samples!();
    let Some(r) = renderer() else { return };
    let sheet = Sheet::open(&sample("Mechanik_1_massen_ziehen.mcdx")).expect("opens");
    let options = PageOptions {
        dpi: 600.0,
        max_side: 400,
        ..PageOptions::default()
    };
    let png = r.render_page(&sheet, &options).expect("renders");
    assert_eq!(png.width.max(png.height), 400);
    let wide = PageOptions {
        width_px: Some(300),
        ..PageOptions::default()
    };
    assert_eq!(r.render_page(&sheet, &wide).unwrap().width, 300);
}

#[test]
fn region_render_is_smaller_than_the_page_and_not_blank() {
    require_samples!();
    let Some(r) = renderer() else { return };
    let sheet = Sheet::open(&sample("17_rata_schiefer_wurf.mcdx")).expect("opens");
    let id = first_plot(&sheet);
    let item = sheet.vm.items.iter().find(|i| i.region_id == id).unwrap();
    let options = RegionOptions {
        scale: 1.0,
        padding: 4.0,
        ..RegionOptions::default()
    };
    let png = r
        .render_region(&sheet, RegionRef::Id(id), &options)
        .expect("renders");
    assert_eq!(png.width, (item.rect[2] + 8.0).round() as u32);
    assert_eq!(png.height, (item.rect[3] + 8.0).round() as u32);
    assert!(ink(&png) > 200, "plot region is blank");
    let page = r.render_page(&sheet, &PageOptions::default()).unwrap();
    assert!(png.width < page.width && png.height < page.height);
    // A math region renders too, by index.
    let math = r
        .render_region(&sheet, RegionRef::Index(0), &RegionOptions::default())
        .expect("renders");
    assert!(ink(&math) > 20);
    assert!(r
        .render_region(&sheet, RegionRef::Id(u32::MAX), &options)
        .is_err());
}

/// Bad options are refused by the validators, which run before any GPU work, so these tests need
/// no adapter.
#[test]
fn bad_options_are_refused_before_rendering() {
    for dpi in [0.0, -5.0, f32::NAN] {
        let options = PageOptions {
            dpi,
            ..PageOptions::default()
        };
        assert!(validate_page(&options).is_err(), "dpi {dpi} accepted");
    }
    let zero_width = PageOptions {
        width_px: Some(0),
        ..PageOptions::default()
    };
    assert!(validate_page(&zero_width).is_err());
    assert!(validate_page(&PageOptions::default()).is_ok());

    for scale in [0.0, -1.0, f32::NAN] {
        let options = RegionOptions {
            scale,
            ..RegionOptions::default()
        };
        assert!(validate_region(&options).is_err(), "scale {scale} accepted");
    }
    assert!(validate_region(&RegionOptions::default()).is_ok());

    let oversize = WindowOptions {
        width: MAX_WINDOW_SIDE + 1.0,
        ..WindowOptions::default()
    };
    assert!(validate_window(&oversize).is_err());
    let tall = WindowOptions {
        height: MAX_WINDOW_SIDE * 2.0,
        ..WindowOptions::default()
    };
    assert!(validate_window(&tall).is_err());
    let at_cap = WindowOptions {
        width: MAX_WINDOW_SIDE,
        height: MAX_WINDOW_SIDE,
        ..WindowOptions::default()
    };
    assert!(validate_window(&at_cap).is_ok());
}

/// The same refusals reach the public methods (skipped without a GPU renderer, like the other
/// GPU tests; the window size is refused before any window is made).
#[test]
fn render_methods_refuse_bad_options() {
    let Some(r) = renderer() else { return };
    let sheet = Sheet::from_document(&Document::untitled("test"));
    let bad_page = PageOptions {
        dpi: 0.0,
        ..PageOptions::default()
    };
    assert!(r.render_page(&sheet, &bad_page).is_err());
    let bad_region = RegionOptions {
        scale: f32::NAN,
        ..RegionOptions::default()
    };
    assert!(r
        .render_region(&sheet, RegionRef::Index(0), &bad_region)
        .is_err());
    let oversize = WindowOptions {
        width: MAX_WINDOW_SIDE + 1.0,
        ..WindowOptions::default()
    };
    assert!(r.render_window(None, &oversize).is_err());
}

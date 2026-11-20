//! `render`: one page or one region of a worksheet as PNG. The drawing is the app's offscreen
//! renderer (`mathcraft_app::render`), compiled in only with the `render` feature so that the
//! default CLI build does not pull in egui and wgpu.

use crate::args::RenderArgs;

/// Draws the page or region and writes the PNG. Prints the output path and the image size.
#[cfg(feature = "render")]
pub fn render_png(args: &RenderArgs) -> Result<(), String> {
    use mathcraft_app::render::{
        PageOptions, RegionOptions, RegionRef, Renderer, Sheet, DEFAULT_MAX_SIDE,
    };
    let sheet = Sheet::open(&args.file).map_err(|e| e.to_string())?;
    let renderer = Renderer::new().map_err(|e| e.to_string())?;
    let max_side = args.max_side.unwrap_or(DEFAULT_MAX_SIDE);
    let png = match args.region {
        Some(id) => {
            let mut options = RegionOptions {
                show_grid: args.grid,
                max_side,
                ..RegionOptions::default()
            };
            if let Some(scale) = args.scale {
                options.scale = scale;
            }
            renderer.render_region(&sheet, RegionRef::Id(id), &options)
        }
        None => {
            let mut options = PageOptions {
                page: args.page,
                show_grid: args.grid,
                max_side,
                ..PageOptions::default()
            };
            if let Some(dpi) = args.dpi {
                options.dpi = dpi;
            }
            renderer.render_page(&sheet, &options)
        }
    }
    .map_err(|e| e.to_string())?;
    if let Some(parent) = args.out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&args.out, &png.bytes).map_err(|e| format!("{}: {e}", args.out.display()))?;
    println!(
        "wrote {} ({}x{} px)",
        args.out.display(),
        png.width,
        png.height
    );
    Ok(())
}

/// Without the `render` feature the command only explains how to get it.
#[cfg(not(feature = "render"))]
pub fn render_png(_args: &RenderArgs) -> Result<(), String> {
    Err(
        "this build has no renderer; rebuild with `--features render` \
         (cargo run -p mathcraft-cli --features render -- render ...)"
            .to_string(),
    )
}

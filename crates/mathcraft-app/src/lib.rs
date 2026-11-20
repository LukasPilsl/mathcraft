//! Mathcraft application: a worksheet viewer and editor for `.mcdx` files.
//!
//! The window is started by [`run`] (`mathcraft-app [file.mcdx]`, see `main.rs`). The modules are
//! public so that `examples/screenshot.rs` can drive the same [`app::App`] headless.

// First, so that its `require_samples!` macro is visible to every test module below and to the
// examples (`#[macro_export]`). Doc-hidden: it is test support, not part of the API.
#[macro_use]
#[doc(hidden)]
pub mod testdata;

pub mod app;
pub mod canvas;
pub mod catalog;
pub mod chrome;
pub mod chrome_dialogs;
pub mod config;
pub mod dialogs;
pub mod draftlayout;
pub mod edit;
pub mod export;
pub mod file_access;
pub mod file_picker;
pub mod fonts;
pub mod format;
pub use mathcraft_doc::functions_table;
pub mod furniture;
pub mod glyphs;
pub mod i18n;
pub mod icon;
pub mod images;
pub mod link_window;
pub mod live;
pub mod load;
pub mod loader;
pub mod mathbox;
pub mod mathlayout;
#[cfg(feature = "render")]
pub mod mcp_render;
/// Gives the MCP render tools their offscreen renderer (`mathcraft-mcp` calls this at start). Without
/// the `render` feature this does nothing and the render tools answer `render_unavailable`.
#[cfg(feature = "render")]
pub fn install_mcp_renderer() {
    mcp_render::install();
}

/// See the `render` version above.
#[cfg(not(feature = "render"))]
pub fn install_mcp_renderer() {}

pub mod menus;
pub mod messages;
pub mod pages;
pub mod palette;
pub mod pdf;
pub mod plot;
pub mod plot3d;
pub mod plot_dialog;
#[cfg(feature = "render")]
pub mod render;
pub mod ribbon;
pub mod ribbon_icons;
pub mod settings_window;
pub mod statusbar;
pub mod table;
pub mod temp_file;
pub mod theme;
pub mod units_dialog;
pub mod view_state;
pub mod viewmodel;

#[cfg(test)]
mod chrome_tests;
#[cfg(test)]
mod draft_tab_tests;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod functions_table_tests;
#[cfg(test)]
mod load_tests;
#[cfg(test)]
mod quit_tests;
#[cfg(test)]
mod search_tests;

use std::path::PathBuf;

use eframe::egui;

/// Starts the window. An optional first argument is a worksheet to open.
pub fn run() -> eframe::Result {
    let settings = config::load();
    i18n::set_language(settings.language.unwrap_or_else(i18n::system_language));
    // `--no-ai` / `--ai` override the Settings switch of the AI endpoint for this run.
    let mut initial = None;
    let mut ai_flag = None;
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("--no-ai") => ai_flag = Some(false),
            Some("--ai") => ai_flag = Some(true),
            _ if initial.is_none() => initial = Some(PathBuf::from(arg)),
            _ => {}
        }
    }
    let ai_on = ai_flag.or(settings.live).unwrap_or(true);
    // The live endpoint runs the render tools inside this window: give them the renderer. The GPU
    // device itself is created by the first render call, not here.
    install_mcp_renderer();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mathcraft")
            // Opens maximised; 1200 x 820 is the size the window returns to when restored.
            .with_inner_size([1200.0, 820.0])
            .with_maximized(true)
            .with_min_inner_size([640.0, 420.0])
            // The native title bar stays above the content (no content under the bar).
            .with_titlebar_shown(true)
            .with_fullsize_content_view(false)
            .with_drag_and_drop(true)
            .with_icon(std::sync::Arc::new(icon::icon_data(256))),
        ..Default::default()
    };
    eframe::run_native(
        "Mathcraft",
        options,
        Box::new(move |cc| {
            let mut app = app::App::new(cc, initial);
            app.set_live(&cc.egui_ctx, ai_on, config::config_dir());
            Ok(Box::new(app))
        }),
    )
}

//! Mathcraft application entry point: `mathcraft-app [file.mcdx]`. The app lives in the library
//! (`lib.rs`), so that the screenshot example can use it too.

// Release builds on Windows are a GUI program: without this Windows opens a console window too.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let result = mathcraft_app::run();
    mathcraft_app::export::remove_print_files();
    result
}

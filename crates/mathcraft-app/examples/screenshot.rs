//! Headless screenshots of the worksheet window, for inspection without a display.
//!
//! A thin front end of `mathcraft_app::render` (the render library, cargo feature `render`): the
//! real `App` runs frame by frame on an egui context and egui-wgpu draws it into an offscreen
//! texture, which is written as a PNG. Nothing opens a window. For one page or one region
//! instead of the window, see `mathcraft-cli render`. It needs a wgpu adapter (Metal on macOS, Vulkan or a software rasterizer such as
//! lavapipe on Linux); without one the example says so and stops.
//!
//! An input of `-` opens an empty worksheet.
//!
//! ```text
//! cargo run -p mathcraft-app --example screenshot -- <file.mcdx> <out.png> \
//!     [--width 1400] [--height 900] [--scale 1.0] [--lang en|de] [--theme light|dark] \
//!     [--tab Calculation] [--dialog insert-function] [--page-layout]
//! ```
//!
//! * `--theme` is light by default (Prime's windows are light). A headless run has no operating
//!   system theme to follow, so this is set explicitly.
//!
//! * `--tab` is a ribbon tab: Math, Insert, Functions, Matrices, Calculation, Plots, Format, View,
//!   Help (any case).
//! * `--dialog` is one of: insert-function, insert-unit, symbols, matrix, about, shortcuts,
//!   result-format, page-setup, worksheet-options, file-access, startup-notice.
//! * `--app-menu`, `--app-menu-sub new|open|save-as|print` show the application menu;
//!   `--options-window` the application Options window; `--units-dialog` the Einheitensystem
//!   (units) window; `--resource <button id>` the window of a Resources button (ids in
//!   `ribbon/resources.rs`, for example `ref_readme`); `--tabs N` opens N more untitled tabs.
//! * `--select-plot`, `--select-math` select the first plot or math region; `--popup <button id>`
//!   opens that ribbon dropdown (ids are in `ribbon/*.rs`, for example `matrix`, `vm_ops`, `type`,
//!   `styles`, `axes`, `insert`); `--grid-hover ROW,COL` shows the size grid with that cell hovered.
//! * `--zoom PCT` sets the worksheet zoom; `--click X,Y`, `--dblclick X,Y`, `--type TEXT` and
//!   `--key NAME` (Backspace, Delete, Tab, Enter, Escape, arrows, `ctrl+.`, `shift+Tab`) script
//!   input in command-line order, after the staged options, so a draft can be photographed.
//! * `--ai-connected` turns the AI endpoint on with one simulated client and flashes the first
//!   region, to photograph the status bar indicator and the highlight.
//! * `--page-layout` draws the sheet as pages instead of the draft view.
//! * The output folder is created when missing. Keep PNGs out of the repository (`/screenshots/`
//!   is ignored).
//!
//! `cargo test -p mathcraft-app --example screenshot` runs the smoke test: it renders one sample
//! into the system temp folder and skips with a message when no GPU renderer is available.

use std::path::PathBuf;

use eframe::egui;
use mathcraft_app::app::Staged;
use mathcraft_app::dialogs::Dialog;
use mathcraft_app::i18n::{self, Lang};
use mathcraft_app::menus::Submenu;
use mathcraft_app::render::{parse_key, RenderError, Renderer, Step, WindowOptions};
use mathcraft_app::ribbon::TABS;

/// What to draw, from the command line (or from a test).
#[derive(Debug, Clone)]
struct Options {
    input: PathBuf,
    output: PathBuf,
    window: WindowOptions,
}

/// Draws the window as configured in `options` (through the render library) and writes the PNG.
fn render(options: &Options) -> Result<(), RenderError> {
    let input = (options.input.as_os_str() != "-").then_some(options.input.as_path());
    let png = Renderer::new()?.render_window(input, &options.window)?;
    if let Some(parent) = options.output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RenderError::Failed(e.to_string()))?;
    }
    std::fs::write(&options.output, &png.bytes)
        .map_err(|e| RenderError::Failed(format!("{}: {e}", options.output.display())))
}

/// The ribbon tab index for a name such as `Calculation` (any case).
fn tab_index(name: &str) -> Option<usize> {
    let name = name.trim();
    TABS.iter().position(|key| {
        key.trim_start_matches("tab.").eq_ignore_ascii_case(name)
            || i18n::tr_in(Lang::English, key).eq_ignore_ascii_case(name)
            || i18n::tr_in(Lang::German, key).eq_ignore_ascii_case(name)
    })
}

/// The window or dialog for a name such as `insert-function`.
fn window(name: &str) -> Option<Staged> {
    Some(match name {
        "insert-function" => Staged::Dialog(Dialog::Function(None)),
        "insert-unit" => Staged::Dialog(Dialog::Unit),
        "symbols" => Staged::Dialog(Dialog::Symbols),
        "matrix" => Staged::Dialog(Dialog::Matrix),
        "about" => Staged::Dialog(Dialog::About),
        "shortcuts" => Staged::Dialog(Dialog::Shortcuts),
        "result-format" => Staged::ResultFormat,
        "page-setup" => Staged::PageSetup,
        "worksheet-options" => Staged::WorksheetOptions,
        "file-access" => Staged::FileAccess,
        "startup-notice" => Staged::StartupNotice,
        _ => return None,
    })
}

const USAGE: &str = "usage: screenshot <file.mcdx> <out.png> [--width N] [--height N] \
[--scale F] [--lang en|de] [--tab NAME] [--dialog NAME] [--page-layout]";

fn parse(args: &[String]) -> Result<Options, String> {
    let mut positional = Vec::new();
    let mut options = Options {
        input: PathBuf::new(),
        output: PathBuf::new(),
        window: WindowOptions::default(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--width" => options.window.width = number(&value("--width")?)?,
            "--height" => options.window.height = number(&value("--height")?)?,
            "--scale" => options.window.scale = number(&value("--scale")?)?,
            "--lang" => {
                let code = value("--lang")?;
                options.window.lang = match code.to_ascii_lowercase().as_str() {
                    "en" | "english" => Lang::English,
                    "de" | "german" | "deutsch" => Lang::German,
                    other => return Err(format!("unknown language {other:?} (en or de)")),
                };
            }
            "--theme" => {
                let name = value("--theme")?;
                options.window.theme = match name.to_ascii_lowercase().as_str() {
                    "light" => egui::ThemePreference::Light,
                    "dark" => egui::ThemePreference::Dark,
                    other => return Err(format!("unknown theme {other:?} (light or dark)")),
                };
            }
            "--tab" => {
                let name = value("--tab")?;
                let index = tab_index(&name).ok_or_else(|| format!("unknown tab {name:?}"))?;
                options.window.staged.push(Staged::Tab(index));
            }
            "--dialog" => {
                let name = value("--dialog")?;
                let staged = window(&name).ok_or_else(|| format!("unknown dialog {name:?}"))?;
                options.window.staged.push(staged);
            }
            "--zoom" => options
                .window
                .staged
                .push(Staged::Zoom(number(&value("--zoom")?)?)),
            "--click" | "--dblclick" => {
                let text = value(arg)?;
                let (x, y) = text
                    .split_once(',')
                    .ok_or_else(|| format!("{arg} needs X,Y"))?;
                let (x, y) = (number(x)?, number(y)?);
                options.window.script.push(if arg == "--click" {
                    Step::Click(x, y)
                } else {
                    Step::DoubleClick(x, y)
                });
            }
            "--type" => options.window.script.push(Step::Type(value("--type")?)),
            "--key" => {
                let name = value("--key")?;
                let (key, mods) =
                    parse_key(&name).ok_or_else(|| format!("unknown key {name:?}"))?;
                options.window.script.push(Step::Key(key, mods));
            }
            "--page-layout" => options.window.staged.push(Staged::PageLayout),
            "--ai-connected" => options.window.staged.push(Staged::Live),
            "--select-plot" => options.window.staged.push(Staged::SelectPlot),
            "--select-math" => options.window.staged.push(Staged::SelectMath),
            "--popup" => options.window.staged.push(Staged::Popup(value("--popup")?)),
            "--grid-hover" => {
                let text = value("--grid-hover")?;
                let (r, c) = text
                    .split_once(',')
                    .ok_or_else(|| "--grid-hover needs ROW,COL".to_string())?;
                let n = |t: &str| {
                    t.trim()
                        .parse::<usize>()
                        .map_err(|_| "--grid-hover needs ROW,COL".to_string())
                };
                options.window.staged.push(Staged::GridHover(n(r)?, n(c)?));
            }
            "--app-menu" => options.window.staged.push(Staged::AppMenu(None)),
            "--app-menu-sub" => {
                let name = value("--app-menu-sub")?;
                let sub = match name.to_ascii_lowercase().as_str() {
                    "new" => Submenu::New,
                    "open" => Submenu::Open,
                    "save-as" => Submenu::SaveAs,
                    "print" => Submenu::Print,
                    _ => return Err(format!("unknown submenu {name:?}")),
                };
                options.window.staged.push(Staged::AppMenu(Some(sub)));
            }
            "--units-dialog" => options.window.staged.push(Staged::UnitsDialog),
            "--resource" => options
                .window
                .staged
                .push(Staged::Resource(value("--resource")?)),
            "--options-window" => options.window.staged.push(Staged::AppOptions),
            "--tabs" => {
                let n = value("--tabs")?
                    .parse::<usize>()
                    .map_err(|_| "--tabs needs a number".to_string())?;
                options.window.staged.push(Staged::Tabs(n));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => positional.push(arg.clone()),
        }
    }
    let [input, output] = positional.as_slice() else {
        return Err("need an input .mcdx and an output .png".to_string());
    };
    options.input = PathBuf::from(input);
    options.output = PathBuf::from(output);
    Ok(options)
}

fn number(text: &str) -> Result<f32, String> {
    text.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| format!("not a positive number: {text}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            std::process::exit(2);
        }
    };
    match render(&options) {
        Ok(()) => println!("wrote {}", options.output.display()),
        Err(RenderError::Renderer(message)) => {
            eprintln!("no GPU renderer available here ({message}); cannot take screenshots");
            std::process::exit(1);
        }
        Err(RenderError::Failed(message)) => {
            eprintln!("screenshot failed: {message}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_app::require_samples;

    /// Renders one sample into the temp folder. Skips (passes, with a note) without the samples or
    /// without a renderer.
    #[test]
    fn renders_a_sample_to_png() {
        require_samples!();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../documentation/examples/Mechanik_1_massen_ziehen.mcdx");
        let dir = std::env::temp_dir().join(format!("mathcraft-shot-{}", std::process::id()));
        let output = dir.join("smoke.png");
        let options = Options {
            input: sample,
            output: output.clone(),
            window: WindowOptions {
                width: 800.0,
                height: 600.0,
                staged: vec![Staged::Tab(4)],
                ..WindowOptions::default()
            },
        };
        match render(&options) {
            Ok(()) => {
                let size = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
                assert!(size > 1000, "PNG too small: {size} bytes");
            }
            Err(RenderError::Renderer(message)) => {
                eprintln!("skipped: no GPU renderer available ({message})");
            }
            Err(RenderError::Failed(message)) => panic!("screenshot failed: {message}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tab_and_dialog_names_resolve() {
        assert_eq!(tab_index("Calculation"), Some(7));
        assert_eq!(tab_index("Berechnung"), Some(7));
        assert_eq!(tab_index("Eingabe/Ausgabe"), Some(1));
        assert_eq!(tab_index("math_format"), Some(5));
        assert_eq!(tab_index("help"), None);
        assert_eq!(tab_index("nope"), None);
        assert!(window("insert-function").is_some());
        assert!(window("result-format").is_some());
        assert!(window("nope").is_none());
    }

    #[test]
    fn command_line_is_parsed() {
        let args: Vec<String> = [
            "a.mcdx",
            "b.png",
            "--lang",
            "de",
            "--tab",
            "Plots",
            "--page-layout",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let options = parse(&args).expect("valid arguments");
        assert_eq!(options.window.lang, Lang::German);
        assert_eq!(
            options.window.staged,
            vec![Staged::Tab(4), Staged::PageLayout]
        );
        assert!(parse(&["only-one".to_string()]).is_err());
    }
}

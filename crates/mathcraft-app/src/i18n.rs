//! The window's strings in English and German, keyed. Every user-visible string goes through
//! [`tr`] (or [`trf`] with `{}` placeholders filled in order, or [`term`] for a category or unit
//! dimension name). A missing German text falls back to English, and a missing key shows the key.
//!
//! German wording follows the German Mathcad Prime 11 interface where the screenshots show it
//! (see `screenshots/Mathcad_Prime_11_Documentation_Index.md`); other German wording is our own.
//! Function names and descriptions stay English (Prime keeps English function names).
//!
//! The language is process-wide (set once at start-up and when the user picks one in the View
//! tab), so drawing code calls [`tr`] without passing a language around. Tests use [`tr_in`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use mathcraft_core::eval::NumberSeparators;

/// A display language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    English,
    German,
}

impl Lang {
    /// Every language, in the order the selector lists them.
    pub const ALL: [Lang; 2] = [Lang::English, Lang::German];

    /// The code stored in the config file.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::German => "de",
        }
    }

    /// The language with a stored code, `None` for an unknown code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Lang::English),
            "de" => Some(Lang::German),
            _ => None,
        }
    }

    /// The name shown in the language selector, in that language.
    #[must_use]
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::German => "Deutsch",
        }
    }

    /// The number separators of this language's results.
    #[must_use]
    pub fn separators(self) -> NumberSeparators {
        match self {
            Lang::English => NumberSeparators::INVARIANT,
            Lang::German => NumberSeparators::GERMAN,
        }
    }
}

/// The language the window currently shows. English until [`set_language`] is called.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Makes `lang` the language of [`tr`], [`trf`], [`term`] and the number display.
pub fn set_language(lang: Lang) {
    CURRENT.store(
        match lang {
            Lang::English => 0,
            Lang::German => 1,
        },
        Ordering::Relaxed,
    );
    // The document API (`mathcraft-doc`) gives its messages in this language too.
    mathcraft_doc::locale::set_translator(|key, args| Some(trf(key, args)));
    mathcraft_doc::locale::set_decimal_comma(lang == Lang::German);
}

/// The language the window currently shows.
#[must_use]
pub fn language() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::German,
        _ => Lang::English,
    }
}

/// The number separators of the current language (decimal comma in German).
#[must_use]
pub fn number_separators() -> NumberSeparators {
    language().separators()
}

/// True when the math editor reads a comma inside a number as a decimal point (German).
#[must_use]
pub fn decimal_comma() -> bool {
    language() == Lang::German
}

/// Picks a language from a locale name such as `de_DE.UTF-8`, `de-DE` or `en_US`. `None` for
/// the C or POSIX locale and for languages without a translation.
#[must_use]
pub fn lang_from_locale(locale: &str) -> Option<Lang> {
    let head: String = locale
        .trim()
        .trim_matches(|c: char| c == '"' || c == '(' || c == ')')
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_lowercase();
    match head.as_str() {
        "de" => Some(Lang::German),
        "en" => Some(Lang::English),
        _ => None,
    }
}

/// The language from the environment variables a POSIX system uses, in their usual priority:
/// `LC_ALL`, `LC_MESSAGES`, `LANG`, `LANGUAGE`. `get` reads a variable. `None` when no variable
/// names a language that has a translation.
#[must_use]
pub fn detect_from_env(get: impl Fn(&str) -> Option<String>) -> Option<Lang> {
    for name in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Some(value) = get(name) {
            // LANGUAGE may hold a list such as `de:en`; the first entry decides.
            let first = value.split(':').next().unwrap_or("");
            if let Some(lang) = lang_from_locale(first) {
                return Some(lang);
            }
        }
    }
    None
}

/// The language from the output of `defaults read -g AppleLanguages` (a property list such as
/// `(\n "de-DE",\n "en-US"\n)`): the first listed language that has a translation.
#[must_use]
pub fn lang_from_apple_languages(text: &str) -> Option<Lang> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .find_map(lang_from_locale)
}

/// The language of the operating system: the environment first, then on macOS the system
/// language list. English when nothing says otherwise.
#[must_use]
pub fn system_language() -> Lang {
    detect_from_env(|name| std::env::var(name).ok())
        .or_else(apple_language)
        .unwrap_or(Lang::English)
}

#[cfg(target_os = "macos")]
fn apple_language() -> Option<Lang> {
    let output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()?;
    lang_from_apple_languages(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn apple_language() -> Option<Lang> {
    None
}

/// The text of `key` in `lang`: the German text when it is not empty, else the English, else the
/// key itself.
#[must_use]
pub fn tr_in(lang: Lang, key: &'static str) -> &'static str {
    match index().get(key) {
        Some(&i) => {
            let (_, english, german) = STRINGS[i];
            match lang {
                Lang::English => english,
                Lang::German if !german.is_empty() => german,
                Lang::German => english,
            }
        }
        None => key,
    }
}

/// The text of `key` in the current language. See [`tr_in`].
#[must_use]
pub fn tr(key: &'static str) -> &'static str {
    tr_in(language(), key)
}

/// The text of `key` in the current language with each `{}` replaced by the next argument. A
/// `{}` without an argument stays as it is.
#[must_use]
pub fn trf(key: &'static str, args: &[&str]) -> String {
    trf_in(language(), key, args)
}

/// [`trf`] in a given language.
#[must_use]
pub fn trf_in(lang: Lang, key: &'static str, args: &[&str]) -> String {
    fill(tr_in(lang, key), args)
}

/// Replaces the `{}` placeholders of `template` with `args`, in order.
#[must_use]
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut args = args.iter();
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        match args.next() {
            Some(arg) => out.push_str(arg),
            None => out.push_str("{}"),
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// The text of a category, dimension or other short name in the current language. Names not in
/// the table are returned unchanged.
#[must_use]
pub fn term(english: &'static str) -> &'static str {
    if language() == Lang::English {
        return english;
    }
    TERMS
        .iter()
        .find(|(en, _)| *en == english)
        .map_or(english, |(_, german)| *german)
}

/// [`term`] for a name that is not a static string (a dimension label built at run time).
#[must_use]
pub fn term_of(english: &str) -> String {
    if language() == Lang::English {
        return english.to_string();
    }
    TERMS
        .iter()
        .find(|(en, _)| *en == english)
        .map_or(english, |(_, german)| german)
        .to_string()
}

fn index() -> &'static HashMap<&'static str, usize> {
    static INDEX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    INDEX.get_or_init(|| {
        STRINGS
            .iter()
            .enumerate()
            .map(|(i, (key, _, _))| (*key, i))
            .collect()
    })
}

/// Short names: function categories, unit dimensions, and the page-margin presets.
pub const TERMS: &[(&str, &str)] = &[
    ("Data Analysis", "Datenanalyse"),
    ("Design of Experiments", "Statistische Versuchsplanung"),
    ("Finance", "Finanzen"),
    ("Graphing", "Diagramme"),
    ("Image Processing", "Bildverarbeitung"),
    ("Miscellaneous Functions", "Verschiedene Funktionen"),
    ("Other", "Sonstige"),
    ("Reading and Writing Files", "Dateizugriff"),
    ("Signal Processing", "Signalverarbeitung"),
    ("Solving and Optimization", "Auflösung"),
    ("Special Functions", "Spezialfunktionen"),
    ("Statistics", "Statistik"),
    ("Symbolic Functions", "Symbolische Funktionen"),
    ("Transforms and Filters", "Transformationen und Filter"),
    (
        "Trigonometric, Log, and Exponential",
        "Trigonometrie, Logarithmus und Exponential",
    ),
    ("Utility Functions", "Hilfsfunktionen"),
    ("Vector and Matrix", "Vektor und Matrix"),
    ("length", "Länge"),
    ("mass", "Masse"),
    ("time", "Zeit"),
    ("current", "Stromstärke"),
    ("temperature", "Temperatur"),
    ("amount of substance", "Stoffmenge"),
    ("luminous intensity", "Lichtstärke"),
    ("dimensionless", "dimensionslos"),
    ("Narrow", "Schmal"),
    ("Standard", "Standard"),
    ("Legacy", "Legacy"),
    ("Wide", "Breit"),
];

/// Key, English, German. The German text is never empty (a test checks it). Placeholders `{}`
/// appear in the same number and order in both languages (a test checks it).
pub const STRINGS: &[(&str, &str, &str)] = &[
    // Ribbon tabs.
    ("tab.math", "Math", "Rechnen"),
    ("tab.insert", "Insert", "Einfügen"),
    ("tab.functions", "Functions", "Funktionen"),
    ("tab.matrices", "Matrices/Tables", "Matrizen/Tabellen"),
    ("tab.io", "Input/Output", "Eingabe/Ausgabe"),
    ("tab.math_format", "Math Formatting", "Mathematische Formatierung"),
    ("tab.text_format", "Text Formatting", "Textformatierung"),
    ("tab.document", "Document", "Dokument"),
    ("tab.resources", "Resources", "Ressourcen"),
    ("tip.not_available", "not available yet", "noch nicht verfügbar"),
    ("tip.more_groups", "More groups", "Weitere Gruppen"),
    ("tip.help_icon", "Help", "Hilfe"),
    ("tab.calculation", "Calculation", "Berechnung"),
    ("tab.plots", "Plots", "Diagramme"),
    ("tab.format", "Format", "Formatierung"),
    ("tab.view", "View", "Ansicht"),
    ("tab.help", "Help", "Hilfe"),
    // Ribbon group captions (drawn under each group of buttons).
    ("grp.regions", "Regions", "Bereiche"),
    ("grp.templates", "Templates", "Vorlagen"),
    ("grp.calculus", "Calculus", "Analysis"),
    ("grp.operators", "Operators and symbols", "Operatoren und Symbole"),
    ("grp.functions_units", "Functions and units", "Funktionen und Einheiten"),
    ("grp.objects", "Objects", "Objekte"),
    ("grp.function_list", "Functions", "Funktionen"),
    ("grp.categories", "Categories", "Kategorien"),
    ("grp.matrix", "Matrix", "Matrix"),
    ("grp.matrix_edit", "Edit matrix", "Matrix bearbeiten"),
    ("grp.tables", "Tables", "Tabellen"),
    ("grp.controls", "Controls", "Steuerelemente"),
    ("grp.worksheet", "Worksheet", "Arbeitsblatt"),
    ("grp.plots", "Plots", "Diagramme"),
    ("grp.align", "Align", "Ausrichten"),
    ("grp.distribute", "Distribute", "Verteilen"),
    ("grp.lines", "Lines", "Zeilen"),
    ("grp.results", "Results", "Ergebnisse"),
    ("grp.text", "Text", "Text"),
    ("grp.zoom", "Zoom", "Zoom"),
    ("grp.display", "Display", "Anzeige"),
    ("grp.page_layout", "Page layout", "Seitenlayout"),
    ("grp.header_footer", "Header and footer", "Kopf- und Fußzeile"),
    ("btn.page_break_insert", "Page break", "Seitenumbruch"),
    ("btn.page_break_delete", "Remove break", "Umbruch entfernen"),
    ("btn.header_edit", "Header", "Kopfzeile"),
    ("btn.footer_edit", "Footer", "Fußzeile"),
    ("btn.field_page", "Page no.", "Seitennr."),
    ("btn.field_pages", "Page count", "Seitenzahl"),
    ("btn.field_date", "Date", "Datum"),
    ("btn.field_filename", "Title", "Titel"),
    ("grp.language", "Language", "Sprache"),
    ("grp.help", "Help", "Hilfe"),
    // Buttons.
    ("btn.math_region", "Math region", "Rechnen"),
    ("btn.text_region", "Text region", "Textblock"),
    ("btn.fraction", "Fraction", "Bruch"),
    ("btn.power", "Power", "Potenz"),
    ("btn.square_root", "Square root", "Quadratwurzel"),
    ("btn.subscript", "Subscript", "Index"),
    ("btn.define", ":=", ":="),
    ("btn.evaluate", "=", "="),
    ("btn.symbolic", "->", "->"),
    ("btn.sum", "Summation", "Summe"),
    ("btn.product", "Product", "Produkt"),
    ("btn.integral", "Integral", "Integral"),
    ("btn.derivative", "Derivative", "Ableitung"),
    ("btn.symbols", "Symbols...", "Symbole..."),
    ("btn.function", "Function... (Ctrl+E)", "Funktion... (Strg+E)"),
    ("btn.unit", "Unit... (Ctrl+U)", "Einheit... (Strg+U)"),
    ("btn.all_functions", "All functions...", "Alle Funktionen..."),
    ("btn.image", "Image... (Ctrl+I)", "Bild... (Strg+I)"),
    ("btn.area", "Area (Ctrl+Shift+A)", "Bereich (Strg+Umschalt+A)"),
    ("btn.area_around", "Area around selection", "Bereich um Auswahl"),
    ("btn.worksheet", "Worksheet", "Arbeitsblatt einschließen"),
    ("btn.matrix", "Matrix...", "Matrix einfügen..."),
    ("btn.matrix_2x2", "Matrix 2x2", "Matrix 2x2"),
    ("btn.matrix_add_row", "Add row", "Zeile einfügen"),
    ("btn.matrix_add_column", "Add column", "Spalte einfügen"),
    ("btn.matrix_remove_row", "Delete row", "Zeile löschen"),
    ("btn.matrix_remove_column", "Delete column", "Spalte löschen"),
    ("btn.align_left", "Align left", "Links ausrichten"),
    ("btn.align_top", "Align top", "Oben ausrichten"),
    ("btn.align_right", "Align right", "Rechts ausrichten"),
    ("btn.align_bottom", "Align bottom", "Unten ausrichten"),
    ("btn.align_hcentre", "Centre horizontally", "Horizontal zentrieren"),
    ("btn.align_vcentre", "Centre vertically", "Vertikal zentrieren"),
    ("btn.distribute_h", "Distribute horizontally", "Horizontal verteilen"),
    ("btn.distribute_v", "Distribute vertically", "Vertikal verteilen"),
    ("btn.insert_lines", "Insert lines", "Leerzeile einfügen"),
    ("btn.table", "Table", "Tabelle einfügen"),
    ("btn.calculate", "Calculate (F9)", "Jetzt berechnen (F9)"),
    ("btn.recalculate", "Recalculate all", "Arbeitsblatt berechnen"),
    ("btn.auto_calc", "Automatic calculation", "Automatische Berechnung"),
    ("btn.file_access", "File access...", "Dateizugriff..."),
    ("dlg.file_access", "File access", "Dateizugriff"),
    ("dlg.open_title", "Open worksheet", "Arbeitsblatt öffnen"),
    ("dlg.save_title", "Save worksheet", "Arbeitsblatt speichern"),
    ("dlg.export_title", "Export as PDF", "Als PDF exportieren"),
    ("dlg.image_title", "Insert image", "Bild einfügen"),
    ("dlg.filter_worksheets", "Mathcad Prime worksheets", "Mathcad-Prime-Arbeitsblätter"),
    ("dlg.filter_pdf", "PDF documents", "PDF-Dokumente"),
    ("dlg.filter_images", "Images (PNG, JPEG)", "Bilder (PNG, JPEG)"),
    (
        "file.intro",
        "Functions such as READCSV and WRITECSV can use files next to this worksheet. Both switches are off when a worksheet opens, and they last only for this session.",
        "Funktionen wie READCSV und WRITECSV können Dateien neben diesem Arbeitsblatt verwenden. Beide Schalter sind beim Öffnen eines Arbeitsblatts aus und gelten nur für diese Sitzung.",
    ),
    ("file.allow_read", "Allow reading files", "Dateien lesen erlauben"),
    ("file.allow_write", "Allow writing files", "Dateien schreiben erlauben"),
    (
        "file.waits",
        "Writing waits for a calculation: press F9 (Calculate) to write the files.",
        "Schreiben wartet auf eine Berechnung: F9 (Jetzt berechnen) drücken, um die Dateien zu schreiben.",
    ),
    (
        "file.confirm_read_title",
        "Allow reading files?",
        "Dateien lesen erlauben?",
    ),
    (
        "file.confirm_read_body",
        "The READ functions can read text and data files in this worksheet's folder (.csv, .txt, .dat, .prn, .bmp, .wav). Their contents are shown on the page and can be saved in the worksheet. Allow this only for worksheets you trust.",
        "Die READ-Funktionen können Text- und Datendateien im Ordner dieses Arbeitsblatts lesen (.csv, .txt, .dat, .prn, .bmp, .wav). Ihr Inhalt wird auf der Seite angezeigt und kann im Arbeitsblatt gespeichert werden. Erlauben Sie das nur für Arbeitsblätter, denen Sie vertrauen.",
    ),
    (
        "file.confirm_write_title",
        "Allow writing files? This is a stronger permission",
        "Dateien schreiben erlauben? Das ist eine stärkere Berechtigung",
    ),
    (
        "file.confirm_write_body",
        "The WRITE functions can create or replace files in this worksheet's folder. A replaced file cannot be recovered. Files are written only on F9 (Calculate), never while you type. Allow this only for worksheets you trust.",
        "Die WRITE-Funktionen können Dateien im Ordner dieses Arbeitsblatts erstellen oder ersetzen. Eine ersetzte Datei lässt sich nicht wiederherstellen. Dateien werden nur bei F9 (Jetzt berechnen) geschrieben, nie während der Eingabe. Erlauben Sie das nur für Arbeitsblätter, denen Sie vertrauen.",
    ),
    ("btn.results_format", "Results format...", "Ergebnisformat..."),
    (
        "btn.worksheet_options",
        "Worksheet options...",
        "Arbeitsblatteinstellungen...",
    ),
    ("btn.xy_plot", "X-Y plot (@)", "x-y-Diagramm (@)"),
    ("btn.polar_plot", "Polar plot (Ctrl+4)", "Polardiagramm (Strg+4)"),
    ("btn.edit_plot", "Edit plot...", "Diagramm bearbeiten..."),
    ("sc.xy_plot", "Insert an x-y plot", "x-y-Diagramm einfügen"),
    ("sc.polar_plot", "Insert a polar plot", "Polardiagramm einfügen"),
    ("plot.window", "Plot", "Diagramm"),
    ("plot.x", "X-axis expressions", "x-Achsen-Ausdrücke"),
    ("plot.angle", "Angle expressions", "Winkel-Ausdrücke"),
    ("plot.y", "Y-axis expressions", "y-Achsen-Ausdrücke"),
    ("plot.radius", "Radius expressions", "Radius-Ausdrücke"),
    ("plot.hint", "One trace per expression, separated by commas (semicolons with the decimal comma). Empty limits are automatic.", "Eine Spur je Ausdruck, durch Kommas getrennt (Semikolons beim Dezimalkomma). Leere Grenzen sind automatisch."),
    ("plot.x_limits", "X axis from / to", "x-Achse von / bis"),
    ("plot.y_limits", "Y axis from / to", "y-Achse von / bis"),
    ("plot.grid", "Grid lines", "Gitternetzlinien"),
    ("plot.title", "Title", "Titel"),
    ("plot.apply", "Apply", "Übernehmen"),
    ("plot.cancel", "Cancel", "Abbrechen"),
    ("edit.plot_bad_limit", "The limit {} is not valid: {}", "Die Grenze {} ist ungültig: {}"),
    ("edit.plot_x_min", "x start", "x-Anfang"),
    ("edit.plot_x_max", "x end", "x-Ende"),
    ("edit.plot_y_min", "y start", "y-Anfang"),
    ("edit.plot_y_max", "y end", "y-Ende"),
    ("btn.plot_3d", "3D plot", "3D-Diagramm"),
    ("btn.bold", "Bold", "Fett"),
    ("btn.italic", "Italic", "Kursiv"),
    ("btn.font", "Font...", "Schriftart..."),
    ("btn.zoom_in", "Zoom in", "Vergrößern"),
    ("btn.zoom_out", "Zoom out", "Verkleinern"),
    ("btn.zoom_reset", "Zoom 100%", "Zoom 100 %"),
    ("btn.grid", "Grid", "Raster anzeigen"),
    ("btn.page_layout", "Page layout", "Seitenansicht"),
    ("btn.region_borders", "Region borders", "Rahmen anzeigen"),
    ("btn.export_pdf", "Export PDF", "PDF exportieren"),
    ("btn.print", "Print", "Drucken"),
    ("btn.page_setup", "Page setup", "Seite einrichten"),
    ("btn.about", "About Mathcraft", "Info zu Mathcraft"),
    ("btn.shortcuts", "Keyboard shortcuts", "Tastaturbefehle"),
    ("btn.help_topics", "Help topics", "Hilfethemen"),
    ("btn.insert", "Insert", "Einfügen"),
    ("btn.close", "Close", "Schließen"),
    ("btn.cancel", "Cancel", "Abbrechen"),
    ("btn.new", "New", "Neu"),
    ("btn.save", "Save", "Speichern"),
    ("btn.save_as", "Save As", "Speichern unter"),
    ("btn.open", "Open", "Öffnen"),
    ("btn.discard", "Discard", "Nicht speichern"),
    ("btn.apply", "Apply to worksheet", "Auf Arbeitsblatt anwenden"),
    ("btn.apply_region", "Apply to selected region", "Auf gewählten Bereich anwenden"),
    ("btn.clear_format", "Clear format", "Format löschen"),
    // Tooltips and notes.
    ("tip.not_built", "not implemented yet", "noch nicht implementiert"),
    (
        "tip.no_worksheet",
        "Open or create a worksheet first.",
        "Zuerst ein Arbeitsblatt öffnen oder anlegen.",
    ),
    (
        "tip.worksheet_options",
        "ORIGIN, TOL, CTOL and PRNPRECISION of this worksheet",
        "ORIGIN, TOL, CTOL und PRNPRECISION dieses Arbeitsblatts",
    ),
    (
        "tip.calc_toggle",
        "Automatic: every change is calculated. Manual: only F9 or Calculate.",
        "Automatisch: jede Änderung wird berechnet. Manuell: nur F9 oder Jetzt berechnen.",
    ),
    // Dialogs.
    ("dlg.function", "Insert function", "Funktion einfügen"),
    ("dlg.unit", "Insert unit", "Einheit einfügen"),
    ("dlg.symbols", "Operators and symbols", "Operatoren und Symbole"),
    ("dlg.matrix", "Insert matrix", "Matrix einfügen"),
    ("dlg.about", "About Mathcraft", "Info zu Mathcraft"),
    ("dlg.shortcuts", "Keyboard shortcuts", "Tastaturbefehle"),
    ("dlg.search", "Search:", "Suchen:"),
    (
        "dlg.search_hint",
        "name, category or description",
        "Name, Kategorie oder Beschreibung",
    ),
    (
        "dlg.unit_search_hint",
        "symbol or name, e.g. kg or newton",
        "Zeichen oder Name, z. B. kg oder newton",
    ),
    ("dlg.no_match", "Nothing matches the search.", "Nichts passt zur Suche."),
    ("dlg.category", "Category", "Kategorie"),
    ("dlg.category_line", "{}: {}", "{}: {}"),
    (
        "dlg.function_title",
        "{} - {}",
        "{} - {}",
    ),
    (
        "dlg.descriptions_english",
        "Descriptions are in English.",
        "Beschreibungen sind nur auf Englisch.",
    ),
    (
        "dlg.greek",
        "Greek letters (Ctrl+G, then the key; Shift for capitals)",
        "Griechische Buchstaben (Strg+G, dann die Taste; Umschalt für Großbuchstaben)",
    ),
    (
        "dlg.constants",
        "Constants and operators",
        "Konstanten und Operatoren",
    ),
    ("dlg.matrix_size", "Choose rows and columns", "Zeilen und Spalten wählen"),
    ("dlg.matrix_hover", "{} x {}", "{} x {}"),
    ("dlg.about_licence", "Licence: MIT OR Apache-2.0", "Lizenz: MIT OR Apache-2.0"),
    (
        "dlg.about_text",
        "An open-source worksheet for engineering calculation, written in Rust with egui.",
        "Ein Open-Source-Arbeitsblatt für technische Berechnungen, geschrieben in Rust mit egui.",
    ),
    (
        "dlg.about_affiliation",
        "Mathcraft is not affiliated with PTC or Mathcad. Mathcad is a trademark of its owner.",
        "Mathcraft ist nicht mit PTC oder Mathcad verbunden. Mathcad ist eine Marke ihres Inhabers.",
    ),
    ("about.version", "Version {}", "Version {}"),
    ("dlg.page_setup", "Page setup", "Seite einrichten"),
    ("dlg.unsaved", "Unsaved changes", "Ungespeicherte Änderungen"),
    ("dlg.results_format", "Results format", "Ergebnisformat"),
    ("dlg.worksheet_options", "Worksheet options", "Arbeitsblatteinstellungen"),
    ("dlg.unit_group", "{} ({})", "{} ({})"),
    // Keyboard shortcut list (edit/shortcut.rs).
    ("sc.new", "New worksheet", "Neues Arbeitsblatt"),
    (
        "sc.open",
        "Open (the path in the File box, or a file dialog when it is empty)",
        "Öffnen (der Pfad im Feld Datei, sonst ein Dateidialog)",
    ),
    ("sc.save", "Save", "Speichern"),
    (
        "sc.save_as",
        "Save As (the path in the File box, or a file dialog when it is empty)",
        "Speichern unter (der Pfad im Feld Datei, sonst ein Dateidialog)",
    ),
    ("sc.image", "Insert an image (file dialog)", "Bild einfügen (Dateidialog)"),
    ("sc.area", "Insert an empty area", "Leeren Bereich einfügen"),
    ("sc.undo", "Undo", "Rückgängig"),
    ("sc.redo", "Redo", "Wiederholen"),
    ("sc.select_all", "Select all regions", "Alle Bereiche auswählen"),
    ("sc.sqrt", "Square root template", "Vorlage Quadratwurzel"),
    ("sc.sqrt_key", "Square root template (key \\)", "Vorlage Quadratwurzel (Taste \\)"),
    ("sc.derivative", "Derivative template", "Vorlage Ableitung"),
    ("sc.integral", "Integral template", "Vorlage Integral"),
    ("sc.sum", "Summation template", "Vorlage Summe"),
    ("sc.product", "Product template", "Vorlage Produkt"),
    ("sc.prime", "Prime mark, as in f'(t)", "Strich, wie in f'(t)"),
    ("sc.limit", "Limit template", "Vorlage Grenzwert"),
    ("sc.partial", "Partial derivative template", "Vorlage partielle Ableitung"),
    ("sc.gradient", "Gradient template", "Vorlage Gradient"),
    ("sc.cross", "Cross product", "Kreuzprodukt"),
    (
        "sc.term",
        "Move by one term (letters or digits) in a math region",
        "In Mathbereichen um einen Term (Buchstaben oder Ziffern) bewegen",
    ),
    (
        "sc.matrix_row",
        "Insert a matrix row below the cursor's row",
        "Matrixzeile unterhalb der Cursorzeile einfügen",
    ),
    ("sc.function", "Insert Function dialog", "Dialog Funktion einfügen"),
    ("sc.unit", "Insert Unit dialog", "Dialog Einheit einfügen"),
    (
        "sc.greek",
        "Greek letter for that key (Shift for a capital)",
        "Griechischer Buchstabe zu dieser Taste (Umschalt für Großbuchstaben)",
    ),
    ("sc.zoom100", "Zoom 100%", "Zoom 100 %"),
    ("sc.zoom_wheel", "Ctrl+mouse wheel or pinch", "Strg+Mausrad oder Pinch"),
    ("sc.zoom", "Zoom", "Zoom"),
    (
        "sc.calculate",
        "Calculate the worksheet (needed in manual calculation mode)",
        "Arbeitsblatt berechnen (nötig im manuellen Berechnungsmodus)",
    ),
    ("sc.enter", "Finish the math region being typed", "Rechenbereich beenden, der gerade geschrieben wird"),
    (
        "sc.escape",
        "Leave a text region, or clear the selection",
        "Textblock verlassen oder Auswahl aufheben",
    ),
    ("sc.delete", "Remove the selected regions", "Gewählte Bereiche entfernen"),
    ("sc.arrows", "Move the selected regions", "Gewählte Bereiche verschieben"),
    // Status bar and document strip.
    // Window chrome: application menu, quick access, tabs, status bar, context menu, options.
    ("menu.new", "New", "Neu"),
    ("menu.open", "Open", "Öffnen"),
    ("menu.save", "Save", "Speichern"),
    ("menu.save_as", "Save as", "Speichern unter"),
    ("menu.print", "Print", "Drucken"),
    ("menu.close", "Close", "Schließen"),
    ("menu.options", "Options", "Optionen"),
    ("menu.about", "About", "Info"),
    ("menu.quit", "Exit", "Beenden"),
    ("menu.new_blank", "New worksheet", "Neues Arbeitsblatt"),
    ("menu.from_my_templates", "From 'My templates'", "Aus 'Meine Vorlagen'"),
    ("menu.from_shared_templates", "From 'Shared templates'", "Aus 'Gemeinsame Vorlagen'"),
    ("menu.save_as_worksheet", "Worksheet", "Arbeitsblatt"),
    ("menu.save_as_template", "Template", "Vorlage"),
    ("menu.save_as_pdf", "PDF", "PDF"),
    ("menu.recent_documents", "Recent documents", "Zuletzt verwendete Dokumente"),
    ("menu.recent_worksheets", "Recent worksheets", "Zuletzt verwendete Arbeitsblätter"),
    ("menu.no_recent", "No recent worksheets", "Keine zuletzt verwendeten Arbeitsblätter"),
    ("menu.app_button", "Application menu", "Anwendungsmenü"),
    ("qat.undo", "Undo", "Rückgängig"),
    ("qat.redo", "Redo", "Wiederholen"),
    ("tab.close", "Close worksheet", "Arbeitsblatt schließen"),
    ("save.close_doc", "close this worksheet", "dieses Arbeitsblatt schließen"),
    ("status.calc_tip", "Click to switch between automatic and manual calculation", "Klicken, um zwischen automatischer und manueller Berechnung zu wechseln"),
    ("status.find", "Find:", "Suchen:"),
    ("status.ai_ready", "AI ready", "AI bereit"),
    ("status.ai_connected", "AI connected", "AI verbunden"),
    ("status.ai_paused", "AI paused", "AI pausiert"),
    ("status.ai_pause_tip", "Click to pause or resume the AI connection (paused: every AI call is refused)", "Klicken, um die AI-Verbindung zu pausieren oder fortzusetzen (pausiert: jeder AI-Aufruf wird abgelehnt)"),
    ("opt.ai_endpoint", "Allow AI assistants to connect (local MCP endpoint)", "AI-Assistenten die Verbindung erlauben (lokaler MCP-Endpunkt)"),
    ("startup.title", "Notice", "Hinweis"),
    ("startup.text", "Mathcraft is still in development. Errors and missing features can be reported on GitHub.", "Mathcraft befindet sich noch in der Entwicklung. Fehler und fehlende Funktionen bitte auf GitHub melden."),
    ("startup.link", "Report on GitHub: github.com/lukaspilsl/mathcraft/issues", "Auf GitHub melden: github.com/lukaspilsl/mathcraft/issues"),
    ("startup.dont_show", "Don't show again", "Nicht mehr anzeigen"),
    ("opt.ai_endpoint_tip", "Listens on 127.0.0.1 only; a client needs the token from a private file in your settings folder. Edits by the AI are single undo steps.", "Hört nur auf 127.0.0.1; ein Client braucht das Token aus einer privaten Datei im Einstellungsordner. Änderungen der AI sind einzelne Rückgängig-Schritte."),
    ("status.replace_with", "Replace with:", "Ersetzen durch:"),
    ("status.options", "Options", "Optionen"),
    ("status.search_equations", "Search in equations", "In Gleichungen suchen"),
    ("status.search_text", "Search in text", "Im Text suchen"),
    ("status.match_count", "{} of {}", "{} von {}"),
    ("status.no_matches", "No matches", "Keine Treffer"),
    ("status.previous_match", "Previous match (Shift+Enter)", "Vorheriger Treffer (Umschalt+Eingabe)"),
    ("status.next_match", "Next match (Enter)", "Nächster Treffer (Eingabe)"),
    ("status.replace_one_tip", "Replace the current match", "Aktuellen Treffer ersetzen"),
    ("status.replace_all", "All", "Alle"),
    ("status.replace_all_tip", "Replace all matches (one undo step)", "Alle Treffer ersetzen (ein Rückgängig-Schritt)"),
    ("status.replaced_count", "Replaced {} matches.", "{} Treffer ersetzt."),
    ("status.refused_count", "{} regions unchanged: the new text would not read.", "{} Bereiche unverändert: der neue Text ließe sich nicht lesen."),
    ("status.match_case", "Match case", "Groß-/Kleinschreibung"),
    ("status.whole_words", "Whole words only", "Nur ganze Wörter"),
    ("status.page_view", "Page view", "Seitenansicht"),
    ("status.list_view", "List view", "Listenansicht"),
    ("ws.paste", "Paste", "Einfügen"),
    ("ws.insert_text", "Insert text block", "Textblock einfügen"),
    ("ws.insert_textbox", "Insert text box", "Textfeld einfügen"),
    ("ws.insert_image", "Insert image", "Bild einfügen"),
    ("ws.insert_component", "Insert chart component", "Diagrammkomponente einfügen"),
    ("ws.insert_xy", "Insert x-y plot", "x-y-Diagramm einfügen"),
    ("ws.insert_solve", "Insert solve block", "Lösungsblock einfügen"),
    ("ws.insert_table", "Insert table", "Tabelle einfügen"),
    ("ws.add_space", "Add space", "Abstand hinzufügen"),
    ("ws.remove_space", "Remove space", "Abstand entfernen"),
    ("ws.add_page", "Add page", "Seite hinzufügen"),
    ("ws.add_break", "Add page break", "Seitenumbruch hinzufügen"),
    ("ws.hide_grid", "Hide grid", "Raster ausblenden"),
    ("ws.show_grid", "Show grid", "Raster einblenden"),
    ("ws.split_v", "Split regions vertically", "Bereiche vertikal trennen"),
    ("ws.split_h", "Split regions horizontally", "Bereiche horizontal trennen"),
    ("opt.title", "Mathcraft Options", "Mathcraft Optionen"),
    ("opt.tab_options", "Options", "Optionen"),
    ("opt.tab_licence", "Licence", "Lizenz"),
    ("opt.show_prime", "Show Prime's stored results", "Gespeicherte Prime-Ergebnisse anzeigen"),
    ("opt.resources_tab", "Enable the \"Resources\" tab", "Registerkarte \"Ressourcen\" aktivieren"),
    ("opt.windchill", "Enable Windchill integration", "Windchill Integration aktivieren"),
    ("opt.help_path", "Set a path to open the help from another location", "Pfad für den Zugriff auf die Hilfe von einem anderen Speicherort aus festlegen"),
    ("opt.diagnostics", "Disable diagnostic tools reporting", "Diagnostic Tools Berichterstattung deaktivieren"),
    ("opt.my_templates", "Specify another folder for 'My templates'", "Anderen Ordner für 'Meine Vorlagen' angeben"),
    ("opt.shared_templates", "Specify another folder for 'Shared templates'", "Anderen Ordner für 'Gemeinsame Vorlagen' angeben"),
    ("opt.new_template", "Use another template for new worksheets", "Beim Erstellen neuer Arbeitsblätter eine andere Vorlage verwenden"),
    ("opt.hide_control_warning", "Hide the advanced controls warning", "Warnung für erweiterte Steuerungen ausblenden"),
    ("opt.script_on_open", "Run advanced controls script when the worksheet opens", "Skript für erweiterte Steuerungen beim Öffnen des Arbeitsblatts ausführen"),
    ("opt.python_path", "Set the path of the Python installation folder", "Pfad zum Python-Installationsordner festlegen"),
    ("opt.licence_text", "Mathcraft is free software. It needs no licence key.", "Mathcraft ist freie Software und braucht keinen Lizenzschlüssel."),
    ("about.product", "Product: Mathcraft", "Produkt: Mathcraft"),
    ("about.locale", "Locale: {}", "Gebietsschema: {}"),
    ("app.busy_loading", "Another worksheet is still loading.", "Ein anderes Arbeitsblatt wird noch geladen."),
    ("doc.none", "No document", "Kein Dokument"),
    ("doc.file", "File:", "Datei:"),
    ("doc.path_hint", "path to a .mcdx file", "Pfad zu einer .mcdx-Datei"),
    ("status.show_prime", "Show Prime results", "Prime-Ergebnisse anzeigen"),
    ("status.zoom", "Zoom", "Zoom"),
    ("view.language", "Language", "Sprache"),
    ("calc.auto", "AUTO", "AUTO"),
    ("calc.manual", "MANUAL", "MANUAL"),
    // Page setup window.
    ("page.paper", "Paper", "Papier"),
    ("page.orientation", "Orientation", "Ausrichtung"),
    ("page.portrait", "Portrait", "Hochformat"),
    ("page.landscape", "Landscape", "Querformat"),
    ("page.margins", "Margins", "Ränder"),
    ("page.grid", "Grid size", "Rastergröße"),
    ("page.grid_standard", "Standard (24 px)", "Standard (24 px)"),
    ("page.grid_fine", "Fine (12 px)", "Fein (12 px)"),
    (
        "page.margins_note",
        "Margins {} mm on each side (Mathcraft's values; not yet read from Prime).",
        "Ränder {} mm auf jeder Seite (Werte von Mathcraft; noch nicht aus Prime gelesen).",
    ),
    // Results format and worksheet options windows.
    ("res.preview", "Preview", "Vorschau"),
    ("res.result_format", "Result format", "Ergebnisformat"),
    ("res.precision", "Display precision", "Anzeigegenauigkeit"),
    ("res.trailing_zeros", "Show trailing zeros", "Nachfolgende Nullen anzeigen"),
    ("res.radix", "Radix", "Zahlensystem"),
    ("res.imaginary", "Imaginary unit", "Imaginäre Einheit"),
    ("res.exp_threshold", "Exponential threshold", "Schwelle für Exponentialschreibweise"),
    ("res.zero_threshold", "Zero threshold", "Nullschwelle"),
    ("res.complex_threshold", "Complex threshold", "Schwelle für komplexe Zahlen"),
    ("num.general", "General", "Allgemein"),
    ("num.decimal", "Decimal", "Dezimal"),
    ("num.scientific", "Scientific", "Wissenschaftlich"),
    ("num.engineering", "Engineering", "Ingenieurtechnisch"),
    ("num.percent", "Percent", "Prozent"),
    ("radix.decimal", "Decimal", "Dezimal"),
    ("radix.binary", "Binary", "Binär"),
    ("radix.octal", "Octal", "Oktal"),
    ("radix.hex", "Hexadecimal", "Hexadezimal"),
    (
        "res.unstorable_number",
        "Only the General result format can be saved in this file yet.",
        "In dieser Datei kann bisher nur das Ergebnisformat Allgemein gespeichert werden.",
    ),
    (
        "res.unstorable_complex",
        "The complex threshold cannot be saved in this file yet.",
        "Die Schwelle für komplexe Zahlen kann in dieser Datei bisher nicht gespeichert werden.",
    ),
    (
        "res.unstorable_grouping",
        "Digit grouping cannot be saved in this file yet.",
        "Das Tausendertrennzeichen kann in dieser Datei bisher nicht gespeichert werden.",
    ),
    (
        "res.unstorable_polar",
        "The polar complex form cannot be saved in this file yet.",
        "Die Polarform komplexer Zahlen kann in dieser Datei bisher nicht gespeichert werden.",
    ),
    ("res.digit_grouping", "Digit grouping", "Tausendertrennzeichen"),
    ("res.complex_form", "Complex values", "Komplexe Werte"),
    ("res.matrix_heading", "Matrix display (this region)", "Matrixdarstellung (dieser Bereich)"),
    (
        "res.matrix_needs_region",
        "Select a math region to set its matrix display. Matrix display is stored per region.",
        "Einen Mathematikbereich wählen, um seine Matrixdarstellung einzustellen. Sie wird je Bereich gespeichert.",
    ),
    ("res.matrix_rows", "Visible rows", "Sichtbare Zeilen"),
    ("res.matrix_cols", "Visible columns", "Sichtbare Spalten"),
    ("res.matrix_indices", "Show indices", "Indizes anzeigen"),
    ("res.matrix_collapse", "Collapse nested matrices", "Verschachtelte Matrizen verkleinern"),
    ("cplx.cartesian", "Cartesian", "Kartesisch"),
    ("cplx.polar_radians", "Polar, radians", "Polar, Radiant"),
    ("cplx.polar_degrees", "Polar, degrees", "Polar, Grad"),
    ("set.tol_positive", "TOL and CTOL must be positive numbers.", "TOL und CTOL müssen positive Zahlen sein."),
    ("set.origin", "ORIGIN (array origin)", "ORIGIN (Indexursprung)"),
    ("set.unit_system", "Unit system", "Einheitensystem"),
    ("set.tol", "TOL (convergence tolerance)", "TOL (Konvergenztoleranz)"),
    ("set.ctol", "CTOL (constraint tolerance)", "CTOL (Nebenbedingungstoleranz)"),
    // Save prompt.
    (
        "save.prompt",
        "Save the changes to {} before you {}?",
        "Änderungen an {} speichern, bevor Sie {}?",
    ),
    ("save.open", "open another worksheet", "ein anderes Arbeitsblatt öffnen"),
    ("save.new", "create a new worksheet", "ein neues Arbeitsblatt anlegen"),
    ("save.close", "close Mathcraft", "Mathcraft schließen"),
    ("save.draft", "a draft being typed is an unsaved edit", "ein gerade geschriebener Entwurf ist eine ungespeicherte Änderung"),
    ("save.disabled_lost", "{} disabled region(s) are saved as enabled: the file format of a disabled region is not verified.", "{} deaktivierte Bereiche werden als aktiv gespeichert: das Dateiformat deaktivierter Bereiche ist nicht verifiziert."),
    ("app.region_disabled", "Region disabled: it is not calculated (not saved in the file).", "Bereich deaktiviert: er wird nicht berechnet (nicht in der Datei gespeichert)."),
    ("app.region_enabled", "Region enabled.", "Bereich aktiviert."),
    ("save.cannot_save", "Cannot save {}: {}", "{} kann nicht gespeichert werden: {}"),
    // Window and file texts.
    ("app.title", "Mathcraft {} - {}", "Mathcraft {} - {}"),
    ("app.untitled", "Untitled.mcdx", "Unbenannt.mcdx"),
    ("app.worksheet", "Worksheet", "Arbeitsblatt"),
    ("app.loading", "Loading and evaluating {}...", "{} wird geladen und berechnet ..."),
    ("app.evaluating", "Evaluating...", "Wird berechnet ..."),
    ("app.cannot_open_file", "Cannot open the file: {}", "Die Datei kann nicht geöffnet werden: {}"),
    (
        "app.choose_other_file",
        "Choose another file in the File box (Ctrl+O) or drop a .mcdx file here.",
        "Eine andere Datei im Feld Datei wählen (Strg+O) oder eine .mcdx-Datei hier ablegen.",
    ),
    (
        "app.open_hint",
        "Open a .mcdx file: type its path in the File box and press Open,",
        "Eine .mcdx-Datei öffnen: Pfad im Feld Datei eingeben und Öffnen drücken,",
    ),
    (
        "app.no_file_save",
        "This worksheet has no file yet: type a file name in the File box and press Save As.",
        "Das Arbeitsblatt hat noch keine Datei: Dateinamen im Feld Datei eingeben und Speichern unter drücken.",
    ),
    ("app.saved", "Saved {}.", "{} gespeichert."),
    (
        "app.save_warning",
        "WARNING: {}.",
        "WARNUNG: {}.",
    ),
    (
        "app.save_warning_more",
        "({} more warnings.)",
        "({} weitere Warnungen.)",
    ),
    (
        "app.image_read_failed",
        "The image could not be read: {}",
        "Das Bild konnte nicht gelesen werden: {}",
    ),
    (
        "app.open_before_export",
        "Open a worksheet before exporting it.",
        "Vor dem Export ein Arbeitsblatt öffnen.",
    ),
    ("app.exported", "Exported {} page(s) to {}.", "{} Seite(n) nach {} exportiert."),
    ("app.cannot_write", "Cannot write {}: {}", "{} kann nicht geschrieben werden: {}"),
    ("app.export_failed", "Export failed: {}", "Export fehlgeschlagen: {}"),
    (
        "app.open_before_print",
        "Open a worksheet before printing it.",
        "Vor dem Drucken ein Arbeitsblatt öffnen.",
    ),
    ("app.print_failed", "Print failed: {}", "Drucken fehlgeschlagen: {}"),
    (
        "app.opened_viewer",
        "Opened {} page(s) in the system viewer: print from there.",
        "{} Seite(n) im Systemprogramm geöffnet: dort drucken.",
    ),
    (
        "app.print_needs_viewer",
        "Printing needs a PDF viewer: {}. The file is {}.",
        "Zum Drucken wird ein PDF-Programm benötigt: {}. Die Datei ist {}.",
    ),
    ("app.calc_mode_notice", "Calculation mode: {}.", "Berechnungsmodus: {}."),
    (
        "app.page_limit",
        " The page limit was reached; the rest is not on a page.",
        " Die Seitenobergrenze wurde erreicht; der Rest liegt nicht auf einer Seite.",
    ),
    ("app.the_file", "the file", "die Datei"),
    (
        "app.drop_hint",
        "or drop a .mcdx file onto this window. Ctrl+N starts a new worksheet.",
        "oder eine .mcdx-Datei auf dieses Fenster ziehen. Strg+N legt ein neues Arbeitsblatt an.",
    ),
    (
        "app.config_error",
        "The language could not be saved: {}",
        "Die Sprache konnte nicht gespeichert werden: {}",
    ),
    (
        "app.drop_multi",
        "Only {} was taken from the drop; {} more file(s) were ignored: {}.",
        "Nur {} wurde aus dem Drop übernommen; {} weitere Datei(en) wurden ignoriert: {}.",
    ),
    ("btn.ok", "OK", "OK"),
    ("edit.not_found", "region {} is not on the page", "Bereich {} ist nicht auf der Seite"),
    ("edit.not_in_sync", "the document changed: {}", "das Dokument hat sich geändert: {}"),
    // Result text of values (format.rs).
    ("val.matrix_shape", "matrix {}×{}", "Matrix {}×{}"),
    ("val.matrix", "matrix", "Matrix"),
    ("val.matrices", "matrices", "Matrizen"),
    ("val.item", "item", "Element"),
    ("val.items", "items", "Elemente"),
    ("val.list_empty", "[0 items]", "[0 Elemente]"),
    ("val.list", "[{} {}: {}]", "[{} {}: {}]"),
    ("val.error", "error {}", "Fehler {}"),
    ("val.plot_points", "plot data ({} points)", "Diagrammdaten ({} Punkte)"),
    ("val.symbolic", "symbolic <{}>", "symbolisch <{}>"),
    // Calculation results and comparison with Prime.
    ("res.matches_prime", "matches Prime's stored result", "stimmt mit dem gespeicherten Prime-Ergebnis überein"),
    ("res.engine_error_accepted", "engine error accepted for stored {}", "Rechenfehler für gespeichertes {} akzeptiert"),
    ("res.not_computed", "not computed", "nicht berechnet"),
    ("res.evaluation_stopped", "evaluation stopped", "Berechnung abgebrochen"),
    ("res.no_stored", "no stored result to compare with", "kein gespeichertes Ergebnis zum Vergleich"),
    ("res.plot_no_numbers", "plot evaluated; no stored numbers to compare with", "Diagramm berechnet; keine gespeicherten Zahlen zum Vergleich"),
    ("res.prime_stored", "Prime stored: {}", "Prime gespeichert: {}"),
    ("res.no_stored_short", "no stored result", "kein gespeichertes Ergebnis"),
    ("res.region", "region {}: {}", "Bereich {}: {}"),
    ("res.plot_not_drawn", "plot not drawn", "Diagramm nicht gezeichnet"),
    ("plot.traces_not_drawn", "{} of {} traces not drawn", "{} von {} Kurven nicht gezeichnet"),
    ("res.plot_not_evaluated", "plot not evaluated", "Diagramm nicht berechnet"),
    ("res.unsupported", "unsupported element", "nicht unterstütztes Element"),
    ("res.solve_block", "solve block", "Lösungsblock"),
    ("res.na", "n/a", "k. A."),
    // Load and file errors.
    ("load.cannot_open", "cannot open {}: {}", "{} kann nicht geöffnet werden: {}"),
    (
        "load.too_big",
        "cannot open {}: the file is {} bytes, over the {}-byte limit",
        "{} kann nicht geöffnet werden: die Datei hat {} Bytes, über der Grenze von {} Bytes",
    ),
    (
        "load.cannot_read_worksheet",
        "cannot read the worksheet in {}: {}",
        "Arbeitsblatt in {} kann nicht gelesen werden: {}",
    ),
    (
        "load.cannot_read_results",
        "cannot read the stored results of {}: {}",
        "Gespeicherte Ergebnisse von {} können nicht gelesen werden: {}",
    ),
    ("load.internal", "internal error while loading the file", "interner Fehler beim Laden der Datei"),
    ("load.thread", "cannot start the loading thread: {}", "Der Ladevorgang kann nicht gestartet werden: {}"),
    // PDF export.
    ("pdf.no_pages", "the document has no pages", "das Dokument hat keine Seiten"),
    ("pdf.font_missing", "font file {} is missing or unreadable", "Schriftdatei {} fehlt oder ist nicht lesbar"),
    ("pdf.too_large", "output of {} bytes is over {}", "Ausgabe mit {} Bytes ist größer als {}"),
    // Palette entries that are not built yet (the reason shows on hover).
    ("pal.member", "not implemented yet: the expression model has no membership test", "noch nicht implementiert: das Ausdrucksmodell hat keine Elementprüfung"),
    ("pal.logic", "not implemented yet: logical operators", "noch nicht implementiert: logische Operatoren"),
    // Math editor and region text.
    ("edit.kind_solve", "Solve block", "Lösungsblock"),
    ("edit.kind_plot", "Plot", "Diagramm"),
    ("edit.kind_table", "Table", "Tabelle"),
    ("edit.kind_area", "Area", "Bereich"),
    ("edit.kind_image", "Image", "Bild"),
    ("edit.lock_area", "Lock area", "Bereich sperren"),
    ("edit.unlock_area", "Unlock area", "Bereich entsperren"),
    (
        "edit.right_click_area",
        "Right-click inside an area to lock or unlock it.",
        "Rechtsklick in einen Bereich, um ihn zu sperren oder zu entsperren.",
    ),
    (
        "edit.area_needs_selection",
        "Select the regions to put in an area first.",
        "Zuerst die Bereiche auswählen, die in einen Bereich sollen.",
    ),
    (
        "edit.area_member_refused",
        "Only regions with a position can be put in an area, and a locked area cannot.",
        "Nur Bereiche mit Position können in einen Bereich gesetzt werden, ein gesperrter Bereich nicht.",
    ),
    ("edit.cmd_area", "Put in area", "In Bereich setzen"),
    (
        "edit.area_locked",
        "The area is locked. Unprotect it first.",
        "Der Bereich ist gesperrt. Zuerst die Sperre aufheben.",
    ),
    (
        "edit.image_format",
        "Only PNG and JPEG images can be inserted.",
        "Es können nur PNG- und JPEG-Bilder eingefügt werden.",
    ),
    (
        "edit.image_too_large",
        "The image file is larger than 4 MB.",
        "Die Bilddatei ist größer als 4 MB.",
    ),
    ("edit.kind_unsupported", "Unsupported", "Nicht unterstützt"),
    ("edit.kind_page_break", "Page break", "Seitenumbruch"),
    ("edit.this", "This", "Dieser Bereich"),
    (
        "edit.not_copyable",
        "{} regions cannot be copied in this stage.",
        "Bereiche vom Typ {} können in dieser Stufe nicht kopiert werden.",
    ),
    (
        "edit.clip_bad_math",
        "A math line on the clipboard does not read: {}.",
        "Eine Mathezeile in der Zwischenablage lässt sich nicht lesen: {}.",
    ),
    ("save.open_path", "open {}", "{} öffnen"),
    ("pdf.too_many_pages", "{} pages, the limit is {}", "{} Seiten, die Grenze ist {}"),
    ("edit.clip_too_large", "The clipboard text is larger than {} KiB.", "Der Text in der Zwischenablage ist größer als {} KiB."),
    ("edit.clip_max_regions", "Copy at most {} regions at a time.", "Höchstens {} Bereiche auf einmal kopieren."),
    ("edit.clip_damaged", "The clipboard regions are damaged: {}.", "Die Bereiche in der Zwischenablage sind beschädigt: {}."),
    ("edit.cmd_insert", "Insert region", "Bereich einfügen"),
    ("edit.cmd_delete", "Delete region", "Bereich löschen"),
    ("edit.cmd_delete_many", "Delete regions", "Bereiche löschen"),
    ("edit.cmd_edit", "Edit region", "Bereich bearbeiten"),
    ("edit.cmd_move", "Move region", "Bereich verschieben"),
    ("edit.cmd_arrange", "Arrange regions", "Bereiche anordnen"),
    ("edit.cmd_insert_lines", "Insert lines", "Leerzeilen einfügen"),
    ("edit.field_none", "The header and footer have no such field.", "Kopf- und Fußzeile enthalten kein solches Feld."),
    ("edit.cmd_remove_lines", "Remove space", "Leerzeile entfernen"),
    ("edit.cmd_separate", "Separate regions", "Bereiche trennen"),
    ("edit.no_space_below", "There is no empty space below the insertion point.", "Unter der Einfügemarke ist kein leerer Raum."),
    ("edit.separate_needs_two", "Select two or more regions to separate them.", "Zum Trennen zwei oder mehr Bereiche auswählen."),
    ("edit.nothing_overlaps", "The selected regions do not overlap.", "Die ausgewählten Bereiche überlappen sich nicht."),
    ("app.calc_stopped", "The calculation was stopped.", "Die Berechnung wurde angehalten."),
    ("app.no_calc_running", "No calculation is running.", "Es läuft keine Berechnung."),
    ("app.no_errors", "No region shows an error.", "Kein Bereich zeigt einen Fehler."),
    ("dlg.resource", "Resource", "Ressource"),
    ("res.target_web", "Mathcraft does not open a web browser. The address is:", "Mathcraft öffnet keinen Webbrowser. Die Adresse lautet:"),
    ("res.target_help", "This opens the help topic:", "Dies öffnet das Hilfethema:"),
    ("res.unverified", "The address is not shown in Prime's screenshots (UNVERIFIED).", "Die Adresse ist in den Prime-Screenshots nicht zu sehen (UNVERIFIED)."),
    ("btn.copy_address", "Copy", "Kopieren"),
    ("dlg.unit_system", "Unit system", "Einheitensystem"),
    ("units.pick_system", "Select unit system:", "Einheitensystem auswählen:"),
    ("units.custom", "Custom", "Benutzerdefiniert"),
    ("units.based_on", "Based on:", "Basierend auf:"),
    ("units.base_dims", "Base dimensions:", "Basisdimensionen:"),
    ("units.derived", "Derived units:", "Abgeleitete Einheiten:"),
    ("units.change", "Change", "Ändern"),
    ("units.insert", "Insert", "Einfügen"),
    ("units.remove", "Remove", "Entfernen"),
    ("units.help", "Help", "Hilfe"),
    ("units.custom_note", "Custom unit systems cannot be stored in the file yet (UNVERIFIED); OK keeps the system it is based on.", "Benutzerdefinierte Einheitensysteme lassen sich noch nicht in der Datei speichern (UNVERIFIED); OK behält das zugrunde liegende System."),
    ("units.predefined_note", "Predefined systems cannot be changed.", "Vordefinierte Systeme können nicht geändert werden."),
    ("dlg.base_dimension_unit", "Base dimension unit", "Basisdimensions-Einheit"),
    ("dlg.derived_units", "Derived units", "Abgeleitete Einheiten"),
    ("units.dimension", "Dimension", "Dimension"),
    ("units.unit", "Unit", "Einheit"),
    ("edit.cmd_paste", "Paste regions", "Bereiche einfügen"),
    ("edit.cmd_duplicate", "Duplicate regions", "Bereiche duplizieren"),
    ("edit.solve_readonly", "Solve-block members are read-only in this stage.", "Glieder eines Lösungsblocks sind in dieser Stufe nur lesbar."),
    ("edit.click_first", "Click on the page to place the insertion point first.", "Zuerst auf die Seite klicken, um die Einfügemarke zu setzen."),
    ("edit.text_in_math", "Text goes into a math region.", "Text gehört in einen Rechenbereich."),
    ("edit.click_text_first", "Click on the page to place the text first.", "Zuerst auf die Seite klicken, um den Text zu setzen."),
    ("edit.cannot_read", "Cannot read the expression: {}", "Der Ausdruck kann nicht gelesen werden: {}"),
    ("edit.select_first", "Select a region first.", "Zuerst einen Bereich wählen."),
    ("edit.no_position", "This region has no stored position, so it cannot be moved.", "Dieser Bereich hat keine gespeicherte Position und kann daher nicht verschoben werden."),
    ("edit.select_one", "Select one region to edit it.", "Einen Bereich wählen, um ihn zu bearbeiten."),
    ("edit.click_insert", "Click on the page to place the insertion point.", "Auf die Seite klicken, um die Einfügemarke zu setzen."),
    ("edit.plot_op_nothing", "Nothing to change: a plot keeps at least one trace, and a marker must exist to be deleted.", "Nichts zu ändern: Ein Diagramm behält mindestens eine Spur, und eine Markierung muss existieren, um gelöscht zu werden."),
    ("edit.not_editable", "This element cannot be edited in this stage.", "Dieses Element kann in dieser Stufe nicht bearbeitet werden."),
    ("edit.field_needs_header", "Open the header or footer first (double-click its margin), then insert the field.", "Zuerst die Kopf- oder Fußzeile öffnen (Doppelklick auf den Rand), dann das Feld einfügen."),
    ("edit.page_break_needs_point", "Click on the page where the page break goes.", "Auf die Seite klicken, wo der Seitenumbruch stehen soll."),
    ("edit.page_break_none", "No page break near the insertion point. Click on its dashed line first.", "Kein Seitenumbruch an der Einfügemarke. Zuerst auf seine gestrichelte Linie klicken."),
    ("edit.arrange_min", "Select at least {} movable regions to arrange them.", "Mindestens {} verschiebbare Bereiche wählen, um sie anzuordnen."),
    ("edit.select_part_copy", "Select part of the expression to copy it.", "Einen Teil des Ausdrucks wählen, um ihn zu kopieren."),
    ("edit.finish_text_copy", "Finish typing the text to copy regions.", "Den Text fertig schreiben, um Bereiche zu kopieren."),
    ("edit.select_copy", "Select a region to copy it.", "Einen Bereich wählen, um ihn zu kopieren."),
    ("edit.select_part_cut", "Select part of the expression to cut it.", "Einen Teil des Ausdrucks wählen, um ihn auszuschneiden."),
    ("edit.paste_max", "Paste at most {} characters at a time.", "Höchstens {} Zeichen auf einmal einfügen."),
    ("edit.click_paste", "Click on the page to place the paste.", "Auf die Seite klicken, um die Einfügung zu setzen."),
    ("edit.paste_line", "Line {} of the paste does not read as math: {}.", "Zeile {} der Einfügung wird nicht als Mathematik gelesen: {}."),
    ("edit.too_many", "The worksheet has too many regions.", "Das Arbeitsblatt hat zu viele Bereiche."),
    (
        "edit.structural",
        "Structural editing is not available for this expression, so it was left unchanged.",
        "Strukturbearbeitung ist für diesen Ausdruck nicht verfügbar, daher blieb er unverändert.",
    ),
    // Errors the calculation reports (see messages.rs).
    ("err.division_by_zero", "division by zero", "Division durch null"),
    ("err.singular", "matrix is singular", "Matrix ist singulär"),
    ("err.not_finite", "result is not finite", "Ergebnis ist nicht endlich"),
    ("err.overflow", "arithmetic overflow", "Arithmetischer Überlauf"),
    ("err.undefined_name", "undefined name `{}`", "Name `{}` ist nicht definiert"),
    ("err.undefined_function", "undefined function `{}`", "Funktion `{}` ist nicht definiert"),
    (
        "err.work_limit",
        "work limit exceeded: this expression is too expensive; reduce the size or the argument range",
        "Rechenlimit überschritten: dieser Ausdruck ist zu aufwendig; Größe oder Argumentbereich verkleinern",
    ),
    ("err.dimension", "dimension mismatch: {} and {}", "Dimensionen passen nicht zusammen: {} und {}"),
    ("err.not_dimensionless", "expected a dimensionless value, found {}", "Eine dimensionslose Größe wurde erwartet, gefunden: {}"),
    ("err.index", "index {} out of range (bound {})", "Index {} außerhalb des Bereichs (Grenze {})"),
    ("err.call_limit", "more than {} function calls in one evaluation", "mehr als {} Funktionsaufrufe in einer Berechnung"),
    ("err.not_converged", "{} did not converge", "{} konvergiert nicht"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_keys_translate() {
        assert_eq!(tr_in(Lang::English, "tab.math"), "Math");
        assert_eq!(tr_in(Lang::English, "btn.function"), "Function... (Ctrl+E)");
        assert_eq!(tr_in(Lang::German, "tab.math"), "Rechnen");
        assert_eq!(tr_in(Lang::German, "tab.functions"), "Funktionen");
        assert_eq!(tr_in(Lang::German, "btn.calculate"), "Jetzt berechnen (F9)");
    }

    #[test]
    fn unknown_keys_show_the_key() {
        assert_eq!(tr_in(Lang::German, "no.such.key"), "no.such.key");
        assert_eq!(tr_in(Lang::English, "no.such.key"), "no.such.key");
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<&str> = STRINGS.iter().map(|(k, _, _)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len());
    }

    #[test]
    fn every_key_has_an_english_and_a_german_text() {
        for (key, english, german) in STRINGS {
            assert!(!english.is_empty(), "empty English text for {key}");
            assert!(!german.is_empty(), "empty German text for {key}");
        }
    }

    #[test]
    fn placeholders_match_between_languages() {
        for (key, english, german) in STRINGS {
            assert_eq!(
                english.matches("{}").count(),
                german.matches("{}").count(),
                "placeholder count differs for {key}"
            );
        }
    }

    #[test]
    fn every_term_has_a_german_text() {
        for (english, german) in TERMS {
            assert!(!english.is_empty());
            assert!(!german.is_empty(), "empty German term for {english}");
        }
    }

    #[test]
    fn every_key_used_in_the_window_sources_exists() {
        let sources = [
            include_str!("ribbon.rs"),
            include_str!("dialogs.rs"),
            include_str!("app.rs"),
            include_str!("settings_window.rs"),
            include_str!("catalog.rs"),
            include_str!("viewmodel.rs"),
            include_str!("edit/session.rs"),
            include_str!("edit/command.rs"),
            include_str!("edit/clipboard.rs"),
            include_str!("edit/save.rs"),
            include_str!("edit/shortcut.rs"),
            include_str!("load.rs"),
            include_str!("loader.rs"),
            include_str!("pdf.rs"),
            include_str!("file_picker.rs"),
            include_str!("plot.rs"),
            include_str!("plot3d.rs"),
            include_str!("canvas.rs"),
            include_str!("export.rs"),
            include_str!("messages.rs"),
            include_str!("palette.rs"),
        ];
        for source in sources {
            for marker in ["tr(\"", "trf(\"", "tr_in(Lang::English, \""] {
                for (at, _) in source.match_indices(marker) {
                    let rest = &source[at + marker.len()..];
                    let Some(end) = rest.find('"') else { continue };
                    let key = &rest[..end];
                    if key.contains('.') {
                        assert!(
                            STRINGS.iter().any(|(k, _, _)| *k == key),
                            "key {key} is used but not in the table"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fill_replaces_placeholders_in_order() {
        assert_eq!(fill("{} x {}", &["2", "3"]), "2 x 3");
        assert_eq!(fill("Version {}", &[]), "Version {}");
        assert_eq!(fill("no holes", &["x"]), "no holes");
    }

    #[test]
    fn german_term_lookup_falls_back_to_the_name() {
        assert_eq!(
            TERMS.iter().find(|(e, _)| *e == "length").map(|t| t.1),
            Some("Länge")
        );
    }

    #[test]
    fn language_codes_round_trip() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code("fr"), None);
        assert_eq!(Lang::from_code(" DE "), Some(Lang::German));
    }

    #[test]
    fn locale_names_pick_a_language() {
        assert_eq!(lang_from_locale("de_DE.UTF-8"), Some(Lang::German));
        assert_eq!(lang_from_locale("de-DE"), Some(Lang::German));
        assert_eq!(lang_from_locale("en_US"), Some(Lang::English));
        assert_eq!(lang_from_locale("C"), None);
        assert_eq!(lang_from_locale("fr_FR"), None);
        assert_eq!(lang_from_locale(""), None);
    }

    #[test]
    fn environment_decides_before_the_defaults() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_string())
            }
        };
        assert_eq!(
            detect_from_env(env(&[("LANG", "de_DE.UTF-8")])),
            Some(Lang::German)
        );
        assert_eq!(
            detect_from_env(env(&[("LC_ALL", "en_GB"), ("LANG", "de_DE")])),
            Some(Lang::English)
        );
        assert_eq!(
            detect_from_env(env(&[("LC_ALL", "C"), ("LANG", "de")])),
            Some(Lang::German)
        );
        assert_eq!(
            detect_from_env(env(&[("LANGUAGE", "de:en")])),
            Some(Lang::German)
        );
        assert_eq!(detect_from_env(env(&[("LANG", "C")])), None);
        assert_eq!(detect_from_env(env(&[])), None);
    }

    #[test]
    fn apple_language_list_takes_the_first_known_entry() {
        let text = "(\n    \"fr-FR\",\n    \"de-DE\",\n    \"en-US\"\n)\n";
        assert_eq!(lang_from_apple_languages(text), Some(Lang::German));
        assert_eq!(lang_from_apple_languages("(\n)"), None);
    }

    #[test]
    fn german_uses_the_decimal_comma() {
        assert_eq!(Lang::German.separators(), NumberSeparators::GERMAN);
        assert_eq!(Lang::English.separators(), NumberSeparators::INVARIANT);
    }
}

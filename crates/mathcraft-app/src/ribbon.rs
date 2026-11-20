//! Window chrome: the ribbon (ten tabs, one fixed-height row, declared in `ribbon/`), the document
//! strip with the file buttons and the path box, and the status bar. Buttons that do something
//! return an [`Action`] for the window to apply; buttons for commands that do not exist yet are
//! greyed with the tooltip "not available yet", never faked. How to add a tab or a button:
//! `documentation/UI_PARITY_RIBBON.md`.

use eframe::egui::{self, Color32, RichText, Ui};

use crate::dialogs::Dialog;
use crate::edit::layout::Arrange;
use crate::edit::math::{MatrixOp, Template};
use crate::edit::plot::PlotOp;
use crate::edit::session::EditSession;
use crate::i18n::{tr, Lang};
use crate::view_state::{CalcMode, DisplayFlags};

pub mod calculation;
pub mod doc_info;
pub mod document;
pub mod functions;
pub mod io;
pub mod math;
pub mod math_format;
pub mod matrices;
pub mod plots;
pub mod render;
pub mod resources;
pub mod spec;
pub mod text_format;

/// The ribbon tabs, in Prime's order (keys into `i18n.rs`).
pub const TABS: [&str; 10] = [
    "tab.math",
    "tab.io",
    "tab.functions",
    "tab.matrices",
    "tab.plots",
    "tab.math_format",
    "tab.text_format",
    "tab.calculation",
    "tab.document",
    "tab.resources",
];

/// A zoom command from the View tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomStep {
    In,
    Out,
    Reset,
}

/// What a ribbon button asks the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A new math region at the insertion point.
    InsertMath,
    /// A new text region at the insertion point.
    InsertText,
    /// Pick a PNG or JPEG file and insert it at the insertion point.
    InsertImage,
    /// An empty area at the insertion point.
    InsertArea,
    /// A solve block skeleton (Given/Find) at the insertion point.
    InsertSolveBlock,
    /// A text field at the insertion point (Mathcraft stores it as a text region).
    InsertTextField,
    /// Switch the selected regions off or on (they are not evaluated).
    ToggleDisabled,
    /// An empty x-y plot (`false`) or polar plot (`true`) at the insertion point.
    InsertPlot(bool),
    /// Open the plot window for the selected plot.
    EditPlot,
    /// Put the selected regions into a new area that surrounds them.
    AreaAroundSelection,
    /// A template typed into the math draft.
    Template(Template),
    /// Open a dialog.
    Dialog(Dialog),
    /// Evaluate the worksheet now (F9).
    Calculate,
    /// Open the File access window (reading and writing files, for this session only).
    FileAccess,
    /// Switch between automatic and manual calculation.
    ToggleCalcMode,
    /// Zoom the page.
    Zoom(ZoomStep),
    /// Show or hide the grid.
    ToggleGrid,
    /// Show or hide the region outlines.
    ToggleRegionBorders,
    /// Switch between the draft view (one endless sheet) and the page layout.
    TogglePageLayout,
    /// Edit the page header (false) or footer (true) at its place on the page.
    EditFurniture(bool),
    /// Type a header or footer field such as `{page}` into the header or footer being edited.
    InsertField(&'static str),
    /// Insert a page break at the insertion point: the next page starts there.
    InsertPageBreak,
    /// Remove the selected page break, or the one at the insertion point.
    DeletePageBreak,
    /// Add or delete a matrix row or column at the caret.
    MatrixOp(MatrixOp),
    /// An n by m matrix (rows, columns) at the insertion point or into the math draft.
    InsertMatrix(usize, usize),
    /// Show or hide the row and column indices of the matrix results of the target math region.
    ToggleMatrixIndices,
    /// Collapse or expand the nested matrices of the target math region's results.
    ToggleMatrixNested,
    /// A change of the selected plot (traces, markers, axes, trace style, chart type).
    PlotOp(PlotOp),
    /// Align or distribute the selected regions.
    Arrange(Arrange),
    /// Insert lines: vertical space at the insertion point.
    InsertLines,
    /// Open the Results format window (the worksheet, or the selected region).
    ResultFormat,
    /// Open the Worksheet options window.
    WorksheetOptions,
    /// Switch the display language (and save the choice).
    SetLanguage(Lang),
    /// Type this text into the math draft (a palette cell).
    TypeText(&'static str),
    /// Type `name(` into the math draft (a function of the Functions tab).
    InsertFunction(&'static str),
    /// Delete the selected regions.
    DeleteSelected,
    /// Copy the selection to the clipboard and delete it.
    Cut,
    /// Copy the selection to the clipboard.
    Copy,
    /// Paste the system clipboard (the window asks for it; it arrives as a paste event).
    Paste,
    /// ORIGIN := 0 or 1 (Berechnung tab).
    SetOrigin(u32),
    /// TOL := 10^n.
    SetTol(i32),
    /// CTOL := 10^n.
    SetCtol(i32),
    /// Stop the calculation that is running.
    StopCalc,
    /// Select an erroneous region: the first, the previous, the next or the last.
    ErrorTrace(Trace),
    /// Set the paper (a code of `known_paper_codes`).
    SetPaper(&'static str),
    /// Set the orientation: `true` is landscape.
    SetLandscape(bool),
    /// Set the margin type (`Standard`, `Narrow`, `Wide`, `Legacy`).
    SetMargins(&'static str),
    /// Set the grid size: `true` is Fine.
    SetFineGrid(bool),
    /// Open the page setup window (custom margins).
    PageSetup,
    /// Show the page (`true`) or the draft view.
    SetPageView(bool),
    /// Separate the selected overlapping regions: vertically (`true`) or horizontally.
    SeparateRegions(bool),
    /// Insert a blank page: the next page starts at the insertion point.
    InsertBlankPage,
    /// Remove the blank line at the insertion point.
    RemoveLines,
    /// Remove the empty space between the regions around the insertion point.
    RemoveEmptyArea,
    /// Remove the fields with this name (`page`, `date`) from the header and footer.
    RemoveField(&'static str),
    /// Open the Einheitensystem window.
    UnitSystem,
    /// Show where a Resources button leads (a web address or a help topic): never opens a browser.
    Resource(&'static str),
    /// Apply a character or paragraph format to the selected text being typed.
    TextFormat(TextFormat),
    /// Open the Link window for the selected text (or the caret).
    TextLink,
}

/// A move along the erroneous regions (Fehlerrückverfolgung).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trace {
    First,
    Previous,
    Next,
    Last,
}

/// A format of the Textformatierung tab, applied to the selected text of a text region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    /// Bold on the selection, or off when all of it is bold already.
    ToggleBold,
    /// Italic on the selection, or off when all of it is italic already.
    ToggleItalic,
    /// Underline on the selection, or off when all of it is underlined already.
    ToggleUnderline,
    /// Text colour, red, green, blue and alpha.
    Colour([u8; 4]),
    /// Font family (a name of Prime's list).
    Font(&'static str),
    /// Font size in whole points (Prime's list is whole points).
    Size(u16),
    /// Paragraph alignment of the selected paragraphs.
    Align(mathcraft_import::rich_text::Align),
    /// A named text style of the Textstile gallery.
    Look(TextLook),
    /// Remove the link from the selection.
    Unlink,
}

/// The named text styles of the Textstile gallery. Their looks are those of Prime's default
/// text styles (`presentation.xml` of the samples: `textStyle` elements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextLook {
    Normal,
    Title,
    Heading1,
    Heading2,
}

/// What a file button or the path box asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    New,
    Save,
    /// Open the file at the path (text as typed; `~/` is expanded by the caller).
    Open(String),
    /// Save to the path (text as typed; `~/` is expanded by the caller).
    SaveAs(String),
    /// Choose the file to open in a file dialog (the File box is empty).
    BrowseOpen,
    /// Choose where to save in a file dialog (the File box is empty).
    BrowseSaveAs,
    /// Write the pages as a PDF: to the path in the box when it ends in `.pdf`, otherwise next to
    /// the worksheet's file.
    ExportPdf,
    /// Export to a temporary PDF and open it with the system's default viewer.
    Print,
    /// Open the page setup window.
    PageSetup,
}

/// What is being edited, for the enabled-rules of the buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editing {
    None,
    Math,
    Text,
}

/// The state the ribbon shows its toggles and enabled buttons from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RibbonState {
    /// False when no document is open, so the buttons that edit are greyed out.
    pub enabled: bool,
    /// True while exactly one 2D plot is selected.
    pub plot_selected: bool,
    pub calc_mode: CalcMode,
    pub display: DisplayFlags,
    /// A region is being typed.
    pub editing: Editing,
    /// Number of selected regions.
    pub selected: usize,
    /// The worksheet's settings and page setup.
    pub doc: doc_info::DocInfo,
    /// Number of regions that show an error (for the error trace buttons).
    pub errors: usize,
    /// The text being typed has a selection (the Textformatierung buttons need one).
    pub text_selected: bool,
    /// The selected text has a hyperlink in it.
    pub text_linked: bool,
    /// The selected plot has a marker to delete.
    pub plot_marker: bool,
    /// The selected plot is a polar plot.
    pub plot_polar: bool,
    /// A math region is selected or being edited: the matrix result flags apply to it.
    pub matrix_target: bool,
    /// The target region shows row and column indices.
    pub show_indices: bool,
    /// The target region collapses nested matrices.
    pub shrink_nested: bool,
}

impl RibbonState {
    /// The state for `session` (`None`: no worksheet open).
    pub fn new(session: Option<&EditSession>, calc_mode: CalcMode, display: DisplayFlags) -> Self {
        let editing = match session.and_then(|s| s.draft_view()) {
            Some(d) if d.math => Editing::Math,
            Some(_) => Editing::Text,
            None => Editing::None,
        };
        let text_styles = session
            .map(|s| s.selected_text_styles())
            .unwrap_or_default();
        let target = session.and_then(|s| {
            let id = s.matrix_target()?;
            s.document().region(id)
        });
        Self {
            enabled: session.is_some(),
            plot_selected: session.is_some_and(|s| s.selected_plot().is_some()),
            calc_mode,
            display,
            editing,
            selected: session.map_or(0, |s| s.selected().len()),
            doc: doc_info::DocInfo::of(session),
            errors: 0,
            text_selected: !text_styles.is_empty(),
            text_linked: text_styles.iter().any(|st| st.link.is_some()),
            plot_marker: session
                .and_then(|s| s.selected_plot_model())
                .is_some_and(crate::edit::plot::has_marker),
            plot_polar: session
                .and_then(|s| s.selected_plot_model())
                .is_some_and(|p| p.polar),
            matrix_target: target.is_some(),
            show_indices: target.is_some_and(|r| r.matrix.show_indices),
            shrink_nested: target.is_some_and(|r| !r.matrix.expand_nested),
        }
    }

    /// The same state with the number of erroneous regions.
    pub fn with_errors(mut self, errors: usize) -> Self {
        self.errors = errors;
        self
    }
}

/// Size of the tab labels.
const TAB_TEXT: f32 = 13.5;

/// The ribbon's tab row, with the LearningConnector and help icons at the far right. Returns the
/// index of the selected tab.
pub fn tab_row(ui: &mut Ui, selected: usize) -> usize {
    let mut chosen = selected;
    // The labels shrink (down to 9 pt) in a narrow window, so the row never widens the window.
    let avail = ui.available_width() - 50.0;
    let width_at = |size: f32| -> f32 {
        TABS.iter()
            .map(|key| {
                let job = egui::text::LayoutJob::simple(
                    tr(key).to_owned(),
                    egui::FontId::proportional(size),
                    Color32::BLACK,
                    f32::INFINITY,
                );
                ui.fonts_mut(|f| f.layout_job(job)).size().x + 10.0
            })
            .sum()
    };
    let mut size = TAB_TEXT;
    while size > 9.0 && width_at(size) > avail {
        size -= 0.5;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, key) in TABS.iter().enumerate() {
            let name = tr(key);
            let text = if i == selected {
                RichText::new(name).size(size).strong()
            } else {
                RichText::new(name).size(size)
            };
            if ui.selectable_label(i == selected, text).clicked() {
                chosen = i;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled(false, egui::Button::new("?").frame(false))
                .on_disabled_hover_text(format!(
                    "{}\n{}",
                    tr("tip.help_icon"),
                    tr("tip.not_available")
                ));
            ui.add_enabled(false, egui::Button::new("\u{1F393}").frame(false))
                .on_disabled_hover_text(format!("LearningConnector\n{}", tr("tip.not_available")));
        });
    });
    chosen
}

/// The ribbon body for the selected tab: one row of fixed height (see [`render`]).
pub fn tab_body(ui: &mut Ui, tab: usize, state: &RibbonState) -> Option<Action> {
    let tabs = spec::tabs();
    let spec = tabs.get(tab)?;
    ui.add_space(2.0);
    let picked = render::tab_body(ui, spec, state);
    ui.add_space(2.0);
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ribbon::spec::{Rule, Status};

    fn state() -> RibbonState {
        RibbonState {
            enabled: true,
            plot_selected: false,
            calc_mode: CalcMode::Auto,
            display: DisplayFlags::default(),
            editing: Editing::None,
            selected: 0,
            doc: doc_info::DocInfo::default(),
            errors: 0,
            text_selected: false,
            text_linked: false,
            plot_marker: false,
            plot_polar: false,
            matrix_target: false,
            show_indices: false,
            shrink_nested: false,
        }
    }

    fn button(tab: usize, id: &str) -> spec::Button {
        *spec::tabs()[tab]
            .groups
            .iter()
            .flat_map(|g| g.buttons())
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("no button {id}"))
    }

    #[test]
    fn there_are_ten_tabs_in_prime_order() {
        let german: Vec<&str> = TABS
            .iter()
            .map(|k| crate::i18n::tr_in(Lang::German, k))
            .collect();
        assert_eq!(
            german,
            [
                "Rechnen",
                "Eingabe/Ausgabe",
                "Funktionen",
                "Matrizen/Tabellen",
                "Diagramme",
                "Mathematische Formatierung",
                "Textformatierung",
                "Berechnung",
                "Dokument",
                "Ressourcen"
            ]
        );
        assert_eq!(spec::tabs().len(), TABS.len());
        for (spec, key) in spec::tabs().iter().zip(TABS) {
            assert_eq!(spec.key, key);
        }
    }

    #[test]
    fn textblock_is_disabled_while_a_region_is_selected_or_edited() {
        let text = button(0, "textblock");
        assert_eq!(text.status(&state()), Status::Ready);
        let selected = RibbonState {
            selected: 1,
            ..state()
        };
        assert_eq!(text.status(&selected), Status::Disabled);
        let editing = RibbonState {
            editing: Editing::Math,
            ..state()
        };
        assert_eq!(text.status(&editing), Status::Disabled);
        let closed = RibbonState {
            enabled: false,
            ..state()
        };
        assert_eq!(text.status(&closed), Status::Disabled);
    }

    #[test]
    fn delete_region_needs_a_selection() {
        let delete = button(0, "delete");
        assert_eq!(delete.status(&state()), Status::Disabled);
        assert_eq!(
            delete.status(&RibbonState {
                selected: 2,
                ..state()
            }),
            Status::Ready
        );
        assert_eq!(delete.action, Some(Action::DeleteSelected));
    }

    #[test]
    fn xy_plot_is_greyed_while_editing_and_edit_needs_a_plot() {
        assert!(Rule::NoDraft.holds(&state()));
        assert!(!Rule::NoDraft.holds(&RibbonState {
            editing: Editing::Math,
            ..state()
        }));
        assert!(!Rule::Plot.holds(&state()));
        assert!(Rule::Plot.holds(&RibbonState {
            plot_selected: true,
            ..state()
        }));
        assert!(Rule::MathDraft.holds(&RibbonState {
            editing: Editing::Math,
            ..state()
        }));
        assert!(!Rule::MathDraft.holds(&RibbonState {
            editing: Editing::Text,
            ..state()
        }));
    }

    #[test]
    fn missing_features_are_not_available_whatever_the_state() {
        assert_eq!(button(1, "convert").status(&state()), Status::NotAvailable);
    }

    #[test]
    fn solve_block_text_field_and_disable_enable_with_their_rules() {
        let selected = RibbonState {
            selected: 1,
            ..state()
        };
        // Solve block and Text field: enabled on a fresh sheet, greyed while a region is selected.
        for (tab, id) in [(0, "solve"), (0, "textfield"), (8, "textfield")] {
            let b = button(tab, id);
            assert_eq!(b.status(&state()), Status::Ready, "{id}");
            assert_eq!(b.status(&selected), Status::Disabled, "{id} selected");
        }
        assert_eq!(button(0, "solve").action, Some(Action::InsertSolveBlock));
        // Disable region follows the selection.
        let disable = button(7, "disable");
        assert_eq!(disable.status(&state()), Status::Disabled);
        assert_eq!(disable.status(&selected), Status::Ready);
    }

    #[test]
    fn plot_rule_follows_the_plot_selection() {
        let plot = RibbonState {
            plot_selected: true,
            selected: 1,
            ..state()
        };
        assert!(Rule::Plot.holds(&plot));
        assert!(!Rule::Plot.holds(&state()));
    }

    #[test]
    fn buttons_map_to_the_editing_commands() {
        assert_eq!(button(0, "calc").action, Some(Action::InsertMath));
        assert_eq!(button(0, "textblock").action, Some(Action::InsertText));
        assert_eq!(button(0, "image").action, Some(Action::InsertImage));
        assert_eq!(
            button(2, "all").action,
            Some(Action::Dialog(Dialog::Function(None)))
        );
        assert_eq!(button(7, "now").action, Some(Action::Calculate));
        assert_eq!(
            button(3, "delcol").action,
            Some(Action::MatrixOp(MatrixOp::DeleteColumn))
        );
        let Some(spec::Menu::Items(items)) = button(8, "format").menu_of() else {
            panic!("Bereichsformatierung is a dropdown");
        };
        assert!(items
            .iter()
            .any(|i| i.action == Some(Action::Arrange(Arrange::AlignLeft))));
    }

    /// The actions of every line of a button's menu, submenus included.
    fn menu_actions(items: &'static [spec::Item]) -> Vec<Action> {
        items
            .iter()
            .flat_map(|i| match i.sub {
                Some(sub) => menu_actions(sub),
                None => i.action.into_iter().collect(),
            })
            .collect()
    }

    #[test]
    fn calculation_fields_offer_prime_values_and_mark_the_current_one() {
        let origin = button(7, "origin");
        let Some(spec::Menu::Items(items)) = origin.menu_of() else {
            panic!("ORIGIN is a list");
        };
        assert_eq!(
            menu_actions(items),
            [Action::SetOrigin(0), Action::SetOrigin(1)]
        );
        let Some(spec::Menu::Items(tol)) = button(7, "tol").menu_of() else {
            panic!("TOL is a list");
        };
        assert_eq!(tol.len(), 7);
        assert!(tol[2].mark.on(&state()), "10^-3 is the default");
        assert!(!tol[0].mark.on(&state()));
        let other = RibbonState {
            doc: doc_info::DocInfo {
                origin: 1,
                tol: 1e-5,
                ..doc_info::DocInfo::default()
            },
            ..state()
        };
        assert!(tol[4].mark.on(&other));
        assert!(items[1].mark.on(&other));
    }

    #[test]
    fn calculate_buttons_are_active_only_in_manual_mode() {
        for id in ["sheet", "now"] {
            let b = button(7, id);
            assert_eq!(b.status(&state()), Status::Disabled, "{id} in AUTO");
            let manual = RibbonState {
                calc_mode: CalcMode::Manual,
                ..state()
            };
            assert_eq!(b.status(&manual), Status::Ready, "{id} in MANUAL");
        }
    }

    #[test]
    fn error_trace_needs_an_erroneous_region() {
        let next = button(7, "next");
        assert_eq!(next.status(&state()), Status::Disabled);
        assert_eq!(next.status(&state().with_errors(2)), Status::Ready);
    }

    #[test]
    fn page_menus_cover_prime_lists() {
        let Some(spec::Menu::Items(items)) = button(8, "page").menu_of() else {
            panic!("Seite is a dropdown");
        };
        let actions = menu_actions(items);
        for paper in ["Letter", "Legal", "Tabloid", "A3", "A4", "A5"] {
            assert!(actions.contains(&Action::SetPaper(paper)), "{paper}");
        }
        for margins in ["Standard", "Narrow", "Wide", "Legacy"] {
            assert!(actions.contains(&Action::SetMargins(margins)), "{margins}");
        }
        assert!(actions.contains(&Action::PageSetup));
        assert!(actions.contains(&Action::SetFineGrid(true)));
        assert!(actions.contains(&Action::SetLandscape(true)));
        let a4 = RibbonState {
            doc: doc_info::DocInfo {
                paper: "A5",
                ..doc_info::DocInfo::default()
            },
            ..state()
        };
        assert!(spec::Flag::Paper("A5").on(&a4));
        assert!(!spec::Flag::Paper("A4").on(&a4));
    }

    #[test]
    fn header_menu_inserts_fields() {
        let Some(spec::Menu::Items(items)) = button(8, "header").menu_of() else {
            panic!("Kopfzeilen und Fußzeilen is a dropdown");
        };
        let actions = menu_actions(items);
        assert!(actions.contains(&Action::EditFurniture(false)));
        assert!(actions.contains(&Action::EditFurniture(true)));
        assert!(actions.contains(&Action::InsertField("{page}")));
        assert!(actions.contains(&Action::InsertField("{filename}")));
        assert!(actions.contains(&Action::RemoveField("page")));
        assert!(actions.contains(&Action::RemoveField("date")));
    }

    #[test]
    fn every_menu_line_of_the_three_tabs_is_wired_or_explained() {
        // A greyed line is allowed, but it must be one the parity document lists by name.
        let doc = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../documentation/UI_PARITY_RIBBON.md"
        ))
        .expect("documentation/UI_PARITY_RIBBON.md");
        let doc = doc.split_whitespace().collect::<Vec<_>>().join(" ");
        fn walk(items: &'static [spec::Item], doc: &str, tab: &str) {
            for i in items {
                match i.sub {
                    Some(sub) => walk(sub, doc, tab),
                    None if i.action.is_none() => assert!(
                        doc.contains(i.label.0),
                        "{tab}: greyed line {} is not in the parity list",
                        i.label.0
                    ),
                    None => {}
                }
            }
        }
        for index in [7, 8, 9] {
            for g in spec::tabs()[index].groups {
                for b in g.buttons() {
                    if let Some(spec::Menu::Items(items)) = b.menu_of() {
                        walk(items, &doc, b.label.0);
                    }
                }
            }
        }
    }

    #[test]
    fn operator_palette_cells_insert_templates_and_text() {
        let Some(spec::Menu::Palette(sections)) = button(0, "operators").menu_of() else {
            panic!("operators is a palette");
        };
        let cell = |glyph: &str| {
            sections
                .iter()
                .flat_map(|s| s.cells.iter())
                .find(|c| c.glyph == glyph)
                .unwrap_or_else(|| panic!("no cell {glyph}"))
        };
        assert_eq!(
            cell("∫dx").action,
            Some(Action::Template(Template::Integral))
        );
        assert_eq!(cell("≥").action, Some(Action::TypeText(">=")));
        assert_eq!(cell("%").action, Some(Action::TypeText("%")));
        assert_eq!(cell("∇").action, Some(Action::Template(Template::Gradient)));
        assert_eq!(
            cell("∂/∂x").action,
            Some(Action::Template(Template::Partial))
        );
        assert_eq!(cell("×").action, Some(Action::Template(Template::Cross)));
        assert_eq!(cell("‖x‖").action, Some(Action::Template(Template::Norm)));
        assert_eq!(cell("x′").action, Some(Action::TypeText("'")));
        assert_eq!(cell("Mᵀ").action, Some(Action::TypeText("^T")));
        // Still greyed: the engine has no such operator.
        for glyph in ["[‖]", "M⟨⟩", "M⌒", "V⃗", "z̄", "∈", "+↵"] {
            assert_eq!(cell(glyph).action, None, "{glyph}");
        }
    }

    #[test]
    fn function_lists_are_filled_and_know_mean() {
        for id in [
            "bessel", "fit", "analysis", "doe", "ode", "files", "image", "prob", "signal", "solve",
            "stats", "vector",
        ] {
            let menu = button(2, id).menu_of().expect("a dropdown");
            assert!(!spec::function_names(menu).is_empty(), "{id}");
        }
        let names = spec::function_names(button(2, "stats").menu_of().unwrap());
        assert!(names.contains(&"mean"));
        assert!(crate::functions_table::FUNCTIONS
            .iter()
            .any(|f| f.name == "mean"));
    }

    #[test]
    fn narrow_windows_shrink_then_overflow_and_never_wrap() {
        let full = [100.0, 100.0, 100.0];
        let compact = [60.0, 60.0, 60.0];
        let p = render::plan(&full, &compact, 400.0);
        assert_eq!((p.density, p.visible), (render::Density::Full, 3));
        let p = render::plan(&full, &compact, 250.0);
        assert_eq!((p.density, p.visible), (render::Density::Compact, 3));
        let p = render::plan(&full, &compact, 130.0);
        assert_eq!((p.density, p.visible), (render::Density::Compact, 1));
    }

    #[test]
    fn unavailable_buttons_are_listed_in_the_parity_document() {
        let doc = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../documentation/UI_PARITY_RIBBON.md"
        ))
        .expect("documentation/UI_PARITY_RIBBON.md");
        // The document wraps lines inside a name; compare with single spaces.
        let doc = doc.split_whitespace().collect::<Vec<_>>().join(" ");
        for tab in spec::tabs() {
            for g in tab.groups {
                for b in g.buttons() {
                    if !b.implemented() {
                        assert!(
                            doc.contains(b.label.0),
                            "{} is not in the parity list",
                            b.label.0
                        );
                    }
                }
            }
        }
    }
}

//! The Insert dialogs (function, unit, operators and symbols, matrix size) and the Help windows
//! (about, keyboard shortcuts). The searching and grouping live in `catalog.rs` and `palette.rs`;
//! this file only draws them. A dialog returns what to insert, and the window puts it into the
//! math draft.
//!
//! # Manual test checklist (for the shareholder; nobody has seen these windows yet)
//!
//! Open `cargo run -p mathcraft-app -- documentation/examples/Mechanik_1_massen_ziehen.mcdx`, or
//! press Ctrl+N for a new worksheet, click an empty spot (a cross appears), then:
//! 1. Ctrl+E: the Insert function window opens with the search box focused. Type `sq`: sqrt is
//!    first. Arrow keys move the highlight; Enter (or double-click) types `sqrt(` into the draft.
//! 2. Type `zz:=` in the draft: the function window and the draft stay usable (Escape closes).
//! 3. Ctrl+U: the Insert unit window lists units in groups (length, mass, ...). Type `kg`, click
//!    it: `kg` is typed into the draft.
//! 4. Ctrl+G, then `p`: a pi is typed. Ctrl+G, then `W` (capital): an upper-case omega.
//! 5. Math tab > Operators and symbols: `≤` types `<=`; `×` is greyed out, hovering shows the
//!    reason. Summation types `sum(■, ■, ■, ■)`; Enter with the placeholders gives a message.
//! 6. Matrices/Tables > Matrix...: hover the grid (the title shows rows x columns), click 2 x 3:
//!    `[■, ■, ■; ■, ■, ■]` is typed.
//! 7. Help > Keyboard shortcuts: the table lists the keys. Help > About: version and MIT OR
//!    Apache-2.0.
//! 8. Calculation > Automatic calculation: untick it; the status bar says MANUAL. Type a change:
//!    the result does not update until F9 (or Calculation > Calculate).
//! 9. View > Zoom in / Zoom out / Zoom 100%; Ctrl+mouse wheel zooms. Grid and Region borders
//!    tick boxes show or hide the grid and the outlines of the regions.
//! 10. Greyed buttons (Insert > Image, Plots, Format > Bold) show "not implemented yet" on hover.

use eframe::egui::{self, Context, Key, RichText, Ui};

use crate::catalog::{self, MAX_MATRIX_SIDE};
use crate::edit::math::Template;
use crate::edit::shortcut::DOCUMENTED;
use crate::i18n::{language, term, term_of, tr, trf, Lang};
use crate::palette::{greek_letters, SymbolInsert, SYMBOLS};

/// Most rows the function list shows at once (the search narrows it further).
const MAX_ROWS: usize = 300;

/// The dialogs the ribbon and the shortcuts can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    /// The function list, all of it or one category.
    Function(Option<&'static str>),
    /// The unit list, grouped by dimension.
    Unit,
    /// Greek letters, constants, operators and templates.
    Symbols,
    /// The matrix size grid.
    Matrix,
    About,
    Shortcuts,
}

/// What a dialog puts into the math draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insertion {
    /// Characters, typed as they are.
    Text(String),
    /// A template of the math editor.
    Template(Template),
    /// A matrix of empty cells (rows x cols).
    Matrix { rows: usize, cols: usize },
}

/// The open dialog and its search state.
#[derive(Debug, Default)]
pub struct Dialogs {
    open: Option<Dialog>,
    query: String,
    selected: usize,
    focus_query: bool,
}

impl Dialogs {
    /// Opens `dialog` with an empty search.
    pub fn open(&mut self, dialog: Dialog) {
        self.open = Some(dialog);
        self.query.clear();
        self.selected = 0;
        self.focus_query = true;
    }

    /// True while a dialog is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Draws the open dialog. Returns what to insert when the user picked something; the dialog
    /// then closes. A closed window closes the dialog too.
    pub fn show(&mut self, ctx: &Context) -> Option<Insertion> {
        let dialog = self.open?;
        let focus = std::mem::take(&mut self.focus_query);
        let mut keep_open = true;
        let mut picked = None;
        let title = title(dialog);
        egui::Window::new(title)
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .show(ctx, |ui| {
                picked = match dialog {
                    Dialog::Function(category) => self.function_body(ui, category, focus),
                    Dialog::Unit => self.unit_body(ui, focus),
                    Dialog::Symbols => symbols_body(ui),
                    Dialog::Matrix => matrix_body(ui),
                    Dialog::About => {
                        about_body(ui);
                        None
                    }
                    Dialog::Shortcuts => {
                        shortcuts_body(ui);
                        None
                    }
                };
            });
        if !keep_open || picked.is_some() {
            self.open = None;
        }
        picked
    }

    fn function_body(
        &mut self,
        ui: &mut Ui,
        category: Option<&'static str>,
        focus: bool,
    ) -> Option<Insertion> {
        let mut picked = None;
        if let Some(name) = category {
            ui.label(trf("dlg.category_line", &[tr("dlg.category"), term(name)]));
        }
        ui.horizontal(|ui| {
            ui.label(tr("dlg.search"));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(tr("dlg.search_hint"))
                    .desired_width(280.0),
            );
            if focus {
                edit.request_focus();
            }
        });
        let entries = catalog::search_functions(&self.query, category);
        if entries.is_empty() {
            ui.label(tr("dlg.no_match"));
            return None;
        }
        self.selected = self.selected.min(entries.len() - 1);
        let (down, up, enter) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter),
            )
        });
        if down {
            self.selected = (self.selected + 1).min(entries.len() - 1);
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if enter {
            picked = Some(call_text(entries[self.selected].name));
        }
        let selected = self.selected;
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, entry) in entries.iter().enumerate().take(MAX_ROWS) {
                    let label = format!("{}   [{}]", entry.name, entry.category);
                    let response = ui.selectable_label(i == selected, label);
                    if response.clicked() {
                        self.selected = i;
                    }
                    if response.double_clicked() {
                        picked = Some(call_text(entry.name));
                    }
                }
            });
        let chosen = entries[self.selected];
        ui.separator();
        ui.label(RichText::new(chosen.description).italics());
        if language() != Lang::English {
            ui.label(RichText::new(tr("dlg.descriptions_english")).small());
        }
        if ui
            .button(format!("{} {}(", tr("btn.insert"), chosen.name))
            .clicked()
        {
            picked = Some(call_text(chosen.name));
        }
        picked
    }

    fn unit_body(&mut self, ui: &mut Ui, focus: bool) -> Option<Insertion> {
        let mut picked = None;
        ui.horizontal(|ui| {
            ui.label(tr("dlg.search"));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(tr("dlg.unit_search_hint"))
                    .desired_width(280.0),
            );
            if focus {
                edit.request_focus();
            }
        });
        let groups = catalog::unit_groups(&self.query);
        if groups.is_empty() {
            ui.label(tr("dlg.no_match"));
            return None;
        }
        let searching = !self.query.trim().is_empty();
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for group in &groups {
                    let count = group.units.len().to_string();
                    let heading = trf("dlg.unit_group", &[&term_of(&group.label), &count]);
                    egui::CollapsingHeader::new(heading)
                        .id_salt(group.label.as_str())
                        .default_open(searching || group.label == "length")
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for unit in &group.units {
                                    if ui.button(unit.symbol).on_hover_text(unit.name).clicked() {
                                        picked = Some(Insertion::Text(unit.symbol.to_string()));
                                    }
                                }
                            });
                        });
                }
            });
        picked
    }
}

fn call_text(name: &str) -> Insertion {
    Insertion::Text(format!("{name}("))
}

fn title(dialog: Dialog) -> String {
    match dialog {
        Dialog::Function(Some(category)) => {
            trf("dlg.function_title", &[tr("dlg.function"), term(category)])
        }
        Dialog::Function(None) => tr("dlg.function").to_string(),
        Dialog::Unit => tr("dlg.unit").to_string(),
        Dialog::Symbols => tr("dlg.symbols").to_string(),
        Dialog::Matrix => tr("dlg.matrix").to_string(),
        Dialog::About => tr("dlg.about").to_string(),
        Dialog::Shortcuts => tr("dlg.shortcuts").to_string(),
    }
}

/// Greek letters, then the constants, operators and templates. A greyed entry shows its reason.
fn symbols_body(ui: &mut Ui) -> Option<Insertion> {
    let mut picked = None;
    ui.label(tr("dlg.greek"));
    ui.horizontal_wrapped(|ui| {
        for (letter, _) in greek_letters() {
            if ui.button(letter.to_string()).clicked() {
                picked = Some(Insertion::Text(letter.to_string()));
            }
        }
    });
    ui.separator();
    ui.label(tr("dlg.constants"));
    ui.horizontal_wrapped(|ui| {
        for symbol in SYMBOLS {
            match symbol.insert {
                SymbolInsert::Text(text) => {
                    if ui.button(symbol.label).clicked() {
                        picked = Some(Insertion::Text(text.to_string()));
                    }
                }
                SymbolInsert::Template(template) => {
                    if ui.button(symbol.label).clicked() {
                        picked = Some(Insertion::Template(template));
                    }
                }
                SymbolInsert::Unavailable(reason) => {
                    ui.add_enabled(false, egui::Button::new(symbol.label))
                        .on_disabled_hover_text(tr(reason));
                }
            }
        }
    });
    picked
}

/// The matrix size grid: hover shows the size, a click types an empty matrix of that size.
fn matrix_body(ui: &mut Ui) -> Option<Insertion> {
    let mut picked = None;
    ui.label(tr("dlg.matrix_size"));
    egui::Grid::new("matrix_size_grid")
        .spacing([2.0, 2.0])
        .show(ui, |ui| {
            for rows in 1..=MAX_MATRIX_SIDE {
                for cols in 1..=MAX_MATRIX_SIDE {
                    let response = ui
                        .add(egui::Button::new("  ").min_size(egui::vec2(18.0, 18.0)))
                        .on_hover_text(trf(
                            "dlg.matrix_hover",
                            &[&rows.to_string(), &cols.to_string()],
                        ));
                    if response.clicked() {
                        picked = Some(Insertion::Matrix { rows, cols });
                    }
                }
                ui.end_row();
            }
        });
    picked
}

fn about_body(ui: &mut Ui) {
    ui.heading("Mathcraft");
    ui.label(tr("about.product"));
    ui.label(trf("about.version", &[env!("CARGO_PKG_VERSION")]));
    let locale = if language() == Lang::German {
        "de-DE"
    } else {
        "en-US"
    };
    ui.label(trf("about.locale", &[locale]));
    ui.label(tr("dlg.about_licence"));
    ui.label(tr("dlg.about_text"));
    ui.label(RichText::new(tr("dlg.about_affiliation")).small());
}

fn shortcuts_body(ui: &mut Ui) {
    egui::Grid::new("shortcut_grid")
        .striped(true)
        .spacing([18.0, 4.0])
        .show(ui, |ui| {
            for (keys, what) in DOCUMENTED.iter() {
                ui.label(RichText::new(*keys).strong());
                ui.label(tr(what));
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_dialogs_set_is_closed() {
        let dialogs = Dialogs::default();
        assert!(!dialogs.is_open());
    }

    #[test]
    fn opening_clears_the_search_and_focuses_it() {
        let mut dialogs = Dialogs {
            query: "old".to_string(),
            selected: 4,
            ..Dialogs::default()
        };
        dialogs.open(Dialog::Unit);
        assert!(dialogs.is_open());
        assert!(dialogs.query.is_empty());
        assert_eq!(dialogs.selected, 0);
        assert!(dialogs.focus_query);
    }

    #[test]
    fn a_function_pick_types_the_name_and_a_bracket() {
        assert_eq!(call_text("sqrt"), Insertion::Text("sqrt(".to_string()));
    }

    #[test]
    fn titles_name_the_category() {
        assert_eq!(
            title(Dialog::Function(Some("Statistics"))),
            "Insert function - Statistics"
        );
        assert_eq!(title(Dialog::Unit), "Insert unit");
    }
}

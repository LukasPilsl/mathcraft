//! The status bar, laid out like Prime's (screenshots 0003, 0158 to 0161): page counter, the green
//! AUTO indicator, Find and Replace fields with their buttons and the Options menu, the zoom
//! percentage with minus, slider and plus, and the two view buttons.

use eframe::egui::{self, Color32, Key, Modifiers, RichText, Sense, Ui};

use crate::chrome::{paint_symbol, symbol_button, Symbol};
use crate::i18n::{tr, trf};
use crate::live::AiState;
use crate::menus::SearchOptions;
use crate::theme;
use crate::view_state::{CalcMode, ZOOM_MAX, ZOOM_MIN};
use mathcraft_doc::search::Match as SearchMatch;
use mathcraft_doc::SearchOptions as DocSearchOptions;

/// The id of the Suchen field, so that Ctrl+F can focus it.
fn find_field() -> egui::Id {
    egui::Id::new("status_find_field")
}

/// The text the search fields hold, the search options, and the matches of the query.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub find: String,
    pub replace: String,
    pub options: SearchOptions,
    /// The matches of `find` in the worksheet, in document order (see [`SearchState::refresh_with`]).
    pub matches: Vec<SearchMatch>,
    /// The match the user is on: an index into `matches`.
    pub current: Option<usize>,
    /// The query, the options and the worksheet revision that `matches` were made for.
    made_for: Option<(String, DocSearchOptions, u64)>,
    /// A region to bring into view on the next frame. The canvas takes it.
    pub scroll_to: Option<u32>,
}

impl SearchState {
    /// The core search options for the check items of the Options menu.
    pub fn doc_options(&self) -> DocSearchOptions {
        DocSearchOptions {
            match_case: self.options.match_case,
            whole_word: self.options.whole_words,
            in_math: self.options.in_equations,
            in_text: self.options.in_text,
        }
    }

    /// Recomputes the matches with `find` when the query, the options or the worksheet revision
    /// changed. The current match stays the same index when the query is the same (clamped), and
    /// is cleared when the query changes. An empty query has no matches.
    pub fn refresh_with(
        &mut self,
        revision: u64,
        find: impl Fn(&str, DocSearchOptions) -> Vec<SearchMatch>,
    ) {
        let opts = self.doc_options();
        let key = (self.find.clone(), opts, revision);
        if self.made_for.as_ref() == Some(&key) {
            return;
        }
        let query_changed = self
            .made_for
            .as_ref()
            .is_none_or(|(query, _, _)| *query != self.find);
        self.matches = if self.find.is_empty() {
            Vec::new()
        } else {
            find(&self.find, opts)
        };
        self.current = if query_changed {
            None
        } else {
            self.current.filter(|i| *i < self.matches.len())
        };
        self.made_for = Some(key);
    }
}

/// A search action the user asked for in the status bar. The app carries it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchAction {
    /// The next match (the right-hand Suchen arrow, Enter).
    Next,
    /// The previous match (the left-hand arrow, Shift+Enter).
    Previous,
    /// Replace the current match (the Ersetzen durch arrow).
    ReplaceOne,
    /// Replace every match.
    ReplaceAll,
}

/// What the user did in the status bar this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatusEvents {
    /// The AUTO / MANUAL indicator was clicked.
    pub toggle_calc: bool,
    pub zoom_out: bool,
    pub zoom_in: bool,
    /// `Some(true)` for the page view button, `Some(false)` for the list view button.
    pub page_view: Option<bool>,
    /// The AI indicator was clicked: pause or resume.
    pub toggle_ai_pause: bool,
    /// A search or replace action.
    pub search: Option<SearchAction>,
}

/// Draws the status bar contents. `zoom_pct` is changed by the slider. `message` is the message
/// line (a problem with the last action): it takes the room left of the zoom group, cut short, with
/// the whole text as its tooltip.
#[allow(clippy::too_many_arguments)]
pub fn status_bar(
    ui: &mut Ui,
    page: (usize, usize),
    calc: CalcMode,
    zoom_pct: &mut f32,
    page_layout: bool,
    search: &mut SearchState,
    message: Option<&str>,
    ai: Option<AiState>,
) -> StatusEvents {
    let mut ev = StatusEvents::default();
    // The message is shown whole when it can be: the other items shrink first (see `Tier`).
    let message_width = message.map_or(0.0, |m| {
        ui.painter()
            .layout_no_wrap(
                m.to_owned(),
                egui::FontId::proportional(13.0),
                theme::PROBLEM,
            )
            .size()
            .x
            + 14.0
    });
    let tier = Tier::pick(ui.available_width() - message_width, ai.is_some());
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(format!("{} / {}", page.0, page.1));
        ui.add_space(10.0);
        // The AUTO indicator: a green dot and the word; a click switches the mode (0194).
        let (rect, response) = ui.allocate_exact_size(egui::vec2(66.0, 18.0), Sense::click());
        let colour = match calc {
            CalcMode::Auto => theme::STATUS_OK,
            CalcMode::Manual => Color32::from_rgb(200, 150, 30),
        };
        ui.painter()
            .circle_filled(rect.left_center() + egui::vec2(7.0, 0.0), 5.0, colour);
        ui.painter().text(
            rect.left_center() + egui::vec2(20.0, 0.0),
            egui::Align2::LEFT_CENTER,
            calc.label(),
            egui::FontId::proportional(13.0),
            ui.visuals().text_color(),
        );
        if response.on_hover_text(tr("status.calc_tip")).clicked() {
            ev.toggle_calc = true;
        }
        if let Some(state) = ai {
            let (colour, key) = match state {
                AiState::Ready => (Color32::from_rgb(150, 155, 165), "status.ai_ready"),
                AiState::Connected => (theme::STATUS_OK, "status.ai_connected"),
                AiState::Paused => (Color32::from_rgb(200, 150, 30), "status.ai_paused"),
            };
            let label = tr(key);
            let width = 34.0 + label.chars().count() as f32 * 7.0;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 18.0), Sense::click());
            ui.painter()
                .circle_filled(rect.left_center() + egui::vec2(7.0, 0.0), 5.0, colour);
            ui.painter().text(
                rect.left_center() + egui::vec2(20.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                ui.visuals().text_color(),
            );
            if response.on_hover_text(tr("status.ai_pause_tip")).clicked() {
                ev.toggle_ai_pause = true;
            }
        }
        ui.separator();
        if tier.labels {
            ui.label(tr("status.find"));
        }
        search_keys(ui, search, &mut ev);
        ui.add(
            egui::TextEdit::singleline(&mut search.find)
                .id(find_field())
                .hint_text(if tier.labels { "" } else { tr("status.find") })
                .desired_width(tier.field),
        );
        let has_matches = !search.matches.is_empty();
        if !search.find.is_empty() && tier.count {
            let text = if has_matches {
                let current = search.current.map_or(0, |i| i + 1);
                trf(
                    "status.match_count",
                    &[&current.to_string(), &search.matches.len().to_string()],
                )
            } else {
                tr("status.no_matches").to_string()
            };
            ui.label(RichText::new(text).color(Color32::from_rgb(60, 70, 85)));
        }
        if symbol_button(
            ui,
            Symbol::ArrowLeft,
            has_matches,
            tr("status.previous_match"),
        )
        .clicked()
        {
            ev.search = Some(SearchAction::Previous);
        }
        if symbol_button(ui, Symbol::ArrowRight, has_matches, tr("status.next_match")).clicked() {
            ev.search = Some(SearchAction::Next);
        }
        if tier.replace {
            if tier.labels {
                ui.label(tr("status.replace_with"));
            }
            ui.add(
                egui::TextEdit::singleline(&mut search.replace)
                    .hint_text(if tier.labels {
                        ""
                    } else {
                        tr("status.replace_with")
                    })
                    .desired_width(tier.field),
            );
            if symbol_button(
                ui,
                Symbol::ArrowRight,
                has_matches,
                tr("status.replace_one_tip"),
            )
            .clicked()
            {
                ev.search = Some(SearchAction::ReplaceOne);
            }
            let all = ui
                .add_enabled(
                    has_matches,
                    egui::Button::new(tr("status.replace_all")).small(),
                )
                .on_hover_text(tr("status.replace_all_tip"));
            if all.clicked() {
                ev.search = Some(SearchAction::ReplaceAll);
            }
        }
        search_options_menu(ui, &mut search.options);
        if let Some(text) = message {
            // The room left for the message: the zoom group and the view buttons come after it.
            let width = (ui.available_width() - tier.zoom_group()).max(60.0);
            ui.add_sized(
                [width, 18.0],
                egui::Label::new(RichText::new(text).color(theme::PROBLEM)).truncate(),
            )
            .on_hover_text(text);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // Right to left: the list view button, the page view button, then the zoom group.
            if view_button(ui, false, !page_layout).clicked() {
                ev.page_view = Some(false);
            }
            if view_button(ui, true, page_layout).clicked() {
                ev.page_view = Some(true);
            }
            ui.add_space(8.0);
            if symbol_button(ui, Symbol::Plus, true, "").clicked() {
                ev.zoom_in = true;
            }
            if tier.slider {
                ui.spacing_mut().slider_width = 110.0;
                ui.add(
                    egui::Slider::new(zoom_pct, ZOOM_MIN..=ZOOM_MAX)
                        .show_value(false)
                        .trailing_fill(false),
                );
            }
            if symbol_button(ui, Symbol::Minus, true, "").clicked() {
                ev.zoom_out = true;
            }
            ui.label(format!("{:.0}%", zoom_pct));
        });
    });
    ev
}

/// How much of the status bar is drawn at a given width. The widest tier that fits wins; at the
/// narrowest the search labels become hints and the zoom slider goes (its minus, plus and the
/// percentage stay), so nothing overlaps at 640 px (QA finding F16).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tier {
    /// Width of the Suchen and Ersetzen fields.
    pub field: f32,
    /// The words "Find:" and "Replace with:" next to the fields (else hints inside them).
    pub labels: bool,
    /// The zoom slider.
    pub slider: bool,
    /// The match count next to the Suchen field.
    pub count: bool,
    /// The Ersetzen field and its buttons.
    pub replace: bool,
}

impl Tier {
    const TIERS: [Tier; 5] = [
        Tier {
            field: 130.0,
            labels: true,
            slider: true,
            count: true,
            replace: true,
        },
        Tier {
            field: 90.0,
            labels: true,
            slider: true,
            count: false,
            replace: true,
        },
        Tier {
            field: 80.0,
            labels: false,
            slider: true,
            count: false,
            replace: true,
        },
        Tier {
            field: 60.0,
            labels: false,
            slider: false,
            count: false,
            replace: true,
        },
        Tier {
            field: 70.0,
            labels: false,
            slider: false,
            count: false,
            replace: false,
        },
    ];

    /// The room the zoom group and the view buttons take.
    pub(crate) fn zoom_group(self) -> f32 {
        190.0 + if self.slider { 120.0 } else { 0.0 }
    }

    /// The width the items left of the message need.
    fn needs(self, ai: bool) -> f32 {
        // Page number, AUTO, separator; the search arrows, All and Options; the zoom group.
        let fixed = 50.0 + 80.0 + 20.0 + 175.0 + self.zoom_group();
        let labels = if self.labels { 125.0 } else { 0.0 };
        let replace = if self.replace {
            self.field + 70.0 + 22.0
        } else {
            0.0
        };
        let count = if self.count { 70.0 } else { 0.0 };
        fixed + labels + count + self.field + replace + if ai { 90.0 } else { 0.0 }
    }

    /// The widest tier whose items fit in `room`; the narrowest when none does.
    pub(crate) fn pick(room: f32, ai: bool) -> Tier {
        Self::TIERS
            .into_iter()
            .find(|t| t.needs(ai) <= room)
            .unwrap_or(Self::TIERS[4])
    }
}

/// The keys of the Suchen field, taken before the field is drawn (a text field would take Escape
/// for itself). Ctrl+F focuses the field. Escape clears it. Enter goes to the next match and
/// Shift+Enter to the previous one. Shift is tested first, since Enter with no modifier matches
/// any modifiers.
fn search_keys(ui: &mut Ui, search: &mut SearchState, ev: &mut StatusEvents) {
    let focused = ui.memory(|m| m.has_focus(find_field()));
    if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
        ui.memory_mut(|m| m.request_focus(find_field()));
    }
    if !focused {
        return;
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        search.find.clear();
        search.current = None;
    } else if ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter)) {
        ev.search = Some(SearchAction::Previous);
    } else if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
        ev.search = Some(SearchAction::Next);
    }
}

/// The Options drop-down: the four search options as check items (0159).
fn search_options_menu(ui: &mut Ui, options: &mut SearchOptions) {
    let galley = ui.painter().layout_no_wrap(
        tr("status.options").to_string(),
        egui::FontId::proportional(13.0),
        ui.visuals().text_color(),
    );
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(galley.size().x + 30.0, 20.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_stroke(
            rect,
            3.0,
            ui.visuals().widgets.hovered.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().galley(
        rect.left_center() + egui::vec2(6.0, -galley.size().y / 2.0),
        galley,
        Color32::BLACK,
    );
    paint_symbol(
        ui.painter(),
        egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(10.0, 0.0),
            egui::vec2(10.0, 10.0),
        ),
        Symbol::CaretDown,
        Color32::from_rgb(60, 70, 85),
    );
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.checkbox(&mut options.in_equations, tr("status.search_equations"));
            ui.checkbox(&mut options.in_text, tr("status.search_text"));
            ui.checkbox(&mut options.match_case, tr("status.match_case"));
            ui.checkbox(&mut options.whole_words, tr("status.whole_words"));
        });
}

/// One view button: a page outline (page view) or a list of lines (list view).
fn view_button(ui: &mut Ui, page: bool, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 22.0), Sense::click());
    let visuals = ui.visuals();
    if selected {
        ui.painter()
            .rect_filled(rect, 3.0, visuals.selection.bg_fill.gamma_multiply(0.6));
    }
    if response.hovered() {
        ui.painter().rect_stroke(
            rect,
            3.0,
            visuals.widgets.hovered.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    let stroke = egui::Stroke::new(1.2, visuals.text_color());
    let inner = rect.shrink2(egui::vec2(7.0, 4.0));
    if page {
        ui.painter()
            .rect_stroke(inner, 1.0, stroke, egui::StrokeKind::Middle);
        for k in 1..4 {
            let y = inner.top() + inner.height() * k as f32 / 4.0;
            ui.painter().line_segment(
                [
                    egui::pos2(inner.left() + 2.0, y),
                    egui::pos2(inner.right() - 2.0, y),
                ],
                egui::Stroke::new(0.8, stroke.color),
            );
        }
    } else {
        for k in 0..4 {
            let y = inner.top() + inner.height() * (k as f32 + 0.5) / 4.0;
            ui.painter().line_segment(
                [egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)],
                stroke,
            );
        }
    }
    response.on_hover_text(tr(if page {
        "status.page_view"
    } else {
        "status.list_view"
    }))
}

#[cfg(test)]
mod tier_tests {
    use super::Tier;

    #[test]
    fn the_status_bar_sheds_items_as_the_room_shrinks() {
        let wide = Tier::pick(1600.0, false);
        assert!(wide.labels && wide.slider && wide.replace);
        let narrow = Tier::pick(640.0, false);
        assert!(!narrow.slider, "no zoom slider at 640 px");
        let tiny = Tier::pick(100.0, true);
        assert!(!tiny.replace && !tiny.slider);
        let mut last = f32::MAX;
        for t in Tier::TIERS {
            assert!(t.needs(false) <= last);
            last = t.needs(false);
        }
        assert!(Tier::pick(640.0, false).needs(false) <= 640.0);
    }
}

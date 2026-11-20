//! The Link window of the Textformatierung tab (screenshot 0222): the text of the link and its
//! address. The window only collects the two fields; the session makes the hyperlink run. The
//! link is display only: Mathcraft never opens it.

use eframe::egui::{self, Context};

use crate::i18n::{language, Lang};

/// The fields of the Link window. The text starts as the selected text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkDraft {
    pub text: String,
    pub address: String,
}

/// Draws the window. Returns `Some(true)` for OK, `Some(false)` for Cancel (or closing it), and
/// `None` while it stays open. OK is greyed while the text is empty.
pub fn show(ctx: &Context, draft: &mut LinkDraft) -> Option<bool> {
    let de = language() == Lang::German;
    let (text_label, address_label) = if de {
        ("Text:", "Adresse:")
    } else {
        ("Text:", "Address:")
    };
    let mut result = None;
    let mut open = true;
    egui::Window::new("Link")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(text_label);
            ui.add(egui::TextEdit::singleline(&mut draft.text).desired_width(320.0));
            ui.add_space(6.0);
            ui.label(address_label);
            ui.add(egui::TextEdit::singleline(&mut draft.address).desired_width(320.0));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let cancel = if de { "Abbrechen" } else { "Cancel" };
                let can_ok = !draft.text.trim().is_empty();
                if ui.add_enabled(can_ok, egui::Button::new("OK")).clicked() {
                    result = Some(true);
                }
                if ui.button(cancel).clicked() {
                    result = Some(false);
                }
            });
        });
    if !open && result.is_none() {
        return Some(false);
    }
    result
}

//! The application Options window of the orb menu (screenshots 0012 to 0015): a tab with the
//! program options and a licence tab. Options Mathcraft does not support are shown greyed out.

use eframe::egui::{self, Align2, Context};

use crate::i18n::tr;

/// The tabs of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptionsTab {
    #[default]
    Options,
    Licence,
}

/// One option row: the i18n key, and whether Mathcraft supports it.
pub const OPTION_ROWS: [(&str, bool); 11] = [
    ("opt.show_prime", true),
    ("opt.resources_tab", false),
    ("opt.windchill", false),
    ("opt.help_path", false),
    ("opt.diagnostics", false),
    ("opt.my_templates", false),
    ("opt.shared_templates", false),
    ("opt.new_template", false),
    ("opt.hide_control_warning", false),
    ("opt.script_on_open", false),
    ("opt.python_path", false),
];

/// Draws the window. `show_prime` is the one supported option. Returns false once it is closed.
pub fn options_window(
    ctx: &Context,
    tab: &mut OptionsTab,
    show_prime: &mut bool,
    ai_endpoint: &mut bool,
) -> bool {
    let mut open = true;
    let mut close = false;
    egui::Window::new(tr("opt.title"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .default_width(560.0)
        .show(ctx, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_size(egui::vec2(540.0, 300.0));
                match *tab {
                    OptionsTab::Options => {
                        ui.checkbox(ai_endpoint, tr("opt.ai_endpoint"))
                            .on_hover_text(tr("opt.ai_endpoint_tip"));
                        ui.add_space(2.0);
                        for (key, supported) in OPTION_ROWS {
                            if supported {
                                ui.checkbox(show_prime, tr(key));
                            } else {
                                let mut off = false;
                                ui.add_enabled(false, egui::Checkbox::new(&mut off, tr(key)));
                            }
                            ui.add_space(2.0);
                        }
                    }
                    OptionsTab::Licence => {
                        ui.label(tr("dlg.about_licence"));
                        ui.add_space(4.0);
                        ui.label(tr("opt.licence_text"));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.selectable_value(tab, OptionsTab::Licence, tr("opt.tab_licence"));
                ui.selectable_value(tab, OptionsTab::Options, tr("opt.tab_options"));
            });
            ui.vertical_centered(|ui| {
                if ui.button(tr("btn.close")).clicked() {
                    close = true;
                }
            });
        });
    open && !close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_supported_options_are_enabled() {
        assert_eq!(OPTION_ROWS.iter().filter(|(_, s)| *s).count(), 1);
        assert!(OPTION_ROWS.len() >= 10);
    }
}

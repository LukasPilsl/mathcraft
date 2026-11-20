//! The start-up notice: a small window on every start that says Mathcraft is still in
//! development and points to the issue tracker. A "Don't show again" box is recorded in the
//! settings file, but only when the window is closed (OK or the window's close button).

use eframe::egui::{self, Align2, Context};

use crate::config::Config;
use crate::i18n::tr;

/// Where errors and missing features are reported.
pub const ISSUES_URL: &str = "https://github.com/lukaspilsl/mathcraft/issues";

/// The window's state. Shown on start unless the settings say `startup_notice=off`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StartupNotice {
    open: bool,
    dont_show_again: bool,
}

impl StartupNotice {
    /// The notice for a start with `config`: open unless the user chose "Don't show again".
    #[must_use]
    pub fn for_config(config: &Config) -> Self {
        Self {
            open: config.startup_notice != Some(false),
            dont_show_again: false,
        }
    }

    /// Opens the window again (the screenshot harness uses this).
    pub fn open(&mut self) {
        self.open = true;
        self.dont_show_again = false;
    }

    /// True while the window is shown.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Writes the "Don't show again" choice into `config`. Without the box ticked nothing changes,
    /// so the notice shows again on the next start.
    pub fn record(&self, config: &mut Config) {
        if self.dont_show_again {
            config.startup_notice = Some(false);
        }
    }

    /// Draws the window while it is open. Returns true in the frame it closes.
    pub fn show(&mut self, ctx: &Context) -> bool {
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut ok = false;
        egui::Window::new(tr("startup.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(tr("startup.text"));
                ui.add_space(6.0);
                ui.hyperlink_to(tr("startup.link"), ISSUES_URL);
                ui.add_space(6.0);
                ui.checkbox(&mut self.dont_show_again, tr("startup.dont_show"));
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    if ui.button(tr("btn.ok")).clicked() {
                        ok = true;
                    }
                });
            });
        if ok || !open {
            self.open = false;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_is_shown_by_default() {
        assert!(StartupNotice::for_config(&Config::default()).is_open());
    }

    #[test]
    fn the_notice_stays_hidden_once_switched_off() {
        let config = Config {
            startup_notice: Some(false),
            ..Config::default()
        };
        assert!(!StartupNotice::for_config(&config).is_open());
    }

    #[test]
    fn dont_show_again_is_recorded_on_close_and_then_hides_the_notice() {
        let mut config = Config::default();
        let mut notice = StartupNotice::for_config(&config);
        notice.dont_show_again = true;
        notice.open = false; // the window closed this frame
        notice.record(&mut config);
        assert_eq!(config.startup_notice, Some(false));
        assert!(!StartupNotice::for_config(&config).is_open());
    }

    #[test]
    fn closing_without_the_box_keeps_showing_the_notice() {
        let mut config = Config::default();
        let mut notice = StartupNotice::for_config(&config);
        notice.open = false;
        notice.record(&mut config);
        assert_eq!(config.startup_notice, None);
        assert!(StartupNotice::for_config(&config).is_open());
    }

    #[test]
    fn reopening_clears_the_box() {
        let mut notice = StartupNotice {
            open: false,
            dont_show_again: true,
        };
        notice.open();
        assert!(notice.is_open());
        assert!(!notice.dont_show_again);
    }

    #[test]
    fn the_link_points_at_the_issue_tracker() {
        assert_eq!(ISSUES_URL, "https://github.com/lukaspilsl/mathcraft/issues");
    }
}

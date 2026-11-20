//! The plot window: the expressions of the x and y axes (one trace per expression), the axis
//! limits, the grid and the title of one 2D plot. It edits a [`PlotSpec`]; the session turns that
//! into the plot (see `edit/plot.rs`). Opened by a double click on a plot, or by Plots > Edit plot.

use eframe::egui::{self, Context, TextEdit};

use crate::edit::plot::PlotSpec;
use crate::i18n::tr;

/// The open plot window.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotDialog {
    /// The plot region being edited.
    pub id: u32,
    /// The text being edited.
    pub spec: PlotSpec,
    /// Why the last Apply did not change the plot.
    pub error: Option<String>,
    focus: bool,
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlotChoice {
    Apply,
    Cancel,
}

impl PlotDialog {
    /// A window for plot `id` showing `spec`.
    pub fn new(id: u32, spec: PlotSpec) -> Self {
        Self {
            id,
            spec,
            error: None,
            focus: true,
        }
    }

    /// Draws the window. `None` while it stays open.
    pub fn show(&mut self, ctx: &Context) -> Option<PlotChoice> {
        let mut choice = None;
        let mut open = true;
        let polar = self.spec.polar;
        let focus = std::mem::take(&mut self.focus);
        egui::Window::new(tr("plot.window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(tr("plot.hint"));
                ui.add_space(6.0);
                let x_label = if polar { "plot.angle" } else { "plot.x" };
                let y_label = if polar { "plot.radius" } else { "plot.y" };
                ui.label(tr(x_label));
                let x = ui.add(TextEdit::singleline(&mut self.spec.x).desired_width(f32::INFINITY));
                if focus {
                    x.request_focus();
                }
                ui.label(tr(y_label));
                ui.add(TextEdit::singleline(&mut self.spec.y).desired_width(f32::INFINITY));
                ui.add_space(6.0);
                egui::Grid::new("plot_limits").show(ui, |ui| {
                    ui.label(tr("plot.x_limits"));
                    ui.add(TextEdit::singleline(&mut self.spec.x_min).desired_width(80.0));
                    ui.add(TextEdit::singleline(&mut self.spec.x_max).desired_width(80.0));
                    ui.end_row();
                    ui.label(tr("plot.y_limits"));
                    ui.add(TextEdit::singleline(&mut self.spec.y_min).desired_width(80.0));
                    ui.add(TextEdit::singleline(&mut self.spec.y_max).desired_width(80.0));
                    ui.end_row();
                    ui.label(tr("plot.title"));
                    ui.add(TextEdit::singleline(&mut self.spec.title).desired_width(170.0));
                    ui.end_row();
                });
                ui.checkbox(&mut self.spec.grid, tr("plot.grid"));
                if let Some(error) = &self.error {
                    ui.colored_label(crate::theme::PROBLEM, error);
                }
                ui.horizontal(|ui| {
                    if ui.button(tr("plot.apply")).clicked() {
                        choice = Some(PlotChoice::Apply);
                    }
                    if ui.button(tr("plot.cancel")).clicked() {
                        choice = Some(PlotChoice::Cancel);
                    }
                });
            });
        if !open {
            choice = Some(PlotChoice::Cancel);
        }
        choice
    }
}

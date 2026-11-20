//! The Format > Results format window and the Calculation > Worksheet options window.
//!
//! The windows only draw and edit drafts. The window applies a draft to the document (through
//! the edit session) when the user chooses Apply; the writer then stores what changed. Results
//! format settings that the file cannot hold yet are refused here with the reason, never dropped.

use eframe::egui::{self, Context, RichText, Ui};
use mathcraft_core::eval::{
    format_complex_with, format_real_with, AngleUnit, ComplexForm, MatrixDisplay, NumberFormat,
    Radix, ResultFormat,
};
use mathcraft_core::units::UnitSystem;

use crate::i18n::{number_separators, tr};
use mathcraft_import::regions::Settings;

/// What the user did in a window this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Nothing chosen; the window stays open.
    Open,
    /// Apply the draft to the whole worksheet.
    ApplyWorksheet,
    /// Apply the draft to the selected region.
    ApplyRegion,
    /// Close without applying.
    Cancel,
}

/// The open Results format window: the draft, the region it applies to when one was selected
/// (`None` applies to the worksheet only), and the format the target shows before the change.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultDialog {
    /// The selected top-level region, or the one being typed in, when there is one.
    pub region: Option<u32>,
    /// The target region is a math region: its matrix display can be set.
    pub math: bool,
    /// The format being edited.
    pub draft: ResultFormat,
    /// The worksheet's format, which a region without its own format shows.
    pub base: ResultFormat,
    /// The matrix display being edited (region only).
    pub matrix: MatrixDisplay,
    /// The matrix display the region has now.
    pub base_matrix: MatrixDisplay,
}

/// The sample values the preview shows: the help's examples and the screenshot's 50000.
const PREVIEW: [f64; 6] = [12461.7556, 0.000125, 50000.0, -2.5e-9, 1.5, 36.8698976458];

/// Why a result format cannot be stored, when it cannot. `old` is the format the file has now.
/// The writer stores General formats only, and the complex threshold has no attribute.
pub fn unstorable(new: &ResultFormat, old: &ResultFormat) -> Option<&'static str> {
    if new.number != NumberFormat::General {
        return Some(tr("res.unstorable_number"));
    }
    if new.complex_threshold != old.complex_threshold {
        return Some(tr("res.unstorable_complex"));
    }
    if new.digit_grouping != old.digit_grouping {
        return Some(tr("res.unstorable_grouping"));
    }
    if new.complex_form != old.complex_form {
        return Some(tr("res.unstorable_polar"));
    }
    None
}

/// The Results format window: the number format and its settings, with a live preview of the
/// draft. The matrix display is edited only when `has_region` (it is stored per region), and
/// `has_region` also enables the button that applies the draft to the selected region.
pub fn result_format_window(
    ctx: &Context,
    draft: &mut ResultFormat,
    matrix: &mut MatrixDisplay,
    has_region: bool,
    has_math: bool,
) -> Choice {
    let mut choice = Choice::Open;
    egui::Window::new(tr("dlg.results_format"))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            result_format_controls(ui, draft);
            ui.separator();
            ui.label(RichText::new(tr("res.matrix_heading")).strong());
            ui.add_enabled_ui(has_math, |ui| matrix_controls(ui, matrix));
            if !has_math {
                ui.label(tr("res.matrix_needs_region"));
            }
            ui.separator();
            ui.label(RichText::new(tr("res.preview")).strong());
            let seps = number_separators();
            for x in PREVIEW {
                ui.label(format!(
                    "{} = {}",
                    input_text(x),
                    format_real_with(x, draft, &seps)
                ));
            }
            ui.label(format!(
                "1 + {}i = {}",
                input_text(0.002),
                format_complex_with(1.0, 0.002, draft, &seps)
            ));
            ui.label(format!(
                "√-3 = {}",
                format_complex_with(0.0, 3f64.sqrt(), draft, &seps)
            ));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(tr("btn.apply")).clicked() {
                    choice = Choice::ApplyWorksheet;
                }
                if ui
                    .add_enabled(has_region, egui::Button::new(tr("btn.apply_region")))
                    .clicked()
                {
                    choice = Choice::ApplyRegion;
                }
                if ui.button(tr("btn.clear_format")).clicked() {
                    *draft = ResultFormat::default();
                }
                if ui.button(tr("btn.cancel")).clicked() {
                    choice = Choice::Cancel;
                }
            });
        });
    choice
}

/// The controls of a result format, in the help's order.
fn result_format_controls(ui: &mut Ui, draft: &mut ResultFormat) {
    egui::Grid::new("result_format_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr("res.result_format"));
            egui::ComboBox::from_id_salt("result_format_number")
                .selected_text(number_label(draft.number))
                .show_ui(ui, |ui| {
                    for number in [
                        NumberFormat::General,
                        NumberFormat::Decimal,
                        NumberFormat::Scientific,
                        NumberFormat::Engineering,
                        NumberFormat::Percent,
                    ] {
                        ui.selectable_value(&mut draft.number, number, number_label(number));
                    }
                });
            ui.end_row();
            ui.label(tr("res.precision"));
            ui.add(egui::DragValue::new(&mut draft.precision).range(0..=16));
            ui.end_row();
            ui.label(tr("res.trailing_zeros"));
            ui.checkbox(&mut draft.trailing_zeros, "");
            ui.end_row();
            ui.label(tr("res.radix"));
            egui::ComboBox::from_id_salt("result_format_radix")
                .selected_text(radix_label(draft.radix))
                .show_ui(ui, |ui| {
                    for radix in [
                        Radix::Decimal,
                        Radix::Binary,
                        Radix::Octal,
                        Radix::Hexadecimal,
                    ] {
                        ui.selectable_value(&mut draft.radix, radix, radix_label(radix));
                    }
                });
            ui.end_row();
            ui.label(tr("res.imaginary"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut draft.imaginary_j, false, "i");
                ui.radio_value(&mut draft.imaginary_j, true, "j");
            });
            ui.end_row();
            ui.label(tr("res.digit_grouping"));
            ui.checkbox(&mut draft.digit_grouping, "");
            ui.end_row();
            ui.label(tr("res.complex_form"));
            egui::ComboBox::from_id_salt("result_format_complex_form")
                .selected_text(complex_form_label(draft.complex_form))
                .show_ui(ui, |ui| {
                    for form in [
                        ComplexForm::Cartesian,
                        ComplexForm::Polar(AngleUnit::Radians),
                        ComplexForm::Polar(AngleUnit::Degrees),
                    ] {
                        ui.selectable_value(
                            &mut draft.complex_form,
                            form,
                            complex_form_label(form),
                        );
                    }
                });
            ui.end_row();
            ui.label(tr("res.exp_threshold"));
            ui.add(egui::DragValue::new(&mut draft.exponential_threshold).range(0..=15));
            ui.end_row();
            ui.label(tr("res.zero_threshold"));
            ui.add(egui::DragValue::new(&mut draft.zero_threshold).range(0..=307));
            ui.end_row();
            ui.label(tr("res.complex_threshold"));
            ui.add(egui::DragValue::new(&mut draft.complex_threshold).range(0..=307));
            ui.end_row();
        });
}

/// The text of an example input in the current language (`0,5` in German).
fn input_text(x: f64) -> String {
    let plain = ResultFormat {
        number: NumberFormat::Decimal,
        precision: 16,
        ..ResultFormat::default()
    };
    format_real_with(x, &plain, &number_separators())
}

fn number_label(number: NumberFormat) -> &'static str {
    match number {
        NumberFormat::General => tr("num.general"),
        NumberFormat::Decimal => tr("num.decimal"),
        NumberFormat::Scientific => tr("num.scientific"),
        NumberFormat::Engineering => tr("num.engineering"),
        NumberFormat::Percent => tr("num.percent"),
    }
}

/// The name of a unit system as the Unit System dialog shows it (the names are not translated).
fn unit_system_label(system: UnitSystem) -> &'static str {
    match system {
        UnitSystem::Si => "SI",
        UnitSystem::Us => "USCS",
        UnitSystem::Cgs => "CGS",
    }
}

/// The most rows or columns a matrix display shows, as the dialog allows.
const MAX_MATRIX_SHOWN: u32 = 1000;

/// The matrix display controls: the visible size, the index toggle, and whether nested matrices
/// show their elements. The file stores the nested matrices as expanded or collapsed; the dialog
/// shows the help's wording, Collapse Nested Matrices, which is the opposite.
fn matrix_controls(ui: &mut Ui, matrix: &mut MatrixDisplay) {
    egui::Grid::new("matrix_display_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(tr("res.matrix_rows"));
            ui.add(egui::DragValue::new(&mut matrix.rows).range(1..=MAX_MATRIX_SHOWN));
            ui.end_row();
            ui.label(tr("res.matrix_cols"));
            ui.add(egui::DragValue::new(&mut matrix.cols).range(1..=MAX_MATRIX_SHOWN));
            ui.end_row();
            ui.label(tr("res.matrix_indices"));
            ui.checkbox(&mut matrix.show_indices, "");
            ui.end_row();
            ui.label(tr("res.matrix_collapse"));
            let mut collapsed = !matrix.expand_nested;
            if ui.checkbox(&mut collapsed, "").changed() {
                matrix.expand_nested = !collapsed;
            }
            ui.end_row();
        });
}

fn complex_form_label(form: ComplexForm) -> &'static str {
    match form {
        ComplexForm::Cartesian => tr("cplx.cartesian"),
        ComplexForm::Polar(AngleUnit::Radians) => tr("cplx.polar_radians"),
        ComplexForm::Polar(AngleUnit::Degrees) => tr("cplx.polar_degrees"),
    }
}

fn radix_label(radix: Radix) -> &'static str {
    match radix {
        Radix::Decimal => tr("radix.decimal"),
        Radix::Binary => tr("radix.binary"),
        Radix::Octal => tr("radix.octal"),
        Radix::Hexadecimal => tr("radix.hex"),
    }
}

/// The draft of the Worksheet options window: the settings it edits, as the window shows them.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionsDraft {
    /// `ORIGIN`: 0 or 1.
    pub origin: u32,
    /// `TOL`: the convergence tolerance.
    pub tolerance: f64,
    /// `CTOL`: the constraint tolerance.
    pub constraint_tolerance: f64,
    /// `PRNPRECISION`.
    pub prn_precision: u32,
    /// The worksheet's unit system (`currentUnitSystem`). A custom system shows as SI.
    pub unit_system: UnitSystem,
}

impl OptionsDraft {
    /// The draft of the settings, with the values a file does not state shown as Prime's defaults.
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            origin: settings.array_origin.unwrap_or(0),
            tolerance: settings.convergence_tolerance.unwrap_or(1e-3),
            constraint_tolerance: settings.constraint_tolerance.unwrap_or(1e-3),
            prn_precision: settings.prn_precision.unwrap_or(4),
            unit_system: settings
                .unit_system
                .as_deref()
                .and_then(UnitSystem::from_attr)
                .unwrap_or_default(),
        }
    }

    /// The settings with the draft's values written in. A value the draft did not change keeps
    /// the file's state (absent stays absent). Refused when a tolerance is not a positive number.
    pub fn applied_to(&self, settings: &Settings) -> Result<Settings, &'static str> {
        let positive = |t: f64| t.is_finite() && t > 0.0;
        if !positive(self.tolerance) || !positive(self.constraint_tolerance) {
            return Err(tr("set.tol_positive"));
        }
        let mut out = settings.clone();
        let base = Self::from_settings(settings);
        if self.origin != base.origin {
            out.array_origin = Some(self.origin);
        }
        if self.tolerance != base.tolerance {
            out.convergence_tolerance = Some(self.tolerance);
        }
        if self.constraint_tolerance != base.constraint_tolerance {
            out.constraint_tolerance = Some(self.constraint_tolerance);
        }
        if self.prn_precision != base.prn_precision {
            out.prn_precision = Some(self.prn_precision);
        }
        if self.unit_system != base.unit_system {
            out.unit_system = Some(self.unit_system.attr().to_owned());
        }
        Ok(out)
    }
}

/// The Worksheet options window (ORIGIN, TOL, CTOL, PRNPRECISION).
pub fn worksheet_options_window(ctx: &Context, draft: &mut OptionsDraft) -> Choice {
    let mut choice = Choice::Open;
    egui::Window::new(tr("dlg.worksheet_options"))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("worksheet_options_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label(tr("set.origin"));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut draft.origin, 0, "0");
                        ui.radio_value(&mut draft.origin, 1, "1");
                    });
                    ui.end_row();
                    ui.label(tr("set.tol"));
                    ui.add(
                        egui::DragValue::new(&mut draft.tolerance)
                            .speed(1e-4)
                            .max_decimals(15),
                    );
                    ui.end_row();
                    ui.label(tr("set.ctol"));
                    ui.add(
                        egui::DragValue::new(&mut draft.constraint_tolerance)
                            .speed(1e-4)
                            .max_decimals(15),
                    );
                    ui.end_row();
                    ui.label("PRNPRECISION");
                    ui.add(egui::DragValue::new(&mut draft.prn_precision).range(1..=16));
                    ui.end_row();
                    ui.label(tr("set.unit_system"));
                    egui::ComboBox::from_id_salt("worksheet_unit_system")
                        .selected_text(unit_system_label(draft.unit_system))
                        .show_ui(ui, |ui| {
                            for system in [UnitSystem::Si, UnitSystem::Us, UnitSystem::Cgs] {
                                ui.selectable_value(
                                    &mut draft.unit_system,
                                    system,
                                    unit_system_label(system),
                                );
                            }
                        });
                    ui.end_row();
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    choice = Choice::ApplyWorksheet;
                }
                if ui.button(tr("btn.cancel")).clicked() {
                    choice = Choice::Cancel;
                }
            });
        });
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_general_format_or_complex_threshold_is_refused_with_a_reason() {
        let base = ResultFormat::default();
        let decimal = ResultFormat {
            number: NumberFormat::Decimal,
            ..base.clone()
        };
        assert!(unstorable(&decimal, &base).is_some());
        let complex = ResultFormat {
            complex_threshold: 3,
            ..base.clone()
        };
        assert!(unstorable(&complex, &base).is_some());
        let precision = ResultFormat {
            precision: 6,
            ..base.clone()
        };
        assert_eq!(unstorable(&precision, &base), None);
    }

    #[test]
    fn options_write_only_what_changed() {
        let settings = Settings::default();
        let draft = OptionsDraft::from_settings(&settings);
        assert_eq!(draft.applied_to(&settings), Ok(settings.clone()));
        let changed = OptionsDraft { origin: 1, ..draft };
        let out = changed.applied_to(&settings).expect("valid");
        assert_eq!(out.array_origin, Some(1));
        assert_eq!(out.convergence_tolerance, None);
        assert_eq!(out.prn_precision, None);
    }

    #[test]
    fn a_non_positive_tolerance_is_refused() {
        let settings = Settings::default();
        let draft = OptionsDraft {
            tolerance: 0.0,
            ..OptionsDraft::from_settings(&settings)
        };
        assert!(draft.applied_to(&settings).is_err());
    }
}

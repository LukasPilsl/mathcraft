//! The Einheitensystem window (Prime screenshots 0170, 0176-0184): the radio buttons SI, USCS, CGS
//! and Benutzerdefiniert (with "Basierend auf"), the table of base dimensions and the table of
//! derived units, and the sub windows that change, insert and remove a row. The three predefined
//! systems are stored in the worksheet (`units/currentUnitSystem`). A custom system cannot be
//! stored yet because no sample holds one (UNVERIFIED, see `documentation/UNVERIFIED_FORMATS.md`),
//! so its rows are edited here only and OK keeps the system it is based on. The rows beyond those
//! visible in the screenshots are Mathcraft's own completion of the table.

use eframe::egui::{self, Align2, Context, RichText};
use mathcraft_core::units::UnitSystem;

use crate::i18n::tr;

/// One row of a table: a dimension and the unit it is shown in (both display names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub dimension: &'static str,
    pub unit: &'static str,
}

/// The radio buttons of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    System(UnitSystem),
    Custom,
}

/// What the window asks for after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Open,
    /// OK: store this system.
    Ok(UnitSystem),
    /// Abbrechen. Prime still marks the worksheet as modified (0184).
    Cancel,
}

/// A sub window on top of the main one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Sub {
    /// Change the unit of base dimension `row`.
    Base { row: usize, unit: usize },
    /// Change the unit of derived row `row`.
    Derived { row: usize, unit: usize },
    /// Insert a derived unit: the chosen dimension and unit of its list.
    Insert { dimension: usize, unit: usize },
}

/// The state of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitsDialog {
    choice: Choice,
    based_on: UnitSystem,
    base: Vec<Row>,
    derived: Vec<Row>,
    base_sel: usize,
    derived_sel: usize,
    sub: Option<Sub>,
    /// True once a row of a custom system was changed here.
    edited: bool,
}

/// `(dimension, units)` the sub windows offer (display names, German like the screenshots).
const CHOICES: &[(&str, &[&str])] = &[
    ("Geld", &["Basiswährung (¤)"]),
    (
        "Länge",
        &["Meter (m)", "Zentimeter (cm)", "Fuß (ft)", "Zoll (in)"],
    ),
    ("Masse", &["Kilogramm (kg)", "Gramm (gm)", "Pfund (lb)"]),
    ("Lichtstärke", &["Candela (cd)"]),
    ("Stoffmenge", &["Mol (mol)"]),
    ("Strom", &["Ampere (A)"]),
    ("Temperatur", &["Kelvin (K)"]),
    ("Zeit", &["Sekunde (s)"]),
    (
        "Druck",
        &[
            "Kilopascal (kPa)",
            "Kips pro Quadratfuß (ksf)",
            "Kips pro Quadratzoll (ksi)",
            "Megapascal (MPa)",
            "Pascal (Pa)",
            "Pfund (Kraft) pro Quadratzoll (psi)",
            "Barye (Ba)",
        ],
    ),
    ("Energie", &["Joule (J)", "Erg (erg)", "Kalorie (cal)"]),
    ("Kraft", &["Newton (N)", "Pfund (Kraft) (lbf)", "Dyn (dyn)"]),
    ("Leistung", &["Watt (W)", "Pferdestärke (hp)"]),
    (
        "Beschleunigung",
        &[
            "Gravitationsbeschleunigung (g)",
            "Meter pro Quadratsekunde (m/s²)",
        ],
    ),
    ("Aktivität", &["Becquerel (Bq)"]),
    ("Winkel", &["Grad (deg)", "Radiant (rad)"]),
    (
        "Flussrate",
        &["Gallonen pro Minute (gpm)", "Kubikmeter pro Sekunde (m³/s)"],
    ),
    ("Illuminosität", &["Lux (lx)"]),
    ("Induktivität", &["Henry (H)"]),
    ("Kapazität", &["Farad (F)"]),
];

fn row(dimension: &'static str, unit: &'static str) -> Row {
    Row { dimension, unit }
}

/// The base dimension rows of a predefined system.
pub fn base_rows(system: UnitSystem) -> Vec<Row> {
    let (length, mass) = match system {
        UnitSystem::Si => ("Meter (m)", "Kilogramm (kg)"),
        UnitSystem::Us => ("Fuß (ft)", "Pfund (lb)"),
        UnitSystem::Cgs => ("Zentimeter (cm)", "Gramm (gm)"),
    };
    vec![
        row("Geld", "Basiswährung (¤)"),
        row("Länge", length),
        row("Lichtstärke", "Candela (cd)"),
        row("Masse", mass),
        row("Stoffmenge", "Mol (mol)"),
        row("Strom", "Ampere (A)"),
        row("Temperatur", "Kelvin (K)"),
        row("Zeit", "Sekunde (s)"),
    ]
}

/// The derived unit rows of a predefined system.
pub fn derived_rows(system: UnitSystem) -> Vec<Row> {
    match system {
        UnitSystem::Si => vec![
            row("Druck", "Pascal (Pa)"),
            row("Energie", "Joule (J)"),
            row("Illuminosität", "Lux (lx)"),
            row("Induktivität", "Henry (H)"),
            row("Kapazität", "Farad (F)"),
            row("Kraft", "Newton (N)"),
            row("Leistung", "Watt (W)"),
        ],
        UnitSystem::Us => vec![
            row("Druck", "Pfund (Kraft) pro Quadratzoll (psi)"),
            row("Flussrate", "Gallonen pro Minute (gpm)"),
            row("Illuminosität", "Lux (lx)"),
            row("Induktivität", "Henry (H)"),
            row("Kapazität", "Farad (F)"),
            row("Kraft", "Pfund (Kraft) (lbf)"),
            row("Leistung", "Pferdestärke (hp)"),
        ],
        UnitSystem::Cgs => vec![
            row("Druck", "Barye (Ba)"),
            row("Energie", "Erg (erg)"),
            row("Illuminosität", "Lux (lx)"),
            row("Induktivität", "Henry (H)"),
            row("Kapazität", "Farad (F)"),
            row("Kraft", "Dyn (dyn)"),
            row("Leistung", "Watt (W)"),
        ],
    }
}

/// The units a dimension offers in the sub windows.
fn units_of(dimension: &str) -> &'static [&'static str] {
    CHOICES
        .iter()
        .find(|(d, _)| *d == dimension)
        .map_or(&[], |(_, units)| *units)
}

impl UnitsDialog {
    /// The window for a worksheet whose unit system is `system`.
    pub fn new(system: UnitSystem) -> Self {
        Self {
            choice: Choice::System(system),
            based_on: system,
            base: base_rows(system),
            derived: derived_rows(system),
            base_sel: 0,
            derived_sel: 0,
            sub: None,
            edited: false,
        }
    }

    /// The radio selection.
    pub fn choice(&self) -> Choice {
        self.choice
    }

    /// Selects a radio button: a predefined system shows its tables again; Benutzerdefiniert
    /// keeps the tables it shows now and is based on the system last shown.
    pub fn select(&mut self, choice: Choice) {
        match choice {
            Choice::System(system) => {
                self.base = base_rows(system);
                self.derived = derived_rows(system);
                self.based_on = system;
                self.base_sel = 0;
                self.derived_sel = 0;
                self.edited = false;
            }
            Choice::Custom => {}
        }
        self.choice = choice;
    }

    /// Selects the system a custom system is based on and shows its tables.
    pub fn set_based_on(&mut self, system: UnitSystem) {
        self.based_on = system;
        self.base = base_rows(system);
        self.derived = derived_rows(system);
        self.base_sel = 0;
        self.derived_sel = 0;
        self.edited = false;
    }

    pub fn base(&self) -> &[Row] {
        &self.base
    }

    pub fn derived(&self) -> &[Row] {
        &self.derived
    }

    /// The system OK stores: the chosen one, or the one a custom system is based on.
    pub fn result(&self) -> UnitSystem {
        match self.choice {
            Choice::System(system) => system,
            Choice::Custom => self.based_on,
        }
    }

    /// Changes the unit of base row `row` to `unit` (custom systems only).
    pub fn change_base(&mut self, row: usize, unit: &'static str) -> bool {
        match (self.choice, self.base.get_mut(row)) {
            (Choice::Custom, Some(r)) => {
                r.unit = unit;
                self.edited = true;
                true
            }
            _ => false,
        }
    }

    /// Changes the unit of derived row `row` to `unit` (custom systems only).
    pub fn change_derived(&mut self, row: usize, unit: &'static str) -> bool {
        match (self.choice, self.derived.get_mut(row)) {
            (Choice::Custom, Some(r)) => {
                r.unit = unit;
                self.edited = true;
                true
            }
            _ => false,
        }
    }

    /// Adds a derived row (custom systems only).
    pub fn insert_derived(&mut self, dimension: &'static str, unit: &'static str) -> bool {
        if self.choice != Choice::Custom {
            return false;
        }
        self.derived.push(row(dimension, unit));
        self.derived.sort_by(|a, b| a.dimension.cmp(b.dimension));
        self.edited = true;
        true
    }

    /// Removes derived row `row` (custom systems only).
    pub fn remove_derived(&mut self, row: usize) -> bool {
        if self.choice != Choice::Custom || row >= self.derived.len() {
            return false;
        }
        self.derived.remove(row);
        self.derived_sel = self.derived_sel.min(self.derived.len().saturating_sub(1));
        self.edited = true;
        true
    }
}

fn system_label(system: UnitSystem) -> &'static str {
    match system {
        UnitSystem::Si => "SI",
        UnitSystem::Us => "USCS",
        UnitSystem::Cgs => "CGS",
    }
}

/// A row as shown: "Dimension - Unit" (the screenshots show the German names in both languages).
fn row_text(r: &Row) -> String {
    format!("{} - {}", r.dimension, r.unit)
}

fn list(ui: &mut egui::Ui, id: &str, rows: &[Row], selected: &mut usize, height: f32) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .min_scrolled_height(height)
        .show(ui, |ui| {
            ui.set_min_width(210.0);
            for (i, r) in rows.iter().enumerate() {
                if ui.selectable_label(*selected == i, row_text(r)).clicked() {
                    *selected = i;
                }
            }
        });
}

/// Draws the window; returns what the user chose this frame.
pub fn show(ctx: &Context, dlg: &mut UnitsDialog) -> Outcome {
    let mut outcome = Outcome::Open;
    let custom = dlg.choice == Choice::Custom;
    let blocked = dlg.sub.is_some();
    egui::Window::new(tr("dlg.unit_system"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.add_enabled_ui(!blocked, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.label(tr("units.pick_system"));
                        let mut picked = None;
                        for system in [UnitSystem::Si, UnitSystem::Us, UnitSystem::Cgs] {
                            let on = dlg.choice == Choice::System(system);
                            if ui.radio(on, system_label(system)).clicked() && !on {
                                picked = Some(Choice::System(system));
                            }
                        }
                        if ui.radio(custom, tr("units.custom")).clicked() && !custom {
                            picked = Some(Choice::Custom);
                        }
                        ui.label(tr("units.based_on"));
                        ui.add_enabled_ui(custom, |ui| {
                            let mut based = dlg.based_on;
                            egui::ComboBox::from_id_salt("units_based_on")
                                .selected_text(system_label(based))
                                .show_ui(ui, |ui| {
                                    for system in [UnitSystem::Si, UnitSystem::Us, UnitSystem::Cgs]
                                    {
                                        ui.selectable_value(
                                            &mut based,
                                            system,
                                            system_label(system),
                                        );
                                    }
                                });
                            if based != dlg.based_on {
                                dlg.set_based_on(based);
                            }
                        });
                        if let Some(choice) = picked {
                            dlg.select(choice);
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label(tr("units.base_dims"));
                        ui.horizontal_top(|ui| {
                            let base = dlg.base.clone();
                            list(ui, "units_base", &base, &mut dlg.base_sel, 96.0);
                            if ui
                                .add_enabled(custom, egui::Button::new(tr("units.change")))
                                .clicked()
                            {
                                dlg.sub = Some(Sub::Base {
                                    row: dlg.base_sel,
                                    unit: 0,
                                });
                            }
                        });
                        ui.label(tr("units.derived"));
                        ui.horizontal_top(|ui| {
                            let derived = dlg.derived.clone();
                            list(ui, "units_derived", &derived, &mut dlg.derived_sel, 110.0);
                            ui.vertical(|ui| {
                                if ui
                                    .add_enabled(custom, egui::Button::new(tr("units.change")))
                                    .clicked()
                                {
                                    dlg.sub = Some(Sub::Derived {
                                        row: dlg.derived_sel,
                                        unit: 0,
                                    });
                                }
                                if ui
                                    .add_enabled(custom, egui::Button::new(tr("units.insert")))
                                    .clicked()
                                {
                                    dlg.sub = Some(Sub::Insert {
                                        dimension: 0,
                                        unit: 0,
                                    });
                                }
                                if ui
                                    .add_enabled(custom, egui::Button::new(tr("units.remove")))
                                    .clicked()
                                {
                                    dlg.remove_derived(dlg.derived_sel);
                                }
                            });
                        });
                    });
                });
                let note = if custom {
                    tr("units.custom_note")
                } else {
                    tr("units.predefined_note")
                };
                ui.label(RichText::new(note).small().weak());
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("btn.ok")).clicked() {
                        outcome = Outcome::Ok(dlg.result());
                    }
                    if ui.button(tr("btn.cancel")).clicked() {
                        outcome = Outcome::Cancel;
                    }
                    ui.add_enabled(false, egui::Button::new(tr("units.help")))
                        .on_disabled_hover_text(tr("tip.not_available"));
                });
            });
        });
    sub_window(ctx, dlg);
    outcome
}

/// The "Basisdimensions-Einheit" and "Abgeleitete Einheiten" windows.
fn sub_window(ctx: &Context, dlg: &mut UnitsDialog) {
    let Some(sub) = dlg.sub.clone() else {
        return;
    };
    let mut sub = sub;
    let mut done: Option<bool> = None;
    let title = match sub {
        Sub::Base { .. } => tr("dlg.base_dimension_unit"),
        _ => tr("dlg.derived_units"),
    };
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, egui::vec2(20.0, 20.0))
        .show(ctx, |ui| {
            // The dimension the units belong to.
            let dimension: &'static str = match &mut sub {
                Sub::Base { row, .. } => dlg.base.get(*row).map_or("", |r| r.dimension),
                Sub::Derived { row, .. } => dlg.derived.get(*row).map_or("", |r| r.dimension),
                Sub::Insert { dimension, .. } => {
                    ui.label(tr("units.dimension"));
                    egui::ScrollArea::vertical()
                        .id_salt("units_dims")
                        .max_height(96.0)
                        .show(ui, |ui| {
                            for (i, (name, _)) in CHOICES.iter().enumerate() {
                                if ui.selectable_label(*dimension == i, *name).clicked() {
                                    *dimension = i;
                                }
                            }
                        });
                    CHOICES.get(*dimension).map_or("", |(d, _)| *d)
                }
            };
            if !matches!(sub, Sub::Insert { .. }) {
                ui.label(tr("units.dimension"));
                ui.add_enabled(false, egui::Label::new(dimension));
            }
            ui.label(tr("units.unit"));
            let units = units_of(dimension);
            let unit = match &mut sub {
                Sub::Base { unit, .. } | Sub::Derived { unit, .. } | Sub::Insert { unit, .. } => {
                    unit
                }
            };
            *unit = (*unit).min(units.len().saturating_sub(1));
            egui::ScrollArea::vertical()
                .id_salt("units_units")
                .max_height(110.0)
                .show(ui, |ui| {
                    for (i, name) in units.iter().enumerate() {
                        if ui.selectable_label(*unit == i, *name).clicked() {
                            *unit = i;
                        }
                    }
                });
            let chosen = units.get(*unit).copied();
            ui.horizontal(|ui| {
                if ui.button(tr("btn.ok")).clicked() {
                    done = Some(true);
                }
                if ui.button(tr("btn.cancel")).clicked() {
                    done = Some(false);
                }
            });
            if done == Some(true) {
                if let Some(unit) = chosen {
                    match sub {
                        Sub::Base { row, .. } => {
                            dlg.change_base(row, unit);
                        }
                        Sub::Derived { row, .. } => {
                            dlg.change_derived(row, unit);
                        }
                        Sub::Insert { .. } => {
                            dlg.insert_derived(dimension, unit);
                        }
                    }
                }
            }
        });
    dlg.sub = if done.is_some() { None } else { Some(sub) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_radio_buttons_show_each_systems_base_units() {
        let mut dlg = UnitsDialog::new(UnitSystem::Si);
        assert_eq!(dlg.base()[1].unit, "Meter (m)");
        dlg.select(Choice::System(UnitSystem::Us));
        assert_eq!(dlg.base()[1].unit, "Fuß (ft)");
        assert_eq!(dlg.base()[3].unit, "Pfund (lb)");
        dlg.select(Choice::System(UnitSystem::Cgs));
        assert_eq!(dlg.base()[1].unit, "Zentimeter (cm)");
        assert_eq!(dlg.base()[3].unit, "Gramm (gm)");
        assert_eq!(dlg.result(), UnitSystem::Cgs);
    }

    #[test]
    fn predefined_systems_cannot_be_edited() {
        let mut dlg = UnitsDialog::new(UnitSystem::Si);
        assert!(!dlg.change_base(1, "Fuß (ft)"));
        assert!(!dlg.remove_derived(0));
        assert!(!dlg.insert_derived("Kraft", "Newton (N)"));
        assert_eq!(dlg.derived().len(), derived_rows(UnitSystem::Si).len());
    }

    #[test]
    fn a_custom_system_edits_its_tables_and_stores_its_base() {
        let mut dlg = UnitsDialog::new(UnitSystem::Si);
        dlg.select(Choice::Custom);
        assert_eq!(dlg.result(), UnitSystem::Si);
        assert!(dlg.change_base(1, "Fuß (ft)"));
        assert_eq!(dlg.base()[1].unit, "Fuß (ft)");
        let before = dlg.derived().len();
        assert!(dlg.remove_derived(0));
        assert_eq!(dlg.derived().len(), before - 1);
        assert!(dlg.insert_derived("Beschleunigung", "Gravitationsbeschleunigung (g)"));
        assert_eq!(dlg.derived().len(), before);
        dlg.set_based_on(UnitSystem::Us);
        assert_eq!(dlg.result(), UnitSystem::Us);
        assert_eq!(dlg.base()[1].unit, "Fuß (ft)");
    }
}

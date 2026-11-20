//! What the Berechnung and Dokument tabs show of the open worksheet: ORIGIN, TOL, CTOL, the unit
//! system and the page setup. A small `Copy` snapshot, so [`RibbonState`](super::RibbonState)
//! stays `Copy`.

use crate::edit::session::EditSession;
use mathcraft_import::page::known_paper_codes;

/// The settings of the open worksheet, as the ribbon shows them (Prime's defaults when no
/// worksheet is open or the file does not state a value).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocInfo {
    /// `ORIGIN`: 0 or 1.
    pub origin: u32,
    /// `TOL`.
    pub tol: f64,
    /// `CTOL`.
    pub ctol: f64,
    /// The unit system: `si`, `us` or `cgs` (anything else shows as `si`).
    pub units: &'static str,
    /// The paper code, one of `known_paper_codes()` (`A4` for an unknown one).
    pub paper: &'static str,
    pub landscape: bool,
    /// `Standard`, `Narrow`, `Wide` or `Legacy`.
    pub margin: &'static str,
    pub fine_grid: bool,
}

impl Default for DocInfo {
    fn default() -> Self {
        Self {
            origin: 0,
            tol: 1e-3,
            ctol: 1e-3,
            units: "si",
            paper: "A4",
            landscape: false,
            margin: "Standard",
            fine_grid: false,
        }
    }
}

/// The margin types of Prime's Ränder list, in its order.
pub const MARGIN_TYPES: [&str; 4] = ["Standard", "Narrow", "Wide", "Legacy"];

impl DocInfo {
    /// The snapshot of `session` (the defaults for `None`).
    pub fn of(session: Option<&EditSession>) -> Self {
        let Some(session) = session else {
            return Self::default();
        };
        let doc = session.document();
        let settings = doc.settings();
        let page = doc.page();
        let paper = known_paper_codes()
            .find(|c| c.eq_ignore_ascii_case(&page.paper_code))
            .unwrap_or("A4");
        let margin = MARGIN_TYPES
            .into_iter()
            .find(|m| m.eq_ignore_ascii_case(&page.margin_type))
            .unwrap_or("Standard");
        let units = match settings.unit_system.as_deref() {
            Some("us") => "us",
            Some("cgs") => "cgs",
            _ => "si",
        };
        Self {
            origin: settings.array_origin.unwrap_or(0),
            tol: settings.convergence_tolerance.unwrap_or(1e-3),
            ctol: settings.constraint_tolerance.unwrap_or(1e-3),
            units,
            paper,
            landscape: page.is_landscape(),
            margin,
            fine_grid: page.grid_size.eq_ignore_ascii_case("Fine"),
        }
    }
}

/// The value shown in a ribbon field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Origin,
    Tol,
    Ctol,
}

impl Value {
    /// The text of the field for `info`.
    pub fn text(self, info: &DocInfo) -> String {
        match self {
            Value::Origin => info.origin.to_string(),
            Value::Tol => tolerance_text(info.tol),
            Value::Ctol => tolerance_text(info.ctol),
        }
    }
}

/// `10⁻³` for a power of ten, else the plain number.
pub fn tolerance_text(value: f64) -> String {
    match power_of_ten(value) {
        Some(exp) => power_text(exp),
        None => format!("{value}"),
    }
}

/// `10` with the exponent as superscript characters.
pub fn power_text(exp: i32) -> String {
    let mut out = String::from("10");
    for c in exp.to_string().chars() {
        out.push(match c {
            '-' => '⁻',
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            _ => '⁹',
        });
    }
    out
}

/// The exponent when `value` is `10^exp` (to the precision of a stored number).
pub fn power_of_ten(value: f64) -> Option<i32> {
    if !(value.is_finite() && value > 0.0) {
        return None;
    }
    let exp = value.log10().round();
    let back = 10f64.powi(exp as i32);
    ((value - back).abs() <= back * 1e-9).then_some(exp as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerances_show_as_powers_of_ten() {
        assert_eq!(tolerance_text(1e-3), "10⁻³");
        assert_eq!(tolerance_text(0.1), "10⁻¹");
        assert_eq!(tolerance_text(1e-7), "10⁻⁷");
        assert_eq!(tolerance_text(0.0005), "0.0005");
    }

    #[test]
    fn defaults_are_primes() {
        let info = DocInfo::of(None);
        assert_eq!(
            (info.origin, info.paper, info.margin),
            (0, "A4", "Standard")
        );
    }
}

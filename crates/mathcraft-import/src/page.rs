//! Page settings: paper, orientation and margin type, read from `<pageModel>` in
//! `mathcad/settings/presentation.xml`.
//!
//! Every sample stores `paper-code="A4" orientation="Portrait" margin-type="Standard"`. The file
//! stores only the margin *type*, not its values. Standard is 25 mm, as measured on Prime's
//! screenshot 0003; the other types are Mathcraft's own guesses (see [`Margins::preset`]) until a
//! sample saved with other margins shows the real ones. Header and footer parts are empty in every
//! sample, so they are not modelled.

use crate::dom::{self, DomError};

/// Namespace of `presentation.xml`.
pub const PRESENTATION_NS: &str = "http://schemas.ptc.com/mathcad/settings/presentation10";

/// Paper sizes in points (1 pt = 1/72 inch), portrait: width, then height.
const PAPERS: [(&str, [f64; 2]); 6] = [
    ("A4", [595.275_590_551, 841.889_763_78]),
    ("Letter", [612.0, 792.0]),
    ("Legal", [612.0, 1008.0]),
    ("A3", [841.889_763_78, 1_190.551_181_1]),
    // `Tabloid` (Prime: Ledger/US-Tabloid, 11 x 17 in) and `A5` are Mathcraft's spelling of the
    // codes: no sample is saved with them (UNVERIFIED).
    ("Tabloid", [792.0, 1224.0]),
    ("A5", [419.527_559_055, 595.275_590_551]),
];

/// The page settings of a worksheet. Attribute values are kept as the file spells them, so an
/// unknown paper code survives a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSettings {
    /// `pageModel/@paper-code`, for example `A4`.
    pub paper_code: String,
    /// `pageModel/@orientation`: `Portrait` or `Landscape`.
    pub orientation: String,
    /// `pageModel/@margin-type`: `Standard`, `Narrow`, `Wide` or `Legacy`.
    pub margin_type: String,
    /// `pageModel/@grid-size`: `Standard` or `Fine`.
    pub grid_size: String,
}

impl Default for PageSettings {
    fn default() -> Self {
        Self {
            paper_code: "A4".to_owned(),
            orientation: "Portrait".to_owned(),
            margin_type: "Standard".to_owned(),
            grid_size: "Standard".to_owned(),
        }
    }
}

/// Margins in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Margins {
    /// The margins of a margin type, from Prime's Ränder list (screenshot 0200): Standard 25 mm,
    /// Narrow (Schmal) 12.5 mm, Wide (Breit) 50 mm, Legacy 30.48 mm (1.2 in) on every side.
    /// Unknown types use Standard.
    pub fn preset(margin_type: &str) -> Self {
        let mm = match margin_type {
            "Narrow" => 12.5,
            "Legacy" => 30.48,
            "Wide" => 50.0,
            _ => 25.0,
        };
        let pt = mm / 25.4 * 72.0;
        Self {
            top: pt,
            right: pt,
            bottom: pt,
            left: pt,
        }
    }
}

impl PageSettings {
    /// Reads `<pageModel>`. A missing part or element gives the defaults.
    ///
    /// # Errors
    /// [`DomError`] when the XML does not parse.
    pub fn from_presentation(xml: Option<&str>) -> Result<Self, DomError> {
        let mut page = Self::default();
        let Some(xml) = xml else {
            return Ok(page);
        };
        let doc = dom::parse(xml)?;
        let Some(node) = doc
            .descendants()
            .find(|n| n.has_tag_name((PRESENTATION_NS, "pageModel")))
        else {
            return Ok(page);
        };
        if let Some(v) = node.attribute("paper-code") {
            page.paper_code = v.to_owned();
        }
        if let Some(v) = node.attribute("orientation") {
            page.orientation = v.to_owned();
        }
        if let Some(v) = node.attribute("margin-type") {
            page.margin_type = v.to_owned();
        }
        if let Some(v) = node.attribute("grid-size") {
            page.grid_size = v.to_owned();
        }
        Ok(page)
    }

    /// True for `Landscape`.
    pub fn is_landscape(&self) -> bool {
        self.orientation.eq_ignore_ascii_case("Landscape")
    }

    /// Paper width and height in points after orientation. An unknown paper code uses A4.
    pub fn paper_points(&self) -> [f64; 2] {
        let base = PAPERS
            .iter()
            .find(|(code, _)| code.eq_ignore_ascii_case(&self.paper_code))
            .map_or(PAPERS[0].1, |(_, size)| *size);
        if self.is_landscape() {
            [base[1], base[0]]
        } else {
            base
        }
    }

    /// The margins for this page's margin type.
    pub fn margins(&self) -> Margins {
        Margins::preset(&self.margin_type)
    }
}

/// The paper codes Mathcraft knows, in the order the page setup dialog lists them.
pub fn known_paper_codes() -> impl Iterator<Item = &'static str> {
    PAPERS.iter().map(|(code, _)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<presentation xmlns="http://schemas.ptc.com/mathcad/settings/presentation10"><pageModel paper-code="A4" grid-size="Standard" orientation="Portrait" margin-type="Standard" /></presentation>"#;

    #[test]
    fn reads_the_sample_page_model() {
        let page = PageSettings::from_presentation(Some(SAMPLE)).unwrap();
        assert_eq!(page, PageSettings::default());
        assert_eq!(page.paper_points()[0].round(), 595.0);
        assert_eq!(page.paper_points()[1].round(), 842.0);
    }

    #[test]
    fn landscape_swaps_the_paper() {
        let xml = SAMPLE.replace("Portrait", "Landscape");
        let page = PageSettings::from_presentation(Some(&xml)).unwrap();
        let [w, h] = page.paper_points();
        assert!(w > h);
    }

    #[test]
    fn missing_part_gives_defaults() {
        assert_eq!(
            PageSettings::from_presentation(None).unwrap(),
            PageSettings::default()
        );
    }

    #[test]
    fn unknown_paper_code_falls_back_to_a4_but_keeps_its_name() {
        let xml = SAMPLE.replace("A4", "B5");
        let page = PageSettings::from_presentation(Some(&xml)).unwrap();
        assert_eq!(page.paper_code, "B5");
        assert_eq!(page.paper_points()[0].round(), 595.0);
    }

    #[test]
    fn margin_presets_are_ordered() {
        let narrow = Margins::preset("Narrow").top;
        let standard = Margins::preset("Standard").top;
        let wide = Margins::preset("Wide").top;
        assert!(narrow < standard && standard < wide);
    }

    #[test]
    fn standard_margins_are_25_mm_on_each_side() {
        let m = Margins::preset("Standard");
        for side in [m.top, m.right, m.bottom, m.left] {
            assert!((side * 25.4 / 72.0 - 25.0).abs() < 1e-9, "{side} pt");
        }
    }
}

//! The bundled fonts and their egui families.
//!
//! Math and results use STIX Two Text in four faces (roman, italic, bold, bold italic), which
//! also carries Greek. Operators, relations and arrows come from STIX Two Math, which is listed
//! after the text face in every family, so a symbol the text face lacks is drawn from the math
//! face. Text regions and the window chrome use Noto Sans in the same four faces (a Tahoma-like
//! sans, as Prime uses for text). Licences and sources: `assets/fonts/LICENSES/` and
//! `documentation/THIRD_PARTY_LICENSES.md`.
//!
//! The font files are subsets of the STIX Two releases (`pyftsubset` from fonttools, keeping the
//! Latin, Greek, punctuation, letterlike, arrow, operator and mathematical-alphanumeric ranges).
//!
//! Everything here is safe without a font: a missing face or glyph is reported by [`covers`] and
//! drawn as a box by `glyphs::printable`; nothing panics.

use std::sync::{Arc, OnceLock};

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

/// Roman, italic, bold and bold italic faces of the sheet text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Face {
    Roman,
    Italic,
    Bold,
    BoldItalic,
}

impl Face {
    /// Every face, in index order.
    pub const ALL_FACES: [Face; 4] = [Face::Roman, Face::Italic, Face::Bold, Face::BoldItalic];

    /// Every face, for checks over all of them.
    #[cfg(test)]
    pub const ALL: [Face; 4] = [Face::Roman, Face::Italic, Face::Bold, Face::BoldItalic];

    /// The face of a run with the given weight and slant.
    pub fn of(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => Face::Roman,
            (false, true) => Face::Italic,
            (true, false) => Face::Bold,
            (true, true) => Face::BoldItalic,
        }
    }

    fn index(self) -> usize {
        match self {
            Face::Roman => 0,
            Face::Italic => 1,
            Face::Bold => 2,
            Face::BoldItalic => 3,
        }
    }
}

const TEXT_NAMES: [&str; 4] = [
    "STIXTwoText-Regular",
    "STIXTwoText-Italic",
    "STIXTwoText-Bold",
    "STIXTwoText-BoldItalic",
];
const MATH_NAME: &str = "STIXTwoMath-Regular";

/// The sans faces of text regions, in the same order as [`Face`].
const SANS_NAMES: [&str; 4] = [
    "NotoSans-Regular",
    "NotoSans-Italic",
    "NotoSans-Bold",
    "NotoSans-BoldItalic",
];
const SANS_DATA: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../assets/fonts/NotoSans-Italic.ttf"),
    include_bytes!("../assets/fonts/NotoSans-Bold.ttf"),
    include_bytes!("../assets/fonts/NotoSans-BoldItalic.ttf"),
];

const TEXT_DATA: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/STIXTwoText-Regular.ttf"),
    include_bytes!("../assets/fonts/STIXTwoText-Italic.ttf"),
    include_bytes!("../assets/fonts/STIXTwoText-Bold.ttf"),
    include_bytes!("../assets/fonts/STIXTwoText-BoldItalic.ttf"),
];
const MATH_DATA: &[u8] = include_bytes!("../assets/fonts/STIXTwoMath-Regular.ttf");

/// The parsed faces, read once for coverage and advance widths. `None` only if a file is corrupt.
struct Parsed {
    text: [Option<ttf_parser::Face<'static>>; 4],
    sans: [Option<ttf_parser::Face<'static>>; 4],
    math: Option<ttf_parser::Face<'static>>,
}

fn parsed() -> &'static Parsed {
    static PARSED: OnceLock<Parsed> = OnceLock::new();
    PARSED.get_or_init(|| Parsed {
        text: TEXT_DATA.map(|data| ttf_parser::Face::parse(data, 0).ok()),
        sans: SANS_DATA.map(|data| ttf_parser::Face::parse(data, 0).ok()),
        math: ttf_parser::Face::parse(MATH_DATA, 0).ok(),
    })
}

/// The egui family of a math or result face (STIX Two Text).
pub fn family(face: Face) -> FontFamily {
    FontFamily::Name(TEXT_NAMES[face.index()].into())
}

/// The egui family of a text region's face (Noto Sans, the sans of Prime's text regions).
pub fn text_family(face: Face) -> FontFamily {
    FontFamily::Name(SANS_NAMES[face.index()].into())
}

/// The glyph of `c` in `face`, from the text face or, failing that, the math face.
fn glyph(face: Face, c: char) -> Option<(&'static ttf_parser::Face<'static>, ttf_parser::GlyphId)> {
    let parsed = parsed();
    if let Some(font) = parsed.text[face.index()].as_ref() {
        if let Some(id) = font.glyph_index(c) {
            return Some((font, id));
        }
    }
    let math = parsed.math.as_ref()?;
    math.glyph_index(c).map(|id| (math, id))
}

/// The glyph of `c` in the sans face of `face` (text regions), when Noto Sans has it.
fn sans_glyph(
    face: Face,
    c: char,
) -> Option<(&'static ttf_parser::Face<'static>, ttf_parser::GlyphId)> {
    let font = parsed().sans[face.index()].as_ref()?;
    font.glyph_index(c).map(|id| (font, id))
}

/// Advance of `c` in the text-region face `face`, in ems: Noto Sans, else the sheet face (see
/// [`text_slot`]). `None` when no bundled face has the glyph.
pub fn text_advance_em(face: Face, c: char) -> Option<f32> {
    match sans_glyph(face, c) {
        Some((font, id)) => {
            let units = f32::from(font.units_per_em());
            let adv = font.glyph_hor_advance(id)?;
            (units > 0.0).then(|| f32::from(adv) / units)
        }
        None => advance_em(face, c),
    }
}

/// Whether one of the bundled faces of `face` has a glyph for `c`.
pub fn covers(face: Face, c: char) -> bool {
    glyph(face, c).is_some()
}

/// Advance of `c` in `face`, in ems. `None` when no bundled face has the glyph.
pub fn advance_em(face: Face, c: char) -> Option<f32> {
    let (font, id) = glyph(face, c)?;
    let units = f32::from(font.units_per_em());
    if units <= 0.0 {
        return None;
    }
    let adv = font.glyph_hor_advance(id)?;
    Some(f32::from(adv) / units)
}

/// Index of the math face in the bundled files.
const MATH_FILE: usize = 4;

/// The bundled font file and glyph id that draw `c` in `face`, the same choice [`covers`] and
/// [`advance_em`] make. Used by the PDF writer, which embeds the file and names the glyph.
pub fn slot(face: Face, c: char) -> Option<(usize, u16)> {
    let parsed = parsed();
    if let Some(font) = parsed.text[face.index()].as_ref() {
        if let Some(id) = font.glyph_index(c) {
            return Some((face.index(), id.0));
        }
    }
    let id = parsed.math.as_ref()?.glyph_index(c)?;
    Some((MATH_FILE, id.0))
}

/// True for the faces without slant.
fn upright(face: Face) -> bool {
    matches!(face, Face::Roman | Face::Bold)
}

/// Index of the first Noto Sans file in the bundled files (four faces, after the STIX ones).
const SANS_FILE: usize = 5;

/// The bundled font file and glyph id that draw `c` in the text-region face `face`: Noto Sans when it
/// has the glyph, else the same choice as [`slot`] (STIX text face, then math face). Math and
/// results never use this; they use [`slot`].
pub fn text_slot(face: Face, c: char) -> Option<(usize, u16)> {
    match sans_glyph(face, c) {
        Some((_, id)) => Some((SANS_FILE + face.index(), id.0)),
        None => slot(face, c),
    }
}

/// The bytes of a bundled font file, for [`slot`] and [`text_slot`]. `None` for an unknown index.
pub fn file_data(file: usize) -> Option<&'static [u8]> {
    match file {
        0..=3 => Some(TEXT_DATA[file]),
        MATH_FILE => Some(MATH_DATA),
        f if (SANS_FILE..SANS_FILE + 4).contains(&f) => Some(SANS_DATA[f - SANS_FILE]),
        _ => None,
    }
}

/// Registers the bundled faces with egui. Each math face family is the face itself, then the math
/// face for symbols, then egui's default proportional fonts (emoji and other scripts). Each sans
/// face family is the sans face, then the STIX face of the same slant (Greek and symbols), then
/// the math face and the defaults. The proportional family (window chrome) starts with Noto Sans.
///
/// Must run before any text is laid out in a context, because egui panics on an unbound family.
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let fallback = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, data) in TEXT_NAMES.iter().zip(TEXT_DATA) {
        defs.font_data
            .insert((*name).to_owned(), Arc::new(FontData::from_static(data)));
    }
    defs.font_data.insert(
        MATH_NAME.to_owned(),
        Arc::new(FontData::from_static(MATH_DATA)),
    );
    for name in TEXT_NAMES {
        let mut chain = vec![name.to_owned(), MATH_NAME.to_owned()];
        chain.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(name.into()), chain);
    }
    for (index, (name, data)) in SANS_NAMES.iter().zip(SANS_DATA).enumerate() {
        defs.font_data
            .insert((*name).to_owned(), Arc::new(FontData::from_static(data)));
        // Upright faces take what Noto Sans lacks (≤ ≥ ≠ and Greek) from egui's default sans
        // before STIX Text, which draws these symbols small and raised (QA finding F20); italic
        // ones keep STIX Text first (italic Greek). The PDF still uses `text_slot`'s STIX choice.
        let mut chain = vec![(*name).to_owned()];
        if upright(Face::ALL_FACES[index]) {
            chain.extend(fallback.iter().take(1).cloned());
            chain.push(TEXT_NAMES[index].to_owned());
            chain.push(MATH_NAME.to_owned());
        } else {
            chain.push(TEXT_NAMES[index].to_owned());
            chain.push(MATH_NAME.to_owned());
        }
        chain.extend(fallback.iter().cloned());
        defs.families
            .insert(FontFamily::Name((*name).into()), chain);
    }
    if let Some(chrome) = defs.families.get_mut(&FontFamily::Proportional) {
        chrome.insert(0, SANS_NAMES[0].to_owned());
    }
    ctx.set_fonts(defs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_parses() {
        let p = parsed();
        assert!(p.text.iter().all(Option::is_some));
        assert!(p.math.is_some());
        for data in SANS_DATA {
            assert!(ttf_parser::Face::parse(data, 0).is_ok());
        }
    }

    #[test]
    fn sans_faces_cover_latin_and_the_bold_is_wider() {
        let sans = |index: usize, c: char| {
            let face = ttf_parser::Face::parse(SANS_DATA[index], 0).expect("parses");
            face.glyph_index(c).is_some()
        };
        for c in ['A', 'z', 'ä', 'ß', '€', '°'] {
            assert!(sans(0, c), "{c}");
        }
        let regular = ttf_parser::Face::parse(SANS_DATA[0], 0).expect("parses");
        let bold = ttf_parser::Face::parse(SANS_DATA[2], 0).expect("parses");
        let adv = |f: &ttf_parser::Face, c: char| {
            let id = f.glyph_index(c).expect("covered");
            f.glyph_hor_advance(id).expect("advance")
        };
        assert!(adv(&bold, 'o') > adv(&regular, 'o'));
    }

    #[test]
    fn faces_are_chosen_from_weight_and_slant() {
        assert_eq!(Face::of(false, false), Face::Roman);
        assert_eq!(Face::of(false, true), Face::Italic);
        assert_eq!(Face::of(true, false), Face::Bold);
        assert_eq!(Face::of(true, true), Face::BoldItalic);
    }

    #[test]
    fn italic_and_bold_faces_are_really_different_from_roman() {
        // The italic 'a' slants, so its outline box differs from the roman one; the bold 'a' is
        // wider than the roman one.
        let bounds = |face: Face| {
            let (font, id) = glyph(face, 'a').expect("covered");
            font.glyph_bounding_box(id).map(|r| (r.x_min, r.x_max))
        };
        assert_ne!(bounds(Face::Roman), bounds(Face::Italic));
        let roman_adv = advance_em(Face::Roman, 'a').expect("covered");
        let bold_adv = advance_em(Face::Bold, 'a').expect("covered");
        assert!(bold_adv > roman_adv, "{bold_adv} > {roman_adv}");
    }

    #[test]
    fn text_regions_draw_in_noto_sans_and_math_in_stix() {
        let (file, _) = text_slot(Face::Roman, 'a').expect("covered");
        assert_eq!(file, SANS_FILE);
        assert_eq!(file_data(file), Some(SANS_DATA[0]));
        let (bold, _) = text_slot(Face::Bold, 'a').expect("covered");
        assert_eq!(bold, SANS_FILE + 2);
        // The sheet's math keeps the STIX choice.
        assert_eq!(slot(Face::Roman, 'a').map(|(f, _)| f), Some(0));
        // A character Noto Sans lacks falls back to the sheet choice.
        for c in ['α', '∑', '→'] {
            let fallback = slot(Face::Roman, c).map(|(f, _)| f);
            let chosen = text_slot(Face::Roman, c).map(|(f, _)| f);
            assert!(
                chosen == fallback || chosen.is_some_and(|f| f >= SANS_FILE),
                "{c}"
            );
        }
        assert!(text_advance_em(Face::Roman, 'W').expect("covered") > 0.0);
    }

    #[test]
    fn advances_are_positive_for_drawn_glyphs() {
        for c in ['a', 'W', 'α', '1', '+', '→'] {
            let adv = advance_em(Face::Roman, c).expect("covered");
            assert!(adv > 0.0 && adv < 2.0, "{c}: {adv}");
        }
    }
}

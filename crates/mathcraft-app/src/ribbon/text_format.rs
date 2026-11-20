//! The Textformatierung tab (screenshots 0021, 0215-0223). The buttons act on the selected text of
//! a text region being typed, and are greyed until some is selected (0215). Font, size, bold,
//! italic, underline, colour, alignment, the Textstile looks and the link are wired. Lists, tags,
//! spelling, the subtitle style and the text background have no model yet and are greyed (see
//! `documentation/UI_PARITY_RIBBON.md`).

use mathcraft_import::rich_text::{Align, RunStyle, StyleChange};

use crate::ribbon::spec::{large, stack, Button, Col, Group, Item, Menu, Rule, TabSpec};
use crate::ribbon::{Action, TextFormat, TextLook};
use crate::ribbon_icons::Icon;

/// Prime's font list (0216). The names are stored in the file; drawing uses Mathcraft's own faces.
const FONTS: &[Item] = &[
    font_item("Tahoma"),
    font_item("Arial"),
    font_item("Calibri"),
    font_item("Times New Roman"),
    font_item("Courier New"),
    font_item("Mathcad UniMath Prime"),
];

const fn font_item(name: &'static str) -> Item {
    Item::act(
        name,
        name,
        Rule::TextSelection,
        Action::TextFormat(TextFormat::Font(name)),
    )
}

/// Prime's size list (0217): 8 to 24 points.
const SIZES: &[Item] = &[
    size_item("8", 8),
    size_item("9", 9),
    size_item("10", 10),
    size_item("11", 11),
    size_item("12", 12),
    size_item("14", 14),
    size_item("16", 16),
    size_item("18", 18),
    size_item("20", 20),
    size_item("22", 22),
    size_item("24", 24),
];

const fn size_item(label: &'static str, points: u16) -> Item {
    Item::act(
        label,
        label,
        Rule::TextSelection,
        Action::TextFormat(TextFormat::Size(points)),
    )
}

const FONT_COL: Col = stack(&[
    Button::new("font", "Text font", "Textschriftart", Icon::Font)
        .menu(Menu::Items(FONTS))
        .rule(Rule::TextSelection),
    Button::new("size", "Size", "Größe", Icon::Blank)
        .menu(Menu::Items(SIZES))
        .rule(Rule::TextSelection),
]);

const EMPHASIS_COL: Col = stack(&[
    Button::new("bold", "Bold", "Fett", Icon::Bold)
        .act(Action::TextFormat(TextFormat::ToggleBold))
        .rule(Rule::TextSelection),
    Button::new("italic", "Italic", "Kursiv", Icon::Italic)
        .act(Action::TextFormat(TextFormat::ToggleItalic))
        .rule(Rule::TextSelection),
    Button::new("underline", "Underline", "Unterstrichen", Icon::Text("U"))
        .act(Action::TextFormat(TextFormat::ToggleUnderline))
        .rule(Rule::TextSelection),
]);

const COLOUR_COL: Col = stack(&[
    Button::new("colour", "Colour", "Textfarbe", Icon::Text("A"))
        .menu(Menu::TextColours)
        .rule(Rule::TextSelection),
    // Prime's text highlight (Hintergrundfarbe). Mathcraft keeps no text background yet.
    Button::new(
        "highlight",
        "Background colour",
        "Hintergrundfarbe",
        Icon::Text("▮"),
    ),
]);

const LIST_COL: Col = stack(&[
    Button::new("bullets", "Bullets", "Aufzählung", Icon::Table),
    Button::new("numbering", "Numbering", "Nummerierung", Icon::Text("1.")),
]);

const ALIGN_COL: Col = stack(&[
    Button::new("align_left", "Align left", "Links", Icon::AlignLeft)
        .act(Action::TextFormat(TextFormat::Align(Align::Left)))
        .rule(Rule::TextSelection),
    Button::new("align_centre", "Centre", "Zentriert", Icon::AlignCentre)
        .act(Action::TextFormat(TextFormat::Align(Align::Center)))
        .rule(Rule::TextSelection),
]);

const ALIGN_COL_2: Col = stack(&[
    Button::new("align_right", "Align right", "Rechts", Icon::AlignRight)
        .act(Action::TextFormat(TextFormat::Align(Align::Right)))
        .rule(Rule::TextSelection),
    Button::new("align_justify", "Justify", "Blocksatz", Icon::Text("J"))
        .act(Action::TextFormat(TextFormat::Align(Align::Justify)))
        .rule(Rule::TextSelection),
]);

const FONT_GROUP: &[Col] = &[FONT_COL, EMPHASIS_COL, COLOUR_COL];
const PARAGRAPH_GROUP: &[Col] = &[LIST_COL, ALIGN_COL, ALIGN_COL_2];

const STYLES: &[Col] = &[
    large(
        Button::new("normal", "Normal", "Normal", Icon::Text("Aa"))
            .act(Action::TextFormat(TextFormat::Look(TextLook::Normal)))
            .rule(Rule::TextSelection),
    ),
    large(
        Button::new("title", "Title", "Titel", Icon::Text("Aa"))
            .act(Action::TextFormat(TextFormat::Look(TextLook::Title)))
            .rule(Rule::TextSelection),
    ),
    large(
        Button::new("heading", "Heading 1", "Überschrift 1", Icon::Text("Aa"))
            .act(Action::TextFormat(TextFormat::Look(TextLook::Heading1)))
            .rule(Rule::TextSelection),
    ),
    large(
        Button::new("heading2", "Heading 2", "Überschrift 2", Icon::Text("Aa"))
            .act(Action::TextFormat(TextFormat::Look(TextLook::Heading2)))
            .rule(Rule::TextSelection),
    ),
    large(Button::new(
        "subtitle",
        "Subtitle",
        "Untertitel",
        Icon::Text("Aa"),
    )),
];

const LINKS: &[Col] = &[stack(&[
    Button::new("link", "Link", "Link", Icon::Text("↗"))
        .act(Action::TextLink)
        .rule(Rule::TextSelection),
    Button::new("unlink", "Remove link", "Link entfernen", Icon::Text("×"))
        .act(Action::TextFormat(TextFormat::Unlink))
        .rule(Rule::TextLinked),
    Button::new("tag", "Tag", "Tag", Icon::Text("#")),
])];

const SPELLING: &[Col] = &[large(Button::new(
    "spell",
    "Spelling",
    "Rechtschreibung",
    Icon::Text("abc"),
))];

pub static TAB: TabSpec = TabSpec {
    key: "tab.text_format",
    groups: &[
        Group::new("Text font", "Textschriftart", FONT_GROUP),
        Group::new("Paragraph", "Absatz", PARAGRAPH_GROUP),
        Group::new("Text styles", "Textstile", STYLES),
        Group::new("Links", "Links", LINKS),
        Group::new("Spelling", "Rechtschreibung", SPELLING),
    ],
};

/// The rows of the Textfarbe palette (screenshot 0129): the standard colours, then tints and
/// shades of each. Prime's exact values are not in the screenshot, so these are close guesses
/// (see `documentation/UNVERIFIED_FORMATS.md`).
pub fn colour_rows() -> Vec<[[u8; 3]; 10]> {
    const HUES: [[u8; 3]; 10] = [
        [0x00, 0x00, 0x00],
        [0x7f, 0x4f, 0x24],
        [0x2f, 0x55, 0x97],
        [0x1f, 0x1f, 0x8b],
        [0x1a, 0x7f, 0x8e],
        [0xd7, 0x26, 0x2c],
        [0x70, 0x30, 0xa0],
        [0x2e, 0x8b, 0x3d],
        [0xff, 0xd7, 0x00],
        [0xf2, 0x8c, 0x28],
    ];
    // Moves `c` by `t` of the way towards `toward` (255 for a tint, 0 for a shade).
    let mix = |c: [u8; 3], t: f32, toward: f32| -> [u8; 3] {
        c.map(|v| (v as f32 + (toward - v as f32) * t).round() as u8)
    };
    let mut rows = vec![HUES];
    for t in [0.8, 0.6, 0.4] {
        rows.push(HUES.map(|c| mix(c, t, 255.0)));
    }
    for t in [0.25, 0.5] {
        rows.push(HUES.map(|c| mix(c, t, 0.0)));
    }
    rows
}

/// The change a format makes to the selected text, whose current styles are `selected` (one per
/// run). The toggles switch off when all of the selection has the property already. `None` for
/// the alignment, which the session sets per paragraph.
pub fn style_change(format: TextFormat, selected: &[RunStyle]) -> Option<StyleChange> {
    let all = |f: fn(&RunStyle) -> bool| !selected.is_empty() && selected.iter().all(f);
    Some(match format {
        TextFormat::ToggleBold => StyleChange::Bold(!all(|s| s.bold)),
        TextFormat::ToggleItalic => StyleChange::Italic(!all(|s| s.italic)),
        TextFormat::ToggleUnderline => StyleChange::Underline(!all(|s| s.underline)),
        TextFormat::Colour(c) => StyleChange::Colour(c),
        TextFormat::Font(name) => StyleChange::Family(name),
        TextFormat::Size(points) => StyleChange::Size(f32::from(points)),
        TextFormat::Unlink => StyleChange::Link(None),
        TextFormat::Look(look) => {
            let (family, size, colour, bold) = match look {
                TextLook::Normal => (DEFAULT_FAMILY, 14.666_667, BLACK, false),
                TextLook::Title => (DEFAULT_FAMILY, 37.333_333, BLACK, false),
                TextLook::Heading1 => (DEFAULT_FAMILY, 21.333_333, HEADING, false),
                TextLook::Heading2 => (DEFAULT_FAMILY, 17.333_333, HEADING, false),
            };
            StyleChange::Look {
                family,
                size,
                colour,
                bold,
                italic: false,
            }
        }
        TextFormat::Align(_) => return None,
    })
}

/// Prime's default text font (`presentation.xml` textStyles of the samples).
const DEFAULT_FAMILY: &str = "Tahoma";
const BLACK: [u8; 4] = [0, 0, 0, 255];
/// The heading colour of Prime's Überschrift styles (`presentation.xml`, `#FF2E3192`).
const HEADING: [u8; 4] = [0x2e, 0x31, 0x92, 0xff];

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bold: bool) -> RunStyle {
        RunStyle {
            bold,
            ..RunStyle::default()
        }
    }

    #[test]
    fn a_toggle_switches_off_only_when_all_of_the_selection_has_it() {
        let mixed = [run(true), run(false)];
        assert_eq!(
            style_change(TextFormat::ToggleBold, &mixed),
            Some(StyleChange::Bold(true))
        );
        let all_bold = [run(true), run(true)];
        assert_eq!(
            style_change(TextFormat::ToggleBold, &all_bold),
            Some(StyleChange::Bold(false))
        );
        assert_eq!(
            style_change(TextFormat::ToggleBold, &[]),
            Some(StyleChange::Bold(true))
        );
    }

    #[test]
    fn alignment_is_not_a_character_change_and_unlink_removes_the_link() {
        assert_eq!(style_change(TextFormat::Align(Align::Center), &[]), None);
        assert_eq!(
            style_change(TextFormat::Unlink, &[]),
            Some(StyleChange::Link(None))
        );
    }

    #[test]
    fn the_heading_look_uses_the_sample_colour_and_size() {
        let Some(StyleChange::Look { size, colour, .. }) =
            style_change(TextFormat::Look(TextLook::Heading1), &[])
        else {
            panic!("a look")
        };
        assert_eq!(colour, HEADING);
        assert!((size - 21.333_333).abs() < 1e-3);
    }

    #[test]
    fn the_palette_has_rows_of_ten_and_starts_black() {
        let rows = colour_rows();
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|r| r.len() == 10));
        assert_eq!(rows[0][0], [0, 0, 0]);
    }
}

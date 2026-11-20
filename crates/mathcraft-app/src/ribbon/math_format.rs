//! The Mathematische Formatierung tab (screenshot 0020). Stub: the Results format window is wired.

use crate::ribbon::spec::{large, stack, Button, Col, Group, TabSpec};
use crate::ribbon::Action;
use crate::ribbon_icons::Icon;

const FONT: &[Col] = &[
    stack(&[
        Button::new("font", "Math font", "Mathematische Schriftart", Icon::Font)
            .combo(&["Mathcad UniMath Print"]),
        Button::new("size", "Size", "Größe", Icon::Blank).number("11"),
        Button::new("color", "Text colour", "Textfarbe", Icon::Text("A")),
    ]),
    large(Button::new("fill", "Fill colour", "Füllfarbe", Icon::Area)),
];

const RESULTS: &[Col] = &[
    large(
        Button::new(
            "results",
            "Result format",
            "Ergebnisformat",
            Icon::ResultFormat,
        )
        .act(Action::ResultFormat),
    ),
    large(Button::new(
        "clear",
        "Clear format",
        "Format löschen",
        Icon::Text("×"),
    )),
];

const LABELS: &[Col] = &[stack(&[
    Button::new("bold", "Bold", "Fett", Icon::Bold),
    Button::new("italic", "Italic", "Kursiv", Icon::Italic),
    Button::new("underline", "Underline", "Unterstrichen", Icon::Text("U")),
])];

pub static TAB: TabSpec = TabSpec {
    key: "tab.math_format",
    groups: &[
        Group::new("Math font", "Mathematische Schriftart", FONT),
        Group::new("Results", "Ergebnisse", RESULTS),
        Group::new("Label styles", "Beschriftungsstile", LABELS),
    ],
};

//! The Eingabe/Ausgabe tab (screenshots 0016, 0072-0083, 0243, 0244). Nothing here exists in
//! Mathcraft yet; every button is listed in `documentation/UI_PARITY_RIBBON.md`.

use crate::ribbon::spec::{large, stack, Button, Col, Group, Item, Menu, Rule, TabSpec};
use crate::ribbon_icons::Icon;

const WORKSHEETS: &[Col] = &[
    large(Button::new(
        "embed",
        "Embed worksheet",
        "Arbeitsblatt einschließen",
        Icon::Worksheet,
    )),
    large(Button::new(
        "cache",
        "Save worksheet in cache",
        "Arbeitsblatt im Cache speichern",
        Icon::Folder,
    )),
    large(
        Button::new(
            "convert",
            "XMCD, MCD converter",
            "XMCD-, MCD-Konvertierer",
            Icon::Arrow,
        )
        .rule(Rule::Never),
    ),
    large(
        Button::new(
            "clear_notes",
            "Delete annotations",
            "Anmerkungen löschen",
            Icon::Text("×"),
        )
        .rule(Rule::Never),
    ),
];

const EXCEL: &[Item] = &[
    Item::missing("Insert Excel component", "Excel-Komponente einfügen"),
    Item::missing("Insert input expression", "Eingabeausdruck einfügen"),
    Item::missing("Insert output expression", "Ausgabeausdruck einfügen"),
];

const DATA: &[Col] = &[
    large(
        Button::new(
            "datafile",
            "Data file name",
            "Name der Datendatei",
            Icon::Folder,
        )
        .rule(Rule::Never),
    ),
    large(Button::new("readexcel", "READEXCEL", "READEXCEL", Icon::Folder).rule(Rule::Never)),
    large(
        Button::new(
            "excel",
            "Excel component",
            "Excel-Komponente",
            Icon::Text("X"),
        )
        .menu(Menu::Items(EXCEL)),
    ),
];

const OBJECTS: &[Col] = &[
    large(Button::new(
        "object",
        "Insert object",
        "Objekt einfügen",
        Icon::Area,
    )),
    stack(&[
        Button::new(
            "aspect",
            "Keep aspect ratio",
            "Seitenverhältnis beibehalten",
            Icon::Area,
        ),
        Button::new(
            "resetsize",
            "Reset object size",
            "Objektgröße zurücksetzen",
            Icon::Recalculate,
        )
        .rule(Rule::Never),
        Button::new(
            "link",
            "Update link",
            "Link aktualisieren",
            Icon::Recalculate,
        )
        .rule(Rule::Never),
    ]),
];

const SIMPLE: &[Item] = &[Item::missing("Combo box", "Kombinationsfeld")];
const ADVANCED: &[Item] = &[
    Item::missing("List box", "Listenfeld"),
    Item::missing("Check box", "Kontrollkästchen"),
    Item::missing("Text box", "Textfeld"),
    Item::missing("Option button", "Optionsfeld"),
    Item::missing("Slider", "Schieberegler"),
    Item::missing("Button", "Schaltfläche"),
];

const CONTROLS: &[Col] = &[
    large(Button::new("simple", "Simple", "Einfach", Icon::Area).menu(Menu::Items(SIMPLE))),
    large(
        Button::new("advanced", "Advanced", "Erweitert", Icon::Text("</>"))
            .menu(Menu::Items(ADVANCED)),
    ),
    large(Button::new("edit", "Edit", "Bearbeiten", Icon::Options).rule(Rule::Never)),
    large(Button::new(
        "hide_left",
        "Hide left side",
        "Linke Seite ausblenden",
        Icon::Arrow,
    )),
    large(
        Button::new(
            "shrink_left",
            "Shrink left side",
            "Linke Seite verkleinern",
            Icon::Arrow,
        )
        .rule(Rule::Never),
    ),
];

const INTEGRATION: &[Col] = &[
    large(
        Button::new(
            "assign_in",
            "Assign inputs",
            "Eingaben zuweisen",
            Icon::Arrow,
        )
        .rule(Rule::Never),
    ),
    large(
        Button::new(
            "assign_out",
            "Assign outputs",
            "Ausgaben zuweisen",
            Icon::Arrow,
        )
        .rule(Rule::Never),
    ),
    large(Button::new(
        "as_list",
        "Show as list",
        "Als Liste anzeigen",
        Icon::Table,
    )),
    large(Button::new(
        "in_sheet",
        "Show in worksheet",
        "Im Arbeitsblatt anzeigen",
        Icon::Grid,
    )),
];

pub static TAB: TabSpec = TabSpec {
    key: "tab.io",
    groups: &[
        Group::new("Worksheets (.mcdx)", "Arbeitsblätter (.mcdx)", WORKSHEETS),
        Group::new("Data import/export", "Datenimport/-export", DATA),
        Group::new("Objects", "Objekte", OBJECTS),
        Group::new("Controls", "Steuerelemente", CONTROLS).collapsed(Icon::Area),
        Group::new("Integration", "Integration", INTEGRATION).collapsed(Icon::Arrow),
    ],
};

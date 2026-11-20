//! The Dokument tab (screenshots 0023, 0138-0144, 0197-0204, 0208-0212): Bereiche, Abstand and the
//! five dropdown groups Seite, Ansicht, Kopfzeilen und Fußzeilen, Bereichsformatierung and
//! Zwischenablage. Page setup is stored in the worksheet (`pageModel`).

use crate::edit::layout::Arrange;
use crate::ribbon::spec::{large, stack, Button, Col, Flag, Group, Item, Menu, Rule, TabSpec};
use crate::ribbon::{Action, ZoomStep};
use crate::ribbon_icons::Icon;

const REGIONS: &[Col] = &[
    large(Button::new("section", "Section", "Abschnitt", Icon::Area).act(Action::InsertArea)),
    stack(&[
        Button::new("textblock", "Text block", "Textblock", Icon::TextRegion)
            .act(Action::InsertText)
            .rule(Rule::NoRegion),
        Button::new("textfield", "Text field", "Textfeld", Icon::Text("A"))
            .act(Action::InsertTextField)
            .rule(Rule::NoRegion),
        Button::new("image", "Image", "Bild", Icon::Image)
            .act(Action::InsertImage)
            .rule(Rule::NoRegion),
    ]),
];

// Separate regions (0197): the selected regions that overlap move apart.
const SEPARATE: &[Item] = &[
    Item::act(
        "Vertically",
        "Vertikal",
        Rule::Selection,
        Action::SeparateRegions(true),
    ),
    Item::act(
        "Horizontally",
        "Horizontal",
        Rule::Selection,
        Action::SeparateRegions(false),
    ),
];
// Insert blank line (0202).
const BLANK: &[Item] = &[
    Item::act(
        "Single line",
        "Einzelne Zeile",
        Rule::Doc,
        Action::InsertLines,
    ),
    Item::act("Page", "Seite", Rule::Doc, Action::InsertBlankPage),
];
// Remove blank line (0203). The page break entry is Mathcraft's own: Prime removes one with Delete.
const UNBLANK: &[Item] = &[
    Item::act(
        "Single line",
        "Einzelne Zeile",
        Rule::Doc,
        Action::RemoveLines,
    ),
    Item::act(
        "Empty area",
        "Leerer Bereich",
        Rule::Doc,
        Action::RemoveEmptyArea,
    ),
    Item::act(
        "Page break",
        "Seitenumbruch",
        Rule::Doc,
        Action::DeletePageBreak,
    ),
];

const SPACING: &[Col] = &[
    large(
        Button::new(
            "separate",
            "Separate regions",
            "Bereiche trennen",
            Icon::InsertLines,
        )
        .menu(Menu::Items(SEPARATE)),
    ),
    large(
        Button::new(
            "pagebreak",
            "Add page break",
            "Seitenumbruch hinzufügen",
            Icon::PageLayout,
        )
        .act(Action::InsertPageBreak),
    ),
    large(
        Button::new(
            "blank",
            "Insert blank line",
            "Leerzeile einfügen",
            Icon::InsertLines,
        )
        .menu(Menu::Items(BLANK)),
    ),
    large(
        Button::new(
            "delblank",
            "Remove blank line",
            "Leerzeile entfernen",
            Icon::Minus,
        )
        .menu(Menu::Items(UNBLANK)),
    ),
];

// Seite (0140, 0198-0201).
const PAPER: &[Item] = &[
    Item::act(
        "Letter (8.5\" x 11\")",
        "Letter (8,5\" x 11\")",
        Rule::Doc,
        Action::SetPaper("Letter"),
    )
    .mark(Flag::Paper("Letter")),
    Item::act(
        "US Legal (8.5\" x 14\")",
        "US-Legal (8,5\" x 14\")",
        Rule::Doc,
        Action::SetPaper("Legal"),
    )
    .mark(Flag::Paper("Legal")),
    Item::act(
        "Ledger/US Tabloid (11\" x 17\")",
        "Ledger/US-Tabloid (11\" x 17\")",
        Rule::Doc,
        Action::SetPaper("Tabloid"),
    )
    .mark(Flag::Paper("Tabloid")),
    Item::act(
        "A3 (297 mm x 420 mm)",
        "A3 (297 mm x 420 mm)",
        Rule::Doc,
        Action::SetPaper("A3"),
    )
    .mark(Flag::Paper("A3")),
    Item::act(
        "A4 (210 mm x 297 mm)",
        "A4 (210 mm x 297 mm)",
        Rule::Doc,
        Action::SetPaper("A4"),
    )
    .mark(Flag::Paper("A4")),
    Item::act(
        "A5 (148 mm x 210 mm)",
        "A5 (148 mm x 210 mm)",
        Rule::Doc,
        Action::SetPaper("A5"),
    )
    .mark(Flag::Paper("A5")),
];
const ORIENTATION: &[Item] = &[
    Item::act(
        "Portrait",
        "Hochformat",
        Rule::Doc,
        Action::SetLandscape(false),
    )
    .mark(Flag::Portrait),
    Item::act(
        "Landscape",
        "Querformat",
        Rule::Doc,
        Action::SetLandscape(true),
    )
    .mark(Flag::Landscape),
];
const MARGINS: &[Item] = &[
    Item::act(
        "Standard (25 mm)",
        "Standard (25 mm)",
        Rule::Doc,
        Action::SetMargins("Standard"),
    )
    .mark(Flag::Margin("Standard")),
    Item::act(
        "Narrow (12.5 mm)",
        "Schmal (12,5 mm)",
        Rule::Doc,
        Action::SetMargins("Narrow"),
    )
    .mark(Flag::Margin("Narrow")),
    Item::act(
        "Wide (50 mm)",
        "Breit (50 mm)",
        Rule::Doc,
        Action::SetMargins("Wide"),
    )
    .mark(Flag::Margin("Wide")),
    Item::act(
        "Legacy (30.48 mm)",
        "Legacy (30,48 mm)",
        Rule::Doc,
        Action::SetMargins("Legacy"),
    )
    .mark(Flag::Margin("Legacy")),
    Item::act(
        "Custom margins...",
        "Benutzerdefinierte Ränder...",
        Rule::Doc,
        Action::PageSetup,
    ),
];
const GRID_SIZE: &[Item] = &[
    Item::act(
        "Standard",
        "Standard",
        Rule::Doc,
        Action::SetFineGrid(false),
    )
    .mark(Flag::StandardGrid),
    Item::act("Fine", "Fein", Rule::Doc, Action::SetFineGrid(true)).mark(Flag::FineGrid),
];
const PAGE_MENU: &[Item] = &[
    Item::sub("Paper size", "Papiergröße", PAPER),
    Item::sub("Orientation", "Ausrichtung", ORIENTATION),
    Item::sub("Margins", "Ränder", MARGINS),
    Item::act(
        "Show grid",
        "Raster anzeigen",
        Rule::Doc,
        Action::ToggleGrid,
    )
    .mark(Flag::Grid),
    // Prime's page frame and major grid lines: how they are stored is not known (UNVERIFIED).
    Item::missing("Show frame", "Rahmen anzeigen").tip(
        "UNVERIFIED: the page frame is not stored in the samples.",
        "UNVERIFIED: der Seitenrahmen ist in den Beispielen nicht gespeichert.",
    ),
    Item::sub("Grid size", "Rastergröße", GRID_SIZE),
    Item::missing("Show major grid lines", "Hauptrasterlinien anzeigen").tip(
        "UNVERIFIED: major grid lines are not stored in the samples.",
        "UNVERIFIED: Hauptrasterlinien sind in den Beispielen nicht gespeichert.",
    ),
];

// Ansicht (0141): page or draft view; the zoom entries are Mathcraft's (Prime has a slider).
const VIEW_MENU: &[Item] = &[
    Item::act("Page", "Seite", Rule::Doc, Action::SetPageView(true)).mark(Flag::PageLayout),
    Item::act("Draft", "Entwurf", Rule::Doc, Action::SetPageView(false)).mark(Flag::DraftView),
    Item::act(
        "Zoom in",
        "Vergrößern",
        Rule::Doc,
        Action::Zoom(ZoomStep::In),
    ),
    Item::act(
        "Zoom out",
        "Verkleinern",
        Rule::Doc,
        Action::Zoom(ZoomStep::Out),
    ),
    Item::act(
        "Zoom 100%",
        "Zoom 100%",
        Rule::Doc,
        Action::Zoom(ZoomStep::Reset),
    ),
];

// Kopfzeilen und Fußzeilen (0208-0211). The tokens are Mathcraft's own (see `furniture.rs`).
const PAGE_NUMBER: &[Item] = &[
    Item::act("#", "#", Rule::Doc, Action::InsertField("{page}")),
    Item::act(
        "Page #",
        "Seite #",
        Rule::Doc,
        Action::InsertField("Page {page}"),
    ),
    Item::act("p. #", "S. #", Rule::Doc, Action::InsertField("p. {page}")),
    Item::act(
        "# of #",
        "# von #",
        Rule::Doc,
        Action::InsertField("{page} of {pages}"),
    ),
    Item::act(
        "Page # of #",
        "Seite # von #",
        Rule::Doc,
        Action::InsertField("Page {page} of {pages}"),
    ),
    Item::act(
        "- # -",
        "- # -",
        Rule::Doc,
        Action::InsertField("- {page} -"),
    ),
    Item::act("(#)", "(#)", Rule::Doc, Action::InsertField("({page})")),
    Item::act(
        "#(#)",
        "#(#)",
        Rule::Doc,
        Action::InsertField("{page}({pages})"),
    ),
    Item::act(
        "Remove page number",
        "Seitenzahl entfernen",
        Rule::Doc,
        Action::RemoveField("page"),
    ),
];
// Prime lists five numeric formats; the two with a repeated pattern are given the slash form here
// (UNVERIFIED).
const SAVE_DATE: &[Item] = &[
    Item::act(
        "dd.mm.yyyy",
        "tt.mm.jjjj",
        Rule::Doc,
        Action::InsertField("{date:dd.mm.yyyy}"),
    ),
    Item::act(
        "dd.mm.yy",
        "tt.mm.jj",
        Rule::Doc,
        Action::InsertField("{date:dd.mm.yy}"),
    ),
    Item::act(
        "dd/mm/yyyy",
        "tt/mm/jjjj",
        Rule::Doc,
        Action::InsertField("{date:dd/mm/yyyy}"),
    ),
    Item::act(
        "dd/mm/yy",
        "tt/mm/jj",
        Rule::Doc,
        Action::InsertField("{date:dd/mm/yy}"),
    ),
    Item::act(
        "yyyy-mm-dd",
        "jjjj-mm-tt",
        Rule::Doc,
        Action::InsertField("{date:yyyy-mm-dd}"),
    ),
    Item::act(
        "Short system date",
        "Kurzes Systemdatum",
        Rule::Doc,
        Action::InsertField("{date:short}"),
    ),
    Item::act(
        "Long system date",
        "Langes Systemdatum",
        Rule::Doc,
        Action::InsertField("{date:long}"),
    ),
    Item::act(
        "Remove saved date",
        "Gespeichertes Datum entfernen",
        Rule::Doc,
        Action::RemoveField("date"),
    ),
];
const FILE: &[Item] = &[
    Item::act("Name", "Name", Rule::Doc, Action::InsertField("{filename}")),
    Item::act("Path", "Pfad", Rule::Doc, Action::InsertField("{path}")),
];
const HEADER_MENU: &[Item] = &[
    Item::act(
        "Header",
        "Kopfzeile",
        Rule::Doc,
        Action::EditFurniture(false),
    ),
    Item::act("Footer", "Fußzeile", Rule::Doc, Action::EditFurniture(true)),
    Item::sub("Page number", "Seitenzahl", PAGE_NUMBER),
    Item::sub("Save date", "Speicherdatum", SAVE_DATE),
    Item::sub("File", "Datei", FILE),
];

// Bereichsformatierung (0143) holds only "Rand anzeigen" in Prime; the other lines are
// Mathcraft's own arrange commands (they have no ribbon place in Prime).
const FORMAT_MENU: &[Item] = &[
    Item::act(
        "Show border",
        "Rand anzeigen",
        Rule::Doc,
        Action::ToggleRegionBorders,
    )
    .mark(Flag::RegionBorders),
    Item::act(
        "Area around selection",
        "Bereich um Auswahl",
        Rule::Selection,
        Action::AreaAroundSelection,
    ),
    Item::act(
        "Align left",
        "Links ausrichten",
        Rule::Selection,
        Action::Arrange(Arrange::AlignLeft),
    ),
    Item::act(
        "Align right",
        "Rechts ausrichten",
        Rule::Selection,
        Action::Arrange(Arrange::AlignRight),
    ),
    Item::act(
        "Align centre",
        "Zentrieren",
        Rule::Selection,
        Action::Arrange(Arrange::AlignCenterX),
    ),
    Item::act(
        "Align top",
        "Oben ausrichten",
        Rule::Selection,
        Action::Arrange(Arrange::AlignTop),
    ),
    Item::act(
        "Align bottom",
        "Unten ausrichten",
        Rule::Selection,
        Action::Arrange(Arrange::AlignBottom),
    ),
    Item::act(
        "Align middle",
        "Mittig ausrichten",
        Rule::Selection,
        Action::Arrange(Arrange::AlignCenterY),
    ),
    Item::act(
        "Distribute horizontally",
        "Horizontal verteilen",
        Rule::Selection,
        Action::Arrange(Arrange::DistributeHorizontal),
    ),
    Item::act(
        "Distribute vertically",
        "Vertikal verteilen",
        Rule::Selection,
        Action::Arrange(Arrange::DistributeVertical),
    ),
];

const CLIPBOARD_MENU: &[Item] = &[
    Item::act("Cut", "Ausschneiden", Rule::Selection, Action::Cut),
    Item::act("Copy", "Kopieren", Rule::Selection, Action::Copy),
    Item::act("Paste", "Einfügen", Rule::Doc, Action::Paste),
];

const PAGE: &[Col] = &[large(
    Button::new("page", "Page", "Seite", Icon::PageSetup)
        .menu(Menu::Items(PAGE_MENU))
        .below(),
)];
const VIEW: &[Col] = &[large(
    Button::new("view", "View", "Ansicht", Icon::PageLayout)
        .menu(Menu::Items(VIEW_MENU))
        .below(),
)];
const HEADER: &[Col] = &[large(
    Button::new(
        "header",
        "Headers and footers",
        "Kopfzeilen und Fußzeilen",
        Icon::PageSetup,
    )
    .menu(Menu::Items(HEADER_MENU))
    .below(),
)];
const FORMAT: &[Col] = &[large(
    Button::new(
        "format",
        "Region formatting",
        "Bereichsformatierung",
        Icon::Borders,
    )
    .menu(Menu::Items(FORMAT_MENU))
    .below(),
)];
const CLIPBOARD: &[Col] = &[large(
    Button::new("clipboard", "Clipboard", "Zwischenablage", Icon::Folder)
        .menu(Menu::Items(CLIPBOARD_MENU))
        .below(),
)];

pub static TAB: TabSpec = TabSpec {
    key: "tab.document",
    groups: &[
        Group::new("Regions", "Bereiche", REGIONS),
        Group::new("Spacing", "Abstand", SPACING),
        Group::new("", "", PAGE),
        Group::new("", "", VIEW),
        Group::new("", "", HEADER),
        Group::new("", "", FORMAT),
        Group::new("", "", CLIPBOARD),
    ],
};

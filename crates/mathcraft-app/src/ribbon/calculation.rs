//! The Berechnung tab (screenshots 0022, 0134-0137, 0190-0197). Controls, error trace, the
//! redefinition warnings (greyed: Mathcraft has no such warnings), and the worksheet settings
//! ORIGIN, TOL and CTOL, which are stored in the worksheet (`builtInVariables`).

use crate::ribbon::doc_info::Value;
use crate::ribbon::spec::{large, stack, Button, Col, Flag, Group, Item, Menu, Rule, TabSpec};
use crate::ribbon::{Action, Trace};
use crate::ribbon_icons::Icon;

const CONTROLS: &[Col] = &[
    large(
        Button::new(
            "auto",
            "Automatic calculation",
            "Automatische Berechnung",
            Icon::Calculate,
        )
        .act(Action::ToggleCalcMode)
        .toggle(Flag::AutoCalc),
    ),
    large(
        Button::new(
            "halt",
            "Stop all calculations",
            "Alle Berechnungen anhalten",
            Icon::Text("||"),
        )
        .act(Action::StopCalc),
    ),
    large(
        Button::new(
            "sheet",
            "Calculate worksheet",
            "Arbeitsblatt berechnen",
            Icon::Recalculate,
        )
        .act(Action::Calculate)
        .rule(Rule::Manual)
        .tip(
            "Active while automatic calculation is off.",
            "Aktiv, solange die automatische Berechnung aus ist.",
        ),
    ),
    large(
        Button::new("now", "Calculate now", "Jetzt berechnen", Icon::Calculate)
            .act(Action::Calculate)
            .rule(Rule::Manual)
            .tip(
                "Active while automatic calculation is off.",
                "Aktiv, solange die automatische Berechnung aus ist.",
            ),
    ),
    // Prime stores a disabled flag per region; the .mcdx samples have none (UNVERIFIED), so the
    // flag lives in the session and a save warns.
    large(
        Button::new(
            "disable",
            "Disable region",
            "Bereich deaktivieren",
            Icon::Text("x"),
        )
        .act(Action::ToggleDisabled)
        .rule(Rule::Selection)
        .tip(
            "Not calculated while disabled. UNVERIFIED: not saved in the file (its format is not known).",
            "Wird nicht berechnet. UNVERIFIED: wird nicht gespeichert (das Dateiformat ist nicht bekannt).",
        ),
    ),
];

const TRACE: &[Col] = &[
    stack(&[
        Button::new("first", "First", "Erster", Icon::Text("|<"))
            .act(Action::ErrorTrace(Trace::First))
            .rule(Rule::Errors),
        Button::new("prev", "Previous", "Zurück", Icon::Text("<"))
            .act(Action::ErrorTrace(Trace::Previous))
            .rule(Rule::Errors),
    ]),
    stack(&[
        Button::new("next", "Next", "Weiter", Icon::Text(">"))
            .act(Action::ErrorTrace(Trace::Next))
            .rule(Rule::Errors),
        Button::new("last", "Last", "Letzter", Icon::Text(">|"))
            .act(Action::ErrorTrace(Trace::Last))
            .rule(Rule::Errors),
    ]),
];

const BUILT_IN: &[Item] = &[
    Item::missing("Functions", "Funktionen"),
    Item::missing("Units", "Einheiten"),
    Item::missing("Constants", "Konstanten"),
    Item::missing("Variables", "Variablen"),
];
const USER_DEFINED: &[Item] = &[
    Item::missing("Functions", "Funktionen"),
    Item::missing("Scalar variables", "Skalare Variablen"),
    Item::missing("Vectors and matrices", "Vektoren und Matrizen"),
];
// Mathcraft has no redefinition warnings, so every entry is greyed (0193).
const REDEFINITION: &[Item] = &[
    Item::sub("Built-in", "Integriert", BUILT_IN),
    Item::sub("User-defined", "Benutzerdefiniert", USER_DEFINED),
];

const WARNINGS: &[Col] = &[large(
    Button::new("redef", "Redefinition", "Neudefinition", Icon::Text("!"))
        .menu(Menu::Items(REDEFINITION))
        .below()
        .tip(
            "Mathcraft gives no redefinition warnings yet.",
            "Mathcraft gibt noch keine Neudefinitionswarnungen aus.",
        ),
)];

const ORIGIN_MENU: &[Item] = &[
    Item::act("0", "0", Rule::Doc, Action::SetOrigin(0)).mark(Flag::Origin(0)),
    Item::act("1", "1", Rule::Doc, Action::SetOrigin(1)).mark(Flag::Origin(1)),
];
const TOL_MENU: &[Item] = &[
    Item::act("10⁻¹", "10⁻¹", Rule::Doc, Action::SetTol(-1)).mark(Flag::Tol(-1)),
    Item::act("10⁻²", "10⁻²", Rule::Doc, Action::SetTol(-2)).mark(Flag::Tol(-2)),
    Item::act("10⁻³", "10⁻³", Rule::Doc, Action::SetTol(-3)).mark(Flag::Tol(-3)),
    Item::act("10⁻⁴", "10⁻⁴", Rule::Doc, Action::SetTol(-4)).mark(Flag::Tol(-4)),
    Item::act("10⁻⁵", "10⁻⁵", Rule::Doc, Action::SetTol(-5)).mark(Flag::Tol(-5)),
    Item::act("10⁻⁶", "10⁻⁶", Rule::Doc, Action::SetTol(-6)).mark(Flag::Tol(-6)),
    Item::act("10⁻⁷", "10⁻⁷", Rule::Doc, Action::SetTol(-7)).mark(Flag::Tol(-7)),
];
const CTOL_MENU: &[Item] = &[
    Item::act("10⁻¹", "10⁻¹", Rule::Doc, Action::SetCtol(-1)).mark(Flag::Ctol(-1)),
    Item::act("10⁻²", "10⁻²", Rule::Doc, Action::SetCtol(-2)).mark(Flag::Ctol(-2)),
    Item::act("10⁻³", "10⁻³", Rule::Doc, Action::SetCtol(-3)).mark(Flag::Ctol(-3)),
    Item::act("10⁻⁴", "10⁻⁴", Rule::Doc, Action::SetCtol(-4)).mark(Flag::Ctol(-4)),
    Item::act("10⁻⁵", "10⁻⁵", Rule::Doc, Action::SetCtol(-5)).mark(Flag::Ctol(-5)),
    Item::act("10⁻⁶", "10⁻⁶", Rule::Doc, Action::SetCtol(-6)).mark(Flag::Ctol(-6)),
    Item::act("10⁻⁷", "10⁻⁷", Rule::Doc, Action::SetCtol(-7)).mark(Flag::Ctol(-7)),
];

// Prime's list (0135, 0196). These options have no place in the worksheet model yet (UNVERIFIED
// where the file keeps them), so they are greyed; the first line is Mathcraft's own window for the
// settings the model does hold.
const OPTIONS_MENU: &[Item] = &[
    Item::act(
        "Worksheet options...",
        "Arbeitsblattoptionen...",
        Rule::Doc,
        Action::WorksheetOptions,
    ),
    Item::missing("Approximate equality", "Annähernde Gleichheit"),
    Item::missing("ORIGIN in strings", "ORIGIN in Zeichenfolgen"),
    Item::missing("Strict singularity check", "Strikte Singularitätsprüfung"),
    Item::missing("Multithreading", "Multithreading"),
    Item::missing(
        "Units/constants in symbolic math",
        "Einheiten/Konstanten in symbolischer Mathematik",
    ),
    Item::missing("Bin/Oct/Hex mode", "Bin/Okt/Hex-Modus"),
];

const SETTINGS: &[Col] = &[
    stack(&[
        Button::new("origin", "ORIGIN :=", "ORIGIN :=", Icon::Blank)
            .field(Value::Origin, Menu::Items(ORIGIN_MENU)),
        Button::new("tol", "TOL :=", "TOL :=", Icon::Blank)
            .field(Value::Tol, Menu::Items(TOL_MENU)),
        Button::new("ctol", "CTOL :=", "CTOL :=", Icon::Blank)
            .field(Value::Ctol, Menu::Items(CTOL_MENU)),
    ]),
    large(
        Button::new(
            "options",
            "Calculation options",
            "Berechnungsoptionen",
            Icon::Options,
        )
        .menu(Menu::Items(OPTIONS_MENU))
        .below(),
    ),
    large(
        Button::new("files", "File access", "Dateizugriff", Icon::Folder).act(Action::FileAccess),
    ),
];

pub static TAB: TabSpec = TabSpec {
    key: "tab.calculation",
    groups: &[
        Group::new("Controls", "Steuerelemente", CONTROLS),
        Group::new("Error trace", "Fehlerrückverfolgung", TRACE),
        Group::new("Warnings", "Warnungen", WARNINGS),
        Group::new("Worksheet settings", "Arbeitsblatteinstellungen", SETTINGS),
    ],
};

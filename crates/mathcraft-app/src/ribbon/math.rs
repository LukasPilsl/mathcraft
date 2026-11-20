//! The Rechnen tab (screenshots 0003, 0025-0039): regions, operator and symbol palettes, style,
//! units and clipboard.

use crate::dialogs::Dialog;
use crate::edit::math::Template;
use crate::ribbon::spec::{
    large, stack, Button, Cell, Col, Group, Item, Menu, Rule, Section, TabSpec, L,
};
use crate::ribbon::Action;
use crate::ribbon_icons::Icon;

const CHART_MENU: &[Item] = &[
    Item::missing("Insert chart component", "Diagrammkomponente einfügen"),
    Item::missing("Insert X-axis expression", "X-Achsen-Ausdruck einfügen"),
    Item::missing("Insert Y-axis expression", "Y-Achsen-Ausdruck einfügen"),
];

const OPERATORS: &[Section] = &[
    Section {
        title: L("Algebra", "Algebra"),
        cols: 5,
        cells: &[
            Cell::text("+"),
            Cell::typed("−", "-"),
            Cell::typed("·", "*"),
            Cell::with("÷", Action::Template(Template::Fraction)),
            Cell::text("/"),
            Cell::with("xʸ", Action::Template(Template::Power)),
            Cell::with("√x", Action::Template(Template::SquareRoot)),
            Cell::with("|x|", Action::Template(Template::Abs)),
            Cell::with("x!", Action::Template(Template::Factorial)),
            Cell::with("ⁿ√x", Action::Template(Template::NthRoot)),
            Cell::text("%"),
            Cell::typed("()", "("),
            Cell::text(","),
        ],
    },
    Section {
        title: L("Equation wrap", "Gleichungsumbruch"),
        cols: 5,
        cells: &[
            Cell::missing("+↵"),
            Cell::missing("−↵"),
            Cell::missing("·↵"),
            Cell::missing("÷↵"),
        ],
    },
    Section {
        title: L("Calculus", "Analysis"),
        cols: 5,
        cells: &[
            Cell::missing("⊛"),
            Cell::typed("*", "*"),
            Cell::with("d/dx", Action::Template(Template::Derivative)),
            Cell::with("∇", Action::Template(Template::Gradient)),
            Cell::with("∫dx", Action::Template(Template::Integral)),
            Cell::with("lim", Action::Template(Template::Limit)),
            Cell::missing("="),
            Cell::with("∂/∂x", Action::Template(Template::Partial)),
            Cell::typed("x′", "'").tip(
                "Prime: f′(x), the derivative function",
                "Strich: f′(x), die Ableitungsfunktion",
            ),
            Cell::with("∏", Action::Template(Template::Product)),
            Cell::with("∑", Action::Template(Template::Sum)),
            // Mathcraft's own cells: the nth derivative and nth partial derivative with an order slot.
            Cell::with("dⁿ/dxⁿ", Action::Template(Template::NthDerivative)),
            Cell::with("∂ⁿ/∂xⁿ", Action::Template(Template::NthPartial)),
        ],
    },
    Section {
        title: L("Definition and evaluation", "Definition und Auswertung"),
        cols: 5,
        cells: &[
            Cell::with(":=", Action::Template(Template::Define)),
            Cell::with("=", Action::Template(Template::Evaluate)),
            Cell::text("≡"),
            Cell::with("→", Action::Template(Template::Symbolic)),
        ],
    },
    Section {
        title: L("Engineering", "Ingenieurtechnisch"),
        cols: 5,
        cells: &[
            Cell::missing("z̄"),
            Cell::text("°"),
            Cell::missing("∠"),
            Cell::missing("∘"),
        ],
    },
    Section {
        title: L("Vector and matrix", "Vektor und Matrix"),
        cols: 5,
        cells: &[
            Cell::with("×", Action::Template(Template::Cross)),
            Cell::with("‖x‖", Action::Template(Template::Norm)),
            Cell::missing("[‖]"),
            Cell::missing("M⟨⟩"),
            Cell::typed("Mᵢ", "["),
            Cell::missing("M⌒"),
            Cell::typed("Mᵀ", "^T"),
            Cell::with("1..n", Action::Template(Template::Range)),
            Cell::missing("1,3..n"),
            Cell::missing("V⃗"),
        ],
    },
    Section {
        title: L("Comparison", "Vergleich"),
        cols: 5,
        cells: &[
            Cell::missing("∈"),
            Cell::typed("=", "=="),
            Cell::text("⊕"),
            Cell::text(">"),
            Cell::typed("≥", ">="),
            Cell::text("<"),
            Cell::typed("≤", "<="),
            Cell::text("∧"),
            Cell::text("¬"),
            Cell::text("∨"),
            Cell::typed("≠", "<>"),
        ],
    },
];

const SYMBOLS: &[Section] = &[
    Section {
        title: L("Greek capital letters", "Griechischer Großbuchstabe"),
        cols: 8,
        cells: &[
            Cell::text("Α"),
            Cell::text("Β"),
            Cell::text("Γ"),
            Cell::text("Δ"),
            Cell::text("Ε"),
            Cell::text("Ζ"),
            Cell::text("Η"),
            Cell::text("Θ"),
            Cell::text("Ι"),
            Cell::text("Κ"),
            Cell::text("Λ"),
            Cell::text("Μ"),
            Cell::text("Ν"),
            Cell::text("Ξ"),
            Cell::text("Ο"),
            Cell::text("Π"),
            Cell::text("Ρ"),
            Cell::text("Σ"),
            Cell::text("Τ"),
            Cell::text("Υ"),
            Cell::text("Φ"),
            Cell::text("Χ"),
            Cell::text("Ψ"),
            Cell::text("Ω"),
        ],
    },
    Section {
        title: L("Greek small letters", "Griechischer Kleinbuchstabe"),
        cols: 8,
        cells: &[
            Cell::text("α"),
            Cell::text("β"),
            Cell::text("γ"),
            Cell::text("δ"),
            Cell::text("ε"),
            Cell::text("ζ"),
            Cell::text("η"),
            Cell::text("θ"),
            Cell::text("ι"),
            Cell::text("κ"),
            Cell::text("λ"),
            Cell::text("μ"),
            Cell::text("ν"),
            Cell::text("ξ"),
            Cell::text("ο"),
            Cell::text("π"),
            Cell::text("ρ"),
            Cell::text("σ"),
            Cell::text("τ"),
            Cell::text("υ"),
            Cell::text("φ"),
            Cell::text("χ"),
            Cell::text("ψ"),
            Cell::text("ω"),
        ],
    },
    Section {
        title: L("Math symbols", "Rechensymbole"),
        cols: 8,
        cells: &[
            Cell::missing("∠"),
            Cell::missing("∀"),
            Cell::missing("∃"),
            Cell::missing("∩"),
            Cell::missing("∪"),
            Cell::text("∞"),
            Cell::text("ℂ"),
            Cell::text("ℕ"),
            Cell::text("ℚ"),
            Cell::text("ℝ"),
            Cell::text("ℤ"),
        ],
    },
    Section {
        title: L("Currency symbols", "Geldsymbole"),
        cols: 8,
        cells: &[
            Cell::missing("¢"),
            Cell::missing("£"),
            Cell::missing("¤"),
            Cell::missing("¥"),
            Cell::missing("€"),
        ],
    },
    Section {
        title: L("Hebrew", "Hebräisch"),
        cols: 8,
        cells: &[
            Cell::text("ℵ"),
            Cell::text("ℶ"),
            Cell::text("ℷ"),
            Cell::text("ℸ"),
        ],
    },
    Section {
        title: L("Tags", "Tags"),
        cols: 8,
        cells: &[
            Cell::missing("©"),
            Cell::missing("®"),
            Cell::missing("†"),
            Cell::missing("‡"),
        ],
    },
];

const PROGRAMMING: &[Section] = &[Section {
    title: L("Programming", "Programmierung"),
    cols: 4,
    cells: &[
        Cell::missing("‖"),
        Cell::missing("←"),
        Cell::missing("if"),
        Cell::missing("else"),
        Cell::missing("else if"),
        Cell::missing("also if"),
        Cell::missing("while"),
        Cell::missing("for"),
        Cell::missing("break"),
        Cell::missing("continue"),
        Cell::missing("return"),
        Cell::missing("try"),
    ],
}];

const CONSTANTS: &[Section] = &[
    Section {
        title: L("Physics", "Physik"),
        cols: 5,
        cells: &[
            Cell::missing("c"),
            Cell::missing("e.c"),
            Cell::missing("h"),
            Cell::missing("ℏ"),
            Cell::missing("k"),
            Cell::missing("m.u"),
            Cell::missing("N.A"),
            Cell::missing("R"),
            Cell::missing("R∞"),
            Cell::missing("α"),
            Cell::missing("γ"),
            Cell::missing("ε₀"),
            Cell::missing("μ₀"),
            Cell::missing("σ"),
            Cell::missing("Φ₀"),
        ],
    },
    Section {
        title: L("Math", "Rechnen"),
        cols: 5,
        cells: &[Cell::text("∞"), Cell::text("e"), Cell::text("π")],
    },
];

const SYMBOLIC: &[Section] = &[
    Section {
        title: L("Keywords", "Schlüsselwörter"),
        cols: 6,
        cells: &[
            Cell::text("assume"),
            Cell::text("cauchy"),
            Cell::text("coeffs"),
            Cell::text("collect"),
            Cell::text("combine"),
            Cell::text("confrac"),
            Cell::text("decomp"),
            Cell::text("expand"),
            Cell::text("explicit"),
            Cell::text("factor"),
            Cell::text("float"),
            Cell::text("fourier"),
            Cell::text("fully"),
            Cell::text("invfourier"),
            Cell::text("invlaplace"),
            Cell::text("invztrans"),
            Cell::text("laplace"),
            Cell::text("parfrac"),
            Cell::text("rectangular"),
            Cell::text("rewrite"),
            Cell::text("series"),
            Cell::text("simplify"),
            Cell::text("solve"),
            Cell::text("standard"),
            Cell::text("substitute"),
            Cell::text("using"),
            Cell::text("ztrans"),
        ],
    },
    Section {
        title: L("Modifiers", "Modifikatoren"),
        cols: 6,
        cells: &[
            Cell::text("ALL"),
            Cell::text("acos"),
            Cell::text("acot"),
            Cell::text("arg"),
            Cell::text("asin"),
            Cell::text("atan"),
            Cell::text("atan2"),
            Cell::text("cosh"),
            Cell::text("coth"),
            Cell::text("tan"),
        ],
    },
];

const LABEL_STYLES: &[Item] = &[Item::missing("Label styles", "Beschriftungsstile")];
const STYLE_MENU: &[Item] = &[
    Item::sub("Labels (-)", "Beschriftungen (-)", LABEL_STYLES),
    Item::missing("Subscript", "Tiefgestellt"),
    Item::missing("Equation wrap", "Gleichungsumbruch"),
];
// The Einheiten dropdown (0175): the unit palette, the Einheitensystem window and Basiseinheiten.
// Prime's Basiseinheiten opens no window (0186) and its effect is not documented (UNVERIFIED).
const UNITS_MENU: &[Item] = &[
    Item::act(
        "Units",
        "Einheiten",
        Rule::Doc,
        Action::Dialog(Dialog::Unit),
    ),
    Item::act(
        "Unit system:",
        "Einheitensystem:",
        Rule::Doc,
        Action::UnitSystem,
    ),
    Item::missing("Base units", "Basiseinheiten").tip(
        "UNVERIFIED: Prime opens no window and the effect is not documented.",
        "UNVERIFIED: Prime öffnet kein Fenster, die Wirkung ist nicht dokumentiert.",
    ),
];
const CLIPBOARD_MENU: &[Item] = &[
    Item::act("Cut", "Ausschneiden", Rule::Selection, Action::Cut),
    Item::act("Copy", "Kopieren", Rule::Selection, Action::Copy),
    Item::missing("Paste", "Einfügen"),
];

const REGIONS: &[Col] = &[
    large(Button::new("calc", "Math", "Rechnen", Icon::Text("x+y")).act(Action::InsertMath)),
    large(
        Button::new("solve", "Solve block", "Lösungsblock", Icon::Text("}"))
            .act(Action::InsertSolveBlock)
            .rule(Rule::NoRegion)
            .tip(
                "Inserts a Given/Find skeleton (guess, constraint, find) to edit.",
                "Fügt ein Vorgabe/Suchen-Gerüst ein (Schätzwert, Bedingung, find).",
            ),
    ),
    large(
        Button::new(
            "chart",
            "Chart component",
            "Diagrammkomponente",
            Icon::Plot2d,
        )
        .split(Menu::Items(CHART_MENU)),
    ),
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
    large(
        Button::new(
            "delete",
            "Delete region",
            "Bereich löschen",
            Icon::Text("×"),
        )
        .act(Action::DeleteSelected)
        .rule(Rule::Selection),
    ),
];

const PALETTES: &[Col] = &[
    large(
        Button::new("operators", "Operators", "Operatoren", Icon::Text("√"))
            .menu(Menu::Palette(OPERATORS)),
    ),
    large(
        Button::new("symbols", "Symbols", "Symbole", Icon::Text("β")).menu(Menu::Palette(SYMBOLS)),
    ),
    large(
        Button::new(
            "programming",
            "Programming",
            "Programmierung",
            Icon::Text("if"),
        )
        .menu(Menu::Palette(PROGRAMMING)),
    ),
    large(
        Button::new("constants", "Constants", "Konstanten", Icon::Text("π"))
            .menu(Menu::Palette(CONSTANTS)),
    ),
    large(
        Button::new(
            "symbolic",
            "Symbolic math",
            "Symbolische Mathematik",
            Icon::Arrow,
        )
        .menu(Menu::Palette(SYMBOLIC)),
    ),
];

const STYLE: &[Col] = &[large(
    Button::new("style", "Style", "Stil", Icon::Text("m"))
        .menu(Menu::Items(STYLE_MENU))
        .below(),
)];
const UNITS: &[Col] = &[large(
    Button::new("units", "Units", "Einheiten", Icon::Unit)
        .menu(Menu::Items(UNITS_MENU))
        .below(),
)];
const CLIPBOARD: &[Col] = &[large(
    Button::new("clipboard", "Clipboard", "Zwischenablage", Icon::Folder)
        .menu(Menu::Items(CLIPBOARD_MENU))
        .below(),
)];

pub static TAB: TabSpec = TabSpec {
    key: "tab.math",
    groups: &[
        Group::new("Regions", "Bereiche", REGIONS),
        Group::new("Operators and symbols", "Operatoren und Symbole", PALETTES),
        Group::new("", "", STYLE),
        Group::new("", "", UNITS),
        Group::new("", "", CLIPBOARD),
    ],
};

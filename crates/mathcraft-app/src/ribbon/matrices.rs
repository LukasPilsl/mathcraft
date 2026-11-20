//! The Matrizen/Tabellen tab (screenshots 0018, 0097-0114). Matrix einfügen opens Prime's size grid,
//! the operator palette and the function list insert into the math draft, the row and column buttons
//! work on the matrix under the caret, and the two result flags toggle on the selected math region.
//! Insert table and Delete cells are greyed: Mathcraft has no table region and no cell selection.

use crate::edit::math::{MatrixOp, Template};
use crate::ribbon::spec::{
    large, stack, Button, Cell, Col, Flag, Group, Menu, Rule, Section, TabSpec, L,
};
use crate::ribbon::Action;
use crate::ribbon_icons::Icon;

/// Prime's Vektor-/Matrixoperatoren palette (0101): only the cells Mathcraft's parser can read are
/// active.
const OPERATORS: &[Section] = &[Section {
    title: L(
        "Vector and matrix operators",
        "Vektor- und Matrixoperatoren",
    ),
    cols: 4,
    cells: &[
        Cell::typed("Mᵢ", "[").tip("Matrix index", "Matrixindex"),
        Cell::missing("M⟨⟩").tip("Matrix column", "Matrixspalte"),
        Cell::missing("M⌢").tip("Row of a matrix", "Matrixzeile"),
        Cell::with("‖x‖", Action::Template(Template::Norm)).tip("Norm", "Norm"),
        Cell::typed("Mᵀ", "^T").tip("Transpose", "Transponierte"),
        Cell::missing("V⃗").tip("Vectorize", "Vektorisieren"),
        Cell::with("×", Action::Template(Template::Cross)).tip("Cross product", "Kreuzprodukt"),
    ],
}];

/// Prime's Vektor-/Matrixfunktionen list (0102), in its order.
const FUNCTIONS: &[&str] = &[
    "abs",
    "augment",
    "Cholesky",
    "cols",
    "cond1",
    "cond2",
    "conde",
    "condi",
    "convolve",
    "crosscorr",
    "deconvolve",
    "det",
    "diag",
    "eigenvals",
    "eigenvec",
    "eigenvecs",
    "geninv",
    "genvals",
    "genvecs",
    "hlookup",
    "identity",
    "IsArray",
    "IsScalar",
    "kronecker",
    "last",
    "length",
    "logpts",
    "logspace",
    "lookup",
    "lsolve",
    "LU",
    "match",
    "matrix",
    "max",
    "min",
    "norm",
    "norm1",
    "norm2",
    "norme",
    "normi",
    "QR",
    "rank",
    "Rank",
    "rows",
    "rref",
    "stack",
    "submatrix",
    "svd",
    "svds",
    "tr",
    "vec",
    "vhlookup",
    "vlookup",
];

const INSERT: &[Col] = &[
    large(
        Button::new("matrix", "Insert matrix", "Matrix einfügen", Icon::Matrix)
            .menu(Menu::SizeGrid(L("Insert matrix", "Matrix einfügen"))),
    ),
    large(
        Button::new("table", "Insert table", "Tabelle einfügen", Icon::Table).tip(
            "Mathcraft has no table region yet.",
            "Mathcraft kennt noch keine Tabellenregion.",
        ),
    ),
    large(
        Button::new(
            "vm_ops",
            "Vector/matrix operators",
            "Vektor-/Matrixoperatoren",
            Icon::Matrix2x2,
        )
        .menu(Menu::Palette(OPERATORS)),
    ),
    large(
        Button::new(
            "vm_funcs",
            "Vector/matrix functions",
            "Vektor-/Matrixfunktionen",
            Icon::Function,
        )
        .menu(Menu::Functions(FUNCTIONS)),
    ),
];

const ROWS: &[Col] = &[
    stack(&[
        Button::new("above", "Insert above", "Oberhalb einfügen", Icon::Plus)
            .act(Action::MatrixOp(MatrixOp::InsertRowAbove))
            .rule(Rule::MathDraft),
        Button::new("below", "Insert below", "Unterhalb einfügen", Icon::Plus)
            .act(Action::MatrixOp(MatrixOp::InsertRow))
            .rule(Rule::MathDraft),
        Button::new("delrow", "Delete row", "Zeile löschen", Icon::Minus)
            .act(Action::MatrixOp(MatrixOp::DeleteRow))
            .rule(Rule::MathDraft),
    ]),
    stack(&[
        Button::new("left", "Insert left", "Links einfügen", Icon::Plus)
            .act(Action::MatrixOp(MatrixOp::InsertColumnLeft))
            .rule(Rule::MathDraft),
        Button::new("right", "Insert right", "Rechts einfügen", Icon::Plus)
            .act(Action::MatrixOp(MatrixOp::InsertColumn))
            .rule(Rule::MathDraft),
        Button::new("delcol", "Delete column", "Spalte löschen", Icon::Minus)
            .act(Action::MatrixOp(MatrixOp::DeleteColumn))
            .rule(Rule::MathDraft),
    ]),
    stack(&[
        Button::new("delcells", "Delete cells", "Zellen löschen", Icon::Minus).tip(
            "Mathcraft has no cell selection yet.",
            "Mathcraft kennt noch keine Zellenauswahl.",
        ),
    ]),
];

const FORMAT: &[Col] = &[stack(&[
    Button::new("indices", "Show indices", "Indizes anzeigen", Icon::Blank)
        .check(Flag::MatrixIndices)
        .act(Action::ToggleMatrixIndices)
        .rule(Rule::MatrixTarget),
    Button::new(
        "nested",
        "Shrink nested matrices",
        "Verschachtelte Matrizen verkleinern",
        Icon::Blank,
    )
    .check(Flag::MatrixNested)
    .act(Action::ToggleMatrixNested)
    .rule(Rule::MatrixTarget),
])];

pub static TAB: TabSpec = TabSpec {
    key: "tab.matrices",
    groups: &[
        Group::new("Matrices and tables", "Matrizen und Tabellen", INSERT),
        Group::new("Rows and columns", "Zeilen und Spalten", ROWS),
        Group::new("Result format", "Ergebnisformat", FORMAT),
    ],
};

//! The Diagramme tab (screenshots 0019, 0115-0125, 0226-0234). Insert chart is greyed while a
//! region is being edited (0115, 0224); the other buttons need a selected plot (0226). The trace,
//! marker, style and axis buttons change the plot model as one undo step. Style changes apply to
//! the last trace (Mathcraft cannot select a single trace yet). Not available: the chart types
//! Mathcraft cannot draw, Format chart values, Number of points, the contour and 3D charts.

use crate::edit::plot::{PlotOp, StyleChange};
use crate::ribbon::spec::{large, stack, Button, Col, Group, Item, Menu, Rule, TabSpec};
use crate::ribbon::Action;
use crate::ribbon_icons::Icon;

const INSERT_MENU: &[Item] = &[
    Item::act(
        "x-y chart",
        "x-y-Diagramm",
        Rule::NoDraft,
        Action::InsertPlot(false),
    ),
    Item::act(
        "Polar chart",
        "Kreisdiagramm",
        Rule::NoDraft,
        Action::InsertPlot(true),
    ),
    Item::missing("Contour plot", "Isolinienplot"),
    Item::missing("3D chart", "3D-Diagramm"),
];

const TYPE_MENU: &[Item] = &[
    Item::act(
        "Line chart",
        "Liniendiagramm",
        Rule::Plot,
        Action::PlotOp(PlotOp::SetPolar(false)),
    ),
    Item::act(
        "Polar chart",
        "Kreisdiagramm (polar)",
        Rule::Plot,
        Action::PlotOp(PlotOp::SetPolar(true)),
    ),
    Item::missing("Column chart", "Säulendiagramm"),
    Item::missing("Bar chart", "Balkendiagramm"),
    Item::missing("Stem chart", "Stammdiagramm"),
    Item::missing("Waterfall chart", "Wasserfalldiagramm"),
    Item::missing("Error chart", "Fehlerdiagramm"),
    Item::missing("Box plot", "Boxplot-Diagramm"),
    Item::missing("Impact chart", "Wirkungsdiagramm"),
];

const fn style(en: &'static str, de: &'static str, change: StyleChange) -> Item {
    Item::act(en, de, Rule::Plot, Action::PlotOp(PlotOp::Style(change)))
}

const SYMBOL_MENU: &[Item] = &[
    style("None", "Kein", StyleChange::Symbol("none")),
    style("Circle", "Kreis", StyleChange::Symbol("circle")),
    style("Square", "Quadrat", StyleChange::Symbol("square")),
    style("Diamond", "Raute", StyleChange::Symbol("diamond")),
    style("Triangle", "Dreieck", StyleChange::Symbol("triangle")),
    style("Cross", "Kreuz", StyleChange::Symbol("cross")),
];

const LINE_MENU: &[Item] = &[
    style("Solid", "Durchgezogen", StyleChange::Line("Solid")),
    style("Dashed", "Gestrichelt", StyleChange::Line("Dash")),
    style("Dotted", "Gepunktet", StyleChange::Line("Dot")),
];

/// The colours of Prime's trace palette, in the order of `edit::plot::PALETTE`.
const COLOUR_MENU: &[Item] = &[
    style("Dark blue", "Dunkelblau", StyleChange::Color(0)),
    style("Black", "Schwarz", StyleChange::Color(1)),
    style("Red", "Rot", StyleChange::Color(2)),
    style("Yellow", "Gelb", StyleChange::Color(3)),
    style("Orange", "Orange", StyleChange::Color(4)),
    style("Green", "Grün", StyleChange::Color(5)),
    style("Blue", "Blau", StyleChange::Color(6)),
];

const WEIGHT_MENU: &[Item] = &[
    style("1 pt", "1 pt", StyleChange::Weight(1)),
    style("2 pt", "2 pt", StyleChange::Weight(2)),
    style("4 pt", "4 pt", StyleChange::Weight(4)),
    style("6 pt", "6 pt", StyleChange::Weight(6)),
];

/// The Stile dropdown (0234). Colour scheme, area fill, contour values, perspective and the chart
/// background are for other chart types or not drawn here, so they are greyed.
const STYLE_MENU: &[Item] = &[
    Item::sub("Symbol", "Symbol", SYMBOL_MENU),
    Item::sub("Line style", "Linienstil", LINE_MENU),
    Item::missing("Colour scheme", "Farbschema"),
    Item::sub("Trace colour", "Spurfarbe", COLOUR_MENU),
    Item::sub("Trace weight", "Spurdicke", WEIGHT_MENU),
    Item::missing("Area fill", "Flächenfüllung"),
    Item::missing("Chart background", "Diagrammhintergrund"),
    Item::missing("Contour values", "Isolinienwerte"),
    Item::missing("Perspective", "Perspektive"),
];

const LOG_MENU: &[Item] = &[
    Item::act(
        "x axis",
        "X-Achse",
        Rule::Plot,
        Action::PlotOp(PlotOp::ToggleLog(false)),
    ),
    Item::act(
        "y axis",
        "Y-Achse",
        Rule::Plot,
        Action::PlotOp(PlotOp::ToggleLog(true)),
    ),
];

/// The Achsen dropdown (0123). Grid lines and Axis limits are Mathcraft's additions (Prime sets
/// them in the chart's own panes); the limits open the plot window.
const AXES_MENU: &[Item] = &[
    Item::sub("Logarithmic scale", "Logarithmische Skalierung", LOG_MENU),
    Item::missing("Axes cross at 0,0", "Achsen kreuzen an 0,0"),
    Item::missing("Tick marks", "Teilstriche"),
    Item::missing("Tick values", "Teilstrichwerte"),
    Item::act(
        "Axis expressions and limits...",
        "Achsenausdrücke und Grenzen ...",
        Rule::Plot,
        Action::EditPlot,
    ),
    Item::act(
        "Grid lines",
        "Gitterlinien",
        Rule::Plot,
        Action::PlotOp(PlotOp::ToggleGrid),
    ),
];

const CHART: &[Col] = &[
    large(
        Button::new("insert", "Insert chart", "Diagramm einfügen", Icon::Plot2d)
            .menu(Menu::Items(INSERT_MENU)),
    ),
    large(
        Button::new("type", "Change type", "Typ ändern", Icon::Plot3d)
            .menu(Menu::Items(TYPE_MENU))
            .rule(Rule::Plot),
    ),
    large(
        Button::new("edit", "Edit chart", "Diagramm bearbeiten", Icon::Options)
            .act(Action::EditPlot)
            .rule(Rule::Plot),
    ),
];

const TRACES: &[Col] = &[stack(&[
    Button::new("addtrace", "Add trace", "Spur hinzufügen", Icon::Plus)
        .act(Action::PlotOp(PlotOp::AddTrace))
        .rule(Rule::Plot),
    Button::new("deltrace", "Remove trace", "Spur entfernen", Icon::Minus)
        .act(Action::PlotOp(PlotOp::RemoveTrace))
        .rule(Rule::Plot),
    Button::new(
        "points",
        "Number of points",
        "Anzahl der Punkte",
        Icon::Blank,
    )
    .number("500")
    .rule(Rule::Plot),
])];

const MARKERS: &[Col] = &[stack(&[
    Button::new(
        "vmark",
        "Add vertical marker",
        "Vertikale Markierung hinzufügen",
        Icon::Plus,
    )
    .act(Action::PlotOp(PlotOp::AddMarker(true)))
    .rule(Rule::Plot),
    Button::new(
        "hmark",
        "Add horizontal marker",
        "Horizontale Markierung hinzufügen",
        Icon::Plus,
    )
    .act(Action::PlotOp(PlotOp::AddMarker(false)))
    .rule(Rule::Plot),
    Button::new(
        "delmark",
        "Delete marker",
        "Markierung löschen",
        Icon::Minus,
    )
    .act(Action::PlotOp(PlotOp::DeleteMarker))
    .rule(Rule::PlotMarker),
])];

const STYLES: &[Col] = &[
    large(
        Button::new("styles", "Styles", "Stile", Icon::Grid)
            .menu(Menu::Items(STYLE_MENU))
            .below()
            .rule(Rule::Plot),
    ),
    large(
        Button::new("axes", "Axes", "Achsen", Icon::Grid)
            .menu(Menu::Items(AXES_MENU))
            .below()
            .rule(Rule::Plot),
    ),
    large(
        Button::new(
            "fmt",
            "Format chart values",
            "Diagrammwerte formatieren",
            Icon::ResultFormat,
        )
        .rule(Rule::Plot),
    ),
];

pub static TAB: TabSpec = TabSpec {
    key: "tab.plots",
    groups: &[
        Group::new("Chart", "Diagramm", CHART),
        Group::new("Traces", "Spuren", TRACES),
        Group::new("Markers", "Markierungen", MARKERS),
        Group::new("Format", "Format", STYLES),
    ],
};

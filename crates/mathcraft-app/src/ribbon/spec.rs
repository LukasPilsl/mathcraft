//! The declarative description of the ribbon: tabs, groups, buttons, menus and palettes. Nothing
//! here draws (see `render.rs`) and everything is `const`, so a tab is one `static`. How to add a
//! tab or a button is in `documentation/UI_PARITY_RIBBON.md`.

use crate::i18n::{language, Lang};
use crate::ribbon::{Action, Editing, RibbonState};
use crate::ribbon_icons::Icon;

/// A label in both languages: English first, German second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct L(pub &'static str, pub &'static str);

impl L {
    /// The text in the current display language.
    pub fn get(self) -> &'static str {
        if language() == Lang::German && !self.1.is_empty() {
            self.1
        } else {
            self.0
        }
    }

    /// True for the empty label (no tooltip text of its own).
    pub const fn is_empty(self) -> bool {
        self.0.is_empty()
    }
}

/// When a button is enabled, as a function of what is selected (Prime greys buttons the same way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Always (help, links).
    Always,
    /// While a worksheet is open.
    Doc,
    /// A worksheet is open and no region is selected or being edited (Textblock, Textfeld, Bild
    /// are greyed while a math region is selected, screenshot 0061).
    NoRegion,
    /// A worksheet is open and a region is selected or being edited (delete region, cut, copy).
    Selection,
    /// A worksheet is open and one 2D plot is selected.
    Plot,
    /// A worksheet is open and a math region is being edited (matrix row and column commands).
    MathDraft,
    /// A worksheet is open and the selected plot has a marker (Markierung löschen, 0234).
    PlotMarker,
    /// A math region is selected or being edited (the matrix result flags apply to it).
    MatrixTarget,
    /// Insert x-y plot: not while a matrix or a text region is being edited (0115, 0224).
    NoDraft,
    /// A worksheet is open and the calculation mode is MANUAL (Calculate worksheet and Calculate
    /// now are active only then, 0194).
    Manual,
    /// A worksheet is open and at least one region shows an error (the error trace buttons).
    Errors,
    /// A text region is being typed and part of its text is selected (Textformatierung).
    TextSelection,
    /// As `TextSelection`, and the selected text has a hyperlink in it (Link entfernen).
    TextLinked,
    /// The feature does not exist in Mathcraft yet.
    Never,
}

impl Rule {
    /// True when `state` satisfies the rule.
    pub fn holds(self, state: &RibbonState) -> bool {
        match self {
            Rule::Always => true,
            Rule::Doc => state.enabled,
            Rule::NoRegion => {
                state.enabled && state.editing == Editing::None && state.selected == 0
            }
            Rule::Selection => {
                state.enabled && (state.editing != Editing::None || state.selected > 0)
            }
            Rule::Plot => state.enabled && state.plot_selected,
            Rule::PlotMarker => state.enabled && state.plot_selected && state.plot_marker,
            Rule::MatrixTarget => state.enabled && state.matrix_target,
            Rule::MathDraft => state.enabled && state.editing == Editing::Math,
            Rule::NoDraft => state.enabled && state.editing == Editing::None,
            Rule::Manual => state.enabled && state.calc_mode == crate::view_state::CalcMode::Manual,
            Rule::Errors => state.enabled && state.errors > 0,
            Rule::TextSelection => {
                state.enabled && state.editing == Editing::Text && state.text_selected
            }
            Rule::TextLinked => {
                state.enabled
                    && state.editing == Editing::Text
                    && state.text_selected
                    && state.text_linked
            }
            Rule::Never => false,
        }
    }
}

/// The state a toggle, a checkbox or a checked menu line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    None,
    AutoCalc,
    Grid,
    RegionBorders,
    PageLayout,
    /// The draft view (the page layout is off).
    DraftView,
    /// This paper code is the worksheet's.
    Paper(&'static str),
    Landscape,
    Portrait,
    /// This margin type is the worksheet's.
    Margin(&'static str),
    FineGrid,
    StandardGrid,
    /// ORIGIN has this value.
    Origin(u32),
    /// TOL is `10^n`.
    Tol(i32),
    /// CTOL is `10^n`.
    Ctol(i32),
    /// The worksheet's unit system is this one (`si`, `us`, `cgs`).
    Units(&'static str),
    /// The matrix result shows its row and column indices.
    MatrixIndices,
    /// The matrix result shrinks nested matrices.
    MatrixNested,
}

impl Flag {
    /// Whether the flag is on in `state`.
    pub fn on(self, state: &RibbonState) -> bool {
        use crate::ribbon::doc_info::power_of_ten;
        let doc = &state.doc;
        match self {
            Flag::None => false,
            Flag::AutoCalc => state.calc_mode == crate::view_state::CalcMode::Auto,
            Flag::Grid => state.display.grid,
            Flag::RegionBorders => state.display.region_borders,
            Flag::PageLayout => state.display.page_layout,
            Flag::DraftView => !state.display.page_layout,
            Flag::Paper(code) => doc.paper == code,
            Flag::Landscape => doc.landscape,
            Flag::Portrait => !doc.landscape,
            Flag::Margin(m) => doc.margin == m,
            Flag::FineGrid => doc.fine_grid,
            Flag::StandardGrid => !doc.fine_grid,
            Flag::Origin(v) => doc.origin == v,
            Flag::Tol(e) => power_of_ten(doc.tol) == Some(e),
            Flag::Ctol(e) => power_of_ten(doc.ctol) == Some(e),
            Flag::Units(u) => doc.units == u,
            Flag::MatrixIndices => state.show_indices,
            Flag::MatrixNested => state.shrink_nested,
        }
    }
}

/// One cell of a palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub glyph: &'static str,
    pub tip: L,
    /// `None` greys the cell: Mathcraft cannot do it yet.
    pub action: Option<Action>,
}

impl Cell {
    /// A cell that types `text`.
    pub const fn text(glyph: &'static str) -> Cell {
        Cell {
            glyph,
            tip: L(glyph, glyph),
            action: Some(Action::TypeText(glyph)),
        }
    }

    /// A cell that types `text` but is shown as `glyph`.
    pub const fn typed(glyph: &'static str, text: &'static str) -> Cell {
        Cell {
            glyph,
            tip: L(glyph, glyph),
            action: Some(Action::TypeText(text)),
        }
    }

    /// A cell with an action (a template).
    pub const fn with(glyph: &'static str, action: Action) -> Cell {
        Cell {
            glyph,
            tip: L(glyph, glyph),
            action: Some(action),
        }
    }

    /// A cell that is not available yet.
    pub const fn missing(glyph: &'static str) -> Cell {
        Cell {
            glyph,
            tip: L(glyph, glyph),
            action: None,
        }
    }

    /// The same cell with a tooltip.
    pub const fn tip(mut self, en: &'static str, de: &'static str) -> Cell {
        self.tip = L(en, de);
        self
    }
}

/// A titled block of palette cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub title: L,
    pub cols: usize,
    pub cells: &'static [Cell],
}

/// One line of a dropdown menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub label: L,
    pub rule: Rule,
    pub action: Option<Action>,
    /// A submenu (the item then has no action of its own).
    pub sub: Option<&'static [Item]>,
    /// The line shows a check mark while the flag is on.
    pub mark: Flag,
    /// Extra tooltip text (why a line is greyed).
    pub tip: L,
}

impl Item {
    /// An item that is not available yet.
    pub const fn missing(en: &'static str, de: &'static str) -> Item {
        Item {
            label: L(en, de),
            rule: Rule::Doc,
            action: None,
            sub: None,
            mark: Flag::None,
            tip: L("", ""),
        }
    }

    /// An item that runs `action` while `rule` holds.
    pub const fn act(en: &'static str, de: &'static str, rule: Rule, action: Action) -> Item {
        Item {
            label: L(en, de),
            rule,
            action: Some(action),
            sub: None,
            mark: Flag::None,
            tip: L("", ""),
        }
    }

    /// An item that opens a submenu.
    pub const fn sub(en: &'static str, de: &'static str, items: &'static [Item]) -> Item {
        Item {
            label: L(en, de),
            rule: Rule::Doc,
            action: None,
            sub: Some(items),
            mark: Flag::None,
            tip: L("", ""),
        }
    }

    /// The same item with a check mark while `flag` is on.
    pub const fn mark(mut self, flag: Flag) -> Item {
        self.mark = flag;
        self
    }

    /// The same item with a tooltip.
    pub const fn tip(mut self, en: &'static str, de: &'static str) -> Item {
        self.tip = L(en, de);
        self
    }
}

/// What a dropdown opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// A plain list of lines.
    Items(&'static [Item]),
    /// Sections of symbol cells.
    Palette(&'static [Section]),
    /// A list of function names (in Prime's order). Names Mathcraft lacks are greyed.
    Functions(&'static [&'static str]),
    /// Every function of one category of `functions_table.rs`.
    Category(&'static str),
    /// The Textfarbe palette of swatches with "Mehr Farben" (a colour picker). Its colours come
    /// from `text_format::colour_rows`.
    TextColours,
    /// A grid of 12 by 12 cells; the cell under the pointer chooses rows and columns (Prime's
    /// Matrix einfügen, 0098). The text is the title; it shows the size while hovering.
    SizeGrid(L),
}

/// How a button behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A click runs the action.
    Button,
    /// A click on the button runs the action; the arrow opens the menu.
    Split(Menu),
    /// A click opens the menu.
    Dropdown(Menu),
    /// A button that looks pressed while the flag is on.
    Toggle(Flag),
    /// A checkbox.
    Check(Flag),
    /// A number field showing the text (the value is not wired yet unless there is an action).
    Number(&'static str),
    /// A combo box with these entries, the first shown.
    Combo(&'static [&'static str]),
    /// A field that shows a worksheet setting and opens a list to change it (ORIGIN, TOL, CTOL).
    Field(crate::ribbon::doc_info::Value, Menu),
}

/// One ribbon button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    pub id: &'static str,
    pub label: L,
    pub tip: L,
    pub icon: Icon,
    pub kind: Kind,
    pub rule: Rule,
    /// `None`: the command does not exist in Mathcraft yet (greyed, "not available yet").
    pub action: Option<Action>,
    /// Draw the dropdown arrow under the label instead of at its end.
    pub arrow_below: bool,
}

impl Button {
    /// A button without an action (not available yet) that is enabled by an open worksheet.
    pub const fn new(id: &'static str, en: &'static str, de: &'static str, icon: Icon) -> Button {
        Button {
            id,
            label: L(en, de),
            tip: L("", ""),
            icon,
            kind: Kind::Button,
            rule: Rule::Doc,
            action: None,
            arrow_below: false,
        }
    }

    /// The action a click runs.
    pub const fn act(mut self, action: Action) -> Button {
        self.action = Some(action);
        self
    }

    /// When the button is enabled.
    pub const fn rule(mut self, rule: Rule) -> Button {
        self.rule = rule;
        self
    }

    /// A tooltip text beyond the label.
    pub const fn tip(mut self, en: &'static str, de: &'static str) -> Button {
        self.tip = L(en, de);
        self
    }

    /// A dropdown that opens `menu`.
    pub const fn menu(mut self, menu: Menu) -> Button {
        self.kind = Kind::Dropdown(menu);
        self
    }

    /// A split button: the action, and the arrow opens `menu`.
    pub const fn split(mut self, menu: Menu) -> Button {
        self.kind = Kind::Split(menu);
        self
    }

    /// A toggle that shows `flag`.
    pub const fn toggle(mut self, flag: Flag) -> Button {
        self.kind = Kind::Toggle(flag);
        self
    }

    /// A checkbox that shows `flag`.
    pub const fn check(mut self, flag: Flag) -> Button {
        self.kind = Kind::Check(flag);
        self
    }

    /// A number field.
    pub const fn number(mut self, text: &'static str) -> Button {
        self.kind = Kind::Number(text);
        self
    }

    /// A combo box.
    pub const fn combo(mut self, entries: &'static [&'static str]) -> Button {
        self.kind = Kind::Combo(entries);
        self
    }

    /// A field showing `value` of the worksheet; a click opens `menu` to change it.
    pub const fn field(mut self, value: crate::ribbon::doc_info::Value, menu: Menu) -> Button {
        self.kind = Kind::Field(value, menu);
        self
    }

    /// The dropdown arrow goes under the label.
    pub const fn below(mut self) -> Button {
        self.arrow_below = true;
        self
    }

    /// The menu of a dropdown or split button.
    pub const fn menu_of(&self) -> Option<Menu> {
        match self.kind {
            Kind::Split(m) | Kind::Dropdown(m) | Kind::Field(_, m) => Some(m),
            _ => None,
        }
    }

    /// True when the button can do something in Mathcraft (an action or a menu).
    pub fn implemented(&self) -> bool {
        if self.rule == Rule::Never {
            return false;
        }
        match self.kind {
            Kind::Split(_) | Kind::Dropdown(_) | Kind::Field(..) => true,
            _ => self.action.is_some(),
        }
    }

    /// What the button is in `state`.
    pub fn status(&self, state: &RibbonState) -> Status {
        if !self.implemented() {
            Status::NotAvailable
        } else if !self.rule.holds(state) {
            Status::Disabled
        } else {
            Status::Ready
        }
    }
}

/// Whether a button can be pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ready,
    /// Not now (no worksheet, wrong selection).
    Disabled,
    /// Never, until the feature is built: the tooltip says so.
    NotAvailable,
}

/// A column of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    /// A large button: icon above a label of one or two lines.
    Large(Button),
    /// Up to three small buttons stacked (icon, label to the right).
    Stack(&'static [Button]),
}

/// A captioned group of columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub caption: L,
    pub cols: &'static [Col],
    /// A group that is always a single dropdown button (Prime's Steuerelemente, Integration):
    /// this is its icon, and its content opens as a panel.
    pub collapsed: Option<Icon>,
}

impl Group {
    pub const fn new(en: &'static str, de: &'static str, cols: &'static [Col]) -> Group {
        Group {
            caption: L(en, de),
            cols,
            collapsed: None,
        }
    }

    /// The group as one dropdown button with this icon.
    pub const fn collapsed(mut self, icon: Icon) -> Group {
        self.collapsed = Some(icon);
        self
    }

    /// Every button of the group.
    pub fn buttons(&self) -> impl Iterator<Item = &'static Button> {
        let cols: &'static [Col] = self.cols;
        cols.iter().flat_map(|col| {
            let buttons: &'static [Button] = match col {
                Col::Large(b) => std::slice::from_ref(b),
                Col::Stack(bs) => bs,
            };
            buttons.iter()
        })
    }
}

/// One ribbon tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabSpec {
    /// The key of the tab name in `i18n.rs`.
    pub key: &'static str,
    pub groups: &'static [Group],
}

/// The ten tabs of Prime, in order.
pub fn tabs() -> [&'static TabSpec; 10] {
    use crate::ribbon::{
        calculation, document, functions, io, math, math_format, matrices, plots, resources,
        text_format,
    };
    [
        &math::TAB,
        &io::TAB,
        &functions::TAB,
        &matrices::TAB,
        &plots::TAB,
        &math_format::TAB,
        &text_format::TAB,
        &calculation::TAB,
        &document::TAB,
        &resources::TAB,
    ]
}

/// Large button shorthand for the tab modules.
pub const fn large(b: Button) -> Col {
    Col::Large(b)
}

/// Stack of small buttons shorthand for the tab modules.
pub const fn stack(bs: &'static [Button]) -> Col {
    Col::Stack(bs)
}

/// A function category entry for the dropdown of the Functions tab.
pub fn function_names(menu: Menu) -> Vec<&'static str> {
    match menu {
        Menu::Functions(names) => names.to_vec(),
        Menu::Category(category) => {
            let mut names: Vec<&'static str> = crate::functions_table::FUNCTIONS
                .iter()
                .filter(|f| f.category == category)
                .map(|f| f.name)
                .collect();
            names.sort_unstable_by_key(|n| n.to_lowercase());
            names
        }
        _ => Vec::new(),
    }
}

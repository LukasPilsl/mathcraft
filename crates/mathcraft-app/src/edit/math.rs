//! Math region editor: a cursor over a two-dimensional expression tree.
//!
//! The tree is made of rows. A row is a list of items; an item is a character or a structure
//! (a bracket group, a square root, a fraction, a power, an array index). A structure holds one
//! or two sub-rows, called slots. The cursor is a path of (item, slot) steps from the root row,
//! plus a position inside the innermost row. Keys insert, move, or remove at the cursor.
//!
//! The tree is turned into Mathcad text (`to_text`) and the text is read by the core parser
//! (`parse_math`). The parser gives the `Expr`, so a typed expression means exactly what the same
//! text means in a worksheet loaded from a file. Parentheses are added only where the text needs
//! them (a fraction part or a power base with an operator in it); the parser drops parentheses
//! around operator expressions, so the `Expr` does not change.
//!
//! Limitations of this stage (see also the notes in `session.rs`):
//! - An existing region is opened as flat characters of its printed text. Its structure
//!   (fractions, powers) is not rebuilt from the `Expr`; the text is parsed again on commit.
//! - Typing a letter directly after a number inserts a space, so `2x` is `2 x` (product).
//!   The exception is an `e` between digits and a following digit, or a sign and a digit:
//!   `2e3` and `1e-5` are written without a space, so they read as the exponent literals the
//!   parser keeps. A `2 e` with no digit after it is still written `2 e`.
//! - `|` (absolute value) and `'` are plain characters; no bar structure is built.
//! - Operators are not checked while typing. An incomplete or invalid expression is kept in the
//!   draft and reported when it is committed.
//! - Only `^`, `/`, `[`, `(` and `sqrt` build structures. The matrix template inserts its text
//!   (`[■, ■; ■, ■]`) as plain characters, so its cells are typed as text.
//! - Summation, product, integral and derivative templates type the calculus functions
//!   (`sum(i, m, n, x)`, `integral(x, a, b, f)`, ...). The parser reads them as the
//!   `Expr::Accumulate`, `Expr::Integral` and `Expr::Derivative` nodes, so the sign is drawn as
//!   text here, not as the operator. Their placeholders must be filled before the region commits.

use crate::catalog::MAX_MATRIX_SIDE;
#[cfg(test)]
use mathcraft_core::expr::Expr;
#[cfg(test)]
use mathcraft_core::parser::ParseError;

/// The placeholder that marks an empty slot. The parser reads it as `Expr::Placeholder`.
pub const PLACEHOLDER: char = '■';

/// Marks the caret inside the rendered text. Never part of the text that is parsed.
const MARK: char = '\u{E000}';
/// Marks the start and the end of the selection in the rendered text (display only).
const SEL_START: char = '\u{E001}';
const SEL_END: char = '\u{E002}';

/// Whether `c` is one of the display markers, which are not text.
fn is_marker(c: char) -> bool {
    matches!(c, MARK | SEL_START | SEL_END)
}

/// The templates the Math ribbon inserts. Each one is typed through the same keys as a user would
/// type (see [`MathEditor::apply_template`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// `num/den`, the operand before the caret is the numerator.
    Fraction,
    /// `base^exp`, the operand before the caret is the base.
    Power,
    /// `sqrt(radicand)`.
    SquareRoot,
    /// `_`, for a name with a subscript such as `v_0`.
    Subscript,
    /// `:=`, a definition.
    Define,
    /// `=`, an evaluation.
    Evaluate,
    /// `->`, a symbolic evaluation.
    Symbolic,
    /// A 2x2 matrix with placeholders.
    Matrix2x2,
    /// `sum(i, m, n, x)`: Mathcad's summation over a range variable.
    Sum,
    /// `product(i, m, n, x)`: Mathcad's product over a range variable.
    Product,
    /// `integral(x, a, b, f)`: a definite integral.
    Integral,
    /// `derivative(x, f)`: the first derivative.
    Derivative,
    /// `d^n/dx^n f`: the nth derivative.
    NthDerivative,
    /// `partial(x, f)`: the partial derivative `∂/∂x f` (Prime's Ctrl+D).
    Partial,
    /// `∂^n/∂x^n f`: the nth partial derivative.
    NthPartial,
    /// `gradient(f, x, y)`: `∇ f` (Prime's Ctrl+Shift+G).
    Gradient,
    /// `‖x‖`: the norm of a vector or matrix.
    Norm,
    /// `×`: the cross product of two vectors (Prime's Ctrl+8).
    Cross,
    /// `root(x, n)`: the nth root.
    NthRoot,
    /// `|x|`.
    Abs,
    /// `x!`.
    Factorial,
    /// `a..b`.
    Range,
    /// `limit(x, a, f)`: the limit of `f` as `x` approaches `a` (Prime's Ctrl+L).
    Limit,
}

/// Operators typed as several characters: deleted as one token.
/// The symbolic evaluation arrow as one atom: typed `->`, written `->` in the text form.
const ARROW: char = '→';

/// The keywords of the Symbolic math dropdown (see the ribbon).
const SYMBOLIC_KEYWORDS: [&str; 27] = [
    "assume",
    "cauchy",
    "coeffs",
    "collect",
    "combine",
    "confrac",
    "decomp",
    "expand",
    "explicit",
    "factor",
    "float",
    "fourier",
    "fully",
    "invfourier",
    "invlaplace",
    "invztrans",
    "laplace",
    "parfrac",
    "rectangular",
    "rewrite",
    "series",
    "simplify",
    "solve",
    "standard",
    "substitute",
    "using",
    "ztrans",
];

const MULTI_OPS: [&str; 7] = [":=", "<=", ">=", "<>", "!=", "->", "=="];

fn atoms_equal(items: &[Item], text: &str) -> bool {
    items.len() == text.chars().count()
        && items
            .iter()
            .zip(text.chars())
            .all(|(item, c)| matches!(item, Item::Atom(a) if *a == c))
}

mod structure;

/// A list of items: one line of an expression, or the inside of a slot.
pub type Row = Vec<Item>;

/// One element of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// One typed character: a letter, digit, operator, space or punctuation.
    Atom(char),
    /// `( inner )`.
    Group(Row),
    /// `sqrt( radicand )`.
    Sqrt(Row),
    /// `num / den`.
    Fraction { num: Row, den: Row },
    /// `base ^ exp`. The base is the operand the `^` was typed after.
    Power { base: Row, exp: Row },
    /// `base [ index ]`. The base is the operand the `[` was typed after.
    Index { base: Row, index: Row },
    /// `|inner|`, the absolute value (or the norm of a vector).
    Abs(Row),
    /// A matrix of `cells.len() / cols` rows, one slot per cell in row-major order. Written as
    /// `[a, b; c, d]`.
    Matrix { cols: usize, cells: Vec<Row> },
    /// A calculus operator with one slot per operand: `d/dx f`, `∫ f dx`, `∑`, `lim`, nth root.
    /// Shown typeset; written as the function call the parser reads (`derivative(x, f)`).
    Op { kind: OpKind, slots: Vec<Row> },
}

/// The operators that are built from slots (see [`Item::Op`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Derivative,
    NthDerivative,
    Partial,
    NthPartial,
    /// Slots: the variables (may stay empty), the function.
    Gradient,
    /// `‖x‖`, one slot. Written with the double bars, not as a call.
    Norm,
    Integral,
    Sum,
    Product,
    Limit,
    NthRoot,
}

/// A piece of an operator's typeset display: fixed text or a slot.
enum Seg {
    Lit(&'static str),
    Slot(usize),
}

impl OpKind {
    fn slot_total(self) -> usize {
        match self {
            OpKind::Norm => 1,
            OpKind::Derivative | OpKind::Partial | OpKind::Gradient | OpKind::NthRoot => 2,
            OpKind::NthDerivative | OpKind::NthPartial | OpKind::Limit => 3,
            OpKind::Integral | OpKind::Sum | OpKind::Product => 4,
        }
    }

    /// The function name the parser reads.
    fn name(self) -> &'static str {
        match self {
            OpKind::Derivative | OpKind::NthDerivative => "derivative",
            OpKind::Partial | OpKind::NthPartial => "partial",
            OpKind::Gradient => "gradient",
            OpKind::Norm => "norm",
            OpKind::Integral => "integral",
            OpKind::Sum => "sum",
            OpKind::Product => "product",
            OpKind::Limit => "limit",
            OpKind::NthRoot => "root",
        }
    }

    /// The slots in the order of the function's arguments. Slots are kept in display order.
    fn arguments(self) -> &'static [usize] {
        match self {
            OpKind::Derivative
            | OpKind::Partial
            | OpKind::Sum
            | OpKind::Product
            | OpKind::Limit
            | OpKind::Norm => &[0, 1, 2, 3],
            // slots: order, variable, body; arguments: variable, order, body
            OpKind::NthDerivative | OpKind::NthPartial => &[1, 0, 2],
            // slots: variables, body; arguments: body, then the variables
            OpKind::Gradient => &[1, 0],
            // slots: lower, upper, body, variable; arguments: variable, lower, upper, body
            OpKind::Integral => &[3, 0, 1, 2],
            // slots: degree, radicand; arguments: radicand, degree
            OpKind::NthRoot => &[1, 0],
        }
    }

    /// The typeset display: operators in the shape Prime shows (`d/dx f`, `∫ₐᵇ f dx`).
    fn layout(self) -> Vec<Seg> {
        use Seg::{Lit, Slot};
        match self {
            OpKind::Derivative => vec![Lit("d/d"), Slot(0), Lit(" "), Slot(1)],
            OpKind::NthDerivative => {
                vec![Lit("dⁿ"), Slot(0), Lit("/d"), Slot(1), Lit("ⁿ "), Slot(2)]
            }
            OpKind::Partial => vec![Lit("∂/∂"), Slot(0), Lit(" "), Slot(1)],
            OpKind::NthPartial => {
                vec![Lit("∂ⁿ"), Slot(0), Lit("/∂"), Slot(1), Lit("ⁿ "), Slot(2)]
            }
            OpKind::Gradient => vec![Lit("∇_"), Slot(0), Lit(" "), Slot(1)],
            OpKind::Norm => vec![Lit("‖"), Slot(0), Lit("‖")],
            OpKind::Integral => vec![
                Lit("∫("),
                Slot(0),
                Lit(" to "),
                Slot(1),
                Lit(") "),
                Slot(2),
                Lit(" d"),
                Slot(3),
            ],
            OpKind::Sum => vec![
                Lit("∑("),
                Slot(0),
                Lit("="),
                Slot(1),
                Lit(" to "),
                Slot(2),
                Lit(") "),
                Slot(3),
            ],
            OpKind::Product => vec![
                Lit("∏("),
                Slot(0),
                Lit("="),
                Slot(1),
                Lit(" to "),
                Slot(2),
                Lit(") "),
                Slot(3),
            ],
            OpKind::Limit => vec![Lit("lim("), Slot(0), Lit("→"), Slot(1), Lit(") "), Slot(2)],
            OpKind::NthRoot => vec![Slot(0), Lit("√("), Slot(1), Lit(")")],
        }
    }
}

impl Item {
    /// Number of sub-rows (slots) of this item.
    fn slot_count(&self) -> usize {
        match self {
            Item::Atom(_) => 0,
            Item::Group(_) | Item::Sqrt(_) | Item::Abs(_) => 1,
            Item::Fraction { .. } | Item::Power { .. } | Item::Index { .. } => 2,
            Item::Matrix { cells, .. } => cells.len(),
            Item::Op { slots, .. } => slots.len(),
        }
    }

    /// Sub-row `slot`, in the order the cursor visits them.
    fn slot(&self, slot: usize) -> Option<&Row> {
        match (self, slot) {
            (Item::Group(r) | Item::Sqrt(r) | Item::Abs(r), 0) => Some(r),
            (Item::Fraction { num, .. }, 0) => Some(num),
            (Item::Fraction { den, .. }, 1) => Some(den),
            (Item::Power { base, .. } | Item::Index { base, .. }, 0) => Some(base),
            (Item::Power { exp, .. }, 1) => Some(exp),
            (Item::Index { index, .. }, 1) => Some(index),
            (Item::Matrix { cells, .. }, i) => cells.get(i),
            (Item::Op { slots, .. }, i) => slots.get(i),
            _ => None,
        }
    }

    fn slot_mut(&mut self, slot: usize) -> Option<&mut Row> {
        match (self, slot) {
            (Item::Group(r) | Item::Sqrt(r) | Item::Abs(r), 0) => Some(r),
            (Item::Fraction { num, .. }, 0) => Some(num),
            (Item::Fraction { den, .. }, 1) => Some(den),
            (Item::Power { base, .. } | Item::Index { base, .. }, 0) => Some(base),
            (Item::Power { exp, .. }, 1) => Some(exp),
            (Item::Index { index, .. }, 1) => Some(index),
            (Item::Matrix { cells, .. }, i) => cells.get_mut(i),
            (Item::Op { slots, .. }, i) => slots.get_mut(i),
            _ => None,
        }
    }
}

/// Follows a cursor path down from `root`. `None` when the path does not fit the tree.
/// A row with nothing typed in it: spaces, and operators whose slots are all blank.
fn row_is_blank(row: &Row) -> bool {
    row.iter().all(|item| match item {
        Item::Atom(c) => c.is_whitespace(),
        Item::Op { slots, .. } => slots.iter().all(row_is_blank),
        _ => false,
    })
}

fn row_at<'a>(root: &'a Row, path: &[(usize, usize)]) -> Option<&'a Row> {
    let mut row = root;
    for &(at, slot) in path {
        row = row.get(at)?.slot(slot)?;
    }
    Some(row)
}

/// The path of every row of the tree, in document order (a row before its slots, slots in
/// order). Used by Tab. Depth is bounded by [`MAX_NESTING`], as the editor builds the tree.
fn slot_paths(root: &Row) -> Vec<Vec<(usize, usize)>> {
    fn walk(row: &Row, path: &mut Vec<(usize, usize)>, out: &mut Vec<Vec<(usize, usize)>>) {
        out.push(path.clone());
        for (at, item) in row.iter().enumerate() {
            for slot in 0..item.slot_count() {
                if let Some(inner) = item.slot(slot) {
                    path.push((at, slot));
                    walk(inner, path, out);
                    path.pop();
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

fn row_at_mut<'a>(root: &'a mut Row, path: &[(usize, usize)]) -> Option<&'a mut Row> {
    let mut row = root;
    for &(at, slot) in path {
        row = row.get_mut(at)?.slot_mut(slot)?;
    }
    Some(row)
}

/// The kind of a character that can be an operand of `^`, `/` or `[`: a name (letters) or a
/// number (digits and the decimal point). Other characters are not operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperandClass {
    Letters,
    Digits,
}

fn operand_class(c: char) -> Option<OperandClass> {
    if c.is_alphabetic() {
        Some(OperandClass::Letters)
    } else if c.is_ascii_digit() || c == '.' {
        Some(OperandClass::Digits)
    } else {
        None
    }
}

/// Removes the operand that ends at `pos` from `row` and returns it with the new position.
/// The operand is a run of characters of one class (`x2` gives `2`), or one structure.
fn take_operand(row: &mut Row, pos: usize) -> (Row, usize) {
    let pos = pos.min(row.len());
    if pos == 0 {
        return (Vec::new(), 0);
    }
    let last_char = match row[pos - 1] {
        Item::Atom(c) => Some(c),
        _ => None,
    };
    match last_char {
        Some(c) => {
            let Some(class) = operand_class(c) else {
                return (Vec::new(), pos);
            };
            let mut start = pos - 1;
            while start > 0 {
                match row[start - 1] {
                    Item::Atom(d) if operand_class(d) == Some(class) => start -= 1,
                    _ => break,
                }
            }
            (row.drain(start..pos).collect(), start)
        }
        None => {
            let item = row.remove(pos - 1);
            (vec![item], pos - 1)
        }
    }
}

/// The start of the term that ends at `pos` in `row`, when the item before `pos` is a letter or a
/// digit or point. A run of one operand class is one term.
fn term_start(row: &Row, pos: usize) -> Option<usize> {
    let Some(Item::Atom(c)) = pos.checked_sub(1).and_then(|at| row.get(at)) else {
        return None;
    };
    let class = operand_class(*c)?;
    let mut start = pos - 1;
    while start > 0 {
        match row[start - 1] {
            Item::Atom(d) if operand_class(d) == Some(class) => start -= 1,
            _ => break,
        }
    }
    Some(start)
}

/// The end of the term that starts at `pos` in `row`: the mirror of [`term_start`].
fn term_end(row: &Row, pos: usize) -> Option<usize> {
    let Some(Item::Atom(c)) = row.get(pos) else {
        return None;
    };
    let class = operand_class(*c)?;
    let mut end = pos + 1;
    while end < row.len() {
        match row[end] {
            Item::Atom(d) if operand_class(d) == Some(class) => end += 1,
            _ => break,
        }
    }
    Some(end)
}

/// Which closing character a key stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closer {
    Group,
    Index,
    Abs,
}

/// Deepest structure nesting the editor builds. The core parser refuses deeper text, so the
/// editor stops at the same depth: a key that would go deeper types its character instead.
pub const MAX_NESTING: usize = 100;

/// Longest text one paste or insert puts into a math region (characters).
pub const MAX_PASTE_CHARS: usize = 4096;

/// A row or column change of a matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixOp {
    /// A row below the caret's row.
    InsertRow,
    /// A row above the caret's row.
    InsertRowAbove,
    /// A column right of the caret's column.
    InsertColumn,
    /// A column left of the caret's column.
    InsertColumnLeft,
    DeleteRow,
    DeleteColumn,
}

/// The state of one math region being typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MathEditor {
    root: Row,
    /// Steps from the root row to the row that holds the caret: (item index, slot).
    path: Vec<(usize, usize)>,
    /// Caret position inside that row, `0..=len`.
    pos: usize,
    /// The other end of the selection, as (path, position) like the caret. `None` when no
    /// selection is made. A selection lies inside one row, the row of the caret.
    anchor: Option<(Vec<(usize, usize)>, usize)>,
    /// German input: a comma after a digit is the decimal point, and `;` is the argument
    /// separator (`,` in the text). English input is unchanged.
    decimal_comma: bool,
}

impl MathEditor {
    /// An empty editor with the caret in the empty root row.
    pub fn new() -> Self {
        Self::default()
    }

    /// The same editor with German number input (`decimal_comma`) when `on` is true.
    #[must_use]
    pub fn with_decimal_comma(mut self, on: bool) -> Self {
        self.decimal_comma = on;
        self
    }

    /// An editor holding the characters of `text`, with the caret at the end. Structure is not
    /// rebuilt from the text (see the module notes).
    pub fn from_text(text: &str) -> Self {
        let flat = Self::from_flat_text(text);
        // Structure (operators, fractions, roots, powers) is rebuilt when it writes the very
        // same text again; otherwise the flat text stays.
        if let Some(root) = structure::rebuild(text) {
            let pos = root.len();
            let rebuilt = Self {
                root,
                path: Vec::new(),
                pos,
                anchor: None,
                decimal_comma: false,
            };
            if rebuilt.to_text() == flat.to_text() {
                return rebuilt;
            }
        }
        flat
    }

    fn from_flat_text(text: &str) -> Self {
        let mut root: Row = Vec::new();
        for c in text.chars().filter(|c| !c.is_control()) {
            if c == '>' && matches!(root.last(), Some(Item::Atom('-'))) {
                root.pop();
                root.push(Item::Atom(ARROW));
            } else {
                root.push(Item::Atom(c));
            }
        }
        let pos = root.len();
        Self {
            root,
            path: Vec::new(),
            pos,
            anchor: None,
            decimal_comma: false,
        }
    }

    /// The item tree with the caret in it, for the typeset draft (`draftlayout.rs`).
    pub fn tree(&self) -> crate::draftlayout::DraftTree {
        let fits = row_at(&self.root, &self.path).is_some_and(|row| self.pos <= row.len());
        let (path, pos) = if fits {
            (self.path.clone(), self.pos)
        } else {
            (Vec::new(), self.root.len())
        };
        crate::draftlayout::DraftTree {
            root: self.root.clone(),
            path,
            pos,
        }
    }

    /// True when nothing has been typed: only spaces, and operator placeholders (derivative,
    /// integral, ...) whose slots are all empty too.
    pub fn is_blank(&self) -> bool {
        row_is_blank(&self.root)
    }

    /// The Mathcad text of the expression. Empty slots are written as the placeholder.
    pub fn to_text(&self) -> String {
        fold_inf(&self.render(false).0)
    }

    /// The text with the caret shown: the text without the caret, and the caret as a character
    /// index into it. Used to draw the draft.
    pub fn display(&self) -> (String, usize) {
        let (text, caret, _) = self.render(true);
        (text, caret)
    }

    /// Like [`Self::display`], and the selected character range `(start, end)` of the text, when a
    /// selection is made.
    pub fn display_selected(&self) -> (String, usize, Option<(usize, usize)>) {
        self.render(true)
    }

    /// The caret slot nearest to character `index` of the rendered text (see `display`): the first
    /// slot at that index, else the last one before it. Returns the row's path and the position.
    fn slot_at(&self, index: usize) -> (Vec<(usize, usize)>, usize) {
        let mut w = Writer::new(None);
        w.typeset = true;
        w.slots = Some(Vec::new());
        emit_row(&self.root, &mut Vec::new(), &mut w);
        let slots = w.slots.unwrap_or_default();
        let found = slots
            .iter()
            .find(|s| s.2 == index)
            .or_else(|| slots.iter().rev().find(|s| s.2 < index))
            .or(slots.first());
        match found {
            Some((path, pos, _)) => (path.clone(), *pos),
            None => (Vec::new(), self.root.len()),
        }
    }

    /// Puts the caret at character `index` of the rendered text: a mouse press in the draft. Clears
    /// the selection.
    pub fn place_caret(&mut self, index: usize) {
        self.sync();
        let (path, pos) = self.slot_at(index);
        self.path = path;
        self.pos = pos;
        self.anchor = None;
    }

    /// Moves the caret to character `index` and extends the selection from where the drag began.
    /// A selection stays in one row: when the caret leaves that row, the selection is not shown.
    pub fn extend_to(&mut self, index: usize) {
        self.sync();
        let before = (self.path.clone(), self.pos);
        let (path, pos) = self.slot_at(index);
        self.anchor.get_or_insert(before);
        self.path = path;
        self.pos = pos;
    }

    /// Selects the word under character `index` (a double click): the run of letters and digits
    /// around it in the same row. Nothing is selected on an operator or a structure.
    pub fn select_word_at(&mut self, index: usize) {
        self.place_caret(index);
        let is_word = |item: &Item| matches!(item, Item::Atom(c) if c.is_alphanumeric());
        let (start, end) = {
            let row = self.row();
            let mut start = self.pos;
            while start > 0 && is_word(&row[start - 1]) {
                start -= 1;
            }
            let mut end = self.pos;
            while end < row.len() && is_word(&row[end]) {
                end += 1;
            }
            (start, end)
        };
        if start < end {
            self.anchor = Some((self.path.clone(), start));
            self.pos = end;
        }
    }

    /// The Mathcad text of the selected items, or `None` when nothing is selected.
    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection_range()?;
        let row = row_at(&self.root, &self.path)?;
        let sub: Row = row.get(a..b)?.to_vec();
        let mut w = Writer::new(None);
        emit_row(&sub, &mut Vec::new(), &mut w);
        Some(w.out)
    }

    /// Removes the selected items and returns their text (cut). `None` when nothing is selected.
    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.take_selection();
        Some(text)
    }

    /// Types one character at the caret. `:` types `:=`. `(`, `[`, `^`, `/`, `|` open a structure
    /// around the operand before the caret (or around the selection). `)`, `]` and `|` leave the
    /// structure they close. A space with a selection expands the selection instead.
    pub fn key(&mut self, c: char) {
        self.sync();
        if c.is_control() {
            return;
        }
        if !is_name_char(c) {
            self.fold_inf_before_caret();
        }
        if c == ' ' && self.selection_range().is_some() {
            self.expand_selection();
            return;
        }
        let structure = matches!(c, '(' | '[' | '^' | '/' | '|' | '\\');
        if self.selection_range().is_some() {
            if structure && self.path.len() < MAX_NESTING {
                if let Some(content) = self.take_selection() {
                    self.wrap_selection(c, content);
                    return;
                }
            }
            self.take_selection();
        }
        let c = self.locale_char(c);
        let deep = structure && self.path.len() >= MAX_NESTING;
        match c {
            ':' => {
                self.put(Item::Atom(':'));
                self.put(Item::Atom('='));
            }
            // Typing `=` straight after `:` (already `:=`) does not give `:==`.
            '=' if self.after_define() => {}
            // `->` becomes the one-token symbolic evaluation arrow.
            '>' if self.dash_before_caret() => {
                self.pos -= 1;
                if let Some(row) = row_at_mut(&mut self.root, &self.path) {
                    row.remove(self.pos);
                }
                self.put(Item::Atom(ARROW));
            }
            '(' if !deep => self.enter(Item::Group(Vec::new()), 0),
            '[' if !deep => self.enter_with_operand(
                |base| Item::Index {
                    base,
                    index: Vec::new(),
                },
                1,
            ),
            '^' if !deep => self.enter_with_operand(
                |base| Item::Power {
                    base,
                    exp: Vec::new(),
                },
                1,
            ),
            '/' if !deep => self.enter_with_operand(
                |num| Item::Fraction {
                    num,
                    den: Vec::new(),
                },
                1,
            ),
            // The backslash is Prime's square root key (help: Square Root and Nth Root Operator).
            '\\' if !deep => self.enter(Item::Sqrt(Vec::new()), 0),
            '|' => {
                if self.leave(Closer::Abs) {
                    // The bar closed the absolute value it was typed in.
                } else if deep {
                    self.put(Item::Atom('|'));
                } else {
                    self.enter(Item::Abs(Vec::new()), 0);
                }
            }
            ')' => {
                if !self.leave(Closer::Group) {
                    self.put(Item::Atom(')'));
                }
            }
            ']' => {
                if !self.leave(Closer::Index) {
                    self.put(Item::Atom(']'));
                }
            }
            _ => self.put(Item::Atom(c)),
        }
    }

    /// The character that is typed for `c` in this editor's input language: in German, `;` is
    /// the argument separator (`,`), and a comma that follows digits of a number is the decimal
    /// point (`.`). Any other character, and every character in English, is unchanged.
    fn locale_char(&self, c: char) -> char {
        if !self.decimal_comma {
            return c;
        }
        match c {
            ';' => ',',
            ',' if self.in_number_before_caret() => '.',
            other => other,
        }
    }

    /// True when the characters before the caret are a number that has no decimal point yet:
    /// at least one digit, not part of a name (`x2`) or of an exponent (`2e3`), and no `.`.
    fn in_number_before_caret(&self) -> bool {
        let row = self.row();
        let mut start = self.pos.min(row.len());
        let end = start;
        while start > 0 && matches!(row.get(start - 1), Some(Item::Atom(c)) if c.is_ascii_digit()) {
            start -= 1;
        }
        if start == end {
            return false;
        }
        match start.checked_sub(1).and_then(|at| row.get(at)) {
            Some(Item::Atom(c)) => !(c.is_alphabetic() || *c == '_' || *c == '.'),
            _ => true,
        }
    }

    /// True when the caret is directly after a `-` atom.
    fn dash_before_caret(&self) -> bool {
        self.pos >= 1 && matches!(self.row().get(self.pos - 1), Some(Item::Atom('-')))
    }

    /// The word `inf` typed just before the caret becomes the infinity sign, as Prime shows an
    /// infinite integral bound. A longer name (`info`, `x_inf`) is left alone.
    fn fold_inf_before_caret(&mut self) {
        let Some(row) = row_at_mut(&mut self.root, &self.path) else {
            return;
        };
        let end = self.pos.min(row.len());
        if end < 3 {
            return;
        }
        let word = matches!(row[end - 3], Item::Atom('i'))
            && matches!(row[end - 2], Item::Atom('n'))
            && matches!(row[end - 1], Item::Atom('f'));
        let glued = end > 3 && matches!(row[end - 4], Item::Atom(c) if is_name_char(c));
        if word && !glued {
            row.drain(end - 2..end);
            row[end - 3] = Item::Atom('∞');
            self.pos = end - 2;
        }
    }

    /// True when the caret is directly after a `:=` pair typed by `:`.
    fn after_define(&self) -> bool {
        let row = self.row();
        self.pos >= 2
            && matches!(row.get(self.pos - 1), Some(Item::Atom('=')))
            && matches!(row.get(self.pos - 2), Some(Item::Atom(':')))
    }

    /// Starts a square root at the caret, with the caret in its radicand.
    pub fn insert_sqrt(&mut self) {
        self.sync();
        self.anchor = None;
        if self.path.len() < MAX_NESTING {
            self.enter(Item::Sqrt(Vec::new()), 0);
        }
    }

    /// Inserts a matrix of `rows` by `cols` empty cells at the caret, with the caret in its first
    /// cell. Sizes are clamped to 1..=[`MAX_MATRIX_SIDE`].
    pub fn insert_matrix(&mut self, rows: usize, cols: usize) {
        self.sync();
        self.anchor = None;
        if self.path.len() >= MAX_NESTING {
            return;
        }
        let rows = rows.clamp(1, MAX_MATRIX_SIDE);
        let cols = cols.clamp(1, MAX_MATRIX_SIDE);
        let cells = (0..rows * cols).map(|_| Row::new()).collect();
        self.enter(Item::Matrix { cols, cells }, 0);
    }

    /// Ctrl+Left: moves the caret to the start of the term before it. A term is a run of letters
    /// or a run of digits and points (`x2` has the terms `x` and `2`). Next to a bracket, root or
    /// other symbol it moves one item, as Left does.
    pub fn term_left(&mut self) {
        self.sync();
        self.anchor = None;
        match term_start(self.row(), self.pos) {
            Some(start) => self.pos = start,
            None => self.left(),
        }
    }

    /// Ctrl+Right: moves the caret to the end of the term after it, or one item as Right does.
    pub fn term_right(&mut self) {
        self.sync();
        self.anchor = None;
        match term_end(self.row(), self.pos) {
            Some(end) => self.pos = end,
            None => self.right(),
        }
    }

    /// Moves the caret one step left. Entering a structure from its right side puts the caret at
    /// the end of its last slot. Clears a selection.
    pub fn left(&mut self) {
        self.sync();
        self.anchor = None;
        if self.pos > 0 {
            let at = self.pos - 1;
            let count = self.row().get(at).map_or(0, Item::slot_count);
            if count == 0 {
                self.pos = at;
            } else {
                let len = self
                    .row()
                    .get(at)
                    .and_then(|item| item.slot(count - 1))
                    .map_or(0, Vec::len);
                self.path.push((at, count - 1));
                self.pos = len;
            }
        } else if let Some(&(at, slot)) = self.path.last() {
            if slot > 0 {
                let len = self
                    .container()
                    .and_then(|item| item.slot(slot - 1))
                    .map_or(0, Vec::len);
                if let Some(last) = self.path.last_mut() {
                    *last = (at, slot - 1);
                }
                self.pos = len;
            } else {
                self.path.pop();
                self.pos = at;
            }
        }
    }

    /// Moves the caret one step right. Entering a structure from its left side puts the caret at
    /// the start of its first slot; leaving the last slot puts it after the structure. Clears a
    /// selection.
    pub fn right(&mut self) {
        self.sync();
        self.anchor = None;
        if self.pos < self.row().len() {
            let at = self.pos;
            let count = self.row().get(at).map_or(0, Item::slot_count);
            if count == 0 {
                self.pos += 1;
            } else {
                self.path.push((at, 0));
                self.pos = 0;
            }
        } else if let Some(&(at, slot)) = self.path.last() {
            let count = self.container().map_or(0, Item::slot_count);
            if slot + 1 < count {
                if let Some(last) = self.path.last_mut() {
                    *last = (at, slot + 1);
                }
                self.pos = 0;
            } else {
                self.path.pop();
                self.pos = at + 1;
            }
        }
    }

    /// Extends the selection one item to the left (Shift+Left). Stays in the caret's row.
    pub fn select_left(&mut self) {
        self.sync();
        if self.pos == 0 {
            return;
        }
        self.start_selection();
        self.pos -= 1;
    }

    /// Extends the selection one item to the right (Shift+Right). Stays in the caret's row.
    pub fn select_right(&mut self) {
        self.sync();
        if self.pos >= self.row().len() {
            return;
        }
        self.start_selection();
        self.pos += 1;
    }

    /// Sets the anchor at the caret unless the selection already starts in this row.
    fn start_selection(&mut self) {
        let same_row = matches!(&self.anchor, Some((p, _)) if *p == self.path);
        if !same_row {
            self.anchor = Some((self.path.clone(), self.pos));
        }
    }

    /// Moves the caret to the start of the expression (Home). Clears a selection.
    pub fn home(&mut self) {
        self.anchor = None;
        self.path.clear();
        self.pos = 0;
    }

    /// Moves the caret to the end of the expression (End). Clears a selection.
    pub fn end(&mut self) {
        self.anchor = None;
        self.path.clear();
        self.pos = self.root.len();
    }

    /// Moves to the same column one row up or down inside the matrix that holds the caret. Does
    /// nothing outside a matrix.
    pub fn vertical(&mut self, down: bool) {
        self.sync();
        self.anchor = None;
        let Some(depth) = self.enclosing_matrix() else {
            return;
        };
        let Some((_, rows, cols)) = self.matrix_dims(depth) else {
            return;
        };
        let slot = self.path[depth].1;
        let (row, col) = (slot / cols, slot % cols);
        let target = if down { row + 1 } else { row.wrapping_sub(1) };
        if target >= rows {
            return;
        }
        self.path.truncate(depth + 1);
        self.path[depth].1 = target * cols + col;
        self.pos = 0;
    }

    /// Tab: moves to the next empty slot (Shift+Tab: the previous one). In a matrix, Tab from the
    /// last cell adds a row. With no empty slot left, Tab goes to the end of the expression
    /// (Shift+Tab to its start).
    pub fn tab(&mut self, forward: bool) {
        self.sync();
        self.anchor = None;
        if forward && self.caret_in_last_cell() && self.matrix_op(MatrixOp::InsertRow) {
            return;
        }
        // In a matrix, Tab and Shift+Tab step through the cells in order, filled or not (help:
        // "moves the cursor to the next element in the row"). Forward lands at the start of the
        // cell, back at its end.
        if let Some((path, end)) = self.matrix_neighbour_cell(forward) {
            self.path = path;
            self.pos = if end {
                row_at(&self.root, &self.path).map_or(0, Vec::len)
            } else {
                0
            };
            return;
        }
        let paths = slot_paths(&self.root);
        let Some(index) = paths.iter().position(|p| *p == self.path) else {
            return;
        };
        let is_empty = |p: &&Vec<(usize, usize)>| row_at(&self.root, p).is_some_and(Vec::is_empty);
        let target = if forward {
            paths[index + 1..].iter().find(is_empty).cloned()
        } else {
            paths[..index].iter().rev().find(is_empty).cloned()
        };
        match target {
            Some(path) => {
                self.path = path;
                self.pos = 0;
            }
            None if forward => self.end(),
            None => self.home(),
        }
    }

    /// The cell of the matrix that holds the caret, next to it in Tab order: the next cell forward
    /// or the previous one back, empty or not. The flag is true when the caret goes to the end of
    /// that cell (backward). None when the caret is not in a matrix cell or at its end.
    fn matrix_neighbour_cell(&self, forward: bool) -> Option<(Vec<(usize, usize)>, bool)> {
        let (&(at, _), parent) = self.path.split_last()?;
        if !matches!(
            row_at(&self.root, parent)?.get(at),
            Some(Item::Matrix { .. })
        ) {
            return None;
        }
        let cells: Vec<Vec<(usize, usize)>> = slot_paths(&self.root)
            .into_iter()
            .filter(|p| {
                p.len() == self.path.len()
                    && p.split_last()
                        .is_some_and(|(&(a, _), q)| a == at && q == parent)
            })
            .collect();
        let index = cells.iter().position(|p| *p == self.path)?;
        if forward {
            cells.get(index + 1).map(|p| (p.clone(), false))
        } else {
            index.checked_sub(1).map(|i| (cells[i].clone(), true))
        }
    }

    /// Inserts a matrix row above or below, or a column, or deletes one, in the innermost matrix
    /// that holds the caret. The caret goes to the new or nearest cell. Returns false (and changes
    /// nothing) when there is no matrix, the matrix is at its size limit, or it would be emptied.
    pub fn matrix_op(&mut self, op: MatrixOp) -> bool {
        self.sync();
        self.anchor = None;
        let Some(depth) = self.enclosing_matrix() else {
            return false;
        };
        let Some((at, rows, cols)) = self.matrix_dims(depth) else {
            return false;
        };
        let slot = self.path[depth].1;
        let (r, c) = (slot / cols, slot % cols);
        let prefix = self.path[..depth].to_vec();
        let Some(parent) = row_at_mut(&mut self.root, &prefix) else {
            return false;
        };
        let Some(Item::Matrix {
            cols: cols_field,
            cells,
        }) = parent.get_mut(at)
        else {
            return false;
        };
        let (new_cols, new_slot) = match op {
            MatrixOp::InsertRow | MatrixOp::InsertRowAbove => {
                if rows >= MAX_MATRIX_SIDE {
                    return false;
                }
                let new_row = if op == MatrixOp::InsertRow { r + 1 } else { r };
                let first = new_row * cols;
                cells.splice(first..first, (0..cols).map(|_| Row::new()));
                (cols, first)
            }
            MatrixOp::InsertColumn | MatrixOp::InsertColumnLeft => {
                if cols >= MAX_MATRIX_SIDE {
                    return false;
                }
                let new_col = if op == MatrixOp::InsertColumn {
                    c + 1
                } else {
                    c
                };
                for row in (0..rows).rev() {
                    cells.insert(row * cols + new_col, Row::new());
                }
                (cols + 1, r * (cols + 1) + new_col)
            }
            MatrixOp::DeleteRow => {
                if rows <= 1 {
                    return false;
                }
                cells.drain(r * cols..(r + 1) * cols);
                let kept = r.min(rows - 2);
                (cols, kept * cols + c)
            }
            MatrixOp::DeleteColumn => {
                if cols <= 1 {
                    return false;
                }
                for row in (0..rows).rev() {
                    cells.remove(row * cols + c);
                }
                let kept = c.min(cols - 2);
                (cols - 1, r * (cols - 1) + kept)
            }
        };
        *cols_field = new_cols;
        self.path.truncate(depth + 1);
        self.path[depth].1 = new_slot;
        self.pos = 0;
        true
    }

    /// Deletes the item before the caret (or the selection). A structure is deleted with its
    /// content. At the start of a bracket, root or absolute value, the bracket is removed and its
    /// content kept.
    pub fn backspace(&mut self) {
        self.sync();
        if self.take_selection().is_some() {
            return;
        }
        if self.pos == 0 {
            self.unwrap_bracket();
            return;
        }
        // A multi-character operator (`:=`, `<=`, `->` ...) is one token: it goes as a whole.
        let len = self.operator_len_before();
        let at = self.pos - len;
        if let Some(row) = row_at_mut(&mut self.root, &self.path) {
            row.drain(at..at + len);
        }
        self.pos = at;
    }

    /// Length of the multi-character operator that ends at the caret, or 1.
    fn operator_len_before(&self) -> usize {
        let row = self.row();
        let end = self.pos.min(row.len());
        for op in MULTI_OPS {
            let n = op.chars().count();
            if end >= n && atoms_equal(&row[end - n..end], op) {
                return n;
            }
        }
        1
    }

    /// Length of the multi-character operator that starts at the caret, or 1.
    fn operator_len_after(&self) -> usize {
        let row = self.row();
        for op in MULTI_OPS {
            let n = op.chars().count();
            if self.pos + n <= row.len() && atoms_equal(&row[self.pos..self.pos + n], op) {
                return n;
            }
        }
        1
    }

    /// Deletes the item after the caret (or the selection).
    pub fn delete(&mut self) {
        self.sync();
        if self.take_selection().is_some() {
            return;
        }
        let at = self.pos;
        let len = self.operator_len_after();
        if let Some(row) = row_at_mut(&mut self.root, &self.path) {
            if at < row.len() {
                let n = len.min(row.len() - at);
                row.drain(at..at + n);
            }
        }
    }

    /// Inserts an operator with empty slots and the caret in the first one.
    fn insert_op(&mut self, kind: OpKind) {
        self.sync();
        self.anchor = None;
        let slots = vec![Vec::new(); kind.slot_total()];
        self.enter(Item::Op { kind, slots }, 0);
    }

    /// Inserts a template at the caret, through the same keys the user would type. Structures take
    /// the operand before the caret (`x` then Fraction gives `x/■`).
    pub fn apply_template(&mut self, template: Template) {
        match template {
            Template::Fraction => self.key('/'),
            Template::Power => self.key('^'),
            Template::SquareRoot => self.insert_sqrt(),
            // A name subscript is written with `_`, as the core reads it (`v_0`).
            Template::Subscript => self.key('_'),
            Template::Define => self.key(':'),
            Template::Evaluate => self.key('='),
            Template::Symbolic => {
                self.key('-');
                self.key('>');
            }
            Template::Matrix2x2 => self.insert_matrix(2, 2),
            Template::Sum => self.insert_op(OpKind::Sum),
            Template::Product => self.insert_op(OpKind::Product),
            Template::Integral => self.insert_op(OpKind::Integral),
            Template::Derivative => self.insert_op(OpKind::Derivative),
            Template::NthDerivative => self.insert_op(OpKind::NthDerivative),
            Template::Partial => self.insert_op(OpKind::Partial),
            Template::NthPartial => self.insert_op(OpKind::NthPartial),
            Template::Gradient => self.insert_op(OpKind::Gradient),
            Template::Norm => self.insert_op(OpKind::Norm),
            Template::Cross => self.put_text("×"),
            Template::Limit => self.insert_op(OpKind::Limit),
            Template::NthRoot => self.insert_op(OpKind::NthRoot),
            Template::Abs => self.key('|'),
            Template::Factorial => self.key('!'),
            Template::Range => {
                self.key('.');
                self.key('.');
            }
        }
    }

    /// Types `text` at the caret as plain characters (a matrix, a function name, a symbol). The
    /// text is not checked; the parser reads it when the region is committed. Control characters
    /// are dropped.
    pub fn insert_text(&mut self, text: &str) {
        // A symbolic keyword from the ribbon follows the arrow; without one, the arrow is added.
        if SYMBOLIC_KEYWORDS.contains(&text)
            && !self.row()[..self.pos.min(self.row().len())].contains(&Item::Atom(ARROW))
        {
            self.key('-');
            self.key('>');
            self.put_text(" ");
        }
        self.put_text(text);
    }

    /// Puts the characters of `text` at the caret as plain items: no key handling, no structure.
    fn put_text(&mut self, text: &str) {
        self.sync();
        self.anchor = None;
        for c in text.chars().filter(|c| !c.is_control()) {
            self.put(Item::Atom(c));
        }
    }

    /// Makes the cursor valid again. A cursor that does not fit the tree (which keys never make)
    /// goes to the end of the root row, so no key can panic. A selection that no longer fits is
    /// dropped.
    fn sync(&mut self) {
        let fits = row_at(&self.root, &self.path).is_some_and(|row| self.pos <= row.len());
        if !fits {
            self.path.clear();
            self.pos = self.root.len();
            self.anchor = None;
        }
        if self.selection_range().is_none() {
            self.anchor = None;
        }
    }

    /// The row that holds the caret. Valid after `sync`.
    fn row(&self) -> &Row {
        row_at(&self.root, &self.path).unwrap_or(&self.root)
    }

    /// The item that owns the slot the caret is in, if the caret is inside a structure.
    fn container(&self) -> Option<&Item> {
        let (&(at, _), prefix) = self.path.split_last()?;
        row_at(&self.root, prefix)?.get(at)
    }

    /// The selected range of the caret's row, as (start, end), or `None` when the selection is
    /// empty or does not fit the row.
    fn selection_range(&self) -> Option<(usize, usize)> {
        let (path, at) = self.anchor.as_ref()?;
        if *path != self.path || *at == self.pos {
            return None;
        }
        let (lo, hi) = if *at < self.pos {
            (*at, self.pos)
        } else {
            (self.pos, *at)
        };
        (hi <= self.row().len()).then_some((lo, hi))
    }

    /// Removes the selected items from the caret's row and returns them. The caret goes to the
    /// start of the removed run.
    fn take_selection(&mut self) -> Option<Row> {
        let (a, b) = self.selection_range()?;
        self.anchor = None;
        let row = row_at_mut(&mut self.root, &self.path)?;
        let taken: Row = row.drain(a..b).collect();
        self.pos = a;
        Some(taken)
    }

    /// Grows the selection (Space): first to the whole row it is in, then, when the whole row is
    /// selected, to the item that owns the row.
    fn expand_selection(&mut self) {
        let Some((a, b)) = self.selection_range() else {
            return;
        };
        let len = self.row().len();
        if (a, b) != (0, len) {
            self.anchor = Some((self.path.clone(), 0));
            self.pos = len;
            return;
        }
        let Some(&(at, _)) = self.path.last() else {
            return;
        };
        self.path.pop();
        self.anchor = Some((self.path.clone(), at));
        self.pos = at + 1;
    }

    /// Puts the selected `content` into the structure that `c` opens. A fraction, power or index
    /// takes it as its first part; a group or absolute value takes it and the caret goes after.
    fn wrap_selection(&mut self, c: char, content: Row) {
        match c {
            '/' => self.enter(
                Item::Fraction {
                    num: content,
                    den: Vec::new(),
                },
                1,
            ),
            '^' => self.enter(
                Item::Power {
                    base: content,
                    exp: Vec::new(),
                },
                1,
            ),
            '[' => self.enter(
                Item::Index {
                    base: content,
                    index: Vec::new(),
                },
                1,
            ),
            '(' => self.put(Item::Group(content)),
            '|' => self.put(Item::Abs(content)),
            '\\' => self.put(Item::Sqrt(content)),
            _ => {}
        }
    }

    /// Removes the bracket (group, root or absolute value) the caret starts inside, keeping its
    /// content in its place. Returns false when the caret is not at the start of such a bracket.
    fn unwrap_bracket(&mut self) -> bool {
        let Some(&(at, 0)) = self.path.last() else {
            return false;
        };
        let depth = self.path.len() - 1;
        let prefix = self.path[..depth].to_vec();
        let Some(parent) = row_at_mut(&mut self.root, &prefix) else {
            return false;
        };
        let content = match parent.get_mut(at) {
            Some(Item::Group(r) | Item::Sqrt(r) | Item::Abs(r)) => std::mem::take(r),
            _ => return false,
        };
        parent.splice(at..=at, content);
        self.path.truncate(depth);
        self.pos = at;
        true
    }

    /// The innermost matrix that holds the caret, as its depth in `path`.
    fn enclosing_matrix(&self) -> Option<usize> {
        (0..self.path.len()).rev().find(|&depth| {
            let (at, _) = self.path[depth];
            matches!(
                row_at(&self.root, &self.path[..depth]).and_then(|row| row.get(at)),
                Some(Item::Matrix { .. })
            )
        })
    }

    /// The position in the row, the row count and the column count of the matrix at `depth`.
    fn matrix_dims(&self, depth: usize) -> Option<(usize, usize, usize)> {
        let (at, _) = *self.path.get(depth)?;
        match row_at(&self.root, &self.path[..depth])?.get(at)? {
            Item::Matrix { cols, cells } => {
                let cols = (*cols).max(1);
                Some((at, cells.len() / cols, cols))
            }
            _ => None,
        }
    }

    /// True when the caret is in the last cell (bottom right) of the innermost matrix.
    fn caret_in_last_cell(&self) -> bool {
        let Some(depth) = self.enclosing_matrix() else {
            return false;
        };
        match self.matrix_dims(depth) {
            Some((_, rows, cols)) => self.path[depth].1 + 1 == rows * cols,
            None => false,
        }
    }

    /// Inserts a character item at the caret and moves past it.
    fn put(&mut self, item: Item) {
        let at = self.pos;
        if let Some(row) = row_at_mut(&mut self.root, &self.path) {
            row.insert(at, item);
            self.pos = at + 1;
        }
    }

    /// Inserts a structure at the caret and moves the caret into its slot `slot`.
    fn enter(&mut self, item: Item, slot: usize) {
        let at = self.pos;
        if let Some(row) = row_at_mut(&mut self.root, &self.path) {
            row.insert(at, item);
            self.path.push((at, slot));
            self.pos = 0;
        }
    }

    /// Like `enter`, but the new structure takes the operand before the caret as its first slot.
    fn enter_with_operand(&mut self, make: impl FnOnce(Row) -> Item, slot: usize) {
        let pos = self.pos;
        let Some(row) = row_at_mut(&mut self.root, &self.path) else {
            return;
        };
        let (operand, at) = take_operand(row, pos);
        row.insert(at, make(operand));
        self.path.push((at, slot));
        self.pos = 0;
    }

    /// Leaves the nearest enclosing structure that `closer` closes, together with any structure
    /// between the caret and it (so `(1/2)` typed in order ends the group after the fraction).
    /// Returns false when no such structure encloses the caret; the closing character is then
    /// typed as a plain character.
    fn leave(&mut self, closer: Closer) -> bool {
        for depth in (0..self.path.len()).rev() {
            let (at, slot) = self.path[depth];
            let Some(owner) = row_at(&self.root, &self.path[..depth]).and_then(|row| row.get(at))
            else {
                continue;
            };
            let found = match owner {
                Item::Group(_) => closer == Closer::Group && slot == 0,
                Item::Index { .. } => closer == Closer::Index && slot == 1,
                Item::Abs(_) => closer == Closer::Abs && slot == 0,
                _ => false,
            };
            if found {
                self.path.truncate(depth);
                self.pos = at + 1;
                return true;
            }
        }
        false
    }

    /// Renders the tree. With `caret`, the caret is marked and its index returned.
    fn render(&self, caret: bool) -> (String, usize, Option<(usize, usize)>) {
        let marker = caret.then(|| (self.path.clone(), self.pos));
        let mut w = Writer::new(marker);
        w.typeset = caret;
        if caret {
            w.sel = self
                .selection_range()
                .map(|(lo, hi)| (self.path.clone(), lo, hi));
        }
        let mut path = Vec::new();
        emit_row(&self.root, &mut path, &mut w);
        // Strip the markers, counting the text characters before each one.
        let (mut text, mut caret_at, mut start, mut end) = (String::new(), None, None, None);
        let mut n = 0;
        for c in w.out.chars() {
            match c {
                MARK => {
                    caret_at.get_or_insert(n);
                }
                SEL_START => start = Some(n),
                SEL_END => end = Some(n),
                _ => {
                    text.push(c);
                    n += 1;
                }
            }
        }
        let selection = start.zip(end).filter(|(a, b)| a < b);
        (text, caret_at.unwrap_or(0), selection)
    }
}

/// A selection to render: its row's path, and the start and end positions in that row.
type SelectionMark = (Vec<(usize, usize)>, usize, usize);

/// Builds the rendered text. Tracks the run of letters or digits just written, so a number that
/// is followed by a name gets a space (`2 x`), and a name followed by digits does not (`x2`).
struct Writer {
    out: String,
    run: Run,
    /// Path and position of the caret, when the caret is rendered.
    caret: Option<(Vec<(usize, usize)>, usize)>,
    /// The selection of one row, as (path, start, end), when it is rendered.
    sel: Option<SelectionMark>,
    /// When set, every caret position visited is recorded as (path, position, character index in
    /// the text). Used to find the caret slot under the mouse (see `MathEditor::slot_at`).
    slots: Option<Vec<CaretSlot>>,
    /// Draw the symbolic arrow as `→` (the draft) instead of the text form `->`.
    typeset: bool,
}

/// A caret position of the expression: the row's path, the position in the row, and the character
/// index of that position in the rendered text.
type CaretSlot = (Vec<(usize, usize)>, usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Free,
    Digits,
    Letters,
}

impl Writer {
    fn new(caret: Option<(Vec<(usize, usize)>, usize)>) -> Self {
        Self {
            out: String::new(),
            run: Run::Free,
            caret,
            sel: None,
            slots: None,
            typeset: false,
        }
    }

    fn push(&mut self, c: char) {
        if is_marker(c) {
            self.out.push(c);
            return;
        }
        if c.is_alphabetic() {
            if self.run == Run::Digits {
                self.out.push(' ');
            }
            self.run = Run::Letters;
        } else if c.is_ascii_digit() {
            if self.run == Run::Free {
                self.run = Run::Digits;
            }
        } else if c == '.' {
            if self.run != Run::Digits {
                self.run = Run::Free;
            }
        } else {
            self.run = Run::Free;
        }
        self.out.push(c);
    }

    /// Writes the `e` of an exponent literal: no space before it, and the run is reset so the
    /// digits (or sign and digits) after it start a new run.
    fn push_exponent(&mut self, c: char) {
        self.out.push(c);
        self.run = Run::Free;
    }

    fn push_str(&mut self, text: &str) {
        for c in text.chars() {
            self.push(c);
        }
    }

    fn caret_here(&mut self, path: &[(usize, usize)], pos: usize) -> bool {
        if let Some(slots) = self.slots.as_mut() {
            let index = self.out.chars().filter(|c| !is_marker(*c)).count();
            slots.push((path.to_vec(), pos, index));
        }
        matches!(&self.caret, Some((p, q)) if p.as_slice() == path && *q == pos)
    }

    /// Writes the selection's start and end markers at `pos` of the row at `path`.
    fn mark_selection(&mut self, path: &[(usize, usize)], pos: usize) {
        let Some((p, lo, hi)) = &self.sel else {
            return;
        };
        if p.as_slice() != path {
            return;
        }
        if *lo == pos {
            self.out.push(SEL_START);
        }
        if *hi == pos {
            self.out.push(SEL_END);
        }
    }
}

fn emit_row(row: &Row, path: &mut Vec<(usize, usize)>, w: &mut Writer) {
    if row.is_empty() {
        if w.caret_here(path, 0) {
            w.push(MARK);
        }
        w.push(PLACEHOLDER);
        return;
    }
    for (at, item) in row.iter().enumerate() {
        w.mark_selection(path, at);
        if w.caret_here(path, at) {
            w.push(MARK);
        }
        if let Item::Atom(c @ ('e' | 'E')) = item {
            if w.run == Run::Digits && exponent_digits_follow(row, at) {
                w.push_exponent(*c);
                continue;
            }
        }
        emit_item(item, at, path, w);
    }
    w.mark_selection(path, row.len());
    if w.caret_here(path, row.len()) {
        w.push(MARK);
    }
}

/// Whether the atom after the `e` at `at` is a digit, or a sign followed by a digit. Then the
/// `e` is an exponent marker (`2e3`, `1e-5`), not the start of a name.
fn exponent_digits_follow(row: &Row, at: usize) -> bool {
    let is_digit = |item: Option<&Item>| matches!(item, Some(Item::Atom(d)) if d.is_ascii_digit());
    match row.get(at + 1) {
        Some(Item::Atom('+' | '-')) => is_digit(row.get(at + 2)),
        next => is_digit(next),
    }
}

fn emit_slot(row: &Row, at: usize, slot: usize, path: &mut Vec<(usize, usize)>, w: &mut Writer) {
    path.push((at, slot));
    emit_row(row, path, w);
    path.pop();
}

/// Like `emit_slot`, but the slot is wrapped in parentheses when its text needs them.
fn emit_wrapped(row: &Row, at: usize, slot: usize, path: &mut Vec<(usize, usize)>, w: &mut Writer) {
    path.push((at, slot));
    let mut sub = Writer::new(w.caret.clone());
    sub.sel = w.sel.clone();
    sub.typeset = w.typeset;
    emit_row(row, path, &mut sub);
    path.pop();
    if needs_parens(row, &sub.out) {
        w.push('(');
        w.push_str(&sub.out);
        w.push(')');
    } else {
        w.push_str(&sub.out);
    }
}

fn emit_item(item: &Item, at: usize, path: &mut Vec<(usize, usize)>, w: &mut Writer) {
    match item {
        Item::Atom(ARROW) if !w.typeset => w.push_str("->"),
        Item::Atom(c) => w.push(*c),
        Item::Group(inner) => {
            w.push('(');
            emit_slot(inner, at, 0, path, w);
            w.push(')');
        }
        Item::Sqrt(inner) => {
            w.push_str("sqrt(");
            emit_slot(inner, at, 0, path, w);
            w.push(')');
        }
        Item::Fraction { num, den } => {
            emit_wrapped(num, at, 0, path, w);
            w.push('/');
            emit_wrapped(den, at, 1, path, w);
        }
        Item::Power { base, exp } => {
            emit_wrapped(base, at, 0, path, w);
            w.push('^');
            emit_wrapped(exp, at, 1, path, w);
        }
        Item::Index { base, index } => {
            emit_wrapped(base, at, 0, path, w);
            w.push('[');
            emit_slot(index, at, 1, path, w);
            w.push(']');
        }
        Item::Abs(inner) => {
            w.push('|');
            emit_slot(inner, at, 0, path, w);
            w.push('|');
        }
        Item::Op { kind, slots } => {
            if w.typeset {
                for seg in kind.layout() {
                    match seg {
                        Seg::Lit(text) => w.push_str(text),
                        Seg::Slot(i) => emit_slot(&slots[i], at, i, path, w),
                    }
                }
            } else if *kind == OpKind::Norm {
                w.push('‖');
                emit_slot(&slots[0], at, 0, path, w);
                w.push('‖');
            } else if *kind == OpKind::Gradient && slots[0].is_empty() {
                // No variables: the arguments of the function are the variables.
                w.push_str("gradient(");
                emit_slot(&slots[1], at, 1, path, w);
                w.push(')');
            } else {
                w.push_str(kind.name());
                w.push('(');
                for (n, &i) in kind.arguments().iter().take(slots.len()).enumerate() {
                    if n > 0 {
                        w.push_str(", ");
                    }
                    emit_slot(&slots[i], at, i, path, w);
                }
                w.push(')');
            }
        }
        Item::Matrix { cols, cells } => {
            let cols = (*cols).max(1);
            w.push('[');
            for (index, cell) in cells.iter().enumerate() {
                if index > 0 {
                    w.push_str(if index % cols == 0 { "; " } else { ", " });
                }
                emit_slot(cell, at, index, path, w);
            }
            w.push(']');
        }
    }
}

/// Whether a slot's text must be wrapped in parentheses to keep its meaning when it is a fraction
/// part or a power base or exponent. A single bracket or root is already delimited. Text made of
/// letters, digits, `.`, `_`, `'` and placeholders is an atom and needs none.
fn needs_parens(row: &Row, text: &str) -> bool {
    if let [only] = row.as_slice() {
        if matches!(
            only,
            Item::Group(_)
                | Item::Sqrt(_)
                | Item::Index { .. }
                | Item::Abs(_)
                | Item::Matrix { .. }
                | Item::Op { .. }
        ) {
            return false;
        }
    }
    let plain = text
        .chars()
        .filter(|c| !is_marker(*c))
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '\'' | PLACEHOLDER));
    !plain
}

pub use mathcraft_doc::context::{parse_math_in, EditContext};

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// The word `inf` in Mathcad text as the infinity sign (text between double quotes and longer
/// names are untouched).
fn fold_inf(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut quoted, mut i) = (String::with_capacity(text.len()), false, 0);
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            quoted = !quoted;
        }
        let is_inf = !quoted
            && chars[i..].starts_with(&['i', 'n', 'f'])
            && (i == 0 || !is_name_char(chars[i - 1]))
            && chars.get(i + 3).is_none_or(|n| !is_name_char(*n));
        if is_inf {
            out.push('∞');
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Reads Mathcad text into an expression with the default identifier rules (no document).
/// Used by the tests only; the editor reads with `parse_math_in`.
#[cfg(test)]
///
/// # Errors
/// The parser's error, with the byte position in `text`.
pub fn parse_math(text: &str) -> Result<Expr, ParseError> {
    parse_math_in(text, &EditContext::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Types `keys`. `→` and `←` move the caret, `⌫` is backspace and `√` starts a square root.
    fn run(keys: &str) -> MathEditor {
        let mut ed = MathEditor::new();
        for c in keys.chars() {
            match c {
                '→' => ed.right(),
                '←' => ed.left(),
                '⌫' => ed.backspace(),
                '√' => ed.insert_sqrt(),
                _ => ed.key(c),
            }
        }
        ed
    }

    #[test]
    fn typed_inf_becomes_the_infinity_sign() {
        assert_eq!(run("inf").to_text(), "∞");
        assert_eq!(run("inf+1").to_text(), "∞+1");
        assert_eq!(run("info+inf").to_text(), "info+∞");
        assert_eq!(run("x_inf").to_text(), "x_inf");
    }

    /// The typed keys give the expected text, and that text reads as the same expression as the
    /// expected text does.
    fn assert_typed(keys: &str, expected: &str) {
        let ed = run(keys);
        assert_eq!(ed.to_text(), expected, "keys {keys:?}");
        let typed = parse_math(&ed.to_text()).expect("typed text parses");
        let direct = parse_math(expected).expect("expected text parses");
        assert_eq!(typed, direct, "keys {keys:?}");
    }

    #[test]
    fn typed_expressions_read_as_the_parser_reads_them() {
        assert_typed("1/x", "1/x");
        assert_typed("(a+b)/c", "(a+b)/c");
        assert_typed("a+b/c", "a+b/c");
        assert_typed("x^2→+1", "x^2+1");
        assert_typed("2x", "2 x");
        assert_typed("(a+b)^2", "(a+b)^2");
        assert_typed("v[1]+2", "v[1]+2");
        assert_typed("√x", "sqrt(x)");
        assert_typed("x:5", "x:=5");
        assert_typed("x/(2+y)", "x/(2+y)");
        assert_typed("1/2", "1/2");
        assert_typed("1/(x/2)", "1/(x/2)");
        assert_typed("(1/2)^3", "(1/2)^3");
        assert_typed("2*(3+x)", "2*(3+x)");
    }

    #[test]
    fn a_typed_fraction_read_back_is_the_rational_the_parser_gives() {
        let expr = parse_math(&run("1/2").to_text()).expect("parses");
        assert_eq!(expr, parse_math("1/2").expect("parses"));
    }

    #[test]
    fn colon_types_the_define_operator() {
        let ed = run("x:");
        assert_eq!(ed.to_text(), "x:=");
    }

    #[test]
    fn an_empty_fraction_part_is_a_placeholder() {
        assert_eq!(run("1/").to_text(), "1/■");
        let expr = parse_math("1/■").expect("placeholder parses");
        assert!(matches!(expr, Expr::Binary { .. }));
    }

    #[test]
    fn left_enters_a_fraction_at_its_denominator_end() {
        // Typing 2 after moving left from the denominator goes into the numerator.
        let ed = run("1/←2");
        assert_eq!(ed.to_text(), "12/■");
    }

    #[test]
    fn right_leaves_the_power_and_continues_after_it() {
        assert_typed("x^2→→+1", "x^2+1");
        assert_typed("x:=1/2", "x:=1/2");
    }

    #[test]
    fn backspace_deletes_characters_and_whole_structures() {
        assert_eq!(run("12⌫").to_text(), "1");
        assert_eq!(run("1/2⌫").to_text(), "1/■");
        assert_eq!(run("x^2⌫").to_text(), "x^■");
    }

    #[test]
    fn delete_removes_the_item_after_the_caret() {
        let mut ed = run("abc←←");
        ed.delete();
        assert_eq!(ed.to_text(), "ac");
    }

    #[test]
    fn left_steps_into_a_group_from_its_right() {
        let ed = run("(a)←b");
        assert_eq!(ed.to_text(), "(ab)");
    }

    #[test]
    fn closing_a_group_moves_past_it() {
        let ed = run("(a)b");
        assert_eq!(ed.to_text(), "(a)b");
    }

    #[test]
    fn a_selection_is_reported_as_a_range_of_the_display_text() {
        let mut ed = run("1/x+ab");
        ed.select_left();
        ed.select_left();
        // The denominator is drawn in parentheses; the selection is its last two characters.
        let (text, caret, selection) = ed.display_selected();
        assert_eq!(text, "1/(x+ab)");
        assert_eq!(caret, 5);
        assert_eq!(selection, Some((5, 7)));
    }

    #[test]
    fn a_selection_inside_a_fraction_slot_is_reported_in_the_text() {
        let mut ed = run("1/ab");
        ed.select_left();
        let (text, caret, selection) = ed.display_selected();
        assert_eq!(text, "1/ab");
        assert_eq!(caret, 3);
        assert_eq!(selection, Some((3, 4)));
    }

    #[test]
    fn caret_is_reported_inside_the_denominator() {
        let (text, caret) = run("1/").display();
        assert_eq!(text, "1/■");
        assert_eq!(caret, 2);
    }

    #[test]
    fn caret_at_the_end_of_plain_text() {
        let ed = MathEditor::from_text("ab");
        assert_eq!(ed.display(), ("ab".to_string(), 2));
    }

    #[test]
    fn from_text_round_trips_flat_text() {
        let ed = MathEditor::from_text("x:=2*y+1");
        assert_eq!(ed.to_text(), "x:=2*y+1");
    }

    #[test]
    fn a_blank_editor_is_blank() {
        assert!(MathEditor::new().is_blank());
        assert!(!run("x").is_blank());
    }

    #[test]
    fn a_broken_cursor_does_not_panic() {
        let mut ed = run("1/2");
        ed.path = vec![(99, 0), (3, 1)];
        ed.pos = 40;
        ed.key('a');
        ed.right();
        ed.left();
        ed.backspace();
        ed.delete();
        let (text, _) = ed.display();
        assert!(!text.is_empty());
    }

    #[test]
    fn exponent_literals_keep_their_text_and_meaning() {
        assert_eq!(MathEditor::from_text("2e3").to_text(), "2e3");
        assert_eq!(MathEditor::from_text("1e-5").to_text(), "1e-5");
        assert_eq!(MathEditor::from_text("1E+5").to_text(), "1E+5");
        for text in ["2e3", "1e-5", "1E+5"] {
            let read = parse_math(&MathEditor::from_text(text).to_text()).expect("parses");
            assert_eq!(read, parse_math(text).expect("parses"), "text {text}");
        }
    }

    #[test]
    fn a_letter_e_without_digits_after_it_is_still_a_product() {
        assert_eq!(MathEditor::from_text("2e").to_text(), "2 e");
        assert_eq!(run("2e").to_text(), "2 e");
        assert_eq!(run("2 e3").to_text(), "2 e3");
        assert_eq!(run("2e-x").to_text(), "2 e-x");
        assert_eq!(run("2e3x").to_text(), "2e3 x");
    }

    #[test]
    fn an_unparsable_text_is_an_error_not_a_panic() {
        assert!(parse_math("a $ b").is_err());
    }

    #[test]
    fn a_press_puts_the_caret_at_the_clicked_character_of_the_draft() {
        let mut ed = MathEditor::from_text("ab+cd");
        ed.place_caret(1);
        assert_eq!(ed.display().1, 1);
        ed.place_caret(4);
        assert_eq!(ed.display().1, 4);
    }

    #[test]
    fn a_drag_and_a_double_click_select_in_the_draft() {
        let mut ed = MathEditor::from_text("abcdef");
        ed.place_caret(1);
        ed.extend_to(4);
        assert_eq!(ed.display_selected().2, Some((1, 4)));

        let mut ed = MathEditor::from_text("alpha+beta");
        ed.select_word_at(7);
        assert_eq!(ed.display_selected().2, Some((6, 10)));
    }
}

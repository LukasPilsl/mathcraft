//! Tests of the editing keys that the Mathcad Prime help documents (see
//! `documentation/EDITING_PARITY.md`): the math keys, the Ctrl+Shift operator shortcuts, Shift+Enter
//! in a matrix, and the region alignment commands with their undo steps.

use std::collections::BTreeMap;

use mathcraft_import::regions::{ImportReport, RegionKind, RegionModel, Settings, Worksheet};
use mathcraft_import::results::ResultTable;

use super::document::Document;
use super::layout::Arrange;
use super::math::{parse_math, MathEditor, Template};
use super::session::{EditKey, EditSession};

fn worksheet(regions: Vec<RegionModel>) -> Worksheet {
    Worksheet {
        regions,
        settings: Settings::default(),
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer: Vec::new(),
    }
}

fn session_with(regions: Vec<RegionModel>) -> EditSession {
    let doc = Document::from_worksheet(
        "parity",
        worksheet(regions),
        ResultTable {
            by_id: BTreeMap::new(),
        },
    );
    EditSession::new(doc)
}

fn empty_session() -> EditSession {
    session_with(Vec::new())
}

/// A math region at `pos` with size `size`, holding `text`.
fn boxed(id: u32, pos: (f64, f64), size: (f64, f64), text: &str) -> RegionModel {
    RegionModel {
        id,
        pos: Some(pos),
        size: Some(size),
        result_ref: None,
        kind: RegionKind::Math(parse_math(text).expect("test text parses")),
        solve_category: None,
        children: Vec::new(),
        result_format: None,
        matrix: Default::default(),
    }
}

/// The text of the open math draft, as the session shows it.
fn draft_text(s: &EditSession) -> String {
    s.draft_view().expect("a draft is open").text
}

fn positions(s: &EditSession) -> Vec<Option<(f64, f64)>> {
    s.document().regions().iter().map(|r| r.pos).collect()
}

/// Three regions: left edges 10, 80, 200; sizes (50,15), (30,10), (20,40).
fn three_boxes() -> Vec<RegionModel> {
    vec![
        boxed(1, (10.0, 20.0), (50.0, 15.0), "a"),
        boxed(2, (80.0, 60.0), (30.0, 10.0), "b"),
        boxed(3, (200.0, 30.0), (20.0, 40.0), "c"),
    ]
}

/// Types `keys` into an editor: `\t` is Tab, `\u{8}` Backspace, `<` Left, `>` Right.
fn math(keys: &str) -> MathEditor {
    let mut ed = MathEditor::new();
    for c in keys.chars() {
        match c {
            '\t' => ed.tab(true),
            '\u{8}' => ed.backspace(),
            '<' => ed.left(),
            '>' => ed.right(),
            c => ed.key(c),
        }
    }
    ed
}

// ----- Math keys (help: Keyboard Shortcuts for Editing Math) -----

#[test]
fn backslash_types_a_square_root_around_what_follows() {
    // Help: Square Root and Nth Root Operator, keyboard shortcut `\`.
    assert_eq!(math("\\x+1").to_text(), "sqrt(x+1)");
}

#[test]
fn backslash_wraps_a_selection_in_a_square_root() {
    let mut ed = math("x");
    ed.select_left();
    ed.key('\\');
    assert_eq!(ed.to_text(), "sqrt(x)");
}

#[test]
fn ctrl_left_moves_to_the_start_of_the_term_before_the_caret() {
    // Help: Ctrl+Left moves the cursor to the left of the current term. A term is a run of letters
    // or a run of digits, so `ab12` has the terms `ab` and `12`.
    let mut ed = math("ab12");
    ed.term_left();
    ed.key('X');
    assert_eq!(ed.to_text(), "abX12");
    ed.term_left();
    ed.key('Y');
    assert_eq!(ed.to_text(), "YabX12");
}

#[test]
fn ctrl_right_moves_to_the_end_of_the_term_after_the_caret() {
    let mut ed = math("ab12");
    ed.home();
    ed.term_right();
    ed.key('Z');
    assert_eq!(ed.to_text(), "abZ12");
    ed.term_right();
    ed.key('W');
    assert_eq!(ed.to_text(), "abZ12W");
}

#[test]
fn ctrl_left_next_to_a_bracket_moves_one_item_like_left() {
    // The group is one item, so Ctrl+Left enters it from its right side, as Left does.
    let mut ed = math("(a)");
    ed.term_left();
    ed.key('b');
    assert_eq!(ed.to_text(), "(ab)");
}

#[test]
fn ctrl_left_clears_a_selection_and_moves_from_the_caret() {
    // Shift+Left selects `c` and leaves the caret before it; Ctrl+Left then goes to the start of
    // the term `abc` that the caret is in, and the selection is gone.
    let mut ed = math("abc");
    ed.select_left();
    ed.term_left();
    ed.key('X');
    assert_eq!(ed.to_text(), "Xabc");
}

// ----- Ctrl+Shift operators and the prime mark -----

#[test]
fn ctrl_shift_d_inserts_the_derivative_template() {
    // Help: Keyboard Shortcuts for Calculus Operators, Derivative, Ctrl+Shift+D.
    let mut s = empty_session();
    s.click([0.0, 50.0], None);
    s.insert_template(Template::Derivative);
    assert!(draft_text(&s).starts_with("d/d"));
}

#[test]
fn ctrl_shift_i_inserts_the_integral_template() {
    // Help: Integral Operator, Ctrl+Shift+I.
    let mut s = empty_session();
    s.click([0.0, 50.0], None);
    s.insert_template(Template::Integral);
    assert!(draft_text(&s).starts_with("∫"));
}

#[test]
fn ctrl_shift_dollar_inserts_the_summation_template() {
    // Help: Summation, Ctrl+Shift+$ (the Prime example "Sum the components of vector v").
    let mut s = empty_session();
    s.click([0.0, 50.0], None);
    s.insert_template(Template::Sum);
    assert!(draft_text(&s).starts_with("∑"));
}

#[test]
fn ctrl_shift_hash_inserts_the_product_template() {
    // Help: Product, Ctrl+Shift+#.
    let mut s = empty_session();
    s.click([0.0, 50.0], None);
    s.insert_template(Template::Product);
    assert!(draft_text(&s).starts_with("∏"));
}

#[test]
fn apostrophe_types_the_prime_mark_after_the_name() {
    // Help: Prime Operator, Ctrl+' (apostrophe): f'(t).
    let mut s = empty_session();
    s.click([0.0, 50.0], None);
    s.type_char('f');
    s.type_char('\'');
    assert_eq!(draft_text(&s), "f'");
}

#[test]
fn ctrl_l_inserts_the_limit_template() {
    // Help: Limit operator, Ctrl+L.
    let mut ed = MathEditor::new();
    ed.apply_template(Template::Limit);
    assert_eq!(ed.to_text(), "limit(■, ■, ■)");
}

#[test]
fn open_bracket_types_a_subscript_after_the_name() {
    // Help: "Type excel and press [ to insert a subscripted string".
    assert_eq!(math("v[1]").to_text(), "v[1]");
}

#[test]
fn symbolic_evaluation_template_types_the_arrow() {
    // Help: Symbolic Evaluation, Ctrl+. inserts the arrow.
    let mut ed = MathEditor::new();
    ed.apply_template(Template::Symbolic);
    assert_eq!(ed.to_text(), "->");
}

// ----- Shift+Enter in a matrix (help: Keyboard Shortcuts for Editing Arrays and Tables) -----

#[test]
fn shift_enter_in_a_matrix_adds_a_row_below() {
    let mut s = empty_session();
    s.click([0.0, 50.0], None);
    s.insert_matrix(1, 2);
    s.key(EditKey::Tab);
    s.key(EditKey::ShiftEnter);
    // One row became two: one row separator in the text.
    let text = draft_text(&s);
    assert_eq!(text.matches(';').count(), 1, "text was {text}");
}

#[test]
fn shift_tab_goes_back_to_the_previous_cell_even_when_it_is_filled() {
    // Help: Shift+Tab moves the cursor to the previous element in a row. Cells that hold text
    // count, so the caret does not leave the matrix.
    let mut ed = MathEditor::new();
    ed.insert_matrix(1, 2);
    ed.key('1');
    ed.tab(true);
    ed.key('2');
    ed.tab(false);
    ed.key('X');
    // A digit followed by a letter is written with a space, as the editor always writes it.
    assert_eq!(ed.to_text(), "[1 X, 2]");
}

#[test]
fn tab_goes_to_the_next_cell_even_when_it_is_filled() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(1, 3);
    ed.key('1');
    ed.tab(true);
    ed.key('2');
    ed.tab(true);
    ed.key('3');
    ed.tab(false);
    ed.tab(false);
    ed.key('X');
    assert_eq!(ed.to_text(), "[1 X, 2, 3]");
}

#[test]
fn shift_enter_outside_a_matrix_ends_the_region_like_enter() {
    let mut s = empty_session();
    s.click([0.0, 50.0], None);
    s.type_char('x');
    s.key(EditKey::ShiftEnter);
    assert!(s.draft_view().is_none());
    assert_eq!(s.document().regions().len(), 1);
}

// ----- Region alignment and spacing (Format tab commands), each one undoable -----

#[test]
fn align_top_moves_every_region_to_the_top_edge() {
    let mut s = session_with(three_boxes());
    let before = s.document().regions().to_vec();
    s.select_all();
    s.arrange(Arrange::AlignTop);
    assert_eq!(
        positions(&s),
        vec![Some((10.0, 20.0)), Some((80.0, 20.0)), Some((200.0, 20.0))]
    );
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn align_bottom_moves_every_region_to_the_bottom_edge() {
    let mut s = session_with(three_boxes());
    let before = s.document().regions().to_vec();
    s.select_all();
    s.arrange(Arrange::AlignBottom);
    // Lowest bottom edge is 70 (regions 2 and 3). Heights 15, 10, 40.
    assert_eq!(
        positions(&s),
        vec![Some((10.0, 55.0)), Some((80.0, 60.0)), Some((200.0, 30.0))]
    );
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn align_right_moves_every_region_to_the_right_edge() {
    let mut s = session_with(three_boxes());
    let before = s.document().regions().to_vec();
    s.select_all();
    s.arrange(Arrange::AlignRight);
    // Rightmost edge is 220 (region 3). Widths 50, 30, 20.
    assert_eq!(
        positions(&s),
        vec![
            Some((170.0, 20.0)),
            Some((190.0, 60.0)),
            Some((200.0, 30.0))
        ]
    );
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn center_horizontally_puts_every_centre_on_the_middle_of_the_group() {
    let mut s = session_with(three_boxes());
    let before = s.document().regions().to_vec();
    s.select_all();
    s.arrange(Arrange::AlignCenterX);
    // Group spans 10..220, middle 115. Widths 50, 30, 20.
    assert_eq!(
        positions(&s),
        vec![Some((90.0, 20.0)), Some((100.0, 60.0)), Some((105.0, 30.0))]
    );
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn center_vertically_puts_every_middle_on_the_middle_of_the_group() {
    let mut s = session_with(three_boxes());
    let before = s.document().regions().to_vec();
    s.select_all();
    s.arrange(Arrange::AlignCenterY);
    // Group spans 20..70, middle 45. Heights 15, 10, 40.
    assert_eq!(
        positions(&s),
        vec![Some((10.0, 37.5)), Some((80.0, 40.0)), Some((200.0, 25.0))]
    );
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn distribute_vertically_spaces_three_regions_evenly() {
    let mut s = session_with(vec![
        boxed(1, (10.0, 0.0), (50.0, 10.0), "a"),
        boxed(2, (10.0, 50.0), (50.0, 10.0), "b"),
        boxed(3, (10.0, 200.0), (50.0, 10.0), "c"),
    ]);
    let before = s.document().regions().to_vec();
    s.select_all();
    s.arrange(Arrange::DistributeVertical);
    // Span 210, heights 30 in total: gaps of 90.
    assert_eq!(
        positions(&s),
        vec![Some((10.0, 0.0)), Some((10.0, 100.0)), Some((10.0, 200.0))]
    );
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

#[test]
fn insert_blank_line_moves_the_regions_below_and_undo_restores_them() {
    // Help (Dokument tab): Leerzeile einfügen inserts a blank line at the insertion point.
    let mut s = session_with(three_boxes());
    let before = s.document().regions().to_vec();
    s.click([0.0, 50.0], None);
    s.insert_lines(12.0);
    assert_eq!(positions(&s)[1], Some((80.0, 72.0)));
    assert!(s.undo());
    assert_eq!(s.document().regions(), before.as_slice());
}

//! Tests of the math editor's keys: structures, matrices, Tab and Home/End, selection and Space,
//! and the property checks (typed text reads as the parser reads it; no key sequence panics).

use super::math::{parse_math, MathEditor, MatrixOp, MAX_NESTING};

/// Types `keys` one character at a time. `\t` is Tab, `\u{8}` Backspace, `<` Left, `>` Right,
/// `{` Shift+Left, `}` Shift+Right, `^` Up, `v` Down, `h` Home, `e` End.
fn type_keys(ed: &mut MathEditor, keys: &str) {
    for c in keys.chars() {
        match c {
            '\t' => ed.tab(true),
            '\u{8}' => ed.backspace(),
            '<' => ed.left(),
            '>' => ed.right(),
            '{' => ed.select_left(),
            '}' => ed.select_right(),
            '\u{1b}' => ed.vertical(false),
            'v' => ed.vertical(true),
            'h' => ed.home(),
            'E' => ed.end(),
            c => ed.key(c),
        }
    }
}

fn run(keys: &str) -> MathEditor {
    let mut ed = MathEditor::new();
    type_keys(&mut ed, keys);
    ed
}

#[test]
fn absolute_value_is_typed_with_its_bars() {
    let ed = run("|x|+1");
    assert_eq!(ed.to_text(), "|x|+1");
    assert!(parse_math("|x|+1").is_ok());
}

#[test]
fn a_bar_inside_an_absolute_value_closes_it() {
    // The second bar leaves the absolute value, so the 2 is outside it.
    assert_eq!(run("|x|2").to_text(), "|x|2");
}

#[test]
fn a_selection_becomes_the_absolute_value() {
    // x is selected, then | wraps it and the caret goes after the bars.
    let mut ed = run("x");
    ed.select_left();
    ed.key('|');
    ed.key('1');
    assert_eq!(ed.to_text(), "|x|1");
}

#[test]
fn a_selection_becomes_the_power_base() {
    let mut ed = run("ab");
    ed.select_left();
    ed.key('^');
    ed.key('2');
    assert_eq!(ed.to_text(), "ab^2");
    assert_eq!(parse_math(&ed.to_text()), parse_math("ab^2"));
}

#[test]
fn a_matrix_is_typed_cell_by_cell_with_tab() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(2, 2);
    assert_eq!(ed.to_text(), "[■, ■; ■, ■]");
    type_keys(&mut ed, "1\t2\t3\t4");
    assert_eq!(ed.to_text(), "[1, 2; 3, 4]");
    assert!(parse_math(&ed.to_text()).is_ok());
}

#[test]
fn tab_from_the_last_cell_adds_a_row() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(1, 2);
    type_keys(&mut ed, "a\tb\tc");
    assert_eq!(ed.to_text(), "[a, b; c, ■]");
}

#[test]
fn a_matrix_size_is_clamped() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(99, 99);
    let text = ed.to_text();
    // Ten rows of ten cells: nine row separators, and nine separators in each row.
    assert_eq!(text.matches("; ").count(), 9);
    assert_eq!(text.matches(", ").count(), 90);
}

#[test]
fn up_and_down_move_between_cells_of_a_column() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(2, 2);
    type_keys(&mut ed, "1\t2\t3\t4");
    // Cell (1,1) holds 4; up goes to (0,1), which holds 2.
    type_keys(&mut ed, "\u{1b}");
    type_keys(&mut ed, "x");
    assert_eq!(ed.to_text(), "[1, x2; 3, 4]");
}

#[test]
fn matrix_rows_and_columns_are_added_and_deleted() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(2, 2);
    assert!(ed.matrix_op(MatrixOp::InsertColumn));
    assert_eq!(ed.to_text(), "[■, ■, ■; ■, ■, ■]");
    assert!(ed.matrix_op(MatrixOp::DeleteRow));
    assert_eq!(ed.to_text(), "[■, ■, ■]");
    assert!(ed.matrix_op(MatrixOp::DeleteColumn));
    assert!(ed.matrix_op(MatrixOp::DeleteColumn));
    assert_eq!(ed.to_text(), "[■]");
    // A matrix keeps one row and one column.
    assert!(!ed.matrix_op(MatrixOp::DeleteColumn));
    assert!(!ed.matrix_op(MatrixOp::DeleteRow));
    assert_eq!(ed.to_text(), "[■]");
}

#[test]
fn matrix_operations_outside_a_matrix_change_nothing() {
    let mut ed = run("a");
    assert!(!ed.matrix_op(MatrixOp::InsertRow));
    assert_eq!(ed.to_text(), "a");
}

#[test]
fn a_matrix_row_insert_keeps_the_cells_of_the_other_rows() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(2, 1);
    type_keys(&mut ed, "1\t2");
    // Up to the first cell, then insert a row below it: the caret goes to the new row.
    type_keys(&mut ed, "\u{1b}");
    assert!(ed.matrix_op(MatrixOp::InsertRow));
    type_keys(&mut ed, "x");
    assert_eq!(ed.to_text(), "[1; x; 2]");
}

#[test]
fn tab_moves_into_an_empty_numerator() {
    // `/` on an empty root makes a fraction with the caret in its denominator.
    let mut ed = run("/1");
    ed.home();
    ed.tab(true);
    type_keys(&mut ed, "2");
    assert_eq!(ed.to_text(), "2/1");
}

#[test]
fn tab_with_no_empty_place_goes_to_the_end_and_back_to_the_start() {
    let mut ed = run("a+b");
    ed.home();
    ed.tab(true);
    type_keys(&mut ed, "c");
    assert_eq!(ed.to_text(), "a+bc");
    ed.tab(false);
    type_keys(&mut ed, "d");
    assert_eq!(ed.to_text(), "da+bc");
}

#[test]
fn home_and_end_go_to_the_ends_of_the_expression() {
    let mut ed = run("ab");
    ed.left();
    ed.home();
    type_keys(&mut ed, "x");
    ed.end();
    type_keys(&mut ed, "y");
    assert_eq!(ed.to_text(), "xaby");
}

#[test]
fn backspace_at_the_start_of_a_group_removes_the_brackets_and_keeps_the_content() {
    let mut ed = run("(a");
    ed.left();
    ed.backspace();
    assert_eq!(ed.to_text(), "a");
}

#[test]
fn backspace_removes_one_character_or_a_whole_structure() {
    let mut ed = run("a/b");
    ed.backspace();
    assert_eq!(ed.to_text(), "a/■");
    let mut ed = run("a/b");
    ed.right();
    ed.backspace();
    // The whole fraction went; an empty expression shows its placeholder.
    assert_eq!(ed.to_text(), "■");
}

#[test]
fn shift_arrows_select_and_typing_replaces_the_selection() {
    let mut ed = run("a+b");
    type_keys(&mut ed, "{{");
    assert_eq!(ed.selected_text().as_deref(), Some("+b"));
    ed.key('x');
    assert_eq!(ed.to_text(), "ax");
}

#[test]
fn space_grows_the_selection_to_the_whole_row() {
    let mut ed = run("a+b");
    type_keys(&mut ed, "{{");
    ed.key(' ');
    assert_eq!(ed.selected_text().as_deref(), Some("a+b"));
}

#[test]
fn space_without_a_selection_types_a_space() {
    assert_eq!(run("a b").to_text(), "a b");
}

#[test]
fn space_selects_the_structure_that_owns_a_selected_row() {
    // Select the whole denominator, then Space selects the fraction it belongs to.
    let mut ed = run("1/2");
    type_keys(&mut ed, "{");
    assert_eq!(ed.selected_text().as_deref(), Some("2"));
    ed.key(' ');
    assert_eq!(ed.selected_text().as_deref(), Some("1/2"));
    // The top level has no owner: a further Space changes nothing.
    ed.key(' ');
    assert_eq!(ed.selected_text().as_deref(), Some("1/2"));
}

#[test]
fn cut_takes_the_selected_text_out() {
    let mut ed = run("a+b");
    type_keys(&mut ed, "{{");
    assert_eq!(ed.cut_selection().as_deref(), Some("+b"));
    assert_eq!(ed.to_text(), "a");
    assert_eq!(ed.cut_selection(), None);
}

#[test]
fn a_selection_is_dropped_by_a_move() {
    let mut ed = run("a+b");
    type_keys(&mut ed, "{");
    ed.left();
    assert_eq!(ed.selected_text(), None);
}

#[test]
fn typing_deep_nesting_stops_at_the_cap_and_does_not_panic() {
    let mut ed = MathEditor::new();
    for _ in 0..(MAX_NESTING + 50) {
        ed.key('(');
    }
    let (text, caret) = ed.display();
    assert!(caret <= text.chars().count());
    assert!(!ed.to_text().is_empty());
}

#[test]
fn inserted_text_drops_control_characters() {
    let mut ed = MathEditor::new();
    ed.insert_text("sqrt(2)\u{0}\n");
    assert_eq!(ed.to_text(), "sqrt(2)");
}

#[test]
fn a_flat_typed_sequence_reads_as_the_parser_reads_its_text() {
    // Property: typing plain characters gives the typed text, with the one space the editor adds
    // between a number and a following name (`6b` is typed as `6 b`, a product). The editor's
    // text and the model's text then read the same way in the parser.
    let alphabet: Vec<char> = "abxyz0123456789+-*= ".chars().collect();
    let mut rng = Lcg(7);
    for _ in 0..300 {
        let len = 1 + rng.below(12);
        let typed: String = (0..len)
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        if typed.trim().is_empty() {
            continue;
        }
        // The writer's run rule: a name after a digit run gets a space; a digit inside a name
        // does not start a run.
        let mut expected = String::new();
        let mut in_digits = false;
        let mut in_letters = false;
        for c in typed.chars() {
            if c.is_alphabetic() {
                if in_digits {
                    expected.push(' ');
                }
                in_digits = false;
                in_letters = true;
            } else if c.is_ascii_digit() {
                if !in_letters {
                    in_digits = true;
                }
            } else {
                in_digits = false;
                in_letters = false;
            }
            expected.push(c);
        }
        let ed = run(&typed);
        assert_eq!(ed.to_text(), expected, "typed {typed:?}");
        assert_eq!(
            parse_math(&ed.to_text()).is_ok(),
            parse_math(&expected).is_ok()
        );
    }
}

#[test]
fn random_key_sequences_never_panic_and_keep_the_caret_valid() {
    let keys = [
        'a', 'b', '1', '+', '(', ')', '/', '^', '[', ']', '|', ':', '=', ' ', '\t', '\u{8}', '<',
        '>', '{', '}', '\u{1b}', 'v', 'h', 'E',
    ];
    let mut rng = Lcg(42);
    for _ in 0..400 {
        let mut ed = MathEditor::new();
        for _ in 0..(20 + rng.below(60)) {
            match rng.below(40) {
                0 => ed.insert_matrix(1 + rng.below(4), 1 + rng.below(4)),
                1 => {
                    let op = [
                        MatrixOp::InsertRow,
                        MatrixOp::InsertColumn,
                        MatrixOp::DeleteRow,
                        MatrixOp::DeleteColumn,
                    ][rng.below(4)];
                    ed.matrix_op(op);
                }
                2 => ed.insert_text("sqrt(x)"),
                3 => ed.insert_sqrt(),
                _ => {
                    let c = keys[rng.below(keys.len())];
                    type_keys(&mut ed, &c.to_string());
                }
            }
            let (text, caret) = ed.display();
            assert!(caret <= text.chars().count());
            let _ = ed.to_text();
        }
    }
}

#[test]
fn a_matrix_typed_with_tab_parses_as_the_matrix_it_shows() {
    let mut ed = MathEditor::new();
    ed.insert_matrix(2, 2);
    type_keys(&mut ed, "1\t2\t3\t4");
    let typed = "[1, 2; 3, 4]";
    assert_eq!(ed.to_text(), typed);
    assert_eq!(parse_math(&ed.to_text()), parse_math(typed));
}

/// A small deterministic generator, so the property tests need no crate.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

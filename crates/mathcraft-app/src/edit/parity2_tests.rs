//! Tests of the second editor parity pass: atomic operators, symbolic arrow, operator templates.

use super::math::{parse_math, MathEditor, Template};

fn typed(ed: &mut MathEditor, text: &str) {
    for c in text.chars() {
        ed.key(c);
    }
}

/// Backspace and Delete take a multi-character operator as one token.
#[test]
fn backspace_and_delete_remove_multi_char_operators_whole() {
    for op in [":=", "<=", ">=", "<>", "!=", "->", "=="] {
        let mut ed = MathEditor::new();
        typed(&mut ed, &format!("a{op}b"));
        ed.left();
        ed.backspace();
        assert_eq!(ed.to_text(), "ab", "backspace after {op}");
        let mut ed = MathEditor::new();
        typed(&mut ed, &format!("a{op}b"));
        ed.home();
        ed.right();
        ed.delete();
        assert_eq!(ed.to_text(), "ab", "delete before {op}");
    }
    let mut ed = MathEditor::new();
    typed(&mut ed, "x:");
    ed.backspace();
    assert_eq!(ed.to_text(), "x", "':' then Backspace leaves no stray '='");
}

/// Applies `template`, types each of `fills` in the slots (Tab between), and returns the editor.
fn filled(template: Template, fills: &[&str]) -> MathEditor {
    let mut ed = MathEditor::new();
    ed.apply_template(template);
    for (i, fill) in fills.iter().enumerate() {
        if i > 0 {
            ed.tab(true);
        }
        typed(&mut ed, fill);
    }
    ed
}

/// Operator audit: each template shows typeset with empty placeholders, Tab visits the slots in
/// order, and the text it writes is what the parser reads.
#[test]
fn operator_templates_are_typeset_and_navigable() {
    let cases: [(&str, Template, &[&str], &str, &str); 7] = [
        (
            "derivative",
            Template::Derivative,
            &["t", "v"],
            "d/d",
            "derivative(t, v)",
        ),
        (
            "nth derivative",
            Template::NthDerivative,
            &["2", "t", "v"],
            "dⁿ",
            "derivative(t, 2, v)",
        ),
        (
            "integral",
            Template::Integral,
            &["0", "1", "x", "x"],
            "∫",
            "integral(x, 0, 1, x)",
        ),
        (
            "sum",
            Template::Sum,
            &["i", "1", "5", "i"],
            "∑",
            "sum(i, 1, 5, i)",
        ),
        (
            "product",
            Template::Product,
            &["i", "1", "5", "i"],
            "∏",
            "product(i, 1, 5, i)",
        ),
        (
            "limit",
            Template::Limit,
            &["x", "0", "x"],
            "lim",
            "limit(x, 0, x)",
        ),
        (
            "nth root",
            Template::NthRoot,
            &["3", "27"],
            "√",
            "root(27, 3)",
        ),
    ];
    for (name, template, fills, glyph, text) in cases {
        let mut empty = MathEditor::new();
        empty.apply_template(template);
        let (shown, _) = empty.display();
        assert!(
            shown.contains(glyph),
            "{name}: typeset display, got {shown}"
        );
        assert!(!shown.contains(", ■"), "{name}: no call syntax in {shown}");
        assert!(shown.contains('■'), "{name}: empty placeholders in {shown}");
        let ed = filled(template, fills);
        assert_eq!(ed.to_text(), text, "{name}");
        assert!(parse_math(&ed.to_text()).is_ok(), "{name} parses");
    }
}

#[test]
fn simple_operator_templates_insert_structures() {
    let ed = filled(Template::Abs, &["x"]);
    assert_eq!(ed.to_text(), "|x|");
    let mut ed = MathEditor::new();
    typed(&mut ed, "n");
    ed.apply_template(Template::Factorial);
    assert_eq!(ed.to_text(), "n!");
    let mut ed = MathEditor::new();
    typed(&mut ed, "1");
    ed.apply_template(Template::Range);
    typed(&mut ed, "5");
    assert_eq!(ed.to_text(), "1..5");
    let mut ed = MathEditor::new();
    typed(&mut ed, "a");
    ed.apply_template(Template::Fraction);
    typed(&mut ed, "b");
    assert_eq!(ed.to_text(), "a/b");
}

#[test]
fn symbolic_arrow_is_one_typeset_token() {
    let mut ed = MathEditor::new();
    typed(&mut ed, "e:=4+2->");
    assert_eq!(ed.to_text(), "e:=4+2->");
    assert!(ed.display().0.contains('→'));
    assert!(!ed.display().0.contains("->"));
    ed.backspace();
    assert_eq!(ed.to_text(), "e:=4+2");
    // The ribbon arrow, then a keyword from the dropdown.
    let mut ed = MathEditor::new();
    typed(&mut ed, "x^2");
    ed.apply_template(Template::Symbolic);
    ed.insert_text("simplify");
    assert!(parse_math(&ed.to_text()).is_ok(), "{}", ed.to_text());
    // A keyword without an arrow adds it.
    let mut ed = MathEditor::new();
    typed(&mut ed, "x+x");
    ed.insert_text("simplify");
    assert_eq!(ed.to_text(), "x+x-> simplify");
}

/// A region typed with the arrow is a symbolic evaluation and shows a symbolic result.
#[test]
fn symbolic_arrow_region_evaluates_to_a_symbolic_result() {
    use super::session::{EditKey, EditSession};
    use crate::viewmodel::Body;
    use mathcraft_import::regions::{ImportReport, Settings, Worksheet};
    use mathcraft_import::results::ResultTable;
    let ws = Worksheet {
        regions: Vec::new(),
        settings: Settings::default(),
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer: Vec::new(),
    };
    let table = ResultTable {
        by_id: Default::default(),
    };
    let mut s = EditSession::new(super::document::Document::from_worksheet("t", ws, table));
    s.click([10.0, 10.0], None);
    for c in "x+x->".chars() {
        s.type_char(c);
    }
    s.key(EditKey::Enter);
    let vm = crate::load::evaluate_document(s.document());
    let shown = vm.items.iter().any(
        |item| matches!(&item.body, Body::Math { result: Some(line), .. } if line.math.is_some()),
    );
    assert!(shown, "symbolic result laid out beside the region");
}

/// A new region's box has its bottom-left corner at the snapped click point: the stored top is
/// one grid row above it, and the crosshair stays at the point.
#[test]
fn a_new_region_sits_in_the_row_above_the_snapped_click() {
    use super::session::{EditKey, EditSession};
    use mathcraft_import::regions::{ImportReport, Settings, Worksheet};
    use mathcraft_import::results::ResultTable;
    let ws = Worksheet {
        regions: Vec::new(),
        settings: Settings::default(),
        page: Default::default(),
        report: ImportReport::default(),
        header: Vec::new(),
        footer: Vec::new(),
    };
    let table = ResultTable {
        by_id: Default::default(),
    };
    let mut s = EditSession::new(super::document::Document::from_worksheet("t", ws, table));
    let grid = f64::from(crate::canvas::grid_points(&Default::default()));
    let at = crate::canvas::snap_to_grid([grid * 3.0 + 4.0, grid * 5.0 - 3.0], grid as f32);
    s.click(at, None);
    assert_eq!(s.crosshair(), Some(at));
    s.type_char('x');
    // The box is centred in its grid row: a one-line formula is 12.24 pt high.
    let centred = at[1] - grid + (grid - 12.24) / 2.0;
    assert_eq!(s.draft_view().expect("draft").at, [at[0], centred]);
    s.key(EditKey::Enter);
    let region = s.document().regions().last().expect("region").clone();
    assert_eq!(region.pos, Some((at[0], centred)));
}

/// In the first row the box cannot go above the margin box.
#[test]
fn a_click_in_the_first_row_clamps_to_the_top() {
    assert_eq!(super::session::box_top(0.0, 5.0, 14.0), 0.0);
    // A box is centred in its grid rows: a 5 pt box in one 14 pt row is 4.5 pt down from the row top.
    assert_eq!(super::session::box_top(14.0, 5.0, 14.0), 4.5);
    // 30 pt is two rows (28 pt) and a little over: centred, it starts 1 pt above the first row.
    assert_eq!(super::session::box_top(70.0, 30.0, 14.0), 41.0);
}

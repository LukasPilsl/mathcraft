//! Editing drafts of the operators of the Operators palette: partial derivative, gradient, norm,
//! cross product and the factorial. Each is typed through its template, shown typeset, written as
//! the text the parser reads, and given its structure back when the region is reopened.

use super::math::{parse_math, MathEditor, Template};
use crate::draftlayout::DraftTree;
use crate::mathbox::Prim;

fn typed(ed: &mut MathEditor, text: &str) {
    for c in text.chars() {
        ed.key(c);
    }
}

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

fn glyphs(tree: &DraftTree) -> Vec<String> {
    tree.layout(20.0)
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn partial_derivative_template_writes_the_partial_call() {
    let ed = filled(Template::Partial, &["x", "x*y"]);
    assert_eq!(ed.to_text(), "partial(x, x*y)");
    assert!(parse_math(&ed.to_text()).is_ok());
    let ed = filled(Template::NthPartial, &["2", "x", "x^3"]);
    assert_eq!(ed.to_text(), "partial(x, 2, x^3)");
    assert!(parse_math(&ed.to_text()).is_ok());
}

#[test]
fn partial_derivative_is_drawn_with_the_partial_sign() {
    let mut ed = MathEditor::new();
    ed.apply_template(Template::Partial);
    let shown = glyphs(&ed.tree());
    assert_eq!(shown, ["∂", "∂"]);
    let (flat, _) = ed.display();
    assert!(flat.contains("∂/∂"), "{flat}");
}

#[test]
fn gradient_template_with_and_without_variables() {
    let ed = filled(Template::Gradient, &["a, b", "f(a, b)"]);
    assert_eq!(ed.to_text(), "gradient(f(a, b), a, b)");
    assert!(parse_math(&ed.to_text()).is_ok());
    // An empty variable slot writes no placeholder: the function's arguments are the variables.
    let mut ed = MathEditor::new();
    ed.apply_template(Template::Gradient);
    ed.tab(true);
    typed(&mut ed, "f(a, b)");
    assert_eq!(ed.to_text(), "gradient(f(a, b))");
    assert!(parse_math(&ed.to_text()).is_ok());
    let mut empty = MathEditor::new();
    empty.apply_template(Template::Gradient);
    assert_eq!(glyphs(&empty.tree()), ["∇"]);
}

#[test]
fn norm_template_writes_the_double_bars() {
    let ed = filled(Template::Norm, &["v"]);
    assert_eq!(ed.to_text(), "‖v‖");
    let parsed = parse_math(&ed.to_text()).expect("parses");
    assert_eq!(parsed.to_string(), "‖v‖");
    // Drawn as two bars on each side, not as text.
    let rects = ed
        .tree()
        .layout(20.0)
        .prims
        .iter()
        .filter(|p| matches!(p, Prim::Rect { .. }))
        .count();
    assert_eq!(rects, 4);
}

#[test]
fn cross_product_is_typed_between_two_operands() {
    let mut ed = MathEditor::new();
    typed(&mut ed, "u");
    ed.apply_template(Template::Cross);
    typed(&mut ed, "v");
    assert_eq!(ed.to_text(), "u×v");
    assert_eq!(parse_math("u×v").expect("parses").to_string(), "u×v");
}

#[test]
fn factorial_template_follows_its_operand() {
    let mut ed = MathEditor::new();
    typed(&mut ed, "5");
    ed.apply_template(Template::Factorial);
    assert_eq!(ed.to_text(), "5!");
    assert_eq!(parse_math("5!").expect("parses").to_string(), "5!");
}

#[test]
fn reopened_text_gets_the_structure_of_the_new_operators() {
    for text in [
        "partial(x, x*y)",
        "partial(x, 2, x^3)",
        "gradient(f(a, b, c), a, b, c)",
        "gradient(f(a, b, c))",
        "‖v‖",
        "‖[3; 4]‖+1",
    ] {
        let ed = MathEditor::from_text(text);
        assert_eq!(ed.to_text(), text);
        assert!(ed.tree().is_structured(), "{text} stays flat");
    }
}

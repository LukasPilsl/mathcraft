//! German number input in the math editor: a comma after digits is the decimal point, `;` is the
//! argument separator, and English input is unchanged.

use super::math::{parse_math, MathEditor};

/// Types `keys` into a German editor, one character at a time.
fn german(keys: &str) -> MathEditor {
    let mut ed = MathEditor::new().with_decimal_comma(true);
    for c in keys.chars() {
        ed.key(c);
    }
    ed
}

/// Types `keys` into an English editor.
fn english(keys: &str) -> MathEditor {
    let mut ed = MathEditor::new();
    for c in keys.chars() {
        ed.key(c);
    }
    ed
}

#[test]
fn a_comma_after_digits_is_the_decimal_point() {
    assert_eq!(german("2,5").to_text(), "2.5");
    assert_eq!(german("12,25+1").to_text(), "12.25+1");
    assert_eq!(german("0,001").to_text(), "0.001");
}

#[test]
fn a_second_comma_in_a_number_is_not_a_second_point() {
    // The number already has a point: the comma stays a separator.
    assert_eq!(german("2,5,3").to_text(), "2.5,3");
}

#[test]
fn a_comma_after_a_name_is_a_separator() {
    // `x2` is a name, not a number, so the comma after it is not a decimal point.
    assert_eq!(german("x2,y").to_text(), "x2,y");
    assert_eq!(german("(a,b)").to_text(), "(a,b)");
}

#[test]
fn a_comma_after_an_operator_or_a_bracket_is_a_separator() {
    assert_eq!(german("2+,").to_text(), "2+,");
    assert_eq!(german("2*,").to_text(), "2*,");
}

#[test]
fn a_semicolon_is_the_argument_separator() {
    assert_eq!(german("f(2;3)").to_text(), "f(2,3)");
    assert_eq!(german("max(1;2;3)").to_text(), "max(1,2,3)");
}

#[test]
fn a_comma_after_an_exponent_is_a_separator() {
    // `2e3` is a number with an exponent; a comma after it has no digits in front of it.
    assert_eq!(german("2e3,5").to_text(), "2e3,5");
}

#[test]
fn a_decimal_comma_reads_as_the_parser_reads_the_point() {
    let typed = german("2,5*4");
    assert_eq!(typed.to_text(), "2.5*4");
    assert!(parse_math(&typed.to_text()).is_ok());
}

#[test]
fn english_input_keeps_the_comma_and_the_point() {
    assert_eq!(english("2,5").to_text(), "2,5");
    assert_eq!(english("2.5").to_text(), "2.5");
    assert_eq!(english("f(2,3)").to_text(), "f(2,3)");
}

#[test]
fn a_semicolon_in_english_is_not_changed() {
    assert_eq!(english("f(2;3)").to_text(), "f(2;3)");
}

#[test]
fn the_default_editor_is_english() {
    // An empty slot is written as the placeholder.
    assert_eq!(MathEditor::new().to_text(), "■");
    assert!(MathEditor::new().with_decimal_comma(true).is_blank());
    assert_eq!(
        MathEditor::from_text("2,5")
            .with_decimal_comma(false)
            .to_text(),
        "2,5"
    );
}

#[test]
fn a_decimal_point_typed_in_german_mode_is_kept() {
    assert_eq!(german("2.5").to_text(), "2.5");
}

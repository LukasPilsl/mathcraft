//! Localised text for the calculation errors the engine reports in English. The engine's messages
//! are the English fallback: a message this file does not know is shown as it is.

use crate::i18n::{language, tr_in, trf_in, Lang};

/// Engine messages with no part that changes, and their keys.
const FIXED: &[(&str, &str)] = &[
    ("division by zero", "err.division_by_zero"),
    ("matrix is singular", "err.singular"),
    ("result is not finite", "err.not_finite"),
    ("arithmetic overflow", "err.overflow"),
];

/// The text of an engine error in the current language.
#[must_use]
pub fn localize_error(english: &str) -> String {
    localize_error_in(language(), english)
}

/// The text of an engine error in `lang`.
#[must_use]
pub fn localize_error_in(lang: Lang, english: &str) -> String {
    for (text, key) in FIXED {
        if english == *text {
            return tr_in(lang, key).to_string();
        }
    }
    if english.starts_with("work limit exceeded") {
        return tr_in(lang, "err.work_limit").to_string();
    }
    if let Some(name) = between(english, "undefined name `", "`") {
        return trf_in(lang, "err.undefined_name", &[name]);
    }
    if let Some(name) = between(english, "undefined function `", "`") {
        return trf_in(lang, "err.undefined_function", &[name]);
    }
    if let Some(rest) = english.strip_prefix("dimension mismatch: ") {
        if let Some((left, right)) = rest.split_once(" and ") {
            return trf_in(lang, "err.dimension", &[left, right]);
        }
    }
    if let Some(rest) = english.strip_prefix("expected a dimensionless value, found ") {
        return trf_in(lang, "err.not_dimensionless", &[rest]);
    }
    english.to_string()
}

/// The text between `start` and `end`, when `text` has both, with `start` first.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(start)?;
    rest.strip_suffix(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_messages_are_shown_as_they_are() {
        assert_eq!(
            localize_error_in(Lang::German, "something new went wrong"),
            "something new went wrong"
        );
    }

    #[test]
    fn english_texts_are_the_engine_texts() {
        assert_eq!(
            localize_error_in(Lang::English, "division by zero"),
            "division by zero"
        );
        assert_eq!(
            localize_error_in(Lang::English, "undefined name `x`"),
            "undefined name `x`"
        );
    }

    #[test]
    fn german_texts_keep_the_names() {
        assert_eq!(
            localize_error_in(Lang::German, "division by zero"),
            "Division durch null"
        );
        assert_eq!(
            localize_error_in(Lang::German, "undefined name `x`"),
            "Name `x` ist nicht definiert"
        );
        assert_eq!(
            localize_error_in(Lang::German, "undefined function `f`"),
            "Funktion `f` ist nicht definiert"
        );
        assert_eq!(
            localize_error_in(Lang::German, "dimension mismatch: m and s"),
            "Dimensionen passen nicht zusammen: m und s"
        );
    }

    #[test]
    fn between_needs_both_ends() {
        assert_eq!(
            between("undefined name `x`", "undefined name `", "`"),
            Some("x")
        );
        assert_eq!(between("other", "undefined name `", "`"), None);
    }
}

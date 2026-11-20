//! Text that the window can draw with the bundled fonts. A character that no bundled face has is
//! replaced by a box (the fallback glyph), and very long tokens are cut with an ellipsis, so that
//! a hostile name in a worksheet cannot make one run of text enormous.
//!
//! The tests in `mathlayout_tests.rs` and `fonts.rs` check that the symbols the layout emits have
//! glyphs in the bundled faces.

use crate::fonts::{covers, Face};

/// Longest token drawn as one run of math text, in characters. Longer names are cut.
pub const MAX_TOKEN_CHARS: usize = 80;
/// Longest result or tooltip text, in characters.
pub const MAX_RESULT_CHARS: usize = 2_000;
/// The mark that ends a cut text.
pub const ELLIPSIS: char = '…';
/// Drawn in place of a character that no bundled face has (U+25A1 WHITE SQUARE, in STIX Two Math).
pub const FALLBACK_BOX: char = '□';

/// The character as drawn in `face`: itself when a bundled face has its glyph, else the box.
pub fn drawable(c: char, face: Face) -> char {
    if c.is_ascii() || covers(face, c) {
        c
    } else {
        FALLBACK_BOX
    }
}

/// The text as drawn: at most `max_chars` characters (the last one an ellipsis when cut), with
/// every character the face lacks replaced by [`FALLBACK_BOX`].
pub fn printable(text: &str, max_chars: usize, face: Face) -> String {
    let mut out = String::with_capacity(text.len().min(max_chars.saturating_mul(3)));
    let cut = text.chars().count() > max_chars;
    let keep = if cut {
        max_chars.saturating_sub(1)
    } else {
        max_chars
    };
    for c in text.chars().take(keep) {
        out.push(drawable(c, face));
    }
    if cut {
        out.push(ELLIPSIS);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_keep_their_real_glyphs() {
        assert_eq!(printable("a → b ≠ c", 80, Face::Roman), "a → b ≠ c");
        assert_eq!(printable("x ≤ y ≥ z", 80, Face::Roman), "x ≤ y ≥ z");
        assert_eq!(printable("1−2", 80, Face::Roman), "1−2");
        assert_eq!(printable("α·β", 80, Face::Italic), "α·β");
    }

    #[test]
    fn a_character_without_a_glyph_becomes_the_box() {
        // A CJK ideograph is in no bundled face.
        assert_eq!(
            printable("a漢b", 80, Face::Roman),
            format!("a{FALLBACK_BOX}b")
        );
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        let long = "x".repeat(1_000_000);
        let cut = printable(&long, MAX_TOKEN_CHARS, Face::Italic);
        assert_eq!(cut.chars().count(), MAX_TOKEN_CHARS);
        assert!(cut.ends_with(ELLIPSIS));
        assert_eq!(printable("abc", 3, Face::Roman), "abc");
        assert_eq!(printable("abcd", 3, Face::Roman), "ab…");
    }

    #[test]
    fn cut_counts_characters_not_bytes() {
        let text = "α".repeat(10);
        assert_eq!(printable(&text, 4, Face::Roman), "ααα…");
    }
}

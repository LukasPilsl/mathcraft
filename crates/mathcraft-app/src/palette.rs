//! The symbol palette and the Greek letter chord. Ctrl+G arms the chord; the next letter types
//! the Greek letter with that Latin key (Shift for the capital). The symbols insert text the
//! parser reads, or a template, or are greyed out with the reason. Nothing here draws.

use crate::edit::math::Template;

/// Latin key, lower-case Greek letter, capital Greek letter, in the usual Greek keyboard order.
pub const GREEK: &[(char, char, char)] = &[
    ('a', 'α', 'Α'),
    ('b', 'β', 'Β'),
    ('g', 'γ', 'Γ'),
    ('d', 'δ', 'Δ'),
    ('e', 'ε', 'Ε'),
    ('z', 'ζ', 'Ζ'),
    ('h', 'η', 'Η'),
    ('q', 'θ', 'Θ'),
    ('i', 'ι', 'Ι'),
    ('k', 'κ', 'Κ'),
    ('l', 'λ', 'Λ'),
    ('m', 'μ', 'Μ'),
    ('n', 'ν', 'Ν'),
    ('x', 'ξ', 'Ξ'),
    ('o', 'ο', 'Ο'),
    ('p', 'π', 'Π'),
    ('r', 'ρ', 'Ρ'),
    ('s', 'σ', 'Σ'),
    ('t', 'τ', 'Τ'),
    ('u', 'υ', 'Υ'),
    ('f', 'φ', 'Φ'),
    ('c', 'χ', 'Χ'),
    ('y', 'ψ', 'Ψ'),
    ('w', 'ω', 'Ω'),
];

/// The Greek letter that a Latin key stands for (capital for an upper-case key), if any.
pub fn greek_for(key: char) -> Option<char> {
    let lower = key.to_ascii_lowercase();
    GREEK
        .iter()
        .find(|(k, _, _)| *k == lower)
        .map(|(_, small, big)| {
            if key.is_ascii_uppercase() {
                *big
            } else {
                *small
            }
        })
}

/// The Ctrl+G chord. Armed by Ctrl+G; the next typed character is the key of a Greek letter (or
/// is typed as it is when no letter stands for it). Any other key disarms it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GreekChord {
    armed: bool,
}

impl GreekChord {
    pub fn arm(&mut self) {
        self.armed = true;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// The character to type for a typed `c`. Disarms the chord.
    pub fn type_for(&mut self, c: char) -> char {
        if !self.armed {
            return c;
        }
        self.armed = false;
        greek_for(c).unwrap_or(c)
    }
}

/// What a palette entry does when chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolInsert {
    /// Typed into the math draft as these characters.
    Text(&'static str),
    /// A template of the math editor (structures or calculus functions).
    Template(Template),
    /// Greyed out; the string says why.
    Unavailable(&'static str),
}

/// One entry of the symbol palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    pub label: &'static str,
    pub insert: SymbolInsert,
}

/// The constants and operators palette, in the order the dialog shows them.
pub const SYMBOLS: &[Symbol] = &[
    Symbol {
        label: "π",
        insert: SymbolInsert::Text("π"),
    },
    Symbol {
        label: "e",
        insert: SymbolInsert::Text("e"),
    },
    Symbol {
        label: "i",
        insert: SymbolInsert::Text("i"),
    },
    Symbol {
        label: "∞",
        insert: SymbolInsert::Text("∞"),
    },
    Symbol {
        label: "=",
        insert: SymbolInsert::Template(Template::Evaluate),
    },
    Symbol {
        label: ":=",
        insert: SymbolInsert::Template(Template::Define),
    },
    Symbol {
        label: "≤",
        insert: SymbolInsert::Text("<="),
    },
    Symbol {
        label: "≥",
        insert: SymbolInsert::Text(">="),
    },
    Symbol {
        label: "≠",
        insert: SymbolInsert::Text("<>"),
    },
    Symbol {
        label: "·",
        insert: SymbolInsert::Text("*"),
    },
    Symbol {
        label: "÷",
        insert: SymbolInsert::Template(Template::Fraction),
    },
    Symbol {
        label: "xʸ",
        insert: SymbolInsert::Template(Template::Power),
    },
    Symbol {
        label: "√",
        insert: SymbolInsert::Template(Template::SquareRoot),
    },
    Symbol {
        label: "x₀",
        insert: SymbolInsert::Template(Template::Subscript),
    },
    Symbol {
        label: "Σ",
        insert: SymbolInsert::Template(Template::Sum),
    },
    Symbol {
        label: "Π",
        insert: SymbolInsert::Template(Template::Product),
    },
    Symbol {
        label: "∫",
        insert: SymbolInsert::Template(Template::Integral),
    },
    Symbol {
        label: "d/dx",
        insert: SymbolInsert::Template(Template::Derivative),
    },
    Symbol {
        label: "×",
        insert: SymbolInsert::Template(Template::Cross),
    },
    Symbol {
        label: "|x|",
        insert: SymbolInsert::Template(Template::Abs),
    },
    Symbol {
        label: "∈",
        insert: SymbolInsert::Unavailable("pal.member"),
    },
    Symbol {
        label: "∂",
        insert: SymbolInsert::Template(Template::Partial),
    },
    Symbol {
        label: "∧ ∨ ¬",
        insert: SymbolInsert::Unavailable("pal.logic"),
    },
];

/// The Greek letters the palette shows: lower case first, then capitals.
pub fn greek_letters() -> Vec<(char, char)> {
    let lower = GREEK.iter().map(|(_, s, _)| (*s, *s));
    let upper = GREEK.iter().map(|(_, _, b)| (*b, *b));
    lower.chain(upper).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_types_the_greek_letter_for_its_key() {
        let mut chord = GreekChord::default();
        chord.arm();
        assert_eq!(chord.type_for('p'), 'π');
        assert_eq!(chord.type_for('p'), 'p');
    }

    #[test]
    fn shift_gives_the_capital() {
        assert_eq!(greek_for('W'), Some('Ω'));
        assert_eq!(greek_for('w'), Some('ω'));
        assert_eq!(greek_for('S'), Some('Σ'));
    }

    #[test]
    fn a_key_without_a_letter_is_typed_as_it_is() {
        let mut chord = GreekChord::default();
        chord.arm();
        assert_eq!(chord.type_for('1'), '1');
        assert_eq!(chord.type_for('a'), 'a');
    }

    #[test]
    fn unarmed_keys_are_plain() {
        let mut chord = GreekChord::default();
        assert_eq!(chord.type_for('a'), 'a');
    }

    #[test]
    fn every_greek_letter_is_listed_once() {
        let letters = greek_letters();
        assert_eq!(letters.len(), GREEK.len() * 2);
    }

    #[test]
    fn unavailable_entries_say_not_implemented_yet() {
        let mut greyed = 0;
        for symbol in SYMBOLS {
            if let SymbolInsert::Unavailable(reason) = symbol.insert {
                greyed += 1;
                assert!(reason.starts_with("pal."), "{reason}");
            }
        }
        assert!(greyed > 0);
    }
}

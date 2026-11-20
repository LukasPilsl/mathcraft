//! Keyboard shortcuts as commands. The window turns a key press with Ctrl (or Command) into a
//! letter and a Shift flag, and asks [`shortcut`] what it means. Nothing here knows about egui.

use super::math::Template;

/// A command the shortcuts can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Undo,
    Redo,
    Save,
    SaveAs,
    Open,
    New,
    SelectAll,
    SquareRoot,
    /// Insert Function dialog (Prime's Ctrl+E).
    InsertFunction,
    /// Insert Unit dialog (Ctrl+U).
    InsertUnit,
    /// Arms the Greek letter chord: the next letter key types a Greek letter.
    GreekPrefix,
    /// Ctrl+M: a one-element matrix (Prime's Matrix operator shortcut).
    Matrix,
    /// Ctrl+.: the symbolic evaluation arrow `->`.
    SymbolicEvaluate,
    /// Ctrl+I: Insert Image (file dialog, then the insertion point).
    InsertImage,
    /// Ctrl+Shift+A: an empty area at the insertion point.
    InsertArea,
    /// A math operator template: Ctrl+Shift+D (derivative), Ctrl+Shift+I (integral), Ctrl+Shift+$
    /// (summation) and Ctrl+Shift+# (product), as Prime's calculus table lists them.
    MathTemplate(Template),
    /// Ctrl+': the prime mark, as in f'(t).
    Prime,
    /// Ctrl+2: an empty x-y plot at the insertion point (`@` types the same).
    InsertXyPlot,
    /// Ctrl+4: an empty polar plot at the insertion point (help `to_create_a_polar_plot.md`).
    InsertPolarPlot,
}

/// The command for Ctrl+`letter` (with Shift when `shift` is set), or `None` when the key is not
/// a shortcut. Ctrl+Z and Ctrl+Shift+Z undo and redo; Ctrl+Y also redoes.
pub fn shortcut(letter: char, shift: bool) -> Option<Shortcut> {
    Some(match (letter.to_ascii_lowercase(), shift) {
        ('z', false) => Shortcut::Undo,
        ('z', true) | ('y', false) => Shortcut::Redo,
        ('s', false) => Shortcut::Save,
        ('s', true) => Shortcut::SaveAs,
        ('o', false) => Shortcut::Open,
        ('n', false) => Shortcut::New,
        ('a', false) => Shortcut::SelectAll,
        ('r', false) => Shortcut::SquareRoot,
        ('e', false) => Shortcut::InsertFunction,
        ('u', false) => Shortcut::InsertUnit,
        ('g', false) => Shortcut::GreekPrefix,
        ('m', false) => Shortcut::Matrix,
        ('.', false) => Shortcut::SymbolicEvaluate,
        ('i', false) => Shortcut::InsertImage,
        ('a', true) => Shortcut::InsertArea,
        ('d', true) => Shortcut::MathTemplate(Template::Derivative),
        ('i', true) => Shortcut::MathTemplate(Template::Integral),
        ('$', true) => Shortcut::MathTemplate(Template::Sum),
        ('#', true) => Shortcut::MathTemplate(Template::Product),
        ('l', false) => Shortcut::MathTemplate(Template::Limit),
        ('d', false) => Shortcut::MathTemplate(Template::Partial),
        ('g', true) => Shortcut::MathTemplate(Template::Gradient),
        ('8', false) => Shortcut::MathTemplate(Template::Cross),
        ('\'', false) => Shortcut::Prime,
        ('2', false) => Shortcut::InsertXyPlot,
        ('4', false) => Shortcut::InsertPolarPlot,
        _ => return None,
    })
}

/// The shortcuts the Help > Keyboard shortcuts window lists: the keys, then the key of what they
/// do (an `i18n` string). Keys that the window does not handle are not listed.
pub const DOCUMENTED: &[(&str, &str)] = &[
    ("Ctrl+N", "sc.new"),
    ("Ctrl+O", "sc.open"),
    ("Ctrl+S", "sc.save"),
    ("Ctrl+Shift+S", "sc.save_as"),
    ("Ctrl+Z", "sc.undo"),
    ("Ctrl+Shift+Z or Ctrl+Y", "sc.redo"),
    ("Ctrl+A", "sc.select_all"),
    ("Ctrl+R", "sc.sqrt"),
    ("\\", "sc.sqrt_key"),
    ("Ctrl+Shift+D", "sc.derivative"),
    ("Ctrl+Shift+I", "sc.integral"),
    ("Ctrl+Shift+$", "sc.sum"),
    ("Ctrl+Shift+#", "sc.product"),
    ("Ctrl+D", "sc.partial"),
    ("Ctrl+Shift+G", "sc.gradient"),
    ("Ctrl+8", "sc.cross"),
    ("Ctrl+'", "sc.prime"),
    ("Ctrl+L", "sc.limit"),
    ("Ctrl+E", "sc.function"),
    ("Ctrl+U", "sc.unit"),
    ("Ctrl+I", "sc.image"),
    ("Ctrl+Shift+A", "sc.area"),
    ("Ctrl+2 or @", "sc.xy_plot"),
    ("Ctrl+4", "sc.polar_plot"),
    ("Ctrl+G, then a letter", "sc.greek"),
    ("Ctrl+0", "sc.zoom100"),
    ("Ctrl+mouse wheel or pinch", "sc.zoom"),
    ("F9", "sc.calculate"),
    ("Enter", "sc.enter"),
    ("Escape", "sc.escape"),
    ("Delete", "sc.delete"),
    ("Arrow keys", "sc.arrows"),
    ("Ctrl+Left or Ctrl+Right", "sc.term"),
    ("Shift+Enter in a matrix", "sc.matrix_row"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_keys() {
        assert_eq!(shortcut('z', false), Some(Shortcut::Undo));
        assert_eq!(shortcut('z', true), Some(Shortcut::Redo));
        assert_eq!(shortcut('y', false), Some(Shortcut::Redo));
        assert_eq!(shortcut('y', true), None);
    }

    #[test]
    fn file_keys() {
        assert_eq!(shortcut('s', false), Some(Shortcut::Save));
        assert_eq!(shortcut('s', true), Some(Shortcut::SaveAs));
        assert_eq!(shortcut('o', false), Some(Shortcut::Open));
        assert_eq!(shortcut('n', false), Some(Shortcut::New));
    }

    #[test]
    fn select_all_and_square_root() {
        assert_eq!(shortcut('a', false), Some(Shortcut::SelectAll));
        assert_eq!(shortcut('r', false), Some(Shortcut::SquareRoot));
    }

    #[test]
    fn insert_and_greek_keys() {
        assert_eq!(shortcut('e', false), Some(Shortcut::InsertFunction));
        assert_eq!(shortcut('u', false), Some(Shortcut::InsertUnit));
        assert_eq!(shortcut('g', false), Some(Shortcut::GreekPrefix));
        assert_eq!(
            shortcut('g', true),
            Some(Shortcut::MathTemplate(Template::Gradient))
        );
    }

    #[test]
    fn image_and_area_keys() {
        assert_eq!(shortcut('i', false), Some(Shortcut::InsertImage));
        assert_eq!(
            shortcut('i', true),
            Some(Shortcut::MathTemplate(Template::Integral))
        );
        assert_eq!(shortcut('a', true), Some(Shortcut::InsertArea));
        assert_eq!(shortcut('a', false), Some(Shortcut::SelectAll));
    }

    #[test]
    fn calculus_and_sum_operator_keys_insert_their_templates() {
        assert_eq!(
            shortcut('d', true),
            Some(Shortcut::MathTemplate(Template::Derivative))
        );
        assert_eq!(
            shortcut('$', true),
            Some(Shortcut::MathTemplate(Template::Sum))
        );
        assert_eq!(
            shortcut('#', true),
            Some(Shortcut::MathTemplate(Template::Product))
        );
        // Without Shift the letter is the partial derivative (Prime's Ctrl+D).
        assert_eq!(
            shortcut('d', false),
            Some(Shortcut::MathTemplate(Template::Partial))
        );
        assert_eq!(
            shortcut('g', true),
            Some(Shortcut::MathTemplate(Template::Gradient))
        );
        assert_eq!(
            shortcut('8', false),
            Some(Shortcut::MathTemplate(Template::Cross))
        );
        assert_eq!(
            shortcut('l', false),
            Some(Shortcut::MathTemplate(Template::Limit))
        );
        assert_eq!(shortcut('$', false), None);
    }

    #[test]
    fn apostrophe_is_the_prime_mark() {
        assert_eq!(shortcut('\'', false), Some(Shortcut::Prime));
        assert_eq!(shortcut('\'', true), None);
    }

    #[test]
    fn letters_are_case_insensitive_and_other_keys_are_not_shortcuts() {
        assert_eq!(shortcut('Z', false), Some(Shortcut::Undo));
        assert_eq!(shortcut('q', false), None);
        assert_eq!(shortcut('1', false), None);
        assert_eq!(shortcut('2', false), Some(Shortcut::InsertXyPlot));
        assert_eq!(shortcut('4', false), Some(Shortcut::InsertPolarPlot));
    }

    #[test]
    fn documented_list_mentions_each_new_shortcut() {
        let keys: Vec<&str> = DOCUMENTED.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Ctrl+E"));
        assert!(keys.contains(&"Ctrl+U"));
        assert!(keys.iter().any(|k| k.starts_with("Ctrl+G")));
        assert!(keys.contains(&"F9"));
        assert!(keys.contains(&"Ctrl+I"));
        assert!(keys.contains(&"Ctrl+Shift+A"));
    }
}

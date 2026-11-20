//! Plain text editing for text regions: a string with a caret and an optional selection. Paragraphs
//! are lines joined with `\n`, as the import stores them. No formatting and no undo here (the
//! document's history covers the whole edit). No egui types here.

/// A text being edited. The caret is a character index in `0..=len`. The selection runs from the
/// anchor to the caret (either way round); it is empty when the anchor is `None` or equals the
/// caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditor {
    chars: Vec<char>,
    caret: usize,
    anchor: Option<usize>,
}

impl TextEditor {
    /// Edits `text`, with the caret at the end. `\r` is dropped.
    pub fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().filter(|c| *c != '\r').collect();
        let caret = chars.len();
        Self {
            chars,
            caret,
            anchor: None,
        }
    }

    /// The selected character range, `(start, end)` with `start < end`, or `None` when nothing is
    /// selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }

    /// Removes the selected characters and puts the caret where they were. `false` when nothing
    /// was selected.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.chars.drain(start..end);
        self.caret = start;
        self.anchor = None;
        true
    }

    /// Moves the caret one character left, extending the selection (Shift+Left).
    pub fn select_left(&mut self) {
        self.anchor.get_or_insert(self.caret);
        self.caret = self.caret.saturating_sub(1);
    }

    /// Moves the caret one character right, extending the selection (Shift+Right).
    pub fn select_right(&mut self) {
        self.anchor.get_or_insert(self.caret);
        if self.caret < self.chars.len() {
            self.caret += 1;
        }
    }

    /// The text as it is now.
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// The caret as a character index.
    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Inserts a character at the caret. Control characters other than newline are ignored, so
    /// stray key codes do not end up in the text.
    pub fn insert(&mut self, c: char) {
        if c.is_control() && c != '\n' {
            return;
        }
        self.delete_selection();
        self.chars.insert(self.caret, c);
        self.caret += 1;
    }

    /// Deletes the selection, or the character before the caret.
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.caret > 0 {
            self.caret -= 1;
            self.chars.remove(self.caret);
        }
    }

    /// Deletes the selection, or the character after the caret.
    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.caret < self.chars.len() {
            self.chars.remove(self.caret);
        }
    }

    /// Moves the caret one character left. A selection collapses to its start.
    pub fn left(&mut self) {
        if let Some((start, _)) = self.selection() {
            self.caret = start;
        } else {
            self.caret = self.caret.saturating_sub(1);
        }
        self.anchor = None;
    }

    /// Moves the caret one character right. A selection collapses to its end.
    pub fn right(&mut self) {
        if let Some((_, end)) = self.selection() {
            self.caret = end;
        } else if self.caret < self.chars.len() {
            self.caret += 1;
        }
        self.anchor = None;
    }

    /// Moves the caret to the start of its line.
    pub fn home(&mut self) {
        self.anchor = None;
        while self.caret > 0 && self.chars[self.caret - 1] != '\n' {
            self.caret -= 1;
        }
    }

    /// Moves the caret to the end of its line.
    pub fn end(&mut self) {
        self.anchor = None;
        while self.caret < self.chars.len() && self.chars[self.caret] != '\n' {
            self.caret += 1;
        }
    }

    /// Puts the caret at character `index` (clamped to the text) and clears the selection. A mouse
    /// press in the text does this.
    pub fn place_caret(&mut self, index: usize) {
        self.caret = index.min(self.chars.len());
        self.anchor = None;
    }

    /// Moves the caret to character `index` and extends the selection from where the drag began
    /// (the selection's anchor is kept). A mouse drag in the text does this.
    pub fn extend_to(&mut self, index: usize) {
        self.anchor.get_or_insert(self.caret);
        self.caret = index.min(self.chars.len());
    }

    /// Selects the word under character `index`: the run of letters, digits and underscores around
    /// it. On a space or a mark, the caret is placed there and nothing is selected.
    pub fn select_word_at(&mut self, index: usize) {
        let index = index.min(self.chars.len());
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = index;
        while start > 0 && is_word(self.chars[start - 1]) {
            start -= 1;
        }
        let mut end = index;
        while end < self.chars.len() && is_word(self.chars[end]) {
            end += 1;
        }
        if start < end {
            self.anchor = Some(start);
            self.caret = end;
        } else {
            self.place_caret(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_appends_at_the_caret() {
        let mut t = TextEditor::new("");
        for c in "Hello".chars() {
            t.insert(c);
        }
        assert_eq!(t.text(), "Hello");
        assert_eq!(t.caret(), 5);
    }

    #[test]
    fn caret_moves_and_inserts_in_the_middle() {
        let mut t = TextEditor::new("ac");
        t.left();
        t.insert('b');
        assert_eq!(t.text(), "abc");
        t.home();
        assert_eq!(t.caret(), 0);
        t.end();
        assert_eq!(t.caret(), 3);
        t.right();
        assert_eq!(t.caret(), 3);
    }

    #[test]
    fn backspace_and_delete() {
        let mut t = TextEditor::new("abcd");
        t.left();
        t.backspace();
        assert_eq!(t.text(), "abd");
        t.delete();
        assert_eq!(t.text(), "ab");
        t.backspace();
        t.backspace();
        t.backspace();
        assert_eq!(t.text(), "");
        assert!(t.is_empty());
    }

    #[test]
    fn newlines_are_kept_and_home_end_work_per_line() {
        let mut t = TextEditor::new("one\ntwo");
        t.insert('\n');
        assert_eq!(t.text(), "one\ntwo\n");
        t.home();
        assert_eq!(t.caret(), 8);
        let mut u = TextEditor::new("one\ntwo");
        u.home();
        assert_eq!(u.caret(), 4);
        u.end();
        assert_eq!(u.caret(), 7);
    }

    #[test]
    fn control_characters_are_ignored() {
        let mut t = TextEditor::new("a");
        t.insert('\u{7}');
        assert_eq!(t.text(), "a");
    }

    #[test]
    fn shift_arrows_select_and_typing_replaces_the_selection() {
        let mut t = TextEditor::new("abcdef");
        t.home();
        t.right();
        t.select_right();
        t.select_right();
        assert_eq!(t.selection(), Some((1, 3)));
        t.insert('X');
        assert_eq!(t.text(), "aXdef");
        assert_eq!(t.caret(), 2);
        assert_eq!(t.selection(), None);
    }

    #[test]
    fn a_selection_made_backwards_and_plain_moves_collapse_it() {
        let mut t = TextEditor::new("abcdef");
        t.select_left();
        t.select_left();
        assert_eq!(t.selection(), Some((4, 6)));
        t.left();
        assert_eq!(t.caret(), 4);
        assert_eq!(t.selection(), None);
        t.select_left();
        t.backspace();
        assert_eq!(t.text(), "abcef");
        t.select_right();
        t.end();
        assert_eq!(t.selection(), None);
    }

    #[test]
    fn unicode_is_counted_in_characters() {
        let mut t = TextEditor::new("Größe");
        assert_eq!(t.caret(), 5);
        t.backspace();
        assert_eq!(t.text(), "Größ");
    }

    #[test]
    fn a_press_places_the_caret_and_a_drag_selects_from_the_press() {
        let mut t = TextEditor::new("abcdef");
        t.place_caret(2);
        assert_eq!((t.caret(), t.selection()), (2, None));
        t.extend_to(5);
        assert_eq!(t.selection(), Some((2, 5)));
        // Dragging back past the press reverses the selection, as a mouse does.
        t.extend_to(0);
        assert_eq!(t.selection(), Some((0, 2)));
        t.place_caret(99);
        assert_eq!(t.caret(), 6, "clamped to the text");
    }

    #[test]
    fn a_double_click_selects_the_word_under_it() {
        let mut t = TextEditor::new("alpha beta, gamma");
        t.select_word_at(7);
        assert_eq!(t.selection(), Some((6, 10)));
        t.insert('X');
        assert_eq!(t.text(), "alpha X, gamma");
        // On a mark there is no word: only the caret moves.
        let mut t = TextEditor::new("a, b");
        t.select_word_at(2);
        assert_eq!((t.caret(), t.selection()), (2, None));
    }
}

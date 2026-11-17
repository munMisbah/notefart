use std::collections::VecDeque;
use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldView<'a> {
    pub text: &'a str,
    pub cursor: usize,
    pub selection: Option<Range<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldKind {
    Path,
    Query,
    Replacement,
}

const HISTORY_TRANSACTIONS: usize = 128;
const HISTORY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    before: (usize, Option<usize>),
    after: (usize, Option<usize>),
}

impl Edit {
    fn payload(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    revision: u64,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    history_bytes: usize,
}

impl Field {
    pub(crate) fn new(text: String) -> Self {
        let cursor = text.len();
        Self {
            text,
            cursor,
            anchor: None,
            revision: 0,
            undo: VecDeque::new(),
            redo: Vec::new(),
            history_bytes: 0,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn view(&self) -> FieldView<'_> {
        FieldView {
            text: &self.text,
            cursor: self.cursor,
            selection: self.selection(),
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn selected(&self) -> Option<&str> {
        self.selection().map(|range| &self.text[range])
    }

    pub(crate) fn selection(&self) -> Option<Range<usize>> {
        self.anchor
            .filter(|&anchor| anchor != self.cursor)
            .map(|anchor| anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn clear(&mut self) {
        self.replace(0..self.text.len(), "");
    }

    pub(crate) fn insert(&mut self, text: &str, limit: usize) -> bool {
        let sel = self.selection();
        let sel_len = sel.as_ref().map_or(0, |r| r.len());
        let new_len = self
            .text
            .len()
            .saturating_sub(sel_len)
            .saturating_add(text.len());
        if new_len > limit {
            return false;
        }

        let range = sel.unwrap_or(self.cursor..self.cursor);
        self.replace(range, text);
        true
    }

    fn replace(&mut self, range: Range<usize>, text: &str) {
        if &self.text[range.clone()] == text {
            if !range.is_empty() {
                self.cursor = range.end;
                self.anchor = None;
            }
            return;
        }
        let mut edit = Edit {
            start: range.start,
            removed: self.text[range.clone()].to_owned(),
            inserted: text.to_owned(),
            before: (self.cursor, self.anchor),
            after: (0, None),
        };
        self.text.replace_range(range, text);
        self.cursor = edit.start + text.len();
        self.anchor = None;
        self.clamp_cursor_and_anchor();
        edit.after = (self.cursor, self.anchor);
        self.revision = self.revision.saturating_add(1);

        for discarded in self.redo.drain(..) {
            self.history_bytes -= discarded.payload();
        }
        let bytes = edit.payload();
        if bytes > HISTORY_BYTES {
            self.undo.clear();
            self.history_bytes = 0;
            return;
        }
        while self.undo.len() >= HISTORY_TRANSACTIONS || self.history_bytes + bytes > HISTORY_BYTES
        {
            if let Some(oldest) = self.undo.pop_front() {
                self.history_bytes -= oldest.payload();
            }
        }
        self.history_bytes += bytes;
        self.undo.push_back(edit);
    }

    fn undo(&mut self) {
        if let Some(edit) = self.undo.pop_back() {
            self.text
                .replace_range(edit.start..edit.start + edit.inserted.len(), &edit.removed);
            (self.cursor, self.anchor) = edit.before;
            self.revision = self.revision.saturating_add(1);
            self.redo.push(edit);
        }
    }

    fn redo(&mut self) {
        if let Some(edit) = self.redo.pop() {
            self.text
                .replace_range(edit.start..edit.start + edit.removed.len(), &edit.inserted);
            (self.cursor, self.anchor) = edit.after;
            self.revision = self.revision.saturating_add(1);
            self.undo.push_back(edit);
        }
    }

    fn delete(&mut self, backward: bool, word: bool) {
        let range = self.selection().unwrap_or_else(|| {
            if backward {
                let start = if word {
                    prev_word_boundary(&self.text, self.cursor)
                } else {
                    self.text[..self.cursor]
                        .grapheme_indices(true)
                        .next_back()
                        .map_or(0, |(i, _)| i)
                };
                start..self.cursor
            } else {
                let end = if word {
                    next_word_boundary(&self.text, self.cursor)
                } else {
                    self.text[self.cursor..]
                        .graphemes(true)
                        .next()
                        .map_or(self.cursor, |g| self.cursor + g.len())
                };
                self.cursor..end
            }
        });
        self.replace(range, "");
    }

    pub(crate) fn move_to(&mut self, byte: usize, extend: bool) {
        let snapped = snap_to_grapheme_boundary(&self.text, byte);
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = snapped;
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if alt {
            return false;
        }

        if control {
            match key.code {
                KeyCode::Char('z') | KeyCode::Char('Z') => {
                    if shift {
                        self.redo();
                    } else {
                        self.undo();
                    }
                    return true;
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.redo();
                    return true;
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    self.delete(key.code == KeyCode::Backspace, true);
                    return true;
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    self.anchor = Some(0);
                    self.cursor = self.text.len();
                    return true;
                }
                KeyCode::Char('u') | KeyCode::Char('U') => {
                    self.clear();
                    return true;
                }
                KeyCode::Left => {
                    if !shift && let Some(range) = self.selection() {
                        self.cursor = range.start;
                        self.anchor = None;
                    } else {
                        let next_pos = prev_word_boundary(&self.text, self.cursor);
                        self.move_to(next_pos, shift);
                    }
                    return true;
                }
                KeyCode::Right => {
                    if !shift && let Some(range) = self.selection() {
                        self.cursor = range.end;
                        self.anchor = None;
                    } else {
                        let next_pos = next_word_boundary(&self.text, self.cursor);
                        self.move_to(next_pos, shift);
                    }
                    return true;
                }
                KeyCode::Home => {
                    self.move_to(0, shift);
                    return true;
                }
                KeyCode::End => {
                    let len = self.text.len();
                    self.move_to(len, shift);
                    return true;
                }
                _ => return false,
            }
        }

        match key.code {
            KeyCode::Left => {
                if !shift && let Some(range) = self.selection() {
                    self.cursor = range.start;
                    self.anchor = None;
                } else {
                    let prev = self.text[..self.cursor]
                        .grapheme_indices(true)
                        .next_back()
                        .map_or(0, |(idx, _)| idx);
                    self.move_to(prev, shift);
                }
                true
            }
            KeyCode::Right => {
                if !shift && let Some(range) = self.selection() {
                    self.cursor = range.end;
                    self.anchor = None;
                } else {
                    let next = self.text[self.cursor..]
                        .grapheme_indices(true)
                        .next()
                        .map_or(self.text.len(), |(_, g)| self.cursor + g.len());
                    self.move_to(next, shift);
                }
                true
            }
            KeyCode::Home => {
                self.move_to(0, shift);
                true
            }
            KeyCode::End => {
                let len = self.text.len();
                self.move_to(len, shift);
                true
            }
            KeyCode::Backspace | KeyCode::Delete => {
                self.delete(key.code == KeyCode::Backspace, false);
                true
            }
            _ => false,
        }
    }

    fn clamp_cursor_and_anchor(&mut self) {
        self.cursor = snap_forward(&self.text, self.cursor);
        if let Some(anchor) = self.anchor {
            self.anchor = Some(snap_to_grapheme_boundary(
                &self.text,
                anchor.min(self.text.len()),
            ));
        }
    }
}

impl From<String> for Field {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for Field {
    fn from(s: &str) -> Self {
        Self::new(s.to_string())
    }
}

fn snap_to_grapheme_boundary(text: &str, byte: usize) -> usize {
    if byte >= text.len() {
        return text.len();
    }
    if byte == 0 {
        return 0;
    }
    let mut prev = 0;
    for (idx, _) in text.grapheme_indices(true) {
        if idx == byte {
            return idx;
        }
        if idx > byte {
            return if byte - prev <= idx - byte { prev } else { idx };
        }
        prev = idx;
    }
    let last = text.len();
    if byte - prev <= last - byte {
        prev
    } else {
        last
    }
}

fn snap_forward(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|&index| index >= byte)
        .unwrap_or(text.len())
}

fn prev_word_boundary(text: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let valid_cursor = snap_to_grapheme_boundary(text, cursor);
    let slice = &text[..valid_cursor];
    let mut end = valid_cursor;
    for (idx, ch) in slice.char_indices().rev() {
        if ch.is_whitespace() {
            end = idx;
        } else {
            break;
        }
    }
    if end == 0 {
        return 0;
    }
    if let Some((idx, _)) = slice[..end].split_word_bound_indices().next_back() {
        snap_to_grapheme_boundary(text, idx)
    } else {
        0
    }
}

fn next_word_boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    let valid_cursor = snap_to_grapheme_boundary(text, cursor);
    let slice = &text[valid_cursor..];
    let mut word_bounds = slice.split_word_bound_indices();
    let mut next_pos = if let Some((_, first_word)) = word_bounds.next() {
        first_word.len()
    } else {
        return text.len();
    };
    for (_, ch) in slice[next_pos..].char_indices() {
        if ch.is_whitespace() {
            next_pos += ch.len_utf8();
        } else {
            break;
        }
    }
    snap_to_grapheme_boundary(text, valid_cursor + next_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_insert_cap_net_replacement_and_rejection() {
        let mut f = Field::new("hello world".to_string());
        f.move_to(6, false);
        f.move_to(11, true); // selected "world" (5 bytes)
        assert_eq!(f.selected(), Some("world"));

        // Net cap: text len 11, replace 5 bytes with 6 bytes -> new len 12
        assert!(f.insert("friend", 12));
        assert_eq!(f.as_str(), "hello friend");
        assert_eq!(f.revision(), 1);

        // Atomic rejection: select "friend" (6 bytes)
        f.move_to(6, false);
        f.move_to(12, true);
        assert_eq!(f.selected(), Some("friend"));
        // Attempt to insert 10 bytes with limit 15 (12 - 6 + 10 = 16 > 15) -> rejected!
        assert!(!f.insert("everybody!!", 15));
        // Selection and text preserved!
        assert_eq!(f.as_str(), "hello friend");
        assert_eq!(f.selected(), Some("friend"));
        assert_eq!(f.revision(), 1);
    }

    #[test]
    fn field_equal_replacement_is_noop_revision_unchanged() {
        let mut f = Field::new("foobar".to_string());
        f.move_to(0, false);
        f.move_to(3, true);
        assert_eq!(f.selected(), Some("foo"));

        assert!(f.insert("foo", 100));
        assert_eq!(f.as_str(), "foobar");
        assert_eq!(f.revision(), 0); // Unchanged!
        assert_eq!(f.selection(), None);
        assert_eq!(f.cursor(), 3);
    }

    #[test]
    fn field_unicode_grapheme_and_zwj_navigation_and_deletion() {
        // "e\u{301}" is 'e' with acute accent (2 code points, 3 bytes, 1 grapheme)
        let mut f = Field::new("e\u{301}b".to_string());
        assert_eq!(f.as_str().len(), 4);
        assert_eq!(f.cursor(), 4);

        // Move Left: from 4 (after 'b') to 3 (before 'b', after 'e\u{301}')
        f.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(f.cursor(), 3);

        // Move Left again: skips whole "e\u{301}" grapheme cluster to 0!
        f.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(f.cursor(), 0);

        // Move Right with Shift: selects "e\u{301}"
        f.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        assert_eq!(f.cursor(), 3);
        assert_eq!(f.selection(), Some(0..3));
        assert_eq!(f.selected(), Some("e\u{301}"));

        // Backspace deletes selection
        f.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(f.as_str(), "b");
        assert_eq!(f.cursor(), 0);
        assert_eq!(f.revision(), 1);
    }

    #[test]
    fn field_word_navigation() {
        let mut f = Field::new("first second third".to_string());
        f.move_to(0, false);

        // Ctrl+Right moves word by word
        f.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(f.cursor(), 6); // start of "second"

        f.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
        assert_eq!(f.cursor(), 13); // start of "third"

        // Ctrl+Left moves back
        f.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL));
        assert_eq!(f.cursor(), 6);

        // Ctrl+A selects all
        f.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(f.selection(), Some(0..18));

        // Ctrl+U clears
        f.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(f.as_str(), "");
        assert_eq!(f.cursor(), 0);
        assert_eq!(f.selection(), None);
    }

    #[test]
    fn inserted_text_keeps_caret_after_a_newly_merged_grapheme() {
        for (original, inserted, merged) in [
            ("\u{301}", "a", "a\u{301}"),
            ("🇦", "🇧", "🇧🇦"),
            ("💻", "👩\u{200d}", "👩\u{200d}💻"),
        ] {
            let mut field = Field::new(original.into());
            field.move_to(0, false);
            assert!(field.insert(inserted, 4096));
            assert!(field.insert("x", 4096));
            assert_eq!(field.as_str(), format!("{merged}x"));
        }
    }
    #[test]
    fn polish_field_history_selection_and_branching() {
        let ctrl = KeyModifiers::CONTROL;
        let mut f = Field::from("hello");
        f.move_to(5, false);
        f.move_to(1, true);
        assert!(f.insert("", 100));
        assert_eq!(f.as_str(), "h");
        f.handle_key(KeyEvent::new(KeyCode::Char('z'), ctrl));
        assert_eq!(
            (f.as_str(), f.cursor(), f.selection()),
            ("hello", 1, Some(1..5))
        );
        assert!(!f.insert("too long", 5));
        assert!(f.insert("ello", 5));
        f.handle_key(KeyEvent::new(KeyCode::Char('y'), ctrl));
        assert_eq!(f.as_str(), "h");
        f.handle_key(KeyEvent::new(KeyCode::Char('z'), ctrl));
        assert!(f.insert("X", 100));
        f.handle_key(KeyEvent::new(KeyCode::Char('y'), ctrl));
        assert_eq!(f.as_str(), "hX");
        f.clear();
        f.handle_key(KeyEvent::new(KeyCode::Char('z'), ctrl));
        assert_eq!(f.as_str(), "hX");
        f.handle_key(KeyEvent::new(
            KeyCode::Char('Z'),
            ctrl | KeyModifiers::SHIFT,
        ));
        assert_eq!(f.as_str(), "");
    }

    #[test]
    fn polish_field_grapheme_seam_undo_redo() {
        let mut f = Field::from("\u{301}x");
        f.move_to(0, false);
        f.insert("a", 100);
        assert_eq!(f.cursor(), "a\u{301}".len());
        f.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!((f.as_str(), f.cursor()), ("\u{301}x", 0));
        f.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!((f.as_str(), f.cursor()), ("a\u{301}x", "a\u{301}".len()));
    }

    #[test]
    fn polish_field_word_delete_matches_navigation() {
        for (nav, delete) in [
            (KeyCode::Left, KeyCode::Backspace),
            (KeyCode::Right, KeyCode::Delete),
        ] {
            let mut f = Field::from("éclair,  世界!");
            f.move_to("éclair,  ".len(), false);
            let mut moved = f.clone();
            moved.handle_key(KeyEvent::new(nav, KeyModifiers::CONTROL));
            let range = f.cursor().min(moved.cursor())..f.cursor().max(moved.cursor());
            let mut expected = f.as_str().to_owned();
            expected.replace_range(range, "");
            f.handle_key(KeyEvent::new(delete, KeyModifiers::CONTROL));
            assert_eq!(f.as_str(), expected);
            f.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
            assert_eq!(f.as_str(), "éclair,  世界!");
        }
    }

    #[test]
    fn polish_field_history_limits_preserve_chain() {
        let ctrl = KeyModifiers::CONTROL;
        let mut f = Field::default();
        for _ in 0..130 {
            f.insert("x", 200);
        }
        for _ in 0..130 {
            f.handle_key(KeyEvent::new(KeyCode::Char('z'), ctrl));
        }
        assert_eq!(f.as_str(), "xx");
        for _ in 0..130 {
            f.handle_key(KeyEvent::new(KeyCode::Char('y'), ctrl));
        }
        assert_eq!(f.as_str(), "x".repeat(130));

        let mut f = Field::default();
        let chunk = "x".repeat(HISTORY_BYTES / 2 + 1);
        f.insert(&chunk, usize::MAX);
        f.insert(&chunk, usize::MAX);
        f.handle_key(KeyEvent::new(KeyCode::Char('z'), ctrl));
        f.handle_key(KeyEvent::new(KeyCode::Char('z'), ctrl));
        assert_eq!(f.as_str(), chunk);
    }
}

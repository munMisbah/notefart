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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    revision: u64,
}

impl Field {
    pub(crate) fn new(text: String) -> Self {
        let cursor = text.len();
        Self {
            text,
            cursor,
            anchor: None,
            revision: 0,
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
        if !self.text.is_empty() {
            self.text.clear();
            self.cursor = 0;
            self.anchor = None;
            self.revision = self.revision.saturating_add(1);
        } else {
            self.cursor = 0;
            self.anchor = None;
        }
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

        if let Some(range) = sel {
            if &self.text[range.clone()] == text {
                // Equal replacement: true no-op; revision unchanged
                self.cursor = range.start + text.len();
                self.anchor = None;
                return true;
            }
            self.text.replace_range(range.clone(), text);
            self.cursor = range.start + text.len();
            self.anchor = None;
            self.revision = self.revision.saturating_add(1);
        } else {
            if text.is_empty() {
                return true;
            }
            self.text.insert_str(self.cursor, text);
            self.cursor += text.len();
            self.anchor = None;
            self.revision = self.revision.saturating_add(1);
        }
        self.clamp_cursor_and_anchor();
        true
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
            KeyCode::Backspace => {
                if let Some(range) = self.selection() {
                    self.text.drain(range.clone());
                    self.cursor = range.start;
                    self.anchor = None;
                    self.revision = self.revision.saturating_add(1);
                    self.clamp_cursor_and_anchor();
                } else if self.cursor > 0 {
                    if let Some((idx, _)) =
                        self.text[..self.cursor].grapheme_indices(true).next_back()
                    {
                        self.text.drain(idx..self.cursor);
                        self.cursor = idx;
                        self.anchor = None;
                        self.revision = self.revision.saturating_add(1);
                        self.clamp_cursor_and_anchor();
                    }
                }
                true
            }
            KeyCode::Delete => {
                if let Some(range) = self.selection() {
                    self.text.drain(range.clone());
                    self.cursor = range.start;
                    self.anchor = None;
                    self.revision = self.revision.saturating_add(1);
                    self.clamp_cursor_and_anchor();
                } else if self.cursor < self.text.len() {
                    if let Some((_, g)) = self.text[self.cursor..].grapheme_indices(true).next() {
                        let end = self.cursor + g.len();
                        self.text.drain(self.cursor..end);
                        self.anchor = None;
                        self.revision = self.revision.saturating_add(1);
                        self.clamp_cursor_and_anchor();
                    }
                }
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
}

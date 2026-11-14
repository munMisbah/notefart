use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt::Write as _;
use std::ops::Range;

use ropey::iter::Chunks;
use ropey::str_utils::byte_to_char_idx;
use ropey::{Rope, RopeSlice};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};
use unicode_width::UnicodeWidthStr;

pub(crate) const TAB_STOP: usize = 4;
const TAB_SPACES: &str = "    ";
const GEOMETRY_STRIDE: usize = 1024;
const GEOMETRY_LINES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditError {
    OutOfBounds {
        start: usize,
        end: usize,
        len: usize,
    },
    InvalidRange {
        start: usize,
        end: usize,
    },
    SplitsGrapheme {
        pos: usize,
    },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds { start, end, len } => {
                write!(
                    f,
                    "edit range {}..{} out of bounds for document length {}",
                    start, end, len
                )
            }
            Self::InvalidRange { start, end } => {
                write!(f, "invalid edit range: start {} > end {}", start, end)
            }
            Self::SplitsGrapheme { pos } => {
                write!(
                    f,
                    "edit position {} splits an extended grapheme cluster",
                    pos
                )
            }
        }
    }
}

impl std::error::Error for EditError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cell<'a> {
    pub text: Cow<'a, str>,
    pub start: usize,
    pub end: usize,
    pub column: usize,
    pub width: usize,
}

#[derive(Debug, Clone)]
struct EditDelta {
    rev_before: u64,
    rev_after: u64,
    char_start: usize,
    removed: String,
    inserted_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextChange {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
}

impl TextChange {
    pub(crate) fn map(self, position: usize, right_affinity: bool) -> usize {
        if position < self.start {
            position
        } else if position > self.old_end {
            self.new_end.saturating_add(position - self.old_end)
        } else if position == self.old_end && self.start < self.old_end {
            self.new_end
        } else if right_affinity {
            self.new_end
        } else {
            self.start
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct GeometryPoint {
    chars: usize,
    bytes: usize,
    column: usize,
}

#[derive(Debug)]
struct LineGeometry {
    line: usize,
    checkpoints: Vec<GeometryPoint>,
    frontier: GeometryPoint,
}

// Geometry is derived from the rope, and must never be copied into a clone.
#[derive(Debug, Default)]
struct GeometryCache(RefCell<Vec<LineGeometry>>);

impl Clone for GeometryCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy)]
enum GeometryTarget {
    Chars(usize),
    Column(usize),
}

impl GeometryTarget {
    fn includes(self, point: GeometryPoint) -> bool {
        match self {
            Self::Chars(chars) => point.chars <= chars,
            Self::Column(column) => point.column <= column,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Document {
    rope: Rope,
    undo_stack: Vec<EditDelta>,
    redo_stack: Vec<EditDelta>,
    current_rev: u64,
    saved_rev: Option<u64>,
    next_rev: u64,
    text_revision: u64,
    changed_from: usize,
    last_change: Option<TextChange>,
    geometry: GeometryCache,
}

impl Default for Document {
    fn default() -> Self {
        Self::new("")
    }
}

impl Document {
    pub(crate) fn new(text: &str) -> Self {
        let rope = Rope::from_str(text);
        Self {
            rope,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_rev: 0,
            saved_rev: Some(0),
            next_rev: 1,
            text_revision: 0,
            changed_from: 0,
            last_change: None,
            geometry: GeometryCache::default(),
        }
    }

    pub(crate) fn text(&self) -> &Rope {
        &self.rope
    }

    /// Monotonic text generation, distinct from undo's saved-state identity.
    pub(crate) fn revision(&self) -> u64 {
        self.text_revision
    }

    pub(crate) fn changed_from(&self) -> usize {
        self.changed_from
    }

    pub(crate) fn last_change(&self) -> Option<TextChange> {
        self.last_change
    }

    fn record_change(&mut self, position: usize) {
        self.changed_from = self.line_of(position);
        let start = self.line_start(self.changed_from);
        let safe = self.previous(position).max(start) - start;
        let lines = self.geometry.0.get_mut();
        lines.retain(|entry| entry.line <= self.changed_from);
        if let Some(entry) = lines
            .iter_mut()
            .find(|entry| entry.line == self.changed_from)
        {
            let keep = entry
                .checkpoints
                .partition_point(|point| point.chars <= safe);
            entry.checkpoints.truncate(keep);
            if entry.frontier.chars > safe {
                entry.frontier = entry.checkpoints.last().copied().unwrap_or_default();
            }
        }
        self.text_revision += 1;
    }

    pub(crate) fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub(crate) fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub(crate) fn line_start(&self, line: usize) -> usize {
        let count = self.line_count();
        if count == 0 {
            return 0;
        }
        let line = line.min(count.saturating_sub(1));
        self.rope.line_to_char(line)
    }

    pub(crate) fn line_end(&self, line: usize) -> usize {
        let count = self.line_count();
        if count == 0 {
            return 0;
        }
        let line = line.min(count.saturating_sub(1));
        let start = self.rope.line_to_char(line);
        let line_slice = self.rope.line(line);
        let len = line_slice.len_chars();
        let body_len =
            if len >= 2 && line_slice.char(len - 2) == '\r' && line_slice.char(len - 1) == '\n' {
                len - 2
            } else if len >= 1 && line_slice.char(len - 1) == '\n' {
                len - 1
            } else {
                len
            };
        start + body_len
    }

    pub(crate) fn line_of(&self, pos: usize) -> usize {
        let len = self.len_chars();
        self.rope.char_to_line(pos.min(len))
    }

    pub(crate) fn column_of(&self, pos: usize) -> usize {
        let pos = pos.min(self.len_chars());
        let line = self.line_of(pos);
        let start = self.line_start(line);
        if pos == start {
            return 0;
        }
        self.geometry_at(line, GeometryTarget::Chars(pos - start))
            .column
    }

    pub(crate) fn position_at(&self, line: usize, column: usize) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        let start = self.line_start(line);
        if column == 0 {
            return start;
        }
        start + self.geometry_at(line, GeometryTarget::Column(column)).chars
    }

    pub(crate) fn is_grapheme_boundary(&self, pos: usize) -> bool {
        let len = self.len_chars();
        if pos == 0 || pos == len {
            return true;
        }
        if pos > len {
            return false;
        }
        is_rope_grapheme_boundary(self.rope.slice(..), pos)
    }

    pub(crate) fn previous(&self, pos: usize) -> usize {
        if pos == 0 || self.rope.len_chars() == 0 {
            return 0;
        }
        let pos = pos.min(self.len_chars());
        prev_rope_grapheme_boundary(self.rope.slice(..), pos)
    }

    pub(crate) fn next(&self, pos: usize) -> usize {
        let len = self.len_chars();
        if pos >= len || len == 0 {
            return len;
        }
        next_rope_grapheme_boundary(self.rope.slice(..), pos)
    }

    pub(crate) fn word_range(&self, pos: usize) -> Range<usize> {
        let len = self.len_chars();
        if len == 0 {
            return 0..0;
        }
        let pos = pos.min(len);
        let line = self.line_of(pos);
        let line_start = self.line_start(line);
        let line_end = self.line_end(line);

        if line_start >= line_end {
            return line_start..line_start;
        }

        let target = if pos >= line_end {
            self.previous(line_end)
        } else if self.is_grapheme_boundary(pos) {
            pos
        } else {
            self.previous(pos)
        };
        let target_class = char_class(self.rope.char(target));
        let mut start = target;
        while start > line_start {
            let previous = self.previous(start);
            if char_class(self.rope.char(previous)) != target_class {
                break;
            }
            start = previous;
        }
        let mut end = self.next(target).min(line_end);
        while end < line_end && char_class(self.rope.char(end)) == target_class {
            end = self.next(end).min(line_end);
        }

        start..end
    }

    pub(crate) fn replace(
        &mut self,
        range: Range<usize>,
        insert: &str,
    ) -> Result<usize, EditError> {
        self.last_change = None;
        if range.start > range.end {
            return Err(EditError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        let len = self.len_chars();
        if range.end > len {
            return Err(EditError::OutOfBounds {
                start: range.start,
                end: range.end,
                len,
            });
        }
        if !self.is_grapheme_boundary(range.start) {
            return Err(EditError::SplitsGrapheme { pos: range.start });
        }
        if !self.is_grapheme_boundary(range.end) {
            return Err(EditError::SplitsGrapheme { pos: range.end });
        }

        if range.is_empty() && insert.is_empty() {
            return Ok(range.start);
        }

        self.record_change(range.start);
        let removed = String::from(self.rope.slice(range.clone()));
        let inserted_chars = insert.chars().count();

        if !range.is_empty() {
            self.rope.remove(range.clone());
        }
        if !insert.is_empty() {
            self.rope.insert(range.start, insert);
        }

        let mut cursor = range.start + inserted_chars;
        if !self.is_grapheme_boundary(cursor) {
            cursor = self.next(cursor);
        }

        let new_rev = self.next_rev;
        self.next_rev += 1;

        self.undo_stack.push(EditDelta {
            rev_before: self.current_rev,
            rev_after: new_rev,
            char_start: range.start,
            removed,
            inserted_chars,
        });
        self.current_rev = new_rev;
        self.redo_stack.clear();
        self.last_change = Some(TextChange {
            start: range.start,
            old_end: range.end,
            new_end: range.start + inserted_chars,
        });

        Ok(cursor)
    }

    pub(crate) fn undo(&mut self) -> Option<usize> {
        self.last_change = None;
        let delta = self.undo_stack.pop()?;
        self.record_change(delta.char_start);
        let remove_end = delta.char_start + delta.inserted_chars;
        let redo_removed = String::from(self.rope.slice(delta.char_start..remove_end));

        if delta.inserted_chars > 0 {
            self.rope.remove(delta.char_start..remove_end);
        }
        if !delta.removed.is_empty() {
            self.rope.insert(delta.char_start, &delta.removed);
        }

        let restored_chars = delta.removed.chars().count();
        let mut cursor = delta.char_start + restored_chars;
        if !self.is_grapheme_boundary(cursor) {
            cursor = self.next(cursor);
        }

        self.current_rev = delta.rev_before;
        self.redo_stack.push(EditDelta {
            rev_before: delta.rev_before,
            rev_after: delta.rev_after,
            char_start: delta.char_start,
            removed: redo_removed,
            inserted_chars: restored_chars,
        });
        self.last_change = Some(TextChange {
            start: delta.char_start,
            old_end: remove_end,
            new_end: delta.char_start + restored_chars,
        });

        Some(cursor)
    }

    pub(crate) fn redo(&mut self) -> Option<usize> {
        self.last_change = None;
        let delta = self.redo_stack.pop()?;
        self.record_change(delta.char_start);
        let remove_end = delta.char_start + delta.inserted_chars;
        let undo_removed = String::from(self.rope.slice(delta.char_start..remove_end));

        if delta.inserted_chars > 0 {
            self.rope.remove(delta.char_start..remove_end);
        }
        if !delta.removed.is_empty() {
            self.rope.insert(delta.char_start, &delta.removed);
        }

        let restored_chars = delta.removed.chars().count();
        let mut cursor = delta.char_start + restored_chars;
        if !self.is_grapheme_boundary(cursor) {
            cursor = self.next(cursor);
        }

        self.current_rev = delta.rev_after;
        self.undo_stack.push(EditDelta {
            rev_before: delta.rev_before,
            rev_after: delta.rev_after,
            char_start: delta.char_start,
            removed: undo_removed,
            inserted_chars: restored_chars,
        });
        self.last_change = Some(TextChange {
            start: delta.char_start,
            old_end: remove_end,
            new_end: delta.char_start + restored_chars,
        });

        Some(cursor)
    }

    pub(crate) fn mark_saved(&mut self) {
        self.saved_rev = Some(self.current_rev);
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.saved_rev != Some(self.current_rev)
    }
    // Points describe boundaries in the full line, not a suffix: the grapheme
    // cursor can still request arbitrarily distant preceding context.
    fn geometry_at(&self, line: usize, target: GeometryTarget) -> GeometryPoint {
        let start = self.line_start(line);
        let end = self.line_end(line);
        if start == end {
            return GeometryPoint::default();
        }
        let slice = self.rope.slice(start..end);
        if end - start < GEOMETRY_STRIDE {
            return Self::scan_geometry(slice, start, target, GeometryPoint::default(), None);
        }

        let mut cache = self.geometry.0.borrow_mut();
        let index = cache.iter().position(|entry| entry.line == line);
        let mut entry = if let Some(index) = index {
            cache.remove(index)
        } else {
            if cache.len() == GEOMETRY_LINES {
                cache.remove(0);
            }
            LineGeometry {
                line,
                checkpoints: vec![GeometryPoint::default()],
                frontier: GeometryPoint::default(),
            }
        };
        let index = entry
            .checkpoints
            .partition_point(|point| target.includes(*point));
        let mut point = entry.checkpoints[index.saturating_sub(1)];
        if target.includes(entry.frontier) && entry.frontier.chars > point.chars {
            point = entry.frontier;
        }
        let result = Self::scan_geometry(slice, start, target, point, Some(&mut entry));
        cache.push(entry);
        result
    }

    fn scan_geometry(
        slice: RopeSlice<'_>,
        start: usize,
        target: GeometryTarget,
        mut point: GeometryPoint,
        mut entry: Option<&mut LineGeometry>,
    ) -> GeometryPoint {
        let mut cells = LineCells::from_point(slice, start, point);
        while let Some(cell) = cells.next() {
            let next = GeometryPoint {
                chars: cell.end - start,
                bytes: cells.cursor.cur_cursor(),
                column: cell.column + cell.width,
            };
            if !target.includes(next) {
                break;
            }
            point = next;
            if let Some(entry) = entry.as_deref_mut() {
                if point.chars > entry.frontier.chars {
                    if point.chars - entry.checkpoints.last().copied().unwrap_or_default().chars
                        >= GEOMETRY_STRIDE
                    {
                        entry.checkpoints.push(point);
                    }
                    entry.frontier = point;
                }
            }
        }
        point
    }

    /// Resume at or before a visual column; callers must still clip prefix cells.
    /// Source offsets stay document-absolute and columns stay line-absolute.
    pub(crate) fn cells_from_column(&self, line: usize, column: usize) -> LineCells<'_> {
        if column == 0 {
            return self.cells(line);
        }
        if line >= self.line_count() {
            return LineCells::empty(self.rope.slice(0..0), 0);
        }
        let start = self.line_start(line);
        let end = self.line_end(line);
        if start == end {
            return LineCells::empty(self.rope.slice(0..0), start);
        }
        let point = if end - start < GEOMETRY_STRIDE {
            GeometryPoint::default()
        } else {
            self.geometry_at(line, GeometryTarget::Column(column))
        };
        LineCells::from_point(self.rope.slice(start..end), start, point)
    }

    pub(crate) fn cells(&self, line: usize) -> LineCells<'_> {
        let count = self.line_count();
        if line >= count {
            return LineCells::empty(self.rope.slice(0..0), 0);
        }
        let start = self.line_start(line);
        let end = self.line_end(line);
        if start >= end {
            return LineCells::empty(self.rope.slice(0..0), start);
        }
        LineCells::from_point(self.rope.slice(start..end), start, GeometryPoint::default())
    }
}

pub(crate) struct LineCells<'a> {
    text: RopeSlice<'a>,
    chunks: Chunks<'a>,
    cur_chunk: &'a str,
    cur_chunk_start: usize,
    cursor: GraphemeCursor,
    cur_char: usize,
    cur_col: usize,
    finished: bool,
}

impl<'a> LineCells<'a> {
    fn cursor_at(
        text: RopeSlice<'a>,
        offset: usize,
        chunk: &str,
        chunk_start: usize,
    ) -> (GraphemeCursor, bool) {
        let mut cursor = GraphemeCursor::new(offset, text.len_bytes(), true);
        loop {
            match cursor.is_boundary(chunk, chunk_start) {
                Ok(boundary) => return (cursor, boundary),
                Err(GraphemeIncomplete::PreContext(end)) => {
                    let (context, start, _, _) = text.chunk_at_byte(end.saturating_sub(1));
                    cursor.provide_context(&context[..end - start], start);
                }
                Err(error) => panic!("grapheme context invariant failed: {error:?}"),
            }
        }
    }

    fn from_point(text: RopeSlice<'a>, start: usize, point: GeometryPoint) -> Self {
        let (mut chunks, cur_chunk_start, _, _) = text.chunks_at_byte(point.bytes);
        let cur_chunk = chunks.next().unwrap_or("");
        let (cursor, boundary) = Self::cursor_at(text, point.bytes, cur_chunk, cur_chunk_start);
        assert!(boundary, "geometry checkpoint must be a grapheme boundary");
        Self {
            text,
            chunks,
            cur_chunk,
            cur_chunk_start,
            cursor,
            cur_char: start + point.chars,
            cur_col: point.column,
            finished: point.bytes == text.len_bytes(),
        }
    }

    fn empty(slice: RopeSlice<'a>, start: usize) -> Self {
        let mut chunks = slice.chunks();
        let cur_chunk = chunks.next().unwrap_or("");
        let cursor = GraphemeCursor::new(0, 0, true);
        Self {
            text: slice,
            chunks,
            cur_chunk,
            cur_chunk_start: 0,
            cursor,
            cur_char: start,
            cur_col: 0,
            finished: true,
        }
    }
}

impl<'a> Iterator for LineCells<'a> {
    type Item = Cell<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let a = self.cursor.cur_cursor();
        let b = loop {
            match self
                .cursor
                .next_boundary(self.cur_chunk, self.cur_chunk_start)
            {
                Ok(None) => {
                    self.finished = true;
                    return None;
                }
                Ok(Some(n)) => break n,
                Err(GraphemeIncomplete::NextChunk) => {
                    self.cur_chunk_start += self.cur_chunk.len();
                    self.cur_chunk = self.chunks.next().unwrap_or("");
                    // Re-establish lookbehind at the seam before advancing.
                    // Reusing a suspended cursor can count the preceding RI
                    // run twice when the next chunk requests pre-context.
                    let offset = self.cursor.cur_cursor();
                    let (cursor, boundary) =
                        Self::cursor_at(self.text, offset, self.cur_chunk, self.cur_chunk_start);
                    self.cursor = cursor;
                    if boundary && offset > a {
                        break offset;
                    }
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_start, _, _) =
                        self.text.chunk_at_byte(idx.saturating_sub(1));
                    self.cursor
                        .provide_context(&ctx_chunk[..idx - ctx_start], ctx_start);
                }
                Err(error) => panic!("grapheme iterator invariant failed: {error:?}"),
            }
        };

        let (raw_cow, g_chars) = if a < self.cur_chunk_start {
            let slice = self.text.byte_slice(a..b);
            let chars = slice.len_chars();
            (Cow::Owned(String::from(slice)), chars)
        } else {
            let a_rel = a - self.cur_chunk_start;
            let b_rel = b - self.cur_chunk_start;
            let sub = &self.cur_chunk[a_rel..b_rel];
            let chars = sub.chars().count();
            (Cow::Borrowed(sub), chars)
        };

        let start = self.cur_char;
        let end = start + g_chars;
        self.cur_char = end;

        let col = self.cur_col;
        let (text, width) = if raw_cow.as_ref() == "\t" {
            let w = TAB_STOP - (col % TAB_STOP);
            (Cow::Borrowed(&TAB_SPACES[..w]), w)
        } else if raw_cow.chars().any(is_control_or_bidi) {
            let mut out = String::with_capacity(raw_cow.len().saturating_mul(2));
            for ch in raw_cow.chars() {
                if is_control_or_bidi(ch) {
                    push_escaped_char(&mut out, ch);
                } else {
                    out.push(ch);
                }
            }
            let w = out.width().max(1);
            (Cow::Owned(out), w)
        } else {
            let w = raw_cow.width();
            if w == 0 {
                let mut out = String::with_capacity(8);
                for ch in raw_cow.chars() {
                    let _ = write!(out, "\\u{{{:04x}}}", ch as u32);
                }
                let w = out.width().max(1);
                (Cow::Owned(out), w)
            } else {
                (raw_cow, w)
            }
        };

        self.cur_col += width;
        Some(Cell {
            text,
            start,
            end,
            column: col,
            width,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Whitespace,
    Punctuation,
}

fn char_class(c: char) -> CharClass {
    if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Whitespace
    } else {
        CharClass::Punctuation
    }
}

fn is_bidi_or_invisible_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}'
            | '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

fn is_control_or_bidi(ch: char) -> bool {
    ch.is_control() || is_bidi_or_invisible_format(ch)
}

fn push_escaped_char(out: &mut String, ch: char) {
    match ch {
        '\0' => out.push_str("\\0"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        c if c.is_ascii_control() => {
            let _ = write!(out, "\\x{:02x}", c as u32);
        }
        c => {
            let _ = write!(out, "\\u{{{:04x}}}", c as u32);
        }
    }
}

fn is_rope_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> bool {
    if char_idx == 0 || char_idx == slice.len_chars() {
        return true;
    }
    if char_idx > slice.len_chars() {
        return false;
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (chunk, chunk_byte_idx, _, _) = slice.chunk_at_byte(byte_idx);
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match gc.is_boundary(chunk, chunk_byte_idx) {
            Ok(is_bound) => return is_bound,
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx_chunk, ctx_byte_start, _, _) = slice.chunk_at_byte(n.saturating_sub(1));
                gc.provide_context(ctx_chunk, ctx_byte_start);
            }
            Err(_) => return false,
        }
    }
}

fn prev_rope_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    if char_idx == 0 || slice.len_chars() == 0 {
        return 0;
    }
    let char_idx = char_idx.min(slice.len_chars());
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte_idx, mut chunk_char_idx, _) = slice.chunk_at_byte(byte_idx);
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match gc.prev_boundary(chunk, chunk_byte_idx) {
            Ok(None) => return 0,
            Ok(Some(n)) => {
                return chunk_char_idx + byte_to_char_idx(chunk, n - chunk_byte_idx);
            }
            Err(GraphemeIncomplete::PrevChunk) => {
                if chunk_byte_idx == 0 {
                    return 0;
                }
                let (a, b, c, _) = slice.chunk_at_byte(chunk_byte_idx - 1);
                chunk = a;
                chunk_byte_idx = b;
                chunk_char_idx = c;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx_chunk, ctx_byte_start, _, _) = slice.chunk_at_byte(n.saturating_sub(1));
                gc.provide_context(ctx_chunk, ctx_byte_start);
            }
            Err(_) => return 0,
        }
    }
}

fn next_rope_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    let len_chars = slice.len_chars();
    if char_idx >= len_chars || slice.len_bytes() == 0 {
        return len_chars;
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte_idx, mut chunk_char_idx, _) = slice.chunk_at_byte(byte_idx);
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match gc.next_boundary(chunk, chunk_byte_idx) {
            Ok(None) => return len_chars,
            Ok(Some(n)) => {
                return chunk_char_idx + byte_to_char_idx(chunk, n - chunk_byte_idx);
            }
            Err(GraphemeIncomplete::NextChunk) => {
                let next_byte = chunk_byte_idx + chunk.len();
                if next_byte >= slice.len_bytes() {
                    return len_chars;
                }
                let (a, _, c, _) = slice.chunk_at_byte(next_byte);
                chunk_byte_idx = next_byte;
                chunk = a;
                chunk_char_idx = c;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx_chunk, ctx_byte_start, _, _) = slice.chunk_at_byte(n.saturating_sub(1));
                gc.provide_context(ctx_chunk, ctx_byte_start);
            }
            Err(_) => return len_chars,
        }
    }
}

#[cfg(test)]
mod tests {
    fn assert_geometry_matches_cells(doc: &Document, line: usize) {
        use unicode_segmentation::UnicodeSegmentation;

        let cells: Vec<_> = doc.cells(line).collect();
        let start = doc.line_start(line);
        let body = doc.text().slice(start..doc.line_end(line)).to_string();
        let mut next = start;
        let expected: Vec<_> = body
            .graphemes(true)
            .map(|grapheme| {
                let begin = next;
                next += grapheme.chars().count();
                (begin, next)
            })
            .collect();
        assert_eq!(
            cells
                .iter()
                .map(|cell| (cell.start, cell.end))
                .collect::<Vec<_>>(),
            expected,
            "streaming cell boundaries must match whole-string segmentation"
        );
        let end = doc.line_end(line);
        let end_col = cells.last().map_or(0, |cell| cell.column + cell.width);
        assert_eq!(doc.column_of(end), end_col);
        assert_eq!(doc.position_at(line, usize::MAX), end);
        assert!(
            doc.cells_from_column(line, end_col + 10)
                .all(|cell| cell.column + cell.width <= end_col)
        );

        for (index, cell) in cells.iter().enumerate() {
            if index % 127 != 0 && !(1018..1050).contains(&index) && index + 2 < cells.len() {
                continue;
            }
            for pos in [cell.start, cell.end - 1] {
                assert_eq!(doc.column_of(pos), cell.column, "position {pos}");
            }
            for column in [cell.column, cell.column + cell.width - 1] {
                assert_eq!(doc.position_at(line, column), cell.start, "column {column}");
                let resumed: Vec<_> = doc.cells_from_column(line, column).collect();
                let first = resumed.first().expect("target cell exists");
                assert!(first.column <= column);
                let baseline_index = cells.iter().position(|c| c.start == first.start).unwrap();
                assert_eq!(resumed, cells[baseline_index..], "column {column}");
            }
        }
    }

    #[test]
    fn sparse_geometry_matches_full_cells_across_unicode_and_line_endings() {
        let mut text = "x".repeat(1021);
        text.push_str("\t世e\u{301}👩\u{200d}🔬\u{600}a\x1b\u{301}\u{202e}\u{200b}");
        text.push_str(&"🇦🇧🇨🇩🇪".repeat(300));
        text.push_str(&"z\t界\u{301}".repeat(350));
        text.push_str("\r\n");
        text.push_str(&"q\t世".repeat(600));
        text.push_str("\nshort");
        let doc = Document::new(&text);
        assert_geometry_matches_cells(&doc, 0);
        assert_geometry_matches_cells(&doc, 1);
        assert_geometry_matches_cells(&doc, 2);
        assert!(doc.cells_from_column(99, 10).next().is_none());
        assert_eq!(
            doc.column_of(doc.line_end(0) + 1),
            doc.column_of(doc.line_end(0))
        );
    }

    #[test]
    fn sparse_geometry_survives_mutations_line_shifts_and_clone() {
        let text = format!(
            "{}\n{}\r\n{}",
            "a".repeat(2500),
            "b\t世".repeat(800),
            "c".repeat(2200)
        );
        let mut doc = Document::new(&text);
        for line in 0..3 {
            assert_geometry_matches_cells(&doc, line);
        }
        let mut cloned = doc.clone();
        let end = doc.line_end(0);
        doc.replace(end..end, "\u{301}👩\u{200d}🔬").unwrap();
        assert_geometry_matches_cells(&doc, 0);
        assert_geometry_matches_cells(&cloned, 0);
        doc.undo().unwrap();
        assert_geometry_matches_cells(&doc, 0);
        doc.redo().unwrap();
        assert_geometry_matches_cells(&doc, 0);

        let split = doc.line_start(1) + 1200;
        let split = if doc.is_grapheme_boundary(split) {
            split
        } else {
            doc.previous(split)
        };
        doc.replace(split..split, "\n").unwrap();
        for line in 0..doc.line_count() {
            assert_geometry_matches_cells(&doc, line);
        }
        doc.undo().unwrap();
        for line in 0..doc.line_count() {
            assert_geometry_matches_cells(&doc, line);
        }
        doc.redo().unwrap();
        let join = doc.line_end(1);
        doc.replace(join..join + 1, "").unwrap();
        for line in 0..doc.line_count() {
            assert_geometry_matches_cells(&doc, line);
        }
        doc.undo().unwrap();
        for line in 0..doc.line_count() {
            assert_geometry_matches_cells(&doc, line);
        }
        doc.redo().unwrap();
        for line in 0..doc.line_count() {
            assert_geometry_matches_cells(&doc, line);
        }
        let clone_end = cloned.line_end(2);
        cloned.replace(clone_end..clone_end, "\t\u{200b}").unwrap();
        assert_geometry_matches_cells(&cloned, 2);
        assert_geometry_matches_cells(&doc, 2);
    }

    use super::*;

    #[test]
    fn undo_redo_checkpoint_and_branching() {
        let mut doc = Document::new("initial text");
        assert!(!doc.is_dirty());

        let c1 = doc.replace(0..0, "hello ").expect("valid insert");
        assert!(doc.is_dirty());

        doc.mark_saved();
        assert!(!doc.is_dirty());

        let c2 = doc.replace(c1..c1, "world ").expect("valid insert");
        assert!(doc.is_dirty());

        let u1 = doc.undo().expect("undo available");
        assert_eq!(u1, c1);
        assert!(!doc.is_dirty(), "undo back to saved state must be clean");

        let r1 = doc.redo().expect("redo available");
        assert_eq!(r1, c2);
        assert!(doc.is_dirty(), "redo back to unsaved state must be dirty");

        doc.undo();
        assert!(!doc.is_dirty());

        // Branching: a new edit clears redo history and does not match the previous checkpoint
        doc.replace(c1..c1, "notefart ").expect("branch edit");
        assert!(doc.is_dirty());
        assert_eq!(doc.redo(), None, "redo history must be cleared on branch");

        // Undoing branch returns to the checkpoint state
        let u2 = doc.undo().expect("undo branch");
        assert_eq!(u2, c1);
        assert!(
            !doc.is_dirty(),
            "undoing branch back to saved checkpoint is clean"
        );
    }

    #[test]
    fn no_op_edit_preserves_redo_history() {
        let mut doc = Document::new("abc");
        let c = doc.replace(0..0, "x").expect("insert");
        assert_eq!(c, 1);
        doc.undo().expect("undo");

        // No-op edit (empty range + empty insert)
        let res = doc.replace(1..1, "").expect("no-op replace");
        assert_eq!(res, 1);
        assert!(
            doc.redo().is_some(),
            "no-op replace must not discard redo history"
        );
    }

    #[test]
    fn crlf_and_grapheme_boundary_protection() {
        let mut doc = Document::new("line1\r\nline2\nline3");
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.line_start(0), 0);
        assert_eq!(doc.line_end(0), 5); // excludes \r\n
        assert_eq!(doc.line_start(1), 7);
        assert_eq!(doc.line_end(1), 12); // excludes \n
        assert_eq!(doc.line_start(2), 13);
        assert_eq!(doc.line_end(2), 18);

        // Position 5 is \r, 6 is \n. Pos 6 must not be a boundary.
        assert!(!doc.is_grapheme_boundary(6));
        assert!(matches!(
            doc.replace(6..6, "a"),
            Err(EditError::SplitsGrapheme { pos: 6 })
        ));
        assert!(matches!(
            doc.replace(5..6, ""),
            Err(EditError::SplitsGrapheme { pos: 6 })
        ));

        // Navigation skips splitting \r\n
        assert_eq!(doc.previous(7), 5);
        assert_eq!(doc.next(5), 7);
        assert_eq!(doc.previous(6), 5);
        assert_eq!(doc.next(6), 7);
    }

    #[test]
    fn cursor_invariant_on_merged_grapheme_replacement() {
        // Replacement that merges inserted text with following grapheme
        let mut doc = Document::new("🔬");
        // Insert woman emoji + ZWJ before microscope emoji
        let cursor = doc.replace(0..0, "👩\u{200D}").expect("valid insert");
        // The newly formed "👩‍🔬" is a single extended grapheme cluster of length 3 scalars
        assert!(doc.is_grapheme_boundary(cursor));
        assert_eq!(
            cursor,
            doc.len_chars(),
            "cursor must snap forward past merged cluster"
        );

        // Undo cursor is similarly valid
        let u = doc.undo().expect("undo");
        assert!(doc.is_grapheme_boundary(u));
        assert_eq!(u, 0);

        // Redo cursor is similarly valid
        let r = doc.redo().expect("redo");
        assert!(doc.is_grapheme_boundary(r));
        assert_eq!(r, doc.len_chars());
    }

    #[test]
    fn safe_cells_and_column_mapping() {
        // Test tab expansion, C0 control (\x1b, \0), zero-width format (\u{200b}),
        // combining grapheme ("e\u{0301}") and CJK ("世")
        let doc = Document::new("\t世\x1b\u{200b}e\u{0301}");
        let cells: Vec<_> = doc.cells(0).collect();

        // 1. Tab at col 0: width 4
        assert_eq!(cells[0].text, "    ");
        assert_eq!(cells[0].column, 0);
        assert_eq!(cells[0].width, 4);
        assert_eq!(cells[0].start, 0);
        assert_eq!(cells[0].end, 1);

        // 2. CJK '世' at col 4: width 2
        assert_eq!(cells[1].text, "世");
        assert_eq!(cells[1].column, 4);
        assert_eq!(cells[1].width, 2);
        assert_eq!(cells[1].start, 1);
        assert_eq!(cells[1].end, 2);

        // 3. \x1b (ESC) at col 6: safe visible representation
        assert_eq!(cells[2].text, "\\x1b");
        assert_eq!(cells[2].column, 6);
        assert_eq!(cells[2].width, 4);

        // 4. \u{200b} (ZWSP) at col 10: safe visible representation
        assert_eq!(cells[3].text, "\\u{200b}");
        assert_eq!(cells[3].column, 10);
        assert_eq!(cells[3].width, 8);

        // 5. "e\u{0301}" combining grapheme at col 18: kept as regular grapheme, width 1
        assert_eq!(cells[4].text, "e\u{0301}");
        assert_eq!(cells[4].column, 18);
        assert_eq!(cells[4].width, 1);
        assert_eq!(cells[4].start, 4);
        assert_eq!(cells[4].end, 6);

        // position_at snap-left and clamp-eol tests
        assert_eq!(doc.position_at(0, 0), 0);
        assert_eq!(doc.position_at(0, 1), 0); // snap left in tab
        assert_eq!(doc.position_at(0, 2), 0); // snap left in tab
        assert_eq!(doc.position_at(0, 3), 0); // snap left in tab
        assert_eq!(doc.position_at(0, 4), 1); // '世'
        assert_eq!(doc.position_at(0, 5), 1); // snap left in '世'
        assert_eq!(doc.position_at(0, 6), 2); // \x1b
        assert_eq!(doc.position_at(0, 200), doc.line_end(0)); // clamp to EOL

        // column_of tests
        assert_eq!(doc.column_of(0), 0);
        assert_eq!(doc.column_of(1), 4);
        assert_eq!(doc.column_of(2), 6);
        assert_eq!(doc.column_of(4), 18);
        assert_eq!(doc.column_of(5), 18); // inside combining cluster snaps left to 18
        assert_eq!(doc.column_of(6), 19); // EOL
    }

    #[test]
    fn chunk_seam_grapheme_navigation_and_cells() {
        // Construct multi-chunk rope where a ZWJ emoji is split across a chunk seam
        let mut seam_sample: Option<(Rope, usize)> = None;
        for pad in 0..16usize {
            let mut line = "x".repeat(pad);
            for _ in 0..360 {
                line.push_str("👩‍🔬");
            }
            let candidate = Rope::from_str(&line);
            let mut char_acc = 0usize;
            let total_chunks = candidate.chunks().count();
            if total_chunks >= 3 {
                for (chunk_idx, chunk) in candidate.chunks().enumerate() {
                    char_acc += chunk.chars().count();
                    if chunk_idx + 1 < total_chunks
                        && !is_rope_grapheme_boundary(candidate.slice(..), char_acc)
                    {
                        seam_sample = Some((candidate, char_acc));
                        break;
                    }
                }
            }
            if seam_sample.is_some() {
                break;
            }
        }

        let (rope, seam_char) =
            seam_sample.expect("padding sweep guarantees ZWJ emoji across chunk seam");
        let mut doc = Document {
            rope,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_rev: 0,
            saved_rev: Some(0),
            next_rev: 1,
            text_revision: 0,
            changed_from: 0,
            last_change: None,
            geometry: GeometryCache::default(),
        };

        // Navigation across seam
        let prev = doc.previous(seam_char);
        let next = doc.next(seam_char);
        assert!(prev < seam_char && seam_char < next);
        assert!(doc.is_grapheme_boundary(prev));
        assert!(doc.is_grapheme_boundary(next));
        assert!(!doc.is_grapheme_boundary(seam_char));

        // Edits splitting the seam grapheme are rejected
        assert!(matches!(
            doc.replace(seam_char..seam_char, "test"),
            Err(EditError::SplitsGrapheme { .. })
        ));

        // LineCells can iterate the seam line without panics and yields all cells
        let cells: Vec<_> = doc.cells(0).collect();
        assert!(!cells.is_empty());
        let mut expected_col = 0;
        for cell in &cells {
            assert_eq!(cell.column, expected_col);
            expected_col += cell.width;
        }
    }

    #[test]
    fn word_range_behavior() {
        let doc = Document::new("let mut my_var = 42;\nsecond_line");
        assert_eq!(doc.word_range(0), 0..3); // "let"
        assert_eq!(doc.word_range(3), 3..4); // " "
        assert_eq!(doc.word_range(8), 8..14); // "my_var"
        assert_eq!(doc.word_range(15), 15..16); // "="
        assert_eq!(doc.word_range(17), 17..19); // "42"
        assert_eq!(doc.word_range(19), 19..20); // ";"

        // Word range at line end does not cross newline
        let eol = doc.line_end(0);
        assert_eq!(doc.word_range(eol), 19..20); // selects ";" at EOL

        // Word range on empty document
        let empty = Document::new("");
        assert_eq!(empty.word_range(0), 0..0);
    }

    #[test]
    fn lone_cr_is_visible_content_and_reachable_at_line_end() {
        let doc = Document::new("a\r");
        assert_eq!(doc.line_end(0), 2);
        let cells: Vec<_> = doc.cells(0).collect();
        assert_eq!(cells.len(), 2);
        assert!(cells[1].text.chars().all(|ch| !ch.is_control()));
        assert_eq!(doc.position_at(0, usize::MAX), 2);
        assert!(doc.column_of(2) > doc.column_of(1));
    }

    #[test]
    fn decomposed_marks_do_not_split_word_selection() {
        let doc = Document::new("a\u{0301}b");
        assert_eq!(doc.word_range(0), 0..3);
        assert_eq!(doc.word_range(2), 0..3);
        let composed = Document::new("áb");
        assert_eq!(composed.word_range(0), 0..composed.len_chars());
    }

    #[test]
    fn text_generation_invalidates_caches_even_when_undo_restores_saved_state() {
        let mut doc = Document::new("first\nsecond");
        let before = doc.revision();
        let start = doc.line_start(1);
        doc.replace(start..start, "new\n").unwrap();
        let edited = doc.revision();
        assert!(edited > before);
        assert_eq!(doc.changed_from(), 1);
        doc.undo().unwrap();
        assert!(!doc.is_dirty());
        assert!(doc.revision() > edited);
        assert_eq!(doc.changed_from(), 1);
        let undone = doc.revision();
        doc.redo().unwrap();
        assert!(doc.revision() > undone);
        assert!(doc.is_dirty());
    }

    #[test]
    fn text_change_map_affinity_and_ranges() {
        // Pure insertion: 5..5 -> 5..8 (+3)
        let insert = TextChange {
            start: 5,
            old_end: 5,
            new_end: 8,
        };
        assert_eq!(insert.map(0, false), 0);
        assert_eq!(insert.map(4, true), 4);
        assert_eq!(insert.map(5, false), 5);
        assert_eq!(insert.map(5, true), 8);
        assert_eq!(insert.map(6, false), 9);
        assert_eq!(insert.map(10, true), 13);
        assert_eq!(insert.map(usize::MAX, false), usize::MAX);

        // Pure deletion: 5..9 -> 5..5 (-4)
        let delete = TextChange {
            start: 5,
            old_end: 9,
            new_end: 5,
        };
        assert_eq!(delete.map(4, true), 4);
        assert_eq!(delete.map(5, false), 5);
        assert_eq!(delete.map(5, true), 5);
        assert_eq!(delete.map(7, false), 5);
        assert_eq!(delete.map(7, true), 5);
        assert_eq!(delete.map(9, false), 5);
        assert_eq!(delete.map(9, true), 5);
        assert_eq!(delete.map(10, false), 6);

        // Expanding replacement: 5..9 -> 5..11 (+2)
        let grow = TextChange {
            start: 5,
            old_end: 9,
            new_end: 11,
        };
        assert_eq!(grow.map(4, true), 4);
        assert_eq!(grow.map(5, false), 5);
        assert_eq!(grow.map(5, true), 11);
        assert_eq!(grow.map(6, false), 5);
        assert_eq!(grow.map(8, true), 11);
        assert_eq!(grow.map(9, false), 11);
        assert_eq!(grow.map(9, true), 11);
        assert_eq!(grow.map(10, false), 12);
        assert_eq!(grow.map(15, true), 17);

        // Shrinking replacement: 5..9 -> 5..7 (-2)
        let shrink = TextChange {
            start: 5,
            old_end: 9,
            new_end: 7,
        };
        assert_eq!(shrink.map(4, false), 4);
        assert_eq!(shrink.map(5, false), 5);
        assert_eq!(shrink.map(5, true), 7);
        assert_eq!(shrink.map(7, false), 5);
        assert_eq!(shrink.map(7, true), 7);
        assert_eq!(shrink.map(9, false), 7);
        assert_eq!(shrink.map(9, true), 7);
        assert_eq!(shrink.map(10, false), 8);
    }

    #[test]
    fn last_change_tracks_replace_undo_redo_and_branches() {
        let mut doc = Document::new("hello world");
        assert_eq!(doc.last_change(), None);

        doc.replace(6..11, "notefart").unwrap();
        assert_eq!(
            doc.last_change(),
            Some(TextChange {
                start: 6,
                old_end: 11,
                new_end: 14,
            })
        );
        assert_eq!(doc.text().to_string(), "hello notefart");

        doc.undo().unwrap();
        assert_eq!(
            doc.last_change(),
            Some(TextChange {
                start: 6,
                old_end: 14,
                new_end: 11,
            })
        );
        assert_eq!(doc.text().to_string(), "hello world");

        doc.redo().unwrap();
        assert_eq!(
            doc.last_change(),
            Some(TextChange {
                start: 6,
                old_end: 11,
                new_end: 14,
            })
        );
        assert_eq!(doc.text().to_string(), "hello notefart");

        // Branch edit after undo
        doc.undo().unwrap();
        doc.replace(0..5, "hi").unwrap();
        assert_eq!(
            doc.last_change(),
            Some(TextChange {
                start: 0,
                old_end: 5,
                new_end: 2,
            })
        );
        assert_eq!(doc.text().to_string(), "hi world");

        doc.undo().unwrap();
        assert_eq!(
            doc.last_change(),
            Some(TextChange {
                start: 0,
                old_end: 2,
                new_end: 5,
            })
        );

        doc.redo().unwrap();
        assert_eq!(
            doc.last_change(),
            Some(TextChange {
                start: 0,
                old_end: 5,
                new_end: 2,
            })
        );
    }

    #[test]
    fn last_change_clears_stale_delta_on_noop_error_and_empty_history() {
        let mut doc = Document::new("a\r\nb");

        // Populate last_change then clear via no-op replace
        doc.replace(0..1, "x").unwrap();
        assert!(doc.last_change().is_some());
        assert_eq!(doc.replace(0..0, ""), Ok(0));
        assert_eq!(doc.last_change(), None);

        // Populate then clear via OutOfBounds error
        doc.replace(0..1, "y").unwrap();
        assert!(doc.last_change().is_some());
        assert!(matches!(
            doc.replace(0..99, "z"),
            Err(EditError::OutOfBounds { .. })
        ));
        assert_eq!(doc.last_change(), None);

        // Populate then clear via InvalidRange error
        doc.replace(0..1, "w").unwrap();
        assert!(doc.last_change().is_some());
        assert!(matches!(
            doc.replace(3..1, "z"),
            Err(EditError::InvalidRange { .. })
        ));
        assert_eq!(doc.last_change(), None);

        // Populate then clear via SplitsGrapheme error (index 2 splits \r\n)
        doc.replace(0..1, "v").unwrap();
        assert!(doc.last_change().is_some());
        assert!(matches!(
            doc.replace(2..2, "z"),
            Err(EditError::SplitsGrapheme { pos: 2 })
        ));
        assert_eq!(doc.last_change(), None);

        // Empty redo clears stale last_change
        assert_eq!(doc.redo(), None);
        assert_eq!(doc.last_change(), None);

        // Drain undo stack, then empty undo clears stale last_change
        while doc.undo().is_some() {
            assert!(doc.last_change().is_some());
        }
        assert_eq!(doc.last_change(), None);

        // Drain redo stack, then empty redo clears stale last_change
        while doc.redo().is_some() {
            assert!(doc.last_change().is_some());
        }
        assert_eq!(doc.last_change(), None);
    }

    #[test]
    fn last_change_records_scalar_splice_across_unicode_seams() {
        // Merging ZWJ cluster: inserting "👩\u{200D}" before "🔬"
        let mut doc = Document::new("🔬");
        let cursor = doc.replace(0..0, "👩\u{200D}").unwrap();
        assert_eq!(cursor, 3);
        let change = doc.last_change().unwrap();
        assert_eq!(
            change,
            TextChange {
                start: 0,
                old_end: 0,
                new_end: 2,
            }
        );
        // Position 0 left stays at 0; right moves to 2 (snaps forward to 3)
        assert_eq!(change.map(0, false), 0);
        let mapped_right = change.map(0, true);
        assert_eq!(mapped_right, 2);
        assert!(!doc.is_grapheme_boundary(mapped_right));
        assert_eq!(doc.next(mapped_right), 3);
        // Position 1 (after 🔬) shifts to 3
        assert_eq!(change.map(1, false), 3);
        assert_eq!(change.map(1, true), 3);

        let undone_cursor = doc.undo().unwrap();
        assert_eq!(undone_cursor, 0);
        assert_eq!(
            doc.last_change(),
            Some(TextChange {
                start: 0,
                old_end: 2,
                new_end: 0,
            })
        );

        // Combining mark appended onto 'e' before 'x': "ex" -> "e\u{0301}x"
        let mut combine_doc = Document::new("ex");
        let c = combine_doc.replace(1..1, "\u{0301}").unwrap();
        assert_eq!(c, 2);
        let ch = combine_doc.last_change().unwrap();
        assert_eq!(
            ch,
            TextChange {
                start: 1,
                old_end: 1,
                new_end: 2,
            }
        );
        assert_eq!(ch.map(0, false), 0);
        assert_eq!(ch.map(1, false), 1);
        assert_eq!(ch.map(1, true), 2);
        assert_eq!(ch.map(2, false), 3);

        // Replacing multi-scalar grapheme "👩‍🔬" in "a👩‍🔬b" with "c"
        let mut replace_doc = Document::new("a👩‍🔬b");
        assert_eq!(replace_doc.len_chars(), 5);
        replace_doc.replace(1..4, "c").unwrap();
        let rch = replace_doc.last_change().unwrap();
        assert_eq!(
            rch,
            TextChange {
                start: 1,
                old_end: 4,
                new_end: 2,
            }
        );
        assert_eq!(rch.map(4, false), 2);
        assert_eq!(rch.map(4, true), 2);
        assert_eq!(rch.map(5, false), 3);
    }
}

//! Batch 0 text foundation validation probe for Notefart.
//!
//! Evaluates Ropey (`1.6.1`, `default-features = false, features = ["simd"]`),
//! `unicode-segmentation` (`1.13.3`), and `unicode-width` (`=0.2.0`, matching
//! Ratatui `0.29.0`) against `String` without defining a production document
//! API or touching filesystem/clipboard state.
//!
//! Run in release mode for realistic benchmark timings:
//! `cargo run --release --example text_probe`

use std::borrow::Cow;
use std::hint::black_box;
use std::time::{Duration, Instant};

use ropey::iter::Chunks;
use ropey::str_utils::byte_to_char_idx;
use ropey::{Rope, RopeSlice};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const TAB_STOP: usize = 4;
const BENCH_LINES: usize = 4_000;
const BENCH_CONSTRUCT_ITERS: usize = 25;
const BENCH_WARMUP_ITERS: usize = 50;
const BENCH_WORKLOAD_ITERS: usize = 1_000;
const BENCH_CLONE_ITERS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    None,
    Lf,
    Crlf,
}

impl LineEnding {
    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LineCellSpan {
    byte_start: usize,
    byte_end: usize,
    char_start: usize,
    char_end: usize,
    col_start: usize,
    col_end: usize,
}

#[derive(Debug, Clone)]
struct LineLayout {
    spans: Vec<LineCellSpan>,
    total_chars: usize,
    total_bytes: usize,
    total_cells: usize,
    chunk_spanning_graphemes: usize,
}

impl LineLayout {
    fn from_str(body: &str, tab_stop: usize) -> Self {
        let mut spans = Vec::new();
        let mut char_offset = 0usize;
        let mut col = 0usize;

        for (byte_start, grapheme) in body.grapheme_indices(true) {
            let char_len = grapheme.chars().count();
            let width = grapheme_cell_width(grapheme, col, tab_stop);
            spans.push(LineCellSpan {
                byte_start,
                byte_end: byte_start + grapheme.len(),
                char_start: char_offset,
                char_end: char_offset + char_len,
                col_start: col,
                col_end: col + width,
            });
            char_offset += char_len;
            col += width;
        }

        Self {
            spans,
            total_chars: char_offset,
            total_bytes: body.len(),
            total_cells: col,
            chunk_spanning_graphemes: 0,
        }
    }

    fn from_rope_slice(body: RopeSlice<'_>, tab_stop: usize) -> Self {
        if let Some(contiguous) = body.as_str() {
            return Self::from_str(contiguous, tab_stop);
        }

        let mut spans = Vec::new();
        let mut byte_offset = 0usize;
        let mut char_offset = 0usize;
        let mut col = 0usize;
        let mut chunk_spanning_graphemes = 0usize;

        for grapheme in RopeGraphemes::new(body) {
            let g_bytes = grapheme.len_bytes();
            let g_chars = grapheme.len_chars();
            let width = if let Some(s) = grapheme.as_str() {
                grapheme_cell_width(s, col, tab_stop)
            } else {
                // Only graphemes physically straddling a chunk seam require a tiny
                // grapheme-local String copy; the line/rope itself is not flattened.
                chunk_spanning_graphemes += 1;
                let owned = String::from(grapheme);
                grapheme_cell_width(&owned, col, tab_stop)
            };

            spans.push(LineCellSpan {
                byte_start: byte_offset,
                byte_end: byte_offset + g_bytes,
                char_start: char_offset,
                char_end: char_offset + g_chars,
                col_start: col,
                col_end: col + width,
            });

            byte_offset += g_bytes;
            char_offset += g_chars;
            col += width;
        }

        Self {
            spans,
            total_chars: char_offset,
            total_bytes: byte_offset,
            total_cells: col,
            chunk_spanning_graphemes,
        }
    }

    /// Maps a terminal cell column to a valid extended-grapheme boundary (char offset)
    /// using the exact same span table produced for rendering.
    ///
    /// - Columns inside a multi-cell grapheme (wide CJK, ZWJ emoji, expanded tab)
    ///   snap to that grapheme's starting char boundary.
    /// - Zero-width spans (`col_start == col_end`) do not absorb the column, so
    ///   clicking a visible cell maps to the visible grapheme rendered there (or EOL
    ///   after trailing zero-width graphemes).
    fn cell_to_grapheme_boundary(&self, target_col: usize) -> usize {
        for span in &self.spans {
            if span.col_end > target_col {
                return span.char_start;
            }
        }
        self.total_chars
    }

    /// Maps a char offset to its starting cell column if it is a valid grapheme
    /// boundary; returns `None` for mid-grapheme scalar offsets.
    fn char_to_cell(&self, char_idx: usize) -> Option<usize> {
        if char_idx == self.total_chars {
            return Some(self.total_cells);
        }
        for span in &self.spans {
            if span.char_start == char_idx {
                return Some(span.col_start);
            }
            if char_idx < span.char_end {
                return None;
            }
        }
        None
    }

    fn assert_byte_coverage(&self, expected_bytes: usize) {
        assert_eq!(self.total_bytes, expected_bytes);
        let mut cursor = 0usize;
        for span in &self.spans {
            assert_eq!(span.byte_start, cursor, "gap/overlap in layout spans");
            assert!(span.byte_start < span.byte_end, "empty span in layout");
            cursor = span.byte_end;
        }
        assert_eq!(cursor, self.total_bytes);
    }
}

fn grapheme_cell_width(grapheme: &str, col: usize, tab_stop: usize) -> usize {
    if grapheme == "\t" {
        tab_stop - (col % tab_stop)
    } else if grapheme == "\r\n"
        || grapheme == "\n"
        || grapheme == "\r"
        || grapheme.chars().all(char::is_control)
    {
        0
    } else {
        grapheme.width()
    }
}

fn expand_tabs_for_render(body: &str, tab_stop: usize) -> String {
    let mut out = String::with_capacity(body.len());
    let mut col = 0usize;
    for grapheme in body.graphemes(true) {
        let w = grapheme_cell_width(grapheme, col, tab_stop);
        if grapheme == "\t" {
            for _ in 0..w {
                out.push(' ');
            }
        } else if !grapheme.chars().all(char::is_control) {
            out.push_str(grapheme);
        }
        col += w;
    }
    out
}

fn split_str_line_ending(line: &str) -> (&str, LineEnding) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, LineEnding::Crlf)
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, LineEnding::Lf)
    } else {
        (line, LineEnding::None)
    }
}

fn split_rope_line_ending(line: RopeSlice<'_>) -> (RopeSlice<'_>, LineEnding) {
    let chars = line.len_chars();
    if chars >= 2 && line.char(chars - 2) == '\r' && line.char(chars - 1) == '\n' {
        (line.slice(..chars - 2), LineEnding::Crlf)
    } else if chars >= 1 && line.char(chars - 1) == '\n' {
        (line.slice(..chars - 1), LineEnding::Lf)
    } else {
        (line, LineEnding::None)
    }
}

/// Chunk-aware extended grapheme iterator over a `RopeSlice`, adapted from Ropey's
/// `graphemes_iter` example. Zero-copy when a grapheme lies within a single chunk.
struct RopeGraphemes<'a> {
    text: RopeSlice<'a>,
    chunks: Chunks<'a>,
    cur_chunk: &'a str,
    cur_chunk_start: usize,
    cursor: GraphemeCursor,
}

impl<'a> RopeGraphemes<'a> {
    fn new(slice: RopeSlice<'a>) -> Self {
        let mut chunks = slice.chunks();
        let first_chunk = chunks.next().unwrap_or("");
        Self {
            text: slice,
            chunks,
            cur_chunk: first_chunk,
            cur_chunk_start: 0,
            cursor: GraphemeCursor::new(0, slice.len_bytes(), true),
        }
    }
}

impl<'a> Iterator for RopeGraphemes<'a> {
    type Item = RopeSlice<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let a = self.cursor.cur_cursor();
        let b = loop {
            match self
                .cursor
                .next_boundary(self.cur_chunk, self.cur_chunk_start)
            {
                Ok(None) => return None,
                Ok(Some(n)) => break n,
                Err(GraphemeIncomplete::NextChunk) => {
                    self.cur_chunk_start += self.cur_chunk.len();
                    self.cur_chunk = self.chunks.next().unwrap_or("");
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_start, _, _) =
                        self.text.chunk_at_byte(idx.saturating_sub(1));
                    self.cursor.provide_context(ctx_chunk, ctx_start);
                }
                Err(other) => unreachable!("unexpected GraphemeIncomplete state: {other:?}"),
            }
        };

        if a < self.cur_chunk_start {
            Some(self.text.byte_slice(a..b))
        } else {
            let a_rel = a - self.cur_chunk_start;
            let b_rel = b - self.cur_chunk_start;
            Some(RopeSlice::from(&self.cur_chunk[a_rel..b_rel]))
        }
    }
}

fn is_rope_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> bool {
    debug_assert!(char_idx <= slice.len_chars());
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
            Err(other) => unreachable!("unexpected GraphemeIncomplete state: {other:?}"),
        }
    }
}

fn prev_rope_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    debug_assert!(char_idx <= slice.len_chars());
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
                let (a, b, c, _) = slice.chunk_at_byte(chunk_byte_idx - 1);
                chunk = a;
                chunk_byte_idx = b;
                chunk_char_idx = c;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx_chunk, ctx_byte_start, _, _) = slice.chunk_at_byte(n.saturating_sub(1));
                gc.provide_context(ctx_chunk, ctx_byte_start);
            }
            Err(other) => unreachable!("unexpected GraphemeIncomplete state: {other:?}"),
        }
    }
}

fn next_rope_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    debug_assert!(char_idx <= slice.len_chars());
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte_idx, mut chunk_char_idx, _) = slice.chunk_at_byte(byte_idx);
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match gc.next_boundary(chunk, chunk_byte_idx) {
            Ok(None) => return slice.len_chars(),
            Ok(Some(n)) => {
                return chunk_char_idx + byte_to_char_idx(chunk, n - chunk_byte_idx);
            }
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_byte_idx += chunk.len();
                let (a, _, c, _) = slice.chunk_at_byte(chunk_byte_idx);
                chunk = a;
                chunk_char_idx = c;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let (ctx_chunk, ctx_byte_start, _, _) = slice.chunk_at_byte(n.saturating_sub(1));
                gc.provide_context(ctx_chunk, ctx_byte_start);
            }
            Err(other) => unreachable!("unexpected GraphemeIncomplete state: {other:?}"),
        }
    }
}

fn is_str_grapheme_boundary(s: &str, char_idx: usize) -> bool {
    let Some(byte_idx) = str_char_to_byte_opt(s, char_idx) else {
        return false;
    };
    let mut gc = GraphemeCursor::new(byte_idx, s.len(), true);
    gc.is_boundary(s, 0)
        .expect("full &str slice has complete context")
}

fn str_char_to_byte_opt(s: &str, char_idx: usize) -> Option<usize> {
    if char_idx == 0 {
        return Some(0);
    }
    let mut count = 0usize;
    for (byte_idx, _) in s.char_indices() {
        if count == char_idx {
            return Some(byte_idx);
        }
        count += 1;
    }
    if count == char_idx {
        Some(s.len())
    } else {
        None
    }
}

fn assert_rope_chunk_invariants(rope: &Rope, expected_str: Option<&str>) {
    if let Some(expected) = expected_str {
        assert_eq!(rope, expected, "rope diverged from string oracle");
        assert_eq!(rope.len_bytes(), expected.len());
        assert_eq!(rope.len_chars(), expected.chars().count());
    }

    let mut byte_sum = 0usize;
    let mut char_sum = 0usize;
    let mut prev_ended_with_cr = false;
    for chunk in rope.chunks() {
        assert!(!chunk.is_empty(), "non-empty rope yielded an empty chunk");
        assert!(
            !(prev_ended_with_cr && chunk.starts_with('\n')),
            "CRLF pair split across Ropey chunk boundary"
        );
        byte_sum += chunk.len();
        char_sum += chunk.chars().count();
        prev_ended_with_cr = chunk.ends_with('\r');
    }
    assert_eq!(byte_sum, rope.len_bytes());
    assert_eq!(char_sum, rope.len_chars());
}

/// Minimal spike-only reversible delta (no generalized transaction/history engine).
#[derive(Debug, Clone, PartialEq, Eq)]
struct EditDelta {
    char_start: usize,
    removed: String,
    inserted_chars: usize,
}

fn try_apply_rope_edit(
    rope: &mut Rope,
    char_start: usize,
    char_end: usize,
    replacement: &str,
) -> Result<EditDelta, &'static str> {
    if char_start > char_end || char_end > rope.len_chars() {
        return Err("char range out of bounds");
    }
    let slice = rope.slice(..);
    if !is_rope_grapheme_boundary(slice, char_start) || !is_rope_grapheme_boundary(slice, char_end)
    {
        return Err("edit range splits an extended grapheme cluster");
    }

    let removed = String::from(rope.slice(char_start..char_end));
    if char_start != char_end {
        rope.remove(char_start..char_end);
    }
    if !replacement.is_empty() {
        rope.insert(char_start, replacement);
    }

    Ok(EditDelta {
        char_start,
        removed,
        inserted_chars: replacement.chars().count(),
    })
}

fn invert_rope_edit(rope: &mut Rope, delta: EditDelta) -> EditDelta {
    let remove_end = delta.char_start + delta.inserted_chars;
    let redo_removed = String::from(rope.slice(delta.char_start..remove_end));
    if delta.inserted_chars > 0 {
        rope.remove(delta.char_start..remove_end);
    }
    if !delta.removed.is_empty() {
        rope.insert(delta.char_start, &delta.removed);
    }
    EditDelta {
        char_start: delta.char_start,
        removed: redo_removed,
        inserted_chars: delta.removed.chars().count(),
    }
}

fn verify_unicode_counts_and_layout() {
    println!("== 1. UTF-8 byte / scalar / grapheme / cell distinctions ==");

    struct SampleCase {
        name: &'static str,
        raw: &'static str,
        expected_bytes: usize,
        expected_chars: usize,
        expected_graphemes: usize,
        expected_body_cells: usize,
        expected_ending: LineEnding,
    }

    let samples = [
        SampleCase {
            name: "combining_acute",
            raw: "e\u{0301}",
            expected_bytes: 3,
            expected_chars: 2,
            expected_graphemes: 1,
            expected_body_cells: 1,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "multi_combining_marks",
            raw: "a\u{0310}e\u{0301}o\u{0308}\u{0332}",
            expected_bytes: 11,
            expected_chars: 7,
            expected_graphemes: 3,
            expected_body_cells: 3,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "cjk_hiragana_kanji",
            raw: "日本語",
            expected_bytes: 9,
            expected_chars: 3,
            expected_graphemes: 3,
            expected_body_cells: 6,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "zwj_woman_scientist",
            raw: "👩‍🔬",
            expected_bytes: 11,
            expected_chars: 3,
            expected_graphemes: 1,
            expected_body_cells: 2,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "zwj_polar_bear",
            raw: "🐻‍❄️",
            expected_bytes: 13,
            expected_chars: 4,
            expected_graphemes: 1,
            expected_body_cells: 2,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "regional_flag_id",
            raw: "🇮🇩",
            expected_bytes: 8,
            expected_chars: 2,
            expected_graphemes: 1,
            expected_body_cells: 2,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "zero_width_space",
            raw: "a\u{200B}b",
            expected_bytes: 5,
            expected_chars: 3,
            expected_graphemes: 3,
            expected_body_cells: 2,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "tabs_tabstop_4",
            raw: "\ta\tb\t",
            expected_bytes: 5,
            expected_chars: 5,
            expected_graphemes: 5,
            expected_body_cells: 12,
            expected_ending: LineEnding::None,
        },
        SampleCase {
            name: "crlf_line",
            raw: "hi\r\n",
            expected_bytes: 4,
            expected_chars: 4,
            expected_graphemes: 3,
            expected_body_cells: 2,
            expected_ending: LineEnding::Crlf,
        },
        SampleCase {
            name: "combined_mixed_crlf",
            raw: "\te\u{0301} 日本 👩‍🔬\u{200B}!\r\n",
            expected_bytes: 29,
            expected_chars: 14,
            expected_graphemes: 10,
            expected_body_cells: 14,
            expected_ending: LineEnding::Crlf,
        },
    ];

    // Confirm raw unicode-width crate semantics for control chars so rationale stays explicit.
    assert_eq!('\t'.width(), None);
    assert_eq!("\t".width(), 1);
    assert_eq!('\r'.width(), None);
    assert_eq!('\n'.width(), None);
    assert_eq!("\r\n".width(), 1);

    for case in &samples {
        let rope = Rope::from_str(case.raw);
        assert_rope_chunk_invariants(&rope, Some(case.raw));

        let raw_bytes = case.raw.len();
        let raw_chars = case.raw.chars().count();
        let raw_graphemes_str = case.raw.graphemes(true).count();
        let raw_graphemes_rope = RopeGraphemes::new(rope.slice(..)).count();

        assert_eq!(raw_bytes, case.expected_bytes, "{}", case.name);
        assert_eq!(raw_chars, case.expected_chars, "{}", case.name);
        assert_eq!(raw_graphemes_str, case.expected_graphemes, "{}", case.name);
        assert_eq!(raw_graphemes_rope, case.expected_graphemes, "{}", case.name);

        let (str_body, str_ending) = split_str_line_ending(case.raw);
        let (rope_body, rope_ending) = split_rope_line_ending(rope.line(0));
        assert_eq!(str_ending, case.expected_ending, "{}", case.name);
        assert_eq!(rope_ending, case.expected_ending, "{}", case.name);
        assert_eq!(rope_body, str_body, "{}", case.name);

        let str_layout = LineLayout::from_str(str_body, TAB_STOP);
        let rope_layout = LineLayout::from_rope_slice(rope_body, TAB_STOP);
        str_layout.assert_byte_coverage(str_body.len());
        rope_layout.assert_byte_coverage(rope_body.len_bytes());
        assert_eq!(str_layout.spans, rope_layout.spans, "{}", case.name);
        assert_eq!(
            rope_layout.total_cells, case.expected_body_cells,
            "{}",
            case.name
        );

        let expanded = expand_tabs_for_render(str_body, TAB_STOP);
        assert_eq!(expanded.width(), rope_layout.total_cells, "{}", case.name);

        println!(
            "  {:<22} bytes={:>2} chars={:>2} graphemes={:>2} body_cells={:>2} ending={} raw_width={}",
            case.name,
            raw_bytes,
            raw_chars,
            raw_graphemes_rope,
            rope_layout.total_cells,
            rope_ending.label(),
            case.raw.width(),
        );
    }

    // Verify Ropey line indexing under CRLF + LF (with default-features=false, features=["simd"]).
    let crlf_doc = "first\r\nsecond\nthird\r\n";
    let crlf_rope = Rope::from_str(crlf_doc);
    assert_eq!(crlf_rope.len_lines(), 4);
    assert_eq!(crlf_rope.line(0), "first\r\n");
    assert_eq!(crlf_rope.line(1), "second\n");
    assert_eq!(crlf_rope.line(2), "third\r\n");
    assert_eq!(crlf_rope.line(3), "");
    // Note: bare CR without LF is NOT split into a separate line when cr_lines is disabled.
    let bare_cr_rope = Rope::from_str("a\rb\n");
    assert_eq!(bare_cr_rope.len_lines(), 2);

    println!("== 2. Tab expansion (stop 4) & cell -> valid grapheme boundary ==");

    // Case A: Tabs at mixed column alignments.
    let tab_line = "\ta\tb\t";
    let tab_rope = Rope::from_str(tab_line);
    let tab_layout = LineLayout::from_rope_slice(tab_rope.slice(..), TAB_STOP);
    assert_eq!(tab_layout.total_cells, 12);
    let expected_tab_cols = [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 1),
        (5, 2),
        (6, 2),
        (7, 2),
        (8, 3),
        (9, 4),
        (10, 4),
        (11, 4),
        (12, 5),
        (20, 5),
    ];
    for &(col, expected_char) in &expected_tab_cols {
        let mapped = tab_layout.cell_to_grapheme_boundary(col);
        assert_eq!(mapped, expected_char, "tab col {col}");
        assert!(is_rope_grapheme_boundary(tab_rope.slice(..), mapped));
        assert!(is_str_grapheme_boundary(tab_line, mapped));
    }
    assert_eq!(tab_layout.char_to_cell(0), Some(0));
    assert_eq!(tab_layout.char_to_cell(1), Some(4));
    assert_eq!(tab_layout.char_to_cell(2), Some(5));
    assert_eq!(tab_layout.char_to_cell(3), Some(8));
    assert_eq!(tab_layout.char_to_cell(4), Some(9));
    assert_eq!(tab_layout.char_to_cell(5), Some(12));

    // Case B: Wide CJK + ZWJ emoji + inside-wide-cell hit testing.
    let wide_line = "語👩‍🔬!";
    let wide_rope = Rope::from_str(wide_line);
    let wide_layout = LineLayout::from_rope_slice(wide_rope.slice(..), TAB_STOP);
    assert_eq!(wide_layout.total_cells, 5);
    assert_eq!(wide_layout.cell_to_grapheme_boundary(0), 0);
    assert_eq!(wide_layout.cell_to_grapheme_boundary(1), 0); // inside wide '語'
    assert_eq!(wide_layout.cell_to_grapheme_boundary(2), 1);
    assert_eq!(wide_layout.cell_to_grapheme_boundary(3), 1); // inside wide '👩‍🔬' (never 2 or 3!)
    assert_eq!(wide_layout.cell_to_grapheme_boundary(4), 4);
    assert_eq!(wide_layout.cell_to_grapheme_boundary(5), 5);
    assert_eq!(wide_layout.char_to_cell(0), Some(0));
    assert_eq!(wide_layout.char_to_cell(1), Some(2));
    assert_eq!(wide_layout.char_to_cell(2), None); // inside ZWJ sequence
    assert_eq!(wide_layout.char_to_cell(3), None); // inside ZWJ sequence
    assert_eq!(wide_layout.char_to_cell(4), Some(4));
    assert_eq!(wide_layout.char_to_cell(5), Some(5));

    // Case C: Zero-width boundaries (middle, leading with combining mark, trailing).
    let zw_mid = "a\u{200B}b";
    let zw_mid_rope = Rope::from_str(zw_mid);
    let zw_mid_layout = LineLayout::from_rope_slice(zw_mid_rope.slice(..), TAB_STOP);
    assert_eq!(zw_mid_layout.total_cells, 2);
    assert_eq!(zw_mid_layout.char_to_cell(0), Some(0));
    assert_eq!(zw_mid_layout.char_to_cell(1), Some(1)); // before \u{200B}
    assert_eq!(zw_mid_layout.char_to_cell(2), Some(1)); // after \u{200B}, before 'b'
    assert_eq!(zw_mid_layout.char_to_cell(3), Some(2));
    assert_eq!(zw_mid_layout.cell_to_grapheme_boundary(0), 0);
    assert_eq!(zw_mid_layout.cell_to_grapheme_boundary(1), 2);
    assert_eq!(zw_mid_layout.cell_to_grapheme_boundary(2), 3);
    for col in 0..=3 {
        let ch = zw_mid_layout.cell_to_grapheme_boundary(col);
        assert!(is_rope_grapheme_boundary(zw_mid_rope.slice(..), ch));
    }

    let zw_lead = "\u{200B}e\u{0301}";
    let zw_lead_rope = Rope::from_str(zw_lead);
    let zw_lead_layout = LineLayout::from_rope_slice(zw_lead_rope.slice(..), TAB_STOP);
    assert_eq!(zw_lead_layout.total_cells, 1);
    assert_eq!(zw_lead_layout.cell_to_grapheme_boundary(0), 1); // start of e\u{0301}
    assert_eq!(zw_lead_layout.cell_to_grapheme_boundary(1), 3); // EOL
    assert_eq!(zw_lead_layout.char_to_cell(0), Some(0));
    assert_eq!(zw_lead_layout.char_to_cell(1), Some(0));
    assert_eq!(zw_lead_layout.char_to_cell(2), None); // inside e\u{0301}
    assert_eq!(zw_lead_layout.char_to_cell(3), Some(1));

    let zw_trail = "a\u{200B}";
    let zw_trail_rope = Rope::from_str(zw_trail);
    let zw_trail_layout = LineLayout::from_rope_slice(zw_trail_rope.slice(..), TAB_STOP);
    assert_eq!(zw_trail_layout.total_cells, 1);
    assert_eq!(zw_trail_layout.cell_to_grapheme_boundary(0), 0);
    assert_eq!(zw_trail_layout.cell_to_grapheme_boundary(1), 2);

    // Case D: Combined mixed line cell -> boundary round-trip and stepping.
    let mixed_rope = Rope::from_str("\te\u{0301} 日本 👩‍🔬\u{200B}!\r\n");
    let (mixed_body, _) = split_rope_line_ending(mixed_rope.line(0));
    let mixed_layout = LineLayout::from_rope_slice(mixed_body, TAB_STOP);
    assert_eq!(mixed_layout.total_cells, 14);
    for col in 0..=16 {
        let ch = mixed_layout.cell_to_grapheme_boundary(col);
        assert!(
            is_rope_grapheme_boundary(mixed_body, ch),
            "col {col} mapped to non-boundary char {ch}"
        );
        if col < mixed_layout.total_cells {
            let start_col = mixed_layout.char_to_cell(ch).expect("valid boundary");
            assert!(start_col <= col);
        } else {
            assert_eq!(ch, mixed_body.len_chars());
        }
    }
    println!(
        "  Verified tab stop 4, inside-wide-cell, and zero-width mappings across {} spans",
        mixed_layout.spans.len()
    );
}

fn apply_checked_edit(
    rope: &mut Rope,
    oracle: &mut String,
    undo_stack: &mut Vec<EditDelta>,
    snapshots: &mut Vec<String>,
    char_start: usize,
    char_end: usize,
    replacement: &str,
) {
    let delta = try_apply_rope_edit(rope, char_start, char_end, replacement)
        .expect("valid grapheme-boundary edit");
    let b_start = str_char_to_byte_opt(oracle, char_start).unwrap();
    let b_end = str_char_to_byte_opt(oracle, char_end).unwrap();
    assert_eq!(&oracle[b_start..b_end], delta.removed);
    oracle.replace_range(b_start..b_end, replacement);
    assert_rope_chunk_invariants(rope, Some(oracle));
    undo_stack.push(delta);
    snapshots.push(oracle.clone());
}

fn verify_reversible_edits_and_boundaries() {
    println!("== 3. Grapheme-boundary edits, retained deltas, and undo ==");

    let original = concat!(
        "alpha\te\u{0301} 日本語 👩‍🔬!\r\n",
        "second\u{200B}line 🐻‍❄️ 🇮🇩 a\u{0310}o\u{0308}\u{0332}\n",
        "\tlast\trow\r\n",
    );
    let mut rope = Rope::from_str(original);
    let mut oracle = original.to_string();
    assert_rope_chunk_invariants(&rope, Some(&oracle));

    // Reject mid-grapheme edits explicitly (combining mark, ZWJ emoji, regional flag, CRLF).
    let acute_char = rope.byte_to_char(original.find("e\u{0301}").unwrap());
    let zwj_char = rope.byte_to_char(original.find("👩‍🔬").unwrap());
    let flag_char = rope.byte_to_char(original.find("🇮🇩").unwrap());
    let crlf_char = rope.byte_to_char(original.find("\r\n").unwrap());

    assert!(!is_rope_grapheme_boundary(rope.slice(..), acute_char + 1));
    assert!(!is_rope_grapheme_boundary(rope.slice(..), zwj_char + 1));
    assert!(!is_rope_grapheme_boundary(rope.slice(..), zwj_char + 2));
    assert!(!is_rope_grapheme_boundary(rope.slice(..), flag_char + 1));
    assert!(!is_rope_grapheme_boundary(rope.slice(..), crlf_char + 1));

    assert!(try_apply_rope_edit(&mut rope, acute_char + 1, acute_char + 2, "").is_err());
    assert!(try_apply_rope_edit(&mut rope, zwj_char + 1, zwj_char + 1, "X").is_err());
    assert!(try_apply_rope_edit(&mut rope, flag_char + 1, flag_char + 2, "").is_err());
    assert!(try_apply_rope_edit(&mut rope, crlf_char + 1, crlf_char + 1, "X").is_err());
    assert_eq!(rope, original);

    let mut undo_stack: Vec<EditDelta> = Vec::new();
    let mut snapshots: Vec<String> = vec![original.to_string()];
    // Edit 1: insert prefix at start.
    apply_checked_edit(
        &mut rope,
        &mut oracle,
        &mut undo_stack,
        &mut snapshots,
        0,
        0,
        "// hdr\t",
    );

    // Edit 2: replace combining cluster "e\u{0301}" with "é+👨‍🚀".
    let e_start = rope.byte_to_char(oracle.find("e\u{0301}").unwrap());
    let e_end = next_rope_grapheme_boundary(rope.slice(..), e_start);
    assert_eq!(e_end - e_start, 2);
    apply_checked_edit(
        &mut rope,
        &mut oracle,
        &mut undo_stack,
        &mut snapshots,
        e_start,
        e_end,
        "é+👨‍🚀",
    );

    // Edit 3: delete across ZWJ emoji + '!' + CRLF line ending.
    let del_start = rope.byte_to_char(oracle.find("👩‍🔬!\r\n").unwrap());
    let del_end = del_start + "👩‍🔬!\r\n".chars().count();
    apply_checked_edit(
        &mut rope,
        &mut oracle,
        &mut undo_stack,
        &mut snapshots,
        del_start,
        del_end,
        " ",
    );

    // Edit 4: insert multiline CRLF+tab+CJK inside line 1.
    let line1_char = rope.line_to_char(1);
    apply_checked_edit(
        &mut rope,
        &mut oracle,
        &mut undo_stack,
        &mut snapshots,
        line1_char,
        line1_char,
        "中\r\n\t",
    );

    // Edit 5: delete "🐻‍❄️ 🇮🇩" (4 + 1 + 2 = 7 scalars) from the middle.
    let bear_start = rope.byte_to_char(oracle.find("🐻‍❄️ 🇮🇩").unwrap());
    let bear_end = bear_start + "🐻‍❄️ 🇮🇩".chars().count();
    apply_checked_edit(
        &mut rope,
        &mut oracle,
        &mut undo_stack,
        &mut snapshots,
        bear_start,
        bear_end,
        "",
    );

    let final_edited = oracle.clone();
    assert_ne!(final_edited, original);

    // Undo all edits in reverse order and check every intermediate snapshot.
    let mut redo_stack = Vec::new();
    while let Some(delta) = undo_stack.pop() {
        snapshots.pop();
        let redo_delta = invert_rope_edit(&mut rope, delta);
        redo_stack.push(redo_delta);
        let expected = snapshots.last().expect("baseline snapshot exists");
        assert_rope_chunk_invariants(&rope, Some(expected));
    }
    assert_eq!(rope, original);

    // Redo forward, then undo back to exact original once more.
    while let Some(redo) = redo_stack.pop() {
        let undo_again = invert_rope_edit(&mut rope, redo);
        undo_stack.push(undo_again);
    }
    assert_eq!(rope, final_edited);

    while let Some(delta) = undo_stack.pop() {
        invert_rope_edit(&mut rope, delta);
    }
    assert_eq!(rope, original);

    println!("  Verified 4 rejected mid-grapheme edits and 5-step edit/undo/redo round-trip.");
}

fn verify_multi_chunk_rope_behavior() {
    println!("== 4. Multi-chunk Rope behavior (no whole-rope flatten) ==");

    // 4A. Structural sharing across a ~256 KiB multi-chunk document during a middle edit.
    let dataset = build_modest_dataset(BENCH_LINES);
    let mut rope = Rope::from_str(&dataset);
    let chunk_count = rope.chunks().count();
    assert!(chunk_count > 1, "fixture must exercise multiple chunks");

    let baseline = rope.clone();

    let mid_line_char = rope.line_to_char(BENCH_LINES / 2);
    let delta_ins = try_apply_rope_edit(&mut rope, mid_line_char, mid_line_char, "// local_edit ")
        .expect("line start is valid grapheme boundary");
    let delta_del = try_apply_rope_edit(
        &mut rope,
        mid_line_char + "// local_edit ".chars().count(),
        mid_line_char + "// local_edit ".chars().count() + 5,
        "",
    )
    .expect("ASCII prefix deletion at grapheme boundary");

    assert_eq!(
        baseline,
        dataset.as_str(),
        "editing must preserve an earlier snapshot"
    );
    assert_ne!(rope, baseline);

    invert_rope_edit(&mut rope, delta_del);
    invert_rope_edit(&mut rope, delta_ins);
    assert_eq!(rope, baseline);

    // 4B. Single long line spanning multiple chunks with a grapheme crossing a chunk seam.
    let mut seam_sample: Option<(String, Rope, usize, usize)> = None;
    for pad in 0..16usize {
        let mut line = "x".repeat(pad);
        for _ in 0..360 {
            line.push_str("👩‍🔬");
        }
        let candidate = Rope::from_str(&line);
        let mut char_acc = 0usize;
        let mut byte_acc = 0usize;
        let total_chunks = candidate.chunks().count();
        assert!(total_chunks >= 3);
        for (chunk_idx, chunk) in candidate.chunks().enumerate() {
            byte_acc += chunk.len();
            char_acc += chunk.chars().count();
            if chunk_idx + 1 < total_chunks
                && !is_rope_grapheme_boundary(candidate.slice(..), char_acc)
            {
                seam_sample = Some((line, candidate, byte_acc, char_acc));
                break;
            }
        }
        if seam_sample.is_some() {
            break;
        }
    }

    let (long_str, long_rope, seam_byte, seam_char) =
        seam_sample.expect("padding sweep guarantees a ZWJ emoji split across a Ropey chunk seam");
    let long_chunks = long_rope.chunks().count();
    let line_slice = long_rope.line(0);
    assert!(line_slice.as_str().is_none());

    // Demonstrate line-local Cow copy vs chunk-streamed LineLayout.
    let line_local_cow: Cow<'_, str> = Cow::from(line_slice);
    assert_eq!(line_local_cow.as_ref(), long_str.as_str());

    let prev_bound = prev_rope_grapheme_boundary(line_slice, seam_char);
    let next_bound = next_rope_grapheme_boundary(line_slice, seam_char);
    assert!(prev_bound < seam_char && seam_char < next_bound);
    assert_eq!(next_bound - prev_bound, 3); // "👩‍🔬" has 3 scalars
    assert!(is_rope_grapheme_boundary(line_slice, prev_bound));
    assert!(!is_rope_grapheme_boundary(line_slice, seam_char));
    assert!(is_rope_grapheme_boundary(line_slice, next_bound));

    let str_layout = LineLayout::from_str(&long_str, TAB_STOP);
    let rope_layout = LineLayout::from_rope_slice(line_slice, TAB_STOP);
    assert_eq!(str_layout.spans, rope_layout.spans);
    rope_layout.assert_byte_coverage(long_str.len());

    println!(
        "  Doc: {} bytes, {} chunks; snapshot preserved across middle edit and undo.",
        dataset.len(),
        chunk_count
    );
    println!(
        "  Long single line: {} bytes across {} chunks; seam at byte {} (char {}) split '👩‍🔬' [{}..{}]; chunk-spanning graphemes copied locally={}.",
        long_str.len(),
        long_chunks,
        seam_byte,
        seam_char,
        prev_bound,
        next_bound,
        rope_layout.chunk_spanning_graphemes
    );
}

fn build_modest_dataset(line_count: usize) -> String {
    let mut out = String::with_capacity(line_count * 72);
    for idx in 0..line_count {
        let line = match idx % 4 {
            0 => format!("line_{idx:04}:\talpha e\u{0301} beta 日本語 👩‍🔬 marker_{idx:04} end\r\n"),
            1 => format!("line_{idx:04}: middle_token_{idx:04} 🐻‍❄️ \u{200B} ascii_segment_ok\n"),
            2 => format!("line_{idx:04}:\ta\u{0310}o\u{0308}\u{0332} 🇮🇩 payload_{idx:04} tail\r\n"),
            _ => format!("line_{idx:04}: plain_ascii_segment_for_editing_{idx:04}_padding\n"),
        };
        out.push_str(&line);
    }
    out
}

fn format_ns_per_op(duration: Duration, iters: usize) -> f64 {
    duration.as_secs_f64() * 1e9 / (iters as f64)
}

fn measure_rope_vs_string_workload() {
    println!("== 5. Measured Rope vs String middle edit + undo workload ==");

    let dataset = build_modest_dataset(BENCH_LINES);
    let probe_rope = Rope::from_str(&dataset);
    let chunk_count = probe_rope.chunks().count();

    // 1) Separate initial construction measurement.
    let rope_build_start = Instant::now();
    for _ in 0..BENCH_CONSTRUCT_ITERS {
        let built = Rope::from_str(black_box(dataset.as_str()));
        black_box(built.len_bytes());
    }
    let rope_build_dur = rope_build_start.elapsed();

    let str_build_start = Instant::now();
    for _ in 0..BENCH_CONSTRUCT_ITERS {
        let built = String::from(black_box(dataset.as_str()));
        black_box(built.len());
    }
    let str_build_dur = str_build_start.elapsed();

    // 2) Precompute 16 valid grapheme-boundary middle locations (around line 50%).
    const TARGET_SLOTS: usize = 16;
    let mid_start_line = (BENCH_LINES / 2) - (TARGET_SLOTS / 2);
    let mut positions: Vec<(usize, usize)> = Vec::with_capacity(TARGET_SLOTS);
    for offset in 0..TARGET_SLOTS {
        let line_idx = mid_start_line + offset;
        let char_pos = probe_rope.line_to_char(line_idx);
        let byte_pos = probe_rope.char_to_byte(char_pos);
        assert!(is_rope_grapheme_boundary(probe_rope.slice(..), char_pos));
        assert!(is_rope_grapheme_boundary(
            probe_rope.slice(..),
            char_pos + 10
        ));
        assert_eq!(&dataset[byte_pos..byte_pos + 5], "line_");
        positions.push((char_pos, byte_pos));
    }

    const INS_TEXT: &str = "/*Δ漢👩‍🔬*/";
    let ins_chars = INS_TEXT.chars().count();
    let ins_bytes = INS_TEXT.len();
    const DEL_ASCII_LEN: usize = 10; // "line_XXXX:" (10 ASCII bytes == 10 chars)

    let run_rope_cycle = |rope: &mut Rope, char_pos: usize| {
        rope.insert(char_pos, black_box(INS_TEXT));
        let del_start = char_pos + ins_chars;
        let del_end = del_start + DEL_ASCII_LEN;
        let removed = String::from(rope.slice(del_start..del_end));
        rope.remove(del_start..del_end);
        rope.insert(del_start, black_box(removed.as_str()));
        rope.remove(char_pos..del_start);
        black_box(removed);
    };

    let run_string_cycle = |buf: &mut String, byte_pos: usize| {
        buf.insert_str(byte_pos, black_box(INS_TEXT));
        let del_start = byte_pos + ins_bytes;
        let del_end = del_start + DEL_ASCII_LEN;
        let removed: String = buf.drain(del_start..del_end).collect();
        buf.insert_str(del_start, black_box(removed.as_str()));
        buf.drain(byte_pos..del_start);
        black_box(removed);
    };

    // Warmup and benchmark Rope.
    let mut bench_rope = Rope::from_str(&dataset);
    for i in 0..BENCH_WARMUP_ITERS {
        let (char_pos, _) = positions[i % positions.len()];
        run_rope_cycle(black_box(&mut bench_rope), char_pos);
    }
    assert_eq!(bench_rope, dataset);

    let rope_work_start = Instant::now();
    for i in 0..BENCH_WORKLOAD_ITERS {
        let (char_pos, _) = positions[i % positions.len()];
        run_rope_cycle(black_box(&mut bench_rope), char_pos);
    }
    let rope_work_dur = rope_work_start.elapsed();
    assert_rope_chunk_invariants(&bench_rope, Some(&dataset));

    // Warmup and benchmark String (using pre-known byte_pos: best-case String without O(N) char scan).
    let mut bench_string = dataset.clone();
    for i in 0..BENCH_WARMUP_ITERS {
        let (_, byte_pos) = positions[i % positions.len()];
        run_string_cycle(black_box(&mut bench_string), byte_pos);
    }
    assert_eq!(bench_string, dataset);

    let str_work_start = Instant::now();
    for i in 0..BENCH_WORKLOAD_ITERS {
        let (_, byte_pos) = positions[i % positions.len()];
        run_string_cycle(black_box(&mut bench_string), byte_pos);
    }
    let str_work_dur = str_work_start.elapsed();
    assert_eq!(bench_string, dataset);

    // 3) Measure snapshot clone cost (relevant to async save / background highlight snapshots).
    let rope_clone_start = Instant::now();
    for _ in 0..BENCH_CLONE_ITERS {
        let snap = black_box(black_box(&bench_rope).clone());
        black_box(snap.len_bytes());
    }
    let rope_clone_dur = rope_clone_start.elapsed();

    let str_clone_start = Instant::now();
    for _ in 0..BENCH_CLONE_ITERS {
        let snap = black_box(black_box(&bench_string).clone());
        black_box(snap.len());
    }
    let str_clone_dur = str_clone_start.elapsed();

    println!(
        "  Dataset: {} lines, {} bytes ({:.1} KiB), {} chars, {} Rope chunks",
        BENCH_LINES,
        dataset.len(),
        dataset.len() as f64 / 1024.0,
        probe_rope.len_chars(),
        chunk_count
    );
    println!(
        "  Construction ({BENCH_CONSTRUCT_ITERS}x): Rope={:.3} ms ({:.0} ns/op) | String={:.3} ms ({:.0} ns/op)",
        rope_build_dur.as_secs_f64() * 1e3,
        format_ns_per_op(rope_build_dur, BENCH_CONSTRUCT_ITERS),
        str_build_dur.as_secs_f64() * 1e3,
        format_ns_per_op(str_build_dur, BENCH_CONSTRUCT_ITERS),
    );
    println!(
        "  Middle edit+undo ({BENCH_WORKLOAD_ITERS}x): Rope={:.3} ms ({:.0} ns/cycle) | String(byte-indexed)={:.3} ms ({:.0} ns/cycle)",
        rope_work_dur.as_secs_f64() * 1e3,
        format_ns_per_op(rope_work_dur, BENCH_WORKLOAD_ITERS),
        str_work_dur.as_secs_f64() * 1e3,
        format_ns_per_op(str_work_dur, BENCH_WORKLOAD_ITERS),
    );
    println!(
        "  Snapshot clone ({BENCH_CLONE_ITERS}x):   Rope={:.3} ms ({:.0} ns/clone) | String={:.3} ms ({:.0} ns/clone)",
        rope_clone_dur.as_secs_f64() * 1e3,
        format_ns_per_op(rope_clone_dur, BENCH_CLONE_ITERS),
        str_clone_dur.as_secs_f64() * 1e3,
        format_ns_per_op(str_clone_dur, BENCH_CLONE_ITERS),
    );
    println!(
        "  Note: String cycle above receives pre-known byte offsets; without a separate line/char table, String also pays O(N) for line/char->byte lookup, whereas Ropey maintains byte/char/line/UTF-16 indices in O(log N)."
    );
}

fn print_summary() {
    println!("== 6. Metadata, rationale, and remaining production work ==");
    println!(
        "  Build profile: {} | unicode-segmentation={:?} | unicode-width={:?} (pinned =0.2.0 for ratatui 0.29)",
        if cfg!(debug_assertions) {
            "debug (run --release for realistic timings)"
        } else {
            "release"
        },
        unicode_segmentation::UNICODE_VERSION,
        unicode_width::UNICODE_VERSION,
    );
    println!(
        "  Licenses (observed in Cargo metadata/sources): ropey 1.6.1 = MIT (smallvec 1.16.5, str_indices 0.4.4 = MIT OR Apache-2.0); unicode-segmentation 1.13.3 = MIT OR Apache-2.0; unicode-width 0.2.0 = MIT OR Apache-2.0."
    );
    println!(
        "  Rationale: Ropey provides O(log N) line/char/byte conversion, O(log N) middle edits without flattening, CRLF-safe chunk seams, and O(1) Arc snapshot clones (beneficial for shared views, dirty checkpoints, and async save/highlight)."
    );
    println!(
        "  Caveat - Unicode width vs terminal reality: unicode-width 0.2.0 (Unicode 15.1.0) vs unicode-segmentation 1.13.3 (Unicode 17.0.0) can differ from actual terminal emulators/fonts (ZWJ fallback splitting, ambiguous width, variation selectors, Brahmic ligatures, Nerd Font PUA glyphs)."
    );
    println!(
        "  Remaining production work: (1) chunk-boundary-safe grapheme cursor/editing wrappers, (2) multi-view cursor/selection/viewport delta remapping, (3) document revision IDs + saved checkpoint vs undo dirty tracking, (4) explicit LF/CRLF/BOM/mixed-EOL & unsafe/adversarial input limits, (5) line-local Cow<&str> vs streaming adapters for highlighting."
    );
}

fn main() {
    println!("Notefart text_probe (deterministic Unicode + Ropey/String validation)");
    verify_unicode_counts_and_layout();
    verify_reversible_edits_and_boundaries();
    verify_multi_chunk_rope_behavior();
    measure_rope_vs_string_workload();
    print_summary();
    println!("ALL text_probe assertions passed.");
}

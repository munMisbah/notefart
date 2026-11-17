//! Literal, case-sensitive document search and replacement.
//!
//! A [`Query`] compiles the needle once into streaming KMP state (one table for
//! forward searches and one for backward structural searches). Searches walk the
//! document's Unicode scalars through Ropey iterators and never materialize the
//! whole document as a `String`.
//!
//! Matching is done on Unicode scalar offsets, and a structural match is accepted
//! only when both of its endpoints are grapheme-cluster boundaries in the document.
//! This keeps edits from splitting combining sequences while still allowing a
//! needle that itself contains combining marks.
//!
//! `replace_all` scans non-overlapping matches left-to-right, composes a single
//! replacement window, and applies it with one `Document::replace` call so the whole
//! operation is one undo transaction. The composed output size is checked before the
//! buffer is allocated, and no list of match ranges is retained.

use std::fmt;
use std::ops::{ControlFlow, Range};

use crate::document::{Document, EditError};

/// Maximum byte size of the composed replacement window (64 MiB).
const MAX_REPLACE_OUTPUT: usize = 64 * 1024 * 1024;

/// Errors from [`replace_all`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// The document rejected the composed replacement.
    Edit(EditError),
    /// The composed replacement window would exceed the 64 MiB output limit.
    OutputTooLarge { limit: usize },
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edit(error) => write!(f, "{error}"),
            Self::OutputTooLarge { limit } => {
                write!(f, "replacement output exceeds {limit} bytes")
            }
        }
    }
}

impl std::error::Error for SearchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Edit(error) => Some(error),
            Self::OutputTooLarge { .. } => None,
        }
    }
}

impl From<EditError> for SearchError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}

/// Bounded prefix of accepted match start offsets for a [`Query`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchIndex {
    /// Sorted Unicode scalar start offsets of accepted matches, at most `limit`.
    pub starts: Vec<usize>,
    /// Whether at least one additional valid match exists beyond the prefix.
    pub has_more: bool,
}

/// A compiled literal, case-sensitive needle.
///
/// The needle is matched as a sequence of Unicode scalars. Regex syntax and
/// case folding are not interpreted.
#[derive(Debug, Clone)]
pub struct Query {
    needle: String,
    pattern: Vec<char>,
    failure: Vec<usize>,
    reverse_pattern: Vec<char>,
    reverse_failure: Vec<usize>,
}

impl Query {
    /// Compiles `needle`. Empty needles are rejected.
    pub fn new(needle: &str) -> Option<Self> {
        if needle.is_empty() {
            return None;
        }
        let pattern: Vec<char> = needle.chars().collect();
        if pattern.is_empty() {
            return None;
        }
        let mut reverse_pattern = pattern.clone();
        reverse_pattern.reverse();
        let failure = failure_table(&pattern);
        let reverse_failure = failure_table(&reverse_pattern);
        Some(Self {
            needle: needle.to_owned(),
            pattern,
            failure,
            reverse_pattern,
            reverse_failure,
        })
    }

    /// The literal needle used to build this query.
    pub fn needle(&self) -> &str {
        &self.needle
    }

    /// Finds the next or previous match.
    ///
    /// `from` is an inclusive character offset and is clamped to the document
    /// length. A forward search considers matches whose start is at or after
    /// `from`; a reverse search considers matches whose start is at or before
    /// `from`. If the direction has no match, the search wraps exactly once to the
    /// opposite end of the document. Returned ranges are character (Unicode
    /// scalar) offsets and their endpoints are grapheme-cluster boundaries.
    pub fn next(&self, doc: &Document, from: usize, reverse: bool) -> Option<Range<usize>> {
        let len = doc.len_chars();
        let from = from.min(len);
        if reverse {
            // A scan from the document length already covers every start.
            if from < len {
                self.find_reverse(doc, from)
                    .or_else(|| self.find_reverse(doc, len))
            } else {
                self.find_reverse(doc, from)
            }
        } else if from > 0 {
            self.find_forward(doc, from)
                .or_else(|| self.find_forward(doc, 0))
        } else {
            self.find_forward(doc, from)
        }
    }

    /// Enumerates up to `limit` valid match starts in ascending scalar order.
    ///
    /// Performs a single streaming pass over `doc` from the start, including
    /// overlapping matches whose endpoints are both grapheme-cluster boundaries.
    /// Scanning stops once an additional valid match beyond `limit` is found,
    /// reporting `has_more = true`.
    pub fn match_index(&self, doc: &Document, limit: usize) -> MatchIndex {
        let mut starts = Vec::new();
        let mut has_more = false;
        self.scan_forward(doc, 0, |range| {
            if starts.len() == limit {
                has_more = true;
                ControlFlow::Break(())
            } else {
                if starts.is_empty() {
                    starts.reserve_exact(limit.min(doc.len_chars()));
                }
                starts.push(range.start);
                ControlFlow::Continue(())
            }
        });
        MatchIndex { starts, has_more }
    }

    /// First accepted match starting at or after `from`, without wrapping.
    fn find_forward(&self, doc: &Document, from: usize) -> Option<Range<usize>> {
        let mut found = None;
        self.scan_forward(doc, from, |range| {
            found = Some(range);
            ControlFlow::Break(())
        });
        found
    }

    /// Streams accepted matches starting at or after `from` in ascending order.
    fn scan_forward(
        &self,
        doc: &Document,
        from: usize,
        mut on_match: impl FnMut(Range<usize>) -> ControlFlow<()>,
    ) {
        let len = doc.len_chars();
        let width = self.pattern.len();
        if width == 0 || width > len || from > len {
            return;
        }

        let mut matched = 0usize;
        for (offset, ch) in doc.text().chars_at(from).enumerate() {
            while matched > 0 && self.pattern[matched] != ch {
                matched = self.failure[matched - 1];
            }
            if self.pattern[matched] == ch {
                matched += 1;
            }
            if matched == width {
                let start = from + offset + 1 - width;
                matched = self.failure[matched - 1];
                let end = start + width;
                if doc.is_grapheme_boundary(start)
                    && doc.is_grapheme_boundary(end)
                    && on_match(start..end).is_break()
                {
                    return;
                }
            }
        }
    }

    /// Greatest accepted match start at or before `from`, without wrapping.
    ///
    /// The document is consumed backwards into the reversed needle, so the first
    /// completion is the greatest start and the scan can stop early.
    fn find_reverse(&self, doc: &Document, from: usize) -> Option<Range<usize>> {
        let len = doc.len_chars();
        let width = self.pattern.len();
        if width == 0 || width > len {
            return None;
        }
        let from = from.min(len);
        // A match with a start <= from must end at or before from + width.
        let mut index = from.saturating_add(width).saturating_sub(1).min(len - 1);
        let mut matched = 0usize;
        let mut chars = doc.text().chars_at(index + 1);

        while let Some(ch) = chars.prev() {
            while matched > 0 && self.reverse_pattern[matched] != ch {
                matched = self.reverse_failure[matched - 1];
            }
            if self.reverse_pattern[matched] == ch {
                matched += 1;
            }
            if matched == width {
                let start = index;
                matched = self.reverse_failure[matched - 1];
                let end = start + width;
                if doc.is_grapheme_boundary(start) && doc.is_grapheme_boundary(end) {
                    return Some(start..end);
                }
            }
            if index == 0 {
                break;
            }
            index -= 1;
        }
        None
    }
}

fn failure_table(pattern: &[char]) -> Vec<usize> {
    let mut table = vec![0; pattern.len()];
    let mut matched = 0usize;
    for index in 1..pattern.len() {
        while matched > 0 && pattern[index] != pattern[matched] {
            matched = table[matched - 1];
        }
        if pattern[index] == pattern[matched] {
            matched += 1;
        }
        table[index] = matched;
    }
    table
}

/// Replaces every non-overlapping literal occurrence of `query` with
/// `replacement`, scanning the document left-to-right.
///
/// Matches are chosen greedily by their start position: after a match at
/// `start..end`, scanning resumes at `end`, so overlapping occurrences are not
/// replaced (for example `aa` occurs once in `aaa`, at `0..2`). Only matches whose
/// endpoints are grapheme-cluster boundaries are used.
///
/// The affected window from the first match start to the last match end is
/// composed in memory and installed with a single document edit. That makes the
/// whole operation one undo transaction, even when matches are far apart, while
/// preserving every non-matching byte. The composed size is checked before
/// allocation against a 64 MiB limit; an oversized result is rejected without
/// mutating the document.
///
/// Returns the number of replacements. An empty match set, or a replacement equal
/// to the query needle, returns `0` and neither dirties the document nor creates a
/// history entry.
pub fn replace_all(
    doc: &mut Document,
    query: &Query,
    replacement: &str,
) -> Result<usize, SearchError> {
    if replacement == query.needle() {
        return Ok(0);
    }

    // Pass 1: size and locate the composed window without retaining match ranges.
    let mut scan_from = 0usize;
    let mut first_start = None;
    let mut last_end = 0usize;
    let mut count = 0usize;
    while let Some(found) = query.find_forward(doc, scan_from) {
        first_start.get_or_insert(found.start);
        last_end = found.end;
        count += 1;
        scan_from = found.end;
    }
    let Some(first_start) = first_start else {
        return Ok(0);
    };

    let window_bytes = doc
        .text()
        .char_to_byte(last_end)
        .saturating_sub(doc.text().char_to_byte(first_start));
    let removed_bytes =
        count
            .checked_mul(query.needle.len())
            .ok_or(SearchError::OutputTooLarge {
                limit: MAX_REPLACE_OUTPUT,
            })?;
    let unmatched_bytes =
        window_bytes
            .checked_sub(removed_bytes)
            .ok_or(SearchError::OutputTooLarge {
                limit: MAX_REPLACE_OUTPUT,
            })?;
    let inserted_bytes =
        count
            .checked_mul(replacement.len())
            .ok_or(SearchError::OutputTooLarge {
                limit: MAX_REPLACE_OUTPUT,
            })?;
    let output_bytes =
        unmatched_bytes
            .checked_add(inserted_bytes)
            .ok_or(SearchError::OutputTooLarge {
                limit: MAX_REPLACE_OUTPUT,
            })?;
    if output_bytes > MAX_REPLACE_OUTPUT {
        return Err(SearchError::OutputTooLarge {
            limit: MAX_REPLACE_OUTPUT,
        });
    }

    // Pass 2: compose the replacement window. Only this buffer is allocated.
    let mut composed = String::with_capacity(output_bytes);
    let mut copied_from = first_start;
    let mut scan_from = first_start;
    while scan_from < last_end {
        let Some(found) = query.find_forward(doc, scan_from) else {
            break;
        };
        if found.end > last_end {
            break;
        }
        push_chars(&mut composed, doc, copied_from..found.start);
        composed.push_str(replacement);
        copied_from = found.end;
        scan_from = found.end;
    }
    push_chars(&mut composed, doc, copied_from..last_end);

    debug_assert_eq!(composed.len(), output_bytes);
    doc.replace(first_start..last_end, &composed)?;
    Ok(count)
}

fn push_chars(out: &mut String, doc: &Document, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    for chunk in doc.text().slice(range).chunks() {
        out.push_str(chunk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(needle: &str) -> Query {
        Query::new(needle).expect("non-empty needle")
    }

    fn document(text: &str) -> Document {
        Document::new(text)
    }

    fn text(doc: &Document) -> String {
        doc.text().to_string()
    }

    mod forward_and_reverse {
        use super::*;

        #[test]
        fn empty_needle_is_rejected_and_needle_is_preserved() {
            assert!(Query::new("").is_none());
            let q = query("é\n世");
            assert_eq!(q.needle(), "é\n世");
        }

        #[test]
        fn forward_search_is_inclusive_and_wraps_once() {
            let doc = document("abcabc");
            let q = query("abc");
            assert_eq!(q.next(&doc, 0, false), Some(0..3));
            assert_eq!(q.next(&doc, 1, false), Some(3..6));
            assert_eq!(q.next(&doc, 3, false), Some(3..6));
            assert_eq!(q.next(&doc, 4, false), Some(0..3));
            assert_eq!(q.next(&doc, 6, false), Some(0..3));
            assert_eq!(q.next(&doc, usize::MAX, false), Some(0..3));
        }

        #[test]
        fn reverse_search_is_inclusive_and_wraps_once() {
            let doc = document("abcabc");
            let q = query("abc");
            assert_eq!(q.next(&doc, 6, true), Some(3..6));
            assert_eq!(q.next(&doc, 5, true), Some(3..6));
            assert_eq!(q.next(&doc, 3, true), Some(3..6));
            assert_eq!(q.next(&doc, 2, true), Some(0..3));
            assert_eq!(q.next(&doc, 0, true), Some(0..3));

            let late = document("xxabc");
            assert_eq!(query("abc").next(&late, 0, true), Some(2..5));
            assert_eq!(query("abc").next(&late, 1, true), Some(2..5));
        }

        #[test]
        fn no_match_empty_document_and_short_document_are_none() {
            let doc = document("abc");
            let q = query("abcd");
            assert_eq!(q.next(&doc, 0, false), None);
            assert_eq!(q.next(&doc, 0, true), None);
            assert_eq!(q.next(&Document::new(""), 0, false), None);
            assert_eq!(q.next(&Document::new(""), 0, true), None);
            assert!(Query::new("").is_none());
        }

        #[test]
        fn overlapping_structural_matches_pick_by_start() {
            let doc = document("aaa");
            let q = query("aa");
            assert_eq!(q.next(&doc, 0, false), Some(0..2));
            assert_eq!(q.next(&doc, 1, false), Some(1..3));
            assert_eq!(q.next(&doc, 2, true), Some(1..3));
            assert_eq!(q.next(&doc, 0, true), Some(0..2));
        }

        #[test]
        fn multibyte_and_multiline_offsets_are_scalar_offsets() {
            let doc = document("héllo é\n世界é");
            let q = query("é");
            assert_eq!(q.next(&doc, 0, false), Some(1..2));
            assert_eq!(q.next(&doc, 2, false), Some(6..7));
            assert_eq!(q.next(&doc, 8, false), Some(10..11));
            assert_eq!(q.next(&doc, 99, false), Some(1..2));
            assert_eq!(q.next(&doc, 99, true), Some(10..11));

            let multi = document("one\ntwo\nthree\n");
            let q = query("two\nthree");
            assert_eq!(q.next(&multi, 0, false), Some(4..13));
            assert_eq!(q.next(&multi, 13, true), Some(4..13));

            let newline = query("\n");
            assert_eq!(newline.next(&multi, 4, false), Some(7..8));
            assert_eq!(newline.next(&multi, 7, true), Some(7..8));
        }

        #[test]
        fn grapheme_interior_matches_are_skipped() {
            let doc = document("e\u{301} e");
            assert_eq!(query("e").next(&doc, 0, false), Some(3..4));
            assert_eq!(query("e").next(&doc, 9, true), Some(3..4));
            assert_eq!(query("\u{301}").next(&doc, 0, false), None);
            assert_eq!(query("\u{301}").next(&doc, 9, true), None);

            let composed = document("xé e\u{301}e");
            assert_eq!(query("é").next(&composed, 0, false), Some(1..2));
            assert_eq!(query("e\u{301}").next(&composed, 0, false), Some(3..5));
            // A scalar match whose endpoint lands inside a cluster is skipped,
            // but the same scalar later at a cluster edge is accepted.
            assert_eq!(query("e").next(&composed, 0, false), Some(5..6));

            // A rejected higher start does not stop a backwards scan from
            // continuing to an earlier accepted match.
            let backwards = document("e e\u{301}");
            assert_eq!(query("e").next(&backwards, 4, true), Some(0..1));
        }

        #[test]
        fn query_is_stateless_across_document_edits() {
            let mut doc = document("ab");
            let q = query("ab");
            assert_eq!(q.next(&doc, 0, false), Some(0..2));
            doc.replace(0..1, "x").unwrap();
            assert_eq!(q.next(&doc, 0, false), None);
            doc.replace(0..2, "ab").unwrap();
            assert_eq!(q.next(&doc, 0, false), Some(0..2));
        }
    }

    mod replace {
        use super::*;

        #[test]
        fn replaces_left_to_right_without_overlap() {
            let mut doc = document("aaaa");
            assert_eq!(replace_all(&mut doc, &query("aa"), "b"), Ok(2));
            assert_eq!(text(&doc), "bb");

            let mut doc = document("aaa");
            assert_eq!(replace_all(&mut doc, &query("aa"), "X"), Ok(1));
            assert_eq!(text(&doc), "Xa");

            let mut doc = document("abab");
            assert_eq!(replace_all(&mut doc, &query("ab"), ""), Ok(2));
            assert_eq!(text(&doc), "");

            let mut expanded = document("aa");
            assert_eq!(replace_all(&mut expanded, &query("a"), "aa"), Ok(2));
            assert_eq!(text(&expanded), "aaaa");
        }

        #[test]
        fn preserves_nonmatching_bytes_and_multiline_shape() {
            let mut doc = document("αaβaγ");
            assert_eq!(replace_all(&mut doc, &query("a"), "🌍"), Ok(2));
            assert_eq!(text(&doc), "α🌍β🌍γ");

            let mut doc = document("one\ntwo\nthree\n");
            assert_eq!(replace_all(&mut doc, &query("two\nthree"), "2\n3"), Ok(1));
            assert_eq!(text(&doc), "one\n2\n3\n");

            let mut doc = document("e\u{301}xe");
            assert_eq!(replace_all(&mut doc, &query("e"), "E"), Ok(1));
            // The 'e' that forms a grapheme with the combining mark is untouched.
            assert_eq!(text(&doc), "e\u{301}xE");
            assert!(!doc.is_grapheme_boundary(1));

            let mut partial = document("e\u{301}x e");
            assert_eq!(replace_all(&mut partial, &query("e"), "E"), Ok(1));
            assert_eq!(text(&partial), "e\u{301}x E");
        }

        #[test]
        fn composed_window_is_one_revision_and_one_undo_transaction() {
            let mut doc = document("xaxbxcx");
            doc.mark_saved();
            let revision_before = doc.revision();
            assert!(!doc.is_dirty());

            assert_eq!(replace_all(&mut doc, &query("a"), "Y"), Ok(1));
            assert!(doc.is_dirty());
            assert_eq!(doc.revision(), revision_before + 1);
            assert_eq!(doc.changed_from(), 0);
            assert!(doc.undo().is_some());
            assert_eq!(text(&doc), "xaxbxcx");
            assert_eq!(doc.revision(), revision_before + 2);
            assert!(!doc.is_dirty());
            assert_eq!(doc.undo(), None);

            // Replacing several separated matches still commits one edit whose
            // window starts at the first match.
            assert_eq!(replace_all(&mut doc, &query("x"), "Z"), Ok(4));
            assert_eq!(text(&doc), "ZaZbZcZ");
            assert_eq!(doc.revision(), revision_before + 3);
            assert!(doc.undo().is_some());
            assert_eq!(text(&doc), "xaxbxcx");
            assert_eq!(doc.undo(), None);

            let mut lines = document("one\ntwo\nx\nx\nfour");
            lines.mark_saved();
            assert_eq!(replace_all(&mut lines, &query("x"), "y"), Ok(2));
            assert_eq!(text(&lines), "one\ntwo\ny\ny\nfour");
            assert_eq!(lines.changed_from(), 2);
            assert!(lines.undo().is_some());
            assert_eq!(text(&lines), "one\ntwo\nx\nx\nfour");
            assert!(!lines.is_dirty());

            // A saved checkpoint taken after replacement distinguishes that text
            // from the pre-replacement state across the same single undo step.
            assert_eq!(replace_all(&mut lines, &query("x"), "y"), Ok(2));
            lines.mark_saved();
            assert!(!lines.is_dirty());
            assert!(lines.undo().is_some());
            assert!(lines.is_dirty());
        }

        #[test]
        fn no_matches_or_identical_replacement_is_a_true_noop() {
            let mut doc = document("abc");
            doc.mark_saved();
            let revision = doc.revision();

            assert_eq!(replace_all(&mut doc, &query("zz"), "q"), Ok(0));
            assert_eq!(text(&doc), "abc");
            assert_eq!(doc.revision(), revision);
            assert!(!doc.is_dirty());
            assert_eq!(doc.undo(), None);

            assert_eq!(replace_all(&mut doc, &query("b"), "b"), Ok(0));
            assert_eq!(text(&doc), "abc");
            assert_eq!(doc.revision(), revision);
            assert!(!doc.is_dirty());
            assert_eq!(doc.undo(), None);

            let mut empty = document("");
            assert_eq!(replace_all(&mut empty, &query("a"), "b"), Ok(0));
            assert_eq!(empty.revision(), 0);
        }

        #[test]
        fn oversized_output_is_rejected_before_any_mutation() {
            let mut doc = document(&"a".repeat(65));
            doc.mark_saved();
            let revision = doc.revision();
            let replacement = "x".repeat((MAX_REPLACE_OUTPUT / 64) + 1);
            assert!(replacement.len() * 65 > MAX_REPLACE_OUTPUT);

            let result = replace_all(&mut doc, &query("a"), &replacement);
            assert_eq!(
                result,
                Err(SearchError::OutputTooLarge {
                    limit: MAX_REPLACE_OUTPUT
                })
            );
            assert_eq!(text(&doc), "a".repeat(65));
            assert_eq!(doc.revision(), revision);
            assert!(!doc.is_dirty());
            assert_eq!(doc.undo(), None);
            assert_eq!(doc.redo(), None);

            let mut empty = document("");
            assert_eq!(replace_all(&mut empty, &query("a"), "b"), Ok(0));
        }
    }

    mod match_index {
        use super::*;

        #[test]
        fn empty_no_match_and_short_document() {
            assert_eq!(
                query("a").match_index(&document(""), 10),
                MatchIndex {
                    starts: vec![],
                    has_more: false,
                }
            );
            assert_eq!(
                query("abc").match_index(&document("ab"), 10),
                MatchIndex {
                    starts: vec![],
                    has_more: false,
                }
            );
            assert_eq!(
                query("zz").match_index(&document("abcdef"), 10),
                MatchIndex {
                    starts: vec![],
                    has_more: false,
                }
            );
        }

        #[test]
        fn limit_zero_and_exact_vs_truncated_prefix() {
            let doc = document("ababab");
            let q = query("ab");

            assert_eq!(
                q.match_index(&doc, 0),
                MatchIndex {
                    starts: vec![],
                    has_more: true,
                }
            );
            assert_eq!(
                q.match_index(&document("xyz"), 0),
                MatchIndex {
                    starts: vec![],
                    has_more: false,
                }
            );
            assert_eq!(
                q.match_index(&doc, 2),
                MatchIndex {
                    starts: vec![0, 2],
                    has_more: true,
                }
            );
            assert_eq!(
                q.match_index(&doc, 3),
                MatchIndex {
                    starts: vec![0, 2, 4],
                    has_more: false,
                }
            );
            assert_eq!(
                q.match_index(&doc, 4),
                MatchIndex {
                    starts: vec![0, 2, 4],
                    has_more: false,
                }
            );

            let exact_100 = document(&"x".repeat(100));
            assert_eq!(
                query("x").match_index(&exact_100, 100),
                MatchIndex {
                    starts: (0..100).collect(),
                    has_more: false,
                }
            );

            let over_100 = document(&"x".repeat(105));
            assert_eq!(
                query("x").match_index(&over_100, 100),
                MatchIndex {
                    starts: (0..100).collect(),
                    has_more: true,
                }
            );
        }

        #[test]
        fn overlapping_matches_respect_limit_and_grapheme_endpoints() {
            let doc = document("aaaa");
            let q = query("aa");
            assert_eq!(
                q.match_index(&doc, 2),
                MatchIndex {
                    starts: vec![0, 1],
                    has_more: true,
                }
            );
            assert_eq!(
                q.match_index(&doc, 3),
                MatchIndex {
                    starts: vec![0, 1, 2],
                    has_more: false,
                }
            );
            assert_eq!(
                query("aba").match_index(&document("ababa"), 10),
                MatchIndex {
                    starts: vec![0, 2],
                    has_more: false,
                }
            );

            let combining_tail = document("aaaa\u{301}");
            assert_eq!(
                q.match_index(&combining_tail, 10),
                MatchIndex {
                    starts: vec![0, 1],
                    has_more: false,
                }
            );
            assert_eq!(
                q.match_index(&combining_tail, 1),
                MatchIndex {
                    starts: vec![0],
                    has_more: true,
                }
            );

            let combining_mid = document("aa\u{301}aa");
            assert_eq!(
                q.match_index(&combining_mid, 10),
                MatchIndex {
                    starts: vec![3],
                    has_more: false,
                }
            );

            let only_invalid = document("aa\u{301}");
            assert_eq!(
                q.match_index(&only_invalid, 0),
                MatchIndex {
                    starts: vec![],
                    has_more: false,
                }
            );
        }

        #[test]
        fn unicode_scalar_offsets_zwj_cjk_and_crlf() {
            let mixed = document("中aa🌍aa");
            assert_eq!(
                query("aa").match_index(&mixed, 10),
                MatchIndex {
                    starts: vec![1, 4],
                    has_more: false,
                }
            );
            assert_eq!(
                query("中a").match_index(&mixed, 10),
                MatchIndex {
                    starts: vec![0],
                    has_more: false,
                }
            );

            let zwj = document("👨\u{200d}👩\u{200d}👧 👩");
            assert_eq!(
                query("👩").match_index(&zwj, 10),
                MatchIndex {
                    starts: vec![6],
                    has_more: false,
                }
            );
            assert_eq!(
                query("👨\u{200d}👩\u{200d}👧").match_index(&zwj, 10),
                MatchIndex {
                    starts: vec![0],
                    has_more: false,
                }
            );

            let crlf = document("a\r\nb\r\n");
            assert_eq!(
                query("\r").match_index(&crlf, 10),
                MatchIndex {
                    starts: vec![],
                    has_more: false,
                }
            );
            assert_eq!(
                query("\n").match_index(&crlf, 10),
                MatchIndex {
                    starts: vec![],
                    has_more: false,
                }
            );
            assert_eq!(
                query("\r\n").match_index(&crlf, 10),
                MatchIndex {
                    starts: vec![1, 4],
                    has_more: false,
                }
            );
        }

        #[test]
        fn matches_next_forward_enumeration() {
            let doc = document("e\u{301}e a\r\nb 👨\u{200d}👩\u{200d}👧 👩 中e");
            let q = query("e");
            assert_eq!(
                q.match_index(&doc, 10),
                MatchIndex {
                    starts: vec![2, 18],
                    has_more: false,
                }
            );
            assert_eq!(q.next(&doc, 0, false), Some(2..3));
            assert_eq!(q.next(&doc, 3, false), Some(18..19));
        }
    }
}

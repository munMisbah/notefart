use ropey::RopeSlice;
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    ffi::OsStr,
    ops::Range,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, Instant},
};
use syntect::{
    easy::ScopeRangeIterator,
    highlighting::ScopeSelectors,
    parsing::{MatchPower, ParseState, ScopeStack, SyntaxReference, SyntaxSet},
};

use crate::document::Document;

const CHECKPOINT_INTERVAL: usize = 128;
const MAX_LINE_BYTES: usize = 16 * 1024;
const FIRST_LINE_SCAN_CHARS: usize = 512;
const RECENT_MAX_ROWS: usize = 512;
const RECENT_MAX_BYTES: usize = 2 * 1024 * 1024;
const HIGHLIGHT_TIME_SLICE: Duration = Duration::from_millis(4);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    Plain,
    Comment,
    Keyword,
    String,
    Number,
    Function,
    Type,
    Heading,
    Emphasis,
    Code,
    Link,
    Quote,
    Marker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSpan {
    pub chars: Range<usize>,
    pub token: Token,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineTokens {
    pub line: usize,
    pub spans: Vec<TokenSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub changed: bool,
    pub pending: bool,
}

struct Checkpoint {
    line: usize,
    parse_state: ParseState,
    scope_stack: ScopeStack,
}

struct Entry {
    path: Option<PathBuf>,
    language: Option<usize>,
    revision: u64,
    text_len: usize,
    line_count: usize,
    cursor: usize,
    parse_state: Option<ParseState>,
    scope_stack: ScopeStack,
    checkpoints: Vec<Checkpoint>,
    rows: Vec<LineTokens>,
    recent: VecDeque<LineTokens>,
    recent_bytes: usize,
    targets: Vec<Range<usize>>,
    blocked: Option<usize>,
    warning: Option<String>,
}

pub struct Highlighter {
    syntaxes: &'static SyntaxSet,
    rules: Vec<(ScopeSelectors, Token)>,
    entries: HashMap<u64, Entry>,
    scratch_targets: Vec<Range<usize>>,
    scratch_evicted: Vec<LineTokens>,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntaxes: crate::syntax::load(),
            rules: semantic_rules(),
            entries: HashMap::new(),
            scratch_targets: Vec::new(),
            scratch_evicted: Vec::new(),
        }
    }

    pub fn update(
        &mut self,
        key: u64,
        doc: &Document,
        path: Option<&Path>,
        visible: &[Range<usize>],
        budget: usize,
    ) -> Progress {
        let line_count = doc.line_count();
        normalize_ranges(visible, line_count, &mut self.scratch_targets);
        let revision = doc.revision();
        let text_len = doc.len_chars();

        let syntaxes = self.syntaxes;
        let rules = &self.rules;
        let targets = &self.scratch_targets;
        let mut changed = false;

        let entry = match self.entries.entry(key) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                let language = resolve_language(syntaxes, doc, path);
                slot.insert(new_entry(
                    syntaxes, path, language, revision, text_len, line_count,
                ))
            }
            std::collections::hash_map::Entry::Occupied(slot) => {
                let entry = slot.into_mut();
                let path_changed = entry.path.as_deref() != path;
                let doc_changed = revision != entry.revision
                    || text_len != entry.text_len
                    || line_count != entry.line_count;

                if path_changed {
                    let language = resolve_language(syntaxes, doc, path);
                    reset_entry(
                        entry,
                        syntaxes,
                        path,
                        language,
                        revision,
                        text_len,
                        line_count,
                        &mut changed,
                    );
                } else if doc_changed {
                    let language = resolve_language(syntaxes, doc, path);
                    if language != entry.language {
                        reset_entry(
                            entry,
                            syntaxes,
                            path,
                            language,
                            revision,
                            text_len,
                            line_count,
                            &mut changed,
                        );
                    } else {
                        let invalidate_line = if entry.revision.checked_add(1) == Some(revision) {
                            doc.changed_from().min(line_count)
                        } else {
                            0
                        };
                        invalidate_from(entry, invalidate_line, &mut changed);
                        entry.revision = revision;
                        entry.text_len = text_len;
                        entry.line_count = line_count;
                    }
                }
                entry
            }
        };

        if entry.targets != *targets {
            let mut write = 0usize;
            for read in 0..entry.rows.len() {
                let line = entry.rows[read].line;
                if in_ranges(targets, line) {
                    if write != read {
                        entry.rows.swap(write, read);
                    }
                    write += 1;
                }
            }
            self.scratch_evicted.clear();
            if write < entry.rows.len() {
                self.scratch_evicted.extend(entry.rows.drain(write..));
                changed = true;
            }

            if !entry.recent.is_empty() && !targets.is_empty() && entry.language.is_some() {
                let parse_limit = entry.blocked.unwrap_or(usize::MAX);
                let recent_count = entry.recent.len();
                let mut restored = false;
                for _ in 0..recent_count {
                    let row = entry.recent.pop_front().unwrap();
                    if row.line < parse_limit && in_ranges(targets, row.line) {
                        entry.recent_bytes =
                            entry.recent_bytes.saturating_sub(row_payload_bytes(&row));
                        entry.rows.push(row);
                        restored = true;
                    } else {
                        entry.recent.push_back(row);
                    }
                }
                if restored {
                    entry.rows.sort_unstable_by_key(|row| row.line);
                    changed = true;
                }
            }

            for row in self.scratch_evicted.drain(..) {
                push_recent(entry, row);
            }

            entry.targets.clear();
            entry.targets.extend(targets.iter().cloned());
        }

        if entry.language.is_none() || entry.targets.is_empty() {
            return Progress {
                changed,
                pending: false,
            };
        }

        let parse_limit = entry.blocked.unwrap_or(usize::MAX);
        let mut missing = earliest_missing_from(&entry.rows, &entry.targets, 0, parse_limit);
        let Some(first_missing) = missing else {
            return Progress {
                changed,
                pending: false,
            };
        };

        if budget == 0 {
            return Progress {
                changed,
                pending: true,
            };
        }

        seek_to_nearest_checkpoint(entry, first_missing);

        let mut worked = 0usize;
        let slice_started = Instant::now();

        while let Some(m) = missing {
            if worked >= budget || (worked > 0 && slice_started.elapsed() >= HIGHLIGHT_TIME_SLICE) {
                break;
            }
            let line = entry.cursor;
            let capture = line == m;

            let line_slice = doc.text().line(line);
            let body_bytes = line_body_bytes(line_slice);
            if body_bytes > MAX_LINE_BYTES {
                let message = format!(
                    "line {} exceeds 16 KiB ({} bytes); highlighting paused",
                    line + 1,
                    body_bytes
                );
                block_entry(entry, line, false, &mut changed);
                entry.warning = Some(message);
                changed = true;
                missing = earliest_missing_from(&entry.rows, &entry.targets, 0, line);
                break;
            }

            let line_text = Cow::from(line_slice);
            let abs_line_start = if capture { doc.line_start(line) } else { 0 };
            let parse_state = entry
                .parse_state
                .as_mut()
                .expect("syntax entry must have parse state");

            match parse_single_line(
                parse_state,
                &mut entry.scope_stack,
                syntaxes,
                rules,
                &line_text,
                body_bytes,
                abs_line_start,
                capture,
            ) {
                Ok(spans) => {
                    entry.cursor = line + 1;
                    if entry.cursor % CHECKPOINT_INTERVAL == 0
                        && entry
                            .checkpoints
                            .last()
                            .is_none_or(|cp| cp.line < entry.cursor)
                    {
                        entry.checkpoints.push(Checkpoint {
                            line: entry.cursor,
                            parse_state: parse_state.clone(),
                            scope_stack: entry.scope_stack.clone(),
                        });
                    }
                    if capture {
                        insert_row(&mut entry.rows, LineTokens { line, spans });
                        changed = true;
                        missing = earliest_missing_from(
                            &entry.rows,
                            &entry.targets,
                            line + 1,
                            parse_limit,
                        );
                        if let Some(next_m) = missing
                            && next_m > entry.cursor
                        {
                            seek_to_nearest_checkpoint(entry, next_m);
                        }
                    }
                    worked += 1;
                }
                Err(err) => {
                    let message = format!("parser stopped at line {}: {}", line + 1, err);
                    block_entry(entry, line, true, &mut changed);
                    entry.warning = Some(message);
                    changed = true;
                    missing = earliest_missing_from(&entry.rows, &entry.targets, 0, line);
                    break;
                }
            }
        }

        Progress {
            changed,
            pending: missing.is_some(),
        }
    }

    pub fn lines(&self, key: u64) -> &[LineTokens] {
        self.entries
            .get(&key)
            .map_or(&[], |entry| entry.rows.as_slice())
    }

    pub fn warning(&self, key: u64) -> Option<&str> {
        self.entries
            .get(&key)
            .and_then(|entry| entry.warning.as_deref())
    }

    pub fn forget(&mut self, key: u64) {
        self.entries.remove(&key);
    }

    #[cfg(test)]
    fn recent_len(&self, key: u64) -> usize {
        self.entries.get(&key).map_or(0, |entry| entry.recent.len())
    }

    #[cfg(test)]
    fn recent_bytes(&self, key: u64) -> usize {
        self.entries.get(&key).map_or(0, |entry| entry.recent_bytes)
    }
}

fn semantic_rules() -> Vec<(ScopeSelectors, Token)> {
    [
        ("comment", Token::Comment),
        (
            "keyword - keyword.operator, storage.type - storage.type.string, storage.modifier",
            Token::Keyword,
        ),
        ("string, storage.type.string", Token::String),
        ("constant.numeric, constant.language", Token::Number),
        (
            "entity.name.function, support.function, support.macro, variable.function",
            Token::Function,
        ),
        (
            "entity.name.struct, entity.name.enum, entity.name.trait, entity.name.type, entity.name.interface, entity.name.class, entity.name.tag, entity.other.inherited-class, support.type, support.class",
            Token::Type,
        ),
        ("markup.heading", Token::Heading),
        ("markup.bold, markup.italic", Token::Emphasis),
        ("markup.raw", Token::Code),
        (
            "meta.link, meta.image, markup.underline.link, entity.name.reference.link",
            Token::Link,
        ),
        ("markup.quote", Token::Quote),
        (
            "markup.list.numbered.bullet, markup.list.unnumbered.bullet, punctuation.definition.list_item.markdown, punctuation.definition.thematic-break.markdown",
            Token::Marker,
        ),
    ]
    .into_iter()
    .map(|(selector, token)| {
        (
            ScopeSelectors::from_str(selector).expect("built-in scope selector must parse"),
            token,
        )
    })
    .collect()
}

fn classify_scope(stack: &ScopeStack, rules: &[(ScopeSelectors, Token)]) -> Token {
    let mut best: Option<(MatchPower, Token)> = None;
    for (selector, token) in rules {
        if let Some(power) = selector.does_match(stack.as_slice())
            && best.is_none_or(|(previous, _)| power > previous)
        {
            best = Some((power, *token));
        }
    }
    best.map_or(Token::Plain, |(_, token)| token)
}

fn resolve_language(syntaxes: &SyntaxSet, doc: &Document, path: Option<&Path>) -> Option<usize> {
    let syntax = if let Some(path) = path {
        let file_name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
        let ext = path.extension().and_then(OsStr::to_str).unwrap_or("");
        let by_path = (!file_name.is_empty())
            .then(|| syntaxes.find_syntax_by_extension(file_name))
            .flatten()
            .or_else(|| {
                (!ext.is_empty())
                    .then(|| syntaxes.find_syntax_by_extension(ext))
                    .flatten()
            });
        match by_path {
            Some(found) => Some(found),
            None => find_by_first_line(syntaxes, doc),
        }
    } else {
        find_by_first_line(syntaxes, doc)
    }?;

    if syntax.name == "Plain Text" {
        return None;
    }

    syntaxes
        .syntaxes()
        .iter()
        .position(|candidate| std::ptr::eq(candidate, syntax))
}

fn find_by_first_line<'a>(syntaxes: &'a SyntaxSet, doc: &Document) -> Option<&'a SyntaxReference> {
    if doc.len_chars() == 0 {
        return None;
    }
    let mut first_line = String::new();
    for ch in doc.text().chars().take(FIRST_LINE_SCAN_CHARS) {
        if ch == '\n' || ch == '\r' {
            break;
        }
        first_line.push(ch);
    }
    if first_line.is_empty() {
        return None;
    }
    syntaxes.find_syntax_by_first_line(&first_line)
}

fn new_entry(
    syntaxes: &SyntaxSet,
    path: Option<&Path>,
    language: Option<usize>,
    revision: u64,
    text_len: usize,
    line_count: usize,
) -> Entry {
    let mut checkpoints = Vec::new();
    let parse_state = language.map(|idx| {
        let state = ParseState::new(&syntaxes.syntaxes()[idx]);
        checkpoints.push(Checkpoint {
            line: 0,
            parse_state: state.clone(),
            scope_stack: ScopeStack::new(),
        });
        state
    });
    Entry {
        path: path.map(Path::to_path_buf),
        language,
        revision,
        text_len,
        line_count,
        cursor: 0,
        parse_state,
        scope_stack: ScopeStack::new(),
        checkpoints,
        rows: Vec::new(),
        recent: VecDeque::new(),
        recent_bytes: 0,
        targets: Vec::new(),
        blocked: None,
        warning: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn reset_entry(
    entry: &mut Entry,
    syntaxes: &SyntaxSet,
    path: Option<&Path>,
    language: Option<usize>,
    revision: u64,
    text_len: usize,
    line_count: usize,
    changed: &mut bool,
) {
    if !entry.rows.is_empty() {
        entry.rows.clear();
        *changed = true;
    }
    entry.recent.clear();
    entry.recent.shrink_to_fit();
    entry.recent_bytes = 0;
    entry.path = path.map(Path::to_path_buf);
    entry.language = language;
    entry.revision = revision;
    entry.text_len = text_len;
    entry.line_count = line_count;
    entry.cursor = 0;
    entry.blocked = None;
    if entry.warning.take().is_some() {
        *changed = true;
    }
    entry.checkpoints.clear();
    entry.scope_stack = ScopeStack::new();
    entry.parse_state = language.map(|idx| {
        let state = ParseState::new(&syntaxes.syntaxes()[idx]);
        entry.checkpoints.push(Checkpoint {
            line: 0,
            parse_state: state.clone(),
            scope_stack: ScopeStack::new(),
        });
        state
    });
}

fn row_payload_bytes(row: &LineTokens) -> usize {
    std::mem::size_of::<LineTokens>() + row.spans.capacity() * std::mem::size_of::<TokenSpan>()
}

fn push_recent(entry: &mut Entry, row: LineTokens) {
    let bytes = row_payload_bytes(&row);
    if bytes > RECENT_MAX_BYTES {
        return;
    }
    if let Some(pos) = entry
        .recent
        .iter()
        .position(|existing| existing.line == row.line)
        && let Some(removed) = entry.recent.remove(pos)
    {
        entry.recent_bytes = entry
            .recent_bytes
            .saturating_sub(row_payload_bytes(&removed));
    }
    while entry.recent.len() >= RECENT_MAX_ROWS
        || entry.recent_bytes.saturating_add(bytes) > RECENT_MAX_BYTES
    {
        let Some(evicted) = entry.recent.pop_front() else {
            break;
        };
        entry.recent_bytes = entry
            .recent_bytes
            .saturating_sub(row_payload_bytes(&evicted));
    }
    entry.recent_bytes += bytes;
    entry.recent.push_back(row);
}

fn retain_recent_below(entry: &mut Entry, limit: usize) {
    if entry.recent.is_empty() {
        return;
    }
    let mut kept_bytes = 0usize;
    entry.recent.retain(|row| {
        if row.line < limit {
            kept_bytes += row_payload_bytes(row);
            true
        } else {
            false
        }
    });
    entry.recent_bytes = kept_bytes;
}

fn invalidate_from(entry: &mut Entry, from: usize, changed: &mut bool) {
    let before_rows = entry.rows.len();
    entry.rows.retain(|row| row.line < from);
    if entry.rows.len() != before_rows {
        *changed = true;
    }
    retain_recent_below(entry, from);
    entry.checkpoints.retain(|cp| cp.line <= from);
    if entry.cursor > from && !entry.checkpoints.is_empty() {
        rewind_to_checkpoint(entry, from);
    }
    if entry.blocked.is_some_and(|blocked| from <= blocked) {
        entry.blocked = None;
        if entry.warning.take().is_some() {
            *changed = true;
        }
    }
}

fn block_entry(entry: &mut Entry, line: usize, rewind: bool, changed: &mut bool) {
    let before_rows = entry.rows.len();
    entry.rows.retain(|row| row.line < line);
    if entry.rows.len() != before_rows {
        *changed = true;
    }
    retain_recent_below(entry, line);
    entry.checkpoints.retain(|cp| cp.line <= line);
    if rewind && !entry.checkpoints.is_empty() {
        rewind_to_checkpoint(entry, line);
    } else if entry.cursor > line && !entry.checkpoints.is_empty() {
        rewind_to_checkpoint(entry, line);
    }
    entry.blocked = Some(line);
}

fn rewind_to_checkpoint(entry: &mut Entry, line: usize) {
    let idx = entry
        .checkpoints
        .partition_point(|cp| cp.line <= line)
        .saturating_sub(1);
    let cp = &entry.checkpoints[idx];
    let state = cp.parse_state.clone();
    let stack = cp.scope_stack.clone();
    let cp_line = cp.line;
    entry.parse_state = Some(state);
    entry.scope_stack = stack;
    entry.cursor = cp_line;
}

fn seek_to_nearest_checkpoint(entry: &mut Entry, target_line: usize) {
    if entry.checkpoints.is_empty() {
        return;
    }
    let idx = entry
        .checkpoints
        .partition_point(|cp| cp.line <= target_line)
        .saturating_sub(1);
    let cp = &entry.checkpoints[idx];
    if entry.cursor > target_line || entry.cursor < cp.line {
        entry.parse_state = Some(cp.parse_state.clone());
        entry.scope_stack = cp.scope_stack.clone();
        entry.cursor = cp.line;
    }
}

fn normalize_ranges(visible: &[Range<usize>], line_count: usize, out: &mut Vec<Range<usize>>) {
    out.clear();
    for r in visible {
        let start = r.start.min(line_count);
        let end = r.end.min(line_count);
        if start < end {
            out.push(start..end);
        }
    }
    if out.len() <= 1 {
        return;
    }
    out.sort_unstable_by_key(|r| (r.start, r.end));
    let mut write = 0usize;
    for read in 1..out.len() {
        if out[read].start <= out[write].end {
            out[write].end = out[write].end.max(out[read].end);
        } else {
            write += 1;
            let next = out[read].start..out[read].end;
            out[write] = next;
        }
    }
    out.truncate(write + 1);
}

fn in_ranges(ranges: &[Range<usize>], line: usize) -> bool {
    let idx = ranges.partition_point(|r| r.end <= line);
    idx < ranges.len() && ranges[idx].start <= line
}

fn earliest_missing_from(
    rows: &[LineTokens],
    targets: &[Range<usize>],
    min_line: usize,
    parse_limit: usize,
) -> Option<usize> {
    for r in targets {
        if r.start >= parse_limit {
            break;
        }
        let start = r.start.max(min_line);
        let end = r.end.min(parse_limit);
        if start < end
            && let Some(m) = earliest_missing(rows, start..end)
        {
            return Some(m);
        }
    }
    None
}

fn earliest_missing(rows: &[LineTokens], range: Range<usize>) -> Option<usize> {
    if range.start >= range.end {
        return None;
    }
    let mut expected = range.start;
    let idx = rows.partition_point(|row| row.line < expected);
    for row in &rows[idx..] {
        if row.line == expected {
            expected += 1;
            if expected >= range.end {
                return None;
            }
        } else if row.line > expected {
            return Some(expected);
        }
    }
    (expected < range.end).then_some(expected)
}

fn insert_row(rows: &mut Vec<LineTokens>, row: LineTokens) {
    match rows.binary_search_by_key(&row.line, |existing| existing.line) {
        Ok(idx) => rows[idx] = row,
        Err(idx) => rows.insert(idx, row),
    }
}

fn line_body_bytes(slice: RopeSlice<'_>) -> usize {
    let bytes = slice.len_bytes();
    let chars = slice.len_chars();
    if chars >= 2 && slice.char(chars - 2) == '\r' && slice.char(chars - 1) == '\n' {
        bytes - 2
    } else if chars >= 1 && slice.char(chars - 1) == '\n' {
        bytes - 1
    } else {
        bytes
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_single_line(
    parse_state: &mut ParseState,
    scope_stack: &mut ScopeStack,
    syntaxes: &SyntaxSet,
    rules: &[(ScopeSelectors, Token)],
    line_text: &str,
    body_bytes: usize,
    abs_line_start: usize,
    capture: bool,
) -> Result<Vec<TokenSpan>, String> {
    let ops = parse_state
        .parse_line(line_text, syntaxes)
        .map_err(|err| err.to_string())?;
    let mut spans: Vec<TokenSpan> = Vec::new();
    let mut last_range_end = 0usize;
    let mut byte_pos = 0usize;
    let mut char_pos = 0usize;

    for (range, op) in ScopeRangeIterator::new(&ops, line_text) {
        if range.start < last_range_end
            || range.end < range.start
            || range.end > line_text.len()
            || !line_text.is_char_boundary(range.start)
            || !line_text.is_char_boundary(range.end)
        {
            return Err("invalid byte range produced by syntax parser".to_string());
        }
        last_range_end = range.end;

        scope_stack.apply(op).map_err(|err| err.to_string())?;

        if !capture {
            continue;
        }

        let start = range.start.min(body_bytes);
        let end = range.end.min(body_bytes);
        if start >= end {
            continue;
        }

        if start > byte_pos {
            char_pos += line_text[byte_pos..start].chars().count();
        }

        let span_char_start = char_pos;
        let span_char_len = line_text[start..end].chars().count();
        let span_char_end = span_char_start + span_char_len;
        byte_pos = end;
        char_pos = span_char_end;

        let token = classify_scope(scope_stack, rules);
        if token != Token::Plain {
            let abs_start = abs_line_start + span_char_start;
            let abs_end = abs_line_start + span_char_end;
            if let Some(prev) = spans.last_mut()
                && prev.token == token
                && prev.chars.end == abs_start
            {
                prev.chars.end = abs_end;
            } else {
                spans.push(TokenSpan {
                    chars: abs_start..abs_end,
                    token,
                });
            }
        }
    }

    Ok(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_updates(
        hl: &mut Highlighter,
        key: u64,
        doc: &Document,
        path: Option<&Path>,
        visible: &[Range<usize>],
        budget: usize,
    ) -> (Progress, usize) {
        let max_steps = doc.line_count().max(1) + 1;
        let mut steps = 0usize;
        let mut result = Progress {
            changed: false,
            pending: true,
        };
        loop {
            steps += 1;
            assert!(steps <= max_steps, "highlight updates failed to converge");
            let prog = hl.update(key, doc, path, visible, budget);
            result.changed |= prog.changed;
            result.pending = prog.pending;
            if !result.pending {
                return (result, steps);
            }
        }
    }

    fn char_offset_of(doc: &Document, needle: &str) -> usize {
        let full = doc.text().to_string();
        let byte_idx = full.find(needle).expect("fixture must contain needle");
        full[..byte_idx].chars().count()
    }

    fn token_at_char(lines: &[LineTokens], char_idx: usize) -> Token {
        for row in lines {
            for span in &row.spans {
                if span.chars.contains(&char_idx) {
                    return span.token;
                }
            }
        }
        Token::Plain
    }

    fn token_for(lines: &[LineTokens], doc: &Document, needle: &str) -> Token {
        token_at_char(lines, char_offset_of(doc, needle))
    }

    #[test]
    fn rust_semantic_categories_and_multibyte_char_offsets() {
        let source = concat!(
            "/* block */\n",
            "struct Thing;\n",
            "fn main() {\n",
            "    let café = \"東京🧪\";/*🔥*/let number = 42;\n",
            "}\n",
        );
        let doc = Document::new(source);
        let mut hl = Highlighter::new();
        let (progress, _) =
            drain_updates(&mut hl, 1, &doc, Some(Path::new("main.rs")), &[0..10], 200);
        assert!(progress.changed);
        assert!(!progress.pending);
        assert!(hl.warning(1).is_none());

        let lines = hl.lines(1);
        assert_eq!(lines.len(), doc.line_count());
        for pair in lines.windows(2) {
            assert!(pair[0].line < pair[1].line);
        }
        for row in lines {
            for pair in row.spans.windows(2) {
                assert!(pair[0].chars.start < pair[0].chars.end);
                assert!(pair[0].chars.end <= pair[1].chars.start);
            }
        }

        assert_eq!(token_for(lines, &doc, "block"), Token::Comment);
        assert_eq!(token_for(lines, &doc, "struct"), Token::Keyword);
        assert_eq!(token_for(lines, &doc, "Thing"), Token::Type);
        assert_eq!(token_for(lines, &doc, "fn main"), Token::Keyword);
        assert_eq!(token_for(lines, &doc, "main()"), Token::Function);
        assert_eq!(token_for(lines, &doc, "café"), Token::Plain);
        assert_eq!(token_for(lines, &doc, "東京🧪"), Token::String);
        assert_eq!(token_for(lines, &doc, "🔥"), Token::Comment);
        assert_eq!(token_for(lines, &doc, "42"), Token::Number);

        // Idle re-update with identical doc/viewport avoids work and redraws.
        let idle = hl.update(1, &doc, Some(Path::new("main.rs")), &[0..10], 200);
        assert!(!idle.changed);
        assert!(!idle.pending);
        assert!(hl.warning(1).is_none());
    }

    #[test]
    fn python_and_json_semantic_categories() {
        let py = concat!(
            "# note\n",
            "class Widget:\n",
            "    def compute(self):\n",
            "        msg = \"hello\"\n",
            "        return 99\n",
        );
        let py_doc = Document::new(py);
        let mut hl = Highlighter::new();
        let (py_prog, _) = drain_updates(
            &mut hl,
            1,
            &py_doc,
            Some(Path::new("script.py")),
            &[0..10],
            200,
        );
        assert!(!py_prog.pending);
        assert!(hl.warning(1).is_none());
        let py_lines = hl.lines(1);
        assert_eq!(token_for(py_lines, &py_doc, "note"), Token::Comment);
        assert_eq!(token_for(py_lines, &py_doc, "class "), Token::Keyword);
        assert_eq!(token_for(py_lines, &py_doc, "Widget"), Token::Type);
        assert_eq!(token_for(py_lines, &py_doc, "def "), Token::Keyword);
        assert_eq!(token_for(py_lines, &py_doc, "compute"), Token::Function);
        assert_eq!(token_for(py_lines, &py_doc, "hello"), Token::String);
        assert_eq!(token_for(py_lines, &py_doc, "99"), Token::Number);

        let json = "{\n  \"name\": \"notefart\",\n  \"count\": 123,\n  \"ok\": true\n}\n";
        let json_doc = Document::new(json);
        let (json_prog, _) = drain_updates(
            &mut hl,
            2,
            &json_doc,
            Some(Path::new("data.json")),
            &[0..10],
            200,
        );
        assert!(!json_prog.pending);
        assert!(hl.warning(2).is_none());
        let json_lines = hl.lines(2);
        assert_eq!(token_for(json_lines, &json_doc, "notefart"), Token::String);
        assert_eq!(token_for(json_lines, &json_doc, "123"), Token::Number);
        assert_eq!(token_for(json_lines, &json_doc, "true"), Token::Number);
    }

    #[test]
    fn unknown_language_is_plain_and_first_line_detection_is_bounded() {
        let mut hl = Highlighter::new();
        let plain_doc = Document::new("fn looks_like_code() { return 42; }\n");
        let plain_prog = hl.update(
            1,
            &plain_doc,
            Some(Path::new("notes.unknownext")),
            &[0..5],
            200,
        );
        assert!(!plain_prog.pending);
        assert!(hl.warning(1).is_none());
        assert!(hl.lines(1).iter().all(|row| row.spans.is_empty()));

        let shebang_doc = Document::new("#!/usr/bin/env python3\ndef greet():\n    return 7\n");
        let (shebang_prog, _) = drain_updates(&mut hl, 2, &shebang_doc, None, &[0..5], 200);
        assert!(!shebang_prog.pending);
        assert!(hl.warning(2).is_none());
        let lines = hl.lines(2);
        assert_eq!(token_for(lines, &shebang_doc, "def "), Token::Keyword);
        assert_eq!(token_for(lines, &shebang_doc, "greet"), Token::Function);
        assert_eq!(token_for(lines, &shebang_doc, "7"), Token::Number);
    }

    #[test]
    fn multiline_comment_edit_and_undo_invalidate_and_restore() {
        let source = concat!(
            "/* multiline comment\n",
            "let hidden = \"not a string\";\n",
            "*/\n",
            "fn main() {\n",
            "    let n = 42;\n",
            "}\n",
        );
        let mut doc = Document::new(source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("test.rs"));
        drain_updates(&mut hl, 1, &doc, path, &[0..10], 200);

        assert_eq!(token_for(hl.lines(1), &doc, "hidden"), Token::Comment);
        assert_eq!(token_for(hl.lines(1), &doc, "fn main"), Token::Keyword);
        assert_eq!(token_for(hl.lines(1), &doc, "42"), Token::Number);

        // Break the multiline comment terminator `*/` into `* /`.
        let close_pos = char_offset_of(&doc, "*/");
        doc.replace(close_pos..close_pos + 2, "* /").unwrap();
        let (edited_prog, _) = drain_updates(&mut hl, 1, &doc, path, &[0..10], 200);
        assert!(edited_prog.changed);
        assert!(!edited_prog.pending);
        assert_eq!(token_for(hl.lines(1), &doc, "fn main"), Token::Comment);
        assert_eq!(token_for(hl.lines(1), &doc, "42"), Token::Comment);

        // Undo restores `*/` and semantic tokens downstream.
        doc.undo().unwrap();
        let (undone_prog, _) = drain_updates(&mut hl, 1, &doc, path, &[0..10], 200);
        assert!(undone_prog.changed);
        assert!(!undone_prog.pending);
        assert_eq!(token_for(hl.lines(1), &doc, "fn main"), Token::Keyword);
        assert_eq!(token_for(hl.lines(1), &doc, "42"), Token::Number);
    }

    #[test]
    fn skipped_revisions_conservatively_invalidate_from_zero() {
        let source = "/* a\nb\n*/\nfn f() { let n = 1; }\n";
        let mut doc = Document::new(source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("skip.rs"));
        drain_updates(&mut hl, 1, &doc, path, &[0..10], 200);
        assert_eq!(token_for(hl.lines(1), &doc, "fn f"), Token::Keyword);

        // Two edits without an intermediate update: last changed_from is on line 3,
        // while line 2 broke `*/`. Skipping revisions must invalidate from line 0.
        let close_pos = char_offset_of(&doc, "*/");
        doc.replace(close_pos..close_pos + 2, "* /").unwrap();
        let fn_pos = char_offset_of(&doc, "fn f");
        doc.replace(fn_pos..fn_pos, " ").unwrap();
        assert_eq!(doc.changed_from(), 3);

        let (prog, _) = drain_updates(&mut hl, 1, &doc, path, &[0..10], 200);
        assert!(prog.changed);
        assert_eq!(token_for(hl.lines(1), &doc, "fn f"), Token::Comment);
    }

    #[test]
    fn path_switch_and_forget_reset_without_stale_state() {
        let doc = Document::new("def greet():\n    fn_like = 10\n");
        let mut hl = Highlighter::new();

        drain_updates(&mut hl, 1, &doc, Some(Path::new("a.py")), &[0..5], 200);
        assert_eq!(token_for(hl.lines(1), &doc, "def "), Token::Keyword);
        assert_eq!(token_for(hl.lines(1), &doc, "greet"), Token::Function);

        // Switching to plain text resets and drops prior Python tokens.
        let switch_plain = hl.update(1, &doc, Some(Path::new("a.txt")), &[0..5], 200);
        assert!(switch_plain.changed);
        assert!(!switch_plain.pending);
        assert!(hl.lines(1).is_empty());

        // Switching to Rust highlights fresh syntax.
        let rust_doc = Document::new("fn ok() {}\n");
        let (switch_rs, _) =
            drain_updates(&mut hl, 1, &rust_doc, Some(Path::new("a.rs")), &[0..5], 200);
        assert!(switch_rs.changed);
        assert_eq!(token_for(hl.lines(1), &rust_doc, "fn "), Token::Keyword);

        // Forget clears entry completely, preventing stale state across rev-0 reload.
        hl.forget(1);
        assert!(hl.lines(1).is_empty());
        let reloaded = Document::new("let n = 5;\n");
        assert_eq!(reloaded.revision(), 0);
        drain_updates(&mut hl, 1, &reloaded, Some(Path::new("a.rs")), &[0..5], 200);
        assert_eq!(hl.lines(1).len(), reloaded.line_count());
        assert_eq!(token_for(hl.lines(1), &reloaded, "5"), Token::Number);
    }

    #[test]
    fn budget_progression_and_sparse_checkpoints_on_scroll() {
        let mut source = String::new();
        for idx in 0..320 {
            if idx == 140 {
                source.push_str("/* open multiline at 140\n");
            } else if idx == 150 {
                source.push_str("*/\n");
            } else {
                source.push_str("fn step() { let x = 42; }\n");
            }
        }
        let doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("big.rs"));

        // Scroll directly to 280..310 with budget 200: must not scan whole doc in one call.
        let p1 = hl.update(1, &doc, path, &[280..310], 200);
        assert!(p1.pending);
        assert!(!p1.changed);
        assert!(hl.lines(1).is_empty());

        let (p_done, _) = drain_updates(&mut hl, 1, &doc, path, &[280..310], 200);
        assert!(!p_done.pending);
        assert_eq!(hl.lines(1).len(), 30);
        assert_eq!(hl.lines(1)[0].line, 280);
        assert_eq!(hl.lines(1)[29].line, 309);

        // Returning above the previous viewport preserves multiline context.
        let (p3, _) = drain_updates(&mut hl, 1, &doc, path, &[135..165], 40);
        assert!(!p3.pending);
        assert_eq!(hl.lines(1).len(), 30);
        assert_eq!(hl.lines(1)[0].line, 135);
        assert_eq!(hl.lines(1)[29].line, 164);

        // Line 145 is inside the multiline comment opened at line 140; line 155 is code again.
        let row_145 = hl.lines(1).iter().find(|r| r.line == 145).unwrap();
        assert!(!row_145.spans.is_empty());
        assert!(row_145.spans.iter().all(|s| s.token == Token::Comment));
        let row_155 = hl.lines(1).iter().find(|r| r.line == 155).unwrap();
        assert!(row_155.spans.iter().any(|s| s.token == Token::Keyword));
        assert!(row_155.spans.iter().any(|s| s.token == Token::Number));

        // Scrolling back near top keeps lines() strictly to 0..20.
        let (p4, _) = drain_updates(&mut hl, 1, &doc, path, &[0..20], 200);
        assert!(!p4.pending);
        assert_eq!(hl.lines(1).len(), 20);
        assert_eq!(hl.lines(1)[0].line, 0);
        assert_eq!(hl.lines(1)[19].line, 19);
    }

    #[test]
    fn budget_zero_and_incremental_viewport_fill() {
        let source = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";
        let doc = Document::new(source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("inc.rs"));

        let p0 = hl.update(1, &doc, path, &[0..5], 0);
        assert!(p0.pending);
        assert!(!p0.changed);
        assert!(hl.lines(1).is_empty());

        let mut previous_rows = 0;
        for _ in 0..5 {
            let progress = hl.update(1, &doc, path, &[0..5], 2);
            let rows = hl.lines(1).len();
            assert!(rows > previous_rows && rows - previous_rows <= 2);
            assert!(progress.changed);
            previous_rows = rows;
            if !progress.pending {
                break;
            }
        }
        assert_eq!(hl.lines(1).len(), 5);
    }

    #[test]
    fn long_line_warning_persists_and_recovers_when_shortened() {
        let exact_16k = format!("let ok = \"{}\";\n", "a".repeat(MAX_LINE_BYTES - 12));
        assert_eq!(exact_16k.len() - 1, MAX_LINE_BYTES);
        let boundary_doc = Document::new(&format!("{exact_16k}fn after_exact() {{}}\n"));
        let mut hl = Highlighter::new();
        let path = Some(Path::new("bound.rs"));
        let (boundary_prog, _) = drain_updates(&mut hl, 1, &boundary_doc, path, &[0..3], 200);
        assert!(hl.warning(1).is_none());
        assert!(!boundary_prog.pending);
        assert_eq!(
            token_for(hl.lines(1), &boundary_doc, "fn after_exact"),
            Token::Keyword
        );

        let long_line = format!("let big = \"{}\";\n", "x".repeat(MAX_LINE_BYTES));
        assert!(long_line.len() - 1 > MAX_LINE_BYTES);
        let source = format!("fn before() {{}}\n{long_line}fn after() {{ let z = 9; }}\n");
        let mut doc = Document::new(&source);

        let (p1, _) = drain_updates(&mut hl, 2, &doc, path, &[0..4], 200);
        assert!(hl.warning(2).is_some());
        assert!(!p1.pending);
        assert!(p1.changed);
        assert_eq!(token_for(hl.lines(2), &doc, "fn before"), Token::Keyword);
        assert_eq!(token_for(hl.lines(2), &doc, "fn after"), Token::Plain);
        assert_eq!(token_for(hl.lines(2), &doc, "9"), Token::Plain);

        // Idle updates retain the same cached warning without reallocation.
        let p2 = hl.update(2, &doc, path, &[0..4], 200);
        assert!(hl.warning(2).is_some());
        assert!(!p2.pending);
        assert!(!p2.changed);

        // Scrolling past the barrier keeps the warning without spinning.
        let p_past = hl.update(2, &doc, path, &[2..4], 200);
        assert!(hl.warning(2).is_some());
        assert!(!p_past.pending);
        assert!(p_past.changed);
        assert!(hl.lines(2).is_empty());
        let p_past_idle = hl.update(2, &doc, path, &[2..4], 200);
        assert!(hl.warning(2).is_some());
        assert!(!p_past_idle.pending);
        assert!(!p_past_idle.changed);

        // Scroll back to 0..4 before fixing line 1.
        let (p_back, _) = drain_updates(&mut hl, 2, &doc, path, &[0..4], 200);
        assert!(hl.warning(2).is_some());
        assert!(!p_back.pending);
        assert!(p_back.changed);
        // Shortening the long line clears the barrier and restores downstream highlighting.
        let start = doc.line_start(1);
        let end = doc.line_end(1);
        doc.replace(start..end, "let small = 1;").unwrap();
        let (p3, _) = drain_updates(&mut hl, 2, &doc, path, &[0..4], 200);
        assert!(hl.warning(2).is_none());
        assert!(!p3.pending);
        assert!(p3.changed);
        assert_eq!(token_for(hl.lines(2), &doc, "fn after"), Token::Keyword);
        assert_eq!(token_for(hl.lines(2), &doc, "9"), Token::Number);
    }

    #[test]
    fn cached_warning_survives_idle_views_and_clears_on_invalidation() {
        let mut doc = Document::new(&format!(
            "{}\nfn later() {{}}\n",
            "x".repeat(MAX_LINE_BYTES + 1)
        ));
        let other = Document::new("fn other() {}\n");
        let mut hl = Highlighter::new();
        let path = Some(Path::new("blocked.rs"));
        let (first, _) = drain_updates(&mut hl, 11, &doc, path, &[1..2], 200);
        assert!(first.changed);
        assert!(hl.lines(11).is_empty());
        let warning_ptr = hl.warning(11).unwrap().as_ptr();

        let idle = hl.update(11, &doc, path, &[1..2], 200);
        assert!(!idle.changed && !idle.pending);
        assert_eq!(hl.warning(11).unwrap().as_ptr(), warning_ptr);
        drain_updates(&mut hl, 12, &other, path, &[0..1], 200);
        let switched = hl.update(11, &doc, path, &[], 200);
        assert!(!switched.pending);
        assert_eq!(hl.warning(11).unwrap().as_ptr(), warning_ptr);

        let later = doc.line_start(1);
        doc.replace(later..later + 2, "pub fn").unwrap();
        hl.update(11, &doc, path, &[1..2], 0);
        assert_eq!(hl.warning(11).unwrap().as_ptr(), warning_ptr);

        let reset = hl.update(11, &doc, Some(Path::new("blocked.txt")), &[1..2], 0);
        assert!(reset.changed);
        assert!(hl.warning(11).is_none());
        let (blocked_again, _) = drain_updates(&mut hl, 11, &doc, path, &[1..2], 200);
        assert!(blocked_again.changed);
        assert!(hl.warning(11).is_some());

        let end = doc.line_end(0);
        doc.replace(0..end, "let short = 1;").unwrap();
        let invalidated = hl.update(11, &doc, path, &[1..2], 0);
        assert!(invalidated.changed && invalidated.pending);
        assert!(hl.warning(11).is_none());
        drain_updates(&mut hl, 11, &doc, path, &[1..2], 200);
        assert_eq!(token_for(hl.lines(11), &doc, "fn later"), Token::Keyword);
    }

    #[test]
    fn far_separated_visible_windows_share_document_without_caching_middle() {
        let mut source = String::new();
        for idx in 0..600 {
            if idx == 140 {
                source.push_str("/* block */\n");
            } else {
                source.push_str("fn step() { let x = 42; }\n");
            }
        }
        let doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("shared.rs"));

        // First bounded step cannot parse all the way to line 520.
        let p1 = hl.update(7, &doc, path, &[10..20, 500..520], 200);
        assert!(p1.pending);

        let (p_done, _) = drain_updates(&mut hl, 7, &doc, path, &[10..20, 500..520], 200);
        assert!(!p_done.pending);
        assert!(hl.warning(7).is_none());

        // Only the two visible windows (10..20 and 500..520) are cached; middle 20..500 is NOT cached.
        let lines = hl.lines(7);
        assert_eq!(lines.len(), 30);
        for (idx, expected_line) in (10..20).chain(500..520).enumerate() {
            assert_eq!(lines[idx].line, expected_line);
            assert!(lines[idx].spans.iter().any(|s| s.token == Token::Keyword));
            assert!(lines[idx].spans.iter().any(|s| s.token == Token::Number));
        }
        assert!(!lines.iter().any(|r| (20..500).contains(&r.line)));
        assert_eq!(hl.recent_len(7), 0);

        // Unsorted / overlapping / adjacent equivalent views are idle no-ops.
        let idle_unsorted = hl.update(7, &doc, path, &[500..520, 10..20], 200);
        assert!(!idle_unsorted.changed);
        assert!(!idle_unsorted.pending);

        let idle_split = hl.update(7, &doc, path, &[10..15, 13..20, 500..510, 510..520], 200);
        assert!(!idle_split.changed);
        assert!(!idle_split.pending);
    }

    #[test]
    fn shared_checkpoints_transitions_edit_undo_and_collapse() {
        let mut source = String::new();
        for _ in 0..600 {
            source.push_str("fn step() { let x = 42; }\n");
        }
        let mut doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("shared_edit.rs"));

        // Prime cache + checkpoints across 0..520.
        drain_updates(&mut hl, 1, &doc, path, &[10..20, 500..520], 200);
        assert_eq!(hl.lines(1).len(), 30);

        // Move one view while preserving the other visible window.
        drain_updates(&mut hl, 1, &doc, path, &[135..145, 500..520], 20);
        assert_eq!(hl.lines(1).len(), 30);
        assert_eq!(hl.lines(1)[0].line, 135);
        assert_eq!(hl.lines(1)[10].line, 500);

        // Move the second view independently.
        drain_updates(&mut hl, 1, &doc, path, &[135..145, 390..400], 20);
        assert_eq!(hl.lines(1).len(), 20);
        assert_eq!(hl.lines(1)[10].line, 390);

        // Edit near top opening a multiline comment affects both shared windows.
        let line5 = doc.line_start(5);
        doc.replace(line5..line5, "/* open\n").unwrap();
        drain_updates(&mut hl, 1, &doc, path, &[135..145, 500..520], 200);
        assert_eq!(hl.lines(1).len(), 30);
        assert!(
            hl.lines(1)
                .iter()
                .all(|r| !r.spans.is_empty() && r.spans.iter().all(|s| s.token == Token::Comment))
        );

        // Undo restores code tokens in both far-separated windows.
        doc.undo().unwrap();
        drain_updates(&mut hl, 1, &doc, path, &[135..145, 500..520], 200);
        assert_eq!(hl.lines(1).len(), 30);
        assert!(
            hl.lines(1)
                .iter()
                .all(|r| r.spans.iter().any(|s| s.token == Token::Keyword))
        );

        // Collapse to single visible pane moves the hidden pane's rows out of lines() into recent.
        let collapsed = hl.update(1, &doc, path, &[500..520], 200);
        assert!(collapsed.changed);
        assert!(!collapsed.pending);
        assert_eq!(hl.lines(1).len(), 20);
        assert_eq!(hl.lines(1)[0].line, 500);

        // All views hidden (&[]): clears visible lines() cleanly while preserving recent + checkpoints.
        let hidden = hl.update(1, &doc, path, &[], 200);
        assert!(hidden.changed);
        assert!(!hidden.pending);
        assert!(hl.lines(1).is_empty());

        // Restore hidden views, including rows not previously visible.
        drain_updates(&mut hl, 1, &doc, path, &[135..145, 515..525], 40);
        assert_eq!(hl.lines(1).len(), 20);
    }

    #[test]
    fn normalize_clamps_merges_and_ignores_invalid_ranges() {
        let mut source = String::new();
        for _ in 0..20 {
            source.push_str("let v = 1;\n");
        }
        let doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("norm.rs"));

        let (prog, _) = drain_updates(
            &mut hl,
            1,
            &doc,
            path,
            &[20..10, 5..5, 100..200, 12..15, 2..8, 5..12, 18..50],
            200,
        );
        assert!(prog.changed);
        assert!(!prog.pending);
        let lines = hl.lines(1);
        // Expected normalized union on 21-line doc (20 trailing newline -> 21 lines): 2..15 and 18..21 => 16 rows.
        let expected: Vec<usize> = (2..15).chain(18..doc.line_count()).collect();
        let actual: Vec<usize> = lines.iter().map(|r| r.line).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn long_line_barrier_across_multiple_visible_windows() {
        let long_line = format!("let big = \"{}\";\n", "x".repeat(MAX_LINE_BYTES));
        let mut source = String::new();
        for _ in 0..10 {
            source.push_str("fn top() { let a = 1; }\n");
        }
        source.push_str(&long_line);
        for _ in 0..20 {
            source.push_str("fn bot() { let b = 2; }\n");
        }
        let mut doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("barrier.rs"));

        let (p1, _) = drain_updates(&mut hl, 9, &doc, path, &[2..6, 15..20], 200);
        assert!(hl.warning(9).is_some());
        assert!(!p1.pending);
        assert!(p1.changed);
        assert_eq!(hl.lines(9).len(), 4);
        assert_eq!(hl.lines(9)[0].line, 2);
        assert_eq!(hl.lines(9)[3].line, 5);

        // Scrolling past the barrier retains the warning without looping.
        let p2 = hl.update(9, &doc, path, &[15..20], 200);
        assert!(hl.warning(9).is_some());
        assert!(!p2.pending);
        assert!(p2.changed);
        assert!(hl.lines(9).is_empty());

        // Fixing the long line clears the barrier and highlights downstream window.
        let start = doc.line_start(10);
        let end = doc.line_end(10);
        doc.replace(start..end, "let ok = 1;").unwrap();
        let (p3, _) = drain_updates(&mut hl, 9, &doc, path, &[2..6, 15..20], 200);
        assert!(hl.warning(9).is_none());
        assert!(!p3.pending);
        assert!(p3.changed);
        assert_eq!(hl.lines(9).len(), 9);
        assert_eq!(hl.lines(9)[4].line, 15);
        assert!(
            hl.lines(9)[4]
                .spans
                .iter()
                .any(|s| s.token == Token::Keyword)
        );
    }

    #[test]
    fn backward_scroll_restores_cached_tokens_with_zero_parse_budget() {
        let mut source = String::new();
        for idx in 0..240 {
            if idx % 20 == 5 {
                source.push_str("/* block */\n");
            } else {
                source.push_str("fn step() { let val = 42; }\n");
            }
        }
        let doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("scroll.rs"));

        drain_updates(&mut hl, 1, &doc, path, &[0..30], 200);

        for start in (3..=90).step_by(3) {
            drain_updates(&mut hl, 1, &doc, path, &[start..start + 30], 200);
        }
        assert_eq!(hl.lines(1).len(), 30);
        assert_eq!(hl.lines(1)[0].line, 90);
        assert_eq!(hl.lines(1)[29].line, 119);
        assert_eq!(hl.recent_len(1), 90);

        // Cached highlights remain available even when no parsing is allowed.
        for start in (0..90).step_by(3).rev() {
            let prog = hl.update(1, &doc, path, &[start..start + 30], 0);
            assert!(!prog.pending);
            assert!(prog.changed);
        }
        assert_eq!(hl.lines(1)[0].line, 0);
        assert_eq!(hl.lines(1)[29].line, 29);
        assert_eq!(hl.lines(1).len(), 30);
        for row in hl.lines(1) {
            let expected = if row.line % 20 == 5 {
                Token::Comment
            } else {
                Token::Keyword
            };
            assert!(row.spans.iter().any(|span| span.token == expected));
        }

        // Idle re-update stays a no-op with pending=false.
        let idle = hl.update(1, &doc, path, &[0..30], 0);
        assert!(!idle.changed);
        assert!(!idle.pending);
    }

    #[test]
    fn recent_cache_bounds_rows_and_payload_and_keeps_large_visible_view_renderable() {
        // 1. Row cap: 512 rows max when scrolling past 650 lines.
        let mut source = String::new();
        for _ in 0..700 {
            source.push_str("let v = 1;\n");
        }
        let doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("rows.rs"));

        for start in (0..640).step_by(20) {
            drain_updates(&mut hl, 1, &doc, path, &[start..start + 20], 200);
        }
        assert_eq!(hl.lines(1).len(), 20);
        assert!(hl.recent_len(1) <= RECENT_MAX_ROWS);
        assert_eq!(hl.recent_len(1), RECENT_MAX_ROWS);
        assert!(hl.recent_bytes(1) <= RECENT_MAX_BYTES);

        // Recently evicted lines (e.g. 600..620) hit recent cache with zero parses;
        // oldest evicted lines (0..20) were dropped and require reparsing.
        let hit = hl.update(1, &doc, path, &[600..620], 0);
        assert!(!hit.pending);
        assert!(hit.changed);

        let miss = hl.update(1, &doc, path, &[0..20], 0);
        assert!(miss.pending);
        drain_updates(&mut hl, 1, &doc, path, &[0..20], 200);

        // Visible view larger than 512 rows remains fully renderable.
        drain_updates(&mut hl, 2, &doc, path, &[0..600], 200);
        assert_eq!(hl.lines(2).len(), 600);
        hl.update(2, &doc, path, &[], 200);
        assert!(hl.lines(2).is_empty());
        assert_eq!(hl.recent_len(2), RECENT_MAX_ROWS);

        // 2. Byte payload cap: many spans per line exceed 2 MiB well before 512 rows.
        let mut dense_line = String::new();
        for i in 0..350 {
            dense_line.push_str(if i % 2 == 0 { "let a=1;" } else { "\"s\";" });
        }
        dense_line.push('\n');
        assert!(dense_line.len() < MAX_LINE_BYTES);
        let mut dense_source = String::new();
        for _ in 0..180 {
            dense_source.push_str(&dense_line);
        }
        let dense_doc = Document::new(&dense_source);
        for start in (0..160).step_by(10) {
            drain_updates(&mut hl, 3, &dense_doc, path, &[start..start + 10], 200);
        }
        assert!(hl.recent_len(3) < RECENT_MAX_ROWS);
        assert!(hl.recent_bytes(3) <= RECENT_MAX_BYTES);
        assert!(hl.recent_bytes(3) > 0);
    }

    #[test]
    fn recent_cache_invalidation_edit_barrier_language_and_forget() {
        let mut source = String::new();
        for _ in 0..100 {
            source.push_str("fn f() { let n = 1; }\n");
        }
        let mut doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let rs_path = Some(Path::new("inv.rs"));

        drain_updates(&mut hl, 1, &doc, rs_path, &[0..30], 200);
        drain_updates(&mut hl, 1, &doc, rs_path, &[30..60], 200);
        assert_eq!(hl.recent_len(1), 30);

        // Edit line 15 while viewing 30..60: recent retains 0..15 and invalidates 15..30.
        let line15 = doc.line_start(15);
        doc.replace(line15..line15, "/* ").unwrap();
        drain_updates(&mut hl, 1, &doc, rs_path, &[30..60], 200);
        assert_eq!(hl.recent_len(1), 15);
        assert!(
            hl.lines(1)
                .iter()
                .all(|r| !r.spans.is_empty() && r.spans.iter().all(|s| s.token == Token::Comment))
        );

        // Scroll back to 0..30: 0..15 comes from recent, only 15..30 reparses.
        let partial = hl.update(1, &doc, rs_path, &[0..30], 0);
        assert!(partial.pending);
        assert!(hl.lines(1).iter().all(|row| row.line < 15));
        drain_updates(&mut hl, 1, &doc, rs_path, &[0..30], 200);
        assert_eq!(token_for(hl.lines(1), &doc, "fn f"), Token::Keyword);
        let row20 = hl.lines(1).iter().find(|r| r.line == 20).unwrap();
        assert!(row20.spans.iter().all(|s| s.token == Token::Comment));

        // Path/language switch clears recent cache completely.
        hl.update(1, &doc, Some(Path::new("inv.txt")), &[0..30], 200);
        assert_eq!(hl.recent_len(1), 0);
        assert_eq!(hl.recent_bytes(1), 0);
        assert!(hl.lines(1).is_empty());

        // Long line barrier clears >= barrier from recent too.
        let long_line = format!("let big = \"{}\";\n", "x".repeat(MAX_LINE_BYTES));
        let mut barrier_doc = Document::new(&format!(
            "{}{}",
            "let a = 1;\n".repeat(11),
            "let b = 2;\n".repeat(10)
        ));
        drain_updates(&mut hl, 2, &barrier_doc, rs_path, &[0..8], 200);
        drain_updates(&mut hl, 2, &barrier_doc, rs_path, &[12..18], 200);
        assert_eq!(hl.recent_len(2), 8);
        // Scroll to 0..8 so 12..18 enters recent, then make line 10 > 16 KiB.
        hl.update(2, &barrier_doc, rs_path, &[0..8], 200);
        assert_eq!(hl.recent_len(2), 6);
        let l10_start = barrier_doc.line_start(10);
        let l10_end = barrier_doc.line_end(10);
        barrier_doc
            .replace(l10_start..l10_end, long_line.trim_end_matches('\n'))
            .unwrap();
        let (blocked, _) = drain_updates(&mut hl, 2, &barrier_doc, rs_path, &[0..18], 200);
        assert!(hl.warning(2).is_some());
        assert!(!blocked.pending);
        assert_eq!(hl.lines(2).len(), 10);
        assert_eq!(hl.recent_len(2), 0);
        hl.forget(2);
        assert!(hl.warning(2).is_none());
        assert_eq!(hl.recent_len(2), 0);
        assert_eq!(hl.recent_bytes(2), 0);
    }

    #[test]
    fn markdown_semantic_categories_preserve_nested_context_and_plain_text() {
        let doc = Document::new(concat!(
            "# Café 🧪\n\n",
            "Ordinary **strong** *slanted* `inline` [caption](https://example.invalid).\n\n",
            "![imagealt](image.png)\n\n",
            "> quoted **nested** `quotedcode`\n\n",
            "- bulletbody\n1. numberedbody\n\n---\n\n",
            "```rust\n# raw_heading **raw_emphasis**\n```\n\n",
            "    indented\n\n",
            "[reference]: https://reference.invalid\n\n",
            "<!-- # hidden_heading **hidden_emphasis** -->\n\n",
            "Final ordinary text",
        ));
        let mut hl = Highlighter::new();
        drain_updates(
            &mut hl,
            1,
            &doc,
            Some(Path::new("notes.md")),
            &[0..doc.line_count()],
            200,
        );
        let lines = hl.lines(1);
        for (text, token) in [
            ("Café 🧪", Token::Heading),
            ("Ordinary", Token::Plain),
            ("strong", Token::Emphasis),
            ("slanted", Token::Emphasis),
            ("inline", Token::Code),
            ("caption", Token::Link),
            ("https://example.invalid", Token::Link),
            ("imagealt", Token::Link),
            ("image.png", Token::Link),
            ("quoted ", Token::Quote),
            ("nested", Token::Emphasis),
            ("quotedcode", Token::Code),
            ("- bulletbody", Token::Marker),
            ("1. numberedbody", Token::Marker),
            ("bulletbody", Token::Plain),
            ("numberedbody", Token::Plain),
            ("---", Token::Marker),
            ("raw_heading", Token::Code),
            ("raw_emphasis", Token::Code),
            ("indented", Token::Code),
            ("reference]", Token::Link),
            ("https://reference.invalid", Token::Link),
            ("hidden_heading", Token::Comment),
            ("hidden_emphasis", Token::Comment),
            ("Final ordinary", Token::Plain),
        ] {
            assert_eq!(token_for(lines, &doc, text), token, "{text}");
        }
    }

    #[test]
    fn markdown_scroll_backward_and_fenced_edit_invalidation() {
        let mut md = String::new();
        md.push_str("# Document Title\n\nIntro `inline` text.\n\n");
        for i in 0..180 {
            md.push_str(&format!("- Bullet item {i} with **bold** text\n"));
        }
        md.push_str("\n```rust\nfn inside_fence() { let k = 99; }\n```\n");
        md.push_str("\nAfter fence.\n");
        let mut doc = Document::new(&md);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("notes.md"));

        drain_updates(&mut hl, 1, &doc, path, &[0..25], 200);
        assert_eq!(hl.lines(1).len(), 25);
        assert_eq!(
            token_for(hl.lines(1), &doc, "Document Title"),
            Token::Heading
        );
        assert_eq!(token_for(hl.lines(1), &doc, "inline"), Token::Code);
        assert_eq!(token_for(hl.lines(1), &doc, "bold"), Token::Emphasis);
        drain_updates(&mut hl, 1, &doc, path, &[160..190], 200);
        assert_eq!(hl.lines(1).len(), 30);
        assert_eq!(token_for(hl.lines(1), &doc, "inside_fence"), Token::Code);
        assert_eq!(hl.recent_len(1), 25);

        // Backward scroll to 0..25 hits recent cache with zero reparsing.
        let back = hl.update(1, &doc, path, &[0..25], 0);
        assert!(!back.pending);
        assert!(back.changed);
        assert_eq!(hl.lines(1)[0].line, 0);
        assert_eq!(
            token_for(hl.lines(1), &doc, "Document Title"),
            Token::Heading
        );
        assert_eq!(token_for(hl.lines(1), &doc, "inline"), Token::Code);

        // An HTML comment opened before the fence invalidates downstream scopes.
        let fence_pos = char_offset_of(&doc, "```rust");
        doc.replace(fence_pos..fence_pos, "<!--\n").unwrap();
        drain_updates(&mut hl, 1, &doc, path, &[175..190], 200);
        assert_eq!(token_for(hl.lines(1), &doc, "inside_fence"), Token::Comment);
        doc.undo().unwrap();
        drain_updates(&mut hl, 1, &doc, path, &[175..190], 200);
        assert_eq!(token_for(hl.lines(1), &doc, "inside_fence"), Token::Code);
    }

    #[test]
    fn large_jump_does_not_capture_invisible_gap_into_recent_cache() {
        let mut source = String::new();
        for _ in 0..1000 {
            source.push_str("let item = 7;\n");
        }
        let doc = Document::new(&source);
        let mut hl = Highlighter::new();
        let path = Some(Path::new("jump.rs"));

        drain_updates(&mut hl, 1, &doc, path, &[0..20], 200);
        assert_eq!(hl.lines(1).len(), 20);

        drain_updates(&mut hl, 1, &doc, path, &[900..920], 200);
        assert_eq!(hl.lines(1).len(), 20);
        assert_eq!(hl.lines(1)[0].line, 900);
        assert_eq!(hl.lines(1)[19].line, 919);
        // Only the previously visible 0..20 window was retained; the 880-line gap was never cached.
        assert_eq!(hl.recent_len(1), 20);
    }
}

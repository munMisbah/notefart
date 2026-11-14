//! F1 help overlay.
//!
//! Streams the shared shortcut catalog (see [`crate::shortcuts`]) into a
//! responsive, scrollable dialog. Drawing, row counting, the scroll limit,
//! and the page size all go through the exact same layout and wrapping pass,
//! so scroll offsets always line up with what is rendered. All catalog text
//! is static ASCII; nothing here touches the filesystem and no per-frame
//! heap copies of the catalog are made — the wrapper streams `&'static str`
//! words through a fixed-size stack row.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Clear},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::shortcuts::SECTIONS;
use crate::theme::{IconMode, Theme};
use crate::ui::{self, CommandHit, MIN_DIALOG_SIZE};

/// Largest help dialog width; wider terminals center it with side margins.
const HELP_MAX_WIDTH: u16 = 72;
/// Largest help dialog height; taller terminals center it with margins.
const HELP_MAX_HEIGHT: u16 = 24;
/// Footer width below which chips drop their labels (`[Esc]` / `[F1]`).
const FOOTER_COMPACT_WIDTH: u16 = 24;

/// Styling group of a streamed catalog segment.
#[derive(Copy, Clone, Eq, PartialEq)]
enum SegKind {
    Title,
    Key,
    Desc,
}

/// One placed word on a visual row: `col` is the column inside the catalog
/// width where the word starts (including any leading space gap).
#[derive(Clone, Copy)]
struct Slot {
    col: u16,
    kind: SegKind,
    text: &'static str,
}

/// Maximum words placed on one visual row. Rows break early at the cap, and
/// because every consumer shares that rule through the same traversal, row
/// counts and drawing stay consistent even at absurd wrap widths.
const ROW_SLOTS: usize = 16;

/// A wrapped visual row. An empty row (`len == 0`) is a blank separator row.
struct TextRow {
    slots: [Slot; ROW_SLOTS],
    len: usize,
}

impl Default for TextRow {
    fn default() -> Self {
        Self {
            slots: [Slot {
                col: 0,
                kind: SegKind::Title,
                text: "",
            }; ROW_SLOTS],
            len: 0,
        }
    }
}

/// A word awaiting placement, plus whether a single-space gap precedes it
/// when it does not start a visual row.
#[derive(Clone, Copy)]
struct Word {
    kind: SegKind,
    text: &'static str,
    spaced: bool,
}

/// Greedy word wrapper for one logical catalog line (a section title, or a
/// `key: description` binding joined by a `: ` separator). Words wider than
/// the wrap width are hard-wrapped at grapheme boundaries.
struct WrapState {
    head: &'static str,
    tail: &'static str,
    /// When true, a `:` separator word is appended after `head` and `head`
    /// words are styled as keys instead of titles.
    colon: bool,
    in_head: bool,
    offset: usize,
    pending: Option<Word>,
    done: bool,
}

impl WrapState {
    fn binding(key: &'static str, desc: &'static str) -> Self {
        Self {
            head: key,
            tail: desc,
            colon: true,
            in_head: true,
            offset: 0,
            pending: None,
            done: false,
        }
    }

    fn plain(text: &'static str) -> Self {
        Self {
            head: text,
            tail: "",
            colon: false,
            in_head: true,
            offset: 0,
            pending: None,
            done: false,
        }
    }

    fn head_kind(&self) -> SegKind {
        if self.colon {
            SegKind::Key
        } else {
            SegKind::Title
        }
    }

    fn next_word(&mut self) -> Option<Word> {
        loop {
            if self.in_head {
                if let Some((text, first)) = next_word_of(self.head, &mut self.offset) {
                    return Some(Word {
                        kind: self.head_kind(),
                        text,
                        spaced: !first,
                    });
                }
                self.in_head = false;
                self.offset = 0;
                if self.colon {
                    return Some(Word {
                        kind: SegKind::Key,
                        text: ":",
                        spaced: false,
                    });
                }
                continue;
            }
            // Every description word is preceded by a single space: the one
            // after the `:` separator and the ones between words alike. The
            // gap is dropped again when the word starts a visual row.
            return next_word_of(self.tail, &mut self.offset).map(|(text, _)| Word {
                kind: SegKind::Desc,
                text,
                spaced: true,
            });
        }
    }

    /// Wraps and returns the next visual row, or `None` once the line is
    /// exhausted.
    fn next_row(&mut self, wrap_width: u16) -> Option<TextRow> {
        if self.done || wrap_width == 0 {
            self.done = true;
            return None;
        }
        let max = wrap_width as usize;
        let mut row = TextRow::default();
        let mut col = 0usize;
        loop {
            let word = match self.pending.take().or_else(|| self.next_word()) {
                Some(word) => word,
                None => {
                    self.done = true;
                    break;
                }
            };
            if row.len >= ROW_SLOTS {
                // Slot cap reached: carry the word to the next row intact.
                self.pending = Some(Word {
                    spaced: false,
                    ..word
                });
                break;
            }
            let word_w = UnicodeWidthStr::width(word.text).max(1);
            let gap = usize::from(word.spaced && col > 0);
            if col + gap + word_w <= max {
                row.slots[row.len] = Slot {
                    col: (col + gap) as u16,
                    kind: word.kind,
                    text: word.text,
                };
                row.len += 1;
                col += gap + word_w;
                continue;
            }
            if col == 0 {
                // The word alone cannot fit the row: hard-wrap it at
                // grapheme boundaries and queue the remainder.
                let mut used = 0usize;
                let mut end = 0usize;
                for grapheme in word.text.graphemes(true) {
                    let gw = UnicodeWidthStr::width(grapheme).max(1);
                    if used + gw > max {
                        break;
                    }
                    used += gw;
                    end += grapheme.len();
                }
                if end == 0 {
                    // Lone grapheme wider than the row: place it; drawing
                    // clips it to the row width.
                    let grapheme = word.text.graphemes(true).next().unwrap_or("");
                    end = grapheme.len();
                }
                row.slots[0] = Slot {
                    col: 0,
                    kind: word.kind,
                    text: &word.text[..end],
                };
                row.len = 1;
                let rest = &word.text[end..];
                self.pending = (!rest.is_empty()).then(|| Word {
                    kind: word.kind,
                    text: rest,
                    spaced: false,
                });
                break;
            }
            // Row full: continue this word on the next row.
            self.pending = Some(Word {
                spaced: false,
                ..word
            });
            break;
        }
        if row.len == 0 {
            self.done = true;
            return None;
        }
        Some(row)
    }
}

/// Splits the next space-separated word of `text` starting at `*offset`.
/// Returns the word slice and whether it is the segment's first word.
/// Offsets only advance across ASCII-space boundaries, so slicing stays on
/// char boundaries.
fn next_word_of(text: &'static str, offset: &mut usize) -> Option<(&'static str, bool)> {
    let rest = &text[*offset..];
    let skipped = rest.len() - rest.trim_start_matches(' ').len();
    let rest = &rest[skipped..];
    if rest.is_empty() {
        *offset += skipped;
        return None;
    }
    let end = rest.find(' ').unwrap_or(rest.len());
    let first = *offset == 0;
    let word = &rest[..end];
    *offset += skipped + end;
    Some((word, first))
}

/// Logical catalog lines streamed without allocation.
enum Logical {
    Blank,
    Title(&'static str),
    Entry {
        key: &'static str,
        desc: &'static str,
    },
}

enum Phase {
    Title,
    Bindings,
    Blank,
    Done,
}

struct LogicalRows {
    section: usize,
    binding: usize,
    phase: Phase,
}

impl LogicalRows {
    fn next_logical(&mut self) -> Option<Logical> {
        loop {
            match self.phase {
                Phase::Blank => {
                    self.phase = Phase::Title;
                    return Some(Logical::Blank);
                }
                Phase::Title => {
                    let title = SECTIONS[self.section].title;
                    self.phase = Phase::Bindings;
                    return Some(Logical::Title(title));
                }
                Phase::Bindings => {
                    let bindings = SECTIONS[self.section].bindings;
                    if self.binding < bindings.len() {
                        let (key, desc) = bindings[self.binding];
                        self.binding += 1;
                        return Some(Logical::Entry { key, desc });
                    }
                    self.section += 1;
                    self.binding = 0;
                    if self.section >= SECTIONS.len() {
                        self.phase = Phase::Done;
                        return None;
                    }
                    self.phase = Phase::Blank;
                }
                Phase::Done => return None,
            }
        }
    }
}

/// The single visual-row traversal shared by draw, counting, and the scroll
/// limits.
struct RowStream {
    wrap_width: u16,
    logical: LogicalRows,
    active: Option<WrapState>,
}

impl RowStream {
    fn new(wrap_width: u16) -> Self {
        Self {
            wrap_width,
            logical: LogicalRows {
                section: 0,
                binding: 0,
                phase: Phase::Title,
            },
            active: None,
        }
    }

    fn next_row(&mut self) -> Option<TextRow> {
        loop {
            if let Some(active) = &mut self.active {
                match active.next_row(self.wrap_width) {
                    Some(row) => return Some(row),
                    None => self.active = None,
                }
            }
            match self.logical.next_logical()? {
                Logical::Blank => return Some(TextRow::default()),
                Logical::Title(title) => self.active = Some(WrapState::plain(title)),
                Logical::Entry { key, desc } => {
                    self.active = Some(WrapState::binding(key, desc));
                }
            }
        }
    }
}

/// Counts every visual row of the catalog at `wrap_width` using the same
/// traversal as drawing, without allocating.
fn total_rows(wrap_width: u16) -> usize {
    let mut stream = RowStream::new(wrap_width);
    let mut rows = 0usize;
    while stream.next_row().is_some() {
        rows += 1;
    }
    rows
}

/// Shared overlay geometry for a viewport-sized area. Below
/// [`MIN_DIALOG_SIZE`] the overlay degrades to a resize notice (`tiny`).
fn layout(viewport: Rect) -> HelpLayout {
    let (width, height) = (viewport.width, viewport.height);
    if width < MIN_DIALOG_SIZE.0 || height < MIN_DIALOG_SIZE.1 {
        return HelpLayout {
            tiny: true,
            popup: viewport,
            catalog: Rect::default(),
            footer: Rect {
                x: viewport.x,
                y: viewport.y.saturating_add(height.saturating_sub(1)),
                width,
                height: u16::from(height > 0),
            },
        };
    }
    let popup_w = width.min(HELP_MAX_WIDTH);
    let popup_h = height.min(HELP_MAX_HEIGHT);
    let popup = Rect {
        x: viewport.x + (width - popup_w) / 2,
        y: viewport.y + (height - popup_h) / 2,
        width: popup_w,
        height: popup_h,
    };
    let catalog = Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup_w - 2,
        height: popup_h - 3,
    };
    let footer = Rect {
        x: popup.x + 1,
        y: popup.y + popup_h - 2,
        width: popup_w - 2,
        height: 1,
    };
    HelpLayout {
        tiny: false,
        popup,
        catalog,
        footer,
    }
}

struct HelpLayout {
    tiny: bool,
    popup: Rect,
    catalog: Rect,
    footer: Rect,
}

/// Number of catalog rows visible at once (minimum 1).
pub(crate) fn page_size(width: u16, height: u16) -> usize {
    let layout = layout(Rect::new(0, 0, width, height));
    if layout.tiny {
        return 1;
    }
    layout.catalog.height.max(1) as usize
}

/// Largest scroll offset (in visual rows) that still shows content.
pub(crate) fn scroll_limit(width: u16, height: u16) -> usize {
    let layout = layout(Rect::new(0, 0, width, height));
    if layout.tiny {
        return 0;
    }
    total_rows(layout.catalog.width).saturating_sub(page_size(width, height))
}

/// Renders the help overlay: clears the dialog area, streams the wrapped
/// catalog rows `[scroll, scroll + visible)`, draws the static return chips,
/// and registers their exact visible hit rectangles in `commands` (cleared
/// by the caller). The underlying editor cursor stays hidden because no
/// cursor position is ever set here.
pub(crate) fn draw(
    frame: &mut Frame,
    viewport: Rect,
    scroll: usize,
    theme: &Theme,
    icons: IconMode,
    commands: &mut Vec<CommandHit>,
) {
    let layout = layout(viewport);
    if layout.tiny {
        draw_too_small(frame, layout.popup, layout.footer, theme, commands);
        return;
    }

    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        ui::dialog_block("Help", crate::theme::ChromeIcon::Help, theme, icons),
        layout.popup,
    );

    let buf = frame.buffer_mut();
    draw_footer(buf, layout.footer, theme, commands);

    let title_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(theme.fg).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(theme.muted);

    let mut stream = RowStream::new(layout.catalog.width);
    let scroll = if scroll == 0 {
        0
    } else {
        scroll.min(total_rows(layout.catalog.width).saturating_sub(layout.catalog.height as usize))
    };
    for _ in 0..scroll {
        if stream.next_row().is_none() {
            return;
        }
    }
    for row_y in layout.catalog.top()..layout.catalog.bottom() {
        let Some(row) = stream.next_row() else {
            break;
        };
        if row.len == 0 {
            continue; // blank separator between sections
        }
        for slot in &row.slots[..row.len] {
            let style = match slot.kind {
                SegKind::Title => title_style,
                SegKind::Key => key_style,
                SegKind::Desc => desc_style,
            };
            let text = ui::sanitize_ui_cow(slot.text);
            let budget = layout
                .catalog
                .right()
                .saturating_sub(layout.catalog.x + slot.col) as usize;
            ui::write_clipped_str(
                buf,
                layout.catalog.x + slot.col,
                row_y,
                budget,
                text.as_ref(),
                style,
            );
        }
    }
}

/// Draws the visible static return chips and appends a [`CommandHit`] with
/// the exact visible rectangle and the trusted key event for each chip that
/// actually got cells. Never registers anything that is not drawn.
fn draw_footer(buf: &mut Buffer, footer: Rect, theme: &Theme, commands: &mut Vec<CommandHit>) {
    if footer.width == 0 || footer.height == 0 {
        return;
    }
    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(theme.muted);
    let compact = footer.width < FOOTER_COMPACT_WIDTH;
    let chips = [
        ("Esc", "Return", KeyCode::Esc),
        ("F1", "Close", KeyCode::F(1)),
    ];
    let mut cursor = footer.x;
    for (key, label, code) in chips {
        let chip_x = cursor;
        let mut written = 0u16;
        let budget = |x: u16| -> usize { footer.right().saturating_sub(x) as usize };
        written += ui::write_clipped_str(
            buf,
            cursor.saturating_add(written),
            footer.y,
            budget(cursor.saturating_add(written)),
            "[",
            key_style,
        );
        written += ui::write_clipped_str(
            buf,
            cursor.saturating_add(written),
            footer.y,
            budget(cursor.saturating_add(written)),
            key,
            key_style,
        );
        written += ui::write_clipped_str(
            buf,
            cursor.saturating_add(written),
            footer.y,
            budget(cursor.saturating_add(written)),
            "]",
            key_style,
        );
        if !compact {
            written += ui::write_clipped_str(
                buf,
                cursor.saturating_add(written),
                footer.y,
                budget(cursor.saturating_add(written)),
                " ",
                label_style,
            );
            written += ui::write_clipped_str(
                buf,
                cursor.saturating_add(written),
                footer.y,
                budget(cursor.saturating_add(written)),
                label,
                label_style,
            );
        }
        if written > 0 {
            commands.push(CommandHit {
                area: Rect {
                    x: chip_x,
                    y: footer.y,
                    width: written,
                    height: 1,
                },
                key: KeyEvent::new(code, KeyModifiers::NONE),
            });
        }
        cursor = chip_x
            .saturating_add(written)
            .saturating_add(u16::from(compact) + 1);
    }
}

/// Below the minimum dialog size there is no room for the catalog: show the
/// resize notice and the visible return chips only. No scrolling and no
/// other commands exist in this mode.
fn draw_too_small(
    frame: &mut Frame,
    viewport: Rect,
    footer: Rect,
    theme: &Theme,
    commands: &mut Vec<CommandHit>,
) {
    frame.render_widget(Clear, viewport);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.canvas)),
        viewport,
    );
    let buf = frame.buffer_mut();
    let notice_style = Style::default().bg(theme.canvas).fg(theme.fg);
    let lines = ["Terminal too small", "Minimum size 20x6"];
    for (idx, line) in lines.iter().enumerate() {
        // Keep the last row for the return chips.
        if idx + 1 >= viewport.height as usize {
            break;
        }
        let y = viewport.y + idx as u16;
        let line_w = UnicodeWidthStr::width(*line) as u16;
        let x = viewport.x + viewport.width.saturating_sub(line_w.min(viewport.width)) / 2;
        ui::write_clipped_str(buf, x, y, viewport.width as usize, *line, notice_style);
    }
    draw_footer(buf, footer, theme, commands);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rightmost occupied display column of the actual placed words.
    fn row_extent(row: &TextRow) -> usize {
        row.slots[..row.len]
            .iter()
            .map(|slot| slot.col as usize + UnicodeWidthStr::width(slot.text))
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn word_gaps_and_attached_colon_have_correct_columns() {
        for (mut wrapping, expected) in [
            (WrapState::plain("aa bb cc"), &[0u16, 3, 6][..]),
            (WrapState::binding("K", "a b"), &[0u16, 1, 3, 5][..]),
        ] {
            let row = wrapping.next_row(8).unwrap();
            assert_eq!(
                row.slots[..row.len]
                    .iter()
                    .map(|slot| slot.col)
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(wrapping.next_row(8).is_none());
        }
    }

    #[test]
    fn old_scroll_offset_after_resize_renders_the_last_page() {
        use crate::theme::{ColorMode, ThemeKind};
        use ratatui::{Terminal, backend::TestBackend};
        let theme = Theme::new(ThemeKind::Dark, ColorMode::None);
        let old_offset = scroll_limit(20, 6);
        let limit = scroll_limit(100, 24);
        assert!(old_offset > limit);
        let render = |offset| {
            let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
            let mut commands = Vec::new();
            terminal
                .draw(|frame| {
                    let viewport = frame.area();
                    draw(
                        frame,
                        viewport,
                        offset,
                        &theme,
                        IconMode::Ascii,
                        &mut commands,
                    );
                })
                .unwrap();
            terminal
        };
        assert_eq!(
            render(old_offset).backend().buffer(),
            render(limit).backend().buffer()
        );
    }

    #[test]
    fn rows_never_exceed_wrap_width() {
        for width in [18u16, 20, 37, 70] {
            let mut stream = RowStream::new(width);
            while let Some(row) = stream.next_row() {
                assert!(
                    row_extent(&row) <= width as usize,
                    "row extent exceeds wrap width {width}"
                );
            }
        }
    }

    #[test]
    fn scroll_and_page_match_geometry() {
        for (width, height) in [(20u16, 6u16), (40, 10), (80, 30), (19, 6), (20, 5), (10, 4)] {
            let limit = scroll_limit(width, height);
            let page = page_size(width, height);
            assert!(page >= 1, "page_size must be at least 1");
            let layout = layout(Rect::new(0, 0, width, height));
            if layout.tiny {
                assert_eq!(limit, 0, "tiny viewport must not scroll");
            } else {
                let total = total_rows(layout.catalog.width);
                assert_eq!(limit, total.saturating_sub(page));
            }
        }
    }
}

use std::borrow::Cow;
use std::fmt::{self, Write as _};
use std::ops::Range;
use std::path::Path;

use crate::field::{FieldKind, FieldView};
use crate::file_io::{FileFormat, Newline};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::chrome::{self, MIN_PANE_HEIGHT};
use crate::document::Document;
use crate::explorer::{EntryKind, Explorer};
use crate::highlight::{LineTokens, Token, TokenSpan};
use crate::layout::{Axis, Divider, Layout, PaneMinimum, SplitTree};
use crate::theme::{self, ChromeIcon, IconKind, IconMode, Theme};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub const MIN_DIALOG_SIZE: (u16, u16) = (20, 6);
pub const MIN_SEARCH_SIZE: (u16, u16) = (40, 10);

const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabCaption<'a> {
    pub id: u64,
    pub label: &'a str,
    pub dirty: bool,
    pub read_only: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabHit {
    pub id: u64,
    pub body: Rect,
    pub close: Rect,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PaneGeometry {
    pub id: u64,
    pub area: Rect,
    pub text_area: Rect,
    pub tabs: Vec<TabHit>,
    pub previous_tab: Option<Rect>,
    pub next_tab: Option<Rect>,
}

#[derive(Debug)]
pub(crate) struct FieldHit {
    pub kind: FieldKind,
    pub area: Rect,
    pub stops: Vec<(u16, usize)>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommandHit {
    pub area: Rect,
    pub key: KeyEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChoiceKind {
    Unsaved,
    Overwrite,
    Conflict,
}

#[derive(Debug, Default)]
pub struct Geometry {
    pub panes: Vec<PaneGeometry>,
    pub sidebar: Option<Rect>,
    pub sidebar_parent: Option<Rect>,
    pub sidebar_panel: Option<Rect>,
    pub explorer_divider: Option<Rect>,
    pub splits: Vec<Divider>,
    pub collapsed: bool,
    pub fields: Vec<FieldHit>,
    pub commands: Vec<CommandHit>,
    field_pool: Vec<FieldHit>,
    status_scratch: String,
    layout_scratch: Layout,
    mins_scratch: Vec<PaneMinimum>,
}

impl Geometry {
    pub fn pane(&self, id: u64) -> Option<&PaneGeometry> {
        self.panes.iter().find(|p| p.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog<'a> {
    Path {
        title: &'a str,
        value: FieldView<'a>,
    },
    Choices {
        title: &'a str,
        lines: &'a [&'a str],
        kind: ChoiceKind,
    },
    Search {
        query: FieldView<'a>,
        replacement: Option<FieldView<'a>>,
        replacement_focus: bool,
    },
    Help {
        scroll: usize,
    },
}

pub struct PaneScreen<'a> {
    pub id: u64,
    pub document_id: u64,
    pub document: &'a Document,
    pub cursor: usize,
    pub selection: Option<Range<usize>>,
    pub top_line: usize,
    pub left_cell: usize,
    pub read_only: bool,
    pub tabs: &'a [TabCaption<'a>],
    pub highlights: &'a [LineTokens],
}

impl fmt::Debug for PaneScreen<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaneScreen")
            .field("id", &self.id)
            .field("document_id", &self.document_id)
            .field("cursor", &self.cursor)
            .field("selection", &self.selection)
            .field("top_line", &self.top_line)
            .field("left_cell", &self.left_cell)
            .field("read_only", &self.read_only)
            .field("tabs", &self.tabs)
            .field("highlight_lines", &self.highlights.len())
            .finish()
    }
}

pub struct Screen<'a> {
    pub panes: &'a [PaneScreen<'a>],
    pub active_pane: u64,
    pub tree: &'a SplitTree,
    pub title: &'a str,
    pub status: &'a str,
    pub active_path: Option<&'a Path>,
    pub format: FileFormat,
    pub dirty: bool,
    pub dialog: Option<Dialog<'a>>,
    pub explorer: Option<&'a Explorer>,
    pub sidebar_width: u16,
    pub explorer_focused: bool,
    pub theme: &'a Theme,
    pub icons: IconMode,
}

impl fmt::Debug for Screen<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Screen")
            .field("pane_count", &self.panes.len())
            .field("active_pane", &self.active_pane)
            .field("title", &self.title)
            .field("status", &self.status)
            .field("dialog", &self.dialog)
            .field("has_explorer", &self.explorer.is_some())
            .field("sidebar_width", &self.sidebar_width)
            .field("explorer_focused", &self.explorer_focused)
            .field("icons", &self.icons)
            .finish()
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

pub(crate) fn sanitize_ui_cow(input: &str) -> Cow<'_, str> {
    if !input
        .chars()
        .any(|c| c.is_control() || is_bidi_or_invisible_format(c))
    {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\0' => out.push_str("\\0"),
            '\x1b' => out.push_str("\\x1b"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            c if c.is_ascii_control() => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if is_bidi_or_invisible_format(c) || c.is_control() => {
                let _ = write!(out, "\\u{{{:04x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}
#[cfg(test)]
pub(crate) fn sanitize_ui_text(input: &str) -> String {
    sanitize_ui_cow(input).into_owned()
}
fn ellipsis_for_mode(icons: IconMode) -> (&'static str, usize) {
    match icons {
        IconMode::Ascii => ("...", 3),
        IconMode::Unicode | IconMode::Nerd => ("…", 1),
    }
}

fn fit_str_with_ellipsis_mode(s: &str, max_width: usize, icons: IconMode) -> Cow<'_, str> {
    if max_width == 0 {
        return Cow::Borrowed("");
    }
    let full_width = UnicodeWidthStr::width(s);
    if full_width <= max_width {
        return Cow::Borrowed(s);
    }
    let (ellipsis, ellipsis_w) = ellipsis_for_mode(icons);
    if max_width <= ellipsis_w {
        return Cow::Borrowed(if max_width == ellipsis_w {
            ellipsis
        } else {
            &ellipsis[..max_width] // Only the ASCII ellipsis occupies multiple columns.
        });
    }

    let target_width = max_width - ellipsis_w;
    let mut cur_width = 0usize;
    let mut end_byte = 0usize;

    for (byte_idx, grapheme) in s.grapheme_indices(true) {
        let gw = UnicodeWidthStr::width(grapheme);
        if cur_width + gw > target_width {
            break;
        }
        cur_width += gw;
        end_byte = byte_idx + grapheme.len();
    }

    let mut out = String::with_capacity(end_byte + ellipsis.len());
    out.push_str(&s[..end_byte]);
    out.push_str(ellipsis);
    Cow::Owned(out)
}

fn fit_path_tail_with_ellipsis_mode(s: &str, max_width: usize, icons: IconMode) -> Cow<'_, str> {
    if max_width == 0 {
        return Cow::Borrowed("");
    }
    if UnicodeWidthStr::width(s) <= max_width {
        return Cow::Borrowed(s);
    }
    if !s.contains('/') {
        return fit_str_with_ellipsis_mode(s, max_width, icons);
    }
    let (ellipsis, ellipsis_w) = ellipsis_for_mode(icons);
    if max_width <= ellipsis_w {
        return Cow::Borrowed(if max_width == ellipsis_w {
            ellipsis
        } else {
            &ellipsis[..max_width]
        });
    }

    let target_width = max_width - ellipsis_w;
    let mut tail_width = 0usize;
    let mut start_byte = s.len();

    for (byte_idx, grapheme) in s.grapheme_indices(true).rev() {
        let gw = UnicodeWidthStr::width(grapheme);
        if tail_width + gw > target_width {
            break;
        }
        tail_width += gw;
        start_byte = byte_idx;
    }

    let mut out = String::with_capacity(ellipsis.len() + (s.len() - start_byte));
    out.push_str(ellipsis);
    out.push_str(&s[start_byte..]);
    Cow::Owned(out)
}

#[cfg(test)]
pub(crate) fn fit_str_with_ellipsis(s: &str, max_width: usize) -> String {
    fit_str_with_ellipsis_mode(s, max_width, IconMode::Unicode).into_owned()
}

#[cfg(test)]
pub(crate) fn fit_path_tail_with_ellipsis(s: &str, max_width: usize) -> String {
    fit_path_tail_with_ellipsis_mode(s, max_width, IconMode::Unicode).into_owned()
}

pub(crate) fn write_clipped_str(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max_width: usize,
    text: &str,
    style: Style,
) -> u16 {
    if max_width == 0 {
        return 0;
    }
    let mut col = 0usize;
    for grapheme in text.graphemes(true) {
        let gw = UnicodeWidthStr::width(grapheme);
        if gw == 0 {
            continue;
        }
        if col + gw > max_width {
            for k in col..max_width {
                buf[(x + k as u16, y)].set_symbol(" ").set_style(style);
            }
            return max_width as u16;
        }
        let sx = x + col as u16;
        buf[(sx, y)].set_symbol(grapheme).set_style(style);
        for w in 1..gw {
            let nx = sx + w as u16;
            buf[(nx, y)].reset();
            buf[(nx, y)].set_style(style);
        }
        col += gw;
    }
    col as u16
}

fn fill_row(buf: &mut Buffer, rect: Rect, style: Style) {
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

fn wrapped_line_rows(line: &str, max_width: usize) -> usize {
    if max_width == 0 {
        return 1;
    }
    if UnicodeWidthStr::width(line) <= max_width {
        return 1;
    }
    let mut rows = 1usize;
    let mut col = 0usize;
    for word in line.split_whitespace() {
        let ww = UnicodeWidthStr::width(word).max(1);
        if col == 0 {
            if ww <= max_width {
                col = ww;
            } else {
                rows += (ww - 1) / max_width;
                col = ((ww - 1) % max_width) + 1;
            }
        } else if col + 1 + ww <= max_width {
            col += 1 + ww;
        } else {
            rows += 1;
            if ww <= max_width {
                col = ww;
            } else {
                rows += (ww - 1) / max_width;
                col = ((ww - 1) % max_width) + 1;
            }
        }
    }
    rows
}

fn clear_geometry(geometry: &mut Geometry) {
    geometry.panes.clear();
    geometry.sidebar = None;
    geometry.sidebar_parent = None;
    geometry.sidebar_panel = None;
    geometry.explorer_divider = None;
    geometry.splits.clear();
    geometry.collapsed = false;
    geometry.field_pool.append(&mut geometry.fields);
    geometry.commands.clear();
}

fn divider_touches_rect(div: &Divider, rect: Rect) -> bool {
    match div.axis {
        Axis::Columns => {
            (rect.right() == div.area.x || div.area.right() == rect.x)
                && rect.y < div.area.bottom()
                && div.area.y < rect.bottom()
        }
        Axis::Rows => {
            (rect.bottom() == div.area.y || div.area.bottom() == rect.y)
                && rect.x < div.area.right()
                && div.area.x < rect.right()
        }
    }
}

fn draw_split_dividers(
    frame: &mut Frame,
    splits: &[Divider],
    active_pane_area: Option<Rect>,
    model: &Screen<'_>,
) {
    if splits.is_empty() {
        return;
    }
    let theme = model.theme;
    let buf = frame.buffer_mut();
    let col_sym = if model.icons == IconMode::Ascii {
        ":"
    } else {
        "┊"
    };
    let row_sym = if model.icons == IconMode::Ascii {
        "."
    } else {
        "⋯"
    };
    let inactive_style = Style::default().bg(theme.canvas).fg(theme.border);
    let active_style = Style::default().bg(theme.canvas).fg(theme.accent);

    for div in splits {
        if div.area.is_empty() {
            continue;
        }
        let touches_active = !model.explorer_focused
            && active_pane_area.is_some_and(|rect| divider_touches_rect(div, rect));
        let style = if touches_active {
            active_style
        } else {
            inactive_style
        };
        fill_row(
            buf,
            div.area,
            Style::default().bg(theme.canvas).fg(theme.border),
        );
        match div.axis {
            Axis::Columns => {
                let mid_y = div.area.y + div.area.height / 2;
                if mid_y < div.area.bottom() {
                    for x in div.area.left()..div.area.right() {
                        buf[(x, mid_y)].set_symbol(col_sym).set_style(style);
                    }
                }
            }
            Axis::Rows => {
                let mid_x = div.area.x + div.area.width / 2;
                if mid_x < div.area.right() {
                    for y in div.area.top()..div.area.bottom() {
                        buf[(mid_x, y)].set_symbol(row_sym).set_style(style);
                    }
                }
            }
        }
    }
}

pub fn draw(frame: &mut Frame, model: &Screen<'_>, geometry: &mut Geometry) {
    let area = frame.area();
    geometry.sidebar = None;
    geometry.sidebar_parent = None;
    geometry.sidebar_panel = None;
    geometry.explorer_divider = None;
    geometry.splits.clear();
    geometry.collapsed = false;
    geometry.mins_scratch.clear();
    geometry.field_pool.append(&mut geometry.fields);
    geometry.commands.clear();

    let mut active_screen_idx = None;
    let mut active_min_w = chrome::MIN_EDITOR_WIDTH;
    for (idx, pane) in model.panes.iter().enumerate() {
        let lines = pane.document.line_count().max(1);
        let min_w = chrome::pane_min_width(lines);
        geometry.mins_scratch.push(PaneMinimum {
            id: pane.id,
            width: min_w,
            height: MIN_PANE_HEIGHT,
        });
        if pane.id == model.active_pane {
            active_screen_idx = Some(idx);
            active_min_w = min_w;
        }
    }

    let Some(active_idx) = active_screen_idx else {
        clear_geometry(geometry);
        render_safe_exit_notice(frame, area, model, "Terminal too small (Ctrl+Q to exit)");
        if let Some(dialog) = &model.dialog {
            draw_dialog(frame, area, dialog, model.theme, model.icons, geometry);
        }
        return;
    };
    let active_pane = &model.panes[active_idx];

    let (tree_min_w, tree_min_h) = model.tree.minimum(&geometry.mins_scratch);
    let regions = chrome::regions(area, tree_min_h);

    if area.height < 4 || regions.body.height < MIN_PANE_HEIGHT || area.width < active_min_w {
        clear_geometry(geometry);
        render_safe_exit_notice(frame, area, model, "Terminal too small (Ctrl+Q to exit)");
        if let Some(dialog) = &model.dialog {
            draw_dialog(frame, area, dialog, model.theme, model.icons, geometry);
        }
        return;
    }

    let sidebar_areas = chrome::sidebar(
        regions.body,
        model.sidebar_width,
        active_min_w,
        (tree_min_w, tree_min_h),
        model.explorer.is_some() && model.sidebar_width > 0,
    );

    model.tree.layout(
        sidebar_areas.editor,
        model.active_pane,
        &geometry.mins_scratch,
        &mut geometry.layout_scratch,
    );

    if geometry.layout_scratch.panes.is_empty() {
        clear_geometry(geometry);
        render_safe_exit_notice(frame, area, model, "Terminal too small (Ctrl+Q to exit)");
        if let Some(dialog) = &model.dialog {
            draw_dialog(frame, area, dialog, model.theme, model.icons, geometry);
        }
        return;
    }

    draw_header(frame, regions.header, regions.framed, model);

    if let (Some(panel), Some(parent), Some(rows), Some(div), Some(explorer)) = (
        sidebar_areas.panel,
        sidebar_areas.parent,
        sidebar_areas.rows,
        sidebar_areas.divider,
        model.explorer,
    ) {
        draw_sidebar(frame, panel, parent, rows, div, explorer, model);
        geometry.sidebar_panel = Some(panel);
        geometry.sidebar_parent = if explorer.parent_path().is_some() {
            Some(parent)
        } else {
            None
        };
        geometry.sidebar = Some(rows);
        geometry.explorer_divider = Some(div);
    }

    let mut written_panes = 0usize;
    let mut active_text_area = None;
    let mut active_pane_area = None;
    for i in 0..geometry.layout_scratch.panes.len() {
        let pane_rect = geometry.layout_scratch.panes[i];
        let Some(pane) = model.panes.iter().find(|p| p.id == pane_rect.id) else {
            continue;
        };
        if written_panes >= geometry.panes.len() {
            geometry.panes.push(PaneGeometry::default());
        }
        let pg = &mut geometry.panes[written_panes];
        pg.id = pane_rect.id;
        pg.area = pane_rect.area;
        pg.tabs.clear();
        pg.previous_tab = None;
        pg.next_tab = None;

        let pr = pane_rect.area;
        let is_active_pane = pane.id == model.active_pane;
        let pane_focused = is_active_pane && !model.explorer_focused;
        let total_lines = pane.document.line_count().max(1);
        let areas = chrome::pane_areas(pr, total_lines);
        pg.text_area = areas.text;

        draw_pane_card(frame, pr, pane_focused, model.theme, model.icons);

        let (prev_tab, next_tab) = draw_tab_bar(
            frame,
            areas.tabs,
            pane.tabs,
            pane_focused,
            model.theme,
            model.icons,
            &mut pg.tabs,
        );
        pg.previous_tab = prev_tab;
        pg.next_tab = next_tab;

        let line_digits = chrome::line_digits(total_lines);
        draw_gutter_and_body(
            frame,
            areas.gutter,
            areas.text,
            pane,
            is_active_pane,
            model.theme,
            line_digits,
            total_lines,
        );

        if is_active_pane {
            active_text_area = Some(areas.text);
            active_pane_area = Some(pr);
        }
        written_panes += 1;
    }
    geometry.panes.truncate(written_panes);
    geometry.collapsed = geometry.layout_scratch.collapsed;
    geometry
        .splits
        .extend_from_slice(&geometry.layout_scratch.dividers);

    draw_split_dividers(frame, &geometry.splits, active_pane_area, model);
    draw_dock(
        frame,
        regions.dock,
        regions.status,
        regions.helper,
        regions.framed,
        model,
        active_pane,
        sidebar_areas.rows.is_some(),
        &mut geometry.status_scratch,
        &mut geometry.commands,
    );

    if let Some(dialog) = &model.dialog {
        draw_dialog(frame, area, dialog, model.theme, model.icons, geometry);
    } else if (!model.explorer_focused || sidebar_areas.rows.is_none())
        && let Some(text_area) = active_text_area
    {
        position_editor_cursor(frame, text_area, active_pane);
    }
}

fn render_safe_exit_notice(frame: &mut Frame, area: Rect, model: &Screen<'_>, notice: &str) {
    if area.is_empty() {
        return;
    }
    let theme = model.theme;
    let buf = frame.buffer_mut();
    fill_row(buf, area, Style::default().bg(theme.canvas).fg(theme.fg));

    let mid_y = area.y + area.height / 2;
    let fitted = fit_str_with_ellipsis_mode(notice, area.width as usize, model.icons);
    let fitted_width = UnicodeWidthStr::width(fitted.as_ref()) as u16;
    let start_x = area.x + (area.width.saturating_sub(fitted_width)) / 2;

    write_clipped_str(
        buf,
        start_x,
        mid_y,
        area.right().saturating_sub(start_x) as usize,
        fitted.as_ref(),
        Style::default()
            .bg(theme.canvas)
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD),
    );
}

fn icon_title<'a>(icon: &'a str, label: &'a str, style: Style) -> Line<'a> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(icon, style.add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(label, style),
        Span::raw(" "),
    ])
}

fn draw_header(frame: &mut Frame, area: Rect, framed: bool, model: &Screen<'_>) {
    if area.is_empty() {
        return;
    }
    let theme = model.theme;
    let ws_icon = theme::chrome_icon(theme::ChromeIcon::Workspace, model.icons);

    if framed {
        let border_set = if model.icons == IconMode::Ascii {
            ASCII_BORDER
        } else {
            border::ROUNDED
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border_set)
            .border_style(Style::default().bg(theme.canvas).fg(theme.border))
            .style(Style::default().bg(theme.canvas).fg(theme.fg))
            .title(icon_title(
                ws_icon,
                "Notefart",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(block, area);
        let buf = frame.buffer_mut();

        let inner_rect = chrome::inner(area);
        if !inner_rect.is_empty() {
            let clean_title = sanitize_ui_cow(model.title);
            let title_display = if clean_title.is_empty() {
                "Workspace"
            } else {
                clean_title.as_ref()
            };
            let max_w = inner_rect.width.saturating_sub(2) as usize;
            let fitted = fit_path_tail_with_ellipsis_mode(title_display, max_w, model.icons);
            write_clipped_str(
                buf,
                inner_rect.x + 1,
                inner_rect.y,
                max_w,
                fitted.as_ref(),
                Style::default().bg(theme.canvas).fg(theme.fg),
            );
        }
    } else {
        let buf = frame.buffer_mut();
        let base_style = Style::default().bg(theme.canvas).fg(theme.fg);

        let clean_title = sanitize_ui_cow(model.title);
        let title_display = if clean_title.is_empty() {
            "Workspace"
        } else {
            clean_title.as_ref()
        };

        const BRAND: &str = "Notefart";
        let show_brand = area.width >= 36;
        let brand_reserve = if show_brand { BRAND.len() + 3 } else { 1 };

        let icon_w = UnicodeWidthStr::width(ws_icon) + 1;
        let x_start = area.x + 1;
        let max_title_width = (area.width as usize).saturating_sub(1 + brand_reserve + icon_w);
        let fitted_title =
            fit_path_tail_with_ellipsis_mode(title_display, max_title_width, model.icons);

        let icon_style = Style::default()
            .bg(theme.canvas)
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD);
        let mut written_icon =
            write_clipped_str(buf, x_start, area.y, icon_w - 1, ws_icon, icon_style);
        written_icon += write_clipped_str(buf, x_start + written_icon, area.y, 1, " ", icon_style);
        let written = write_clipped_str(
            buf,
            x_start + written_icon,
            area.y,
            max_title_width,
            fitted_title.as_ref(),
            base_style.add_modifier(Modifier::BOLD),
        );

        if show_brand {
            let brand_len = BRAND.len() as u16;
            let brand_x = area.right().saturating_sub(brand_len + 1);
            if brand_x > x_start + written_icon + written + 2 {
                write_clipped_str(
                    buf,
                    brand_x,
                    area.y,
                    brand_len as usize,
                    BRAND,
                    Style::default().bg(theme.canvas).fg(theme.muted),
                );
            }
        }
    }
}

fn draw_sidebar(
    frame: &mut Frame,
    panel_rect: Rect,
    parent_rect: Rect,
    rows_rect: Rect,
    divider_rect: Rect,
    explorer: &Explorer,
    model: &Screen<'_>,
) {
    let theme = model.theme;
    let base_style = Style::default().bg(theme.canvas).fg(theme.fg);

    // 1. Framed card around explorer
    let border_set = if model.icons == IconMode::Ascii {
        ASCII_BORDER
    } else {
        border::ROUNDED
    };
    let exp_icon = theme::chrome_icon(theme::ChromeIcon::Explorer, model.icons);
    let (border_style, title_style, title_text) = if model.explorer_focused {
        (
            Style::default()
                .bg(theme.canvas)
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
            if model.icons == IconMode::Ascii {
                "Explorer *"
            } else {
                "Explorer ●"
            },
        )
    } else {
        (
            Style::default().bg(theme.canvas).fg(theme.border),
            Style::default().fg(theme.muted),
            "Explorer",
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border_set)
        .border_style(border_style)
        .style(Style::default().bg(theme.canvas).fg(theme.fg))
        .title(icon_title(exp_icon, title_text, title_style));
    frame.render_widget(block, panel_rect);
    let buf = frame.buffer_mut();

    // 2. Divider grab column (breathing space with subtle centered drag grip)
    let grip_sym = if model.icons == IconMode::Ascii {
        ":"
    } else {
        "┊"
    };
    let grip_style = if model.explorer_focused {
        Style::default().bg(theme.canvas).fg(theme.accent)
    } else {
        Style::default().bg(theme.canvas).fg(theme.border)
    };
    let mid_y = divider_rect.y + divider_rect.height / 2;
    for y in divider_rect.top()..divider_rect.bottom() {
        let sym = if y == mid_y { grip_sym } else { " " };
        buf[(divider_rect.x, y)]
            .set_symbol(sym)
            .set_style(grip_style);
    }

    let slot_w = theme::icon_slot(model.icons).max(1);

    // 3. Parent navigation row (top inner row of sidebar)
    if !parent_rect.is_empty() {
        fill_row(buf, parent_rect, base_style);
        let has_parent = explorer.parent_path().is_some();
        let right = parent_rect.right();
        let mut cur_x = parent_rect.x + 1;
        let dir_kind = IconKind::Directory { expanded: false };

        if has_parent {
            let icon_str = theme::icon(Path::new(".."), dir_kind, model.icons);
            let icon_style = Style::default()
                .bg(theme.canvas)
                .fg(theme::icon_color(Path::new(".."), dir_kind, theme))
                .add_modifier(Modifier::BOLD);
            if cur_x + slot_w < right {
                write_clipped_str(
                    buf,
                    cur_x,
                    parent_rect.y,
                    slot_w as usize,
                    icon_str,
                    icon_style,
                );
                cur_x += slot_w + 1;
            }
            let label_style = base_style.add_modifier(Modifier::BOLD);
            let avail = right.saturating_sub(cur_x) as usize;
            if avail > 0 {
                write_clipped_str(buf, cur_x, parent_rect.y, avail, "..", label_style);
            }
        } else {
            let icon_str = theme::icon(explorer.root(), dir_kind, model.icons);
            let disabled_style = Style::default().bg(theme.canvas).fg(theme.muted);
            let icon_style = disabled_style.add_modifier(Modifier::BOLD);
            if cur_x + slot_w < right {
                write_clipped_str(
                    buf,
                    cur_x,
                    parent_rect.y,
                    slot_w as usize,
                    icon_str,
                    icon_style,
                );
                cur_x += slot_w + 1;
            }
            let avail = right.saturating_sub(cur_x) as usize;
            if avail > 0 {
                let root_name = explorer
                    .root()
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_else(|| Cow::Borrowed("/"));
                let clean = sanitize_ui_cow(root_name.as_ref());
                let fitted = fit_str_with_ellipsis_mode(clean.as_ref(), avail, model.icons);
                write_clipped_str(
                    buf,
                    cur_x,
                    parent_rect.y,
                    avail,
                    fitted.as_ref(),
                    disabled_style,
                );
            }
        }
    }

    // 4. Sidebar tree rows inside rows_rect (below parent row)
    fill_row(buf, rows_rect, base_style);
    let rows = explorer.rows();
    if rows.is_empty() {
        if !rows_rect.is_empty() && rows_rect.width >= 3 {
            let msg = fit_str_with_ellipsis_mode(
                "(empty)",
                rows_rect.width.saturating_sub(1) as usize,
                model.icons,
            );
            write_clipped_str(
                buf,
                rows_rect.x + 1,
                rows_rect.y,
                rows_rect.width.saturating_sub(1) as usize,
                msg.as_ref(),
                Style::default().bg(theme.canvas).fg(theme.muted),
            );
        }
        return;
    }

    let top = explorer.top();
    let selected = explorer.selected();
    let right = rows_rect.right();

    for row_offset in 0..rows_rect.height {
        let idx = top.saturating_add(row_offset as usize);
        if idx >= rows.len() {
            break;
        }
        let entry = &rows[idx];
        let y = rows_rect.y + row_offset;
        let is_selected = idx == selected;

        let row_style = if is_selected {
            let s = theme.selection_style();
            if model.explorer_focused {
                s.add_modifier(Modifier::BOLD)
            } else {
                s
            }
        } else {
            base_style
        };

        if is_selected {
            for x in rows_rect.left()..right {
                buf[(x, y)].set_symbol(" ").set_style(row_style);
            }
        }

        let mut cur_x = rows_rect.x;
        if cur_x >= right {
            continue;
        }

        // Focus / selection marker column
        let focus_marker = if is_selected && model.explorer_focused {
            ">"
        } else {
            " "
        };
        let marker_style = if is_selected && model.explorer_focused {
            if theme.selection == theme.canvas {
                row_style
            } else {
                row_style.fg(theme.accent)
            }
        } else {
            row_style
        };
        buf[(cur_x, y)]
            .set_symbol(focus_marker)
            .set_style(marker_style);
        cur_x += 1;

        // Depth indent capped so entry name stays legible on narrow sidebars
        let max_indent = (rows_rect.width as usize).saturating_sub(8);
        let indent = entry.depth.saturating_mul(2).min(max_indent) as u16;
        cur_x = (cur_x + indent).min(right);

        // Chevron slot (2 cols: chevron + space)
        if cur_x + 2 <= right {
            if entry.kind == EntryKind::Directory {
                let chev = theme::chevron(entry.expanded, model.icons);
                let chev_style = if is_selected {
                    row_style
                } else {
                    Style::default().bg(theme.canvas).fg(theme.muted)
                };
                write_clipped_str(buf, cur_x, y, 1, chev, chev_style);
            }
            cur_x += 2;
        }

        // Icon slot (fixed slot_w + 1 space separator)
        let icon_kind = match entry.kind {
            EntryKind::Directory => IconKind::Directory {
                expanded: entry.expanded,
            },
            EntryKind::File => IconKind::File,
            EntryKind::Symlink => IconKind::Symlink,
            EntryKind::Other => IconKind::Other,
        };
        if cur_x + slot_w < right {
            let icon_str = theme::icon(&entry.path, icon_kind, model.icons);
            let icon_style = if is_selected {
                row_style.add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .bg(theme.canvas)
                    .fg(theme::icon_color(&entry.path, icon_kind, model.theme))
                    .add_modifier(Modifier::BOLD)
            };
            write_clipped_str(buf, cur_x, y, slot_w as usize, icon_str, icon_style);
            cur_x += slot_w + 1;
        }

        // Entry label + optional issue indicator
        let avail = right.saturating_sub(cur_x) as usize;
        if avail > 0 {
            let issue_reserve = if entry.issue.is_some() && avail >= 4 {
                2
            } else {
                0
            };
            let name_budget = avail - issue_reserve;
            let raw_name = entry.name.to_string_lossy();
            let clean_name = sanitize_ui_cow(raw_name.as_ref());
            let fitted_name =
                fit_str_with_ellipsis_mode(clean_name.as_ref(), name_budget, model.icons);
            let name_style = if entry.kind == EntryKind::Directory && !is_selected {
                row_style.add_modifier(Modifier::BOLD)
            } else {
                row_style
            };
            let written =
                write_clipped_str(buf, cur_x, y, name_budget, fitted_name.as_ref(), name_style);
            if issue_reserve > 0 {
                let ix = cur_x + written;
                if ix + 2 <= right {
                    let issue_style = if is_selected {
                        row_style.fg(theme.warning).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .bg(theme.canvas)
                            .add_modifier(Modifier::BOLD)
                    };
                    write_clipped_str(buf, ix, y, 2, " !", issue_style);
                }
            }
        }
    }
}

fn draw_pane_card(
    frame: &mut Frame,
    area: Rect,
    pane_focused: bool,
    theme: &Theme,
    icons: IconMode,
) {
    if area.is_empty() {
        return;
    }
    let border_set = if icons == IconMode::Ascii {
        ASCII_BORDER
    } else {
        border::ROUNDED
    };
    let editor_icon = theme::chrome_icon(theme::ChromeIcon::Editor, icons);
    let (border_style, title_style, title_text) = if pane_focused {
        (
            Style::default()
                .bg(theme.canvas)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
            if icons == IconMode::Ascii {
                "Editor *"
            } else {
                "Editor ●"
            },
        )
    } else {
        (
            Style::default().bg(theme.canvas).fg(theme.border),
            Style::default().fg(theme.muted),
            "Editor",
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border_set)
        .border_style(border_style)
        .style(Style::default().bg(theme.canvas).fg(theme.fg))
        .title(icon_title(editor_icon, title_text, title_style));
    frame.render_widget(block, area);
}

fn draw_dock(
    frame: &mut Frame,
    dock_rect: Rect,
    status_rect: Rect,
    helper_rect: Option<Rect>,
    framed: bool,
    model: &Screen<'_>,
    active: &PaneScreen<'_>,
    has_sidebar: bool,
    status_scratch: &mut String,
    commands: &mut Vec<CommandHit>,
) {
    if dock_rect.is_empty() {
        return;
    }
    let theme = model.theme;
    if framed {
        let border_set = if model.icons == IconMode::Ascii {
            ASCII_BORDER
        } else {
            border::ROUNDED
        };
        let status_icon = theme::chrome_icon(theme::ChromeIcon::Status, model.icons);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border_set)
            .border_style(Style::default().bg(theme.canvas).fg(theme.border))
            .style(Style::default().bg(theme.canvas).fg(theme.fg))
            .title(icon_title(
                status_icon,
                "Status",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(block, status_rect);
    }

    let status_inner = if framed {
        chrome::inner(status_rect)
    } else {
        status_rect
    };
    draw_status(frame, status_inner, model, active, status_scratch);

    if let Some(helper) = helper_rect {
        draw_helper(frame, helper, model, has_sidebar, commands);
    }
}

fn desired_tab_width(tab: &TabCaption<'_>, icons: IconMode, close_w: u16) -> u16 {
    let clean = sanitize_ui_cow(tab.label);
    let label_str = if clean.is_empty() {
        "Untitled"
    } else {
        clean.as_ref()
    };
    let label_w = UnicodeWidthStr::width(label_str) as u16;
    let dirty_w: u16 = if tab.dirty { 2 } else { 0 };
    let ro_w: u16 = if tab.read_only { 5 } else { 0 };
    let icon_w =
        UnicodeWidthStr::width(theme::icon(Path::new(label_str), IconKind::File, icons)) as u16;
    let overhead = 1 + icon_w + 1 + dirty_w + ro_w + 1 + close_w;
    let natural = overhead.saturating_add(label_w);
    natural.clamp(14, 28).max(overhead.saturating_add(2))
}

#[allow(clippy::too_many_arguments)]
fn render_one_tab(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    tab: &TabCaption<'_>,
    pane_focused: bool,
    close_sym: &str,
    close_w: u16,
    theme: &Theme,
    icons: IconMode,
) -> TabHit {
    let close_w = close_w.min(width.saturating_sub(2)).max(1);
    let close_x = x + width - close_w;
    let body_rect = Rect {
        x,
        y,
        width: close_x - x,
        height: 1,
    };
    let close_rect = Rect {
        x: close_x,
        y,
        width: close_w,
        height: 1,
    };

    let reverse_cues = theme.uses_reverse_cues();
    let active_mod = if pane_focused {
        if reverse_cues {
            Modifier::BOLD | Modifier::REVERSED
        } else {
            Modifier::BOLD
        }
    } else if reverse_cues {
        Modifier::REVERSED
    } else {
        Modifier::empty()
    };

    let base_style = if tab.active {
        Style::default()
            .bg(theme.canvas)
            .fg(theme.fg)
            .add_modifier(active_mod)
    } else {
        Style::default().bg(theme.canvas).fg(theme.muted)
    };
    let dirty_style = if tab.active {
        Style::default()
            .bg(theme.canvas)
            .fg(theme.warning)
            .add_modifier(active_mod)
    } else {
        Style::default()
            .bg(theme.canvas)
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD)
    };
    let ro_style = if tab.active {
        let mut s = Style::default().bg(theme.canvas).fg(theme.warning);
        if reverse_cues {
            s = s.add_modifier(Modifier::REVERSED);
        }
        s
    } else {
        Style::default().bg(theme.canvas).fg(theme.warning)
    };
    let mut close_style = if tab.active {
        let mut s = Style::default().bg(theme.canvas).fg(theme.fg);
        if reverse_cues {
            s = s.add_modifier(Modifier::REVERSED);
        }
        s
    } else {
        Style::default().bg(theme.canvas).fg(theme.muted)
    };
    close_style = close_style.add_modifier(Modifier::BOLD);

    for cx in x..x + width {
        buf[(cx, y)].set_symbol(" ").set_style(base_style);
    }

    let prefix = if tab.active {
        if pane_focused {
            ">"
        } else if icons == IconMode::Ascii {
            "-"
        } else {
            "·"
        }
    } else {
        " "
    };
    buf[(x, y)].set_symbol(prefix).set_style(base_style);

    let inner_w = body_rect.width.saturating_sub(1) as usize;
    let content_w = if inner_w >= 3 {
        inner_w - 1 // preserve 1-col gap before close icon
    } else {
        inner_w
    };

    if content_w > 0 {
        let dirty_text = if tab.dirty {
            if content_w >= 3 { " *" } else { "*" }
        } else {
            ""
        };
        let dirty_w = dirty_text.len();

        let ro_text = if tab.read_only && content_w >= dirty_w + 6 {
            " [RO]"
        } else {
            ""
        };
        let ro_w = ro_text.len();

        let clean = sanitize_ui_cow(tab.label);
        let label_str = if clean.is_empty() {
            "Untitled"
        } else {
            clean.as_ref()
        };

        let tab_path = Path::new(label_str);
        let file_icon = theme::icon(tab_path, IconKind::File, icons);
        let icon_w = UnicodeWidthStr::width(file_icon);
        let show_icon = content_w >= icon_w + dirty_w + ro_w + 3;

        let icon_style = if tab.active && !reverse_cues {
            Style::default()
                .bg(theme.canvas)
                .fg(theme::icon_color(tab_path, IconKind::File, theme))
                .add_modifier(active_mod | Modifier::BOLD)
        } else {
            base_style.add_modifier(Modifier::BOLD)
        };

        let icon_reserve = if show_icon { icon_w + 1 } else { 0 };
        let label_budget = content_w.saturating_sub(dirty_w + ro_w + icon_reserve);

        let mut cursor_x = x + 1;
        if show_icon {
            let written = write_clipped_str(buf, cursor_x, y, icon_w, file_icon, icon_style);
            cursor_x += written + 1;
        }
        if label_budget > 0 {
            let fitted = fit_str_with_ellipsis_mode(label_str, label_budget, icons);
            let written =
                write_clipped_str(buf, cursor_x, y, label_budget, fitted.as_ref(), base_style);
            cursor_x += written;
        }
        if !dirty_text.is_empty() {
            let written = write_clipped_str(buf, cursor_x, y, dirty_w, dirty_text, dirty_style);
            cursor_x += written;
        }
        if !ro_text.is_empty() {
            write_clipped_str(buf, cursor_x, y, ro_w, ro_text, ro_style);
        }
    }

    write_clipped_str(buf, close_x, y, close_w as usize, close_sym, close_style);

    TabHit {
        id: tab.id,
        body: body_rect,
        close: close_rect,
    }
}

fn draw_tab_bar(
    frame: &mut Frame,
    area: Rect,
    tabs: &[TabCaption<'_>],
    pane_focused: bool,
    theme: &Theme,
    icons: IconMode,
    hits_out: &mut Vec<TabHit>,
) -> (Option<Rect>, Option<Rect>) {
    if area.is_empty() {
        return (None, None);
    }
    let buf = frame.buffer_mut();
    let bar_style = Style::default().bg(theme.canvas).fg(theme.muted);
    fill_row(buf, area, bar_style);

    let avail_tabs_width = area.width;

    if tabs.is_empty() || avail_tabs_width < 4 {
        return (None, None);
    }

    let close_sym = theme::close_icon(icons);
    let close_w = UnicodeWidthStr::width(close_sym).max(1) as u16;
    let sep_sym = if icons == IconMode::Ascii { "|" } else { "│" };
    let sep_style = Style::default().bg(theme.canvas).fg(theme.border);

    if tabs.len() == 1 {
        let w = desired_tab_width(&tabs[0], icons, close_w).min(avail_tabs_width);
        hits_out.push(render_one_tab(
            buf,
            area.x,
            area.y,
            w,
            &tabs[0],
            pane_focused,
            close_sym,
            close_w,
            theme,
            icons,
        ));
        return (None, None);
    }

    let mut total_all = (tabs.len().saturating_sub(1)) as u16;
    for tab in tabs {
        total_all = total_all.saturating_add(desired_tab_width(tab, icons, close_w));
    }

    if total_all <= avail_tabs_width {
        let mut cur_x = area.x;
        for (i, tab) in tabs.iter().enumerate() {
            let w = desired_tab_width(tab, icons, close_w);
            hits_out.push(render_one_tab(
                buf,
                cur_x,
                area.y,
                w,
                tab,
                pane_focused,
                close_sym,
                close_w,
                theme,
                icons,
            ));
            cur_x += w;
            if i + 1 < tabs.len() {
                buf[(cur_x, area.y)]
                    .set_symbol(sep_sym)
                    .set_style(sep_style);
                cur_x += 1;
            }
        }
        return (None, None);
    }

    // Overflow mode: active tab always visible; previous/next buttons clickable.
    let active_idx = tabs.iter().position(|t| t.active).unwrap_or(0);
    let (prev_str, next_str, btn_w, gap) = if avail_tabs_width >= 20 {
        match icons {
            IconMode::Ascii => ("[<]", "[>]", 3u16, 1u16),
            IconMode::Unicode | IconMode::Nerd => ("[‹]", "[›]", 3u16, 1u16),
        }
    } else if avail_tabs_width >= 10 {
        match icons {
            IconMode::Ascii => ("<", ">", 1u16, 1u16),
            IconMode::Unicode | IconMode::Nerd => ("‹", "›", 1u16, 1u16),
        }
    } else {
        ("", "", 0u16, 0u16)
    };

    let (prev_rect, next_rect, region_x, region_w) = if btn_w > 0 {
        let p = Rect {
            x: area.x,
            y: area.y,
            width: btn_w,
            height: 1,
        };
        let n = Rect {
            x: area.x + avail_tabs_width - btn_w,
            y: area.y,
            width: btn_w,
            height: 1,
        };
        let rx = area.x + btn_w + gap;
        let rw = avail_tabs_width.saturating_sub(2 * (btn_w + gap));
        (Some(p), Some(n), rx, rw)
    } else {
        (None, None, area.x, avail_tabs_width)
    };

    let mut start = active_idx;
    let mut end = active_idx;
    let active_w = desired_tab_width(&tabs[active_idx], icons, close_w).min(region_w);
    let mut used = active_w;

    loop {
        let mut added = false;
        if end + 1 < tabs.len() {
            let need = 1 + desired_tab_width(&tabs[end + 1], icons, close_w);
            if used.saturating_add(need) <= region_w {
                end += 1;
                used += need;
                added = true;
            }
        }
        if start > 0 {
            let need = 1 + desired_tab_width(&tabs[start - 1], icons, close_w);
            if used.saturating_add(need) <= region_w {
                start -= 1;
                used += need;
                added = true;
            }
        }
        if !added {
            break;
        }
    }

    if let Some(p) = prev_rect {
        let style = if start > 0 {
            Style::default()
                .bg(theme.canvas)
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(theme.canvas).fg(theme.muted)
        };
        write_clipped_str(buf, p.x, p.y, p.width as usize, prev_str, style);
    }
    if let Some(n) = next_rect {
        let style = if end + 1 < tabs.len() {
            Style::default()
                .bg(theme.canvas)
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(theme.canvas).fg(theme.muted)
        };
        write_clipped_str(buf, n.x, n.y, n.width as usize, next_str, style);
    }

    if region_w >= 4 {
        let mut cur_x = region_x;
        for (i, tab) in tabs.iter().enumerate().take(end + 1).skip(start) {
            let w = if i == active_idx {
                active_w
            } else {
                desired_tab_width(tab, icons, close_w)
            };
            hits_out.push(render_one_tab(
                buf,
                cur_x,
                area.y,
                w,
                tab,
                pane_focused,
                close_sym,
                close_w,
                theme,
                icons,
            ));
            cur_x += w;
            if i < end {
                buf[(cur_x, area.y)]
                    .set_symbol(sep_sym)
                    .set_style(sep_style);
                cur_x += 1;
            }
        }
    }

    (prev_rect, next_rect)
}

fn is_error_status(s: &str) -> bool {
    s.starts_with("Error")
        || s.starts_with("Err:")
        || s.starts_with('!')
        || s.starts_with("Save failed")
        || s.starts_with("Open failed")
        || s.starts_with("Reload failed")
        || s.starts_with("Copy failed")
        || s.starts_with("Cut failed")
        || s.starts_with("Paste failed")
        || s.starts_with("Path input rejected")
        || s.starts_with("Replace failed")
        || s.starts_with("Search failed")
}

fn is_warning_status(s: &str) -> bool {
    s.starts_with("Warn")
        || s.starts_with("Read-only")
        || s.starts_with("Highlight")
        || s.starts_with("Resize")
}

fn is_success_status(s: &str) -> bool {
    s == "Saved" || s.starts_with("Saved ") || s.starts_with("Opened") || s.starts_with("Replaced")
}

fn draw_status(
    frame: &mut Frame,
    area: Rect,
    model: &Screen<'_>,
    active: &PaneScreen<'_>,
    scratch: &mut String,
) {
    if area.is_empty() {
        return;
    }
    let theme = model.theme;
    let buf = frame.buffer_mut();
    let base = Style::default().bg(theme.canvas).fg(theme.fg);
    fill_row(buf, area, base);
    let clean_status = sanitize_ui_cow(model.status);
    let error = is_error_status(clean_status.as_ref());
    let warning = is_warning_status(clean_status.as_ref());
    let critical = error || warning;
    let operation_style = if error {
        base.fg(theme.error).add_modifier(Modifier::BOLD)
    } else if warning {
        base.fg(theme.warning).add_modifier(Modifier::BOLD)
    } else if is_success_status(clean_status.as_ref()) {
        base.fg(theme.success)
    } else {
        base
    };
    let width = area.width.saturating_sub(2) as usize;
    let message_width = UnicodeWidthStr::width(clean_status.as_ref()) + if error { 2 } else { 0 };
    // Errors and warnings claim the row ahead of the optional cursor/format
    // metadata. The smallest compact meta is "1:1" plus its gap, so when even
    // that cannot sit beside a critical message, skip the metadata entirely.
    let meta_possible = width >= 9 && (!critical || message_width + 5 <= width);
    if meta_possible {
        let cursor = active.cursor.min(active.document.len_chars());
        let line = active.document.line_of(cursor) + 1;
        let col = active.document.column_of(cursor) + 1;
        let newline = match model.format.newline {
            Newline::Lf => "LF",
            Newline::Crlf => "CRLF",
            Newline::Mixed => "MIXED",
        };
        scratch.clear();
        let _ = write!(
            scratch,
            "{}{}Ln {line}, Col {col}  {newline} UTF-8{}",
            if active.read_only { "RO " } else { "" },
            if model.dirty { "* " } else { "" },
            if model.format.bom { "+BOM" } else { "" },
        );
        if scratch.len() + 10 > width {
            scratch.clear();
            let _ = write!(
                scratch,
                "{}{}{line}:{col}",
                if active.read_only { "RO " } else { "" },
                if model.dirty { "* " } else { "" }
            );
        }
    } else {
        scratch.clear();
    }
    let show_meta = meta_possible
        && scratch.len() + 6 <= width
        && (!critical || message_width <= width.saturating_sub(scratch.len() + 2));
    let left_width = if show_meta {
        width.saturating_sub(scratch.len() + 2)
    } else {
        width
    };
    let mut x = area.x + 1;
    let mut remaining = left_width;
    if error && remaining >= 2 {
        x += write_clipped_str(buf, x, area.y, 2, "! ", operation_style);
        remaining -= 2;
    }
    let used = write_middle_elided(
        buf,
        x,
        area.y,
        remaining,
        clean_status.as_ref(),
        operation_style,
        model.icons,
    );
    x += used;
    remaining = remaining.saturating_sub(used as usize);
    if !error && !warning && remaining >= 6 {
        if let Some(path) = model.active_path {
            x += write_clipped_str(buf, x, area.y, 3, " | ", base.fg(theme.muted));
            remaining = remaining.saturating_sub(3);
            let path = path.to_string_lossy();
            let clean_path = sanitize_ui_cow(path.as_ref());
            write_elided(
                buf,
                x,
                area.y,
                remaining,
                clean_path.as_ref(),
                base.fg(theme.muted),
                model.icons,
            );
        }
    }
    if show_meta {
        write_clipped_str(
            buf,
            area.right() - 1 - scratch.len() as u16,
            area.y,
            scratch.len(),
            scratch,
            base.fg(theme.muted),
        );
    }
}

struct HelperAction {
    keycap: &'static str,
    label: &'static str,
    key: KeyEvent,
}

const fn helper_action(
    keycap: &'static str,
    label: &'static str,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> HelperAction {
    HelperAction {
        keycap,
        label,
        key: KeyEvent::new(code, modifiers),
    }
}

const EXPLORER_HELPER_ACTIONS: &[HelperAction] = &[
    helper_action("[F1]", "Help", KeyCode::F(1), KeyModifiers::NONE),
    helper_action("[Enter]", "Open", KeyCode::Enter, KeyModifiers::NONE),
    helper_action("[Bksp]", "Up", KeyCode::Backspace, KeyModifiers::NONE),
    helper_action("[F2]", "Editor", KeyCode::F(2), KeyModifiers::NONE),
    helper_action("[F5]", "Refresh", KeyCode::F(5), KeyModifiers::NONE),
    helper_action(
        "[Ctrl+O]",
        "Path",
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    ),
    helper_action(
        "[Ctrl+Q]",
        "Quit",
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
    ),
];

const EDITOR_HELPER_ACTIONS: &[HelperAction] = &[
    helper_action("[F1]", "Help", KeyCode::F(1), KeyModifiers::NONE),
    helper_action(
        "[Ctrl+O]",
        "Open",
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    ),
    helper_action(
        "[Ctrl+S]",
        "Save",
        KeyCode::Char('s'),
        KeyModifiers::CONTROL,
    ),
    helper_action(
        "[Ctrl+F]",
        "Find",
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    ),
    helper_action(
        "[Ctrl+R]",
        "Replace",
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    ),
    helper_action(
        "[Ctrl+Q]",
        "Quit",
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
    ),
    helper_action("[F2]", "Files", KeyCode::F(2), KeyModifiers::NONE),
    helper_action("[F10]", "Pane", KeyCode::F(10), KeyModifiers::NONE),
];

fn draw_helper(
    frame: &mut Frame,
    area: Rect,
    model: &Screen<'_>,
    has_sidebar: bool,
    commands: &mut Vec<CommandHit>,
) {
    if area.is_empty() {
        return;
    }
    let theme = model.theme;
    let buf = frame.buffer_mut();
    fill_row(buf, area, Style::default().bg(theme.canvas).fg(theme.muted));
    if model.dialog.is_some() {
        if matches!(
            model.dialog,
            Some(Dialog::Path { .. } | Dialog::Search { .. })
        ) {
            write_clipped_str(
                buf,
                area.x + 1,
                area.y,
                area.width.saturating_sub(2) as usize,
                "Shift+Arrows Select  ^A All  ^C/X/V Clipboard",
                Style::default().bg(theme.canvas).fg(theme.muted),
            );
        }
        return;
    }

    let actions = if model.explorer_focused && has_sidebar {
        EXPLORER_HELPER_ACTIONS
    } else {
        EDITOR_HELPER_ACTIONS
    };

    let sep = if model.icons == IconMode::Ascii {
        " | "
    } else {
        " · "
    };
    let sep_w = 3u16;

    let key_style = Style::default()
        .bg(theme.canvas)
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().bg(theme.canvas).fg(theme.muted);
    let sep_style = Style::default().bg(theme.canvas).fg(theme.border);

    let max_right = area.right().saturating_sub(1);
    let mut cur_x = area.x + 1;
    let mut first = true;

    for action in actions {
        let key_w = UnicodeWidthStr::width(action.keycap) as u16;
        let label_w = UnicodeWidthStr::width(action.label) as u16;
        let group_w = key_w + 1 + label_w;

        let needed = if first { group_w } else { sep_w + group_w };
        if needed > max_right.saturating_sub(cur_x) {
            break;
        }

        if !first {
            write_clipped_str(buf, cur_x, area.y, sep_w as usize, sep, sep_style);
            cur_x += sep_w;
        }

        let chip_x = cur_x;
        write_clipped_str(buf, cur_x, area.y, key_w as usize, action.keycap, key_style);
        cur_x += key_w;
        buf[(cur_x, area.y)].set_symbol(" ").set_style(label_style);
        cur_x += 1;
        write_clipped_str(
            buf,
            cur_x,
            area.y,
            label_w as usize,
            action.label,
            label_style,
        );
        cur_x += label_w;

        commands.push(CommandHit {
            area: Rect::new(chip_x, area.y, group_w, 1),
            key: action.key,
        });
        first = false;
    }
}

fn write_elided(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: usize,
    text: &str,
    style: Style,
    icons: IconMode,
) -> u16 {
    if UnicodeWidthStr::width(text) <= width {
        return write_clipped_str(buf, x, y, width, text, style);
    }
    let (marker, marker_width) = ellipsis_for_mode(icons);
    if width < marker_width {
        return 0;
    }
    let budget = width - marker_width;
    let (suffix, _) = tail_within_width(text, budget);
    let used = write_clipped_str(buf, x, y, marker_width, marker, style);
    used + write_clipped_str(buf, x + used, y, budget, suffix, style)
}

/// Draw `text` within `width` columns; when it does not fit, keep both the
/// head and the tail joined by the profile ellipsis, so a status keeps its
/// operation identity and a trailing reason survives a long embedded path.
/// Head and tail are borrowed slices written straight to the buffer.
fn write_middle_elided(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: usize,
    text: &str,
    style: Style,
    icons: IconMode,
) -> u16 {
    if UnicodeWidthStr::width(text) <= width {
        return write_clipped_str(buf, x, y, width, text, style);
    }
    let (marker, marker_width) = ellipsis_for_mode(icons);
    if width <= marker_width {
        // Too narrow for head + marker + tail: degrade to a plain clip.
        return write_clipped_str(buf, x, y, width, text, style);
    }
    let budget = width - marker_width;
    let head_budget = budget.div_ceil(2);
    let tail_budget = budget - head_budget;
    let (head, head_width) = head_within_width(text, head_budget);
    let (tail, _) = tail_within_width(text, tail_budget);
    let mut used = write_clipped_str(buf, x, y, head_width, head, style);
    used += write_clipped_str(buf, x + used, y, marker_width, marker, style);
    used + write_clipped_str(buf, x + used, y, tail_budget, tail, style)
}

fn draw_commands<'a>(
    buf: &mut Buffer,
    area: Rect,
    actions: impl IntoIterator<Item = (&'a str, KeyEvent)>,
    style: Style,
    hits: &mut Vec<CommandHit>,
) {
    if area.is_empty() {
        return;
    }
    let (mut x, mut y) = (area.x, area.y);
    for (label, key) in actions {
        let width = UnicodeWidthStr::width(label) as u16;
        if width > area.width {
            continue;
        }
        if width > area.right().saturating_sub(x) {
            x = area.x;
            y = y.saturating_add(1);
        }
        if y >= area.bottom() {
            break;
        }
        write_clipped_str(buf, x, y, width as usize, label, style);
        hits.push(CommandHit {
            area: Rect::new(x, y, width, 1),
            key,
        });
        x = x.saturating_add(width).saturating_add(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_gutter_and_body(
    frame: &mut Frame,
    gutter_area: Rect,
    text_area: Rect,
    pane: &PaneScreen<'_>,
    is_active_pane: bool,
    theme: &Theme,
    line_digits: usize,
    total_lines: usize,
) {
    let doc_len = pane.document.len_chars();
    let (sel_min, sel_max) = match &pane.selection {
        Some(r) => (
            r.start.min(r.end).min(doc_len),
            r.start.max(r.end).min(doc_len),
        ),
        None => (0, 0),
    };
    let has_selection = pane.selection.is_some() && sel_min < sel_max;
    let selection_style = theme.selection_style();

    let cursor_pos = pane.cursor.min(doc_len);
    let cursor_line = pane.document.line_of(cursor_pos);

    let left_cell = pane.left_cell;
    let right_cell = left_cell.saturating_add(text_area.width as usize);

    let buf = frame.buffer_mut();
    let mut line_num_buf = String::with_capacity(line_digits);
    let mut hl_idx = pane
        .highlights
        .partition_point(|lt| lt.line < pane.top_line);
    let row_count = gutter_area.height.max(text_area.height);

    for row_idx in 0..row_count {
        let line_idx = pane.top_line.saturating_add(row_idx as usize);
        let screen_y = gutter_area.y + row_idx;
        let is_cursor_line = is_active_pane && line_idx == cursor_line;

        // 1. Draw gutter cell
        if !gutter_area.is_empty() && row_idx < gutter_area.height {
            line_num_buf.clear();
            let (num_style, sep_style) = if line_idx < total_lines {
                let _ = write!(
                    &mut line_num_buf,
                    "{:>width$}",
                    line_idx + 1,
                    width = line_digits
                );
                if is_cursor_line {
                    (
                        Style::default()
                            .bg(theme.canvas)
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                        Style::default().bg(theme.canvas).fg(theme.muted),
                    )
                } else {
                    (
                        Style::default().bg(theme.canvas).fg(theme.muted),
                        Style::default().bg(theme.canvas).fg(theme.muted),
                    )
                }
            } else {
                let _ = write!(&mut line_num_buf, "{:>width$}", "~", width = line_digits);
                (
                    Style::default().bg(theme.canvas).fg(theme.muted),
                    Style::default().bg(theme.canvas).fg(theme.muted),
                )
            };

            let num_clip = (gutter_area.width as usize).min(line_digits);
            buf.set_stringn(gutter_area.x, screen_y, &line_num_buf, num_clip, num_style);
            let sep_x = gutter_area.x + line_digits as u16;
            if sep_x < gutter_area.right() {
                buf[(sep_x, screen_y)].set_symbol(" ").set_style(sep_style);
            }
        }

        if text_area.is_empty() || row_idx >= text_area.height {
            continue;
        }

        // 2. Pre-fill text_area row with uniform canvas background
        let line_bg = theme.canvas;
        let default_line_style = Style::default().bg(line_bg).fg(theme.fg);

        for x in text_area.left()..text_area.right() {
            buf[(x, screen_y)]
                .set_symbol(" ")
                .set_style(default_line_style);
        }

        // 3. Ordered highlight span lookup for this line
        while hl_idx < pane.highlights.len() && pane.highlights[hl_idx].line < line_idx {
            hl_idx += 1;
        }
        let line_spans: &[TokenSpan] =
            if hl_idx < pane.highlights.len() && pane.highlights[hl_idx].line == line_idx {
                &pane.highlights[hl_idx].spans
            } else {
                &[]
            };
        let mut span_idx = 0usize;

        // 4. Render document cells for this line. cells_from_column resumes
        // from a checkpoint at or before left_cell and may still yield prefix
        // cells, so the column clips below are load-bearing; keep them.
        if line_idx < pane.document.line_count() {
            for cell in pane.document.cells_from_column(line_idx, left_cell) {
                if cell.width == 0 || cell.column + cell.width <= left_cell {
                    continue;
                }
                if cell.column >= right_cell {
                    break;
                }

                while span_idx < line_spans.len()
                    && (line_spans[span_idx].chars.is_empty()
                        || line_spans[span_idx].chars.end <= cell.start)
                {
                    span_idx += 1;
                }
                let token = if span_idx < line_spans.len()
                    && line_spans[span_idx].chars.start < cell.end
                    && line_spans[span_idx].chars.end > cell.start
                {
                    line_spans[span_idx].token
                } else {
                    Token::Plain
                };

                let is_selected = has_selection && cell.start < sel_max && cell.end > sel_min;
                let cell_style = if is_selected {
                    selection_style
                } else if token == Token::Plain {
                    default_line_style
                } else {
                    Style::default().bg(line_bg).fg(theme.syntax(token))
                };

                if cell.width == 1 {
                    let sx = text_area.x + (cell.column - left_cell) as u16;
                    buf[(sx, screen_y)]
                        .set_symbol(&cell.text)
                        .set_style(cell_style);
                    continue;
                }

                // Multi-column cell: wide glyph or safe multi-grapheme expansion (tab/escape).
                let mut sub_col = cell.column;
                for grapheme in cell.text.graphemes(true) {
                    let gw = UnicodeWidthStr::width(grapheme);
                    if gw == 0 {
                        continue;
                    }
                    if sub_col + gw <= left_cell {
                        sub_col += gw;
                        continue;
                    }
                    if sub_col >= right_cell {
                        break;
                    }
                    if sub_col < left_cell {
                        let vis = (sub_col + gw - left_cell).min(right_cell - left_cell);
                        for k in 0..vis {
                            let sx = text_area.x + k as u16;
                            buf[(sx, screen_y)].set_symbol(" ").set_style(cell_style);
                        }
                    } else if sub_col + gw > right_cell {
                        let start = sub_col - left_cell;
                        let vis = right_cell - sub_col;
                        for k in 0..vis {
                            let sx = text_area.x + (start + k) as u16;
                            buf[(sx, screen_y)].set_symbol(" ").set_style(cell_style);
                        }
                        break;
                    } else {
                        let sx = text_area.x + (sub_col - left_cell) as u16;
                        buf[(sx, screen_y)]
                            .set_symbol(grapheme)
                            .set_style(cell_style);
                        for w in 1..gw {
                            let nx = sx + w as u16;
                            buf[(nx, screen_y)].reset();
                            buf[(nx, screen_y)].set_style(cell_style);
                        }
                    }
                    sub_col += gw;
                }
            }

            // Highlight selected newline / line break if selection spans across EOL
            if has_selection {
                let line_end = pane.document.line_end(line_idx);
                if sel_min <= line_end && sel_max > line_end {
                    let eol_col = pane.document.column_of(line_end);
                    if eol_col >= left_cell && eol_col < right_cell {
                        let eol_screen_x = text_area.x + (eol_col - left_cell) as u16;
                        buf[(eol_screen_x, screen_y)]
                            .set_symbol(" ")
                            .set_style(selection_style);
                    }
                }
            }
        }
    }
}

fn position_editor_cursor(frame: &mut Frame, text_area: Rect, pane: &PaneScreen<'_>) {
    if text_area.is_empty() {
        return;
    }
    let cursor_pos = pane.cursor.min(pane.document.len_chars());
    let cursor_line = pane.document.line_of(cursor_pos);
    let cursor_col = pane.document.column_of(cursor_pos);

    let is_line_visible =
        cursor_line >= pane.top_line && cursor_line < pane.top_line + text_area.height as usize;
    let is_col_visible =
        cursor_col >= pane.left_cell && cursor_col < pane.left_cell + text_area.width as usize;

    if is_line_visible && is_col_visible {
        let cur_x = text_area.x + (cursor_col - pane.left_cell) as u16;
        let cur_y = text_area.y + (cursor_line - pane.top_line) as u16;
        frame.set_cursor_position((cur_x, cur_y));
    }
}

pub(crate) fn dialog_block<'a>(
    title: &'a str,
    role: ChromeIcon,
    theme: &Theme,
    icons: IconMode,
) -> Block<'a> {
    let border_set = if icons == IconMode::Ascii {
        ASCII_BORDER
    } else {
        border::ROUNDED
    };
    Block::default()
        .borders(Borders::ALL)
        .border_set(border_set)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.canvas).fg(theme.fg))
        .title(icon_title(
            theme::chrome_icon(role, icons),
            title,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
}

fn draw_dialog(
    frame: &mut Frame,
    area: Rect,
    dialog: &Dialog<'_>,
    theme: &Theme,
    icons: IconMode,
    geometry: &mut Geometry,
) {
    geometry.commands.clear();
    geometry.field_pool.append(&mut geometry.fields);
    if let Dialog::Help { scroll } = dialog {
        crate::help::draw(frame, area, *scroll, theme, icons, &mut geometry.commands);
        return;
    }
    if area.is_empty() {
        return;
    }
    let min_size = if matches!(dialog, Dialog::Search { .. }) {
        MIN_SEARCH_SIZE
    } else {
        MIN_DIALOG_SIZE
    };
    if area.width < min_size.0 || area.height < min_size.1 {
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new("Resize!\nEsc back")
                .style(Style::default().fg(theme.warning).bg(theme.canvas)),
            area,
        );
        return;
    }
    let width = area.width.saturating_sub(2).max(20).min(80).min(area.width);
    let inner_width = width.saturating_sub(4);
    let button_style = Style::default()
        .fg(theme.accent)
        .bg(theme.canvas)
        .add_modifier(Modifier::BOLD);
    match dialog {
        Dialog::Path { title, value } => {
            let actions = [
                ("Enter Accept", KeyCode::Enter),
                ("Esc Cancel", KeyCode::Esc),
                ("F1 Help", KeyCode::F(1)),
            ];
            let rows = command_rows(actions.iter().map(|action| action.0), inner_width);
            let rect = centered_dialog(area, width, (rows + 3).max(7).min(area.height));
            let clean_title = sanitize_ui_cow(title);
            let fitted_title = fit_str_with_ellipsis_mode(
                clean_title.as_ref(),
                width.saturating_sub(4) as usize,
                icons,
            );
            frame.render_widget(Clear, rect);
            frame.render_widget(
                dialog_block(fitted_title.as_ref(), ChromeIcon::Workspace, theme, icons),
                rect,
            );
            let commands_y = rect.bottom() - 1 - rows;
            let input_y = rect.y + if commands_y > rect.y + 2 { 2 } else { 1 };
            let cursor = draw_field(
                frame,
                geometry,
                FieldKind::Path,
                Rect::new(rect.x + 2, input_y, inner_width, 1),
                "> ",
                value,
                true,
                theme,
                icons,
            );
            if let Some(pos) = cursor {
                frame.set_cursor_position(pos);
            }
            draw_commands(
                frame.buffer_mut(),
                Rect::new(rect.x + 2, commands_y, inner_width, rows),
                actions
                    .iter()
                    .map(|&(label, code)| (label, KeyEvent::new(code, KeyModifiers::NONE))),
                button_style,
                &mut geometry.commands,
            );
        }
        Dialog::Search {
            query,
            replacement,
            replacement_focus,
        } => {
            let actions: &[(&str, KeyCode, KeyModifiers)] = if replacement.is_some() {
                &[
                    ("F3 Next", KeyCode::F(3), KeyModifiers::NONE),
                    ("S-F3 Prev", KeyCode::F(3), KeyModifiers::SHIFT),
                    ("F6 Replace", KeyCode::F(6), KeyModifiers::NONE),
                    ("F7 All", KeyCode::F(7), KeyModifiers::NONE),
                    ("Tab Field", KeyCode::Tab, KeyModifiers::NONE),
                    ("Esc Close", KeyCode::Esc, KeyModifiers::NONE),
                    ("F1 Help", KeyCode::F(1), KeyModifiers::NONE),
                ]
            } else {
                &[
                    ("F3 Next", KeyCode::F(3), KeyModifiers::NONE),
                    ("S-F3 Prev", KeyCode::F(3), KeyModifiers::SHIFT),
                    ("Esc Close", KeyCode::Esc, KeyModifiers::NONE),
                    ("F1 Help", KeyCode::F(1), KeyModifiers::NONE),
                ]
            };
            let rows = command_rows(actions.iter().map(|action| action.0), inner_width);
            let field_rows = if replacement.is_some() { 2 } else { 1 };
            let rect = centered_dialog(area, width, (field_rows + rows + 5).min(area.height));
            frame.render_widget(Clear, rect);
            frame.render_widget(
                dialog_block(
                    if replacement.is_some() {
                        "Replace"
                    } else {
                        "Find"
                    },
                    ChromeIcon::Search,
                    theme,
                    icons,
                ),
                rect,
            );
            let query_focused = replacement.is_none() || !*replacement_focus;
            let y = rect.y + 2;
            let mut cursor = draw_field(
                frame,
                geometry,
                FieldKind::Query,
                Rect::new(rect.x + 2, y, inner_width, 1),
                if query_focused {
                    "> Find: "
                } else {
                    "  Find: "
                },
                query,
                query_focused,
                theme,
                icons,
            );
            if let Some(value) = replacement {
                let replacement_cursor = draw_field(
                    frame,
                    geometry,
                    FieldKind::Replacement,
                    Rect::new(rect.x + 2, y + 1, inner_width, 1),
                    if *replacement_focus {
                        "> Repl: "
                    } else {
                        "  Repl: "
                    },
                    value,
                    *replacement_focus,
                    theme,
                    icons,
                );
                cursor = replacement_cursor.or(cursor);
            }
            if let Some(pos) = cursor {
                frame.set_cursor_position(pos);
            }
            draw_commands(
                frame.buffer_mut(),
                Rect::new(rect.x + 2, rect.bottom() - 1 - rows, inner_width, rows),
                actions
                    .iter()
                    .map(|&(label, code, modifiers)| (label, KeyEvent::new(code, modifiers))),
                button_style,
                &mut geometry.commands,
            );
        }
        Dialog::Choices { title, lines, kind } => {
            let actions = kind.actions();
            let rows = command_rows(actions.iter().map(|action| action.0), inner_width);
            let message_rows = lines
                .iter()
                .map(|line| wrapped_line_rows(sanitize_ui_cow(line).as_ref(), inner_width as usize))
                .sum::<usize>()
                .min(u16::MAX as usize) as u16;
            let height = rows
                .saturating_add(message_rows)
                .saturating_add(5)
                .min(area.height);
            let rect = centered_dialog(area, width, height);
            let clean_title = sanitize_ui_cow(title);
            let fitted_title = fit_str_with_ellipsis_mode(
                clean_title.as_ref(),
                width.saturating_sub(4) as usize,
                icons,
            );
            frame.render_widget(Clear, rect);
            frame.render_widget(
                dialog_block(fitted_title.as_ref(), ChromeIcon::Status, theme, icons),
                rect,
            );
            let visible_rows = rows.min(height.saturating_sub(2));
            let command_y = rect.bottom() - 1 - visible_rows;
            let mut y = rect.y + 1;
            if command_y.saturating_sub(y) > message_rows {
                y += 1;
            }
            for line in *lines {
                let available = command_y.saturating_sub(y);
                if available == 0 {
                    break;
                }
                let clean_line = sanitize_ui_cow(line);
                let height = wrapped_line_rows(clean_line.as_ref(), inner_width as usize)
                    .min(available as usize) as u16;
                frame.render_widget(
                    Paragraph::new(clean_line.as_ref())
                        .style(Style::default().fg(theme.fg).bg(theme.canvas))
                        .wrap(Wrap { trim: true }),
                    Rect::new(rect.x + 2, y, inner_width, height),
                );
                y += height;
            }
            draw_commands(
                frame.buffer_mut(),
                Rect::new(rect.x + 2, command_y, inner_width, visible_rows),
                actions
                    .iter()
                    .map(|&(label, code)| (label, KeyEvent::new(code, KeyModifiers::NONE))),
                button_style,
                &mut geometry.commands,
            );
        }
        Dialog::Help { .. } => unreachable!(),
    }
}

fn centered_dialog(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 3,
        width,
        height,
    )
}

fn command_rows<'a>(labels: impl IntoIterator<Item = &'a str>, width: u16) -> u16 {
    let (mut rows, mut column) = (1u16, 0usize);
    for label in labels {
        let length = UnicodeWidthStr::width(label);
        if length > width as usize {
            continue;
        }
        if column + length > width as usize {
            rows += 1;
            column = 0;
        }
        column += length + 1;
    }
    rows
}

impl ChoiceKind {
    fn actions(self) -> &'static [(&'static str, KeyCode)] {
        match self {
            Self::Unsaved => &[
                ("S Save", KeyCode::Char('s')),
                ("D Discard", KeyCode::Char('d')),
                ("C Cancel", KeyCode::Char('c')),
                ("F1 Help", KeyCode::F(1)),
            ],
            Self::Overwrite => &[
                ("Y Overwrite", KeyCode::Char('y')),
                ("N Cancel", KeyCode::Char('n')),
                ("F1 Help", KeyCode::F(1)),
            ],
            Self::Conflict => &[
                ("O Overwrite", KeyCode::Char('o')),
                ("A SaveAs", KeyCode::Char('a')),
                ("R Discard+load", KeyCode::Char('r')),
                ("C Cancel", KeyCode::Char('c')),
                ("F1 Help", KeyCode::F(1)),
            ],
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_field(
    frame: &mut Frame,
    geometry: &mut Geometry,
    kind: FieldKind,
    area: Rect,
    prefix: &str,
    value: &FieldView<'_>,
    focused: bool,
    theme: &Theme,
    icons: IconMode,
) -> Option<(u16, u16)> {
    let mut hit = geometry.field_pool.pop().unwrap_or_else(|| FieldHit {
        kind,
        area: Rect::default(),
        stops: Vec::new(),
    });
    hit.kind = kind;
    let cursor = crate::field_ui::draw(
        frame.buffer_mut(),
        area,
        prefix,
        value,
        focused,
        theme,
        icons,
        &mut hit,
    );
    if hit.area.is_empty() {
        geometry.field_pool.push(hit);
    } else {
        geometry.fields.push(hit);
    }
    cursor
}

fn head_within_width(s: &str, max_width: usize) -> (&str, usize) {
    let mut end_byte = 0usize;
    let mut width = 0usize;
    for (idx, grapheme) in s.grapheme_indices(true) {
        let gw = UnicodeWidthStr::width(grapheme);
        if width + gw > max_width {
            break;
        }
        width += gw;
        end_byte = idx + grapheme.len();
    }
    (&s[..end_byte], width)
}

fn tail_within_width(s: &str, max_width: usize) -> (&str, usize) {
    let mut start_byte = s.len();
    let mut width = 0usize;
    for (idx, grapheme) in s.grapheme_indices(true).rev() {
        let gw = UnicodeWidthStr::width(grapheme);
        if width + gw > max_width {
            break;
        }
        width += gw;
        start_byte = idx;
    }
    (&s[start_byte..], width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ColorMode, ThemeKind};
    use ratatui::{
        Terminal,
        backend::{Backend, TestBackend},
        layout::Position,
    };
    use std::fs;
    use tempfile::TempDir;

    fn render_screen(
        width: u16,
        height: u16,
        screen: &Screen<'_>,
    ) -> (Terminal<TestBackend>, Geometry) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test backend init");
        let mut geom = Geometry::default();
        terminal
            .draw(|frame| {
                draw(frame, screen, &mut geom);
            })
            .expect("draw test frame");
        (terminal, geom)
    }

    fn render_into(
        width: u16,
        height: u16,
        screen: &Screen<'_>,
        geom: &mut Geometry,
    ) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test backend init");
        terminal
            .draw(|frame| {
                draw(frame, screen, geom);
            })
            .expect("draw test frame");
        terminal
    }

    fn base_tabs<'a>() -> [TabCaption<'a>; 1] {
        [TabCaption {
            id: 1,
            label: "note.txt",
            dirty: false,
            read_only: false,
            active: true,
        }]
    }

    fn single_pane<'a>(
        doc: &'a Document,
        tabs: &'a [TabCaption<'a>],
        highlights: &'a [LineTokens],
    ) -> PaneScreen<'a> {
        PaneScreen {
            id: 1,
            document_id: 10,
            document: doc,
            cursor: 0,
            selection: None,
            top_line: 0,
            left_cell: 0,
            read_only: false,
            tabs,
            highlights,
        }
    }

    fn rects_overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    #[test]
    fn critical_choices_remain_visible_at_minimum_size_without_parsing_labels() {
        let doc = Document::new("unchanged");
        let tabs = base_tabs();
        let panes = [single_pane(&doc, &tabs, &[])];
        let tree = SplitTree::new(1);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::None);
        let cases: &[(ChoiceKind, &[char])] = &[
            (ChoiceKind::Unsaved, &['s', 'd', 'c']),
            (ChoiceKind::Overwrite, &['y', 'n']),
            (ChoiceKind::Conflict, &['o', 'a', 'r', 'c']),
        ];
        for &(kind, expected) in cases {
            let screen = Screen {
                panes: &panes,
                active_pane: 1,
                tree: &tree,
                title: "[Q] not an action",
                status: "[Q] not an action",
                active_path: None,
                format: FileFormat::default(),
                dirty: false,
                dialog: Some(Dialog::Choices {
                    title: "[Q] not an action",
                    lines: &["[Q] not an action"],
                    kind,
                }),
                explorer: None,
                sidebar_width: 0,
                explorer_focused: false,
                theme: &theme,
                icons: IconMode::Ascii,
            };
            for (width, height) in [(20, 6), (80, 24)] {
                let (_, geometry) = render_screen(width, height, &screen);
                for &key in expected {
                    assert!(
                        geometry
                            .commands
                            .iter()
                            .any(|hit| hit.key.code == KeyCode::Char(key))
                    );
                }
                assert!(
                    !geometry
                        .commands
                        .iter()
                        .any(|hit| hit.key.code == KeyCode::Char('q'))
                );
                for hit in &geometry.commands {
                    assert!(!hit.area.is_empty());
                    assert_eq!(
                        hit.area.intersection(Rect::new(0, 0, width, height)),
                        hit.area
                    );
                }
            }
        }
    }

    fn row_text(buf: &Buffer, rect: Rect, row_offset: u16) -> String {
        let y = rect.y + row_offset;
        (rect.left()..rect.right())
            .map(|x| buf[(x, y)].symbol())
            .collect()
    }

    /// Expected row for a far viewport: `cells` lists (absolute terminal
    /// column, symbol) for every visible cell of the line; columns not listed
    /// (continuation halves excepted, which tests list explicitly) render as
    /// canvas blanks.
    fn row_expected(left_cell: usize, width: usize, cells: &[(usize, &str)]) -> String {
        (left_cell..left_cell + width)
            .map(|col| {
                cells
                    .iter()
                    .find(|&&(c, _)| c == col)
                    .map(|(_, sym)| *sym)
                    .unwrap_or(" ")
            })
            .collect()
    }

    fn status_screen<'a>(
        panes: &'a [PaneScreen<'a>],
        tree: &'a SplitTree,
        theme: &'a Theme,
        status: &'a str,
        active_path: Option<&'a Path>,
        icons: IconMode,
    ) -> Screen<'a> {
        Screen {
            panes,
            active_pane: 1,
            tree,
            title: "note.txt",
            status,
            active_path,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme,
            icons,
        }
    }

    // Framed chrome places the status row at height-3; unframed docks keep it
    // on the row above the helper (or the last row without one).
    fn status_row_y(width: u16, height: u16) -> u16 {
        if width >= 40 && height >= 18 {
            height - 3
        } else if height >= 8 {
            height - 2
        } else {
            height.saturating_sub(1)
        }
    }

    fn status_row_text(term: &Terminal<TestBackend>, width: u16, height: u16) -> String {
        row_text(
            term.backend().buffer(),
            Rect::new(0, status_row_y(width, height), width, 1),
            0,
        )
    }

    #[test]
    fn status_clipboard_error_reason_survives_instead_of_path_at_120_cols() {
        let doc = Document::new("unchanged");
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let panes = [single_pane(&doc, &tabs, &[])];
        let status = "Paste failed: clipboard returned invalid UTF-8; previous content preserved";
        let screen = status_screen(
            &panes,
            &tree,
            &theme,
            status,
            Some(Path::new("/home/example/notes/report.txt")),
            IconMode::Unicode,
        );
        let term = render_screen(120, 24, &screen).0;
        let row = status_row_text(&term, 120, 24);
        // The whole reason fits; it is neither cut at half the row nor
        // replaced by the active file path.
        assert!(row.contains(status), "row: {row}");
        // Metadata still fits beside the message at this width.
        assert_eq!(
            term.backend().buffer()[(117, status_row_y(120, 24))]
                .style()
                .fg,
            Some(theme.muted)
        );
        // Error classification reaches the drawn cells.
        let y = status_row_y(120, 24);
        let buf = term.backend().buffer();
        for x in [2u16, 4] {
            let style = buf[(x, y)].style();
            assert_eq!(style.fg, Some(theme.error));
            assert!(style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn status_long_save_error_elision_keeps_action_and_final_cause() {
        let doc = Document::new("unchanged");
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let panes = [single_pane(&doc, &tabs, &[])];
        let middle = "m".repeat(96);
        let status = format!("Save failed: /data/{middle}/paper.tex: No space left on device");
        let screen = status_screen(
            &panes,
            &tree,
            &theme,
            &status,
            Some(Path::new("/tmp/notes/report.txt")),
            IconMode::Unicode,
        );
        let term = render_screen(120, 24, &screen).0;
        let row = status_row_text(&term, 120, 24);
        assert!(row.contains("Save failed:"), "row: {row}");
        assert!(row.contains("No space left on device"));
        assert!(row.contains('…'));
        assert!(!row.contains(&middle[..50]), "row: {row}");
        // Oversized critical message: metadata is skipped, not drawn behind it.
        assert_eq!(
            term.backend().buffer()[(117, status_row_y(120, 24))]
                .style()
                .fg,
            Some(theme.error)
        );
    }

    #[test]
    fn status_long_warning_displaces_metadata_before_eliding() {
        let doc = Document::new("unchanged");
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let panes = [single_pane(&doc, &tabs, &[])];
        let status = format!(
            "Highlight: pattern '{}' flooded the visible pane so it cannot be shown in full",
            "w".repeat(90)
        );
        let screen = status_screen(
            &panes,
            &tree,
            &theme,
            &status,
            Some(Path::new("/home/example/notes/report.txt")),
            IconMode::Unicode,
        );
        let term = render_screen(120, 24, &screen).0;
        let row = status_row_text(&term, 120, 24);
        assert!(row.contains("Highlight:"), "row: {row}");
        assert!(row.contains("shown in full"));
        assert!(row.contains('…'));
        assert_eq!(
            term.backend().buffer()[(117, status_row_y(120, 24))]
                .style()
                .fg,
            Some(theme.warning)
        );
        assert!(!row.contains("report.txt"));
        let style = term.backend().buffer()[(2, status_row_y(120, 24))].style();
        assert_eq!(style.fg, Some(theme.warning));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn status_fitting_plain_message_keeps_full_text_path_and_metadata() {
        let doc = Document::new("unchanged");
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let panes = [single_pane(&doc, &tabs, &[])];
        let status = "Normalized indentation across the whole document and refreshed markers";
        let screen = status_screen(
            &panes,
            &tree,
            &theme,
            status,
            Some(Path::new("/home/example/notes/report.txt")),
            IconMode::Unicode,
        );
        let term = render_screen(120, 24, &screen).0;
        let row = status_row_text(&term, 120, 24);
        // Longer than the old half-row cap yet fitting: drawn in full.
        assert!(row.contains(status), "row: {row}");
        assert_eq!(
            term.backend().buffer()[(117, status_row_y(120, 24))]
                .style()
                .fg,
            Some(theme.muted)
        );
        assert!(row.contains("notes/report.txt"));
        let style = term.backend().buffer()[(2, status_row_y(120, 24))].style();
        assert_eq!(style.fg, Some(theme.fg));
        assert!(style.add_modifier.is_empty());
    }

    #[test]
    fn status_tiny_widths_icon_profiles_and_controls_degrade_safely() {
        let doc = Document::new("unchanged");
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::None);
        let panes = [single_pane(&doc, &tabs, &[])];
        let middle = "m".repeat(96);
        let long_error = format!("Save failed: /data/{middle}/paper.tex: No space left on device");
        let hostile = "Saved café-\u{202e}日本語🦀.rs\x07";
        for icons in [IconMode::Ascii, IconMode::Unicode, IconMode::Nerd] {
            for width in [12u16, 20, 40] {
                let err_screen = status_screen(&panes, &tree, &theme, &long_error, None, icons);
                let err_row = status_row_text(&render_screen(width, 8, &err_screen).0, width, 8);
                if width >= 40 {
                    assert!(err_row.contains("Save failed:"), "row: {err_row}");
                    assert!(err_row.contains("on device"));
                }

                let hostile_screen = status_screen(&panes, &tree, &theme, hostile, None, icons);
                let hostile_row =
                    status_row_text(&render_screen(width, 8, &hostile_screen).0, width, 8);
                assert!(!hostile_row.contains('\u{7}'), "row: {hostile_row}");
                assert!(!hostile_row.contains('\u{202e}'), "row: {hostile_row}");
                if width >= 40 {
                    assert!(hostile_row.contains("Saved"), "row: {hostile_row}");
                }
            }
        }
    }

    #[test]
    fn test_sanitize_ui_text_escapes_dangerous_controls_preserving_unicode() {
        let osc8 = "\x1b]8;;https://evil.example\x07click\x1b]8;;\x07";
        let clean = sanitize_ui_text(osc8);
        assert!(!clean.contains('\x1b'));
        assert!(!clean.contains('\x07'));
        assert!(clean.contains("\\x1b"));
        assert!(clean.contains("\\x07"));

        let bidi = "test\u{202e}spoof\u{202c}";
        let clean_bidi = sanitize_ui_text(bidi);
        assert!(!clean_bidi.contains('\u{202e}'));
        assert!(!clean_bidi.contains('\u{202c}'));
        assert!(clean_bidi.contains("\\u{202e}"));

        let invisible = "zero\u{200b}width\u{feff}bom\u{2060}wj";
        let clean_inv = sanitize_ui_text(invisible);
        assert!(!clean_inv.contains('\u{200b}'));
        assert!(!clean_inv.contains('\u{feff}'));
        assert!(!clean_inv.contains('\u{2060}'));

        let newlines = "a\nb\rc\td";
        assert_eq!(sanitize_ui_text(newlines), "a\\nb\\rc\\td");

        let unicode_name = "café-日本語-🦀.rs";
        assert_eq!(sanitize_ui_text(unicode_name), unicode_name);
    }

    #[test]
    fn test_fit_truncation_respects_display_width_unicode_and_ascii() {
        assert_eq!(fit_str_with_ellipsis("short", 10), "short");
        assert_eq!(fit_str_with_ellipsis("hello world", 5), "hell…");
        assert_eq!(fit_str_with_ellipsis("anything", 1), "…");
        assert_eq!(fit_str_with_ellipsis("anything", 0), "");

        assert_eq!(
            fit_str_with_ellipsis_mode("hello world", 7, IconMode::Ascii),
            "hell..."
        );
        assert_eq!(
            fit_str_with_ellipsis_mode("hello", 2, IconMode::Ascii),
            ".."
        );

        let wide = "漢字テキスト";
        let fitted = fit_str_with_ellipsis(wide, 5);
        assert!(UnicodeWidthStr::width(fitted.as_str()) <= 5);
        assert!(fitted.ends_with('…'));

        let path = "/very/long/directory/structure/target_file.rs";
        let fitted_path = fit_path_tail_with_ellipsis(path, 16);
        assert!(UnicodeWidthStr::width(fitted_path.as_str()) <= 16);
        assert!(fitted_path.starts_with('…'));
        assert!(fitted_path.ends_with("target_file.rs"));

        let ascii_path = fit_path_tail_with_ellipsis_mode(path, 18, IconMode::Ascii);
        assert!(UnicodeWidthStr::width(ascii_path.as_ref()) <= 18);
        assert!(ascii_path.starts_with("..."));
        assert!(ascii_path.ends_with("target_file.rs"));
        for mode in [IconMode::Unicode, IconMode::Nerd, IconMode::Ascii] {
            let tiny_path = fit_path_tail_with_ellipsis_mode(path, 1, mode);
            assert_eq!(UnicodeWidthStr::width(tiny_path.as_ref()), 1);
        }
    }

    #[test]
    fn test_wide_glyph_clipping_across_left_and_right_edges() {
        // Viewport columns 2..9: both 漢 (cols 1..3) and 🦀 (cols 8..10) straddle edges
        let doc = Document::new("x漢yyyyy🦀z");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let mut pane = single_pane(&doc, &tabs, &[]);
        pane.left_cell = 2;
        let panes = [pane];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let (terminal, geom) = render_screen(12, 6, &screen);
        let pg = geom.pane(1).expect("pane 1 geometry");
        assert!(!pg.text_area.is_empty());
        assert_eq!(pg.text_area.width, 7);

        let buf = terminal.backend().buffer();
        let row = pg.text_area.y;
        let x0 = pg.text_area.x;
        // col 2 was second half of "漢" -> must be space, never "漢"
        assert_eq!(buf[(x0, row)].symbol(), " ");
        for k in 1..6 {
            assert_eq!(buf[(x0 + k, row)].symbol(), "y");
        }
        // col 8 was first half of "🦀" -> must be space, never "🦀"
        assert_eq!(buf[(x0 + 6, row)].symbol(), " ");
    }

    #[test]
    fn test_syntax_highlighting_and_selection_priority() {
        let doc = Document::new("\t\x1bfn main\nworld");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let highlights = [LineTokens {
            line: 0,
            spans: vec![
                TokenSpan {
                    chars: 2..4,
                    token: Token::Keyword,
                },
                TokenSpan {
                    chars: 5..9,
                    token: Token::Function,
                },
            ],
        }];
        let mut pane = single_pane(&doc, &tabs, &highlights);
        pane.cursor = 5;
        pane.selection = Some(1..3);
        let panes = [pane];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let (mut term, geom) = render_screen(40, 10, &screen);
        let pg = geom.pane(1).expect("pane 1 geometry");
        let x0 = pg.text_area.x;
        let y0 = pg.text_area.y;
        let pos = term.backend_mut().get_cursor_position().unwrap();
        assert_eq!(pos, Position::new(x0 + 11, y0));

        let buf = term.backend().buffer();
        assert_eq!(buf[(x0 + 4, y0)].symbol(), "\\");
        assert_eq!(buf[(x0 + 4, y0)].bg, theme.selection);
        assert_eq!(buf[(x0 + 8, y0)].symbol(), "f");
        assert_eq!(buf[(x0 + 8, y0)].bg, theme.selection);
        assert_eq!(buf[(x0 + 8, y0)].fg, theme.fg);

        assert_eq!(buf[(x0 + 9, y0)].symbol(), "n");
        assert_eq!(buf[(x0 + 9, y0)].bg, theme.canvas);
        assert_eq!(buf[(x0 + 9, y0)].fg, theme.syntax(Token::Keyword));

        assert_eq!(buf[(x0 + 11, y0)].symbol(), "m");
        assert_eq!(buf[(x0 + 11, y0)].bg, theme.canvas);
        assert_eq!(buf[(x0 + 11, y0)].fg, theme.syntax(Token::Function));
    }

    #[test]
    fn test_far_viewport_esc_tab_wide_clip_syntax_selection_and_cursor() {
        // One 1 KiB+ line viewed at far horizontal offsets through two panes.
        // Char index -> absolute terminal column (TAB_STOP = 4):
        //   'x'x1022   chars 0..1022     cols 0..1021
        //   '\t'       char 1022         cols 1022..1023 (2 wide, stop at 1024)
        //   '\x1b'     char 1023         cols 1024..1027 (escaped "\x1b", 4 wide)
        //   '漢'       char 1024         cols 1028..1029
        //   'y' / 'z'  chars 1025 / 1026 cols 1030 / 1031
        //   "🇮🇩"      chars 1027..1029  cols 1032..1033
        //   "e\u{301}" chars 1029..1031  col 1034
        //   ZWJ family chars 1031..1036  cols 1035..1036
        //   '\u{202e}' char 1036         cols 1037..1044 (escaped "\u{202e}", 8 wide)
        //   "end"      chars 1037..1040  cols 1045..1047
        //   '漢'       char 1040         cols 1048..1049
        //   '\n'       char 1041         EOL col 1050; line 1 is "second"
        let mut line = "x".repeat(1022);
        line.push('\t');
        line.push('\x1b');
        line.push('漢');
        line.push('y');
        line.push('z');
        line.push_str("🇮🇩");
        line.push('e');
        line.push('\u{301}');
        line.push_str("👨\u{200d}👩\u{200d}👧");
        line.push('\u{202e}');
        line.push_str("end");
        line.push('漢');
        line.push('\n');
        line.push_str("second");
        let doc = Document::new(&line);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let highlights = [LineTokens {
            line: 0,
            spans: vec![
                TokenSpan {
                    chars: 1025..1027, // 'y','z': selection must beat this Keyword
                    token: Token::Keyword,
                },
                TokenSpan {
                    chars: 1027..1029, // flag grapheme
                    token: Token::Number,
                },
                TokenSpan {
                    chars: 1029..1031, // combining cluster
                    token: Token::Function,
                },
                TokenSpan {
                    chars: 1037..1040, // "end"
                    token: Token::Keyword,
                },
            ],
        }];
        let tabs1 = base_tabs();
        let tabs2 = [TabCaption {
            id: 2,
            label: "far.txt",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let mut tree = SplitTree::new(1);
        assert!(tree.split(1, 2, Axis::Columns).is_some());
        let panes = [
            PaneScreen {
                id: 1,
                document_id: 7,
                document: &doc,
                cursor: doc.position_at(0, 1037), // start of the escaped RLO cell
                selection: Some(1039..1042),      // 'd' + wide 漢 + newline
                top_line: 0,
                left_cell: 1025, // inside the escaped ESC cell (cols 1024..1027)
                read_only: false,
                tabs: &tabs1,
                highlights: &highlights,
            },
            PaneScreen {
                id: 2,
                document_id: 7,
                document: &doc,
                cursor: 0,
                selection: Some(1025..1027), // 'y','z' under the Keyword span
                top_line: 0,
                left_cell: 1023, // inside the tab expansion (cols 1022..1023)
                read_only: false,
                tabs: &tabs2,
                highlights: &highlights,
            },
        ];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "far.txt",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let cells: &[(usize, &str)] = &[
            (1022, " "),
            (1023, " "),
            (1024, "\\"),
            (1025, "x"),
            (1026, "1"),
            (1027, "b"),
            (1028, "漢"),
            (1029, " "),
            (1030, "y"),
            (1031, "z"),
            (1032, "🇮🇩"),
            (1033, " "),
            (1034, "e\u{301}"),
            (1035, "👨\u{200d}👩\u{200d}👧"),
            (1036, " "),
            (1037, "\\"),
            (1038, "u"),
            (1039, "{"),
            (1040, "2"),
            (1041, "0"),
            (1042, "2"),
            (1043, "e"),
            (1044, "}"),
            (1045, "e"),
            (1046, "n"),
            (1047, "d"),
            (1048, "漢"),
            (1049, " "),
            (1050, " "),
        ];
        let (mut term, geom) = render_screen(80, 20, &screen);
        let pg1 = geom.pane(1).expect("pane 1 geometry");
        let pg2 = geom.pane(2).expect("pane 2 geometry");
        // Viewport widths come from the rendered geometry; the fixture only
        // needs at least 26 columns so its asserted content is on screen.
        let vw1 = pg1.text_area.width as usize;
        let vw2 = pg2.text_area.width as usize;
        assert!(vw1 >= 26 && vw2 >= 26, "viewports too narrow: {vw1}, {vw2}");
        let pos = term.backend_mut().get_cursor_position().unwrap();

        let buf = term.backend().buffer();
        // Pane 1 resumes inside the escaped ESC cell: its leading '\' is
        // clipped and none of the 1022-char 'x' prefix bleeds in.
        assert_eq!(
            row_text(buf, pg1.text_area, 0),
            row_expected(1025, vw1, cells)
        );
        // Pane 2 resumes inside the tab expansion, on its trailing space.
        assert_eq!(
            row_text(buf, pg2.text_area, 0),
            row_expected(1023, vw2, cells)
        );

        let (x1, y1) = (pg1.text_area.x, pg1.text_area.y);
        // Cursor sits on the first column of the escaped RLO cell.
        assert_eq!(pos, Position::new(x1 + 12, y1));
        // 'd' is selected, the wide 漢 it overlaps is fully styled selected,
        // and the EOL marker at col 1050 renders with the selection style.
        assert_eq!(buf[(x1 + 22, y1)].bg, theme.selection);
        assert_eq!(buf[(x1 + 23, y1)].symbol(), "漢");
        assert_eq!(buf[(x1 + 23, y1)].bg, theme.selection);
        assert_eq!(buf[(x1 + 25, y1)].bg, theme.selection);
        // Syntax mapping survives the checkpoint resume: "end" stays Keyword.
        assert_eq!(buf[(x1 + 20, y1)].fg, theme.syntax(Token::Keyword));
        assert_eq!(buf[(x1 + 20, y1)].bg, theme.canvas);

        let (x2, y2) = (pg2.text_area.x, pg2.text_area.y);
        // Selection wins over the Keyword span on 'y'/'z'.
        assert_eq!(buf[(x2 + 7, y2)].symbol(), "y");
        assert_eq!(buf[(x2 + 7, y2)].bg, theme.selection);
        assert_eq!(buf[(x2 + 7, y2)].fg, theme.fg);
        assert_eq!(buf[(x2 + 8, y2)].bg, theme.selection);
        // Flag keeps Number, combining cluster keeps Function after resume.
        assert_eq!(buf[(x2 + 9, y2)].fg, theme.syntax(Token::Number));
        assert_eq!(buf[(x2 + 9, y2)].bg, theme.canvas);
        assert_eq!(buf[(x2 + 11, y2)].fg, theme.syntax(Token::Function));
        assert_eq!(buf[(x2 + 22, y2)].fg, theme.syntax(Token::Keyword));
        // Far 漢 (cols 1048..1049) with its blank continuation half.
        assert_eq!(buf[(x2 + 25, y2)].symbol(), "漢");
        // Horizontal scroll is shared by all lines in this view; the short
        // second line is entirely left of either viewport.
        assert_eq!(row_text(buf, pg1.text_area, 1), " ".repeat(vw1));
        assert_eq!(row_text(buf, pg2.text_area, 1), " ".repeat(vw2));
    }

    #[test]
    fn test_far_viewport_wide_straddle_shared_independent_left_cells_tab_phase() {
        // Shared document, two panes at independent far offsets: one starts
        // inside a wide glyph, the other on a regional-indicator flag. The tab
        // after the checkpoint must keep its true column phase (3010 % 4 = 2,
        // expansion width 2); a lost phase would push 'z' two cells right.
        // Char index -> absolute terminal column:
        //   'x'x3000  chars 0..3000     cols 0..2999
        //   '漢'      char 3000         cols 3000..3001
        //   'y'       char 3001         col 3002
        //   "🇮🇩"     chars 3002..3004  cols 3003..3004
        //   "end"     chars 3004..3007  cols 3005..3007
        //   'q' / 'r' chars 3007 / 3008 cols 3008 / 3009
        //   '\t'      char 3009         cols 3010..3011
        //   'z'       char 3010         col 3012 (EOL col 3013)
        let mut line = "x".repeat(3000);
        line.push('漢');
        line.push('y');
        line.push_str("🇮🇩");
        line.push_str("end");
        line.push('q');
        line.push('r');
        line.push('\t');
        line.push('z');
        let doc = Document::new(&line);
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tabs1 = base_tabs();
        let tabs2 = [TabCaption {
            id: 2,
            label: "wide.txt",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let mut tree = SplitTree::new(1);
        assert!(tree.split(1, 2, Axis::Columns).is_some());
        let panes = [
            PaneScreen {
                id: 1,
                document_id: 9,
                document: &doc,
                cursor: doc.position_at(0, 3005), // 'e'
                selection: None,
                top_line: 0,
                left_cell: 3001, // second half of the far 漢
                read_only: false,
                tabs: &tabs1,
                highlights: &[],
            },
            PaneScreen {
                id: 2,
                document_id: 9,
                document: &doc,
                cursor: 0,
                selection: Some(3004..3007), // exactly "end"
                top_line: 0,
                left_cell: 3003, // on the flag grapheme
                read_only: false,
                tabs: &tabs2,
                highlights: &[],
            },
        ];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "wide.txt",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let cells: &[(usize, &str)] = &[
            (3000, "漢"),
            (3001, " "),
            (3002, "y"),
            (3003, "🇮🇩"),
            (3004, " "),
            (3005, "e"),
            (3006, "n"),
            (3007, "d"),
            (3008, "q"),
            (3009, "r"),
            (3010, " "),
            (3011, " "),
            (3012, "z"),
        ];
        let (mut term, geom) = render_screen(80, 20, &screen);
        let pg1 = geom.pane(1).expect("pane 1 geometry");
        let pg2 = geom.pane(2).expect("pane 2 geometry");
        let vw1 = pg1.text_area.width as usize;
        let vw2 = pg2.text_area.width as usize;
        assert!(vw1 >= 12 && vw2 >= 12, "viewports too narrow: {vw1}, {vw2}");
        let pos = term.backend_mut().get_cursor_position().unwrap();

        let buf = term.backend().buffer();
        assert_eq!(
            row_text(buf, pg1.text_area, 0),
            row_expected(3001, vw1, cells)
        );
        assert_eq!(
            row_text(buf, pg2.text_area, 0),
            row_expected(3003, vw2, cells)
        );

        let (x1, y1) = (pg1.text_area.x, pg1.text_area.y);
        assert_eq!(pos, Position::new(x1 + 4, y1));

        // Selection boundaries stay exact after resume: cells whose range only
        // touches the selection edges (flag end == sel start, 'q' start ==
        // sel end) are not selected.
        let (x2, y2) = (pg2.text_area.x, pg2.text_area.y);
        assert_eq!(buf[(x2, y2)].bg, theme.canvas);
        assert_eq!(buf[(x2 + 2, y2)].bg, theme.selection);
        assert_eq!(buf[(x2 + 4, y2)].bg, theme.selection);
        assert_eq!(buf[(x2 + 5, y2)].bg, theme.canvas);
    }

    #[test]
    fn test_tab_hit_regions_disjoint_and_overflow_keeps_active_visible() {
        let doc = Document::new("hello");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tree = SplitTree::new(1);

        let two_tabs = [
            TabCaption {
                id: 100,
                label: "alpha.rs",
                dirty: true,
                read_only: false,
                active: true,
            },
            TabCaption {
                id: 205,
                label: "beta.rs",
                dirty: false,
                read_only: true,
                active: false,
            },
        ];
        let panes_fit = [single_pane(&doc, &two_tabs, &[])];
        let screen_fit = Screen {
            panes: &panes_fit,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let (term_fit, geom_fit) = render_screen(80, 12, &screen_fit);
        let pg_fit = geom_fit.pane(1).expect("pane 1");
        assert_eq!(pg_fit.previous_tab, None);
        assert_eq!(pg_fit.next_tab, None);
        assert_eq!(pg_fit.tabs.len(), 2);
        let buf_fit = term_fit.backend().buffer();
        for (expected_id, hit) in [100u64, 205u64].into_iter().zip(pg_fit.tabs.iter()) {
            assert_eq!(hit.id, expected_id);
            assert!(!hit.body.is_empty());
            assert!(!hit.close.is_empty());
            assert!(!rects_overlap(hit.body, hit.close));
            assert_eq!(buf_fit[(hit.close.x, hit.close.y)].symbol(), "×");
        }
        let t0 = pg_fit.tabs[0];
        let has_star_in_body =
            (t0.body.left()..t0.body.right()).any(|x| buf_fit[(x, t0.body.y)].symbol() == "*");
        assert!(has_star_in_body);

        let many_labels = [
            "one.rs", "two.rs", "three.rs", "four.rs", "five.rs", "six.rs",
        ];
        for active_pos in [0, 3, 5] {
            let tabs: Vec<TabCaption<'_>> = many_labels
                .iter()
                .enumerate()
                .map(|(i, label)| TabCaption {
                    id: 500 + (i as u64) * 7,
                    label,
                    dirty: i == active_pos,
                    read_only: false,
                    active: i == active_pos,
                })
                .collect();
            let panes_ov = [single_pane(&doc, &tabs, &[])];
            let screen_ov = Screen {
                panes: &panes_ov,
                active_pane: 1,
                tree: &tree,
                title: "/ws",
                status: "ok",
                active_path: None,
                format: FileFormat::default(),
                dirty: false,
                dialog: None,
                explorer: None,
                sidebar_width: 0,
                explorer_focused: false,
                theme: &theme,
                icons: IconMode::Ascii,
            };
            let (term_ov, geom_ov) = render_screen(48, 12, &screen_ov);
            let pg_ov = geom_ov.pane(1).expect("pane 1 overflow");
            let prev = pg_ov.previous_tab.expect("previous_tab overflow button");
            let next = pg_ov.next_tab.expect("next_tab overflow button");
            assert!(!rects_overlap(prev, next));
            let expected_active_id = tabs[active_pos].id;
            assert!(
                pg_ov.tabs.iter().any(|h| h.id == expected_active_id),
                "active tab id {expected_active_id} must remain visible in overflow"
            );
            let buf_ov = term_ov.backend().buffer();
            for hit in &pg_ov.tabs {
                assert!(tabs.iter().any(|t| t.id == hit.id));
                assert!(!rects_overlap(hit.body, hit.close));
                assert!(!rects_overlap(hit.body, prev));
                assert!(!rects_overlap(hit.close, prev));
                assert!(!rects_overlap(hit.body, next));
                assert!(!rects_overlap(hit.close, next));
                assert_eq!(buf_ov[(hit.close.x, hit.close.y)].symbol(), "x");
            }
        }
    }

    #[test]
    fn test_sidebar_geometry_narrow_hide_and_divider() {
        let dir = TempDir::new().expect("tempdir");
        fs::create_dir(dir.path().join("alpha_dir")).expect("mkdir");
        fs::write(dir.path().join("alpha_dir/nested.txt"), "nested").expect("write child");
        fs::write(dir.path().join("beta.rs"), "fn main() {}").expect("write");
        let mut explorer = Explorer::new(dir.path().to_path_buf()).expect("explorer");
        explorer.toggle_selected().expect("expand alpha_dir");
        let doc = Document::new("hello");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let panes = [single_pane(&doc, &tabs, &[])];

        let screen_wide = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "root",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: Some(&explorer),
            sidebar_width: 28,
            explorer_focused: true,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let (mut term_wide, geom_wide) = render_screen(80, 20, &screen_wide);
        let sb = geom_wide.sidebar.expect("sidebar visible on 80x20");
        let sb_panel = geom_wide
            .sidebar_panel
            .expect("sidebar_panel visible on 80x20");
        let div = geom_wide
            .explorer_divider
            .expect("explorer divider visible on 80x20");
        let pg = geom_wide.pane(1).expect("pane 1");
        assert_eq!(sb_panel.width, 28);
        assert_eq!(sb.width, 26);
        let parent = geom_wide
            .sidebar_parent
            .expect("sidebar_parent visible on 80x20");
        assert_eq!(parent.width, 26);
        assert_eq!(parent.x, sb_panel.x + 1);
        assert_eq!(parent.y, sb_panel.y + 1);
        assert_eq!(parent.height, 1);
        assert_eq!(sb.x, sb_panel.x + 1);
        assert_eq!(sb.y, parent.bottom());
        assert_eq!(div.x, sb_panel.right());
        assert_eq!(div.width, 1);
        assert!(div.right() <= pg.area.x);
        assert_ne!(
            term_wide.backend_mut().get_cursor_position().ok(),
            Some(Position::new(pg.text_area.x, pg.text_area.y))
        );

        let buf = term_wide.backend().buffer();
        let row0_text = row_text(buf, sb, 0);
        let row1_text = row_text(buf, sb, 1);
        let row2_text = row_text(buf, sb, 2);
        assert!(row0_text.contains("▾") && row0_text.contains("alpha_dir"));
        assert!(row1_text.contains("nested.txt"));
        let child_column =
            UnicodeWidthStr::width(&row1_text[..row1_text.find("nested.txt").unwrap()]);
        let parent_column =
            UnicodeWidthStr::width(&row0_text[..row0_text.find("alpha_dir").unwrap()]);
        assert!(child_column > parent_column);
        assert!(row2_text.contains("beta.rs"));

        let (_, geom_narrow) = render_screen(26, 12, &screen_wide);
        assert_eq!(geom_narrow.sidebar_parent, None);
        assert_eq!(geom_narrow.sidebar, None);
        assert_eq!(geom_narrow.sidebar_panel, None);
        assert_eq!(geom_narrow.explorer_divider, None);
        assert!(!geom_narrow.pane(1).unwrap().text_area.is_empty());
    }

    #[test]
    fn test_no_color_focus_and_ascii_structural_ui() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("café.txt"), "hi").expect("write");
        let explorer = Explorer::new(dir.path().to_path_buf()).expect("explorer");
        let doc = Document::new("abcdef");
        let theme_none = Theme::new(ThemeKind::Dark, ColorMode::None);
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let mut pane = single_pane(&doc, &tabs, &[]);
        pane.selection = Some(1..4);
        let panes = [pane];

        let screen_exp = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: Some(&explorer),
            sidebar_width: 24,
            explorer_focused: true,
            theme: &theme_none,
            icons: IconMode::Ascii,
        };
        let screen_ed = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: Some(&explorer),
            sidebar_width: 24,
            explorer_focused: false,
            theme: &theme_none,
            icons: IconMode::Ascii,
        };
        let (term_exp, geom_exp) = render_screen(70, 14, &screen_exp);
        let (term_ed, geom_ed) = render_screen(70, 14, &screen_ed);

        let sb = geom_exp.sidebar.unwrap();
        let div = geom_exp.explorer_divider.unwrap();
        let pg_exp = geom_exp.pane(1).unwrap();
        let pg_ed = geom_ed.pane(1).unwrap();
        let buf_exp = term_exp.backend().buffer();
        let buf_ed = term_ed.backend().buffer();

        assert!(buf_exp[(div.x, div.y + div.height / 2)].symbol().is_ascii());
        assert_eq!(
            buf_exp[(pg_exp.tabs[0].close.x, pg_exp.tabs[0].close.y)].symbol(),
            "x"
        );
        let row0 = row_text(buf_exp, sb, 0);
        assert!(row0.contains("café.txt"));
        assert!(row0.starts_with('>'));

        let differs_structurally = (0..14).any(|y| {
            (0..70).any(|x| {
                let a = &buf_exp[(x, y)];
                let b = &buf_ed[(x, y)];
                a.symbol() != b.symbol() || a.modifier != b.modifier
            })
        });
        assert!(differs_structurally);

        assert!(
            buf_ed[(pg_ed.text_area.x + 1, pg_ed.text_area.y)]
                .modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn test_modal_size_guards_and_search_dialog_focus() {
        let doc = Document::new("abc");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::None);
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let panes = [single_pane(&doc, &tabs, &[])];

        for (w, h) in [(0, 0), (1, 1), (5, 2), (8, 3), (9, 4)] {
            let plain = Screen {
                panes: &panes,
                active_pane: 1,
                tree: &tree,
                title: "a",
                status: "b",
                active_path: None,
                format: FileFormat::default(),
                dirty: false,
                dialog: None,
                explorer: None,
                sidebar_width: 0,
                explorer_focused: false,
                theme: &theme,
                icons: IconMode::Unicode,
            };
            let (_, geom) = render_screen(w, h, &plain);
            assert!(geom.panes.is_empty());
            assert_eq!(geom.sidebar, None);
            assert_eq!(geom.explorer_divider, None);
            assert!(geom.splits.is_empty());
            assert!(!geom.collapsed);
        }

        let search_hidden = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "a",
            status: "b",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: Some(Dialog::Search {
                query: FieldView {
                    text: "needle",
                    cursor: "needle".len(),
                    selection: None,
                },
                replacement: Some(FieldView {
                    text: "repl",
                    cursor: "repl".len(),
                    selection: None,
                }),
                replacement_focus: true,
            }),
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Ascii,
        };
        let (term_small, _) = render_screen(39, 10, &search_hidden);
        let small_text: String = (0..10)
            .flat_map(|y| (0..39).map(move |x| (x, y)))
            .map(|(x, y)| term_small.backend().buffer()[(x, y)].symbol())
            .collect();
        assert!(!small_text.contains("needle"));
        assert!(!small_text.contains("repl"));

        let (mut term_ok_repl, _) = render_screen(40, 10, &search_hidden);
        let pos_repl = term_ok_repl.backend_mut().get_cursor_position().unwrap();
        let search_find_focus = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "a",
            status: "b",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: Some(Dialog::Search {
                query: FieldView {
                    text: "needle\nline2",
                    cursor: "needle\nline2".len(),
                    selection: None,
                },
                replacement: Some(FieldView {
                    text: "repl",
                    cursor: "repl".len(),
                    selection: None,
                }),
                replacement_focus: false,
            }),
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Ascii,
        };
        let (mut term_ok_find, _) = render_screen(40, 10, &search_find_focus);
        let pos_find = term_ok_find.backend_mut().get_cursor_position().unwrap();
        assert_eq!(pos_repl.y, pos_find.y + 1);

        let full_text: String = (0..10)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .map(|(x, y)| term_ok_find.backend().buffer()[(x, y)].symbol())
            .collect();
        assert!(full_text.contains("needle\\nline2"));
        assert!(full_text.contains("repl"));
    }

    #[test]
    fn test_multi_pane_clipping_hits_shared_highlights_focus_and_cursor() {
        // Shared doc has 30 lines; pane 1 views top (lines 0..), pane 2 views bottom (lines 20..)
        let mut text = String::from(
            "fn alpha_long_content_never_bleeds_across_pane_divider_1234567890()\nplain1\n",
        );
        for i in 2..20 {
            text.push_str(&format!("middle_{i}\n"));
        }
        text.push_str("let beta = 42;\n");
        for i in 21..30 {
            text.push_str(&format!("tail_{i}\n"));
        }
        let shared_doc = Document::new(&text);
        let other_doc = Document::new("漢字🦀pane3_line0\nsecond_line");
        let line20_start = shared_doc.line_start(20);

        // Union of visible lines (0..1 and 20..21) shared by pane 1 and pane 2
        let shared_hl = [
            LineTokens {
                line: 0,
                spans: vec![TokenSpan {
                    chars: 0..2,
                    token: Token::Keyword,
                }],
            },
            LineTokens {
                line: 20,
                spans: vec![
                    TokenSpan {
                        chars: line20_start..line20_start + 3,
                        token: Token::Keyword,
                    },
                    TokenSpan {
                        chars: line20_start + 11..line20_start + 13,
                        token: Token::Number,
                    },
                ],
            },
        ];

        let mut tree = SplitTree::new(1);
        assert!(tree.split(1, 2, Axis::Columns).is_some());
        assert!(tree.split(2, 3, Axis::Rows).is_some());

        let tabs1 = [TabCaption {
            id: 11,
            label: "shared.rs",
            dirty: true,
            read_only: false,
            active: true,
        }];
        let tabs2 = [
            TabCaption {
                id: 21,
                label: "other.txt",
                dirty: false,
                read_only: false,
                active: false,
            },
            TabCaption {
                id: 22,
                label: "shared.rs",
                dirty: true,
                read_only: false,
                active: true,
            },
        ];
        let tabs3 = [TabCaption {
            id: 31,
            label: "wide.txt",
            dirty: false,
            read_only: true,
            active: true,
        }];

        let panes = [
            PaneScreen {
                id: 1,
                document_id: 100,
                document: &shared_doc,
                cursor: 1,             // inside "fn", line 1 col 2
                selection: Some(0..1), // 'f' selected -> selection wins over Keyword
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &tabs1,
                highlights: &shared_hl,
            },
            PaneScreen {
                id: 2,
                document_id: 100,
                document: &shared_doc,
                cursor: line20_start + 4,
                selection: None,
                top_line: 20,
                left_cell: 0,
                read_only: false,
                tabs: &tabs2,
                highlights: &shared_hl,
            },
            PaneScreen {
                id: 3,
                document_id: 200,
                document: &other_doc,
                cursor: other_doc.line_start(1) + 2, // line 2, col 3
                selection: None,
                top_line: 0,
                left_cell: 1, // clips first half of '漢'
                read_only: true,
                tabs: &tabs3,
                highlights: &[],
            },
        ];

        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let screen_p1 = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/repo",
            status: "Ready",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let (mut term1, geom1) = render_screen(80, 20, &screen_p1);
        assert!(!geom1.collapsed);
        assert_eq!(geom1.panes.len(), 3);
        assert_eq!(geom1.splits.len(), 2);

        let pg1 = geom1.pane(1).unwrap();
        let pg2 = geom1.pane(2).unwrap();
        let pg3 = geom1.pane(3).unwrap();
        assert!(!rects_overlap(pg1.area, pg2.area));
        assert!(!rects_overlap(pg1.area, pg3.area));
        assert!(!rects_overlap(pg2.area, pg3.area));
        for div in &geom1.splits {
            assert!(!rects_overlap(div.area, pg1.area));
            assert!(!rects_overlap(div.area, pg2.area));
            assert!(!rects_overlap(div.area, pg3.area));
        }
        assert_eq!(pg1.tabs[0].id, 11);
        assert_eq!(
            pg2.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![21, 22]
        );
        assert_eq!(pg3.tabs[0].id, 31);

        // Hardware cursor in pane 1 at (x0 + 1, y0)
        let cur1 = term1.backend_mut().get_cursor_position().unwrap();
        assert_eq!(cur1, Position::new(pg1.text_area.x + 1, pg1.text_area.y));

        let buf1 = term1.backend().buffer();
        // Selection wins on char 0 ('f'), Keyword color applies on char 1 ('n')
        assert_eq!(buf1[(pg1.text_area.x, pg1.text_area.y)].bg, theme.selection);
        assert_eq!(
            buf1[(pg1.text_area.x + 1, pg1.text_area.y)].fg,
            theme.syntax(Token::Keyword)
        );
        // Pane 2 renders line 20 with shared highlight ("let" Keyword, "42" Number)
        assert_eq!(
            buf1[(pg2.text_area.x, pg2.text_area.y)].fg,
            theme.syntax(Token::Keyword)
        );
        assert_eq!(
            buf1[(pg2.text_area.x + 11, pg2.text_area.y)].fg,
            theme.syntax(Token::Number)
        );
        // Pane 3 clips straddling left wide char '漢' -> space
        assert_eq!(buf1[(pg3.text_area.x, pg3.text_area.y)].symbol(), " ");
        // Long content never bleeds across the card boundary.
        assert!(!row_text(buf1, pg2.text_area, 0).contains("1234567890"));

        // Switch active pane: hardware cursor follows pane 3 and dividers use ASCII.
        let screen_p3 = Screen {
            active_pane: 3,
            icons: IconMode::Ascii,
            ..screen_p1
        };
        let (mut term3, geom3) = render_screen(80, 20, &screen_p3);
        let pg3_new = geom3.pane(3).unwrap();
        let cur3 = term3.backend_mut().get_cursor_position().unwrap();
        assert_eq!(
            cur3,
            Position::new(pg3_new.text_area.x + 1, pg3_new.text_area.y + 1)
        );
        let buf3 = term3.backend().buffer();
        let col_div3 = geom3
            .splits
            .iter()
            .find(|d| d.axis == Axis::Columns)
            .unwrap();
        let row_div3 = geom3.splits.iter().find(|d| d.axis == Axis::Rows).unwrap();
        assert!(
            buf3[(col_div3.area.x, col_div3.area.y + col_div3.area.height / 2)]
                .symbol()
                .is_ascii()
        );
        assert!(
            buf3[(row_div3.area.x + row_div3.area.width / 2, row_div3.area.y)]
                .symbol()
                .is_ascii()
        );
    }

    #[test]
    fn test_multi_pane_non_color_focus_distinction_at_narrow_pane_width() {
        let doc = Document::new("alpha\nbeta");
        let theme_none = Theme::new(ThemeKind::Dark, ColorMode::None);
        let mut tree = SplitTree::new(1);
        assert!(tree.split(1, 2, Axis::Columns).is_some());

        let tabs1 = [TabCaption {
            id: 10,
            label: "a.rs",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let tabs2 = [TabCaption {
            id: 20,
            label: "b.rs",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let panes = [
            PaneScreen {
                id: 1,
                document_id: 1,
                document: &doc,
                cursor: 0,
                selection: None,
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &tabs1,
                highlights: &[],
            },
            PaneScreen {
                id: 2,
                document_id: 2,
                document: &doc,
                cursor: 0,
                selection: None,
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &tabs2,
                highlights: &[],
            },
        ];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme_none,
            icons: IconMode::Ascii,
        };
        // Narrow panes must retain a non-color distinction between focused/unfocused tabs.
        let (term, geom) = render_screen(41, 10, &screen);
        let pg1 = geom.pane(1).unwrap();
        let pg2 = geom.pane(2).unwrap();
        assert!(pg1.area.width < 30 && pg2.area.width < 30);

        let buf = term.backend().buffer();
        assert!(
            buf[(pg1.area.x, pg1.area.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            !buf[(pg2.area.x, pg2.area.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn test_narrow_sidebar_hide_collapse_active_switch_and_restore() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("file.rs"), "fn main() {}").expect("write");
        let explorer = Explorer::new(dir.path().to_path_buf()).expect("explorer");
        let doc1 = Document::new("pane1 text");
        let doc2 = Document::new("pane2 text");
        let doc3 = Document::new("pane3 text");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);

        let mut tree = SplitTree::new(1);
        assert!(tree.split(1, 2, Axis::Columns).is_some());
        let tabs1 = [TabCaption {
            id: 1,
            label: "one.rs",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let tabs2 = [TabCaption {
            id: 2,
            label: "two.rs",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let tabs3 = [TabCaption {
            id: 3,
            label: "three.rs",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let panes2 = [
            PaneScreen {
                id: 1,
                document_id: 1,
                document: &doc1,
                cursor: 0,
                selection: None,
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &tabs1,
                highlights: &[],
            },
            PaneScreen {
                id: 2,
                document_id: 2,
                document: &doc2,
                cursor: 0,
                selection: None,
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &tabs2,
                highlights: &[],
            },
        ];

        let mut geom = Geometry::default();
        let mut screen = Screen {
            panes: &panes2,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: Some(&explorer),
            sidebar_width: 24,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };

        // 1. Wide (60x16): sidebar + 2-pane full tree
        let _ = render_into(60, 16, &screen, &mut geom);
        assert!(geom.sidebar.is_some());
        assert!(geom.sidebar_panel.is_some());
        assert!(geom.explorer_divider.is_some());
        assert!(!geom.collapsed);
        assert_eq!(geom.panes.len(), 2);
        assert_eq!(geom.splits.len(), 1);

        // 2. Narrow for sidebar+tree (30 < 18+1+21=40), but full tree fits (30 >= 21): hide sidebar first
        let _ = render_into(30, 16, &screen, &mut geom);
        assert_eq!(geom.sidebar, None);
        assert_eq!(geom.sidebar_panel, None);
        assert_eq!(geom.explorer_divider, None);
        assert!(!geom.collapsed);
        assert_eq!(geom.panes.len(), 2);
        assert_eq!(geom.splits.len(), 1);

        // 3. Too narrow even for full tree without sidebar (18 < 21): collapse to active pane 1
        let term_c1 = render_into(18, 16, &screen, &mut geom);
        assert!(geom.collapsed);
        assert_eq!(geom.sidebar, None);
        assert_eq!(geom.sidebar_panel, None);
        assert_eq!(geom.panes.len(), 1);
        assert_eq!(geom.panes[0].id, 1);
        assert!(geom.splits.is_empty());
        let text_c1 = row_text(term_c1.backend().buffer(), geom.panes[0].text_area, 0);
        assert!(text_c1.contains("pane1 text"));

        // 4. Focus hidden pane 2 while collapsed -> reveals pane 2
        screen.active_pane = 2;
        let term_c2 = render_into(18, 16, &screen, &mut geom);
        assert!(geom.collapsed);
        assert_eq!(geom.sidebar, None);
        assert_eq!(geom.sidebar_panel, None);
        assert_eq!(geom.panes[0].id, 2);
        let text_c2 = row_text(term_c2.backend().buffer(), geom.panes[0].text_area, 0);
        assert!(text_c2.contains("pane2 text"));

        // 5. Restore to 60x16 -> full tree + sidebar return cleanly on reused Geometry
        let _ = render_into(60, 16, &screen, &mut geom);
        assert!(geom.sidebar.is_some());
        assert!(geom.sidebar_panel.is_some());
        assert!(!geom.collapsed);
        assert_eq!(geom.panes.len(), 2);
        assert_eq!(geom.splits.len(), 1);

        // 6. Collapse WITH sidebar kept when 3-column tree doesn't fit at width 32,
        //    but sidebar (18) + divider (1) + active pane (min 12) = 31 <= 32 does fit
        assert!(tree.split(2, 3, Axis::Columns).is_some());
        let panes3 = [
            panes2[0].clone_for_test(),
            panes2[1].clone_for_test(),
            PaneScreen {
                id: 3,
                document_id: 3,
                document: &doc3,
                cursor: 0,
                selection: None,
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &tabs3,
                highlights: &[],
            },
        ];
        let screen3 = Screen {
            panes: &panes3,
            active_pane: 3,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: Some(&explorer),
            sidebar_width: 18,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let _ = render_into(32, 16, &screen3, &mut geom);
        assert!(geom.collapsed);
        assert!(geom.sidebar.is_some());
        assert!(geom.sidebar_panel.is_some());
        assert_eq!(geom.panes.len(), 1);
        assert_eq!(geom.panes[0].id, 3);
    }

    impl<'a> PaneScreen<'a> {
        fn clone_for_test(&self) -> Self {
            Self {
                id: self.id,
                document_id: self.document_id,
                document: self.document,
                cursor: self.cursor,
                selection: self.selection.clone(),
                top_line: self.top_line,
                left_cell: self.left_cell,
                read_only: self.read_only,
                tabs: self.tabs,
                highlights: self.highlights,
            }
        }
    }

    #[test]
    fn test_geometry_drops_removed_pane_and_tab_hit_targets() {
        let doc = Document::new("hello");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let two_tabs = [
            TabCaption {
                id: 101,
                label: "a.rs",
                dirty: false,
                read_only: false,
                active: true,
            },
            TabCaption {
                id: 102,
                label: "b.rs",
                dirty: true,
                read_only: false,
                active: false,
            },
        ];
        let one_tab = [TabCaption {
            id: 303,
            label: "c.rs",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let mut tree = SplitTree::new(1);
        assert!(tree.split(1, 2, Axis::Rows).is_some());
        let panes2 = [
            PaneScreen {
                id: 1,
                document_id: 1,
                document: &doc,
                cursor: 0,
                selection: None,
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &two_tabs,
                highlights: &[],
            },
            PaneScreen {
                id: 2,
                document_id: 1,
                document: &doc,
                cursor: 0,
                selection: None,
                top_line: 0,
                left_cell: 0,
                read_only: false,
                tabs: &one_tab,
                highlights: &[],
            },
        ];
        let mut geom = Geometry::default();
        let screen2 = Screen {
            panes: &panes2,
            active_pane: 1,
            tree: &tree,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let _ = render_into(60, 14, &screen2, &mut geom);
        assert_eq!(geom.panes.len(), 2);
        assert_eq!(geom.pane(1).unwrap().tabs.len(), 2);
        assert_eq!(geom.pane(2).unwrap().tabs.len(), 1);

        let tree1 = SplitTree::new(1);
        let panes1 = [PaneScreen {
            id: 1,
            document_id: 1,
            document: &doc,
            cursor: 0,
            selection: None,
            top_line: 0,
            left_cell: 0,
            read_only: false,
            tabs: &one_tab,
            highlights: &[],
        }];
        let screen1 = Screen {
            panes: &panes1,
            active_pane: 1,
            tree: &tree1,
            title: "/ws",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let _ = render_into(60, 14, &screen1, &mut geom);
        assert_eq!(geom.panes.len(), 1);
        assert!(geom.pane(2).is_none());
        assert_eq!(geom.pane(1).unwrap().tabs.len(), 1);
        assert_eq!(geom.pane(1).unwrap().tabs[0].id, 303);
        assert!(geom.splits.is_empty());
    }
    #[test]
    fn explorer_divider_uses_the_configured_canvas_instead_of_terminal_default() {
        let dir = TempDir::new().unwrap();
        let explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        let doc = Document::new("content");
        let tabs = base_tabs();
        let panes = [single_pane(&doc, &tabs, &[])];
        let tree = SplitTree::new(1);
        for kind in [ThemeKind::Dark, ThemeKind::Light] {
            let theme = Theme::new(kind, ColorMode::Truecolor);
            let screen = Screen {
                panes: &panes,
                active_pane: 1,
                tree: &tree,
                title: "workspace",
                status: "ready",
                active_path: None,
                format: FileFormat::default(),
                dirty: false,
                dialog: None,
                explorer: Some(&explorer),
                sidebar_width: 28,
                explorer_focused: false,
                theme: &theme,
                icons: IconMode::Unicode,
            };
            let (terminal, geometry) = render_screen(100, 28, &screen);
            let buffer = terminal.backend().buffer();
            let gap = geometry.explorer_divider.unwrap();
            for y in gap.top()..gap.bottom() {
                assert_eq!(buffer[(gap.x, y)].bg, theme.canvas);
            }
        }
    }

    #[test]
    fn test_card_containment_and_border_exclusion() {
        let doc = Document::new("fn main() {\n    println!(\"hello\");\n}\n");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tabs = [TabCaption {
            id: 42,
            label: "main.rs",
            dirty: false,
            read_only: false,
            active: true,
        }];
        let tree = SplitTree::new(1);
        let panes = [single_pane(&doc, &tabs, &[])];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/workspace",
            status: "ready",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let (_, geom) = render_screen(60, 20, &screen);
        let pg = geom.pane(1).expect("pane 1");

        assert_eq!(pg.area.x, 0);
        assert!(!pg.area.is_empty());

        let inner = chrome::inner(pg.area);

        assert!(pg.text_area.x >= inner.x);
        assert!(pg.text_area.right() <= inner.right());
        assert!(pg.text_area.y >= inner.y);
        assert!(pg.text_area.bottom() <= inner.bottom());

        assert!(pg.text_area.x > pg.area.x);
        assert!(pg.text_area.right() < pg.area.right());
        assert!(pg.text_area.y > pg.area.y);
        assert!(pg.text_area.bottom() < pg.area.bottom());

        for hit in &pg.tabs {
            assert!(hit.body.x >= inner.x);
            assert!(hit.close.right() <= inner.right());
            assert_eq!(hit.body.y, inner.y);
            assert_eq!(hit.close.y, inner.y);
            assert!(hit.body.x > pg.area.x);
            assert!(hit.close.right() < pg.area.right());
        }
    }

    #[test]
    fn test_sidebar_panel_and_rows_geometry() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("a.txt"), "hello").expect("write");
        let explorer = Explorer::new(dir.path().to_path_buf()).expect("explorer");
        let doc = Document::new("test");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let panes = [single_pane(&doc, &tabs, &[])];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "repo",
            status: "ok",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: Some(&explorer),
            sidebar_width: 20,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };
        let (_, geom) = render_screen(80, 24, &screen);
        let panel = geom.sidebar_panel.expect("sidebar_panel visible");
        let rows = geom.sidebar.expect("sidebar rows visible");
        let div = geom.explorer_divider.expect("divider visible");
        let pg = geom.pane(1).expect("pane 1");

        assert_eq!(panel.width, 20);
        assert_eq!(rows.width, panel.width - 2);
        let parent = geom.sidebar_parent.expect("sidebar_parent visible");
        assert_eq!(parent.width, panel.width - 2);
        assert_eq!(parent.x, panel.x + 1);
        assert_eq!(parent.y, panel.y + 1);
        assert_eq!(parent.height, 1);
        assert_eq!(rows.x, panel.x + 1);
        assert_eq!(rows.y, parent.bottom());
        assert_eq!(rows.height, panel.height - 3);

        assert_eq!(div.x, panel.right());
        assert_eq!(div.width, 1);

        assert_eq!(pg.area.x, div.right());
    }

    #[test]
    fn test_rich_vs_compact_chrome_regions() {
        let doc = Document::new("hello\nworld\n");
        let theme = Theme::new(ThemeKind::Dark, ColorMode::Truecolor);
        let tabs = base_tabs();
        let tree = SplitTree::new(1);
        let panes = [single_pane(&doc, &tabs, &[])];
        let screen = Screen {
            panes: &panes,
            active_pane: 1,
            tree: &tree,
            title: "/repo",
            status: "Ln 1, Col 1",
            active_path: None,
            format: FileFormat::default(),
            dirty: false,
            dialog: None,
            explorer: None,
            sidebar_width: 0,
            explorer_focused: false,
            theme: &theme,
            icons: IconMode::Unicode,
        };

        // Rich framed mode: 80x24
        let (_, geom_rich) = render_screen(80, 24, &screen);
        assert!(!geom_rich.commands.is_empty());
        for cmd in &geom_rich.commands {
            assert!(cmd.area.y >= 20);
            assert!(!cmd.area.is_empty());
        }

        // Compact mode: 38x16 (width < 40 and height < 18)
        let (_, geom_compact) = render_screen(38, 16, &screen);
        let pg = geom_compact.pane(1).expect("pane 1 in compact");
        assert_eq!(pg.area.y, 1); // 1-row header
        assert!(!pg.text_area.is_empty());
    }
}

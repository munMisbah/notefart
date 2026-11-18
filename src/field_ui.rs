use std::borrow::Cow;
use std::fmt::Write as _;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::field::FieldView;
use crate::theme::{IconMode, Theme};
use crate::ui::{FieldHit, sanitize_ui_cow, write_clipped_str};

fn truncation_marker(icons: IconMode) -> &'static str {
    match icons {
        IconMode::Ascii => ".",
        IconMode::Unicode | IconMode::Nerd => "…",
    }
}

fn sanitize_grapheme<'a>(grapheme: &'a str) -> (Cow<'a, str>, usize) {
    let sanitized = sanitize_ui_cow(grapheme);
    let width = UnicodeWidthStr::width(sanitized.as_ref());
    if width > 0 {
        (sanitized, width)
    } else {
        // Standalone zero-width cluster: must be visible escape!
        let mut out = String::new();
        for ch in grapheme.chars() {
            let _ = write!(out, "\\u{{{:04x}}}", ch as u32);
        }
        let w = UnicodeWidthStr::width(out.as_str());
        (Cow::Owned(out), w)
    }
}

pub(crate) fn draw(
    buf: &mut Buffer,
    area: Rect,
    prefix: &str,
    value: &FieldView<'_>,
    focused: bool,
    theme: &Theme,
    icons: IconMode,
    hit: &mut FieldHit,
) -> Option<(u16, u16)> {
    hit.stops.clear();
    hit.area = Rect::default();

    if area.width == 0 || area.height == 0 {
        return None;
    }

    if area.x >= buf.area.right() || area.y >= buf.area.bottom() {
        return None;
    }

    let avail_width = area.width.min(buf.area.right().saturating_sub(area.x));
    if avail_width == 0 {
        return None;
    }

    let val_style = if focused {
        Style::default().bg(theme.canvas).fg(theme.fg)
    } else {
        Style::default().bg(theme.canvas).fg(theme.muted)
    };
    let prefix_style = if focused {
        theme.focus_style()
    } else {
        Style::default().bg(theme.canvas).fg(theme.muted)
    };

    let written_prefix = write_clipped_str(
        buf,
        area.x,
        area.y,
        avail_width as usize,
        prefix,
        prefix_style,
    );

    if written_prefix >= avail_width {
        return None;
    }

    let value_x = area.x + written_prefix;
    let value_width = avail_width - written_prefix;

    let sel_range = value.selection.as_ref().and_then(|r| {
        let start = r.start.min(r.end);
        let end = r.start.max(r.end);
        if start < end { Some(start..end) } else { None }
    });
    let selection_style = theme.selection_style();

    let cursor_byte = value.cursor.min(value.text.len());
    let mut total_width = 0usize;
    let mut cursor_col = 0usize;
    let mut cursor_col_found = cursor_byte == 0;

    for (byte_idx, grapheme) in value.text.grapheme_indices(true) {
        if !cursor_col_found && byte_idx >= cursor_byte {
            cursor_col = total_width;
            cursor_col_found = true;
        }
        let (_, gw) = sanitize_grapheme(grapheme);
        total_width += gw;
    }
    if !cursor_col_found {
        cursor_col = total_width;
    }

    let window_start = |width: usize| {
        cursor_col
            .saturating_sub(width / 2)
            .min((total_width + 1).saturating_sub(width))
    };
    let full_width = usize::from(value_width);
    let full_start = window_start(full_width);
    let gutter =
        u16::from(value_width >= 3 && (full_start > 0 || total_width > full_start + full_width));
    let val_x = value_x + gutter;
    let val_width = value_width - 2 * gutter;
    let w = usize::from(val_width);
    let window_col_start = window_start(w);
    let window_col_end = window_col_start + w;
    hit.area = Rect::new(val_x, area.y, val_width, 1);
    hit.stops.reserve(w);

    if gutter != 0 {
        let (left, right) = match icons {
            IconMode::Ascii => ("<", ">"),
            IconMode::Unicode | IconMode::Nerd => ("‹", "›"),
        };
        let indicator_style = if focused {
            val_style.fg(theme.accent)
        } else {
            val_style
        };
        buf[(value_x, area.y)]
            .set_symbol(if window_col_start > 0 { left } else { " " })
            .set_style(indicator_style);
        buf[(value_x + value_width - 1, area.y)]
            .set_symbol(if total_width > window_col_end {
                right
            } else {
                " "
            })
            .set_style(indicator_style);
    }

    let marker = truncation_marker(icons);
    let mut cur_col = 0usize;

    for (byte_idx, grapheme) in value.text.grapheme_indices(true) {
        let (sanitized, gw) = sanitize_grapheme(grapheme);
        let next_col = cur_col + gw;

        if next_col <= window_col_start {
            cur_col = next_col;
            continue;
        }
        if cur_col >= window_col_end {
            break;
        }

        let is_selected = sel_range
            .as_ref()
            .is_some_and(|r| r.start < byte_idx + grapheme.len() && r.end > byte_idx);
        let style = if is_selected {
            selection_style
        } else {
            val_style
        };

        let visible_start = cur_col.max(window_col_start);
        let visible_end = next_col.min(window_col_end);
        if sanitized.is_ascii() && sanitized.len() > 1 {
            // Escapes are printable ASCII spans, not multi-cell Unicode graphemes.
            for col in visible_start..visible_end {
                let sx = val_x + (col - window_col_start) as u16;
                let offset = col - cur_col;
                buf[(sx, area.y)]
                    .set_symbol(&sanitized.as_ref()[offset..offset + 1])
                    .set_style(style);
                hit.stops.push((sx, byte_idx));
            }
        } else {
            let complete = cur_col >= window_col_start && next_col <= window_col_end;
            for col in visible_start..visible_end {
                let sx = val_x + (col - window_col_start) as u16;
                if complete && col == cur_col {
                    buf[(sx, area.y)]
                        .set_symbol(sanitized.as_ref())
                        .set_style(style);
                } else if complete {
                    buf[(sx, area.y)].reset();
                    buf[(sx, area.y)].set_style(style);
                } else {
                    buf[(sx, area.y)].set_symbol(marker).set_style(style);
                }
                hit.stops.push((sx, byte_idx));
            }
        }

        cur_col = next_col;
    }

    let blank_start = cur_col.max(window_col_start);
    for col in blank_start..window_col_end {
        let sx = val_x + (col - window_col_start) as u16;
        buf[(sx, area.y)].set_symbol(" ").set_style(val_style);
        hit.stops.push((sx, value.text.len()));
    }

    if focused && cursor_col >= window_col_start && cursor_col < window_col_end {
        let cur_x = val_x + (cursor_col - window_col_start) as u16;
        Some((cur_x, area.y))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{FieldKind, FieldView};
    use crate::theme::{ColorMode, ThemeKind};

    fn make_test_theme() -> Theme {
        Theme::new(ThemeKind::Dark, ColorMode::None)
    }

    #[test]
    fn test_zero_width_area_returns_none() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let theme = make_test_theme();
        let mut hit = FieldHit {
            kind: FieldKind::Query,
            area: Rect::default(),
            stops: Vec::new(),
        };
        let view = FieldView {
            text: "hello",
            cursor: 0,
            selection: None,
        };
        let res = draw(
            &mut buf,
            Rect::new(0, 0, 0, 1),
            "> ",
            &view,
            true,
            &theme,
            IconMode::Ascii,
            &mut hit,
        );
        assert!(res.is_none());
        assert_eq!(hit.area, Rect::default());
        assert!(hit.stops.is_empty());
    }

    #[test]
    fn test_empty_field_maps_stops_and_cursor() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let theme = make_test_theme();
        let mut hit = FieldHit {
            kind: FieldKind::Path,
            area: Rect::default(),
            stops: Vec::new(),
        };
        let view = FieldView {
            text: "",
            cursor: 0,
            selection: None,
        };
        let res = draw(
            &mut buf,
            Rect::new(0, 0, 10, 1),
            ">",
            &view,
            true,
            &theme,
            IconMode::Ascii,
            &mut hit,
        );
        assert_eq!(res, Some((1, 0)));
        assert_eq!(hit.area, Rect::new(1, 0, 9, 1));
        assert_eq!(hit.stops.len(), 9);
        for &(x, byte) in &hit.stops {
            assert_eq!(byte, 0);
            assert!(x >= 1 && x < 10);
        }
    }

    #[test]
    fn three_cell_unicode_remains_intact_and_clips_without_controls() {
        let glyph = "៘";
        let glyph_width = UnicodeWidthStr::width(glyph);
        let text = "៘x";
        let theme = make_test_theme();
        for width in [1u16, 2, 8] {
            let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
            let mut hit = FieldHit {
                kind: FieldKind::Query,
                area: Rect::default(),
                stops: Vec::new(),
            };
            let value = FieldView {
                text,
                cursor: 0,
                selection: None,
            };
            draw(
                &mut buf,
                Rect::new(0, 0, width, 1),
                "",
                &value,
                true,
                &theme,
                IconMode::Ascii,
                &mut hit,
            );
            for column in 0..glyph_width.min(width as usize) {
                assert_eq!(hit.stops[column], (column as u16, 0));
            }
            assert!(
                buf.content
                    .iter()
                    .all(|cell| !cell.symbol().chars().any(char::is_control))
            );
            if width as usize > glyph_width {
                assert_eq!(buf[(0, 0)].symbol(), glyph);
                assert_eq!(buf[(glyph_width as u16, 0)].symbol(), "x");
                assert_eq!(hit.stops[glyph_width].1, glyph.len());
            }
        }
    }

    #[test]
    fn test_wide_graphemes_halves_mapping() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let theme = make_test_theme();
        let mut hit = FieldHit {
            kind: FieldKind::Query,
            area: Rect::default(),
            stops: Vec::new(),
        };
        // "你好" is two 2-width characters = 4 columns
        let view = FieldView {
            text: "你好",
            cursor: 0,
            selection: None,
        };
        let res = draw(
            &mut buf,
            Rect::new(0, 0, 10, 1),
            "",
            &view,
            true,
            &theme,
            IconMode::Unicode,
            &mut hit,
        );
        assert_eq!(res, Some((0, 0)));
        assert_eq!(hit.area, Rect::new(0, 0, 10, 1));
        // Cell 0 and 1 belong to 你 (byte 0)
        assert_eq!(hit.stops[0], (0, 0));
        assert_eq!(hit.stops[1], (1, 0));
        // Cell 2 and 3 belong to 好 (byte 3)
        assert_eq!(hit.stops[2], (2, 3));
        assert_eq!(hit.stops[3], (3, 3));
        // Remaining cells 4..10 map to text.len() (6)
        for i in 4..10 {
            assert_eq!(hit.stops[i], (i as u16, 6));
        }
    }

    #[test]
    fn test_control_escape_cells_mapping() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let theme = make_test_theme();
        let mut hit = FieldHit {
            kind: FieldKind::Query,
            area: Rect::default(),
            stops: Vec::new(),
        };
        // "\t" escapes to "\\t" (2 cells)
        let view = FieldView {
            text: "\t",
            cursor: 0,
            selection: None,
        };
        let res = draw(
            &mut buf,
            Rect::new(0, 0, 10, 1),
            "",
            &view,
            true,
            &theme,
            IconMode::Ascii,
            &mut hit,
        );
        assert_eq!(res, Some((0, 0)));
        // Both escape cells (0 and 1) map to byte 0
        assert_eq!(hit.stops[0], (0, 0));
        assert_eq!(hit.stops[1], (1, 0));
    }

    #[test]
    fn test_horizontal_window_caret_following() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        let theme = make_test_theme();
        let mut hit = FieldHit {
            kind: FieldKind::Query,
            area: Rect::default(),
            stops: Vec::new(),
        };
        // Text is 10 chars "0123456789". Available width is 6.
        let view_home = FieldView {
            text: "0123456789",
            cursor: 0,
            selection: None,
        };
        let res_home = draw(
            &mut buf,
            Rect::new(0, 0, 6, 1),
            "",
            &view_home,
            true,
            &theme,
            IconMode::Ascii,
            &mut hit,
        );
        // Home exposes byte zero at the first text cell, after any passive gutter.
        assert_eq!(res_home, Some((hit.area.x, hit.area.y)));
        assert_eq!(hit.stops.first(), Some(&(hit.area.x, 0)));

        // End tail: cursor at byte 10
        let view_end = FieldView {
            text: "0123456789",
            cursor: 10,
            selection: None,
        };
        let res_end = draw(
            &mut buf,
            Rect::new(0, 0, 6, 1),
            "",
            &view_end,
            true,
            &theme,
            IconMode::Ascii,
            &mut hit,
        );
        // End exposes the insertion position in text, not in the passive right gutter.
        let caret_stop = hit
            .stops
            .last()
            .copied()
            .expect("End must expose its caret");
        assert_eq!(res_end, Some((caret_stop.0, hit.area.y)));
        assert_eq!(caret_stop.1, view_end.text.len());
    }

    #[test]
    fn overflow_indicators_follow_caret_and_exclude_decorations_from_hits() {
        let theme = make_test_theme();
        let text = "0123456789abcdef";
        let area = Rect::new(2, 1, 10, 1);
        let left = area.x + 2;
        let right = area.right() - 1;

        for (icons, left_symbol, right_symbol) in [
            (IconMode::Ascii, "<", ">"),
            (IconMode::Unicode, "‹", "›"),
            (IconMode::Nerd, "‹", "›"),
        ] {
            for cursor in [0, 8, text.len()] {
                let mut buf = Buffer::empty(Rect::new(0, 0, 14, 3));
                let mut hit = FieldHit {
                    kind: FieldKind::Query,
                    area: Rect::default(),
                    stops: Vec::new(),
                };
                let value = FieldView {
                    text,
                    cursor,
                    selection: Some(2..14),
                };
                let (cursor_x, cursor_y) =
                    draw(&mut buf, area, "> ", &value, true, &theme, icons, &mut hit)
                        .expect("the focused caret must remain visible");

                assert_eq!(
                    buf[(left, area.y)].symbol(),
                    if cursor == 0 { " " } else { left_symbol }
                );
                assert_eq!(
                    buf[(right, area.y)].symbol(),
                    if cursor == text.len() {
                        " "
                    } else {
                        right_symbol
                    }
                );
                assert!(hit.area.x > left && hit.area.right() <= right);
                assert_eq!(cursor_y, area.y);
                assert!(cursor_x >= hit.area.x && cursor_x < hit.area.right());
                assert_eq!(
                    hit.stops
                        .iter()
                        .find_map(|&(x, byte)| (x == cursor_x).then_some(byte)),
                    Some(cursor)
                );
                for &(x, byte) in &hit.stops {
                    assert!(x > left && x < right);
                    let expected = if byte == text.len() {
                        " "
                    } else {
                        &text[byte..byte + 1]
                    };
                    assert_eq!(buf[(x, area.y)].symbol(), expected);
                }
            }
        }
    }

    #[test]
    fn overflow_compact_fields_keep_caret_and_grapheme_boundaries() {
        let theme = make_test_theme();
        let text = "a你e\u{0301}🙂z";
        let boundaries: Vec<_> = text
            .grapheme_indices(true)
            .map(|(byte, _)| byte)
            .chain(std::iter::once(text.len()))
            .collect();

        for width in 1..=3 {
            for &cursor in &boundaries {
                for focused in [false, true] {
                    let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
                    let mut hit = FieldHit {
                        kind: FieldKind::Path,
                        area: Rect::default(),
                        stops: Vec::new(),
                    };
                    let value = FieldView {
                        text,
                        cursor,
                        selection: Some(1..text.len() - 1),
                    };
                    let result = draw(
                        &mut buf,
                        Rect::new(0, 0, width, 1),
                        "",
                        &value,
                        focused,
                        &theme,
                        IconMode::Ascii,
                        &mut hit,
                    );

                    assert!(hit.stops.iter().all(|&(x, byte)| {
                        x >= hit.area.x && x < hit.area.right() && boundaries.contains(&byte)
                    }));
                    if focused {
                        let (cursor_x, _) = result.expect("compact fields must keep their caret");
                        assert_eq!(
                            hit.stops
                                .iter()
                                .find_map(|&(x, byte)| (x == cursor_x).then_some(byte)),
                            Some(cursor)
                        );
                    } else {
                        assert_eq!(result, None);
                    }
                }
            }
        }
    }

    #[test]
    fn test_unfocused_returns_none_cursor() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let theme = make_test_theme();
        let mut hit = FieldHit {
            kind: FieldKind::Path,
            area: Rect::default(),
            stops: Vec::new(),
        };
        let view = FieldView {
            text: "test",
            cursor: 0,
            selection: None,
        };
        let res = draw(
            &mut buf,
            Rect::new(0, 0, 10, 1),
            "> ",
            &view,
            false,
            &theme,
            IconMode::Ascii,
            &mut hit,
        );
        assert_eq!(res, None);
        assert_eq!(hit.area, Rect::new(2, 0, 8, 1));
    }
}

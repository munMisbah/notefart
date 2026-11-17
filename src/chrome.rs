use ratatui::layout::Rect;

pub(crate) const MIN_EDITOR_WIDTH: u16 = 12;
pub(crate) const MIN_PANE_HEIGHT: u16 = 4;
pub(crate) const MIN_SIDEBAR_WIDTH: u16 = 18;
pub(crate) const MAX_SIDEBAR_WIDTH: u16 = 40;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Regions {
    pub header: Rect,
    pub body: Rect,
    pub dock: Rect,
    pub status: Rect,
    pub helper: Option<Rect>,
    pub framed: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PaneAreas {
    pub tabs: Rect,
    pub gutter: Rect,
    pub text: Rect,
    pub vertical_scrollbar: Option<Rect>,
    pub horizontal_scrollbar: Option<Rect>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SidebarAreas {
    pub panel: Option<Rect>,
    pub parent: Option<Rect>,
    pub rows: Option<Rect>,
    pub scrollbar: Option<Rect>,
    pub divider: Option<Rect>,
    pub editor: Rect,
}

fn bounded(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y,
        area.width.min(u16::MAX - area.x),
        area.height.min(u16::MAX - area.y),
    )
}

pub(crate) fn inner(area: Rect) -> Rect {
    let area = bounded(area);
    Rect::new(
        area.x + u16::from(area.width > 0),
        area.y + u16::from(area.height > 0),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

pub(crate) fn regions(area: Rect, tree_height: u32) -> Regions {
    let area = bounded(area);
    let framed = area.width >= 40
        && area.height >= 18
        && u32::from(area.height.saturating_sub(7)) >= tree_height;
    layout_regions(area, framed)
}

pub(crate) fn max_editor_area(area: Rect) -> Rect {
    layout_regions(bounded(area), false).body
}

fn layout_regions(area: Rect, framed: bool) -> Regions {
    let header_height = if framed { 3 } else { 1 }.min(area.height);
    let available = area.height.saturating_sub(header_height);
    let dock_height = if framed {
        4
    } else if area.height >= 8 {
        2
    } else {
        1
    }
    .min(available);
    let body_height = available.saturating_sub(dock_height);
    let header = Rect::new(area.x, area.y, area.width, header_height);
    let body = Rect::new(area.x, area.y + header_height, area.width, body_height);
    let dock = Rect::new(
        area.x,
        area.bottom().saturating_sub(dock_height),
        area.width,
        dock_height,
    );
    let status_height = if framed { 3 } else { 1 }.min(dock_height);
    let status = Rect::new(dock.x, dock.y, dock.width, status_height);
    let helper = (dock_height > status_height)
        .then(|| Rect::new(dock.x, dock.y + status_height, dock.width, 1));
    Regions {
        header,
        body,
        dock,
        status,
        helper,
        framed,
    }
}

pub(crate) fn line_digits(lines: usize) -> usize {
    ((lines.max(1).ilog10() as usize) + 1).max(2)
}

pub(crate) fn pane_min_width(lines: usize) -> u16 {
    MIN_EDITOR_WIDTH.max(u16::try_from(line_digits(lines) + 5).unwrap_or(u16::MAX))
}

pub(crate) fn pane_areas(panel: Rect, lines: usize) -> PaneAreas {
    let inside = inner(panel);
    let tabs = Rect::new(inside.x, inside.y, inside.width, inside.height.min(1));
    let text_offset = tabs.height + u16::from(inside.height >= 3);
    let y = inside.y + text_offset;
    let height = inside.height.saturating_sub(text_offset);
    let gutter_width = u16::try_from(line_digits(lines) + 1)
        .unwrap_or(u16::MAX)
        .min(inside.width);
    let body_width = inside.width - gutter_width;
    // Each active rail needs two cells after reserving the other axis.
    let horizontal = height >= 2 && body_width >= 2 && (height == 2 || body_width >= 3);
    let text_height = height - u16::from(horizontal);
    let vertical = text_height >= 2 && body_width >= 2;
    let text_width = body_width - u16::from(vertical);
    PaneAreas {
        tabs,
        gutter: Rect::new(inside.x, y, gutter_width, text_height),
        text: Rect::new(inside.x + gutter_width, y, text_width, text_height),
        vertical_scrollbar: vertical.then_some(Rect::new(
            inside.x + gutter_width + text_width,
            y,
            1,
            text_height,
        )),
        horizontal_scrollbar: horizontal.then_some(Rect::new(
            inside.x + gutter_width,
            y + text_height,
            text_width,
            1,
        )),
    }
}

/// Reserve a vertical rail when there is room for a thumb, travel, and a content column.
pub(crate) fn scroll_rows(area: Rect) -> (Rect, Option<Rect>) {
    let area = bounded(area);
    if area.width < 2 || area.height < 2 {
        return (area, None);
    }
    let rows = Rect::new(area.x, area.y, area.width - 1, area.height);
    (rows, Some(Rect::new(rows.right(), rows.y, 1, rows.height)))
}

pub(crate) fn sidebar(
    body: Rect,
    requested: u16,
    active_min: u16,
    tree_min: (u32, u32),
    enabled: bool,
) -> SidebarAreas {
    let body = bounded(body);
    let full_fits = u32::from(body.width) >= tree_min.0 && u32::from(body.height) >= tree_min.1;
    let full_available = if u32::from(body.height) >= tree_min.1 {
        u32::from(body.width).saturating_sub(tree_min.0.saturating_add(1))
    } else {
        0
    };
    let width = if !enabled || body.height < MIN_PANE_HEIGHT || body.width < active_min {
        0
    } else if full_available >= u32::from(MIN_SIDEBAR_WIDTH) {
        requested
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
            .min(full_available.min(u32::from(u16::MAX)) as u16)
    } else if full_fits {
        0
    } else {
        let available = body.width.saturating_sub(active_min.saturating_add(1));
        if available >= MIN_SIDEBAR_WIDTH {
            requested
                .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
                .min(available)
        } else {
            0
        }
    };
    if width == 0 {
        SidebarAreas {
            panel: None,
            parent: None,
            rows: None,
            scrollbar: None,
            divider: None,
            editor: body,
        }
    } else {
        let panel = Rect::new(body.x, body.y, width, body.height);
        let inside = inner(panel);
        let parent_height = inside.height.min(1);
        let parent = Some(Rect::new(inside.x, inside.y, inside.width, parent_height));
        let (rows, scrollbar) = scroll_rows(Rect::new(
            inside.x,
            inside.y + parent_height,
            inside.width,
            inside.height.saturating_sub(parent_height),
        ));
        SidebarAreas {
            panel: Some(panel),
            parent,
            rows: Some(rows),
            scrollbar,
            divider: Some(Rect::new(body.x + width, body.y, 1, body.height)),
            editor: Rect::new(
                body.x + width + 1,
                body.y,
                body.width - width - 1,
                body.height,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_content_stay_inside_the_viewport_at_edges() {
        for viewport in [
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 0, 12, 6),
            Rect::new(7, 11, 120, 36),
            Rect::new(65530, 65530, 12, 12),
            Rect::new(0, 0, u16::MAX, u16::MAX),
        ] {
            let limits = bounded(viewport);
            let areas = regions(viewport, 4);
            for area in [areas.header, areas.body, areas.dock, areas.status]
                .into_iter()
                .chain(areas.helper)
            {
                if !area.is_empty() {
                    assert_eq!(area.intersection(limits), area);
                }
            }
            assert!(areas.header.intersection(areas.body).is_empty());
            assert!(areas.body.intersection(areas.dock).is_empty());
            assert_eq!(areas.body.y, areas.header.bottom());
            assert_eq!(areas.body.bottom(), areas.dock.y);
            if !areas.status.is_empty() {
                assert_eq!(areas.status.intersection(areas.dock), areas.status);
            }
        }
    }

    #[test]
    fn minimum_panels_keep_text_outside_all_borders_and_tabs() {
        for lines in [1, 9, 10, 1000, 1_000_000, usize::MAX] {
            for height in [
                MIN_PANE_HEIGHT,
                MIN_PANE_HEIGHT + 1,
                MIN_PANE_HEIGHT + 2,
                12,
            ] {
                let panel = Rect::new(5, 7, pane_min_width(lines), height);
                let areas = pane_areas(panel, lines);
                assert!(!areas.text.is_empty());
                assert!(areas.text.x > panel.x);
                assert!(areas.text.right() < panel.right());
                let gap = u16::from(height > MIN_PANE_HEIGHT);
                assert_eq!(areas.text.y, areas.tabs.bottom() + gap);
                assert!(areas.text.bottom() < panel.bottom());
                assert!(areas.text.intersection(areas.gutter).is_empty());
                assert!(areas.text.intersection(areas.tabs).is_empty());
                for track in [areas.vertical_scrollbar, areas.horizontal_scrollbar]
                    .into_iter()
                    .flatten()
                {
                    assert_eq!(track.intersection(inner(panel)), track);
                    assert!(track.width.max(track.height) >= 2);
                    for content in [areas.tabs, areas.text, areas.gutter] {
                        assert!(track.intersection(content).is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn global_decoration_never_hides_a_tree_that_compact_chrome_can_fit() {
        for height in 6..40 {
            for tree_height in [4u32, 9, 14, 19, u32::MAX] {
                let viewport = Rect::new(0, 0, 100, height);
                let maximum = max_editor_area(viewport);
                let actual = regions(viewport, tree_height);
                if u32::from(maximum.height) >= tree_height {
                    assert!(u32::from(actual.body.height) >= tree_height);
                }
                if actual.framed {
                    assert!(u32::from(actual.body.height) >= tree_height);
                }
            }
        }
    }

    #[test]
    fn sidebar_prefers_the_full_tree_and_excludes_its_border_from_rows() {
        let minimum = (80, 8);
        let exact = Rect::new(0, 3, 80, 20);
        assert!(sidebar(exact, 28, 12, minimum, true).panel.is_none());
        let wide = sidebar(Rect::new(0, 3, 120, 20), 28, 12, minimum, true);
        assert!(u32::from(wide.editor.width) >= minimum.0);
        let (panel, parent, rows) = (
            wide.panel.unwrap(),
            wide.parent.unwrap(),
            wide.rows.unwrap(),
        );
        assert!(parent.x > panel.x && parent.right() < panel.right());
        assert_eq!(parent.y, panel.y + 1);
        assert_eq!(parent.height, 1);
        assert!(rows.x > panel.x && rows.right() < panel.right());
        assert_eq!(rows.y, parent.bottom());
        assert!(rows.bottom() < panel.bottom());
        assert!(parent.intersection(rows).is_empty());
        let rail = wide.scrollbar.unwrap();
        assert!(rail.intersection(rows).is_empty());
        assert!(rail.intersection(parent).is_empty());
        assert_eq!(rail.intersection(inner(panel)), rail);
        let tiny = sidebar(Rect::new(0, 0, 100, MIN_PANE_HEIGHT), 28, 12, (12, 4), true);
        assert_eq!(tiny.rows.unwrap().height, 1);
        assert!(tiny.scrollbar.is_none());
        let collapsed = sidebar(Rect::new(0, 3, 80, 7), 28, 12, minimum, true);
        assert!(collapsed.panel.is_some());
        assert!(collapsed.editor.width >= 12);
        let extreme = sidebar(Rect::new(0, 0, 100, 20), 28, 12, (u32::MAX, 4), true);
        assert!(extreme.editor.width >= 12);
    }
}

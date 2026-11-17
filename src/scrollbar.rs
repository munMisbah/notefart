use ratatui::layout::Rect;

use crate::{
    document::{Document, LineWidthCursor},
    layout::Axis,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scrollbar {
    pub track: Rect,
    pub thumb: Rect,
    pub axis: Axis,
    pub offset: usize,
    pub max_offset: usize,
    page: usize,
}

impl Scrollbar {
    pub(crate) fn new(track: Rect, axis: Axis, offset: usize, page: usize, content: usize) -> Self {
        let length = match axis {
            Axis::Columns => track.width,
            Axis::Rows => track.height,
        };
        let max_offset = content.saturating_sub(page);
        let size = if max_offset == 0 {
            length
        } else {
            ((u128::from(length) * page as u128 / content as u128) as u16)
                .max(1)
                .min(length.saturating_sub(1))
        };
        let travel = length.saturating_sub(size);
        let offset = offset.min(max_offset);
        let position = if max_offset == 0 {
            0
        } else {
            ((offset as u128 * u128::from(travel) + max_offset as u128 / 2) / max_offset as u128)
                as u16
        };
        let thumb = match axis {
            Axis::Columns => Rect::new(track.x + position, track.y, size, track.height),
            Axis::Rows => Rect::new(track.x, track.y + position, track.width, size),
        };
        Self {
            track,
            thumb,
            axis,
            offset,
            max_offset,
            page,
        }
    }

    fn start(self, rect: Rect) -> u16 {
        match self.axis {
            Axis::Columns => rect.x,
            Axis::Rows => rect.y,
        }
    }

    fn length(self, rect: Rect) -> u16 {
        match self.axis {
            Axis::Columns => rect.width,
            Axis::Rows => rect.height,
        }
    }

    pub(crate) fn page_offset(self, coordinate: u16) -> usize {
        if coordinate < self.start(self.thumb) {
            self.offset.saturating_sub(self.page.max(1))
        } else if coordinate >= self.start(self.thumb) + self.length(self.thumb) {
            self.offset
                .saturating_add(self.page.max(1))
                .min(self.max_offset)
        } else {
            self.offset
        }
    }

    /// Map a moved thumb from its original offset; endpoint cells clamp exactly.
    pub(crate) fn drag_offset(self, anchor: u16, coordinate: u16) -> usize {
        let travel = self
            .length(self.track)
            .saturating_sub(self.length(self.thumb));
        if travel == 0 {
            return self.offset;
        }
        let delta = i32::from(coordinate) - i32::from(anchor);
        let target = i32::from(self.start(self.thumb)) + delta;
        if target <= i32::from(self.start(self.track)) {
            return 0;
        }
        if target >= i32::from(self.start(self.track)) + i32::from(travel) {
            return self.max_offset;
        }
        let distance = ((u128::from(delta.unsigned_abs()) * self.max_offset as u128
            + u128::from(travel) / 2)
            / u128::from(travel)) as usize;
        if delta < 0 {
            self.offset.saturating_sub(distance)
        } else {
            self.offset.saturating_add(distance).min(self.max_offset)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WidthKey {
    revision: u64,
    top: usize,
    rows: u16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MeasuredWidth {
    key: WidthKey,
    pub cells: usize,
}

impl MeasuredWidth {
    pub(crate) fn for_view(self, revision: u64, top: usize, rows: u16) -> Option<usize> {
        (self.key
            == WidthKey {
                revision,
                top,
                rows,
            })
        .then_some(self.cells)
    }
}

/// Per-view, derived work. It is neither shared history nor session state.
#[derive(Debug, Default)]
pub(crate) struct WidthCache {
    key: Option<WidthKey>,
    next_line: usize,
    end_line: usize,
    widest: usize,
    cursor: LineWidthCursor,
    ready: Option<MeasuredWidth>,
}

impl WidthCache {
    pub(crate) fn prepare(&mut self, document: &Document, top: usize, rows: u16) {
        let key = WidthKey {
            revision: document.revision(),
            top,
            rows,
        };
        if self.key == Some(key) {
            return;
        }
        *self = Self {
            key: Some(key),
            next_line: top,
            end_line: top
                .saturating_add(usize::from(rows))
                .min(document.line_count()),
            ..Self::default()
        };
    }

    pub(crate) fn measurement(&self) -> Option<MeasuredWidth> {
        self.ready
    }

    pub(crate) fn pending(&self) -> bool {
        self.key.is_some() && self.ready.is_none()
    }

    pub(crate) fn advance(&mut self, document: &Document, mut budget: usize) -> bool {
        if !self.pending() {
            return false;
        }
        while budget > 0 && self.next_line < self.end_line {
            let (used, width) =
                document.measure_line_width(self.next_line, &mut self.cursor, budget);
            budget = budget.saturating_sub(used.max(1));
            if let Some(width) = width {
                self.widest = self.widest.max(width);
                self.next_line += 1;
                self.cursor = LineWidthCursor::default();
            }
        }
        if self.next_line < self.end_line {
            return false;
        }
        self.ready = Some(MeasuredWidth {
            key: self.key.expect("pending width has a viewport key"),
            cells: self.widest.saturating_add(1), // Keep the end-of-line caret reachable.
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish_width(cache: &mut WidthCache, document: &Document, budget: usize) {
        for _ in 0..=document.len_chars() + document.line_count() {
            if !cache.pending() {
                break;
            }
            cache.advance(document, budget);
        }
        assert!(
            !cache.pending(),
            "width scan must finish within a finite cell budget"
        );
    }

    #[test]
    fn quantized_drag_keeps_anchor_and_reaches_both_ends() {
        for axis in [Axis::Rows, Axis::Columns] {
            let track = match axis {
                Axis::Rows => Rect::new(5, 7, 1, 9),
                Axis::Columns => Rect::new(7, 5, 9, 1),
            };
            let bar = Scrollbar::new(track, axis, 237, 9, 1000);
            let anchor = bar.start(bar.thumb);
            assert_eq!(bar.drag_offset(anchor, anchor), 237);
            assert_eq!(bar.drag_offset(anchor, 0), 0);
            assert_eq!(bar.drag_offset(anchor, u16::MAX), 991);
            assert_eq!(bar.page_offset(bar.start(track) + 8), 246);
            let near_start = Scrollbar::new(track, axis, 37, 9, 1000);
            let endpoint = near_start.start(near_start.thumb);
            assert_eq!(near_start.drag_offset(endpoint, endpoint), 0);
        }
    }

    #[test]
    fn huge_content_maps_without_overflow() {
        let bar = Scrollbar::new(
            Rect::new(1, 1, 1, 20),
            Axis::Rows,
            usize::MAX / 2,
            20,
            usize::MAX,
        );
        assert_eq!(bar.thumb.height, 1);
        assert_eq!(bar.drag_offset(bar.thumb.y, u16::MAX), usize::MAX - 20);
        assert_eq!(bar.drag_offset(bar.thumb.y, 0), 0);
    }

    #[test]
    fn measured_width_resumes_and_restarts_after_edits_and_viewport_changes() {
        let mut doc = Document::new(&format!("{}\nshort\n界\tend", "界\t".repeat(1600)));
        let mut cache = WidthCache::default();
        cache.prepare(&doc, 0, 2);
        assert!(!cache.advance(&doc, 13));
        finish_width(&mut cache, &doc, 13);
        assert_eq!(cache.measurement().unwrap().cells, 6401);
        cache.prepare(&doc, 1, 2);
        finish_width(&mut cache, &doc, 3);
        assert_eq!(cache.measurement().unwrap().cells, 8);
        doc.replace(0..doc.len_chars(), "x").unwrap();
        assert!(
            cache
                .measurement()
                .unwrap()
                .for_view(doc.revision(), 1, 2)
                .is_none()
        );
        cache.prepare(&doc, 0, 2);
        finish_width(&mut cache, &doc, 1);
        assert_eq!(cache.measurement().unwrap().cells, 2);
    }
}

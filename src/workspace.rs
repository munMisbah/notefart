use std::{
    io,
    ops::Range,
    path::{Path, PathBuf},
};

use ratatui::layout::Rect;

use crate::{
    chrome,
    document::{Document, TextChange},
    explorer::Explorer,
    file_io::{self, DiskState, FileError, FileFormat, LoadedFile},
    layout::{Axis, PaneMinimum, SplitTree},
    ui::TabCaption,
};

pub(crate) const DEFAULT_SIDEBAR_WIDTH: u16 = 28;

pub(crate) fn snap_right(doc: &Document, pos: usize) -> usize {
    let pos = pos.min(doc.len_chars());
    if doc.is_grapheme_boundary(pos) {
        pos
    } else {
        doc.next(pos)
    }
}

pub(crate) fn snap_left(doc: &Document, pos: usize) -> usize {
    let pos = pos.min(doc.len_chars());
    if doc.is_grapheme_boundary(pos) {
        pos
    } else {
        doc.previous(pos)
    }
}

pub(crate) struct DocumentBuffer {
    pub id: u64,
    pub document: Document,
    pub path: Option<PathBuf>,
    display_label: String,
    pub baseline: Option<DiskState>,
    pub format: FileFormat,
    pub read_only: Option<String>,
}

impl DocumentBuffer {
    pub(crate) fn new_blank(id: u64) -> Self {
        Self {
            id,
            document: Document::new(""),
            path: None,
            display_label: "Untitled".to_string(),
            baseline: None,
            format: FileFormat::default(),
            read_only: None,
        }
    }

    pub(crate) fn from_loaded(id: u64, path: PathBuf, loaded: LoadedFile) -> Self {
        let display_label = compute_display_label(Some(&path));
        Self {
            id,
            document: Document::new(&loaded.text),
            path: Some(path),
            display_label,
            baseline: Some(loaded.baseline),
            format: loaded.format,
            read_only: loaded.read_only,
        }
    }

    pub(crate) fn label(&self) -> &str {
        &self.display_label
    }

    pub(crate) fn update_path(&mut self, path: Option<PathBuf>) {
        self.display_label = compute_display_label(path.as_deref());
        self.path = path;
    }
}

pub(crate) struct Tab {
    pub id: u64,
    pub document_id: u64,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub preferred_column: Option<usize>,
    pub top_line: usize,
    pub left_cell: usize,
}

impl Tab {
    pub(crate) fn new(id: u64, document_id: u64) -> Self {
        Self {
            id,
            document_id,
            cursor: 0,
            anchor: None,
            preferred_column: None,
            top_line: 0,
            left_cell: 0,
        }
    }

    pub(crate) fn duplicate_for_split(&self, new_id: u64) -> Self {
        Self {
            id: new_id,
            document_id: self.document_id,
            cursor: self.cursor,
            anchor: self.anchor,
            preferred_column: self.preferred_column,
            top_line: self.top_line,
            left_cell: self.left_cell,
        }
    }

    pub(crate) fn reset_view(&mut self) {
        self.cursor = 0;
        self.anchor = None;
        self.preferred_column = None;
        self.top_line = 0;
        self.left_cell = 0;
    }

    pub(crate) fn selection(&self) -> Option<Range<usize>> {
        self.anchor
            .filter(|&anchor| anchor != self.cursor)
            .map(|anchor| anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub(crate) fn select_cursor(&mut self, doc: &Document, position: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = position.min(doc.len_chars());
    }

    pub(crate) fn ensure_visible(&mut self, doc: &Document, area: Rect) -> bool {
        if area.is_empty() {
            return false;
        }
        let old = (self.top_line, self.left_cell);
        let line = doc.line_of(self.cursor);
        let col = doc.column_of(self.cursor);
        if line < self.top_line {
            self.top_line = line;
        }
        if line >= self.top_line + usize::from(area.height) {
            self.top_line = line + 1 - usize::from(area.height);
        }
        if col < self.left_cell {
            self.left_cell = col;
        }
        if col >= self.left_cell + usize::from(area.width) {
            self.left_cell = col + 1 - usize::from(area.width);
        }
        old != (self.top_line, self.left_cell)
    }

    pub(crate) fn hit(&self, doc: &Document, area: Rect, column: u16, row: u16) -> usize {
        let line = self.top_line
            + usize::from(
                row.saturating_sub(area.y)
                    .min(area.height.saturating_sub(1)),
            );
        let col = self.left_cell
            + usize::from(
                column
                    .saturating_sub(area.x)
                    .min(area.width.saturating_sub(1)),
            );
        doc.position_at(line.min(doc.line_count().saturating_sub(1)), col)
    }

    fn remap_non_initiating(&mut self, doc: &Document, old_top_anchor: usize, change: TextChange) {
        let mapped_top = snap_left(doc, change.map(old_top_anchor, false));
        self.top_line = doc.line_of(mapped_top);

        let old_cursor = self.cursor;
        let new_cursor = snap_right(doc, change.map(old_cursor, true));
        self.cursor = new_cursor;

        if let Some(old_anchor) = self.anchor {
            let new_anchor = snap_right(doc, change.map(old_anchor, true));
            self.anchor = (new_anchor != new_cursor).then_some(new_anchor);
        }

        if old_cursor >= change.start {
            self.preferred_column = None;
        }
    }
}

pub(crate) struct Pane {
    pub id: u64,
    tabs: Vec<Tab>,
    active_tab_id: u64,
}

impl Pane {
    pub(crate) fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub(crate) fn active_tab_id(&self) -> u64 {
        self.active_tab_id
    }

    pub(crate) fn active_tab(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active_tab_id)
    }

    pub(crate) fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let id = self.active_tab_id;
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub(crate) fn tab(&self, view_id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == view_id)
    }

    pub(crate) fn tab_mut(&mut self, view_id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == view_id)
    }

    pub(crate) fn activate(&mut self, view_id: u64) -> bool {
        if self.tabs.iter().any(|t| t.id == view_id) {
            let changed = self.active_tab_id != view_id;
            self.active_tab_id = view_id;
            changed
        } else {
            false
        }
    }

    pub(crate) fn cycle_tab(&mut self, delta: isize) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        let current = self
            .tabs
            .iter()
            .position(|t| t.id == self.active_tab_id)
            .unwrap_or(0);
        let len = self.tabs.len() as isize;
        let next_idx = ((current as isize + delta).rem_euclid(len)) as usize;
        let next_id = self.tabs[next_idx].id;
        let changed = next_id != self.active_tab_id;
        self.active_tab_id = next_id;
        changed
    }

    pub(crate) fn find_document_view(&self, doc_id: u64) -> Option<u64> {
        self.tabs
            .iter()
            .find(|t| t.document_id == doc_id)
            .map(|t| t.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenOutcome {
    pub view_id: u64,
    pub document_id: u64,
    pub already_in_pane: bool,
    pub shared_existing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CloseViewOutcome {
    pub closed: bool,
    pub removed_document: Option<u64>,
}

pub(crate) struct Workspace {
    documents: Vec<DocumentBuffer>,
    panes: Vec<Pane>,
    active_pane_id: u64,
    pub tree: SplitTree,
    next_document_id: u64,
    next_view_id: u64,
    next_pane_id: u64,
    tx_top_anchors_scratch: Vec<(u64, usize)>,
    mins_scratch: Vec<PaneMinimum>,
    pub explorer: Option<Explorer>,
    pub explorer_root: PathBuf,
    pub root_label: String,
    pub explorer_visible: bool,
    pub explorer_focused: bool,
    pub sidebar_width: u16,
}

impl Workspace {
    pub(crate) fn new(explorer_root: PathBuf) -> (Self, Option<String>) {
        let (explorer, warning, explorer_root) = match Explorer::new(explorer_root.clone()) {
            // Keep explorer_root/root_label in sync with the explorer's
            // canonical root so retries and parent navigation stay consistent.
            Ok(exp) => {
                let root = exp.root().to_path_buf();
                (Some(exp), None, root)
            }
            Err(err) => (None, Some(err.to_string()), explorer_root),
        };
        let root_label = explorer_root.to_string_lossy().into_owned();
        let workspace = Self {
            documents: vec![DocumentBuffer::new_blank(1)],
            panes: vec![Pane {
                id: 1,
                tabs: vec![Tab::new(1, 1)],
                active_tab_id: 1,
            }],
            active_pane_id: 1,
            tree: SplitTree::new(1),
            next_document_id: 2,
            next_view_id: 2,
            next_pane_id: 2,
            tx_top_anchors_scratch: Vec::new(),
            mins_scratch: Vec::new(),
            explorer,
            explorer_root,
            root_label,
            explorer_visible: true,
            explorer_focused: false,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        };
        (workspace, warning)
    }

    #[cfg(test)]
    pub(crate) fn documents(&self) -> &[DocumentBuffer] {
        &self.documents
    }
    pub(crate) fn active_view_and_doc_mut(&mut self) -> Option<(&mut Tab, &DocumentBuffer)> {
        let active_pane_id = self.active_pane_id;
        self.pane_active_view_and_doc_mut(active_pane_id)
    }

    pub(crate) fn pane_active_view_and_doc_mut(
        &mut self,
        pane_id: u64,
    ) -> Option<(&mut Tab, &DocumentBuffer)> {
        let pane = self.panes.iter_mut().find(|p| p.id == pane_id)?;
        let tab = pane.active_tab_mut()?;
        let doc_id = tab.document_id;
        let doc = self.documents.iter().find(|d| d.id == doc_id)?;
        Some((tab, doc))
    }

    pub(crate) fn view_and_doc_mut(
        &mut self,
        pane_id: u64,
        view_id: u64,
    ) -> Option<(&mut Tab, &DocumentBuffer)> {
        let pane = self.panes.iter_mut().find(|p| p.id == pane_id)?;
        let tab = pane.tab_mut(view_id)?;
        let doc_id = tab.document_id;
        let doc = self.documents.iter().find(|d| d.id == doc_id)?;
        Some((tab, doc))
    }

    pub(crate) fn document(&self, doc_id: u64) -> Option<&DocumentBuffer> {
        self.documents.iter().find(|d| d.id == doc_id)
    }

    pub(crate) fn document_mut(&mut self, doc_id: u64) -> Option<&mut DocumentBuffer> {
        self.documents.iter_mut().find(|d| d.id == doc_id)
    }

    pub(crate) fn panes(&self) -> &[Pane] {
        &self.panes
    }

    pub(crate) fn pane(&self, pane_id: u64) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == pane_id)
    }

    pub(crate) fn pane_mut(&mut self, pane_id: u64) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == pane_id)
    }

    pub(crate) fn active_pane_id(&self) -> u64 {
        self.active_pane_id
    }

    pub(crate) fn active_pane(&self) -> Option<&Pane> {
        self.pane(self.active_pane_id)
    }

    pub(crate) fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        let id = self.active_pane_id;
        self.pane_mut(id)
    }

    pub(crate) fn active_view_id(&self) -> u64 {
        self.active_pane().map_or(0, Pane::active_tab_id)
    }

    pub(crate) fn active_view(&self) -> Option<&Tab> {
        self.active_pane().and_then(Pane::active_tab)
    }

    pub(crate) fn active_view_mut(&mut self) -> Option<&mut Tab> {
        self.active_pane_mut().and_then(Pane::active_tab_mut)
    }

    pub(crate) fn active_document(&self) -> Option<&DocumentBuffer> {
        let doc_id = self.active_view()?.document_id;
        self.document(doc_id)
    }

    #[cfg(test)]
    pub(crate) fn view(&self, view_id: u64) -> Option<(u64, &Tab)> {
        for pane in &self.panes {
            if let Some(tab) = pane.tab(view_id) {
                return Some((pane.id, tab));
            }
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn view_mut(&mut self, view_id: u64) -> Option<(u64, &mut Tab)> {
        for pane in &mut self.panes {
            let pane_id = pane.id;
            if let Some(tab) = pane.tab_mut(view_id) {
                return Some((pane_id, tab));
            }
        }
        None
    }

    pub(crate) fn view_count_for_document(&self, doc_id: u64) -> usize {
        self.panes
            .iter()
            .flat_map(|p| p.tabs.iter())
            .filter(|t| t.document_id == doc_id)
            .count()
    }

    pub(crate) fn first_view_for_document(&self, doc_id: u64) -> Option<(u64, u64)> {
        if let Some(pane) = self.active_pane()
            && let Some(view_id) = pane.find_document_view(doc_id)
        {
            return Some((pane.id, view_id));
        }
        for pane in &self.panes {
            if let Some(view_id) = pane.find_document_view(doc_id) {
                return Some((pane.id, view_id));
            }
        }
        None
    }

    pub(crate) fn activate_pane(&mut self, pane_id: u64) -> bool {
        if self.panes.iter().any(|p| p.id == pane_id) {
            let changed = self.active_pane_id != pane_id;
            self.active_pane_id = pane_id;
            changed
        } else {
            false
        }
    }

    pub(crate) fn activate_view(&mut self, pane_id: u64, view_id: u64) -> bool {
        let pane_changed = self.active_pane_id != pane_id;
        let Some(pane) = self.pane_mut(pane_id) else {
            return false;
        };
        if pane.tab(view_id).is_none() {
            return false;
        }
        let tab_changed = pane.activate(view_id);
        self.active_pane_id = pane_id;
        pane_changed || tab_changed
    }

    pub(crate) fn cycle_pane(&mut self, delta: isize) -> bool {
        let order = self.tree.panes();
        if order.len() <= 1 {
            return false;
        }
        let current = order
            .iter()
            .position(|&id| id == self.active_pane_id)
            .unwrap_or(0);
        let len = order.len() as isize;
        let next_idx = ((current as isize + delta).rem_euclid(len)) as usize;
        let next_id = order[next_idx];
        let changed = next_id != self.active_pane_id;
        self.active_pane_id = next_id;
        changed
    }

    pub(crate) fn cycle_tab(&mut self, delta: isize) -> bool {
        self.active_pane_mut()
            .is_some_and(|pane| pane.cycle_tab(delta))
    }

    pub(crate) fn add_blank_tab(&mut self) -> u64 {
        let doc_id = self.next_document_id;
        self.next_document_id += 1;
        let view_id = self.next_view_id;
        self.next_view_id += 1;

        self.documents.push(DocumentBuffer::new_blank(doc_id));
        if let Some(pane) = self.active_pane_mut() {
            pane.tabs.push(Tab::new(view_id, doc_id));
            pane.active_tab_id = view_id;
        }
        view_id
    }

    pub(crate) fn set_startup_missing_file(&mut self, path: PathBuf) {
        if let Some(doc_id) = self.active_view().map(|t| t.document_id)
            && let Some(buf) = self.document_mut(doc_id)
        {
            buf.update_path(Some(path));
        }
    }

    pub(crate) fn init_startup_file(&mut self, path: PathBuf, loaded: LoadedFile) {
        if let Some(doc_id) = self.active_view().map(|t| t.document_id)
            && let Some(buf) = self.document_mut(doc_id)
        {
            *buf = DocumentBuffer::from_loaded(doc_id, path, loaded);
        }
    }

    pub(crate) fn find_open_document(
        &self,
        path: &Path,
        exclude_doc_id: Option<u64>,
    ) -> Option<u64> {
        let target_id = file_identity(path);
        let target_canon = resolve_canonical_path(path).ok();
        for buf in &self.documents {
            if Some(buf.id) == exclude_doc_id {
                continue;
            }
            if let Some(tid) = target_id {
                if buf.baseline.as_ref().map(DiskState::identity) == Some(tid) {
                    return Some(buf.id);
                }
                if buf.path.as_deref().and_then(file_identity) == Some(tid) {
                    return Some(buf.id);
                }
            }
            if let (Some(tc), Some(doc_path)) = (&target_canon, buf.path.as_deref())
                && resolve_canonical_path(doc_path).ok().as_ref() == Some(tc)
            {
                return Some(buf.id);
            }
        }
        None
    }

    pub(crate) fn open_path(&mut self, path: PathBuf) -> Result<OpenOutcome, FileError> {
        let (doc_id, shared_existing) =
            if let Some(existing_doc) = self.find_open_document(&path, None) {
                (existing_doc, true)
            } else {
                let loaded = file_io::load(&path)?;
                let loaded_id = loaded.baseline.identity();
                if let Some(existing) = self
                    .documents
                    .iter()
                    .find(|d| d.baseline.as_ref().map(DiskState::identity) == Some(loaded_id))
                {
                    (existing.id, true)
                } else {
                    let doc_id = self.next_document_id;
                    self.next_document_id += 1;
                    self.documents
                        .push(DocumentBuffer::from_loaded(doc_id, path, loaded));
                    (doc_id, false)
                }
            };

        let active_pane_id = self.active_pane_id;
        let target_pane = self
            .panes
            .iter_mut()
            .find(|pane| pane.id == active_pane_id)
            .expect("workspace always owns at least one pane");
        if let Some(existing_view_id) = target_pane.find_document_view(doc_id) {
            target_pane.active_tab_id = existing_view_id;
            return Ok(OpenOutcome {
                view_id: existing_view_id,
                document_id: doc_id,
                already_in_pane: true,
                shared_existing: true,
            });
        }

        let view_id = self.next_view_id;
        self.next_view_id += 1;
        target_pane.tabs.push(Tab::new(view_id, doc_id));
        target_pane.active_tab_id = view_id;
        Ok(OpenOutcome {
            view_id,
            document_id: doc_id,
            already_in_pane: false,
            shared_existing,
        })
    }

    pub(crate) fn check_save_as_collision(&self, target_doc_id: u64, dest: &Path) -> Option<u64> {
        self.find_open_document(dest, Some(target_doc_id))
    }

    pub(crate) fn pane_minimums(&self, out: &mut Vec<PaneMinimum>) {
        out.clear();
        for pane in &self.panes {
            let doc = pane
                .active_tab()
                .and_then(|t| self.document(t.document_id))
                .map(|b| &b.document);
            let width = doc.map_or(chrome::MIN_EDITOR_WIDTH, |b| {
                chrome::pane_min_width(b.line_count())
            });
            out.push(PaneMinimum {
                id: pane.id,
                width,
                height: chrome::MIN_PANE_HEIGHT,
            });
        }
    }

    pub(crate) fn tree_minimum(&mut self) -> (u32, u32) {
        let mut mins = std::mem::take(&mut self.mins_scratch);
        self.pane_minimums(&mut mins);
        let size = self.tree.minimum(&mins);
        self.mins_scratch = mins;
        size
    }

    pub(crate) fn can_split_active(&mut self, axis: Axis, max_editor_area: Rect) -> bool {
        let active_id = self.active_pane_id;
        let mut mins = std::mem::take(&mut self.mins_scratch);
        self.pane_minimums(&mut mins);
        let ok = self.tree.can_split(active_id, axis, max_editor_area, &mins);
        self.mins_scratch = mins;
        ok
    }

    pub(crate) fn split_active_pane(&mut self, axis: Axis, max_editor_area: Rect) -> Option<u64> {
        if !self.can_split_active(axis, max_editor_area) {
            return None;
        }
        let old_active_pane_id = self.active_pane_id;
        let new_view_id = self.next_view_id;
        let new_pane_id = self.next_pane_id;
        let duplicated = self
            .active_pane()?
            .active_tab()?
            .duplicate_for_split(new_view_id);

        self.tree.split(old_active_pane_id, new_pane_id, axis)?;
        self.next_view_id += 1;
        self.next_pane_id += 1;

        self.panes.push(Pane {
            id: new_pane_id,
            tabs: vec![duplicated],
            active_tab_id: new_view_id,
        });
        self.active_pane_id = new_pane_id;
        Some(new_pane_id)
    }

    pub(crate) fn edit_transaction<R>(
        &mut self,
        doc_id: u64,
        initiating_view_id: u64,
        use_edit_cursor: bool,
        f: impl FnOnce(&mut Document) -> (R, Option<usize>),
    ) -> Option<(R, bool)> {
        let doc_idx = self.documents.iter().position(|d| d.id == doc_id)?;
        let old_revision = self.documents[doc_idx].document.revision();

        self.tx_top_anchors_scratch.clear();
        for pane in &self.panes {
            for tab in &pane.tabs {
                if tab.document_id == doc_id {
                    let anchor = self.documents[doc_idx].document.line_start(tab.top_line);
                    self.tx_top_anchors_scratch.push((tab.id, anchor));
                }
            }
        }

        let (result, edit_cursor) = f(&mut self.documents[doc_idx].document);
        let doc = &self.documents[doc_idx].document;
        let mutated = doc.revision() != old_revision;

        if mutated && let Some(change) = doc.last_change() {
            for pane in &mut self.panes {
                for tab in &mut pane.tabs {
                    if tab.document_id != doc_id {
                        continue;
                    }
                    let old_top_anchor = self
                        .tx_top_anchors_scratch
                        .iter()
                        .find_map(|&(id, pos)| (id == tab.id).then_some(pos))
                        .unwrap_or(0);
                    if tab.id == initiating_view_id {
                        let mapped_top = snap_left(doc, change.map(old_top_anchor, false));
                        tab.top_line = doc.line_of(mapped_top);
                        if use_edit_cursor {
                            tab.cursor = edit_cursor
                                .unwrap_or_else(|| snap_right(doc, change.map(tab.cursor, true)));
                        } else {
                            tab.cursor = snap_right(doc, change.map(tab.cursor, true));
                        }
                        tab.anchor = None;
                        tab.preferred_column = None;
                    } else {
                        tab.remap_non_initiating(doc, old_top_anchor, change);
                    }
                }
            }
        }

        Some((result, mutated))
    }

    pub(crate) fn reload_document(
        &mut self,
        doc_id: u64,
        path: PathBuf,
        loaded: LoadedFile,
    ) -> bool {
        let Some(buf) = self.document_mut(doc_id) else {
            return false;
        };
        *buf = DocumentBuffer::from_loaded(doc_id, path, loaded);
        for pane in &mut self.panes {
            for tab in &mut pane.tabs {
                if tab.document_id == doc_id {
                    tab.reset_view();
                }
            }
        }
        true
    }

    pub(crate) fn close_view_unconditional(&mut self, view_id: u64) -> CloseViewOutcome {
        let Some((pane_idx, tab_idx)) = self.panes.iter().enumerate().find_map(|(pi, pane)| {
            pane.tabs
                .iter()
                .position(|t| t.id == view_id)
                .map(|ti| (pi, ti))
        }) else {
            return CloseViewOutcome {
                closed: false,
                removed_document: None,
            };
        };

        let pane_id = self.panes[pane_idx].id;
        let doc_id = self.panes[pane_idx].tabs[tab_idx].document_id;
        self.panes[pane_idx].tabs.remove(tab_idx);

        if self.panes[pane_idx].tabs.is_empty() {
            if self.panes.len() > 1 {
                self.panes.remove(pane_idx);
                self.tree.remove(pane_id);
                if self.active_pane_id == pane_id {
                    self.active_pane_id = self
                        .tree
                        .panes()
                        .first()
                        .copied()
                        .unwrap_or_else(|| self.panes[0].id);
                }
            } else {
                let blank_doc_id = self.next_document_id;
                self.next_document_id += 1;
                let blank_view_id = self.next_view_id;
                self.next_view_id += 1;
                self.documents.push(DocumentBuffer::new_blank(blank_doc_id));
                self.panes[pane_idx]
                    .tabs
                    .push(Tab::new(blank_view_id, blank_doc_id));
                self.panes[pane_idx].active_tab_id = blank_view_id;
            }
        } else if self.panes[pane_idx].active_tab_id == view_id {
            let next_idx = tab_idx.min(self.panes[pane_idx].tabs.len() - 1);
            self.panes[pane_idx].active_tab_id = self.panes[pane_idx].tabs[next_idx].id;
        }

        let removed_document = if self.view_count_for_document(doc_id) == 0 {
            self.documents.retain(|d| d.id != doc_id);
            Some(doc_id)
        } else {
            None
        };

        CloseViewOutcome {
            closed: true,
            removed_document,
        }
    }

    pub(crate) fn first_dirty_final_view_in_pane(&self, pane_id: u64) -> Option<(u64, u64)> {
        let pane = self.pane(pane_id)?;
        for tab in &pane.tabs {
            let Some(buf) = self.document(tab.document_id) else {
                continue;
            };
            if buf.document.is_dirty() && self.view_count_for_document(buf.id) == 1 {
                return Some((tab.id, buf.id));
            }
        }
        None
    }

    pub(crate) fn discard_tab_in_pane_for_close_pane(
        &mut self,
        pane_id: u64,
        doc_id: u64,
    ) -> Option<u64> {
        let pane = self.pane_mut(pane_id)?;
        let tab_idx = pane.tabs.iter().position(|t| t.document_id == doc_id)?;
        let removed_view_id = pane.tabs[tab_idx].id;
        pane.tabs.remove(tab_idx);
        if !pane.tabs.is_empty() && pane.active_tab_id == removed_view_id {
            let next_idx = tab_idx.min(pane.tabs.len() - 1);
            pane.active_tab_id = pane.tabs[next_idx].id;
        }
        if self.view_count_for_document(doc_id) == 0 {
            self.documents.retain(|d| d.id != doc_id);
            Some(doc_id)
        } else {
            None
        }
    }

    pub(crate) fn close_pane_unconditional(&mut self, pane_id: u64) -> Vec<u64> {
        let Some(pane_idx) = self.panes.iter().position(|p| p.id == pane_id) else {
            return Vec::new();
        };

        if self.panes.len() > 1 {
            self.panes.remove(pane_idx);
            self.tree.remove(pane_id);
            if self.active_pane_id == pane_id {
                self.active_pane_id = self
                    .tree
                    .panes()
                    .first()
                    .copied()
                    .unwrap_or_else(|| self.panes[0].id);
            }
        } else {
            self.panes[pane_idx].tabs.clear();
            let blank_doc_id = self.next_document_id;
            self.next_document_id += 1;
            let blank_view_id = self.next_view_id;
            self.next_view_id += 1;
            self.documents.push(DocumentBuffer::new_blank(blank_doc_id));
            self.panes[pane_idx]
                .tabs
                .push(Tab::new(blank_view_id, blank_doc_id));
            self.panes[pane_idx].active_tab_id = blank_view_id;
        }

        let mut removed_docs = Vec::new();
        let mut i = 0;
        while i < self.documents.len() {
            let doc_id = self.documents[i].id;
            if self.view_count_for_document(doc_id) == 0 {
                self.documents.remove(i);
                removed_docs.push(doc_id);
            } else {
                i += 1;
            }
        }
        removed_docs
    }

    pub(crate) fn remove_document_for_quit(&mut self, doc_id: u64) -> bool {
        let existed = self.documents.iter().any(|d| d.id == doc_id);
        if !existed {
            return false;
        }
        self.documents.retain(|d| d.id != doc_id);
        let mut i = 0;
        while i < self.panes.len() {
            let can_remove_pane = self.panes.len() > 1;
            let pane = &mut self.panes[i];
            let mut ti = 0;
            while ti < pane.tabs.len() {
                if pane.tabs[ti].document_id == doc_id {
                    let removed_id = pane.tabs[ti].id;
                    pane.tabs.remove(ti);
                    if !pane.tabs.is_empty() && pane.active_tab_id == removed_id {
                        let next_idx = ti.min(pane.tabs.len() - 1);
                        pane.active_tab_id = pane.tabs[next_idx].id;
                    }
                } else {
                    ti += 1;
                }
            }
            if pane.tabs.is_empty() && can_remove_pane {
                let removed_pane_id = pane.id;
                self.panes.remove(i);
                self.tree.remove(removed_pane_id);
                if self.active_pane_id == removed_pane_id {
                    self.active_pane_id = self
                        .tree
                        .panes()
                        .first()
                        .copied()
                        .unwrap_or_else(|| self.panes[0].id);
                }
            } else {
                i += 1;
            }
        }
        true
    }

    pub(crate) fn first_dirty_document_id(&self) -> Option<u64> {
        self.documents
            .iter()
            .find(|d| d.document.is_dirty())
            .map(|d| d.id)
    }

    pub(crate) fn has_dirty_documents(&self) -> bool {
        self.documents.iter().any(|d| d.document.is_dirty())
    }

    pub(crate) fn drain_all_documents(&mut self) -> Vec<u64> {
        self.panes.clear();
        self.documents.drain(..).map(|d| d.id).collect()
    }

    pub(crate) fn set_sidebar_width(
        &mut self,
        width: u16,
        viewport_width: u16,
        collapsed: bool,
    ) -> bool {
        let min_editor = if collapsed {
            self.active_document()
                .map_or(chrome::MIN_EDITOR_WIDTH, |b| {
                    chrome::pane_min_width(b.document.line_count())
                })
        } else {
            u16::try_from(self.tree_minimum().0).unwrap_or(u16::MAX)
        };
        let max_for_editor = viewport_width.saturating_sub(min_editor.saturating_add(1));
        if max_for_editor < chrome::MIN_SIDEBAR_WIDTH {
            return false;
        }
        let upper = chrome::MAX_SIDEBAR_WIDTH.min(max_for_editor);
        let clamped = width.clamp(chrome::MIN_SIDEBAR_WIDTH, upper);
        if self.sidebar_width != clamped {
            self.sidebar_width = clamped;
            true
        } else {
            false
        }
    }

    pub(crate) fn retry_explorer(&mut self) -> io::Result<()> {
        let exp = Explorer::new(self.explorer_root.clone())?;
        self.explorer = Some(exp);
        self.sync_explorer_root();
        Ok(())
    }

    /// Browses the explorer's filesystem parent. `explorer_root`/`root_label`
    /// update only after the explorer commits the move; `Ok(false)` means the
    /// explorer is already at the filesystem root.
    pub(crate) fn explorer_go_parent(&mut self) -> io::Result<bool> {
        let moved = self
            .explorer
            .as_mut()
            .map_or(Ok(false), Explorer::go_parent)?;
        if moved {
            self.sync_explorer_root();
        }
        Ok(moved)
    }

    /// Enters the explorer's selected directory as the new browsing root,
    /// keeping root/label in sync only after a successful commit.
    pub(crate) fn explorer_enter_selected(&mut self) -> io::Result<bool> {
        let entered = self
            .explorer
            .as_mut()
            .map_or(Ok(false), Explorer::enter_selected_directory)?;
        if entered {
            self.sync_explorer_root();
        }
        Ok(entered)
    }

    fn sync_explorer_root(&mut self) {
        if let Some(exp) = self.explorer.as_ref() {
            self.explorer_root = exp.root().to_path_buf();
            self.root_label = exp.root().to_string_lossy().into_owned();
        }
    }

    pub(crate) fn push_pane_captions<'a>(&'a self, pane: &Pane, out: &mut Vec<TabCaption<'a>>) {
        for tab in &pane.tabs {
            if let Some(buf) = self.document(tab.document_id) {
                out.push(TabCaption {
                    id: tab.id,
                    label: buf.label(),
                    dirty: buf.document.is_dirty(),
                    read_only: buf.read_only.is_some(),
                    active: tab.id == pane.active_tab_id,
                });
            }
        }
    }
}

fn compute_display_label(path: Option<&Path>) -> String {
    match path {
        Some(p) => match p.file_name() {
            Some(name) if !name.is_empty() => name.to_string_lossy().into_owned(),
            _ => p.to_string_lossy().into_owned(),
        },
        None => "Untitled".to_string(),
    }
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn file_identity(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// Canonicalizes existing paths, or canonicalizes the deepest existing ancestor
/// for missing paths without lexically collapsing `..` across symlinks.
pub(crate) fn resolve_canonical_path(path: &Path) -> io::Result<PathBuf> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(canonical);
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut missing_tail = Vec::new();
    let mut cur = abs.as_path();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(cur) {
            let mut resolved = canonical;
            for component in missing_tail.into_iter().rev() {
                resolved.push(component);
            }
            return Ok(resolved);
        }
        let Some(last) = cur.components().next_back() else {
            return Ok(abs);
        };
        let Some(parent) = cur.parent() else {
            return Ok(abs);
        };
        if parent == cur {
            return Ok(abs);
        }
        missing_tail.push(last.as_os_str().to_os_string());
        cur = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn stable_ids_are_never_reused_across_domains() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ws, _) = Workspace::new(dir.path().to_path_buf());
        let p1 = ws.active_pane_id();
        let v1 = ws.active_view_id();
        let d1 = ws.active_document().unwrap().id;

        let v2 = ws.add_blank_tab();
        let d2 = ws.active_document().unwrap().id;
        assert!(v2 > v1);
        assert!(d2 > d1);

        let p2 = ws
            .split_active_pane(Axis::Columns, Rect::new(0, 0, 80, 20))
            .unwrap();
        let v3 = ws.active_view_id();
        assert!(p2 > p1);
        assert!(v3 > v2);
        // Split reuses existing document d2
        assert_eq!(ws.active_document().unwrap().id, d2);

        ws.close_view_unconditional(v2);
        ws.close_view_unconditional(v3);
        let v4 = ws.add_blank_tab();
        let d3 = ws.active_document().unwrap().id;
        assert!(v4 > v3);
        assert!(d3 > d2);

        let p3 = ws
            .split_active_pane(Axis::Rows, Rect::new(0, 0, 80, 20))
            .unwrap();
        assert!(p3 > p2);
    }

    #[test]
    fn duplicate_path_aliases_symlink_and_hardlink_activate_existing_tab() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.txt");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(&real, "hello").unwrap();
        let sym = dir.path().join("sym.txt");
        symlink(&real, &sym).unwrap();
        let hard = dir.path().join("hard.txt");
        std::fs::hard_link(&real, &hard).unwrap();
        let dotdot = sub.join("..").join("real.txt");

        let (mut ws, _) = Workspace::new(dir.path().to_path_buf());
        let first = ws.open_path(real.clone()).unwrap();
        assert!(!first.already_in_pane);
        assert_eq!(ws.active_pane().unwrap().tabs().len(), 2);

        let out_sym = ws.open_path(sym).unwrap();
        assert!(out_sym.already_in_pane);
        assert_eq!(out_sym.view_id, first.view_id);
        assert_eq!(out_sym.document_id, first.document_id);

        let out_hard = ws.open_path(hard).unwrap();
        assert!(out_hard.already_in_pane);
        assert_eq!(out_hard.view_id, first.view_id);

        let out_dotdot = ws.open_path(dotdot).unwrap();
        assert!(out_dotdot.already_in_pane);
        assert_eq!(out_dotdot.view_id, first.view_id);
        assert_eq!(ws.active_pane().unwrap().tabs().len(), 2);
    }

    #[test]
    fn open_alias_in_other_pane_creates_shared_view_without_reloading() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("shared.txt");
        std::fs::write(&real, "disk").unwrap();
        let sym = dir.path().join("alias.txt");
        symlink(&real, &sym).unwrap();

        let (mut ws, _) = Workspace::new(dir.path().to_path_buf());
        let pane1 = ws.active_pane_id();
        let open1 = ws.open_path(real).unwrap();
        let doc_id = open1.document_id;

        // Edit unsaved in pane1
        ws.edit_transaction(doc_id, open1.view_id, true, |doc| {
            let c = doc.replace(0..0, "unsaved ").unwrap();
            (c, Some(c))
        });

        // Split to pane2, switch pane2 to a fresh blank, then open symlink alias in pane2
        let pane2 = ws
            .split_active_pane(Axis::Columns, Rect::new(0, 0, 80, 20))
            .unwrap();
        let blank_in_p2 = ws.add_blank_tab();
        let p2_shared_view = ws.pane(pane2).unwrap().find_document_view(doc_id).unwrap();
        ws.close_view_unconditional(p2_shared_view);
        assert_eq!(ws.active_pane_id(), pane2);
        assert_eq!(ws.active_view_id(), blank_in_p2);

        let open2 = ws.open_path(sym).unwrap();
        assert!(!open2.already_in_pane);
        assert!(open2.shared_existing);
        assert_eq!(open2.document_id, doc_id);
        assert_ne!(open2.view_id, open1.view_id);
        assert_eq!(ws.view_count_for_document(doc_id), 2);
        assert_eq!(
            ws.document(doc_id).unwrap().document.text().to_string(),
            "unsaved disk"
        );
        // Re-opening in pane1 switches to open1.view_id without duplicating in pane1
        ws.activate_pane(pane1);
        let again = ws.open_path(dir.path().join("alias.txt")).unwrap();
        assert!(again.already_in_pane);
        assert_eq!(again.view_id, open1.view_id);
    }

    #[test]
    fn missing_path_canonicalization_respects_symlink_parent_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let real_parent = dir.path().join("real_parent");
        let real_nested = real_parent.join("nested");
        std::fs::create_dir_all(&real_nested).unwrap();
        let link = dir.path().join("link_nested");
        symlink(&real_nested, &link).unwrap();

        let via_link_parent = link.join("..").join("missing.txt");
        let direct_real = real_parent.join("missing.txt");
        let wrong_lexical = dir.path().join("missing.txt");

        let resolved_link = resolve_canonical_path(&via_link_parent).unwrap();
        let resolved_real = resolve_canonical_path(&direct_real).unwrap();
        let resolved_wrong = resolve_canonical_path(&wrong_lexical).unwrap();

        assert_eq!(resolved_link, resolved_real);
        assert_ne!(resolved_link, resolved_wrong);
    }

    #[test]
    fn save_as_collision_excludes_self_document_and_detects_other_documents() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::write(&file_a, "alpha").unwrap();
        std::fs::write(&file_b, "beta").unwrap();
        let sym_a = dir.path().join("sym_a.txt");
        symlink(&file_a, &sym_a).unwrap();
        let hard_a = dir.path().join("hard_a.txt");
        std::fs::hard_link(&file_a, &hard_a).unwrap();

        let (mut ws, _) = Workspace::new(dir.path().to_path_buf());
        let open_a = ws.open_path(file_a.clone()).unwrap();
        let open_b = ws.open_path(file_b.clone()).unwrap();

        // Split so doc_b has two views across two panes
        ws.split_active_pane(Axis::Columns, Rect::new(0, 0, 80, 20))
            .unwrap();
        assert_eq!(ws.view_count_for_document(open_b.document_id), 2);

        assert_eq!(
            ws.check_save_as_collision(open_b.document_id, &file_a),
            Some(open_a.document_id)
        );
        assert_eq!(
            ws.check_save_as_collision(open_b.document_id, &sym_a),
            Some(open_a.document_id)
        );
        assert_eq!(
            ws.check_save_as_collision(open_b.document_id, &hard_a),
            Some(open_a.document_id)
        );
        // Own document path (even with multiple views) is NOT a collision
        assert_eq!(
            ws.check_save_as_collision(open_b.document_id, &file_b),
            None
        );

        // Missing file alias collision
        ws.add_blank_tab();
        let doc_missing = ws.active_document().unwrap().id;
        ws.document_mut(doc_missing)
            .unwrap()
            .update_path(Some(dir.path().join("future.txt")));
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(
            ws.check_save_as_collision(open_b.document_id, &sub.join("..").join("future.txt")),
            Some(doc_missing)
        );
    }

    #[test]
    fn shared_edit_undo_redo_remaps_other_view_and_keeps_other_docs_independent() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ws, _) = Workspace::new(dir.path().to_path_buf());
        let pane1 = ws.active_pane_id();
        let v1 = ws.active_view_id();
        let doc1 = ws.active_document().unwrap().id;

        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.replace(0..0, "line0\nline1\nline2\nline3\n").unwrap();
            (c, Some(c))
        });

        let v_other_doc = ws.add_blank_tab();
        let doc2 = ws.active_document().unwrap().id;
        ws.edit_transaction(doc2, v_other_doc, true, |doc| {
            let c = doc.replace(0..0, "independent").unwrap();
            (c, Some(c))
        });

        // Switch pane1 back to v1 and split into pane2
        ws.activate_view(pane1, v1);
        let pane2 = ws
            .split_active_pane(Axis::Columns, Rect::new(0, 0, 100, 30))
            .unwrap();
        let v2 = ws.active_view_id();
        assert_ne!(v1, v2);
        assert_eq!(ws.pane(pane1).unwrap().tabs().len(), 2);
        assert_eq!(ws.pane(pane2).unwrap().tabs().len(), 1);

        // Set independent cursor/selection/top_line on v2:
        // "line0\n" (0..6), "line1\n" (6..12), "line2\n" (12..18), "line3\n" (18..24)
        {
            let v2_tab = ws.view_mut(v2).unwrap().1;
            v2_tab.top_line = 2; // line_start(2) == 12
            v2_tab.anchor = Some(13); // 'i' in line2
            v2_tab.cursor = 18; // start of line3
            v2_tab.preferred_column = Some(4);
        }

        // Insert "added\n" at 0 from v1 in pane1
        ws.activate_view(pane1, v1);
        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.replace(0..0, "added\n").unwrap();
            (c, Some(c))
        });

        let v2_tab = ws.view(v2).unwrap().1;
        assert_eq!(v2_tab.top_line, 3);
        assert_eq!(v2_tab.anchor, Some(19));
        assert_eq!(v2_tab.cursor, 24);
        assert_eq!(v2_tab.preferred_column, None);

        // Undo from v1 shifts v2 back
        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.undo();
            (c.is_some(), c)
        });
        let v2_tab = ws.view(v2).unwrap().1;
        assert_eq!(v2_tab.top_line, 2);
        assert_eq!(v2_tab.anchor, Some(13));
        assert_eq!(v2_tab.cursor, 18);

        // Redo from v2 (initiating view is v2 -> v1 shifts)
        ws.activate_pane(pane2);
        {
            let v1_tab = ws.view_mut(v1).unwrap().1;
            v1_tab.cursor = 20;
            v1_tab.top_line = 3;
        }
        ws.edit_transaction(doc1, v2, true, |doc| {
            let c = doc.redo();
            (c.is_some(), c)
        });
        let v1_tab = ws.view(v1).unwrap().1;
        assert_eq!(v1_tab.cursor, 26);
        assert_eq!(v1_tab.top_line, 4);

        // Independent doc2 was never touched
        assert_eq!(
            ws.document(doc2).unwrap().document.text().to_string(),
            "independent"
        );
        assert_eq!(ws.view(v_other_doc).unwrap().1.cursor, 11);
    }

    #[test]
    fn grapheme_remap_handles_before_within_after_and_combining_seams() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ws, _) = Workspace::new(dir.path().to_path_buf());
        let pane1 = ws.active_pane_id();
        let v1 = ws.active_view_id();
        let doc1 = ws.active_document().unwrap().id;

        // "e\u{0301}x\ny" -> chars: 'e'(0), '\u{0301}'(1), 'x'(2), '\n'(3), 'y'(4)
        // Wait: start with "ab\ncd" (5 chars)
        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.replace(0..0, "ab\ncd").unwrap();
            (c, Some(c))
        });

        let _pane2 = ws
            .split_active_pane(Axis::Columns, Rect::new(0, 0, 100, 30))
            .unwrap();
        let v2 = ws.active_view_id();

        // Case 1: v2 selection spans inside a deleted range -> collapses and anchor clears
        {
            let v2_tab = ws.view_mut(v2).unwrap().1;
            v2_tab.anchor = Some(1);
            v2_tab.cursor = 2;
            v2_tab.preferred_column = Some(2);
        }
        ws.activate_pane(pane1);
        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.replace(0..3, "Z\n").unwrap(); // deletes "ab\n", inserts "Z\n"
            (c, Some(c))
        });
        let v2_tab = ws.view(v2).unwrap().1;
        // Both anchor (1) and cursor (2) collapse to new_end (2) with right affinity -> anchor cleared
        assert_eq!(v2_tab.cursor, 2);
        assert_eq!(v2_tab.anchor, None);
        assert_eq!(v2_tab.preferred_column, None);

        // Case 2: position before change keeps preferred_column and position
        {
            let v2_tab = ws.view_mut(v2).unwrap().1;
            v2_tab.cursor = 0;
            v2_tab.preferred_column = Some(0);
        }
        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.replace(2..4, "XY").unwrap();
            (c, Some(c))
        });
        let v2_tab = ws.view(v2).unwrap().1;
        assert_eq!(v2_tab.cursor, 0);
        assert_eq!(v2_tab.preferred_column, Some(0));

        // Case 3: combining mark merge across seam:
        // Replace all with "aXb", place v2 cursor at 1 (between 'a' and 'X'),
        // then delete 'X' and insert combining acute '\u{0301}'? Wait: if doc is "aXb",
        // replace 1..2 with "\u{0301}" -> "a\u{0301}b":
        // old_end = 2, new_end = 2. If v2 cursor was at 1 (=start), right affinity moves to 2 (after \u{0301}).
        // What if doc is "a\u{0301}"? Wait: replace 0..len with "aXb", v2 cursor at 2 (before 'b'),
        // delete 'X' (1..2 -> "") and insert '\u{0301}' at 1..2 -> "a\u{0301}b" (2 is valid after '\u{0301}').
        // Now test when right-affinity lands *inside* a merged grapheme:
        // Doc: "aXb\u{0301}" (0:'a', 1:'X', 2:'b'? Wait: 'X' deleted -> "ab\u{0301}", 1 is valid between 'a' and 'b').
        // How can cursor land inside a newly merged grapheme?
        // Doc: "a\u{200d}b" vs "aX\u{0301}":
        // Start with "aX\u{0301}"? No, X and \u{0301} are one grapheme "X\u{0301}" (chars 1..3).
        // Start with "a" + "X" (2 chars) where v2 cursor = 2 (right after 'X', before end).
        // Wait: replace 1..2 ("X") with "" while inserting "\u{0301}"? Still right affinity -> 2.
        // Start with "a" + "b" + "\u{0301}"? No, start with "a" + "\n" + "\u{0301}" (3 chars: 0:'a', 1:'\n', 2:'\u{0301}').
        // v2 cursor is at 2 (right after '\n', before '\u{0301}', valid boundary because '\n' breaks grapheme!).
        // Also v2 top_line = 1 (line_start(1) == 1? No, line_start(1) == 2, right before '\u{0301}'!).
        // Now delete '\n' (1..2 -> ""):
        // Text becomes "a\u{0301}" (len 2; grapheme cluster spans 0..2!).
        // Change: start=1, old_end=2, new_end=1.
        // At old_end=2 (start < old_end): TextChange::map(2, true) = 1, TextChange::map(2, false) = 1!
        // Index 1 is INSIDE the merged grapheme "a\u{0301}" (is_grapheme_boundary(1) == false)!
        // Right-affinity cursor snaps FORWARD to 2; left-affinity viewport anchor snaps BACKWARD to 0!
        let len = ws.document(doc1).unwrap().document.len_chars();
        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.replace(0..len, "a\n\u{0301}").unwrap();
            (c, Some(c))
        });
        {
            let v2_tab = ws.view_mut(v2).unwrap().1;
            v2_tab.cursor = 2;
            v2_tab.top_line = 1; // line_start(1) == 2
        }
        ws.edit_transaction(doc1, v1, true, |doc| {
            let c = doc.replace(1..2, "").unwrap();
            (c, Some(c))
        });
        let v2_tab = ws.view(v2).unwrap().1;
        assert_eq!(v2_tab.cursor, 2);
        assert_eq!(v2_tab.top_line, 0);
    }

    #[test]
    fn sidebar_width_clamping_respects_editor_minimum() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ws, _) = Workspace::new(dir.path().to_path_buf());
        assert!(ws.set_sidebar_width(50, 120, false));
        assert_eq!(ws.sidebar_width, chrome::MAX_SIDEBAR_WIDTH);
        assert!(ws.set_sidebar_width(5, 120, false));
        assert_eq!(ws.sidebar_width, chrome::MIN_SIDEBAR_WIDTH);
        // With viewport 35: max sidebar is 35 - 1 - 12 = 22 (blank doc min pane width 12)
        assert!(ws.set_sidebar_width(30, 35, false));
        assert_eq!(ws.sidebar_width, 22);
        // Too narrow (< 31): refuses resize without clobbering stored width
        assert!(!ws.set_sidebar_width(18, 25, false));
        assert_eq!(ws.sidebar_width, 22);
    }

    #[test]
    fn unreadable_or_missing_explorer_root_is_nonfatal() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does_not_exist");
        let (ws, warning) = Workspace::new(missing);
        assert!(ws.explorer.is_none());
        assert!(warning.is_some());
        assert_eq!(ws.panes().len(), 1);
        assert_eq!(ws.active_pane().unwrap().tabs().len(), 1);
    }

    #[test]
    fn explorer_navigation_keeps_root_and_label_in_sync_with_documents_untouched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("inner")).unwrap();
        std::fs::write(dir.path().join("inner/note.txt"), b"from tree").unwrap();

        let (mut ws, warning) = Workspace::new(dir.path().join("inner"));
        assert!(warning.is_none());
        // Constructor syncs to the explorer's canonical root.
        assert_eq!(ws.explorer_root, dir.path().join("inner"));
        assert_eq!(ws.root_label, dir.path().join("inner").to_string_lossy());

        let blank_before = ws.active_document().unwrap().document.text().to_string();

        // Backspace path: parent listing selects the previous root folder.
        assert!(ws.explorer_go_parent().unwrap());
        assert_eq!(ws.explorer_root, dir.path());
        assert_eq!(ws.root_label, dir.path().to_string_lossy());
        let exp = ws.explorer.as_ref().unwrap();
        assert_eq!(exp.selected(), 0);
        assert_eq!(exp.rows()[0].name, "inner");

        // Enter path: the selected folder becomes the browsing root again.
        assert!(ws.explorer_enter_selected().unwrap());
        assert_eq!(ws.explorer_root, dir.path().join("inner"));
        assert_eq!(ws.root_label, dir.path().join("inner").to_string_lossy());
        let exp = ws.explorer.as_ref().unwrap();
        assert_eq!(exp.rows().len(), 1);
        assert_eq!(exp.rows()[0].name, "note.txt");

        // Shared documents and the process working directory stay untouched.
        assert_eq!(
            ws.active_document().unwrap().document.text().to_string(),
            blank_before
        );
        assert!(ws.active_document().unwrap().path.is_none());
    }

    #[test]
    fn explorer_retry_syncs_recovered_symlink_root_before_parent_navigation() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let alias = dir.path().join("late-alias");
        let real = dir.path().join("actual/inner");
        let (mut ws, warning) = Workspace::new(alias.clone());
        assert!(warning.is_some());
        assert!(ws.retry_explorer().is_err());
        assert!(ws.explorer.is_none());
        assert_eq!(ws.explorer_root, alias);

        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("note.txt"), b"recovered").unwrap();
        symlink(&real, &alias).unwrap();
        ws.retry_explorer().unwrap();

        assert_eq!(ws.explorer_root, real);
        assert_eq!(ws.root_label, real.to_string_lossy());
        assert_eq!(
            ws.explorer.as_ref().unwrap().rows()[0].path,
            real.join("note.txt")
        );
        assert!(ws.explorer_go_parent().unwrap());
        assert_eq!(ws.explorer_root, dir.path().join("actual"));
        assert_eq!(
            ws.explorer.as_ref().unwrap().selected_entry().unwrap().path,
            real
        );
    }
}

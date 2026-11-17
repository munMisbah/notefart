use std::{
    borrow::Cow,
    fmt::Write as _,
    io,
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::{
    chrome,
    clipboard::{Clipboard, ClipboardError},
    explorer::EntryKind,
    field::{Field, FieldKind},
    file_io::{self, FileError},
    highlight::Highlighter,
    input::{Input, InputEvent},
    layout::{Axis, Divider, Layout, PaneMinimum},
    scrollbar::Scrollbar,
    search::{self, Query},
    session::SessionStore,
    terminal::TerminalSession,
    theme::{IconMode, Options, Theme},
    ui::{self, ChoiceKind, Dialog, Geometry, PaneScreen, Screen, StatusLevel, TabCaption},
    workspace::{DocumentBuffer, EditViewPolicy, Workspace},
};

const TICK: Duration = Duration::from_millis(50);
const HIGHLIGHT_IDLE_TICK: Duration = Duration::from_millis(16);
const DOUBLE_CLICK: Duration = Duration::from_millis(450);
const MAX_PATH_BYTES: usize = 4096;
const MAX_SEARCH_BYTES: usize = 4096;
const MAX_COUNTED_MATCHES: usize = 100;
const HIGHLIGHT_BUDGET: usize = 200;
const SCROLL_WIDTH_TICK_BUDGET: Duration = Duration::from_millis(2);

#[derive(Clone, Copy)]
enum AfterSave {
    CloseView { view_id: u64 },
    ClosePane { pane_id: u64 },
    Quit,
}

#[derive(Clone, Copy)]
enum PathAction {
    Open,
    Save { doc_id: u64 },
}

enum Modal {
    Path {
        action: PathAction,
        value: Field,
    },
    Unsaved {
        doc_id: u64,
        after: AfterSave,
        focused: usize,
    },
    Overwrite {
        doc_id: u64,
        path: PathBuf,
        focused: usize,
    },
    Conflict {
        doc_id: u64,
        path: PathBuf,
        focused: usize,
    },
    Search {
        replace_mode: bool,
        replacement_focus: bool,
    },
    Help {
        scroll: usize,
    },
}

impl Modal {
    fn choice(&self) -> Option<(ChoiceKind, usize)> {
        match self {
            Self::Unsaved { focused, .. } => Some((ChoiceKind::Unsaved, *focused)),
            Self::Overwrite { focused, .. } => Some((ChoiceKind::Overwrite, *focused)),
            Self::Conflict { focused, .. } => Some((ChoiceKind::Conflict, *focused)),
            _ => None,
        }
    }

    fn target<'a>(&'a self, workspace: &'a Workspace) -> Option<&'a Path> {
        match self {
            Self::Unsaved { doc_id, .. } => workspace.document(*doc_id).map(|buf| {
                buf.path
                    .as_deref()
                    .unwrap_or_else(|| Path::new(buf.label()))
            }),
            Self::Overwrite { path, .. } | Self::Conflict { path, .. } => Some(path),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum ScrollbarTarget {
    Editor {
        pane_id: u64,
        view_id: u64,
        revision: u64,
        text_area: Rect,
    },
    Explorer {
        rows: usize,
    },
    Help {
        viewport: (u16, u16),
    },
}

#[derive(Clone, Copy)]
enum DragState {
    Text {
        pane_id: u64,
        view_id: u64,
        x: u16,
        y: u16,
    },
    ExplorerDivider {
        anchor_x: u16,
        start_width: u16,
    },
    SplitDivider {
        divider: Divider,
    },
    Scrollbar {
        target: ScrollbarTarget,
        bar: Scrollbar,
        anchor: u16,
    },
    Field {
        kind: FieldKind,
    },
}

#[derive(Clone)]
struct SearchMatch {
    pane_id: u64,
    view_id: u64,
    document_id: u64,
    doc_revision: u64,
    query: String,
    range: Range<usize>,
}

struct SearchCache {
    document_id: u64,
    doc_revision: u64,
    compiled: Query,
    index: search::MatchIndex,
}

#[derive(Default)]
struct SearchState {
    query: Field,
    replacement: Field,
    last_match: Option<SearchMatch>,
    cache: Option<SearchCache>,
    feedback: String,
    feedback_has_position: bool,
}

impl SearchState {
    fn update_feedback(&mut self, range: Option<&Range<usize>>, wrapped: bool) {
        let Some(cache) = &self.cache else { return };
        let count = cache.index.starts.len();
        let position = range.and_then(|r| cache.index.starts.binary_search(&r.start).ok());
        self.feedback.clear();
        self.feedback_has_position = range.is_some();
        if count == 0 {
            self.feedback.push_str("No matches");
            return;
        }
        if let Some(position) = position {
            write!(self.feedback, "Match {} of {count}", position + 1)
                .expect("writing to String cannot fail");
        } else if range.is_some() && cache.index.has_more {
            write!(self.feedback, "Match >{count} of {count}")
                .expect("writing to String cannot fail");
        } else {
            write!(self.feedback, "Matches: {count}").expect("writing to String cannot fail");
        }
        if cache.index.has_more {
            self.feedback.push('+');
        }
        if wrapped {
            self.feedback.push_str(" (wrapped)");
        }
    }

    fn current_feedback(&self, workspace: &Workspace) -> Option<&str> {
        let cache = self.cache.as_ref()?;
        let buf = workspace.active_document()?;
        if cache.document_id != buf.id
            || cache.doc_revision != buf.document.revision()
            || cache.compiled.needle() != self.query.as_str()
            || self.feedback.is_empty()
        {
            return None;
        }
        if self.feedback_has_position {
            let matched = self.last_match.as_ref()?;
            let view = workspace.active_view()?;
            if matched.pane_id != workspace.active_pane_id()
                || matched.view_id != view.id
                || matched.document_id != buf.id
                || matched.doc_revision != buf.document.revision()
                || matched.query != self.query.as_str()
                || view.selection().as_ref() != Some(&matched.range)
            {
                return None;
            }
        }
        Some(&self.feedback)
    }
}

#[derive(Debug)]
struct StatusMessage {
    level: StatusLevel,
    text: String,
}

impl StatusMessage {
    fn new(level: StatusLevel, text: String) -> Self {
        Self { level, text }
    }
}

struct App {
    workspace: Workspace,
    session_store: Option<SessionStore>,
    highlighter: Highlighter,
    highlight_pending: bool,
    theme: Theme,
    icons: IconMode,
    search: SearchState,
    geometry: Geometry,
    hit_targets_valid: bool,
    viewport: (u16, u16),
    clipboard: Clipboard,
    status: StatusMessage,
    modal: Option<Modal>,
    help_return: Option<Modal>,
    after_save: Option<AfterSave>,
    path_retry: Option<(PathAction, Field)>,
    input_sequence_pending: bool,
    quit: bool,
    drag: Option<DragState>,
    last_click: Option<(u64, u16, u16, Instant)>,
    mouse_down: Option<(u16, u16)>,
    dragged: bool,
    layout_scratch: Layout,
    mins_scratch: Vec<PaneMinimum>,
    hl_docs_scratch: Vec<u64>,
    hl_ranges_scratch: Vec<Range<usize>>,
    scroll_scan_cursor: usize,
    scroll_metrics_dirty: bool,
}

impl App {
    fn new_with_root(root: PathBuf, options: Options) -> Self {
        let (workspace, exp_warn) = Workspace::new(root);
        Self::with_workspace(workspace, exp_warn, options)
    }

    fn with_workspace(workspace: Workspace, exp_warn: Option<String>, options: Options) -> Self {
        let clipboard = Clipboard::new(options.clipboard);
        let status = match exp_warn {
            Some(warn) => StatusMessage::new(
                StatusLevel::Warning,
                format!(
                    "Warn: explorer: {warn} | Clipboard: {}",
                    clipboard.startup_note()
                ),
            ),
            None => StatusMessage::new(
                StatusLevel::Info,
                format!(
                    "Clipboard: {} | Ctrl+Q exits safely",
                    clipboard.startup_note()
                ),
            ),
        };
        Self {
            workspace,
            session_store: None,
            highlighter: Highlighter::new(),
            highlight_pending: false,
            theme: Theme::new(options.theme, options.colors),
            icons: options.icons,
            search: SearchState::default(),
            geometry: Geometry::default(),
            hit_targets_valid: false,
            viewport: (120, 36),
            clipboard,
            status,
            modal: None,
            help_return: None,
            after_save: None,
            path_retry: None,
            input_sequence_pending: false,
            quit: false,
            drag: None,
            last_click: None,
            mouse_down: None,
            dragged: false,
            layout_scratch: Layout::default(),
            mins_scratch: Vec::new(),
            hl_docs_scratch: Vec::new(),
            hl_ranges_scratch: Vec::new(),
            scroll_scan_cursor: 0,
            scroll_metrics_dirty: false,
        }
    }

    fn bootstrap(path: Option<PathBuf>, options: Options) -> io::Result<Self> {
        match path {
            None => {
                let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                Ok(Self::new_with_root(root, options))
            }
            Some(p) if std::fs::metadata(&p).is_ok_and(|m| m.is_dir()) => {
                let root = if p == Path::new(".") {
                    std::env::current_dir().unwrap_or(p)
                } else {
                    std::fs::canonicalize(&p)
                        .unwrap_or_else(|_| std::path::absolute(&p).unwrap_or(p))
                };
                Ok(Self::new_with_root(root, options))
            }
            Some(p) => {
                let root = resolve_parent_dir(&p);
                let mut app = Self::new_with_root(root, options);
                app.workspace.explorer_visible = false;
                let exp_warn = app.workspace.explorer.is_none();
                match file_io::load(&p) {
                    Ok(loaded) => {
                        app.workspace.init_startup_file(p, loaded);
                        app.highlight_pending = true;
                        if let Some(ro) = app
                            .workspace
                            .active_document()
                            .and_then(|d| d.read_only.clone())
                        {
                            app.status = StatusMessage::new(
                                StatusLevel::Warning,
                                format!(
                                    "Read-only: {ro} | Clipboard: {}",
                                    app.clipboard.startup_note()
                                ),
                            );
                        } else if !exp_warn {
                            app.status = StatusMessage::new(
                                StatusLevel::Success,
                                format!("Opened | Clipboard: {}", app.clipboard.startup_note()),
                            );
                        }
                    }
                    Err(FileError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                        app.workspace.set_startup_missing_file(p);
                        if !exp_warn {
                            app.status = StatusMessage::new(
                                StatusLevel::Info,
                                format!(
                                    "New file; created only when saved | Clipboard: {}",
                                    app.clipboard.startup_note()
                                ),
                            );
                        }
                    }
                    Err(error) => return Err(io::Error::other(error.to_string())),
                }
                Ok(app)
            }
        }
    }

    fn bootstrap_session(options: Options) -> io::Result<Self> {
        let store = SessionStore::open_default()?;
        let loaded = store.load().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("could not restore {}: {error}", store.path().display()),
            )
        })?;
        let (mut app, restored) = match loaded {
            Some(loaded) => {
                let (workspace, warning) = Workspace::restore_session(loaded)?;
                (Self::with_workspace(workspace, warning, options), true)
            }
            None => (Self::bootstrap(None, options)?, false),
        };
        app.workspace.explorer_visible = true;
        app.status = StatusMessage::new(
            app.status.level,
            format!(
                "Session {}; Ctrl+Q keeps buffers | {}",
                if restored { "restored" } else { "active" },
                app.status.text,
            ),
        );
        app.highlight_pending = restored;
        app.session_store = Some(store);
        Ok(app)
    }

    fn invalidate_hit_targets(&mut self) {
        self.hit_targets_valid = false;
        self.drag = None;
        self.last_click = None;
        self.mouse_down = None;
        self.dragged = false;
    }

    fn invalidate_hit_targets_keep_drag(&mut self) {
        self.hit_targets_valid = false;
        self.last_click = None;
        self.mouse_down = None;
    }

    fn max_editor_area(&self) -> Rect {
        let (w, h) = self.viewport;
        // Compact chrome layout: split preflight must never be refused because
        // of decorative header/dock space the rich layout would add.
        chrome::max_editor_area(Rect::new(0, 0, w, h))
    }

    fn active_text_area(&self) -> Rect {
        let pane_id = self.workspace.active_pane_id();
        self.geometry
            .pane(pane_id)
            .map_or(Rect::default(), |p| p.text_area)
    }

    fn open_tab_from_path(&mut self, path: PathBuf) -> Result<(), FileError> {
        let outcome = self.workspace.open_path(path)?;
        self.workspace.explorer_focused = false;
        self.search.last_match = None;
        self.invalidate_hit_targets();
        self.highlight_pending = true;
        if outcome.already_in_pane {
            self.status =
                StatusMessage::new(StatusLevel::Info, "Switched to already-open tab".into());
        } else {
            let ro = self
                .workspace
                .document(outcome.document_id)
                .and_then(|d| d.read_only.clone());
            self.status = match ro {
                Some(reason) => StatusMessage::new(
                    StatusLevel::Warning,
                    format!(
                        "Read-only: {reason} | Clipboard: {}",
                        self.clipboard.startup_note()
                    ),
                ),
                None => StatusMessage::new(
                    StatusLevel::Success,
                    format!(
                        "{} | Clipboard: {}",
                        if outcome.shared_existing {
                            "Opened shared view"
                        } else {
                            "Opened"
                        },
                        self.clipboard.startup_note(),
                    ),
                ),
            };
        }
        Ok(())
    }

    fn replace_in_active_tab(&mut self, text: &str) {
        let Some(view) = self.workspace.active_view() else {
            return;
        };
        let view_id = view.id;
        let doc_id = view.document_id;
        let range = view.selection().unwrap_or(view.cursor..view.cursor);
        let Some(buf) = self.workspace.document(doc_id) else {
            return;
        };
        if let Some(reason) = &buf.read_only {
            self.status = StatusMessage::new(StatusLevel::Warning, format!("Read-only: {reason}"));
            return;
        }
        let Some((result, mutated)) =
            self.workspace
                .edit_transaction(doc_id, view_id, EditViewPolicy::FollowEdit, |doc| match doc
                    .replace(range, text)
                {
                    Ok(cursor) => (Ok(cursor), Some(cursor)),
                    Err(err) => (Err(err), None),
                })
        else {
            return;
        };
        match result {
            Ok(_) => {
                if mutated {
                    self.search.last_match = None;
                    self.highlight_pending = true;
                    self.invalidate_hit_targets();
                }
            }
            Err(error) => self.status = StatusMessage::new(StatusLevel::Error, error.to_string()),
        }
    }

    fn indent_in_active_tab(&mut self, dedent: bool) {
        let Some(view) = self.workspace.active_view() else {
            return;
        };
        let view_id = view.id;
        let doc_id = view.document_id;
        let selection = view.selection();
        let range = selection.clone().unwrap_or(view.cursor..view.cursor);
        let Some(buf) = self.workspace.document(doc_id) else {
            return;
        };
        let multiline = selection.is_some()
            && buf.document.line_of(range.start) != buf.document.line_of(range.end);
        if !dedent && !multiline {
            let spaces = "    ";
            let stop = crate::document::TAB_STOP;
            let count = stop - buf.document.column_of(view.cursor) % stop;
            self.replace_in_active_tab(&spaces[..count]);
            return;
        }
        if let Some(reason) = &buf.read_only {
            self.status = StatusMessage::new(StatusLevel::Warning, format!("Read-only: {reason}"));
            return;
        }
        let Some((result, mutated)) = self.workspace.edit_transaction(
            doc_id,
            view_id,
            EditViewPolicy::PreserveSelection,
            |doc| (doc.indent_lines(range, dedent), None),
        ) else {
            return;
        };
        match result {
            Ok(()) if mutated => {
                self.search.last_match = None;
                self.highlight_pending = true;
                self.invalidate_hit_targets();
            }
            Ok(()) => {}
            Err(error) => self.status = StatusMessage::new(StatusLevel::Error, error.to_string()),
        }
    }

    fn paste_in_active_tab(&mut self, text: &str) {
        // File buffers use LF internally; incoming control bytes remain data.
        let normalized = normalize_paste(text);
        self.replace_in_active_tab(normalized.as_ref());
    }

    fn new_tab(&mut self) {
        self.workspace.add_blank_tab();
        self.workspace.explorer_focused = false;
        self.search.last_match = None;
        self.invalidate_hit_targets();
        self.highlight_pending = true;
        self.status = StatusMessage::new(StatusLevel::Info, "New tab".into());
    }

    fn open_path_dialog(&mut self) {
        self.drag = None;
        self.path_retry = None;
        self.modal = Some(Modal::Path {
            action: PathAction::Open,
            value: Field::new(String::new()),
        });
    }

    fn split_active_pane(&mut self, axis: Axis) {
        let max_area = self.max_editor_area();
        if self.workspace.split_active_pane(axis, max_area).is_some() {
            self.workspace.explorer_focused = false;
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.highlight_pending = true;
            self.status = StatusMessage::new(
                StatusLevel::Info,
                match axis {
                    Axis::Columns => "Split left/right".into(),
                    Axis::Rows => "Split top/bottom".into(),
                },
            );
        } else {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Cannot split: terminal too small for minimum pane size".into(),
            );
        }
    }

    fn cycle_pane(&mut self, delta: isize) {
        self.workspace.explorer_focused = false;
        if self.workspace.cycle_pane(delta) {
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.highlight_pending = true;
        }
    }

    fn effective_editor_area_for_resize(&mut self) -> Rect {
        let (w, h) = self.viewport;
        let (tree_min_w, tree_min_h) = self.workspace.tree_minimum();
        let body = chrome::regions(Rect::new(0, 0, w, h), tree_min_h).body;
        let enabled = self.workspace.explorer_visible
            && self.workspace.explorer.is_some()
            && self.workspace.sidebar_width != 0;
        let active_min_w = self
            .workspace
            .active_document()
            .map_or(chrome::MIN_EDITOR_WIDTH, |b| {
                chrome::pane_min_width(b.document.line_count())
            });
        chrome::sidebar(
            body,
            self.workspace.sidebar_width,
            active_min_w,
            (tree_min_w, tree_min_h),
            enabled,
        )
        .editor
    }

    fn resize_nearest_divider(&mut self, axis: Axis, delta: isize) {
        if axis == Axis::Columns && self.workspace.explorer_focused {
            let editor = self.effective_editor_area_for_resize();
            if editor.x == 0 {
                self.status = StatusMessage::new(
                    StatusLevel::Warning,
                    "Explorer hidden; show it or enlarge the terminal before resizing".into(),
                );
                return;
            }
            // The sidebar starts at x=0; the editor follows its one-column gap.
            let visible_width = editor.x - 1;
            let step = u16::try_from(delta.unsigned_abs()).unwrap_or(u16::MAX);
            let desired = if delta < 0 {
                visible_width.saturating_sub(step)
            } else {
                visible_width.saturating_add(step)
            };
            let (min_w, min_h) = self.workspace.tree_minimum();
            let collapsed = u32::from(editor.width) < min_w || u32::from(editor.height) < min_h;
            if self
                .workspace
                .set_sidebar_width(desired, self.viewport.0, collapsed)
            {
                self.highlight_pending = true;
                self.invalidate_hit_targets();
            }
            return;
        }
        let active_pane_id = self.workspace.active_pane_id();
        let (active_rect, splits) = if self.hit_targets_valid && !self.geometry.panes.is_empty() {
            let Some(pane_geom) = self.geometry.pane(active_pane_id) else {
                return;
            };
            (pane_geom.area, self.geometry.splits.as_slice())
        } else {
            let area = self.effective_editor_area_for_resize();
            self.workspace.pane_minimums(&mut self.mins_scratch);
            self.workspace.tree.layout(
                area,
                active_pane_id,
                &self.mins_scratch,
                &mut self.layout_scratch,
            );
            let Some(pane_rect) = self
                .layout_scratch
                .panes
                .iter()
                .find(|p| p.id == active_pane_id)
            else {
                return;
            };
            (pane_rect.area, self.layout_scratch.dividers.as_slice())
        };

        let nearest = splits
            .iter()
            .filter(|d| {
                d.axis == axis
                    && d.parent.x <= active_rect.x
                    && d.parent.y <= active_rect.y
                    && d.parent.right() >= active_rect.right()
                    && d.parent.bottom() >= active_rect.bottom()
            })
            .min_by_key(|d| u32::from(d.parent.width) * u32::from(d.parent.height))
            .copied();

        let Some(div) = nearest else {
            self.status =
                StatusMessage::new(StatusLevel::Info, "No split divider on that axis".into());
            return;
        };

        let current_coord = match axis {
            Axis::Columns => div.area.x,
            Axis::Rows => div.area.y,
        };
        let target_coord = if delta < 0 {
            current_coord.saturating_sub(u16::try_from(-delta).unwrap_or(u16::MAX))
        } else {
            current_coord.saturating_add(u16::try_from(delta).unwrap_or(u16::MAX))
        };

        if self.workspace.tree.resize(&div, target_coord) {
            self.highlight_pending = true;
            self.invalidate_hit_targets();
        }
    }

    fn request_close_active_view(&mut self) {
        let pane_id = self.workspace.active_pane_id();
        let view_id = self.workspace.active_view_id();
        self.request_close_view(pane_id, view_id);
    }

    fn request_close_view(&mut self, pane_id: u64, view_id: u64) {
        let Some(doc_id) = self
            .workspace
            .pane(pane_id)
            .and_then(|p| p.tab(view_id))
            .map(|t| t.document_id)
        else {
            return;
        };
        let Some(buf) = self.workspace.document(doc_id) else {
            return;
        };
        if buf.document.is_dirty() && self.workspace.view_count_for_document(doc_id) == 1 {
            self.drag = None;
            self.workspace.activate_view(pane_id, view_id);
            self.workspace.explorer_focused = false;
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.modal = Some(Modal::Unsaved {
                doc_id,
                after: AfterSave::CloseView { view_id },
                focused: ChoiceKind::Unsaved.cancel_index(),
            });
        } else {
            self.close_view_unconditional(view_id);
            self.status = StatusMessage::new(StatusLevel::Info, "Closed tab".into());
        }
    }

    fn close_view_unconditional(&mut self, view_id: u64) {
        let outcome = self.workspace.close_view_unconditional(view_id);
        if let Some(removed_doc_id) = outcome.removed_document {
            self.highlighter.forget(removed_doc_id);
        }
        self.search.last_match = None;
        self.invalidate_hit_targets();
        self.highlight_pending = true;
    }

    fn request_close_active_pane(&mut self) {
        let pane_id = self.workspace.active_pane_id();
        self.drag = None;
        self.continue_close_pane(pane_id);
    }

    fn continue_close_pane(&mut self, pane_id: u64) {
        if let Some((view_id, doc_id)) = self.workspace.first_dirty_final_view_in_pane(pane_id) {
            self.workspace.activate_view(pane_id, view_id);
            self.workspace.explorer_focused = false;
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.modal = Some(Modal::Unsaved {
                doc_id,
                after: AfterSave::ClosePane { pane_id },
                focused: ChoiceKind::Unsaved.cancel_index(),
            });
        } else {
            for removed_doc in self.workspace.close_pane_unconditional(pane_id) {
                self.highlighter.forget(removed_doc);
            }
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.highlight_pending = true;
            self.modal = None;
            self.after_save = None;
            self.status = StatusMessage::new(StatusLevel::Info, "Closed pane".into());
        }
    }

    fn request_quit(&mut self) {
        self.drag = None;
        if let Some(store) = &self.session_store {
            match self
                .workspace
                .session_image()
                .and_then(|image| store.save(&image))
            {
                Ok(()) => self.quit = true,
                Err(error) => {
                    self.status = StatusMessage::new(
                        StatusLevel::Error,
                        format!("Save failed: session: {error}; still in editor"),
                    );
                }
            }
            return;
        }
        self.continue_quit();
    }

    fn continue_quit(&mut self) {
        if let Some(doc_id) = self.workspace.first_dirty_document_id() {
            if let Some((pane_id, view_id)) = self.workspace.first_view_for_document(doc_id) {
                self.workspace.activate_view(pane_id, view_id);
            }
            self.workspace.explorer_focused = false;
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.modal = Some(Modal::Unsaved {
                doc_id,
                after: AfterSave::Quit,
                focused: ChoiceKind::Unsaved.cancel_index(),
            });
        } else {
            for id in self.workspace.drain_all_documents() {
                self.highlighter.forget(id);
            }
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.modal = None;
            self.after_save = None;
            self.quit = true;
        }
    }

    fn cycle_tab(&mut self, delta: isize) {
        if self.workspace.cycle_tab(delta) {
            self.search.last_match = None;
            self.invalidate_hit_targets();
            self.highlight_pending = true;
        }
    }

    fn save_active_document(&mut self) {
        let Some(doc_id) = self.workspace.active_view().map(|v| v.document_id) else {
            return;
        };
        self.save_document(doc_id);
    }

    fn save_document(&mut self, doc_id: u64) {
        let Some(path) = self.workspace.document(doc_id).and_then(|d| d.path.clone()) else {
            self.save_document_as(doc_id);
            return;
        };
        self.save_document_to(doc_id, path, false, false);
    }

    fn save_active_document_as(&mut self) {
        let Some(doc_id) = self.workspace.active_view().map(|v| v.document_id) else {
            return;
        };
        self.save_document_as(doc_id);
    }

    fn save_document_as(&mut self, doc_id: u64) {
        self.path_retry = None;
        let value = self
            .workspace
            .document(doc_id)
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_owned();
        self.modal = Some(Modal::Path {
            action: PathAction::Save { doc_id },
            value: Field::new(value),
        });
    }

    fn restore_failed_path(&mut self) {
        self.modal = self
            .path_retry
            .take()
            .map(|(action, value)| Modal::Path { action, value });
        if self.modal.is_none() {
            self.after_save = None;
        }
        self.invalidate_hit_targets();
    }

    fn save_document_to(&mut self, doc_id: u64, path: PathBuf, overwrite: bool, save_as: bool) {
        let Some(buf) = self.workspace.document(doc_id) else {
            self.after_save = None;
            self.modal = None;
            self.path_retry = None;
            return;
        };
        if let Some(reason) = &buf.read_only {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                format!("Read-only: {reason}; save blocked (no implicit normalization)"),
            );
            self.restore_failed_path();
            return;
        }
        if let Some(other_doc_id) = self.workspace.check_save_as_collision(doc_id, &path) {
            let other_label = self
                .workspace
                .document(other_doc_id)
                .map_or("another document", DocumentBuffer::label);
            self.status = StatusMessage::new(
                StatusLevel::Error,
                format!(
                    "Save failed: target matches open tab '{other_label}'; switch to or close that tab first"
                ),
            );
            self.restore_failed_path();
            return;
        }
        let expected = if !save_as && buf.path.as_ref() == Some(&path) {
            buf.baseline.as_ref()
        } else {
            None
        };
        // Materializing once at explicit save is separate from input/render work.
        let text = buf.document.text().to_string();
        let format = buf.format;
        match file_io::save(&path, &text, format, expected, overwrite) {
            Ok(baseline) => {
                let old_path = self.workspace.document(doc_id).and_then(|d| d.path.clone());
                let path_changed = old_path.as_ref() != Some(&path);
                if let Some(buf) = self.workspace.document_mut(doc_id) {
                    buf.update_path(Some(path));
                    buf.baseline = Some(baseline);
                    buf.document.mark_saved();
                }
                if path_changed {
                    self.highlighter.forget(doc_id);
                    self.highlight_pending = true;
                }
                self.invalidate_hit_targets();
                self.status = StatusMessage::new(StatusLevel::Success, "Saved".into());
                self.modal = None;
                self.path_retry = None;
                if let Some(after) = self.after_save.take() {
                    match after {
                        AfterSave::CloseView { view_id } => self.close_view_unconditional(view_id),
                        AfterSave::ClosePane { pane_id } => self.continue_close_pane(pane_id),
                        AfterSave::Quit => self.continue_quit(),
                    }
                }
            }
            Err(FileError::Exists) => {
                self.modal = Some(Modal::Overwrite {
                    doc_id,
                    path,
                    focused: ChoiceKind::Overwrite.cancel_index(),
                })
            }
            Err(FileError::Conflict) => {
                self.modal = Some(Modal::Conflict {
                    doc_id,
                    path,
                    focused: ChoiceKind::Conflict.cancel_index(),
                })
            }
            Err(error) => {
                self.status =
                    StatusMessage::new(StatusLevel::Error, format!("Save failed: {error}"));
                self.restore_failed_path();
            }
        }
    }

    fn reload_document(&mut self, doc_id: u64, path: PathBuf) {
        match file_io::load(&path) {
            Ok(loaded) => {
                self.path_retry = None;
                self.highlighter.forget(doc_id);
                self.highlight_pending = true;
                self.workspace.reload_document(doc_id, path, loaded);
                if self
                    .search
                    .last_match
                    .as_ref()
                    .is_some_and(|m| m.document_id == doc_id)
                {
                    self.search.last_match = None;
                }
                // Reload replaces Document and can reuse its revision number.
                if self
                    .search
                    .cache
                    .as_ref()
                    .is_some_and(|cache| cache.document_id == doc_id)
                {
                    self.search.cache = None;
                    self.search.feedback.clear();
                    self.search.feedback_has_position = false;
                }
                self.invalidate_hit_targets();
                self.status = StatusMessage::new(
                    StatusLevel::Success,
                    "Reloaded; local changes discarded by explicit request".into(),
                );
            }
            Err(error) => {
                self.status = StatusMessage::new(
                    StatusLevel::Error,
                    format!("Reload failed: {error}; buffer retained"),
                );
            }
        }
    }

    fn open_help(&mut self) {
        if matches!(self.modal, Some(Modal::Help { .. })) {
            return;
        }
        self.drag = None;
        self.help_return = self.modal.take();
        self.modal = Some(Modal::Help { scroll: 0 });
        self.invalidate_hit_targets();
    }

    fn close_help(&mut self) {
        if matches!(self.modal, Some(Modal::Help { .. })) {
            self.modal = self.help_return.take();
            self.invalidate_hit_targets();
        }
    }

    fn cancel_modal(&mut self) {
        let was_search = matches!(self.modal, Some(Modal::Search { .. }));
        self.modal = None;
        self.after_save = None;
        self.help_return = None;
        self.path_retry = None;
        self.invalidate_hit_targets();
        self.status = StatusMessage::new(
            StatusLevel::Info,
            if was_search {
                "Search closed".into()
            } else {
                "Cancelled; document unchanged".into()
            },
        );
    }

    fn path_input(&mut self, text: &str) {
        if self.viewport.0 < ui::MIN_DIALOG_SIZE.0 || self.viewport.1 < ui::MIN_DIALOG_SIZE.1 {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Resize to at least 20x6 to edit a dialog; Esc cancels".into(),
            );
            return;
        }
        if text.chars().any(char::is_control) {
            self.status = StatusMessage::new(
                StatusLevel::Error,
                "Path input rejected: control character or over 4096 bytes".into(),
            );
            return;
        }
        if let Some(Modal::Path { value, .. }) = &mut self.modal {
            if !value.insert(text, MAX_PATH_BYTES) {
                self.status = StatusMessage::new(
                    StatusLevel::Error,
                    "Path input rejected: control character or over 4096 bytes".into(),
                );
            } else {
                self.invalidate_hit_targets();
            }
        }
    }

    fn search_input(&mut self, text: &str, to_replacement: bool) {
        if self.viewport.0 < ui::MIN_SEARCH_SIZE.0 || self.viewport.1 < ui::MIN_SEARCH_SIZE.1 {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Resize to at least 40x10 to edit search dialog; Esc cancels".into(),
            );
            return;
        }
        let field = if to_replacement {
            &mut self.search.replacement
        } else {
            &mut self.search.query
        };
        let retained = field.as_str().len() - field.selected().map_or(0, str::len);
        let available = MAX_SEARCH_BYTES.saturating_sub(retained);
        // CRLF removes one byte; reject before allocating normalized clipboard text.
        if text.len() > available
            && (text.len() > available * 2
                || text.len()
                    - text
                        .as_bytes()
                        .windows(2)
                        .filter(|pair| *pair == b"\r\n")
                        .count()
                    > available)
        {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Warn: search field limit is 4096 bytes".into(),
            );
            return;
        }
        let normalized = normalize_paste(text);
        let text = normalized.as_ref();
        let before_rev = field.revision();
        if !field.insert(text, MAX_SEARCH_BYTES) {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Warn: search field limit is 4096 bytes".into(),
            );
            return;
        }
        if !to_replacement && field.revision() != before_rev {
            self.search.last_match = None;
        }
        self.invalidate_hit_targets();
    }

    fn modal_copy(&mut self, cut: bool) {
        match &mut self.modal {
            Some(Modal::Path { value, .. }) => {
                let Some(sel) = value.selected() else {
                    self.status = StatusMessage::new(StatusLevel::Info, no_selection_status(cut));
                    return;
                };
                let text = sel.to_owned();
                match self.clipboard.copy(text) {
                    Ok(target) => {
                        if cut {
                            value.insert("", MAX_PATH_BYTES);
                            self.invalidate_hit_targets();
                        }
                        self.status = StatusMessage::new(
                            StatusLevel::Success,
                            copy_success_status(cut, target),
                        );
                    }
                    Err(error) => {
                        self.status = StatusMessage::new(
                            StatusLevel::Error,
                            copy_failure_status(cut, &error),
                        );
                    }
                }
            }
            Some(Modal::Search {
                replace_mode,
                replacement_focus,
            }) => {
                let to_replacement = *replace_mode && *replacement_focus;
                let (field, is_query) = if to_replacement {
                    (&mut self.search.replacement, false)
                } else {
                    (&mut self.search.query, true)
                };
                let Some(sel) = field.selected() else {
                    self.status = StatusMessage::new(StatusLevel::Info, no_selection_status(cut));
                    return;
                };
                let text = sel.to_owned();
                match self.clipboard.copy(text) {
                    Ok(target) => {
                        if cut {
                            let before_rev = field.revision();
                            field.insert("", MAX_SEARCH_BYTES);
                            if is_query && field.revision() != before_rev {
                                self.search.last_match = None;
                            }
                            self.invalidate_hit_targets();
                        }
                        self.status = StatusMessage::new(
                            StatusLevel::Success,
                            copy_success_status(cut, target),
                        );
                    }
                    Err(error) => {
                        self.status = StatusMessage::new(
                            StatusLevel::Error,
                            copy_failure_status(cut, &error),
                        );
                    }
                }
            }
            _ => {
                self.status = StatusMessage::new(StatusLevel::Info, no_selection_status(cut));
            }
        }
    }

    fn modal_paste(&mut self) {
        let to_replacement = match &self.modal {
            Some(Modal::Path { .. }) => None,
            Some(Modal::Search {
                replace_mode,
                replacement_focus,
            }) => Some(*replace_mode && *replacement_focus),
            _ => return,
        };
        let internal = self.clipboard.is_internal();
        let text = if internal {
            self.clipboard.take_internal()
        } else {
            match self.clipboard.read_external() {
                Ok(text) => text,
                Err(error) => {
                    self.status =
                        StatusMessage::new(StatusLevel::Error, format!("Paste failed: {error}"));
                    return;
                }
            }
        };
        if text.is_empty() {
            self.status = StatusMessage::new(StatusLevel::Info, self.clipboard.empty_note().into());
        } else if let Some(to_replacement) = to_replacement {
            self.search_input(&text, to_replacement);
        } else {
            self.path_input(&text);
        }
        if internal {
            self.clipboard.put_internal(text);
        }
    }

    fn open_search_modal(&mut self, replace_mode: bool) {
        let prefill = self.workspace.active_view().and_then(|view| {
            let selection = view.selection()?;
            let buf = self.workspace.document(view.document_id)?;
            let text = buf.document.text().slice(selection);
            (text.len_bytes() <= MAX_SEARCH_BYTES
                && !text.chars().any(|ch| matches!(ch, '\n' | '\r')))
            .then(|| text.to_string())
        });
        if let Some(text) = prefill {
            if text != self.search.query.as_str() {
                self.search.last_match = None;
            }
            self.search.query = Field::new(text);
            self.search
                .query
                .handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        }
        self.drag = None;
        self.workspace.explorer_focused = false;
        self.modal = Some(Modal::Search {
            replace_mode,
            replacement_focus: false,
        });
        self.invalidate_hit_targets();
    }

    fn refresh_search_cache(&mut self) -> bool {
        let Some(buf) = self.workspace.active_document() else {
            return false;
        };
        let valid = self.search.cache.as_ref().is_some_and(|cache| {
            cache.document_id == buf.id
                && cache.doc_revision == buf.document.revision()
                && cache.compiled.needle() == self.search.query.as_str()
        });
        if !valid {
            let Some(compiled) = Query::new(self.search.query.as_str()) else {
                self.search.cache = None;
                self.search.feedback.clear();
                return false;
            };
            let index = compiled.match_index(&buf.document, MAX_COUNTED_MATCHES);
            self.search.cache = Some(SearchCache {
                document_id: buf.id,
                doc_revision: buf.document.revision(),
                compiled,
                index,
            });
        }
        true
    }

    fn find_in_active_tab(&mut self, reverse: bool) {
        if self.search.query.is_empty() {
            self.search.last_match = None;
            self.status = StatusMessage::new(StatusLevel::Info, "Enter a search query".into());
            return;
        }
        if !self.refresh_search_cache() {
            return;
        }
        let area = self.active_text_area();
        let pane_id = self.workspace.active_pane_id();
        let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() else {
            return;
        };
        let view_id = tab.id;
        let doc_id = buf.id;
        let doc = &buf.document;
        let current_match = self.search.last_match.as_ref().filter(|m| {
            m.pane_id == pane_id
                && m.view_id == view_id
                && m.document_id == doc_id
                && m.doc_revision == doc.revision()
                && m.query == self.search.query.as_str()
                && tab.selection().as_ref() == Some(&m.range)
        });
        let (from, forced_wrap) = match (reverse, current_match) {
            (true, Some(m)) if m.range.start == 0 => (doc.len_chars(), true),
            (true, Some(m)) => (doc.previous(m.range.start), false),
            (false, Some(m)) => (doc.next(m.range.start), false),
            _ => (tab.cursor, false),
        };
        let compiled = &self
            .search
            .cache
            .as_ref()
            .expect("refreshed search cache")
            .compiled;
        let found = compiled.next(doc, from, reverse);
        if let Some(range) = found {
            let wrapped = forced_wrap
                || if reverse {
                    range.start > from
                } else {
                    range.start < from
                };
            let prev_top = tab.top_line;
            tab.anchor = Some(range.start);
            tab.cursor = range.end;
            tab.preferred_column = None;
            let scrolled = tab.ensure_visible(doc, area);
            let top_changed = tab.top_line != prev_top;
            let line = doc.line_of(range.start) + 1;
            let col = doc.column_of(range.start) + 1;
            self.search.update_feedback(Some(&range), wrapped);
            self.search.last_match = Some(SearchMatch {
                pane_id,
                view_id,
                document_id: doc_id,
                doc_revision: doc.revision(),
                query: self.search.query.as_str().to_string(),
                range,
            });
            self.status = StatusMessage::new(
                StatusLevel::Info,
                format!("{} at Ln {line}, Col {col}", self.search.feedback),
            );
            if scrolled {
                self.invalidate_hit_targets_keep_drag();
            }
            if top_changed {
                self.highlight_pending = true;
            }
        } else {
            self.search.last_match = None;
            self.search.update_feedback(None, false);
            self.status = StatusMessage::new(
                StatusLevel::Info,
                format!("No match for '{}'", self.search.query.as_str()),
            );
        }
    }

    fn replace_current_in_active_tab(&mut self) {
        if self.search.query.is_empty() {
            self.search.last_match = None;
            self.status = StatusMessage::new(StatusLevel::Info, "Enter a search query".into());
            return;
        }
        let pane_id = self.workspace.active_pane_id();
        let Some(tab) = self.workspace.active_view() else {
            return;
        };
        let view_id = tab.id;
        let doc_id = tab.document_id;
        let tab_sel = tab.selection();
        let Some(buf) = self.workspace.document(doc_id) else {
            return;
        };
        if let Some(reason) = &buf.read_only {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                format!("Read-only: {reason}; replace blocked"),
            );
            return;
        }
        let valid_match = self
            .search
            .last_match
            .as_ref()
            .filter(|m| {
                m.pane_id == pane_id
                    && m.view_id == view_id
                    && m.document_id == doc_id
                    && m.doc_revision == buf.document.revision()
                    && m.query == self.search.query.as_str()
                    && tab_sel.as_ref() == Some(&m.range)
            })
            .map(|m| m.range.clone());
        let Some(matched_range) = valid_match else {
            self.search.last_match = None;
            self.status = StatusMessage::new(
                StatusLevel::Info,
                "No active match at current revision; press Enter or F3 to find".into(),
            );
            return;
        };

        if self.search.replacement.as_str() == self.search.query.as_str() {
            self.find_in_active_tab(false);
            self.status
                .text
                .insert_str(0, "Replacement matches query; ");
            return;
        }

        let replacement = self.search.replacement.as_str();
        let Some((result, mutated)) =
            self.workspace
                .edit_transaction(doc_id, view_id, EditViewPolicy::FollowEdit, |doc| match doc
                    .replace(matched_range, replacement)
                {
                    Ok(new_cursor) => (Ok(new_cursor), Some(new_cursor)),
                    Err(err) => (Err(err), None),
                })
        else {
            return;
        };

        match result {
            Ok(_) => {
                self.search.last_match = None;
                if mutated {
                    self.highlight_pending = true;
                    self.invalidate_hit_targets();
                }
                self.find_in_active_tab(false);
                self.status.level = StatusLevel::Success;
                self.status.text.insert_str(0, "Replaced; ");
            }
            Err(error) => {
                self.search.last_match = None;
                self.status = StatusMessage::new(
                    StatusLevel::Error,
                    format!("Error: replace failed: {error}"),
                );
            }
        }
    }

    fn replace_all_in_active_tab(&mut self) {
        if self.search.query.is_empty() {
            self.search.last_match = None;
            self.status = StatusMessage::new(StatusLevel::Info, "Enter a search query".into());
            return;
        }
        let Some(compiled) = Query::new(self.search.query.as_str()) else {
            self.search.last_match = None;
            self.status = StatusMessage::new(StatusLevel::Info, "Enter a search query".into());
            return;
        };
        let area = self.active_text_area();
        let Some(view) = self.workspace.active_view() else {
            return;
        };
        let view_id = view.id;
        let doc_id = view.document_id;
        let Some(buf) = self.workspace.document(doc_id) else {
            return;
        };
        if let Some(reason) = &buf.read_only {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                format!("Read-only: {reason}; replace blocked"),
            );
            return;
        }
        let replacement = self.search.replacement.as_str();
        let Some((result, mutated)) =
            self.workspace
                .edit_transaction(doc_id, view_id, EditViewPolicy::MapCursor, |doc| {
                    (search::replace_all(doc, &compiled, replacement), None)
                })
        else {
            return;
        };
        let succeeded = result.is_ok();
        match result {
            Ok(0) => {
                self.search.last_match = None;
                if self.search.query.as_str() == self.search.replacement.as_str() {
                    self.status = StatusMessage::new(
                        StatusLevel::Info,
                        "Replacement matches query; 0 changes".into(),
                    );
                } else {
                    self.status = StatusMessage::new(StatusLevel::Info, "No matches found".into());
                }
            }
            Ok(count) => {
                self.search.last_match = None;
                if mutated {
                    self.highlight_pending = true;
                    self.invalidate_hit_targets();
                }
                if let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() {
                    tab.ensure_visible(&buf.document, area);
                }
                self.status = StatusMessage::new(
                    StatusLevel::Success,
                    format!("Replaced {count} occurrence(s)"),
                );
            }
            Err(error) => {
                self.search.last_match = None;
                self.status = StatusMessage::new(
                    StatusLevel::Error,
                    format!("Error: replace failed: {error}"),
                );
            }
        }
        if succeeded && self.refresh_search_cache() {
            self.search.update_feedback(None, false);
        }
    }

    fn help_modal_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc || key.code == KeyCode::F(1) {
            self.close_help();
            return;
        }
        if self.viewport.0 < ui::MIN_DIALOG_SIZE.0 || self.viewport.1 < ui::MIN_DIALOG_SIZE.1 {
            return;
        }
        let Some(Modal::Help { scroll }) = &mut self.modal else {
            return;
        };
        let limit = crate::help::scroll_limit(self.viewport.0, self.viewport.1);
        *scroll = (*scroll).min(limit);
        let page_step = crate::help::page_size(self.viewport.0, self.viewport.1);
        match key.code {
            KeyCode::Up => {
                *scroll = scroll.saturating_sub(1);
                self.invalidate_hit_targets();
            }
            KeyCode::Down => {
                *scroll = (*scroll + 1).min(limit);
                self.invalidate_hit_targets();
            }
            KeyCode::PageUp => {
                *scroll = scroll.saturating_sub(page_step);
                self.invalidate_hit_targets();
            }
            KeyCode::PageDown => {
                *scroll = (*scroll + page_step).min(limit);
                self.invalidate_hit_targets();
            }
            KeyCode::Home => {
                *scroll = 0;
                self.invalidate_hit_targets();
            }
            KeyCode::End => {
                *scroll = limit;
                self.invalidate_hit_targets();
            }
            _ => {}
        }
    }

    fn modal_key(&mut self, mut key: KeyEvent) {
        if matches!(self.modal, Some(Modal::Help { .. })) {
            self.help_modal_key(key);
            return;
        }
        if key.code == KeyCode::F(1)
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            self.open_help();
            return;
        }
        if key.code == KeyCode::Esc {
            self.cancel_modal();
            return;
        }
        if matches!(self.modal, Some(Modal::Search { .. })) {
            self.search_modal_key(key);
            return;
        }
        if self.viewport.0 < ui::MIN_DIALOG_SIZE.0 || self.viewport.1 < ui::MIN_DIALOG_SIZE.1 {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Resize to at least 20x6 before confirming; Esc cancels".into(),
            );
            return;
        }
        let choice = self.modal.as_ref().and_then(Modal::choice);
        if let Some((kind, _)) = choice
            && !ui::choices_fit(
                Rect::new(0, 0, self.viewport.0, self.viewport.1),
                kind,
                kind.message_lines(),
                self.modal
                    .as_ref()
                    .and_then(|modal| modal.target(&self.workspace)),
            )
        {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Resize to read the full confirmation; Esc cancels".into(),
            );
            return;
        }
        if matches!(self.modal, Some(Modal::Path { .. })) {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
            {
                match key.code {
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        self.modal_copy(false);
                        return;
                    }
                    KeyCode::Char('x') | KeyCode::Char('X') => {
                        self.modal_copy(true);
                        return;
                    }
                    KeyCode::Char('v') | KeyCode::Char('V') => {
                        self.modal_paste();
                        return;
                    }
                    KeyCode::Char('z')
                    | KeyCode::Char('Z')
                    | KeyCode::Char('y')
                    | KeyCode::Char('Y') => {
                        return;
                    }
                    _ => {}
                }
            }
            match key.code {
                KeyCode::Enter => {
                    let Some(Modal::Path { action, value }) = self.modal.take() else {
                        return;
                    };
                    if value.as_str().is_empty() {
                        self.status =
                            StatusMessage::new(StatusLevel::Warning, "Enter a file path".into());
                        self.modal = Some(Modal::Path { action, value });
                        return;
                    }
                    let path = PathBuf::from(value.as_str());
                    self.path_retry = Some((action, value));
                    match action {
                        PathAction::Open => match self.open_tab_from_path(path) {
                            Ok(()) => self.path_retry = None,
                            Err(error) => {
                                self.status = StatusMessage::new(
                                    StatusLevel::Error,
                                    format!("Open failed: {error}; previous documents retained"),
                                );
                                self.restore_failed_path();
                            }
                        },
                        PathAction::Save { doc_id } => {
                            self.save_document_to(doc_id, path, false, true);
                        }
                    }
                    return;
                }
                _ => {
                    if let Some(Modal::Path { value, .. }) = &mut self.modal {
                        if value.handle_key(key) {
                            self.invalidate_hit_targets();
                            return;
                        }
                    }
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    {
                        if let KeyCode::Char(ch) = key.code {
                            self.path_input(ch.encode_utf8(&mut [0; 4]));
                            return;
                        }
                    }
                }
            }
            return;
        }
        if let Some((kind, focused)) = choice
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            let backward = match key.code {
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up => Some(true),
                KeyCode::Tab => Some(key.modifiers.contains(KeyModifiers::SHIFT)),
                KeyCode::Right | KeyCode::Down => Some(false),
                _ => None,
            };
            if let Some(backward) = backward {
                let len = kind.actions().len();
                let next = if backward {
                    (focused + len - 1) % len
                } else {
                    (focused + 1) % len
                };
                if let Some(
                    Modal::Unsaved { focused: value, .. }
                    | Modal::Overwrite { focused: value, .. }
                    | Modal::Conflict { focused: value, .. },
                ) = &mut self.modal
                {
                    *value = next;
                }
                self.invalidate_hit_targets();
                return;
            }
            if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                let Some(&(_, code)) = kind.actions().get(focused) else {
                    return;
                };
                if code == KeyCode::F(1) {
                    self.open_help();
                    return;
                }
                key = KeyEvent::new(code, KeyModifiers::NONE);
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    self.status = StatusMessage::new(
                        StatusLevel::Info,
                        "No selection; Ctrl+C does not exit".into(),
                    );
                    return;
                }
                KeyCode::Char('z')
                | KeyCode::Char('Z')
                | KeyCode::Char('y')
                | KeyCode::Char('Y')
                | KeyCode::Char('x')
                | KeyCode::Char('X')
                | KeyCode::Char('v')
                | KeyCode::Char('V') => {
                    return;
                }
                _ => {}
            }
        }
        let KeyCode::Char(ch) = key.code else { return };
        let ch = ch.to_ascii_lowercase();
        let decision = match &self.modal {
            Some(Modal::Unsaved { .. }) => matches!(ch, 's' | 'd' | 'c'),
            Some(Modal::Overwrite { .. }) => matches!(ch, 'y' | 'n' | 'c'),
            Some(Modal::Conflict { .. }) => matches!(ch, 'o' | 'a' | 'r' | 'c'),
            _ => false,
        };
        if !decision {
            return;
        }
        match self.modal.take() {
            Some(Modal::Unsaved { doc_id, after, .. }) => match ch {
                's' => {
                    self.after_save = Some(after);
                    self.save_document(doc_id);
                }
                'd' => {
                    self.after_save = None;
                    match after {
                        AfterSave::CloseView { view_id } => {
                            self.close_view_unconditional(view_id);
                            self.status = StatusMessage::new(
                                StatusLevel::Info,
                                "Discarded unsaved changes".into(),
                            );
                        }
                        AfterSave::ClosePane { pane_id } => {
                            if let Some(removed_doc) = self
                                .workspace
                                .discard_tab_in_pane_for_close_pane(pane_id, doc_id)
                            {
                                self.highlighter.forget(removed_doc);
                            }
                            self.search.last_match = None;
                            self.invalidate_hit_targets();
                            self.continue_close_pane(pane_id);
                        }
                        AfterSave::Quit => {
                            if self.workspace.remove_document_for_quit(doc_id) {
                                self.highlighter.forget(doc_id);
                            }
                            self.search.last_match = None;
                            self.invalidate_hit_targets();
                            self.continue_quit();
                        }
                    }
                }
                _ => self.cancel_modal(),
            },
            Some(Modal::Overwrite { doc_id, path, .. }) if ch == 'y' => {
                self.save_document_to(doc_id, path, true, true);
            }
            Some(Modal::Conflict { doc_id, path, .. }) => match ch {
                'o' => self.save_document_to(doc_id, path, true, false),
                'a' => self.save_document_as(doc_id),
                'r' => {
                    self.after_save = None;
                    self.reload_document(doc_id, path);
                }
                _ => self.cancel_modal(),
            },
            _ => self.cancel_modal(),
        }
    }

    fn search_modal_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::F(1)
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            self.open_help();
            return;
        }
        if self.viewport.0 < ui::MIN_SEARCH_SIZE.0 || self.viewport.1 < ui::MIN_SEARCH_SIZE.1 {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Resize to at least 40x10 before using search; Esc cancels".into(),
            );
            return;
        }
        let Some(Modal::Search {
            replace_mode,
            replacement_focus,
        }) = self.modal
        else {
            return;
        };
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if control && !alt {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    self.modal_copy(false);
                    return;
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    self.modal_copy(true);
                    return;
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    self.modal_paste();
                    return;
                }
                KeyCode::Char('z')
                | KeyCode::Char('Z')
                | KeyCode::Char('y')
                | KeyCode::Char('Y') => {
                    return;
                }
                KeyCode::Char('u') | KeyCode::Char('U') => {
                    if replace_mode && replacement_focus {
                        self.search.replacement.clear();
                    } else {
                        self.search.query.clear();
                        self.search.last_match = None;
                    }
                    self.invalidate_hit_targets();
                    return;
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    self.modal = Some(Modal::Search {
                        replace_mode: false,
                        replacement_focus: false,
                    });
                    self.invalidate_hit_targets();
                    return;
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    self.modal = Some(Modal::Search {
                        replace_mode: true,
                        replacement_focus: false,
                    });
                    self.invalidate_hit_targets();
                    return;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab if replace_mode => {
                self.modal = Some(Modal::Search {
                    replace_mode,
                    replacement_focus: !replacement_focus,
                });
                self.invalidate_hit_targets();
            }
            KeyCode::Enter => self.find_in_active_tab(false),
            KeyCode::F(3) => self.find_in_active_tab(shift),
            KeyCode::F(6) => {
                if replace_mode {
                    self.replace_current_in_active_tab();
                } else {
                    self.status = StatusMessage::new(
                        StatusLevel::Info,
                        "Open Replace (Ctrl+R) to replace".into(),
                    );
                }
            }
            KeyCode::F(7) => {
                if replace_mode {
                    self.replace_all_in_active_tab();
                } else {
                    self.status = StatusMessage::new(
                        StatusLevel::Info,
                        "Open Replace (Ctrl+R) to replace all".into(),
                    );
                }
            }
            _ => {
                let to_replacement = replace_mode && replacement_focus;
                let (field, is_query) = if to_replacement {
                    (&mut self.search.replacement, false)
                } else {
                    (&mut self.search.query, true)
                };
                let before_rev = field.revision();
                if field.handle_key(key) {
                    if is_query && field.revision() != before_rev {
                        self.search.last_match = None;
                    }
                    self.invalidate_hit_targets();
                    return;
                }
                if !control && !alt {
                    if let KeyCode::Char(ch) = key.code {
                        self.search_input(
                            ch.encode_utf8(&mut [0; 4]),
                            replace_mode && replacement_focus,
                        );
                    }
                }
            }
        }
    }

    fn copy(&mut self, cut: bool) {
        let Some(view) = self.workspace.active_view() else {
            return;
        };
        let Some(buf) = self.workspace.document(view.document_id) else {
            return;
        };
        let Some(range) = view.selection() else {
            self.status = StatusMessage::new(StatusLevel::Info, no_selection_status(cut));
            return;
        };
        if cut && let Some(reason) = &buf.read_only {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                format!("Read-only: {reason}; cut blocked"),
            );
            return;
        }
        let text = buf.document.text().slice(range).to_string();
        match self.clipboard.copy(text) {
            Ok(target) => {
                if cut {
                    self.status =
                        StatusMessage::new(StatusLevel::Success, copy_success_status(true, target));
                    self.replace_in_active_tab("");
                } else {
                    self.status = StatusMessage::new(
                        StatusLevel::Success,
                        copy_success_status(false, target),
                    );
                }
            }
            Err(error) => {
                self.status =
                    StatusMessage::new(StatusLevel::Error, copy_failure_status(cut, &error));
            }
        }
    }

    fn horizontal(&mut self, right: bool, word: bool, extend: bool) {
        let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() else {
            return;
        };
        let doc = &buf.document;
        let position = if !extend && let Some(range) = tab.selection() {
            if right { range.end } else { range.start }
        } else if word {
            if right {
                doc.word_right(tab.cursor)
            } else {
                doc.word_left(tab.cursor)
            }
        } else if right {
            doc.next(tab.cursor)
        } else {
            doc.previous(tab.cursor)
        };
        tab.select_cursor(doc, position, extend);
        tab.preferred_column = None;
    }

    fn vertical(&mut self, delta: isize, extend: bool) {
        let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() else {
            return;
        };
        let doc = &buf.document;
        let column = *tab
            .preferred_column
            .get_or_insert_with(|| doc.column_of(tab.cursor));
        let line = doc
            .line_of(tab.cursor)
            .saturating_add_signed(delta)
            .min(doc.line_count().saturating_sub(1));
        let pos = doc.position_at(line, column);
        tab.select_cursor(doc, pos, extend);
    }

    fn explorer_fits_viewport(&mut self) -> bool {
        let (w, h) = self.viewport;
        let (tree_min_w, tree_min_h) = self.workspace.tree_minimum();
        let body = chrome::regions(Rect::new(0, 0, w, h), tree_min_h).body;
        let active_min_w = self
            .workspace
            .active_document()
            .map_or(chrome::MIN_EDITOR_WIDTH, |b| {
                chrome::pane_min_width(b.document.line_count())
            });
        chrome::sidebar(
            body,
            self.workspace.sidebar_width,
            active_min_w,
            (tree_min_w, tree_min_h),
            true,
        )
        .panel
        .is_some()
    }

    fn toggle_explorer(&mut self) {
        self.workspace.explorer_visible = !self.workspace.explorer_visible;
        if !self.workspace.explorer_visible {
            self.workspace.explorer_focused = false;
        } else if self.workspace.explorer.is_none() {
            if let Err(err) = self.workspace.retry_explorer() {
                self.status =
                    StatusMessage::new(StatusLevel::Warning, format!("Warn: explorer: {err}"));
            } else {
                self.status = StatusMessage::new(StatusLevel::Info, "Explorer shown".into());
            }
        }
        self.highlight_pending = true;
        self.invalidate_hit_targets();
    }

    fn toggle_focus(&mut self) {
        if self.workspace.explorer_focused {
            self.workspace.explorer_focused = false;
            self.status = StatusMessage::new(StatusLevel::Info, "Editor focused".into());
            return;
        }
        if !self.workspace.explorer_visible {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Explorer hidden; press Ctrl+B to show".into(),
            );
            return;
        }
        if self.workspace.explorer.is_none() {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Explorer unavailable; press F5 to retry".into(),
            );
            return;
        }
        if !self.explorer_fits_viewport() {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Terminal too narrow for explorer".into(),
            );
            return;
        }
        self.workspace.explorer_focused = true;
        self.status = StatusMessage::new(StatusLevel::Info, "Explorer focused".into());
    }

    fn refresh_explorer(&mut self) {
        if let Some(exp) = self.workspace.explorer.as_mut() {
            match exp.refresh() {
                Ok(()) => {
                    self.ensure_explorer_visible();
                    self.invalidate_hit_targets();
                    self.status =
                        StatusMessage::new(StatusLevel::Success, "Explorer refreshed".into());
                }
                Err(err) => {
                    self.status = StatusMessage::new(
                        StatusLevel::Warning,
                        format!("Warn: explorer refresh: {err}"),
                    );
                }
            }
        } else {
            match self.workspace.retry_explorer() {
                Ok(()) => {
                    self.ensure_explorer_visible();
                    self.invalidate_hit_targets();
                    self.status =
                        StatusMessage::new(StatusLevel::Success, "Explorer loaded".into());
                }
                Err(err) => {
                    self.status =
                        StatusMessage::new(StatusLevel::Warning, format!("Warn: explorer: {err}"));
                }
            }
        }
    }

    fn explorer_view_height(&self) -> usize {
        if let Some(sidebar) = self.geometry.sidebar {
            usize::from(sidebar.height.max(1))
        } else {
            // No frame yet: estimate the sidebar rows as the panel body minus
            // its border and the reserved parent navigation row.
            let (w, h) = self.viewport;
            let body = chrome::max_editor_area(Rect::new(0, 0, w, h));
            usize::from(body.height.saturating_sub(3)).max(1)
        }
    }

    fn ensure_explorer_visible(&mut self) -> bool {
        let height = self.explorer_view_height();
        let Some(exp) = self.workspace.explorer.as_mut() else {
            return false;
        };
        let before = exp.top();
        exp.ensure_visible(height);
        let changed = exp.top() != before;
        if changed {
            self.invalidate_hit_targets();
        }
        changed
    }

    fn activate_selected_explorer_entry(&mut self) {
        let Some(entry) = self
            .workspace
            .explorer
            .as_ref()
            .and_then(|e| e.selected_entry())
        else {
            return;
        };
        match entry.kind {
            // Mouse activation keeps tree toggle semantics; re-rooting into a
            // folder stays keyboard-only so a single click never changes the
            // browsing root.
            EntryKind::Directory => {
                if let Some(exp) = self.workspace.explorer.as_mut()
                    && let Err(err) = exp.toggle_selected()
                {
                    self.status =
                        StatusMessage::new(StatusLevel::Warning, format!("Warn: explorer: {err}"));
                }
                self.ensure_explorer_visible();
                self.invalidate_hit_targets();
            }
            EntryKind::File | EntryKind::Symlink => {
                if let Err(err) = self.open_tab_from_path(entry.path.clone()) {
                    self.status =
                        StatusMessage::new(StatusLevel::Error, format!("Open failed: {err}"));
                }
            }
            EntryKind::Other => {
                self.status = StatusMessage::new(
                    StatusLevel::Warning,
                    entry.issue.clone().unwrap_or_else(|| {
                        "Unsupported entry type (not a regular file or directory)".into()
                    }),
                );
            }
        }
    }

    fn enter_selected_explorer_entry(&mut self) {
        let Some(entry) = self
            .workspace
            .explorer
            .as_ref()
            .and_then(|e| e.selected_entry())
        else {
            return;
        };
        match entry.kind {
            // Enter on a folder browses into it as the new root; the tree and
            // header stay usable when the atomic navigation fails.
            EntryKind::Directory => match self.workspace.explorer_enter_selected() {
                Ok(true) => {
                    self.ensure_explorer_visible();
                    self.invalidate_hit_targets();
                    self.status = StatusMessage::new(
                        StatusLevel::Info,
                        format!("Explorer: {}", self.workspace.root_label),
                    );
                }
                Ok(false) => {}
                Err(err) => {
                    self.status =
                        StatusMessage::new(StatusLevel::Warning, format!("Warn: explorer: {err}"))
                }
            },
            EntryKind::File | EntryKind::Symlink => {
                if let Err(err) = self.open_tab_from_path(entry.path.clone()) {
                    self.status =
                        StatusMessage::new(StatusLevel::Error, format!("Open failed: {err}"));
                }
            }
            EntryKind::Other => {
                self.status = StatusMessage::new(
                    StatusLevel::Warning,
                    entry.issue.clone().unwrap_or_else(|| {
                        "Unsupported entry type (not a regular file or directory)".into()
                    }),
                );
            }
        }
    }

    /// Shared parent navigation for Backspace and the sidebar parent row; it
    /// never touches the active document.
    fn explorer_go_parent(&mut self) {
        match self.workspace.explorer_go_parent() {
            Ok(true) => {
                self.ensure_explorer_visible();
                self.invalidate_hit_targets();
                self.status = StatusMessage::new(
                    StatusLevel::Info,
                    format!("Explorer: {}", self.workspace.root_label),
                );
            }
            Ok(false) => {
                self.status =
                    StatusMessage::new(StatusLevel::Info, "Already at filesystem root".into())
            }
            Err(err) => {
                self.status =
                    StatusMessage::new(StatusLevel::Warning, format!("Warn: explorer: {err}"))
            }
        }
    }

    fn explorer_key(&mut self, key: KeyEvent) {
        if !self.workspace.explorer_visible
            || self.workspace.explorer.is_none()
            || !self.explorer_fits_viewport()
        {
            self.status = StatusMessage::new(
                StatusLevel::Warning,
                "Explorer hidden on narrow screen; press Esc or F2 for editor".into(),
            );
            return;
        }
        let page_step = isize::try_from(self.explorer_view_height().max(1)).unwrap_or(1);
        match key.code {
            KeyCode::Up => {
                if let Some(exp) = self.workspace.explorer.as_mut() {
                    exp.move_by(-1);
                }
                self.ensure_explorer_visible();
            }
            KeyCode::Down => {
                if let Some(exp) = self.workspace.explorer.as_mut() {
                    exp.move_by(1);
                }
                self.ensure_explorer_visible();
            }
            KeyCode::PageUp => {
                if let Some(exp) = self.workspace.explorer.as_mut() {
                    exp.move_by(-page_step);
                }
                self.ensure_explorer_visible();
            }
            KeyCode::PageDown => {
                if let Some(exp) = self.workspace.explorer.as_mut() {
                    exp.move_by(page_step);
                }
                self.ensure_explorer_visible();
            }
            KeyCode::Home => {
                if let Some(exp) = self.workspace.explorer.as_mut() {
                    exp.select(0);
                }
                self.ensure_explorer_visible();
            }
            KeyCode::End => {
                if let Some(exp) = self.workspace.explorer.as_mut() {
                    let last = exp.rows().len().saturating_sub(1);
                    exp.select(last);
                }
                self.ensure_explorer_visible();
            }
            KeyCode::Right => {
                if let Some(exp) = self.workspace.explorer.as_mut()
                    && let Err(err) = exp.expand_selected()
                {
                    self.status =
                        StatusMessage::new(StatusLevel::Warning, format!("Warn: explorer: {err}"));
                }
                self.ensure_explorer_visible();
                self.invalidate_hit_targets();
            }
            KeyCode::Left => {
                if let Some(exp) = self.workspace.explorer.as_mut() {
                    exp.collapse_selected();
                }
                self.ensure_explorer_visible();
                self.invalidate_hit_targets();
            }
            KeyCode::Enter => self.enter_selected_explorer_entry(),
            // Backspace browses the filesystem parent; it never reaches the
            // document because explorer focus consumes the key here.
            KeyCode::Backspace => self.explorer_go_parent(),
            _ => {}
        }
    }

    fn key(&mut self, key: KeyEvent) {
        if matches!(self.drag, Some(DragState::Scrollbar { .. })) {
            self.drag = None;
            self.dragged = false;
        }
        if self.modal.is_some() {
            self.modal_key(key);
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if alt && !control && shift {
            match key.code {
                KeyCode::Left => {
                    self.resize_nearest_divider(Axis::Columns, -2);
                    return;
                }
                KeyCode::Right => {
                    self.resize_nearest_divider(Axis::Columns, 2);
                    return;
                }
                KeyCode::Up => {
                    self.resize_nearest_divider(Axis::Rows, -1);
                    return;
                }
                KeyCode::Down => {
                    self.resize_nearest_divider(Axis::Rows, 1);
                    return;
                }
                _ => {}
            }
        }
        if alt && !control && !shift {
            match key.code {
                KeyCode::Left => {
                    self.cycle_tab(-1);
                    return;
                }
                KeyCode::Right => {
                    self.cycle_tab(1);
                    return;
                }
                _ => {}
            }
        }
        if control && !alt {
            match key.code {
                KeyCode::PageUp => {
                    self.cycle_tab(-1);
                    return;
                }
                KeyCode::PageDown => {
                    self.cycle_tab(1);
                    return;
                }
                _ => {}
            }
        }
        if !control && !alt {
            match key.code {
                KeyCode::F(1) => {
                    self.open_help();
                    return;
                }
                KeyCode::F(2) => {
                    self.toggle_focus();
                    return;
                }
                KeyCode::F(3) => {
                    self.find_in_active_tab(shift);
                    return;
                }
                KeyCode::F(4) => {
                    self.save_active_document_as();
                    return;
                }
                KeyCode::F(5) => {
                    self.refresh_explorer();
                    return;
                }
                KeyCode::F(8) => {
                    self.split_active_pane(Axis::Columns);
                    return;
                }
                KeyCode::F(9) => {
                    self.split_active_pane(Axis::Rows);
                    return;
                }
                KeyCode::F(10) => {
                    self.cycle_pane(if shift { -1 } else { 1 });
                    return;
                }
                KeyCode::F(12) => {
                    self.request_close_active_pane();
                    return;
                }
                _ => {}
            }
        }
        if control
            && !alt
            && let KeyCode::Char(ch) = key.code
        {
            match ch.to_ascii_lowercase() {
                'q' => {
                    self.request_quit();
                    return;
                }
                'w' => {
                    self.request_close_active_view();
                    return;
                }
                'n' => {
                    self.new_tab();
                    return;
                }
                'o' => {
                    self.open_path_dialog();
                    return;
                }
                's' => {
                    self.save_active_document();
                    return;
                }
                'b' => {
                    self.toggle_explorer();
                    return;
                }
                'f' => {
                    self.open_search_modal(false);
                    return;
                }
                'r' => {
                    self.open_search_modal(true);
                    return;
                }
                _ => {}
            }
        }
        if key.code == KeyCode::Esc && !control && !alt {
            if self.workspace.explorer_focused {
                self.workspace.explorer_focused = false;
                self.status = StatusMessage::new(StatusLevel::Info, "Editor focused".into());
            } else {
                if let Some(view) = self.workspace.active_view_mut() {
                    view.anchor = None;
                }
                self.drag = None;
            }
            return;
        }

        if self.workspace.explorer_focused {
            if !control && !alt {
                self.explorer_key(key);
            }
            return;
        }

        if control
            && !alt
            && let KeyCode::Char(ch) = key.code
        {
            match ch.to_ascii_lowercase() {
                'a' => {
                    if let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() {
                        tab.anchor = Some(0);
                        tab.cursor = buf.document.len_chars();
                    }
                }
                'c' => self.copy(false),
                'x' => self.copy(true),
                'v' => {
                    let internal = self.clipboard.is_internal();
                    let text = if internal {
                        self.clipboard.take_internal()
                    } else {
                        match self.clipboard.read_external() {
                            Ok(text) => text,
                            Err(error) => {
                                self.status = StatusMessage::new(
                                    StatusLevel::Error,
                                    format!("Paste failed: {error}"),
                                );
                                return;
                            }
                        }
                    };
                    if text.is_empty() {
                        self.status = StatusMessage::new(
                            StatusLevel::Info,
                            self.clipboard.empty_note().into(),
                        );
                    } else {
                        self.paste_in_active_tab(&text);
                    }
                    if internal {
                        self.clipboard.put_internal(text);
                    }
                }
                'z' | 'y' => {
                    let is_undo = ch.eq_ignore_ascii_case(&'z');
                    let Some(view) = self.workspace.active_view() else {
                        return;
                    };
                    let view_id = view.id;
                    let doc_id = view.document_id;
                    let Some(buf) = self.workspace.document(doc_id) else {
                        return;
                    };
                    if let Some(reason) = &buf.read_only {
                        self.status = StatusMessage::new(
                            StatusLevel::Warning,
                            format!("Read-only: {reason}; undo/redo blocked"),
                        );
                        return;
                    }
                    if let Some((_, true)) = self.workspace.edit_transaction(
                        doc_id,
                        view_id,
                        EditViewPolicy::FollowEdit,
                        |doc| {
                            let cursor = if is_undo { doc.undo() } else { doc.redo() };
                            (cursor.is_some(), cursor)
                        },
                    ) {
                        self.search.last_match = None;
                        self.highlight_pending = true;
                        self.invalidate_hit_targets();
                    }
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Left if !alt => self.horizontal(false, control, shift),
            KeyCode::Right if !alt => self.horizontal(true, control, shift),
            KeyCode::Up if !control && !alt => self.vertical(-1, shift),
            KeyCode::Down if !control && !alt => self.vertical(1, shift),
            KeyCode::PageUp if !control && !alt => {
                let step = isize::try_from(self.active_text_area().height.max(1)).unwrap_or(1);
                self.vertical(-step, shift);
            }
            KeyCode::PageDown if !control && !alt => {
                let step = isize::try_from(self.active_text_area().height.max(1)).unwrap_or(1);
                self.vertical(step, shift);
            }
            KeyCode::Home | KeyCode::End if !alt => {
                if let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() {
                    let doc = &buf.document;
                    let line = doc.line_of(tab.cursor);
                    let position = match (key.code, control) {
                        (KeyCode::Home, true) => 0,
                        (KeyCode::End, true) => doc.len_chars(),
                        (KeyCode::Home, false) => doc.line_start(line),
                        _ => doc.line_end(line),
                    };
                    tab.select_cursor(doc, position, shift);
                    tab.preferred_column = None;
                }
            }
            KeyCode::Backspace if !control && !alt => {
                if let Some((tab, buf)) = self.workspace.active_view_and_doc_mut()
                    && buf.read_only.is_none()
                    && tab.selection().is_none()
                {
                    tab.anchor = Some(buf.document.previous(tab.cursor));
                }
                self.replace_in_active_tab("");
            }
            KeyCode::Delete if !control && !alt => {
                if let Some((tab, buf)) = self.workspace.active_view_and_doc_mut()
                    && buf.read_only.is_none()
                    && tab.selection().is_none()
                {
                    tab.anchor = Some(buf.document.next(tab.cursor));
                }
                self.replace_in_active_tab("");
            }
            KeyCode::Enter if !control && !alt => self.replace_in_active_tab("\n"),
            KeyCode::Tab if !control && !alt => self.indent_in_active_tab(shift),
            KeyCode::BackTab if !control && !alt => self.indent_in_active_tab(true),
            KeyCode::Char(ch) if !control && !alt => {
                self.replace_in_active_tab(ch.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
    }

    fn modal_mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Up(_)) {
            self.drag = None;
            self.dragged = false;
            return;
        }
        let continues_help_drag = matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
            && matches!(
                self.drag,
                Some(DragState::Scrollbar {
                    target: ScrollbarTarget::Help { .. },
                    ..
                })
            );
        if !self.hit_targets_valid && !continues_help_drag {
            return;
        }
        let pos = (mouse.column, mouse.row).into();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(cmd) = self
                    .geometry
                    .commands
                    .iter()
                    .find(|c| c.area.contains(pos))
                    .copied()
                {
                    self.modal_key(cmd.key);
                    return;
                }
                if let Some(bar) = self
                    .geometry
                    .help_scrollbar
                    .filter(|bar| bar.track.contains(pos))
                {
                    self.start_scrollbar(
                        ScrollbarTarget::Help {
                            viewport: self.viewport,
                        },
                        Some(bar),
                        mouse,
                    );
                    return;
                }
                if let Some(hit) = self.geometry.fields.iter().find(|f| f.area.contains(pos)) {
                    let kind = hit.kind;
                    if let Some(Modal::Search {
                        replace_mode,
                        replacement_focus,
                    }) = &mut self.modal
                    {
                        match kind {
                            FieldKind::Query => *replacement_focus = false,
                            FieldKind::Replacement if *replace_mode => *replacement_focus = true,
                            _ => {}
                        }
                    }
                    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
                    let byte = byte_for_column(&hit.stops, mouse.column);
                    match kind {
                        FieldKind::Path => {
                            if let Some(Modal::Path { value, .. }) = &mut self.modal {
                                value.move_to(byte, shift);
                            }
                        }
                        FieldKind::Query => {
                            self.search.query.move_to(byte, shift);
                        }
                        FieldKind::Replacement => {
                            self.search.replacement.move_to(byte, shift);
                        }
                    }
                    self.drag = Some(DragState::Field { kind });
                    self.dragged = false;
                    self.invalidate_hit_targets_keep_drag();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if matches!(self.drag, Some(DragState::Scrollbar { .. })) {
                    self.drag_scrollbar(mouse);
                    return;
                }
                if let Some(DragState::Field { kind }) = self.drag {
                    if let Some(hit) = self.geometry.fields.iter().find(|f| f.kind == kind) {
                        let clamped_x = mouse
                            .column
                            .clamp(hit.area.left(), hit.area.right().saturating_sub(1));
                        let byte = byte_for_column(&hit.stops, clamped_x);
                        match kind {
                            FieldKind::Path => {
                                if let Some(Modal::Path { value, .. }) = &mut self.modal {
                                    value.move_to(byte, true);
                                }
                            }
                            FieldKind::Query => {
                                self.search.query.move_to(byte, true);
                            }
                            FieldKind::Replacement => {
                                self.search.replacement.move_to(byte, true);
                            }
                        }
                        self.dragged = true;
                        self.invalidate_hit_targets_keep_drag();
                    }
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if let Some(Modal::Help { scroll }) = &mut self.modal {
                    let limit = crate::help::scroll_limit(self.viewport.0, self.viewport.1);
                    *scroll = (*scroll).min(limit);
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            *scroll = scroll.saturating_sub(3);
                        }
                        MouseEventKind::ScrollDown => {
                            *scroll = (*scroll + 3).min(limit);
                        }
                        _ => {}
                    }
                    self.invalidate_hit_targets();
                }
            }
            _ => {}
        }
    }

    fn mouse(&mut self, mouse: MouseEvent, now: Instant) {
        if self.modal.is_some() {
            self.modal_mouse(mouse);
            return;
        }
        let completes_drag = matches!(mouse.kind, MouseEventKind::Up(_))
            || (matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
                && matches!(
                    self.drag,
                    Some(
                        DragState::ExplorerDivider { .. }
                            | DragState::SplitDivider { .. }
                            | DragState::Scrollbar { .. }
                    )
                ));
        if !self.hit_targets_valid && !completes_drag {
            return;
        }
        let pos = (mouse.column, mouse.row).into();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(cmd) = self
                    .geometry
                    .commands
                    .iter()
                    .find(|c| c.area.contains(pos))
                    .copied()
                {
                    self.key(cmd.key);
                    return;
                }
                // 1. Check pane tab-bar controls and tabs across all visible panes.
                let mut tab_action = None;
                for pane_geom in &self.geometry.panes {
                    if let Some(prev_rect) = pane_geom.previous_tab
                        && prev_rect.contains(pos)
                    {
                        tab_action = Some((pane_geom.id, None, -1isize));
                        break;
                    }
                    if let Some(next_rect) = pane_geom.next_tab
                        && next_rect.contains(pos)
                    {
                        tab_action = Some((pane_geom.id, None, 1isize));
                        break;
                    }
                    if let Some((view_id, is_close)) = pane_geom.tabs.iter().find_map(|hit| {
                        if hit.close.contains(pos) {
                            Some((hit.id, true))
                        } else if hit.body.contains(pos) {
                            Some((hit.id, false))
                        } else {
                            None
                        }
                    }) {
                        tab_action = Some((pane_geom.id, Some((view_id, is_close)), 0));
                        break;
                    }
                }
                if let Some((pane_id, maybe_tab, cycle_delta)) = tab_action {
                    match maybe_tab {
                        None => {
                            let switched_pane = self.workspace.activate_pane(pane_id);
                            self.workspace.explorer_focused = false;
                            let cycled = self.workspace.cycle_tab(cycle_delta);
                            if switched_pane || cycled {
                                self.search.last_match = None;
                                self.highlight_pending = true;
                            }
                            self.invalidate_hit_targets();
                        }
                        Some((view_id, true)) => {
                            self.request_close_view(pane_id, view_id);
                        }
                        Some((view_id, false)) => {
                            self.workspace.explorer_focused = false;
                            if self.workspace.activate_view(pane_id, view_id) {
                                self.search.last_match = None;
                                self.highlight_pending = true;
                            }
                            self.invalidate_hit_targets();
                        }
                    }
                    return;
                }
                if let Some(bar) = self
                    .geometry
                    .sidebar_scrollbar
                    .filter(|bar| bar.track.contains(pos))
                {
                    if let Some(explorer) = self.workspace.explorer.as_ref() {
                        let target = ScrollbarTarget::Explorer {
                            rows: explorer.rows().len(),
                        };
                        self.start_scrollbar(target, Some(bar), mouse);
                    }
                    return;
                }
                if let Some((pane_id, area, bar)) = self.geometry.panes.iter().find_map(|pane| {
                    if let Some(bar) = pane.vertical_scrollbar
                        && bar.track.contains(pos)
                    {
                        return Some((pane.id, pane.text_area, Some(bar)));
                    }
                    if pane
                        .horizontal_track
                        .is_some_and(|track| track.contains(pos))
                    {
                        return Some((pane.id, pane.text_area, pane.horizontal_scrollbar));
                    }
                    None
                }) {
                    let Some(view) = self
                        .workspace
                        .pane(pane_id)
                        .and_then(|pane| pane.active_tab())
                    else {
                        return;
                    };
                    let Some(buffer) = self.workspace.document(view.document_id) else {
                        return;
                    };
                    let target = ScrollbarTarget::Editor {
                        pane_id,
                        view_id: view.id,
                        revision: buffer.document.revision(),
                        text_area: area,
                    };
                    self.start_scrollbar(target, bar, mouse);
                    return;
                }

                // 2. Check split dividers.
                if let Some(divider) = self
                    .geometry
                    .splits
                    .iter()
                    .find(|d| d.area.contains(pos))
                    .copied()
                {
                    self.drag = Some(DragState::SplitDivider { divider });
                    self.mouse_down = None;
                    self.dragged = false;
                    return;
                }

                // 3. Check explorer divider.
                if let Some(divider) = self.geometry.explorer_divider
                    && divider.contains(pos)
                {
                    self.drag = Some(DragState::ExplorerDivider {
                        anchor_x: mouse.column,
                        start_width: self.workspace.sidebar_width,
                    });
                    self.mouse_down = None;
                    self.dragged = false;
                    return;
                }

                // 4. Check explorer sidebar frame (retains last active editor pane).
                //    Row clicks select entries; frame border clicks only focus the
                //    explorer and never change selection or open files.
                if let Some(panel) = self.geometry.sidebar_panel
                    && panel.contains(pos)
                {
                    self.workspace.explorer_focused = true;
                    // The parent navigation row precedes ordinary rows so its
                    // click never selects the tree row underneath, and it is
                    // handled here instead of forwarding Backspace to the
                    // active document.
                    if let Some(parent) = self.geometry.sidebar_parent
                        && parent.contains(pos)
                    {
                        self.explorer_go_parent();
                        self.invalidate_hit_targets();
                        return;
                    }
                    if let Some(rows) = self.geometry.sidebar
                        && rows.contains(pos)
                    {
                        let clicked_idx = self
                            .workspace
                            .explorer
                            .as_ref()
                            .map(|e| e.top() + usize::from(mouse.row - rows.y));
                        if let Some(idx) = clicked_idx
                            && let Some(exp) = self.workspace.explorer.as_mut()
                            && idx < exp.rows().len()
                        {
                            exp.select(idx);
                            self.activate_selected_explorer_entry();
                        } else {
                            self.invalidate_hit_targets();
                        }
                    }
                    return;
                }

                // The frame focuses its pane; body and gutter keep text navigation.
                // Border and tab-padding clicks never become document positions.
                let clicked_pane = self
                    .geometry
                    .panes
                    .iter()
                    .find(|p| p.area.contains(pos))
                    .map(|p| (p.id, p.text_area, p.area));
                let Some((pane_id, area, panel)) = clicked_pane else {
                    return;
                };
                let switched_pane = self.workspace.activate_pane(pane_id);
                self.workspace.explorer_focused = false;
                if switched_pane {
                    self.search.last_match = None;
                    self.highlight_pending = true;
                }

                let gutter_hit = mouse.column > panel.x
                    && mouse.column < area.x
                    && mouse.row >= area.y
                    && mouse.row < area.bottom();
                if area.is_empty() || !(area.contains(pos) || gutter_hit) {
                    return;
                }
                let extend = mouse.modifiers.contains(KeyModifiers::SHIFT);
                let double = self.last_click.is_some_and(|(p, x, y, at)| {
                    p == pane_id
                        && x == mouse.column
                        && y == mouse.row
                        && now.duration_since(at) <= DOUBLE_CLICK
                });
                let Some((tab, buf)) = self.workspace.pane_active_view_and_doc_mut(pane_id) else {
                    return;
                };
                let view_id = tab.id;
                let doc = &buf.document;
                self.drag = Some(DragState::Text {
                    pane_id,
                    view_id,
                    x: mouse.column,
                    y: mouse.row,
                });
                self.mouse_down = Some((mouse.column, mouse.row));
                self.dragged = false;
                let position = tab.hit(doc, area, mouse.column, mouse.row);
                if double {
                    let range = doc.word_range(position);
                    tab.anchor = Some(range.start);
                    tab.cursor = range.end;
                    self.last_click = None;
                } else {
                    tab.select_cursor(doc, position, extend);
                }
                tab.preferred_column = None;
            }
            MouseEventKind::Drag(MouseButton::Left) => match self.drag {
                Some(DragState::SplitDivider { divider }) => {
                    let coord = match divider.axis {
                        Axis::Columns => mouse.column,
                        Axis::Rows => mouse.row,
                    };
                    if self.workspace.tree.resize(&divider, coord) {
                        self.highlight_pending = true;
                        self.invalidate_hit_targets_keep_drag();
                    }
                }
                Some(DragState::ExplorerDivider {
                    anchor_x,
                    start_width,
                }) => {
                    let delta = i32::from(mouse.column) - i32::from(anchor_x);
                    let desired =
                        u16::try_from((i32::from(start_width) + delta).max(0)).unwrap_or(u16::MAX);
                    if self.workspace.set_sidebar_width(
                        desired,
                        self.viewport.0,
                        self.geometry.collapsed,
                    ) {
                        self.highlight_pending = true;
                        self.invalidate_hit_targets_keep_drag();
                    }
                }
                Some(DragState::Scrollbar { .. }) => self.drag_scrollbar(mouse),
                Some(DragState::Text {
                    pane_id, view_id, ..
                }) => {
                    let Some(pane) = self.workspace.pane_mut(pane_id) else {
                        self.drag = None;
                        return;
                    };
                    if pane.active_tab_id() != view_id {
                        self.drag = None;
                        return;
                    }
                    if let Some(tab) = pane.tab_mut(view_id) {
                        tab.anchor.get_or_insert(tab.cursor);
                    }
                    self.drag = Some(DragState::Text {
                        pane_id,
                        view_id,
                        x: mouse.column,
                        y: mouse.row,
                    });
                    self.dragged = true;
                    self.last_click = None;
                    self.drag_tick();
                }
                Some(DragState::Field { .. }) => {
                    self.drag = None;
                    self.dragged = false;
                }
                None => {}
            },
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(DragState::Text { pane_id, .. }) = self.drag
                    && let Some((x, y)) = self.mouse_down.take()
                    && !self.dragged
                    && (x, y) == (mouse.column, mouse.row)
                {
                    self.last_click = Some((pane_id, x, y, now));
                }
                self.mouse_down = None;
                self.dragged = false;
                self.drag = None;
            }
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                if matches!(self.drag, Some(DragState::Scrollbar { .. })) {
                    self.drag = None;
                }
                // Wheel targets the whole sidebar panel but scrolls by the real
                // inner rows height.
                if let Some(panel) = self.geometry.sidebar_panel
                    && panel.contains(pos)
                {
                    if let Some(exp) = self.workspace.explorer.as_mut() {
                        let rows_height = self
                            .geometry
                            .sidebar
                            .map_or(panel.height.saturating_sub(3), |rows| rows.height);
                        match mouse.kind {
                            MouseEventKind::ScrollUp => exp.set_top(exp.top().saturating_sub(3)),
                            MouseEventKind::ScrollDown => {
                                let max_top = exp
                                    .rows()
                                    .len()
                                    .saturating_sub(usize::from(rows_height.max(1)));
                                exp.set_top((exp.top() + 3).min(max_top));
                            }
                            _ => {}
                        }
                        self.hit_targets_valid = false;
                    }
                    return;
                }
                let Some((pane_id, text_area)) = self
                    .geometry
                    .panes
                    .iter()
                    .find(|p| !p.text_area.is_empty() && p.area.contains(pos))
                    .map(|p| (p.id, p.text_area))
                else {
                    return;
                };
                if let Some((tab, buf)) = self.workspace.pane_active_view_and_doc_mut(pane_id) {
                    let before = (tab.top_line, tab.left_cell);
                    match mouse.kind {
                        MouseEventKind::ScrollUp => tab.top_line = tab.top_line.saturating_sub(3),
                        MouseEventKind::ScrollDown => {
                            tab.top_line = (tab.top_line + 3).min(
                                buf.document
                                    .line_count()
                                    .saturating_sub(usize::from(text_area.height)),
                            );
                        }
                        MouseEventKind::ScrollLeft => {
                            tab.left_cell = tab.left_cell.saturating_sub(4);
                        }
                        MouseEventKind::ScrollRight => {
                            let content_width = tab
                                .horizontal_width
                                .measurement()
                                .and_then(|width| {
                                    width.for_view(
                                        buf.document.revision(),
                                        tab.top_line,
                                        text_area.height,
                                    )
                                })
                                .unwrap_or_else(|| {
                                    let end = tab
                                        .top_line
                                        .saturating_add(usize::from(text_area.height))
                                        .min(buf.document.line_count());
                                    (tab.top_line..end)
                                        .map(|line| {
                                            buf.document.column_of(buf.document.line_end(line))
                                        })
                                        .max()
                                        .unwrap_or(0)
                                        .saturating_add(1)
                                });
                            let max_left =
                                content_width.saturating_sub(usize::from(text_area.width));
                            tab.left_cell = tab.left_cell.saturating_add(4).min(max_left);
                        }
                        _ => {}
                    }
                    if before.0 != tab.top_line {
                        self.highlight_pending = true;
                    }
                    if before != (tab.top_line, tab.left_cell) {
                        self.invalidate_hit_targets_keep_drag();
                    }
                }
            }
            _ => {}
        }
    }

    fn start_scrollbar(
        &mut self,
        target: ScrollbarTarget,
        bar: Option<Scrollbar>,
        mouse: MouseEvent,
    ) {
        self.drag = None;
        self.mouse_down = None;
        self.dragged = false;
        self.last_click = None;
        let Some(bar) = bar.filter(|bar| bar.max_offset > 0) else {
            return;
        };
        let coordinate = match bar.axis {
            Axis::Columns => mouse.column,
            Axis::Rows => mouse.row,
        };
        if bar.thumb.contains((mouse.column, mouse.row).into()) {
            self.drag = Some(DragState::Scrollbar {
                target,
                bar,
                anchor: coordinate,
            });
        } else {
            self.scroll_to(target, bar.axis, bar.page_offset(coordinate));
        }
    }

    fn captured_scrollbar(&self, target: ScrollbarTarget, axis: Axis) -> Option<Scrollbar> {
        match target {
            ScrollbarTarget::Editor {
                pane_id,
                view_id,
                revision,
                text_area,
            } => {
                if self.modal.is_some() {
                    return None;
                }
                let view = self.workspace.pane(pane_id)?.active_tab()?;
                if view.id != view_id
                    || self
                        .workspace
                        .document(view.document_id)?
                        .document
                        .revision()
                        != revision
                {
                    return None;
                }
                let pane = self.geometry.pane(pane_id)?;
                if pane.text_area != text_area {
                    return None;
                }
                match axis {
                    Axis::Rows => pane.vertical_scrollbar,
                    Axis::Columns => pane.horizontal_scrollbar,
                }
            }
            ScrollbarTarget::Explorer { rows } => {
                if self.modal.is_some()
                    || !self.workspace.explorer_visible
                    || self.workspace.explorer.as_ref()?.rows().len() != rows
                {
                    return None;
                }
                self.geometry.sidebar_scrollbar
            }
            ScrollbarTarget::Help { viewport } => {
                if self.viewport != viewport || !matches!(self.modal, Some(Modal::Help { .. })) {
                    return None;
                }
                self.geometry.help_scrollbar
            }
        }
    }

    fn drag_scrollbar(&mut self, mouse: MouseEvent) {
        let Some(DragState::Scrollbar {
            target,
            bar,
            anchor,
        }) = self.drag
        else {
            return;
        };
        let valid = self
            .captured_scrollbar(target, bar.axis)
            .is_some_and(|current| {
                current.track == bar.track && current.max_offset == bar.max_offset
            });
        if !valid {
            self.drag = None;
            self.dragged = false;
            return;
        }
        let coordinate = match bar.axis {
            Axis::Columns => mouse.column,
            Axis::Rows => mouse.row,
        };
        self.dragged |= coordinate != anchor;
        if self.dragged {
            self.scroll_to(target, bar.axis, bar.drag_offset(anchor, coordinate));
        }
    }

    fn scroll_to(&mut self, target: ScrollbarTarget, axis: Axis, offset: usize) {
        match target {
            ScrollbarTarget::Editor { pane_id, .. } => {
                let Some((tab, _)) = self.workspace.pane_active_view_and_doc_mut(pane_id) else {
                    return;
                };
                let position = match axis {
                    Axis::Rows => &mut tab.top_line,
                    Axis::Columns => &mut tab.left_cell,
                };
                if *position == offset {
                    return;
                }
                *position = offset;
                self.highlight_pending |= axis == Axis::Rows;
            }
            ScrollbarTarget::Explorer { .. } => {
                let Some(explorer) = self.workspace.explorer.as_mut() else {
                    return;
                };
                let previous = explorer.top();
                explorer.set_top(offset);
                if previous == explorer.top() {
                    return;
                }
            }
            ScrollbarTarget::Help { .. } => {
                let Some(Modal::Help { scroll }) = &mut self.modal else {
                    return;
                };
                if *scroll == offset {
                    return;
                }
                *scroll = offset;
            }
        }
        self.invalidate_hit_targets_keep_drag();
    }

    fn scroll_width_pending(&self) -> bool {
        self.geometry.panes.iter().any(|pane| {
            pane.horizontal_track.is_some()
                && self
                    .workspace
                    .pane(pane.id)
                    .and_then(|pane| pane.active_tab())
                    .is_some_and(|view| view.horizontal_width.pending())
        })
    }

    fn update_scroll_metrics(&mut self) -> bool {
        if !self.hit_targets_valid {
            return false;
        }
        let mut changed = false;
        let mut viewport_changed = false;
        for pane in &self.geometry.panes {
            let Some((view, buf)) = self.workspace.pane_active_view_and_doc_mut(pane.id) else {
                continue;
            };
            let top = view.top_line.min(
                buf.document
                    .line_count()
                    .saturating_sub(usize::from(pane.text_area.height)),
            );
            if view.top_line != top {
                view.top_line = top;
                self.highlight_pending = true;
                viewport_changed = true;
            }
            if pane.horizontal_track.is_some() {
                view.horizontal_width
                    .prepare(&buf.document, top, pane.text_area.height);
                if let Some(width) = view.horizontal_width.measurement() {
                    let left = view.left_cell.min(
                        width
                            .cells
                            .saturating_sub(usize::from(pane.text_area.width)),
                    );
                    viewport_changed |= left != view.left_cell;
                    view.left_cell = left;
                }
            }
        }
        let count = self.geometry.panes.len();
        let started = Instant::now();
        let mut idle_visits = 0;
        while count > 0 && idle_visits < count && started.elapsed() < SCROLL_WIDTH_TICK_BUDGET {
            let index = self.scroll_scan_cursor % count;
            self.scroll_scan_cursor = (index + 1) % count;
            let pane = &self.geometry.panes[index];
            let Some((view, buf)) = self.workspace.pane_active_view_and_doc_mut(pane.id) else {
                idle_visits += 1;
                continue;
            };
            if pane.horizontal_track.is_none() || !view.horizontal_width.pending() {
                idle_visits += 1;
                continue;
            }
            idle_visits = 0;
            if view.horizontal_width.advance(&buf.document, 512) {
                changed = true;
                let width = view
                    .horizontal_width
                    .measurement()
                    .expect("completed width scan has a measurement")
                    .cells;
                let left = view
                    .left_cell
                    .min(width.saturating_sub(usize::from(pane.text_area.width)));
                viewport_changed |= left != view.left_cell;
                view.left_cell = left;
            }
        }
        if viewport_changed {
            self.invalidate_hit_targets_keep_drag();
        }
        changed || viewport_changed
    }

    fn drag_tick(&mut self) -> bool {
        let Some(DragState::Text {
            pane_id,
            view_id,
            x,
            y,
        }) = self.drag
        else {
            return false;
        };
        if !self.dragged {
            return false;
        }
        let area = self
            .geometry
            .pane(pane_id)
            .map_or(Rect::default(), |p| p.text_area);
        if area.is_empty() {
            return false;
        }
        let Some((tab, buf)) = self.workspace.view_and_doc_mut(pane_id, view_id) else {
            return false;
        };
        let doc = &buf.document;
        let before = (tab.cursor, tab.top_line, tab.left_cell);
        if y < area.y {
            tab.top_line = tab.top_line.saturating_sub(1);
        }
        if y >= area.bottom() {
            tab.top_line =
                (tab.top_line + 1).min(doc.line_count().saturating_sub(usize::from(area.height)));
        }
        if x < area.x {
            tab.left_cell = tab.left_cell.saturating_sub(1);
        }
        if x >= area.right() {
            let line = (tab.top_line + usize::from(y.saturating_sub(area.y).min(area.height - 1)))
                .min(doc.line_count().saturating_sub(1));
            let end_column = doc.column_of(doc.line_end(line));
            let maximum_left = end_column.saturating_sub(usize::from(area.width - 1));
            tab.left_cell = tab.left_cell.saturating_add(1).min(maximum_left);
        }
        tab.cursor = tab.hit(doc, area, x, y);
        let changed = before != (tab.cursor, tab.top_line, tab.left_cell);
        let viewport_changed = before.1 != tab.top_line || before.2 != tab.left_cell;
        let top_changed = before.1 != tab.top_line;
        if top_changed {
            self.highlight_pending = true;
        }
        if viewport_changed {
            self.invalidate_hit_targets_keep_drag();
        }
        changed
    }

    fn update_highlight(&mut self) -> bool {
        self.hl_docs_scratch.clear();
        if self.geometry.panes.is_empty() {
            if let Some(view) = self.workspace.active_view() {
                self.hl_docs_scratch.push(view.document_id);
            }
        } else {
            for pane_geom in &self.geometry.panes {
                if pane_geom.text_area.is_empty() {
                    continue;
                }
                if let Some(pane) = self.workspace.pane(pane_geom.id)
                    && let Some(view) = pane.active_tab()
                    && !self.hl_docs_scratch.contains(&view.document_id)
                {
                    self.hl_docs_scratch.push(view.document_id);
                }
            }
        }

        if self.hl_docs_scratch.is_empty() {
            self.highlight_pending = false;
            return false;
        }

        let mut any_pending = false;
        let mut need_redraw = false;

        for idx in 0..self.hl_docs_scratch.len() {
            let doc_id = self.hl_docs_scratch[idx];
            let Some(buf) = self.workspace.document(doc_id) else {
                continue;
            };
            let line_count = buf.document.line_count();
            self.hl_ranges_scratch.clear();
            if line_count > 0 {
                if self.geometry.panes.is_empty() {
                    if let Some(view) = self.workspace.active_view() {
                        let start = view.top_line.min(line_count);
                        let (w, h) = self.viewport;
                        let body = chrome::max_editor_area(Rect::new(0, 0, w, h));
                        let height = usize::from(body.height).max(1);
                        let end = start.saturating_add(height).min(line_count);
                        if start < end {
                            self.hl_ranges_scratch.push(start..end);
                        }
                    }
                } else {
                    for pane_geom in &self.geometry.panes {
                        if pane_geom.text_area.is_empty() {
                            continue;
                        }
                        if let Some(pane) = self.workspace.pane(pane_geom.id)
                            && let Some(view) = pane.active_tab()
                            && view.document_id == doc_id
                        {
                            let start = view.top_line.min(line_count);
                            let height = usize::from(pane_geom.text_area.height);
                            let end = start.saturating_add(height).min(line_count);
                            if start < end {
                                self.hl_ranges_scratch.push(start..end);
                            }
                        }
                    }
                }
            }

            let progress = self.highlighter.update(
                doc_id,
                &buf.document,
                buf.path.as_deref(),
                &self.hl_ranges_scratch,
                HIGHLIGHT_BUDGET,
            );
            any_pending |= progress.pending;
            need_redraw |= progress.changed;
        }

        self.highlight_pending = any_pending;
        need_redraw
    }

    fn event(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::ControlSequencePending => self.input_sequence_pending = true,
            InputEvent::ControlSequenceResumed => self.input_sequence_pending = false,
            InputEvent::PasteRejected => {
                self.status = StatusMessage::new(
                    StatusLevel::Error,
                    "Paste rejected: invalid UTF-8 or over 1 MiB; nothing inserted".into(),
                );
            }
            InputEvent::Terminal(Event::Paste(text)) => match &self.modal {
                Some(Modal::Path { .. }) => self.path_input(&text),
                Some(Modal::Search {
                    replace_mode,
                    replacement_focus,
                }) => {
                    let to_replacement = *replace_mode && *replacement_focus;
                    self.search_input(&text, to_replacement);
                }
                Some(_) => {}
                None => {
                    if !self.workspace.explorer_focused {
                        self.paste_in_active_tab(&text);
                        let area = self.active_text_area();
                        if let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() {
                            let prev_top = tab.top_line;
                            if tab.ensure_visible(&buf.document, area) {
                                if tab.top_line != prev_top {
                                    self.highlight_pending = true;
                                }
                                self.invalidate_hit_targets_keep_drag();
                            }
                        }
                    } else {
                        self.status = StatusMessage::new(
                            StatusLevel::Warning,
                            "Explorer focused; press Esc or F2 to focus editor before pasting"
                                .into(),
                        );
                    }
                }
            },
            InputEvent::Terminal(Event::Key(key)) => {
                self.key(key);
                if self.modal.is_none() && !self.workspace.explorer_focused {
                    let area = self.active_text_area();
                    if let Some((tab, buf)) = self.workspace.active_view_and_doc_mut() {
                        let prev_top = tab.top_line;
                        if tab.ensure_visible(&buf.document, area) {
                            if tab.top_line != prev_top {
                                self.highlight_pending = true;
                            }
                            self.invalidate_hit_targets_keep_drag();
                        }
                    }
                }
            }
            InputEvent::Terminal(Event::Mouse(mouse)) => self.mouse(mouse, Instant::now()),
            InputEvent::Terminal(_) => {}
        }
        true
    }

    fn render_with<R>(&mut self, draw_fn: impl FnOnce(&Screen<'_>, &mut Geometry) -> R) -> R {
        let active_pane_id = self.workspace.active_pane_id();
        let active_buf = self.workspace.active_document();

        let total_tabs: usize = self.workspace.panes().iter().map(|p| p.tabs().len()).sum();
        let mut all_captions: Vec<TabCaption<'_>> = Vec::with_capacity(total_tabs);
        let mut caption_ranges: Vec<(usize, usize)> =
            Vec::with_capacity(self.workspace.panes().len());
        for pane in self.workspace.panes() {
            let start = all_captions.len();
            self.workspace.push_pane_captions(pane, &mut all_captions);
            let end = all_captions.len();
            caption_ranges.push((start, end));
        }

        let mut pane_screens: Vec<PaneScreen<'_>> =
            Vec::with_capacity(self.workspace.panes().len());
        for (pane, (start, end)) in self.workspace.panes().iter().zip(caption_ranges) {
            let Some(view) = pane.active_tab() else {
                continue;
            };
            let Some(buf) = self.workspace.document(view.document_id) else {
                continue;
            };
            pane_screens.push(PaneScreen {
                id: pane.id,
                document_id: buf.id,
                document: &buf.document,
                cursor: view.cursor,
                selection: view.selection(),
                top_line: view.top_line,
                left_cell: view.left_cell,
                horizontal_extent: view.horizontal_width.measurement(),
                read_only: buf.read_only.is_some(),
                tabs: &all_captions[start..end],
                highlights: self.highlighter.lines(buf.id),
            });
        }

        let explorer = if self.workspace.explorer_visible {
            self.workspace.explorer.as_ref()
        } else {
            None
        };
        let dialog = match &self.modal {
            Some(Modal::Path { action, value }) => Some(Dialog::Path {
                title: match action {
                    PathAction::Open => "Open file",
                    PathAction::Save { .. } => "Save As",
                },
                value: value.view(),
            }),
            Some(Modal::Unsaved { focused, .. }) => Some(Dialog::Choices {
                title: "Unsaved changes",
                lines: ChoiceKind::Unsaved.message_lines(),
                kind: ChoiceKind::Unsaved,
                target: self
                    .modal
                    .as_ref()
                    .and_then(|modal| modal.target(&self.workspace)),
                focused: *focused,
            }),
            Some(Modal::Overwrite { focused, .. }) => Some(Dialog::Choices {
                title: "Confirm overwrite",
                lines: ChoiceKind::Overwrite.message_lines(),
                kind: ChoiceKind::Overwrite,
                target: self
                    .modal
                    .as_ref()
                    .and_then(|modal| modal.target(&self.workspace)),
                focused: *focused,
            }),
            Some(Modal::Conflict { focused, .. }) => Some(Dialog::Choices {
                title: "External file conflict",
                lines: ChoiceKind::Conflict.message_lines(),
                kind: ChoiceKind::Conflict,
                target: self
                    .modal
                    .as_ref()
                    .and_then(|modal| modal.target(&self.workspace)),
                focused: *focused,
            }),
            Some(Modal::Search {
                replace_mode,
                replacement_focus,
            }) => Some(Dialog::Search {
                query: self.search.query.view(),
                replacement: replace_mode.then(|| self.search.replacement.view()),
                replacement_focus: *replacement_focus,
                feedback: self.search.current_feedback(&self.workspace),
            }),
            Some(Modal::Help { scroll }) => Some(Dialog::Help { scroll: *scroll }),
            None => None,
        };
        let (active_path, format, dirty) = match active_buf {
            Some(buf) => (buf.path.as_deref(), buf.format, buf.document.is_dirty()),
            None => (None, crate::file_io::FileFormat::default(), false),
        };
        let model = Screen {
            panes: &pane_screens,
            active_pane: active_pane_id,
            tree: &self.workspace.tree,
            title: &self.workspace.root_label,
            status: if self.input_sequence_pending {
                "Ctrl+G resumes input; terminal sequence incomplete"
            } else {
                &self.status.text
            },
            status_level: if self.input_sequence_pending {
                StatusLevel::Warning
            } else {
                self.status.level
            },
            highlight_warning: active_buf.and_then(|buf| self.highlighter.warning(buf.id)),
            active_path,
            format,
            dirty,
            dialog,
            explorer,
            sidebar_width: self.workspace.sidebar_width,
            explorer_focused: self.workspace.explorer_focused,
            theme: &self.theme,
            icons: self.icons,
        };
        let out = draw_fn(&model, &mut self.geometry);
        self.hit_targets_valid = true;
        self.scroll_metrics_dirty = self.update_scroll_metrics();
        out
    }

    fn render(&mut self, session: &mut TerminalSession) -> io::Result<()> {
        self.render_with(|model, geometry| {
            session.draw(|frame| {
                ui::draw(frame, model, geometry);
            })
        })
    }

    fn render_step<E>(
        &mut self,
        skip_pre_render_highlight: &mut bool,
        prev_visible: &mut Vec<(u64, Rect)>,
        mut draw: impl FnMut(&mut Self) -> Result<(), E>,
    ) -> Result<bool, E> {
        let active_pane_id = self.workspace.active_pane_id();
        let previous_text_area = self.geometry.pane(active_pane_id).map(|p| p.text_area);
        let previous_sidebar = self.geometry.sidebar;
        prev_visible.clear();
        for p in &self.geometry.panes {
            prev_visible.push((p.id, p.text_area));
        }

        let just_rendered_highlight = *skip_pre_render_highlight;
        let mut highlighted_this_step = false;
        if !self.geometry.panes.is_empty() && self.highlight_pending && !just_rendered_highlight {
            self.update_highlight();
            highlighted_this_step = true;
        }
        *skip_pre_render_highlight = false;

        draw(self)?;

        let mut post_render_dirty = self.scroll_metrics_dirty;
        let new_text_area = self.geometry.pane(active_pane_id).map(|p| p.text_area);
        if let Some(area) = new_text_area
            && previous_text_area != Some(area)
            && let Some((tab, buf)) = self.workspace.active_view_and_doc_mut()
            && tab.ensure_visible(&buf.document, area)
        {
            post_render_dirty = true;
        }
        if previous_sidebar != self.geometry.sidebar {
            post_render_dirty |= self.ensure_explorer_visible();
        }
        let panes_changed = prev_visible.len() != self.geometry.panes.len()
            || prev_visible
                .iter()
                .zip(self.geometry.panes.iter())
                .any(|((id1, r1), p2)| *id1 != p2.id || *r1 != p2.text_area);
        // Scrollbar-only redraws do not change the syntax viewport.
        if panes_changed {
            self.highlight_pending = true;
        }
        if post_render_dirty {
            draw(self)?;
        }

        if !highlighted_this_step
            && !just_rendered_highlight
            && self.highlight_pending
            && self.update_highlight()
        {
            *skip_pre_render_highlight = true;
            return Ok(true);
        }
        Ok(self.scroll_metrics_dirty)
    }
}

fn resolve_parent_dir(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    match abs.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf())
        }
        _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}
fn copy_success_status(cut: bool, target: &'static str) -> String {
    if cut {
        format!("Cut to {target}")
    } else {
        format!("Copied to {target}")
    }
}

fn copy_failure_status(cut: bool, error: &ClipboardError) -> String {
    if cut {
        format!("Cut failed: {error}; source retained")
    } else {
        format!("Copy failed: {error}")
    }
}

fn no_selection_status(cut: bool) -> String {
    if cut {
        "No selection; Ctrl+X does nothing".into()
    } else {
        "No selection; Ctrl+C does not exit".into()
    }
}

fn normalize_paste(text: &str) -> Cow<'_, str> {
    let mut parts = text.split('\r');
    let first = parts.next().unwrap_or_default();
    let Some(second) = parts.next() else {
        return Cow::Borrowed(text);
    };
    let mut normalized = String::with_capacity(text.len());
    normalized.push_str(first);
    for part in std::iter::once(second).chain(parts) {
        normalized.push('\n');
        normalized.push_str(part.strip_prefix('\n').unwrap_or(part));
    }
    Cow::Owned(normalized)
}

fn byte_for_column(stops: &[(u16, usize)], col: u16) -> usize {
    if stops.is_empty() {
        return 0;
    }
    if col <= stops[0].0 {
        return stops[0].1;
    }
    if col >= stops.last().unwrap().0 {
        return stops.last().unwrap().1;
    }
    stops
        .iter()
        .rev()
        .find(|(x, _)| *x <= col)
        .map(|(_, b)| *b)
        .unwrap_or(0)
}

pub(crate) fn run(
    path: Option<PathBuf>,
    options: Options,
    session_enabled: bool,
) -> io::Result<()> {
    crate::terminal::verify_tty()?;
    let mut app = if session_enabled {
        App::bootstrap_session(options).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{error}; use --no-session for separate editing"),
            )
        })?
    } else {
        App::bootstrap(path, options)?
    };
    crate::terminal::use_theme_color_policy();
    let mut session = TerminalSession::enter()?;
    let outcome = run_loop(&mut session, &mut app);
    let restored = session.restore();
    match (outcome, restored) {
        (Err(error), Err(cleanup)) => Err(io::Error::other(format!(
            "{error}; terminal restore failed: {cleanup}"
        ))),
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

fn sync_dimensions(
    session: &TerminalSession,
    app: &mut App,
    size: &mut (u16, u16),
) -> io::Result<bool> {
    let observed = session.size()?;
    if observed == *size {
        return Ok(false);
    }
    *size = observed;
    app.viewport = observed;
    app.highlight_pending = true;
    app.invalidate_hit_targets();
    Ok(true)
}

fn run_loop(session: &mut TerminalSession, app: &mut App) -> io::Result<()> {
    let mut input = Input::new();
    let mut size = session.size()?;
    app.viewport = size;
    let mut redraw = true;
    let mut skip_pre_render_highlight = false;
    let mut prev_visible: Vec<(u64, Rect)> = Vec::new();
    while !app.quit {
        if let Some(signal) = session.pending_signal() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                format!(
                    "terminated by {}{}",
                    signal.name(),
                    if app.workspace.has_dirty_documents() {
                        "; unsaved changes were not written (no crash recovery)"
                    } else {
                        ""
                    }
                ),
            ));
        }
        redraw |= sync_dimensions(session, app, &mut size)?;
        if redraw {
            redraw = app.render_step(&mut skip_pre_render_highlight, &mut prev_visible, |app| {
                app.render(session)
            })?;
        }
        let wait_timeout = if redraw {
            Duration::ZERO
        } else if app.highlight_pending || app.scroll_width_pending() {
            HIGHLIGHT_IDLE_TICK
        } else {
            TICK
        };
        if let Some(event) = input.poll(wait_timeout)? {
            skip_pre_render_highlight = false;
            // A split preflight must observe a resize that happened during poll,
            // just as mouse hits and modal decisions must.
            redraw |= sync_dimensions(session, app, &mut size)?;
            redraw |= app.event(event);
            for _ in 1..64 {
                if app.quit || session.pending_signal().is_some() {
                    break;
                }
                let Some(event) = input.poll(Duration::ZERO)? else {
                    break;
                };
                redraw |= sync_dimensions(session, app, &mut size)?;
                redraw |= app.event(event);
            }
        } else {
            redraw |= app.drag_tick();
            redraw |= app.update_scroll_metrics();
            if redraw {
                skip_pre_render_highlight = false;
            } else if app.highlight_pending && app.update_highlight() {
                redraw = true;
                skip_pre_render_highlight = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::{ClipboardMode, MAX_CLIPBOARD_BYTES};
    use crate::ui::{PaneGeometry, TabHit};
    use ratatui::{Terminal, backend::TestBackend};

    fn internal_options() -> Options {
        let mut options = Options::default();
        options.clipboard = ClipboardMode::Internal;
        options
    }

    fn test_app(root: &Path) -> App {
        let mut app = App::new_with_root(root.to_path_buf(), internal_options());
        // Mouse fixtures below supply geometry from a completed frame.
        app.hit_targets_valid = true;
        app
    }

    fn draw_frame(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        app.viewport = (width, height);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        app.render_with(|model, geometry| {
            terminal
                .draw(|frame| {
                    ui::draw(frame, model, geometry);
                })
                .unwrap();
        });
        terminal
    }

    fn draw_measured_frame(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = draw_frame(app, width, height);
        // Complete fixture measurement deterministically, independent of the idle time budget.
        for pane in &app.geometry.panes {
            if let Some((view, buf)) = app.workspace.pane_active_view_and_doc_mut(pane.id)
                && pane.horizontal_track.is_some()
            {
                view.horizontal_width.advance(&buf.document, usize::MAX);
            }
        }
        app.render_step(&mut false, &mut Vec::new(), |app| {
            app.render_with(|model, geometry| {
                terminal
                    .draw(|frame| ui::draw(frame, model, geometry))
                    .map(|_| ())
            })
        })
        .unwrap();
        terminal
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    fn fkey(n: u8) -> KeyEvent {
        KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)
    }

    fn set_single_pane_text_area(app: &mut App, text_area: Rect) {
        let pane_id = app.workspace.active_pane_id();
        app.geometry.panes = vec![PaneGeometry {
            id: pane_id,
            area: text_area,
            text_area,
            tabs: Vec::new(),
            previous_tab: None,
            next_tab: None,
            ..PaneGeometry::default()
        }];
    }

    #[test]
    fn keyboard_scrolling_rejects_click_from_previous_frame() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab(&"line\n".repeat(40));
        app.workspace.active_view_mut().unwrap().cursor = 0;
        draw_frame(&mut app, 80, 12);
        let area = app.active_text_area();
        app.event(InputEvent::Terminal(Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::NONE,
        ))));
        let moved = app.workspace.active_view().unwrap().cursor;
        assert!(app.workspace.active_view().unwrap().top_line > 0);
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_view().unwrap().cursor, moved);
    }

    #[test]
    fn autoscroll_release_rejects_stale_click_until_new_frame() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab(&"line\n".repeat(40));
        app.workspace.active_view_mut().unwrap().cursor = 0;
        draw_frame(&mut app, 80, 12);
        let area = app.active_text_area();
        let mut mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        };
        app.mouse(mouse, Instant::now());
        mouse.kind = MouseEventKind::Drag(MouseButton::Left);
        mouse.row = area.bottom();
        app.mouse(mouse, Instant::now());
        let dragged_cursor = app.workspace.active_view().unwrap().cursor;
        assert!(app.workspace.active_view().unwrap().top_line > 0);
        mouse.kind = MouseEventKind::Up(MouseButton::Left);
        app.mouse(mouse, Instant::now());
        mouse.kind = MouseEventKind::Down(MouseButton::Left);
        mouse.row = area.y;
        app.mouse(mouse, Instant::now());
        assert_eq!(app.workspace.active_view().unwrap().cursor, dragged_cursor);
        draw_frame(&mut app, 80, 12);
        app.mouse(mouse, Instant::now());
        assert!(app.workspace.active_view().unwrap().cursor < dragged_cursor);
    }

    #[test]
    fn cancelling_save_as_during_exit_retains_dirty_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("unsaved");
        app.request_quit();
        app.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(matches!(
            app.modal,
            Some(Modal::Path {
                action: PathAction::Save { .. },
                ..
            })
        ));
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.quit);
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
        assert!(app.after_save.is_none());
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "unsaved"
        );
    }

    #[test]
    fn failed_save_does_not_complete_pending_quit_or_clear_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let doc_id = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("keep me");
        app.after_save = Some(AfterSave::Quit);
        app.save_document_to(
            doc_id,
            dir.path().join("missing").join("file.txt"),
            false,
            true,
        );
        assert!(!app.quit);
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
        assert!(app.after_save.is_none());
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep me"
        );
    }

    #[test]
    fn external_conflict_requires_explicit_choice_before_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut app = test_app(dir.path());
        app.open_tab_from_path(path.clone()).unwrap();
        app.replace_in_active_tab("local ");
        std::fs::write(&path, "external").unwrap();
        app.save_active_document();
        assert!(matches!(app.modal, Some(Modal::Conflict { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external");
        app.modal_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "local original");
        assert!(!app.workspace.active_document().unwrap().document.is_dirty());
    }

    #[test]
    fn save_as_current_path_still_requires_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "original").unwrap();
        let mut app = test_app(dir.path());
        app.open_tab_from_path(path.clone()).unwrap();
        let doc_id = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("local ");
        app.save_document_to(doc_id, path.clone(), false, true);
        assert!(matches!(app.modal, Some(Modal::Overwrite { .. })));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
        app.cancel_modal();
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
    }

    #[test]
    fn drag_scroll_redraws_even_when_cursor_is_already_at_eof() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let pane_id = app.workspace.active_pane_id();
        let view_id = app.workspace.active_view_id();
        let doc_id = app.workspace.active_document().unwrap().id;
        app.workspace
            .edit_transaction(doc_id, view_id, EditViewPolicy::FollowEdit, |doc| {
                let c = doc.replace(0..0, "a\nb\nc").unwrap();
                (c, Some(c))
            });
        app.workspace.active_view_mut().unwrap().top_line = 1;
        set_single_pane_text_area(&mut app, Rect::new(3, 1, 5, 1));
        app.drag = Some(DragState::Text {
            pane_id,
            view_id,
            x: 7,
            y: 5,
        });
        app.dragged = true;
        assert!(app.drag_tick());
        let tab = app.workspace.active_view().unwrap();
        let doc = &app.workspace.active_document().unwrap().document;
        assert_eq!(tab.cursor, doc.len_chars());
        assert_eq!(tab.top_line, 2);
        assert!(!app.drag_tick());
    }

    #[test]
    fn tiny_modal_refuses_blind_discard_but_allows_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("keep this");
        app.request_quit();
        app.viewport = (8, 2);
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(!app.quit);
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
        assert!(matches!(app.modal, Some(Modal::Unsaved { .. })));
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.modal.is_none());
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep this"
        );
    }

    #[test]
    fn multi_tab_quit_discard_and_cancel_preserves_remaining_dirty_and_clean_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let clean_path = dir.path().join("clean.txt");
        std::fs::write(&clean_path, "clean").unwrap();

        let mut app = test_app(dir.path());
        let doc1 = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("dirty1");

        app.key(ctrl('n'));
        let doc2 = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("dirty2");

        app.open_tab_from_path(clean_path).unwrap();
        let doc3 = app.workspace.active_document().unwrap().id;

        app.key(ctrl('q'));
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == doc1
        ));

        // Discard doc1: doc1 is removed (never marked saved), quit continues to doc2
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.workspace.document(doc1).is_none());
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == doc2
        ));
        assert_eq!(app.workspace.active_document().unwrap().id, doc2);

        // Save doc2 -> Save As -> Esc cancels remaining Quit
        app.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(matches!(
            app.modal,
            Some(Modal::Path {
                action: PathAction::Save { doc_id },
                ..
            }) if doc_id == doc2
        ));
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.quit);
        assert!(app.modal.is_none());
        assert!(app.after_save.is_none());
        assert!(app.workspace.document(doc2).unwrap().document.is_dirty());
        assert_eq!(
            app.workspace
                .document(doc2)
                .unwrap()
                .document
                .text()
                .to_string(),
            "dirty2"
        );
        assert!(app.workspace.document(doc3).is_some());
    }

    #[test]
    fn multi_tab_quit_drains_all_tabs_when_completed() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("one.txt");
        std::fs::write(&p1, "old").unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(p1.clone()).unwrap();
        app.replace_in_active_tab("new ");

        app.key(ctrl('n'));
        app.replace_in_active_tab("scratch");

        app.key(ctrl('q'));
        // Save first dirty doc
        app.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(std::fs::read_to_string(&p1).unwrap(), "new old");
        // Prompt moves to second dirty doc
        assert!(matches!(app.modal, Some(Modal::Unsaved { .. })));
        // Discard second dirty doc -> drains remaining docs/panes and exits
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.quit);
        assert!(app.workspace.documents().is_empty());
        assert!(app.workspace.panes().is_empty());
    }

    #[test]
    fn closing_last_dirty_tab_creates_fresh_blank_buffer_without_quitting() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let first_view = app.workspace.active_view_id();
        let first_doc = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("unsaved");
        app.key(ctrl('w'));
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == first_doc
        ));
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(!app.quit);
        assert_eq!(app.workspace.panes().len(), 1);
        assert_eq!(app.workspace.active_pane().unwrap().tabs().len(), 1);
        let blank_view = app.workspace.active_view().unwrap();
        let blank_doc = app.workspace.active_document().unwrap();
        assert_ne!(blank_view.id, first_view);
        assert_ne!(blank_doc.id, first_doc);
        assert!(!blank_doc.document.is_dirty());
        assert_eq!(blank_doc.document.len_chars(), 0);
    }

    #[test]
    fn closing_inactive_tab_activates_it_and_invalidates_stale_hit_targets() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let id1 = app.workspace.active_view_id();
        app.replace_in_active_tab("first");

        app.key(ctrl('n'));
        let id2 = app.workspace.active_view_id();
        let doc2 = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("second");

        // Switch back to tab 1
        app.key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(app.workspace.active_view_id(), id1);

        // Simulate rendered hit target for tab 2's close button at (20, 1)
        let close_rect = Rect::new(20, 1, 1, 1);
        app.geometry.panes = vec![PaneGeometry {
            id: app.workspace.active_pane_id(),
            area: Rect::new(0, 1, 80, 20),
            text_area: Rect::new(3, 2, 77, 19),
            tabs: vec![
                TabHit {
                    id: id1,
                    body: Rect::new(2, 1, 8, 1),
                    close: Rect::new(10, 1, 1, 1),
                },
                TabHit {
                    id: id2,
                    body: Rect::new(12, 1, 8, 1),
                    close: close_rect,
                },
            ],
            previous_tab: None,
            next_tab: None,
            ..PaneGeometry::default()
        }];
        app.hit_targets_valid = true;

        // Clicking inactive dirty tab 2's close button activates tab 2 for confirmation
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 20,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_view_id(), id2);
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == doc2
        ));
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.workspace.view(id2).is_none());
        assert!(app.workspace.document(doc2).is_none());
        assert_eq!(app.workspace.active_view_id(), id1);
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "first"
        );
    }

    #[test]
    fn save_as_collision_rejects_overwrite_and_cancels_quit() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::write(&file_a, "content A").unwrap();
        std::fs::write(&file_b, "content B").unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(file_a.clone()).unwrap();
        let doc_a = app.workspace.active_document().unwrap().id;

        app.open_tab_from_path(file_b.clone()).unwrap();
        let doc_b = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("edited ");
        app.after_save = Some(AfterSave::Quit);

        app.save_document_to(doc_b, file_a.clone(), false, true);
        assert!(!app.quit);
        assert!(app.after_save.is_none());
        assert_eq!(std::fs::read_to_string(&file_a).unwrap(), "content A");
        assert!(app.workspace.document(doc_b).unwrap().document.is_dirty());
        assert_eq!(
            app.workspace
                .document(doc_a)
                .unwrap()
                .document
                .text()
                .to_string(),
            "content A"
        );
    }

    #[test]
    fn search_replace_validates_revision_and_blocks_stale_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("foo bar foo");

        // Open Replace via Ctrl+R
        app.key(ctrl('r'));
        assert!(matches!(
            app.modal,
            Some(Modal::Search {
                replace_mode: true,
                replacement_focus: false
            })
        ));
        for ch in "foo".chars() {
            app.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for ch in "baz".chars() {
            app.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        // Move cursor to start before finding
        app.workspace.active_view_mut().unwrap().cursor = 0;
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..3));

        // Esc closes modal without erasing last query
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.modal.is_none());
        assert_eq!(app.search.query.as_str(), "foo");
        assert_eq!(app.search.replacement.as_str(), "baz");

        // Edit document manually -> old match revision becomes stale
        app.workspace.active_view_mut().unwrap().anchor = None;
        app.workspace.active_view_mut().unwrap().cursor = 0;
        app.replace_in_active_tab("XYZ");

        // Reopen Replace and press F6 directly -> stale range must be rejected!
        app.key(ctrl('r'));
        app.key(fkey(6));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "XYZfoo bar foo"
        );

        // Find fresh match at rev, then F6 replaces and advances to next
        app.workspace.active_view_mut().unwrap().cursor = 0;
        app.key(fkey(3));
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(3..6));
        app.key(fkey(6));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "XYZbaz bar foo"
        );
        assert_eq!(
            app.workspace.active_view().unwrap().selection(),
            Some(11..14)
        );

        // F7 Replace All in one undo step
        app.key(fkey(7));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "XYZbaz bar baz"
        );
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.key(ctrl('z'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "XYZbaz bar foo"
        );
    }

    #[test]
    fn search_modal_enforces_min_size_and_read_only_protection() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("abc");
        let doc_id = app.workspace.active_document().unwrap().id;
        app.workspace.document_mut(doc_id).unwrap().read_only = Some("locked".into());

        app.key(ctrl('r'));
        app.viewport = (39, 9);
        app.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(app.search.query.is_empty());

        app.viewport = (80, 24);
        app.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        app.key(fkey(3));
        app.key(fkey(6));
        app.key(fkey(7));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "abc"
        );
    }

    #[test]
    fn explorer_wheel_scrolls_without_changing_keyboard_focus() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(dir.path().join(format!("f{i:02}.txt")), "x").unwrap();
        }
        let mut app = test_app(dir.path());
        // Frame layout: parent row first, then the tree rows.
        app.geometry.sidebar_panel = Some(Rect::new(0, 2, 26, 7));
        app.geometry.sidebar_parent = Some(Rect::new(1, 3, 24, 1));
        app.geometry.sidebar = Some(Rect::new(1, 4, 24, 4));
        set_single_pane_text_area(&mut app, Rect::new(30, 2, 50, 5));
        assert!(!app.workspace.explorer_focused);

        // The wheel lands on the parent navigation row, which is part of the
        // sidebar panel: it scrolls the tree without stealing focus.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 5,
                row: 3,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(!app.workspace.explorer_focused);
        assert!(app.workspace.explorer.as_ref().unwrap().top() > 0);
    }

    #[test]
    fn queued_sidebar_click_cannot_use_rows_shifted_by_keyboard_navigation() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            std::fs::write(dir.path().join(format!("f{i:02}.txt")), "content").unwrap();
        }
        let mut app = test_app(dir.path());
        app.workspace.explorer_focused = true;
        app.geometry.sidebar_panel = Some(Rect::new(0, 2, 26, 7));
        app.geometry.sidebar_parent = Some(Rect::new(1, 3, 24, 1));
        app.geometry.sidebar = Some(Rect::new(1, 4, 24, 4));
        app.explorer_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.workspace.explorer.as_ref().unwrap().top(), 1);
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 8,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(app.workspace.active_document().unwrap().path.is_none());
        assert_eq!(app.workspace.explorer.as_ref().unwrap().selected(), 4);
    }

    #[test]
    fn explorer_frame_border_click_focuses_without_selecting_or_cursor_change() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..4 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "x").unwrap();
        }
        let mut app = test_app(dir.path());
        app.geometry.sidebar_panel = Some(Rect::new(0, 2, 26, 7));
        app.geometry.sidebar_parent = Some(Rect::new(1, 3, 24, 1));
        app.geometry.sidebar = Some(Rect::new(1, 4, 24, 4));
        set_single_pane_text_area(&mut app, Rect::new(30, 2, 50, 5));
        let selected_before = app.workspace.explorer.as_ref().unwrap().selected();
        let cursor_before = app.workspace.active_view().unwrap().cursor;
        assert!(!app.workspace.explorer_focused);

        // Click lands on the framed panel's top border, outside the rows rect.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 8,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(app.workspace.explorer_focused);
        assert!(app.workspace.active_document().unwrap().path.is_none());
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().selected(),
            selected_before
        );
        assert_eq!(app.workspace.active_view().unwrap().cursor, cursor_before);
        assert!(app.drag.is_none());
        assert!(app.mouse_down.is_none());
    }

    #[test]
    fn explorer_wheel_over_frame_border_scrolls_by_rows_height() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            std::fs::write(dir.path().join(format!("f{i:02}.txt")), "content").unwrap();
        }
        let mut app = test_app(dir.path());
        app.geometry.sidebar_panel = Some(Rect::new(0, 2, 26, 7));
        app.geometry.sidebar_parent = Some(Rect::new(1, 3, 24, 1));
        app.geometry.sidebar = Some(Rect::new(1, 4, 24, 4));
        let mut wheel = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 8, // bottom border row of the panel, outside the rows rect
            modifiers: KeyModifiers::NONE,
        };
        for expected in [3, 6, 8] {
            app.mouse(wheel, Instant::now());
            assert_eq!(app.workspace.explorer.as_ref().unwrap().top(), expected);
            // Frames redraw after every accepted wheel tick.
            app.hit_targets_valid = true;
        }
        // 12 rows - 4 visible rows = 8: clamped by the inner rows height
        // (panel minus border and parent row), not the 7-row panel height.
        wheel.kind = MouseEventKind::ScrollUp;
        wheel.row = 2; // top border row of the panel
        app.mouse(wheel, Instant::now());
        assert_eq!(app.workspace.explorer.as_ref().unwrap().top(), 5);
    }

    #[test]
    fn explorer_fit_follows_chrome_sidebar_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        app.viewport = (120, 36);
        assert!(app.explorer_fits_viewport());

        // Body shorter than a framed pane cannot host the sidebar.
        app.viewport = (10, 4);
        assert!(!app.explorer_fits_viewport());

        // Full tree fits without a sidebar, so the explorer hides instead of
        // squeezing the editor below its minimum.
        app.viewport = (30, 20);
        assert!(!app.explorer_fits_viewport());
    }

    #[test]
    fn gutter_navigation_preserves_line_hits_without_treating_frame_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("first\nsecond\nthird");
        app.workspace.active_view_mut().unwrap().cursor = 0;
        draw_frame(&mut app, 80, 24);
        let (panel, text_area) = (app.geometry.panes[0].area, app.geometry.panes[0].text_area);
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: text_area.x - 1,
            row: text_area.y + 1,
            modifiers: KeyModifiers::NONE,
        };
        app.mouse(click, Instant::now());
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..click
            },
            Instant::now(),
        );
        let expected = app
            .workspace
            .active_document()
            .unwrap()
            .document
            .position_at(1, 0);
        assert_eq!(app.workspace.active_view().unwrap().cursor, expected);
        draw_frame(&mut app, 80, 24);
        app.mouse(
            MouseEvent {
                column: panel.x,
                ..click
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_view().unwrap().cursor, expected);
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "first\nsecond\nthird"
        );
    }

    #[test]
    fn pane_frame_border_click_focuses_pane_without_moving_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("hello\nworld\n");
        let pane_id = app.workspace.active_pane_id();
        let cursor_before = app.workspace.active_view().unwrap().cursor;
        app.geometry.panes = vec![PaneGeometry {
            id: pane_id,
            area: Rect::new(0, 1, 80, 20),
            text_area: Rect::new(3, 4, 74, 15),
            tabs: Vec::new(),
            previous_tab: None,
            next_tab: None,
            ..PaneGeometry::default()
        }];
        app.hit_targets_valid = true;

        // Click on the framed panel's left border, outside the text area.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_pane_id(), pane_id);
        assert_eq!(app.workspace.active_view().unwrap().cursor, cursor_before);
        assert!(app.drag.is_none());
        assert!(app.mouse_down.is_none());

        // MouseUp stays a clean no-op even though no drag ever started.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 0,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(app.drag.is_none());
        assert!(app.mouse_down.is_none());
    }

    #[test]
    fn editor_wheel_only_scrolls_its_area_and_rejects_queued_stale_click() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let view_id = app.workspace.active_view_id();
        let doc_id = app.workspace.active_document().unwrap().id;
        app.workspace
            .edit_transaction(doc_id, view_id, EditViewPolicy::MapCursor, |doc| {
                let c = doc.replace(0..0, "zero\none\ntwo\nthree\nfour\n").unwrap();
                (c, Some(c))
            });
        set_single_pane_text_area(&mut app, Rect::new(30, 2, 50, 3));
        let cursor_before_wheel = app.workspace.active_view().unwrap().cursor;
        let mut mouse = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 1,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        app.mouse(mouse, Instant::now());
        assert_eq!(app.workspace.active_view().unwrap().top_line, 0);
        mouse.column = 30;
        mouse.row = 2;
        app.mouse(mouse, Instant::now());
        mouse.kind = MouseEventKind::Down(MouseButton::Left);
        app.mouse(mouse, Instant::now());
        let tab = app.workspace.active_view().unwrap();
        assert_eq!(tab.top_line, 3);
        assert_eq!(tab.cursor, cursor_before_wheel);
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
    }

    #[test]
    fn explorer_navigation_focus_and_open_flows() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("alpha_dir");
        std::fs::create_dir(&sub).unwrap();
        let file = sub.join("note.txt");
        std::fs::write(&file, "from tree").unwrap();

        let mut app = test_app(dir.path());
        app.key(fkey(2));
        assert!(app.workspace.explorer_focused);

        // Expand directory and enter file
        app.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.workspace.explorer_focused);
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "from tree"
        );

        // Ctrl+B hides explorer without discarding state; Esc returns Explorer->Editor
        app.key(fkey(2));
        assert!(app.workspace.explorer_focused);
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.workspace.explorer_focused);
        app.key(fkey(2));
        assert!(app.workspace.explorer_focused);
        app.key(ctrl('b'));
        assert!(!app.workspace.explorer_visible);
        assert!(!app.workspace.explorer_focused);
    }

    #[test]
    fn explorer_enter_and_backspace_navigate_while_dirty_document_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("alpha_dir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("note.txt"), "from tree").unwrap();
        std::fs::write(dir.path().join("top.txt"), "t").unwrap();

        let file = dir.path().join("doc.txt");
        std::fs::write(&file, "dirty text").unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(file.clone()).unwrap();
        app.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        app.replace_in_active_tab("dirty text edited");
        let doc_id = app.workspace.active_document().unwrap().id;
        assert!(app.workspace.document(doc_id).unwrap().document.is_dirty());

        app.key(fkey(2));
        assert!(app.workspace.explorer_focused);
        // Rows are folder-first, so the directory is selected by default.
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.workspace.explorer_focused);
        assert_eq!(app.workspace.explorer_root, sub);
        assert_eq!(app.workspace.root_label, sub.to_string_lossy());
        let exp = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(exp.rows().len(), 1);
        assert_eq!(exp.rows()[0].name, "note.txt");
        assert_eq!(exp.selected(), 0);
        assert_eq!(exp.top(), 0);

        // Re-rooting never touches open documents.
        let buf = app.workspace.document(doc_id).unwrap();
        assert!(buf.document.is_dirty());
        assert_eq!(buf.document.text().to_string(), "dirty text edited");
        assert_eq!(buf.path.as_deref(), Some(file.as_path()));

        // Backspace browses up and selects the previous root folder again.
        app.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(app.workspace.explorer_focused);
        assert_eq!(app.workspace.explorer_root, dir.path());
        assert_eq!(app.workspace.root_label, dir.path().to_string_lossy());
        let exp = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(exp.selected(), 0);
        assert_eq!(exp.rows()[0].name, "alpha_dir");
        assert_eq!(
            app.workspace
                .document(doc_id)
                .unwrap()
                .document
                .text()
                .to_string(),
            "dirty text edited"
        );
    }

    #[test]
    fn explorer_parent_row_click_navigates_and_rejects_stale_repeat() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("alpha_dir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("note.txt"), "n").unwrap();

        let file = dir.path().join("doc.txt");
        std::fs::write(&file, "keep").unwrap();

        // Browsing root is the inner folder so the parent stays in the fixture.
        let mut app = test_app(&sub);
        app.open_tab_from_path(file.clone()).unwrap();
        app.workspace.active_view_mut().unwrap().cursor = 2;
        let cursor_before = app.workspace.active_view().unwrap().cursor;

        draw_frame(&mut app, 80, 24);
        let parent = app.geometry.sidebar_parent.unwrap();

        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: parent.x,
                row: parent.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(app.workspace.explorer_focused);
        assert_eq!(app.workspace.explorer_root, dir.path());
        let exp = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(exp.selected(), 0);
        assert_eq!(exp.rows()[0].name, "alpha_dir");

        // The parent-row click is handled as navigation, not forwarded as
        // Backspace to the active document.
        assert_eq!(app.workspace.active_view().unwrap().cursor, cursor_before);
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep"
        );

        // Without a redraw in between, a second parent-row click is stale and
        // cannot navigate past the fixture.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: parent.x,
                row: parent.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.explorer_root, dir.path());
        assert_eq!(app.workspace.explorer.as_ref().unwrap().selected(), 0);
    }

    #[test]
    fn explorer_mouse_folder_click_toggles_tree_without_navigation() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("alpha_dir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("note.txt"), "n").unwrap();

        let mut app = test_app(dir.path());
        app.workspace.explorer_focused = true;
        app.geometry.sidebar_panel = Some(Rect::new(0, 2, 26, 8));
        app.geometry.sidebar_parent = Some(Rect::new(1, 3, 24, 1));
        app.geometry.sidebar = Some(Rect::new(1, 4, 24, 4));

        // Click the folder row directly below the parent row.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        let exp = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(app.workspace.explorer_root, dir.path());
        assert_eq!(exp.rows().len(), 2);
        assert!(exp.rows()[0].expanded);
        assert_eq!(exp.rows()[1].name, "note.txt");

        // A second click collapses again; still no re-rooting.
        app.hit_targets_valid = true;
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        let exp = app.workspace.explorer.as_ref().unwrap();
        assert_eq!(app.workspace.explorer_root, dir.path());
        assert_eq!(exp.rows().len(), 1);
        assert!(!exp.rows()[0].expanded);
    }

    #[test]
    fn explorer_enter_failure_and_empty_selection_preserve_state() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone_dir");
        std::fs::create_dir(&gone).unwrap();

        let mut app = test_app(dir.path());
        app.key(fkey(2));
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().rows()[0].name,
            "gone_dir"
        );
        std::fs::remove_dir(&gone).unwrap();
        let root_before = app.workspace.explorer_root.clone();
        let label_before = app.workspace.root_label.clone();
        let rows_before = app.workspace.explorer.as_ref().unwrap().rows().to_vec();

        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.workspace.explorer_root, root_before);
        assert_eq!(app.workspace.root_label, label_before);
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().rows(),
            rows_before.as_slice()
        );
        assert_eq!(app.status.level, StatusLevel::Warning);

        // Enter with no selectable row on an empty root is a safe no-op.
        let empty = tempfile::tempdir().unwrap();
        let mut empty_app = test_app(empty.path());
        empty_app.workspace.explorer_focused = true;
        empty_app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(empty_app.workspace.explorer_root, empty.path());
        assert!(
            empty_app
                .workspace
                .explorer
                .as_ref()
                .unwrap()
                .rows()
                .is_empty()
        );
    }

    #[test]
    fn search_shift_f3_reverse_and_adjacent_matches() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("foofoo bar");
        app.workspace.active_view_mut().unwrap().cursor = 0;

        app.key(ctrl('f'));
        for ch in "foo".chars() {
            app.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..3));

        app.key(fkey(3));
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(3..6));

        app.key(KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT));
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..3));

        // Close modal with Esc and repeat F3 / Shift+F3 directly in editor
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.modal.is_none());
        app.key(KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT));
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(3..6));
        app.key(fkey(3));
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..3));
    }

    #[test]
    fn divider_drag_clamps_width_and_preserves_minimum_editor() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.viewport = (50, 24);
        app.geometry.explorer_divider = Some(Rect::new(28, 1, 1, 20));

        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 28,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 48,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        // 50 - 1 (divider) - 12 (blank doc min pane width) = 37
        assert_eq!(app.workspace.sidebar_width, 37);

        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.sidebar_width, 18);

        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(app.drag.is_none());
    }

    #[test]
    fn atomic_save_updates_identity_and_preserves_dedup() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("atomic.txt");
        std::fs::write(&path, "v1").unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(path.clone()).unwrap();
        let view_id = app.workspace.active_view_id();
        let doc_id = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("v2 ");
        app.save_active_document();

        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(
            app.workspace
                .document(doc_id)
                .unwrap()
                .baseline
                .as_ref()
                .unwrap()
                .identity(),
            (meta.dev(), meta.ino())
        );
        let outcome = app.workspace.open_path(path).unwrap();
        assert!(outcome.already_in_pane);
        assert_eq!(outcome.view_id, view_id);
        assert_eq!(app.workspace.active_view_id(), view_id);
    }

    #[test]
    fn bootstrap_handles_folder_missing_file_and_rejects_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let folder_app =
            App::bootstrap(Some(dir.path().to_path_buf()), internal_options()).unwrap();
        assert_eq!(folder_app.workspace.panes().len(), 1);
        assert!(
            !folder_app
                .workspace
                .active_document()
                .unwrap()
                .document
                .is_dirty()
        );

        let missing = dir.path().join("not_created_yet.txt");
        let missing_app = App::bootstrap(Some(missing.clone()), internal_options()).unwrap();
        assert!(!missing.exists());
        assert_eq!(
            missing_app
                .workspace
                .active_document()
                .unwrap()
                .path
                .as_deref(),
            Some(missing.as_path())
        );
        assert!(
            !missing_app
                .workspace
                .active_document()
                .unwrap()
                .document
                .is_dirty()
        );

        let bad = dir.path().join("bad.bin");
        std::fs::write(&bad, [0xFF, 0xFE, 0x00]).unwrap();
        assert!(App::bootstrap(Some(bad), internal_options()).is_err());
    }

    #[test]
    fn reload_forgets_highlighter_and_discards_local_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("code.rs");
        std::fs::write(&path, "fn main() {}\n// line2\n// line3\n").unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(path.clone()).unwrap();
        let doc_id = app.workspace.active_document().unwrap().id;
        app.update_highlight();

        app.replace_in_active_tab("/* edit */\n");
        std::fs::write(&path, "let x = 1;\n").unwrap();
        app.save_active_document();
        assert!(matches!(app.modal, Some(Modal::Conflict { .. })));
        app.modal_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        let buf = app.workspace.document(doc_id).unwrap();
        assert_eq!(buf.document.text().to_string(), "let x = 1;\n");
        assert_eq!(buf.document.revision(), 0);
        assert!(!buf.document.is_dirty());
        app.update_highlight();
        assert!(app.highlighter.lines(doc_id).iter().any(|line| {
            line.line == 0
                && line.spans.iter().any(|span| {
                    span.token == crate::highlight::Token::Keyword && span.chars == (0..3)
                })
        }));
    }

    #[test]
    fn tab_overflow_buttons_and_ctrl_page_keys_cycle_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let id1 = app.workspace.active_view_id();
        app.key(ctrl('n'));
        let id2 = app.workspace.active_view_id();
        app.key(ctrl('n'));
        let id3 = app.workspace.active_view_id();
        assert_eq!(app.workspace.active_view_id(), id3);

        app.key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
        assert_eq!(app.workspace.active_view_id(), id2);
        app.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
        assert_eq!(app.workspace.active_view_id(), id3);
        app.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
        assert_eq!(app.workspace.active_view_id(), id1);

        let pane_id = app.workspace.active_pane_id();
        app.geometry.panes = vec![PaneGeometry {
            id: pane_id,
            area: Rect::new(28, 1, 52, 20),
            text_area: Rect::new(31, 2, 49, 19),
            tabs: Vec::new(),
            previous_tab: Some(Rect::new(28, 1, 1, 1)),
            next_tab: Some(Rect::new(79, 1, 1, 1)),
            ..PaneGeometry::default()
        }];
        app.hit_targets_valid = true;
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 28,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_view_id(), id3);

        app.hit_targets_valid = true;
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 79,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_view_id(), id1);
    }

    #[test]
    fn f5_refreshes_explorer_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let initial_count = app.workspace.explorer.as_ref().unwrap().rows().len();
        std::fs::write(dir.path().join("late.txt"), "new").unwrap();
        app.key(fkey(5));
        let rows = app.workspace.explorer.as_ref().unwrap().rows();
        assert_eq!(rows.len(), initial_count + 1);
        assert!(rows.iter().any(|e| e.name == "late.txt"));
    }

    #[test]
    fn f8_f9_f10_shift_f10_f12_and_alt_shift_arrow_split_navigate_resize_and_close() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.viewport = (120, 36);
        let p1 = app.workspace.active_pane_id();
        let v1 = app.workspace.active_view_id();
        app.key(ctrl('n'));
        let v2 = app.workspace.active_view_id();

        // F8 splits Columns: duplicates ONLY active tab (v2), v1 remains in p1
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();
        assert_ne!(p2, p1);
        assert_eq!(app.workspace.pane(p1).unwrap().tabs().len(), 2);
        assert_eq!(app.workspace.pane(p2).unwrap().tabs().len(), 1);
        assert_eq!(
            app.workspace
                .pane(p2)
                .unwrap()
                .active_tab()
                .unwrap()
                .document_id,
            app.workspace.pane(p1).unwrap().tab(v2).unwrap().document_id
        );

        // F9 splits p2 into Rows -> p3
        app.key(fkey(9));
        let p3 = app.workspace.active_pane_id();
        assert_ne!(p3, p2);
        assert_eq!(app.workspace.panes().len(), 3);

        // Draw frame to verify real divider geometry & resize via Alt+Shift+Arrows
        draw_frame(&mut app, 120, 36);
        let p2_before = app.geometry.pane(p2).unwrap().area;
        let p3_before = app.geometry.pane(p3).unwrap().area;
        assert_eq!(p2_before.x, p3_before.x);
        assert!(p3_before.y > p2_before.y);

        // Alt+Shift+Up moves nearest Rows divider up by 1 cell (p3 grows, p2 shrinks)
        app.key(KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        ));
        draw_frame(&mut app, 120, 36);
        assert_eq!(app.geometry.pane(p3).unwrap().area.y, p3_before.y - 1);
        assert_eq!(
            app.geometry.pane(p2).unwrap().area.height,
            p2_before.height - 1
        );

        // Alt+Shift+Left moves nearest Columns divider left by 2 cells
        let col_x_before = app.geometry.pane(p3).unwrap().area.x;
        app.key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        ));
        draw_frame(&mut app, 120, 36);
        assert_eq!(app.geometry.pane(p3).unwrap().area.x, col_x_before - 2);

        // F10 / Shift+F10 cycle panes in spatial order: p1 -> p2 -> p3
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p1);
        app.key(KeyEvent::new(KeyCode::F(10), KeyModifiers::SHIFT));
        assert_eq!(app.workspace.active_pane_id(), p3);

        // Alt+Left still cycles tabs WITHIN active pane (p1 has v1, v2)
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p1);
        assert_eq!(app.workspace.active_view_id(), v2);
        app.key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(app.workspace.active_view_id(), v1);
        assert_eq!(app.workspace.active_pane_id(), p1);

        // F12 closes current clean pane
        app.key(fkey(12));
        assert_eq!(app.workspace.panes().len(), 2);
        assert!(app.workspace.pane(p1).is_none());
    }

    #[test]
    fn shared_edit_undo_redo_and_replace_all_remap_all_views_independently() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.viewport = (120, 36);

        // Doc 1: multiline content
        let v1 = app.workspace.active_view_id();
        let doc1 = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("alpha foo\nbeta\ngamma foo\n");

        // Doc 2: independent history & buffer
        app.key(ctrl('n'));
        let v_doc2 = app.workspace.active_view_id();
        let doc2 = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("other doc");

        // Switch back to v1 in pane1, split into pane2 (v2 shares doc1)
        app.key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(app.workspace.active_view_id(), v1);
        let p1 = app.workspace.active_pane_id();
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();
        let v2 = app.workspace.active_view_id();

        // Set v2 independent viewport & cursor/selection on line 2 ("gamma foo\n")
        {
            let v2_tab = app.workspace.view_mut(v2).unwrap().1;
            v2_tab.top_line = 2;
            v2_tab.anchor = Some(15); // start of "gamma foo"
            v2_tab.cursor = 24; // end of "gamma foo"
        }

        // Switch back to p1 and edit at start of doc1
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p1);
        app.workspace.active_view_mut().unwrap().cursor = 0;
        app.workspace.active_view_mut().unwrap().anchor = None;
        app.replace_in_active_tab("HEADER\n");

        // v2 shifted by 7 chars and +1 line, keeping independent scroll (no forced scroll to 0)
        let v2_tab = app.workspace.view(v2).unwrap().1;
        assert_eq!(v2_tab.top_line, 3);
        assert_eq!(v2_tab.anchor, Some(22));
        assert_eq!(v2_tab.cursor, 31);

        // Undo from p1 restores v2
        app.key(ctrl('z'));
        let v2_tab = app.workspace.view(v2).unwrap().1;
        assert_eq!(v2_tab.top_line, 2);
        assert_eq!(v2_tab.anchor, Some(15));
        assert_eq!(v2_tab.cursor, 24);

        // Redo from p2 updates v1
        app.workspace.view_mut(v1).unwrap().1.cursor = 15;
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p2);
        app.key(ctrl('y'));
        assert_eq!(app.workspace.view(v1).unwrap().1.cursor, 22);
        // Redo placed its initiating view at HEADER's end. Move that independent
        // view after the last match before testing a foreign Replace All.
        let last_content = app.workspace.document(doc1).unwrap().document.len_chars() - 1;
        app.workspace.view_mut(v2).unwrap().1.cursor = last_content;

        // F7 ReplaceAll ("foo" -> "BARBAZ", +3 chars each) from p1 remaps v2
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p1);
        app.search.query = "foo".into();
        app.search.replacement = "BARBAZ".into();
        app.replace_all_in_active_tab();
        // Text: "HEADER\nalpha BARBAZ\nbeta\ngamma BARBAZ\n"
        // A foreign position at the replaced window's end shifts by both expansions.
        assert_eq!(app.workspace.view(v2).unwrap().1.cursor, last_content + 6);
        assert_eq!(
            app.workspace
                .document(doc1)
                .unwrap()
                .document
                .text()
                .to_string(),
            "HEADER\nalpha BARBAZ\nbeta\ngamma BARBAZ\n"
        );

        // Doc 2 remained completely unaffected
        assert_eq!(
            app.workspace
                .document(doc2)
                .unwrap()
                .document
                .text()
                .to_string(),
            "other doc"
        );
        assert_eq!(app.workspace.view(v_doc2).unwrap().1.cursor, 9);
    }

    #[test]
    fn save_as_updates_shared_views_and_rejects_only_other_documents() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::write(&file_a, "alpha").unwrap();
        std::fs::write(&file_b, "beta").unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(file_a.clone()).unwrap();
        app.open_tab_from_path(file_b.clone()).unwrap();
        let doc_b = app.workspace.active_document().unwrap().id;
        app.key(fkey(8)); // split doc_b into pane2
        let p2 = app.workspace.active_pane_id();
        app.replace_in_active_tab("modified ");

        // SaveAs to file_a (another doc) is rejected
        app.save_document_to(doc_b, file_a.clone(), false, true);
        assert!(app.workspace.document(doc_b).unwrap().document.is_dirty());

        // SaveAs to new path updates shared buffer & all views' label/clean state
        let dest = dir.path().join("renamed.txt");
        app.save_document_to(doc_b, dest.clone(), false, true);
        let buf = app.workspace.document(doc_b).unwrap();
        assert!(!buf.document.is_dirty());
        assert_eq!(buf.label(), "renamed.txt");
        assert_eq!(buf.path.as_deref(), Some(dest.as_path()));
        // Verify captions for both panes show updated label and clean state
        let mut caps = Vec::new();
        app.workspace
            .push_pane_captions(app.workspace.pane(p2).unwrap(), &mut caps);
        assert_eq!(caps[0].label, "renamed.txt");
        assert!(!caps[0].dirty);
    }

    #[test]
    fn close_shared_dirty_view_no_prompt_and_last_dirty_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let p1 = app.workspace.active_pane_id();
        let v1 = app.workspace.active_view_id();
        let doc_id = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("shared dirty");

        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();
        let v2 = app.workspace.active_view_id();
        assert_eq!(app.workspace.view_count_for_document(doc_id), 2);

        // Close v2 while v1 survives -> NO dirty prompt, pane2 collapses back to pane1
        app.key(ctrl('w'));
        assert!(app.modal.is_none());
        assert!(app.workspace.view(v2).is_none());
        assert!(app.workspace.pane(p2).is_none());
        assert_eq!(app.workspace.active_pane_id(), p1);
        assert_eq!(app.workspace.active_view_id(), v1);
        assert!(app.workspace.document(doc_id).unwrap().document.is_dirty());

        // Now close v1 (the LAST view of doc_id) -> prompts Unsaved!
        app.key(ctrl('w'));
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id: d, .. }) if d == doc_id
        ));
    }

    #[test]
    fn f12_close_pane_multi_doc_save_discard_cancel_and_last_pane_blank() {
        let dir = tempfile::tempdir().unwrap();
        let p_save = dir.path().join("save_me.txt");
        std::fs::write(&p_save, "orig").unwrap();

        let mut app = test_app(dir.path());
        // Pane 1 starts with shared dirty doc
        app.replace_in_active_tab("shared");
        app.key(fkey(8)); // Pane 2 gets shared view; Pane 1 still has it
        // Switch back to Pane 1 and add: clean doc, dirty doc A (untitled), dirty doc B (file-backed)
        app.key(fkey(10));
        let pane1 = app.workspace.active_pane_id();
        app.key(ctrl('n')); // clean blank
        app.key(ctrl('n'));
        let doc_a = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("discard A");
        app.open_tab_from_path(p_save.clone()).unwrap();
        let doc_b = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("new ");

        // F12 on Pane 1: skips shared dirty and clean, prompts doc_a first
        app.key(fkey(12));
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == doc_a
        ));

        // Discard doc_a -> moves to doc_b
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.workspace.document(doc_a).is_none());
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == doc_b
        ));

        // Cancel on doc_b -> pane1 stays open with doc_b still dirty; doc_a stayed discarded
        app.modal_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(app.modal.is_none());
        assert!(app.workspace.pane(pane1).is_some());
        assert!(app.workspace.document(doc_b).unwrap().document.is_dirty());

        // F12 again -> prompts doc_b -> Save -> completes closing Pane 1
        app.key(fkey(12));
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == doc_b
        ));
        app.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(std::fs::read_to_string(&p_save).unwrap(), "new orig");
        assert!(app.workspace.pane(pane1).is_none());
        assert!(app.workspace.document(doc_b).is_none());
        assert_eq!(app.workspace.panes().len(), 1);

        // F12 on the LAST remaining pane (which has dirty shared doc) -> prompts, Discard -> 1 blank, never quits
        let last_doc = app.workspace.active_document().unwrap().id;
        app.key(fkey(12));
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == last_doc
        ));
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(!app.quit);
        assert_eq!(app.workspace.panes().len(), 1);
        assert_eq!(app.workspace.active_pane().unwrap().tabs().len(), 1);
        assert!(!app.workspace.active_document().unwrap().document.is_dirty());
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .len_chars(),
            0
        );
    }

    #[test]
    fn quit_checks_shared_dirty_document_once_and_discards_all_views() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        let shared_doc = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("shared dirty");
        app.key(fkey(8)); // view 2 of shared_doc in pane 2
        assert_eq!(app.workspace.view_count_for_document(shared_doc), 2);

        app.key(ctrl('n'));
        let other_dirty = app.workspace.active_document().unwrap().id;
        app.replace_in_active_tab("other dirty");

        app.key(ctrl('q'));
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == shared_doc
        ));
        // Discard shared_doc -> removes BOTH views across panes and proceeds directly to other_dirty
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.workspace.document(shared_doc).is_none());
        assert_eq!(app.workspace.view_count_for_document(shared_doc), 0);
        assert!(matches!(
            app.modal,
            Some(Modal::Unsaved { doc_id, .. }) if doc_id == other_dirty
        ));

        // Cancel second prompt -> app stays alive with other_dirty intact and never falsely saved
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.quit);
        assert!(
            app.workspace
                .document(other_dirty)
                .unwrap()
                .document
                .is_dirty()
        );
    }

    #[test]
    fn small_screen_split_refusal_and_narrow_collapse_restore() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());

        // Refuses split on 20-col screen (12 + 1 + 12 = 25 > 20) without mutating tree
        app.viewport = (20, 20);
        app.key(fkey(8));
        assert_eq!(app.workspace.panes().len(), 1);
        assert_eq!(app.status.level, StatusLevel::Warning);

        // Refuses vertical split on 6-row screen (compact body 4 < 4 + 1 + 4)
        app.viewport = (80, 6);
        app.key(fkey(9));
        assert_eq!(app.workspace.panes().len(), 1);

        // Split on 80x24
        app.viewport = (80, 24);
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();
        app.key(fkey(10));
        let p1 = app.workspace.active_pane_id();
        assert_eq!(app.workspace.panes().len(), 2);

        // Shrink to 35x24: 35 < 25 + 1 + 18 so the explorer hides first while
        // the full 2-pane tree (25 cols) still fits!
        draw_frame(&mut app, 35, 24);
        assert!(app.geometry.sidebar.is_none());
        assert!(!app.geometry.collapsed);
        assert_eq!(app.geometry.panes.len(), 2);

        // Shrink to 16x24: full tree (21) doesn't fit -> temporarily renders ONLY active pane
        draw_frame(&mut app, 16, 24);
        assert!(app.geometry.collapsed);
        assert_eq!(app.geometry.panes.len(), 1);
        assert_eq!(app.geometry.panes[0].id, p1);
        // Stored tree & panes are NOT mutated or dropped
        assert_eq!(app.workspace.panes().len(), 2);

        // F10 focuses hidden pane p2 and reveals it
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p2);
        draw_frame(&mut app, 16, 24);
        assert!(app.geometry.collapsed);
        assert_eq!(app.geometry.panes.len(), 1);
        assert_eq!(app.geometry.panes[0].id, p2);

        // Restore 80x24 -> explorer and full split layout restore intact!
        draw_frame(&mut app, 80, 24);
        assert!(!app.geometry.collapsed);
        assert!(app.geometry.sidebar.is_some());
        assert_eq!(app.geometry.panes.len(), 2);
    }

    #[test]
    fn multi_pane_mouse_click_wheel_drag_split_divider_and_stale_hits() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.explorer_visible = false;
        let text = (0..30).map(|i| format!("line{i:02}\n")).collect::<String>();
        app.replace_in_active_tab(&text);
        app.workspace.active_view_mut().unwrap().cursor = 0;

        let p1 = app.workspace.active_pane_id();
        app.viewport = (80, 20);
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();

        draw_frame(&mut app, 80, 20);
        let p1_geom = app.geometry.pane(p1).unwrap().clone();
        let p2_geom = app.geometry.pane(p2).unwrap().clone();
        let split_div = app.geometry.splits[0];

        // Wheel over inactive pane p1 scrolls ONLY p1 without changing active pane p2
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: p1_geom.text_area.x,
                row: p1_geom.text_area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_pane_id(), p2);
        assert_eq!(
            app.workspace
                .pane(p1)
                .unwrap()
                .active_tab()
                .unwrap()
                .top_line,
            3
        );
        assert_eq!(
            app.workspace
                .pane(p2)
                .unwrap()
                .active_tab()
                .unwrap()
                .top_line,
            0
        );
        assert!(!app.hit_targets_valid);

        // Queued stale click on p1 text is rejected until next frame
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: p1_geom.text_area.x,
                row: p1_geom.text_area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_pane_id(), p2);

        // Wheel on header (row 0) and divider ignored
        draw_frame(&mut app, 80, 20);
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: split_div.area.x,
                row: split_div.area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 5,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(
            app.workspace
                .pane(p1)
                .unwrap()
                .active_tab()
                .unwrap()
                .top_line,
            3
        );

        // Drag starting in p1 and crossing into p2 clamps/updates ONLY origin pane p1
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: p1_geom.text_area.x,
                row: p1_geom.text_area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(app.workspace.active_pane_id(), p1);
        let p2_cursor_before = app.workspace.pane(p2).unwrap().active_tab().unwrap().cursor;
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: p2_geom.text_area.x + 5,
                row: p2_geom.text_area.y + 2,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: p2_geom.text_area.x + 5,
                row: p2_geom.text_area.y + 2,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(
            app.workspace
                .pane(p1)
                .unwrap()
                .active_tab()
                .unwrap()
                .selection()
                .is_some()
        );
        assert_eq!(
            app.workspace.pane(p2).unwrap().active_tab().unwrap().cursor,
            p2_cursor_before
        );

        // Text mutation invalidates queued mouse coordinates even without rectangle change
        draw_frame(&mut app, 80, 20);
        assert!(app.hit_targets_valid);
        app.replace_in_active_tab("X");
        assert!(!app.hit_targets_valid);

        // Split divider mouse drag resizes tree ratio
        draw_frame(&mut app, 80, 20);
        let div = app.geometry.splits[0];
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: div.area.x,
                row: div.area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: div.area.x + 6,
                row: div.area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: div.area.x + 6,
                row: div.area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        draw_frame(&mut app, 80, 20);
        assert_eq!(app.geometry.splits[0].area.x, div.area.x + 6);
    }

    #[test]
    fn reload_resets_all_shared_views_and_forgets_highlighter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.rs");
        std::fs::write(&path, "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(path.clone()).unwrap();
        let doc_id = app.workspace.active_document().unwrap().id;
        let v1 = app.workspace.active_view_id();
        app.key(fkey(8));
        let v2 = app.workspace.active_view_id();

        app.workspace.view_mut(v1).unwrap().1.cursor = 5;
        app.workspace.view_mut(v1).unwrap().1.top_line = 1;
        app.workspace.view_mut(v2).unwrap().1.cursor = 12;
        app.workspace.view_mut(v2).unwrap().1.anchor = Some(10);
        app.workspace.view_mut(v2).unwrap().1.top_line = 2;
        app.workspace.view_mut(v2).unwrap().1.left_cell = 3;

        app.replace_in_active_tab("// local\n");
        std::fs::write(&path, "let z = 9;\n").unwrap();
        app.save_active_document();
        assert!(matches!(app.modal, Some(Modal::Conflict { .. })));
        app.modal_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        for view_id in [v1, v2] {
            let tab = app.workspace.view(view_id).unwrap().1;
            assert_eq!(tab.cursor, 0);
            assert_eq!(tab.anchor, None);
            assert_eq!(tab.top_line, 0);
            assert_eq!(tab.left_cell, 0);
        }
        assert_eq!(
            app.workspace
                .document(doc_id)
                .unwrap()
                .document
                .text()
                .to_string(),
            "let z = 9;\n"
        );
    }

    #[test]
    fn stale_replace_current_rejected_on_pane_view_or_generation_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("foo bar foo");
        app.workspace.active_view_mut().unwrap().cursor = 0;

        let p1 = app.workspace.active_pane_id();
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();
        let v2 = app.workspace.active_view_id();

        // Find in p1
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p1);
        app.search.query = "foo".into();
        app.search.replacement = "baz".into();
        app.find_in_active_tab(false);
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..3));

        // Switch to p2 (same document, same revision, even identical selection 0..3!)
        app.key(fkey(10));
        assert_eq!(app.workspace.active_pane_id(), p2);
        app.workspace.view_mut(v2).unwrap().1.anchor = Some(0);
        app.workspace.view_mut(v2).unwrap().1.cursor = 3;

        // Pressing F6 in p2 must reject stale match from p1!
        app.key(ctrl('r'));
        app.key(fkey(6));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "foo bar foo"
        );
        assert_eq!(app.status.level, StatusLevel::Info);
    }

    #[test]
    fn per_document_union_highlighting_for_two_panes_and_hidden_pane_exclusion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.rs");
        let mut text = String::from("fn top() {}\n");
        for i in 1..250 {
            text.push_str(&format!("let v{i} = {i};\n"));
        }
        std::fs::write(&path, &text).unwrap();

        let mut app = test_app(dir.path());
        app.workspace.explorer_visible = false;
        app.open_tab_from_path(path).unwrap();
        let doc_id = app.workspace.active_document().unwrap().id;
        let p1 = app.workspace.active_pane_id();
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();

        app.workspace
            .pane_mut(p2)
            .unwrap()
            .active_tab_mut()
            .unwrap()
            .top_line = 220;
        draw_frame(&mut app, 80, 12);
        // Feed ticks until highlight finishes
        for _ in 0..300 {
            if !app.highlight_pending {
                break;
            }
            app.update_highlight();
        }
        assert!(!app.highlight_pending);
        let lines = app.highlighter.lines(doc_id);
        assert!(lines.iter().any(|l| l.line == 0));
        assert!(lines.iter().any(|l| l.line == 220));
        // Middle gap rows are not stored
        assert!(!lines.iter().any(|l| l.line == 100));

        // Collapse to active pane p2 on narrow terminal (width 16) -> only p2 range remains visible
        app.workspace
            .pane_mut(p1)
            .unwrap()
            .active_tab_mut()
            .unwrap()
            .top_line = 0;
        draw_frame(&mut app, 16, 12);
        assert!(app.geometry.collapsed);
        app.update_highlight();
        let lines_collapsed = app.highlighter.lines(doc_id);
        assert!(lines_collapsed.iter().any(|l| l.line == 220));
        assert!(!lines_collapsed.iter().any(|l| l.line == 0));
    }

    #[test]
    fn path_control_rejection_preserves_selection_and_respects_net_cap() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.open_path_dialog();

        for ch in "/path/to/my_file.txt".chars() {
            app.modal_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        let (field_val, sel) = match &app.modal {
            Some(Modal::Path { value, .. }) => (value.as_str().to_string(), value.selection()),
            _ => panic!("Expected Modal::Path"),
        };
        assert_eq!(field_val, "/path/to/my_file.txt");
        assert_eq!(sel, None);

        // Select "my_file" (range 9..16)
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(9, false);
            value.move_to(16, true);
        }
        assert_eq!(
            match &app.modal {
                Some(Modal::Path { value, .. }) => value.selected(),
                _ => None,
            },
            Some("my_file")
        );

        // Input control char '\n' -> rejected before mutation, preserving selection
        app.modal_key(KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::NONE));
        assert_eq!(app.status.level, StatusLevel::Error);
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.as_str(), "/path/to/my_file.txt");
            assert_eq!(value.selected(), Some("my_file"));
            assert_eq!(value.selection(), Some(9..16));
        } else {
            panic!("Expected Modal::Path");
        }

        // Net cap: replace selected "my_file" (7 bytes) with "new_name" (8 bytes)
        for ch in "new_name".chars() {
            app.modal_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.as_str(), "/path/to/new_name.txt");
            assert_eq!(value.selection(), None);
        }

        // Atomic rejection when net limit is exceeded preserves selection
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(9, false);
            value.move_to(17, true);
        }
        let huge_paste = "a".repeat(4100);
        app.path_input(&huge_paste);
        assert_eq!(app.status.level, StatusLevel::Error);
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.as_str(), "/path/to/new_name.txt");
            assert_eq!(value.selected(), Some("new_name"));
        }
    }

    #[test]
    fn field_clipboard_isolated_from_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("DOCUMENT ORIGINAL TEXT");
        app.workspace.active_view_mut().unwrap().cursor = 0;
        app.workspace.active_view_mut().unwrap().anchor = Some(8);

        app.open_path_dialog();
        for ch in "dialog_path_text".chars() {
            app.modal_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(0, false);
            value.move_to(6, true);
        }

        // Ctrl+C in field copies field selection, not document, and pastes back into field
        app.modal_key(ctrl('c'));
        assert_eq!(app.status.level, StatusLevel::Success);
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "DOCUMENT ORIGINAL TEXT"
        );
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(value.as_str().len(), false);
        }
        app.modal_key(ctrl('v'));
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            assert_eq!(value.as_str(), "dialog_path_textdialog");
            value.clear();
            value.insert("dialog_path_text", MAX_PATH_BYTES);
            value.move_to(0, false);
        }

        // Empty copy (no selection) in field does not touch document or clipboard
        app.modal_key(ctrl('c'));
        assert_eq!(app.status.level, StatusLevel::Info);
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(value.as_str().len(), false);
        }
        app.modal_key(ctrl('v'));
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            assert_eq!(value.as_str(), "dialog_path_textdialog");
            value.clear();
            value.insert("dialog_path_text", MAX_PATH_BYTES);
            value.move_to(6, false);
            value.move_to(12, true);
        }

        // Ctrl+X cuts from field, document unchanged, paste confirms cut text
        app.modal_key(ctrl('x'));
        assert_eq!(app.status.level, StatusLevel::Success);
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            assert_eq!(value.as_str(), "dialogtext");
            value.move_to(value.as_str().len(), false);
        }
        app.modal_key(ctrl('v'));
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.as_str(), "dialogtext_path_");
        }
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "DOCUMENT ORIGINAL TEXT"
        );

        // Ctrl+Z / Ctrl+Y while modal open does not reach document
        app.modal_key(ctrl('z'));
        app.modal_key(ctrl('y'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "DOCUMENT ORIGINAL TEXT"
        );
    }

    #[test]
    fn f1_help_suspension_restores_dirty_save_as_continuation_and_field_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("dirty file content");

        app.request_quit();
        assert!(matches!(app.modal, Some(Modal::Unsaved { .. })));

        app.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(matches!(
            app.modal,
            Some(Modal::Path {
                action: PathAction::Save { .. },
                ..
            })
        ));
        assert!(matches!(app.after_save, Some(AfterSave::Quit)));

        for ch in "my_save.txt".chars() {
            app.modal_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(3, false);
            value.move_to(7, true);
        }

        // F1 opens Help from within Save As modal
        app.modal_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(matches!(app.modal, Some(Modal::Help { scroll: 0 })));

        // Help cannot execute editor commands
        app.modal_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.modal_key(ctrl('w'));
        app.modal_key(ctrl('s'));
        assert!(!app.quit);
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "dirty file content"
        );

        // Esc closes help and restores Save As modal with continuation and field state
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(
            app.modal,
            Some(Modal::Path {
                action: PathAction::Save { .. },
                ..
            })
        ));
        assert!(matches!(app.after_save, Some(AfterSave::Quit)));
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.as_str(), "my_save.txt");
            assert_eq!(value.cursor(), 7);
            assert_eq!(value.selection(), Some(3..7));
        }

        // Completing Save As succeeds and completes the pending Quit continuation
        let dest = dir.path().join("my_save.txt");
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.clear();
            value.insert(dest.to_str().unwrap(), MAX_PATH_BYTES);
        }
        app.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.quit);
        assert_eq!(std::fs::read_to_string(dest).unwrap(), "dirty file content");
    }

    #[test]
    fn field_mouse_click_drag_and_stale_hit_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.open_path_dialog();

        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.insert("/abc/def", MAX_PATH_BYTES);
        }

        draw_frame(&mut app, 80, 24);
        let hit = app
            .geometry
            .fields
            .iter()
            .find(|hit| hit.kind == FieldKind::Path)
            .unwrap();
        let column_at = |byte| {
            hit.stops
                .iter()
                .find(|&&(_, index)| index == byte)
                .unwrap()
                .0
        };
        let (start_column, end_column, row) = (column_at(5), column_at(8), hit.area.y);

        // Click before the final path component.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: start_column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.cursor(), 5);
            assert_eq!(value.selection(), None);
        }
        draw_frame(&mut app, 80, 24);

        // Select the final component through its end after a fresh frame.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: end_column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.cursor(), 8);
            assert_eq!(value.selection(), Some(5..8));
            assert_eq!(value.selected(), Some("def"));
        }

        // MouseUp completes drag
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: end_column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(app.drag.is_none());

        // Invalidate hit targets
        app.invalidate_hit_targets();
        assert!(!app.hit_targets_valid);

        // Click when hit targets are invalid is rejected
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: start_column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.cursor(), 8);
        }
    }

    #[test]
    fn search_query_cursor_movement_preserves_last_match_but_text_edit_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("needle other needle");
        app.workspace.active_view_mut().unwrap().cursor = 0;

        app.key(ctrl('f'));
        for ch in "needle".chars() {
            app.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.search.last_match.is_some());
        let match_range = app.search.last_match.as_ref().unwrap().range.clone();
        assert_eq!(match_range, 0..6);

        // Moving cursor in query field with Left arrow preserves last_match
        app.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(app.search.last_match.is_some());
        assert_eq!(app.search.query.cursor(), 5);

        // Home key in query field preserves last_match
        app.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert!(app.search.last_match.is_some());
        assert_eq!(app.search.query.cursor(), 0);

        // Shift+Right selection in query field preserves last_match
        app.key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        assert!(app.search.last_match.is_some());
        assert_eq!(app.search.query.selection(), Some(0..1));

        // Editing query text (typing a char) invalidates last_match
        app.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(app.search.last_match.is_none());
    }

    #[test]
    fn help_scroll_keys_clamp_and_wheel_does_not_change_panes() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.viewport = (80, 24);
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();

        app.key(fkey(1));
        assert!(matches!(app.modal, Some(Modal::Help { scroll: 0 })));

        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(matches!(app.modal, Some(Modal::Help { scroll: 1 })));
        draw_frame(&mut app, 80, 24);

        app.mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 10,
                row: 10,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Help { scroll: 4 })));
        assert_eq!(app.workspace.active_pane_id(), p2);

        app.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert!(matches!(app.modal, Some(Modal::Help { scroll: 0 })));

        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.modal.is_none());
        assert_eq!(app.workspace.active_pane_id(), p2);
    }

    #[test]
    fn field_selection_left_right_collapses_to_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.open_path_dialog();

        for ch in "hello_world".chars() {
            app.modal_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        // Select "lo_wo" (range 3..8)
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(3, false);
            value.move_to(8, true);
        }

        // Plain Left arrow collapses to range.start (3) without selection
        app.modal_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.cursor(), 3);
            assert_eq!(value.selection(), None);
        }

        // Select 3..8 again
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.move_to(3, false);
            value.move_to(8, true);
        }

        // Plain Right arrow collapses to range.end (8) without selection
        app.modal_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        if let Some(Modal::Path { value, .. }) = &app.modal {
            assert_eq!(value.cursor(), 8);
            assert_eq!(value.selection(), None);
        }
    }

    #[test]
    fn help_scroll_responds_immediately_after_growing_the_terminal() {
        let dir = tempfile::tempdir().unwrap();
        for key in [Some(KeyCode::Up), Some(KeyCode::PageUp), None] {
            let mut app = test_app(dir.path());
            app.viewport = (20, 6);
            app.key(fkey(1));
            app.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
            let old_limit = crate::help::scroll_limit(20, 6);
            let limit = crate::help::scroll_limit(100, 24);
            assert!(old_limit > limit);
            draw_frame(&mut app, 100, 24);
            let step = match key {
                Some(KeyCode::PageUp) => crate::help::page_size(100, 24),
                Some(_) => 1,
                None => 3,
            };
            if let Some(code) = key {
                app.key(KeyEvent::new(code, KeyModifiers::NONE));
            } else {
                app.mouse(
                    MouseEvent {
                        kind: MouseEventKind::ScrollUp,
                        column: 10,
                        row: 10,
                        modifiers: KeyModifiers::NONE,
                    },
                    Instant::now(),
                );
            }
            let Some(Modal::Help { scroll }) = app.modal else {
                panic!("help closed while scrolling");
            };
            assert_eq!(scroll, limit.saturating_sub(step));
        }
    }

    #[test]
    fn search_limit_counts_normalized_bytes_and_rejects_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.viewport = (80, 24);
        app.open_search_modal(false);
        app.search.query = "old query".into();
        app.key(ctrl('a'));
        app.search_input(&"\r\n".repeat(MAX_SEARCH_BYTES), false);
        assert_eq!(app.search.query.as_str(), "\n".repeat(MAX_SEARCH_BYTES));
        app.key(ctrl('a'));
        app.search_input(&format!("{}\r", "\r\n".repeat(MAX_SEARCH_BYTES)), false);
        assert_eq!(
            app.search.query.selected(),
            Some("\n".repeat(MAX_SEARCH_BYTES).as_str())
        );
        app.search_input("needle", false);
        assert_eq!(app.search.query.as_str(), "needle");
    }

    #[test]
    fn wheel_scroll_and_find_jump_preserve_stale_mouse_guard_and_converge() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.md");
        let mut md = String::from("# Start\n");
        for i in 1..300 {
            if i == 250 {
                md.push_str("NEEDLE_TARGET\n");
            } else {
                md.push_str(&format!("Paragraph line {i} with **emphasis**\n"));
            }
        }
        std::fs::write(&path, &md).unwrap();

        let mut app = test_app(dir.path());
        app.open_tab_from_path(path).unwrap();
        let doc_id = app.workspace.active_document().unwrap().id;
        let mut skip_pre_render = false;
        let mut prev_visible = Vec::new();

        app.render_step(&mut skip_pre_render, &mut prev_visible, |app| {
            draw_frame(app, 80, 20);
            Ok::<_, ()>(())
        })
        .unwrap();
        for _ in 0..350 {
            if !app.highlight_pending {
                break;
            }
            app.update_highlight();
        }
        assert!(!app.highlight_pending);
        skip_pre_render = false;

        let pane_area = app.active_text_area();
        // Scroll down then back up with mouse wheel; hit targets invalidate until next draw.
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: pane_area.x,
                row: pane_area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(!app.hit_targets_valid);
        assert!(app.highlight_pending);
        app.render_step(&mut skip_pre_render, &mut prev_visible, |app| {
            draw_frame(app, 80, 20);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert!(app.hit_targets_valid);
        for _ in 0..10 {
            if !app.highlight_pending {
                break;
            }
            app.update_highlight();
        }
        assert!(!app.highlight_pending);
        skip_pre_render = false;

        app.mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: pane_area.x,
                row: pane_area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(!app.hit_targets_valid);
        assert!(app.highlight_pending);
        app.render_step(&mut skip_pre_render, &mut prev_visible, |app| {
            draw_frame(app, 80, 20);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert!(app.hit_targets_valid);

        // Jump to line 250 via Search (F3) and drain incremental highlight ticks.
        app.search.query = "NEEDLE_TARGET".into();
        app.find_in_active_tab(false);
        assert!(!app.hit_targets_valid);
        assert!(app.highlight_pending);

        // Stale click before frame render is rejected.
        let cursor_after_find = app.workspace.active_view().unwrap().cursor;
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: pane_area.x,
                row: pane_area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(
            app.workspace.active_view().unwrap().cursor,
            cursor_after_find
        );

        app.render_step(&mut skip_pre_render, &mut prev_visible, |app| {
            draw_frame(app, 80, 20);
            Ok::<_, ()>(())
        })
        .unwrap();
        for _ in 0..350 {
            if !app.highlight_pending {
                break;
            }
            app.update_highlight();
        }
        assert!(!app.highlight_pending);
        assert!(app.highlighter.lines(doc_id).iter().any(|l| l.line == 250));
    }

    #[test]
    fn failed_system_cut_preserves_editor_source_selection_dirty_state_and_modal_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("alpha beta");
        let before_rev = app.workspace.active_document().unwrap().document.revision();
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
        {
            let view = app.workspace.active_view_mut().unwrap();
            view.anchor = Some(6);
            view.cursor = 10;
        }

        app.clipboard = Clipboard::test_write_failure();
        app.key(ctrl('x'));
        let doc = &app.workspace.active_document().unwrap().document;
        assert_eq!(doc.text().to_string(), "alpha beta");
        assert!(doc.is_dirty());
        assert_eq!(doc.revision(), before_rev);
        assert_eq!(
            app.workspace.active_view().unwrap().selection(),
            Some(6..10)
        );

        // Path modal failed cut preserves text and selection
        app.open_path_dialog();
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.insert("foo_bar", MAX_PATH_BYTES);
            value.move_to(0, false);
            value.move_to(3, true);
        }
        app.modal_key(ctrl('x'));
        let Some(Modal::Path { value, .. }) = &app.modal else {
            panic!("failed cut must keep the path dialog open");
        };
        assert_eq!(value.as_str(), "foo_bar");
        assert_eq!(value.selection(), Some(0..3));
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        // Replace modal failed cut preserves query/replacement text and selections
        app.key(ctrl('r'));
        app.search.query.insert("find", MAX_SEARCH_BYTES);
        app.search.query.move_to(0, false);
        app.search.query.move_to(2, true);
        app.modal_key(ctrl('x'));
        assert_eq!(app.search.query.as_str(), "find");
        assert_eq!(app.search.query.selection(), Some(0..2));

        app.modal_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.search.replacement.insert("repl", MAX_SEARCH_BYTES);
        app.search.replacement.move_to(0, false);
        app.search.replacement.move_to(2, true);
        app.modal_key(ctrl('x'));
        assert_eq!(app.search.replacement.as_str(), "repl");
        assert_eq!(app.search.replacement.selection(), Some(0..2));
    }

    #[test]
    fn failed_or_empty_external_paste_never_inserts_stale_internal_or_deletes_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("keep");
        {
            let view = app.workspace.active_view_mut().unwrap();
            view.anchor = Some(0);
            view.cursor = 4;
        }

        // Copy retains "keep" internally even when external write fails;
        // subsequent failed external read must not insert that internal text.
        app.clipboard = Clipboard::test_write_failure();
        app.key(ctrl('c'));
        app.key(ctrl('v'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep"
        );
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..4));

        // Invalid UTF-8 external paste is rejected atomically
        app.clipboard = Clipboard::test_read_bytes(vec![0xff, 0xfe]);
        app.key(ctrl('v'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep"
        );
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..4));

        // Oversized external paste is rejected atomically
        app.clipboard = Clipboard::test_read_bytes(vec![b'a'; MAX_CLIPBOARD_BYTES + 1]);
        app.key(ctrl('v'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep"
        );
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..4));

        // Empty external clipboard is an honest no-op preserving selection
        app.clipboard = Clipboard::test_read_bytes(Vec::new());
        app.key(ctrl('v'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep"
        );
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..4));

        // Same empty no-op preservation inside modal field
        app.open_path_dialog();
        if let Some(Modal::Path { value, .. }) = &mut app.modal {
            value.insert("abc", MAX_PATH_BYTES);
            value.move_to(0, false);
            value.move_to(3, true);
        }
        app.modal_key(ctrl('v'));
        let Some(Modal::Path { value, .. }) = &app.modal else {
            panic!("empty paste must keep the path dialog open");
        };
        assert_eq!(value.as_str(), "abc");
        assert_eq!(value.selection(), Some(0..3));
    }

    #[test]
    fn readonly_cut_is_blocked_while_copy_remains_usable() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("locked text");
        let doc_id = app.workspace.active_document().unwrap().id;
        app.workspace.document_mut(doc_id).unwrap().read_only = Some("locked".into());
        {
            let view = app.workspace.active_view_mut().unwrap();
            view.anchor = Some(0);
            view.cursor = 6;
        }

        app.key(ctrl('x'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "locked text"
        );

        app.key(ctrl('c'));
        app.open_path_dialog();
        app.modal_key(ctrl('v'));
        let Some(Modal::Path { value, .. }) = &app.modal else {
            panic!("copy from a read-only document must remain pasteable");
        };
        assert_eq!(value.as_str(), "locked");
    }

    #[test]
    fn search_and_replace_fields_support_clipboard_with_normalization_and_size_guard() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.key(ctrl('r'));

        app.clipboard = Clipboard::test_read_bytes(b"a\r\nb".to_vec());
        app.modal_key(ctrl('v'));
        assert_eq!(app.search.query.as_str(), "a\nb");

        // Atomic rejection over 4096 bytes preserves existing query selection
        app.search.query.move_to(0, false);
        app.search.query.move_to(3, true);
        app.clipboard = Clipboard::test_read_bytes(vec![b'x'; MAX_SEARCH_BYTES + 1]);
        app.modal_key(ctrl('v'));
        assert_eq!(app.search.query.as_str(), "a\nb");
        assert_eq!(app.search.query.selection(), Some(0..3));
    }

    #[test]
    fn agy_p0_failed_open_preserves_path_caret_and_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("keep this buffer");
        app.open_path_dialog();
        app.path_input(dir.path().join("missing.txt").to_str().unwrap());
        let expected = match &mut app.modal {
            Some(Modal::Path { value, .. }) => {
                value.move_to(1, false);
                value.move_to(4, true);
                value.clone()
            }
            _ => panic!("expected path editor"),
        };
        app.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let Some(Modal::Path {
            action: PathAction::Open,
            value,
        }) = &app.modal
        else {
            panic!("failed Open must retain the editable path");
        };
        assert_eq!(value, &expected);
        assert_eq!(app.status.level, StatusLevel::Error);
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "keep this buffer"
        );
    }

    #[test]
    fn agy_p0_failed_save_as_keeps_pending_quit_until_retry() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("keep me");
        app.key(ctrl('q'));
        app.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let parent = dir.path().join("missing");
        let target = parent.join("note.txt");
        app.path_input(target.to_str().unwrap());
        let expected = match &mut app.modal {
            Some(Modal::Path { value, .. }) => {
                value.move_to(1, false);
                value.move_to(4, true);
                value.clone()
            }
            _ => panic!("expected Save As"),
        };
        app.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.quit);
        assert!(matches!(app.after_save, Some(AfterSave::Quit)));
        let Some(Modal::Path { value, .. }) = &app.modal else {
            panic!("failed Save As must remain editable");
        };
        assert_eq!(value, &expected);
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
        std::fs::create_dir(parent).unwrap();
        app.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep me");
        assert!(app.quit);
        assert!(app.path_retry.is_none());
    }

    #[test]
    fn agy_p0_failed_overwrite_restores_path_and_cancel_clears_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("destination");
        std::fs::create_dir(&parent).unwrap();
        let target = parent.join("note.txt");
        std::fs::write(&target, "old").unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("new");
        app.key(ctrl('q'));
        app.modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.path_input(target.to_str().unwrap());
        app.modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(app.modal, Some(Modal::Overwrite { .. })));
        std::fs::remove_file(&target).unwrap();
        std::fs::remove_dir(parent).unwrap();
        app.modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        let Some(Modal::Path { value, .. }) = &app.modal else {
            panic!("overwrite failure must restore Save As");
        };
        assert_eq!(value.as_str(), target.to_str().unwrap());
        assert!(matches!(app.after_save, Some(AfterSave::Quit)));
        app.modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.after_save.is_none() && app.path_retry.is_none());
        assert!(!app.quit);
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
    }

    #[test]
    fn agy_p0_small_choices_block_hidden_discard() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("keep me");
        app.key(ctrl('q'));
        draw_frame(&mut app, 20, 6);
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(!app.quit);
        assert!(matches!(app.modal, Some(Modal::Unsaved { .. })));
        assert!(app.geometry.commands.is_empty());
        assert!(app.workspace.active_document().unwrap().document.is_dirty());
        draw_frame(&mut app, 120, 36);
        app.modal_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.quit);
    }

    #[test]
    fn agy_p0_highlight_warning_does_not_replace_saved_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.rs");
        std::fs::write(&path, "x".repeat(16 * 1024 + 1)).unwrap();
        let mut app = test_app(dir.path());
        app.open_tab_from_path(path).unwrap();
        app.save_active_document();
        let before = (app.status.level, app.status.text.clone());
        app.update_highlight();
        assert_eq!((app.status.level, &app.status.text), (before.0, &before.1));
        let doc_id = app.workspace.active_document().unwrap().id;
        assert!(app.highlighter.warning(doc_id).is_some());
        app.render_with(|screen, _| {
            assert_eq!(screen.status_level, StatusLevel::Success);
            assert!(screen.highlight_warning.is_some());
        });
        app.update_highlight();
        assert_eq!((app.status.level, &app.status.text), (before.0, &before.1));
    }

    #[test]
    fn agy_p0_horizontal_wheel_stops_at_visible_content() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.explorer_visible = false;
        app.replace_in_active_tab(&"x".repeat(100));
        draw_frame(&mut app, 50, 12);
        let area = app.active_text_area();
        let mouse = MouseEvent {
            kind: MouseEventKind::ScrollRight,
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        };
        for _ in 0..30 {
            app.mouse(mouse, Instant::now());
            draw_frame(&mut app, 50, 12);
        }
        assert_eq!(
            app.workspace.active_view().unwrap().left_cell,
            101usize.saturating_sub(usize::from(area.width))
        );
        app.key(ctrl('a'));
        app.replace_in_active_tab("short");
        draw_frame(&mut app, 50, 12);
        app.mouse(mouse, Instant::now());
        assert_eq!(app.workspace.active_view().unwrap().left_cell, 0);
    }

    #[test]
    fn p1_confirmation_keyboard_defaults_cancel_and_preserves_focus_through_help() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("unsaved text");
        let doc_id = app.workspace.active_document().unwrap().id;
        app.request_close_active_view();
        draw_frame(&mut app, 80, 24);
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.modal.is_none());
        assert!(app.workspace.document(doc_id).unwrap().document.is_dirty());

        app.request_close_active_view();
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(app.modal, Some(Modal::Help { .. })));
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            app.modal.as_ref().and_then(Modal::choice),
            Some((ChoiceKind::Unsaved, 3))
        );
        app.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        app.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        app.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.workspace.document(doc_id).is_none());
        assert!(app.modal.is_none());
        assert!(!app.workspace.active_document().unwrap().document.is_dirty());
    }

    #[test]
    fn p1_explorer_keyboard_resize_uses_visible_width_and_refuses_hidden_sidebar() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.sidebar_width = 40;
        app.workspace.explorer_focused = true;
        draw_frame(&mut app, 50, 12);
        let visible = app.geometry.sidebar_panel.unwrap().width;
        assert!(visible < app.workspace.sidebar_width);
        app.key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        ));
        draw_frame(&mut app, 50, 12);
        assert_eq!(app.geometry.sidebar_panel.unwrap().width, visible - 2);
        let saved_width = app.workspace.sidebar_width;
        draw_frame(&mut app, 30, 12);
        assert!(app.geometry.sidebar_panel.is_none());
        app.key(KeyEvent::new(
            KeyCode::Right,
            KeyModifiers::ALT | KeyModifiers::SHIFT,
        ));
        assert_eq!(app.workspace.sidebar_width, saved_width);
        assert!(app.workspace.explorer_focused);
        assert!(!app.workspace.active_document().unwrap().document.is_dirty());
    }

    #[test]
    fn p1_search_prefill_count_wrap_replace_and_undo_follow_document_revision() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("é é é");
        {
            let tab = app.workspace.active_view_mut().unwrap();
            tab.anchor = Some(0);
            tab.cursor = 1;
        }
        app.open_search_modal(true);
        assert_eq!(app.search.query.as_str(), "é");
        app.find_in_active_tab(false);
        assert_eq!(app.search.last_match.as_ref().unwrap().range, 2..3);
        assert_eq!(app.search.feedback, "Match 2 of 3");
        app.find_in_active_tab(false);
        app.find_in_active_tab(false);
        assert_eq!(app.search.last_match.as_ref().unwrap().range, 0..1);
        assert_eq!(app.search.feedback, "Match 1 of 3 (wrapped)");

        app.search.replacement = Field::new("x".into());
        app.replace_current_in_active_tab();
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "x é é"
        );
        assert_eq!(
            app.search.current_feedback(&app.workspace),
            Some("Match 1 of 2")
        );
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.key(ctrl('z'));
        assert!(app.search.current_feedback(&app.workspace).is_none());
        app.find_in_active_tab(false);
        assert_eq!(
            app.search.cache.as_ref().unwrap().index.starts,
            vec![0, 2, 4]
        );
    }

    #[test]
    fn p1_search_prefill_never_truncates_multiline_or_oversized_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        for text in ["one\ntwo".to_owned(), "é".repeat(2049)] {
            app.search.query = Field::new("keep".into());
            app.replace_in_active_tab(&text);
            let len = app
                .workspace
                .active_document()
                .unwrap()
                .document
                .len_chars();
            let tab = app.workspace.active_view_mut().unwrap();
            tab.anchor = Some(0);
            tab.cursor = len;
            app.open_search_modal(false);
            assert_eq!(app.search.query.as_str(), "keep");
            app.cancel_modal();
        }
    }

    #[test]
    fn p1_find_visits_all_counted_overlapping_matches_in_both_directions() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.replace_in_active_tab("aaaa");
        app.workspace.active_view_mut().unwrap().cursor = 0;
        app.search.query = Field::new("aa".into());
        for range in [0..2, 1..3, 2..4, 0..2] {
            app.find_in_active_tab(false);
            assert_eq!(app.search.last_match.as_ref().unwrap().range, range);
        }
        assert_eq!(app.search.feedback, "Match 1 of 3 (wrapped)");
        app.find_in_active_tab(true);
        assert_eq!(app.search.last_match.as_ref().unwrap().range, 2..4);
        assert_eq!(app.search.feedback, "Match 3 of 3 (wrapped)");
        app.find_in_active_tab(true);
        assert_eq!(app.search.last_match.as_ref().unwrap().range, 1..3);
    }

    #[test]
    fn p1_reload_invalidates_counts_even_when_document_revision_is_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reload.txt");
        std::fs::write(&path, "nothing").unwrap();
        let mut app = test_app(dir.path());
        app.open_tab_from_path(path.clone()).unwrap();
        let doc_id = app.workspace.active_document().unwrap().id;
        app.search.query = Field::new("z".into());
        app.find_in_active_tab(false);
        let revision = app.workspace.active_document().unwrap().document.revision();
        assert_eq!(
            app.search.current_feedback(&app.workspace),
            Some("No matches")
        );

        std::fs::write(&path, "z z").unwrap();
        app.reload_document(doc_id, path);
        assert_eq!(
            app.workspace.active_document().unwrap().document.revision(),
            revision
        );
        assert!(app.search.current_feedback(&app.workspace).is_none());
        app.find_in_active_tab(false);
        assert_eq!(app.search.cache.as_ref().unwrap().index.starts, vec![0, 2]);
        assert_eq!(app.search.feedback, "Match 1 of 2");
    }

    #[test]
    fn p1_open_search_rejects_mouse_commands_from_the_previous_screen() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        draw_frame(&mut app, 80, 24);
        let old_help = app
            .geometry
            .commands
            .iter()
            .find(|hit| hit.key.code == KeyCode::F(1))
            .unwrap()
            .area;
        app.open_search_modal(false);
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: old_help.x,
                row: old_help.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Search { .. })));
    }

    #[test]
    fn block_indent_preserves_selection_direction_excludes_end_line_and_groups_undo() {
        for reverse in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut app = test_app(dir.path());
            app.workspace.explorer_focused = false;
            app.replace_in_active_tab("α\nb\nlast");
            app.workspace
                .document_mut(app.workspace.active_document().unwrap().id)
                .unwrap()
                .document
                .mark_saved();
            {
                let view = app.workspace.active_view_mut().unwrap();
                view.cursor = if reverse { 0 } else { 4 };
                view.anchor = Some(if reverse { 4 } else { 0 });
            }
            app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(
                app.workspace
                    .active_document()
                    .unwrap()
                    .document
                    .text()
                    .to_string(),
                "    α\n    b\nlast"
            );
            let view = app.workspace.active_view().unwrap();
            assert_eq!(view.cursor, if reverse { 4 } else { 12 });
            assert_eq!(view.anchor, Some(if reverse { 12 } else { 4 }));
            assert_eq!(view.selection(), Some(4..12));
            app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(
                app.workspace
                    .active_document()
                    .unwrap()
                    .document
                    .text()
                    .to_string(),
                "        α\n        b\nlast"
            );
            app.key(ctrl('z'));
            assert_eq!(
                app.workspace
                    .active_document()
                    .unwrap()
                    .document
                    .text()
                    .to_string(),
                "    α\n    b\nlast"
            );
            app.key(ctrl('z'));
            assert_eq!(
                app.workspace
                    .active_document()
                    .unwrap()
                    .document
                    .text()
                    .to_string(),
                "α\nb\nlast"
            );
            assert!(!app.workspace.active_document().unwrap().document.is_dirty());
        }
    }

    #[test]
    fn block_dedent_accepts_backtab_and_shift_tab_without_noop_history_or_readonly_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.explorer_focused = false;
        app.replace_in_active_tab("\tα\n  b\nplain");
        app.workspace
            .document_mut(app.workspace.active_document().unwrap().id)
            .unwrap()
            .document
            .mark_saved();
        app.key(ctrl('a'));
        app.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "α\nb\nplain"
        );
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..9));
        let revision = app.workspace.active_document().unwrap().document.revision();
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
        assert_eq!(
            app.workspace.active_document().unwrap().document.revision(),
            revision
        );
        assert_eq!(app.workspace.active_view().unwrap().selection(), Some(0..9));
        app.key(ctrl('z'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "\tα\n  b\nplain"
        );
        assert!(!app.workspace.active_document().unwrap().document.is_dirty());
        app.workspace
            .document_mut(app.workspace.active_document().unwrap().id)
            .unwrap()
            .read_only = Some("fixture".into());
        app.key(ctrl('a'));
        let selected = app.workspace.active_view().unwrap().selection();
        let revision = app.workspace.active_document().unwrap().document.revision();
        for key in [
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        ] {
            app.key(key);
            assert_eq!(
                app.workspace
                    .active_document()
                    .unwrap()
                    .document
                    .text()
                    .to_string(),
                "\tα\n  b\nplain"
            );
            assert_eq!(
                app.workspace.active_document().unwrap().document.revision(),
                revision
            );
            assert_eq!(app.workspace.active_view().unwrap().selection(), selected);
            assert_eq!(app.status.level, StatusLevel::Warning);
        }
    }

    #[test]
    fn block_indent_shared_views_keep_interior_carets_selection_and_viewports() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.viewport = (120, 36);
        app.workspace.explorer_focused = false;
        app.replace_in_active_tab("aa\nbb\ncc\ndd");
        app.workspace
            .document_mut(app.workspace.active_document().unwrap().id)
            .unwrap()
            .document
            .mark_saved();
        let p1 = app.workspace.active_pane_id();
        let v1 = app.workspace.active_view_id();
        app.key(fkey(8));
        let p2 = app.workspace.active_pane_id();
        let v2 = app.workspace.active_view_id();
        {
            let view = app.workspace.view_mut(v2).unwrap().1;
            view.cursor = 7;
            view.anchor = Some(4);
            view.top_line = 2;
            view.left_cell = 1;
        }
        app.workspace.activate_view(p1, v1);
        {
            let view = app.workspace.active_view_mut().unwrap();
            view.cursor = 8;
            view.anchor = Some(0);
        }
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "    aa\n    bb\n    cc\ndd"
        );
        let other = app.workspace.view(v2).unwrap().1;
        assert_eq!(
            (other.cursor, other.anchor, other.top_line, other.left_cell),
            (19, Some(12), 2, 1)
        );
        app.workspace.activate_view(p2, v2);
        app.key(ctrl('z'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "aa\nbb\ncc\ndd"
        );
        assert!(!app.workspace.active_document().unwrap().document.is_dirty());
        let other = app.workspace.view(v1).unwrap().1;
        assert_eq!((other.cursor, other.anchor), (8, Some(0)));
        {
            let view = app.workspace.active_view_mut().unwrap();
            view.cursor = 7;
            view.anchor = Some(4);
            view.top_line = 2;
        }
        app.workspace.activate_view(p1, v1);
        app.key(ctrl('y'));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "    aa\n    bb\n    cc\ndd"
        );
        let other = app.workspace.view(v2).unwrap().1;
        assert_eq!(
            (other.cursor, other.anchor, other.top_line),
            (19, Some(12), 2)
        );
    }

    #[test]
    fn block_indent_keeps_single_line_tab_and_modal_explorer_routing() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.explorer_focused = false;
        app.replace_in_active_tab("a界e\u{301}z");
        {
            let view = app.workspace.active_view_mut().unwrap();
            view.cursor = 1;
            view.anchor = Some(0);
        }
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "   界e\u{301}z"
        );
        assert_eq!(app.workspace.active_view().unwrap().anchor, None);
        app.key(ctrl('z'));
        {
            let view = app.workspace.active_view_mut().unwrap();
            view.cursor = 1;
            view.anchor = None;
        }
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "a   界e\u{301}z"
        );
        app.open_search_modal(true);
        for code in [KeyCode::Tab, KeyCode::BackTab] {
            app.key(KeyEvent::new(code, KeyModifiers::NONE));
            assert_eq!(
                app.workspace
                    .active_document()
                    .unwrap()
                    .document
                    .text()
                    .to_string(),
                "a   界e\u{301}z"
            );
        }
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.workspace.explorer_focused = true;
        app.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "a   界e\u{301}z"
        );
    }
    #[test]
    fn scrollbars_drag_only_the_origin_shared_view_without_changing_edit_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.explorer_visible = false;
        app.replace_in_active_tab(&format!("{}\n", "界\t".repeat(40)).repeat(80));
        let doc_id = app.workspace.active_document().unwrap().id;
        app.workspace
            .document_mut(doc_id)
            .unwrap()
            .document
            .mark_saved();
        let revision = app.workspace.document(doc_id).unwrap().document.revision();
        let first = app.workspace.active_pane_id();
        app.viewport = (80, 28);
        app.key(fkey(8));
        let second = app.workspace.active_pane_id();
        for id in [first, second] {
            let view = app
                .workspace
                .pane_mut(id)
                .unwrap()
                .active_tab_mut()
                .unwrap();
            view.cursor = 2;
            view.anchor = Some(1);
            view.preferred_column = Some(7);
            let offset = if id == first { 1 } else { 0 };
            view.top_line = offset;
            view.left_cell = offset;
        }
        for axis in [Axis::Rows, Axis::Columns] {
            let terminal = draw_measured_frame(&mut app, 80, 28);
            let geom = app.geometry.pane(first).unwrap();
            let bar = match axis {
                Axis::Rows => geom.vertical_scrollbar.unwrap(),
                Axis::Columns => geom.horizontal_scrollbar.unwrap(),
            };
            for y in bar.track.top()..bar.track.bottom() {
                for x in bar.track.left()..bar.track.right() {
                    assert_eq!(
                        terminal.backend().buffer()[(x, y)].bg,
                        ratatui::style::Color::Reset
                    );
                }
            }
            let x = bar.thumb.x;
            let y = bar.thumb.y;
            app.mouse(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
                Instant::now(),
            );
            let before = app.workspace.pane(first).unwrap().active_tab().unwrap();
            assert_eq!(
                match axis {
                    Axis::Rows => before.top_line,
                    Axis::Columns => before.left_cell,
                },
                bar.offset
            );
            app.mouse(
                MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
                Instant::now(),
            );
            let before = app.workspace.pane(first).unwrap().active_tab().unwrap();
            assert_eq!(
                match axis {
                    Axis::Rows => before.top_line,
                    Axis::Columns => before.left_cell,
                },
                bar.offset
            );
            app.mouse(
                MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: 79,
                    row: 27,
                    modifiers: KeyModifiers::NONE,
                },
                Instant::now(),
            );
            // Return to the quantized first cell after moving, not outside the rail.
            app.mouse(
                MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
                Instant::now(),
            );
            let returned = app.workspace.pane(first).unwrap().active_tab().unwrap();
            assert_eq!(
                match axis {
                    Axis::Rows => returned.top_line,
                    Axis::Columns => returned.left_cell,
                },
                0
            );
            app.mouse(
                MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: 79,
                    row: 27,
                    modifiers: KeyModifiers::NONE,
                },
                Instant::now(),
            );
            app.mouse(
                MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: 79,
                    row: 27,
                    modifiers: KeyModifiers::NONE,
                },
                Instant::now(),
            );
            draw_measured_frame(&mut app, 80, 28);
            let view = app.workspace.pane(first).unwrap().active_tab().unwrap();
            assert_eq!(
                match axis {
                    Axis::Rows => view.top_line,
                    Axis::Columns => view.left_cell,
                },
                bar.max_offset
            );
            assert_eq!(
                (view.cursor, view.anchor, view.preferred_column),
                (2, Some(1), Some(7))
            );
            let other = app.workspace.pane(second).unwrap().active_tab().unwrap();
            assert_eq!(
                (other.cursor, other.anchor, other.top_line, other.left_cell),
                (2, Some(1), 0, 0)
            );
            assert_eq!(app.workspace.active_pane_id(), second);
            let doc = &app.workspace.document(doc_id).unwrap().document;
            assert_eq!(doc.revision(), revision);
            assert!(!doc.is_dirty());
        }
    }

    #[test]
    fn scrollbar_track_rejects_stale_text_hits_and_changed_document_drag() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.explorer_visible = false;
        app.replace_in_active_tab(&"long line of text\n".repeat(100));
        let view = app.workspace.active_view_mut().unwrap();
        view.cursor = 0;
        view.anchor = Some(2);
        let view_id = view.id;
        let doc_id = view.document_id;
        draw_measured_frame(&mut app, 50, 16);
        let geometry = app.geometry.panes[0].clone();
        let bar = geometry.vertical_scrollbar.unwrap();
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: bar.track.x,
                row: bar.track.bottom() - 1,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        let top = app.workspace.active_view().unwrap().top_line;
        assert_eq!(top, usize::from(geometry.text_area.height));
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: geometry.text_area.x + 3,
                row: geometry.text_area.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        draw_measured_frame(&mut app, 50, 16);
        let view = app.workspace.active_view().unwrap();
        assert_eq!((view.cursor, view.anchor, view.top_line), (0, Some(2), top));
        let bar = app.geometry.panes[0].vertical_scrollbar.unwrap();
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: bar.thumb.x,
                row: bar.thumb.y,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        app.workspace
            .edit_transaction(doc_id, view_id, EditViewPolicy::PreserveSelection, |doc| {
                let end = doc.len_chars();
                let caret = doc.replace(end..end, "tail").unwrap();
                ((), Some(caret))
            });
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: bar.track.x,
                row: bar.track.bottom() - 1,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert!(app.drag.is_none());
        let view = app.workspace.active_view().unwrap();
        assert_eq!((view.cursor, view.anchor, view.top_line), (0, Some(2), top));
        draw_measured_frame(&mut app, 12, 6);
        let pane = &app.geometry.panes[0];
        assert!(!pane.text_area.is_empty());
        assert!(pane.vertical_scrollbar.is_none());
        assert!(pane.horizontal_track.is_none());
    }

    #[test]
    fn cached_horizontal_extent_clamps_inactive_view_after_width_only_resize() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.workspace.explorer_visible = false;
        app.replace_in_active_tab(&format!("{}\n", "x".repeat(200)).repeat(40));
        app.workspace.active_view_mut().unwrap().cursor = 0;
        let first = app.workspace.active_pane_id();
        app.viewport = (80, 28);
        app.key(fkey(8));
        let second = app.workspace.active_pane_id();
        draw_measured_frame(&mut app, 80, 28);
        let old = app
            .geometry
            .pane(first)
            .unwrap()
            .horizontal_scrollbar
            .unwrap();
        app.workspace
            .pane_mut(first)
            .unwrap()
            .active_tab_mut()
            .unwrap()
            .left_cell = old.max_offset;
        draw_measured_frame(&mut app, 80, 28);
        let terminal = draw_measured_frame(&mut app, 120, 28);
        let pane = app.geometry.pane(first).unwrap();
        assert!(pane.text_area.width > old.track.width);
        let view = app.workspace.pane(first).unwrap().active_tab().unwrap();
        let expected_left = 201usize.saturating_sub(usize::from(pane.text_area.width));
        assert_eq!(view.left_cell, expected_left);
        assert_eq!(
            terminal.backend().buffer()[(pane.text_area.right() - 2, pane.text_area.y)].symbol(),
            "x"
        );
        assert_eq!(app.workspace.active_pane_id(), second);
        assert_eq!(
            app.workspace
                .pane(second)
                .unwrap()
                .active_tab()
                .unwrap()
                .left_cell,
            0
        );
    }

    #[test]
    fn explorer_scrollbar_preserves_selection_focus_and_document_and_rejects_changed_rows() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..128 {
            std::fs::write(dir.path().join(format!("entry-{index:03}.txt")), "fixture").unwrap();
        }
        let mut app = test_app(dir.path());
        app.workspace.explorer_visible = true;
        app.workspace.explorer_focused = false;
        app.workspace.explorer.as_mut().unwrap().set_top(1);
        let doc_id = app.workspace.active_document().unwrap().id;
        let revision = app.workspace.active_document().unwrap().document.revision();
        let caret = {
            let view = app.workspace.active_view().unwrap();
            (view.cursor, view.anchor)
        };
        let selected = app.workspace.explorer.as_ref().unwrap().selected();
        let mouse = |kind, column, row| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        draw_measured_frame(&mut app, 120, 36);
        let bar = app.geometry.sidebar_scrollbar.unwrap();
        app.mouse(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                bar.track.x,
                bar.track.bottom() - 1,
            ),
            Instant::now(),
        );
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().top(),
            1 + usize::from(bar.track.height)
        );
        let rows = app.geometry.sidebar.unwrap();
        // The rows changed: a queued click must not select/open the old row.
        app.mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), rows.x + 5, rows.y),
            Instant::now(),
        );
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().selected(),
            selected
        );
        assert_eq!(app.workspace.active_document().unwrap().id, doc_id);
        app.workspace.explorer.as_mut().unwrap().set_top(1);
        draw_measured_frame(&mut app, 120, 36);
        let bar = app.geometry.sidebar_scrollbar.unwrap();
        app.mouse(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                bar.thumb.x,
                bar.thumb.y,
            ),
            Instant::now(),
        );
        app.mouse(
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                bar.thumb.x,
                bar.thumb.y,
            ),
            Instant::now(),
        );
        assert_eq!(app.workspace.explorer.as_ref().unwrap().top(), 1);
        app.mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 119, u16::MAX),
            Instant::now(),
        );
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().top(),
            bar.max_offset
        );
        app.mouse(
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                bar.thumb.x,
                bar.track.y,
            ),
            Instant::now(),
        );
        app.mouse(
            mouse(MouseEventKind::Up(MouseButton::Left), 119, 35),
            Instant::now(),
        );
        assert_eq!(app.workspace.explorer.as_ref().unwrap().top(), 0);
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().selected(),
            selected
        );
        assert!(!app.workspace.explorer_focused);
        assert_eq!(app.workspace.active_document().unwrap().id, doc_id);
        assert_eq!(
            app.workspace.active_document().unwrap().document.revision(),
            revision
        );
        let view = app.workspace.active_view().unwrap();
        assert_eq!((view.cursor, view.anchor), caret);

        app.workspace.explorer.as_mut().unwrap().set_top(1);
        draw_measured_frame(&mut app, 120, 36);
        let bar = app.geometry.sidebar_scrollbar.unwrap();
        app.mouse(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                bar.thumb.x,
                bar.thumb.y,
            ),
            Instant::now(),
        );
        std::fs::write(dir.path().join("zz-extra.txt"), "new entry").unwrap();
        app.workspace.explorer.as_mut().unwrap().refresh().unwrap();
        app.mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 119, u16::MAX),
            Instant::now(),
        );
        assert_eq!(app.workspace.explorer.as_ref().unwrap().top(), 1);
        assert_eq!(
            app.workspace.explorer.as_ref().unwrap().selected(),
            selected
        );
        app.key(KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::NONE));
        assert_eq!(
            app.workspace
                .active_document()
                .unwrap()
                .document
                .text()
                .to_string(),
            "Z"
        );
    }

    #[test]
    fn help_scrollbar_preserves_suspended_field_and_cancels_on_resize_or_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app(dir.path());
        app.open_search_modal(true);
        app.search.query = "needle".into();
        app.search.query.move_to(1, false);
        app.search.query.move_to(4, true);
        app.search.replacement = "keep".into();
        if let Some(Modal::Search {
            replacement_focus, ..
        }) = &mut app.modal
        {
            *replacement_focus = true;
        }
        let doc_id = app.workspace.active_document().unwrap().id;
        let revision = app.workspace.active_document().unwrap().document.revision();
        let mouse = |kind, column, row| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        app.key(fkey(1));
        draw_frame(&mut app, 80, 24);
        let bar = app.geometry.help_scrollbar.unwrap();
        let footer = app.geometry.commands[0].area;
        app.mouse(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                bar.track.x,
                bar.track.bottom() - 1,
            ),
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Help { scroll })
            if scroll == usize::from(bar.track.height)));
        // A footer hit from before scrolling must not dismiss the modal.
        app.mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), footer.x, footer.y),
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Help { .. })));
        draw_frame(&mut app, 80, 24);
        let bar = app.geometry.help_scrollbar.unwrap();
        app.mouse(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                bar.thumb.x,
                bar.thumb.y,
            ),
            Instant::now(),
        );
        app.mouse(
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                bar.thumb.x,
                bar.thumb.y,
            ),
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Help { scroll }) if scroll == bar.offset));
        app.mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 0, u16::MAX),
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Help { scroll }) if scroll == bar.max_offset));
        app.mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 0, bar.track.y),
            Instant::now(),
        );
        app.mouse(
            mouse(MouseEventKind::Up(MouseButton::Left), 0, 0),
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Help { scroll: 0 })));
        app.key(fkey(1));
        assert!(matches!(
            app.modal,
            Some(Modal::Search {
                replace_mode: true,
                replacement_focus: true
            })
        ));
        assert_eq!(app.search.query.as_str(), "needle");
        assert_eq!(app.search.query.selected(), Some("eed"));
        assert_eq!(app.search.query.cursor(), 4);
        assert_eq!(app.search.replacement.as_str(), "keep");

        app.key(fkey(1));
        draw_frame(&mut app, 80, 24);
        let bar = app.geometry.help_scrollbar.unwrap();
        app.mouse(
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                bar.thumb.x,
                bar.thumb.y,
            ),
            Instant::now(),
        );
        draw_frame(&mut app, 19, 5);
        app.mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 0, u16::MAX),
            Instant::now(),
        );
        assert!(matches!(app.modal, Some(Modal::Help { scroll: 0 })));
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(
            app.modal,
            Some(Modal::Search {
                replacement_focus: true,
                ..
            })
        ));
        assert_eq!(app.search.query.selected(), Some("eed"));
        assert_eq!(app.workspace.active_document().unwrap().id, doc_id);
        assert_eq!(
            app.workspace.active_document().unwrap().document.revision(),
            revision
        );
        assert!(!app.workspace.active_document().unwrap().document.is_dirty());
    }
}

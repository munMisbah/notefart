use std::{
    collections::VecDeque,
    fmt::Write as _,
    io,
    time::{Duration, Instant},
};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Paragraph, Wrap},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    input::{Input, InputEvent},
    terminal::{TerminalSession, TerminationSignal},
};

const POLL_TIMEOUT: Duration = Duration::from_millis(50);
const MAX_EVENTS_PER_BATCH: usize = 64;
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);

const MAX_EVENT_LOG: usize = 64;
const MAX_LOG_ENTRY_GRAPHEMES: usize = 96;
const MAX_LOG_ENTRY_BYTES: usize = 160;
const MAX_PASTE_PREVIEW_GRAPHEMES: usize = 72;
const MAX_PASTE_PREVIEW_BYTES: usize = 256;

const TAB_STOP: u16 = 4;
const SAMPLE_TEXT: &str = "a\tbcd:\t|e\u{0301}|o\u{0308}\u{0332}|漢|🦀|👩\u{200D}🔬|🇺🇸";

mod palette {
    use ratatui::style::Color;

    // Notefart Dark tokens from docs/DESIGN.md
    pub const CANVAS: Color = Color::from_u32(0x17_1B_24);
    pub const SURFACE: Color = Color::from_u32(0x1E_24_30);
    pub const RAISED: Color = Color::from_u32(0x29_33_44);
    pub const FG: Color = Color::from_u32(0xDC_E3_EE);
    pub const MUTED: Color = Color::from_u32(0xA4_AF_C0);
    pub const BORDER: Color = Color::from_u32(0x74_82_9A);
    pub const ACCENT: Color = Color::from_u32(0x8B_C5_FF);
    pub const SELECTION: Color = Color::from_u32(0x34_48_66);
    pub const SUCCESS: Color = Color::from_u32(0x91_D5_A5);
    pub const WARNING: Color = Color::from_u32(0xF2_CC_8F);
    pub const ERROR: Color = Color::from_u32(0xFF_9E_AE);
}

pub fn run() -> io::Result<()> {
    let mut session = TerminalSession::enter()?;
    let loop_res = session.size().and_then(|initial_size| {
        let mut app = ProbeApp::new(initial_size);
        run_event_loop(&mut session, &mut app)
    });
    let restore_res = session.restore();

    match (loop_res, restore_res) {
        (Ok(None), Ok(())) => Ok(()),
        (Ok(Some(sig)), Ok(())) => Err(io::Error::new(
            io::ErrorKind::Interrupted,
            format!("terminated by signal {}", sig.name()),
        )),
        (Ok(Some(sig)), Err(restore_err)) => Err(io::Error::new(
            io::ErrorKind::Interrupted,
            format!(
                "terminated by signal {}; terminal restore failed: {restore_err}",
                sig.name()
            ),
        )),
        (Ok(None), Err(restore_err)) => Err(restore_err),
        (Err(loop_err), Ok(())) => Err(loop_err),
        (Err(loop_err), Err(restore_err)) => Err(io::Error::new(
            loop_err.kind(),
            format!("{loop_err}; terminal restore failed: {restore_err}"),
        )),
    }
}

fn run_event_loop(
    session: &mut TerminalSession,
    app: &mut ProbeApp,
) -> io::Result<Option<TerminationSignal>> {
    let mut input = Input::new();
    let mut needs_redraw = true;

    loop {
        if let Some(sig) = session.pending_signal() {
            return Ok(Some(sig));
        }
        let size = session.size()?;
        if size != app.terminal_size {
            app.handle_event(Event::Resize(size.0, size.1), Instant::now());
            needs_redraw = true;
        }

        if needs_redraw {
            app.frames_drawn = app.frames_drawn.saturating_add(1);
            session.draw(|frame| app.render(frame))?;
            needs_redraw = false;
        }

        if app.should_quit {
            return Ok(None);
        }

        if let Some(sig) = session.pending_signal() {
            return Ok(Some(sig));
        }

        let Some(first) = input.poll(POLL_TIMEOUT)? else {
            continue;
        };
        if handle_input(app, first) {
            needs_redraw = true;
        }

        for _ in 1..MAX_EVENTS_PER_BATCH {
            if app.should_quit || session.pending_signal().is_some() {
                break;
            }
            let Some(next) = input.poll(Duration::ZERO)? else {
                break;
            };
            if handle_input(app, next) {
                needs_redraw = true;
            }
        }

        if app.should_quit {
            return Ok(None);
        }
    }
}

fn handle_input(app: &mut ProbeApp, event: InputEvent) -> bool {
    match event {
        InputEvent::Terminal(event) => app.handle_event(event, Instant::now()),
        InputEvent::ControlSequencePending => {
            app.push_log(
                LogCategory::Key,
                "Ctrl+G resumes input; terminal sequence incomplete".into(),
                None,
            );
            true
        }
        InputEvent::ControlSequenceResumed => {
            app.push_log(LogCategory::Key, "Input resumed".into(), None);
            true
        }
        InputEvent::PasteRejected => {
            app.push_log(
                LogCategory::Paste,
                "Paste rejected: invalid UTF-8 or over 1 MiB; no payload executed".into(),
                None,
            );
            true
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundedPreview {
    text: String,
    truncated: bool,
    byte_len: usize,
    char_count: usize,
    grapheme_count: usize,
    escaped_controls: usize,
    newline_count: usize,
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
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

fn is_escaped_control_or_format(ch: char) -> bool {
    ch.is_control() || is_bidi_or_invisible_format(ch)
}

fn escaped_char_len(ch: char) -> usize {
    match ch {
        '\0' | '\t' | '\n' | '\r' | '\\' => 2,
        c if c.is_ascii_control() => 4,
        c if is_escaped_control_or_format(c) => {
            let hex_digits = match c as u32 {
                0..=0xFFFF => 4,
                0x10000..=0xFFFFF => 5,
                _ => 6,
            };
            4 + hex_digits
        }
        c => c.len_utf8(),
    }
}

fn push_escaped_char(out: &mut String, ch: char) {
    match ch {
        '\0' => out.push_str("\\0"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\\' => out.push_str("\\\\"),
        c if c.is_ascii_control() => {
            let _ = write!(out, "\\x{:02x}", c as u32);
        }
        c if is_escaped_control_or_format(c) => {
            let _ = write!(out, "\\u{{{:04x}}}", c as u32);
        }
        c => out.push(c),
    }
}

fn bounded_escape_preview(input: &str, max_graphemes: usize, max_bytes: usize) -> BoundedPreview {
    let mut text = String::with_capacity(max_bytes.min(input.len().saturating_mul(2)));
    let mut truncated = false;
    let mut preview_graphemes = 0usize;
    let mut char_count = 0usize;
    let mut grapheme_count = 0usize;
    let mut escaped_controls = 0usize;
    let mut newline_count = 0usize;

    for grapheme in input.graphemes(true) {
        grapheme_count += 1;
        if matches!(grapheme, "\n" | "\r" | "\r\n") {
            newline_count += 1;
        }
        for ch in grapheme.chars() {
            char_count += 1;
            if is_escaped_control_or_format(ch) {
                escaped_controls += 1;
            }
        }

        if !truncated {
            if preview_graphemes >= max_graphemes {
                truncated = true;
            } else {
                let grapheme_escaped_len: usize = grapheme.chars().map(escaped_char_len).sum();
                if text.len().saturating_add(grapheme_escaped_len) <= max_bytes {
                    for ch in grapheme.chars() {
                        push_escaped_char(&mut text, ch);
                    }
                    preview_graphemes += 1;
                } else {
                    if text.is_empty() && max_bytes > 0 {
                        // A single huge grapheme cluster (e.g. stacked combining marks) still
                        // respects `max_bytes` on a UTF-8 scalar boundary.
                        for ch in grapheme.chars() {
                            let clen = escaped_char_len(ch);
                            if text.len().saturating_add(clen) > max_bytes {
                                break;
                            }
                            push_escaped_char(&mut text, ch);
                        }
                    }
                    truncated = true;
                }
            }
        }
    }

    BoundedPreview {
        text,
        truncated,
        byte_len: input.len(),
        char_count,
        grapheme_count,
        escaped_controls,
        newline_count,
    }
}

fn escape_bounded_str(input: &str, max_graphemes: usize, max_bytes: usize) -> String {
    let preview = bounded_escape_preview(input, max_graphemes, max_bytes);
    if !preview.truncated {
        return preview.text;
    }

    const ELLIPSIS: &str = "…";
    let mut out = preview.text;
    while out.len().saturating_add(ELLIPSIS.len()) > max_bytes {
        let Some((last_idx, _)) = out.grapheme_indices(true).next_back() else {
            break;
        };
        out.truncate(last_idx);
    }
    if out.len().saturating_add(ELLIPSIS.len()) <= max_bytes {
        out.push_str(ELLIPSIS);
    }
    out
}

#[derive(Debug, Clone)]
struct SampleCluster {
    index: usize,
    raw: &'static str,
    display: String,
    byte_start: usize,
    byte_end: usize,
    col_start: u16,
    cell_width: u16,
    is_tab: bool,
}

#[derive(Debug, Clone)]
struct SampleLayout {
    clusters: Vec<SampleCluster>,
    byte_len: usize,
    char_count: usize,
    grapheme_count: usize,
    total_width: u16,
}

impl SampleLayout {
    fn new() -> Self {
        let mut clusters = Vec::new();
        let mut col: u16 = 0;

        for (index, (byte_start, grapheme)) in SAMPLE_TEXT.grapheme_indices(true).enumerate() {
            let byte_end = byte_start + grapheme.len();
            let (display, cell_width, is_tab) = if grapheme == "\t" {
                let advance = TAB_STOP - (col % TAB_STOP);
                let mut tab_vis = String::with_capacity(1 + usize::from(advance) * 2);
                tab_vis.push('→');
                for _ in 1..advance {
                    tab_vis.push('·');
                }
                (tab_vis, advance, true)
            } else {
                let width = u16::try_from(grapheme.width()).unwrap_or(u16::MAX);
                (grapheme.to_string(), width, false)
            };

            clusters.push(SampleCluster {
                index,
                raw: grapheme,
                display,
                byte_start,
                byte_end,
                col_start: col,
                cell_width,
                is_tab,
            });
            col = col.saturating_add(cell_width);
        }

        Self {
            grapheme_count: clusters.len(),
            clusters,
            byte_len: SAMPLE_TEXT.len(),
            char_count: SAMPLE_TEXT.chars().count(),
            total_width: col,
        }
    }

    fn hit_at_col(&self, rel_col: u16) -> Option<&SampleCluster> {
        self.clusters.iter().find(|cluster| {
            cluster.cell_width > 0
                && rel_col >= cluster.col_start
                && rel_col < cluster.col_start.saturating_add(cluster.cell_width)
        })
    }
}

#[derive(Debug, Clone)]
struct SampleHit {
    rel_col: u16,
    cluster_index: usize,
    raw_escaped: String,
    col_start: u16,
    col_end: u16,
    byte_start: usize,
    byte_end: usize,
    cell_width: u16,
}

#[derive(Debug, Clone, Copy)]
struct DragState {
    button: MouseButton,
    start: (u16, u16),
    current: (u16, u16),
    steps: u64,
}

#[derive(Debug, Clone, Copy)]
struct ClickSummary {
    button: MouseButton,
    down_pos: (u16, u16),
    up_pos: (u16, u16),
    is_double: bool,
    double_dt_ms: Option<u128>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogCategory {
    Key,
    Mouse,
    Paste,
    Resize,
    Focus,
}

impl LogCategory {
    const fn tag(self) -> &'static str {
        match self {
            Self::Key => "KEY",
            Self::Mouse => "MOUSE",
            Self::Paste => "PASTE",
            Self::Resize => "RESIZE",
            Self::Focus => "FOCUS",
        }
    }
}

#[derive(Debug, Clone)]
struct LogEntry {
    seq: u64,
    category: LogCategory,
    message: String,
    active_drag_button: Option<MouseButton>,
}

struct ProbeApp {
    should_quit: bool,
    terminal_size: (u16, u16),
    focus_state: Option<bool>,
    frames_drawn: u64,

    key_events: u64,
    key_repeats: u64,
    mouse_events: u64,
    mouse_down: u64,
    mouse_up: u64,
    mouse_drag: u64,
    mouse_moved: u64,
    mouse_wheel_up: u64,
    mouse_wheel_down: u64,
    mouse_wheel_left: u64,
    mouse_wheel_right: u64,
    double_clicks: u64,
    paste_events: u64,
    total_pasted_bytes: u64,
    resize_events: u64,

    last_key: Option<String>,

    hover_pos: Option<(u16, u16)>,
    last_mouse_desc: Option<String>,
    active_down: Option<(MouseButton, u16, u16)>,
    last_down_timing: Option<(MouseButton, u16, u16, Instant, u8)>,
    last_click: Option<ClickSummary>,
    last_double_click: Option<(MouseButton, u16, u16, u128)>,
    active_drag: Option<DragState>,
    last_drag: Option<DragState>,
    last_wheel: Option<(MouseEventKind, u16, u16, KeyModifiers)>,

    sample: SampleLayout,
    sample_rect: Option<Rect>,
    last_sample_hit: Option<SampleHit>,

    last_paste: Option<BoundedPreview>,
    event_log: VecDeque<LogEntry>,
    next_seq: u64,
}

impl ProbeApp {
    fn new(initial_size: (u16, u16)) -> Self {
        Self {
            should_quit: false,
            terminal_size: initial_size,
            focus_state: None,
            frames_drawn: 0,
            key_events: 0,
            key_repeats: 0,
            mouse_events: 0,
            mouse_down: 0,
            mouse_up: 0,
            mouse_drag: 0,
            mouse_moved: 0,
            mouse_wheel_up: 0,
            mouse_wheel_down: 0,
            mouse_wheel_left: 0,
            mouse_wheel_right: 0,
            double_clicks: 0,
            paste_events: 0,
            total_pasted_bytes: 0,
            resize_events: 0,
            last_key: None,
            hover_pos: None,
            last_mouse_desc: None,
            active_down: None,
            last_down_timing: None,
            last_click: None,
            last_double_click: None,
            active_drag: None,
            last_drag: None,
            last_wheel: None,
            sample: SampleLayout::new(),
            sample_rect: None,
            last_sample_hit: None,
            last_paste: None,
            event_log: VecDeque::with_capacity(MAX_EVENT_LOG),
            next_seq: 1,
        }
    }

    fn handle_event(&mut self, event: Event, now: Instant) -> bool {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse, now),
            Event::Paste(data) => {
                self.handle_paste(&data);
                true
            }
            Event::Resize(cols, rows) => {
                self.resize_events = self.resize_events.saturating_add(1);
                self.terminal_size = (cols, rows);
                self.sample_rect = None;
                self.last_sample_hit = None;
                self.last_down_timing = None;
                self.push_log(LogCategory::Resize, format!("{cols}x{rows}"), None);
                true
            }
            Event::FocusGained => {
                self.focus_state = Some(true);
                self.push_log(LogCategory::Focus, "Gained".to_string(), None);
                true
            }
            Event::FocusLost => {
                self.focus_state = Some(false);
                self.push_log(LogCategory::Focus, "Lost".to_string(), None);
                true
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }

        self.key_events = self.key_events.saturating_add(1);
        if key.kind == KeyEventKind::Repeat {
            self.key_repeats = self.key_repeats.saturating_add(1);
        }

        let desc = format_key_event(key);
        self.last_key = Some(desc.clone());

        if is_quit_shortcut(key) {
            self.push_log(LogCategory::Key, format!("{desc} -> quit"), None);
            self.should_quit = true;
            return true;
        }

        if is_ctrl_z(key) {
            self.push_log(
                LogCategory::Key,
                format!("{desc} (suspend unsupported; captured as event)"),
                None,
            );
            return true;
        }

        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            self.push_log(LogCategory::Key, format!("{desc} (logged, no exit)"), None);
            return true;
        }

        self.push_log(LogCategory::Key, desc, None);
        true
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, now: Instant) -> bool {
        self.mouse_events = self.mouse_events.saturating_add(1);
        self.hover_pos = Some((mouse.column, mouse.row));
        self.last_mouse_desc = Some(format_mouse_event(mouse));

        let mods = format_modifiers_suffix(mouse.modifiers);

        match mouse.kind {
            MouseEventKind::Down(button) => {
                self.mouse_down = self.mouse_down.saturating_add(1);
                if let Some(prev_drag) = self.active_drag.take() {
                    self.last_drag = Some(prev_drag);
                }
                self.active_down = Some((button, mouse.column, mouse.row));
                self.update_sample_hit(mouse.column, mouse.row);
                let (is_double, double_dt_ms) = match self.last_down_timing {
                    Some((prev_btn, prev_col, prev_row, prev_at, streak))
                        if prev_btn == button
                            && prev_col == mouse.column
                            && prev_row == mouse.row
                            && now.duration_since(prev_at) <= DOUBLE_CLICK_WINDOW =>
                    {
                        if streak == 1 {
                            let dt = now.duration_since(prev_at).as_millis();
                            self.double_clicks = self.double_clicks.saturating_add(1);
                            self.last_double_click = Some((button, mouse.column, mouse.row, dt));
                            self.last_down_timing = Some((button, mouse.column, mouse.row, now, 2));
                            (true, Some(dt))
                        } else {
                            self.last_down_timing = Some((button, mouse.column, mouse.row, now, 1));
                            (false, None)
                        }
                    }
                    _ => {
                        self.last_down_timing = Some((button, mouse.column, mouse.row, now, 1));
                        (false, None)
                    }
                };

                self.last_click = Some(ClickSummary {
                    button,
                    down_pos: (mouse.column, mouse.row),
                    up_pos: (mouse.column, mouse.row),
                    is_double,
                    double_dt_ms,
                });

                let dbl_tag = match double_dt_ms {
                    Some(dt) => format!(" [DOUBLE {dt}ms]"),
                    None => String::new(),
                };
                self.push_log(
                    LogCategory::Mouse,
                    format!(
                        "Down {} @({},{}){mods}{dbl_tag}",
                        format_mouse_button(button),
                        mouse.column,
                        mouse.row
                    ),
                    None,
                );
            }
            MouseEventKind::Up(button) => {
                self.mouse_up = self.mouse_up.saturating_add(1);
                let finished_drag = if let Some(mut drag) = self.active_drag.take() {
                    drag.current = (mouse.column, mouse.row);
                    self.last_drag = Some(drag);
                    Some(drag)
                } else {
                    None
                };

                let (down_pos, is_double, double_dt_ms) =
                    match (self.active_down.take(), self.last_click) {
                        (Some((_, dx, dy)), Some(prev)) if prev.down_pos == (dx, dy) => {
                            ((dx, dy), prev.is_double, prev.double_dt_ms)
                        }
                        (Some((_, dx, dy)), _) => ((dx, dy), false, None),
                        (None, _) => ((mouse.column, mouse.row), false, None),
                    };

                self.last_click = Some(ClickSummary {
                    button,
                    down_pos,
                    up_pos: (mouse.column, mouse.row),
                    is_double,
                    double_dt_ms,
                });

                let drag_note = match finished_drag {
                    Some(drag) => {
                        format!(" (dragged from {},{})", drag.start.0, drag.start.1)
                    }
                    None => String::new(),
                };
                self.push_log(
                    LogCategory::Mouse,
                    format!(
                        "Up {} @({},{}){mods}{drag_note}",
                        format_mouse_button(button),
                        mouse.column,
                        mouse.row
                    ),
                    None,
                );
            }
            MouseEventKind::Drag(button) => {
                self.mouse_drag = self.mouse_drag.saturating_add(1);
                self.last_down_timing = None;
                self.update_sample_hit(mouse.column, mouse.row);

                let start = match (self.active_drag, self.active_down) {
                    (Some(drag), _) if drag.button == button => drag.start,
                    (_, Some((_, dx, dy))) => (dx, dy),
                    _ => (mouse.column, mouse.row),
                };
                let steps = match self.active_drag {
                    Some(drag) if drag.button == button => drag.steps.saturating_add(1),
                    _ => 1,
                };
                let drag = DragState {
                    button,
                    start,
                    current: (mouse.column, mouse.row),
                    steps,
                };
                self.active_drag = Some(drag);

                let msg = format!(
                    "Drag {} ({},{})->({},{}) [{steps}]{mods}",
                    format_mouse_button(button),
                    drag.start.0,
                    drag.start.1,
                    drag.current.0,
                    drag.current.1,
                );
                if matches!(
                    self.event_log.front(),
                    Some(front) if front.active_drag_button == Some(button)
                ) {
                    if let Some(front) = self.event_log.front_mut() {
                        front.message =
                            escape_bounded_str(&msg, MAX_LOG_ENTRY_GRAPHEMES, MAX_LOG_ENTRY_BYTES);
                    }
                } else {
                    self.push_log(LogCategory::Mouse, msg, Some(button));
                }
            }
            MouseEventKind::Moved => {
                self.mouse_moved = self.mouse_moved.saturating_add(1);
            }
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        self.mouse_wheel_up = self.mouse_wheel_up.saturating_add(1);
                    }
                    MouseEventKind::ScrollDown => {
                        self.mouse_wheel_down = self.mouse_wheel_down.saturating_add(1);
                    }
                    MouseEventKind::ScrollLeft => {
                        self.mouse_wheel_left = self.mouse_wheel_left.saturating_add(1);
                    }
                    MouseEventKind::ScrollRight => {
                        self.mouse_wheel_right = self.mouse_wheel_right.saturating_add(1);
                    }
                    _ => {}
                }
                self.last_wheel = Some((mouse.kind, mouse.column, mouse.row, mouse.modifiers));
                self.push_log(
                    LogCategory::Mouse,
                    format!(
                        "{} @({},{}){mods}",
                        format_mouse_kind(mouse.kind),
                        mouse.column,
                        mouse.row
                    ),
                    None,
                );
            }
        }

        true
    }

    fn handle_paste(&mut self, data: &str) {
        self.paste_events = self.paste_events.saturating_add(1);
        self.total_pasted_bytes = self.total_pasted_bytes.saturating_add(data.len() as u64);

        // Paste is strictly data: never interpret Ctrl+Q / Ctrl+C / ESC inside pasted payloads.
        let preview =
            bounded_escape_preview(data, MAX_PASTE_PREVIEW_GRAPHEMES, MAX_PASTE_PREVIEW_BYTES);
        let log_snippet = escape_bounded_str(&preview.text, 36, 72);
        let trunc_mark = if preview.truncated { " [trunc]" } else { "" };
        self.push_log(
            LogCategory::Paste,
            format!(
                "bytes={} g={} ctrl={} \"{log_snippet}\"{trunc_mark}",
                preview.byte_len, preview.grapheme_count, preview.escaped_controls,
            ),
            None,
        );
        self.last_paste = Some(preview);
    }

    fn update_sample_hit(&mut self, col: u16, row: u16) {
        let Some(rect) = self.sample_rect else {
            return;
        };
        if row == rect.y && col >= rect.x && col < rect.right() {
            let rel_col = col - rect.x;
            if let Some(cluster) = self.sample.hit_at_col(rel_col) {
                self.last_sample_hit = Some(SampleHit {
                    rel_col,
                    cluster_index: cluster.index,
                    raw_escaped: escape_bounded_str(cluster.raw, 8, 32),
                    col_start: cluster.col_start,
                    col_end: cluster.col_start.saturating_add(cluster.cell_width),
                    byte_start: cluster.byte_start,
                    byte_end: cluster.byte_end,
                    cell_width: cluster.cell_width,
                });
            }
        }
    }

    fn push_log(
        &mut self,
        category: LogCategory,
        message: String,
        active_drag_button: Option<MouseButton>,
    ) {
        if self.event_log.len() >= MAX_EVENT_LOG {
            self.event_log.pop_back();
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.event_log.push_front(LogEntry {
            seq,
            category,
            message: escape_bounded_str(&message, MAX_LOG_ENTRY_GRAPHEMES, MAX_LOG_ENTRY_BYTES),
            active_drag_button,
        });
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.terminal_size = (area.width, area.height);
        self.sample_rect = None;

        frame.render_widget(
            Block::new().style(Style::new().bg(palette::CANVAS).fg(palette::FG)),
            area,
        );

        if area.is_empty() {
            return;
        }

        if area.width < 36 || area.height < 12 {
            self.render_tiny(frame, area);
        } else if area.width < 72 || area.height < 22 {
            self.render_compact(frame, area);
        } else {
            self.render_full(frame, area);
        }
    }

    fn render_tiny(&self, frame: &mut Frame, area: Rect) {
        let (w, h) = self.terminal_size;
        let last_event = match self.event_log.front() {
            Some(entry) => format!(
                "#{} [{}] {}",
                entry.seq,
                entry.category.tag(),
                entry.message
            ),
            None => "no events yet".to_string(),
        };
        let text = Text::from(vec![
            Line::from(vec![
                Span::styled(
                    "PROBE ",
                    Style::new()
                        .fg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("too small ({w}x{h}; need >=36x12)"),
                    Style::new().fg(palette::WARNING),
                ),
            ]),
            Line::from(Span::styled(
                format!(
                    "K:{} M:{} P:{} R:{}",
                    self.key_events, self.mouse_events, self.paste_events, self.resize_events
                ),
                Style::new().fg(palette::FG),
            )),
            Line::from(Span::styled(last_event, Style::new().fg(palette::MUTED))),
            Line::from(Span::styled(
                "Ctrl+Q / Ctrl+C: Quit",
                Style::new()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )),
        ]);

        let widget = Paragraph::new(text)
            .style(Style::new().bg(palette::SURFACE).fg(palette::FG))
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);
    }

    fn render_compact(&mut self, frame: &mut Frame, area: Rect) {
        let [header_area, body_area, helper_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);

        self.render_header(frame, header_area, true);
        self.render_helper(frame, helper_area, true);

        if body_area.is_empty() {
            return;
        }

        if body_area.height >= 10 {
            let [stats_area, sample_area, paste_area, log_area] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Min(2),
            ])
            .areas(body_area);

            self.render_compact_stats(frame, stats_area);
            self.render_sample_inner(frame, sample_area);
            self.render_paste_inner(frame, paste_area);
            self.render_log_inner(frame, log_area);
        } else {
            let [stats_area, sample_area, log_area] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .areas(body_area);

            self.render_compact_stats(frame, stats_area);
            self.render_sample_inner(frame, sample_area);
            self.render_log_inner(frame, log_area);
        }
    }

    fn render_compact_stats(&self, frame: &mut Frame, area: Rect) {
        if area.is_empty() {
            return;
        }
        let drag_str = self.format_drag_summary();
        let wheel_total = self
            .mouse_wheel_up
            .saturating_add(self.mouse_wheel_down)
            .saturating_add(self.mouse_wheel_left)
            .saturating_add(self.mouse_wheel_right);
        let lines = vec![
            Line::from(vec![
                Span::styled("Counts ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    format!(
                        "K:{} (rep:{}) M:{} (dbl:{}) P:{} R:{}",
                        self.key_events,
                        self.key_repeats,
                        self.mouse_events,
                        self.double_clicks,
                        self.paste_events,
                        self.resize_events
                    ),
                    Style::new().fg(palette::FG),
                ),
            ]),
            Line::from(vec![
                Span::styled("Key ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    self.last_key.as_deref().unwrap_or("none"),
                    Style::new().fg(palette::ACCENT),
                ),
                Span::styled(" | Ptr ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    self.format_hover_coords(),
                    Style::new().fg(palette::SUCCESS),
                ),
            ]),
            Line::from(vec![
                Span::styled("Drag ", Style::new().fg(palette::MUTED)),
                Span::styled(drag_str, Style::new().fg(palette::FG)),
                Span::styled(
                    format!(" | Wheel:{wheel_total}"),
                    Style::new().fg(palette::MUTED),
                ),
            ]),
        ];
        let paragraph =
            Paragraph::new(lines).style(Style::new().bg(palette::SURFACE).fg(palette::FG));
        frame.render_widget(paragraph, area);
    }

    fn render_full(&mut self, frame: &mut Frame, area: Rect) {
        let [
            header_area,
            top_area,
            sample_area,
            paste_area,
            log_area,
            helper_area,
        ] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .areas(area);

        self.render_header(frame, header_area, false);

        let [counters_area, mouse_area] =
            Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
                .areas(top_area);

        self.render_counters_panel(frame, counters_area);
        self.render_mouse_panel(frame, mouse_area);
        self.render_sample_panel(frame, sample_area);
        self.render_paste_panel(frame, paste_area);
        self.render_log_panel(frame, log_area);
        self.render_helper(frame, helper_area, false);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, compact: bool) {
        if area.is_empty() {
            return;
        }
        let (w, h) = self.terminal_size;
        let focus = match self.focus_state {
            Some(true) => "Gained",
            Some(false) => "Lost",
            None => "Unknown",
        };
        let line = if compact {
            Line::from(vec![
                Span::styled(
                    " Notefart Probe ",
                    Style::new()
                        .fg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("[NOT AN EDITOR] ", Style::new().fg(palette::WARNING)),
                Span::styled(
                    format!("{w}x{h} F:{focus}"),
                    Style::new().fg(palette::MUTED),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    " Notefart Terminal Diagnostics ",
                    Style::new()
                        .fg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "[READ-ONLY PROBE — NOT AN EDITOR]",
                    Style::new().fg(palette::WARNING),
                ),
                Span::styled(
                    format!(" | Size: {w}x{h} | Focus: {focus} | TTY: stdin+stdout | Suspend: off"),
                    Style::new().fg(palette::MUTED),
                ),
            ])
        };
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(palette::SURFACE).fg(palette::FG)),
            area,
        );
    }

    fn render_counters_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::ACCENT))
            .style(Style::new().bg(palette::SURFACE).fg(palette::FG))
            .title_top(Line::from(Span::styled(
                " Counters & Keyboard ",
                Style::new()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.is_empty() {
            return;
        }

        let lines = vec![
            Line::from(vec![
                Span::styled("Keys: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    format!(
                        "{} (repeat: {}) | Resize: {}",
                        self.key_events, self.key_repeats, self.resize_events
                    ),
                    Style::new().fg(palette::FG),
                ),
            ]),
            Line::from(vec![
                Span::styled("Mouse: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    format!(
                        "{} (down:{} up:{} drag:{} dbl:{})",
                        self.mouse_events,
                        self.mouse_down,
                        self.mouse_up,
                        self.mouse_drag,
                        self.double_clicks
                    ),
                    Style::new().fg(palette::FG),
                ),
            ]),
            Line::from(vec![
                Span::styled("Paste: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    format!(
                        "{} ({} B) | Frames: {}",
                        self.paste_events, self.total_pasted_bytes, self.frames_drawn
                    ),
                    Style::new().fg(palette::FG),
                ),
            ]),
            Line::from(vec![
                Span::styled("Last Key: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    self.last_key.as_deref().unwrap_or("none"),
                    Style::new().fg(palette::ACCENT),
                ),
            ]),
            Line::from(Span::styled(
                "Key releases ignored; Ctrl+Z captured as event",
                Style::new().fg(palette::MUTED),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_mouse_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::BORDER))
            .style(Style::new().bg(palette::SURFACE).fg(palette::FG))
            .title_top(Line::from(Span::styled(
                " Mouse, Drag, Wheel & Hit State ",
                Style::new().fg(palette::FG).add_modifier(Modifier::BOLD),
            )));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.is_empty() {
            return;
        }

        let click_str = match self.last_click {
            Some(c) if c.is_double => format!(
                "{} ({},{})->({},{}) DOUBLE {}ms",
                format_mouse_button(c.button),
                c.down_pos.0,
                c.down_pos.1,
                c.up_pos.0,
                c.up_pos.1,
                c.double_dt_ms.unwrap_or(0)
            ),
            Some(c) => format!(
                "{} ({},{})->({},{})",
                format_mouse_button(c.button),
                c.down_pos.0,
                c.down_pos.1,
                c.up_pos.0,
                c.up_pos.1
            ),
            None => "none".to_string(),
        };
        let dbl_suffix = match self.last_double_click {
            Some((btn, col, row, dt)) => {
                format!(" | Dbl: {} @({col},{row}) {dt}ms", format_mouse_button(btn))
            }
            None => String::new(),
        };
        let wheel_str = match self.last_wheel {
            Some((kind, col, row, mods)) => {
                let m = format_modifiers_suffix(mods);
                format!("{} @({col},{row}){m}", format_mouse_kind(kind))
            }
            None => "none".to_string(),
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("Ptr: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    format!(
                        "{} (moved:{}) | ",
                        self.format_hover_coords(),
                        self.mouse_moved
                    ),
                    Style::new().fg(palette::SUCCESS),
                ),
                Span::styled(
                    self.last_mouse_desc.as_deref().unwrap_or("none"),
                    Style::new().fg(palette::FG),
                ),
            ]),
            Line::from(vec![
                Span::styled("Click: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    format!("{click_str}{dbl_suffix}"),
                    Style::new().fg(palette::FG),
                ),
            ]),
            Line::from(vec![
                Span::styled("Drag: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    self.format_drag_summary(),
                    if self.active_drag.is_some() {
                        Style::new()
                            .fg(palette::WARNING)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(palette::FG)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("Wheel: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    format!(
                        "U:{} D:{} L:{} R:{} | {wheel_str}",
                        self.mouse_wheel_up,
                        self.mouse_wheel_down,
                        self.mouse_wheel_left,
                        self.mouse_wheel_right
                    ),
                    Style::new().fg(palette::FG),
                ),
            ]),
            Line::from(vec![
                Span::styled("Sample Hit: ", Style::new().fg(palette::MUTED)),
                Span::styled(
                    self.format_sample_hit(),
                    if self.last_sample_hit.is_some() {
                        Style::new().fg(palette::ACCENT)
                    } else {
                        Style::new().fg(palette::MUTED)
                    },
                ),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_sample_panel(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::BORDER))
            .style(Style::new().bg(palette::SURFACE).fg(palette::FG))
            .title_top(Line::from(Span::styled(
                " Unicode / Grapheme / Tab Width Sample (tab=4, unicode-width 0.2.0) ",
                Style::new().fg(palette::FG).add_modifier(Modifier::BOLD),
            )));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.render_sample_inner(frame, inner);
    }

    fn render_sample_inner(&mut self, frame: &mut Frame, area: Rect) {
        if area.is_empty() {
            return;
        }

        let max_cols = self.sample.total_width.min(area.width);
        let ruler_spans: Vec<Span> = (0..max_cols)
            .map(|col| {
                let digit = char::from(b'0' + (col % 10) as u8);
                let style = if col % TAB_STOP == 0 {
                    Style::new().fg(palette::ACCENT)
                } else {
                    Style::new().fg(palette::MUTED)
                };
                Span::styled(digit.to_string(), style)
            })
            .collect();

        let mut sample_spans = Vec::with_capacity(self.sample.clusters.len());
        let mut rendered_cols: u16 = 0;
        let hit_idx = self.last_sample_hit.as_ref().map(|h| h.cluster_index);

        for cluster in &self.sample.clusters {
            let next_col = cluster.col_start.saturating_add(cluster.cell_width);
            if next_col > area.width {
                break;
            }
            let style = if hit_idx == Some(cluster.index) {
                Style::new()
                    .bg(palette::SELECTION)
                    .fg(palette::FG)
                    .add_modifier(Modifier::BOLD)
            } else if cluster.is_tab {
                Style::new().fg(palette::MUTED)
            } else {
                Style::new().fg(palette::FG)
            };
            sample_spans.push(Span::styled(cluster.display.as_str(), style));
            rendered_cols = next_col;
        }

        // Row 0 = ruler, Row 1 = rendered sample (when height >= 2).
        let ruler_rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(Line::from(ruler_spans)), ruler_rect);

        if area.height >= 2 {
            let sample_line_area = Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(Line::from(sample_spans)), sample_line_area);
            if rendered_cols > 0 {
                self.sample_rect = Some(Rect {
                    x: sample_line_area.x,
                    y: sample_line_area.y,
                    width: rendered_cols,
                    height: 1,
                });
            }
        }

        if area.height >= 3 {
            let metrics_area = Rect {
                x: area.x,
                y: area.y + 2,
                width: area.width,
                height: 1,
            };
            let hit_info = if self.last_sample_hit.is_some() {
                self.format_sample_hit()
            } else {
                "click sample row to inspect cell -> grapheme".to_string()
            };
            let line = Line::from(vec![
                Span::styled(
                    format!(
                        "bytes:{} chars:{} graphemes:{} width:{} cols | ",
                        self.sample.byte_len,
                        self.sample.char_count,
                        self.sample.grapheme_count,
                        self.sample.total_width
                    ),
                    Style::new().fg(palette::MUTED),
                ),
                Span::styled(hit_info, Style::new().fg(palette::ACCENT)),
            ]);
            frame.render_widget(Paragraph::new(line), metrics_area);
        }
    }

    fn render_paste_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::BORDER))
            .style(Style::new().bg(palette::RAISED).fg(palette::FG))
            .title_top(Line::from(Span::styled(
                " Bracketed Paste (Data Only, Controls Escaped, Memory Bounded) ",
                Style::new()
                    .fg(palette::WARNING)
                    .add_modifier(Modifier::BOLD),
            )));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.render_paste_inner(frame, inner);
    }

    fn render_paste_inner(&self, frame: &mut Frame, area: Rect) {
        if area.is_empty() {
            return;
        }
        let lines = match &self.last_paste {
            Some(p) => {
                let trunc_label = if p.truncated { " [TRUNCATED]" } else { "" };
                let ellipsis = if p.truncated { "…" } else { "" };
                vec![
                    Line::from(vec![
                        Span::styled(
                            format!(
                                "Count: {} | bytes={} chars={} graphemes={} escaped_ctrl={} newlines={}",
                                self.paste_events,
                                p.byte_len,
                                p.char_count,
                                p.grapheme_count,
                                p.escaped_controls,
                                p.newline_count
                            ),
                            Style::new().fg(palette::FG),
                        ),
                        Span::styled(trunc_label, Style::new().fg(palette::WARNING)),
                    ]),
                    Line::from(vec![
                        Span::styled("Preview: ", Style::new().fg(palette::MUTED)),
                        Span::styled(
                            format!("\"{}\"{ellipsis}", p.text),
                            Style::new().fg(palette::SUCCESS),
                        ),
                    ]),
                ]
            }
            None => vec![
                Line::from(Span::styled(
                    "No bracketed paste received yet (Ctrl+Q / ESC inside paste stays data).",
                    Style::new().fg(palette::MUTED),
                )),
                Line::from(Span::styled(
                    format!(
                        "Preview bound: <= {MAX_PASTE_PREVIEW_GRAPHEMES} graphemes, <= {MAX_PASTE_PREVIEW_BYTES} B."
                    ),
                    Style::new().fg(palette::MUTED),
                )),
            ],
        };
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_log_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::BORDER))
            .style(Style::new().bg(palette::SURFACE).fg(palette::FG))
            .title_top(Line::from(Span::styled(
                format!(
                    " Recent Events ({}/{MAX_EVENT_LOG}, Newest First, No Disk Log) ",
                    self.event_log.len()
                ),
                Style::new().fg(palette::FG).add_modifier(Modifier::BOLD),
            )));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.render_log_inner(frame, inner);
    }

    fn render_log_inner(&self, frame: &mut Frame, area: Rect) {
        if area.is_empty() {
            return;
        }
        let max_rows = usize::from(area.height);
        if self.event_log.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Waiting for keyboard, mouse, paste, or resize events...",
                    Style::new().fg(palette::MUTED),
                ))),
                area,
            );
            return;
        }

        let lines: Vec<Line> = self
            .event_log
            .iter()
            .take(max_rows)
            .map(|entry| {
                let tag_color = match entry.category {
                    LogCategory::Key => palette::ACCENT,
                    LogCategory::Mouse => palette::SUCCESS,
                    LogCategory::Paste => palette::WARNING,
                    LogCategory::Resize => palette::ERROR,
                    LogCategory::Focus => palette::MUTED,
                };
                Line::from(vec![
                    Span::styled(
                        format!("#{:04} ", entry.seq),
                        Style::new().fg(palette::MUTED),
                    ),
                    Span::styled(
                        format!("[{:6}] ", entry.category.tag()),
                        Style::new().fg(tag_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(entry.message.as_str(), Style::new().fg(palette::FG)),
                ])
            })
            .collect();

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_helper(&self, frame: &mut Frame, area: Rect, compact: bool) {
        if area.is_empty() {
            return;
        }
        let line = if compact {
            Line::from(vec![
                Span::styled(
                    " Ctrl+Q/C",
                    Style::new()
                        .fg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Quit ", Style::new().fg(palette::MUTED)),
                Span::styled("Esc", Style::new().fg(palette::ACCENT)),
                Span::styled(" Log ", Style::new().fg(palette::MUTED)),
                Span::styled("Ctrl+Z", Style::new().fg(palette::ACCENT)),
                Span::styled(" Event only", Style::new().fg(palette::MUTED)),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    " Ctrl+Q",
                    Style::new()
                        .fg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("/", Style::new().fg(palette::MUTED)),
                Span::styled(
                    "Ctrl+C",
                    Style::new()
                        .fg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Quit • ", Style::new().fg(palette::MUTED)),
                Span::styled("Esc", Style::new().fg(palette::ACCENT)),
                Span::styled(" Logged (no exit) • ", Style::new().fg(palette::MUTED)),
                Span::styled("Ctrl+Z", Style::new().fg(palette::ACCENT)),
                Span::styled(
                    " Event only (suspend unsupported) • ",
                    Style::new().fg(palette::MUTED),
                ),
                Span::styled("Mouse/Paste/Resize", Style::new().fg(palette::FG)),
                Span::styled(" Active", Style::new().fg(palette::SUCCESS)),
            ])
        };

        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(palette::SURFACE).fg(palette::FG)),
            area,
        );
    }

    fn format_hover_coords(&self) -> String {
        match self.hover_pos {
            Some((col, row)) => format!("({col},{row})"),
            None => "(-,-)".to_string(),
        }
    }

    fn format_drag_summary(&self) -> String {
        if let Some(drag) = self.active_drag {
            format!(
                "ACTIVE {} ({},{})->({},{}) [{}]",
                format_mouse_button(drag.button),
                drag.start.0,
                drag.start.1,
                drag.current.0,
                drag.current.1,
                drag.steps
            )
        } else if let Some(drag) = self.last_drag {
            format!(
                "idle | last {} ({},{})->({},{}) [{}]",
                format_mouse_button(drag.button),
                drag.start.0,
                drag.start.1,
                drag.current.0,
                drag.current.1,
                drag.steps
            )
        } else {
            "idle".to_string()
        }
    }

    fn format_sample_hit(&self) -> String {
        match &self.last_sample_hit {
            Some(hit) => format!(
                "col +{} -> g#{} \"{}\" cells {}..{} bytes {}..{} w={}",
                hit.rel_col,
                hit.cluster_index,
                hit.raw_escaped,
                hit.col_start,
                hit.col_end,
                hit.byte_start,
                hit.byte_end,
                hit.cell_width
            ),
            None => "none (click sample line)".to_string(),
        }
    }
}

fn is_quit_shortcut(key: KeyEvent) -> bool {
    key.modifiers == KeyModifiers::CONTROL
        && matches!(key.code, KeyCode::Char('q' | 'Q' | 'c' | 'C'))
}

fn is_ctrl_z(key: KeyEvent) -> bool {
    key.modifiers == KeyModifiers::CONTROL && matches!(key.code, KeyCode::Char('z' | 'Z'))
}

fn format_modifiers_prefix(mods: KeyModifiers) -> String {
    let mut parts = Vec::with_capacity(6);
    if mods.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if mods.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if mods.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }
    if mods.contains(KeyModifiers::SUPER) {
        parts.push("Super");
    }
    if mods.contains(KeyModifiers::HYPER) {
        parts.push("Hyper");
    }
    if mods.contains(KeyModifiers::META) {
        parts.push("Meta");
    }
    parts.join("+")
}

fn format_modifiers_suffix(mods: KeyModifiers) -> String {
    if mods.is_empty() {
        String::new()
    } else {
        format!(" [{}]", format_modifiers_prefix(mods))
    }
}

fn format_key_event(key: KeyEvent) -> String {
    let kind = match key.kind {
        KeyEventKind::Press => "Press",
        KeyEventKind::Repeat => "Repeat",
        KeyEventKind::Release => "Release",
    };

    let code_str = match key.code {
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(ch) => {
            if is_escaped_control_or_format(ch) || ch == '\\' {
                let mut escaped = String::with_capacity(8);
                push_escaped_char(&mut escaped, ch);
                format!("'{escaped}'")
            } else {
                format!("'{ch}'")
            }
        }
        KeyCode::Null => "Null".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::CapsLock => "CapsLock".to_string(),
        KeyCode::ScrollLock => "ScrollLock".to_string(),
        KeyCode::NumLock => "NumLock".to_string(),
        KeyCode::PrintScreen => "PrintScreen".to_string(),
        KeyCode::Pause => "Pause".to_string(),
        KeyCode::Menu => "Menu".to_string(),
        KeyCode::KeypadBegin => "KeypadBegin".to_string(),
        KeyCode::Media(media) => format!("Media({media})"),
        KeyCode::Modifier(modifier) => format!("Modifier({modifier})"),
    };

    if key.modifiers.is_empty() {
        format!("{kind} {code_str}")
    } else {
        format!(
            "{kind} {}+{code_str}",
            format_modifiers_prefix(key.modifiers)
        )
    }
}

const fn format_mouse_button(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "Left",
        MouseButton::Right => "Right",
        MouseButton::Middle => "Middle",
    }
}

const fn format_mouse_kind(kind: MouseEventKind) -> &'static str {
    match kind {
        MouseEventKind::Down(_) => "Down",
        MouseEventKind::Up(_) => "Up",
        MouseEventKind::Drag(_) => "Drag",
        MouseEventKind::Moved => "Moved",
        MouseEventKind::ScrollDown => "ScrollDown",
        MouseEventKind::ScrollUp => "ScrollUp",
        MouseEventKind::ScrollLeft => "ScrollLeft",
        MouseEventKind::ScrollRight => "ScrollRight",
    }
}

fn format_mouse_event(mouse: MouseEvent) -> String {
    let kind = match mouse.kind {
        MouseEventKind::Down(b) => format!("Down({})", format_mouse_button(b)),
        MouseEventKind::Up(b) => format!("Up({})", format_mouse_button(b)),
        MouseEventKind::Drag(b) => format!("Drag({})", format_mouse_button(b)),
        other => format_mouse_kind(other).to_string(),
    };
    format!(
        "{kind} @({},{}){}",
        mouse.column,
        mouse.row,
        format_modifiers_suffix(mouse.modifiers)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_osc_csi_c1_and_bidi_controls() {
        let osc8 = "\x1b]8;;https://evil.example\x07click\x1b]8;;\x07";
        let preview = bounded_escape_preview(osc8, 128, 512);
        assert_eq!(
            preview.text,
            "\\x1b]8;;https://evil.example\\x07click\\x1b]8;;\\x07"
        );
        assert_eq!(preview.escaped_controls, 4);
        assert!(!preview.truncated);
        assert!(
            preview
                .text
                .chars()
                .all(|c| !is_escaped_control_or_format(c))
        );

        let c1_and_bidi = "\u{009d}0;title\u{009c}\u{202e}abc\u{2028}d";
        let preview = bounded_escape_preview(c1_and_bidi, 128, 512);
        assert_eq!(
            preview.text,
            "\\u{009d}0;title\\u{009c}\\u{202e}abc\\u{2028}d"
        );
        assert_eq!(preview.escaped_controls, 4);
        assert!(
            preview
                .text
                .chars()
                .all(|c| !is_escaped_control_or_format(c))
        );
    }

    #[test]
    fn escaped_char_len_matches_push_escaped_char() {
        let samples = [
            '\0', '\x01', '\x07', '\t', '\n', '\r', '\x1b', '\\', '\x7f', '\u{0085}', '\u{009b}',
            '\u{009d}', '\u{200b}', '\u{2028}', '\u{202e}', '\u{feff}', 'a', 'é', '漢', '🦀',
        ];
        for ch in samples {
            let mut s = String::new();
            push_escaped_char(&mut s, ch);
            assert_eq!(s.len(), escaped_char_len(ch), "mismatch for {ch:?}");
        }
    }

    #[test]
    fn bounded_truncation_respects_grapheme_and_byte_limits() {
        let combining = "a\u{0301}b\u{0301}c\u{0301}";
        let p = bounded_escape_preview(combining, 2, 64);
        assert_eq!(p.text, "a\u{0301}b\u{0301}");
        assert!(p.truncated);
        assert_eq!(p.grapheme_count, 3);

        let zwj = "👩\u{200D}🔬🦀";
        let p = bounded_escape_preview(zwj, 1, 64);
        assert_eq!(p.text, "👩\u{200D}🔬");
        assert!(p.truncated);
        assert_eq!(p.grapheme_count, 2);

        // Byte budget stops cleanly before "漢" (4 + 3 = 7 > 6).
        let multi = "🦀漢a";
        let p = bounded_escape_preview(multi, 10, 6);
        assert_eq!(p.text, "🦀");
        assert!(p.truncated);
        assert!(p.text.len() <= 6);

        // Pathological single grapheme with thousands of combining marks still obeys max_bytes.
        let mut zalgo = String::from("a");
        for _ in 0..500 {
            zalgo.push('\u{0301}');
        }
        let p = bounded_escape_preview(&zalgo, 10, 32);
        assert!(p.truncated);
        assert_eq!(p.grapheme_count, 1);
        assert!(p.text.len() <= 32);
        assert!(!p.text.is_empty());

        // Ellipsis truncation also respects grapheme boundaries and max_bytes.
        let ell = escape_bounded_str(combining, 10, 6);
        assert_eq!(ell, "a\u{0301}…");
        assert!(ell.len() <= 6);
    }

    #[test]
    fn paste_containing_ctrl_q_and_osc_does_not_quit_and_stays_bounded() {
        let mut app = ProbeApp::new((80, 24));
        let mut huge = String::from("\x11\x03\x1b]52;c;AAAA\x07");
        huge.push_str(&"x".repeat(50_000));

        let redraw = app.handle_event(Event::Paste(huge), Instant::now());
        assert!(redraw);
        assert!(!app.should_quit);
        assert_eq!(app.paste_events, 1);
        let preview = app.last_paste.as_ref().expect("paste recorded");
        assert!(preview.truncated);
        assert!(preview.text.len() <= MAX_PASTE_PREVIEW_BYTES);
        assert!(preview.text.starts_with("\\x11\\x03\\x1b]52;c;AAAA\\x07"));
        assert!(preview.escaped_controls >= 4);
    }

    #[test]
    fn key_press_and_repeat_accepted_release_ignored_and_ctrl_z_captured() {
        let mut app = ProbeApp::new((80, 24));
        let t0 = Instant::now();

        assert!(!app.handle_event(
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('a'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            )),
            t0,
        ));
        assert_eq!(app.key_events, 0);

        assert!(app.handle_event(
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            )),
            t0,
        ));
        assert!(!app.should_quit);
        assert_eq!(app.key_events, 1);

        assert!(app.handle_event(
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('a'),
                KeyModifiers::NONE,
                KeyEventKind::Repeat,
            )),
            t0,
        ));
        assert_eq!(app.key_events, 2);
        assert_eq!(app.key_repeats, 1);

        assert!(app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            t0,
        ));
        assert!(!app.should_quit);

        assert!(app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            t0,
        ));
        assert!(app.should_quit);
    }

    #[test]
    fn double_click_detection_and_sample_hit_mapping() {
        let mut app = ProbeApp::new((80, 24));
        app.sample_rect = Some(Rect::new(2, 10, app.sample.total_width, 1));
        let t0 = Instant::now();

        let click = |col, row| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: col,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };

        // First click on second cell of first tab (col_start=1, width=3 -> cols 1..4).
        app.handle_event(click(4, 10), t0);
        assert_eq!(app.double_clicks, 0);
        let hit = app.last_sample_hit.as_ref().expect("sample hit");
        assert_eq!(hit.raw_escaped, "\\t");
        assert_eq!((hit.col_start, hit.col_end), (1, 4));

        // Second click on same cell within window triggers double-click.
        app.handle_event(click(4, 10), t0 + Duration::from_millis(200));
        assert_eq!(app.double_clicks, 1);

        // Quick 3rd click starts a new streak rather than double-counting.
        app.handle_event(click(4, 10), t0 + Duration::from_millis(300));
        assert_eq!(app.double_clicks, 1);

        // Slow click beyond window does not trigger double-click.
        app.handle_event(click(4, 10), t0 + Duration::from_millis(900));
        assert_eq!(app.double_clicks, 1);

        // Verify wide CJK char hit from either of its two cells.
        let cjk = app
            .sample
            .clusters
            .iter()
            .find(|c| c.raw == "漢")
            .expect("CJK cluster present");
        assert_eq!(cjk.cell_width, 2);
        let second_cell_col = 2 + cjk.col_start + 1;
        app.handle_event(click(second_cell_col, 10), t0 + Duration::from_secs(2));
        let hit = app.last_sample_hit.as_ref().expect("CJK sample hit");
        assert_eq!(hit.raw_escaped, "漢");
    }

    #[test]
    fn resize_invalidates_sample_target_before_next_render() {
        let mut app = ProbeApp::new((120, 36));
        app.sample_rect = Some(Rect::new(2, 10, app.sample.total_width, 1));
        let now = Instant::now();
        app.handle_event(Event::Resize(50, 16), now);
        app.handle_event(
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row: 10,
                modifiers: KeyModifiers::NONE,
            }),
            now,
        );
        assert!(app.sample_rect.is_none());
        assert!(app.last_sample_hit.is_none());
    }

    #[test]
    fn drag_does_not_seed_a_later_double_click() {
        let mut app = ProbeApp::new((80, 24));
        let now = Instant::now();
        for (kind, column, millis) in [
            (MouseEventKind::Down(MouseButton::Left), 4, 0),
            (MouseEventKind::Drag(MouseButton::Left), 8, 50),
            (MouseEventKind::Up(MouseButton::Left), 8, 100),
            (MouseEventKind::Down(MouseButton::Left), 4, 200),
            (MouseEventKind::Up(MouseButton::Left), 4, 220),
        ] {
            app.handle_event(
                Event::Mouse(MouseEvent {
                    kind,
                    column,
                    row: 10,
                    modifiers: KeyModifiers::NONE,
                }),
                now + Duration::from_millis(millis),
            );
        }
        assert_eq!(app.double_clicks, 0);
        assert_eq!(app.mouse_drag, 1);
        assert_eq!(app.last_drag.unwrap().current, (8, 10));
    }
}

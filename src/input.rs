//! Bounded streaming terminal input decoder.
//!
//! Replaces crossterm's event reader thread with a bounded, single-threaded ingress
//! parser on Linux using `rustix::event::poll` and bounded reads.
//!
//! # Supported Protocols
//! - Standard ASCII and C0 control characters (Ctrl+A..Ctrl+Z, Backspace, Tab, Enter, Esc).
//! - Multi-byte UTF-8 grapheme and code point input.
//! - Standard ANSI / xterm CSI escape sequences:
//!   - Standard arrows (Up, Down, Right, Left) and modified arrows (Shift, Ctrl, Alt).
//!   - Home, End, PageUp, PageDown, Insert, Delete, BackTab (Shift+Tab).
//!   - Function keys F1–F12 (SS3 and CSI encodings).
//! - SGR 1006 mouse protocol:
//!   - Press, release, drag, move.
//!   - Left, middle, right buttons.
//!   - 4-direction scroll wheel (up, down, left, right).
//!   - Shift, Alt, Control modifiers.
//!   - Strict 16-bit coordinate validation preventing overflow.
//! - Bracketed paste mode (2004):
//!   - Delimited by `\x1b[200~` and `\x1b[201~`.
//!   - Bounded at 1 MiB payload.
//!   - Overflow drops entire payload and drains to delimiter with at most 6 delimiter bytes retained.
//!   - Control characters inside paste (e.g. Ctrl+Q) are never interpreted as commands.
//!   - Invalid UTF-8 yields `InputEvent::PasteRejected`.
//!
//! # Explicitly Unsupported / Ignored Protocols
//! - Kitty keyboard protocol (no flags negotiation or progressive enhancement).
//! - Windows Console API / ConPTY sequences.
//! - OSC sequences (OSC 52 clipboard reads, color queries, etc.).
//! - Legacy non-SGR mouse tracking (X10 / normal mouse mode).
//! - SIGWINCH parser (window resize is queried via ioctl on terminal timeout).

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rustix::event::{PollFd, PollFlags, Timespec, poll as rustix_poll};
use rustix::io::{Errno, read as rustix_read};
use rustix::stdio::stdin;

/// Maximum payload size for bracketed paste (1 MiB).
pub const MAX_PASTE_BYTES: usize = 1024 * 1024;

/// Maximum length of a CSI sequence before treating as overlong and discarding.
const MAX_CSI_BYTES: usize = 64;

/// Bracketed paste terminator: `\x1b[201~` (6 bytes).
const PASTE_END_DELIMITER: &[u8; 6] = b"\x1b[201~";

/// High-level event produced by bounded input ingress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    /// Standard crossterm terminal event (key, mouse, paste).
    Terminal(Event),
    /// A bracketed paste that was rejected because it exceeded 1 MiB or contained invalid UTF-8.
    PasteRejected,
}

/// Linux single-threaded bounded input reader.
pub struct Input {
    decoder: StreamingDecoder,
    read_buf: [u8; 4096],
    last_read: Instant,
}

impl Input {
    /// Creates a new infallible input reader.
    pub fn new() -> Self {
        Self {
            decoder: StreamingDecoder::new(),
            read_buf: [0u8; 4096],
            last_read: Instant::now(),
        }
    }

    /// Polls stdin for terminal events with a specified timeout.
    ///
    /// Uses `rustix::event::poll` to wait for readiness on stdin, then performs at most
    /// one bounded read up to 4096 bytes. Avoids mutating stdin fd flags.
    ///
    /// - Returns `Ok(Some(event))` when an event is ready.
    /// - Returns `Ok(None)` on timeout or `EINTR`.
    /// - Returns `Err(UnexpectedEof)` on stdin EOF/hangup.
    pub fn poll(&mut self, timeout: Duration) -> io::Result<Option<InputEvent>> {
        // 1. Check if the decoder already has an event ready to yield from previously buffered bytes.
        if let Some(event) = self.decoder.next_event() {
            return Ok(Some(event));
        }

        // 2. Poll stdin readiness.
        let stdin_fd = stdin();
        let mut poll_fds = [PollFd::new(
            &stdin_fd,
            PollFlags::IN | PollFlags::HUP | PollFlags::ERR,
        )];

        let timeout_value = Timespec {
            tv_sec: timeout.as_secs().min(i64::MAX as u64) as i64,
            tv_nsec: i64::from(timeout.subsec_nanos()),
        };

        match rustix_poll(&mut poll_fds, Some(&timeout_value)) {
            Err(Errno::INTR) => Ok(None),
            Err(err) => Err(io::Error::from(err)),
            Ok(0) => {
                // Timeout elapsed with no new data.
                // Flush pending lone Escape key if present after idle.
                if self.last_read.elapsed() >= Duration::from_millis(30) {
                    Ok(self.decoder.flush_escape())
                } else {
                    Ok(None)
                }
            }
            Ok(_) => {
                // Stdin is ready for reading.
                match rustix_read(&stdin_fd, &mut self.read_buf) {
                    Ok(0) => Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "stdin reached EOF",
                    )),
                    Ok(n) => {
                        self.last_read = Instant::now();
                        self.decoder.feed(&self.read_buf[..n]);
                        Ok(self.decoder.next_event())
                    }
                    Err(Errno::INTR) => Ok(None),
                    Err(Errno::AGAIN) => Ok(None),
                    Err(err) => Err(io::Error::from(err)),
                }
            }
        }
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure streaming raw terminal decoder.
///
/// Factored privately for deterministic unit testing without requiring OS or mock stdin.
#[derive(Debug)]
pub(crate) struct StreamingDecoder {
    buffer: Vec<u8>,
    cursor: usize,
    in_paste: bool,
    paste_payload: Vec<u8>,
    paste_delim_matched: usize,
    paste_rejected: bool,
    discarding_overlong_csi: bool,
    discarding_string: bool,
    string_escape: bool,
}

impl StreamingDecoder {
    pub(crate) fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
            cursor: 0,
            in_paste: false,
            paste_payload: Vec::new(),
            paste_delim_matched: 0,
            paste_rejected: false,
            discarding_overlong_csi: false,
            discarding_string: false,
            string_escape: false,
        }
    }

    /// Ingests raw bytes from stdin.
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        if self.cursor > 0 {
            self.buffer.drain(..self.cursor);
            self.cursor = 0;
        }
        self.buffer.extend_from_slice(bytes);
    }

    /// Flushes any pending lone escape character after an idle timeout.
    pub(crate) fn flush_escape(&mut self) -> Option<InputEvent> {
        if self.in_paste {
            return None;
        }
        if self.cursor < self.buffer.len() {
            let remaining = &self.buffer[self.cursor..];
            if remaining == [0x1B] && !self.discarding_overlong_csi {
                self.buffer.clear();
                self.cursor = 0;
                return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                    KeyCode::Esc,
                    KeyModifiers::NONE,
                ))));
            }
        }
        None
    }

    /// Parses and produces the next available input event from buffered bytes.
    pub(crate) fn next_event(&mut self) -> Option<InputEvent> {
        loop {
            if self.discarding_string {
                while self.cursor < self.buffer.len() {
                    let byte = self.buffer[self.cursor];
                    self.cursor += 1;
                    if byte == 0x07 || (self.string_escape && byte == b'\\') {
                        self.discarding_string = false;
                        self.string_escape = false;
                        break;
                    }
                    self.string_escape = byte == 0x1b;
                }
                if self.discarding_string {
                    self.buffer.clear();
                    self.cursor = 0;
                    return None;
                }
            }
            if self.discarding_overlong_csi {
                while self.cursor < self.buffer.len() {
                    let b = self.buffer[self.cursor];
                    self.cursor += 1;
                    if b == 0x1b {
                        self.cursor -= 1;
                        self.discarding_overlong_csi = false;
                        break;
                    }
                    if (0x40..=0x7E).contains(&b) {
                        self.discarding_overlong_csi = false;
                        break;
                    }
                }
                if self.discarding_overlong_csi {
                    self.buffer.clear();
                    self.cursor = 0;
                    return None;
                }
            }

            if self.in_paste {
                if let Some(event) = self.process_paste_bytes() {
                    return Some(event);
                }
                if self.cursor >= self.buffer.len() {
                    self.buffer.clear();
                    self.cursor = 0;
                    return None;
                }
            } else {
                if self.cursor >= self.buffer.len() {
                    self.buffer.clear();
                    self.cursor = 0;
                    return None;
                }

                let b = self.buffer[self.cursor];

                // 1. Escape sequences
                if b == 0x1B {
                    match self.process_escape_sequence() {
                        EscapeParseResult::Event(event) => return Some(event),
                        EscapeParseResult::Incomplete => return None,
                        EscapeParseResult::EnterPaste => {
                            self.in_paste = true;
                            self.paste_payload.clear();
                            self.paste_delim_matched = 0;
                            self.paste_rejected = false;
                            continue;
                        }
                        EscapeParseResult::Discarded => continue,
                    }
                }

                // 2. Control characters (ASCII 0x00..=0x1F and 0x7F)
                if b == 0x7F || b == 0x08 {
                    self.cursor += 1;
                    return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                        KeyCode::Backspace,
                        KeyModifiers::NONE,
                    ))));
                }

                if b == 0x09 {
                    self.cursor += 1;
                    return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                        KeyCode::Tab,
                        KeyModifiers::NONE,
                    ))));
                }

                if b == 0x0A || b == 0x0D {
                    self.cursor += 1;
                    return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                        KeyCode::Enter,
                        KeyModifiers::NONE,
                    ))));
                }

                if b == 0x00 {
                    self.cursor += 1;
                    return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                        KeyCode::Char(' '),
                        KeyModifiers::CONTROL,
                    ))));
                }

                if (0x01..=0x1A).contains(&b) {
                    self.cursor += 1;
                    let ch = (b'a' + (b - 1)) as char;
                    return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                        KeyCode::Char(ch),
                        KeyModifiers::CONTROL,
                    ))));
                }

                if (0x1C..=0x1F).contains(&b) {
                    self.cursor += 1;
                    let ch = match b {
                        0x1C => '\\',
                        0x1D => ']',
                        0x1E => '^',
                        0x1F => '_',
                        _ => unreachable!(),
                    };
                    return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                        KeyCode::Char(ch),
                        KeyModifiers::CONTROL,
                    ))));
                }

                // 3. Printable ASCII characters (0x20..=0x7E)
                if (0x20..=0x7E).contains(&b) {
                    self.cursor += 1;
                    return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                        KeyCode::Char(b as char),
                        KeyModifiers::NONE,
                    ))));
                }

                // 4. Multi-byte UTF-8 sequences (0x80..=0xFF)
                let need = match b {
                    0xC2..=0xDF => 2,
                    0xE0..=0xEF => 3,
                    0xF0..=0xF4 => 4,
                    _ => {
                        // Invalid lead byte; discard safely and resume next byte
                        self.cursor += 1;
                        continue;
                    }
                };

                let available = need.min(self.buffer.len() - self.cursor);
                if let Err(error) =
                    std::str::from_utf8(&self.buffer[self.cursor..self.cursor + available])
                    && error.error_len().is_some()
                {
                    self.cursor += 1;
                    continue;
                }
                if self.cursor + need > self.buffer.len() {
                    // Incomplete UTF-8 sequence, await more bytes
                    return None;
                }

                let slice = &self.buffer[self.cursor..self.cursor + need];
                match std::str::from_utf8(slice) {
                    Ok(s) => {
                        let ch = s.chars().next().expect("non-empty valid UTF-8");
                        self.cursor += need;
                        return Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                            KeyCode::Char(ch),
                            KeyModifiers::NONE,
                        ))));
                    }
                    Err(_) => {
                        // Invalid continuation sequence; discard first byte and resume
                        self.cursor += 1;
                        continue;
                    }
                }
            }
        }
    }

    /// Consumes buffered bytes while inside bracketed paste mode.
    fn process_paste_bytes(&mut self) -> Option<InputEvent> {
        while self.cursor < self.buffer.len() {
            let b = self.buffer[self.cursor];
            self.cursor += 1;

            if b == PASTE_END_DELIMITER[self.paste_delim_matched] {
                self.paste_delim_matched += 1;
                if self.paste_delim_matched == PASTE_END_DELIMITER.len() {
                    // Completed `\x1b[201~` terminator!
                    self.in_paste = false;
                    self.paste_delim_matched = 0;

                    if self.paste_rejected {
                        self.paste_payload.clear();
                        self.paste_payload.shrink_to_fit();
                        self.paste_rejected = false;
                        return Some(InputEvent::PasteRejected);
                    }

                    let payload = std::mem::take(&mut self.paste_payload);
                    self.paste_rejected = false;

                    return match String::from_utf8(payload) {
                        Ok(text) => Some(InputEvent::Terminal(Event::Paste(text))),
                        Err(_) => Some(InputEvent::PasteRejected),
                    };
                }
            } else {
                // Delimiter mismatch: candidate bytes in PASTE_END_DELIMITER[..matched] were payload
                let candidate_prefix = &PASTE_END_DELIMITER[..self.paste_delim_matched];
                if !self.paste_rejected {
                    if self.paste_payload.len() + candidate_prefix.len() > MAX_PASTE_BYTES {
                        self.paste_rejected = true;
                        self.paste_payload.clear();
                        self.paste_payload.shrink_to_fit();
                    } else {
                        self.paste_payload.extend_from_slice(candidate_prefix);
                    }
                }

                if b == 0x1B {
                    self.paste_delim_matched = 1;
                } else {
                    self.paste_delim_matched = 0;
                    if !self.paste_rejected {
                        if self.paste_payload.len() + 1 > MAX_PASTE_BYTES {
                            self.paste_rejected = true;
                            self.paste_payload.clear();
                            self.paste_payload.shrink_to_fit();
                        } else {
                            self.paste_payload.push(b);
                        }
                    }
                }
            }
        }
        None
    }

    /// Handles escape sequence starting at `self.cursor`.
    fn process_escape_sequence(&mut self) -> EscapeParseResult {
        if self.cursor + 1 >= self.buffer.len() {
            // Lone ESC at end of buffer; wait for next chunk or idle flush
            return EscapeParseResult::Incomplete;
        }

        let b2 = self.buffer[self.cursor + 1];
        if matches!(b2, b']' | b'P' | b'^' | b'_' | b'X') {
            self.cursor += 2;
            self.discarding_string = true;
            self.string_escape = false;
            return EscapeParseResult::Discarded;
        }

        if b2 == b'[' {
            // CSI sequence
            let start = self.cursor;
            let mut final_idx = None;

            for i in (start + 2)..self.buffer.len() {
                let len = i - start + 1;
                if len > MAX_CSI_BYTES {
                    // Overlong sequence: discard safely until CSI final byte
                    self.discarding_overlong_csi = true;
                    self.cursor = i;
                    return EscapeParseResult::Discarded;
                }
                let c = self.buffer[i];
                if c == 0x1b {
                    self.cursor = i;
                    return EscapeParseResult::Discarded;
                }
                if i == start + 2 && c == b'[' {
                    continue; // Linux console F1..F5 prefix, not a CSI final.
                }
                if (0x40..=0x7E).contains(&c) {
                    final_idx = Some(i);
                    break;
                }
            }

            let Some(end_idx) = final_idx else {
                if self.buffer.len() - start >= MAX_CSI_BYTES {
                    self.discarding_overlong_csi = true;
                    self.cursor = self.buffer.len();
                    return EscapeParseResult::Discarded;
                }
                return EscapeParseResult::Incomplete;
            };

            let csi_bytes = &self.buffer[start..=end_idx];
            self.cursor = end_idx + 1;

            if let Some(res) = parse_csi(csi_bytes) {
                res
            } else {
                // Unknown CSI discarded safely
                EscapeParseResult::Discarded
            }
        } else if b2 == b'O' {
            // SS3 sequence (\x1bO <char>)
            if self.cursor + 2 >= self.buffer.len() {
                return EscapeParseResult::Incomplete;
            }
            let c = self.buffer[self.cursor + 2];
            self.cursor += 3;
            if let Some(event) = parse_ss3(c) {
                EscapeParseResult::Event(event)
            } else {
                EscapeParseResult::Discarded
            }
        } else if b2 == 0x1B {
            // Double escape: emit first ESC
            self.cursor += 1;
            EscapeParseResult::Event(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            ))))
        } else if (0x20..=0x7E).contains(&b2) {
            // Alt + character
            self.cursor += 2;
            EscapeParseResult::Event(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(b2 as char),
                KeyModifiers::ALT,
            ))))
        } else if b2 == 0x7F || b2 == 0x08 {
            self.cursor += 2;
            EscapeParseResult::Event(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Backspace,
                KeyModifiers::ALT,
            ))))
        } else if b2 == 0x0A || b2 == 0x0D {
            self.cursor += 2;
            EscapeParseResult::Event(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::ALT,
            ))))
        } else if b2 == 0x09 {
            self.cursor += 2;
            EscapeParseResult::Event(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Tab,
                KeyModifiers::ALT,
            ))))
        } else if (0x01..=0x1A).contains(&b2) {
            self.cursor += 2;
            let ch = (b'a' + (b2 - 1)) as char;
            EscapeParseResult::Event(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::ALT | KeyModifiers::CONTROL,
            ))))
        } else {
            // Unknown escape byte: discard
            self.cursor += 2;
            EscapeParseResult::Discarded
        }
    }
}

enum EscapeParseResult {
    Event(InputEvent),
    Incomplete,
    EnterPaste,
    Discarded,
}

/// Parses a complete CSI sequence (starting with `\x1b[` and ending with `0x40..=0x7E`).
fn parse_csi(csi_bytes: &[u8]) -> Option<EscapeParseResult> {
    if csi_bytes.len() < 3 {
        return None;
    }

    let final_byte = csi_bytes[csi_bytes.len() - 1];
    let body = &csi_bytes[2..csi_bytes.len() - 1];

    // 1. SGR Mouse: `\x1b[< Cb ; Cx ; Cy (M|m)`
    if body.starts_with(b"<") {
        return parse_sgr_mouse(body, final_byte)
            .map(|ev| EscapeParseResult::Event(InputEvent::Terminal(ev)));
    }

    // 2. Bracketed paste markers: `\x1b[200~` and `\x1b[201~`
    if final_byte == b'~' && body == b"200" {
        return Some(EscapeParseResult::EnterPaste);
    }
    if final_byte == b'~' && body == b"201" {
        return Some(EscapeParseResult::Discarded);
    }

    // 3. Linux console / VT F1..F5: `\x1b[[A` .. `\x1b[[E`
    if body == b"[" {
        let key_code = match final_byte {
            b'A' => KeyCode::F(1),
            b'B' => KeyCode::F(2),
            b'C' => KeyCode::F(3),
            b'D' => KeyCode::F(4),
            b'E' => KeyCode::F(5),
            _ => return None,
        };
        return Some(EscapeParseResult::Event(InputEvent::Terminal(Event::Key(
            KeyEvent::new(key_code, KeyModifiers::NONE),
        ))));
    }

    // 4. BackTab: `\x1b[Z`
    if final_byte == b'Z' && body.is_empty() {
        return Some(EscapeParseResult::Event(InputEvent::Terminal(Event::Key(
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        ))));
    }

    // 5. Numeric parameters parsing on the stack (at most 4 params)
    let (params, count) = parse_csi_params_stack(body)?;

    // 6. Arrow and Home/End keys: `A`, `B`, `C`, `D`, `H`, `F`
    match final_byte {
        b'A' | b'B' | b'C' | b'D' | b'H' | b'F' => {
            let code = match final_byte {
                b'A' => KeyCode::Up,
                b'B' => KeyCode::Down,
                b'C' => KeyCode::Right,
                b'D' => KeyCode::Left,
                b'H' => KeyCode::Home,
                b'F' => KeyCode::End,
                _ => unreachable!(),
            };
            let modifiers = if count >= 2 {
                modifier_from_param(params[1])
            } else if count == 1 && params[0] > 1 {
                modifier_from_param(params[0])
            } else {
                KeyModifiers::NONE
            };
            return Some(EscapeParseResult::Event(InputEvent::Terminal(Event::Key(
                KeyEvent::new(code, modifiers),
            ))));
        }
        b'P' | b'Q' | b'R' | b'S' if count >= 1 => {
            let f_num = match final_byte {
                b'P' => 1,
                b'Q' => 2,
                b'R' => 3,
                b'S' => 4,
                _ => unreachable!(),
            };
            let modifiers = if count >= 2 {
                modifier_from_param(params[1])
            } else {
                KeyModifiers::NONE
            };
            return Some(EscapeParseResult::Event(InputEvent::Terminal(Event::Key(
                KeyEvent::new(KeyCode::F(f_num), modifiers),
            ))));
        }
        b'~' => {
            if count == 0 {
                return None;
            }
            let code_num = params[0];
            let modifiers = if count >= 2 {
                modifier_from_param(params[1])
            } else {
                KeyModifiers::NONE
            };
            let key_code = match code_num {
                1 => KeyCode::Home,
                2 => KeyCode::Insert,
                3 => KeyCode::Delete,
                4 => KeyCode::End,
                5 => KeyCode::PageUp,
                6 => KeyCode::PageDown,
                7 => KeyCode::Home,
                8 => KeyCode::End,
                11..=15 => KeyCode::F((code_num - 10) as u8),
                17..=21 => KeyCode::F((code_num - 11) as u8),
                23..=24 => KeyCode::F((code_num - 12) as u8),
                _ => return None,
            };
            return Some(EscapeParseResult::Event(InputEvent::Terminal(Event::Key(
                KeyEvent::new(key_code, modifiers),
            ))));
        }
        _ => {}
    }

    None
}

/// Parses SS3 single-character sequence (`\x1bO <char>`).
fn parse_ss3(c: u8) -> Option<InputEvent> {
    let code = match c {
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'M' => KeyCode::Enter,
        _ => return None,
    };
    Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    ))))
}

/// Parses SGR mouse sequence: `body` starts with `<` followed by `Cb;Cx;Cy` and ends with `M` or `m`.
fn parse_sgr_mouse(body: &[u8], final_byte: u8) -> Option<Event> {
    if !body.starts_with(b"<") || (final_byte != b'M' && final_byte != b'm') {
        return None;
    }
    let inner = &body[1..];
    let (params, count) = parse_csi_params_stack(inner)?;
    if count != 3 {
        return None;
    }
    let cb = params[0];
    let cx = params[1];
    let cy = params[2];

    // Coordinate validation and 16-bit overflow prevention (terminal coordinates are 1-based)
    if cx == 0 || cy == 0 || cx > u16::MAX as u32 || cy > u16::MAX as u32 {
        return None;
    }
    let column = (cx - 1) as u16;
    let row = (cy - 1) as u16;

    let mut modifiers = KeyModifiers::empty();
    if cb & 4 != 0 {
        modifiers |= KeyModifiers::SHIFT;
    }
    if cb & 8 != 0 {
        modifiers |= KeyModifiers::ALT;
    }
    if cb & 16 != 0 {
        modifiers |= KeyModifiers::CONTROL;
    }

    let kind = if cb & 64 != 0 {
        // Wheel events
        match cb & 3 {
            0 => MouseEventKind::ScrollUp,
            1 => MouseEventKind::ScrollDown,
            2 => MouseEventKind::ScrollLeft,
            3 => MouseEventKind::ScrollRight,
            _ => return None,
        }
    } else if cb & 32 != 0 {
        // Motion / Drag
        match cb & 3 {
            0 => MouseEventKind::Drag(MouseButton::Left),
            1 => MouseEventKind::Drag(MouseButton::Middle),
            2 => MouseEventKind::Drag(MouseButton::Right),
            3 => MouseEventKind::Moved,
            _ => return None,
        }
    } else if final_byte == b'm' {
        // Release (Up)
        match cb & 3 {
            0 => MouseEventKind::Up(MouseButton::Left),
            1 => MouseEventKind::Up(MouseButton::Middle),
            2 => MouseEventKind::Up(MouseButton::Right),
            _ => return None,
        }
    } else if final_byte == b'M' {
        // Press (Down)
        match cb & 3 {
            0 => MouseEventKind::Down(MouseButton::Left),
            1 => MouseEventKind::Down(MouseButton::Middle),
            2 => MouseEventKind::Down(MouseButton::Right),
            _ => return None,
        }
    } else {
        return None;
    };

    Some(Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    }))
}

/// Parses up to 4 semicolon-separated numeric parameters on the stack without heap allocation.
fn parse_csi_params_stack(body: &[u8]) -> Option<([u32; 4], usize)> {
    if body.is_empty() {
        return Some(([0; 4], 0));
    }
    let mut params = [0u32; 4];
    let mut count = 0;
    let mut current: u32 = 0;
    let mut has_digit = false;

    for &b in body {
        if b.is_ascii_digit() {
            has_digit = true;
            current = current.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        } else if b == b';' {
            if count >= 4 {
                return None;
            }
            params[count] = current;
            count += 1;
            current = 0;
            has_digit = false;
        } else {
            return None;
        }
    }

    if has_digit || count > 0 {
        if count >= 4 {
            return None;
        }
        params[count] = current;
        count += 1;
    }

    Some((params, count))
}

/// Decodes standard xterm modifier parameter `m` (where `m = 1 + modifiers`).
fn modifier_from_param(param: u32) -> KeyModifiers {
    let mut mods = KeyModifiers::empty();
    let val = param.saturating_sub(1);
    if val & 1 != 0 {
        mods |= KeyModifiers::SHIFT;
    }
    if val & 2 != 0 {
        mods |= KeyModifiers::ALT;
    }
    if val & 4 != 0 {
        mods |= KeyModifiers::CONTROL;
    }
    mods
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_and_c0_keys() {
        let mut dec = StreamingDecoder::new();
        dec.feed(b"aZ 1\x7F\x08\t\r\n");

        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('a'),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('Z'),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(' '),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('1'),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Backspace,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Backspace,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Tab,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(dec.next_event(), None);
    }

    #[test]
    fn test_ctrl_keys() {
        let mut dec = StreamingDecoder::new();
        dec.feed(b"\x01\x11\x03\x13\x1A\x00");

        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('a'),
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('q'),
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('s'),
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char(' '),
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(dec.next_event(), None);
    }

    #[test]
    fn test_standard_and_modified_arrows_and_keys() {
        let mut dec = StreamingDecoder::new();
        dec.feed(b"\x1b[A\x1b[B\x1b[C\x1b[D");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Up,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Down,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::NONE
            ))))
        );

        // Shift selection and Ctrl modified arrows
        dec.feed(b"\x1b[1;2C\x1b[1;5D\x1b[1;6A\x1b[3;5~\x1b[Z");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::SHIFT
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Up,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Delete,
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::BackTab,
                KeyModifiers::SHIFT
            ))))
        );
    }

    #[test]
    fn test_function_keys() {
        let mut dec = StreamingDecoder::new();
        dec.feed(b"\x1bOP\x1bOS\x1b[15~\x1b[24~");

        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::F(1),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::F(4),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::F(5),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::F(12),
                KeyModifiers::NONE
            ))))
        );
    }

    #[test]
    fn test_fragmented_csi_and_utf8() {
        let mut dec = StreamingDecoder::new();

        // Fragmented CSI: `\x1b[1;5C`
        dec.feed(b"\x1b");
        assert_eq!(dec.next_event(), None);
        dec.feed(b"[1");
        assert_eq!(dec.next_event(), None);
        dec.feed(b";5");
        assert_eq!(dec.next_event(), None);
        dec.feed(b"C");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::CONTROL
            ))))
        );

        // Fragmented 3-byte UTF-8: '€' (0xE2 0x82 0xAC)
        dec.feed(b"\xE2\x82");
        assert_eq!(dec.next_event(), None);
        dec.feed(b"\xAC");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('€'),
                KeyModifiers::NONE
            ))))
        );

        // Fragmented 4-byte UTF-8: '🦀' (0xF0 0x9F 0xA6 0x80)
        dec.feed(b"\xF0");
        assert_eq!(dec.next_event(), None);
        dec.feed(b"\x9F\xA6");
        assert_eq!(dec.next_event(), None);
        dec.feed(b"\x80");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('🦀'),
                KeyModifiers::NONE
            ))))
        );
    }

    #[test]
    fn test_resumed_valid_keys_after_bad_utf8() {
        let mut dec = StreamingDecoder::new();
        // Feed bad continuation and invalid UTF-8 bytes, followed by valid key 'x'
        dec.feed(b"\xFF\x80\xC0x");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE
            ))))
        );
    }

    #[test]
    fn test_partial_escape_flush_and_unknown_csi() {
        let mut dec = StreamingDecoder::new();

        // Lone ESC flushed on idle timeout
        dec.feed(b"\x1b");
        assert_eq!(dec.next_event(), None);
        assert_eq!(
            dec.flush_escape(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE
            ))))
        );

        // Unknown CSI safely discarded without injecting as text
        dec.feed(b"\x1b[999;888zhello");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('h'),
                KeyModifiers::NONE
            ))))
        );

        // Overlong CSI (>64 bytes) discarded safely without injection
        let mut dec = StreamingDecoder::new();
        dec.feed(b"\x1b[");
        dec.feed(&vec![b'9'; 70]);
        dec.feed(b"~world");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('w'),
                KeyModifiers::NONE
            ))))
        );

        // Overlong CSI fragmented across separate feed calls
        let mut dec = StreamingDecoder::new();
        dec.feed(b"\x1b[");
        dec.feed(&vec![b'8'; 80]);
        assert_eq!(dec.next_event(), None);
        dec.feed(&vec![b'8'; 80]);
        dec.feed(b"~resumed");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('r'),
                KeyModifiers::NONE
            ))))
        );
    }

    #[test]
    fn test_bracketed_paste_fragmented_and_normal() {
        let mut dec = StreamingDecoder::new();
        dec.feed(b"\x1b[200~Hello, ");
        assert_eq!(dec.next_event(), None);
        dec.feed(b"world!\x1b[20");
        assert_eq!(dec.next_event(), None);
        dec.feed(b"1~");

        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Paste("Hello, world!".into())))
        );
        assert_eq!(dec.next_event(), None);
    }

    #[test]
    fn test_paste_payload_limit_overflow_and_drain_then_ctrl_q() {
        let mut dec = StreamingDecoder::new();

        // Enter paste
        dec.feed(b"\x1b[200~");
        assert_eq!(dec.next_event(), None);

        // Feed oversize payload exceeding 1 MiB
        let oversize = vec![b'A'; MAX_PASTE_BYTES + 1024];
        dec.feed(&oversize);
        assert_eq!(dec.next_event(), None);

        // Embedded Ctrl+Q (0x11) inside paste is not executed as command!
        dec.feed(b"some more text with \x11\x1b[20");
        assert_eq!(dec.next_event(), None);

        // End delimiter arrives, immediately followed by real Ctrl+Q outside paste
        dec.feed(b"1~\x11");

        // First event is PasteRejected
        assert_eq!(dec.next_event(), Some(InputEvent::PasteRejected));

        // Subsequent event is genuine Ctrl+Q command
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('q'),
                KeyModifiers::CONTROL
            ))))
        );
        assert_eq!(dec.next_event(), None);
    }

    #[test]
    fn test_invalid_utf8_paste() {
        let mut dec = StreamingDecoder::new();
        dec.feed(b"\x1b[200~Valid prefix \xFF\xFE invalid\x1b[201~");

        assert_eq!(dec.next_event(), Some(InputEvent::PasteRejected));
        assert_eq!(dec.next_event(), None);
    }

    #[test]
    fn test_sgr_mouse_events_and_coordinate_overflow() {
        let mut dec = StreamingDecoder::new();

        // Left press at col 10, row 20 (1-based: 11, 21)
        dec.feed(b"\x1b[<0;11;21M");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 20,
                modifiers: KeyModifiers::NONE,
            })))
        );

        // Left release
        dec.feed(b"\x1b[<0;11;21m");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 10,
                row: 20,
                modifiers: KeyModifiers::NONE,
            })))
        );

        // Left drag with Shift
        dec.feed(b"\x1b[<36;15;25M");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 14,
                row: 24,
                modifiers: KeyModifiers::SHIFT,
            })))
        );

        // Mouse motion without button held
        dec.feed(b"\x1b[<35;5;5M");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: 4,
                row: 4,
                modifiers: KeyModifiers::NONE,
            })))
        );

        // 4 wheel directions
        dec.feed(b"\x1b[<64;1;1M\x1b[<65;1;1M\x1b[<66;1;1M\x1b[<67;1;1M");
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            })))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            })))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollLeft,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            })))
        );
        assert_eq!(
            dec.next_event(),
            Some(InputEvent::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollRight,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            })))
        );

        // Coordinate overflow check: cx > u16::MAX or cy == 0
        dec.feed(b"\x1b[<0;70000;1M\x1b[<0;0;10M");
        assert_eq!(dec.next_event(), None);
    }

    #[test]
    fn idle_partial_paste_prefix_stays_data() {
        let mut decoder = StreamingDecoder::new();
        decoder.feed(b"x\x1b[200");
        assert!(matches!(
            decoder.next_event(),
            Some(InputEvent::Terminal(Event::Key(_)))
        ));
        assert_eq!(decoder.next_event(), None);
        assert_eq!(decoder.flush_escape(), None);
        decoder.feed(b"~safe\x11\x1b[201~");
        assert_eq!(
            decoder.next_event(),
            Some(InputEvent::Terminal(Event::Paste("safe\x11".into())))
        );
        assert_eq!(decoder.next_event(), None);
    }

    #[test]
    fn new_escape_resynchronizes_partial_and_overlong_csi_before_paste() {
        for prefix in [
            b"\x1b[".to_vec(),
            [b"\x1b[".as_slice(), &[b'9'; 80]].concat(),
        ] {
            let mut decoder = StreamingDecoder::new();
            decoder.feed(&prefix);
            assert_eq!(decoder.next_event(), None);
            decoder.feed(b"\x1b[200~safe\x11\x1b[201~");
            assert_eq!(
                decoder.next_event(),
                Some(InputEvent::Terminal(Event::Paste("safe\x11".into())))
            );
        }
    }

    #[test]
    fn osc_payload_is_discarded_through_bel_or_fragmented_st() {
        for terminator in [b"\x07".as_slice(), b"\x1b\\"] {
            let mut decoder = StreamingDecoder::new();
            for fragment in [b"\x1b]10;".as_slice(), b"rgb:ffff\x11", terminator] {
                decoder.feed(fragment);
                assert_eq!(decoder.next_event(), None);
                assert_eq!(decoder.flush_escape(), None);
            }
            decoder.feed(b"a");
            assert_eq!(
                decoder.next_event(),
                Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                    KeyCode::Char('a'),
                    KeyModifiers::NONE
                ))))
            );
        }
    }

    #[test]
    fn invalid_utf8_prefix_does_not_hold_following_ctrl_q() {
        let mut decoder = StreamingDecoder::new();
        decoder.feed(b"\xf0\x11");
        assert_eq!(
            decoder.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::Char('q'),
                KeyModifiers::CONTROL
            ))))
        );
    }

    #[test]
    fn fragmented_linux_f4_does_not_insert_its_final_letter() {
        let mut decoder = StreamingDecoder::new();
        decoder.feed(b"\x1b[[");
        assert_eq!(decoder.next_event(), None);
        decoder.feed(b"D");
        assert_eq!(
            decoder.next_event(),
            Some(InputEvent::Terminal(Event::Key(KeyEvent::new(
                KeyCode::F(4),
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(decoder.next_event(), None);
    }
}

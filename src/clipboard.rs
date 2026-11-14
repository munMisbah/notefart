//! Bounded local clipboard backends.
//!
//! External access is deliberately narrow: fixed executables and arguments,
//! no shell, no terminal attachment, and every child interaction has a deadline.
//! Wayland uses `wl-copy`/`wl-paste`; X11 uses `xclip` on the CLIPBOARD
//! selection (never PRIMARY). Content is never logged. OSC 52 is not
//! implemented or activated here.
//!
//! Selection is capability-only and happens before the editor loop starts. No
//! clipboard read or write occurs during initialization, idle ticks, or
//! rendering.

use std::{
    fmt, io,
    os::fd::AsFd,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    str::FromStr,
    time::{Duration, Instant},
};

use std::os::unix::process::ExitStatusExt;

use rustix::{
    event::{PollFd, PollFlags, Timespec, poll as rustix_poll},
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    io::{Errno, read as rustix_read, write as rustix_write},
    process::{Pid, Signal, kill_process_group},
};

/// Maximum clipboard text accepted from an external helper (1 MiB).
pub(crate) const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;

/// Deadline for one external clipboard read or write, including child exit.
const OPERATION_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a timed-out helper is allowed to shut down after SIGKILL.
const REAP_TIMEOUT: Duration = Duration::from_millis(250);

/// Poll/sleep slice while waiting for a helper to exit.
const WAIT_SLICE: Duration = Duration::from_millis(2);

/// Clipboard selection configured by CLI/file options.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ClipboardMode {
    /// Local helpers when unambiguous; otherwise internal. SSH stays internal.
    Auto,
    /// In-process clipboard only; never starts an external helper.
    Internal,
    /// Local or forwarded display backend; explicit error when unavailable.
    System,
}

impl Default for ClipboardMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl FromStr for ClipboardMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("auto") {
            Ok(Self::Auto)
        } else if s.eq_ignore_ascii_case("internal") {
            Ok(Self::Internal)
        } else if s.eq_ignore_ascii_case("system") {
            Ok(Self::System)
        } else {
            Err("invalid clipboard mode (expected auto, internal, or system)")
        }
    }
}

/// Selected backend; resolved once before the editor event loop starts.
enum Backend {
    Internal {
        /// Empty for an explicit internal selection, otherwise why Auto chose it.
        reason: &'static str,
    },
    Wayland {
        copy: PathBuf,
        paste: PathBuf,
    },
    X11 {
        xclip: PathBuf,
    },
    /// Explicit System mode without a usable local/forwarded display helper.
    Unavailable {
        reason: &'static str,
    },
    /// In-memory stand-in used by tests; never constructed by production entry points.
    #[cfg(test)]
    TestRead(Vec<u8>),
    #[cfg(test)]
    TestWriteFail,
}

impl Backend {
    fn is_internal(&self) -> bool {
        matches!(self, Self::Internal { .. })
    }

    fn startup_note(&self) -> String {
        match self {
            Self::Internal { reason } if reason.is_empty() => "internal".into(),
            Self::Internal { reason } => format!("internal ({reason})"),
            Self::Wayland { .. } => "Wayland (wl-copy/wl-paste)".into(),
            Self::X11 { .. } => "X11 (xclip)".into(),
            Self::Unavailable { reason } => format!("system unavailable ({reason})"),
            #[cfg(test)]
            Self::TestRead(_) => "test external".into(),
            #[cfg(test)]
            Self::TestWriteFail => "test external write failure".into(),
        }
    }

    fn empty_note(&self) -> &'static str {
        match self {
            Self::Internal { .. } => "Internal clipboard is empty; nothing pasted",
            Self::Wayland { .. } => "Wayland clipboard is empty; nothing pasted",
            Self::X11 { .. } => "X11 clipboard is empty; nothing pasted",
            Self::Unavailable { .. } => "System clipboard is unavailable; nothing pasted",
            #[cfg(test)]
            Self::TestRead(_) | Self::TestWriteFail => "Clipboard is empty; nothing pasted",
        }
    }
}

/// One clipboard instance owned by the editor event loop.
pub(crate) struct Clipboard {
    backend: Backend,
    /// Last successfully copied text. Retained even when an external write fails
    /// so the data is never silently lost, and reused by the internal backend.
    internal: String,
}

impl Clipboard {
    /// Resolves the configured mode without reading or writing any clipboard.
    pub(crate) fn new(mode: ClipboardMode) -> Self {
        let backend = match mode {
            ClipboardMode::Internal => Backend::Internal { reason: "" },
            ClipboardMode::Auto => {
                if env_present("SSH_CONNECTION") || env_present("SSH_TTY") {
                    Backend::Internal {
                        reason: "SSH session for privacy",
                    }
                } else {
                    auto_backend(&Probe::detect())
                }
            }
            ClipboardMode::System => system_backend(&Probe::detect()),
        };
        Self {
            backend,
            internal: String::new(),
        }
    }

    /// Startup/open status fragment naming the actual selected backend.
    pub(crate) fn startup_note(&self) -> String {
        self.backend.startup_note()
    }

    /// True when paste must read in-process text instead of an external helper.
    pub(crate) fn is_internal(&self) -> bool {
        self.backend.is_internal()
    }

    /// Text shown when a successful external read returns zero bytes.
    pub(crate) fn empty_note(&self) -> &'static str {
        self.backend.empty_note()
    }

    /// Moves the in-process clipboard out for routing; put it back afterwards.
    pub(crate) fn take_internal(&mut self) -> String {
        std::mem::take(&mut self.internal)
    }

    pub(crate) fn put_internal(&mut self, text: String) {
        self.internal = text;
    }

    /// Copies `text` to the selected backend without an extra selection copy.
    ///
    /// Moves `text` into the internal clipboard first, so a failed external
    /// write never loses data. `Ok(target)` means the selected backend accepted
    /// the write; only then may a cut delete its source. `Err` never claims
    /// external success, even though the internal copy remains available.
    pub(crate) fn copy(&mut self, text: String) -> Result<&'static str, ClipboardError> {
        self.internal = text;
        match &self.backend {
            Backend::Internal { .. } => Ok("internal clipboard"),
            Backend::Wayland { copy, .. } => {
                run_copy(
                    copy,
                    &["--type", "text/plain;charset=utf-8"],
                    "wl-copy",
                    &self.internal,
                )
                .map_err(|failure| ClipboardError::write("Wayland", failure))?;
                Ok("Wayland clipboard")
            }
            Backend::X11 { xclip } => {
                run_copy(
                    xclip,
                    &["-selection", "clipboard", "-i"],
                    "xclip",
                    &self.internal,
                )
                .map_err(|failure| ClipboardError::write("X11", failure))?;
                Ok("X11 clipboard")
            }
            Backend::Unavailable { reason } => Err(ClipboardError::new(format!(
                "system clipboard unavailable: {reason}"
            ))),
            #[cfg(test)]
            Backend::TestRead(_) => Err(ClipboardError::new(
                "test clipboard backend is read-only".to_string(),
            )),
            #[cfg(test)]
            Backend::TestWriteFail => Err(ClipboardError::new(
                "system clipboard write failed: injected test error".to_string(),
            )),
        }
    }

    /// Test-only external backend whose paste returns fixed raw bytes.
    #[cfg(test)]
    pub(crate) fn test_read_bytes(bytes: Vec<u8>) -> Self {
        Self {
            backend: Backend::TestRead(bytes),
            internal: String::new(),
        }
    }

    /// Test-only external backend whose copy always fails after retaining text.
    #[cfg(test)]
    pub(crate) fn test_write_failure() -> Self {
        Self {
            backend: Backend::TestWriteFail,
            internal: String::new(),
        }
    }

    /// Reads text only for an explicit paste action.
    ///
    /// Empty output is `Ok("")`, a legitimate empty clipboard. Any timeout,
    /// oversize, invalid UTF-8, helper failure, or unavailable backend is an
    /// error; the internal clipboard is never substituted.
    pub(crate) fn read_external(&self) -> Result<String, ClipboardError> {
        match &self.backend {
            Backend::Internal { .. } => Err(ClipboardError::read(
                "internal",
                Failure::NoExternalClipboard,
            )),
            Backend::Unavailable { reason } => Err(ClipboardError::new(format!(
                "system clipboard unavailable: {reason}; nothing pasted"
            ))),
            Backend::Wayland { paste, .. } => run_read(paste, &["--no-newline"], "wl-paste")
                .map_err(|failure| ClipboardError::read("Wayland", failure)),
            Backend::X11 { xclip } => run_read(xclip, &["-selection", "clipboard", "-o"], "xclip")
                .map_err(|failure| ClipboardError::read("X11", failure)),
            #[cfg(test)]
            Backend::TestRead(bytes) => decode_bytes(bytes.clone())
                .map_err(|failure| ClipboardError::read("system", failure)),
            #[cfg(test)]
            Backend::TestWriteFail => Err(ClipboardError::read(
                "system",
                Failure::Io(io::Error::other("injected test read error")),
            )),
        }
    }
}

/// Sanitized clipboard failure suitable for a status line.
#[derive(Debug)]
pub(crate) struct ClipboardError {
    message: String,
}

impl ClipboardError {
    fn new(message: String) -> Self {
        Self { message }
    }

    fn read(label: &str, failure: Failure) -> Self {
        Self::new(format!(
            "{label} clipboard read failed: {failure}; nothing pasted"
        ))
    }

    fn write(label: &str, failure: Failure) -> Self {
        Self::new(format!("{label} clipboard write failed: {failure}"))
    }
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClipboardError {}

#[derive(Debug)]
enum Failure {
    Spawn {
        command: &'static str,
        error: io::Error,
    },
    Io(io::Error),
    Timeout,
    Exit {
        command: &'static str,
        status: ExitStatus,
    },
    TooLarge,
    InvalidUtf8,
    NoExternalClipboard,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { command, error } => {
                write!(f, "could not start {command}: {error}")
            }
            Self::Io(error) => write!(f, "{error}"),
            Self::Timeout => {
                write!(f, "timed out after {}s", OPERATION_TIMEOUT.as_secs())
            }
            Self::Exit { command, status } => {
                write!(f, "{command} ")?;
                if let Some(code) = status.code() {
                    write!(f, "exited with status {code}")
                } else if let Some(signal) = status.signal() {
                    write!(f, "was terminated by signal {signal}")
                } else {
                    write!(f, "did not exit successfully")
                }
            }
            Self::TooLarge => write!(f, "content exceeds 1 MiB"),
            Self::InvalidUtf8 => write!(f, "content is not valid UTF-8"),
            Self::NoExternalClipboard => write!(f, "internal backend has no external clipboard"),
        }
    }
}

/// Environment/executable snapshot used for one-time backend selection.
struct Probe {
    ssh: bool,
    wayland_display: bool,
    display: bool,
    wl_copy: Option<PathBuf>,
    wl_paste: Option<PathBuf>,
    xclip: Option<PathBuf>,
}

impl Probe {
    fn detect() -> Self {
        Self {
            ssh: false,
            wayland_display: env_present("WAYLAND_DISPLAY"),
            display: env_present("DISPLAY"),
            wl_copy: find_executable("wl-copy"),
            wl_paste: find_executable("wl-paste"),
            xclip: find_executable("xclip"),
        }
    }

    fn wayland_tools(&self) -> Option<(PathBuf, PathBuf)> {
        match (&self.wl_copy, &self.wl_paste) {
            (Some(copy), Some(paste)) if self.wayland_display => {
                Some((copy.clone(), paste.clone()))
            }
            _ => None,
        }
    }

    fn x11_tool(&self) -> Option<PathBuf> {
        if self.display {
            self.xclip.clone()
        } else {
            None
        }
    }
}

fn auto_backend(probe: &Probe) -> Backend {
    if probe.ssh {
        return Backend::Internal {
            reason: "SSH session for privacy",
        };
    }
    if let Some((copy, paste)) = probe.wayland_tools() {
        return Backend::Wayland { copy, paste };
    }
    if let Some(xclip) = probe.x11_tool() {
        return Backend::X11 { xclip };
    }
    Backend::Internal {
        reason: no_backend_reason(probe),
    }
}

fn system_backend(probe: &Probe) -> Backend {
    if let Some((copy, paste)) = probe.wayland_tools() {
        return Backend::Wayland { copy, paste };
    }
    if let Some(xclip) = probe.x11_tool() {
        return Backend::X11 { xclip };
    }
    Backend::Unavailable {
        reason: no_backend_reason(probe),
    }
}

/// A fixed, control-free explanation for why Auto fell back to internal.
fn no_backend_reason(probe: &Probe) -> &'static str {
    if !probe.wayland_display && !probe.display {
        "no WAYLAND_DISPLAY or DISPLAY"
    } else if probe.display && probe.xclip.is_none() && !probe.wayland_display {
        "xclip unavailable and no WAYLAND_DISPLAY"
    } else if probe.wayland_display
        && probe.wl_copy.is_none()
        && probe.wl_paste.is_none()
        && !probe.display
    {
        "wl-copy/wl-paste unavailable and no DISPLAY"
    } else {
        "wl-copy/wl-paste or xclip unavailable"
    }
}

fn env_present(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

/// Finds an executable using PATH without invoking a shell or starting it.
///
/// Relative and empty PATH entries are skipped: a project directory must not
/// be able to supply a clipboard helper through the user's PATH spelling.
fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() || !dir.is_absolute() {
            continue;
        }
        let candidate = dir.join(name);
        let Ok(metadata) = std::fs::metadata(&candidate) else {
            continue;
        };
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            return Some(candidate);
        }
    }
    None
}

fn run_copy(
    program: &Path,
    args: &[&str],
    command: &'static str,
    text: &str,
) -> Result<(), Failure> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    let mut child = spawn_write(program, args, command)?;
    let mut reaped = false;
    let result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Failure::Io(io::Error::other("clipboard helper stdin not captured")))?;
        set_nonblocking(&stdin)?;
        write_all(&mut stdin, text.as_bytes(), deadline)?;
        drop(stdin);
        let status = wait_for_exit(&mut child, deadline)?;
        reaped = true;
        if status.success() {
            Ok(())
        } else {
            Err(Failure::Exit { command, status })
        }
    })();
    if result.is_err() && !reaped {
        terminate_and_reap(&mut child);
    }
    result
}

fn run_read(program: &Path, args: &[&str], command: &'static str) -> Result<String, Failure> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    let mut child = spawn_read(program, args, command)?;
    let mut reaped = false;
    let result = (|| {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Failure::Io(io::Error::other("clipboard helper stdout not captured")))?;
        set_nonblocking(&stdout)?;
        let bytes = read_bounded(&stdout, deadline)?;
        drop(stdout);
        let status = wait_for_exit(&mut child, deadline)?;
        reaped = true;
        if !status.success() {
            return Err(Failure::Exit { command, status });
        }
        decode_bytes(bytes)
    })();
    if result.is_err() && !reaped {
        terminate_and_reap(&mut child);
    }
    result
}

fn spawn_write(program: &Path, args: &[&str], command: &'static str) -> Result<Child, Failure> {
    let mut cmd = Command::new(program);
    cmd.args(args.iter().copied())
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| Failure::Spawn { command, error })
}

fn spawn_read(program: &Path, args: &[&str], command: &'static str) -> Result<Child, Failure> {
    let mut cmd = Command::new(program);
    cmd.args(args.iter().copied())
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| Failure::Spawn { command, error })
}

fn set_nonblocking<F: AsFd>(fd: &F) -> Result<(), Failure> {
    let flags = fcntl_getfl(fd).map_err(|error| Failure::Io(io::Error::from(error)))?;
    fcntl_setfl(fd, flags | OFlags::NONBLOCK).map_err(|error| Failure::Io(io::Error::from(error)))
}

fn timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: duration.as_secs().min(i64::MAX as u64) as i64,
        tv_nsec: i64::from(duration.subsec_nanos()),
    }
}

fn remaining(deadline: Instant) -> Option<Duration> {
    let now = Instant::now();
    if now >= deadline {
        None
    } else {
        Some(deadline - now)
    }
}

fn write_all(stdin: &mut ChildStdin, bytes: &[u8], deadline: Instant) -> Result<(), Failure> {
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = remaining(deadline).ok_or(Failure::Timeout)?;
        let mut fds = [PollFd::new(stdin, PollFlags::OUT)];
        match rustix_poll(&mut fds, Some(&timespec(remaining))) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(error) => return Err(Failure::Io(error.into())),
        }
        match rustix_write(&*stdin, &bytes[offset..]) {
            Ok(0) => {
                return Err(Failure::Io(io::Error::from(io::ErrorKind::WriteZero)));
            }
            Ok(written) => offset += written,
            Err(Errno::INTR) | Err(Errno::AGAIN) => {}
            Err(error) => return Err(Failure::Io(error.into())),
        }
    }
    Ok(())
}

fn read_bounded<F: AsFd>(stdout: &F, deadline: Instant) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let remaining = remaining(deadline).ok_or(Failure::Timeout)?;
        let mut fds = [PollFd::new(stdout, PollFlags::IN)];
        match rustix_poll(&mut fds, Some(&timespec(remaining))) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(error) => return Err(Failure::Io(error.into())),
        }
        let cap = (MAX_CLIPBOARD_BYTES + 1 - bytes.len()).min(chunk.len());
        match rustix_read(stdout, &mut chunk[..cap]) {
            Ok(0) => return Ok(bytes),
            Ok(read) => {
                if bytes.len().saturating_add(read) > MAX_CLIPBOARD_BYTES {
                    return Err(Failure::TooLarge);
                }
                bytes.extend_from_slice(&chunk[..read]);
            }
            Err(Errno::INTR) | Err(Errno::AGAIN) => {}
            Err(error) => return Err(Failure::Io(error.into())),
        }
    }
}

fn decode_bytes(bytes: Vec<u8>) -> Result<String, Failure> {
    if bytes.len() > MAX_CLIPBOARD_BYTES {
        return Err(Failure::TooLarge);
    }
    String::from_utf8(bytes).map_err(|_| Failure::InvalidUtf8)
}

fn wait_for_exit(child: &mut Child, deadline: Instant) -> Result<ExitStatus, Failure> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                let remaining = remaining(deadline).ok_or(Failure::Timeout)?;
                std::thread::sleep(remaining.min(WAIT_SLICE));
            }
            Err(error) => return Err(Failure::Io(error)),
        }
    }
}

/// Kills the helper's owned process group and reaps without an unbounded wait.
///
/// Called only on failure (never after a successful copy, whose background
/// clipboard owner must keep serving).
fn terminate_and_reap(child: &mut Child) {
    if let Ok(raw_pid) = i32::try_from(child.id())
        && let Some(pgid) = Pid::from_raw(raw_pid)
    {
        let _ = kill_process_group(pgid, Signal::KILL);
    }
    let _ = child.kill();
    let deadline = Instant::now() + REAP_TIMEOUT;
    while let Some(remaining) = remaining(deadline) {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => std::thread::sleep(remaining.min(WAIT_SLICE)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(
        ssh: bool,
        wayland_display: bool,
        display: bool,
        wl_copy: bool,
        wl_paste: bool,
        xclip: bool,
    ) -> Probe {
        Probe {
            ssh,
            wayland_display,
            display,
            wl_copy: wl_copy.then(|| PathBuf::from("/cb/wl-copy")),
            wl_paste: wl_paste.then(|| PathBuf::from("/cb/wl-paste")),
            xclip: xclip.then(|| PathBuf::from("/cb/xclip")),
        }
    }

    #[test]
    fn auto_keeps_ssh_private_even_with_display_helpers() {
        let backend = auto_backend(&probe(true, true, true, true, true, true));
        assert!(matches!(backend, Backend::Internal { .. }));
    }

    #[test]
    fn auto_requires_both_wayland_helpers_before_preferring_wayland() {
        let incomplete = probe(false, true, true, true, false, true);
        assert!(matches!(auto_backend(&incomplete), Backend::X11 { .. }));

        let complete = probe(false, true, true, true, true, true);
        assert!(matches!(auto_backend(&complete), Backend::Wayland { .. }));
    }

    #[test]
    fn auto_without_helpers_uses_internal() {
        let backend = auto_backend(&probe(false, false, false, false, false, false));
        assert!(matches!(backend, Backend::Internal { .. }));
    }

    #[test]
    fn system_reports_unavailable_instead_of_pretending_to_be_internal() {
        assert!(matches!(
            system_backend(&probe(false, false, false, false, false, false)),
            Backend::Unavailable { .. }
        ));
        // Forwarded/remote displays are explicit System choices.
        assert!(matches!(
            system_backend(&probe(true, true, true, true, true, true)),
            Backend::Wayland { .. }
        ));
    }

    #[test]
    fn external_reads_are_bounded_and_rejected_atomically() {
        let exact = vec![b'a'; MAX_CLIPBOARD_BYTES];
        assert_eq!(decode_bytes(exact).unwrap().len(), MAX_CLIPBOARD_BYTES);

        let mut oversized = vec![b'a'; MAX_CLIPBOARD_BYTES];
        oversized.push(b'!');
        assert!(matches!(decode_bytes(oversized), Err(Failure::TooLarge)));

        assert!(matches!(
            decode_bytes(vec![0xff, 0xfe]),
            Err(Failure::InvalidUtf8)
        ));
        assert_eq!(decode_bytes(Vec::new()).unwrap(), "");
    }

    #[test]
    fn clipboard_mode_parsing_accepts_only_the_contract_names() {
        assert_eq!(
            "auto".parse::<ClipboardMode>().unwrap(),
            ClipboardMode::Auto
        );
        assert_eq!(
            "INTERNAL".parse::<ClipboardMode>().unwrap(),
            ClipboardMode::Internal
        );
        assert_eq!(
            "system".parse::<ClipboardMode>().unwrap(),
            ClipboardMode::System
        );
        assert!("primary".parse::<ClipboardMode>().is_err());
        assert!("".parse::<ClipboardMode>().is_err());
    }
}

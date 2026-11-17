use std::{
    io::{self, IsTerminal, Stdout},
    sync::{
        Arc, Once,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};
use signal_hook::SigId;

const MODE_RAW: u8 = 1 << 0;
const MODE_ALT_SCREEN: u8 = 1 << 1;
const MODE_CURSOR_HIDDEN: u8 = 1 << 2;
const MODE_MOUSE_CAPTURE: u8 = 1 << 3;
const MODE_BRACKETED_PASTE: u8 = 1 << 4;

static ACTIVE_MODES: AtomicU8 = AtomicU8::new(0);
static PANIC_HOOK_ONCE: Once = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminationSignal {
    SigTerm,
    #[cfg(unix)]
    SigHup,
    SigInt,
}

impl TerminationSignal {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::SigTerm => "SIGTERM",
            #[cfg(unix)]
            Self::SigHup => "SIGHUP",
            Self::SigInt => "SIGINT",
        }
    }
}

pub(crate) struct SignalGuard {
    ids: Vec<SigId>,
    sigterm: Arc<AtomicBool>,
    #[cfg(unix)]
    sighup: Arc<AtomicBool>,
    sigint: Arc<AtomicBool>,
}

impl SignalGuard {
    fn install() -> io::Result<Self> {
        let sigterm = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let sighup = Arc::new(AtomicBool::new(false));
        let sigint = Arc::new(AtomicBool::new(false));

        let mut guard = Self {
            ids: Vec::with_capacity(3),
            sigterm: Arc::clone(&sigterm),
            #[cfg(unix)]
            sighup: Arc::clone(&sighup),
            sigint: Arc::clone(&sigint),
        };

        guard.ids.push(signal_hook::flag::register(
            signal_hook::consts::SIGTERM,
            sigterm,
        )?);
        #[cfg(unix)]
        guard.ids.push(signal_hook::flag::register(
            signal_hook::consts::SIGHUP,
            sighup,
        )?);
        guard.ids.push(signal_hook::flag::register(
            signal_hook::consts::SIGINT,
            sigint,
        )?);

        Ok(guard)
    }

    pub(crate) fn pending(&self) -> Option<TerminationSignal> {
        if self.sigterm.load(Ordering::Acquire) {
            return Some(TerminationSignal::SigTerm);
        }
        #[cfg(unix)]
        if self.sighup.load(Ordering::Acquire) {
            return Some(TerminationSignal::SigHup);
        }
        if self.sigint.load(Ordering::Acquire) {
            return Some(TerminationSignal::SigInt);
        }
        None
    }

    fn unregister(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        self.unregister();
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        install_panic_hook_once();

        enable_raw_mode()?;
        ACTIVE_MODES.fetch_or(MODE_RAW, Ordering::SeqCst);
        let guard = Self;

        {
            let mut stdout = io::stdout().lock();
            ACTIVE_MODES.fetch_or(MODE_ALT_SCREEN, Ordering::SeqCst);
            execute!(stdout, EnterAlternateScreen)?;

            ACTIVE_MODES.fetch_or(
                MODE_CURSOR_HIDDEN | MODE_MOUSE_CAPTURE | MODE_BRACKETED_PASTE,
                Ordering::SeqCst,
            );
            execute!(stdout, Hide, EnableMouseCapture, EnableBracketedPaste)?;
        }

        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        restore_active_modes()
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Drop is best-effort only (e.g. panic unwind or partial setup failure);
        // normal and signal exits call `TerminalSession::restore` to propagate errors.
        let _ = restore_active_modes();
    }
}

pub(crate) struct TerminalSession {
    // Drop `Terminal` before `TerminalGuard` and `SignalGuard`.
    terminal: Terminal<CrosstermBackend<Stdout>>,
    guard: TerminalGuard,
    signals: SignalGuard,
}

impl TerminalSession {
    pub(crate) fn enter() -> io::Result<Self> {
        verify_tty()?;
        let signals = SignalGuard::install()?;
        let guard = TerminalGuard::enter()?;
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

        Ok(Self {
            terminal,
            guard,
            signals,
        })
    }

    pub(crate) fn size(&self) -> io::Result<(u16, u16)> {
        let size = self.terminal.size()?;
        Ok((size.width, size.height))
    }

    pub(crate) fn draw<F>(&mut self, render: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame),
    {
        self.terminal.draw(render)?;
        Ok(())
    }

    pub(crate) fn pending_signal(&self) -> Option<TerminationSignal> {
        self.signals.pending()
    }

    pub(crate) fn restore(mut self) -> io::Result<()> {
        let mut first_error: Option<io::Error> = None;

        if let Err(err) = self.terminal.show_cursor() {
            first_error = Some(err);
        } else {
            ACTIVE_MODES.fetch_and(!MODE_CURSOR_HIDDEN, Ordering::SeqCst);
        }

        if let Err(err) = self.guard.restore() {
            first_error.get_or_insert(err);
        }
        self.signals.unregister();
        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

/// The editor theme already resolves config/CLI and environment precedence.
/// Do not let the backend independently apply NO_COLOR a second time.
pub(crate) fn use_theme_color_policy() {
    crossterm::style::force_color_output(true);
}

pub(crate) fn verify_tty() -> io::Result<()> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "stdin is not a TTY; --probe requires an interactive terminal",
        ));
    }
    if !io::stdout().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "stdout is not a TTY; --probe requires an interactive terminal",
        ));
    }
    Ok(())
}

fn install_panic_hook_once() {
    PANIC_HOOK_ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Restore terminal state before printing the panic message to stderr.
            let _ = restore_active_modes();
            previous(info);
        }));
    });
}

fn restore_active_modes() -> io::Result<()> {
    let mut first_error: Option<io::Error> = None;

    // Keep failed modes armed so Drop can retry. An interrupted syscall is
    // retryable; a permanently broken output still must not prevent raw cleanup.
    macro_rules! restore_mode {
        ($mode:expr, $operation:expr) => {
            if ACTIVE_MODES.load(Ordering::SeqCst) & $mode != 0 {
                let result = loop {
                    match $operation {
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        result => break result,
                    }
                };
                match result {
                    Ok(()) => {
                        ACTIVE_MODES.fetch_and(!$mode, Ordering::SeqCst);
                    }
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
        };
    }

    // Disable terminal reporting before restoring ECHO.
    let mut stdout = io::stdout().lock();
    restore_mode!(
        MODE_BRACKETED_PASTE,
        execute!(stdout, DisableBracketedPaste)
    );
    restore_mode!(MODE_MOUSE_CAPTURE, execute!(stdout, DisableMouseCapture));
    restore_mode!(MODE_CURSOR_HIDDEN, execute!(stdout, Show));
    restore_mode!(MODE_ALT_SCREEN, execute!(stdout, LeaveAlternateScreen));
    drop(stdout);
    restore_mode!(MODE_RAW, disable_raw_mode());
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::use_theme_color_policy;
    use crate::theme::{ColorMode, Theme, ThemeKind};
    use ratatui::{
        backend::{Backend, CrosstermBackend},
        buffer::Cell,
    };

    #[test]
    fn theme_policy_controls_real_backend_output() {
        const CASE_ENV: &str = "NAF_TEST_THEME_COLOR_CASE";
        let Ok(case) = std::env::var(CASE_ENV) else {
            // Crossterm caches a process-global flag. Isolate each environment
            // instead of racing the rest of the suite with set_var/force calls.
            let thread = std::thread::current();
            let name = thread.name().expect("test thread has a name");
            for case in ["truecolor", "256", "16", "none", "auto"] {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", name, "--nocapture"])
                    .env(CASE_ENV, case)
                    .env("NO_COLOR", "1")
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{case}: {}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                );
            }
            return;
        };
        let mode = match case.as_str() {
            "truecolor" => ColorMode::Truecolor,
            "256" => ColorMode::Ansi256,
            "16" => ColorMode::Ansi16,
            "none" => ColorMode::None,
            "auto" => ColorMode::Auto,
            _ => panic!("unknown isolated color case"),
        };
        let theme = Theme::new(ThemeKind::Dark, mode);
        use_theme_color_policy();
        let mut cell = Cell::new("x");
        // ANSI16 deliberately inherits the terminal foreground and canvas.
        // Exercise a semantic color rather than assuming text must be colored.
        cell.set_fg(theme.accent).set_bg(theme.canvas);
        let mut bytes = Vec::new();
        {
            let mut backend = CrosstermBackend::new(&mut bytes);
            backend.draw(std::iter::once((0, 0, &cell))).unwrap();
        }
        let ansi = String::from_utf8(bytes).unwrap();
        let has_color = ansi.split("\x1b[").any(|sequence| {
            let Some((parameters, _)) = sequence.split_once('m') else {
                return false;
            };
            parameters.split(';').any(|parameter| {
                parameter
                    .parse::<u16>()
                    .is_ok_and(|value| matches!(value, 30..=38 | 40..=48 | 90..=97 | 100..=107))
            })
        });
        assert_eq!(
            has_color,
            !matches!(mode, ColorMode::None | ColorMode::Auto),
            "resolved {case} under NO_COLOR=1 emitted {ansi:?}",
        );
    }
}

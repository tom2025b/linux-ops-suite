//! `Tui`: a RAII terminal scope guard. Setup in `new`, guaranteed teardown in
//! `Drop` (runs on normal return, `?` propagation, and panic unwind alike).

use std::io::{self, IsTerminal, Write, stdout};

use crossterm::execute;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::DefaultTerminal;

/// What can go wrong setting up the terminal.
#[derive(Debug)]
pub enum TuiError {
    /// `require_tty` was set but stdout is not a terminal. `Display` carries an
    /// actionable message pointing the user at the non-interactive CLI.
    NotATerminal,
    /// A terminal setup call failed (entering raw mode, the alt screen, etc.).
    Io(io::Error),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TuiError::NotATerminal => write!(
                f,
                "this command requires an interactive terminal\n\
                 (stdout is not a tty / not connected to a real terminal).\n\n\
                 For non-interactive use, run the CLI subcommands instead."
            ),
            TuiError::Io(e) => write!(f, "terminal setup failed: {e}"),
        }
    }
}

impl std::error::Error for TuiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TuiError::Io(e) => Some(e),
            TuiError::NotATerminal => None,
        }
    }
}

impl From<io::Error> for TuiError {
    fn from(e: io::Error) -> Self {
        TuiError::Io(e)
    }
}

/// Which envelope features the guard should set up. Configures the *envelope*,
/// not the event loop. Cheap to copy.
#[derive(Default, Clone, Copy, Debug)]
pub struct TuiOptions {
    /// Hide the cursor for the duration (Bulwark, RexOps: true; a tool with a
    /// visible text cursor like ScriptVault: false).
    pub hide_cursor: bool,
    /// Enable mouse capture (ScriptVault click-to-select: true; others: false).
    pub mouse_capture: bool,
    /// Fail fast with a friendly [`TuiError::NotATerminal`] when stdout is not a
    /// terminal, instead of entering raw mode in a non-interactive environment.
    pub require_tty: bool,
}

/// A RAII terminal scope guard. Construct it to enter TUI mode; drop it (any
/// exit path — return, `?`, or panic) to restore the terminal.
pub struct Tui {
    terminal: DefaultTerminal,
    opts: TuiOptions,
    /// Lines queued via [`Tui::print_after_exit`] to print to real stdout after
    /// the terminal is restored (drained on `Drop`, skipped on panic).
    out: Vec<String>,
}

impl Tui {
    /// Set up the terminal per `opts`. Order:
    ///   1. require_tty gate (before any side effect)
    ///   2. ratatui::try_init() — raw mode + alt screen + restoring panic hook
    ///   3. optional cursor-hide
    ///   4. optional mouse capture
    pub fn new(opts: TuiOptions) -> Result<Self, TuiError> {
        if opts.require_tty && !stdout().is_terminal() {
            return Err(TuiError::NotATerminal);
        }
        let mut terminal = ratatui::try_init()?;
        if opts.hide_cursor {
            terminal.hide_cursor()?;
        }
        if opts.mouse_capture {
            execute!(stdout(), EnableMouseCapture)?;
        }
        Ok(Self {
            terminal,
            opts,
            out: Vec::new(),
        })
    }

    /// `Tui::new(TuiOptions::default())` — bare alt screen + panic hook, no
    /// cursor-hide, no mouse, no tty gate.
    pub fn simple() -> Result<Self, TuiError> {
        Self::new(TuiOptions::default())
    }

    /// Borrow the terminal to drive your own event loop. The escape hatch for
    /// tools that need background-channel draining or adaptive poll timeouts —
    /// they keep full control of the loop and still get the guard's teardown.
    pub fn terminal(&mut self) -> &mut DefaultTerminal {
        &mut self.terminal
    }

    /// Queue a line to print to real stdout AFTER the terminal is restored, so
    /// it lands in the user's shell (not the alt screen). Drained on `Drop` —
    /// except on a panic, where nothing was "picked" so nothing is printed.
    pub fn print_after_exit(&mut self, line: impl Into<String>) {
        self.out.push(line.into());
    }
}

/// Drain queued lines to a writer, each followed by a newline. Factored out of
/// `Drop` so it is unit-testable without a real terminal: `Drop` calls it with
/// `stdout()`, tests call it with an in-memory buffer.
fn drain_lines(out: &mut Vec<String>, w: &mut impl Write) {
    for line in out.drain(..) {
        let _ = writeln!(w, "{line}");
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Best-effort: undo the optional envelope bits we turned on, in reverse.
        if self.opts.mouse_capture {
            let _ = execute!(stdout(), DisableMouseCapture);
        }
        if self.opts.hide_cursor {
            let _ = self.terminal.show_cursor();
        }
        // Baseline restore: disable raw mode + leave alt screen. Idempotent, so
        // it is safe even though the panic hook may have already run it.
        ratatui::restore();
        // Flush queued stdout — but NOT while panicking: a crash picked nothing,
        // so a queued result must not leak out as if it were a real selection.
        if !std::thread::panicking() {
            drain_lines(&mut self.out, &mut stdout());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_terminal_display_is_actionable() {
        let msg = TuiError::NotATerminal.to_string();
        assert!(msg.contains("interactive terminal"), "names the requirement");
        assert!(msg.contains("CLI"), "points at the non-interactive fallback");
    }

    #[test]
    fn require_tty_rejects_non_terminal_without_touching_setup() {
        // Under `cargo test`, stdout is not a tty. With require_tty set, new()
        // must fail at the gate and return NotATerminal — never reaching
        // raw-mode setup (which would corrupt the test runner's terminal).
        let opts = TuiOptions {
            require_tty: true,
            ..Default::default()
        };
        let result = Tui::new(opts);
        assert!(
            matches!(result, Err(TuiError::NotATerminal)),
            "require_tty must reject a non-tty before any setup"
        );
    }

    #[test]
    fn drain_lines_writes_each_in_order_then_empties() {
        // `print_after_exit` is a plain push; the drain (which Drop runs against
        // stdout) writes each queued line + newline in order, then empties the
        // queue. Tested without a terminal by draining into an in-memory buffer.
        let mut q: Vec<String> = vec!["first".to_string(), "second".to_string()];
        let mut buf: Vec<u8> = Vec::new();
        drain_lines(&mut q, &mut buf);
        assert_eq!(String::from_utf8(buf).unwrap(), "first\nsecond\n");
        assert!(q.is_empty(), "drain empties the queue");
    }
}

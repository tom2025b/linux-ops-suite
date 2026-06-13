//! `Tui`: a RAII terminal scope guard. Setup in `new`, guaranteed teardown in
//! `Drop` (runs on normal return, `?` propagation, and panic unwind alike).

use std::io::{self, stdout, IsTerminal, Write};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
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
        // A failure between try_init and Ok(Self) must undo the raw-mode/alt-
        // screen setup HERE: `Self` was never built, so `Drop` can never run,
        // and without this restore the user's shell would be left raw.
        if let Err(e) = apply_envelope(&mut terminal, opts) {
            ratatui::restore();
            return Err(e.into());
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

    /// Leave the alt screen + raw mode (releasing mouse capture if it was on),
    /// run `f` on the user's real terminal, then re-enter and clear.
    ///
    /// The child runs ONLY if the leave succeeded — it must never be launched
    /// into a half-suspended terminal it would scribble over. Re-entry is
    /// attempted regardless, so the terminal is never left in a suspended
    /// state. On error, the re-enter error dominates (current terminal state
    /// matters most), then a leave error.
    ///
    /// Use this to launch an editor or another full-screen child program.
    pub fn suspended<T>(&mut self, f: impl FnOnce() -> T) -> io::Result<T> {
        with_suspended(self, Self::leave_for_child, f, Self::reenter_after_child)
    }

    /// Undo the envelope for a child process: the reverse of setup, mirroring
    /// `Drop` — mouse capture off first (or the child reads clicks as escape
    /// garbage on stdin), cursor back, then the baseline raw-mode/alt-screen
    /// restore. Attempts ALL steps even when an earlier one fails (`.and()`
    /// keeps the first error) — the child should get the cleanest terminal we
    /// can manage.
    fn leave_for_child(&mut self) -> io::Result<()> {
        let mouse_result = if self.opts.mouse_capture {
            execute!(stdout(), DisableMouseCapture)
        } else {
            Ok(())
        };
        let show_result = self.terminal.show_cursor();
        let restore_result = ratatui::try_restore();
        mouse_result.and(show_result).and(restore_result)
    }

    /// Re-establish the envelope after a child: same order as `new` (init,
    /// cursor-hide, mouse capture), plus a `clear` so ratatui doesn't
    /// diff-render against a buffer the child scribbled over.
    fn reenter_after_child(&mut self) -> io::Result<()> {
        // This assignment drops the OLD terminal AFTER the fresh init. Safe
        // under ratatui 0.29: Terminal's Drop only re-shows a still-hidden
        // cursor, and leave_for_child's show_cursor already cleared that flag.
        self.terminal = ratatui::try_init()?;
        // Unlike in `new`, a failure here needs no manual restore: the fresh
        // terminal is already assigned, so our own `Drop` will clean up.
        apply_envelope(&mut self.terminal, self.opts)?;
        self.terminal.clear()?;
        drain_pending_events()
    }
}

/// Apply the optional envelope bits (cursor-hide, mouse capture) to a freshly
/// initialised terminal — the steps shared by `new` and `reenter_after_child`,
/// which differ only in how they clean up when a step fails.
fn apply_envelope(terminal: &mut DefaultTerminal, opts: TuiOptions) -> io::Result<()> {
    if opts.hide_cursor {
        terminal.hide_cursor()?;
    }
    if opts.mouse_capture {
        execute!(stdout(), EnableMouseCapture)?;
    }
    Ok(())
}

/// Drain queued lines to a writer, each followed by a newline. Factored out of
/// `Drop` so it is unit-testable without a real terminal: `Drop` calls it with
/// `stdout()`, tests call it with an in-memory buffer.
fn drain_lines(out: &mut Vec<String>, w: &mut impl Write) {
    for line in out.drain(..) {
        let _ = writeln!(w, "{line}");
    }
}

/// Discard any input events buffered while a suspended child held the terminal.
/// A full-screen child (editor, pager) leaves keystrokes — and its own mouse/
/// focus escape sequences — queued on stdin; without this drain they would fire
/// in the TUI's event loop on the very first poll after re-entry, e.g. a stray
/// `q` from the editor quitting the cockpit. Polls with a zero timeout so it
/// only ever reads what is already pending and never blocks.
fn drain_pending_events() -> io::Result<()> {
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}

/// The leave→run→re-enter control flow behind [`Tui::suspended`], with the
/// terminal ops injected as `ctx`-taking closures so the SAME code path is
/// unit-testable without a real terminal (prod passes `&mut Tui`; tests pass a
/// counter). The body runs ONLY on a clean leave — a child must never be
/// launched into a half-suspended terminal. Re-enter ALWAYS runs (even if
/// leave failed), so the terminal is never left suspended. The re-enter error
/// dominates (current terminal state matters most); otherwise a leave error
/// propagates; otherwise the body's value is returned.
fn with_suspended<C, T>(
    ctx: &mut C,
    leave: impl FnOnce(&mut C) -> io::Result<()>,
    body: impl FnOnce() -> T,
    reenter: impl FnOnce(&mut C) -> io::Result<()>,
) -> io::Result<T> {
    let leave_result = leave(ctx);
    let value = match leave_result {
        Ok(()) => Ok(body()),
        Err(e) => Err(e),
    };
    let reenter_result = reenter(ctx);
    reenter_result?;
    value
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
        assert!(
            msg.contains("interactive terminal"),
            "names the requirement"
        );
        assert!(
            msg.contains("CLI"),
            "points at the non-interactive fallback"
        );
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

    /// Test context for `with_suspended`: counts leave/re-enter calls. Prod
    /// passes `&mut Tui` here; tests only need the call accounting.
    #[derive(Default)]
    struct Calls {
        left: u32,
        reentered: u32,
    }

    #[test]
    fn suspended_runs_closure_and_returns_value() {
        // suspended() must run the closure and hand back its return value. We
        // test the control flow via `with_suspended` — the SAME helper prod
        // uses — with the terminal ops replaced by counters so no tty is
        // needed in CI.
        let mut calls = Calls::default();
        let value = with_suspended(
            &mut calls,
            |c| {
                c.left += 1;
                Ok(())
            },
            || 42, // the "child" body
            |c| {
                c.reentered += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(value, 42, "returns the closure's value");
        assert_eq!(
            (calls.left, calls.reentered),
            (1, 1),
            "leaves once, re-enters once"
        );
    }

    #[test]
    fn suspended_skips_body_and_reenters_when_leave_fails() {
        // A failed leave means the terminal is NOT safely on the user's real
        // screen — the child must not run (it would scribble over the TUI).
        // Re-enter still runs so the terminal isn't stuck, and the leave error
        // surfaces to the caller.
        let mut calls = Calls::default();
        let mut body_ran = false;
        let result = with_suspended(
            &mut calls,
            |_| Err(io::Error::other("leave failed")),
            || {
                body_ran = true;
                7
            },
            |c| {
                c.reentered += 1;
                Ok(())
            },
        );
        assert!(result.is_err(), "leave failure surfaces as an error");
        assert!(!body_ran, "the child must NOT run after a failed leave");
        assert_eq!(
            calls.reentered, 1,
            "re-enter still runs after a leave failure"
        );
    }

    #[test]
    fn with_suspended_reenter_error_dominates_when_both_fail() {
        // When BOTH leave and re-enter fail, the re-enter error is the one
        // returned (current terminal state matters most). Locks the documented
        // precedence: `reenter_result?` runs before the leave error.
        let mut calls = Calls::default();
        let result = with_suspended(
            &mut calls,
            |_| Err(io::Error::other("leave failed")),
            || (),
            |_| Err(io::Error::other("reenter failed")),
        );
        let err = result.expect_err("both failing must surface an error");
        assert_eq!(
            err.to_string(),
            "reenter failed",
            "the re-enter error dominates"
        );
    }
}

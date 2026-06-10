# suite-ui App Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `suite_ui::app` module with a RAII `Tui` terminal scope guard (the foundation) and a thin optional `App::new(theme).run(root)` runner on top, removing the terminal-lifecycle boilerplate Bulwark/RexOps/ScriptVault each hand-roll.

**Architecture:** Two layers in one module. `Tui` (in `app/tui.rs`) owns the terminal envelope — tty pre-flight, setup via ratatui's `try_init`, optional cursor-hide/mouse-capture, a panic-safe `Drop` teardown, a `suspended` helper for shelling out, and an ordered post-exit stdout flush. `App` (in `app/runner.rs`) is implemented entirely on `Tui`'s public API and drives a minimal draw→poll→dispatch loop over a 3-method `Screen` trait. No `Component`/`Action`/event-bus. Tools needing background channels or adaptive polling use `Tui` directly.

**Tech Stack:** Rust, ratatui 0.29 (re-exports `Frame`, `DefaultTerminal`, `try_init`/`try_restore`/`restore` at the crate root behind its `crossterm` feature), crossterm 0.28 (`event`, `EnableMouseCapture`/`DisableMouseCapture`, `is_terminal`). Tests use ratatui's `TestBackend`, matching the rest of the crate.

---

## Pre-flight: where things live

- Module root: `crates/suite-ui/src/app/mod.rs` — declares submodules, holds `Screen`/`Flow`/`App` re-exports.
- Guard: `crates/suite-ui/src/app/tui.rs` — `Tui`, `TuiOptions`, `TuiError`.
- Runner: `crates/suite-ui/src/app/runner.rs` — `App`, `Screen`, `Flow`, `dispatch_key`.
- Wiring: `crates/suite-ui/src/lib.rs` — `mod app;` + `pub use` + crate-doc lines.
- Example: `crates/suite-ui/examples/gallery.rs` — an `App`/`Screen` demo section.

All commands run from `~/projects/linux-ops-suite`. The crate is `suite-ui`; test it with `cargo test -p suite-ui`. The branch `feat/suite-ui-app-runtime` already exists (the spec is committed on it) — keep working on it.

---

### Task 1: `TuiError` — the error type with the friendly tty message

**Files:**
- Create: `crates/suite-ui/src/app/tui.rs`
- Create: `crates/suite-ui/src/app/mod.rs`
- Modify: `crates/suite-ui/src/lib.rs` (add `mod app;` and re-exports)

- [ ] **Step 1: Create the module root**

Create `crates/suite-ui/src/app/mod.rs`:

```rust
//! Shared App runtime: a terminal scope guard (the foundation every tool can
//! adopt) and a thin optional runner on top.
//!
//! - [`Tui`] is a RAII guard owning the terminal envelope (setup, panic-safe
//!   teardown via `Drop`, ordered post-exit stdout). Drive your own event loop
//!   with [`Tui::terminal`]; this is what tools with background channels or
//!   adaptive polling (RexOps, ScriptVault) use.
//! - [`App`] is a thin builder over `Tui` that runs a minimal
//!   draw→poll→dispatch loop for the simple case (`App::new(theme).run(root)`).
//!
//! There is no `Component`/`Action`/event-bus here by design — `App` is sugar,
//! `Tui` is the contract.

mod runner;
mod tui;

pub use runner::{App, Flow, Screen};
pub use tui::{Tui, TuiError, TuiOptions};
```

- [ ] **Step 2: Write the failing test for `TuiError` Display**

Create `crates/suite-ui/src/app/tui.rs`:

```rust
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_terminal_display_is_actionable() {
        let msg = TuiError::NotATerminal.to_string();
        assert!(msg.contains("interactive terminal"), "names the requirement");
        assert!(msg.contains("CLI"), "points at the non-interactive fallback");
    }
}
```

- [ ] **Step 3: Wire the module into the crate**

In `crates/suite-ui/src/lib.rs`, add `mod app;` to the module list (alongside the other `mod` lines, e.g. right after `mod attention_flag;`) and add this re-export with the others (after the `pub use attention_flag::AttentionFlag;` block is fine):

```rust
pub use app::{App, Flow, Screen, Tui, TuiError, TuiOptions};
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p suite-ui app::tui::tests::not_a_terminal_display_is_actionable`
Expected: PASS (1 passed).

- [ ] **Step 5: Verify the crate still builds clean**

Run: `cargo build -p suite-ui`
Expected: builds with no errors. (Unused-code warnings for not-yet-used items are fine at this stage.)

- [ ] **Step 6: Commit**

```bash
git add crates/suite-ui/src/app/mod.rs crates/suite-ui/src/app/tui.rs crates/suite-ui/src/lib.rs
git commit -m "feat(suite-ui): add TuiError + app module skeleton

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `TuiOptions` + `Tui` construction and the tty gate

**Files:**
- Modify: `crates/suite-ui/src/app/tui.rs`

The `require_tty` gate is the one construction behavior testable in CI (stdout is not a tty under `cargo test`), so it gets a real test. The success path (entering raw mode) can't run in CI; it's covered by the example and by manual verification.

- [ ] **Step 1: Write the failing test for the tty gate**

In `crates/suite-ui/src/app/tui.rs`, add to the `tests` module:

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p suite-ui app::tui::tests::require_tty_rejects_non_terminal_without_touching_setup`
Expected: FAIL — `TuiOptions` / `Tui` / `Tui::new` not found (does not compile).

- [ ] **Step 3: Add `TuiOptions`, the `Tui` struct, and `new`/`simple`**

In `crates/suite-ui/src/app/tui.rs`, add above the `tests` module:

```rust
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
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p suite-ui app::tui::tests::require_tty_rejects_non_terminal_without_touching_setup`
Expected: PASS.

- [ ] **Step 5: Run the whole crate's tests to confirm nothing regressed**

Run: `cargo test -p suite-ui`
Expected: all pass (existing widget tests + the two new `app::tui` tests).

- [ ] **Step 6: Commit**

```bash
git add crates/suite-ui/src/app/tui.rs
git commit -m "feat(suite-ui): Tui::new with TuiOptions + require_tty gate

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `terminal()` accessor, `print_after_exit`, and the `Drop` teardown

**Files:**
- Modify: `crates/suite-ui/src/app/tui.rs`

`Drop`'s real-stdout drain and terminal restore can't be asserted in CI (no tty for raw mode, and `DefaultTerminal` requires a real `CrosstermBackend<Stdout>` — a `TestBackend` won't fit the field). So the queue's *behavior* is factored into a free `drain_lines(out, writer)` function that `Drop` calls with `stdout()` and the test calls with an in-memory `Vec<u8>`. That's the actual unit under test; the terminal restore is covered by the example and manual run.

- [ ] **Step 1: Write the failing test for the queue drain**

In `crates/suite-ui/src/app/tui.rs` `tests` module, add:

```rust
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p suite-ui app::tui::tests::drain_lines_writes_each_in_order_then_empties`
Expected: FAIL — `drain_lines` not found (does not compile).

- [ ] **Step 3: Add the `terminal()` accessor and `print_after_exit`**

In `crates/suite-ui/src/app/tui.rs`, add these methods inside `impl Tui` (below `simple`):

```rust
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
```

- [ ] **Step 4: Add the `drain_lines` helper**

In `crates/suite-ui/src/app/tui.rs`, add this free function (non-test code, e.g. just below the `impl Tui` block):

```rust
/// Drain queued lines to a writer, each followed by a newline. Factored out of
/// `Drop` so it is unit-testable without a real terminal: `Drop` calls it with
/// `stdout()`, tests call it with an in-memory buffer.
fn drain_lines(out: &mut Vec<String>, w: &mut impl Write) {
    for line in out.drain(..) {
        let _ = writeln!(w, "{line}");
    }
}
```

- [ ] **Step 5: Add the `Drop` teardown**

In `crates/suite-ui/src/app/tui.rs`, add the `Drop` impl below the `drain_lines` helper:

```rust
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
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p suite-ui app::tui::tests::drain_lines_writes_each_in_order_then_empties`
Expected: PASS.

- [ ] **Step 7: Run the full crate tests**

Run: `cargo test -p suite-ui`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/suite-ui/src/app/tui.rs
git commit -m "feat(suite-ui): Tui terminal() accessor, print_after_exit, Drop teardown

Drop restores best-effort and drains queued stdout, skipping the drain
while panicking. drain_lines is factored out for unit testing.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `Tui::suspended` — leave/run/re-enter for shelling out

**Files:**
- Modify: `crates/suite-ui/src/app/tui.rs`

This wraps the leave→run→re-enter dance RexOps and ScriptVault both need. The closure form guarantees re-entry even if the child errors. The terminal calls can't run in CI; the test asserts the closure runs and its value is returned (the logic the tool relies on), using a path that doesn't touch raw mode.

- [ ] **Step 1: Write the failing test**

In the `tests` module of `tui.rs`:

```rust
    #[test]
    fn suspended_runs_closure_and_returns_value() {
        // suspended() must run the closure and hand back its return value. We
        // test the closure plumbing via the extracted `with_suspended` helper,
        // which takes the leave/re-enter actions as closures so the real
        // terminal ops aren't needed in CI.
        let mut left = 0;
        let mut reentered = 0;
        let value = with_suspended(
            || {
                left += 1;
                Ok::<(), io::Error>(())
            },
            || 42, // the "child" body
            || {
                reentered += 1;
                Ok::<(), io::Error>(())
            },
        )
        .unwrap();
        assert_eq!(value, 42, "returns the closure's value");
        assert_eq!((left, reentered), (1, 1), "leaves once, re-enters once");
    }

    #[test]
    fn suspended_reenters_even_if_leave_then_body_panics_path() {
        // If re-enter fails, the error surfaces; if leave fails, we still try to
        // re-enter so the terminal isn't stuck. Assert: leave error short-circuits
        // before the body but still re-enters.
        let mut reentered = 0;
        let result = with_suspended(
            || Err::<(), io::Error>(io::Error::other("leave failed")),
            || 7,
            || {
                reentered += 1;
                Ok::<(), io::Error>(())
            },
        );
        assert!(result.is_err(), "leave failure surfaces as an error");
        assert_eq!(reentered, 1, "re-enter still runs after a leave failure");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p suite-ui app::tui::tests::suspended`
Expected: FAIL — `with_suspended` not found.

- [ ] **Step 3: Add the `with_suspended` helper (the tested ordering logic)**

In `tui.rs`, add this free function as non-test code (e.g. below `drain_lines`). The tests target this; the public method below implements the same order inlined.

```rust
/// The leave→run→re-enter control flow, with the terminal ops injected as
/// closures so it is unit-testable without a real terminal. Re-enter ALWAYS
/// runs (even if leave failed), so the terminal is never left suspended. The
/// re-enter error dominates (current terminal state matters most); otherwise a
/// leave error propagates; otherwise the body's value is returned.
fn with_suspended<T>(
    leave: impl FnOnce() -> io::Result<()>,
    body: impl FnOnce() -> T,
    reenter: impl FnOnce() -> io::Result<()>,
) -> io::Result<T> {
    let leave_result = leave();
    let value = body();
    let reenter_result = reenter();
    reenter_result?;
    leave_result?;
    Ok(value)
}
```

- [ ] **Step 4: Add the public `suspended` method**

In `tui.rs`, add this method inside `impl Tui`. It implements the same leave→run→re-enter order as `with_suspended`, **inlined** rather than calling the helper: the leave and re-enter steps both need `&mut self.terminal`, which can't be held by two closures passed to one call at once. The helper exists so the ordering is unit-tested; this method is the real thing against `self`.

```rust
    /// Leave the alt screen + raw mode, run `f` on the user's real terminal,
    /// then re-enter and clear. Re-entry happens even if `f` returns or the
    /// leave step failed, so the terminal is never left in a suspended state.
    ///
    /// Use this to launch an editor or another full-screen child program.
    pub fn suspended<T>(&mut self, f: impl FnOnce() -> T) -> io::Result<T> {
        // Same ordering as `with_suspended` (unit-tested): leave, run, then
        // ALWAYS re-enter. Inlined because both steps need &mut self.terminal.
        let leave_result = self
            .terminal
            .show_cursor()
            .and_then(|()| ratatui::try_restore());

        let value = f();

        let reenter_result = (|| {
            self.terminal = ratatui::try_init()?;
            if self.opts.hide_cursor {
                self.terminal.hide_cursor()?;
            }
            self.terminal.clear()
        })();

        reenter_result?;
        leave_result?;
        Ok(value)
    }
```

- [ ] **Step 5: Run the helper tests to verify they pass**

Run: `cargo test -p suite-ui app::tui::tests::suspended`
Expected: both `suspended_*` tests PASS.

- [ ] **Step 6: Build to confirm the public method compiles**

Run: `cargo build -p suite-ui`
Expected: builds clean (no borrow errors).

- [ ] **Step 7: Run full crate tests**

Run: `cargo test -p suite-ui`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/suite-ui/src/app/tui.rs
git commit -m "feat(suite-ui): Tui::suspended for shelling out to a child program

Leave/run/re-enter with re-entry guaranteed even on failure. The ordering
logic is unit-tested via the with_suspended helper.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `Screen` trait, `Flow`, and the testable `dispatch_key`

**Files:**
- Create: `crates/suite-ui/src/app/runner.rs`
- Modify: `crates/suite-ui/src/app/mod.rs` (already references `runner` from Task 1)

The per-iteration decision is factored into `dispatch_key` so it's testable without a terminal. A fake `Screen` proves the dispatch contract.

- [ ] **Step 1: Write the failing test for `dispatch_key`**

Create `crates/suite-ui/src/app/runner.rs`:

```rust
//! `App`: a thin runner over [`Tui`](super::Tui) for the simple case — one
//! screen, one keymap (which IS `on_key`), no background channels, no adaptive
//! polling. Tools needing any of those drive `Tui` directly.

use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::tui::{Tui, TuiError, TuiOptions};
use crate::Theme;

/// What a key handler tells the loop to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep running.
    Continue,
    /// Quit the loop (and restore the terminal).
    Exit,
}

/// A drawable, key-driven root screen — the whole contract [`App`] needs. The
/// screen owns its own state and mutates it directly in `on_key`; there is no
/// `Action` indirection.
pub trait Screen {
    /// Draw the current state into the frame.
    fn render(&mut self, frame: &mut Frame, theme: Theme);
    /// Handle one key press; return [`Flow::Exit`] to quit.
    fn on_key(&mut self, key: KeyEvent) -> Flow;
    /// Called once per tick when no key arrived (e.g. clear a transient status).
    fn on_tick(&mut self) {}
}

/// One loop iteration's key decision, split out so it is testable without a
/// terminal: only Press events reach `on_key`; everything else is `Continue`.
fn dispatch_key(screen: &mut impl Screen, event: Event) -> Flow {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => screen.on_key(key),
        _ => Flow::Continue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

    /// A fake screen that quits on 'q' and counts calls.
    #[derive(Default)]
    struct Fake {
        keys_seen: usize,
        ticks: usize,
    }
    impl Screen for Fake {
        fn render(&mut self, _f: &mut Frame, _t: Theme) {}
        fn on_key(&mut self, key: KeyEvent) -> Flow {
            self.keys_seen += 1;
            if key.code == KeyCode::Char('q') {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
        fn on_tick(&mut self) {
            self.ticks += 1;
        }
    }

    fn press(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    #[test]
    fn dispatch_quits_on_exit_key_and_continues_otherwise() {
        let mut s = Fake::default();
        assert_eq!(dispatch_key(&mut s, press('j')), Flow::Continue);
        assert_eq!(dispatch_key(&mut s, press('q')), Flow::Exit);
        assert_eq!(s.keys_seen, 2, "both presses reached on_key");
    }

    #[test]
    fn dispatch_ignores_non_press_and_non_key_events() {
        let mut s = Fake::default();
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(dispatch_key(&mut s, release), Flow::Continue);
        assert_eq!(dispatch_key(&mut s, Event::FocusGained), Flow::Continue);
        assert_eq!(s.keys_seen, 0, "no non-press event reached on_key");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p suite-ui app::runner::tests`
Expected: FAIL initially only if `mod.rs` didn't already declare `runner` — but Task 1 created `mod.rs` with `mod runner;` and `pub use runner::{App, Flow, Screen};`. Since `App` doesn't exist yet, the crate won't compile.

To keep this task self-contained and compiling, temporarily narrow the re-export. In `crates/suite-ui/src/app/mod.rs`, change:

```rust
pub use runner::{App, Flow, Screen};
```

to:

```rust
pub use runner::{Flow, Screen};
```

and in `crates/suite-ui/src/lib.rs` change:

```rust
pub use app::{App, Flow, Screen, Tui, TuiError, TuiOptions};
```

to:

```rust
pub use app::{Flow, Screen, Tui, TuiError, TuiOptions};
```

Now run: `cargo test -p suite-ui app::runner::tests`
Expected: PASS (both dispatch tests). `App` is added in Task 6, which restores the export.

- [ ] **Step 3: Run full crate tests**

Run: `cargo test -p suite-ui`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/suite-ui/src/app/runner.rs crates/suite-ui/src/app/mod.rs crates/suite-ui/src/lib.rs
git commit -m "feat(suite-ui): Screen trait, Flow, and testable dispatch_key

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: `App` builder and `run()`

**Files:**
- Modify: `crates/suite-ui/src/app/runner.rs`
- Modify: `crates/suite-ui/src/app/mod.rs` (restore `App` export)
- Modify: `crates/suite-ui/src/lib.rs` (restore `App` export)

`run()`'s loop is terminal-bound (draw/poll/read), so it isn't unit-tested — its decision logic already lives in `dispatch_key` (Task 5, tested) and the teardown in `Tui::Drop` (Task 3). `run()` is the thin wiring between them, verified by the example and manual run.

- [ ] **Step 1: Add the `App` struct, builder, and `run`**

In `crates/suite-ui/src/app/runner.rs`, add above the `tests` module:

```rust
/// A thin runner over [`Tui`]. Construct with a [`Theme`], optionally tweak the
/// envelope and tick rate, then `run` a [`Screen`].
///
/// `run` drives the common-denominator loop: draw → poll(tick) → dispatch a key
/// (or `on_tick` on timeout) → repeat, with the terminal restored on the way
/// out by the underlying `Tui`. It deliberately has **no** background-channel
/// draining or adaptive polling — reach for [`Tui`] directly when you need
/// either (see RexOps/ScriptVault).
pub struct App {
    theme: Theme,
    opts: TuiOptions,
    tick: Duration,
}

impl App {
    /// Start from a resolved [`Theme`]. Defaults: hide the cursor, 200ms tick.
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            opts: TuiOptions {
                hide_cursor: true,
                ..Default::default()
            },
            tick: Duration::from_millis(200),
        }
    }

    /// Override the terminal envelope (mouse capture, require_tty, cursor).
    pub fn with_options(mut self, opts: TuiOptions) -> Self {
        self.opts = opts;
        self
    }

    /// Override the poll timeout / tick cadence (default 200ms).
    pub fn tick_rate(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Set up the terminal, run the loop to completion, and restore on exit.
    pub fn run(self, mut root: impl Screen) -> Result<(), TuiError> {
        let mut tui = Tui::new(self.opts)?;
        loop {
            tui.terminal()
                .draw(|f| root.render(f, self.theme))
                .map_err(TuiError::Io)?;
            if event::poll(self.tick).map_err(TuiError::Io)? {
                let ev = event::read().map_err(TuiError::Io)?;
                if dispatch_key(&mut root, ev) == Flow::Exit {
                    break;
                }
            } else {
                root.on_tick();
            }
        }
        Ok(()) // `tui` drops here → guaranteed restore
    }
}
```

- [ ] **Step 2: Restore the `App` re-exports**

In `crates/suite-ui/src/app/mod.rs`, change `pub use runner::{Flow, Screen};` back to:

```rust
pub use runner::{App, Flow, Screen};
```

In `crates/suite-ui/src/lib.rs`, change `pub use app::{Flow, Screen, Tui, TuiError, TuiOptions};` back to:

```rust
pub use app::{App, Flow, Screen, Tui, TuiError, TuiOptions};
```

- [ ] **Step 3: Build and run the full test suite**

Run: `cargo build -p suite-ui && cargo test -p suite-ui`
Expected: builds clean; all tests pass (the `App` type now resolves; `dispatch_key` tests still green).

- [ ] **Step 4: Confirm the public API is usable from outside the crate**

Run: `cargo build -p suite-ui --example gallery`
Expected: builds (the example doesn't use `App` yet — Task 7 — but this confirms the crate still compiles as a dependency).

- [ ] **Step 5: Commit**

```bash
git add crates/suite-ui/src/app/runner.rs crates/suite-ui/src/app/mod.rs crates/suite-ui/src/lib.rs
git commit -m "feat(suite-ui): App builder + run() over the Tui guard

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Crate-doc update + gallery demo

**Files:**
- Modify: `crates/suite-ui/src/lib.rs` (crate-level doc comment)
- Modify: `crates/suite-ui/examples/gallery.rs`

The gallery renders into a `TestBackend` and prints buffers — it can't run a live `App` loop (no real tty, and `run` blocks). So the demo shows the *type wiring*: a real `Screen` impl, rendered through the same `TestBackend` path the rest of the gallery uses, plus a non-running `App::new(theme)` construction to prove the builder is reachable. This matches the gallery's existing "in-memory smoke test" contract.

- [ ] **Step 1: Add the App/Screen section to the gallery**

In `crates/suite-ui/examples/gallery.rs`, extend the `use suite_ui::{...}` import to add `App, Flow, Screen` and `Theme` is already imported. Then add this demo function and call it once from `main` (after the theme loop, before/after is fine):

```rust
/// Demonstrates the App runtime's public surface: a real `Screen` implementation
/// rendered through the in-memory backend (the gallery never opens a real
/// terminal, and `App::run` would block, so we show the wiring, not a live loop).
fn demo_app_runtime(theme: suite_ui::Theme) {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui::widgets::Paragraph;
    use crossterm::event::{KeyCode, KeyEvent};

    struct Demo {
        message: String,
    }
    impl suite_ui::Screen for Demo {
        fn render(&mut self, frame: &mut ratatui::Frame, theme: suite_ui::Theme) {
            let block = suite_ui::pane("app runtime", theme);
            let inner = block.inner(frame.area());
            frame.render_widget(block, frame.area());
            frame.render_widget(Paragraph::new(self.message.as_str()), inner);
        }
        fn on_key(&mut self, key: KeyEvent) -> suite_ui::Flow {
            if key.code == KeyCode::Char('q') {
                suite_ui::Flow::Exit
            } else {
                suite_ui::Flow::Continue
            }
        }
    }

    // Construct an App to prove the builder is reachable from outside the crate.
    // We do not call `run()` (it would take over the real terminal and block).
    let _app = App::new(theme).tick_rate(std::time::Duration::from_millis(100));

    // Render the Screen once via the in-memory backend, like every other widget
    // in this gallery, so the demo participates in the visual smoke test.
    let mut screen = Demo {
        message: "Screen::render drew this through App's theme.".to_string(),
    };
    let mut terminal = Terminal::new(TestBackend::new(80, 6)).expect("test backend");
    terminal
        .draw(|frame| screen.render(frame, theme))
        .unwrap();
    print!("{}", buffer_to_string(terminal));
}
```

Add the call in `main` inside the existing theme loop, right after `print_frame(theme);`:

```rust
        demo_app_runtime(theme);
```

- [ ] **Step 2: Run the gallery to confirm it renders**

Run: `cargo run -p suite-ui --example gallery`
Expected: runs and prints frames for each theme, including a pane titled `app runtime` containing the demo message. No panic, exit 0.

- [ ] **Step 3: Update the crate-level doc comment**

In `crates/suite-ui/src/lib.rs`, in the top `//!` doc block, add a bullet to the feature list (after the existing widget bullets, before the "## Scope" section) and a short subsection. Add this bullet:

```rust
//! - a minimal App runtime — a RAII [`Tui`] terminal scope guard (setup +
//!   panic-safe teardown + ordered post-exit stdout) every tool can adopt, and
//!   a thin [`App`] runner (`App::new(theme).run(root)`) over a [`Screen`] for
//!   the simple case;
```

And add this subsection just before `## The clap feature`:

```rust
//! ## The App runtime: guard first, runner on top
//!
//! [`Tui`] owns the terminal *lifecycle* (mechanism every tool repeats), not
//! application logic. Drive your own loop via [`Tui::terminal`] when you need
//! background channels or adaptive polling; use [`App`] when you don't. `App` is
//! implemented entirely on `Tui`'s public API — anything it does, a hand-written
//! loop can do too.
```

- [ ] **Step 4: Verify docs build without warnings**

Run: `cargo doc -p suite-ui --no-deps`
Expected: builds; no broken intra-doc links (the `[`Tui`]`, `[`App`]`, `[`Screen`]`, `[`Tui::terminal`]` links resolve).

- [ ] **Step 5: Run the full test suite once more**

Run: `cargo test -p suite-ui`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/suite-ui/src/lib.rs crates/suite-ui/examples/gallery.rs
git commit -m "docs(suite-ui): document the App runtime + add a gallery demo

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Final verification + clippy

**Files:** none (verification only)

- [ ] **Step 1: Full workspace build**

Run: `cargo build`
Expected: the whole `linux-ops-suite` workspace builds clean.

- [ ] **Step 2: Full test run**

Run: `cargo test -p suite-ui`
Expected: all pass. Note the count of `app::` tests (should be: 1 TuiError + 1 tty gate + 1 drain + 2 suspended + 2 dispatch = 7 new tests).

- [ ] **Step 3: Clippy clean**

Run: `cargo clippy -p suite-ui --all-targets -- -D warnings`
Expected: no warnings. Fix any that appear (common ones: `needless_return`, `redundant_closure`) and re-run.

- [ ] **Step 4: Confirm the example still runs**

Run: `cargo run -p suite-ui --example gallery >/dev/null && echo OK`
Expected: `OK` (exit 0).

- [ ] **Step 5: Final commit if clippy required fixes**

```bash
git add -A
git commit -m "chore(suite-ui): clippy clean for the app runtime

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

If clippy required no changes, skip this step.

---

## Post-implementation

- This lands `suite_ui::app` on `feat/suite-ui-app-runtime`. Open a PR to `main` for **suite-ui only**.
- Per the suite CI sibling-ordering constraint, the new symbols (`Tui`, `App`, `Screen`, etc.) must be merged to `linux-ops-suite` `main` **before** any consumer PR that imports them can pass CI.
- Follow-up PRs, one each, in order: Bulwark (decide `App` vs `Tui` at migration), then RexOps onto `Tui`, then ScriptVault onto `Tui`. Those are out of scope for this plan.

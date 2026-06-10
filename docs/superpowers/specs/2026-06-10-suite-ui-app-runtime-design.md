# suite-ui — shared App runtime: terminal guard + thin runner

**Date:** 2026-06-10
**Status:** Proposed — `suite-ui` only this phase. Tool migration is the
documented follow-up (Bulwark first, then RexOps/ScriptVault), each its own PR.

## Summary

A new `suite_ui::app` module that removes the terminal-lifecycle boilerplate the
three consuming TUIs (Bulwark, RexOps, ScriptVault) each hand-roll today. It
ships **two layers**:

1. **`Tui` — a RAII scope guard** that owns the full terminal *envelope*:
   pre-flight tty check → setup (raw mode, alt screen, optional cursor-hide and
   mouse-capture) → a panic-safe, **guaranteed** teardown via `Drop` → an
   ordered post-exit stdout flush. The tool gets a live `DefaultTerminal` and
   keeps full control of its own event loop. This is the solid foundation **all
   three tools can adopt.**

2. **`App` — a thin, optional runner** built *entirely on top of* `Tui` that
   drives a minimal draw→poll→dispatch loop for the simple case
   (`App::new(theme).run(root)`). Useful for simpler tools (Bulwark); the
   stateful tools skip it and drive `Tui` directly.

This is deliberately **incremental**. There is **no** `Component` trait, no
`Action`/message enum, no event bus. The guard is RAII; the runner is ~25 lines
of loop. The module is roughly 150 lines total.

The crate's remit is unchanged in spirit — it still owns *shared mechanism, not
application logic*. The guard owns the terminal lifecycle (mechanism every tool
repeats); each tool still owns its own state, screens, rendering, and key
meaning.

## Motivation — what consumers duplicate today

Evidence gathered by reading every entry point and event loop in the three
repos.

Every tool's TUI entry follows the **same five-beat structure**:

1. **Pre-flight** — load data/config, resolve `suite_ui::Theme`, (Bulwark) check
   `is_terminal()`.
2. **Terminal setup** — `enable_raw_mode()` + `EnterAlternateScreen` + hide
   cursor.
3. **Panic safety** — restore the terminal if the loop panics.
4. **Event loop** — `draw` → drain background channels → `poll(timeout)` →
   dispatch key → repeat.
5. **Teardown** — `disable_raw_mode` + `LeaveAlternateScreen` + show cursor,
   *always* (even on error/panic), then flush any "picked"/printed stdout.

How each does beats 2/3/5 — the same job, three different (and unequal)
implementations:

| Concern            | Bulwark                      | RexOps                          | ScriptVault                        |
|--------------------|------------------------------|---------------------------------|------------------------------------|
| Setup/restore      | Manual `execute!` in `mod.rs`| Hand-rolled `setup`/`restore`   | `ratatui::init()` / `restore()`    |
| Panic safety       | `catch_unwind` around loop   | `std::panic::set_hook` (manual) | `ratatui::init()`'s built-in hook  |
| Mouse capture      | none                         | none                            | `Enable`/`DisableMouseCapture`     |
| Cursor             | hide                         | hide                            | shown (text input)                 |
| Suspend for child  | none                         | `run_foreground_child`          | inside `actions::perform`          |
| Post-exit stdout   | prints picked path           | none                            | `flush_printed_paths()`            |

Two facts drive the design:

- **`ratatui::try_init()` already does the baseline correctly** — raw mode + alt
  screen + a restoring panic hook (it calls `set_panic_hook()`; verified in the
  0.29 source). ScriptVault already leans on it; the other two reinvent it. So
  the guard should **reuse `try_init`/`try_restore`**, not re-implement them. Its
  value-add is the *rest* of the envelope ratatui leaves to the caller (tty
  check, cursor, mouse, ordered stdout flush) — which is exactly the part the
  three tools each rebuild differently.
- **Bulwark's `catch_unwind` is the weakest of the three** — it only restores
  around the one call it wraps, where a panic hook (or RAII `Drop`) restores on
  any unwind path. Consolidating raises Bulwark to the others' safety level.

The real, fragile duplication is **beats 2, 3, and 5** — ~40–60 lines copied
three ways. **Beat 4 (the loop) legitimately differs** (RexOps drains an
`mpsc<OpsSnapshot>` each tick; ScriptVault uses adaptive 40ms/3600s polling
gated on whether a live job is running; Bulwark has neither), so the guard does
**not** own the loop — the tool does.

## Design

A new module `suite_ui::app`, exported from `lib.rs`, with two layers. The
boundary that keeps it safe and additive: **`App` is implemented entirely in
terms of `Tui`'s public API** — no private back-channel. Anything `App` does, a
tool driving `Tui` by hand can also do. That is what lets RexOps and ScriptVault
skip `App` without losing anything.

### Layer 1 — `Tui`, the scope guard

A RAII guard: setup in the constructor, teardown in `Drop`. Teardown therefore
runs on **every** exit path — normal return, `?` error propagation, panic unwind
— because Rust runs `Drop` on unwind. This makes "always restore" structurally
impossible to forget and is strictly stronger than all three current approaches.

```rust
pub struct Tui {
    terminal: DefaultTerminal,
    opts: TuiOptions,
    out: Vec<String>,   // lines to print to real stdout after restore
}
```

#### Envelope options (configure the *envelope*, not the loop)

The tools differ only in which envelope features they want, so the builder
configures exactly those — nothing more.

```rust
#[derive(Default, Clone, Copy)]
pub struct TuiOptions {
    pub hide_cursor: bool,    // Bulwark, RexOps: true; ScriptVault: false
    pub mouse_capture: bool,  // ScriptVault: true; others: false
    pub require_tty: bool,    // Bulwark wants the friendly early error
}
```

#### Construction

```rust
impl Tui {
    /// Resolution order, honest about failure:
    ///   1. if require_tty && !stdout().is_terminal() -> Err(NotATerminal)
    ///   2. ratatui::try_init()            // raw mode + alt screen + panic hook
    ///   3. if hide_cursor   -> terminal.hide_cursor()
    ///   4. if mouse_capture -> execute!(stdout(), EnableMouseCapture)
    pub fn new(opts: TuiOptions) -> Result<Self, TuiError>;

    /// `Tui::new(TuiOptions::default())` — bare alt-screen + panic hook.
    pub fn simple() -> Result<Self, TuiError>;
}
```

#### Borrow the terminal — the guard gets out of the way

```rust
impl Tui {
    /// Borrow the terminal to drive your own loop. The escape hatch:
    /// RexOps and ScriptVault call this and keep their existing loops verbatim.
    pub fn terminal(&mut self) -> &mut DefaultTerminal;
}
```

A tool with a custom loop:

```rust
let mut tui = Tui::new(TuiOptions { hide_cursor: true, ..Default::default() })?;
let result = my_event_loop(tui.terminal(), &mut app, &rx, theme);
result   // no teardown line — `tui` drops here, restoring the terminal
```

#### Suspend / resume — for tools that shell out

RexOps and ScriptVault both drop out of the alt screen to run a child (editor,
specialist tool) and re-enter after. A real shared need, owned as a scoped
helper so the leave/re-enter symmetry can't be broken:

```rust
impl Tui {
    /// Leave the alt screen + raw mode, run `f` on the user's real terminal,
    /// then re-enter and clear. Restores correctly even if `f` panics or errors.
    pub fn suspended<T>(&mut self, f: impl FnOnce() -> T) -> io::Result<T>;
}
```

This folds RexOps' `suspend_terminal_for_child`/`resume_terminal_after_child`
pair and ScriptVault's in-`actions` equivalent into one correct implementation.

#### Post-exit stdout — the "picked path" pattern

Bulwark prints a picked path and ScriptVault flushes printed paths, both *after*
the screen is restored (so the text lands in the shell, not the alt screen). The
guard owns the ordering:

```rust
impl Tui {
    /// Queue a line to print to real stdout AFTER the terminal is restored.
    pub fn print_after_exit(&mut self, line: impl Into<String>);
}
```

#### `Drop` — the whole guarantee

```rust
impl Drop for Tui {
    fn drop(&mut self) {
        // 1. show cursor / disable mouse if we enabled them (best-effort)
        // 2. ratatui::restore()   // disable raw mode + leave alt screen
        // 3. if !std::thread::panicking() { drain self.out to stdout() }
    }
}
```

Terminal calls are best-effort (matching every current tool — a restore error
mid-unwind can't be meaningfully handled). The stdout drain is real, **but is
skipped on the panic path** via `std::thread::panicking()`: on a crash you
picked nothing, so a queued result must not print. This needs nothing from the
tool — no flag to set. (On panic, ratatui's hook restores during unwind and
`Drop` then calls `ratatui::restore()` again; that is idempotent and harmless.)

#### Errors

```rust
pub enum TuiError {
    NotATerminal,        // require_tty failed; Display carries the friendly text
    Io(std::io::Error),  // setup failure (raw mode, etc.)
}
```

`NotATerminal`'s `Display` carries Bulwark's actionable "use the CLI instead"
message so the tool doesn't re-author it. `TuiError` implements
`std::error::Error` so it slots into both consumer styles (`anyhow` and
`Box<dyn Error>`).

### Layer 2 — `App`, the thin runner

The `App::new(theme).run(root)` convenience, for the simple case: one screen,
one keymap, no background channels, no adaptive polling. Bulwark fits exactly;
RexOps and ScriptVault keep driving `Tui` by hand.

#### The trait a tool implements

```rust
/// A drawable, key-driven root screen. The whole contract App needs.
pub trait Screen {
    /// Draw the current state into the frame.
    fn render(&mut self, frame: &mut Frame, theme: Theme);

    /// Handle one key press. Return Flow::Exit to quit the loop.
    fn on_key(&mut self, key: KeyEvent) -> Flow;

    /// Called once per tick when no key arrived (timer-driven redraws,
    /// clearing a transient status line). Default: do nothing.
    fn on_tick(&mut self) {}
}

pub enum Flow { Continue, Exit }
```

Three methods, one defaulted. **No `Action` enum, no message dispatch** — the
screen owns its state and mutates it directly in `on_key`. The keymap *is*
`on_key`: rather than a separate `.with_keymap(k)` builder step (which would mean
two places handle keys), key→behavior lives in the one method. This honors "no
Component/Action architecture yet" literally.

#### The builder

```rust
pub struct App {
    theme: Theme,
    opts: TuiOptions,
    tick: Duration,
}

impl App {
    pub fn new(theme: Theme) -> Self;                  // defaults: hide_cursor, 200ms tick
    pub fn with_options(self, opts: TuiOptions) -> Self;
    pub fn tick_rate(self, d: Duration) -> Self;
    pub fn run(self, root: impl Screen) -> Result<(), TuiError>;
}
```

#### The loop `run()` drives — the common denominator, on purpose

```rust
pub fn run(self, root: impl Screen) -> Result<(), TuiError> {
    let mut tui = Tui::new(self.opts)?;
    let mut root = root;
    loop {
        tui.terminal().draw(|f| root.render(f, self.theme))?;
        if event::poll(self.tick)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    if let Flow::Exit = root.on_key(k) { break; }
                }
            }
        } else {
            root.on_tick();   // timeout elapsed = one tick
        }
    }
    Ok(())   // `tui` drops here → guaranteed restore
}
```

No channel draining, no adaptive timeout. **A tool that needs either does not
use `run()`** — it uses `Tui` directly. `run()`'s doc comment says exactly that,
with a one-line "reach for `Tui` directly when you need background channels or
adaptive polling" pointer.

### The two layers side by side

```rust
// Simple tool (Bulwark) — App owns everything:
App::new(theme)
    .with_options(TuiOptions { require_tty: true, ..Default::default() })
    .run(RootScreen::new(entries))?;

// Stateful tool (RexOps) — Tui guard, own loop, full control:
let mut tui = Tui::new(TuiOptions { hide_cursor: true, ..Default::default() })?;
my_loop(tui.terminal(), &mut app, &rx, theme)?;   // drains mpsc, adaptive poll
```

Same envelope guarantees for both; the only difference is who owns the loop.

## Scope

**This phase — `suite-ui` only, one PR:**

- The `suite_ui::app` module: `Tui`, `TuiOptions`, `TuiError`, `Screen`, `Flow`,
  `App`.
- Exports added to `lib.rs` and documented in the crate-level doc comment.
- Unit tests (see Testing).
- A gallery example entry (or a small dedicated example) demonstrating
  `App::new(theme).run(root)` with a trivial screen.
- A `~/bin/` Rust wrapper is **not** applicable here — this is a library module,
  not a new CLI tool.

**Follow-up phases — one PR each, in this order (CI sibling-ordering: new
suite-ui symbols must be on `main` before a consumer PR's CI can pass):**

1. **Bulwark** onto `App` (the simple-case proof) — and/or onto `Tui` if its
   "pick path" stdout flush reads better with the guard directly.
2. **RexOps** onto `Tui` (keeps its mpsc-draining, adaptive loop; replaces
   `setup_terminal`/`restore_terminal`/the manual panic hook/`run_foreground_child`).
3. **ScriptVault** onto `Tui` (keeps its adaptive-poll live-run loop; replaces
   the `ratatui::init`/`restore` + mouse-capture + `flush_printed_paths`
   bookends, and routes its child-suspend through `Tui::suspended`).

## Non-goals

- No `Component` trait, no `Action`/message enum, no event-bus or reducer.
- `App` does **not** grow knobs for background channels or adaptive polling —
  that is the explicit signal to use `Tui` directly.
- No change to existing `suite-ui` widgets, theme, or overlays.
- No mouse handling *abstraction* — `Tui` only enables/disables capture; what a
  tool does with mouse events stays in the tool.
- Not migrating any tool in this phase.

## Testing

The guard's terminal side effects can't be asserted against a real TTY in CI,
so tests target the parts that carry logic, using ratatui's `TestBackend`
pattern already used across the crate:

- **`TuiError::NotATerminal` Display** contains the actionable CLI-fallback text.
- **`require_tty` gate**: with stdout not a terminal (the CI case),
  `Tui::new(TuiOptions { require_tty: true, .. })` returns `Err(NotATerminal)`
  *without* having touched raw mode. (In CI stdout is not a tty, so this is
  directly assertable.)
- **`print_after_exit` queues, doesn't print eagerly**: after queueing, the
  `out` buffer holds the line (assert via a test-only accessor or by observing
  no stdout write until drop) — full drop-drain is covered by manual/--example
  verification since it writes real stdout.
- **`Screen` + `Flow` dispatch**: tested *without* the real terminal by
  factoring the per-iteration decision into a free function — e.g.
  `fn step(screen, key) -> Flow` (or a `dispatch_key` helper `App::run` calls).
  A fake `Screen` (counting `render`/`on_key` calls) asserts: a key mapped to
  `Flow::Exit` yields `Exit`; an unmapped/`Continue` key yields `Continue`;
  `on_tick` fires on the no-key branch. The terminal-bound `draw`/`poll`/`read`
  wrapper around that helper stays thin and is covered by the example, not unit
  tests.
- **`std::thread::panicking()` skip path**: documented and reviewed; not
  unit-tested (can't assert real-stdout absence mid-unwind portably).

Where a behavior genuinely needs a live terminal (the drop-drain to real stdout,
cursor/mouse restore), it is verified by running the gallery/example by hand and
noted as such — not faked into a passing assertion.

## Risks / trade-offs

- **`App::run` loop verification is shallow** — its value is that it's trivial;
  deep testing belongs to the tools. Accepted: the guard (the part with the real
  guarantee) is what's covered hardest.
- **Double-restore on panic** — harmless (idempotent), documented inline so a
  future reader doesn't "fix" it by removing the hook reliance.
- **`Tui` is not `Send`** (holds `DefaultTerminal`/stdout) — fine; it lives on
  the main thread by construction. Worker threads communicate via the tool's own
  channels, exactly as today.

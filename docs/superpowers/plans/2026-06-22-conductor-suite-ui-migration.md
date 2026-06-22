# Conductor → suite-ui Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild Conductor's interactive TUI on `suite_ui::Tui` + `ratatui` + the shared `Theme`/`pane`/`ConfirmModal`/`HelpSheet` widgets so it presents the same clean, full-screen interface as RexOps, while preserving Conductor's behaviour and exit-code contract exactly.

**Architecture:** Replace everything under `crates/conductor/src/tui/`. The domain layer (`plan/`, `state.rs`, `sources.rs`, `run.rs`, `report.rs`) is untouched. The tested state machine (key→action, the Ring-2 confirm gate, exit-code tally) is ported from the old `tui/mod.rs` into a new `tui/app.rs` driven by `crossterm::KeyEvent` (via `suite_ui::keys`). A new `tui/runtime.rs` owns the `Tui` guard + dirty-flag draw/event loop and forwards child spawns to `Tui::suspended` through a one-method `Spawner` adapter. A new `tui/render.rs` draws the plan screen + confirm/help overlays in the RexOps look.

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28, suite-ui (workspace path dep, re-exporting thomas-tui's `Tui`, `Theme`, `pane`, `ConfirmModal`, `HelpSheet`, `EmptyState`, `keys`).

## Global Constraints

- Crate: `crates/conductor` — a workspace member of `linux-ops-suite`. Run all commands from the worktree root.
- Edition 2021, `rust-version = 1.85` (workspace). suite-ui/ratatui/crossterm already build in this workspace — no MSRV regression.
- Dependencies are added as `{ workspace = true }` (path deps). NO git pin (that is only for the external `rexops` repo).
- The Ring-2 safety gate is the crate's core property: a `ChangesState` step must NEVER spawn on Enter — it opens the confirm; only `y` runs it. Ported tests guard this.
- Exit-code contract: `1` = a step failed, `2` = quit with steps pending/skipped, `0` = clean / all done / nothing to do. Unchanged.
- Scriptable paths unchanged: non-TTY / `--json` bare invocation falls back to `status`; `status`/`plan`/`health`/`--json` never touch the TUI.
- NO new screens, NO command palette, NO background refresh, NO mouse. (YAGNI — Conductor has one screen + two overlays.)
- Colour is owned by `suite_ui::Theme` (handles `NO_COLOR`); the old `--no-color` flag maps to `Theme::with_color(false)` vs `Theme::resolve(ColorChoice::Auto, ThemeChoice::Cyan)`.
- No-panic floor in the new TUI files: prefer `?`/match over `unwrap`/`expect` in non-test code (mirror rexops-tui's `#![deny(clippy::unwrap_used, clippy::expect_used)]` discipline at function level; do not add the crate-wide attribute since `report.rs`/`sources.rs` are out of scope).

## Reference implementations (read before coding)

- `~/projects/rexops/crates/rexops-tui/src/lib.rs` — `Tui::new(TuiOptions{..})`, `ForegroundRunner for Tui` via `Tui::suspended`, `Theme::resolve`.
- `~/projects/rexops/crates/rexops-tui/src/runtime.rs` — the dirty-flag draw/event loop (`run` + `step`).
- `~/projects/rexops/crates/rexops-tui/src/screens/launchpad.rs` — `pane(...)`, the `▌ ` selection rail (`theme.selected_rail()`), row composition, and the `TestBackend`→string render-test helper.
- suite-ui exports: `pane`, `pane_blank`, `Theme`, `ConfirmModal{title,message}`, `HelpSheet{title,rows:&[(&str,&str)]}`, `EmptyState{message,..}`, `keys::{is_cancel,is_confirm}`, `Tui`, `TuiOptions`, `ColorChoice`, `ThemeChoice`.

---

## Task 1: Add suite-ui / ratatui / crossterm to conductor's Cargo.toml

**Files:**
- Modify: `crates/conductor/Cargo.toml`

**Interfaces:**
- Consumes: workspace deps `suite-ui`, `ratatui`, `crossterm` (already declared in the umbrella root `[workspace.dependencies]`).
- Produces: the `suite_ui`, `ratatui`, `crossterm` crates become importable from `conductor`.

- [ ] **Step 1: Add the three dependencies**

In `crates/conductor/Cargo.toml`, under `[dependencies]` (after the existing `chrono` line), add:

```toml
# Shared suite-wide TUI: the same ratatui-based chrome (Theme/pane/overlays + the
# Tui terminal guard) that RexOps/Pulse/ScriptVault render from, so Conductor's
# interactive view matches the rest of the suite. Workspace path dep — no git pin
# (that is only for repos outside this workspace).
suite-ui = { workspace = true }
ratatui = { workspace = true }
crossterm = { workspace = true }
```

- [ ] **Step 2: Verify it resolves**

Run: `cargo metadata --no-deps --format-version 1 -q >/dev/null && cargo tree -p conductor -i suite-ui -q | head -1`
Expected: prints `suite-ui v…` (the dep graph resolves; no error).

- [ ] **Step 3: Commit**

```bash
git add crates/conductor/Cargo.toml
git commit -m "build(conductor): depend on suite-ui/ratatui/crossterm for the TUI migration"
```

---

## Task 2: New `tui/app.rs` — port the state machine to crossterm KeyEvent

This task moves the tested state machine out of the old `tui/mod.rs` into `tui/app.rs`, re-expressed against `crossterm::event::KeyEvent` instead of the deleted `term::Key`. Behaviour and assertions are preserved. The old `tui/mod.rs`, `tui/term.rs`, `tui/style.rs`, `tui/frame.rs` are NOT deleted yet (Task 5 wires + deletes) so the crate still builds between tasks — `app.rs` is added as a sibling and not yet referenced.

**Files:**
- Create: `crates/conductor/src/tui/app.rs`
- (Do not modify `tui/mod.rs` yet.)

**Interfaces:**
- Consumes: `crate::plan::{Plan, Ring, StepStatus}`, `crate::run::{run_step, confirm_command, RunOutcome, Spawner}`, `crate::sources::is_on_path`, `crossterm::event::{KeyCode, KeyEvent}`, `suite_ui::keys`.
- Produces:
  - `pub enum Screen { Plan, Help, Confirm }`
  - `pub enum Action { Redraw, Quit }`
  - `pub struct RunReport { pub failed: usize, pub unfinished: usize }` with `pub fn exit_code(&self) -> u8`
  - `pub fn report_from(plan: &Plan) -> RunReport`
  - `pub struct App { pub plan: Plan, pub cursor: usize, pub screen: Screen, pub notice: Option<String> }` with `pub fn new(plan: Plan) -> Self`
  - `pub fn step(app: &mut App, key: KeyEvent, spawner: &dyn Spawner) -> Action`

- [ ] **Step 1: Write the failing tests**

Create `crates/conductor/src/tui/app.rs` with ONLY the test module first (so it fails to compile/run for the right reason — missing items). Put this at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::run::Spawner;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::cell::RefCell;
    use std::process::ExitStatus;

    fn k(c: char) -> KeyEvent { KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE) }
    fn enter() -> KeyEvent { KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE) }
    fn esc() -> KeyEvent { KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE) }

    struct FakeSpawner { calls: RefCell<Vec<Vec<String>>> }
    impl FakeSpawner { fn new() -> Self { FakeSpawner { calls: RefCell::new(Vec::new()) } } }
    impl Spawner for FakeSpawner {
        fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
            self.calls.borrow_mut().push(argv.to_vec());
            std::process::Command::new("true").status()
        }
    }
    struct ExitSpawner { success: bool }
    impl Spawner for ExitSpawner {
        fn spawn(&self, _argv: &[String]) -> std::io::Result<ExitStatus> {
            use std::os::unix::process::ExitStatusExt;
            Ok(ExitStatus::from_raw(if self.success { 0 } else { 1 << 8 }))
        }
    }

    /// Plan: refresh (Ring2) → capture (Ring2) → investigate (Ring1).
    fn sample() -> Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(), why: "key".into(),
            source: "bulwark".into(), severity: Severity::Critical,
        });
        plan::build(&s)
    }

    fn with_path<F: FnOnce()>(bin: &str, body: &str, f: F) {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join(bin);
        std::fs::write(&stub, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let orig = std::env::var_os("PATH");
        let mut v = std::ffi::OsString::from(dir.path());
        if let Some(p) = &orig { v.push(":"); v.push(p); }
        std::env::set_var("PATH", &v);
        f();
        match orig { Some(v) => std::env::set_var("PATH", v), None => std::env::remove_var("PATH") }
    }

    #[test]
    fn q_quits() {
        let mut app = App::new(sample());
        assert_eq!(step(&mut app, k('q'), &FakeSpawner::new()), Action::Quit);
    }

    #[test]
    fn esc_quits_from_plan() {
        let mut app = App::new(sample());
        assert_eq!(step(&mut app, esc(), &FakeSpawner::new()), Action::Quit);
    }

    #[test]
    fn a_advances_focus_without_running() {
        let mut app = App::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, k('a'), &sp);
        assert_eq!(app.cursor, 1);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn s_skips_and_advances() {
        let mut app = App::new(sample());
        step(&mut app, k('s'), &FakeSpawner::new());
        assert_eq!(app.plan.steps[0].status, StepStatus::Skipped);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn enter_on_ring2_opens_confirm_and_spawns_nothing() {
        let mut app = App::new(sample());
        let sp = FakeSpawner::new();
        let action = step(&mut app, enter(), &sp);
        assert_eq!(action, Action::Redraw);
        assert_eq!(app.screen, Screen::Confirm);
        assert_eq!(app.plan.steps[0].status, StepStatus::Pending);
        assert!(sp.calls.borrow().is_empty(), "opening confirm must not spawn");
    }

    #[test]
    fn confirm_y_spawns_once_marks_done_and_returns_to_plan() {
        with_path("workstate", "#!/bin/sh\nexit 0\n", || {
            let mut app = App::new(sample());
            let sp = FakeSpawner::new();
            step(&mut app, enter(), &sp);          // open confirm
            assert_eq!(app.screen, Screen::Confirm);
            step(&mut app, k('y'), &sp);           // confirm + run
            assert_eq!(app.screen, Screen::Plan);
            assert_eq!(app.plan.steps[0].status, StepStatus::Done);
            assert_eq!(app.cursor, 1);
            assert_eq!(sp.calls.borrow().len(), 1);
            assert_eq!(sp.calls.borrow()[0][0], "workstate");
        });
    }

    #[test]
    fn confirm_non_y_keys_never_spawn_and_back_out() {
        for key in [k('q'), esc(), enter(), k('z')] {
            let mut app = App::new(sample());
            let sp = FakeSpawner::new();
            step(&mut app, enter(), &sp);          // open confirm
            assert_eq!(app.screen, Screen::Confirm);
            step(&mut app, key, &sp);              // decline
            assert_eq!(app.screen, Screen::Plan, "{key:?} must return to plan");
            assert_eq!(app.plan.steps[0].status, StepStatus::Pending);
            assert_eq!(app.cursor, 0, "{key:?} must not advance");
            assert!(sp.calls.borrow().is_empty(), "{key:?} must not spawn");
        }
    }

    #[test]
    fn confirm_s_skips_and_advances_without_spawning() {
        let mut app = App::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, enter(), &sp);
        step(&mut app, k('s'), &sp);
        assert_eq!(app.screen, Screen::Plan);
        assert_eq!(app.plan.steps[0].status, StepStatus::Skipped);
        assert_eq!(app.cursor, 1);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn a_failing_spawn_marks_failed_and_stays_put() {
        with_path("bulwark", "#!/bin/sh\nexit 1\n", || {
            let mut app = App::new(sample());
            let last = app.plan.steps.len() - 1; // Ring-1 investigate
            app.cursor = last;
            let sp = ExitSpawner { success: false };
            step(&mut app, enter(), &sp);
            assert_eq!(app.plan.steps[last].status, StepStatus::Failed);
            assert_eq!(app.cursor, last, "a failed step does not advance");
            assert!(app.notice.is_some());
        });
    }

    #[test]
    fn enter_on_ring1_spawns_marks_done_and_advances() {
        with_path("bulwark", "#!/bin/sh\nexit 0\n", || {
            let mut app = App::new(sample());
            let last = app.plan.steps.len() - 1;
            app.cursor = last;
            let sp = FakeSpawner::new();
            step(&mut app, enter(), &sp);
            assert_eq!(app.plan.steps[last].status, StepStatus::Done);
            assert_eq!(sp.calls.borrow().len(), 1);
            assert_eq!(sp.calls.borrow()[0][0], "bulwark");
        });
    }

    #[test]
    fn question_toggles_help_and_any_key_returns() {
        let mut app = App::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, k('?'), &sp);
        assert_eq!(app.screen, Screen::Help);
        step(&mut app, k('x'), &sp);
        assert_eq!(app.screen, Screen::Plan);
    }

    #[test]
    fn notice_clears_on_next_key() {
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let orig = std::env::var_os("PATH");
        std::env::set_var("PATH", "/nonexistent-conductor-test-dir");
        let mut app = App::new(sample());
        let last = app.plan.steps.len() - 1;
        app.cursor = last;
        let sp = FakeSpawner::new();
        step(&mut app, enter(), &sp);      // bulwark not on PATH -> notice
        assert!(app.notice.is_some());
        step(&mut app, k('a'), &sp);       // any key clears it
        assert!(app.notice.is_none());
        match orig { Some(v) => std::env::set_var("PATH", v), None => std::env::remove_var("PATH") }
    }

    #[test]
    fn run_report_maps_statuses_to_exit_codes() {
        assert_eq!(RunReport { failed: 0, unfinished: 0 }.exit_code(), 0);
        assert_eq!(RunReport { failed: 1, unfinished: 3 }.exit_code(), 1);
        assert_eq!(RunReport { failed: 0, unfinished: 2 }.exit_code(), 2);
    }

    #[test]
    fn report_from_plan_counts_failed_and_unfinished() {
        let mut app = App::new(sample());
        app.plan.steps[0].status = StepStatus::Failed;
        app.plan.steps[1].status = StepStatus::Skipped;
        let r = report_from(&app.plan);
        assert_eq!(r.failed, 1);
        assert!(r.unfinished >= 1);
        assert_eq!(r.exit_code(), 1);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p conductor --lib tui::app 2>&1 | tail -20`
Expected: compile error — `cannot find type App` / `function step` not found (the impl block isn't written yet).

- [ ] **Step 3: Write the implementation (top of the same file, above the test module)**

Insert at the TOP of `crates/conductor/src/tui/app.rs`:

```rust
//! Conductor's interactive state machine: App state + key→action transitions.
//!
//! Pure with respect to the terminal — `step` never touches stdin/stdout, it
//! mutates `App` and returns an `Action`. All rendering lives in `render`, the
//! draw/event loop and terminal guard in `runtime`. Spawning a step is delegated
//! to the `Spawner` (the real loop passes one that suspends the TUI for the
//! child). Ported from the pre-suite-ui `tui/mod.rs`; the key type is now
//! crossterm's `KeyEvent` so the same logic runs against the shared input stack.

use crossterm::event::{KeyCode, KeyEvent};

use crate::plan::{Plan, Ring, StepStatus};
use crate::run::{confirm_command, run_step, RunOutcome, Spawner};

/// Which screen/overlay is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Plan,
    Help,
    /// The Ring-2 confirm modal for the cursor step. `y` runs it; anything else
    /// (incl. Enter) backs out without running.
    Confirm,
}

/// Whether the loop should repaint and continue, or exit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Redraw,
    Quit,
}

/// The outcome of a guided run, mapped to a process exit code. `failed` counts
/// steps whose delegated command exited non-zero; `unfinished` counts steps left
/// Pending or Skipped. A failure outranks unfinished.
pub struct RunReport {
    pub failed: usize,
    pub unfinished: usize,
}

impl RunReport {
    pub fn exit_code(&self) -> u8 {
        if self.failed > 0 {
            1
        } else if self.unfinished > 0 {
            2
        } else {
            0
        }
    }
}

/// Tally a plan's final step statuses into a `RunReport`.
pub fn report_from(plan: &Plan) -> RunReport {
    let mut failed = 0;
    let mut unfinished = 0;
    for s in &plan.steps {
        match s.status {
            StepStatus::Failed => failed += 1,
            StepStatus::Pending | StepStatus::Skipped => unfinished += 1,
            StepStatus::Done => {}
        }
    }
    RunReport { failed, unfinished }
}

/// All interactive state. The plan's per-step `status` carries Done/Skipped;
/// `cursor` is the focused (▸) step; `notice` is a transient one-liner cleared on
/// the next keypress.
pub struct App {
    pub plan: Plan,
    pub cursor: usize,
    pub screen: Screen,
    pub notice: Option<String>,
}

impl App {
    pub fn new(plan: Plan) -> Self {
        App {
            plan,
            cursor: 0,
            screen: Screen::Plan,
            notice: None,
        }
    }

    fn advance(&mut self) {
        if self.cursor + 1 < self.plan.steps.len() {
            self.cursor += 1;
        }
    }
}

/// Apply one key to the state, using `spawner` for any run. Pure with respect to
/// the terminal, so it is fully unit-testable. The real loop wraps `spawner` so a
/// spawn suspends the TUI.
pub fn step(app: &mut App, key: KeyEvent, spawner: &dyn Spawner) -> Action {
    // Any key clears a stale notice first; specific arms may set a fresh one.
    app.notice = None;

    if app.screen == Screen::Confirm {
        return confirm_key(app, key, spawner);
    }

    if app.screen == Screen::Help {
        // In help, any key returns to the plan (q still quits).
        if is_quit(key) {
            return Action::Quit;
        }
        app.screen = Screen::Plan;
        return Action::Redraw;
    }

    // Plan screen.
    if is_quit(key) {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Char('?') => {
            app.screen = Screen::Help;
            Action::Redraw
        }
        KeyCode::Char('a') => {
            app.advance();
            Action::Redraw
        }
        KeyCode::Char('s') => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Skipped;
            }
            app.advance();
            Action::Redraw
        }
        KeyCode::Char('r') => {
            // Hand off to the rexops cockpit if present; else a dim note. A
            // failure here (suspend/re-enter around the child broke) must be
            // shown, not swallowed — otherwise the terminal looks frozen.
            if crate::sources::is_on_path("rexops") {
                if let Err(e) = spawner.spawn(&["rexops".to_string(), "tui".to_string()]) {
                    app.notice = Some(format!("rexops handoff failed ({e}); press a key to redraw"));
                }
            } else {
                app.notice = Some("rexops is not on PATH".to_string());
            }
            Action::Redraw
        }
        KeyCode::Enter => {
            // A changes-state step never fires on Enter — it opens the confirm.
            // Every other runnable step runs immediately.
            let opens_confirm = app
                .plan
                .steps
                .get(app.cursor)
                .map(|s| s.ring == Ring::ChangesState && confirm_command(s).is_some())
                .unwrap_or(false);
            if opens_confirm {
                app.screen = Screen::Confirm;
            } else {
                run_current(app, spawner);
            }
            Action::Redraw
        }
        _ => Action::Redraw,
    }
}

/// `q`, Ctrl-C, or (on the plan screen) Esc/Ctrl-G all quit. We treat
/// `suite_ui::keys::is_cancel` (Esc OR Ctrl-G) as quit on the plain plan screen,
/// matching the old `Key::Esc => Quit`.
fn is_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q')) || suite_ui::keys::is_cancel(key)
}

/// Handle a key while the Ring-2 confirm modal is showing. `y` runs the cursor
/// step (the ONLY spawn trigger); `s` skips it; anything else — including a stray
/// Enter — backs out to the plan without running. This is the core safety gate.
fn confirm_key(app: &mut App, key: KeyEvent, spawner: &dyn Spawner) -> Action {
    match key.code {
        KeyCode::Char('y') => {
            run_current(app, spawner);
            app.screen = Screen::Plan;
            Action::Redraw
        }
        KeyCode::Char('s') => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Skipped;
            }
            app.advance();
            app.screen = Screen::Plan;
            Action::Redraw
        }
        // q, Esc, Enter, any other key: decline, run nothing, back to the plan.
        _ => {
            app.screen = Screen::Plan;
            Action::Redraw
        }
    }
}

/// Run the focused step (Enter on a runnable step, or `y` in the confirm). On
/// success: mark Done + advance. On a non-zero exit: mark Failed + stay. The
/// Ring-2 gate has already happened by the time this runs for a changes-state
/// step.
fn run_current(app: &mut App, spawner: &dyn Spawner) {
    let Some(step_ref) = app.plan.steps.get(app.cursor) else {
        return;
    };
    let ring = step_ref.ring;
    match run_step(step_ref, spawner) {
        RunOutcome::Ran(true) => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Done;
            }
            app.advance();
        }
        RunOutcome::Ran(false) => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Failed;
            }
            app.notice = Some("that step failed (the tool exited non-zero)".to_string());
        }
        RunOutcome::NotAvailable(bin) => {
            app.notice = Some(format!("{bin} is not on PATH — install it first"));
        }
        RunOutcome::SpawnError(e) => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Failed;
            }
            app.notice = Some(format!(
                "could not run that step ({e}); the terminal may need a redraw — press a key"
            ));
        }
        RunOutcome::NotRunnable => {
            if ring == Ring::Info {
                app.notice = Some("informational — run the shown command yourself".to_string());
            }
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p conductor --lib tui::app 2>&1 | tail -20`
Expected: all `tui::app::tests::*` PASS. (The crate still has the OLD `tui/mod.rs` etc.; `app.rs` isn't referenced yet — add `pub mod app;` only if the build complains it's an unused file. It will not: a file not declared with `mod` is simply not compiled. To run its tests, temporarily declare it: see next step.)

- [ ] **Step 5: Wire the module so its tests compile, build the crate**

In `crates/conductor/src/tui/mod.rs`, add at the top of the module declarations (next to `pub mod frame;`):

```rust
pub mod app;
```

Run: `cargo test -p conductor --lib 2>&1 | tail -25`
Expected: the whole lib test suite passes — both the OLD `tui::tests::*` and the new `tui::app::tests::*` (the two coexist; no name clash because they are different modules).

- [ ] **Step 6: Commit**

```bash
git add crates/conductor/src/tui/app.rs crates/conductor/src/tui/mod.rs
git commit -m "feat(conductor): port TUI state machine to crossterm KeyEvent in tui/app.rs"
```

---

## Task 3: New `tui/render.rs` — RexOps-look ratatui renderers

Draw the plan screen + confirm/help overlays into a `ratatui::Frame`, matching RexOps's pane/rail/overlay look. Tested via an off-screen `TestBackend` (no PTY), exactly like rexops-tui's screen tests.

**Files:**
- Create: `crates/conductor/src/tui/render.rs`
- Modify: `crates/conductor/src/tui/mod.rs` (add `pub mod render;`)

**Interfaces:**
- Consumes: `crate::plan::{Plan, Ring, Step, StepStatus}`, `super::app::{App, Screen}`, `ratatui::{Frame, layout::*, text::*, widgets::*}`, `suite_ui::{pane, Theme, ConfirmModal, HelpSheet, EmptyState}`, `crate::run::confirm_command`.
- Produces: `pub fn render(f: &mut Frame, app: &App, theme: Theme)` — the single entry the runtime calls each draw.
- Also: `pub const HELP_ROWS: &[(&str, &str)]` (shared by the help overlay and any help-content test).

- [ ] **Step 1: Write the failing tests**

Create `crates/conductor/src/tui/render.rs` with ONLY the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};
    use ratatui::{backend::TestBackend, Terminal};
    use suite_ui::Theme;

    fn sample() -> plan::Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus { name: "tools", freshness: Freshness::Stale });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(), why: "AWS key".into(),
            source: "bulwark".into(), severity: Severity::Critical,
        });
        plan::build(&s)
    }

    /// Render `app` into an off-screen buffer and flatten it to text so a test can
    /// assert on what actually appears. Mirrors rexops-tui's screen-test helper.
    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal.draw(|f| render(f, app, theme)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let width = buffer.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buffer.content.iter().enumerate() {
            if i % width == 0 && i != 0 { out.push('\n'); }
            out.push_str(cell.symbol());
        }
        out
    }

    #[test]
    fn plan_screen_shows_title_steps_commands_and_ring_tags() {
        let app = App::new(sample());
        let text = render_to_text(&app);
        assert!(text.contains("Conductor"), "header pane title:\n{text}");
        assert!(text.contains("The plan"), "plan pane title:\n{text}");
        assert!(text.contains("workstate snapshot"), "step command shown:\n{text}");
        assert!(text.contains("changes state"), "ring tag shown:\n{text}");
    }

    #[test]
    fn focused_step_shows_the_selection_rail() {
        let app = App::new(sample()); // cursor 0
        let text = render_to_text(&app);
        // The suite selection rail glyph precedes the focused row's number/title.
        assert!(text.contains('▌'), "focused row must show the accent rail:\n{text}");
    }

    #[test]
    fn situation_block_renders_when_present() {
        let app = App::new(sample());
        let text = render_to_text(&app);
        assert!(text.contains("The situation"), "situation pane shown:\n{text}");
    }

    #[test]
    fn empty_plan_shows_nothing_to_conduct() {
        let app = App::new(plan::build(&SuiteState::empty()));
        let text = render_to_text(&app);
        assert!(text.contains("nothing to conduct"), "empty state copy:\n{text}");
        assert!(!text.contains("The plan"), "no plan pane when empty:\n{text}");
    }

    #[test]
    fn confirm_overlay_shows_command_and_caution() {
        let mut app = App::new(sample());
        app.screen = Screen::Confirm; // cursor 0 is the Ring-2 refresh
        let text = render_to_text(&app);
        assert!(text.contains("workstate snapshot"), "confirm shows the literal command:\n{text}");
        assert!(
            text.to_lowercase().contains("changes suite state")
                || text.to_lowercase().contains("changes state"),
            "confirm shows the caution:\n{text}"
        );
    }

    #[test]
    fn help_overlay_lists_the_keys() {
        let mut app = App::new(sample());
        app.screen = Screen::Help;
        let text = render_to_text(&app);
        assert!(text.contains("Keys"), "help title:\n{text}");
        assert!(text.contains("rexops"), "help mentions the rexops handoff:\n{text}");
        assert!(text.contains("skip"), "help mentions skip:\n{text}");
    }

    #[test]
    fn help_rows_describe_changes_state_gate() {
        // The help content must not drift from the gate behaviour.
        let joined: String = HELP_ROWS.iter().map(|(k, d)| format!("{k} {d} ")).collect();
        assert!(joined.contains("run"));
        assert!(joined.to_lowercase().contains("confirm") || joined.to_lowercase().contains("changes-state"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p conductor --lib tui::render 2>&1 | tail -20`
Expected: compile error — `cannot find function render` / `HELP_ROWS` not found.

- [ ] **Step 3: Write the implementation (top of the same file)**

Insert at the TOP of `crates/conductor/src/tui/render.rs`:

```rust
//! Ratatui renderers for Conductor's interactive view, in the shared suite look.
//!
//! `render` is the single entry the runtime calls each draw: it paints the plan
//! screen (header / situation / plan / hints panes) and, when an overlay is up,
//! draws the confirm or help modal over it. Every distinction is carried by a
//! word + glyph as well as colour, so the view reads correctly with `NO_COLOR`.
//! Drawn with `suite_ui::{pane, ConfirmModal, HelpSheet, EmptyState}` + `Theme`
//! so the chrome matches RexOps from one source. No I/O, no app state owned here.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use suite_ui::{pane, ConfirmModal, EmptyState, HelpSheet, Theme};

use super::app::{App, Screen};
use crate::plan::{Plan, Ring, Step, StepStatus};
use crate::run::confirm_command;

/// The keybinding rows for the help overlay. Kept next to the real key handling
/// (in `app::step`) so help can't drift from the bindings.
pub const HELP_ROWS: &[(&str, &str)] = &[
    ("enter", "run the current step (changes-state steps confirm first)"),
    ("s", "skip the current step"),
    ("a", "advance focus without running"),
    ("r", "hand off to the rexops cockpit"),
    ("?", "toggle this help"),
    ("q / Esc", "quit"),
];

/// The one-line key-hint strip shown at the foot of the plan screen.
const HINT: &str = "enter run · s skip · a advance · r rexops · ? help · q quit";

/// The glyph for a step. The focused step overrides this with `▸`.
fn glyph(status: StepStatus, focused: bool) -> char {
    if focused {
        return '▸';
    }
    match status {
        StepStatus::Pending => '○',
        StepStatus::Done => '✓',
        StepStatus::Skipped => '·',
        StepStatus::Failed => '✗',
    }
}

/// Ring tag style: changes-state reads as attention, read-only/info as dim.
fn ring_style(ring: Ring, theme: Theme) -> ratatui::style::Style {
    match ring {
        Ring::ChangesState => theme.confirm(),
        Ring::ReadOnly | Ring::Info => theme.dim(),
    }
}

/// Build one plan row: selection rail (accent on the focused row), status glyph,
/// number, title, the inline annotation, and the right-edge ring tag — composed
/// the way RexOps's launcher rows are.
fn step_line(n: usize, step: &Step, focused: bool, theme: Theme) -> Line<'static> {
    let rail = if focused {
        Span::styled("▌ ", theme.selected_rail())
    } else {
        Span::raw("  ")
    };
    let g = glyph(step.status, focused);
    let title_style = if focused { theme.selection() } else { theme.title() };

    let mut spans = vec![
        rail,
        Span::styled(format!("{g} {n}  "), title_style),
        Span::styled(step.title.clone(), title_style),
    ];
    if let Some(note) = &step.annotation {
        spans.push(Span::styled(format!("  ← {note}"), theme.accent_bar()));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(step.ring.tag().to_string(), ring_style(step.ring, theme)));
    Line::from(spans)
}

/// The plan screen: header / optional situation / plan / hints, in panes.
fn render_plan(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let has_situation = !app.plan.situation.is_empty();
    let constraints = if has_situation {
        vec![
            Constraint::Length(3),                       // header
            Constraint::Length(app.plan.situation.len() as u16 + 2), // situation
            Constraint::Min(3),                          // plan
            Constraint::Length(2),                       // hints + notice
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // Header.
    let header = Paragraph::new(Line::from(Span::styled(
        "Given the suite's state — do these, in this order.",
        theme.dim(),
    )))
    .block(pane("Conductor", theme));
    f.render_widget(header, chunks[0]);

    // Situation (optional) + plan + hints land in shifting indices.
    let (plan_idx, hints_idx) = if has_situation {
        let lines: Vec<Line> = app
            .plan
            .situation
            .iter()
            .map(|s| Line::from(Span::styled(s.clone(), theme.dim())))
            .collect();
        let sit = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(pane("The situation", theme));
        f.render_widget(sit, chunks[1]);
        (2, 3)
    } else {
        (1, 2)
    };

    // Plan.
    let rows: Vec<Line> = app
        .plan
        .steps
        .iter()
        .enumerate()
        .flat_map(|(i, step)| {
            let mut v = vec![step_line(i + 1, step, i == app.cursor, theme)];
            if let Some(cmd) = &step.command {
                v.push(Line::from(Span::styled(format!("       {cmd}"), theme.dim())));
            }
            v
        })
        .collect();
    let title = format!("The plan — {} steps", app.plan.steps.len());
    let plan = Paragraph::new(rows).block(pane(&title, theme));
    f.render_widget(plan, chunks[plan_idx]);

    // Hints + transient notice (notice takes the line when set).
    let hint_line = match &app.notice {
        Some(msg) => Line::from(Span::styled(msg.clone(), theme.status_error())),
        None => Line::from(Span::styled(HINT, theme.dim())),
    };
    f.render_widget(Paragraph::new(hint_line), chunks[hints_idx]);
}

/// The single draw entry the runtime calls each frame.
pub fn render(f: &mut Frame, app: &App, theme: Theme) {
    let area = f.area();

    if app.plan.steps.is_empty() {
        EmptyState {
            message: "nothing to conduct — the suite is healthy and every feed is current",
        }
        .render(f, area, theme);
        return;
    }

    render_plan(f, app, area, theme);

    match app.screen {
        Screen::Confirm => {
            if let Some(step) = app.plan.steps.get(app.cursor) {
                let cmd = confirm_command(step).unwrap_or("(no command)");
                let message = format!("{cmd}   — this changes suite state");
                ConfirmModal {
                    title: &step.title,
                    message: &message,
                }
                .render(f, area, theme);
            }
        }
        Screen::Help => {
            HelpSheet {
                title: "Keys",
                rows: HELP_ROWS,
            }
            .render(f, area, theme);
        }
        Screen::Plan => {}
    }
}
```

NOTE on `EmptyState`: confirm its exact field set before relying on it — run `grep -n "pub " crates/thomas-tui/src/empty_state.rs`. The struct has `pub message: &str` and may have an optional `hint`/icon field with a `Default`. If it does NOT derive `Default` or has required extra fields, construct it with those fields (e.g. `EmptyState { message: "...", hint: None }`) to match the real signature. Adjust this one literal to the real struct; everything else is API-stable.

- [ ] **Step 4: Add the module declaration**

In `crates/conductor/src/tui/mod.rs`, next to `pub mod app;`, add:

```rust
pub mod render;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p conductor --lib tui::render 2>&1 | tail -25`
Expected: all `tui::render::tests::*` PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/conductor/src/tui/render.rs crates/conductor/src/tui/mod.rs
git commit -m "feat(conductor): ratatui render layer in the shared suite-ui look"
```

---

## Task 4: New `tui/runtime.rs` — Tui guard + draw/event loop + spawn adapter

Owns the terminal (`suite_ui::Tui`), the dirty-flag draw/event loop (RexOps's pattern), and the `Spawner` adapter that forwards a child spawn to `Tui::suspended` so the TUI yields the real terminal to the child and re-enters after.

**Files:**
- Create: `crates/conductor/src/tui/runtime.rs`
- Modify: `crates/conductor/src/tui/mod.rs` (add `pub mod runtime;`)

**Interfaces:**
- Consumes: `super::app::{App, Action, RunReport, report_from, step}`, `super::render::render`, `crate::plan::Plan`, `crate::run::Spawner`, `suite_ui::{Tui, TuiOptions, Theme, ColorChoice, ThemeChoice}`, `crossterm::event`.
- Produces:
  - `pub fn run(plan: Plan, no_color: bool) -> std::io::Result<RunReport>` — sets up the Tui, runs the loop to quit, returns the tally.
  - (internal) a `Spawner` adapter type wrapping `&RefCell<&mut Tui>` that forwards to `Tui::suspended(|| RealSpawner.spawn(argv))`.

- [ ] **Step 1: Write the failing test**

The loop itself needs a real terminal, so it is exercised by the manual smoke + the `--dump-view` test (Task 5), not a unit test. The one unit-testable seam here is the theme selection (no_color → mono). Create `crates/conductor/src/tui/runtime.rs` with ONLY the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_true_yields_a_monochrome_theme() {
        assert!(!theme_for(true).color_enabled());
    }

    #[test]
    fn no_color_false_yields_a_colored_theme_unless_env_says_otherwise() {
        // With NO_COLOR unset, Auto resolves to colour on. Guard the env so the
        // suite-wide test lock isn't needed (we only read, then restore).
        let had = std::env::var_os("NO_COLOR");
        std::env::remove_var("NO_COLOR");
        let enabled = theme_for(false).color_enabled();
        if let Some(v) = had { std::env::set_var("NO_COLOR", v); }
        assert!(enabled, "Auto + no NO_COLOR must enable colour");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p conductor --lib tui::runtime 2>&1 | tail -15`
Expected: compile error — `cannot find function theme_for`.

- [ ] **Step 3: Write the implementation (top of the same file)**

Insert at the TOP of `crates/conductor/src/tui/runtime.rs`:

```rust
//! The interactive runtime: the `suite_ui::Tui` terminal guard, the draw/event
//! loop, and the spawn adapter that suspends the TUI around a delegated child.
//!
//! Modeled on rexops-tui's runtime: enter the alternate screen via the shared
//! `Tui` guard (raw mode + alt screen + a panic hook that restores the terminal),
//! then a dirty-flag loop — draw only when something changed, otherwise just poll
//! input. Conductor has no background snapshots or jobs, so a tick is dirty only
//! after a handled keypress. `Tui`'s `Drop` restores the terminal on every exit
//! path (clean return, `?`, or panic).

use std::cell::RefCell;
use std::process::ExitStatus;
use std::time::Duration;

use crossterm::event::{self, Event};

use suite_ui::{ColorChoice, Theme, ThemeChoice, Tui, TuiOptions};

use super::app::{report_from, step, Action, App, RunReport};
use super::render::render;
use crate::plan::Plan;
use crate::run::{RealSpawner, Spawner};

/// Resolve the interactive theme. `--no-color` forces monochrome; otherwise the
/// suite's cyan accent with colour-on-unless-NO_COLOR (Auto). The single place
/// the palette enters Conductor's TUI.
fn theme_for(no_color: bool) -> Theme {
    if no_color {
        Theme::with_color(false)
    } else {
        Theme::resolve(ColorChoice::Auto, ThemeChoice::Cyan)
    }
}

/// A `Spawner` that suspends the TUI for the duration of the child, handing it
/// the real terminal, then resumes. All terminal leave/re-enter is owned by
/// `Tui::suspended`, which guarantees re-entry even if the child fails — so the
/// terminal is never left suspended. Forwards to `RealSpawner` for the actual
/// exec, keeping the no-shell launch discipline in one place (`run.rs`).
struct SuspendSpawner<'a> {
    tui: RefCell<&'a mut Tui>,
}

impl Spawner for SuspendSpawner<'_> {
    fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
        let argv = argv.to_vec();
        // `suspended` returns io::Result<T>; here T is the inner spawn's
        // io::Result<ExitStatus>. Flatten so a suspend failure and a spawn
        // failure both surface as Err (run.rs maps that to SpawnError).
        self.tui
            .borrow_mut()
            .suspended(move || RealSpawner.spawn(&argv))?
    }
}

/// Run the interactive TUI to completion. Sets up the alt-screen guard, loops
/// painting frames and applying keys until Quit, always restores the terminal on
/// the way out (Tui's Drop), and returns a `RunReport` tallying what the operator
/// left behind (for the exit code).
pub fn run(plan: Plan, no_color: bool) -> std::io::Result<RunReport> {
    let theme = theme_for(no_color);
    let mut app = App::new(plan);

    // Enter TUI mode. require_tty is false: the caller (main) already gated on a
    // tty before choosing the interactive path, and the non-interactive path
    // never reaches here. hide_cursor: this is a read-only dashboard (no text
    // field). map TuiError into io::Error so the signature stays io::Result.
    let mut tui = Tui::new(TuiOptions {
        hide_cursor: true,
        mouse_capture: false,
        require_tty: false,
    })
    .map_err(|e| std::io::Error::other(e.to_string()))?;

    let mut dirty = true;
    loop {
        if dirty {
            tui.terminal().draw(|f| render(f, &app, theme))?;
            dirty = false;
        }

        // Block up to 100ms for input; a timeout is an idle tick (no redraw).
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let action = match event::read()? {
            Event::Key(key) => {
                let spawner = SuspendSpawner {
                    tui: RefCell::new(&mut tui),
                };
                step(&mut app, key, &spawner)
            }
            // A resize (or any non-key event) just repaints at the new size.
            Event::Resize(_, _) => Action::Redraw,
            _ => continue,
        };
        match action {
            Action::Quit => break,
            Action::Redraw => dirty = true,
        }
    }

    Ok(report_from(&app.plan))
    // `tui` drops here → guaranteed terminal restore.
}
```

NOTE on the `suspended` flatten: `Tui::suspended<T>(&mut self, f) -> io::Result<T>`. With `f` returning `io::Result<ExitStatus>`, the call is `io::Result<io::Result<ExitStatus>>`; the trailing `?` after `suspended(...)` unwraps the outer (suspend) error, and the expression's value is the inner `io::Result<ExitStatus>` — exactly the `Spawner::spawn` return type. If the compiler reports a type mismatch, write it explicitly: `let inner = self.tui.borrow_mut().suspended(move || RealSpawner.spawn(&argv))?; inner`.

- [ ] **Step 4: Add the module declaration**

In `crates/conductor/src/tui/mod.rs`, next to `pub mod runtime;`:

```rust
pub mod runtime;
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p conductor --lib tui::runtime 2>&1 | tail -15`
Expected: both `tui::runtime::tests::*` PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/conductor/src/tui/runtime.rs crates/conductor/src/tui/mod.rs
git commit -m "feat(conductor): suite_ui::Tui runtime loop + suspend-on-spawn adapter"
```

---

## Task 5: Rewire `tui/mod.rs` + `main.rs`, delete the old stack, fix --dump-view

Make the new modules the live path, delete the hand-rolled stack (`term.rs`, `style.rs`, `frame.rs`, and the old state-machine + loop in `mod.rs`), and re-point `--dump-view` at a `TestBackend` render so its snapshot tests work without a PTY.

**Files:**
- Modify: `crates/conductor/src/tui/mod.rs` (strip to a thin module root + `run`/`should_run_interactive` re-exports)
- Delete: `crates/conductor/src/tui/term.rs`, `crates/conductor/src/tui/style.rs`, `crates/conductor/src/tui/frame.rs`
- Modify: `crates/conductor/src/main.rs` (the `run_bare` + `run_dump_view` call sites)
- Modify: any integration test under `crates/conductor/tests/` that drove `--dump-view` and asserted old ANSI spacing (content assertions only).

**Interfaces:**
- Consumes: `super::runtime::run`, `super::app::RunReport`, `super::render::{render, ...}`.
- Produces (the stable public TUI surface `main` uses, unchanged names):
  - `conductor::tui::run(plan: Plan, no_color: bool) -> std::io::Result<RunReport>`
  - `conductor::tui::should_run_interactive() -> bool`
  - `conductor::tui::dump_view(plan: &Plan, view: &str, no_color: bool) -> Option<String>` (new: renders one frame to text for tests/`--dump-view`).

- [ ] **Step 1: Rewrite `tui/mod.rs` to the thin root**

Replace the ENTIRE contents of `crates/conductor/src/tui/mod.rs` with:

```rust
//! Conductor's interactive TUI, built on the shared `suite_ui` stack (the same
//! ratatui chrome RexOps/Pulse render from). Split by responsibility:
//!   - `app`     — state + key→action transitions (terminal-free, unit-tested)
//!   - `render`  — ratatui renderers (panes + confirm/help overlays), the look
//!   - `runtime` — the `suite_ui::Tui` guard + draw/event loop + spawn adapter
//! `run` wires them; `main` only decides whether to call it (a real TTY) or fall
//! back to the scriptable `status` output.

pub mod app;
pub mod render;
pub mod runtime;

use std::io::IsTerminal;

use crate::plan::Plan;

pub use app::RunReport;
pub use runtime::run;

/// True when the bare invocation should open the interactive TUI: stdout is a
/// real terminal. A non-TTY bare invocation stays scriptable (prints status).
pub fn should_run_interactive() -> bool {
    std::io::stdout().is_terminal()
}

/// Render exactly one frame to text (no event loop), for `--dump-view` and
/// snapshot tests. Draws the chosen view into an off-screen `TestBackend` and
/// flattens the buffer to a string. `None` for an unknown view name.
///
/// `plan` selects the data; `view` selects which screen/overlay to paint by
/// setting the App's screen/cursor before the single render call.
pub fn dump_view(plan: &Plan, view: &str, no_color: bool) -> Option<String> {
    use app::{App, Screen};
    use ratatui::{backend::TestBackend, Terminal};
    use suite_ui::Theme;

    let mut app = App::new(plan.clone());
    match view {
        "plan" | "compact" => {} // one reflowing layout now; "compact" kept as an alias
        "healthy" => {
            // Force the empty-state path regardless of the supplied plan.
            app.plan.steps.clear();
            app.plan.situation.clear();
        }
        "help" => app.screen = Screen::Help,
        "confirm" => {
            // Point the cursor at the first changes-state step (as the real TUI
            // would when opening the gate) and show the confirm overlay.
            if let Some(idx) = app
                .plan
                .steps
                .iter()
                .position(|s| s.ring == crate::plan::Ring::ChangesState)
            {
                app.cursor = idx;
                app.screen = Screen::Confirm;
            } else {
                app.plan.steps.clear(); // nothing to confirm → healthy
            }
        }
        _ => return None,
    }

    let theme = if no_color { Theme::with_color(false) } else { Theme::with_color(true) };
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).ok()?;
    terminal.draw(|f| render::render(f, &app, theme)).ok()?;
    let buffer = terminal.backend().buffer().clone();
    let width = buffer.area.width as usize;
    let mut out = String::new();
    for (i, cell) in buffer.content.iter().enumerate() {
        if i % width == 0 && i != 0 {
            out.push('\n');
        }
        out.push_str(cell.symbol());
    }
    Some(out)
}
```

- [ ] **Step 2: Delete the old hand-rolled files**

```bash
git rm crates/conductor/src/tui/term.rs crates/conductor/src/tui/style.rs crates/conductor/src/tui/frame.rs
```

- [ ] **Step 3: Update `main.rs` call sites**

In `crates/conductor/src/main.rs`:

(a) `run_bare` already calls `conductor::tui::run(plan, cli.no_color)` and `conductor::tui::should_run_interactive()` — both names are preserved, so **leave `run_bare` as-is**. (Confirm by reading it; no edit needed.)

(b) Replace the whole `run_dump_view` function with one that uses the new `dump_view`:

```rust
/// Render exactly one TUI frame (no event loop) and exit 0 — the test backbone.
fn run_dump_view(cli: &Cli, view: &str) -> ExitCode {
    let dir = match data_dir(cli) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("conductor: {e}");
            return ExitCode::from(3);
        }
    };
    let state = load_state(&dir);
    let plan = plan::build(&state);
    match conductor::tui::dump_view(&plan, view, true) {
        Some(frame) => {
            print!("{frame}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!(
                "conductor: --dump-view needs one of: plan healthy compact help confirm (got {view})"
            );
            ExitCode::from(3)
        }
    }
}
```

(c) Remove the now-unused `use conductor::tui::frame;` line inside the old `run_dump_view` (it's gone with the rewrite) and any other `tui::style`/`tui::frame` references. Search and fix:

Run: `grep -rn "tui::frame\|tui::style\|tui::term\|RawMode\|term::Key" crates/conductor/src/`
Expected after fixes: no matches.

- [ ] **Step 4: Update integration tests that used the old layout**

Run: `ls crates/conductor/tests/ 2>/dev/null && grep -rln "dump-view\|dump_view" crates/conductor/tests/ 2>/dev/null`

For each hit, the test shells out to `conductor --dump-view <view>` and asserts on output. The new frames are bordered ratatui panes, so any assertion on exact old spacing/glyphs must move to **content** assertions that the new render guarantees, e.g.:
- plan view: `assert!(out.contains("Conductor") && out.contains("The plan"))`
- a step command appears: `assert!(out.contains("workstate snapshot"))` (use whatever the test's fixture plan contains)
- healthy view: `assert!(out.contains("nothing to conduct"))`
- help view: `assert!(out.contains("Keys"))`
- confirm view: `assert!(out.contains("changes suite state") || out.contains("changes state"))`

Remove assertions that pin exact column widths / the old `▸ 1` inline format / `<=80`-byte-line checks (ratatui owns width now; the 80-col guarantee is the `TestBackend::new(80, 24)` width itself). Keep the exit-code assertions (still 0 for a successful dump).

- [ ] **Step 5: Build + full test + clippy**

Run: `cargo build -p conductor 2>&1 | tail -15`
Expected: builds clean (no references to deleted modules).

Run: `cargo test -p conductor 2>&1 | tail -30`
Expected: all lib + integration tests pass.

Run: `cargo clippy -p conductor --all-targets 2>&1 | tail -20`
Expected: no warnings (treat `unwrap_used`/`expect_used` in non-test code as failures — fix any).

- [ ] **Step 6: Commit**

```bash
git add -A crates/conductor
git commit -m "refactor(conductor): make suite-ui TUI the live path; delete hand-rolled term/style/frame"
```

---

## Task 6: Workspace-wide verification + LAST_WORK.md

Confirm the migration didn't break the umbrella, then record the work (per the user's LAST_WORK rule). No push.

**Files:**
- Modify: `LAST_WORK.md` (repo root)

- [ ] **Step 1: Full workspace build + test**

Run: `cargo build 2>&1 | tail -15`
Expected: the whole workspace builds.

Run: `cargo test 2>&1 | tail -30`
Expected: all workspace tests pass (conductor included). If a non-conductor crate fails, it is pre-existing — note it, do not fix out of scope.

- [ ] **Step 2: Confirm the binary launches the new path (non-TTY fallback is safe to run headless)**

Run: `cargo run -p conductor -- --dump-view plan | head -20`
Expected: a bordered "Conductor" / "The plan" frame prints (the new look), exit 0.

Run: `printf '' | cargo run -p conductor -- --dump-view help | grep -c Keys`
Expected: `1`.

- [ ] **Step 3: Update LAST_WORK.md**

Replace `LAST_WORK.md` at the repo root with a short record (keep its existing house style if it has one — read it first):

```markdown
# Last Work

## Conductor → suite-ui migration (2026-06-22)

Conductor was the only suite TUI still on a hand-rolled ANSI renderer
(`tui/term.rs` + `tui/frame.rs` + `tui/style.rs`, no ratatui/crossterm). It is
now rebuilt on the shared `suite_ui` stack — the same `Tui` guard + `Theme` +
`pane`/`ConfirmModal`/`HelpSheet` chrome RexOps and Pulse render from — so its
interactive view matches the rest of the suite.

- `crates/conductor/Cargo.toml`: + suite-ui / ratatui / crossterm (workspace path deps).
- `src/tui/` rebuilt: `app.rs` (state machine, ported to crossterm KeyEvent),
  `render.rs` (ratatui renderers, suite look), `runtime.rs` (Tui guard + draw/event
  loop + suspend-on-spawn adapter). Deleted: `term.rs`, `style.rs`, `frame.rs`.
- Behaviour preserved: the Ring-2 confirm gate, the `r` rexops handoff, exit codes
  (1 fail / 2 unfinished / 0 clean), the scriptable status/health/json/`--dump-view`
  paths. State-machine tests ported verbatim (KeyEvent-driven); new TestBackend
  render tests cover the look.
- Verified: `cargo test -p conductor`, `cargo clippy -p conductor`, full `cargo
  build`/`cargo test` green. Manual on-terminal smoke pending the user's eyeball.

Branch: `worktree-conductor-suite-ui` (worktree). Not pushed.
```

- [ ] **Step 4: Commit**

```bash
git add LAST_WORK.md
git commit -m "docs: record the Conductor → suite-ui TUI migration"
```

- [ ] **Step 5: Report to the user**

Summarize: what changed, test/clippy/build results (with the actual tail output), and that the on-terminal smoke is the user's acceptance step — offer the `--dump-view plan` text as a proxy and ask whether to push / open a PR (do NOT push without approval).

---

## Self-Review

**Spec coverage:**
- Cargo.toml deps → Task 1. ✓
- Replace everything in `src/tui/` with suite_ui components → Tasks 2 (app), 3 (render), 4 (runtime), 5 (delete old + rewire mod). ✓
- main.rs uses shared Tui + widgets → Task 4 (`run` via Tui) + Task 5 (main call sites, dump_view). ✓
- Alternate screen → Task 4 (`Tui::new` enters alt screen). ✓
- Full rewrite mirroring RexOps (App/runtime/ForegroundRunner-style) → Tasks 2–4 mirror rexops-tui structure. ✓
- Preserve Ring-2 gate / exit codes / handoff / scriptable paths → ported tests in Task 2, scriptable paths untouched, `--dump-view` re-pointed in Task 5. ✓
- TestBackend render tests for the look → Task 3. ✓
- `--dump-view` snapshot + tests/ rewrite → Task 5 step 4. ✓
- Workspace-green + LAST_WORK → Task 6. ✓
- No git pin / workspace path dep → Task 1. ✓

**Placeholder scan:** No TBD/TODO. The two NOTEs (EmptyState fields, `suspended` flatten) are explicit verification instructions with the fallback code given, not placeholders — they tell the implementer exactly what to check and what to write either way.

**Type consistency:** `App` (not `AppState`) used uniformly across Tasks 2/3/4/5. `step(&mut App, KeyEvent, &dyn Spawner) -> Action`, `report_from(&Plan) -> RunReport`, `RunReport::exit_code`, `render(&mut Frame, &App, Theme)`, `runtime::run(Plan, bool) -> io::Result<RunReport>`, `dump_view(&Plan, &str, bool) -> Option<String>` — names and signatures match between producer and consumer tasks. `Screen::{Plan,Help,Confirm}`, `Action::{Redraw,Quit}` consistent. `Ring::{ChangesState,ReadOnly,Info}` and `StepStatus::{Pending,Done,Skipped,Failed}` match the real `plan` module (verified against source).

**Known verification points the implementer must honor (not gaps, but checks):**
1. `EmptyState` exact fields (Task 3 NOTE) — grep before constructing.
2. `Tui::suspended` double-Result flatten (Task 4 NOTE) — explicit fallback given.
3. `SuiteState::empty()`, `FeedStatus`/`Finding`/`Freshness`/`Severity` field shapes used in test fixtures match `state.rs` (verified against the existing `tui/mod.rs` and `frame.rs` tests, which use the identical fixtures).

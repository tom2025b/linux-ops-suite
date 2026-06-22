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

    /// Move focus up one step, clamped at the first step.
    fn retreat(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Jump focus to the 1-indexed step `n` (so `1` is the first step). A number
    /// past the last step — or `0` — is ignored (the cursor stays put).
    fn jump_to(&mut self, n: usize) {
        if n >= 1 && n <= self.plan.steps.len() {
            self.cursor = n - 1;
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
        // In help, any key returns to the plan (q / Esc still quit).
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
        // Move focus down/up: arrows or vim j/k. `a` also advances (kept for
        // muscle memory from the old hand-rolled TUI).
        KeyCode::Down | KeyCode::Char('j') => {
            app.advance();
            Action::Redraw
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.retreat();
            Action::Redraw
        }
        KeyCode::Char('a') => {
            app.advance();
            Action::Redraw
        }
        // Number keys jump straight to that step (1 = first). Out-of-range is
        // ignored by `jump_to`. Conductor has a single screen, so unlike RexOps
        // numbers address steps, not screens.
        KeyCode::Char(c @ '1'..='9') => {
            if let Some(n) = c.to_digit(10) {
                app.jump_to(n as usize);
            }
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
                    app.notice =
                        Some(format!("rexops handoff failed ({e}); press a key to redraw"));
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

/// `q` or (anywhere it reaches here) `suite_ui::keys::is_cancel` (Esc OR Ctrl-G)
/// quits — matching the old `Key::Esc => Quit` on the plain plan screen.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::run::Spawner;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::cell::RefCell;
    use std::process::ExitStatus;

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }
    fn esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    struct FakeSpawner {
        calls: RefCell<Vec<Vec<String>>>,
    }
    impl FakeSpawner {
        fn new() -> Self {
            FakeSpawner {
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl Spawner for FakeSpawner {
        fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
            self.calls.borrow_mut().push(argv.to_vec());
            std::process::Command::new("true").status()
        }
    }
    struct ExitSpawner {
        success: bool,
    }
    impl Spawner for ExitSpawner {
        fn spawn(&self, _argv: &[String]) -> std::io::Result<ExitStatus> {
            use std::os::unix::process::ExitStatusExt;
            Ok(ExitStatus::from_raw(if self.success { 0 } else { 1 << 8 }))
        }
    }

    /// Plan: refresh (Ring2) → capture (Ring2) → investigate (Ring1).
    fn sample() -> Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(),
            why: "key".into(),
            source: "bulwark".into(),
            severity: Severity::Critical,
        });
        plan::build(&s)
    }

    /// Put `bin` on PATH for the closure, holding the crate-wide env lock, then
    /// restore PATH. The stub is prepended (not replacing PATH) so `sh` etc. stay
    /// visible to any concurrently-scheduled probe.
    fn with_path<F: FnOnce()>(bin: &str, body: &str, f: F) {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join(bin);
        std::fs::write(&stub, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let orig = std::env::var_os("PATH");
        let mut v = std::ffi::OsString::from(dir.path());
        if let Some(p) = &orig {
            v.push(":");
            v.push(p);
        }
        std::env::set_var("PATH", &v);
        f();
        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
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

    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn down_arrow_and_j_move_cursor_down_without_running() {
        let mut app = App::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, code(KeyCode::Down), &sp);
        assert_eq!(app.cursor, 1);
        step(&mut app, k('j'), &sp);
        assert_eq!(app.cursor, 2);
        assert!(sp.calls.borrow().is_empty(), "navigation must not spawn");
    }

    #[test]
    fn up_arrow_and_k_move_cursor_up_without_running() {
        let mut app = App::new(sample());
        app.cursor = 2;
        let sp = FakeSpawner::new();
        step(&mut app, code(KeyCode::Up), &sp);
        assert_eq!(app.cursor, 1);
        step(&mut app, k('k'), &sp);
        assert_eq!(app.cursor, 0);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn cursor_does_not_move_past_the_ends() {
        let mut app = App::new(sample());
        let sp = FakeSpawner::new();
        // already at top: Up stays at 0
        step(&mut app, code(KeyCode::Up), &sp);
        assert_eq!(app.cursor, 0);
        // walk to the last step, then Down stays clamped
        let last = app.plan.steps.len() - 1;
        app.cursor = last;
        step(&mut app, code(KeyCode::Down), &sp);
        assert_eq!(app.cursor, last, "Down at the last step must clamp");
    }

    #[test]
    fn number_keys_jump_to_that_step() {
        let mut app = App::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, k('3'), &sp);
        assert_eq!(app.cursor, 2, "'3' focuses the 3rd step (0-indexed 2)");
        step(&mut app, k('1'), &sp);
        assert_eq!(app.cursor, 0, "'1' focuses the 1st step");
        assert!(sp.calls.borrow().is_empty(), "jumping must not spawn");
    }

    #[test]
    fn out_of_range_number_is_ignored() {
        let mut app = App::new(sample()); // 6 steps
        app.cursor = 1;
        let sp = FakeSpawner::new();
        step(&mut app, k('9'), &sp); // no 9th step
        assert_eq!(app.cursor, 1, "a number past the last step does nothing");
        step(&mut app, k('0'), &sp); // there is no step 0
        assert_eq!(app.cursor, 1, "'0' is not a step");
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
            step(&mut app, enter(), &sp); // open confirm
            assert_eq!(app.screen, Screen::Confirm);
            step(&mut app, k('y'), &sp); // confirm + run
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
            step(&mut app, enter(), &sp); // open confirm
            assert_eq!(app.screen, Screen::Confirm);
            step(&mut app, key, &sp); // decline
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
        step(&mut app, enter(), &sp); // bulwark not on PATH -> notice
        assert!(app.notice.is_some());
        step(&mut app, k('a'), &sp); // any key clears it
        assert!(app.notice.is_none());
        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn run_report_maps_statuses_to_exit_codes() {
        assert_eq!(
            RunReport {
                failed: 0,
                unfinished: 0
            }
            .exit_code(),
            0
        );
        assert_eq!(
            RunReport {
                failed: 1,
                unfinished: 3
            }
            .exit_code(),
            1
        );
        assert_eq!(
            RunReport {
                failed: 0,
                unfinished: 2
            }
            .exit_code(),
            2
        );
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

//! Conductor's interactive TUI. Dependency-free, modeled on pulse: a hand-rolled
//! terminal driver (`term`), pure frame renderers (`frame`), a color resolver
//! (`style`), and the event loop (this module, added in a later task).
//!
//! The interactive app: state, navigation, and the event loop. Rendering is
//! delegated to `frame`, terminal I/O to `term`, spawning to `run`. This module
//! only maps keys to state transitions and chooses which frame to paint — so the
//! transitions are unit-testable with a fake spawner and no PTY.

pub mod frame;
pub mod style;
pub mod term;

use std::cell::RefCell;
use std::io::{self, IsTerminal};
use std::process::ExitStatus;

use crate::plan::{Plan, Ring, StepStatus};
use crate::run::{run_step, RealSpawner, RunOutcome, Spawner};
use crate::tui::term::{Key, RawMode};

/// Which screen is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Plan,
    Help,
}

/// Whether the loop should repaint and continue, or exit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Redraw,
    Quit,
}

/// All interactive state. The plan's per-step `status` carries Done/Skipped;
/// `cursor` is the focused (▸) step; `notice` is a transient one-liner cleared on
/// the next keypress.
pub struct AppState {
    pub plan: Plan,
    pub cursor: usize,
    pub screen: Screen,
    pub notice: Option<String>,
}

impl AppState {
    pub fn new(plan: Plan) -> Self {
        AppState {
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

/// Apply one key to the state, using `spawner` for any Ring-1 run. Pure with
/// respect to the terminal — it never touches stdin/stdout — so it is fully
/// unit-testable. The real loop wraps `spawner` so a spawn suspends the TUI.
pub fn step(app: &mut AppState, key: Key, spawner: &dyn Spawner) -> Action {
    // Any key clears a stale notice first; specific arms may set a fresh one.
    app.notice = None;

    if app.screen == Screen::Help {
        // In help, any key returns to the plan (q still quits).
        match key {
            Key::Char('q') | Key::Eof => return Action::Quit,
            _ => {
                app.screen = Screen::Plan;
                return Action::Redraw;
            }
        }
    }

    match key {
        Key::Char('q') | Key::Eof | Key::Esc => Action::Quit,
        Key::Char('?') => {
            app.screen = Screen::Help;
            Action::Redraw
        }
        Key::Char('a') => {
            app.advance();
            Action::Redraw
        }
        Key::Char('s') => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Skipped;
            }
            app.advance();
            Action::Redraw
        }
        Key::Char('r') => {
            // Hand off to the rexops cockpit if present; else a dim note.
            if crate::sources::is_on_path("rexops") {
                let _ = spawner.spawn(&["rexops".to_string(), "tui".to_string()]);
            } else {
                app.notice = Some("rexops is not on PATH".to_string());
            }
            Action::Redraw
        }
        Key::Enter => {
            run_current(app, spawner);
            Action::Redraw
        }
        _ => Action::Redraw,
    }
}

/// Run the focused step (Enter). Ring-1 spawns + marks Done + advances; Ring-2
/// is a no-op with a note; Info / unavailable produce a note and stay put.
fn run_current(app: &mut AppState, spawner: &dyn Spawner) {
    let Some(step_ref) = app.plan.steps.get(app.cursor) else {
        return;
    };
    let ring = step_ref.ring;
    match run_step(step_ref, spawner) {
        RunOutcome::Ran(_) => {
            if let Some(s) = app.plan.steps.get_mut(app.cursor) {
                s.status = StepStatus::Done;
            }
            app.advance();
        }
        RunOutcome::RefusedChangesState => {
            app.notice = Some("this step changes state — needs Phase 3, not run".to_string());
        }
        RunOutcome::NotAvailable(bin) => {
            app.notice = Some(format!("{bin} is not on PATH — install it first"));
        }
        RunOutcome::NotRunnable => {
            if ring == Ring::Info {
                app.notice = Some("informational — run the shown command yourself".to_string());
            }
        }
    }
}

/// Terminal width/height via `ioctl(TIOCGWINSZ)`; falls back to 80×24 if it
/// can't be read (e.g. piped). Used to pick the compact fallback under 80×24.
fn term_size() -> (u16, u16) {
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }
    extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    const TIOCGWINSZ: u64 = 0x5413; // Linux
    let mut ws = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: ioctl fills a correctly-sized Winsize we own.
    let rc = unsafe { ioctl(1, TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
        (ws.ws_col, ws.ws_row)
    } else {
        (80, 24)
    }
}

/// Render the current frame for `app` at the current terminal size.
fn render(app: &AppState, style: &crate::tui::style::Style) -> String {
    let (cols, rows) = term_size();
    if app.screen == Screen::Help {
        return crate::tui::frame::help_screen(style);
    }
    if cols < 80 || rows < 24 {
        return crate::tui::frame::compact_plan(&app.plan, app.cursor, style);
    }
    crate::tui::frame::plan_screen(&app.plan, app.cursor, app.notice.as_deref(), style)
}

/// A spawner that suspends the TUI for the duration of the child, handing it the
/// real terminal, then resumes. Wraps the raw-mode guard.
struct SuspendSpawner<'a> {
    raw: RefCell<&'a mut RawMode>,
}

impl Spawner for SuspendSpawner<'_> {
    fn spawn(&self, argv: &[String]) -> std::io::Result<ExitStatus> {
        let mut raw = self.raw.borrow_mut();
        raw.suspend(|| RealSpawner.spawn(argv))
    }
}

/// Run the interactive TUI to completion. Sets up the panic guard + raw mode,
/// loops painting frames and applying keys until Quit, and always restores the
/// terminal on the way out (RawMode's Drop).
pub fn run(plan: Plan, force_no_color: bool) -> std::io::Result<()> {
    term::install_panic_guard();
    let style = crate::tui::style::Style::resolve(force_no_color);
    let mut app = AppState::new(plan);
    let mut raw = RawMode::enter()?;
    let mut stdin = io::stdin();
    loop {
        term::paint(&render(&app, &style))?;
        let key = term::read_key(&mut stdin)?;
        let action = {
            let spawner = SuspendSpawner {
                raw: RefCell::new(&mut raw),
            };
            step(&mut app, key, &spawner)
        };
        if action == Action::Quit {
            break;
        }
    }
    Ok(())
}

/// True when the bare invocation should open the interactive TUI: stdout is a
/// real terminal. A non-TTY bare invocation stays scriptable (prints status).
pub fn should_run_interactive() -> bool {
    io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::run::Spawner;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};
    use std::cell::RefCell;
    use std::process::ExitStatus;

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

    #[test]
    fn q_quits() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        assert_eq!(step(&mut app, Key::Char('q'), &sp), Action::Quit);
    }

    #[test]
    fn a_advances_focus_without_running() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Char('a'), &sp);
        assert_eq!(app.cursor, 1);
        assert!(sp.calls.borrow().is_empty());
    }

    #[test]
    fn s_skips_and_advances() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Char('s'), &sp);
        assert_eq!(app.plan.steps[0].status, StepStatus::Skipped);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn enter_on_ring2_is_a_noop_with_note_and_no_spawn() {
        let mut app = AppState::new(sample()); // step 0 is the Ring2 refresh
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp);
        assert_eq!(
            app.plan.steps[0].status,
            StepStatus::Pending,
            "ring2 must not be marked done"
        );
        assert_eq!(app.cursor, 0, "ring2 must not advance");
        assert!(sp.calls.borrow().is_empty(), "ring2 must never spawn");
        assert!(app.notice.as_deref().unwrap().contains("needs Phase 3"));
    }

    #[test]
    fn enter_on_ring1_spawns_marks_done_and_advances() {
        // Move focus to the Ring1 investigate step (last step), with a stub on PATH.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("bulwark");
        std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        // $PATH is global; share the crate-wide lock with the other PATH tests.
        // PREPEND the stub dir (don't replace PATH) so `sh` etc. stay visible to
        // any concurrently-scheduled probe.
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let orig = std::env::var_os("PATH");
        let new_path = match &orig {
            Some(p) => {
                let mut v = std::ffi::OsString::from(dir.path());
                v.push(":");
                v.push(p);
                v
            }
            None => std::ffi::OsString::from(dir.path()),
        };
        std::env::set_var("PATH", &new_path);

        let mut app = AppState::new(sample());
        let last = app.plan.steps.len() - 1;
        app.cursor = last;
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp);
        assert_eq!(app.plan.steps[last].status, StepStatus::Done);
        assert_eq!(sp.calls.borrow().len(), 1);
        assert_eq!(sp.calls.borrow()[0][0], "bulwark");

        match orig {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }

    #[test]
    fn question_toggles_help_and_any_key_returns() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Char('?'), &sp);
        assert_eq!(app.screen, Screen::Help);
        step(&mut app, Key::Char('x'), &sp);
        assert_eq!(app.screen, Screen::Plan);
    }

    #[test]
    fn notice_clears_on_next_key() {
        let mut app = AppState::new(sample());
        let sp = FakeSpawner::new();
        step(&mut app, Key::Enter, &sp); // sets the ring2 notice
        assert!(app.notice.is_some());
        step(&mut app, Key::Char('a'), &sp); // any key clears it
        assert!(app.notice.is_none());
    }
}

// tui/mod.rs — thin TUI shell: owns terminal + event loop, wires pure App to effects.
// -----------------------------------------------------------------------------
// All decision logic is in app.rs (pure, unit-tested with no TTY). Effects in
// actions.rs. Invariants: load index before init(); terminal always restored
// (even on panic via ratatui hook).

mod actions;
mod app;
mod highlight;
mod theme;
mod ui;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use scriptvault_core::ScriptVault;
use suite_ui::{Tui, TuiOptions};

use app::{App, Outcome};
use theme::Theme;

// Re-export the colour policy + theme choice so `main.rs` can name them on the
// `--color` and `--theme` flags.
pub use theme::{ColorChoice, ThemeChoice};

// The live-run lifecycle owner: drains output into the pane each tick, does
// finish bookkeeping on disconnect, and starts pending runs. Its Drop kills the
// child, so quitting mid-run can't orphan a script.
use actions::LiveRunner;

/// Build the engine and run the interactive TUI, resolving colour from `--color`
/// (over `NO_COLOR`) and the accent from `--theme`.
pub fn run(color: ColorChoice, theme: ThemeChoice) -> Result<()> {
    // Logger BEFORE the alternate screen: writes to a FILE (stderr would corrupt
    // the UI), active only under `RUST_LOG` (a bare TUI launch has no --verbose).
    let log_path = crate::logging::init_tui(false);

    // Load the index up front so a failure (e.g. malformed config) surfaces
    // before the alternate screen swallows the message.
    let scriptvault = ScriptVault::load()?;
    let mut app = App::with_theme(scriptvault, Theme::resolve(color, theme));

    if let Some(path) = &log_path {
        app.set_status(format!("logging to {}", path.display()));
    }

    // The shared suite guard owns raw mode + alternate screen + restoring panic
    // hook + mouse capture; its Drop restores the terminal and drains queued
    // stdout on every exit. Cursor stays visible (the search bar takes input).
    let mut tui = Tui::new(TuiOptions {
        hide_cursor: false,
        mouse_capture: true,
        require_tty: false,
    })?;

    let result = event_loop(&mut tui, &mut app);

    // Ctrl-O "print" paths are queued on the guard; its Drop emits them to stdout
    // AFTER the screen is restored (so they pipe), and skips them on a panic.
    for path in app.take_printed_paths() {
        tui.print_after_exit(path);
    }

    result // `tui` drops here → restore + drain queued printed paths
}

/// The core loop: draw, tick the live run, read one event, act on it.
///
/// The whole live-run lifecycle (draining output into the pane, finish
/// bookkeeping on disconnect, starting a pending run, and choosing the poll
/// cadence) is owned by [`LiveRunner`] — so this reads top to bottom as
/// "draw → tick the run → handle input." The runner uses a short poll while a run
/// streams (to drain + redraw the pane responsively) and a long, zero-CPU poll
/// when idle. The runner is dropped at loop exit, which kills+reaps any in-flight
/// child, so quitting mid-run never orphans a script.
fn event_loop(tui: &mut Tui, app: &mut App) -> Result<()> {
    let mut runner = LiveRunner::new();

    loop {
        // Draw current state (including any fresh output lines). Capture the exact
        // list_rect from the SAME layout calc so mouse clicks map to the correct
        // row even on narrow/short terminals.
        tui.terminal().draw(|frame| {
            let list_r = ui::list_rect(frame.area());
            app.set_list_rect(list_r);
            ui::render(frame, app)
        })?;

        // Tick the live run: drain output into the pane, and on disconnect record
        // the finished run. Then honor any pending "run live" request. Both no-op
        // when there's nothing to do.
        runner.pump(app);
        runner.start_pending(app);

        // Short poll while a run streams (keep draining + redrawing); long,
        // zero-CPU poll when idle. The poll still returns immediately on a real event.
        if event::poll(runner.poll_timeout())? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.handle_key(key) {
                        Outcome::Continue => {}
                        Outcome::Quit => break,
                        Outcome::Act(kind) => {
                            // Effects live in `actions.rs`. They may suspend/restore
                            // the terminal (editor/run) via the guard, or touch the
                            // clipboard.
                            actions::perform(tui, app, kind);
                        }
                    }
                }
                // A resize just means the next draw uses the new size — nothing to do.
                Event::Resize(_, _) => {}
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse);
                }
                _ => {}
            }
        }
        // A key may have queued a new live run; next iteration's start_pending
        // catches it.
    }
    Ok(())
    // `runner` drops here → its LiveRun drops → child killed+reaped (quit-safe).
}

// Why the split exists: App is pure (testable); this shell only owns the real
// terminal and translates Outcomes into effects. See ARCHITECTURE.md for the
// ports-and-adapters boundary.

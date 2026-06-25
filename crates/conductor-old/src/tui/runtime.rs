//! The interactive runtime: the `suite_ui::Tui` terminal guard, the draw/event
//! loop, and the spawn adapter that suspends the TUI around a delegated child.
//!
//! Modeled on rexops-tui's runtime: enter the alternate screen via the shared
//! `Tui` guard (raw mode + alt screen + a panic hook that restores the terminal),
//! then a dirty-flag loop — draw only when something changed, otherwise just poll
//! input. Conductor has no background snapshots or jobs, so a tick is dirty only
//! after a handled keypress / resize. `Tui`'s `Drop` restores the terminal on
//! every exit path (clean return, `?`, or panic).

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
        // `suspended` returns io::Result<T> where here T is the inner spawn's
        // own io::Result<ExitStatus>. The `?` unwraps the OUTER (suspend/re-enter)
        // error; the expression value is then the INNER io::Result<ExitStatus> —
        // exactly this method's return type. A suspend failure and a spawn failure
        // both surface as Err (run.rs maps that to RunOutcome::SpawnError).
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
    // field). Map TuiError into io::Error so the signature stays io::Result.
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
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let had = std::env::var_os("NO_COLOR");
        std::env::remove_var("NO_COLOR");
        let enabled = theme_for(false).color_enabled();
        if let Some(v) = had {
            std::env::set_var("NO_COLOR", v);
        }
        assert!(enabled, "Auto + no NO_COLOR must enable colour");
    }
}

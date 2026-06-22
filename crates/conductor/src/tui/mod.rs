//! Conductor's interactive TUI, built on the shared `suite_ui` stack (the same
//! ratatui chrome RexOps/Pulse render from). Split by responsibility:
//!
//! - `app` — state + key→action transitions (terminal-free, unit-tested)
//! - `render` — ratatui renderers (panes + confirm/help overlays), the look
//! - `runtime` — the `suite_ui::Tui` guard + draw/event loop + spawn adapter
//!
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
        // One reflowing layout now; "compact" kept as an alias for the old name.
        "plan" | "compact" => {}
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

    let theme = if no_color {
        Theme::with_color(false)
    } else {
        Theme::with_color(true)
    };
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

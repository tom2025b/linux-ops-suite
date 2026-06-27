//! Interactive terminal user interface (TUI) for Bulwark — the "cockpit".
//!
//! This module is deliberately feature-gated behind "tui" (on by default for the
//! binary) and lives entirely outside `core` and `app`. It depends on Ratatui +
//! Crossterm only for presentation and input.
//!
//! # What the TUI provides (user-facing)
//! - Live results table (same data and sort order as `bulwark scan`).
//! - Risk colors identical to the CLI table (Low=green, Medium=yellow, High/Critical=red).
//! - Dashboard header with item counts per risk level.
//! - Details pane for the currently selected entry (full path, language, sidecar
//!   metadata if present, description, etc.).
//! - Live filtering (press `/`, type to filter by path or description; Esc clears).
//! - Keyboard navigation (arrows or vim j/k, g/G, etc.).
//! - Help overlay (`?`).
//! - Rescan (`r`) — re-runs the exact same core collection so external file
//!   changes are picked up without restarting.
//! - Clean terminal restore on any exit path (q, Ctrl-C, error, panic guard).
//!
//! # Architecture inside the TUI (for maintainers)
//! - `mod.rs` (this file): public entry `pub fn run(entries)`, terminal
//!   setup/restore, and the thin bridge to the inner event loop.
//! - `app.rs`: owns all mutable state (`TuiApp`), the filter logic, selection,
//!   and the main `run` loop that calls `event::read` and updates state.
//! - `ui.rs`: pure(ish) rendering — builds the Layout, Table, Paragraphs,
//!   Blocks, and the help popup. No side effects except drawing.
//!
//! Data flow: the caller (main.rs) has already done `collect_classified_inventory`
//! using the library. We receive `Vec<ClassifiedEntry>` (owned) and never call
//! back into scan ourselves except on explicit "rescan" (which re-invokes the
//! library function so we stay in sync with the CLI).
//!
//! # Why this separation?
//! - Core stays 100% testable and free of any terminal concepts.
//! - The TUI can be compiled out for headless/CI/library-only use.
//! - Rendering and state are easy to reason about and unit-test in isolation
//!   (filter math, row building, etc. live in app/ui with no terminal).

use anyhow::{Context, Result};
use suite_ui::{Tui, TuiOptions};

use crate::ScanWarning;
use crate::app::ClassifiedEntry;

mod app;
mod ui;

use app::TuiApp;

/// Public entry point called by the CLI (`bulwark tui`).
///
/// Receives already-classified entries (from the shared core engine) so that
/// TUI output is guaranteed to be consistent with `bulwark scan`.
///
/// Terminal lifecycle is owned by the shared [`suite_ui::Tui`] guard:
/// 1. `require_tty` fails fast (with a friendly message) outside a real terminal.
/// 2. Setup enters raw mode + the alternate screen, hides the cursor, and
///    installs a panic hook that restores the terminal before unwinding.
/// 3. We run the event loop against the guard's terminal.
/// 4. On *any* exit path — clean quit, `?`-error, or panic — the guard's `Drop`
///    restores the terminal, so the user's shell is never left in raw/alt mode.
///    This replaces the previous hand-rolled `catch_unwind` + manual restore,
///    and matches how the rest of the suite (RexOps, ScriptVault) manages the
///    terminal.
///
/// Special "pick" behavior:
/// - Pressing Enter on a row (in normal mode) causes the TUI to quit *and*
///   print the full absolute path of that item to stdout. This lets you use
///   `bulwark tui` as an interactive path picker (e.g. `path=$(bulwark tui)`
///   or in scripts), similar to fzf but with Bulwark's rich classification UI.
///   The pick is queued via [`Tui::print_after_exit`] so it prints to the real
///   shell *after* the terminal is restored (and is suppressed on a panic, where
///   nothing was picked).
pub fn run(
    entries: Vec<ClassifiedEntry>,
    warnings: Vec<ScanWarning>,
    path_overrides: Vec<String>,
) -> Result<()> {
    // If the user gave us zero entries (e.g. all paths missing or filtered by
    // ignore rules), we still launch — the TUI will show a nice empty state
    // and allow rescan / filter help. `path_overrides` carries any CLI paths
    // (`bulwark tui <paths>`) so rescan keeps honouring them.
    let mut app = TuiApp::new(entries, warnings, path_overrides);

    // Enter TUI mode via the shared guard: require a real terminal (so headless
    // use fails fast), hide the cursor for the cockpit look, no mouse capture.
    // The guard owns setup, the panic hook, and the guaranteed restore on Drop.
    let mut tui = Tui::new(TuiOptions {
        hide_cursor: true,
        mouse_capture: false,
        require_tty: true,
    })
    .map_err(|e| match e {
        // No terminal: replace the guard's generic message with Bulwark's own
        // actionable guidance toward the non-interactive CLI subcommands.
        suite_ui::TuiError::NotATerminal => anyhow::anyhow!(
            "bulwark tui requires an interactive terminal\n\
             (stdout is not a tty / not connected to a real terminal).\n\n\
             For non-interactive use, run:\n\
               bulwark scan                 # colored table when on a tty\n\
               bulwark scan --json          # machine-readable\n\
               bulwark scan --markdown      # for docs/READMEs"
        ),
        // A real setup failure: surface the underlying cause, it's diagnostic.
        suite_ui::TuiError::Io(io) => {
            anyhow::Error::new(io).context("failed to initialise the terminal")
        }
    })?;

    // Run the event loop against the guard's terminal. `run_app` returns
    // Some(path) when the user "picked" a row with Enter (fzf-style selector).
    // On `?`-error here, `tui` still drops and restores the terminal.
    let picked = app::run_app(tui.terminal(), &mut app).context("TUI event loop error")?;

    // Queue any picked path to print to the real shell AFTER the guard restores
    // the terminal on Drop (printing now would land in the alternate screen).
    if let Some(path) = picked {
        tui.print_after_exit(path);
    }

    Ok(()) // `tui` drops here → restore + drain the queued pick to stdout
}

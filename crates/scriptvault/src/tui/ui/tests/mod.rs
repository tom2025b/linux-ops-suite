// tui/ui/tests — renderer tests, grouped by the pane/overlay under test.
// -----------------------------------------------------------------------------
// This module owns only the shared fixtures and render helpers; the per-pane
// assertions live in focused submodules so no single test file becomes a
// god-file.
//
// Visibility note: the submodules pull the renderer surface in with
// `use super::super::*;` — their `super` is this module and `super::super` is
// `ui` (ui.rs), which re-exports the test-only helpers (`status_is_error`,
// `highlight_spans`, `menu_title`, …) and the `render` entry point. The shared
// helpers below are `pub(super)` so the submodules can call them.

use super::*;
use crate::tui::app::App;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use scriptvault_core::{Config, ScriptVault};
use std::fs;
use std::path::{Path, PathBuf};

mod footer;
mod list;
mod output;
mod overlays;
mod panes;

/// Build an `App` over a temp vault with two scripts (`Deploy App`, `Backup`),
/// coloured theme on. Returns the app plus the temp dir for cleanup.
pub(super) fn fixture_app() -> (App, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("scriptvault-ui-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("deploy.sh"),
        "#!/bin/bash\n# scriptvault.name: Deploy App\n# scriptvault.desc: ship it\n# scriptvault.tags: ci, prod\necho go\n",
    )
    .unwrap();
    fs::write(
        dir.join("backup.sh"),
        "#!/bin/bash\n# scriptvault.name: Backup\necho b\n",
    )
    .unwrap();
    (app_with_theme(&dir, Theme::with_color(true)), dir)
}

/// Build an `App` over `dir` with an explicit theme (used by the colour tests
/// that need NO_COLOR vs coloured frames).
pub(super) fn app_with_theme(dir: &Path, theme: Theme) -> App {
    let config = Config {
        roots: vec![dir.to_path_buf()],
        ..Default::default()
    };
    let sv = ScriptVault::load_with_state_at(
        config,
        scriptvault_core::State::default(),
        dir.join("state.json"),
    )
    .unwrap();
    App::with_theme(sv, theme)
}

/// Render one frame and return the buffer's symbols concatenated (no row breaks)
/// — handy for a quick `contains` check when row position doesn't matter.
pub(super) fn render_to_string(app: &App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| render(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

/// Render one frame as text with newlines between rows — use when the assertion
/// cares about a whole line (e.g. `[err] boom` on one row).
pub(super) fn render_to_rows(app: &App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| render(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        out.push('\n');
    }
    out
}

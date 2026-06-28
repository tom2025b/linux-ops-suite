// tui/app/tests — App state-machine tests, grouped by behaviour.
// -----------------------------------------------------------------------------
// This module owns only the shared fixtures and helpers; the actual assertions
// live in focused submodules so no single test file grows into a god-file.
//
// Visibility note: these helpers are `pub(super)` so the sibling test
// submodules (`input`, `palette`, …) can call them. The submodules import the
// `App` type and friends via `use crate::tui::app::*;` — their own `super` is
// this module, not `app`, so they reach the app surface by its crate path.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use scriptvault_core::Config;
use std::fs;
use std::path::PathBuf;

mod input;
mod output;
mod palette;
mod results;
mod saved_search;

/// Build an `App` over a fresh temp vault holding two scripts (`deploy`,
/// `backup`). Returns the app plus the temp dir so each test can clean up.
pub(super) fn fixture_app() -> (App, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("scriptvault-tui-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("deploy.sh"),
        "#!/bin/bash\n# scriptvault.name: deploy\n# scriptvault.tags: ci, prod\necho a\n",
    )
    .unwrap();
    fs::write(
        dir.join("backup.sh"),
        "#!/bin/bash\n# scriptvault.name: backup\necho b\n",
    )
    .unwrap();

    let config = Config {
        roots: vec![dir.clone()],
        ..Default::default()
    };
    let scriptvault = ScriptVault::load_with_state_at(
        config,
        scriptvault_core::State::default(),
        dir.join("state.json"),
    )
    .unwrap();
    // Force a deterministic theme (state-machine tests don't depend on colour;
    // this also makes the fixture independent of the runner's NO_COLOR env).
    (App::with_theme(scriptvault, Theme::with_color(false)), dir)
}

/// A bare character key (no modifiers) — i.e. typing into the search box.
pub(super) fn press(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// A `Ctrl`+character chord (the direct action shortcuts).
pub(super) fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// A non-character key (arrows, Enter, Esc, …) with no modifiers.
pub(super) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

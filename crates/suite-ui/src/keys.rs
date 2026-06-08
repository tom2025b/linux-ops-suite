//! Shared keymap *conventions* both suite TUIs already use.
//!
//! This is NOT a key handler — each app keeps its own `match` over key events.
//! These constants just give the keys that BOTH tools share one name, so the
//! bindings don't silently drift apart and the footer hint is built from the
//! same source of truth. Tool-specific keys (ScriptVault's favorite toggle,
//! RexOps's numbered screen switches) stay in their own keymaps.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Quit the application.
pub const QUIT: char = 'q';
/// Toggle the help overlay.
pub const HELP: char = '?';
/// Open the command palette (primary binding; `:` is the alternate, below).
pub const PALETTE: char = 'p';
/// Alternate command-palette binding.
pub const PALETTE_ALT: char = ':';
/// Move the selection up (vi-style; `Up` arrow is the alternate).
pub const UP: char = 'k';
/// Move the selection down (vi-style; `Down` arrow is the alternate).
pub const DOWN: char = 'j';

/// True if this key event opens the command palette: `Ctrl-P` or a bare `:`.
/// The one bit of shared *interpretation* worth centralizing, since it spans a
/// modifier chord and a plain char.
pub fn is_palette(key: KeyEvent) -> bool {
    matches!(
        (key.code, key.modifiers.contains(KeyModifiers::CONTROL)),
        (KeyCode::Char(PALETTE), true) | (KeyCode::Char(PALETTE_ALT), false)
    )
}

/// The conventional footer hint line, e.g.
/// `↑/↓ move · Enter select · ? help · q quit`. Callers can append their own
/// tool-specific keys; this covers the shared core so the wording stays
/// consistent across tools.
pub fn key_hint() -> &'static str {
    "↑/↓ move · Enter select · ^P palette · ? help · q quit"
}

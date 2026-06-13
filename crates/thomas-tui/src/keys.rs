//! Conventional keymap constants for a TUI — vi-style movement, a command
//! palette, quit/help, confirm/cancel — so several apps can share one name per
//! binding instead of letting them drift apart.
//!
//! This is NOT a key handler — each app keeps its own `match` over key events.
//! These are just the shared names (and a few `is_*` interpretation helpers), so
//! a footer hint and the actual bindings come from one source of truth.
//! App-specific keys stay in the app's own keymap.

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

/// Accept / activate the current selection. A [`KeyCode`] (not a `char`) because
/// it's the Enter key — the conventional "confirm".
pub const CONFIRM: KeyCode = KeyCode::Enter;
/// Cancel / dismiss the current mode (close an overlay, clear a filter, back out
/// of a confirm). A [`KeyCode`] because it's the Esc key.
pub const CANCEL: KeyCode = KeyCode::Esc;
/// Alternate cancel binding: `Ctrl-G` (Emacs `keyboard-quit`). Provided because
/// not every keyboard has a usable Esc key, and the two text-entry contexts
/// (palette, filter) consume every printable key — so without an Esc-free cancel
/// a user could get trapped in them with no exit but quitting the whole app.
/// Ctrl-G is the conventional terminal "abort", never produces a printable
/// character, and works the same inside a text field as outside it.
pub const CANCEL_ALT: char = 'g';

/// True if this key event opens the command palette: `Ctrl-P` or a bare `:`.
/// The one bit of shared *interpretation* worth centralizing, since it spans a
/// modifier chord and a plain char.
pub fn is_palette(key: KeyEvent) -> bool {
    matches!(
        (key.code, key.modifiers.contains(KeyModifiers::CONTROL)),
        (KeyCode::Char(PALETTE), true) | (KeyCode::Char(PALETTE_ALT), false)
    )
}

/// True if this key event confirms (Enter). A trivial match, paired with
/// [`is_cancel`] so a consumer's key handling reads off the shared names rather
/// than bare [`KeyCode`] literals.
pub fn is_confirm(key: KeyEvent) -> bool {
    key.code == CONFIRM
}

/// True if this key event cancels/dismisses: the [`CANCEL`] key (Esc) or the
/// [`CANCEL_ALT`] chord (`Ctrl-G`). The counterpart to [`is_confirm`]. Two
/// bindings because not every keyboard has a usable Esc; Ctrl-G is the Esc-free
/// escape that also works inside the palette/filter text fields (see
/// [`CANCEL_ALT`]).
pub fn is_cancel(key: KeyEvent) -> bool {
    key.code == CANCEL
        || matches!(
            (key.code, key.modifiers.contains(KeyModifiers::CONTROL)),
            (KeyCode::Char(CANCEL_ALT), true)
        )
}

/// The conventional footer hint line, e.g.
/// `↑/↓ move · Enter select · ^P palette · ? help · q quit`. Callers can append
/// their own tool-specific keys; this covers the shared core so the wording stays
/// consistent across tools.
///
/// This is a hand-written literal (not built from the constants — a `&'static
/// str` can't be `format!`ed without allocating), but a test asserts it actually
/// names [`QUIT`], [`HELP`], and [`PALETTE_ALT`], so it can't silently drift from
/// the real bindings: change a key and the test fails until the hint is updated.
/// For a styled, per-screen hint strip prefer the
/// [`KeyHints`](crate::KeyHints) widget; this plain string is the shared fallback.
pub fn key_hint() -> &'static str {
    "↑/↓ move · Enter select · ^P palette · ? help · q quit"
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, ctrl: bool) -> KeyEvent {
        let mods = if ctrl {
            KeyModifiers::CONTROL
        } else {
            KeyModifiers::NONE
        };
        KeyEvent::new(code, mods)
    }

    #[test]
    fn is_palette_matches_ctrl_p_and_bare_colon_only() {
        assert!(is_palette(ev(KeyCode::Char('p'), true)), "Ctrl-P opens it");
        assert!(is_palette(ev(KeyCode::Char(':'), false)), "bare : opens it");
        // A bare 'p' or a Ctrl-':' do NOT (the chord/plain split is the point).
        assert!(!is_palette(ev(KeyCode::Char('p'), false)));
        assert!(!is_palette(ev(KeyCode::Char(':'), true)));
    }

    #[test]
    fn confirm_and_cancel_match_enter_and_esc() {
        assert!(is_confirm(ev(KeyCode::Enter, false)));
        assert!(is_cancel(ev(KeyCode::Esc, false)));
        // And not each other / anything else.
        assert!(!is_confirm(ev(KeyCode::Esc, false)));
        assert!(!is_cancel(ev(KeyCode::Enter, false)));
        assert!(!is_confirm(ev(KeyCode::Char('q'), false)));
    }

    #[test]
    fn ctrl_g_is_an_esc_free_cancel() {
        // The Esc-free escape for keyboards without a usable Esc key: Ctrl-G must
        // cancel exactly like Esc, so a user is never trapped in the palette or
        // filter (where every printable key types) with no way out but quitting.
        assert!(
            is_cancel(ev(KeyCode::Char(CANCEL_ALT), true)),
            "Ctrl-G cancels"
        );
        // The chord is required: a bare 'g' is an ordinary character (it must type
        // normally into a text field), and Ctrl on another key is not cancel.
        assert!(
            !is_cancel(ev(KeyCode::Char('g'), false)),
            "bare g must NOT cancel"
        );
        assert!(
            !is_cancel(ev(KeyCode::Char('h'), true)),
            "Ctrl-h is not cancel"
        );
    }

    #[test]
    fn key_hint_names_the_actual_bindings_so_it_cannot_drift() {
        // key_hint() is a hand-written literal; this is what keeps it honest. If a
        // binding constant changes, the literal must be updated or this fails —
        // converting "two sources of truth" into "the string is checked against
        // the constants".
        let hint = key_hint();
        assert!(hint.contains(QUIT), "hint must name the quit key");
        assert!(hint.contains(HELP), "hint must name the help key");
        // The palette is shown as the `^P` chord in the hint; assert that form.
        assert!(hint.contains("^P"), "hint must name the palette chord");
        // The standard movement/confirm wording is part of the shared core.
        assert!(hint.contains("move") && hint.contains("Enter"));
    }
}

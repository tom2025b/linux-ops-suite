//! Pulse's input layer: crossterm key events mapped onto a small [`Key`] enum.
//!
//! The terminal lifecycle (raw mode, alt screen, the restoring panic hook, the
//! cockpit suspend) is owned by `suite_ui::Tui`; this module is only the input
//! side. [`read_event`] blocks for the next keypress and maps it onto [`Key`],
//! the vocabulary the pure state machine in [`crate::app`] matches on — so the
//! navigation model never sees crossterm directly.

use std::io;

/// The keys Pulse acts on. Anything else is ignored by the loop (or, in the
/// search box, `Char` carries the typed letter).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    /// "Leave" — Ctrl-D / Ctrl-C; the loop treats it as quit.
    Eof,
    /// A key we don't model (arrows, function keys, …); the loop ignores it.
    Other,
}

/// Block on the next key from crossterm, mapped onto Pulse's [`Key`] vocabulary.
///
/// Non-key events (resize, mouse, focus, paste) are skipped so the caller only
/// ever sees a real keypress; crossterm decodes CSI/SS3 sequences (arrows,
/// function keys) for us, which then map to [`Key::Other`] and are ignored.
pub fn read_event() -> io::Result<Key> {
    use crossterm::event::{self, Event, KeyEventKind};
    loop {
        match event::read()? {
            Event::Key(k) => {
                // Act only on key *press* — terminals (and Windows) can emit
                // Release/Repeat too, and a release must not double-fire a step.
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                return Ok(map_key(k.code, k.modifiers));
            }
            // Resize/mouse/focus/paste: the loop repaints per key and ratatui
            // re-reads the size on the next draw, so just wait for real input.
            _ => continue,
        }
    }
}

/// Map a crossterm key (code + modifiers) onto Pulse's [`Key`]. Ctrl-D and Ctrl-C
/// both mean "leave" → [`Key::Eof`] (the loop treats it as quit).
fn map_key(code: crossterm::event::KeyCode, mods: crossterm::event::KeyModifiers) -> Key {
    use crossterm::event::{KeyCode, KeyModifiers};
    if mods.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char('d') | KeyCode::Char('c') = code {
            return Key::Eof;
        }
    }
    match code {
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Char(c) => Key::Char(c),
        // Arrows, function keys, Tab, etc. aren't in Pulse's keymap → ignored.
        _ => Key::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossterm_keys_map_to_the_same_key_vocabulary() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let none = KeyModifiers::NONE;
        // The keys Pulse's keymap acts on map straight across.
        assert_eq!(map_key(KeyCode::Enter, none), Key::Enter);
        assert_eq!(map_key(KeyCode::Esc, none), Key::Esc);
        assert_eq!(map_key(KeyCode::Backspace, none), Key::Backspace);
        assert_eq!(map_key(KeyCode::Char('q'), none), Key::Char('q'));
        assert_eq!(map_key(KeyCode::Char('/'), none), Key::Char('/'));
        // Ctrl-D and Ctrl-C both quit (EOF).
        assert_eq!(map_key(KeyCode::Char('d'), KeyModifiers::CONTROL), Key::Eof);
        assert_eq!(map_key(KeyCode::Char('c'), KeyModifiers::CONTROL), Key::Eof);
        // Arrows / function keys aren't in the keymap → Other (ignored).
        assert_eq!(map_key(KeyCode::Up, none), Key::Other);
        assert_eq!(map_key(KeyCode::F(1), none), Key::Other);
        assert_eq!(map_key(KeyCode::Tab, none), Key::Other);
    }

    #[test]
    fn typed_letters_map_to_literal_chars_for_the_search_box() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let none = KeyModifiers::NONE;
        // In the search box every printable key is literal text — including `r`,
        // which is the cockpit shortcut elsewhere. The adapter must yield a plain
        // Char and never special-case command letters; app::handle owns the
        // "search box swallows it" rule, so the *mapping* stays dumb.
        for c in ['r', 'q', 'a', 'Z', '7', ' '] {
            assert_eq!(map_key(KeyCode::Char(c), none), Key::Char(c));
        }
        // Shift-held letters still arrive as a Char (the char is already cased).
        assert_eq!(
            map_key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            Key::Char('A')
        );
    }
}

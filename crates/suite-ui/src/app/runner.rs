//! `App`: a thin runner over [`Tui`](super::Tui) for the simple case — one
//! screen, one keymap (which IS `on_key`), no background channels, no adaptive
//! polling. Tools needing any of those drive `Tui` directly.

use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::tui::{Tui, TuiError, TuiOptions};
use crate::Theme;

/// What a key handler tells the loop to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep running.
    Continue,
    /// Quit the loop (and restore the terminal).
    Exit,
}

/// A drawable, key-driven root screen — the whole contract [`App`] needs. The
/// screen owns its own state and mutates it directly in `on_key`; there is no
/// `Action` indirection.
pub trait Screen {
    /// Draw the current state into the frame.
    fn render(&mut self, frame: &mut Frame, theme: Theme);
    /// Handle one key press; return [`Flow::Exit`] to quit.
    fn on_key(&mut self, key: KeyEvent) -> Flow;
    /// Called once per tick when no key arrived (e.g. clear a transient status).
    fn on_tick(&mut self) {}
}

/// One loop iteration's key decision, split out so it is testable without a
/// terminal: only Press events reach `on_key`; everything else is `Continue`.
fn dispatch_key(screen: &mut impl Screen, event: Event) -> Flow {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => screen.on_key(key),
        _ => Flow::Continue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

    /// A fake screen that quits on 'q' and counts calls.
    #[derive(Default)]
    struct Fake {
        keys_seen: usize,
        ticks: usize,
    }
    impl Screen for Fake {
        fn render(&mut self, _f: &mut Frame, _t: Theme) {}
        fn on_key(&mut self, key: KeyEvent) -> Flow {
            self.keys_seen += 1;
            if key.code == KeyCode::Char('q') {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
        fn on_tick(&mut self) {
            self.ticks += 1;
        }
    }

    fn press(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    #[test]
    fn dispatch_quits_on_exit_key_and_continues_otherwise() {
        let mut s = Fake::default();
        assert_eq!(dispatch_key(&mut s, press('j')), Flow::Continue);
        assert_eq!(dispatch_key(&mut s, press('q')), Flow::Exit);
        assert_eq!(s.keys_seen, 2, "both presses reached on_key");
    }

    #[test]
    fn dispatch_ignores_non_press_and_non_key_events() {
        let mut s = Fake::default();
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(dispatch_key(&mut s, release), Flow::Continue);
        assert_eq!(dispatch_key(&mut s, Event::FocusGained), Flow::Continue);
        assert_eq!(s.keys_seen, 0, "no non-press event reached on_key");
    }
}

//! `App`: a thin runner over [`Tui`](super::Tui) for the simple case — one
//! screen, one keymap (which IS `on_key`), no background channels, no adaptive
//! polling. Tools needing any of those drive `Tui` directly.

use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::Frame;

use super::tui::{Tui, TuiError, TuiOptions};
use crate::Theme;

/// What a key handler tells the loop to do next.
///
/// `#[must_use]`: a dropped `Flow` is almost always a bug — it means an
/// `on_key`/`dispatch_key` result said "quit" and the caller silently ignored it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
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
    /// Called once per tick when no key arrived within the poll timeout (e.g.
    /// clear a transient status). Not called before the first draw, and not
    /// called on iterations where a key WAS handled. Default: do nothing.
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

/// A thin runner over [`Tui`]. Construct with a [`Theme`], optionally tweak the
/// envelope and tick rate, then `run` a [`Screen`].
///
/// `run` drives the common-denominator loop: draw → poll(tick) → dispatch a key
/// (or `on_tick` on timeout) → repeat, with the terminal restored on the way
/// out by the underlying `Tui`. It deliberately has **no** background-channel
/// draining or adaptive polling — reach for [`Tui`] directly when you need
/// either (see RexOps/ScriptVault).
pub struct App {
    theme: Theme,
    opts: TuiOptions,
    tick: Duration,
}

impl App {
    /// Start from a resolved [`Theme`]. Defaults: hide the cursor, 200ms tick.
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            opts: TuiOptions {
                hide_cursor: true,
                ..Default::default()
            },
            tick: Duration::from_millis(200),
        }
    }

    /// Override the terminal envelope (mouse capture, require_tty, cursor).
    ///
    /// This REPLACES all options, including the `hide_cursor: true` that
    /// [`App::new`] sets — pass `hide_cursor: true` in `opts` if you still want
    /// the cursor hidden.
    pub fn with_options(mut self, opts: TuiOptions) -> Self {
        self.opts = opts;
        self
    }

    /// Override the poll timeout — the `Duration` `run` waits for a key each
    /// iteration before calling [`Screen::on_tick`] (default 200ms).
    pub fn tick_rate(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Set up the terminal, run the loop to completion, and restore on exit.
    pub fn run(self, mut root: impl Screen) -> Result<(), TuiError> {
        let mut tui = Tui::new(self.opts)?;
        loop {
            tui.terminal()
                .draw(|f| root.render(f, self.theme))
                .map_err(TuiError::Io)?;
            if event::poll(self.tick).map_err(TuiError::Io)? {
                let ev = event::read().map_err(TuiError::Io)?;
                if dispatch_key(&mut root, ev) == Flow::Exit {
                    break;
                }
            } else {
                root.on_tick();
            }
        }
        Ok(()) // `tui` drops here → guaranteed restore
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

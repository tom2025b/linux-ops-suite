//! # thomas-tui
//!
//! A small, general-purpose terminal-UI toolkit — the terminal plumbing I want
//! in every [`ratatui`]-based app, with no application or domain vocabulary
//! attached. The first piece is the terminal lifecycle:
//!
//! - [`Tui`] — a RAII terminal scope guard. Construct it to enter TUI mode
//!   (raw mode, alt screen, a panic-safe restoring hook, optional cursor-hide /
//!   mouse capture / tty gate); drop it — on normal return, `?` propagation, or
//!   a panic unwind — to restore the terminal. It also handles the awkward
//!   parts: [`Tui::suspended`] leaves the alt screen to run a full-screen child
//!   (an editor, a pager) and re-enters cleanly, draining any input the child
//!   left buffered; [`Tui::print_after_exit`] queues lines to land in the user's
//!   real shell after teardown rather than on the alt screen.
//!
//! Drive your own event loop via [`Tui::terminal`] — the guard owns the
//! terminal *lifecycle* (the mechanism every TUI repeats), never the event loop
//! or any application state.
//!
//! Alongside the guard, two domain-free building blocks:
//!
//! - [`centered_rect`] / [`centered_fixed`] — center a `Rect` in another (by
//!   percentage or at a fixed, parent-clamped size); the basis for any overlay.
//! - [`truncate_path`] / [`truncate_desc`] — Unicode-aware string truncation
//!   with a single `…`, keeping the path tail or the description head.

mod layout;
mod text;
mod tui;

pub use layout::{centered_fixed, centered_rect};
pub use text::{truncate_desc, truncate_path};
pub use tui::{Tui, TuiError, TuiOptions};

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
//! Alongside the guard, the domain-free building blocks:
//!
//! - [`Theme`] — an accent hue behind one `NO_COLOR` gate ([`ThemeChoice`],
//!   [`ColorChoice`]), with the semantic styles a renderer asks for by name
//!   (`prompt`, `title`, `dim`, `selection`, …) plus the [`Severity`]/[`Health`]
//!   status axes.
//! - [`centered_rect`] / [`centered_fixed`] — center a `Rect` in another (by
//!   percentage or at a fixed, parent-clamped size); the basis for any overlay.
//! - [`truncate_path`] / [`truncate_desc`] — Unicode-aware string truncation
//!   with a single `…`, keeping the path tail or the description head.
//! - [`SearchBar`] — a one-line live-filter input affordance (prompt glyph,
//!   query or placeholder, optional match count). Renders only; never captures
//!   input.
//! - [`KeyHints`] — a one-line footer strip of `key → label` shortcut hints,
//!   key accented, label dim, `•`-separated.
//! - [`EmptyState`] — a centered, calm "nothing to show here" placeholder
//!   (dim+bold message, optional dimmer hint); text only, no border.
//! - [`Counted`] — a "N of M" shown-of-total count span, accented when the list
//!   is narrowed and dim when it shows everything.
//!
//! ## The `clap` feature
//!
//! Off by default. Enabling it derives `clap::ValueEnum` on [`ThemeChoice`] and
//! [`ColorChoice`] so a consumer can parse `--theme`/`--color` straight into
//! them. Consumers that don't use clap stay lean.

mod counted;
mod empty_state;
mod key_hints;
mod layout;
mod search_bar;
mod text;
mod theme;
mod tui;

pub use counted::Counted;
pub use empty_state::EmptyState;
pub use key_hints::KeyHints;
pub use layout::{centered_fixed, centered_rect};
pub use search_bar::SearchBar;
pub use text::{truncate_desc, truncate_path};
pub use theme::{ColorChoice, Health, Severity, Theme, ThemeChoice};
pub use tui::{Tui, TuiError, TuiOptions};

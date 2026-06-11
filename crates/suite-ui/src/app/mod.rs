//! Shared App runtime: a terminal scope guard.
//!
//! `Tui` is a RAII guard owning the terminal envelope (setup, panic-safe
//! teardown via `Drop`, ordered post-exit stdout). Drive your own event loop
//! with `Tui::terminal`; this is what every suite tool does.

mod tui;

pub use tui::{Tui, TuiError, TuiOptions};

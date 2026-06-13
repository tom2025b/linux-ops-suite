//! Shared App runtime: a terminal scope guard.
//!
//! `Tui` is a RAII guard owning the terminal envelope (setup, panic-safe
//! teardown via `Drop`, ordered post-exit stdout). Drive your own event loop
//! with `Tui::terminal`; this is what every suite tool does.
//!
//! The guard itself now lives in the general-purpose [`thomas-tui`](thomas_tui)
//! crate — it has no suite vocabulary, so it's reusable across any ratatui app.
//! suite-ui re-exports it here unchanged, so consumers keep importing
//! `suite_ui::Tui` exactly as before.

pub use thomas_tui::{Tui, TuiError, TuiOptions};

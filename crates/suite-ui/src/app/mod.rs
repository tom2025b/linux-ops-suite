//! Shared App runtime: a terminal scope guard (the foundation every tool can
//! adopt) and a thin optional runner on top.
//!
//! - `Tui` is a RAII guard owning the terminal envelope (setup, panic-safe
//!   teardown via `Drop`, ordered post-exit stdout). Drive your own event loop
//!   with `Tui::terminal`; this is what tools with background channels or
//!   adaptive polling (RexOps, ScriptVault) use.
//! - `App` is a thin builder over `Tui` that runs a minimal
//!   draw→poll→dispatch loop for the simple case (`App::new(theme).run(root)`).
//!
//! There is no `Component`/`Action`/event-bus here by design — `App` is sugar,
//! `Tui` is the contract.

mod runner;
mod tui;

pub use runner::{Flow, Screen};
// TODO(Task 6): add App to the runner re-export
pub use tui::{Tui, TuiError, TuiOptions};

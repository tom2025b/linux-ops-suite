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

mod tui;
// TODO(Task 5/6): add `mod runner;` and re-export App/Flow/Screen

pub use tui::TuiError;
// TODO(Task 2): re-export Tui, TuiOptions once they exist

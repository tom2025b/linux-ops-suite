//! TUI application state and event loop.
//!
//! This module keeps state transitions, terminal input handling, and tests in
//! separate files so each part has one clear responsibility.

mod event_loop;
mod state;

#[cfg(test)]
mod tests;

pub(crate) use event_loop::run_app;
pub(crate) use state::{SortMode, TuiApp};

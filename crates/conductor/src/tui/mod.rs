//! Conductor's interactive TUI. Dependency-free, modeled on pulse: a hand-rolled
//! terminal driver (`term`), pure frame renderers (`frame`), a color resolver
//! (`style`), and the event loop (this module, added in a later task).

pub mod style;
pub mod term;

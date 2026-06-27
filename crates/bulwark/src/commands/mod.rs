//! Per-command implementations for the `bulwark` binary.
//!
//! `main.rs` stays a thin CLI parser/dispatcher; each non-trivial subcommand
//! gets its own module here so no single file grows into a grab-bag.

pub mod workstate_feed;

//! Bulwark Core — Read-only, YAML-driven safety and inventory engine.
//!
//! This is the pure library entry point. Reusable config loading, scanning, rule
//! evaluation, classification, and report rendering live here or under
//! `bulwark_core::core::*`.
//!
//! ## Architecture layers (core / app / adapters)
//! - `bulwark_core::core` (and `bulwark_core::core::model`) — pure domain model + services.
//!   Zero side effects beyond read-only filesystem during scan. The single source
//!   of truth for types and logic. Safe to use from TUI, future GUI, tests, or
//!   ScriptVault bridge code.
//! - `bulwark_core::app` — thin application layer facade. Re-exports the most common
//!   high-level operations (`collect_*`) plus the model types. This is the
//!   "use this for building a UI or tool on top of Bulwark" entry point.
//!
//! Presentation adapters live outside this crate. The `bulwark` binary owns
//! Clap argument parsing, stdout/stderr printing, and the optional Ratatui TUI.
//!
//! # Key Design Principles
//! - No god modules. Each file has one clear responsibility.
//! - Library code uses `BulwarkError` (thiserror). CLI layer may use anyhow.
//! - All public functions that can fail return `Result<_, BulwarkError>`.
//! - Deterministic output everywhere (sorted by path).
//! - Read-only in the MVP. We never execute or mutate user files.

pub mod app;
pub mod core;
pub mod error;

// Re-export the main types so users of the library can write `bulwark::Config`
// and `bulwark::DiscoveredFile` instead of the longer paths.
//
// We also surface `bulwark::model::*` (the strong central model layer) and
// `bulwark::app::*` (application services) for consumers who prefer the
// explicit "core / app / tui" separation story.
pub use core::config::Config;
pub use core::engine::{
    ClassifiedEntry, ClassifiedInventory, Inventory, collect_classified_inventory,
    collect_inventory,
};
pub use core::entry::{Language, ScriptEntry, SidecarMetadata};
pub use core::model; // strong canonical model hub: bulwark::model::ScriptEntry etc.
pub use core::report::{
    ColorChoice, WORKSTATE_FEED_SCHEMA_VERSION, render_human_table, render_json_classified,
    render_markdown_table_classified, render_workstate_feed,
};
pub use core::rules::{Classification, MatchSpec, RiskLevel, Rule, RuleEngine};
pub use core::scanner::{DiscoveredFile, ScanOutcome, ScanWarning};
pub use error::BulwarkError;

/// Current version of the Bulwark core library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

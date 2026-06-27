//! Application layer — thin orchestration and re-exports for consumers.
//!
//! This module exists to satisfy the requested "core / app / tui separation"
//! while honoring the "keep it simple" and "no unnecessary abstraction" rules.
//!
//! # Role
//! - Re-exports the high-level entry points (`collect_inventory`,
//!   `collect_classified_inventory`, the result types) so code that wants to
//!   think in "application services" has a single obvious path: `bulwark::app::*`.
//! - In the future this is the natural home for cross-cutting application
//!   concerns that are not pure domain (e.g. summary statistics helpers,
//!   export formats that combine report + data, a "scan context" that carries
//!   timing/warnings, etc.).
//! - Today it is deliberately tiny — a one-file re-export hub. We do not move
//!   the implementation out of `core::engine` because that would be churn with
//!   zero benefit and would risk breaking the existing public API surface
//!   documented in the README and lib.rs.
//!
//! # How TUI and CLI use it
//! - The TUI (when enabled) calls `bulwark::app::collect_classified_inventory`
//!   (or the direct `bulwark::` re-export — both work).
//! - The CLI in `main.rs` continues to use the root re-exports for backward
//!   compatibility and minimal diff.
//!
//! # Design decision: keep this layer (A1)
//! We deliberately keep the explicit `app` re-exports (rather than collapsing
//! them into the root or removing the layer) to maintain clear canonical paths
//! and compatibility with existing code and the `public_api_compat` integration
//! test. If the export strategy is ever revised, update that test in the same
//! change so the contract and the code never drift.

pub use crate::core::engine::{
    ClassifiedEntry, ClassifiedInventory, Inventory, collect_classified_inventory,
    collect_inventory,
};

// Re-export the canonical model types here as well for "app consumers" who
// want a single `bulwark::app::ClassifiedEntry` import path.
pub use crate::core::model::{
    Classification, ColorChoice, Config, DiscoveredFile, Language, MatchSpec, RiskLevel, Rule,
    ScanOutcome, ScanWarning, ScriptEntry, SidecarMetadata,
};

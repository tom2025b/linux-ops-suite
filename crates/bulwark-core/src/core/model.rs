//! Canonical domain model layer for Bulwark.
//!
//! This module is the **strong central source of truth** for the key data types that
//! flow through the entire system (scanner → enrichment → classification → reporting → TUI).
//!
//! # Why a dedicated `core::model` module?
//! - Query requirement: "Create a strong core::model layer with canonical types
//!   (ScriptEntry, Language, Classification, RiskLevel, etc.)."
//! - It gives readers one obvious place to answer "What are the primary nouns
//!   in Bulwark?"
//! - It does **not** duplicate definitions. We re-export from their logical homes
//!   (entry for ScriptEntry/Language, rules for Classification/RiskLevel, scanner
//!   for DiscoveredFile, config for Config, engine for the orchestration results).
//!   This preserves cohesion: language inference logic stays next to Language,
//!   rule types stay with the matching engine, etc.
//! - Future: if we ever need shared behavior (e.g. a `Model` trait or summary
//!   stats methods) we have a single module to evolve without touching 6 files.
//!
//! # Canonical types (the public contract)
//! - `DiscoveredFile` — raw scan result (path, size, exec bit).
//! - `ScriptEntry` — enriched (language, header desc, sidecar).
//! - `Language` — detected script language.
//! - `SidecarMetadata` — optional `.bulwark.yaml` companion data (bridge to ScriptVault).
//! - `Classification` / `RiskLevel` — rule engine output.
//! - `Rule`, `MatchSpec` — YAML rule vocabulary.
//! - `Config` — user configuration.
//! - `ScanOutcome` / `ScanWarning` — raw scan result + non-fatal warnings.
//! - `Inventory` / `ClassifiedEntry` / `ClassifiedInventory` — pipeline results.
//!
//! All of these are re-exported at the crate root (`bulwark::ScriptEntry`) for
//! ergonomic library use, and also available as `bulwark::core::model::ScriptEntry`.
//!
//! See also:
//! - `core::entry` for enrichment details
//! - `core::rules` for classification rules
//! - `core::engine` for the orchestration that produces `ClassifiedEntry`
//!
//! # Design decision: keep these explicit re-exports (A1)
//! We deliberately keep the explicit re-exports here (rather than collapsing to
//! a single hub or removing the layer) to maintain clear canonical paths and
//! compatibility with existing code and the `public_api_compat` integration
//! test. If the export strategy is ever revised, update that test in the same
//! change so the contract and the code never drift.
//!
//! This module stays tiny (< 100 LOC) forever. It is documentation + re-exports only.

pub use crate::core::config::Config;
pub use crate::core::engine::{ClassifiedEntry, ClassifiedInventory, Inventory};
pub use crate::core::entry::{Language, ScriptEntry, SidecarMetadata};
pub use crate::core::rules::{Classification, MatchSpec, RiskLevel, Rule};
pub use crate::core::scanner::{DiscoveredFile, ScanOutcome, ScanWarning};

// Re-export the report color choice here too because it is part of the
// "how consumers ask for presentation" surface that many UIs (CLI today,
// TUI tomorrow) will touch.
pub use crate::core::report::ColorChoice;

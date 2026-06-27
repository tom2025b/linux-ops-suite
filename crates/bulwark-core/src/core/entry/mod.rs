//! Rich script-entry domain model and enrichment helpers.
//!
//! This module turns a low-level [`DiscoveredFile`](crate::core::scanner::DiscoveredFile)
//! into the richer [`ScriptEntry`] used by rules, reports, and future UI
//! adapters. The implementation is split by responsibility so language
//! detection, sidecar metadata, and entry construction can evolve independently.

mod language;
mod metadata;
mod script_entry;

pub use language::Language;
pub use metadata::SidecarMetadata;
pub use script_entry::ScriptEntry;

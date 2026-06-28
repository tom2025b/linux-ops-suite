//! The published snapshot contract: the canonical normalized records, the
//! `Section`/status/provenance spine, and the versioned [`snapshot::Snapshot`]
//! itself.
//!
//! Raw feed-INPUT shapes (`model::raw` in the `workstate` producer) deliberately
//! do NOT live here: this crate is only what CONSUMERS read off disk, not the
//! untrusted upstream wire shapes the producer ingests.
//!
//! The declarations are alphabetized to stay rustfmt-clean.
pub mod normalized; // Script/Tool/Finding — canonical records consumers read.
pub mod provenance; // Section/FeedStatus/Provenance — the shared spine.
pub mod snapshot; // Snapshot — the public, versioned contract.

// `pub mod` declares a module AND makes it part of the library's public API.
// We expose all three stages so tests and the binary can drive each one. Each
// `mod foo;` tells the compiler to look for `foo/mod.rs` or `foo.rs`.
//
// The declarations are alphabetized to stay rustfmt-clean.
pub mod compile; // The builder that assembles a Snapshot from normalized data.
pub mod ingest; // The FeedSource trait + per-tool adapters that read feeds.
pub mod model; // raw feed-input shapes + re-exported contract (see model/mod.rs).

// Re-exports. The snapshot CONTRACT (model types, schema version, canonical path,
// atomic write) now lives in the `workstate-schema` crate so the producer and all
// consumers share ONE definition. We surface its pieces here at the same paths
// callers (and RexOps) already use, so `workstate::Snapshot` / `write_snapshot` /
// `default_output_path` keep resolving unchanged.
pub use workstate_schema::{default_output_path, write_snapshot, Snapshot};

pub use compile::SnapshotBuilder; // entry point for building a snapshot
pub use ingest::{FeedError, FeedSource}; // the ingestion trait + its error type

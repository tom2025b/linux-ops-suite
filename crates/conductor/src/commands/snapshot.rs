//! Read and write the canonical Workstate snapshot, via the shared schema crate.
//!
//! Conductor defines no snapshot model of its own: it loads and validates the
//! single canonical artifact through `workstate_schema` (so the contract can't
//! drift), and the only thing it ever writes is a rewind backup/restore of that
//! same artifact (see `commands::rewind`).

use std::path::Path;

use workstate_schema::{load_snapshot, write_snapshot, LoadError, Snapshot};

use crate::core::error::{Error, Result};

/// Load and validate the canonical snapshot at `path`, mapping the schema crate's
/// typed [`LoadError`] onto conductor's error type.
pub fn load(path: &Path) -> Result<Snapshot> {
    load_snapshot(path).map_err(|e| match e {
        LoadError::NotFound { .. } => Error::NotFound("workstate snapshot".into()),
        LoadError::Io { source, .. } => Error::Io(source),
        other => Error::Snapshot(other.to_string()),
    })
}

/// Atomically write `snapshot` to `path` — used only by rewind capture/restore,
/// which copy the canonical snapshot to/from conductor's restore-point store.
pub fn save(path: &Path, snapshot: &Snapshot) -> Result<()> {
    write_snapshot(snapshot, path).map_err(Error::Io)
}

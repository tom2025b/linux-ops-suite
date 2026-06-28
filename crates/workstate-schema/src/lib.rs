//! # workstate-schema
//!
//! The Workstate snapshot CONTRACT, in one crate: the model types consumers
//! deserialize ([`Snapshot`] and its sections), the [`SCHEMA_VERSION`] they gate
//! on, the single canonical [`default_output_path`] where the snapshot lives, and
//! the atomic [`write_snapshot`] / validating [`load_snapshot`] pair.
//!
//! The `workstate` producer depends on this crate to BUILD and WRITE snapshots;
//! every consumer (RexOps, Pulse, Conductor, toolbox-bridge, rewind, …) depends on
//! it to READ them. Because the format, the path, and the read/write code all live
//! here and nowhere else, the contract cannot drift between tools — there is no
//! second place to re-declare a struct, re-derive the path, or re-implement the
//! version check.

use std::path::{Path, PathBuf};

pub mod loader;
pub mod model;
mod writer;

// The public contract surface, promoted to the crate root so callers write
// `workstate_schema::Snapshot` / `::load_snapshot` rather than the long paths.
pub use loader::{load_snapshot, LoadError};
pub use model::snapshot::{Snapshot, SCHEMA_VERSION};
pub use writer::write_snapshot;

/// The single canonical location of the compiled snapshot:
/// `$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`, falling back to
/// `~/.local/share/rexops/feeds/...` when `$XDG_DATA_HOME` is unset.
///
/// THE one definition of where the snapshot lives. The producer writes here by
/// default and every consumer reads here by default, so no tool re-derives the
/// path. `argv`/config overrides layer on top of this in the respective tools.
///
/// Returns `None` only when neither `$XDG_DATA_HOME` nor `$HOME` is set, in which
/// case the caller decides on a fallback (the producer uses an in-crate path).
pub fn default_output_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(snapshot_path_under(&base))
}

/// The canonical snapshot location under a given data root:
/// `<root>/rexops/feeds/workstate.snapshot.json`. The ONE definition of the path
/// tail — [`default_output_path`] joins it onto the resolved XDG base, and a
/// consumer with its own root override (tests, power users) uses it directly, so
/// the location is never re-spelled anywhere else.
pub fn snapshot_path_under(data_root: &Path) -> PathBuf {
    data_root.join("rexops/feeds/workstate.snapshot.json")
}

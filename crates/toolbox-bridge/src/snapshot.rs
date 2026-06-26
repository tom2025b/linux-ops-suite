//! Read side: load the Workstate snapshot and expose its findings section.
//!
//! This is the ONLY ingestion path the bridge has. It never invokes Bulwark,
//! never reads Bulwark's own feed file — Workstate's compiled snapshot is the
//! single source of truth, deserialized through Workstate's own `Snapshot`
//! type so the contract cannot drift.

use std::path::{Path, PathBuf};

use workstate_schema::model::normalized::FindingInventory;
use workstate_schema::model::provenance::FeedStatus;
use workstate_schema::{LoadError, Snapshot};

use crate::error::BridgeError;

/// The shared suite location of the compiled snapshot. Delegates to the ONE
/// definition of that path — `workstate_schema::default_output_path()`
/// (`$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`, fallback
/// `~/.local/share/...`) — so the bridge can never drift from where the producer
/// actually writes. Only the `None` case (no `$XDG_DATA_HOME`/`$HOME`) is mapped
/// onto the bridge's own error type.
pub fn default_snapshot_path() -> Result<PathBuf, BridgeError> {
    workstate_schema::default_output_path().ok_or(BridgeError::NoDefaultPath { what: "snapshot" })
}

/// `$XDG_DATA_HOME`, falling back to `~/.local/share`. `None` only when neither
/// `$XDG_DATA_HOME` nor `$HOME` is set. Used for the bridge's OWN output feed path
/// (`workstate/feeds/toolbox-bridge.json`), which is not part of the snapshot
/// contract — the snapshot's location comes from `workstate_schema`.
pub(crate) fn xdg_data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// Read and validate the Workstate snapshot at `path`.
///
/// Delegates to the ONE canonical reader, `workstate_schema::load_snapshot`
/// (read → check `schema_version` → typed parse), mapping its typed [`LoadError`]
/// onto the bridge's own [`BridgeError`] so the CLI keeps its existing,
/// operator-facing error wording. The read logic and the version gate live in the
/// schema crate now, so the bridge cannot drift from the producer.
pub fn load_snapshot(path: &Path) -> Result<Snapshot, BridgeError> {
    workstate_schema::load_snapshot(path).map_err(|e| match e {
        LoadError::NotFound { path } => BridgeError::SnapshotNotFound(path.display().to_string()),
        LoadError::Io { path, source } => BridgeError::SnapshotIo {
            path: path.display().to_string(),
            source,
        },
        LoadError::UnsupportedVersion {
            found, supported, ..
        } => BridgeError::UnsupportedSchema {
            // The schema crate reports the declared version as u64; the bridge's
            // error type predates that and carries i64 — widen losslessly.
            found: found.map(|v| v as i64),
            supported,
        },
        LoadError::Malformed { path, reason } => BridgeError::SnapshotParse {
            path: path.display().to_string(),
            reason,
        },
        // LoadError is #[non_exhaustive]: a variant from a newer schema crate
        // degrades to a parse-style error rather than breaking the build.
        other => BridgeError::SnapshotParse {
            path: path.display().to_string(),
            reason: other.to_string(),
        },
    })
}

/// The findings section of a snapshot, reduced to what the bridge acts on.
#[derive(Debug)]
pub struct FindingsView<'a> {
    /// Bulwark's normalized findings plus its feed-level `generated_at`.
    pub inventory: &'a FindingInventory,
    /// True when the data should be treated with caution — Workstate marked the
    /// section `Stale` (known old) OR `FreshnessUnknown` (age undetermined). The
    /// bridge still converts such data (old/unknown-age findings beat no findings)
    /// but the caller should tell the operator to refresh.
    pub stale: bool,
}

/// Extract usable findings from the snapshot, degrading honestly when the
/// section has no data.
///
/// Data-bearing statuses convert: `Fresh`, `Stale`, and (v4) `FreshnessUnknown`
/// — the last two flag `stale: true` so the caller can caution the operator.
/// Everything else (`Missing`, `Failed`, `UnsupportedVersion`, `MissingVersion`,
/// `SourceMismatch`, or a status with no payload) is `FindingsUnavailable` — the
/// snapshot was fine but Bulwark's feed into Workstate wasn't, and the error says
/// how to refresh it.
pub fn findings_view(snapshot: &Snapshot) -> Result<FindingsView<'_>, BridgeError> {
    let section = &snapshot.findings;
    let status = status_label(&section.status);

    // Statuses that carry usable data. `FreshnessUnknown` (v4) joins Fresh/Stale:
    // it has real findings whose age we just couldn't determine, and the bridge's
    // policy is that having findings beats having none. The rejecting statuses
    // (Missing/Failed/*Version/SourceMismatch) genuinely have no data.
    let caution = match section.status {
        FeedStatus::Fresh => false,
        FeedStatus::Stale | FeedStatus::FreshnessUnknown => true,
        _ => return Err(BridgeError::FindingsUnavailable { status }),
    };

    match section.data.as_ref() {
        Some(inventory) => Ok(FindingsView {
            inventory,
            stale: caution,
        }),
        // A data-bearing status with no payload shouldn't happen, but a contract
        // consumer must not panic on it.
        None => Err(BridgeError::FindingsUnavailable {
            status: format!("{status} but carries no data"),
        }),
    }
}

/// Render a `FeedStatus` as the short label used in errors and summaries
/// (same wording as Workstate's own summary line).
pub fn status_label(status: &FeedStatus) -> String {
    match status {
        FeedStatus::Fresh => "Fresh".to_string(),
        FeedStatus::Stale => "Stale".to_string(),
        // v4: read OK but source age unknown — distinct from Stale ("known old").
        FeedStatus::FreshnessUnknown => "FreshnessUnknown".to_string(),
        FeedStatus::UnsupportedVersion { found, supported } => match found {
            Some(found) => format!("UnsupportedVersion ({found}; expected {supported})"),
            None => format!("UnsupportedVersion (missing; expected {supported})"),
        },
        // v4: no schema_version declared at all (distinct from a wrong one).
        FeedStatus::MissingVersion { supported } => {
            format!("MissingVersion (none declared; expected {supported})")
        }
        // v4: the feed's source_tool disagreed with the adapter that read it.
        FeedStatus::SourceMismatch { expected, found } => {
            format!("SourceMismatch (got '{found}'; expected '{expected}')")
        }
        FeedStatus::Missing => "Missing".to_string(),
        FeedStatus::Failed { reason } => format!("Failed ({reason})"),
        // `FeedStatus` is #[non_exhaustive]: a status added in a future Workstate
        // we don't yet label degrades to its Debug form rather than breaking the
        // build. Keeps this consumer forward-compatible across schema bumps.
        other => format!("{other:?}"),
    }
}

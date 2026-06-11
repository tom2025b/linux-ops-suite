//! Read side: load the Workstate snapshot and expose its findings section.
//!
//! This is the ONLY ingestion path the bridge has. It never invokes Bulwark,
//! never reads Bulwark's own feed file — Workstate's compiled snapshot is the
//! single source of truth, deserialized through Workstate's own `Snapshot`
//! type so the contract cannot drift.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use workstate::model::normalized::FindingInventory;
use workstate::model::provenance::FeedStatus;
use workstate::model::snapshot::SCHEMA_VERSION;
use workstate::Snapshot;

use crate::error::BridgeError;

/// The shared suite location of the compiled snapshot:
/// `$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`, falling back to
/// `~/.local/share/rexops/feeds/...`. This MUST mirror Workstate's
/// `default_output_path()` (and RexOps's `WorkstateAdapter::standard_path()`)
/// exactly — it is the contract for where the snapshot lives.
pub fn default_snapshot_path() -> Result<PathBuf, BridgeError> {
    xdg_data_home()
        .map(|base| base.join("rexops/feeds/workstate.snapshot.json"))
        .ok_or(BridgeError::NoDefaultPath { what: "snapshot" })
}

/// `$XDG_DATA_HOME`, falling back to `~/.local/share`. `None` only when
/// neither `$XDG_DATA_HOME` nor `$HOME` is set.
pub(crate) fn xdg_data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// Read and validate the Workstate snapshot at `path`.
///
/// Validation order follows CONTRACT_RULES.md: parse to a generic JSON value,
/// check `schema_version` FIRST, and only then deserialize into the typed
/// `Snapshot`. Checking the version against the raw value (not the typed
/// struct) means a future v4 snapshot whose shape no longer matches our
/// `Snapshot` type still produces the honest "unsupported schema_version 4"
/// message instead of a confusing field-level parse error.
pub fn load_snapshot(path: &Path) -> Result<Snapshot, BridgeError> {
    let display = path.display().to_string();

    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // Missing snapshot is the expected first-run state, with a known fix.
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(BridgeError::SnapshotNotFound(display));
        }
        Err(e) => {
            return Err(BridgeError::SnapshotIo {
                path: display,
                source: e,
            });
        }
    };

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| BridgeError::SnapshotParse {
            path: display.clone(),
            reason: e.to_string(),
        })?;

    let found = value.get("schema_version").and_then(|v| v.as_i64());
    if found != Some(i64::from(SCHEMA_VERSION)) {
        return Err(BridgeError::UnsupportedSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }

    serde_json::from_value(value).map_err(|e| BridgeError::SnapshotParse {
        path: display,
        reason: e.to_string(),
    })
}

/// The findings section of a snapshot, reduced to what the bridge acts on.
#[derive(Debug)]
pub struct FindingsView<'a> {
    /// Bulwark's normalized findings plus its feed-level `generated_at`.
    pub inventory: &'a FindingInventory,
    /// True when Workstate marked the section `Stale`. The bridge still
    /// converts stale data (old findings beat no findings) but the caller
    /// should tell the operator.
    pub stale: bool,
}

/// Extract usable findings from the snapshot, degrading honestly when the
/// section has no data.
///
/// `Fresh` and `Stale` sections with data convert; everything else
/// (`Missing`, `Failed`, `UnsupportedVersion`, or a status with no payload)
/// is `FindingsUnavailable` — the snapshot was fine but Bulwark's feed into
/// Workstate wasn't, and the error says how to refresh it.
pub fn findings_view(snapshot: &Snapshot) -> Result<FindingsView<'_>, BridgeError> {
    let section = &snapshot.findings;
    let status = status_label(&section.status);

    match section.status {
        FeedStatus::Fresh | FeedStatus::Stale => {}
        _ => return Err(BridgeError::FindingsUnavailable { status }),
    }

    match section.data.as_ref() {
        Some(inventory) => Ok(FindingsView {
            inventory,
            stale: matches!(section.status, FeedStatus::Stale),
        }),
        // Fresh/Stale with no payload shouldn't happen, but a contract
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
        FeedStatus::UnsupportedVersion { found, supported } => match found {
            Some(found) => format!("UnsupportedVersion ({found}; expected {supported})"),
            None => format!("UnsupportedVersion (missing; expected {supported})"),
        },
        FeedStatus::Missing => "Missing".to_string(),
        FeedStatus::Failed { reason } => format!("Failed ({reason})"),
    }
}

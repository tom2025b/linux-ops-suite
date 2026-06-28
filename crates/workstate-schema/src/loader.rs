//! Read side of the contract: load and validate a `snapshot.json`.
//!
//! This is the ONE canonical reader every consumer uses. The
//! "read → check `schema_version` → typed parse" sequence and the typed errors it
//! can produce live here and nowhere else, so no consumer re-implements the read
//! and the version gate cannot drift between tools.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::snapshot::{Snapshot, SCHEMA_VERSION};

/// Why loading a snapshot failed. Each consumer maps these to its own UX: a
/// missing snapshot is the expected first-run state (with a known fix); an
/// unsupported version says producer and consumer disagree on the contract; a
/// malformed file is corruption.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LoadError {
    /// No snapshot at `path` yet — the producer hasn't run. Expected first-run.
    #[error("workstate snapshot not found at {path}\nrun `workstate` to compile one")]
    NotFound { path: PathBuf },

    /// The file exists but could not be read (permissions, I/O).
    #[error("could not read workstate snapshot {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The file declares a `schema_version` this build does not understand.
    /// Checked against the raw JSON BEFORE typed parsing, so a future, differently
    /// shaped snapshot still yields this honest message rather than a confusing
    /// field-level parse error.
    #[error(
        "workstate snapshot {path} declares schema_version {found:?}; this build understands {supported}"
    )]
    UnsupportedVersion {
        path: PathBuf,
        found: Option<u64>,
        supported: u32,
    },

    /// The file is the right version but malformed.
    #[error("workstate snapshot {path} is malformed: {reason}")]
    Malformed { path: PathBuf, reason: String },
}

/// Load and validate the snapshot at `path`.
///
/// Order matters: read, then check `schema_version` against the RAW JSON value,
/// and only then deserialize into the typed [`Snapshot`]. Gating on the raw value
/// (not the typed struct) means a future, differently shaped snapshot still
/// produces the honest [`LoadError::UnsupportedVersion`] instead of a field
/// mismatch — the same fail-closed policy the producer uses on its feeds.
pub fn load_snapshot(path: &Path) -> Result<Snapshot, LoadError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(LoadError::NotFound {
                path: path.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(LoadError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| LoadError::Malformed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

    let found = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64);
    if found != Some(u64::from(SCHEMA_VERSION)) {
        return Err(LoadError::UnsupportedVersion {
            path: path.to_path_buf(),
            found,
            supported: SCHEMA_VERSION,
        });
    }

    serde_json::from_value(value).map_err(|e| LoadError::Malformed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::provenance::{FeedId, Section};
    use crate::write_snapshot;
    use chrono::Utc;

    /// A minimal, valid snapshot (every section Missing — no feeds, no fixtures).
    fn missing_snapshot() -> Snapshot {
        Snapshot::new(
            Utc::now(),
            Section::missing(FeedId("scriptvault".to_string())),
            Section::missing(FeedId("toolfoundry".to_string())),
            Section::missing(FeedId("bulwark".to_string())),
            Section::missing(FeedId("proto".to_string())),
        )
    }

    #[test]
    fn write_then_load_roundtrips() {
        let mut path = std::env::temp_dir();
        path.push(format!("ws_schema_loader_rt_{}.json", std::process::id()));
        write_snapshot(&missing_snapshot(), &path).expect("write must succeed");

        let loaded = load_snapshot(&path).expect("load must succeed");
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_not_found() {
        let path =
            std::env::temp_dir().join(format!("ws_schema_absent_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(matches!(
            load_snapshot(&path),
            Err(LoadError::NotFound { .. })
        ));
    }

    #[test]
    fn wrong_version_is_unsupported_not_a_parse_error() {
        let mut path = std::env::temp_dir();
        path.push(format!("ws_schema_badver_{}.json", std::process::id()));
        // A future version with a shape we don't model: must still report the
        // version mismatch, not a field-level parse error.
        std::fs::write(&path, r#"{"schema_version":999,"anything":true}"#).unwrap();

        match load_snapshot(&path) {
            Err(LoadError::UnsupportedVersion {
                found, supported, ..
            }) => {
                assert_eq!(found, Some(999));
                assert_eq!(supported, SCHEMA_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }
}

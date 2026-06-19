//! rewind — history, snapshot, and safe-rollback for the Linux Ops Suite.
//!
//! Where portman watches the *network* surface and tripwire watches the
//! *filesystem* surface, rewind is the suite's *time axis*: it records the
//! suite's own state files (the compiled Workstate snapshot, the producer feeds,
//! tripwire's baseline) into a content-addressed [`store`], lists the timeline,
//! and — in later phases — diffs any two points and restores under a hard safety
//! gate. It is read-only by default; the only write to a live path is a guarded
//! `restore`, and the only thing it writes routinely is its own store.
//!
//! This is **Phase 1**: the storage layer plus `capture`, `list` (the timeline),
//! and `sources`. Diff and the guarded restore land in later phases. The library
//! does the work and returns values; the binary only parses flags and renders.

pub mod capture;
pub mod error;
pub mod hash;
pub mod model;
pub mod report;
pub mod scan;
pub mod set;
pub mod store;
pub mod util;

use std::path::{Path, PathBuf};

use model::Manifest;
use set::CaptureSet;
use store::Store;

pub use error::RewindError;

/// Resolve the store directory: an explicit `--store` wins; otherwise the
/// suite's XDG data location. Errors only when no anchor dir can be found.
pub fn resolve_store_dir(store_override: Option<PathBuf>) -> Result<PathBuf, RewindError> {
    match store_override {
        Some(p) => Ok(p),
        None => util::store_dir().ok_or(RewindError::NoDataDir),
    }
}

/// Resolve the capture set without scanning — for `rewind sources`.
pub fn capture_set(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
) -> Result<CaptureSet, RewindError> {
    set::resolve(cli_paths, config_override)
}

/// Record the resolved capture set as a new immutable capture at `captured_at`
/// (RFC3339 UTC, supplied by the caller), with an optional label. Returns the
/// saved manifest for the caller to report.
pub fn record_capture(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
    store_override: Option<PathBuf>,
    captured_at: &str,
    label: Option<&str>,
) -> Result<Manifest, RewindError> {
    let store_dir = resolve_store_dir(store_override)?;
    let set = capture_set(cli_paths, config_override)?;
    let store = Store::open(store_dir);
    capture::capture(&set, &store, captured_at, label)
}

/// Load the capture timeline (newest first) from the store. Errors with
/// [`RewindError::NoStore`] when nothing has been captured yet, so the CLI can
/// say so cleanly rather than printing an empty list.
pub fn list_captures(
    store_override: Option<PathBuf>,
) -> Result<(Vec<Manifest>, PathBuf), RewindError> {
    let store_dir = resolve_store_dir(store_override)?;
    let store = Store::open(store_dir.clone());
    if !store.exists() {
        return Err(RewindError::NoStore { path: store_dir });
    }
    Ok((store.load_manifests()?, store_dir))
}

/// Store statistics for the `sources` footer: on-disk bytes and capture count.
/// A not-yet-created store reports zeros (not an error — `sources` is meaningful
/// before the first capture).
pub fn store_stats(store_override: Option<PathBuf>) -> Result<(u64, usize, PathBuf), RewindError> {
    let store_dir = resolve_store_dir(store_override)?;
    let store = Store::open(store_dir.clone());
    let (bytes, count) = if store.exists() {
        (store.store_bytes(), store.load_manifests()?.len())
    } else {
        (0, 0)
    };
    Ok((bytes, count, store_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_store_dir_honors_override() {
        let custom = PathBuf::from("/tmp/rewind-test-store");
        assert_eq!(resolve_store_dir(Some(custom.clone())).unwrap(), custom);
    }

    #[test]
    fn capture_then_list_roundtrip() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = dir.path().join("snap.json");
        std::fs::write(&f, br#"{"schema_version":4,"source_tool":"workstate"}"#).unwrap();

        // List before any capture -> NoStore.
        let err = list_captures(Some(store.clone())).unwrap_err();
        assert!(matches!(err, RewindError::NoStore { .. }));

        // Capture, then list shows it.
        let m = record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T14:22:05Z",
            Some("pre-upgrade"),
        )
        .unwrap();
        let (list, _) = list_captures(Some(store.clone())).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, m.id);
        assert_eq!(list[0].label.as_deref(), Some("pre-upgrade"));

        // Stats reflect one capture and a non-zero store.
        let (bytes, count, _) = store_stats(Some(store)).unwrap();
        assert_eq!(count, 1);
        assert!(bytes > 0);
    }

    #[test]
    fn second_identical_capture_dedupes_objects() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = dir.path().join("snap.json");
        std::fs::write(&f, br#"{"schema_version":4,"source_tool":"workstate"}"#).unwrap();

        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();
        let bytes_after_first = store_stats(Some(store.clone())).unwrap().0;

        // Same content, different time -> a new capture but no new object bytes.
        record_capture(
            &[f],
            None,
            Some(store.clone()),
            "2026-06-19T01:00:00Z",
            None,
        )
        .unwrap();
        let (bytes_after_second, count, _) = store_stats(Some(store)).unwrap();
        assert_eq!(count, 2, "two captures recorded");
        assert_eq!(
            bytes_after_first, bytes_after_second,
            "identical content adds no object bytes (dedup)"
        );
    }
}

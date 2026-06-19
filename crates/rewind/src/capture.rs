//! The capture workhorse. Resolve the capture set, scan it into the store
//! (writing each readable file's bytes as a content-addressed object), compute a
//! stable content-derived id, and save the manifest. A capture is immutable once
//! written; the id is derived from the entries (path + content hash + metadata)
//! plus the timestamp, so re-capturing identical content at a different time
//! yields a distinct capture but identical objects (dedup does the rest).

use crate::error::RewindError;
use crate::hash::Sha256;
use crate::model::{CaptureEntry, Manifest, MANIFEST_SCHEMA_VERSION};
use crate::scan::{self, Scanned};
use crate::set::CaptureSet;
use crate::store::Store;

/// Record `set` into `store` at time `captured_at` (RFC3339 UTC, injected by the
/// caller so this is testable), with an optional `label`. Returns the saved
/// [`Manifest`]. Errors only if the set is empty or the store can't be written.
pub fn capture(
    set: &CaptureSet,
    store: &Store,
    captured_at: &str,
    label: Option<&str>,
) -> Result<Manifest, RewindError> {
    if set.specs.is_empty() {
        return Err(RewindError::EmptySet);
    }

    store.ensure_dirs()?;
    let Scanned { entries, source } = scan::scan_into(set, store)?;

    // A capture must cover at least one present path. An empty *result* (every
    // configured path absent) is not a useful capture — treat it like an empty
    // set so the operator gets a clear "nothing to capture" rather than a
    // mysterious empty entry in the timeline.
    if entries.is_empty() {
        return Err(RewindError::EmptySet);
    }

    let id = compute_id(&entries, captured_at, label);

    let manifest = Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        source_tool: "rewind".to_string(),
        id,
        captured_at: captured_at.to_string(),
        label: label.map(str::to_string),
        set_source: source.tag().to_string(),
        entries,
    };

    store.save_manifest(&manifest)?;
    Ok(manifest)
}

/// Compute a stable, content-derived capture id: SHA-256 over the timestamp,
/// label, and each entry's identity (path + content hash + mode/owner). Two
/// captures with the same content at the same instant collide intentionally
/// (they *are* the same capture); differing time or content diverges.
fn compute_id(entries: &[CaptureEntry], captured_at: &str, label: Option<&str>) -> String {
    let mut h = Sha256::new();
    h.update(captured_at.as_bytes());
    h.update(b"\0");
    h.update(label.unwrap_or("").as_bytes());
    h.update(b"\0");
    for e in entries {
        h.update(e.path.as_bytes());
        h.update(b"\0");
        h.update(e.hash.as_deref().unwrap_or("").as_bytes());
        h.update(b"\0");
        h.update(e.mode.as_deref().unwrap_or("").as_bytes());
        h.update(b"\0");
        // Owner is part of identity for restore fidelity, but mtime is NOT —
        // a touched-but-identical file should not change the capture id.
        if let (Some(u), Some(g)) = (e.uid, e.gid) {
            h.update(format!("{u}:{g}").as_bytes());
        }
        h.update(b"\n");
    }
    h.hex()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::set::{CaptureSpec, SetSource};
    use std::fs;
    use tempfile::tempdir;

    fn set_of(specs: Vec<CaptureSpec>) -> CaptureSet {
        CaptureSet {
            specs,
            source: SetSource::Cli,
        }
    }

    #[test]
    fn capture_writes_a_manifest_and_objects() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("store"));
        let f = dir.path().join("snap.json");
        fs::write(&f, br#"{"schema_version":4,"source_tool":"workstate"}"#).unwrap();

        let m = capture(
            &set_of(vec![CaptureSpec::new(f.clone())]),
            &store,
            "2026-06-19T14:22:05Z",
            Some("pre-upgrade"),
        )
        .unwrap();

        assert_eq!(m.source_tool, "rewind");
        assert_eq!(m.label.as_deref(), Some("pre-upgrade"));
        assert_eq!(m.path_count(), 1);
        assert_eq!(m.set_source, "cli");
        assert!(!m.id.is_empty());

        // It is loadable back from the store.
        let loaded = store.load_manifests().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, m.id);
    }

    #[test]
    fn id_is_stable_for_identical_content_and_time() {
        let dir = tempdir().unwrap();
        let store_a = Store::open(dir.path().join("a"));
        let store_b = Store::open(dir.path().join("b"));
        let f = dir.path().join("x");
        fs::write(&f, b"same").unwrap();

        let ma = capture(
            &set_of(vec![CaptureSpec::new(f.clone())]),
            &store_a,
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();
        let mb = capture(
            &set_of(vec![CaptureSpec::new(f)]),
            &store_b,
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();
        assert_eq!(ma.id, mb.id, "same content + time + label -> same id");
    }

    #[test]
    fn id_diverges_on_different_time() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("store"));
        let f = dir.path().join("x");
        fs::write(&f, b"same").unwrap();

        let m1 = capture(
            &set_of(vec![CaptureSpec::new(f.clone())]),
            &store,
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();
        let m2 = capture(
            &set_of(vec![CaptureSpec::new(f)]),
            &store,
            "2026-06-19T01:00:00Z",
            None,
        )
        .unwrap();
        assert_ne!(m1.id, m2.id);
        // Two captures, but the identical content is one shared object.
        assert_eq!(store.load_manifests().unwrap().len(), 2);
    }

    #[test]
    fn empty_set_is_an_error() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("store"));
        let err = capture(&set_of(vec![]), &store, "2026-06-19T00:00:00Z", None).unwrap_err();
        assert!(matches!(err, RewindError::EmptySet));
    }

    #[test]
    fn all_paths_absent_is_an_error() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("store"));
        let err = capture(
            &set_of(vec![CaptureSpec::new(dir.path().join("nope"))]),
            &store,
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap_err();
        assert!(matches!(err, RewindError::EmptySet));
    }
}

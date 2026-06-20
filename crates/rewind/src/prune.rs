//! Pruning — the one path besides restore that *deletes*. It removes whole
//! captures by age (`--older-than`) and/or count (`--keep-last`), and optionally
//! garbage-collects objects no surviving capture references (`--gc`). Nothing is
//! ever auto-pruned; the operator (or their cron line) decides retention.
//!
//! Safety: deletion is immediate (no dry-run gate — that's restore's job), but
//! `--gc` is the only thing that touches the object pool, and the mark-and-sweep
//! computes the live hash set from **all surviving manifests** before deleting,
//! so a blob shared with a kept capture is never removed. Manifests are deleted
//! first, then the sweep runs over what remains.

use serde::Serialize;

use crate::error::RewindError;
use crate::model::Manifest;
use crate::store::Store;

/// One removed capture, recorded for the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrunedCapture {
    pub id: String,
    pub captured_at: String,
}

/// What a prune run removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PruneOutcome {
    pub removed: Vec<PrunedCapture>,
    pub removed_count: usize,
    pub gc: bool,
    pub objects_removed: usize,
    pub bytes_reclaimed: u64,
    pub remaining_captures: usize,
}

/// Run a prune over `store`. `keep_last` keeps the newest N captures;
/// `older_than` (e.g. `30d`, `12h`) removes captures older than that relative to
/// `now` (RFC3339 UTC, injected for testability). Both may combine — a capture is
/// removed if it fails *either* retention rule. `gc` then sweeps unreferenced
/// objects. A bad `--older-than` is a hard error (exit 3), never silent.
pub fn run(
    store: &Store,
    keep_last: Option<usize>,
    older_than: Option<&str>,
    gc: bool,
    now: &str,
) -> Result<PruneOutcome, RewindError> {
    let manifests = store.load_manifests()?; // newest first
    let cutoff = match older_than {
        Some(d) => Some(parse_cutoff(d, now)?),
        None => None,
    };

    // Decide per capture (newest-first index drives keep_last).
    let mut to_remove: Vec<Manifest> = Vec::new();
    let mut kept = 0usize;
    for (i, m) in manifests.iter().enumerate() {
        let over_count = keep_last.is_some_and(|n| i >= n);
        let too_old = match &cutoff {
            Some(c) => &m.captured_at < c,
            None => false,
        };
        if over_count || too_old {
            to_remove.push(m.clone());
        } else {
            kept += 1;
        }
    }

    for m in &to_remove {
        store.delete_manifest(m)?;
    }

    let (objects_removed, bytes_reclaimed) = if gc { sweep(store)? } else { (0, 0) };

    Ok(PruneOutcome {
        removed: to_remove
            .iter()
            .map(|m| PrunedCapture {
                id: m.id.clone(),
                captured_at: m.captured_at.clone(),
            })
            .collect(),
        removed_count: to_remove.len(),
        gc,
        objects_removed,
        bytes_reclaimed,
        remaining_captures: kept,
    })
}

/// Mark-and-sweep: collect every hash referenced by a *surviving* manifest, then
/// delete every stored object not in that set. Run AFTER manifests are deleted,
/// so the live set reflects the post-prune state — a blob shared with a kept
/// capture is in the set and survives.
fn sweep(store: &Store) -> Result<(usize, u64), RewindError> {
    use std::collections::HashSet;
    let live: HashSet<String> = store
        .load_manifests()?
        .iter()
        .flat_map(|m| m.entries.iter().filter_map(|e| e.hash.clone()))
        .collect();

    let mut removed = 0usize;
    let mut freed = 0u64;
    for hash in store.iter_object_hashes() {
        if !live.contains(&hash) {
            freed += store.remove_object(&hash)?;
            removed += 1;
        }
    }
    Ok((removed, freed))
}

/// Parse an `--older-than` duration and return the RFC3339 cutoff timestamp:
/// every capture whose `captured_at` sorts *before* this is too old. Supports a
/// single `<n><unit>` where unit is `s`/`m`/`h`/`d` (seconds/minutes/hours/days).
/// `now` is the reference instant (RFC3339 UTC). A malformed input is an error.
fn parse_cutoff(spec: &str, now: &str) -> Result<String, RewindError> {
    let secs = parse_duration_secs(spec).ok_or_else(|| RewindError::BadDuration {
        spec: spec.to_string(),
    })?;
    let now_dt = chrono::DateTime::parse_from_rfc3339(now)
        .map_err(|_| RewindError::BadDuration {
            spec: format!("internal: bad now timestamp '{now}'"),
        })?
        .to_utc();
    let cutoff = now_dt - chrono::Duration::seconds(secs as i64);
    Ok(cutoff.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Parse `<n><unit>` (e.g. `30d`, `12h`, `90m`, `45s`) into seconds. Returns
/// `None` on any malformed input (no digits, bad unit, overflow).
fn parse_duration_secs(spec: &str) -> Option<u64> {
    let spec = spec.trim();
    let (num, unit) = spec.split_at(spec.find(|c: char| !c.is_ascii_digit())?);
    if num.is_empty() {
        return None;
    }
    let n: u64 = num.parse().ok()?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    n.checked_mul(mult)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CaptureEntry, EntryKind, MANIFEST_SCHEMA_VERSION};
    use std::path::Path;
    use tempfile::tempdir;

    fn store_in(dir: &Path) -> Store {
        let s = Store::open(dir.to_path_buf());
        s.ensure_dirs().unwrap();
        s
    }

    /// Save a manifest referencing one object (written to the store) so gc has a
    /// real blob to consider. `content` keys the object; sharing content across
    /// captures makes them share an object.
    fn save_capture(store: &Store, id: &str, at: &str, content: &[u8]) -> Manifest {
        let hash = store.put_bytes(content).unwrap();
        let m = Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            source_tool: "rewind".into(),
            id: id.into(),
            captured_at: at.into(),
            label: None,
            set_source: "cli".into(),
            entries: vec![CaptureEntry {
                path: format!("/d/{id}.json"),
                kind: EntryKind::File,
                size: Some(content.len() as u64),
                mode: Some("0644".into()),
                uid: Some(1000),
                gid: Some(1000),
                mtime: None,
                hash: Some(hash),
                target: None,
                envelope_tool: None,
                envelope_schema_version: None,
                unreadable: false,
            }],
        };
        store.save_manifest(&m).unwrap();
        m
    }

    #[test]
    fn keep_last_keeps_the_newest_n() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        save_capture(&store, "old1", "2026-06-17T00:00:00Z", b"a");
        save_capture(&store, "mid2", "2026-06-18T00:00:00Z", b"b");
        save_capture(&store, "new3", "2026-06-19T00:00:00Z", b"c");

        let out = run(&store, Some(2), None, false, "2026-06-19T12:00:00Z").unwrap();
        assert_eq!(out.removed_count, 1);
        assert_eq!(out.remaining_captures, 2);
        let remaining: Vec<String> = store
            .load_manifests()
            .unwrap()
            .iter()
            .map(|m| m.id.clone())
            .collect();
        assert!(remaining.contains(&"new3".to_string()));
        assert!(remaining.contains(&"mid2".to_string()));
        assert!(!remaining.contains(&"old1".to_string()));
    }

    #[test]
    fn older_than_removes_by_age_boundary() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        save_capture(&store, "old", "2026-06-01T00:00:00Z", b"a"); // ~18 days old
        save_capture(&store, "new", "2026-06-19T00:00:00Z", b"b"); // fresh

        // Cutoff 10 days before now (2026-06-19T12:00) -> 2026-06-09; "old" is before it.
        let out = run(&store, None, Some("10d"), false, "2026-06-19T12:00:00Z").unwrap();
        assert_eq!(out.removed_count, 1);
        assert_eq!(out.removed[0].id, "old");
        assert_eq!(out.remaining_captures, 1);
    }

    #[test]
    fn bad_older_than_is_an_error() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        save_capture(&store, "a", "2026-06-19T00:00:00Z", b"a");
        for bad in ["", "30", "5y", "abc", "d", "-1d"] {
            let err = run(&store, None, Some(bad), false, "2026-06-19T12:00:00Z").unwrap_err();
            assert!(
                matches!(err, RewindError::BadDuration { .. }),
                "{bad} should error"
            );
        }
    }

    #[test]
    fn gc_deletes_only_unreferenced_objects() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        save_capture(&store, "old", "2026-06-17T00:00:00Z", b"unique-old");
        save_capture(&store, "new", "2026-06-19T00:00:00Z", b"unique-new");
        assert_eq!(store.iter_object_hashes().len(), 2);

        // Prune the old capture + gc: its object is now unreferenced -> removed;
        // the new capture's object stays.
        let out = run(&store, Some(1), None, true, "2026-06-19T12:00:00Z").unwrap();
        assert_eq!(out.removed_count, 1);
        assert_eq!(out.objects_removed, 1);
        assert!(out.bytes_reclaimed > 0);
        assert_eq!(store.iter_object_hashes().len(), 1);
    }

    #[test]
    fn gc_keeps_objects_shared_with_a_surviving_capture() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        // Both captures reference byte-identical content -> ONE shared object.
        save_capture(&store, "old", "2026-06-17T00:00:00Z", b"shared");
        save_capture(&store, "new", "2026-06-19T00:00:00Z", b"shared");
        assert_eq!(store.iter_object_hashes().len(), 1, "dedup -> one object");

        // Prune old + gc: the object is STILL referenced by "new" -> must survive.
        let out = run(&store, Some(1), None, true, "2026-06-19T12:00:00Z").unwrap();
        assert_eq!(out.removed_count, 1);
        assert_eq!(out.objects_removed, 0, "shared object must NOT be deleted");
        assert_eq!(store.iter_object_hashes().len(), 1);
    }

    #[test]
    fn nothing_matched_removes_nothing() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        save_capture(&store, "a", "2026-06-19T00:00:00Z", b"a");
        let out = run(&store, Some(5), None, false, "2026-06-19T12:00:00Z").unwrap();
        assert_eq!(out.removed_count, 0);
        assert_eq!(out.remaining_captures, 1);
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration_secs("45s"), Some(45));
        assert_eq!(parse_duration_secs("90m"), Some(5400));
        assert_eq!(parse_duration_secs("12h"), Some(43200));
        assert_eq!(parse_duration_secs("30d"), Some(2_592_000));
        assert_eq!(parse_duration_secs("0d"), Some(0));
        assert_eq!(parse_duration_secs(""), None);
        assert_eq!(parse_duration_secs("30"), None);
        assert_eq!(parse_duration_secs("5y"), None);
        assert_eq!(parse_duration_secs("abc"), None);
    }
}

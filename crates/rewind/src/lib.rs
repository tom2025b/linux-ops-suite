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
//! **Phase 1** shipped the storage layer plus `capture`, `list` (the timeline),
//! and `sources`. **Phase 2** adds `show` (one capture's manifest), `diff` (two
//! captures, or a capture against the live files), and the `<capture>` selector
//! grammar (`latest` / `latest-good` / `~N` / id / unique prefix) those two
//! share. The guarded restore lands in a later phase. The library does the work
//! and returns values; the binary only parses flags and renders.

pub mod capture;
pub mod diff;
pub mod error;
pub mod hash;
pub mod model;
pub mod report;
pub mod scan;
pub mod set;
pub mod store;
pub mod util;

use std::path::{Path, PathBuf};

use diff::Diff;
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

/// Resolve a capture *selector* against the loaded timeline (newest first), the
/// addressing shared by `show`/`diff`. Accepted forms, checked in this order:
///
/// - `latest` — the newest capture (`manifests[0]`).
/// - `latest-good` — the newest capture whose flagship snapshot is a valid
///   `workstate` envelope ([`Manifest::has_valid_snapshot`]).
/// - `~N` — a relative index from newest (`~0` == latest, `~1` the one before).
/// - a full capture id, or a **unique** id prefix.
///
/// Labels are never selectors — `latest` is always positional, even if a capture
/// happens to be labeled "latest". Every miss (unknown keyword, out-of-range
/// index, no match, or an *ambiguous* prefix matching more than one) maps to
/// [`RewindError::UnknownCapture`], i.e. exit 3 — not a diff/exit-1 outcome.
pub fn resolve_selector<'a>(
    manifests: &'a [Manifest],
    selector: &str,
) -> Result<&'a Manifest, RewindError> {
    let miss = || RewindError::UnknownCapture {
        selector: selector.to_string(),
    };

    if selector == "latest" {
        return manifests.first().ok_or_else(miss);
    }
    if selector == "latest-good" {
        return manifests
            .iter()
            .find(|m| m.has_valid_snapshot())
            .ok_or_else(miss);
    }
    if let Some(rest) = selector.strip_prefix('~') {
        // `~N`: a base-10 index from newest. Require digits-only (Rust's
        // `usize::parse` would otherwise accept a leading `+`), so `~+1`, `~ 1`,
        // and `~x` all miss; overflow misses too. `~0` == latest.
        if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return Err(miss());
        }
        let n: usize = rest.parse().map_err(|_| miss())?;
        return manifests.get(n).ok_or_else(miss);
    }

    // Exact id wins over a prefix; then require a *unique* prefix match.
    if let Some(m) = manifests.iter().find(|m| m.id == selector) {
        return Ok(m);
    }
    let mut hits = manifests.iter().filter(|m| m.id.starts_with(selector));
    match (hits.next(), hits.next()) {
        (Some(m), None) => Ok(m),
        _ => Err(miss()), // zero matches, or ambiguous (>= 2)
    }
}

/// Load the timeline and resolve one selector against it — the shared prologue of
/// `show_capture`/`diff_captures`/`diff_capture_vs_live`. Returns the matched
/// manifest (cloned, so the caller owns it) alongside the whole timeline (so a
/// caller resolving two selectors reuses one load) and the store dir.
fn load_selected(
    store_override: Option<PathBuf>,
    selector: &str,
) -> Result<(Manifest, Vec<Manifest>, PathBuf), RewindError> {
    let store_dir = resolve_store_dir(store_override)?;
    let store = Store::open(store_dir.clone());
    if !store.exists() {
        return Err(RewindError::NoStore { path: store_dir });
    }
    let manifests = store.load_manifests()?;
    let selected = resolve_selector(&manifests, selector)?.clone();
    Ok((selected, manifests, store_dir))
}

/// Load one capture's full manifest by selector, for `rewind show`.
pub fn show_capture(
    store_override: Option<PathBuf>,
    selector: &str,
) -> Result<Manifest, RewindError> {
    let (manifest, _, _) = load_selected(store_override, selector)?;
    Ok(manifest)
}

/// Diff capture `a` against capture `b` (both selectors). Read-only — it compares
/// the manifests' recorded hashes, never reading a blob. Resolves both selectors
/// against a single timeline load.
pub fn diff_captures(
    store_override: Option<PathBuf>,
    a: &str,
    b: &str,
) -> Result<Diff, RewindError> {
    let (from, manifests, _) = load_selected(store_override, a)?;
    let to = resolve_selector(&manifests, b)?;
    Ok(diff::diff_entries(
        &from.entries,
        &to.entries,
        &short(&from.id),
        &short(&to.id),
    ))
}

/// Diff capture `a` against the current **live** files. Re-resolves the capture
/// set and re-walks the live filesystem read-only (no store write), so a file
/// that appeared or vanished under a captured directory shows as added/removed —
/// the honest "has the live state drifted from this pin?" answer. The live set
/// is reconstructed from the current resolution (`--path`/`--config`/builtin);
/// for the common builtin/config capture this matches what was recorded.
///
/// Guard: a capture taken from an explicit `--path` set (`set_source == "cli"`)
/// can only be reconstructed when this run *also* supplies `--path`/`--config`.
/// Without them the live side would silently be the builtin/config set — a diff
/// against the wrong files — so it is refused with [`RewindError::SetMismatch`].
pub fn diff_capture_vs_live(
    store_override: Option<PathBuf>,
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
    a: &str,
) -> Result<Diff, RewindError> {
    let (from, _, _) = load_selected(store_override, a)?;
    let set = set::resolve(cli_paths, config_override)?;

    // The capture was a --path set, but this run can't reconstruct one -> refuse
    // rather than diff against a different (builtin/config) live set.
    if from.set_source == set::SetSource::Cli.tag() && set.source != set::SetSource::Cli {
        return Err(RewindError::SetMismatch {
            selector: a.to_string(),
        });
    }

    let live = scan::live_scan(&set);
    Ok(diff::diff_entries(
        &from.entries,
        &live,
        &short(&from.id),
        "live",
    ))
}

/// A capture id's short prefix, the form the timeline and diff headers show.
fn short(id: &str) -> String {
    id.chars().take(8).collect()
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

    // ---- Phase 2: selectors -----------------------------------------------

    use model::{CaptureEntry, EntryKind, Manifest, MANIFEST_SCHEMA_VERSION};

    /// A manifest with a chosen id/time and an optional valid-snapshot entry.
    fn man(id: &str, at: &str, good_snapshot: bool) -> Manifest {
        let entry = CaptureEntry {
            path: "/d/workstate.snapshot.json".into(),
            kind: EntryKind::File,
            size: Some(8),
            mode: Some("0644".into()),
            uid: Some(1000),
            gid: Some(1000),
            mtime: None,
            hash: Some(format!("hash-{id}")),
            target: None,
            envelope_tool: good_snapshot.then(|| "workstate".to_string()),
            envelope_schema_version: good_snapshot.then_some(4),
            unreadable: false,
        };
        Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            source_tool: "rewind".into(),
            id: id.into(),
            captured_at: at.into(),
            label: None,
            set_source: "builtin".into(),
            entries: vec![entry],
        }
    }

    /// Three manifests, newest first (the order `load_manifests` returns).
    fn timeline() -> Vec<Manifest> {
        vec![
            man("3f9c1111", "2026-06-19T14:00:00Z", false), // newest, snapshot invalid
            man("a17b2222", "2026-06-18T02:00:00Z", true),  // good
            man("c0de3333", "2026-06-17T02:00:00Z", true),  // good
        ]
    }

    #[test]
    fn selector_latest_is_newest() {
        let ms = timeline();
        assert_eq!(resolve_selector(&ms, "latest").unwrap().id, "3f9c1111");
        // ~0 == latest == the full id of newest, all converge.
        assert_eq!(resolve_selector(&ms, "~0").unwrap().id, "3f9c1111");
        assert_eq!(resolve_selector(&ms, "3f9c1111").unwrap().id, "3f9c1111");
    }

    #[test]
    fn selector_latest_good_skips_invalid() {
        let ms = timeline();
        // Newest is snapshot-invalid, so latest-good is the next one.
        assert_eq!(resolve_selector(&ms, "latest-good").unwrap().id, "a17b2222");
    }

    #[test]
    fn selector_latest_good_misses_when_none_good() {
        let ms = vec![man("aaa", "2026-06-19T00:00:00Z", false)];
        let err = resolve_selector(&ms, "latest-good").unwrap_err();
        assert!(matches!(err, RewindError::UnknownCapture { .. }));
    }

    #[test]
    fn selector_relative_index() {
        let ms = timeline();
        assert_eq!(resolve_selector(&ms, "~1").unwrap().id, "a17b2222");
        assert_eq!(resolve_selector(&ms, "~2").unwrap().id, "c0de3333");
        // Out of range -> miss.
        assert!(resolve_selector(&ms, "~99").is_err());
    }

    #[test]
    fn selector_bad_relative_index_forms_miss() {
        let ms = timeline();
        for bad in ["~", "~x", "~-1", "~+1", "~ 1"] {
            assert!(
                resolve_selector(&ms, bad).is_err(),
                "{bad} should not resolve"
            );
        }
    }

    #[test]
    fn selector_unique_prefix_and_ambiguous_and_miss() {
        let ms = timeline();
        // Unique prefix.
        assert_eq!(resolve_selector(&ms, "a17b").unwrap().id, "a17b2222");
        // No match.
        assert!(resolve_selector(&ms, "zzzz").is_err());
        // Ambiguous prefix across two ids.
        let amb = vec![
            man("ab11", "2026-06-19T00:00:00Z", false),
            man("ab22", "2026-06-18T00:00:00Z", false),
        ];
        let err = resolve_selector(&amb, "ab").unwrap_err();
        assert!(matches!(err, RewindError::UnknownCapture { .. }));
    }

    #[test]
    fn selector_exact_id_beats_prefix_collision() {
        // An id that is also a prefix of another id resolves to itself exactly.
        let ms = vec![
            man("abcd", "2026-06-19T00:00:00Z", false),
            man("abcdef", "2026-06-18T00:00:00Z", false),
        ];
        assert_eq!(resolve_selector(&ms, "abcd").unwrap().id, "abcd");
    }

    #[test]
    fn selector_empty_timeline_always_misses() {
        let ms: Vec<Manifest> = vec![];
        for sel in ["latest", "latest-good", "~0", "deadbeef"] {
            assert!(resolve_selector(&ms, sel).is_err());
        }
    }

    #[test]
    fn selector_label_named_latest_is_not_a_selector() {
        // A capture labeled "latest" that is NOT newest must not hijack the
        // keyword — `latest` is positional.
        let mut ms = timeline();
        ms[1].label = Some("latest".into());
        assert_eq!(resolve_selector(&ms, "latest").unwrap().id, "3f9c1111");
    }

    // ---- Phase 2: show / diff roundtrips through the store -----------------

    fn write_snap(dir: &std::path::Path, name: &str, body: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn show_capture_by_id_and_latest() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = write_snap(
            dir.path(),
            "workstate.snapshot.json",
            br#"{"schema_version":4,"source_tool":"workstate"}"#,
        );
        let m = record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T14:22:05Z",
            Some("pin"),
        )
        .unwrap();

        let by_latest = show_capture(Some(store.clone()), "latest").unwrap();
        let by_id = show_capture(Some(store), &m.id).unwrap();
        assert_eq!(by_latest.id, m.id);
        assert_eq!(by_id.id, m.id);
        assert_eq!(by_id.label.as_deref(), Some("pin"));
    }

    #[test]
    fn show_capture_unknown_selector_is_error_not_diff() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = write_snap(dir.path(), "a.json", b"{}");
        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();
        let err = show_capture(Some(store), "nope").unwrap_err();
        assert!(matches!(err, RewindError::UnknownCapture { .. }));
    }

    #[test]
    fn diff_captures_clean_and_dirty() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = write_snap(dir.path(), "snap.json", b"version-one");

        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T01:00:00Z",
            None,
        )
        .unwrap();
        // Identical second capture (different time) -> clean diff.
        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T02:00:00Z",
            None,
        )
        .unwrap();
        let clean = diff_captures(Some(store.clone()), "~1", "latest").unwrap();
        assert!(clean.is_clean(), "identical content -> clean");
        assert_eq!(clean.unchanged, 1);

        // Change the file, capture again -> dirty diff vs the previous.
        std::fs::write(&f, b"version-two-longer").unwrap();
        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T03:00:00Z",
            None,
        )
        .unwrap();
        let dirty = diff_captures(Some(store), "~1", "latest").unwrap();
        assert!(!dirty.is_clean());
        assert_eq!(dirty.changed, 1);
        // Capture-vs-capture labels both sides with a short id, never "live".
        assert_ne!(dirty.from, "live");
        assert_ne!(dirty.to, "live");
    }

    #[test]
    fn diff_capture_vs_live_detects_edit_and_writes_nothing() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = write_snap(dir.path(), "live.json", b"original");

        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();
        let bytes_before = store_stats(Some(store.clone())).unwrap().0;

        // Edit the live file, then diff the capture against live.
        std::fs::write(&f, b"changed-on-disk").unwrap();
        let d = diff_capture_vs_live(
            Some(store.clone()),
            std::slice::from_ref(&f),
            None,
            "latest",
        )
        .unwrap();
        assert!(!d.is_clean());
        assert_eq!(d.changed, 1);
        assert_eq!(d.to, "live");

        // The read-only guarantee: a diff-vs-live wrote no new object bytes.
        let bytes_after = store_stats(Some(store)).unwrap().0;
        assert_eq!(
            bytes_before, bytes_after,
            "diff-vs-live must not write to the store"
        );
    }

    #[test]
    fn diff_capture_vs_live_detects_new_file_under_captured_dir() {
        // The re-walk semantics: a NEW file appearing in a captured directory is
        // drift, reported as added (the cron "did anything change?" check).
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let feeds = dir.path().join("feeds");
        std::fs::create_dir(&feeds).unwrap();
        std::fs::write(feeds.join("a.json"), b"a").unwrap();

        record_capture(
            std::slice::from_ref(&feeds),
            None,
            Some(store.clone()),
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();

        // A new feed lands after the capture.
        std::fs::write(feeds.join("b.json"), b"b").unwrap();
        let d = diff_capture_vs_live(Some(store), std::slice::from_ref(&feeds), None, "latest")
            .unwrap();
        assert!(!d.is_clean(), "a new file under a captured dir is drift");
        assert_eq!(d.added, 1);
        assert!(d
            .changes
            .iter()
            .any(|c| c.path.ends_with("b.json") && c.kind == diff::ChangeKind::Added));
    }

    #[test]
    fn diff_vs_live_refuses_cli_capture_without_path_args() {
        // A capture taken with --path, diffed with NO --path, must refuse rather
        // than silently compare against the builtin/config set.
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = write_snap(dir.path(), "thing.json", b"x");
        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();

        // No cli_paths -> would reconstruct a different (builtin) set -> refused.
        let err = diff_capture_vs_live(Some(store.clone()), &[], None, "latest").unwrap_err();
        assert!(matches!(err, RewindError::SetMismatch { .. }));

        // Supplying the same --path makes it work.
        let ok = diff_capture_vs_live(Some(store), std::slice::from_ref(&f), None, "latest");
        assert!(ok.is_ok());
    }

    #[test]
    fn diff_captures_self_is_clean() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("store");
        let f = write_snap(dir.path(), "s.json", b"data");
        record_capture(
            std::slice::from_ref(&f),
            None,
            Some(store.clone()),
            "2026-06-19T00:00:00Z",
            None,
        )
        .unwrap();
        let d = diff_captures(Some(store), "latest", "latest").unwrap();
        assert!(d.is_clean(), "a capture vs itself is always clean");
    }

    #[test]
    fn selector_leading_zero_index_equals_plain_index() {
        let ms = timeline();
        assert_eq!(
            resolve_selector(&ms, "~01").unwrap().id,
            resolve_selector(&ms, "~1").unwrap().id,
            "~01 is base-10, same as ~1"
        );
    }
}

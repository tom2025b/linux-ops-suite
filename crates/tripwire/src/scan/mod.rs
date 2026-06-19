//! Discovery. [`scan`] is the one entry point: take a resolved watch set, expand
//! each watch entry to the paths it covers ([`walk`]), and turn each path into an
//! [`Entry`] with its metadata and (for readable files) content hash ([`meta`]).
//!
//! Everything here is best-effort, the way portman's chain resolution is: a path
//! that vanished between listing and stat is skipped, a file we can't read
//! becomes an `unreadable` entry rather than an error, and a non-root run simply
//! produces thinner entries for files it can't open. The scan itself never
//! fails — only resolving the watch set (a bad explicit `--config`) can.

pub mod meta;
pub mod walk;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::TripwireError;
use crate::model::{Entry, EntryKind};
use crate::watch::{self, WatchEntry, WatchSet};

/// The result of a scan: the entries (sorted by path, deduped) plus the source
/// the watch set came from, so the renderers can report what was covered.
pub struct Scan {
    pub entries: Vec<Entry>,
    pub source: watch::WatchSource,
}

/// Resolve the watch set from CLI/config/builtin, then scan it. `ignore` names a
/// path to omit from the result — the caller passes tripwire's own baseline file
/// so it never registers as drift when it happens to sit inside a watched dir.
pub fn scan(
    cli_paths: &[std::path::PathBuf],
    config_override: Option<&Path>,
    ignore: Option<&Path>,
) -> Result<Scan, TripwireError> {
    let set = watch::resolve(cli_paths, config_override)?;
    Ok(scan_set(&set, ignore))
}

/// Scan an already-resolved watch set. Pulled out so it can be unit-tested
/// against a temp tree without touching the real config/builtin resolution.
/// `ignore`, when set, drops that one path from the output (see [`scan`]).
pub fn scan_set(set: &WatchSet, ignore: Option<&Path>) -> Scan {
    // Compare against the canonical form of the ignored path so it matches
    // however it's reached during the walk (relative vs absolute, symlinks).
    let ignore_canon = ignore.map(canonical_or_self);

    // A path may be reached by more than one watch entry; the last writer with
    // the most specific options wins, but we key by path so each appears once.
    let mut by_path: BTreeMap<String, Entry> = BTreeMap::new();

    for watch_entry in &set.entries {
        for path in walk::collect(watch_entry) {
            if let Some(ig) = &ignore_canon {
                if &canonical_or_self(&path) == ig {
                    continue;
                }
            }
            if let Some(entry) = entry_for(&path, watch_entry) {
                by_path.insert(entry.path.clone(), entry);
            }
        }
    }

    Scan {
        entries: by_path.into_values().collect(),
        source: set.source,
    }
}

/// Canonicalize a path, falling back to the path itself when it doesn't exist
/// (the baseline-to-ignore may not have been written yet on the first run).
fn canonical_or_self(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Build one [`Entry`] for a concrete path under the given watch options.
/// Returns `None` only when the path doesn't exist at all (it was listed but
/// vanished, or a configured path that isn't present) — those are simply absent
/// from the view, and a diff reports a baselined-but-absent path as `removed`.
fn entry_for(path: &Path, opts: &WatchEntry) -> Option<Entry> {
    // lstat first so a symlink is described as itself. If even that fails the
    // path is gone/inaccessible at the directory level — skip it.
    let lmd = fs::symlink_metadata(path).ok()?;
    let m = meta::Meta::from_metadata(&lmd);

    let path_str = path.to_string_lossy().into_owned();
    let mut entry = Entry {
        path: path_str,
        kind: m.kind,
        size: m.size,
        mode: Some(m.mode),
        uid: Some(m.uid),
        gid: Some(m.gid),
        mtime: m.mtime,
        hash: None,
        target: None,
        unreadable: false,
    };

    match m.kind {
        EntryKind::Symlink => {
            entry.target = meta::read_link_target(path);
        }
        EntryKind::File if opts.content => {
            match meta::hash_file(path) {
                Ok(h) => entry.hash = Some(h),
                // Exists but unreadable (e.g. /etc/shadow as non-root): record
                // metadata, flag unreadable, omit the hash. Never an error.
                Err(_) => entry.unreadable = true,
            }
        }
        _ => {}
    }

    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::{WatchSet, WatchSource};
    use std::fs;
    use tempfile::tempdir;

    fn set_of(entries: Vec<WatchEntry>) -> WatchSet {
        WatchSet {
            entries,
            source: WatchSource::Cli,
        }
    }

    #[test]
    fn scans_a_file_with_hash_and_metadata() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, b"abc").unwrap();

        let scan = scan_set(&set_of(vec![WatchEntry::new(f.clone())]), None);
        assert_eq!(scan.entries.len(), 1);
        let e = &scan.entries[0];
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(e.size, Some(3));
        assert_eq!(
            e.hash.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert!(!e.unreadable);
    }

    #[test]
    fn content_false_records_metadata_without_hash() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("big.log");
        fs::write(&f, b"loads of data").unwrap();
        let mut we = WatchEntry::new(f.clone());
        we.content = false;

        let scan = scan_set(&set_of(vec![we]), None);
        let e = &scan.entries[0];
        assert!(e.hash.is_none());
        assert_eq!(e.size, Some(13));
    }

    #[test]
    fn missing_configured_path_is_simply_absent() {
        let dir = tempdir().unwrap();
        let scan = scan_set(
            &set_of(vec![WatchEntry::new(dir.path().join("nope"))]),
            None,
        );
        assert!(scan.entries.is_empty());
    }

    #[test]
    fn symlink_records_target_and_is_not_hashed() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("real");
        fs::write(&target, b"data").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let scan = scan_set(&set_of(vec![WatchEntry::new(link.clone())]), None);
        let e = &scan.entries[0];
        assert_eq!(e.kind, EntryKind::Symlink);
        assert!(e.hash.is_none());
        assert_eq!(e.target.as_deref(), target.to_str());
    }

    #[test]
    fn ignore_drops_the_baseline_path_from_a_watched_dir() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep.txt"), b"k").unwrap();
        let baseline = dir.path().join("baseline.json");
        fs::write(&baseline, b"{}").unwrap();

        // Watching the dir would normally pick up baseline.json; ignore drops it
        // so tripwire's own file never registers as drift.
        let set = set_of(vec![WatchEntry::new(dir.path().to_path_buf())]);
        let scan = scan_set(&set, Some(&baseline));
        assert!(scan.entries.iter().any(|e| e.path.ends_with("keep.txt")));
        assert!(
            !scan
                .entries
                .iter()
                .any(|e| e.path.ends_with("baseline.json")),
            "the ignored baseline path must not appear"
        );
    }

    #[test]
    fn entries_are_sorted_and_deduped_by_path() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), b"b").unwrap();
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        // Two overlapping watch entries cover the same dir; each path once.
        let set = set_of(vec![
            WatchEntry::new(dir.path().to_path_buf()),
            WatchEntry::new(dir.path().to_path_buf()),
        ]);
        let scan = scan_set(&set, None);
        let paths: Vec<&str> = scan.entries.iter().map(|e| e.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted, "entries must be path-sorted");
        // No duplicates.
        let unique: std::collections::BTreeSet<_> = paths.iter().collect();
        assert_eq!(unique.len(), paths.len());
    }
}

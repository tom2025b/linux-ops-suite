//! Discovery + storage of a capture set. [`scan_into`] is the one entry point:
//! take a resolved capture set, expand each spec to the paths it covers
//! ([`walk`]), and for each path build a [`CaptureEntry`] with its metadata — and
//! for a readable file, stream its bytes into the content-addressed store and
//! record the resulting object hash on the entry.
//!
//! Everything here is best-effort, like tripwire's scan: a path that vanished
//! between listing and stat is skipped, a file we can't read becomes an
//! `unreadable` entry (no blob) rather than an error, and a non-root run simply
//! produces thinner entries. The scan never fails — only resolving the capture
//! set (a bad explicit `--config`) or writing to the store can.

pub mod meta;
pub mod walk;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::error::RewindError;
use crate::model::{CaptureEntry, EntryKind};
use crate::set::{CaptureSet, CaptureSpec, SetSource};
use crate::store::Store;

/// The result of scanning a capture set into the store: the captured entries
/// (sorted by path, deduped) plus the source the set came from.
pub struct Scanned {
    pub entries: Vec<CaptureEntry>,
    pub source: SetSource,
}

/// Scan a resolved capture set, writing each readable file's content into
/// `store` and returning the entries. `store` must already have its dirs
/// (`ensure_dirs`) — the caller does that once before capturing.
pub fn scan_into(set: &CaptureSet, store: &Store) -> Result<Scanned, RewindError> {
    scan_set(set, Some(store))
}

/// Scan a resolved capture set **read-only**: build a [`CaptureEntry`] for every
/// covered path exactly as [`scan_into`] would, but without persisting any blob
/// to a store. Used by capture-vs-live diff, which must never mutate the store.
/// File hashes are computed in memory (the same SHA-256, so they match a real
/// capture's object keys) and envelopes are sniffed from the same buffer.
pub fn live_scan(set: &CaptureSet) -> Vec<CaptureEntry> {
    // No store means no write can fail, so this never errors — only resolving the
    // set (a bad explicit `--config`) can, and the caller did that already.
    scan_set(set, None)
        .map(|s| s.entries)
        .unwrap_or_else(|_| Vec::new())
}

/// Shared core of [`scan_into`]/[`live_scan`]: walk every spec and build an entry
/// per covered path, deduped by absolute path. `store` decides where a readable
/// file's bytes go — `Some(store)` persists the blob (capture), `None` hashes in
/// memory only (read-only diff). The two modes produce identical entries.
fn scan_set(set: &CaptureSet, store: Option<&Store>) -> Result<Scanned, RewindError> {
    // A path may be reached by more than one spec; key by absolute path so each
    // appears once. Last writer wins, which is fine — same path, same content.
    let mut by_path: BTreeMap<String, CaptureEntry> = BTreeMap::new();

    for spec in &set.specs {
        for path in walk::collect(spec) {
            if let Some(entry) = entry_for(&path, spec, store)? {
                by_path.insert(entry.path.clone(), entry);
            }
        }
    }

    Ok(Scanned {
        entries: by_path.into_values().collect(),
        source: set.source,
    })
}

/// Absolute, lexical path used as the stable entry identity. Does not follow the
/// final symlink, so identity stays the captured path even when `follow_symlinks`
/// asks metadata/content to come from the target.
fn absolute_lexical(p: &Path) -> PathBuf {
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    };
    normalize_lexical(joined)
}

fn normalize_lexical(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(out.components().next_back(), Some(Component::RootDir)) {
                    out.pop();
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

/// Build one [`CaptureEntry`] for a concrete path. For a readable file, hash its
/// bytes — persisting the blob into the object pool when `store` is `Some` (a
/// real capture), or hashing in memory only when `None` (a read-only diff). The
/// resulting entry is identical either way. Returns `None` only when the path
/// doesn't exist at all (listed but vanished, or a configured path not present).
fn entry_for(
    path: &Path,
    spec: &CaptureSpec,
    store: Option<&Store>,
) -> Result<Option<CaptureEntry>, RewindError> {
    let md = if spec.follow_symlinks {
        std::fs::metadata(path).ok()
    } else {
        std::fs::symlink_metadata(path).ok()
    };
    let md = match md {
        Some(m) => m,
        None => return Ok(None),
    };
    let m = meta::Meta::from_metadata(&md);

    let path_str = absolute_lexical(path).to_string_lossy().into_owned();
    let mut entry = CaptureEntry {
        path: path_str,
        kind: m.kind,
        size: m.size,
        mode: Some(m.mode),
        uid: Some(m.uid),
        gid: Some(m.gid),
        mtime: m.mtime,
        hash: None,
        target: None,
        envelope_tool: None,
        envelope_schema_version: None,
        unreadable: false,
    };

    match m.kind {
        EntryKind::Symlink => {
            entry.target = meta::read_link_target(path);
        }
        EntryKind::File => {
            // The capture set is small suite JSON by design, but a misconfigured
            // path could point at a huge file. Reading the whole thing into one
            // Vec to hash would let that OOM the process mid-capture. Guard it:
            // anything past the cap is recorded as unreadable (with size noted by
            // the entry's own `size`) rather than slurped into memory. We still
            // read the whole file below — within the cap that is bounded and lets
            // us both hash and sniff the envelope from the same bytes.
            const MAX_CAPTURE_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB
            if m.size.is_some_and(|sz| sz > MAX_CAPTURE_BYTES) {
                entry.unreadable = true;
                return Ok(Some(entry));
            }
            // Read the whole file once: we need the bytes both to hash (store or
            // in-memory) and to sniff the envelope. If reading fails it's an
            // unreadable entry.
            match std::fs::read(path) {
                Ok(bytes) => {
                    // Persist into the object pool for a real capture; hash in
                    // memory only for a read-only diff. Same SHA-256 either way,
                    // so a live entry's hash matches the captured object key.
                    let hash = match store {
                        Some(s) => s.put_bytes(&bytes)?,
                        None => crate::hash::hex_of(&bytes),
                    };
                    entry.hash = Some(hash);
                    if let Some((tool, ver)) = sniff_envelope(&bytes) {
                        entry.envelope_tool = Some(tool);
                        entry.envelope_schema_version = ver;
                    }
                }
                Err(_) => entry.unreadable = true,
            }
        }
        _ => {}
    }

    Ok(Some(entry))
}

/// Cheaply sniff a blob for the suite's universal envelope discriminators —
/// `source_tool` (string) and `schema_version` (integer). Returns the tool name
/// and optional version when the blob is JSON carrying a `source_tool`. Anything
/// that isn't a JSON object with a string `source_tool` yields `None` (not an
/// envelope), which is how a captured non-suite file or invalid snapshot is told
/// apart from a valid one.
fn sniff_envelope(bytes: &[u8]) -> Option<(String, Option<u32>)> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let obj = value.as_object()?;
    let tool = obj.get("source_tool")?.as_str()?.to_string();
    let ver = obj
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok());
    Some((tool, ver))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn set_of(specs: Vec<CaptureSpec>) -> CaptureSet {
        CaptureSet {
            specs,
            source: SetSource::Cli,
        }
    }

    fn store_in(dir: &Path) -> Store {
        let s = Store::open(dir.to_path_buf());
        s.ensure_dirs().unwrap();
        s
    }

    #[test]
    fn captures_a_file_with_hash_metadata_and_stores_the_blob() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("a.txt");
        fs::write(&f, b"abc").unwrap();

        let scanned = scan_into(&set_of(vec![CaptureSpec::new(f.clone())]), &store).unwrap();
        assert_eq!(scanned.entries.len(), 1);
        let e = &scanned.entries[0];
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(e.size, Some(3));
        let hash = e.hash.as_deref().unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // The blob is actually in the store and round-trips.
        assert_eq!(store.read_object(hash).unwrap(), b"abc");
        assert!(e.envelope_tool.is_none()); // "abc" is not an envelope
    }

    #[test]
    fn sniffs_a_suite_envelope() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("workstate.snapshot.json");
        fs::write(
            &f,
            br#"{"schema_version":4,"source_tool":"workstate","x":1}"#,
        )
        .unwrap();

        let scanned = scan_into(&set_of(vec![CaptureSpec::new(f)]), &store).unwrap();
        let e = &scanned.entries[0];
        assert_eq!(e.envelope_tool.as_deref(), Some("workstate"));
        assert_eq!(e.envelope_schema_version, Some(4));
    }

    #[test]
    fn oversize_file_is_marked_unreadable_not_slurped() {
        // M6 regression: a path larger than the in-memory cap must be recorded
        // as unreadable (size still noted) rather than read whole into a Vec,
        // which would risk OOM. A sparse file via set_len gives us the size
        // without writing 64 MiB of real bytes.
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("huge.bin");
        let fh = fs::File::create(&f).unwrap();
        fh.set_len(64 * 1024 * 1024 + 1).unwrap(); // one byte over the cap
        drop(fh);

        let scanned = scan_into(&set_of(vec![CaptureSpec::new(f)]), &store).unwrap();
        let e = &scanned.entries[0];
        assert_eq!(e.kind, EntryKind::File);
        assert!(e.unreadable, "oversize file must be flagged unreadable");
        assert!(e.hash.is_none(), "oversize file must not be hashed/stored");
        assert_eq!(e.size, Some(64 * 1024 * 1024 + 1)); // size is still reported
    }

    #[test]
    fn invalid_snapshot_has_no_envelope_fields() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("workstate.snapshot.json");
        fs::write(&f, b"not json at all").unwrap();

        let scanned = scan_into(&set_of(vec![CaptureSpec::new(f)]), &store).unwrap();
        let e = &scanned.entries[0];
        assert!(e.hash.is_some()); // still captured (bytes stored)
        assert!(e.envelope_tool.is_none()); // but not a valid envelope
    }

    #[test]
    fn missing_path_is_simply_absent() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let scanned = scan_into(
            &set_of(vec![CaptureSpec::new(dir.path().join("nope"))]),
            &store,
        )
        .unwrap();
        assert!(scanned.entries.is_empty());
    }

    #[test]
    fn symlink_records_target_and_is_not_stored() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let target = dir.path().join("real");
        fs::write(&target, b"data").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let scanned = scan_into(&set_of(vec![CaptureSpec::new(link)]), &store).unwrap();
        let e = &scanned.entries[0];
        assert_eq!(e.kind, EntryKind::Symlink);
        assert!(e.hash.is_none());
        assert_eq!(e.target.as_deref(), target.to_str());
    }

    #[test]
    fn two_identical_files_share_one_object() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"same").unwrap();
        fs::write(&b, b"same").unwrap();

        let scanned = scan_into(
            &set_of(vec![CaptureSpec::new(a), CaptureSpec::new(b)]),
            &store,
        )
        .unwrap();
        assert_eq!(scanned.entries.len(), 2);
        assert_eq!(scanned.entries[0].hash, scanned.entries[1].hash);
    }

    #[test]
    fn entries_are_sorted_by_path() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        fs::write(dir.path().join("b.txt"), b"b").unwrap();
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        let scanned = scan_into(
            &set_of(vec![CaptureSpec::new(dir.path().to_path_buf())]),
            &store,
        )
        .unwrap();
        let paths: Vec<&str> = scanned.entries.iter().map(|e| e.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted);
    }

    #[test]
    fn live_scan_writes_nothing_to_any_store() {
        // A read-only live scan must not create the object pool or any blob.
        let dir = tempdir().unwrap();
        let f = dir.path().join("workstate.snapshot.json");
        fs::write(&f, br#"{"schema_version":4,"source_tool":"workstate"}"#).unwrap();

        // No store is opened or passed at all; just confirm we get entries and
        // that nothing object-shaped was written under the data dir.
        let entries = live_scan(&set_of(vec![CaptureSpec::new(f)]));
        assert_eq!(entries.len(), 1);
        assert!(entries[0].hash.is_some());
        // The temp dir holds only the file we wrote — no `objects/` appeared.
        assert!(!dir.path().join("objects").exists());
    }

    #[test]
    fn live_scan_matches_capture_hash_and_envelope() {
        // The in-memory hash AND sniffed envelope must equal what a real capture
        // records, so capture-vs-live diffs compute true content identity.
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("workstate.snapshot.json");
        fs::write(
            &f,
            br#"{"schema_version":4,"source_tool":"workstate","x":1}"#,
        )
        .unwrap();
        let spec = CaptureSpec::new(f);

        let captured = scan_into(&set_of(vec![spec.clone()]), &store).unwrap();
        let live = live_scan(&set_of(vec![spec]));
        assert_eq!(captured.entries.len(), 1);
        assert_eq!(live.len(), 1);

        let (c, l) = (&captured.entries[0], &live[0]);
        assert_eq!(c.hash, l.hash, "same SHA-256");
        assert_eq!(c.envelope_tool, l.envelope_tool, "same sniffed tool");
        assert_eq!(
            c.envelope_schema_version, l.envelope_schema_version,
            "same sniffed schema"
        );
        assert_eq!(c.size, l.size);
        assert_eq!(c.kind, l.kind);
    }

    #[test]
    fn live_scan_marks_unreadable_when_content_cannot_be_read() {
        // A path present as metadata but unreadable as content -> unreadable, no
        // hash — honest per-path data, never an error (parity with capture).
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let f = dir.path().join("secret.json");
        fs::write(&f, b"{}").unwrap();
        // Drop all read permission. (Skipped effectively when run as root, which
        // can read regardless — then this just asserts the readable path.)
        fs::set_permissions(&f, fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable_to_us = fs::read(&f).is_err();

        let live = live_scan(&set_of(vec![CaptureSpec::new(f.clone())]));
        assert_eq!(live.len(), 1);
        if unreadable_to_us {
            assert!(live[0].unreadable);
            assert!(live[0].hash.is_none());
        }
        // Restore perms so the tempdir cleans up.
        let _ = fs::set_permissions(&f, fs::Permissions::from_mode(0o644));
    }
}

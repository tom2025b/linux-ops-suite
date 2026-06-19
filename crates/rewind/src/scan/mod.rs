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

/// Build one [`CaptureEntry`] for a concrete path, storing its content in the
/// object pool when it's a readable file. Returns `None` only when the path
/// doesn't exist at all (listed but vanished, or a configured path not present).
fn entry_for(
    path: &Path,
    spec: &CaptureSpec,
    store: &Store,
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
            // Read the whole file once: we need the bytes both to store the
            // object and to sniff the envelope. Files in the capture set are
            // small suite JSON; if reading fails it's an unreadable entry.
            match std::fs::read(path) {
                Ok(bytes) => {
                    let hash = store.put_bytes(&bytes)?;
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
        .map(|v| v as u32);
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
}

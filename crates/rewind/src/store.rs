//! The on-disk store: a content-addressed object pool plus per-capture
//! manifests, under the suite's XDG data root.
//!
//! ```text
//! <root>/
//!   objects/<aa>/<full-sha256>   deduped file blobs (one per unique content)
//!   captures/<ts>-<idprefix>.json one manifest per capture (the index entry)
//!   HEAD                          id of the most recent capture
//! ```
//!
//! Two captures of byte-identical content reference the *same* object, so a
//! daily capture of an unchanged snapshot costs one small manifest, not a copy.
//! Writes go through a temp file + atomic rename, so a crash leaves the store
//! consistent (at worst an orphan object, reclaimed by a later gc). No git, no
//! packs, no compression — the suite's lean rule.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::RewindError;
use crate::hash::Sha256;
use crate::model::{Manifest, MANIFEST_SCHEMA_VERSION};

/// A handle to a rewind store rooted at one directory.
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Open (do not create) a store at `root`. Used by read commands; the caller
    /// checks [`exists`](Self::exists) to distinguish "nothing captured yet."
    pub fn open(root: PathBuf) -> Self {
        Store { root }
    }

    /// The store root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether the store has been initialized (its `captures/` dir exists).
    pub fn exists(&self) -> bool {
        self.captures_dir().is_dir()
    }

    fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    fn captures_dir(&self) -> PathBuf {
        self.root.join("captures")
    }

    fn head_path(&self) -> PathBuf {
        self.root.join("HEAD")
    }

    /// Ensure the store's directory skeleton exists (idempotent). Called before
    /// the first write of a capture.
    pub fn ensure_dirs(&self) -> Result<(), RewindError> {
        for dir in [self.objects_dir(), self.captures_dir()] {
            fs::create_dir_all(&dir).map_err(|e| RewindError::SaveFailed {
                path: dir.clone(),
                source: e,
            })?;
        }
        Ok(())
    }

    /// The object path for a given content hash: `objects/<aa>/<hash>`.
    fn object_path(&self, hash: &str) -> PathBuf {
        let shard = &hash[..2.min(hash.len())];
        self.objects_dir().join(shard).join(hash)
    }

    /// Whether an object with this hash is already stored.
    pub fn has_object(&self, hash: &str) -> bool {
        self.object_path(hash).is_file()
    }

    /// Read an object's bytes back by hash. Used by `show --content`/`restore`
    /// in later phases; included now so the store is complete and testable.
    pub fn read_object(&self, hash: &str) -> Result<Vec<u8>, RewindError> {
        let path = self.object_path(hash);
        let mut f = File::open(&path).map_err(|e| RewindError::BadManifest {
            path: path.clone(),
            detail: format!("object missing: {e}"),
        })?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| RewindError::BadManifest {
                path,
                detail: e.to_string(),
            })?;
        Ok(buf)
    }

    /// Store `bytes` as a content-addressed object, returning its hash. If an
    /// object with that hash already exists, this is a no-op (dedup). The write
    /// is atomic: a temp file in the shard dir is renamed into place.
    pub fn put_bytes(&self, bytes: &[u8]) -> Result<String, RewindError> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hash = hasher.hex();

        let dest = self.object_path(&hash);
        if dest.is_file() {
            return Ok(hash); // already stored — dedup
        }

        let shard = dest.parent().expect("object path has a shard dir");
        fs::create_dir_all(shard).map_err(|e| RewindError::SaveFailed {
            path: shard.to_path_buf(),
            source: e,
        })?;

        atomic_write(&dest, bytes)?;
        Ok(hash)
    }

    /// Save a manifest under `captures/`. The filename is
    /// `<sanitized-captured_at>-<idprefix>.json`, sortable by time. The manifest
    /// body is written atomically and `HEAD` is updated to its id.
    pub fn save_manifest(&self, manifest: &Manifest) -> Result<PathBuf, RewindError> {
        self.ensure_dirs()?;
        let filename = manifest_filename(manifest);
        let dest = self.captures_dir().join(filename);

        let body = serde_json::to_vec_pretty(manifest).map_err(|e| RewindError::SaveFailed {
            path: dest.clone(),
            source: std::io::Error::other(e),
        })?;
        atomic_write(&dest, &body)?;

        // Update HEAD (best-effort pointer; the timeline does not depend on it).
        let _ = atomic_write(&self.head_path(), manifest.id.as_bytes());
        Ok(dest)
    }

    /// Load every manifest in the store, newest capture first (by `captured_at`,
    /// then id for ties). A manifest that fails to parse is a hard error — a
    /// corrupt capture is "rewind can't produce a faithful view," not data.
    pub fn load_manifests(&self) -> Result<Vec<Manifest>, RewindError> {
        let dir = self.captures_dir();
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => return Ok(Vec::new()),
        };

        let mut manifests = Vec::new();
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            manifests.push(load_manifest_file(&path)?);
        }

        manifests.sort_by(|a, b| {
            b.captured_at
                .cmp(&a.captured_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(manifests)
    }

    /// Total bytes the store occupies on disk (objects only — manifests are
    /// negligible). Walks `objects/`; absent dir = 0.
    pub fn store_bytes(&self) -> u64 {
        dir_size(&self.objects_dir())
    }

    /// Delete one capture's manifest file (its blobs are left to a later `--gc`,
    /// since they may be shared with surviving captures). The filename is derived
    /// from the manifest exactly as [`save_manifest`](Self::save_manifest) wrote
    /// it, so this removes the right file. A missing file is not an error — prune
    /// is idempotent. Used by `prune`.
    pub fn delete_manifest(&self, manifest: &Manifest) -> Result<(), RewindError> {
        let path = self.captures_dir().join(manifest_filename(manifest));
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(RewindError::SaveFailed { path, source: e }),
        }
    }

    /// Every object hash present on disk under `objects/<aa>/<hash>`. Used by the
    /// `--gc` mark-and-sweep to find candidates; entries whose name isn't a plain
    /// hash file are ignored. An absent `objects/` dir yields an empty list.
    pub fn iter_object_hashes(&self) -> Vec<String> {
        let mut hashes = Vec::new();
        let shards = match fs::read_dir(self.objects_dir()) {
            Ok(rd) => rd,
            Err(_) => return hashes,
        };
        for shard in shards.flatten() {
            if !shard.path().is_dir() {
                continue;
            }
            let Ok(objs) = fs::read_dir(shard.path()) else {
                continue;
            };
            for obj in objs.flatten() {
                if obj.path().is_file() {
                    if let Some(name) = obj.file_name().to_str() {
                        hashes.push(name.to_string());
                    }
                }
            }
        }
        hashes
    }

    /// Remove one object blob by hash, returning the bytes it freed (0 if it was
    /// already gone). Used by `--gc` after the live hash set is computed. Best-
    /// effort: a missing object is not an error.
    pub fn remove_object(&self, hash: &str) -> Result<u64, RewindError> {
        let path = self.object_path(hash);
        let freed = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        match fs::remove_file(&path) {
            Ok(()) => Ok(freed),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(RewindError::SaveFailed { path, source: e }),
        }
    }
}

/// Build a manifest's on-disk filename: a filesystem-safe timestamp plus a short
/// id prefix, so `captures/` lists chronologically and ids never collide.
fn manifest_filename(manifest: &Manifest) -> String {
    let ts = manifest.captured_at.replace(':', "-");
    let prefix: String = manifest.id.chars().take(8).collect();
    format!("{ts}-{prefix}.json")
}

/// Load and validate one manifest file. Rejects a manifest whose schema version
/// is newer than this build understands (loudly, not silently misread).
fn load_manifest_file(path: &Path) -> Result<Manifest, RewindError> {
    let text = fs::read_to_string(path).map_err(|e| RewindError::BadManifest {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| RewindError::BadManifest {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })?;
    if manifest.schema_version > MANIFEST_SCHEMA_VERSION {
        return Err(RewindError::BadManifest {
            path: path.to_path_buf(),
            detail: format!(
                "capture schema v{} is newer than this rewind (v{}); upgrade rewind",
                manifest.schema_version, MANIFEST_SCHEMA_VERSION
            ),
        });
    }
    Ok(manifest)
}

/// Write `bytes` to `path` atomically: a sibling temp file then a rename, so a
/// reader never sees a half-written file.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RewindError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // A temp name colocated with the destination so the rename stays same-fs.
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rewind")
    ));

    let write = |dest: &Path| -> std::io::Result<()> {
        let mut f = File::create(dest)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    };

    write(&tmp).map_err(|e| RewindError::SaveFailed {
        path: tmp.clone(),
        source: e,
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        RewindError::SaveFailed {
            path: path.to_path_buf(),
            source: e,
        }
    })?;
    Ok(())
}

/// Sum the byte sizes of all regular files under `dir` (recursive). Best-effort:
/// unreadable entries are skipped, an absent dir is 0.
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = match fs::read_dir(&d) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(md) if md.is_dir() => stack.push(path),
                Ok(md) if md.is_file() => total += md.len(),
                _ => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CaptureEntry, EntryKind};
    use tempfile::tempdir;

    fn sample_manifest(id: &str, at: &str) -> Manifest {
        Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            source_tool: "rewind".into(),
            id: id.into(),
            captured_at: at.into(),
            label: None,
            set_source: "cli".into(),
            entries: vec![CaptureEntry {
                path: "/d/x.json".into(),
                kind: EntryKind::File,
                size: Some(3),
                mode: Some("0644".into()),
                uid: Some(1000),
                gid: Some(1000),
                mtime: None,
                hash: Some("deadbeef".into()),
                target: None,
                envelope_tool: None,
                envelope_schema_version: None,
                unreadable: false,
            }],
        }
    }

    #[test]
    fn put_bytes_is_content_addressed_and_dedupes() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        store.ensure_dirs().unwrap();

        let h1 = store.put_bytes(b"abc").unwrap();
        let h2 = store.put_bytes(b"abc").unwrap();
        assert_eq!(h1, h2, "identical content -> identical hash");
        assert_eq!(
            h1,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(store.has_object(&h1));

        // Only one object on disk for two identical puts.
        let shard = store.objects_dir().join(&h1[..2]);
        let count = fs::read_dir(&shard).unwrap().count();
        assert_eq!(count, 1, "dedup: one object for identical content");

        // Different content -> different object.
        let h3 = store.put_bytes(b"xyz").unwrap();
        assert_ne!(h1, h3);
        assert_eq!(store.read_object(&h3).unwrap(), b"xyz");
    }

    #[test]
    fn save_and_load_manifests_roundtrip_newest_first() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());

        store
            .save_manifest(&sample_manifest("aaa1", "2026-06-17T02:00:00Z"))
            .unwrap();
        store
            .save_manifest(&sample_manifest("bbb2", "2026-06-19T14:22:05Z"))
            .unwrap();

        assert!(store.exists());
        let loaded = store.load_manifests().unwrap();
        assert_eq!(loaded.len(), 2);
        // Newest first.
        assert_eq!(loaded[0].id, "bbb2");
        assert_eq!(loaded[1].id, "aaa1");

        // HEAD points at the most recently saved.
        let head = fs::read_to_string(store.head_path()).unwrap();
        assert_eq!(head, "bbb2");
    }

    #[test]
    fn newer_schema_manifest_is_rejected_loudly() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        store.ensure_dirs().unwrap();

        let mut m = sample_manifest("ccc3", "2026-06-19T00:00:00Z");
        m.schema_version = MANIFEST_SCHEMA_VERSION + 1;
        // Write it raw (bypassing save_manifest's correct version).
        let body = serde_json::to_vec_pretty(&m).unwrap();
        let path = store.captures_dir().join("future.json");
        fs::write(&path, body).unwrap();

        let err = store.load_manifests().unwrap_err();
        assert!(matches!(err, RewindError::BadManifest { .. }));
    }

    #[test]
    fn store_bytes_counts_objects() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        store.ensure_dirs().unwrap();
        assert_eq!(store.store_bytes(), 0);
        store.put_bytes(b"abc").unwrap(); // 3 bytes
        store.put_bytes(b"de").unwrap(); // 2 bytes
        assert_eq!(store.store_bytes(), 5);
    }

    #[test]
    fn unopened_store_reports_not_exists_and_empty_list() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("nope"));
        assert!(!store.exists());
        assert!(store.load_manifests().unwrap().is_empty());
    }

    // ---- Phase 3: prune / gc primitives -----------------------------------

    #[test]
    fn delete_manifest_removes_the_file_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        let m = sample_manifest("aaa1", "2026-06-19T00:00:00Z");
        store.save_manifest(&m).unwrap();
        assert_eq!(store.load_manifests().unwrap().len(), 1);

        store.delete_manifest(&m).unwrap();
        assert!(store.load_manifests().unwrap().is_empty());
        // Deleting again is a no-op, not an error.
        store.delete_manifest(&m).unwrap();
    }

    #[test]
    fn iter_object_hashes_lists_every_stored_blob() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        store.ensure_dirs().unwrap();
        let h1 = store.put_bytes(b"one").unwrap();
        let h2 = store.put_bytes(b"two").unwrap();
        let mut got = store.iter_object_hashes();
        got.sort();
        let mut want = vec![h1, h2];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn remove_object_frees_bytes_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        store.ensure_dirs().unwrap();
        let h = store.put_bytes(b"abcde").unwrap(); // 5 bytes
        assert_eq!(store.remove_object(&h).unwrap(), 5);
        assert!(!store.has_object(&h));
        // Removing again frees nothing, no error.
        assert_eq!(store.remove_object(&h).unwrap(), 0);
    }
}

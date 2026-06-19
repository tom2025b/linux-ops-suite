//! The baseline: a saved snapshot of watched entries, and the diff of a live
//! scan against it. `tripwire baseline` writes one; `tripwire diff`/`verify`
//! reads it and reports what appeared, what vanished, and which kept entries
//! changed — content, permissions, owner, size, or type.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::TripwireError;
use crate::model::Entry;

/// The on-disk baseline. Versioned envelope so a format change is detectable
/// rather than silently misread — a newer schema is rejected loudly on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema_version: u32,
    pub source_tool: String,
    /// Which source the watch set came from when this was recorded (`cli` /
    /// `config` / `builtin`), for the operator's reference.
    #[serde(default)]
    pub watch_source: String,
    /// The recorded entries, sorted by path.
    pub entries: Vec<Entry>,
}

impl Baseline {
    const SCHEMA: u32 = 1;

    /// Wrap a freshly-scanned entry set as a baseline ready to save.
    pub fn from_scan(entries: Vec<Entry>, watch_source: &str) -> Self {
        Baseline {
            schema_version: Self::SCHEMA,
            source_tool: "tripwire".to_string(),
            watch_source: watch_source.to_string(),
            entries,
        }
    }

    /// Write the baseline as pretty JSON to `path`, creating parent dirs.
    pub fn save(&self, path: &Path) -> Result<(), TripwireError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| TripwireError::SaveFailed {
                path: path.to_path_buf(),
                source,
            })?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        fs::write(path, json).map_err(|source| TripwireError::SaveFailed {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Load a baseline from `path`, distinguishing "not recorded yet" from
    /// "recorded but corrupt" so the CLI can give the right next step, and
    /// rejecting a schema newer than this build understands.
    pub fn load(path: &Path) -> Result<Self, TripwireError> {
        if !path.exists() {
            return Err(TripwireError::NoBaseline {
                path: path.to_path_buf(),
            });
        }
        let text = fs::read_to_string(path).map_err(|e| TripwireError::BadBaseline {
            path: path.to_path_buf(),
            detail: e.to_string(),
        })?;
        let baseline: Baseline =
            serde_json::from_str(&text).map_err(|e| TripwireError::BadBaseline {
                path: path.to_path_buf(),
                detail: e.to_string(),
            })?;
        if baseline.schema_version > Self::SCHEMA {
            return Err(TripwireError::BadBaseline {
                path: path.to_path_buf(),
                detail: format!(
                    "baseline schema v{} is newer than this tripwire understands (v{}); upgrade tripwire or re-record",
                    baseline.schema_version,
                    Self::SCHEMA
                ),
            });
        }
        Ok(baseline)
    }
}

/// What kind of metadata changed for a kept entry, spelled out so the renderer
/// and JSON don't have to re-derive it. Each is the smallest honest claim about
/// the drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// The content hash differs (the file's bytes changed).
    Content,
    /// Permission bits changed (security-relevant).
    Mode,
    /// uid/gid changed (security-relevant).
    Owner,
    /// Size changed but content couldn't be compared (one side unreadable).
    Size,
    /// The entry's kind changed (file became a symlink, dir became a file, …).
    Type,
    /// Readability flipped (became readable, or became unreadable).
    Readability,
}

impl Field {
    /// Whether this field is one tripwire flags as security-relevant — the
    /// `[PERM]`/`[OWNER]` tags, the analogue of portman's `[PUBLIC]`.
    pub fn is_security(self) -> bool {
        matches!(self, Field::Mode | Field::Owner)
    }
}

/// One entry in a diff. Carries enough to render a line without re-looking up
/// anything, the same way portman's `Change` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A path present now that the baseline didn't have.
    Added(Entry),
    /// A path in the baseline that's no longer present.
    Removed(Entry),
    /// A path present in both whose recorded state changed. `was`/`now` are the
    /// full entries; `fields` lists what differed.
    Modified {
        was: Box<Entry>,
        now: Box<Entry>,
        fields: Vec<Field>,
    },
}

impl Change {
    /// The path this change concerns.
    pub fn path(&self) -> &str {
        match self {
            Change::Added(e) | Change::Removed(e) => &e.path,
            Change::Modified { now, .. } => &now.path,
        }
    }
}

/// The full result of comparing a live scan to a baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub changes: Vec<Change>,
}

impl Diff {
    /// Whether anything changed at all.
    pub fn is_clean(&self) -> bool {
        self.changes.is_empty()
    }

    /// (added, removed, modified) tallies for the footer.
    pub fn tally(&self) -> (usize, usize, usize) {
        let (mut a, mut r, mut m) = (0, 0, 0);
        for c in &self.changes {
            match c {
                Change::Added(_) => a += 1,
                Change::Removed(_) => r += 1,
                Change::Modified { .. } => m += 1,
            }
        }
        (a, r, m)
    }
}

/// Compare a live scan (`current`) against a recorded `baseline`. Matches
/// entries by their stable key (absolute path). Changes are emitted in
/// path-sorted order for stable, diffable output.
pub fn diff(baseline: &[Entry], current: &[Entry]) -> Diff {
    let base_by_key: BTreeMap<&str, &Entry> = baseline.iter().map(|e| (e.key(), e)).collect();
    let cur_by_key: BTreeMap<&str, &Entry> = current.iter().map(|e| (e.key(), e)).collect();

    let mut changes = Vec::new();

    // Added + modified: walk current.
    for (key, cur) in &cur_by_key {
        match base_by_key.get(key) {
            None => changes.push(Change::Added((*cur).clone())),
            Some(base) => {
                let fields = changed_fields(base, cur);
                if !fields.is_empty() {
                    changes.push(Change::Modified {
                        was: Box::new((*base).clone()),
                        now: Box::new((*cur).clone()),
                        fields,
                    });
                }
            }
        }
    }

    // Removed: in baseline but not current.
    for (key, base) in &base_by_key {
        if !cur_by_key.contains_key(key) {
            changes.push(Change::Removed((*base).clone()));
        }
    }

    // Stable order: by path, with a kind ordering so a given path's single
    // change slots deterministically.
    changes.sort_by(|a, b| a.path().cmp(b.path()));
    Diff { changes }
}

/// Decide what (if anything) differs between a baselined entry and its live
/// counterpart. The rules encode the design's degradation policy:
///
/// - mtime is never compared — a touched-but-identical file is not drift.
/// - type change short-circuits: if the kind flipped, that's the one fact worth
///   reporting; comparing the old file's hash to the new symlink's target is
///   noise.
/// - content is compared by hash only when *both* sides have one. If both are
///   unreadable we can't claim drift we can't see, so we say nothing about
///   content; a readable↔unreadable flip is itself reported as `Readability`.
/// - for symlinks, the target string is the content.
fn changed_fields(was: &Entry, now: &Entry) -> Vec<Field> {
    let mut fields = Vec::new();

    if was.kind != now.kind {
        // A type change is the whole story; don't also diff stale sub-fields.
        return vec![Field::Type];
    }

    // Permissions and ownership — the security-relevant metadata.
    if was.mode != now.mode {
        fields.push(Field::Mode);
    }
    if (was.uid, was.gid) != (now.uid, now.gid) {
        fields.push(Field::Owner);
    }

    // Readability flip is reported on its own; when it flips we can't trust a
    // content comparison either way, so we don't also emit Content.
    if was.unreadable != now.unreadable {
        fields.push(Field::Readability);
    } else {
        match (&was.hash, &now.hash) {
            // Both hashed: the authoritative content comparison.
            (Some(a), Some(b)) if a != b => fields.push(Field::Content),
            (Some(_), Some(_)) => {} // identical content
            // Symlink target acts as content.
            _ if was.target != now.target => fields.push(Field::Content),
            // Neither hashed (content=false, or both unreadable and same state):
            // fall back to size as the only visible content signal.
            _ if was.hash.is_none() && now.hash.is_none() && was.size != now.size => {
                fields.push(Field::Size)
            }
            _ => {}
        }
    }

    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryKind;
    use tempfile::tempdir;

    fn file(path: &str, hash: &str, mode: &str) -> Entry {
        Entry {
            path: path.into(),
            kind: EntryKind::File,
            size: Some(hash.len() as u64),
            mode: Some(mode.into()),
            uid: Some(0),
            gid: Some(0),
            mtime: Some("2026-01-01T00:00:00Z".into()),
            hash: Some(hash.into()),
            target: None,
            unreadable: false,
        }
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sub/baseline.json");
        let b = Baseline::from_scan(vec![file("/etc/hosts", "aa", "0644")], "builtin");
        b.save(&path).expect("save");
        let loaded = Baseline::load(&path).expect("load");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.source_tool, "tripwire");
        assert_eq!(loaded.watch_source, "builtin");
        assert_eq!(loaded.entries.len(), 1);
    }

    #[test]
    fn load_missing_is_no_baseline_not_corrupt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert!(matches!(
            Baseline::load(&path),
            Err(TripwireError::NoBaseline { .. })
        ));
    }

    #[test]
    fn load_garbage_is_bad_baseline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(matches!(
            Baseline::load(&path),
            Err(TripwireError::BadBaseline { .. })
        ));
    }

    #[test]
    fn load_newer_schema_is_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("future.json");
        fs::write(
            &path,
            r#"{"schema_version":999,"source_tool":"tripwire","watch_source":"cli","entries":[]}"#,
        )
        .unwrap();
        assert!(matches!(
            Baseline::load(&path),
            Err(TripwireError::BadBaseline { .. })
        ));
    }

    #[test]
    fn identical_scan_is_clean() {
        let base = vec![file("/etc/hosts", "aa", "0644")];
        let current = vec![file("/etc/hosts", "aa", "0644")];
        assert!(diff(&base, &current).is_clean());
    }

    #[test]
    fn mtime_only_change_is_not_drift() {
        let base = vec![file("/etc/hosts", "aa", "0644")];
        let mut touched = file("/etc/hosts", "aa", "0644");
        touched.mtime = Some("2099-12-31T23:59:59Z".into()); // touched, same bytes
        assert!(diff(&base, &[touched]).is_clean());
    }

    #[test]
    fn detects_added_removed_and_content_change() {
        let base = vec![file("/a", "aa", "0644"), file("/b", "bb", "0644")];
        let current = vec![file("/a", "AA", "0644"), file("/c", "cc", "0644")];
        let d = diff(&base, &current);

        assert!(d
            .changes
            .iter()
            .any(|c| matches!(c, Change::Added(e) if e.path == "/c")));
        assert!(d
            .changes
            .iter()
            .any(|c| matches!(c, Change::Removed(e) if e.path == "/b")));
        assert!(d.changes.iter().any(|c| matches!(
            c, Change::Modified { fields, now, .. }
            if now.path == "/a" && fields == &vec![Field::Content]
        )));
        assert_eq!(d.tally(), (1, 1, 1));
    }

    #[test]
    fn detects_mode_and_owner_changes_as_security() {
        let base = vec![file("/etc/passwd", "aa", "0644")];
        let mut now = file("/etc/passwd", "aa", "0666");
        now.uid = Some(1000);
        let d = diff(&base, &[now]);
        match &d.changes[0] {
            Change::Modified { fields, .. } => {
                assert!(fields.contains(&Field::Mode));
                assert!(fields.contains(&Field::Owner));
                assert!(fields.iter().any(|f| f.is_security()));
            }
            other => panic!("expected modified, got {other:?}"),
        }
    }

    #[test]
    fn type_change_short_circuits_to_just_type() {
        let base = vec![file("/x", "aa", "0644")];
        let mut now = file("/x", "aa", "0644");
        now.kind = EntryKind::Symlink;
        now.hash = None;
        now.target = Some("/elsewhere".into());
        let d = diff(&base, &now.clone().into_one());
        match &d.changes[0] {
            Change::Modified { fields, .. } => assert_eq!(fields, &vec![Field::Type]),
            other => panic!("expected modified type, got {other:?}"),
        }
    }

    #[test]
    fn readability_flip_is_reported_without_content_noise() {
        // baseline readable, now unreadable (e.g. perms tightened so we can't read).
        let base = vec![file("/etc/shadow", "aa", "0640")];
        let mut now = file("/etc/shadow", "aa", "0640");
        now.hash = None;
        now.unreadable = true;
        let d = diff(&base, &[now]);
        match &d.changes[0] {
            Change::Modified { fields, .. } => {
                assert!(fields.contains(&Field::Readability));
                assert!(!fields.contains(&Field::Content));
            }
            other => panic!("expected modified readability, got {other:?}"),
        }
    }

    #[test]
    fn both_unreadable_same_state_is_clean() {
        let mut a = file("/etc/shadow", "aa", "0640");
        a.hash = None;
        a.unreadable = true;
        let b = a.clone();
        assert!(diff(&[a], &[b]).is_clean());
    }

    #[test]
    fn content_false_falls_back_to_size() {
        let mut base = file("/var/log/app.log", "", "0644");
        base.hash = None;
        base.size = Some(100);
        let mut now = base.clone();
        now.size = Some(250); // grew, no hash to compare
        let d = diff(&[base], &[now]);
        match &d.changes[0] {
            Change::Modified { fields, .. } => assert_eq!(fields, &vec![Field::Size]),
            other => panic!("expected size modified, got {other:?}"),
        }
    }

    // Small helper for the type-change test's signature.
    impl Entry {
        fn into_one(self) -> Vec<Entry> {
            vec![self]
        }
    }
}

//! Core types. A [`CaptureEntry`] is one captured path plus everything rewind
//! recorded about it — kind, permission/owner metadata, and (for readable files)
//! the content `hash` that doubles as its object key in the store. A [`Manifest`]
//! is one whole capture: an immutable point in time of the capture set, the unit
//! that `list`/`show`/`diff`/`restore`/`prune` operate on. The scan/store code
//! never prints; the human and JSON renderers derive everything from these.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The schema version of a capture manifest on disk. Bumped only on a
/// breaking change to the manifest shape; a manifest with a higher version is
/// rejected loudly rather than silently misread (see [`crate::capture`]).
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// What kind of filesystem object an entry is. `Other` covers sockets, fifos,
/// and device nodes — present and worth recording but with no content to store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

impl EntryKind {
    /// Short tag used in human output and as a stable JSON value.
    pub fn tag(self) -> &'static str {
        match self {
            EntryKind::File => "file",
            EntryKind::Dir => "dir",
            EntryKind::Symlink => "symlink",
            EntryKind::Other => "other",
        }
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// One captured path and its recorded state. Identity for diffing is the
/// absolute [`path`], so a file rewritten in place between captures is the
/// *same* entry (changed), and a rename is an honest remove + add.
///
/// Optional fields are absent (`None`) when they don't apply or couldn't be
/// resolved — a directory has no `hash`, a non-symlink has no `target`, an
/// unreadable file has no `hash` (and so is not restorable). The absence *is*
/// the signal, the same way tripwire omits a hash it couldn't compute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureEntry {
    /// Absolute path — the stable diff key and the path a restore writes back to.
    pub path: String,
    pub kind: EntryKind,
    /// File size in bytes. Files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Permission bits as a 4-digit octal string, e.g. `0644`. Restored verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Numeric owner uid. Names are a render concern, not stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// Numeric owner gid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
    /// Modification time, RFC3339 (UTC). Informational only — **never** part of
    /// identity or of the change decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
    /// SHA-256 of the file's contents, lowercase hex. Files only, when readable.
    /// This doubles as the object key in the content-addressed store.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Symlink target string. Symlinks only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// `source_tool` parsed from the blob when it is a recognized suite JSON
    /// envelope. Informational — lets `show`/`diff` label a captured file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope_tool: Option<String>,
    /// `schema_version` parsed from the blob when it is a recognized envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope_schema_version: Option<u32>,
    /// True when the path exists but its content couldn't be read. Metadata is
    /// still recorded; `hash` is absent and the entry is not restorable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unreadable: bool,
}

/// serde helper: skip serializing a `false` bool so the common case stays clean.
fn is_false(b: &bool) -> bool {
    !*b
}

impl CaptureEntry {
    /// Stable identity of an entry: its absolute path.
    pub fn key(&self) -> &str {
        &self.path
    }
}

/// One capture: an immutable point in time of the capture set. Persisted as one
/// JSON file under `captures/`; the file blobs it references live in `objects/`,
/// keyed by each entry's `hash`. The `id` is content-derived (see
/// [`crate::capture`]) so it is stable and prefix-addressable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest schema version, for forward-compatibility rejection.
    pub schema_version: u32,
    /// Always `"rewind"` — the suite envelope discriminator.
    pub source_tool: String,
    /// Content-derived capture id (lowercase hex). The timeline shows a prefix.
    pub id: String,
    /// When the capture was taken, RFC3339 (UTC).
    pub captured_at: String,
    /// Optional human label (`--label`), e.g. `pre-upgrade`. `None` for an
    /// unlabeled (e.g. cron) capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Where the capture set was resolved from: `cli` / `config` / `builtin`.
    pub set_source: String,
    /// The captured entries, sorted by path.
    pub entries: Vec<CaptureEntry>,
}

impl Manifest {
    /// Count of captured paths.
    pub fn path_count(&self) -> usize {
        self.entries.len()
    }

    /// Sum of captured file sizes (the logical, pre-dedup size of the capture).
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().filter_map(|e| e.size).sum()
    }

    /// Whether this capture contains a recognized, valid flagship snapshot —
    /// any captured file whose envelope is `workstate`. Drives the
    /// `good`/`snapshot invalid` note and `--latest-good` (later phase). A
    /// capture with no snapshot at all is *not* "good" in this sense, but it is
    /// not "invalid" either; see [`snapshot_state`].
    pub fn has_valid_snapshot(&self) -> bool {
        matches!(self.snapshot_state(), SnapshotState::Good)
    }

    /// The snapshot health of this capture for the timeline `NOTE` column.
    pub fn snapshot_state(&self) -> SnapshotState {
        let mut saw_snapshot = false;
        for e in &self.entries {
            if e.path.ends_with("workstate.snapshot.json") {
                saw_snapshot = true;
                if e.envelope_tool.as_deref() == Some("workstate") {
                    return SnapshotState::Good;
                }
            }
        }
        if saw_snapshot {
            SnapshotState::Invalid
        } else {
            SnapshotState::Absent
        }
    }
}

/// Health of the flagship snapshot within a capture, used for the timeline note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    /// A flagship snapshot is present and parses as a valid `workstate` envelope.
    Good,
    /// A flagship snapshot file is present but did not parse as a valid envelope.
    Invalid,
    /// No flagship snapshot was in the capture set (e.g. a custom `--path` run).
    Absent,
}

impl SnapshotState {
    /// Short note word for the timeline.
    pub fn note(self) -> &'static str {
        match self {
            SnapshotState::Good => "good",
            SnapshotState::Invalid => "snapshot invalid",
            SnapshotState::Absent => "—",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, tool: Option<&str>) -> CaptureEntry {
        CaptureEntry {
            path: path.into(),
            kind: EntryKind::File,
            size: Some(10),
            mode: Some("0644".into()),
            uid: Some(1000),
            gid: Some(1000),
            mtime: Some("2026-06-19T00:00:00Z".into()),
            hash: Some("deadbeef".into()),
            target: None,
            envelope_tool: tool.map(str::to_string),
            envelope_schema_version: tool.map(|_| 4),
            unreadable: false,
        }
    }

    fn manifest(entries: Vec<CaptureEntry>) -> Manifest {
        Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            source_tool: "rewind".into(),
            id: "abc123".into(),
            captured_at: "2026-06-19T14:22:05Z".into(),
            label: Some("pre-upgrade".into()),
            set_source: "builtin".into(),
            entries,
        }
    }

    #[test]
    fn key_is_the_absolute_path() {
        assert_eq!(
            entry("/x/workstate.snapshot.json", None).key(),
            "/x/workstate.snapshot.json"
        );
    }

    #[test]
    fn entry_kind_tags_are_stable() {
        assert_eq!(EntryKind::File.tag(), "file");
        assert_eq!(EntryKind::Dir.tag(), "dir");
        assert_eq!(EntryKind::Symlink.tag(), "symlink");
        assert_eq!(EntryKind::Other.tag(), "other");
    }

    #[test]
    fn snapshot_state_good_invalid_absent() {
        // A valid workstate snapshot -> good.
        let good = manifest(vec![entry("/d/workstate.snapshot.json", Some("workstate"))]);
        assert_eq!(good.snapshot_state(), SnapshotState::Good);
        assert!(good.has_valid_snapshot());

        // A snapshot file that didn't parse as a workstate envelope -> invalid.
        let invalid = manifest(vec![entry("/d/workstate.snapshot.json", None)]);
        assert_eq!(invalid.snapshot_state(), SnapshotState::Invalid);
        assert!(!invalid.has_valid_snapshot());

        // No snapshot in the set -> absent.
        let absent = manifest(vec![entry("/d/some.conf", None)]);
        assert_eq!(absent.snapshot_state(), SnapshotState::Absent);
        assert!(!absent.has_valid_snapshot());
    }

    #[test]
    fn totals_sum_path_count_and_bytes() {
        let m = manifest(vec![
            entry("/a", None),
            entry("/workstate.snapshot.json", Some("workstate")),
        ]);
        assert_eq!(m.path_count(), 2);
        assert_eq!(m.total_bytes(), 20);
    }

    #[test]
    fn optional_fields_omitted_from_json() {
        let mut e = entry("/etc/x", None);
        e.hash = None;
        e.mtime = None;
        e.envelope_tool = None;
        e.envelope_schema_version = None;
        e.unreadable = true;
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert!(v.get("hash").is_none());
        assert!(v.get("mtime").is_none());
        assert!(v.get("envelope_tool").is_none());
        assert_eq!(v["unreadable"], true);

        // A clean entry omits `unreadable` entirely.
        let clean: serde_json::Value = serde_json::to_value(entry("/x", None)).unwrap();
        assert!(clean.get("unreadable").is_none());
    }

    #[test]
    fn manifest_roundtrips_through_serde() {
        let m = manifest(vec![entry("/a", Some("workstate"))]);
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }
}

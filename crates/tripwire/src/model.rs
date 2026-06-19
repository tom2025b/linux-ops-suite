//! Core types. An [`Entry`] is one watched path plus everything tripwire could
//! record about its state — kind, permission/owner metadata, and (for readable
//! files) a content hash. It is the unit of output and the unit a baseline
//! records. The scan functions never print; the human and JSON renderers derive
//! everything they show from these.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What kind of filesystem object an entry is. `Other` covers sockets, fifos,
/// and device nodes — present and worth recording (a new device node appearing
/// under a watched dir is itself interesting) but with no content to hash.
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

/// One watched path and its recorded state — tripwire's unit of output and the
/// unit a baseline records. Identity for diffing is the absolute [`path`], so a
/// file rewritten in place is the *same* entry (modified), and a rename is an
/// honest remove + add.
///
/// Optional fields are absent (`None`) when they don't apply or couldn't be
/// resolved — a directory has no `hash`, a non-symlink has no `target`, an
/// unreadable file has no `hash`. The absence *is* the signal, the same way
/// portman omits an unresolved owner link rather than inventing one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Absolute path — the stable diff key.
    pub path: String,
    pub kind: EntryKind,
    /// File size in bytes. Files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Permission bits as a 4-digit octal string, e.g. `0644`. The
    /// security-relevant part of the mode; type bits live in `kind`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Numeric owner uid. Names are a render concern, not stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// Numeric owner gid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
    /// Modification time, RFC3339 (UTC). Informational only — **never** part of
    /// identity or of the change decision; a touched-but-identical file is not
    /// drift.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
    /// SHA-256 of the file's contents, lowercase hex. Files only, and only when
    /// content hashing was enabled and the file was readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Symlink target string. Symlinks only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// True when the path exists but its content couldn't be read (e.g.
    /// `/etc/shadow` as a non-root user). Metadata is still recorded; `hash` is
    /// absent. Defaults to false and is omitted from JSON when false.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unreadable: bool,
}

/// serde helper: skip serializing a `false` bool so the common case stays clean.
fn is_false(b: &bool) -> bool {
    !*b
}

impl Entry {
    /// Stable identity of an entry for baseline comparison: its absolute path.
    pub fn key(&self) -> &str {
        &self.path
    }

    /// A short, human label for the entry's owner: `uid:gid` when known, else
    /// `?`. Used in the diff's `[OWNER]` change line.
    pub fn owner_label(&self) -> String {
        match (self.uid, self.gid) {
            (Some(u), Some(g)) => format!("{u}:{g}"),
            _ => "?".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, hash: &str) -> Entry {
        Entry {
            path: path.into(),
            kind: EntryKind::File,
            size: Some(10),
            mode: Some("0644".into()),
            uid: Some(0),
            gid: Some(0),
            mtime: Some("2026-06-01T00:00:00Z".into()),
            hash: Some(hash.into()),
            target: None,
            unreadable: false,
        }
    }

    #[test]
    fn key_is_the_absolute_path() {
        assert_eq!(file("/etc/passwd", "deadbeef").key(), "/etc/passwd");
    }

    #[test]
    fn owner_label_is_uid_gid_or_question_mark() {
        let mut e = file("/x", "a");
        assert_eq!(e.owner_label(), "0:0");
        e.uid = None;
        assert_eq!(e.owner_label(), "?");
    }

    #[test]
    fn entry_kind_tags_are_stable() {
        assert_eq!(EntryKind::File.tag(), "file");
        assert_eq!(EntryKind::Dir.tag(), "dir");
        assert_eq!(EntryKind::Symlink.tag(), "symlink");
        assert_eq!(EntryKind::Other.tag(), "other");
    }

    #[test]
    fn unreadable_and_none_fields_are_omitted_from_json() {
        let e = Entry {
            path: "/etc/shadow".into(),
            kind: EntryKind::File,
            size: Some(1),
            mode: Some("0640".into()),
            uid: Some(0),
            gid: Some(0),
            mtime: None,
            hash: None,
            target: None,
            unreadable: true,
        };
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert!(v.get("hash").is_none(), "no hash key when unreadable");
        assert!(v.get("target").is_none(), "no target key for a file");
        assert!(v.get("mtime").is_none(), "absent mtime omitted");
        assert_eq!(v["unreadable"], true);

        // A clean entry omits the `unreadable` key entirely.
        let clean: serde_json::Value = serde_json::to_value(file("/x", "a")).unwrap();
        assert!(clean.get("unreadable").is_none());
    }

    #[test]
    fn entry_roundtrips_through_serde() {
        let e = file("/etc/passwd", "abc123");
        let json = serde_json::to_string(&e).unwrap();
        let back: Entry = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }
}

//! Comparing two points in time. A [`Diff`] is the path-keyed difference between
//! a "from" set of entries and a "to" set — either two captures
//! (`diff_captures`) or one capture against the live filesystem
//! (`diff_capture_vs_live`). Identity is **content**, never time: a file with a
//! changed mtime but identical bytes is `Unchanged`. Each [`Change`] carries
//! enough of both sides to render the human diff and the JSON envelope without
//! re-reading anything, the same way tripwire's `ChangeOut` does.
//!
//! The classifier never reads blobs — it compares the hashes/targets already on
//! the [`CaptureEntry`]s — so a diff is strictly read-only.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::model::{CaptureEntry, EntryKind};

/// How one path changed between the two points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ChangeKind {
    /// Present in `to` but not in `from`.
    Added,
    /// Present in `from` but not in `to`.
    Removed,
    /// Present in both, but content (or symlink target, or kind) differs.
    Changed,
    /// Present in both and provably identical.
    Unchanged,
}

impl ChangeKind {
    /// The one-character marker for the human diff (`+ - ~ =`), suite vocabulary.
    pub fn marker(self) -> char {
        match self {
            ChangeKind::Added => '+',
            ChangeKind::Removed => '-',
            ChangeKind::Changed => '~',
            ChangeKind::Unchanged => '=',
        }
    }
}

/// One path's difference between the two points. The `was_*` fields describe the
/// `from` side, the `now_*` fields the `to` side; each is absent when that side
/// has no such value (a removed path has no `now_*`, a directory has no hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub kind: ChangeKind,
    /// Absolute path — the diff key.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_schema: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now_schema: Option<u32>,
}

/// The full comparison of two points: the `from`/`to` labels (a capture id, or
/// the literal `"live"`), every path's [`Change`] sorted by path, and the
/// category counts. `is_clean` drives the exit code (clean → 0, dirty → 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub from: String,
    pub to: String,
    pub clean: bool,
    pub changed: usize,
    pub added: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub changes: Vec<Change>,
}

impl Diff {
    /// Whether the two points are identical (no added/removed/changed paths).
    pub fn is_clean(&self) -> bool {
        self.clean
    }
}

/// Compare two entry sets keyed by absolute path. `from_label`/`to_label` name
/// the two points for rendering (a capture id, or `"live"`). Content identity is
/// kind-aware (see [`same_content`]); the output is sorted by path.
pub fn diff_entries(
    from: &[CaptureEntry],
    to: &[CaptureEntry],
    from_label: &str,
    to_label: &str,
) -> Diff {
    // Union of every path on either side, sorted (BTreeSet gives both for free).
    let paths: BTreeSet<&str> = from
        .iter()
        .map(|e| e.path.as_str())
        .chain(to.iter().map(|e| e.path.as_str()))
        .collect();

    let find =
        |set: &[CaptureEntry], p: &str| -> Option<usize> { set.iter().position(|e| e.path == p) };

    let mut changes = Vec::with_capacity(paths.len());
    let (mut changed, mut added, mut removed, mut unchanged) = (0, 0, 0, 0);

    for p in paths {
        let f = find(from, p).map(|i| &from[i]);
        let t = find(to, p).map(|i| &to[i]);
        let kind = match (f, t) {
            (Some(a), Some(b)) => {
                if same_content(a, b) {
                    ChangeKind::Unchanged
                } else {
                    ChangeKind::Changed
                }
            }
            (None, Some(_)) => ChangeKind::Added,
            (Some(_), None) => ChangeKind::Removed,
            (None, None) => unreachable!("path came from one of the two sets"),
        };
        match kind {
            ChangeKind::Changed => changed += 1,
            ChangeKind::Added => added += 1,
            ChangeKind::Removed => removed += 1,
            ChangeKind::Unchanged => unchanged += 1,
        }
        changes.push(Change {
            kind,
            path: p.to_string(),
            was_hash: f.and_then(|e| e.hash.clone()),
            now_hash: t.and_then(|e| e.hash.clone()),
            was_bytes: f.and_then(|e| e.size),
            now_bytes: t.and_then(|e| e.size),
            was_schema: f.and_then(|e| e.envelope_schema_version),
            now_schema: t.and_then(|e| e.envelope_schema_version),
        });
    }

    let clean = changed == 0 && added == 0 && removed == 0;
    Diff {
        from: from_label.to_string(),
        to: to_label.to_string(),
        clean,
        changed,
        added,
        removed,
        unchanged,
        changes,
    }
}

/// Whether two entries for the same path are provably identical. Identity is
/// kind-aware and never depends on mtime/mode/owner (those are data, not
/// identity):
///
/// - **File** — equal iff both hashes are present and equal. Two *unreadable*
///   files (both hashes `None`) are **not** provably equal — the content exists
///   but couldn't be read — so they classify as changed, never a false
///   "unchanged" the tool can't justify.
/// - **Symlink** — equal iff both targets are present and equal.
/// - **Dir / Other** — no content to compare; equal by presence (both exist).
///
/// A kind that differs between the two sides (a file became a symlink) is never
/// equal.
fn same_content(a: &CaptureEntry, b: &CaptureEntry) -> bool {
    if a.kind != b.kind {
        return false;
    }
    match a.kind {
        EntryKind::File => match (&a.hash, &b.hash) {
            (Some(x), Some(y)) => x == y,
            // Either side unreadable -> can't confirm equality -> changed.
            _ => false,
        },
        EntryKind::Symlink => match (&a.target, &b.target) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        EntryKind::Dir | EntryKind::Other => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(
        path: &str,
        hash: Option<&str>,
        size: Option<u64>,
        schema: Option<u32>,
    ) -> CaptureEntry {
        CaptureEntry {
            path: path.into(),
            kind: EntryKind::File,
            size,
            mode: Some("0644".into()),
            uid: Some(1000),
            gid: Some(1000),
            mtime: Some("2026-06-19T00:00:00Z".into()),
            hash: hash.map(str::to_string),
            target: None,
            envelope_tool: schema.map(|_| "workstate".into()),
            envelope_schema_version: schema,
            unreadable: hash.is_none(),
        }
    }

    fn symlink(path: &str, target: &str) -> CaptureEntry {
        CaptureEntry {
            path: path.into(),
            kind: EntryKind::Symlink,
            size: None,
            mode: Some("0777".into()),
            uid: Some(1000),
            gid: Some(1000),
            mtime: None,
            hash: None,
            target: Some(target.into()),
            envelope_tool: None,
            envelope_schema_version: None,
            unreadable: false,
        }
    }

    fn dir(path: &str) -> CaptureEntry {
        CaptureEntry {
            path: path.into(),
            kind: EntryKind::Dir,
            size: None,
            mode: Some("0755".into()),
            uid: Some(1000),
            gid: Some(1000),
            mtime: None,
            hash: None,
            target: None,
            envelope_tool: None,
            envelope_schema_version: None,
            unreadable: false,
        }
    }

    #[test]
    fn identical_sets_are_all_unchanged_and_clean() {
        let a = vec![file("/x", Some("h1"), Some(10), Some(4)), dir("/d")];
        let b = a.clone();
        let d = diff_entries(&a, &b, "aaa", "bbb");
        assert!(d.is_clean());
        assert_eq!(d.unchanged, 2);
        assert_eq!(d.changed + d.added + d.removed, 0);
    }

    #[test]
    fn content_change_is_one_changed_with_both_sides() {
        let a = vec![file("/x", Some("h1"), Some(10), Some(4))];
        let b = vec![file("/x", Some("h2"), Some(12), Some(4))];
        let d = diff_entries(&a, &b, "aaa", "bbb");
        assert!(!d.is_clean());
        assert_eq!(d.changed, 1);
        let c = &d.changes[0];
        assert_eq!(c.kind, ChangeKind::Changed);
        assert_eq!(c.was_hash.as_deref(), Some("h1"));
        assert_eq!(c.now_hash.as_deref(), Some("h2"));
        assert_eq!(c.was_bytes, Some(10));
        assert_eq!(c.now_bytes, Some(12));
        assert_eq!(c.was_schema, Some(4));
        assert_eq!(c.now_schema, Some(4));
    }

    #[test]
    fn added_only_and_removed_only() {
        let a = vec![file("/keep", Some("h"), Some(1), None)];
        let b = vec![
            file("/keep", Some("h"), Some(1), None),
            file("/new", Some("n"), Some(2), None),
        ];
        let added = diff_entries(&a, &b, "aaa", "bbb");
        assert_eq!(added.added, 1);
        assert_eq!(
            added
                .changes
                .iter()
                .find(|c| c.path == "/new")
                .unwrap()
                .kind,
            ChangeKind::Added
        );
        // The reverse is a removal.
        let removed = diff_entries(&b, &a, "bbb", "aaa");
        assert_eq!(removed.removed, 1);
    }

    #[test]
    fn mtime_or_mode_differ_but_hash_equal_is_unchanged() {
        // The central principle: time/owner are data, not identity.
        let mut a = file("/x", Some("samehash"), Some(10), Some(4));
        let mut b = a.clone();
        a.mtime = Some("2026-06-19T00:00:00Z".into());
        b.mtime = Some("2026-06-20T23:59:59Z".into());
        a.mode = Some("0644".into());
        b.mode = Some("0600".into());
        let d = diff_entries(
            std::slice::from_ref(&a),
            std::slice::from_ref(&b),
            "aaa",
            "bbb",
        );
        assert!(
            d.is_clean(),
            "byte-identical content -> unchanged despite mtime/mode"
        );
        assert_eq!(d.unchanged, 1);
    }

    #[test]
    fn two_unreadable_files_are_changed_not_unchanged() {
        // Honesty: unknown content on both sides can't be claimed equal.
        let a = vec![file("/secret", None, None, None)];
        let b = vec![file("/secret", None, None, None)];
        let d = diff_entries(&a, &b, "aaa", "bbb");
        assert!(!d.is_clean());
        assert_eq!(d.changed, 1);
        assert_eq!(d.changes[0].kind, ChangeKind::Changed);
    }

    #[test]
    fn one_side_unreadable_is_changed() {
        let a = vec![file("/x", Some("h1"), Some(10), None)];
        let b = vec![file("/x", None, None, None)];
        let d = diff_entries(&a, &b, "aaa", "bbb");
        assert_eq!(d.changed, 1);
    }

    #[test]
    fn symlink_target_change_is_changed() {
        let a = vec![symlink("/link", "/old/target")];
        let b = vec![symlink("/link", "/new/target")];
        let d = diff_entries(&a, &b, "aaa", "bbb");
        assert_eq!(d.changed, 1);
        // Same target -> unchanged.
        let same = diff_entries(&a, &a, "aaa", "bbb");
        assert!(same.is_clean());
    }

    #[test]
    fn kind_change_is_changed() {
        let a = vec![file("/p", Some("h"), Some(3), None)];
        let b = vec![symlink("/p", "/elsewhere")];
        let d = diff_entries(&a, &b, "aaa", "bbb");
        assert_eq!(d.changed, 1);
        assert_eq!(d.changes[0].kind, ChangeKind::Changed);
    }

    #[test]
    fn two_dirs_are_unchanged() {
        let a = vec![dir("/d")];
        let d = diff_entries(&a, &a, "aaa", "bbb");
        assert!(d.is_clean());
        assert_eq!(d.unchanged, 1);
    }

    #[test]
    fn changes_are_sorted_by_path_and_counts_add_up() {
        let a = vec![
            file("/b", Some("h"), Some(1), None),
            file("/a", Some("h"), Some(1), None),
            file("/gone", Some("g"), Some(1), None),
        ];
        let b = vec![
            file("/a", Some("h"), Some(1), None),   // unchanged
            file("/b", Some("h2"), Some(2), None),  // changed
            file("/new", Some("n"), Some(1), None), // added
        ];
        let d = diff_entries(&a, &b, "aaa", "bbb");
        let paths: Vec<&str> = d.changes.iter().map(|c| c.path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted, "sorted by path");
        assert_eq!(d.changed, 1);
        assert_eq!(d.added, 1);
        assert_eq!(d.removed, 1);
        assert_eq!(d.unchanged, 1);
        assert_eq!(d.changes.len(), 4);
        assert!(!d.is_clean());
    }

    #[test]
    fn marker_chars_are_suite_vocabulary() {
        assert_eq!(ChangeKind::Added.marker(), '+');
        assert_eq!(ChangeKind::Removed.marker(), '-');
        assert_eq!(ChangeKind::Changed.marker(), '~');
        assert_eq!(ChangeKind::Unchanged.marker(), '=');
    }
}

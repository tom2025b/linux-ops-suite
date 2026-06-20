//! The guarded restore — the one place rewind writes to live paths. Every write
//! is fenced by the safety contract:
//!
//! - **R1** restore only rewind's own captures, back to the exact captured path.
//! - **R2** dry-run by default; [`plan`] computes a write-free [`RestorePlan`],
//!   [`apply`] is the only thing that writes.
//! - **R3** on apply, a `pre-restore` safety capture of the live state of the
//!   touched paths is taken first (unless skipped), so every restore is itself
//!   undoable.
//! - **R4** each file is written to a temp file in the target dir then renamed
//!   over the original (atomic same-fs replace), with the captured mode and —
//!   best-effort — uid/gid. Can't-set-owner warns and continues.
//! - **R5** restoring an envelope whose schema is older than the live one is
//!   flagged as a downgrade.
//! - **R6** any per-path failure is reported and surfaces as a non-zero exit; a
//!   restore never aborts the batch and never claims a partial run succeeded.
//!
//! The planner reuses [`crate::diff::diff_entries`] as its classification engine
//! (capture-vs-live), then maps each change into a restore action and overrides
//! the cases a diff can't see (unreadable-in-capture, missing object).

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::diff::{self, ChangeKind};
use crate::error::RewindError;
use crate::model::{CaptureEntry, EntryKind, Manifest};
use crate::scan;
use crate::set::{CaptureSet, CaptureSpec, SetSource};
use crate::store::Store;
use crate::util;

/// serde helper: skip serializing a `false` bool so the common case stays clean.
fn is_false(b: &bool) -> bool {
    !*b
}

// ---- Plan model (dry run) -------------------------------------------------

/// What restoring one path would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RestoreAction {
    /// The live file differs and would be overwritten with captured content.
    WouldOverwrite,
    /// The path is absent live and would be created from captured content.
    WouldCreate,
    /// The live file already matches the capture — nothing to do.
    Unchanged,
    /// Not restorable (see [`SkipReason`]); left untouched.
    Skipped,
}

/// Why a path can't be restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SkipReason {
    /// The captured entry was unreadable at capture time (no stored content).
    UnreadableInCapture,
    /// The entry references an object that is gone from the store.
    MissingObject,
}

impl SkipReason {
    /// Human phrase for the note column.
    pub fn word(self) -> &'static str {
        match self {
            SkipReason::UnreadableInCapture => "unreadable in capture",
            SkipReason::MissingObject => "missing object",
        }
    }
}

/// One path's restore classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreItem {
    pub path: String,
    pub action: RestoreAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<SkipReason>,
    /// Live size (the side being replaced); absent when creating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_bytes: Option<u64>,
    /// Captured size (the side being written); absent for a dir/symlink.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now_bytes: Option<u64>,
    /// R5: restoring this would put an older schema under a newer live consumer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub schema_downgrade: bool,
}

/// The write-free restore plan: the target capture + per-path items + counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestorePlan {
    pub from: String,
    pub captured_at: String,
    pub items: Vec<RestoreItem>,
    pub would_change: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub schema_downgrades: usize,
}

impl RestorePlan {
    /// Whether anything would actually be written (drives the apply summary).
    pub fn has_work(&self) -> bool {
        self.would_change > 0
    }
}

// ---- Outcome model (apply) ------------------------------------------------

/// What restoring one path actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RestoreOutcomeKind {
    Restored,
    Created,
    Unchanged,
    Skipped,
    Failed,
}

/// One path's apply result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreResult {
    pub path: String,
    pub outcome: RestoreOutcomeKind,
    /// Failure detail or skip reason, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub was_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now_bytes: Option<u64>,
    /// R4: content+mode landed but uid/gid could not be set.
    #[serde(default, skip_serializing_if = "is_false")]
    pub owner_unset: bool,
}

/// The full apply outcome: the safety-capture id, per-path results, and counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreOutcome {
    pub from: String,
    pub applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_capture: Option<String>,
    pub results: Vec<RestoreResult>,
    pub restored: usize,
    pub failed: usize,
    pub unchanged: usize,
    pub skipped: usize,
}

impl RestoreOutcome {
    /// Whether any path failed — R6: drives the exit-2 code in `main`.
    pub fn has_failure(&self) -> bool {
        self.failed > 0
    }
}

// ---- Planning (R2: write-free) --------------------------------------------

/// Build the restore plan for `target` against the current live files. Reads the
/// live state read-only ([`scan::live_scan`]) and classifies each captured path;
/// writes nothing. `store` is consulted only to check object presence.
pub fn plan(target: &Manifest, store: &Store) -> RestorePlan {
    let live = scan::live_scan(&live_set(target));
    let short_id: String = target.id.chars().take(8).collect();

    // Reuse the diff engine for the capture-vs-live classification, oriented so
    // "the capture" is the `to` side: Added => only in capture (create),
    // Changed => differs (overwrite), Unchanged => live already matches.
    let d = diff::diff_entries(&live, &target.entries, "live", &short_id);

    let mut items = Vec::with_capacity(target.entries.len());
    for change in &d.changes {
        // We only restore paths that exist in the capture; a path present only
        // live (Removed in this orientation) is never touched — R1.
        if change.kind == ChangeKind::Removed {
            continue;
        }
        let captured = target.entries.iter().find(|e| e.path == change.path);
        let Some(captured) = captured else { continue };

        let (action, reason) = classify(captured, change.kind, store);
        let live_entry = live.iter().find(|e| e.path == change.path);
        items.push(RestoreItem {
            path: change.path.clone(),
            action,
            reason,
            was_bytes: live_entry.and_then(|e| e.size),
            now_bytes: captured.size,
            schema_downgrade: schema_downgrade(captured, live_entry),
        });
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));

    let would_change = items
        .iter()
        .filter(|i| {
            matches!(
                i.action,
                RestoreAction::WouldOverwrite | RestoreAction::WouldCreate
            )
        })
        .count();
    let unchanged = items
        .iter()
        .filter(|i| i.action == RestoreAction::Unchanged)
        .count();
    let skipped = items
        .iter()
        .filter(|i| i.action == RestoreAction::Skipped)
        .count();
    let schema_downgrades = items.iter().filter(|i| i.schema_downgrade).count();

    RestorePlan {
        from: short_id,
        captured_at: target.captured_at.clone(),
        items,
        would_change,
        unchanged,
        skipped,
        schema_downgrades,
    }
}

/// Map a captured entry + its diff kind into a restore action, overriding the two
/// cases the diff can't see: an unreadable-in-capture entry and a referenced
/// object that's gone from the store.
fn classify(
    captured: &CaptureEntry,
    kind: ChangeKind,
    store: &Store,
) -> (RestoreAction, Option<SkipReason>) {
    // Only files carry restorable content via a stored object. A file entry with
    // no hash was unreadable at capture time -> not restorable.
    if captured.kind == EntryKind::File {
        match &captured.hash {
            None => {
                return (
                    RestoreAction::Skipped,
                    Some(SkipReason::UnreadableInCapture),
                )
            }
            Some(h) if !store.has_object(h) => {
                return (RestoreAction::Skipped, Some(SkipReason::MissingObject));
            }
            Some(_) => {}
        }
    }
    if captured.unreadable {
        return (
            RestoreAction::Skipped,
            Some(SkipReason::UnreadableInCapture),
        );
    }

    match kind {
        ChangeKind::Added => (RestoreAction::WouldCreate, None),
        ChangeKind::Changed => (RestoreAction::WouldOverwrite, None),
        ChangeKind::Unchanged => (RestoreAction::Unchanged, None),
        // Removed is filtered before classify; treat defensively as unchanged.
        ChangeKind::Removed => (RestoreAction::Unchanged, None),
    }
}

/// R5: would restoring `captured` put an OLDER envelope schema under a NEWER live
/// one? Only meaningful when both sides are recognized envelopes.
fn schema_downgrade(captured: &CaptureEntry, live: Option<&CaptureEntry>) -> bool {
    match (
        captured.envelope_schema_version,
        live.and_then(|e| e.envelope_schema_version),
    ) {
        (Some(cap), Some(live)) => cap < live,
        _ => false,
    }
}

/// The capture set covering exactly the target manifest's paths — used both to
/// re-scan live for the plan and to take the pre-restore safety capture. Built
/// from the recorded entries (NOT the default resolution), so it touches exactly
/// what the restore will.
fn live_set(target: &Manifest) -> CaptureSet {
    let specs: Vec<CaptureSpec> = target
        .entries
        .iter()
        .map(|e| CaptureSpec::new(PathBuf::from(&e.path)))
        .collect();
    CaptureSet {
        specs,
        source: SetSource::Cli,
    }
}

// ---- Apply (R3-R6: the write path) ----------------------------------------

/// Execute the restore. Takes the pre-restore safety capture first (unless
/// `safety_capture` is false), then writes each restorable entry back atomically.
/// `captured_at` timestamps the safety capture (injected for testability).
pub fn apply(
    target: &Manifest,
    store: &Store,
    safety_capture: bool,
    captured_at: &str,
) -> Result<RestoreOutcome, RewindError> {
    let p = plan(target, store);
    let short_id = p.from.clone();

    // R3: capture the live state of the to-be-touched paths BEFORE any write.
    let safety_id = if safety_capture {
        take_safety_capture(target, store, captured_at, &short_id)?
    } else {
        None
    };

    let mut results = Vec::with_capacity(p.items.len());
    for item in &p.items {
        let captured = target
            .entries
            .iter()
            .find(|e| e.path == item.path)
            .expect("plan items come from target entries");
        results.push(apply_one(item, captured, store));
    }

    let restored = results
        .iter()
        .filter(|r| {
            matches!(
                r.outcome,
                RestoreOutcomeKind::Restored | RestoreOutcomeKind::Created
            )
        })
        .count();
    let failed = results
        .iter()
        .filter(|r| r.outcome == RestoreOutcomeKind::Failed)
        .count();
    let unchanged = results
        .iter()
        .filter(|r| r.outcome == RestoreOutcomeKind::Unchanged)
        .count();
    let skipped = results
        .iter()
        .filter(|r| r.outcome == RestoreOutcomeKind::Skipped)
        .count();

    Ok(RestoreOutcome {
        from: short_id,
        applied: true,
        safety_capture: safety_id,
        results,
        restored,
        failed,
        unchanged,
        skipped,
    })
}

/// R3: take the `pre-restore:<id>` safety capture of the live state of exactly
/// the target's paths. If nothing live exists to save (every path is a create),
/// `capture` returns `EmptySet` — treat that as "nothing to protect" and proceed
/// with no safety id rather than blocking the restore.
fn take_safety_capture(
    target: &Manifest,
    store: &Store,
    captured_at: &str,
    short_id: &str,
) -> Result<Option<String>, RewindError> {
    let set = live_set(target);
    let label = format!("pre-restore:{short_id}");
    match crate::capture::capture(&set, store, captured_at, Some(&label)) {
        Ok(m) => Ok(Some(m.id)),
        Err(RewindError::EmptySet) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Apply one item: unchanged/skipped pass through; create/overwrite read the
/// object and atomic-write it back. A write failure is a per-path `Failed`, never
/// a panic and never an abort (R6).
fn apply_one(item: &RestoreItem, captured: &CaptureEntry, store: &Store) -> RestoreResult {
    let base = |outcome, reason| RestoreResult {
        path: item.path.clone(),
        outcome,
        reason,
        was_bytes: item.was_bytes,
        now_bytes: item.now_bytes,
        owner_unset: false,
    };

    match item.action {
        RestoreAction::Unchanged => base(RestoreOutcomeKind::Unchanged, None),
        RestoreAction::Skipped => base(
            RestoreOutcomeKind::Skipped,
            item.reason.map(|r| r.word().to_string()),
        ),
        RestoreAction::WouldCreate | RestoreAction::WouldOverwrite => {
            let creating = item.action == RestoreAction::WouldCreate;
            match write_back(captured, store) {
                Ok(owner_unset) => RestoreResult {
                    owner_unset,
                    ..base(
                        if creating {
                            RestoreOutcomeKind::Created
                        } else {
                            RestoreOutcomeKind::Restored
                        },
                        None,
                    )
                },
                Err(e) => base(RestoreOutcomeKind::Failed, Some(e)),
            }
        }
    }
}

/// R4: write one captured file back to its absolute path atomically. Reads the
/// object, writes a temp file in the SAME directory, sets the captured mode,
/// best-effort sets uid/gid, then renames over the target. Returns whether owner
/// could not be set (content+mode still landed). On any hard failure returns a
/// human reason string.
fn write_back(captured: &CaptureEntry, store: &Store) -> Result<bool, String> {
    let path = Path::new(&captured.path);
    let hash = captured.hash.as_deref().ok_or("no stored content")?;

    // Defensive: object could vanish between plan and apply (TOCTOU).
    let bytes = store
        .read_object(hash)
        .map_err(|_| "stored object is missing".to_string())?;

    let parent = path
        .parent()
        .ok_or_else(|| "target has no parent directory".to_string())?;
    if !parent.is_dir() {
        return Err(format!(
            "target directory {} does not exist",
            parent.display()
        ));
    }

    // Temp file colocated with the target so the rename stays same-fs (atomic).
    let tmp = parent.join(format!(
        ".rewind-restore-{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));

    let write = || -> std::io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("write failed: {e}"));
    }

    // Captured mode, parsed from the 4-digit octal string (e.g. "0644").
    if let Some(mode) = captured.mode.as_deref().and_then(parse_mode) {
        if let Err(e) = fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("set mode failed: {e}"));
        }
    }

    // Best-effort uid/gid: a non-owner run can't chown -> note it, keep going.
    let owner_unset = match (captured.uid, captured.gid) {
        (Some(uid), Some(gid)) => util::set_owner(&tmp, uid, gid).is_err(),
        _ => false,
    };

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename into place failed: {e}"));
    }
    Ok(owner_unset)
}

/// Parse an octal mode string like `0644` into permission bits.
fn parse_mode(s: &str) -> Option<u32> {
    u32::from_str_radix(s, 8).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store_in(dir: &Path) -> Store {
        let s = Store::open(dir.to_path_buf());
        s.ensure_dirs().unwrap();
        s
    }

    /// Capture a set of real files into the store and return the manifest.
    fn capture_files(store: &Store, paths: &[PathBuf], at: &str) -> Manifest {
        let specs = paths.iter().cloned().map(CaptureSpec::new).collect();
        let set = CaptureSet {
            specs,
            source: SetSource::Cli,
        };
        crate::capture::capture(&set, store, at, None).unwrap()
    }

    #[test]
    fn plan_dry_run_writes_nothing() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("a.json");
        fs::write(&f, b"v1").unwrap();
        let m = capture_files(&store, std::slice::from_ref(&f), "2026-06-19T00:00:00Z");

        let bytes_before = store.store_bytes();
        fs::write(&f, b"v2-changed").unwrap(); // live now differs
        let p = plan(&m, &store);

        assert_eq!(p.would_change, 1);
        assert_eq!(p.items[0].action, RestoreAction::WouldOverwrite);
        // Live file is untouched by planning, and no new objects were written.
        assert_eq!(fs::read(&f).unwrap(), b"v2-changed");
        assert_eq!(store.store_bytes(), bytes_before);
    }

    #[test]
    fn plan_classifies_create_overwrite_unchanged() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"aaa").unwrap();
        fs::write(&b, b"bbb").unwrap();
        let m = capture_files(&store, &[a.clone(), b.clone()], "2026-06-19T00:00:00Z");

        fs::write(&a, b"aaa-changed").unwrap(); // overwrite
        fs::remove_file(&b).unwrap(); // create
        let p = plan(&m, &store);

        let by = |path: &Path| {
            p.items
                .iter()
                .find(|i| i.path == path.to_string_lossy())
                .unwrap()
        };
        assert_eq!(by(&a).action, RestoreAction::WouldOverwrite);
        assert_eq!(by(&b).action, RestoreAction::WouldCreate);
        assert_eq!(p.would_change, 2);
    }

    #[test]
    fn apply_overwrite_restores_content_atomically() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("snap.json");
        fs::write(&f, b"original").unwrap();
        let m = capture_files(&store, std::slice::from_ref(&f), "2026-06-19T00:00:00Z");

        fs::write(&f, b"corrupted-now").unwrap();
        let out = apply(&m, &store, true, "2026-06-19T01:00:00Z").unwrap();

        assert_eq!(fs::read(&f).unwrap(), b"original", "content restored");
        assert_eq!(out.restored, 1);
        assert_eq!(out.failed, 0);
        assert!(!out.has_failure());
        // R3: a safety capture of the pre-restore live state was taken.
        assert!(out.safety_capture.is_some());
    }

    #[test]
    fn apply_create_writes_a_missing_file() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("gone.json");
        fs::write(&f, b"content").unwrap();
        let m = capture_files(&store, std::slice::from_ref(&f), "2026-06-19T00:00:00Z");

        fs::remove_file(&f).unwrap();
        let out = apply(&m, &store, true, "2026-06-19T01:00:00Z").unwrap();
        assert!(f.exists());
        assert_eq!(fs::read(&f).unwrap(), b"content");
        assert_eq!(out.restored, 1);
    }

    #[test]
    fn apply_preserves_captured_mode() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("script.sh");
        fs::write(&f, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o750)).unwrap();
        let m = capture_files(&store, std::slice::from_ref(&f), "2026-06-19T00:00:00Z");

        fs::write(&f, b"tampered").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o600)).unwrap();
        apply(&m, &store, false, "2026-06-19T01:00:00Z").unwrap();

        let mode = fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o750, "captured mode restored");
    }

    #[test]
    fn apply_no_safety_capture_skips_only_the_capture() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("a");
        fs::write(&f, b"orig").unwrap();
        let m = capture_files(&store, std::slice::from_ref(&f), "2026-06-19T00:00:00Z");
        let count_before = store.load_manifests().unwrap().len();

        fs::write(&f, b"changed").unwrap();
        let out = apply(&m, &store, false, "2026-06-19T01:00:00Z").unwrap();

        assert!(out.safety_capture.is_none());
        // No new capture was recorded, but the restore still happened.
        assert_eq!(store.load_manifests().unwrap().len(), count_before);
        assert_eq!(fs::read(&f).unwrap(), b"orig");
    }

    #[test]
    fn missing_object_is_skipped_not_a_hard_error() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("a");
        fs::write(&f, b"data").unwrap();
        let m = capture_files(&store, std::slice::from_ref(&f), "2026-06-19T00:00:00Z");

        // Delete the backing object behind the capture's back.
        let hash = m.entries[0].hash.clone().unwrap();
        store.remove_object(&hash).unwrap();
        fs::write(&f, b"changed").unwrap();

        let p = plan(&m, &store);
        assert_eq!(p.items[0].action, RestoreAction::Skipped);
        assert_eq!(p.items[0].reason, Some(SkipReason::MissingObject));

        let out = apply(&m, &store, false, "2026-06-19T01:00:00Z").unwrap();
        assert_eq!(out.skipped, 1);
        assert_eq!(out.failed, 0);
        // The live file was left untouched (still "changed").
        assert_eq!(fs::read(&f).unwrap(), b"changed");
    }

    #[test]
    fn one_path_failure_yields_exit_2_semantics_without_aborting() {
        // Two paths: one restorable, one whose parent dir is removed so its write
        // fails. The good one must still restore; failed>0 drives exit 2.
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let good = dir.path().join("good.json");
        let subdir = dir.path().join("sub");
        fs::create_dir(&subdir).unwrap();
        let bad = subdir.join("bad.json");
        fs::write(&good, b"good-orig").unwrap();
        fs::write(&bad, b"bad-orig").unwrap();
        let m = capture_files(&store, &[good.clone(), bad.clone()], "2026-06-19T00:00:00Z");

        // Change both, then remove the bad one's parent dir so its restore fails.
        fs::write(&good, b"good-changed").unwrap();
        fs::remove_file(&bad).unwrap();
        fs::remove_dir(&subdir).unwrap();

        let out = apply(&m, &store, false, "2026-06-19T01:00:00Z").unwrap();
        assert!(out.has_failure(), "a failed path -> exit 2");
        assert_eq!(out.failed, 1);
        assert_eq!(out.restored, 1, "the good path still restored (no abort)");
        assert_eq!(fs::read(&good).unwrap(), b"good-orig");
    }

    #[test]
    fn schema_downgrade_is_flagged_in_the_plan() {
        let dir = tempdir().unwrap();
        let store = store_in(&dir.path().join("store"));
        let f = dir.path().join("workstate.snapshot.json");
        // Capture an OLD schema (v3).
        fs::write(&f, br#"{"schema_version":3,"source_tool":"workstate"}"#).unwrap();
        let m = capture_files(&store, std::slice::from_ref(&f), "2026-06-19T00:00:00Z");

        // Live is now a NEWER schema (v5).
        fs::write(&f, br#"{"schema_version":5,"source_tool":"workstate"}"#).unwrap();
        let p = plan(&m, &store);

        assert_eq!(p.schema_downgrades, 1);
        assert!(p.items[0].schema_downgrade);
    }

    #[test]
    fn parse_mode_reads_octal() {
        assert_eq!(parse_mode("0644"), Some(0o644));
        assert_eq!(parse_mode("0755"), Some(0o755));
        assert_eq!(parse_mode("0000"), Some(0));
    }
}

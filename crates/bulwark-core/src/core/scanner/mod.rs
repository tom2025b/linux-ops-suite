//! Directory scanner for Bulwark.
//!
//! This module implements the read-only directory walker that discovers user tools,
//! scripts, and files according to the loaded `Config`.
//!
//! Core guarantees (enforced by Bulwark Architect):
//! - **Read-only**: We never execute, modify, delete, or even open files for writing.
//! - **Deterministic**: `scan()` always returns the same `Vec<DiscoveredFile>` sorted
//!   by absolute path for the same inputs and filesystem state.
//! - **Non-fatal errors**: Permission errors, missing directories, or unreadable entries
//!   are skipped. The scan continues and returns whatever it could discover.
//! - **Respect for config**: `scan.paths`, `max_depth`, `follow_symlinks`, and
//!   `ignore.names` are all honored exactly.
//!
//! The public surface is intentionally tiny:
//! - `DiscoveredFile` — the data type consumers (engine, reports) use
//! - `scan(config)` — the one function that runs a full configured scan
//!
//! This file is deliberately kept small and focused. When it grows near the 400-line
//! limit we will split (e.g. into `types.rs` + `walker.rs`).

use std::path::PathBuf;

use serde::Serialize;
use walkdir::{DirEntry, WalkDir};

use crate::core::config::Config;
use crate::error::BulwarkError;

#[cfg(test)]
mod tests;

/// A single file discovered during a scan.
///
/// This struct is intentionally minimal for the MVP. It contains the information
/// needed for later classification, risk scoring, and reporting.
///
/// It is a **canonical model type** — re-exported via `core::model::DiscoveredFile`.
///
/// All paths are absolute. The struct derives `Ord` so that `sort()` produces
/// deterministic output (sorted by `path`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DiscoveredFile {
    /// Absolute path to the file on disk.
    pub path: PathBuf,

    /// Size in bytes (from metadata).
    pub size: u64,

    /// True if the file has any execute bit set (on Unix).
    /// Always false on non-Unix platforms for the MVP.
    pub is_executable: bool,
}

/// A non-fatal problem encountered during a scan.
///
/// Bulwark never aborts a scan because of one bad entry (a permission-denied
/// directory, a broken symlink, unreadable metadata, …). Instead it records a
/// `ScanWarning` and keeps going, so the user still gets a usable inventory
/// *plus* an honest account of what could not be inspected. This is the
/// guard-rail "prefer collecting warnings over crashing" made concrete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanWarning {
    /// The path the warning is about, when one is known (e.g. the directory we
    /// could not read). `None` for warnings not tied to a specific entry.
    pub path: Option<PathBuf>,

    /// A short, human-readable explanation of what went wrong.
    pub message: String,
}

/// The full result of a scan: the discovered files plus any non-fatal warnings.
///
/// `scan` returns this struct rather than a bare `Vec<DiscoveredFile>` so that
/// callers can surface partial-scan problems (permission errors, etc.) instead
/// of silently presenting an incomplete inventory as if it were complete.
#[derive(Debug, Default)]
pub struct ScanOutcome {
    /// Every regular file discovered, sorted by absolute path (determinism).
    pub files: Vec<DiscoveredFile>,

    /// Non-fatal problems encountered while walking. Empty on a clean scan.
    pub warnings: Vec<ScanWarning>,
}

/// Run a complete scan using the supplied configuration.
///
/// This is the primary entry point for Layer 2 and all higher layers.
///
/// Behavior:
/// - Expands and walks every path in `config.scan.paths`
/// - Respects `max_depth` and `follow_symlinks`
/// - Skips any directory or file whose name exactly matches an entry in `ignore.names`
/// - Returns only regular files (directories are never included in the result)
/// - **Always** sorts the final list by absolute path before returning
/// - Missing roots or permission errors on individual entries are non-fatal
///
/// Returns `Ok(outcome)` even if some roots were skipped, as long as the overall
/// operation did not have a fatal problem. Non-fatal issues (a missing root, an
/// unreadable directory) are recorded in `outcome.warnings` rather than dropped.
pub fn scan(config: &Config) -> Result<ScanOutcome, BulwarkError> {
    let roots = config.resolved_scan_paths()?;

    // We use a HashSet for ignore names because lookup is O(1) on average.
    // This matters when walking very large directory trees.
    let ignore_names: std::collections::HashSet<&str> =
        config.ignore.names.iter().map(|s| s.as_str()).collect();

    let mut discovered = Vec::new();
    let mut warnings = Vec::new();

    for root in roots {
        // Non-fatal: if a configured root does not exist, just skip it.
        // This matches real-world usage where users have optional directories
        // (e.g. ~/tools on a machine that doesn't have that folder). The CLI
        // also warns about missing roots up front; we do not duplicate that
        // here to avoid double-reporting the same condition.
        if !root.exists() {
            continue;
        }

        // A configured root that exists but is not a directory (e.g. the user
        // pointed `bulwark scan` at a regular file) is non-fatal, like a missing
        // root: we record a warning and skip it rather than aborting the whole
        // scan. Crashing here would turn a single fat-fingered path argument into
        // a hard failure for an otherwise-valid multi-path scan.
        if !root.is_dir() {
            warnings.push(ScanWarning {
                path: Some(root.clone()),
                message: "scan path is not a directory, skipping".to_owned(),
            });
            continue;
        }

        // WalkDir is from the `walkdir` crate. It is an iterator-based directory
        // walker that is much more ergonomic and efficient than manual recursion.
        //
        // Key design choices here:
        // - `filter_entry` lets us prune entire subtrees *before* descending.
        //   This is much cheaper than visiting every file and then ignoring it.
        // - We respect `max_depth` and `follow_symlinks` directly from config.
        let walker = WalkDir::new(&root)
            .follow_links(config.scan.follow_symlinks)
            .max_depth(config.scan.max_depth)
            .into_iter()
            .filter_entry(|e| {
                // Prune ignored directories before we even descend into them.
                // This is the correct and efficient way to implement ignore with WalkDir.
                if e.file_type().is_dir() {
                    !should_skip(e, &ignore_names)
                } else {
                    true
                }
            });

        for entry_result in walker {
            match entry_result {
                Ok(entry) => {
                    // For files we still check should_skip (a file can be ignored
                    // even if its parent directory was not pruned).
                    if should_skip(&entry, &ignore_names) {
                        continue;
                    }

                    // We only care about regular files for the MVP inventory.
                    // Directories, symlinks, sockets, etc. are skipped.
                    if entry.file_type().is_file() {
                        match file_from_entry(&entry) {
                            Ok(file) => discovered.push(file),
                            // Non-fatal per guard rails, but NEVER silent: a file
                            // we discovered yet cannot stat (concurrent delete,
                            // permission flip, transient FS error) must be reported
                            // so the inventory is honestly partial instead of
                            // appearing complete while quietly missing entries.
                            Err(message) => warnings.push(ScanWarning {
                                path: Some(entry.path().to_path_buf()),
                                message,
                            }),
                        }
                    }
                }
                Err(e) => {
                    // Non-fatal per guard rails: one bad entry (permission denied,
                    // broken symlink, etc.) should not kill the entire scan. We
                    // record a warning so the user knows the inventory is partial,
                    // then keep going.
                    //
                    // `walkdir::Error` exposes the path it failed on via `.path()`
                    // (Some for most I/O errors, None for things like loops), which
                    // we capture so the warning is actionable.
                    warnings.push(ScanWarning {
                        path: e.path().map(|p| p.to_path_buf()),
                        message: e.to_string(),
                    });
                }
            }
        }
    }

    // Critical guarantee: deterministic output.
    // Because `DiscoveredFile` implements `Ord` (based on `path`), this sort
    // is always stable and produces identical results for the same inputs.
    //
    // INVARIANT: After this point, the vector is sorted by absolute path and
    // will remain so for the lifetime of this `scan()` call.
    discovered.sort();

    Ok(ScanOutcome {
        files: discovered,
        warnings,
    })
}

/// Decide whether this entry (file or dir) should be completely ignored.
///
/// We only look at the *basename* (the last component of the path), not the
/// full path. This matches common ignore patterns like ".git", "target", etc.
fn should_skip(entry: &DirEntry, ignore_names: &std::collections::HashSet<&str>) -> bool {
    if let Some(name) = entry.file_name().to_str()
        && ignore_names.contains(name)
    {
        return true;
    }
    false
}

/// Convert a `DirEntry` that we know is a file into a `DiscoveredFile`.
///
/// Returns `Err(message)` if we cannot read the file's metadata. The caller
/// turns that into a non-fatal [`ScanWarning`] so the failure is surfaced (the
/// inventory is reported as partial) rather than the file being dropped
/// silently. This is where we learn the size and executable bit for the file.
fn file_from_entry(entry: &DirEntry) -> Result<DiscoveredFile, String> {
    let metadata = entry
        .metadata()
        .map_err(|e| format!("failed to read file metadata: {e}"))?;

    let is_executable = is_executable(&metadata);

    Ok(DiscoveredFile {
        path: entry.path().to_path_buf(),
        size: metadata.len(),
        is_executable,
    })
}

/// Return whether the file has any execute permission bits set.
///
/// On Unix this checks the owner/group/other execute bits (the classic
/// `chmod +x` bits).
///
/// On other platforms we conservatively return `false`. This keeps the
/// rest of the system simple — on Windows, for example, executability is
/// determined by file extension rather than permission bits.
#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode();
    mode & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

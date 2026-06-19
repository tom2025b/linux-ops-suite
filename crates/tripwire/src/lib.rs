//! tripwire — a read-only file-integrity tripwire for the Linux Ops Suite.
//!
//! Where portman watches the *network* surface (what is listening, and why),
//! tripwire watches the *filesystem* surface: it records a [`baseline`] — a
//! SHA-256 + metadata snapshot of a configured set of files and directories —
//! and later [`diff`](baseline::diff)s the live filesystem against it, reporting
//! what was added, removed, modified, or re-permissioned. "Monitoring" is one
//! command in cron: `tripwire diff` (or the quieter `tripwire verify`) exits 1
//! the moment a watched path drifts.
//!
//! It degrades gracefully — an unreadable file (e.g. `/etc/shadow` as non-root)
//! is recorded as metadata with an `unreadable` flag, never an error — and stays
//! lean: the only file it ever writes is its own baseline, and the only
//! dependencies are the CLI parser and serde (SHA-256 and the directory walk are
//! hand-rolled). The library does the work and returns values; the binary
//! ([`main`](../main/index.html)) only parses flags and renders.

pub mod baseline;
pub mod error;
pub mod hash;
pub mod model;
pub mod report;
pub mod scan;
pub mod util;
pub mod watch;

use std::path::{Path, PathBuf};

use baseline::{Baseline, Diff};
use scan::Scan;
use watch::WatchSet;

pub use error::TripwireError;

/// Resolve the watch set (CLI `--path` flags, else a config file, else the
/// built-in default set) and scan it into the current view. `ignore` omits one
/// path from the result — tripwire's own baseline file, so it never shows up as
/// drift when it happens to live inside a watched directory.
pub fn current(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
    ignore: Option<&Path>,
) -> Result<Scan, TripwireError> {
    scan::scan(cli_paths, config_override, ignore)
}

/// Resolve the watch set without scanning — for `tripwire watch`.
pub fn watch_set(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
) -> Result<WatchSet, TripwireError> {
    watch::resolve(cli_paths, config_override)
}

/// Record the current scan as the baseline at the suite's default path (or
/// `path_override`), returning the path written and the entry count for the
/// caller to report.
pub fn save_baseline(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
    path_override: Option<PathBuf>,
) -> Result<(PathBuf, usize), TripwireError> {
    let path = resolve_baseline_path(path_override)?;
    let scan = current(cli_paths, config_override, Some(&path))?;
    let count = scan.entries.len();
    Baseline::from_scan(scan.entries, scan.source.tag()).save(&path)?;
    Ok((path, count))
}

/// Compute the diff of a live scan against the recorded baseline. Returns the
/// diff and the baseline path it compared against (for the caller's header).
///
/// The watch set scanned live is the same precedence as everywhere else; the
/// baseline supplies the "before" side. A path that's in the baseline but no
/// longer present is a `removed` change — exactly the drift the tool exists to
/// catch — so the live scan does not need to know the baseline's watch set.
pub fn diff_against_baseline(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
    path_override: Option<PathBuf>,
) -> Result<(Diff, PathBuf), TripwireError> {
    let path = resolve_baseline_path(path_override)?;
    let recorded = Baseline::load(&path)?;
    let live = current(cli_paths, config_override, Some(&path))?;
    Ok((baseline::diff(&recorded.entries, &live.entries), path))
}

/// Resolve the baseline path: an explicit `--baseline-file` wins; otherwise the
/// suite's XDG data location. Errors only when no anchor dir can be found.
fn resolve_baseline_path(path_override: Option<PathBuf>) -> Result<PathBuf, TripwireError> {
    match path_override {
        Some(p) => Ok(p),
        None => util::baseline_path().ok_or(TripwireError::NoDataDir),
    }
}

/// Whether a baseline already exists at the default-or-given path. Lets the CLI
/// warn before overwriting, without loading the file.
pub fn baseline_exists(path_override: Option<&Path>) -> bool {
    let path = match path_override {
        Some(p) => p.to_path_buf(),
        None => match util::baseline_path() {
            Some(p) => p,
            None => return false,
        },
    };
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_baseline_path_honors_override() {
        let custom = PathBuf::from("/tmp/tripwire-test-baseline.json");
        assert_eq!(resolve_baseline_path(Some(custom.clone())).unwrap(), custom);
    }

    #[test]
    fn baseline_then_diff_roundtrip_is_clean_then_dirty() {
        let dir = tempdir().unwrap();
        let watched = dir.path().join("watched.txt");
        std::fs::write(&watched, b"original").unwrap();
        let baseline_file = dir.path().join("baseline.json");
        let cli = vec![watched.clone()];

        // Record.
        let (saved, count) = save_baseline(&cli, None, Some(baseline_file.clone())).unwrap();
        assert_eq!(saved, baseline_file);
        assert_eq!(count, 1);

        // Unchanged -> clean.
        let (clean, _) = diff_against_baseline(&cli, None, Some(baseline_file.clone())).unwrap();
        assert!(clean.is_clean());

        // Change the content -> dirty.
        std::fs::write(&watched, b"tampered").unwrap();
        let (dirty, _) = diff_against_baseline(&cli, None, Some(baseline_file)).unwrap();
        assert!(!dirty.is_clean());
        assert_eq!(dirty.tally(), (0, 0, 1));
    }

    #[test]
    fn diff_without_baseline_is_no_baseline_error() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("none.json");
        let err = diff_against_baseline(&[dir.path().join("x")], None, Some(missing)).unwrap_err();
        assert!(matches!(err, TripwireError::NoBaseline { .. }));
    }
}

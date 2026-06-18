//! portman — answers "what is listening on this machine, and why?"
//!
//! Where rexops shows the live cockpit and rex-doctor verifies the install,
//! portman owns one focused question: every listening socket on the host, and
//! the full ownership chain behind each one — socket -> PID -> process ->
//! systemd unit -> package. It reads `/proc` directly (no `ss`/`netstat`),
//! degrades gracefully without root (sockets are always listed; owners fill in
//! as far as privileges allow), and can record a [`baseline`] to [`diff`]
//! against later so "what changed since I last looked?" is one command.
//!
//! The library does the work and returns values; the binary ([`main`]) only
//! parses flags and renders. Output is either an aligned human table or a JSON
//! envelope shaped like the rest of the suite's feeds.

pub mod baseline;
pub mod error;
pub mod model;
pub mod report;
pub mod scan;
pub mod util;

use std::path::{Path, PathBuf};

use baseline::{Baseline, Diff};
use model::Listener;

pub use error::PortmanError;

/// Scan the host and return every listening socket with its ownership chain.
/// Thin re-export of [`scan::scan`] so callers depend on the crate root.
pub fn current() -> Result<Vec<Listener>, PortmanError> {
    scan::scan()
}

/// Record the current listeners as the baseline at the suite's default path
/// (or `path_override`), returning the path written for the caller to report.
pub fn save_baseline(path_override: Option<PathBuf>) -> Result<PathBuf, PortmanError> {
    let path = resolve_baseline_path(path_override)?;
    let listeners = current()?;
    Baseline::from_scan(listeners).save(&path)?;
    Ok(path)
}

/// Compute the diff of the live scan against the recorded baseline. Returns the
/// diff and the baseline path it compared against (for the caller's header).
pub fn diff_against_baseline(
    path_override: Option<PathBuf>,
) -> Result<(Diff, PathBuf), PortmanError> {
    let path = resolve_baseline_path(path_override)?;
    let recorded = Baseline::load(&path)?;
    let live = current()?;
    Ok((baseline::diff(&recorded.listeners, &live), path))
}

/// Resolve the baseline path: an explicit `--baseline-file` wins; otherwise the
/// suite's XDG data location. Errors only when no anchor dir can be found.
fn resolve_baseline_path(path_override: Option<PathBuf>) -> Result<PathBuf, PortmanError> {
    match path_override {
        Some(p) => Ok(p),
        None => util::baseline_path().ok_or(PortmanError::NoDataDir),
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

    #[test]
    fn resolve_baseline_path_honors_override() {
        let custom = PathBuf::from("/tmp/portman-test-baseline.json");
        let resolved = resolve_baseline_path(Some(custom.clone())).unwrap();
        assert_eq!(resolved, custom);
    }

    #[test]
    fn current_scan_runs_without_panicking() {
        // On Linux this returns the host's listeners; the contract is only that
        // it doesn't panic and returns Ok on a normal /proc.
        let result = current();
        assert!(result.is_ok() || matches!(result, Err(PortmanError::NoProc { .. })));
    }
}

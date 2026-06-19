//! Launching the RexOps cockpit from inside Pulse.
//!
//! Pulse is the suite's default status screen (a bare `rexops` opens it); the
//! `r` key is the way back out to the full cockpit — the launcher/jobs interface
//! — via `rexops tui`. Pulse stays dependency-free, so resolving `rexops` is a
//! tiny hand-rolled PATH walk (the same approach rex-check/portman use instead of
//! a `which` crate), and the actual foreground hand-off (suspend Pulse's raw
//! mode, run the child, restore) is owned by [`crate::tui::RawMode::suspend`].
//!
//! Nothing here is an error in the "Pulse failed" sense: a missing `rexops` is
//! reported back to the UI as a short status line, never a crash or a non-zero
//! exit — the same graceful-degradation rule the rest of the suite follows.

use std::path::PathBuf;
use std::process::Command;

use crate::tui::RawMode;

/// The id of the cockpit binary and the subcommand that opens it. `rexops tui`
/// is the explicit cockpit entry point added in the RexOps side of this change
/// (a bare `rexops` now opens Pulse, so we must ask for the cockpit by name).
const REXOPS_BIN: &str = "rexops";
const COCKPIT_ARGS: &[&str] = &["tui"];

/// Outcome of pressing `r`, surfaced to the user as a transient status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// The cockpit ran and exited (cleanly or not); Pulse simply resumes.
    Returned,
    /// `rexops` wasn't found on PATH — tell the user how to get it.
    NotFound,
    /// The cockpit was found but launching/!running it failed (rare).
    Failed(String),
}

impl LaunchOutcome {
    /// The status line Pulse shows after the attempt. `Returned` shows nothing
    /// (the user is simply back); the other two explain what happened.
    pub fn status_line(&self) -> Option<String> {
        match self {
            LaunchOutcome::Returned => None,
            LaunchOutcome::NotFound => Some(
                "rexops not found on PATH — install it to open the cockpit (or run `rexops tui`)."
                    .to_string(),
            ),
            LaunchOutcome::Failed(detail) => Some(format!("could not open cockpit: {detail}")),
        }
    }
}

/// Resolve `rexops` on PATH and, if found, foreground-launch `rexops tui` using
/// `raw` to suspend/restore Pulse's terminal around the child. Returns a
/// [`LaunchOutcome`] describing what to tell the user; it never returns `Err`
/// for "rexops isn't installed" — that's `NotFound`, a normal state.
pub fn open(raw: &mut RawMode) -> LaunchOutcome {
    let Some(program) = resolve_on_path(REXOPS_BIN) else {
        return LaunchOutcome::NotFound;
    };

    let run = raw.suspend(|| {
        Command::new(&program)
            .args(COCKPIT_ARGS)
            .status()
            .map(|_| ())
    });

    match run {
        Ok(()) => LaunchOutcome::Returned,
        Err(e) => LaunchOutcome::Failed(e.to_string()),
    }
}

/// Find an executable named `bin` by walking `$PATH`, returning the first match
/// that exists. Dependency-free (no `which` crate); mirrors how the other lean
/// suite tools resolve a sibling binary.
fn resolve_on_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returned_shows_no_status_line() {
        assert_eq!(LaunchOutcome::Returned.status_line(), None);
    }

    #[test]
    fn not_found_explains_how_to_get_rexops() {
        let line = LaunchOutcome::NotFound.status_line().expect("a line");
        assert!(line.contains("rexops"));
        assert!(line.contains("PATH"));
    }

    #[test]
    fn failed_includes_the_detail() {
        let line = LaunchOutcome::Failed("boom".into())
            .status_line()
            .expect("a line");
        assert!(line.contains("boom"));
    }

    #[test]
    fn resolve_finds_a_known_binary_and_misses_a_bogus_one() {
        // `sh` exists on any POSIX box; a random name does not. This exercises
        // the PATH walk without assuming a specific location.
        assert!(resolve_on_path("sh").is_some());
        assert!(resolve_on_path("definitely-not-a-real-binary-xyz123").is_none());
    }
}

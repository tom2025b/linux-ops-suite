//! Launching the RexOps cockpit from inside Pulse.
//!
//! Pulse is the suite's default status screen (a bare `rexops` opens it); the
//! `r` key is the way back out to the full cockpit — the launcher/jobs interface
//! — via `rexops tui`. Resolving `rexops` is a tiny hand-rolled PATH walk (the
//! same approach rex-check/portman use instead of a `which` crate), and the
//! actual foreground hand-off (leave Pulse's alt screen, run the child, re-enter)
//! is owned by [`suite_ui::Tui::suspended`].
//!
//! Nothing here is an error in the "Pulse failed" sense: a missing `rexops` is
//! reported back to the UI as a short status line, never a crash or a non-zero
//! exit — the same graceful-degradation rule the rest of the suite follows.

use std::path::PathBuf;
use std::process::Command;

use suite_ui::Tui;

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
/// the shared [`suite_ui::Tui`] guard to suspend/restore Pulse's terminal around
/// the child. Returns a [`LaunchOutcome`] describing what to tell the user; it
/// never returns `Err` for "rexops isn't installed" — that's `NotFound`, a normal
/// state.
///
/// `Tui::suspended` gives the same guarantee the old `RawMode::suspend` did —
/// Pulse's terminal is re-entered even if the child errors, so we are never left
/// half-suspended — and additionally drains any input the child left buffered.
pub fn open(tui: &mut Tui) -> LaunchOutcome {
    let Some(program) = resolve_on_path(REXOPS_BIN) else {
        return LaunchOutcome::NotFound;
    };

    // `suspended` runs the child closure on the real terminal and returns
    // io::Result<closure-return>. The closure itself runs the command and yields
    // io::Result<()>, so flatten the two error layers: a failed leave/re-enter
    // (outer) and a failed spawn (inner) both become `Failed`.
    let run = tui
        .suspended(|| {
            Command::new(&program)
                .args(COCKPIT_ARGS)
                .status()
                .map(|_| ())
        })
        .and_then(|inner| inner);

    match run {
        Ok(()) => LaunchOutcome::Returned,
        Err(e) => LaunchOutcome::Failed(e.to_string()),
    }
}

/// Find an executable named `bin` by walking `$PATH`, returning the first
/// executable match. Delegated to suite-core, which checks the execute bit (the
/// old local copy only tested `is_file()`, so a non-executable file shadowing
/// the name could be wrongly "found").
fn resolve_on_path(bin: &str) -> Option<PathBuf> {
    suite_core::path::resolve_on_path(bin)
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

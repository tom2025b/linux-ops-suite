//! Where an adapter's raw feed bytes come from.
//!
//! Every adapter's job splits cleanly into "get the raw JSON text" and "parse +
//! normalize it". The parse/normalize half is identical regardless of source, so
//! the ONLY thing that varies is the transport. This module is that seam: a
//! [`FeedTransport`] yields the raw text (or a typed [`FeedError`]) and the
//! adapter hands it to its pure `parse`.
//!
//! Two transports, one contract:
//!   * [`FeedTransport::File`] — read a file (the original behavior; kept for
//!     tests, `--output` files, and the `OUTPUT` override).
//!   * [`FeedTransport::Command`] — spawn a tool and capture its stdout. This is
//!     how Workstate goes LIVE: it runs the real producer (`bulwark workstate-feed`,
//!     `toolfoundry workstate-feed`, `scriptvault workstate-feed`) each build, so
//!     the snapshot reflects current state instead of a committed fixture.
//!
//! ERROR MAPPING IS THE CONTRACT (it drives graceful degradation, exactly as the
//! file path did — see `compile_section`):
//!   * source absent  → [`FeedError::NotFound`] → compiler marks the section **Missing**
//!       - File: the path does not exist.
//!       - Command: the program is not found on `$PATH` (tool not installed yet).
//!         An uninstalled tool is the normal not-yet-here case, NOT a failure.
//!   * other problem  → [`FeedError::Io`]/[`FeedError::Parse`] → marks it **Failed**
//!       - File: a permissions/IO error.
//!       - Command: it ran but exited non-zero, or its stdout was not valid UTF-8.
//!         The tool IS present but misbehaved — a real, surfaced failure.
//!
//! Workstate stays READ-ONLY by construction: the only commands it ever runs are
//! the fixed producer argv the caller wires in `main.rs`; there is no shell, no
//! interpolation, and no user-supplied program — args are passed as a vector, so
//! nothing is word-split or globbed.

use std::io::ErrorKind;
use std::process::Command;

use crate::ingest::FeedError;

/// The source of one feed's raw bytes. Owned (`String`s) because a transport is
/// held by a long-lived adapter inside the `SnapshotBuilder`.
#[derive(Debug, Clone)]
pub enum FeedTransport {
    /// Read the feed from a file at this path.
    File(String),
    /// Run this program with these args and capture stdout as the feed.
    Command {
        /// The program to run (resolved via `$PATH`). A program not found on
        /// `$PATH` degrades to a Missing section, never a panic.
        program: String,
        /// Arguments, passed as a vector (NOT a shell string): no word-splitting,
        /// no globbing, no interpolation. This is what keeps spawning safe.
        args: Vec<String>,
    },
}

impl FeedTransport {
    /// A command transport from a program plus borrowed args. Convenience for the
    /// fixed producer wiring in `main.rs`.
    pub fn command(program: &str, args: &[&str]) -> Self {
        FeedTransport::Command {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    /// A short label for diagnostics / `NotFound` payloads — the path, or the
    /// program name for a command. Used so a Missing/Failed section can say WHAT
    /// was missing without leaking the full argv.
    fn label(&self) -> String {
        match self {
            FeedTransport::File(path) => path.clone(),
            FeedTransport::Command { program, .. } => program.clone(),
        }
    }

    /// Obtain the raw feed text, mapping every failure onto the graceful-degradation
    /// contract above. The adapter calls this, then hands the `String` to its pure
    /// `parse`.
    pub fn read(&self) -> Result<String, FeedError> {
        match self {
            FeedTransport::File(path) => read_file(path),
            FeedTransport::Command { program, args } => run_command(program, args, &self.label()),
        }
    }
}

/// Read a feed file, mapping a missing file to `NotFound` (→ Missing) and any
/// other I/O problem to `Io` (→ Failed). This is the exact behavior the three
/// adapters previously inlined; centralizing it removes the triplication.
fn read_file(path: &str) -> Result<String, FeedError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        // The one case we promote to "Missing" rather than "Failed".
        Err(e) if e.kind() == ErrorKind::NotFound => Err(FeedError::NotFound(path.to_string())),
        // Any other I/O problem (permissions, etc.) is a genuine failure.
        Err(e) => Err(e.into()),
    }
}

/// Spawn `program args...`, capture stdout, and map failures onto the contract.
///
///   * program not on `$PATH` (`ErrorKind::NotFound`) → `NotFound` (→ Missing):
///     the tool simply is not installed yet, the graceful not-here case.
///   * any other spawn error                          → `Io` (→ Failed).
///   * ran but exited non-zero                        → `Parse` with the exit
///     status + a snippet of stderr (→ Failed): the tool is present but errored.
///   * stdout not valid UTF-8                          → `Parse` (→ Failed).
///
/// stderr is captured (not inherited) so a chatty producer cannot scribble over
/// Workstate's own summary; on a non-zero exit a trimmed snippet is folded into
/// the error so the failure is diagnosable.
fn run_command(program: &str, args: &[String], label: &str) -> Result<String, FeedError> {
    let output = match Command::new(program).args(args).output() {
        Ok(output) => output,
        // Program not found on PATH: the tool isn't installed → Missing, not Failed.
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(FeedError::NotFound(label.to_string()));
        }
        // Any other failure to even spawn (e.g. permission denied on the binary).
        Err(e) => return Err(e.into()),
    };

    if !output.status.success() {
        // Present but errored: a real failure. Carry the status and a short stderr
        // snippet so the Failed section's reason is actionable.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let snippet = stderr.trim();
        let snippet = if snippet.len() > 200 {
            &snippet[..200]
        } else {
            snippet
        };
        return Err(FeedError::Parse(format!(
            "`{program}` exited with {} - {}",
            output.status,
            if snippet.is_empty() {
                "(no stderr)"
            } else {
                snippet
            }
        )));
    }

    // Exit 0: stdout IS the feed. It must be valid UTF-8 (it's JSON text); if not,
    // the producer emitted something wrong — a Failed section, not a crash.
    String::from_utf8(output.stdout)
        .map_err(|e| FeedError::Parse(format!("`{program}` stdout was not valid UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_transport_reads_a_present_file() {
        let dir = std::env::temp_dir().join(format!(
            "ws-transport-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("feed.json");
        std::fs::write(&path, "{\"ok\":true}").unwrap();
        let t = FeedTransport::File(path.to_string_lossy().into_owned());
        assert_eq!(t.read().unwrap(), "{\"ok\":true}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_transport_missing_file_is_notfound_not_failed() {
        let t = FeedTransport::File("/no/such/feed/path.json".to_string());
        match t.read() {
            Err(FeedError::NotFound(p)) => assert!(p.contains("path.json")),
            other => panic!("expected NotFound for a missing file, got {other:?}"),
        }
    }

    #[test]
    fn command_transport_captures_stdout() {
        // `echo` is on every PATH and prints its args to stdout.
        let t = FeedTransport::command("echo", &["{\"live\":1}"]);
        assert_eq!(t.read().unwrap().trim(), "{\"live\":1}");
    }

    #[test]
    fn command_not_on_path_is_notfound_so_section_degrades_to_missing() {
        let t = FeedTransport::command("definitely-not-a-real-tool-xyzzy", &["workstate-feed"]);
        match t.read() {
            Err(FeedError::NotFound(name)) => {
                assert_eq!(name, "definitely-not-a-real-tool-xyzzy");
            }
            other => panic!("a missing program must be NotFound (→ Missing), got {other:?}"),
        }
    }

    #[test]
    fn command_nonzero_exit_is_a_failed_section_with_a_reason() {
        // `false` exits 1 with no stdout — the "tool present but errored" case.
        let t = FeedTransport::command("false", &[]);
        match t.read() {
            Err(FeedError::Parse(msg)) => assert!(msg.contains("exited")),
            other => panic!("a non-zero exit must be Parse (→ Failed), got {other:?}"),
        }
    }

    #[test]
    fn command_args_are_not_shell_interpreted() {
        // If this were run through a shell, `;` would split and `echo` would run
        // twice. Passed as argv, the whole thing is one literal argument to echo.
        let t = FeedTransport::command("echo", &["a; echo b"]);
        let out = t.read().unwrap();
        assert_eq!(out.trim(), "a; echo b", "args must not be shell-split");
    }
}

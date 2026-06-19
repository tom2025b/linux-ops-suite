//! conductor — the Linux Ops Suite's guided operator.
//!
//! Phase 1 (this build) is the Ring 0, read-only foundation: read the suite's
//! contract files, derive a deterministic ordered plan, and render it. The
//! library does the work and returns values; the binary only parses flags and
//! prints. See `CONDUCTOR_DESIGN.md` at the repo root.

pub mod error;
pub mod plan;
pub mod report;
pub mod sources;
pub mod state;
pub mod util;

pub use error::ConductorError;
use state::SuiteState;

/// Assemble the normalized suite state by running every fault-tolerant reader.
/// Pure aggregation: no rules, no rendering. Never fails — a missing feed just
/// yields fewer facts (resolving `DataDir` is the only fallible step, done by the
/// caller via [`sources::DataDir::from_env`]).
pub fn load_state(dir: &sources::DataDir) -> SuiteState {
    let (built_at, feeds) = sources::read_feeds(dir);
    SuiteState {
        built_at,
        feeds,
        findings: sources::read_findings(dir),
        drift: sources::read_drift(dir),
        failed_jobs: sources::read_failed_jobs(dir),
        binaries: sources::read_binaries(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn temp_root(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("conductor-loadstate-{tag}-{nanos}"));
        dir
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::File::create(p).unwrap().write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn load_state_aggregates_every_reader() {
        let root = temp_root("agg");
        write(
            &root,
            "rexops/feeds/workstate.snapshot.json",
            r#"{ "built_at":"2026-06-14T12:00:00Z", "tools": { "status": "Stale" } }"#,
        );
        write(
            &root,
            "rexops/snapshot.json",
            r#"{ "attention": [ { "tool":"bulwark","id":"x.sh","reason":"key","severity":"critical" } ] }"#,
        );
        let dir = sources::DataDir::new(root.clone());
        let s = load_state(&dir);
        assert_eq!(s.built_at.as_deref(), Some("2026-06-14T12:00:00Z"));
        assert!(s.has_stale_or_unavailable_feed());
        assert_eq!(s.findings.len(), 1);
        assert_eq!(s.binaries.len(), sources::SUITE_BINARIES.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_state_on_empty_root_is_all_empty_but_for_binaries() {
        let root = temp_root("empty");
        std::fs::create_dir_all(&root).unwrap();
        let dir = sources::DataDir::new(root.clone());
        let s = load_state(&dir);
        assert!(s.feeds.is_empty());
        assert!(s.findings.is_empty());
        assert!(s.failed_jobs.is_empty());
        assert!(s.drift.is_empty());
        // binaries are always probed (presence may vary by machine)
        assert_eq!(s.binaries.len(), sources::SUITE_BINARIES.len());
        let _ = std::fs::remove_dir_all(&root);
    }
}

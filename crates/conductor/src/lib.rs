//! conductor — the Linux Ops Suite's guided operator.
//!
//! Phase 1 (this build) is the Ring 0, read-only foundation: read the suite's
//! contract files, derive a deterministic ordered plan, and render it. The
//! library does the work and returns values; the binary only parses flags and
//! prints. See `CONDUCTOR_DESIGN.md` at the repo root.

pub mod error;
pub mod plan;
pub mod report;
pub mod run;
pub mod sources;
pub mod state;
pub mod tui;
pub mod util;

pub use error::ConductorError;

/// A process-wide lock shared by every test that mutates or reads process
/// environment variables (`$PATH`, `$HOME`, `$XDG_DATA_HOME`, …). The
/// environment is global mutable state and cargo runs tests in parallel within a
/// binary, so tests in different modules (`sources`, `run`, `tui`, `util`) that
/// touch it would otherwise race — one narrowing `$PATH` or clearing
/// `$XDG_DATA_HOME` while another reads it. Each such test holds this lock for
/// its critical section (set → assert → restore).
#[cfg(test)]
pub(crate) static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
use state::SuiteState;

/// Assemble the normalized suite state. Every snapshot-derived fact comes from a
/// SINGLE read of the canonical Workstate snapshot (via `workstate_schema`, the one
/// source of truth); the two non-snapshot inputs — binary presence on `$PATH` and
/// tripwire drift — are read alongside it. Pure aggregation: no rules, no
/// rendering. Never fails — a missing/unreadable snapshot just yields no facts
/// (resolving `DataDir` is the only fallible step, done by the caller via
/// [`sources::DataDir::from_env`]).
pub fn load_state(dir: &sources::DataDir) -> SuiteState {
    let snapshot = sources::load_snapshot(dir);
    SuiteState {
        built_at: snapshot.as_ref().map(sources::built_at_of),
        feeds: snapshot.as_ref().map(sources::feeds_of).unwrap_or_default(),
        findings: snapshot
            .as_ref()
            .map(sources::findings_of)
            .unwrap_or_default(),
        failed_jobs: snapshot
            .as_ref()
            .map(sources::failed_jobs_of)
            .unwrap_or_default(),
        // Non-snapshot inputs: tripwire drift isn't in the contract yet; binary
        // presence is live environment state, not something the snapshot carries.
        drift: sources::read_drift(dir),
        binaries: sources::read_binaries(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use workstate_schema::model::normalized::{Finding, FindingId, FindingInventory, Severity};
    use workstate_schema::model::provenance::{FeedId, FeedStatus, Provenance, Section};
    use workstate_schema::Snapshot;

    fn prov(feed: &str) -> Provenance {
        Provenance {
            feed_id: FeedId(feed.into()),
            fetched_at: None,
            source_observed_at: None,
            dropped_records: 0,
        }
    }

    /// A canonical v5 snapshot with a STALE tools feed and one Critical finding —
    /// enough to exercise load_state's whole composition (built_at + feeds +
    /// findings) from the ONE artifact.
    fn fixture() -> Snapshot {
        let mut tools = Section::missing(FeedId("toolfoundry".into()));
        tools.status = FeedStatus::Stale;
        let findings = Section {
            status: FeedStatus::Fresh,
            provenance: prov("bulwark"),
            data: Some(FindingInventory {
                generated_at: "2026-06-14T12:00:00Z".into(),
                findings: vec![Finding {
                    id: FindingId("x.sh".into()),
                    name: None,
                    rule_id: None,
                    description: Some("AWS key".into()),
                    severity: Severity::Critical,
                    raw_severity: None,
                    category: None,
                    location: "unknown".into(),
                    path: None,
                    risk: None,
                    owner: None,
                }],
                dropped_records: 0,
            }),
        };
        Snapshot::new(
            Utc.with_ymd_and_hms(2026, 6, 14, 12, 0, 0).unwrap(),
            Section::missing(FeedId("scriptvault".into())),
            tools,
            findings,
            Section::missing(FeedId("proto".into())),
        )
    }

    #[test]
    fn load_state_aggregates_the_single_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sources::DataDir::new(tmp.path().to_path_buf());
        workstate_schema::write_snapshot(&fixture(), &dir.snapshot()).unwrap();
        let s = load_state(&dir);
        assert_eq!(s.built_at.as_deref(), Some("2026-06-14T12:00:00+00:00"));
        assert!(s.has_stale_or_unavailable_feed());
        assert_eq!(s.findings.len(), 1);
        assert_eq!(s.findings[0].what, "x.sh");
        assert_eq!(s.binaries.len(), sources::SUITE_BINARIES.len());
    }

    #[test]
    fn load_state_on_empty_root_is_all_empty_but_for_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = sources::DataDir::new(tmp.path().to_path_buf());
        let s = load_state(&dir);
        // No snapshot ⇒ no snapshot-derived facts at all.
        assert!(s.feeds.is_empty());
        assert!(s.findings.is_empty());
        assert!(s.failed_jobs.is_empty());
        assert!(s.drift.is_empty());
        // binaries are always probed (presence may vary by machine)
        assert_eq!(s.binaries.len(), sources::SUITE_BINARIES.len());
    }
}

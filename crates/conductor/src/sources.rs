//! Reading the suite's state for conductor — through the ONE canonical Workstate
//! snapshot.
//!
//! Conductor is a passive reader (see CONDUCTOR_DESIGN.md): it consumes published
//! artifacts and never mutates a producer. The single source of truth for all
//! snapshot data is the canonical Workstate snapshot, read here through
//! `workstate_schema` (the shared contract) and nothing else — conductor defines
//! no snapshot model of its own, so the contract cannot drift and conductor tracks
//! the schema version automatically. One [`load_snapshot`] call yields every
//! snapshot-derived fact: feed freshness, findings, and failed jobs. A missing,
//! unreadable, or wrong-version snapshot resolves to "no facts" and never panics
//! (`docs/CONTRACT_RULES.md`).
//!
//! Two inputs are deliberately NOT snapshot data, so they stay separate:
//!   * binary presence on `$PATH` — live environment state, probed directly;
//!   * tripwire drift — tripwire has no feed in the snapshot contract yet, so it is
//!     read from its own file until it does (the one place this module still parses
//!     a producer file directly; it becomes a snapshot section later).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use workstate_schema::model::normalized::{
    Finding as WsFinding, JobOutcome, Severity as WsSeverity,
};
use workstate_schema::model::provenance::FeedStatus as WsFeedStatus;
use workstate_schema::Snapshot;

use crate::error::ConductorError;
use crate::state::{
    BinaryStatus, DriftedPath, FailedJob, FeedStatus, Finding, Freshness, Severity,
};
use crate::util;

/// The resolved on-disk layout conductor reads from. Anchored at the suite data
/// root (`$XDG_DATA_HOME`, fallback `~/.local/share`); `--data-dir` overrides it.
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Build from an explicit root (used by `--data-dir` and tests).
    pub fn new(root: PathBuf) -> Self {
        DataDir { root }
    }

    /// Resolve from the environment; `Err(NoDataDir)` when no root is usable.
    pub fn from_env() -> Result<Self, ConductorError> {
        util::data_root()
            .map(|root| DataDir { root })
            .ok_or(ConductorError::NoDataDir)
    }

    /// The ONE canonical Workstate snapshot. The path tail is defined by
    /// `workstate_schema` (`<root>/rexops/feeds/workstate.snapshot.json`), so
    /// conductor never re-spells where the snapshot lives — it reads exactly where
    /// the producer writes.
    pub fn snapshot(&self) -> PathBuf {
        workstate_schema::snapshot_path_under(&self.root)
    }

    /// Tripwire's drift file — the single suite input not yet carried by the
    /// snapshot contract (see [`read_drift`]). Becomes a snapshot section later.
    pub fn tripwire_drift(&self) -> PathBuf {
        self.root.join("tripwire/drift.json")
    }
}

/// Load and validate the canonical snapshot, or `None` on ANY failure (absent,
/// unreadable, malformed, or a schema version this build doesn't understand). The
/// single fault-tolerant read every snapshot-derived fact is mapped from — a `None`
/// here is read by the rules as "the suite view is unavailable", never an error.
pub fn load_snapshot(dir: &DataDir) -> Option<Snapshot> {
    workstate_schema::load_snapshot(&dir.snapshot()).ok()
}

// ── Snapshot → normalized facts ──────────────────────────────────────────────

/// The snapshot's build time (RFC3339), for the "checked Nm ago" stamp.
pub fn built_at_of(snap: &Snapshot) -> String {
    snap.built_at.to_rfc3339()
}

/// Feed freshness for every section conductor tracks, mapped from the canonical
/// `Section.status`. `jobs` joins `scripts`/`tools`/`findings` now that Proto runs
/// are part of the contract.
pub fn feeds_of(snap: &Snapshot) -> Vec<FeedStatus> {
    vec![
        FeedStatus {
            name: "scripts",
            freshness: freshness_of(&snap.scripts.status),
        },
        FeedStatus {
            name: "tools",
            freshness: freshness_of(&snap.tools.status),
        },
        FeedStatus {
            name: "findings",
            freshness: freshness_of(&snap.findings.status),
        },
        FeedStatus {
            name: "jobs",
            freshness: freshness_of(&snap.jobs.status),
        },
    ]
}

/// Collapse the contract's rich `FeedStatus` onto the three buckets conductor's
/// rules act on. `Fresh` is current; a readable-but-old or unknown-age feed is
/// stale (a refresh is the honest remedy); anything rejected or absent is
/// unavailable (re-running the producer may not clear it). Fails closed: a status
/// from a newer Workstate (`FeedStatus` is `#[non_exhaustive]`) is treated as
/// unavailable, never current.
fn freshness_of(status: &WsFeedStatus) -> Freshness {
    match status {
        WsFeedStatus::Fresh => Freshness::Current,
        WsFeedStatus::Stale | WsFeedStatus::FreshnessUnknown => Freshness::Stale,
        WsFeedStatus::UnsupportedVersion { .. }
        | WsFeedStatus::MissingVersion { .. }
        | WsFeedStatus::SourceMismatch { .. }
        | WsFeedStatus::Missing
        | WsFeedStatus::Failed { .. }
        | WsFeedStatus::Unknown(_) => Freshness::Unavailable,
        _ => Freshness::Unavailable,
    }
}

/// The findings the snapshot carries (Bulwark's normalized records), mapped to
/// conductor's `Finding` and sorted worst-first. `what` is the scanned subject
/// (`FindingId`) — the same value rule 5 correlates against a drift path. Empty
/// when the findings section is Missing/Failed (no `data`).
pub fn findings_of(snap: &Snapshot) -> Vec<Finding> {
    let mut findings: Vec<Finding> = snap
        .findings
        .data
        .as_ref()
        .map(|inv| inv.findings.iter().map(map_finding).collect())
        .unwrap_or_default();
    // Highest severity first; stable so equal severities keep producer order.
    findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
    findings
}

/// Map one canonical `Finding` onto conductor's normalized shape.
fn map_finding(f: &WsFinding) -> Finding {
    Finding {
        what: f.id.0.clone(),
        why: f
            .description
            .clone()
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| "flagged by bulwark".to_string()),
        source: "bulwark".to_string(),
        severity: map_severity(f.severity),
    }
}

/// Map the contract's severity onto conductor's four-bucket scale. The contract's
/// non-severity sentinels (`Unrated`/`Unknown`) and the low end (`Info`/`Low`) all
/// land at `Low`, below the High threshold the rules act on — so they never enter
/// the plan, matching the old "high/critical only" behaviour.
fn map_severity(sev: WsSeverity) -> Severity {
    match sev {
        WsSeverity::Critical => Severity::Critical,
        WsSeverity::High => Severity::High,
        WsSeverity::Medium => Severity::Medium,
        WsSeverity::Low | WsSeverity::Info | WsSeverity::Unrated | WsSeverity::Unknown => {
            Severity::Low
        }
        // `Severity` is #[non_exhaustive]; a future bucket below High is safe at Low.
        _ => Severity::Low,
    }
}

/// The failed Proto runs the snapshot carries (`JobOutcome::Failed`), mapped to
/// conductor's `FailedJob`. Empty when the jobs section is Missing/Failed. Replaces
/// the old per-file scan of `proto/sessions/*.json`: the outcome is now decided by
/// the producer and read straight off the snapshot.
pub fn failed_jobs_of(snap: &Snapshot) -> Vec<FailedJob> {
    snap.jobs
        .data
        .as_ref()
        .map(|inv| {
            inv.jobs
                .iter()
                .filter(|j| j.outcome == JobOutcome::Failed)
                .map(|j| FailedJob {
                    title: j.title.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── Binary presence on $PATH (live env state, not snapshot data) ─────────────

/// The suite binaries conductor expects, and may need to spawn in later phases.
pub const SUITE_BINARIES: &[&str] = &[
    "pulse",
    "rewind",
    "tripwire",
    "portman",
    "bulwark",
    "workstate",
    "proto",
    "rexops",
];

/// Probe each suite binary on `$PATH`. Pure filesystem lookups, no subprocess —
/// conductor stays read-only and can't hang on a slow child.
pub fn read_binaries() -> Vec<BinaryStatus> {
    SUITE_BINARIES
        .iter()
        .map(|&name| BinaryStatus {
            name,
            present: is_on_path(name),
        })
        .collect()
}

/// Whether `name` resolves to an executable on `$PATH`. An in-process `which(1)`
/// (delegated to suite-core): scan `$PATH` for an executable file, no fork.
/// Public so `run.rs` can gate a spawn on availability with the same probe
/// `read_binaries` uses.
pub fn is_on_path(name: &str) -> bool {
    suite_core::path::which(name)
}

// ── Drift — optional tripwire feed (not yet in the snapshot contract) ─────────

/// Read a file and parse it as `T`, returning `None` on any failure (absent,
/// unreadable, empty, malformed). The fault-tolerant choke point — now used ONLY
/// for the tripwire drift file, the one suite input not yet carried by the
/// canonical snapshot.
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Deserialize)]
struct TripwireDrift {
    #[serde(default)]
    paths: Vec<String>,
}

/// Read drifted paths from an optional `tripwire/drift.json`. Tripwire has no
/// published Workstate feed in the contract set yet, so this is the single point
/// that becomes a snapshot section later; until the file exists, it returns empty
/// and rule 5 (drift×finding correlation) stays dormant — never an error. Reads
/// via [`DataDir::tripwire_drift`], like every other reader reads its own path.
pub fn read_drift(dir: &DataDir) -> Vec<DriftedPath> {
    match read_json::<TripwireDrift>(&dir.tripwire_drift()) {
        Some(d) => d
            .paths
            .into_iter()
            .map(|p| DriftedPath { path: p })
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use workstate_schema::model::normalized::{
        FindingId, FindingInventory, Job, JobId, JobInventory,
    };
    use workstate_schema::model::provenance::{FeedId, Provenance, Section};

    /// A fixed build time so fixtures are deterministic.
    fn built_at() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 14, 12, 0, 0).unwrap()
    }

    /// A baseline snapshot with every section Missing — each test overrides only
    /// the sections it exercises.
    fn base_snapshot() -> Snapshot {
        Snapshot::new(
            built_at(),
            Section::missing(FeedId("scriptvault".into())),
            Section::missing(FeedId("toolfoundry".into())),
            Section::missing(FeedId("bulwark".into())),
            Section::missing(FeedId("proto".into())),
        )
    }

    /// Wrap a payload in a present `Section<T>` carrying `status`.
    fn present<T>(feed: &str, status: WsFeedStatus, data: T) -> Section<T> {
        Section {
            status,
            provenance: Provenance {
                feed_id: FeedId(feed.into()),
                fetched_at: None,
                source_observed_at: None,
                dropped_records: 0,
            },
            data: Some(data),
        }
    }

    fn ws_finding(id: &str, sev: WsSeverity, desc: Option<&str>) -> WsFinding {
        WsFinding {
            id: FindingId(id.into()),
            name: None,
            rule_id: None,
            description: desc.map(str::to_string),
            severity: sev,
            raw_severity: None,
            category: None,
            location: "unknown".into(),
            path: None,
            risk: None,
            owner: None,
        }
    }

    fn finding_inventory(findings: Vec<WsFinding>) -> FindingInventory {
        FindingInventory {
            generated_at: "2026-06-14T12:00:00Z".into(),
            findings,
            dropped_records: 0,
        }
    }

    fn job(id: &str, title: &str, outcome: JobOutcome) -> Job {
        Job {
            id: JobId(id.into()),
            title: title.into(),
            outcome,
        }
    }

    fn job_inventory(jobs: Vec<Job>) -> JobInventory {
        JobInventory {
            generated_at: "2026-06-14T12:00:00Z".into(),
            jobs,
            dropped_records: 0,
        }
    }

    // ── feeds / freshness ────────────────────────────────────────────────────

    #[test]
    fn feeds_map_every_section_status_to_a_freshness() {
        let mut snap = base_snapshot();
        snap.scripts.status = WsFeedStatus::Fresh;
        snap.tools.status = WsFeedStatus::Stale;
        snap.findings.status = WsFeedStatus::FreshnessUnknown;
        // jobs left Missing (from base_snapshot).
        let feeds = feeds_of(&snap);
        assert_eq!(feeds.len(), 4);
        assert_eq!(feeds[0].name, "scripts");
        assert_eq!(feeds[0].freshness, Freshness::Current);
        assert_eq!(feeds[1].freshness, Freshness::Stale);
        // FreshnessUnknown is "read clean but age unknown" → treated as stale.
        assert_eq!(feeds[2].freshness, Freshness::Stale);
        assert_eq!(feeds[3].name, "jobs");
        assert_eq!(feeds[3].freshness, Freshness::Unavailable);
    }

    #[test]
    fn freshness_fails_closed_for_rejected_and_absent_statuses() {
        assert_eq!(freshness_of(&WsFeedStatus::Fresh), Freshness::Current);
        assert_eq!(freshness_of(&WsFeedStatus::Stale), Freshness::Stale);
        assert_eq!(
            freshness_of(&WsFeedStatus::FreshnessUnknown),
            Freshness::Stale
        );
        for rejected in [
            WsFeedStatus::Missing,
            WsFeedStatus::Failed {
                reason: "boom".into(),
            },
            WsFeedStatus::MissingVersion { supported: 5 },
            WsFeedStatus::UnsupportedVersion {
                found: Some(1),
                supported: 5,
            },
            WsFeedStatus::SourceMismatch {
                expected: "bulwark".into(),
                found: "scriptvault".into(),
            },
        ] {
            assert_eq!(
                freshness_of(&rejected),
                Freshness::Unavailable,
                "{rejected:?} must be unavailable"
            );
        }
    }

    // ── findings ───────────────────────────────────────────────────────────

    #[test]
    fn findings_are_mapped_and_sorted_worst_first() {
        let mut snap = base_snapshot();
        snap.findings = present(
            "bulwark",
            WsFeedStatus::Fresh,
            finding_inventory(vec![
                ws_finding("hi.sh", WsSeverity::High, Some("exec bit on secret")),
                ws_finding("deploy-prod.sh", WsSeverity::Critical, Some("AWS key")),
            ]),
        );
        let f = findings_of(&snap);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].severity, Severity::Critical);
        assert_eq!(f[0].what, "deploy-prod.sh");
        assert_eq!(f[0].why, "AWS key");
        assert_eq!(f[0].source, "bulwark");
    }

    #[test]
    fn finding_without_description_gets_a_default_why() {
        let mut snap = base_snapshot();
        snap.findings = present(
            "bulwark",
            WsFeedStatus::Fresh,
            finding_inventory(vec![ws_finding("x.sh", WsSeverity::High, None)]),
        );
        let f = findings_of(&snap);
        assert_eq!(f[0].why, "flagged by bulwark");
    }

    #[test]
    fn low_band_severities_collapse_below_high() {
        // Info/Low/Unrated/Unknown all map below High, so they never enter the plan.
        for sev in [
            WsSeverity::Info,
            WsSeverity::Low,
            WsSeverity::Unrated,
            WsSeverity::Unknown,
        ] {
            assert!(map_severity(sev) < Severity::High, "{sev:?} must be < High");
        }
        assert_eq!(map_severity(WsSeverity::Medium), Severity::Medium);
        assert_eq!(map_severity(WsSeverity::High), Severity::High);
        assert_eq!(map_severity(WsSeverity::Critical), Severity::Critical);
    }

    #[test]
    fn missing_findings_section_yields_no_findings() {
        assert!(findings_of(&base_snapshot()).is_empty());
    }

    // ── failed jobs ──────────────────────────────────────────────────────────

    #[test]
    fn failed_jobs_keep_only_failures() {
        let mut snap = base_snapshot();
        snap.jobs = present(
            "proto",
            WsFeedStatus::Fresh,
            job_inventory(vec![
                job("a", "Passed Run", JobOutcome::Passed),
                job("b", "Failed Run", JobOutcome::Failed),
                job("c", "In Flight", JobOutcome::Running),
            ]),
        );
        let jobs = failed_jobs_of(&snap);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].title, "Failed Run");
    }

    #[test]
    fn missing_jobs_section_yields_no_failed_jobs() {
        assert!(failed_jobs_of(&base_snapshot()).is_empty());
    }

    // ── built_at ─────────────────────────────────────────────────────────────

    #[test]
    fn built_at_is_the_snapshots_own_rfc3339_time() {
        assert_eq!(built_at_of(&base_snapshot()), "2026-06-14T12:00:00+00:00");
    }

    // ── load_snapshot: path wiring + fault tolerance ─────────────────────────

    #[test]
    fn load_snapshot_roundtrips_through_the_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::new(dir.path().to_path_buf());
        // write_snapshot creates parent dirs, so we can write straight to the
        // canonical path the loader reads back from.
        workstate_schema::write_snapshot(&base_snapshot(), &data.snapshot()).unwrap();
        let snap = load_snapshot(&data).expect("a written snapshot must load");
        assert_eq!(snap.schema_version, workstate_schema::SCHEMA_VERSION);
    }

    #[test]
    fn missing_snapshot_loads_as_none_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::new(dir.path().to_path_buf());
        assert!(load_snapshot(&data).is_none());
    }

    #[test]
    fn malformed_snapshot_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::new(dir.path().to_path_buf());
        let path = data.snapshot();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json ").unwrap();
        assert!(load_snapshot(&data).is_none());
    }

    // ── paths ─────────────────────────────────────────────────────────────────

    #[test]
    fn data_dir_paths_are_rooted_correctly() {
        let d = DataDir::new(PathBuf::from("/data"));
        assert_eq!(
            d.snapshot(),
            PathBuf::from("/data/rexops/feeds/workstate.snapshot.json")
        );
        assert_eq!(
            d.tripwire_drift(),
            PathBuf::from("/data/tripwire/drift.json")
        );
    }

    // ── non-snapshot inputs (kept): binaries + drift ──────────────────────────

    #[test]
    fn binary_probe_covers_the_suite_and_detects_a_real_binary() {
        // Reads $PATH, so it shares the global PATH lock with the tests that
        // mutate it (in `run` and `tui`) to avoid a parallel-test race.
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
        let checks = read_binaries();
        assert_eq!(checks.len(), SUITE_BINARIES.len());
        // `sh` is on every PATH — sanity-check the probe itself without depending
        // on suite tools being installed.
        assert!(is_on_path("sh"));
        assert!(!is_on_path("definitely-not-a-real-binary-xyzzy"));
    }

    #[test]
    fn drift_is_empty_when_absent_and_parsed_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::new(dir.path().to_path_buf());
        assert!(read_drift(&data).is_empty());

        let drift_path = data.tripwire_drift();
        std::fs::create_dir_all(drift_path.parent().unwrap()).unwrap();
        std::fs::write(
            &drift_path,
            r#"{ "paths": ["deploy-prod.sh", "etc/hosts"] }"#,
        )
        .unwrap();
        let d = read_drift(&data);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].path, "deploy-prod.sh");
    }
}

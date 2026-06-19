//! Reading the suite's file contracts for conductor.
//!
//! Conductor is a passive reader (see CONDUCTOR_DESIGN.md): it consumes
//! published artifacts and never mutates a producer. Every reader here is
//! **fault-tolerant by design** — a missing, unreadable, empty, malformed, or
//! wrong-major file resolves to "unavailable" and never panics
//! (`docs/CONTRACT_RULES.md`). serde ignores unknown fields, so additive
//! contract changes (same major) need no change here. This discipline is lifted
//! from pulse's `sources.rs`.
//!
//! Paths follow `docs/INTEGRATION_MAP.md` ("Expected output paths"), rooted at
//! the suite data root (`$XDG_DATA_HOME`, fallback `~/.local/share`).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ConductorError;
use crate::state::{BinaryStatus, FeedStatus, Freshness};
use crate::util;

/// The resolved on-disk layout conductor reads from.
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

    /// Workstate's suite snapshot, as RexOps reads it.
    pub fn workstate_snapshot(&self) -> PathBuf {
        self.root.join("rexops/feeds/workstate.snapshot.json")
    }

    /// RexOps's assembled suite snapshot.
    pub fn rexops_snapshot(&self) -> PathBuf {
        self.root.join("rexops/snapshot.json")
    }

    /// Bulwark's Workstate feed.
    pub fn bulwark_feed(&self) -> PathBuf {
        self.root.join("workstate/feeds/bulwark.json")
    }

    /// Directory of Proto session records, one JSON per run.
    pub fn proto_sessions(&self) -> PathBuf {
        self.root.join("proto/sessions")
    }

    /// Tripwire's (optional, not-yet-contracted) drift file. The single point
    /// that becomes a real tripwire contract later; see `read_drift`.
    pub fn tripwire_drift(&self) -> PathBuf {
        self.root.join("tripwire/drift.json")
    }
}

/// Read a file and parse it as `T`, returning `None` on *any* failure (absent,
/// unreadable, empty, malformed). The single choke point that makes every reader
/// fault-tolerant.
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

// ── Feed freshness — Workstate snapshot ──────────────────────────────────────

#[derive(Deserialize)]
struct WorkstateSnapshot {
    #[serde(default)]
    built_at: Option<String>,
    #[serde(default)]
    scripts: Option<Section>,
    #[serde(default)]
    tools: Option<Section>,
    #[serde(default)]
    findings: Option<Section>,
}

#[derive(Deserialize)]
struct Section {
    /// `"Fresh"`, `"Stale"`, or an object variant like `{"UnsupportedVersion":…}`.
    /// Read as a free `Value` so the object form never fails to parse.
    status: serde_json::Value,
}

impl Section {
    fn freshness(&self) -> Freshness {
        match &self.status {
            serde_json::Value::String(s) if s == "Fresh" => Freshness::Current,
            serde_json::Value::String(s) if s == "Stale" => Freshness::Stale,
            _ => Freshness::Unavailable,
        }
    }
}

/// Read Workstate snapshot freshness. Always returns a value: a missing/malformed
/// file yields `(None, vec![])` — the rules read that as the suite view being
/// unavailable, not as an error.
pub fn read_feeds(dir: &DataDir) -> (Option<String>, Vec<FeedStatus>) {
    let Some(snap): Option<WorkstateSnapshot> = read_json(&dir.workstate_snapshot()) else {
        return (None, Vec::new());
    };
    let mut feeds = Vec::new();
    if let Some(s) = &snap.scripts {
        feeds.push(FeedStatus { name: "scripts", freshness: s.freshness() });
    }
    if let Some(s) = &snap.tools {
        feeds.push(FeedStatus { name: "tools", freshness: s.freshness() });
    }
    if let Some(s) = &snap.findings {
        feeds.push(FeedStatus { name: "findings", freshness: s.freshness() });
    }
    (snap.built_at, feeds)
}

// ── Binary presence on $PATH ─────────────────────────────────────────────────

/// The suite binaries conductor expects, and may need to spawn in later phases.
pub const SUITE_BINARIES: &[&str] =
    &["pulse", "rewind", "tripwire", "portman", "bulwark", "workstate", "proto", "rexops"];

/// Probe each suite binary on `$PATH`. Pure filesystem lookups, no subprocess —
/// conductor stays read-only and can't hang on a slow child.
pub fn read_binaries() -> Vec<BinaryStatus> {
    SUITE_BINARIES
        .iter()
        .map(|&name| BinaryStatus { name, present: which(name) })
        .collect()
}

/// Whether `name` resolves to an executable on `$PATH`. An in-process `which(1)`:
/// scan `$PATH` entries for an executable file, no fork.
fn which(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(name)))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

// ── Findings — RexOps aggregate snapshot, else Bulwark feed ───────────────────

use crate::state::{DriftedPath, FailedJob, Finding, Severity};

fn parse_severity(s: &str) -> Option<Severity> {
    match s {
        "low" => Some(Severity::Low),
        "medium" => Some(Severity::Medium),
        "high" => Some(Severity::High),
        "critical" => Some(Severity::Critical),
        _ => None,
    }
}

#[derive(Deserialize)]
struct RexopsSnapshot {
    #[serde(default)]
    attention: Vec<RexopsAttention>,
}

#[derive(Deserialize)]
struct RexopsAttention {
    #[serde(default)]
    tool: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    severity: String,
}

#[derive(Deserialize)]
struct BulwarkFeed {
    #[serde(default)]
    items: Vec<BulwarkItem>,
}

#[derive(Deserialize)]
struct BulwarkItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    description: String,
}

/// Read findings, preferring the RexOps aggregate (richest single input). When
/// the aggregate is absent, fall back to the Bulwark feed and surface only its
/// high/critical items (low/medium inventory noise is left out of the plan).
/// Always sorted highest-severity first.
pub fn read_findings(dir: &DataDir) -> Vec<Finding> {
    let mut findings: Vec<Finding> = if let Some(snap) =
        read_json::<RexopsSnapshot>(&dir.rexops_snapshot())
    {
        snap.attention
            .into_iter()
            .map(|a| Finding {
                what: if a.id.is_empty() { a.tool.clone() } else { a.id },
                why: a.reason,
                source: a.tool,
                severity: parse_severity(&a.severity).unwrap_or(Severity::Low),
            })
            .collect()
    } else if let Some(feed) = read_json::<BulwarkFeed>(&dir.bulwark_feed()) {
        feed.items
            .into_iter()
            .filter_map(|it| {
                let sev = parse_severity(&it.severity)?;
                if sev < Severity::High {
                    return None;
                }
                Some(Finding {
                    what: if it.name.is_empty() { it.id } else { it.name },
                    why: if it.description.is_empty() {
                        "flagged by bulwark".to_string()
                    } else {
                        it.description
                    },
                    source: "bulwark".to_string(),
                    severity: sev,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    // Highest severity first; stable so equal severities keep producer order.
    findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
    findings
}

// ── Failed jobs — Proto sessions ─────────────────────────────────────────────

#[derive(Deserialize)]
struct ProtoSession {
    #[serde(default)]
    protocol_title: Option<String>,
    #[serde(default)]
    steps: Vec<ProtoStep>,
}

#[derive(Deserialize)]
struct ProtoStep {
    #[serde(default)]
    status: String,
}

/// Read every Proto session and keep only those with a failed step. Unreadable
/// individual files are skipped; a missing directory yields an empty list.
pub fn read_failed_jobs(dir: &DataDir) -> Vec<FailedJob> {
    let Ok(entries) = std::fs::read_dir(dir.proto_sessions()) else {
        return Vec::new();
    };
    let mut jobs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(s): Option<ProtoSession> = read_json(&path) else {
            continue;
        };
        if s.steps.iter().any(|st| st.status == "failed") {
            jobs.push(FailedJob {
                title: s.protocol_title.unwrap_or_else(|| "protocol run".to_string()),
            });
        }
    }
    jobs
}

// ── Drift — optional tripwire feed ───────────────────────────────────────────

#[derive(Deserialize)]
struct TripwireDrift {
    #[serde(default)]
    paths: Vec<String>,
}

/// Read drifted paths from an optional `tripwire/drift.json`. Tripwire has no
/// published Workstate feed in the contract set yet, so this is the single point
/// that becomes a real contract later; until the file exists, it returns empty
/// and rule 5 (drift×finding correlation) stays dormant — never an error. Reads
/// via `DataDir::tripwire_drift()`, like every other reader reads its own path.
pub fn read_drift(dir: &DataDir) -> Vec<DriftedPath> {
    match read_json::<TripwireDrift>(&dir.tripwire_drift()) {
        Some(d) => d.paths.into_iter().map(|p| DriftedPath { path: p }).collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A throwaway data dir, unique per test, cleaned on drop.
    struct TempData {
        dir: PathBuf,
    }

    impl TempData {
        fn new(tag: &str) -> Self {
            let mut dir = std::env::temp_dir();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            dir.push(format!("conductor-test-{tag}-{nanos}"));
            std::fs::create_dir_all(&dir).unwrap();
            TempData { dir }
        }
        fn data(&self) -> DataDir {
            DataDir::new(self.dir.clone())
        }
        fn write(&self, rel: &str, body: &str) {
            let p = self.dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::File::create(p).unwrap().write_all(body.as_bytes()).unwrap();
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn missing_snapshot_reads_as_no_feeds_not_a_panic() {
        let t = TempData::new("missing");
        let (built, feeds) = read_feeds(&t.data());
        assert!(built.is_none());
        assert!(feeds.is_empty());
    }

    #[test]
    fn malformed_snapshot_reads_as_no_feeds() {
        let t = TempData::new("malformed");
        t.write("rexops/feeds/workstate.snapshot.json", "{ not json ");
        let (built, feeds) = read_feeds(&t.data());
        assert!(built.is_none());
        assert!(feeds.is_empty());
    }

    #[test]
    fn snapshot_freshness_maps_each_section_status() {
        let t = TempData::new("fresh");
        t.write(
            "rexops/feeds/workstate.snapshot.json",
            r#"{
              "schema_version": 4,
              "built_at": "2026-06-14T12:00:00Z",
              "scripts":  { "status": "Fresh" },
              "tools":    { "status": "Stale" },
              "findings": { "status": { "UnsupportedVersion": { "found": null, "supported": 1 } } }
            }"#,
        );
        let (built, feeds) = read_feeds(&t.data());
        assert_eq!(built.as_deref(), Some("2026-06-14T12:00:00Z"));
        assert_eq!(feeds.len(), 3);
        assert_eq!(feeds[0].name, "scripts");
        assert_eq!(feeds[0].freshness, Freshness::Current);
        assert_eq!(feeds[1].freshness, Freshness::Stale);
        assert_eq!(feeds[2].freshness, Freshness::Unavailable);
    }

    #[test]
    fn binary_probe_covers_the_suite_and_detects_a_real_binary() {
        let checks = read_binaries();
        assert_eq!(checks.len(), SUITE_BINARIES.len());
        // `sh` is on every PATH — sanity-check the probe itself without depending
        // on suite tools being installed.
        assert!(which("sh"));
        assert!(!which("definitely-not-a-real-binary-xyzzy"));
    }

    #[test]
    fn data_dir_paths_are_rooted_correctly() {
        let d = DataDir::new(PathBuf::from("/data"));
        assert_eq!(
            d.workstate_snapshot(),
            PathBuf::from("/data/rexops/feeds/workstate.snapshot.json")
        );
        assert_eq!(d.rexops_snapshot(), PathBuf::from("/data/rexops/snapshot.json"));
        assert_eq!(d.bulwark_feed(), PathBuf::from("/data/workstate/feeds/bulwark.json"));
        assert_eq!(d.proto_sessions(), PathBuf::from("/data/proto/sessions"));
        assert_eq!(d.tripwire_drift(), PathBuf::from("/data/tripwire/drift.json"));
    }

    #[test]
    fn findings_prefer_rexops_and_sort_by_severity() {
        let t = TempData::new("findings");
        t.write(
            "rexops/snapshot.json",
            r#"{
              "schema_version": 1, "source_tool": "rexops",
              "attention": [
                { "tool": "toolfoundry", "id": "backup-home", "reason": "health failing", "severity": "high" },
                { "tool": "bulwark", "id": "deploy-prod.sh", "reason": "AWS key", "severity": "critical" }
              ]
            }"#,
        );
        let f = read_findings(&t.data());
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].severity, Severity::Critical);
        assert_eq!(f[0].what, "deploy-prod.sh");
        assert_eq!(f[0].source, "bulwark");
    }

    #[test]
    fn findings_fall_back_to_bulwark_high_and_critical_only() {
        let t = TempData::new("bulwark");
        t.write(
            "workstate/feeds/bulwark.json",
            r#"{
              "schema_version": 1, "source_tool": "bulwark", "item_count": 3,
              "items": [
                { "name": "low.sh",  "severity": "low",      "description": "noise" },
                { "name": "hi.sh",   "severity": "high",     "description": "exec bit on secret" },
                { "name": "crit.sh", "severity": "critical", "description": "private key committed" }
              ]
            }"#,
        );
        let f = read_findings(&t.data());
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.severity >= Severity::High));
        assert_eq!(f[0].severity, Severity::Critical);
    }

    #[test]
    fn no_findings_anywhere_reads_as_empty() {
        let t = TempData::new("nofind");
        assert!(read_findings(&t.data()).is_empty());
    }

    #[test]
    fn failed_jobs_keep_only_failures() {
        let t = TempData::new("jobs");
        t.write(
            "proto/sessions/a.json",
            r#"{ "protocol_title":"Passed Run", "steps":[{"status":"passed"}] }"#,
        );
        t.write(
            "proto/sessions/b.json",
            r#"{ "protocol_title":"Failed Run", "steps":[{"status":"failed"}] }"#,
        );
        t.write("proto/sessions/notes.txt", "ignore me");
        let jobs = read_failed_jobs(&t.data());
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].title, "Failed Run");
    }

    #[test]
    fn drift_is_empty_when_absent_and_parsed_when_present() {
        let t = TempData::new("drift");
        assert!(read_drift(&t.data()).is_empty());
        t.write("tripwire/drift.json", r#"{ "paths": ["deploy-prod.sh", "etc/hosts"] }"#);
        let d = read_drift(&t.data());
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].path, "deploy-prod.sh");
    }
}

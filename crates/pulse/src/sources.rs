//! Reading the suite's file contracts for Pulse.
//!
//! Pulse is a passive reader (see PULSE_DESIGN.md): it consumes published
//! artifacts and never mutates a producer or regenerates a feed. Each reader
//! here loads one contract from disk, and every one is **fault-tolerant by
//! design** — a missing, unreadable, empty, malformed, or wrong-major file
//! resolves to "unavailable"/`None` and never panics. That mirrors the suite
//! rule that a missing optional feed must never crash a consumer
//! (`docs/CONTRACT_RULES.md`), and it is what lets Pulse render the *Incomplete*
//! verdict honestly instead of erroring out.
//!
//! Paths follow `docs/INTEGRATION_MAP.md` ("Expected output paths"), rooted at
//! `$XDG_DATA_HOME` (fallback `~/.local/share`). `$PULSE_DATA_DIR` overrides the
//! root wholesale, which is how the tests point Pulse at a fixture directory.
//!
//! serde ignores unknown fields by default, so additive contract changes (same
//! major) need no change here — also a contract rule.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The resolved on-disk layout Pulse reads from. Built once from the
/// environment; all reader functions take paths derived from it.
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Resolve the suite data root. `$PULSE_DATA_DIR` wins (test / power-user
    /// override); otherwise `$XDG_DATA_HOME`; otherwise `~/.local/share`.
    pub fn resolve() -> Self {
        let root = std::env::var_os("PULSE_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from(".local/share"));
        DataDir { root }
    }

    /// Workstate's suite snapshot, as RexOps reads it.
    /// `…/rexops/feeds/workstate.snapshot.json`.
    pub fn workstate_snapshot(&self) -> PathBuf {
        self.root.join("rexops/feeds/workstate.snapshot.json")
    }

    /// RexOps's assembled suite snapshot. The integration map lists this output
    /// as "self/report" with no fixed path, so Pulse reads the natural location
    /// next to its other rexops state.
    pub fn rexops_snapshot(&self) -> PathBuf {
        self.root.join("rexops/snapshot.json")
    }

    /// Bulwark's Workstate feed. `…/workstate/feeds/bulwark.json`.
    pub fn bulwark_feed(&self) -> PathBuf {
        self.root.join("workstate/feeds/bulwark.json")
    }

    /// Directory of Proto session records, one JSON per run.
    /// `…/proto/sessions/`.
    pub fn proto_sessions(&self) -> PathBuf {
        self.root.join("proto/sessions")
    }
}

/// Read a file and parse it as `T`, returning `None` on *any* failure (absent,
/// unreadable, empty, malformed). This is the single choke point that makes
/// every reader fault-tolerant.
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// Freshness — Workstate snapshot
// ─────────────────────────────────────────────────────────────────────────────

/// One source's freshness as Pulse cares about it. Collapses the snapshot's
/// richer per-section status into the three buckets the verdict needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freshness {
    Current,
    Stale,
    /// Present but unusable (unsupported version) or absent entirely.
    Unavailable,
}

/// Workstate snapshot, only the fields Pulse needs. Each section reports its own
/// `status`; the rest of each section is ignored (unknown fields are fine).
#[derive(Deserialize)]
struct WorkstateSnapshot {
    /// When the snapshot was built (RFC3339). Drives the screen's "age".
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
    /// `"Fresh"`, `"Stale"`, or an object like `{"UnsupportedVersion": {...}}`.
    /// Deserialized as a free `Value` so the object-variant form never fails.
    status: serde_json::Value,
}

impl Section {
    fn freshness(&self) -> Freshness {
        match &self.status {
            serde_json::Value::String(s) if s == "Fresh" => Freshness::Current,
            serde_json::Value::String(s) if s == "Stale" => Freshness::Stale,
            // Any object variant (UnsupportedVersion, MissingData, …) or unknown
            // string means the section can't be trusted.
            _ => Freshness::Unavailable,
        }
    }
}

/// The freshness picture from Workstate: the snapshot's build time and the
/// freshness of each named section. Absent snapshot => empty + `None` age, which
/// the verdict reads as the whole suite view being unavailable.
#[derive(Clone)]
pub struct SnapshotFreshness {
    pub built_at: Option<String>,
    /// (section name, freshness), in a stable display order.
    pub sections: Vec<(&'static str, Freshness)>,
}

impl SnapshotFreshness {
    pub fn worst(&self) -> Option<Freshness> {
        self.sections
            .iter()
            .map(|(_, f)| *f)
            .max_by_key(|f| match f {
                Freshness::Current => 0,
                Freshness::Stale => 1,
                Freshness::Unavailable => 2,
            })
    }

    pub fn any_stale(&self) -> bool {
        self.sections.iter().any(|(_, f)| *f == Freshness::Stale)
    }
}

/// Read Workstate snapshot freshness. Always returns a value: a missing file
/// yields an empty section list (no sections == nothing current).
pub fn read_freshness(dir: &DataDir) -> SnapshotFreshness {
    let Some(snap): Option<WorkstateSnapshot> = read_json(&dir.workstate_snapshot()) else {
        return SnapshotFreshness {
            built_at: None,
            sections: Vec::new(),
        };
    };
    let mut sections = Vec::new();
    if let Some(s) = &snap.scripts {
        sections.push(("scripts", s.freshness()));
    }
    if let Some(s) = &snap.tools {
        sections.push(("tools", s.freshness()));
    }
    if let Some(s) = &snap.findings {
        sections.push(("findings", s.freshness()));
    }
    SnapshotFreshness {
        built_at: snap.built_at,
        sections,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Findings / attention — RexOps snapshot (aggregator) + Bulwark feed
// ─────────────────────────────────────────────────────────────────────────────

/// Severity of an attention item, in escalation order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Severity::Low),
            "medium" => Some(Severity::Medium),
            "high" => Some(Severity::High),
            "critical" => Some(Severity::Critical),
            _ => None,
        }
    }
}

/// One thing worth the operator's attention, normalized across producers.
#[derive(Clone)]
pub struct Attention {
    /// What is affected (the producer's item id / name).
    pub what: String,
    /// Why it matters (short reason).
    pub why: String,
    /// Which source reported it.
    pub source: String,
    pub severity: Severity,
}

/// RexOps snapshot: the suite-level aggregator. Provides both per-source
/// availability and pre-aggregated attention items, so when present it is
/// Pulse's richest single input.
#[derive(Deserialize)]
struct RexopsSnapshot {
    #[serde(default)]
    generated_at: Option<String>,
    #[serde(default)]
    sources: std::collections::BTreeMap<String, RexopsSource>,
    #[serde(default)]
    attention: Vec<RexopsAttention>,
}

#[derive(Deserialize)]
struct RexopsSource {
    #[serde(default)]
    present: bool,
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

/// What RexOps tells us, already normalized. Absent => `None` everywhere, which
/// the verdict treats as "the aggregator isn't running" (an Incomplete signal).
#[derive(Clone)]
pub struct RexopsView {
    pub generated_at: Option<String>,
    /// (source name, present?) for the sources RexOps tracked.
    pub sources: Vec<(String, bool)>,
    pub attention: Vec<Attention>,
}

pub fn read_rexops(dir: &DataDir) -> Option<RexopsView> {
    let snap: RexopsSnapshot = read_json(&dir.rexops_snapshot())?;
    let sources = snap
        .sources
        .into_iter()
        .map(|(name, s)| (name, s.present))
        .collect();
    let attention = snap
        .attention
        .into_iter()
        .map(|a| Attention {
            what: if a.id.is_empty() {
                a.tool.clone()
            } else {
                a.id
            },
            why: a.reason,
            source: a.tool,
            severity: Severity::parse(&a.severity).unwrap_or(Severity::Low),
        })
        .collect();
    Some(RexopsView {
        generated_at: snap.generated_at,
        sources,
        attention,
    })
}

/// Bulwark Workstate feed: a flat inventory of items with a severity/risk each.
/// Pulse reads it as a *fallback* source of findings when RexOps hasn't already
/// aggregated them, and as a freshness/presence signal for Bulwark itself.
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

#[derive(Clone)]
pub struct BulwarkView {
    /// Items at `high` or `critical`, surfaced as attention. Low/medium
    /// inventory noise is left for the Attention drill-down, not the verdict.
    pub attention: Vec<Attention>,
    /// Whether the feed was present at all (drives Bulwark's source marker).
    pub present: bool,
}

pub fn read_bulwark(dir: &DataDir) -> BulwarkView {
    let Some(feed): Option<BulwarkFeed> = read_json(&dir.bulwark_feed()) else {
        return BulwarkView {
            attention: Vec::new(),
            present: false,
        };
    };
    let attention = feed
        .items
        .into_iter()
        .filter_map(|it| {
            let sev = Severity::parse(&it.severity)?;
            if sev < Severity::High {
                return None;
            }
            Some(Attention {
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
        .collect();
    BulwarkView {
        attention,
        present: true,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Jobs — Proto sessions (and RexOps job rollup if present)
// ─────────────────────────────────────────────────────────────────────────────

/// A run's outcome, derived from a Proto session record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobOutcome {
    /// Every step has an outcome and none failed.
    Passed,
    /// At least one step failed.
    Failed,
    /// Still in progress (no `finished_at`, or a step is pending).
    Running,
}

#[derive(Clone)]
pub struct Job {
    pub title: String,
    pub outcome: JobOutcome,
}

/// Proto session record (v1), the fields Pulse needs to classify the run. Both
/// the per-step `status` form (the real session schema) and a top-level
/// `status`/`failed` form (older feed shape) are tolerated.
#[derive(Deserialize)]
struct ProtoSession {
    #[serde(default)]
    protocol_title: Option<String>,
    #[serde(default)]
    finished_at: Option<String>,
    #[serde(default)]
    steps: Vec<ProtoStep>,
    /// Older/feed shape fallbacks; ignored when `steps` is present.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    failed: Option<u32>,
}

#[derive(Deserialize)]
struct ProtoStep {
    #[serde(default)]
    status: String,
}

impl ProtoSession {
    fn outcome(&self) -> JobOutcome {
        if !self.steps.is_empty() {
            if self.steps.iter().any(|s| s.status == "failed") {
                return JobOutcome::Failed;
            }
            let unfinished =
                self.finished_at.is_none() || self.steps.iter().any(|s| s.status == "pending");
            return if unfinished {
                JobOutcome::Running
            } else {
                JobOutcome::Passed
            };
        }
        // Feed-shape fallback.
        match self.status.as_deref() {
            Some("complete") if self.failed.unwrap_or(0) > 0 => JobOutcome::Failed,
            Some("complete") => JobOutcome::Passed,
            _ => JobOutcome::Running,
        }
    }
}

/// Read every Proto session file and classify each run. Unreadable individual
/// files are skipped, not fatal; a missing directory yields an empty list.
pub fn read_jobs(dir: &DataDir) -> Vec<Job> {
    let sessions = dir.proto_sessions();
    let Ok(entries) = std::fs::read_dir(&sessions) else {
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
        let title = s
            .protocol_title
            .clone()
            .unwrap_or_else(|| "protocol run".to_string());
        jobs.push(Job {
            title,
            outcome: s.outcome(),
        });
    }
    jobs
}

// ─────────────────────────────────────────────────────────────────────────────
// Environment / binary checks  (the would-be rex-doctor, done locally)
// ─────────────────────────────────────────────────────────────────────────────

/// One suite binary and whether it is on `$PATH`. There is no `rex-doctor`
/// producer in the suite, so Pulse performs these env probes itself — they are
/// purely local `which`-style lookups that need no external feed.
#[derive(Clone, Copy)]
pub struct BinaryCheck {
    pub name: &'static str,
    pub present: bool,
}

/// The suite binaries Pulse expects an operator to have installed. Mirrors the
/// launchable tools the integration map documents.
const SUITE_BINARIES: &[&str] = &["bulwark", "proto", "rexops", "scriptvault", "toolfoundry"];

/// Probe each suite binary on `$PATH`. Pure filesystem lookups; no subprocess is
/// spawned (Pulse stays read-only and side-effect-free), so this also can't hang
/// on a slow child.
pub fn read_binaries() -> Vec<BinaryCheck> {
    SUITE_BINARIES
        .iter()
        .map(|&name| BinaryCheck {
            name,
            present: which(name),
        })
        .collect()
}

/// Whether `name` resolves to an executable on `$PATH`. A `which(1)` done
/// in-process (delegated to suite-core): scan `$PATH` for an executable file,
/// no fork.
fn which(name: &str) -> bool {
    suite_core::path::which(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A throwaway data dir under the system temp, unique per test, cleaned on
    /// drop. Lets readers hit real files without touching the user's data.
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
            dir.push(format!("pulse-test-{tag}-{nanos}"));
            std::fs::create_dir_all(&dir).unwrap();
            TempData { dir }
        }
        fn data(&self) -> DataDir {
            DataDir {
                root: self.dir.clone(),
            }
        }
        fn write(&self, rel: &str, body: &str) {
            let p = self.dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let mut f = std::fs::File::create(p).unwrap();
            f.write_all(body.as_bytes()).unwrap();
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn missing_files_never_panic_and_read_as_unavailable() {
        let t = TempData::new("missing");
        let d = t.data();
        let fresh = read_freshness(&d);
        assert!(fresh.sections.is_empty());
        assert!(fresh.built_at.is_none());
        assert!(read_rexops(&d).is_none());
        assert!(!read_bulwark(&d).present);
        assert!(read_jobs(&d).is_empty());
    }

    #[test]
    fn malformed_json_reads_as_unavailable() {
        let t = TempData::new("malformed");
        t.write("rexops/feeds/workstate.snapshot.json", "{ not json ");
        t.write("rexops/snapshot.json", "garbage");
        let d = t.data();
        assert!(read_freshness(&d).sections.is_empty());
        assert!(read_rexops(&d).is_none());
    }

    #[test]
    fn workstate_freshness_maps_section_status() {
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
        let f = read_freshness(&t.data());
        assert_eq!(f.built_at.as_deref(), Some("2026-06-14T12:00:00Z"));
        assert_eq!(
            f.sections,
            vec![
                ("scripts", Freshness::Current),
                ("tools", Freshness::Stale),
                ("findings", Freshness::Unavailable),
            ]
        );
        assert!(f.any_stale());
        assert_eq!(f.worst(), Some(Freshness::Unavailable));
    }

    #[test]
    fn rexops_attention_is_normalized_and_sorted_by_severity() {
        let t = TempData::new("rexops");
        t.write(
            "rexops/snapshot.json",
            r#"{
              "schema_version": 1,
              "source_tool": "rexops",
              "generated_at": "2026-06-14T00:00:00Z",
              "sources": {
                "workstate": { "present": true,  "last_seen": "x" },
                "scriptvault": { "present": false, "last_seen": null }
              },
              "attention": [
                { "tool": "toolfoundry", "id": "backup-home", "reason": "health failing", "severity": "high" },
                { "tool": "bulwark", "id": "deploy-prod.sh", "reason": "AWS key", "severity": "critical" }
              ]
            }"#,
        );
        let v = read_rexops(&t.data()).expect("present");
        assert_eq!(v.generated_at.as_deref(), Some("2026-06-14T00:00:00Z"));
        assert_eq!(v.attention.len(), 2);
        // sources carried through with presence.
        assert!(v.sources.iter().any(|(n, p)| n == "workstate" && *p));
        assert!(v.sources.iter().any(|(n, p)| n == "scriptvault" && !*p));
        let crit = v
            .attention
            .iter()
            .find(|a| a.severity == Severity::Critical)
            .unwrap();
        assert_eq!(crit.what, "deploy-prod.sh");
        assert_eq!(crit.source, "bulwark");
    }

    #[test]
    fn bulwark_only_surfaces_high_and_critical() {
        let t = TempData::new("bulwark");
        t.write(
            "workstate/feeds/bulwark.json",
            r#"{
              "schema_version": 1, "source_tool": "bulwark",
              "generated_at": "2026-06-06T12:34:56Z", "item_count": 3,
              "items": [
                { "name": "low.sh",  "severity": "low",      "description": "noise" },
                { "name": "hi.sh",   "severity": "high",     "description": "exec bit on secret" },
                { "name": "crit.sh", "severity": "critical", "description": "private key committed" }
              ]
            }"#,
        );
        let v = read_bulwark(&t.data());
        assert!(v.present);
        assert_eq!(v.attention.len(), 2);
        assert!(v.attention.iter().all(|a| a.severity >= Severity::High));
    }

    #[test]
    fn proto_sessions_classify_outcomes() {
        let t = TempData::new("proto");
        // passed: finished, no failed steps
        t.write(
            "proto/sessions/a.json",
            r#"{ "schema_version":1,"source_tool":"proto","generated_at":"x",
                 "protocol_id":"p","protocol_title":"Passed Run","started_at":"x",
                 "finished_at":"y",
                 "steps":[{"step_id":"1","status":"passed"},{"step_id":"2","status":"acknowledged"}] }"#,
        );
        // failed: a failed step
        t.write(
            "proto/sessions/b.json",
            r#"{ "schema_version":1,"source_tool":"proto","generated_at":"x",
                 "protocol_id":"p","protocol_title":"Failed Run","started_at":"x",
                 "finished_at":"y",
                 "steps":[{"step_id":"1","status":"failed"}] }"#,
        );
        // running: no finished_at, a pending step
        t.write(
            "proto/sessions/c.json",
            r#"{ "schema_version":1,"source_tool":"proto","generated_at":"x",
                 "protocol_id":"p","protocol_title":"Live Run","started_at":"x",
                 "steps":[{"step_id":"1","status":"passed"},{"step_id":"2","status":"pending"}] }"#,
        );
        // a non-json file is ignored
        t.write("proto/sessions/notes.txt", "ignore me");

        let mut jobs = read_jobs(&t.data());
        jobs.sort_by(|a, b| a.title.cmp(&b.title));
        assert_eq!(jobs.len(), 3);
        let by = |title: &str| jobs.iter().find(|j| j.title == title).unwrap().outcome;
        assert_eq!(by("Passed Run"), JobOutcome::Passed);
        assert_eq!(by("Failed Run"), JobOutcome::Failed);
        assert_eq!(by("Live Run"), JobOutcome::Running);
    }

    #[test]
    fn binary_checks_cover_the_suite_and_detect_a_real_binary() {
        let checks = read_binaries();
        assert_eq!(checks.len(), SUITE_BINARIES.len());
        // `which` should find `sh`, which every PATH has — sanity-check the probe
        // logic itself without depending on suite tools being installed.
        assert!(which("sh"));
        assert!(!which("definitely-not-a-real-binary-xyzzy"));
    }
}

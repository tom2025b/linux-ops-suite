//! Deriving Pulse's view from the one Workstate snapshot.
//!
//! Pulse is a passive reader: it loads the single canonical snapshot Workstate
//! publishes and DERIVES everything it shows from it — freshness, attention,
//! per-source presence, and protocol-run outcomes. It never reads a raw producer
//! feed (Bulwark/Proto/ScriptVault/ToolFoundry) directly; if Pulse needs a datum,
//! that datum must be in the snapshot. The snapshot is read through
//! `workstate_schema` (its own `Snapshot` type, the canonical path, and the
//! validating loader), so the contract cannot drift.
//!
//! Every derivation is fault-tolerant: a missing, unreadable, malformed, or
//! wrong-version snapshot yields empty/`None` views and never panics — which is
//! what lets Pulse render the *Incomplete* verdict honestly instead of erroring.

use std::path::PathBuf;

use workstate_schema::model::normalized::{JobOutcome as WsJobOutcome, Severity as WsSeverity};
use workstate_schema::model::provenance::FeedStatus;
use workstate_schema::Snapshot;

/// The resolved on-disk layout Pulse reads from, built once from the environment.
/// There is now exactly ONE artifact: the Workstate snapshot.
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

    /// The canonical Workstate snapshot path under this root. Delegates to
    /// `workstate_schema` so Pulse never re-spells where the snapshot lives.
    pub fn snapshot_path(&self) -> PathBuf {
        workstate_schema::snapshot_path_under(&self.root)
    }
}

/// Load and validate the Workstate snapshot, or `None` on any failure (absent,
/// unreadable, malformed, or a schema version this build doesn't understand). This
/// is the ONE read Pulse performs; every view below is derived from its result.
pub fn load(dir: &DataDir) -> Option<Snapshot> {
    workstate_schema::load_snapshot(&dir.snapshot_path()).ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// Freshness — per-section status of the snapshot
// ─────────────────────────────────────────────────────────────────────────────

/// One source's freshness as Pulse cares about it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freshness {
    Current,
    Stale,
    /// Present but unusable (unsupported/missing version, failed), or absent.
    Unavailable,
}

/// Collapse a section's `FeedStatus` into Pulse's three buckets. Only a clean,
/// recent read is `Current`; a known-old read is `Stale`; everything else
/// (unsupported/missing version, source mismatch, failed, missing, or a
/// freshness-unknown read we can't vouch for) is `Unavailable`.
fn section_freshness(status: &FeedStatus) -> Freshness {
    match status {
        FeedStatus::Fresh => Freshness::Current,
        FeedStatus::Stale => Freshness::Stale,
        _ => Freshness::Unavailable,
    }
}

/// The freshness picture: the snapshot's build time and the freshness of each
/// named section. Absent snapshot => empty + `None` age, which the verdict reads
/// as the whole suite view being unavailable.
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

/// Derive freshness from the snapshot. The data-bearing tool sections
/// (scripts/tools/findings) drive the freshness strip; jobs/proto presence is
/// surfaced via [`suite_view`] instead, matching Pulse's prior behavior.
pub fn freshness(snap: Option<&Snapshot>) -> SnapshotFreshness {
    let Some(s) = snap else {
        return SnapshotFreshness {
            built_at: None,
            sections: Vec::new(),
        };
    };
    SnapshotFreshness {
        built_at: Some(s.built_at.to_rfc3339()),
        sections: vec![
            ("scripts", section_freshness(&s.scripts.status)),
            ("tools", section_freshness(&s.tools.status)),
            ("findings", section_freshness(&s.findings.status)),
        ],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Attention & presence — derived from the snapshot's sections
// ─────────────────────────────────────────────────────────────────────────────

/// Severity of an attention item, in escalation order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// Map the snapshot's richer `Severity` onto Pulse's escalation scale. An absent
/// or unrecognized signal (`Unrated`/`Unknown`, or any future bucket) escalates to
/// `High`, never sinks to `Low`: on a security dashboard an unclassified value must
/// surface, not vanish below the attention threshold.
fn map_severity(s: WsSeverity) -> Severity {
    match s {
        WsSeverity::Critical => Severity::Critical,
        WsSeverity::High => Severity::High,
        WsSeverity::Medium => Severity::Medium,
        WsSeverity::Low | WsSeverity::Info => Severity::Low,
        _ => Severity::High,
    }
}

/// One thing worth the operator's attention, normalized across sources.
#[derive(Clone)]
pub struct Attention {
    /// What is affected (the finding subject / tool name).
    pub what: String,
    /// Why it matters (short reason).
    pub why: String,
    /// Which source it came from.
    pub source: String,
    pub severity: Severity,
}

/// Security findings (Bulwark) at High/Critical, surfaced as attention. Low/medium
/// inventory noise stays out of the verdict. Shared by [`suite_view`] and
/// [`bulwark`].
fn findings_attention(s: &Snapshot) -> Vec<Attention> {
    let Some(inv) = s.findings.data.as_ref() else {
        return Vec::new();
    };
    inv.findings
        .iter()
        .filter_map(|f| {
            let severity = map_severity(f.severity);
            if severity < Severity::High {
                return None;
            }
            let what = f
                .name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| f.id.0.clone());
            let why = f
                .description
                .clone()
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| "flagged by bulwark".to_string());
            Some(Attention {
                what,
                why,
                source: "bulwark".to_string(),
                severity,
            })
        })
        .collect()
}

/// Tools (ToolFoundry) that need attention: a failing health ratio, or a status
/// the producer marked "attention". Surfaced as High so they reach the verdict.
fn tools_attention(s: &Snapshot) -> Vec<Attention> {
    let Some(inv) = s.tools.data.as_ref() else {
        return Vec::new();
    };
    inv.tools
        .iter()
        .filter_map(|t| {
            let health_failing = t.health_total > 0 && t.health_passed < t.health_total;
            let flagged = t.status.trim().eq_ignore_ascii_case("attention");
            if !health_failing && !flagged {
                return None;
            }
            let what = if t.display_name.trim().is_empty() {
                t.id.0.clone()
            } else {
                t.display_name.clone()
            };
            let why = if health_failing {
                format!("health {}/{} passing", t.health_passed, t.health_total)
            } else {
                "needs attention".to_string()
            };
            Some(Attention {
                what,
                why,
                source: "toolfoundry".to_string(),
                severity: Severity::High,
            })
        })
        .collect()
}

/// The suite-level view, derived from the snapshot: per-source presence plus the
/// merged attention list (security findings + tool health). `None` when there is
/// no snapshot at all, which the verdict reads as the suite view being unavailable.
///
/// Named for historical reasons: it replaces the old RexOps-aggregator feed that
/// Pulse used to read separately. The data now comes entirely from the Workstate
/// snapshot, so there is no second aggregate to drift from.
#[derive(Clone)]
pub struct RexopsView {
    pub generated_at: Option<String>,
    /// (source name, present?) for each producer the snapshot carries a section for.
    pub sources: Vec<(String, bool)>,
    pub attention: Vec<Attention>,
}

/// Build the suite view from the snapshot. A section is "present" when it carries
/// data (a Fresh/Stale/FreshnessUnknown read); a Missing/Failed/unsupported
/// section reads as absent.
pub fn suite_view(snap: Option<&Snapshot>) -> Option<RexopsView> {
    let s = snap?;
    let sources = vec![
        ("workstate".to_string(), true),
        ("scriptvault".to_string(), s.scripts.data.is_some()),
        ("toolfoundry".to_string(), s.tools.data.is_some()),
        ("bulwark".to_string(), s.findings.data.is_some()),
        ("proto".to_string(), s.jobs.data.is_some()),
    ];
    let mut attention = findings_attention(s);
    attention.extend(tools_attention(s));
    Some(RexopsView {
        generated_at: Some(s.built_at.to_rfc3339()),
        sources,
        attention,
    })
}

/// Bulwark's contribution on its own: its security findings as attention plus a
/// presence flag. Kept as a distinct view for the source-confidence strip and as
/// the attention fallback used when there is no suite view at all.
#[derive(Clone)]
pub struct BulwarkView {
    pub attention: Vec<Attention>,
    pub present: bool,
}

pub fn bulwark(snap: Option<&Snapshot>) -> BulwarkView {
    let Some(s) = snap else {
        return BulwarkView {
            attention: Vec::new(),
            present: false,
        };
    };
    BulwarkView {
        attention: findings_attention(s),
        present: s.findings.data.is_some(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Jobs — Proto protocol runs from the snapshot's jobs section
// ─────────────────────────────────────────────────────────────────────────────

/// A run's outcome, as Pulse displays it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobOutcome {
    Passed,
    Failed,
    Running,
}

#[derive(Clone)]
pub struct Job {
    pub title: String,
    pub outcome: JobOutcome,
}

/// Map the snapshot's job outcome onto Pulse's. A non-terminal/unknown outcome
/// fails closed to `Running`, never `Passed`.
fn map_outcome(o: WsJobOutcome) -> JobOutcome {
    match o {
        WsJobOutcome::Passed => JobOutcome::Passed,
        WsJobOutcome::Failed => JobOutcome::Failed,
        _ => JobOutcome::Running,
    }
}

/// The protocol runs from the snapshot's jobs section (Proto). Workstate already
/// classified each run's outcome, so Pulse just maps it onto its own type.
pub fn jobs(snap: Option<&Snapshot>) -> Vec<Job> {
    let Some(inv) = snap.and_then(|s| s.jobs.data.as_ref()) else {
        return Vec::new();
    };
    inv.jobs
        .iter()
        .map(|j| Job {
            title: j.title.clone(),
            outcome: map_outcome(j.outcome),
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Environment / binary checks
// ─────────────────────────────────────────────────────────────────────────────

/// One suite binary and whether it is on `$PATH`. There is no `rex-doctor`
/// producer in the suite, so Pulse performs these env probes itself — purely local
/// `which`-style lookups that need no external feed.
#[derive(Clone, Copy)]
pub struct BinaryCheck {
    pub name: &'static str,
    pub present: bool,
}

/// The suite binaries Pulse expects an operator to have installed.
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

/// Whether `name` resolves to an executable on `$PATH` (in-process, no fork).
fn which(name: &str) -> bool {
    suite_core::path::which(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use workstate_schema::model::normalized::{
        Finding, FindingId, FindingInventory, Job as WsJob, JobId, JobInventory, Tool, ToolId,
        ToolInventory,
    };
    use workstate_schema::model::provenance::{FeedId, Provenance, Section};

    fn prov() -> Provenance {
        Provenance {
            feed_id: FeedId("x".to_string()),
            fetched_at: None,
            source_observed_at: None,
            dropped_records: 0,
        }
    }

    fn finding(id: &str, name: Option<&str>, sev: WsSeverity, desc: Option<&str>) -> Finding {
        Finding {
            id: FindingId(id.to_string()),
            name: name.map(str::to_string),
            rule_id: None,
            description: desc.map(str::to_string),
            severity: sev,
            raw_severity: None,
            category: None,
            location: "unknown".to_string(),
            path: None,
            risk: None,
            owner: None,
        }
    }

    fn tool(id: &str, display: &str, status: &str, passed: u32, total: u32) -> Tool {
        Tool {
            id: ToolId(id.to_string()),
            display_name: display.to_string(),
            owner: String::new(),
            project: String::new(),
            lifecycle_state: String::new(),
            status: status.to_string(),
            review_due: None,
            review_after: None,
            review_due_flag: false,
            drifted: false,
            health_passed: passed,
            health_total: total,
            manifest_path: String::new(),
        }
    }

    fn data_section<T>(status: FeedStatus, data: T) -> Section<T> {
        Section {
            status,
            provenance: prov(),
            data: Some(data),
        }
    }

    /// A representative v5 snapshot: scripts Missing, tools Stale (one
    /// health-failing tool), findings Fresh (one critical + one low), jobs Fresh
    /// (passed/failed/running).
    fn sample() -> Snapshot {
        let tools = ToolInventory {
            as_of: "2026-06-14".to_string(),
            tool_count: 1,
            attention_count: 0,
            tools: vec![tool("backup-home", "Backup Home", "ok", 1, 3)],
            dropped_records: 0,
        };
        let findings = FindingInventory {
            generated_at: "2026-06-14".to_string(),
            findings: vec![
                finding(
                    "deploy-prod.sh",
                    Some("deploy-prod.sh"),
                    WsSeverity::Critical,
                    Some("AWS access key ID detected"),
                ),
                finding("noise.sh", None, WsSeverity::Low, Some("style nit")),
            ],
            dropped_records: 0,
        };
        let jobs = JobInventory {
            generated_at: "2026-06-14".to_string(),
            jobs: vec![
                WsJob {
                    id: JobId("r1".to_string()),
                    title: "Nightly backup".to_string(),
                    outcome: WsJobOutcome::Passed,
                },
                WsJob {
                    id: JobId("r2".to_string()),
                    title: "Deploy".to_string(),
                    outcome: WsJobOutcome::Failed,
                },
                WsJob {
                    id: JobId("r3".to_string()),
                    title: "Live".to_string(),
                    outcome: WsJobOutcome::Running,
                },
            ],
            dropped_records: 0,
        };
        Snapshot::new(
            Utc.with_ymd_and_hms(2026, 6, 14, 12, 0, 0).unwrap(),
            Section::missing(FeedId("scriptvault".to_string())),
            data_section(FeedStatus::Stale, tools),
            data_section(FeedStatus::Fresh, findings),
            data_section(FeedStatus::Fresh, jobs),
        )
    }

    fn unique(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pulse-{tag}-{}-{}", std::process::id(), nanos))
    }

    #[test]
    fn no_snapshot_is_empty_everything() {
        assert!(freshness(None).sections.is_empty());
        assert!(freshness(None).built_at.is_none());
        assert!(suite_view(None).is_none());
        assert!(!bulwark(None).present);
        assert!(jobs(None).is_empty());
    }

    #[test]
    fn freshness_maps_each_section_status() {
        let s = sample();
        let f = freshness(Some(&s));
        assert_eq!(f.built_at.as_deref(), Some("2026-06-14T12:00:00+00:00"));
        assert_eq!(
            f.sections,
            vec![
                ("scripts", Freshness::Unavailable), // Missing
                ("tools", Freshness::Stale),
                ("findings", Freshness::Current), // Fresh
            ]
        );
        assert!(f.any_stale());
        assert_eq!(f.worst(), Some(Freshness::Unavailable));
    }

    #[test]
    fn suite_view_reports_presence_and_merged_attention() {
        let s = sample();
        let v = suite_view(Some(&s)).expect("snapshot present");
        let present = |name: &str| v.sources.iter().any(|(n, p)| n == name && *p);
        let absent = |name: &str| v.sources.iter().any(|(n, p)| n == name && !*p);
        assert!(present("workstate"));
        assert!(absent("scriptvault")); // scripts section Missing
        assert!(present("toolfoundry")); // tools has data
        assert!(present("bulwark")); // findings has data
        assert!(present("proto")); // jobs has data

        // Attention merges security findings + tool health; the low finding is filtered.
        assert_eq!(v.attention.len(), 2);
        let crit = v
            .attention
            .iter()
            .find(|a| a.severity == Severity::Critical)
            .unwrap();
        assert_eq!(crit.what, "deploy-prod.sh");
        assert_eq!(crit.source, "bulwark");
        let health = v
            .attention
            .iter()
            .find(|a| a.source == "toolfoundry")
            .unwrap();
        assert_eq!(health.what, "Backup Home");
        assert_eq!(health.severity, Severity::High);
    }

    #[test]
    fn bulwark_only_surfaces_high_and_critical() {
        let s = sample();
        let b = bulwark(Some(&s));
        assert!(b.present);
        assert_eq!(b.attention.len(), 1); // the critical; the low finding is filtered
        assert!(b.attention.iter().all(|a| a.severity >= Severity::High));
    }

    #[test]
    fn unrated_or_unknown_severity_escalates_not_drops() {
        // An unrated/unknown finding severity must surface as High, never be dropped
        // below the attention threshold.
        let mut s = sample();
        if let Some(inv) = s.findings.data.as_mut() {
            inv.findings = vec![finding(
                "mystery.sh",
                None,
                WsSeverity::Unrated,
                Some("no severity signal"),
            )];
        }
        let b = bulwark(Some(&s));
        assert_eq!(
            b.attention.len(),
            1,
            "unrated must escalate to High, not drop"
        );
        assert_eq!(b.attention[0].severity, Severity::High);
        assert_eq!(b.attention[0].what, "mystery.sh");
    }

    #[test]
    fn jobs_map_outcomes_from_the_snapshot() {
        let s = sample();
        let js = jobs(Some(&s));
        assert_eq!(js.len(), 3);
        let by = |title: &str| js.iter().find(|j| j.title == title).unwrap().outcome;
        assert_eq!(by("Nightly backup"), JobOutcome::Passed);
        assert_eq!(by("Deploy"), JobOutcome::Failed);
        assert_eq!(by("Live"), JobOutcome::Running);
    }

    #[test]
    fn load_missing_or_malformed_reads_as_none() {
        // A root with no snapshot at all.
        let missing = DataDir {
            root: unique("missing"),
        };
        assert!(load(&missing).is_none());

        // A malformed snapshot at the canonical path reads as None (never panics).
        let root = unique("malformed");
        let path = workstate_schema::snapshot_path_under(&root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let bad = DataDir { root: root.clone() };
        assert!(load(&bad).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn binary_checks_cover_the_suite_and_detect_a_real_binary() {
        let checks = read_binaries();
        assert_eq!(checks.len(), SUITE_BINARIES.len());
        // `which` finds `sh` (every PATH has it) and rejects a nonsense name —
        // sanity-checking the probe without depending on suite tools being installed.
        assert!(which("sh"));
        assert!(!which("definitely-not-a-real-binary-xyzzy"));
    }
}

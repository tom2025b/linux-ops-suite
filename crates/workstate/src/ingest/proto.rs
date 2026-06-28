use chrono::{DateTime, Utc};

use crate::ingest::{parse_source_timestamp, FeedError, FeedSource, FeedTransport};
use crate::model::normalized::{Job, JobId, JobInventory, JobOutcome};
use crate::model::provenance::FeedId;
use crate::model::raw::ProtoRaw;

/// Adapter that obtains Proto's neutral Workstate feed (protocol runs) and
/// normalizes it into the snapshot's `jobs` section.
///
/// Mirrors the other adapters exactly: the bytes come from a [`FeedTransport`] —
/// either a file (tests / `--output`) or, in the live flow, `proto workstate-feed`
/// spawned as a subprocess. Proto not being installed degrades the section to
/// Missing, never an error — the same graceful story as every other feed.
pub struct ProtoFeed {
    /// Where the feed's raw JSON comes from. A missing file / uninstalled tool
    /// degrades to a Missing section; a present-but-broken source to Failed.
    pub transport: FeedTransport,
}

impl ProtoFeed {
    /// Construct from a file path (tests and the `OUTPUT`-override read path).
    pub fn from_path(path: String) -> Self {
        ProtoFeed {
            transport: FeedTransport::File(path),
        }
    }

    /// Construct from a producer command — `proto workstate-feed …` — spawned each
    /// build so the feed is live.
    pub fn from_command(program: &str, args: &[&str]) -> Self {
        ProtoFeed {
            transport: FeedTransport::command(program, args),
        }
    }

    /// Pure parse step: raw JSON text → typed `ProtoRaw`. A `serde_json` failure
    /// maps to `FeedError::Parse` (→ Failed section), never a crash.
    fn parse(text: &str) -> Result<ProtoRaw, FeedError> {
        serde_json::from_str(text).map_err(|e| FeedError::Parse(e.to_string()))
    }
}

impl FeedSource for ProtoFeed {
    type Raw = ProtoRaw;
    type Normalized = JobInventory;

    fn feed_id(&self) -> FeedId {
        FeedId("proto".to_string())
    }

    fn supported_schema_version(&self) -> Option<i64> {
        Some(1)
    }

    fn schema_version(&self, raw: &Self::Raw) -> Option<i64> {
        raw.schema_version
    }

    fn expected_source_tool(&self) -> Option<&str> {
        Some("proto")
    }

    fn source_tool<'a>(&self, raw: &'a Self::Raw) -> Option<&'a str> {
        Some(&raw.source_tool)
    }

    fn source_observed_at(&self, raw: &Self::Raw) -> Option<DateTime<Utc>> {
        parse_source_timestamp(&raw.generated_at)
    }

    fn fetch(&self) -> Result<Self::Raw, FeedError> {
        let text = self.transport.read()?;
        Self::parse(&text)
    }

    /// Map the raw feed into the canonical `JobInventory`.
    ///
    /// PURE and INFALLIBLE per the trait: a bad record is skipped-and-dropped, never
    /// an error. A run without a usable id is dropped (and counted); its title falls
    /// back to "protocol run"; its outcome is bucketed by `bucket_outcome`.
    fn normalize(&self, raw: Self::Raw) -> Self::Normalized {
        let generated_at = raw.generated_at;
        let mut dropped = 0usize;
        let jobs: Vec<Job> = raw
            .items
            .into_iter()
            .filter_map(|ri| {
                let id = ri.id.as_deref().map(str::trim).unwrap_or("");
                if id.is_empty() {
                    dropped += 1; // count the loss before dropping
                    return None;
                }
                let title = ri
                    .protocol_title
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "protocol run".to_string());
                Some(Job {
                    id: JobId(id.to_string()),
                    title,
                    outcome: derive_outcome(&ri.status, ri.failed),
                })
            })
            .collect();

        JobInventory {
            generated_at,
            jobs,
            dropped_records: dropped,
        }
    }
}

/// Derive the canonical [`JobOutcome`] from Proto's completion `status` and failed
/// step count — the classification RexOps' Pulse view used to do over raw session
/// files, now done ONCE, in the producer.
///
/// FAILS CLOSED: any failed step is `Failed`; a fully `complete` run with no
/// failures is `Passed`; everything else ("incomplete", or an unrecognized status)
/// is `Running`, never `Passed`.
fn derive_outcome(status: &str, failed: u32) -> JobOutcome {
    if failed > 0 {
        return JobOutcome::Failed;
    }
    match status.trim().to_ascii_lowercase().as_str() {
        "complete" => JobOutcome::Passed,
        _ => JobOutcome::Running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(json: &str) -> ProtoRaw {
        serde_json::from_str(json).expect("fixture parses")
    }

    #[test]
    fn derives_outcome_from_status_and_failed_count_failing_closed() {
        // Any failure → Failed, even on a complete run.
        assert_eq!(derive_outcome("complete", 1), JobOutcome::Failed);
        // Complete with no failures → Passed.
        assert_eq!(derive_outcome("complete", 0), JobOutcome::Passed);
        assert_eq!(derive_outcome("  COMPLETE ", 0), JobOutcome::Passed);
        // Incomplete (still running) → Running, never Passed.
        assert_eq!(derive_outcome("incomplete", 0), JobOutcome::Running);
        // Unknown / blank status fails closed to Running.
        assert_eq!(derive_outcome("weird", 0), JobOutcome::Running);
        assert_eq!(derive_outcome("", 0), JobOutcome::Running);
    }

    #[test]
    fn normalize_drops_idless_runs_and_classifies_outcomes() {
        let feed = ProtoFeed::from_path(String::new());
        let inv = feed.normalize(raw(r#"{
                "schema_version": 1, "source_tool": "proto", "generated_at": "2026-06-20",
                "item_count": 4,
                "items": [
                    {"id": "run-1", "protocol_title": "Nightly backup", "status": "complete", "passed": 3, "failed": 0, "skipped": 0},
                    {"id": "run-2", "protocol_title": "Deploy", "status": "complete", "passed": 1, "failed": 2, "skipped": 0},
                    {"id": "run-3", "status": "incomplete", "passed": 0, "failed": 0, "skipped": 0},
                    {"id": "  ", "protocol_title": "no id", "status": "complete", "passed": 1, "failed": 0, "skipped": 0}
                ]
            }"#));
        assert_eq!(inv.jobs.len(), 3);
        assert_eq!(inv.dropped_records, 1);
        assert_eq!(inv.jobs[0].title, "Nightly backup");
        assert_eq!(inv.jobs[0].outcome, JobOutcome::Passed);
        assert_eq!(inv.jobs[1].outcome, JobOutcome::Failed);
        // Missing title falls back; incomplete → Running.
        assert_eq!(inv.jobs[2].title, "protocol run");
        assert_eq!(inv.jobs[2].outcome, JobOutcome::Running);
    }
}

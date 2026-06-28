use serde::{Deserialize, Serialize};

/// Outcome of a Proto protocol run, normalized to a fixed bucket set.
///
/// `#[non_exhaustive]` (house style, like `Severity`/`FeedStatus`): consumers must
/// keep a wildcard arm, so a future outcome added behind a schema bump degrades on
/// the consumer rather than breaking its build.
///
/// FAILS CLOSED on an unrecognized/blank value: the adapter buckets it to `Running`,
/// never `Passed`. An unclassifiable run must not read as a green pass on a
/// dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum JobOutcome {
    /// Every step completed and none failed.
    Passed,
    /// At least one step failed.
    Failed,
    /// Still in progress (or outcome not yet determinable).
    Running,
}

/// Identifies a protocol run (Proto's session id).
///
/// Newtype so a job id can't be confused with any other id in a signature; it is a
/// stable label for "which run", not a uniqueness guarantee across re-runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub String);

/// A canonical protocol-run record from Proto.
///
/// `deny_unknown_fields` keeps the persisted shape an explicit allowlist (same
/// policy as `Finding`): an unmodeled Proto field can't silently ride into the
/// snapshot. The run's classification (`outcome`) is computed by the Proto adapter,
/// so consumers read a decided result instead of re-deriving it from raw steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    /// The run's id (Proto session id). MANDATORY: a run with no id has no stable
    /// identity, so the adapter drops such records rather than inventing one.
    pub id: JobId,
    /// Human-facing run title (Proto's `protocol_title`); "protocol run" when the
    /// feed gave none, so it is always displayable.
    pub title: String,
    /// The normalized run outcome.
    pub outcome: JobOutcome,
}

/// Canonical Proto job inventory plus the feed-level fields consumers read.
///
/// Mirrors `FindingInventory`: a `generated_at` envelope string plus the normalized
/// records and the drop count the compiler lifts onto provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInventory {
    /// Proto's source generation string (drives freshness via `source_observed_at`).
    pub generated_at: String,
    /// Normalized run records.
    pub jobs: Vec<Job>,
    /// How many raw runs normalization DROPPED (no usable id). The compiler copies
    /// this onto `Provenance.dropped_records` so the loss is never silent.
    /// `#[serde(default)]` so older snapshots without the field deserialize as `0`.
    #[serde(default)]
    pub dropped_records: usize,
}

use crate::core::protocol::Protocol; // a session is created FROM a protocol
use chrono::{DateTime, Timelike, Utc}; // Timelike: with_nanosecond for second-precision
use serde::{Deserialize, Serialize};

// The integer major version of the SESSION file format. Per the suite contract
// rules: present on every export, an integer, bumped only on a breaking change.
// Additive (new optional fields) keeps this the same; renames/removals bump it.
const SESSION_SCHEMA_VERSION: u32 = 1;

// -----------------------------------------------------------------------------
// now_secs — the current UTC time, truncated to whole seconds.
// -----------------------------------------------------------------------------
// Every session timestamp (started/answered/finished/generated) is stamped with
// this so the serialized RFC3339 reads "2026-06-06T12:00:00Z" — matching the
// contract's schema examples and the workstate-feed-validation protocol — rather
// than carrying noisy nanoseconds. Session timing is human-paced; sub-second
// precision is meaningless here and just makes the audit JSON harder to read.
pub fn now_secs() -> DateTime<Utc> {
    let now = Utc::now();
    // with_nanosecond(0) only returns None for invalid values; 0 is always valid.
    now.with_nanosecond(0).unwrap_or(now)
}

// -----------------------------------------------------------------------------
// Session — one execution of a protocol.
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    // --- Contract header (kept first so the JSON leads with provenance) -----
    // Integer schema version, validated by a future consumer before it trusts
    // the rest of the file (suite rule: "validate schema_version first").
    pub schema_version: u32,

    // Which tool produced this file. The suite recommends every export carry
    // its producer's short name so a consumer can attribute the data.
    pub source_tool: String,

    // When this session record was generated/last written, RFC3339. Mirrors the
    // suite's preferred `generated_at` field name and format.
    pub generated_at: DateTime<Utc>,

    // --- What was run -------------------------------------------------------
    // The protocol's id and title, COPIED in so the session is self-describing
    // even if the protocol file later changes or moves. A consumer shouldn't
    // need the original YAML to understand a session.
    pub protocol_id: String,
    pub protocol_title: String,

    // --- Timing -------------------------------------------------------------
    // When the operator began the walkthrough. `started_at` is set at creation.
    pub started_at: DateTime<Utc>,

    // When the run reached its end. `None` while a session is still in progress;
    // `Some(..)` once every step has an outcome. `skip_serializing_if` keeps the
    // JSON tidy by omitting the key entirely while it's None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,

    // --- Outcomes -----------------------------------------------------------
    // One entry per step, in the protocol's order. Created up front as all
    // Pending, then filled in as the operator answers.
    pub steps: Vec<StepResult>,
}

impl Session {
    // Build a fresh, all-Pending session from a protocol. This is the bridge
    // between the recipe (Protocol) and the run (Session): we snapshot the
    // identifying fields and pre-create a Pending result for each step so the
    // run engine only ever has to UPDATE entries, never insert them.
    pub fn new(protocol: &Protocol) -> Self {
        let now = now_secs(); // single timestamp for both started_at + generated_at
        Session {
            schema_version: SESSION_SCHEMA_VERSION,
            source_tool: "proto".to_string(),
            generated_at: now,
            protocol_id: protocol.id.clone(),
            protocol_title: protocol.title.clone(),
            started_at: now,
            finished_at: None,
            // Map each protocol Step to a Pending StepResult, preserving order.
            steps: protocol
                .steps
                .iter()
                .map(|s| StepResult::pending(&s.id))
                .collect(),
        }
    }

    // True once every step has been answered (none left Pending). The run engine
    // uses this to decide when to stamp `finished_at`.
    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|r| r.status != StepStatus::Pending)
    }

    // Count this session's step outcomes into one struct. Centralizing the count
    // here means `run`'s summary and `sessions`' listing can't drift apart — they
    // were both hand-counting the same five categories before this existed.
    pub fn tally(&self) -> Tally {
        let mut t = Tally::default();
        for r in &self.steps {
            match r.status {
                StepStatus::Passed => t.passed += 1,
                StepStatus::Failed => t.failed += 1,
                StepStatus::Skipped => t.skipped += 1,
                StepStatus::Acknowledged => t.info += 1,
                StepStatus::Pending => t.pending += 1,
            }
        }
        t
    }
}

// -----------------------------------------------------------------------------
// Tally — a count of step outcomes, with one shared way to render it.
// -----------------------------------------------------------------------------
// A plain count struct (not stored, computed on demand via Session::tally). It
// owns the "compact summary line" format so every command prints outcomes the
// same way — the listing and the run summary stay in lockstep by construction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub info: usize,
    pub pending: usize,
}

impl Tally {
    // A compact "N passed, N failed, …" string, OMITTING any zero category so an
    // all-green run reads "11 passed" rather than "11 passed, 0 failed, 0 skipped".
    // Returns "no steps" only when there genuinely are none.
    pub fn summary_line(&self) -> String {
        let mut parts = Vec::new();
        if self.passed > 0 {
            parts.push(format!("{} passed", self.passed));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        if self.info > 0 {
            parts.push(format!("{} info", self.info));
        }
        if self.pending > 0 {
            parts.push(format!("{} pending", self.pending));
        }
        if parts.is_empty() {
            "no steps".to_string()
        } else {
            parts.join(", ")
        }
    }
}

// -----------------------------------------------------------------------------
// StepResult — the outcome of one step within a session.
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    // The protocol Step::id this result corresponds to. The link between recipe
    // and record is by ID, not position, so the JSON stays meaningful on its own.
    pub step_id: String,

    // Where this step stands: Pending / Passed / Failed / Skipped / Acknowledged.
    pub status: StepStatus,

    // When the operator answered this step. None while still Pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_at: Option<DateTime<Utc>>,

    // Optional free-text the operator added (e.g. why a step was skipped, or a
    // value they observed). Empty by default; omitted from JSON when blank.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl StepResult {
    // The starting state for every step: not yet answered.
    fn pending(step_id: &str) -> Self {
        StepResult {
            step_id: step_id.to_string(),
            status: StepStatus::Pending,
            answered_at: None,
            note: String::new(),
        }
    }
}

// -----------------------------------------------------------------------------
// StepStatus — the lifecycle state of a single step's outcome.
// -----------------------------------------------------------------------------
// snake_case on the wire ("passed", "skipped") to match JSON conventions while
// staying CamelCase in Rust. PartialEq/Eq let us compare against Pending above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StepStatus {
    // Not yet reached/answered. The initial state for every step.
    Pending,
    // A ManualCheck/Command step the operator confirmed succeeded.
    Passed,
    // A ManualCheck/Command step the operator marked as not satisfied.
    Failed,
    // The operator chose to skip this step (recorded, not silently dropped).
    Skipped,
    // An Info step the operator has READ — there is no pass/fail for info, just
    // acknowledgement that they saw it.
    Acknowledged,
}

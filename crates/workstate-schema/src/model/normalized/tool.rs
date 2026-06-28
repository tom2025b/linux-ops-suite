// chrono types used by the reserved v3 review timestamp and ToolFoundry's
// lifecycle review date.
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// A newtype for a tool identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolId(pub String);

/// A canonical tool/binary record as reported by ToolFoundry.
///
/// Tools are GLOBAL (no HostId): a tool in the foundry is an organization-level
/// asset (owned by a person, belonging to a project), not something pinned to a
/// single machine. (Decision B — Tool/Script stay global.)
///
/// FIELD TYPE CHOICES:
///   * `lifecycle_state` / `status` are `String` for now — ToolFoundry's set of
///     states is not yet pinned down as a closed list, and a `String` cannot
///     hard-fail normalization on an unanticipated value. They can be promoted
///     to enums behind a schema bump once the upstream set is frozen.
///   * `review_due` stays nullable for the current v3 RexOps consumer. The concrete
///     ToolFoundry review date is carried by the additive `review_after` field.
///   * `health_passed` / `health_total` are a simple pair of counts — the
///     cheapest honest way to express "N of M health checks passed" without
///     inventing a richer health type before we need one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    /// Stable identity of the tool.
    pub id: ToolId,
    /// Human-facing name shown in the cockpit.
    pub display_name: String,
    /// Who owns this tool.
    pub owner: String,
    /// Which project it belongs to.
    pub project: String,
    /// Lifecycle stage (e.g. "active", "deprecated"). String until the set is frozen.
    pub lifecycle_state: String,
    /// Operational status (e.g. "ok", "broken"). String until the set is frozen.
    pub status: String,
    /// Reserved timestamp field in the v3 snapshot. Kept null for current RexOps
    /// compatibility; ToolFoundry's concrete review date is emitted as `review_after`.
    pub review_due: Option<DateTime<Utc>>,
    /// ToolFoundry's declared lifecycle review date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_after: Option<NaiveDate>,
    /// ToolFoundry's current wire-level "review is due" flag.
    ///
    /// Kept alongside `review_after` so consumers can distinguish the scheduled
    /// lifecycle date from the producer's current "due now" decision.
    pub review_due_flag: bool,
    /// Whether the tool has drifted from its declared manifest.
    pub drifted: bool,
    /// Count of health checks that passed.
    pub health_passed: u32,
    /// Total health checks defined for the tool.
    pub health_total: u32,
    /// Path to the tool's manifest, for traceability.
    pub manifest_path: String,
}

/// Canonical ToolFoundry inventory plus feed-level summary fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInventory {
    /// ToolFoundry's source generation date/string.
    pub as_of: String,
    /// Number of tools in `tools`. DERIVED by Workstate from the normalized list
    /// (not echoed from ToolFoundry's self-reported envelope count), so it can never
    /// disagree with `tools.len()`.
    pub tool_count: usize,
    /// Number of tools needing attention. DERIVED by Workstate as the count of
    /// `tools` whose `status` is "attention" (case/whitespace insensitive), so it is
    /// always consistent with the records in `tools`.
    pub attention_count: usize,
    /// Normalized tool records.
    pub tools: Vec<Tool>,
    /// How many raw records normalization DROPPED (no usable id). The compiler
    /// copies this onto `Provenance.dropped_records` so the loss is never silent.
    /// `#[serde(default)]` so older snapshots without the field deserialize as `0`.
    #[serde(default)]
    pub dropped_records: usize,
}

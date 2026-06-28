use serde::{Deserialize, Serialize};
// A loose, schemaless JSON value. We use it ONLY to capture fields ScriptVault
// emits that we don't model yet (its `rest` bag), so an upstream addition never
// hard-fails our parse. It is NOT used for whole records — those are typed.
use serde_json::Value;
// `BTreeMap` (not `HashMap`) for the catch-all `rest` bag: it keeps keys in a
// stable, sorted order. That makes debug output deterministic and the serialized
// snapshot byte-stable, so diffs in review are meaningful.
use std::collections::BTreeMap;

/// Raw shape of Bulwark's *scan export* feed (security findings).
///
/// Matches current RexOps behavior: this mirrors the envelope RexOps's Bulwark scan
/// consumer parses (`rexops-adapters/src/bulwark_feed.rs::BulwarkScanInfo`/`ScanItem`),
/// whose target contract is `contracts/bulwark.scan.schema.json`. That contract is
/// explicitly PROVISIONAL — it fixes only the OUTER envelope (`schema_version`,
/// `source_tool`, `generated_at`, `items[]`) and warns "do not treat the item shape
/// as final". So we type the envelope exactly as RexOps does and keep each item
/// permissive (see `BulwarkRawItem`) so an upstream item-shape change never hard-fails.
///
/// Bulwark emits SECURITY FINDINGS (results of content/script scans), NOT host
/// inventory: each item is "rule X fired on subject Y with severity Z". The richer
/// per-finding fields the canonical `Finding` needs (`rule_id`, `description`,
/// `category`, `location`) are matches RexOps's LIVE-scan finding type
/// (`rexops-adapters/src/bulwark.rs::BulwarkFinding`) and surface as opportunistic
/// optional fields on the item — present in the real fixture, absent-tolerant otherwise.
///
/// PERMISSIVENESS MIRRORS REXOPS: EVERY field carries `#[serde(default)]`, so a
/// sparse export (missing `items`, even a missing `schema_version`) still
/// deserializes instead of erroring — the same leniency RexOps relies on.
///
/// `schema_version` is `Option<i64>` (NOT a required `i64` like RexOps's
/// `BulwarkScanInfo`): `i64` to match the wire type, and `Option` + `default` so a
/// feed that OMITS the version still PARSES to `None` rather than hard-failing at
/// the wire boundary. Leniency here is only about parsing. The compiler then GATES
/// strictly: `compile_section` rejects a version that is missing or not exactly the
/// supported one as `UnsupportedVersion` and drops the data. So the wire stays
/// permissive while the persisted snapshot stays strict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BulwarkRaw {
    /// Major schema version of the export, if stamped. `Option<i64>` + `default`:
    /// a missing version parses to `None` (lenient at the wire) rather than erroring.
    /// The compiler gates on it strictly before normalization (see `compile_section`).
    #[serde(default)]
    pub schema_version: Option<i64>,
    /// How Bulwark labels itself ("bulwark"). Lenient like RexOps: read for
    /// cross-checking provenance, never rejected on mismatch. `default` yields ""
    /// when absent rather than failing the parse.
    #[serde(default)]
    pub source_tool: String,
    /// When Bulwark generated this export. Kept as a raw `String` (NOT parsed to a
    /// `DateTime`): RexOps keeps it as a string and real exports carry loose values
    /// like "2026-06-04" that are not RFC 3339. The adapter parses this into
    /// provenance's `source_observed_at` (via `parse_source_timestamp`) to drive
    /// `Fresh`/`Stale`; an unparseable value leaves the source time unknown.
    #[serde(default)]
    pub generated_at: String,
    /// The scanned items (security findings). `default` so an export with no `items`
    /// key still parses (as an empty list). Named `items` to match RexOps's wire
    /// field exactly (its envelope uses `items`, not `findings`).
    #[serde(default)]
    pub items: Vec<BulwarkRawItem>,
}

/// One raw scanned item as it appears on the wire, BEFORE normalization.
///
/// Permissive, MIRRORING REXOPS's `ScanItem`:
///   * EVERY field is `Option` + `#[serde(default)]`. Bulwark's provisional contract
///     guarantees none of them, so we never reject a sparse item. The strict
///     "a finding needs a subject" rule is a NORMALIZATION decision (enforced in the
///     adapter), not a parse-time one — one bad-but-parseable item never sinks the feed.
///   * `severity` stays a `String` (NOT the canonical `Severity` enum): RexOps reads
///     severity as a free string and buckets it with a trim+lowercase + "unknown"
///     fallback. Deserializing straight into the enum would wrongly route "critical"
///     (or any non-snake-case value) to `Unknown` — so bucketing is the ADAPTER's job.
///   * `location` is a loose `Value`: on the wire Bulwark's location is a TAGGED
///     UNION (`{type: json_path|byte_range|line|unknown}`, per `BulwarkFinding`).
///     The canonical `Finding.location` is a display `String` for v1, so we capture
///     the structured value verbatim here and FLATTEN it to a string in normalize.
///   * `#[serde(flatten)] rest` captures every other key verbatim (RexOps's
///     `additionalProperties` handling) so a provisional upstream schema can grow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BulwarkRawItem {
    /// The scanned SUBJECT's id (e.g. the flagged script filename). Optional at the
    /// wire boundary; promoted toward the canonical `FindingId` during normalization
    /// (with a `name` fallback, then dropped if neither yields a usable subject).
    #[serde(default)]
    pub id: Option<String>,
    /// Human name of the subject; used as a display fallback when `id` is absent
    /// (mirrors RexOps's `ScanItem::label()` precedence: id → name → "<unnamed>").
    #[serde(default)]
    pub name: Option<String>,
    /// Risk severity as a FREE string (e.g. "critical", "high"). Bucketed to the
    /// canonical `Severity` enum in normalize (trim + lowercase), never here.
    #[serde(default)]
    pub severity: Option<String>,
    /// Which Bulwark rule fired (e.g. "aws-key"). Grounded in `BulwarkFinding.rule_id`;
    /// present as a sibling key in the real export fixture.
    #[serde(default)]
    pub rule_id: Option<String>,
    /// Human-readable explanation of the finding. Grounded in
    /// `BulwarkFinding.description`; present in the real export fixture.
    #[serde(default)]
    pub description: Option<String>,
    /// Open-set category label (e.g. "secret_leakage"). Grounded in
    /// `BulwarkFinding.category`, whose type has a `Custom(String)` escape hatch — so
    /// the set is genuinely open and a `String` is the honest wire choice.
    #[serde(default)]
    pub category: Option<String>,
    /// Where in the subject the issue was found. Loose `Value` because the wire shape
    /// is a tagged union (`BulwarkLocation`); normalize flattens it to a display
    /// `String`. `Option` so an item without location data still parses.
    #[serde(default)]
    pub location: Option<Value>,
    /// Filesystem path of the scanned script. Pulled out as a typed field (rather
    /// than left in `rest`) because the canonical `Finding` now carries `path` as a
    /// first-class allowlisted field — downstream consumers key on it. `Option` so
    /// an item without a path still parses.
    #[serde(default)]
    pub path: Option<String>,
    /// Bulwark's free-form `risk` label. Typed for the same reason as `path`: it is
    /// an allowlisted canonical `Finding` field, not open passthrough.
    #[serde(default)]
    pub risk: Option<String>,
    /// Owning user/team. Typed for the same reason as `path`/`risk`.
    #[serde(default)]
    pub owner: Option<String>,
    /// Every other field the export carried, kept verbatim. `flatten` folds these
    /// sibling keys into this map instead of requiring a named field per key. This
    /// stays on the RAW item (forward-compat parsing); it is intentionally NOT
    /// copied onto the canonical `Finding`, whose fields are an explicit allowlist.
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

/// Raw shape of ScriptVault's *export* feed (managed-script inventory).
///
/// Matches current RexOps behavior: this mirrors the envelope RexOps's ScriptVault
/// consumer already parses (`rexops-adapters/src/scriptvault.rs::ScriptVaultInfo`),
/// whose target contract is `contracts/scriptvault.export.schema.json`. The
/// contract is explicitly PROVISIONAL: it fixes the OUTER envelope
/// (`schema_version`, `source_tool`, `generated_at`) plus a free-form `scripts[]`
/// and two id-string arrays (`favorites`, `recents`). We type the envelope and the
/// arrays exactly as RexOps does, and keep each script record permissive (see
/// `ScriptVaultRawScript`) so a future change to the item shape never hard-fails us.
///
/// PERMISSIVENESS MIRRORS REXOPS: EVERY field carries `#[serde(default)]`, so a
/// sparse export (missing `favorites`, even a missing `schema_version`) still
/// deserializes instead of erroring — the same leniency RexOps relies on.
///
/// `schema_version` is `Option<i64>`: `i64` (not `u32`) to match RexOps's wire
/// type, and `Option` + `default` so a feed that OMITS the version still PARSES to
/// `None` rather than hard-failing at the wire boundary. A REQUIRED `i64` would
/// hard-fail the missing-version case into a Parse/Failed section; keeping it
/// optional lets us capture "version absent" faithfully as `None`. PARSING is
/// lenient; GATING is not — `compile_section` then rejects a missing or
/// unsupported version as `UnsupportedVersion` (data dropped), which is stricter
/// than RexOps' display adapter and deliberately so (Workstate is the persisted
/// source of truth). See `compile_section` and the `compile_status` tests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScriptVaultRaw {
    /// Major schema version of the export, if stamped. `Option<i64>` + `default`:
    /// a missing version parses to `None` (lenient at the wire) rather than erroring.
    /// The compiler gates on it strictly before normalization (see `compile_section`).
    #[serde(default)]
    pub schema_version: Option<i64>,
    /// How ScriptVault labels itself ("scriptvault"). Lenient like RexOps: we read
    /// it for cross-checking provenance but never reject a mismatch. `default`
    /// yields "" when absent rather than failing the parse.
    #[serde(default)]
    pub source_tool: String,
    /// When ScriptVault generated this export. Kept as a raw `String` (NOT parsed
    /// to a `DateTime`): RexOps keeps it as a string, and real exports carry loose
    /// values like "2026-06-04" that are not RFC 3339. The adapter parses this into
    /// provenance's `source_observed_at` (via `parse_source_timestamp`) to drive
    /// `Fresh`/`Stale`; an unparseable value leaves the source time unknown.
    #[serde(default)]
    pub generated_at: String,
    /// The managed-script inventory. `default` so an export with no `scripts` key
    /// still parses (as an empty list) rather than erroring.
    #[serde(default)]
    pub scripts: Vec<ScriptVaultRawScript>,
    /// Favorite script ids (a string-id array per the contract, not per-script
    /// booleans). We model it for wire fidelity; normalization currently ignores
    /// it (the canonical `Script` has no "favorite" concept yet).
    #[serde(default)]
    pub favorites: Vec<String>,
    /// Recently launched script ids. Same treatment as `favorites`: modeled for
    /// fidelity, ignored by normalization for now.
    #[serde(default)]
    pub recents: Vec<String>,
}

/// One raw script entry as it appears on the wire, BEFORE normalization.
///
/// Permissive, MIRRORING REXOPS:
///   * `id`, `name`, `description` are all `Option` + `#[serde(default)]`. On the
///     wire ScriptVault guarantees none of them — RexOps treats all three as
///     optional, so we do too. (The MANDATORY-id rule is a NORMALIZATION decision,
///     enforced later in `normalize`, not a parse-time one. Keeping parse lenient
///     means one bad-but-parseable record never sinks the whole feed.)
///   * `#[serde(flatten)] rest` captures every other key verbatim — exactly
///     RexOps's `additionalProperties` handling. We never lose data and never
///     reject an unknown field, so a provisional upstream schema can grow freely.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScriptVaultRawScript {
    /// Stable id, if the export provided one. Optional at the wire boundary;
    /// promoted to MANDATORY during normalization (a record without it is dropped).
    #[serde(default)]
    pub id: Option<String>,
    /// Human-readable name, if present.
    #[serde(default)]
    pub name: Option<String>,
    /// Free-text description, if present.
    #[serde(default)]
    pub description: Option<String>,
    /// Every other field the export carried, kept verbatim. `flatten` folds these
    /// sibling keys into this map instead of requiring a named field per key.
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

/// Raw shape of ToolFoundry's neutral Workstate feed.
///
/// Matches current RexOps behavior: this mirrors ToolFoundry's neutral Workstate feed,
/// whose target contract is `contracts/toolfoundry.workstate-feed.v1.schema.json`.
/// Unlike Bulwark/ScriptVault, ToolFoundry's contract is NOT provisional — its item
/// shape is fixed (`additionalProperties: true`), so we type every contract field
/// and let serde silently ignore any extra keys ToolFoundry adds later. (That is
/// `ToolFoundryRawTool` has NO `#[serde(flatten)] rest` bag because there is no
/// provisional item shape to preserve, so unmodeled keys are dropped.)
///
/// PERMISSIVENESS: EVERY field carries `#[serde(default)]`, so a feed omitting an
/// optional field still parses.
///
/// `schema_version` is `Option<i64>`: `Option` + `default` so a feed that OMITS
/// the version still PARSES to `None` rather than hard-failing at the wire boundary.
/// As with the other feeds, parsing is lenient but gating is not — the compiler
/// rejects a missing or unsupported version as `UnsupportedVersion` (data dropped)
/// before normalization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolFoundryRaw {
    /// Major schema version of the feed, if stamped. `Option<i64>` + `default`: a
    /// missing version parses to `None` (lenient at the wire). The compiler gates on
    /// it strictly before normalization (see `compile_section`).
    #[serde(default)]
    pub schema_version: Option<i64>,
    /// How ToolFoundry labels itself ("toolfoundry"). Lenient like the other feeds.
    #[serde(default)]
    pub source_tool: String,
    /// When ToolFoundry generated this feed.
    #[serde(default)]
    pub generated_at: String,
    /// Semantic date ToolFoundry evaluated lifecycle review status against.
    #[serde(default)]
    pub as_of: String,
    /// ToolFoundry's own count of tools in the feed. Modeled for wire fidelity;
    /// normalization derives the canonical list from `tools` directly and ignores
    /// this (a self-reported count is not authoritative over the actual records).
    #[serde(default)]
    pub tool_count: usize,
    /// ToolFoundry's own count of tools needing attention. Same treatment as
    /// `tool_count`: modeled for fidelity, not consumed by normalization.
    #[serde(default)]
    pub attention_count: usize,
    /// The tool inventory. `default` so a feed with no `tools` key still parses
    /// (as an empty list) rather than erroring.
    #[serde(default)]
    pub tools: Vec<ToolFoundryRawTool>,
}

/// One raw tool record as it appears on the wire, BEFORE normalization.
///
/// Mirrors ToolFoundry's neutral Workstate feed. The concrete lifecycle date is
/// `review_after` plus `review_due_flag`; the existing Workstate v3 `review_due`
/// field stays null during normalization for current RexOps compatibility.
///
/// EVERY field is `#[serde(default)]`; unknown extra keys are silently ignored
/// (serde's default), matching `additionalProperties: true`.
/// There is NO `rest` bag (see `ToolFoundryRaw`'s note): ToolFoundry's record is fixed
/// by a non-provisional contract, so unmodeled keys are dropped, not preserved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolFoundryRawTool {
    /// Stable id of the tool. Optional at the wire boundary; promoted to MANDATORY
    /// during normalization (a record without a usable id is dropped).
    #[serde(default)]
    pub id: Option<String>,
    /// Human-facing name shown in the cockpit.
    #[serde(default)]
    pub display_name: String,
    /// Who owns this tool.
    #[serde(default)]
    pub owner: String,
    /// Which project it belongs to.
    #[serde(default)]
    pub project: String,
    /// Lifecycle stage (e.g. "active", "deprecated"). String — set not frozen upstream.
    #[serde(default)]
    pub lifecycle_state: String,
    /// ToolFoundry's declared lifecycle review date.
    #[serde(default)]
    pub review_after: Option<String>,
    /// Whether a review is due as of the feed's `as_of` date. `Option` lets
    /// normalization distinguish "absent in an old feed" from "explicit false".
    #[serde(default)]
    pub review_due_flag: Option<bool>,
    /// Count of health checks that passed.
    #[serde(default)]
    pub health_passed: u32,
    /// Total health checks defined for the tool.
    #[serde(default)]
    pub health_total: u32,
    /// Whether the tool has drifted from its declared manifest.
    #[serde(default)]
    pub drifted: bool,
    /// Aggregate operational status (e.g. "ok", "attention"). String — set not frozen.
    #[serde(default)]
    pub status: String,
    /// Path to the tool's manifest, for traceability.
    #[serde(default)]
    pub manifest_path: String,
}

/// Raw shape of Proto's neutral Workstate feed (`proto.workstate-feed.v1`).
///
/// Mirrors `contracts/proto.workstate-feed.v1.schema.json`: a fixed envelope
/// (`schema_version`, `source_tool`, `generated_at`, `item_count`) plus an
/// `items[]` list of recent runs. Every field is `#[serde(default)]` so a sparse
/// feed still parses; the compiler gates the version strictly before normalization.
/// The feed reports raw step COUNTS and a completion `status`, NOT a pre-classified
/// outcome — the adapter derives `JobOutcome` from those (see `proto.rs`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProtoRaw {
    /// Major schema version of the feed, if stamped. `Option<i64>` + `default`: a
    /// missing version parses to `None` (lenient at the wire); the compiler gates on
    /// it strictly before normalization.
    #[serde(default)]
    pub schema_version: Option<i64>,
    /// How Proto labels itself ("proto"). Lenient like the other feeds.
    #[serde(default)]
    pub source_tool: String,
    /// When Proto generated this feed. Parsed into provenance's `source_observed_at`
    /// to drive freshness; an unparseable value leaves the source time unknown.
    #[serde(default)]
    pub generated_at: String,
    /// Proto's own item count. Advisory only (like ToolFoundry's `tool_count`):
    /// modeled for wire fidelity, never cross-checked against `items.len()`.
    #[serde(default)]
    pub item_count: usize,
    /// The recent-runs inventory. `default` so a feed with no `items` key still
    /// parses (as an empty list) rather than erroring.
    #[serde(default)]
    pub items: Vec<ProtoRawItem>,
}

/// One raw protocol run as it appears on the wire, BEFORE normalization.
///
/// Permissive: every field is `#[serde(default)]`, and unmodeled keys the contract
/// carries but the snapshot doesn't need (`protocol_id`, `started_at`,
/// `finished_at`, `passed`, `skipped`, `summary`) are simply ignored. The
/// MANDATORY-id rule and the outcome classification are NORMALIZATION decisions, so
/// one bad-but-parseable run never sinks the feed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProtoRawItem {
    /// Stable run id (the per-run session file's stem). Optional at the wire
    /// boundary; promoted to MANDATORY during normalization.
    #[serde(default)]
    pub id: Option<String>,
    /// Human-facing protocol title, if present (becomes the canonical `Job.title`).
    #[serde(default)]
    pub protocol_title: Option<String>,
    /// Completion state ("complete" once no step is pending, else "incomplete").
    /// Combined with `failed` to derive the canonical `JobOutcome`.
    #[serde(default)]
    pub status: String,
    /// Count of failed steps. Any failure makes the run's outcome `Failed`.
    #[serde(default)]
    pub failed: u32,
}

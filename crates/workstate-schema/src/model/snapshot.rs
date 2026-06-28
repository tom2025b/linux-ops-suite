use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Pull in the shared "spine" (Section/status/provenance) and the canonical
// domain models. `super` means "the parent module" (here, `model`).
use super::normalized::{FindingInventory, JobInventory, ScriptInventory, ToolInventory};
use super::provenance::Section;

/// The schema version of the Snapshot contract. RexOps reads this FIRST.
///
/// Version constant: it gives one obvious place to bump on any breaking change,
/// and lets both producer and consumer compare against a known value. Start at
/// 1; increment on every incompatible change to the JSON shape.
///
/// v5 (this version): adds a `jobs` section — `Section<JobInventory>` of normalized
///   Proto protocol runs (id, title, outcome) — so consumers read run status from
///   the snapshot instead of Proto's raw session files. A new REQUIRED top-level
///   `jobs` key reshapes the JSON, so the contract moves 4 -> 5.
///
/// v4: data-integrity hardening from the deep review. Three changes
/// that reshape the JSON RexOps reads:
///   1. `Provenance` gained `dropped_records: usize` — how many raw records a
///      section's normalization dropped (id-less / subject-less). Closes a
///      silent-data-loss gap: a lossy section is no longer indistinguishable from
///      a complete one. Each inventory payload also carries the same count.
///   2. `FeedStatus` gained a `FreshnessUnknown` variant — a feed that read cleanly
///      but whose source age cannot be determined is no longer mislabeled `Fresh`;
///      freshness now fails closed, consistent with the version gate.
///   3. `findings.data.findings[].rest` (the Bulwark passthrough bag) was REMOVED.
///      The canonical `Finding` now carries only an explicit allowlist of fields,
///      so an unmodeled wire field that holds matched content (a secret/PII) can no
///      longer leak into the persisted snapshot. This replaces a one-key `snippet`
///      denylist that scrubbed exactly one field. The audited, non-sensitive fields
///      downstream consumers actually need (`path`, `risk`, `owner`) were promoted
///      to first-class `Finding` fields rather than left in the removed bag.
///
/// NOTE (cross-repo): the shared hub JSON Schema
/// (`linux-ops-suite/contracts/workstate.snapshot.schema.json`, fetched by the CI
/// `contract` job) must track this contract — for v5, add the required `jobs`
/// section (a `Section<JobInventory>`: `generated_at`, `jobs[]`, `dropped_records`)
/// — or that job will fail.
///
/// v3: snapshot sections preserve RexOps-relevant feed envelope
/// fields instead of dropping them during normalization:
///   1. `scripts.data` is now `ScriptInventory` (`generated_at`, `scripts`,
///      `favorites`, `recents`).
///   2. `tools.data` is now `ToolInventory` (`as_of`, counts, `tools`).
///   3. `findings.data` is now `FindingInventory` (`generated_at`, `findings`).
///   4. per-script rest fields, ToolFoundry's review-due flag, and selected
///      Bulwark raw labels/severity/pass-through metadata are preserved.
///
/// v2: two breaking changes landed together —
///   1. `Provenance` gained `source_observed_at` (absolute source-generation
///      time, for real staleness detection).
///   2. The `hosts` section was dropped and replaced by `findings` (Bulwark
///      emits security findings, not host inventory); `Host`/`HostId` removed.
///
/// Both reshape the JSON RexOps reads, so the contract version moves 1 -> 2.
pub const SCHEMA_VERSION: u32 = 5;

/// The master snapshot of system state. This is what gets serialized to
/// `snapshot.json` and what RexOps deserializes.
///
/// DESIGN: per-domain sections, each wrapped in `Section<T>` so every domain
/// independently carries its own freshness/provenance and can degrade alone. The
/// payloads are inventory objects, preserving source envelope fields next to the
/// normalized record collections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Contract version. Consumers validate this before trusting the rest.
    pub schema_version: u32,
    /// When this snapshot was compiled (UTC). Lets RexOps judge overall age.
    pub built_at: DateTime<Utc>,
    /// Managed-scripts section (from ScriptVault).
    pub scripts: Section<ScriptInventory>,
    /// Tools/binaries section (from ToolFoundry).
    pub tools: Section<ToolInventory>,
    /// Security-findings section (from Bulwark), including Bulwark's feed-level
    /// generation string plus normalized finding records.
    pub findings: Section<FindingInventory>,
    /// Protocol-run (jobs) section (from Proto), including Proto's feed-level
    /// generation string plus normalized run records.
    pub jobs: Section<JobInventory>,
}

impl Snapshot {
    /// Stamp a snapshot with the current schema version and build time.
    ///
    /// It does NOT gather data — the compiler fills in the sections.
    pub fn new(
        built_at: DateTime<Utc>,
        scripts: Section<ScriptInventory>,
        tools: Section<ToolInventory>,
        findings: Section<FindingInventory>,
        jobs: Section<JobInventory>,
    ) -> Self {
        Snapshot {
            schema_version: SCHEMA_VERSION, // always tag with the current version
            built_at,
            scripts,
            tools,
            findings,
            jobs,
        }
    }
}

use chrono::{DateTime, Utc};

use crate::ingest::{parse_source_timestamp, FeedError, FeedSource, FeedTransport};
use crate::model::normalized::{Tool, ToolId, ToolInventory};
use crate::model::provenance::FeedId;
use crate::model::raw::ToolFoundryRaw;

/// Adapter that obtains ToolFoundry's neutral Workstate feed and normalizes it.
///
/// The bytes come from a [`FeedTransport`] — either a file (tests / explicit
/// `--output`) or, in the live flow, `toolfoundry workstate-feed` spawned as a
/// subprocess. The adapter is a long-lived value owned by the `SnapshotBuilder`,
/// so it owns its transport outright (no borrowed lifetimes).
pub struct ToolFoundryFeed {
    /// Where the feed's raw JSON comes from. A missing file / uninstalled tool
    /// degrades to a Missing section; a present-but-broken source to Failed.
    pub transport: FeedTransport,
}

impl ToolFoundryFeed {
    /// Construct from a file path (the original constructor; kept for tests and
    /// the `OUTPUT`-override read path).
    pub fn from_path(path: String) -> Self {
        ToolFoundryFeed {
            transport: FeedTransport::File(path),
        }
    }

    /// Construct from a producer command — `toolfoundry workstate-feed …` — spawned
    /// each build so the feed is live.
    pub fn from_command(program: &str, args: &[&str]) -> Self {
        ToolFoundryFeed {
            transport: FeedTransport::command(program, args),
        }
    }
}

impl ToolFoundryFeed {
    /// Pure parse step: raw JSON text → typed `ToolFoundryRaw`.
    ///
    /// PRIVATE: its only caller is `fetch`, in this same module. The adapter's
    /// behavioral tests live OUT of the crate in `tests/toolfoundry.rs` and drive
    /// parsing through the public `fetch()` against on-disk inputs — an external test
    /// crate could not call a `pub(crate)` fn either, so the narrowest visibility
    /// (private) is the honest one.
    ///
    /// Kept separate from `fetch`: keeping the parse pure (a `&str` in, a `Result`
    /// out, no disk) keeps the read path a thin wrapper whose only extra job is mapping
    /// I/O errors. This mirrors RexOps's own `parse_feed`/`read` split (and the other
    /// two adapters).
    ///
    /// A `serde_json` failure is mapped to `FeedError::Parse` (the variant the compiler
    /// turns into a `Failed` section) carrying the parser's message, so a malformed feed
    /// degrades visibly instead of crashing the build.
    fn parse(text: &str) -> Result<ToolFoundryRaw, FeedError> {
        // `serde_json::from_str` returns its own error type; `.map_err` converts it
        // into our domain error. We DON'T use `?`-with-`#[from]` here because
        // `FeedError` has no `From<serde_json::Error>` — a parse failure
        // must land in `Parse`, not be confused with an I/O failure.
        serde_json::from_str(text).map_err(|e| FeedError::Parse(e.to_string()))
    }
}

impl FeedSource for ToolFoundryFeed {
    // This adapter reads `ToolFoundryRaw` and produces a canonical ToolInventory.
    type Raw = ToolFoundryRaw;
    type Normalized = ToolInventory;

    fn feed_id(&self) -> FeedId {
        // Stable, lowercase identifier stamped into provenance for every tool fact
        // this feed contributes, so the snapshot is always attributable.
        FeedId("toolfoundry".to_string())
    }

    fn supported_schema_version(&self) -> Option<i64> {
        Some(1)
    }

    fn schema_version(&self, raw: &Self::Raw) -> Option<i64> {
        raw.schema_version
    }

    fn expected_source_tool(&self) -> Option<&str> {
        Some("toolfoundry")
    }

    fn source_tool<'a>(&self, raw: &'a Self::Raw) -> Option<&'a str> {
        Some(&raw.source_tool)
    }

    fn source_observed_at(&self, raw: &Self::Raw) -> Option<DateTime<Utc>> {
        parse_source_timestamp(&raw.generated_at)
    }

    /// Obtain the feed via `self.transport` and parse it into `ToolFoundryRaw`.
    ///
    /// ERROR MAPPING IS Contract (it drives graceful degradation), and the
    /// transport applies it uniformly for both a file and a spawned command:
    ///   * source absent  → `FeedError::NotFound` → compiler marks the section Missing
    ///     (a missing file, or `toolfoundry` not installed on `$PATH`)
    ///   * other I/O / non-zero exit → `FeedError::Io`/`Parse` → marks it Failed
    ///   * bad JSON       → `FeedError::Parse`    → marks it Failed
    fn fetch(&self) -> Result<Self::Raw, FeedError> {
        // The transport handles the file-vs-command read and the NotFound mapping;
        // the pure parser then maps a JSON error to `FeedError::Parse`.
        let text = self.transport.read()?;
        Self::parse(&text)
    }

    /// Map the raw feed into Workstate's canonical `ToolInventory`.
    ///
    /// PURE and INFALLIBLE per the trait (it returns data + a drop count, never a
    /// `Result`): normalization must never sink the feed. Individual bad records
    /// are handled by SKIP-AND-DROP, not by erroring.
    ///
    /// DROP ACCOUNTING: each id-less record bumps `dropped`, stored on the returned
    /// inventory's `dropped_records` so the loss is never silent. The compiler then
    /// copies it onto `Provenance.dropped_records`.
    ///
    /// Record filter (DIVERGENCE 2): keep a record iff it has a USABLE id (present AND
    /// non-blank). Everything else is a near-direct field copy, because ToolFoundry's
    /// contract is fixed and matches the canonical `Tool` shape — with ONE exception:
    ///
    /// `review_due` (RECONCILIATION 1): keep this existing v3 field null for
    /// RexOps compatibility, preserve the real date as `review_after`, and preserve
    /// the due signal as `review_due_flag`.
    ///
    /// COUNTS ARE RECOMPUTED, NOT ECHOED. ToolFoundry self-reports `tool_count` and
    /// `attention_count` in its envelope, but those are advisory and can disagree
    /// with the actual `tools[]` (the committed sample feed did exactly that). Since
    /// Workstate is the persisted source of truth, shipping a count that contradicts
    /// the records beside it is a defect. We therefore DERIVE both from the
    /// normalized list so they are always internally consistent:
    ///   * `tool_count`      = number of tools that survived normalization.
    ///   * `attention_count` = tools whose `status` is "attention" (case/whitespace
    ///     insensitive) — the same definition the canonical `Tool.status` carries.
    ///
    /// The self-reported envelope numbers are intentionally dropped.
    fn normalize(&self, raw: Self::Raw) -> Self::Normalized {
        let as_of = raw.as_of;
        // Count of records skipped for want of a usable id (DIVERGENCE 2). Reported
        // to the compiler so an id-less drop is accounted, not silent.
        let mut dropped = 0usize;
        let tools: Vec<Tool> = raw
            .tools
            .into_iter() // consume the raw records by value — the envelope is spent
            // `filter_map` is the idiomatic skip-and-drop: each closure returns
            // `Some(tool)` to keep a record or `None` to drop it. A dropped record
            // increments `dropped` first, so the loss is counted, not silent.
            .filter_map(|rt| {
                // Trim once and reuse: an id of "" or "   " is not usable identity.
                let id = rt.id.as_deref().map(str::trim).unwrap_or("");
                if id.is_empty() {
                    dropped += 1; // count the loss before dropping (DIVERGENCE 2)
                    return None; // no usable id → drop
                }
                let review_due_flag = rt.review_due_flag.unwrap_or(false);
                Some(Tool {
                    // Re-own the TRIMMED id into the canonical newtype so identity is
                    // clean and comparisons are stable.
                    id: ToolId(id.to_string()),
                    // Fixed-contract fields pass through 1:1 (String/bool/u32 on both
                    // sides). `rt` is consumed here, so moving the owned values is free.
                    display_name: rt.display_name,
                    owner: rt.owner,
                    project: rt.project,
                    lifecycle_state: rt.lifecycle_state,
                    status: rt.status,
                    // Keep v3's existing nullable field null for RexOps
                    // compatibility; carry ToolFoundry's real review date in
                    // the additive `review_after` field.
                    review_due: None,
                    review_after: rt.review_after.and_then(|value| {
                        chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
                    }),
                    review_due_flag,
                    drifted: rt.drifted,
                    health_passed: rt.health_passed,
                    health_total: rt.health_total,
                    manifest_path: rt.manifest_path,
                })
            })
            .collect(); // gather the survivors into the Vec<Tool> inside the section

        // Derive the counts from the survivors so they can never contradict `tools`.
        let tool_count = tools.len();
        let attention_count = tools
            .iter()
            .filter(|tool| tool.status.trim().eq_ignore_ascii_case("attention"))
            .count();

        ToolInventory {
            as_of,
            tool_count,
            attention_count,
            tools,
            dropped_records: dropped,
        }
    }
}

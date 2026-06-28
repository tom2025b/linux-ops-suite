use chrono::{DateTime, Utc};

use crate::ingest::{parse_source_timestamp, FeedError, FeedSource, FeedTransport};
use crate::model::normalized::{Script, ScriptId, ScriptInventory};
use crate::model::provenance::FeedId;
use crate::model::raw::ScriptVaultRaw;

/// Adapter that obtains ScriptVault's export feed and normalizes it.
///
/// The bytes come from a [`FeedTransport`] — either a file (tests / explicit
/// `--output`) or, in the live flow, `scriptvault workstate-feed` spawned as a
/// subprocess. The adapter is a long-lived value owned by the `SnapshotBuilder`,
/// so it owns its transport outright (no borrowed lifetimes).
pub struct ScriptVaultFeed {
    /// Where the feed's raw JSON comes from. A missing file / uninstalled tool
    /// degrades to a Missing section; a present-but-broken source to Failed.
    pub transport: FeedTransport,
}

impl ScriptVaultFeed {
    /// Construct from a file path (the original constructor; kept for tests and
    /// the `OUTPUT`-override read path).
    pub fn from_path(path: String) -> Self {
        ScriptVaultFeed {
            transport: FeedTransport::File(path),
        }
    }

    /// Construct from a producer command — `scriptvault workstate-feed …` —
    /// spawned each build so the feed is live.
    pub fn from_command(program: &str, args: &[&str]) -> Self {
        ScriptVaultFeed {
            transport: FeedTransport::command(program, args),
        }
    }
}

impl ScriptVaultFeed {
    /// Pure parse step: raw JSON text → typed `ScriptVaultRaw`.
    ///
    /// PRIVATE: its only caller is `fetch`, in this same module. The adapter's
    /// behavioral tests live OUT of the crate in `tests/scriptvault.rs` and drive
    /// parsing through the public `fetch()` against on-disk inputs — an external test
    /// crate could not call a `pub(crate)` fn either, so the narrowest visibility
    /// (private) is the honest one.
    ///
    /// Kept separate from `fetch`: keeping the parse pure (a `&str` in, a
    /// `Result` out, no disk) keeps the read path a thin "read the file, hand the
    /// bytes to `parse`" wrapper whose only extra job is mapping I/O errors. This
    /// mirrors RexOps's own `parse_feed`/`read` split.
    ///
    /// A `serde_json` failure is mapped to `FeedError::Parse` (the variant the
    /// compiler turns into a `Failed` section) carrying the parser's message, so a
    /// malformed feed degrades visibly instead of crashing the build.
    fn parse(text: &str) -> Result<ScriptVaultRaw, FeedError> {
        // `serde_json::from_str` returns its own error type; `.map_err` converts it
        // into our domain error. We DON'T use `?`-with-`#[from]` here because
        // `FeedError` has no `From<serde_json::Error>` — a parse
        // failure must land in `Parse`, not be confused with an I/O failure.
        serde_json::from_str(text).map_err(|e| FeedError::Parse(e.to_string()))
    }
}

impl FeedSource for ScriptVaultFeed {
    // This adapter reads `ScriptVaultRaw` and produces a canonical ScriptInventory.
    type Raw = ScriptVaultRaw;
    type Normalized = ScriptInventory;

    fn feed_id(&self) -> FeedId {
        // Stable, lowercase identifier stamped into provenance for every script
        // fact this feed contributes, so the snapshot is always attributable.
        FeedId("scriptvault".to_string())
    }

    fn supported_schema_version(&self) -> Option<i64> {
        Some(1)
    }

    fn schema_version(&self, raw: &Self::Raw) -> Option<i64> {
        raw.schema_version
    }

    fn expected_source_tool(&self) -> Option<&str> {
        Some("scriptvault")
    }

    fn source_tool<'a>(&self, raw: &'a Self::Raw) -> Option<&'a str> {
        Some(&raw.source_tool)
    }

    fn source_observed_at(&self, raw: &Self::Raw) -> Option<DateTime<Utc>> {
        parse_source_timestamp(&raw.generated_at)
    }

    /// Obtain the feed via `self.transport` and parse it into `ScriptVaultRaw`.
    ///
    /// ERROR MAPPING IS Contract (it drives graceful degradation), and the
    /// transport applies it uniformly for both a file and a spawned command:
    ///   * source absent  → `FeedError::NotFound` → compiler marks the section Missing
    ///     (a missing file, or `scriptvault` not installed on `$PATH`)
    ///   * other I/O / non-zero exit → `FeedError::Io`/`Parse` → marks it Failed
    ///   * bad JSON       → `FeedError::Parse`    → marks it Failed
    fn fetch(&self) -> Result<Self::Raw, FeedError> {
        // The transport handles the file-vs-command read and the NotFound mapping;
        // the pure parser then maps a JSON error to `FeedError::Parse`.
        let text = self.transport.read()?;
        Self::parse(&text)
    }

    /// Map the raw export into Workstate's canonical `ScriptInventory`.
    ///
    /// PURE and INFALLIBLE per the trait (it returns data + a drop count, never a
    /// `Result`): normalization must never sink the feed. Individual bad records
    /// are handled by SKIP-AND-DROP, not by erroring.
    ///
    /// Record filter: keep a record iff it has a USABLE id. "Usable" means present AND
    /// non-blank — an empty/whitespace id is not a stable identity, so we treat it
    /// the same as a missing one and drop it (an empty `ScriptId("")` would be a
    /// false key). This is the stricter-than-RexOps behavior the file header calls
    /// out: RexOps keeps id-less records; we cannot, because `Script.id` is required.
    ///
    /// DROP ACCOUNTING: each dropped record bumps `dropped`, stored on the returned
    /// inventory's `dropped_records` so the loss is never silent. The compiler then
    /// copies it onto `Provenance.dropped_records`.
    fn normalize(&self, raw: Self::Raw) -> Self::Normalized {
        let generated_at = raw.generated_at;
        let favorites = raw.favorites;
        let recents = raw.recents;
        // Count of records skipped for want of a usable id. Reported to the
        // compiler so an id-less drop is accounted, not silent.
        let mut dropped = 0usize;
        let scripts = raw
            .scripts
            .into_iter() // consume the raw records by value — the envelope is spent
            // `filter_map` is the idiomatic skip-and-drop: each closure returns
            // `Some(script)` to keep a record or `None` to drop it. A dropped record
            // increments `dropped` first, so the loss is counted, not silent.
            .filter_map(|rs| {
                // Trim once and reuse: an id of "" or "   " is not usable identity.
                let id = rs.id.as_deref().map(str::trim).unwrap_or("");
                if id.is_empty() {
                    dropped += 1; // count the loss before dropping (stricter than RexOps)
                    return None; // no usable id → drop
                }
                Some(Script {
                    // Re-own the trimmed id into the canonical newtype. We store the
                    // TRIMMED form so identity is clean and comparisons are stable.
                    id: ScriptId(id.to_string()),
                    // name/description pass through as-is: the canonical `Script`
                    // already models them as `Option`, so absence stays honest
                    // (we don't substitute "" for a genuinely missing value).
                    name: rs.name,
                    description: rs.description,
                    rest: rs.rest,
                })
            })
            .collect(); // gather the survivors into the Vec<Script> inside the section

        ScriptInventory {
            generated_at,
            scripts,
            favorites,
            recents,
            dropped_records: dropped,
        }
    }
}

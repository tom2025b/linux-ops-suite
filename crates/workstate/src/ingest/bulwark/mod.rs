use chrono::{DateTime, Utc};

use crate::ingest::{parse_source_timestamp, FeedError, FeedSource, FeedTransport};
use crate::model::normalized::{Finding, FindingId, FindingInventory};
use crate::model::provenance::FeedId;
use crate::model::raw::BulwarkRaw;

// The pure normalization helpers, each in its own focused submodule with its own
// unit tests. `mod` (not `pub mod`): they are internal seams, reachable here via
// the `use` below but not exposed as part of the crate's public API.
mod location;
mod severity;

use location::flatten_location;
use severity::bucket_severity;

/// Adapter that obtains Bulwark's Workstate findings feed and normalizes it.
///
/// The bytes come from a [`FeedTransport`] — either a file (tests / explicit
/// `--output`) or, in the live flow, `bulwark workstate-feed` spawned as a
/// subprocess. The adapter is a long-lived value owned by the `SnapshotBuilder`,
/// so it owns its transport outright (no borrowed lifetimes).
pub struct BulwarkFeed {
    /// Where the feed's raw JSON comes from. A missing file / uninstalled tool
    /// degrades to a Missing section; a present-but-broken source to Failed.
    pub transport: FeedTransport,
}

impl BulwarkFeed {
    /// Construct from a file path (the original constructor; kept for tests and
    /// the `OUTPUT`-override read path).
    pub fn from_path(path: String) -> Self {
        BulwarkFeed {
            transport: FeedTransport::File(path),
        }
    }

    /// Construct from a producer command — `bulwark workstate-feed …` — spawned
    /// each build so the feed is live.
    pub fn from_command(program: &str, args: &[&str]) -> Self {
        BulwarkFeed {
            transport: FeedTransport::command(program, args),
        }
    }
}

impl BulwarkFeed {
    /// Pure parse step: raw JSON text → typed `BulwarkRaw`.
    ///
    /// PRIVATE: `fetch` (its only caller, in this same module) reaches it fine, and
    /// the behavioral tests live OUT of the crate in `tests/bulwark.rs` — they could
    /// not see a `pub(crate)` fn either, so they drive parsing through the public
    /// `fetch()` against on-disk inputs. With no in-crate caller beyond `fetch` and no
    /// out-of-crate caller possible, the narrowest visibility (private) is the honest
    /// one — exposing it wider would advertise an API nobody uses.
    ///
    /// Kept separate from `fetch`: keeping the parse pure (a `&str` in, a
    /// `Result` out, no disk) keeps `fetch` a thin "read the file, hand the bytes to
    /// `parse`" wrapper whose only extra job is mapping I/O errors. This mirrors
    /// RexOps's own `parse_feed`/`read` split.
    ///
    /// A `serde_json` failure is mapped to `FeedError::Parse` (the variant the
    /// compiler turns into a `Failed` section) carrying the parser's message, so a
    /// malformed feed degrades visibly instead of crashing the build.
    fn parse(text: &str) -> Result<BulwarkRaw, FeedError> {
        // `serde_json::from_str` returns its own error type; `.map_err` converts it
        // into our domain error. We DON'T use `?`-with-`#[from]` here because
        // `FeedError` has no `From<serde_json::Error>` — a parse
        // failure must land in `Parse`, not be confused with an I/O failure.
        serde_json::from_str(text).map_err(|e| FeedError::Parse(e.to_string()))
    }
}

impl FeedSource for BulwarkFeed {
    // This adapter reads `BulwarkRaw` and produces a canonical FindingInventory.
    type Raw = BulwarkRaw;
    type Normalized = FindingInventory;

    fn feed_id(&self) -> FeedId {
        // Stable, lowercase identifier stamped into provenance for every finding
        // fact this feed contributes, so the snapshot is always attributable.
        FeedId("bulwark".to_string())
    }

    fn supported_schema_version(&self) -> Option<i64> {
        Some(1)
    }

    fn schema_version(&self, raw: &Self::Raw) -> Option<i64> {
        raw.schema_version
    }

    fn expected_source_tool(&self) -> Option<&str> {
        Some("bulwark")
    }

    fn source_tool<'a>(&self, raw: &'a Self::Raw) -> Option<&'a str> {
        Some(&raw.source_tool)
    }

    fn source_observed_at(&self, raw: &Self::Raw) -> Option<DateTime<Utc>> {
        parse_source_timestamp(&raw.generated_at)
    }

    /// Obtain the feed via `self.transport` and parse it into `BulwarkRaw`.
    ///
    /// ERROR MAPPING IS Contract (it drives graceful degradation), and the
    /// transport applies it uniformly for both a file and a spawned command:
    ///   * source absent  → `FeedError::NotFound` → compiler marks the section Missing
    ///     (a missing file, or `bulwark` not installed on `$PATH`)
    ///   * other I/O / non-zero exit → `FeedError::Io`/`Parse` → marks it Failed
    ///   * bad JSON       → `FeedError::Parse`    → marks it Failed
    fn fetch(&self) -> Result<Self::Raw, FeedError> {
        // The transport handles the file-vs-command read and the NotFound mapping;
        // the pure parser then maps a JSON error to `FeedError::Parse`.
        let text = self.transport.read()?;
        Self::parse(&text)
    }

    /// Map the raw export into Workstate's canonical `FindingInventory`.
    ///
    /// PURE and INFALLIBLE per the trait (it returns data + a drop count, never a
    /// `Result`): normalization must never sink the feed. Individual bad records
    /// are handled by SKIP-AND-DROP, not by erroring.
    ///
    /// DROP ACCOUNTING: each subject-less record bumps `dropped`, stored on the
    /// returned inventory's `dropped_records` so the loss is never silent. The
    /// compiler then copies it onto `Provenance.dropped_records`.
    ///
    /// SECRET SAFETY (ALLOWLIST BY CONSTRUCTION): the canonical `Finding` carries
    /// only an explicit, audited set of fields. Bulwark's raw item `rest` bag (every
    /// unmodeled key) is NOT copied into the snapshot. This replaces the previous
    /// `rest.remove("snippet")` denylist, which scrubbed exactly one key and let any
    /// other field carrying matched content (a secret/PII) flow through. Because
    /// Bulwark's contract is explicitly provisional/additive, only an allowlist is
    /// safe: a future field cannot leak into the persisted snapshot if it is never
    /// copied in the first place.
    ///
    /// SUBJECT RESOLUTION (DIVERGENCE 1):
    ///   Try `item.id` first; if absent or blank, fall back to `item.name`. If
    ///   BOTH are absent or blank → drop the record. This mirrors RexOps's
    ///   `ScanItem::label()` precedence (id → name) while still enforcing the
    ///   stricter "no unnamed findings" rule the canonical `Finding` requires.
    ///
    /// SEVERITY:
    ///   Bucket via `bucket_severity` (trim + lowercase + match), never by direct
    ///   enum deserialization. Absent severity → `Severity::Unrated`; present-but-
    ///   unrecognized → `Severity::Unknown` (the two are kept distinct). See the
    ///   `severity` submodule.
    ///
    /// OPTIONAL STRING FIELDS (`rule_id`, `description`, `category`):
    ///   `Option<String>` on the wire AND on the canonical `Finding`, so they pass
    ///   through unchanged: `None` (absent in the feed) stays `None`, preserving the
    ///   "the feed did not say" signal rather than flattening it to "". This matches
    ///   how `Script.name`/`description` stay optional — absence is modeled honestly.
    fn normalize(&self, raw: Self::Raw) -> Self::Normalized {
        let generated_at = raw.generated_at;
        // Count of records skipped for want of a usable subject (DIVERGENCE 1).
        // Reported to the compiler so a subject-less drop is accounted, not silent.
        let mut dropped = 0usize;
        let findings: Vec<Finding> = raw
            .items
            .into_iter() // consume the raw records by value — the envelope is spent
            // `filter_map` is the idiomatic skip-and-drop: each closure returns
            // `Some(finding)` to keep a record or `None` to drop it. A dropped record
            // increments `dropped` first, so the loss is counted, not silent.
            .filter_map(|item| {
                // --- STEP 1: resolve the subject id (DIVERGENCE 1) ---
                // Try id first; if absent/blank, try name; if also absent/blank, drop.
                let subject = {
                    let id_str = item.id.as_deref().map(str::trim).unwrap_or("");
                    let name_str = item.name.as_deref().map(str::trim).unwrap_or("");
                    // Pick the first non-blank candidate, in RexOps id→name priority.
                    if !id_str.is_empty() {
                        id_str.to_string()
                    } else if !name_str.is_empty() {
                        name_str.to_string()
                    } else {
                        // Both absent or blank — this record cannot become a Finding.
                        dropped += 1; // count the loss before dropping (stricter than RexOps)
                        return None; // skip-and-drop
                    }
                };

                // --- STEP 2: bucket severity ---
                // `bucket_severity` distinguishes the two non-severity cases the
                // canonical `Severity` enum models: an ABSENT severity (`None`)
                // → `Severity::Unrated` ("no risk signal"), while a PRESENT but
                // unrecognized value → `Severity::Unknown`. This preserves the
                // unrated/unknown distinction RexOps keeps in its RiskTally.
                let raw_severity = item.severity;
                let severity = bucket_severity(raw_severity.as_deref());

                // --- STEP 3: flatten the structured location into a display string ---
                let location = flatten_location(item.location.as_ref());

                // --- STEP 4: build the Finding from an EXPLICIT allowlist of fields ---
                // Only the audited canonical fields are copied. `item.rest` (every
                // unmodeled wire key) is intentionally DROPPED, not carried: it can
                // hold matched content (a secret/PII), and the snapshot is written to
                // disk. Dropping the whole bag — rather than denylisting one
                // `snippet` key — is the only safe choice for Bulwark's provisional,
                // additive contract. `rule_id`/`description`/`category` are
                // `Option<String>` on both sides, so `None` (absent) stays `None`.
                //
                // `path`/`risk`/`owner` are audited, non-sensitive fields downstream
                // consumers need (e.g. toolbox-bridge keys sidecars by `path`), so
                // they are promoted to first-class allowlisted fields here rather
                // than left in `rest`. Blank values normalize to `None` so "absent"
                // stays honest (a "" path is not a usable path).
                Some(Finding {
                    id: FindingId(subject),
                    name: item.name,
                    rule_id: item.rule_id,
                    description: item.description,
                    severity,
                    raw_severity,
                    category: item.category,
                    location,
                    path: non_blank(item.path),
                    risk: non_blank(item.risk),
                    owner: non_blank(item.owner),
                })
            })
            .collect(); // gather the survivors into the inventory's Vec<Finding>

        FindingInventory {
            generated_at,
            findings,
            dropped_records: dropped,
        }
    }
}

/// Trim an optional wire string and treat blank as absent. A `Some("")` or
/// `Some("   ")` becomes `None`, so an allowlisted field that the feed left empty
/// is modeled as genuinely absent rather than a misleading present-but-empty value.
fn non_blank(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

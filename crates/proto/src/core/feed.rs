use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::session::Session;

// Integer major version of the FEED format (separate from the session's). Per the
// suite contract rules: present on every export, an integer, bumped only on a
// breaking change. Additive fields keep this the same.
const FEED_SCHEMA_VERSION: u32 = 1;

// How many recent sessions the feed carries at most. The feed is a ROLLING
// summary, not a full archive (the per-run session files are the archive), so we
// cap it to keep the file small and its write cheap no matter how many sessions
// accumulate. Newest-first, so the cap drops the oldest.
pub const DEFAULT_FEED_CAP: usize = 50;

// -----------------------------------------------------------------------------
// WorkstateFeed — the whole emitted document.
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstateFeed {
    // --- Contract header (leads the JSON with provenance) -------------------
    // Integer schema version, gated by the consumer before it trusts the rest.
    pub schema_version: u32,

    // Producer attribution — always "proto" for our feed. A const in the schema.
    pub source_tool: String,

    // When this feed file was generated/last written, RFC3339.
    pub generated_at: DateTime<Utc>,

    // The number of items. MUST equal items.len() — the feed_contract test pins
    // this, since a consumer may trust the count for a quick summary. We keep it
    // explicit (rather than make the consumer count) to match Bulwark's feed.
    pub item_count: usize,

    // One entry per recent session, newest first.
    pub items: Vec<FeedItem>,
}

impl WorkstateFeed {
    // Build a feed from sessions already sorted NEWEST-FIRST (as `store::list`
    // returns them). Takes the first `cap` and summarizes each. `generated_at` is
    // stamped now — this is when the feed was produced. Keeping construction here
    // (not in the store) means the model owns its own invariant: item_count is set
    // from the SAME slice the items come from, so they can't disagree.
    pub fn from_sessions<'a>(
        sessions: impl IntoIterator<Item = (&'a str, &'a Session)>,
        cap: usize,
    ) -> Self {
        let items: Vec<FeedItem> = sessions
            .into_iter()
            .take(cap) // rolling window: only the most recent `cap`
            .map(|(id, s)| FeedItem::from_session(id, s))
            .collect();
        WorkstateFeed {
            schema_version: FEED_SCHEMA_VERSION,
            source_tool: "proto".to_string(),
            generated_at: Utc::now(),
            item_count: items.len(), // set FROM items so the two always agree
            items,
        }
    }
}

// -----------------------------------------------------------------------------
// FeedItem — one recent run, flattened to the fields a consumer cares about.
// -----------------------------------------------------------------------------
// This is a SUMMARY, not the full session: just enough for a dashboard row
// (which protocol, when, how it went). A consumer wanting the full per-step
// detail reads the session file itself (the id here is that file's stem).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItem {
    // The session id (its filename stem) — the handle to the full record.
    pub id: String,

    // Which protocol was run, and its human title (copied from the session, which
    // copied them from the protocol — self-describing all the way down).
    pub protocol_id: String,
    pub protocol_title: String,

    // When the run started, RFC3339. The natural sort/display key for a row.
    pub started_at: DateTime<Utc>,

    // When it finished, RFC3339 — OMITTED while a run is incomplete (mirrors the
    // session's own treatment, and the suite's "absent = missing key" idiom).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,

    // A one-word completion state for a quick consumer glance: "complete" once
    // every step has an outcome, else "incomplete". Derived, not stored, so it
    // can't drift from finished_at.
    pub status: String,

    // The outcome counts, broken out so a consumer can colour/threshold on them
    // without parsing the summary string. (Acknowledged "info" steps and any
    // leftover "pending" are intentionally not surfaced here — a dashboard row
    // cares about pass/fail/skip; the full session has the rest.)
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,

    // The same compact "N passed, N failed, …" line the CLI prints, so a consumer
    // can show Proto's own wording verbatim instead of re-deriving it.
    pub summary: String,
}

impl FeedItem {
    // Summarize one session into a feed row. All derived from the session — the
    // feed never invents data, it only projects the session onto fewer fields.
    fn from_session(id: &str, session: &Session) -> Self {
        let tally = session.tally(); // the shared counter (one source of truth)
        FeedItem {
            id: id.to_string(),
            protocol_id: session.protocol_id.clone(),
            protocol_title: session.protocol_title.clone(),
            started_at: session.started_at,
            finished_at: session.finished_at,
            // "complete" iff no step is still Pending — the session's own notion.
            status: if session.is_complete() {
                "complete".to_string()
            } else {
                "incomplete".to_string()
            },
            passed: tally.passed,
            failed: tally.failed,
            skipped: tally.skipped,
            summary: tally.summary_line(),
        }
    }
}

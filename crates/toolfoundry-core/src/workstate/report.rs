use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

/// Initial serialized contract version for ToolFoundry's Workstate feed.
pub const WORKSTATE_FEED_SCHEMA_VERSION: u32 = 1;

// These types are write-only: ToolFoundry is the *producer* of the feed and only
// ever serializes them to JSON. We deliberately do not derive `Deserialize` —
// Workstate (the consumer) owns its own ingestion types. If a round-trip need
// ever arises, re-add `Deserialize` here together with `#[serde(default)]` on
// `schema_version` so older feeds without the field still parse.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// ToolFoundry-owned tool inventory for Workstate ingestion.
pub struct WorkstateFeed {
    pub schema_version: u32,
    pub source_tool: String,
    pub generated_at: DateTime<Utc>,
    pub as_of: NaiveDate,
    pub tool_count: usize,
    pub attention_count: usize,
    pub tools: Vec<WorkstateTool>,
}

impl WorkstateFeed {
    /// Create a sorted Workstate feed and derive summary counts.
    pub fn new(
        generated_at: DateTime<Utc>,
        as_of: NaiveDate,
        mut tools: Vec<WorkstateTool>,
    ) -> Self {
        tools.sort_by(|left, right| left.id.cmp(&right.id));
        let attention_count = tools
            .iter()
            .filter(|tool| tool.status == ToolStatus::Attention)
            .count();

        Self {
            schema_version: WORKSTATE_FEED_SCHEMA_VERSION,
            source_tool: "toolfoundry".to_string(),
            generated_at,
            as_of,
            tool_count: tools.len(),
            attention_count,
            tools,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// One tool record contributed by ToolFoundry to Workstate.
///
/// Constructed by `WorkstateTool::from_parts` in the sibling `feed` module,
/// which maps a loaded manifest plus health/drift/lifecycle results into this
/// record and applies the attention rule. The type itself stays free of that
/// logic so this module remains pure data + serialization.
pub struct WorkstateTool {
    pub id: String,
    pub display_name: String,
    pub owner: String,
    pub project: String,
    pub lifecycle_state: String,
    pub status: ToolStatus,
    /// The manifest's `lifecycle.review_after` date, surfaced verbatim so the
    /// consumer can display *when* a review is/was expected. Forward-looking:
    /// ToolFoundry does not act on this date beyond deriving `review_due_flag`;
    /// it is carried for downstream scheduling/reporting use.
    pub review_after: NaiveDate,
    /// Whether the review is due as of the feed's `as_of` date
    /// (`as_of >= review_after`). This is a live input to the attention rule —
    /// a due review forces `status: attention` — and is also exposed so the
    /// consumer can explain *why* a tool needs attention without recomputing it.
    pub review_due_flag: bool,
    pub health_passed: usize,
    pub health_total: usize,
    pub drifted: bool,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// ToolFoundry's aggregate attention state for one tool.
pub enum ToolStatus {
    Ok,
    Attention,
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, NaiveDate, Utc};

    use super::{ToolStatus, WORKSTATE_FEED_SCHEMA_VERSION, WorkstateFeed, WorkstateTool};

    // Build a tool record directly (bypassing manifest loading) so report-layer
    // behavior — sorting and count derivation — can be tested in isolation.
    fn tool(id: &str, status: ToolStatus) -> WorkstateTool {
        WorkstateTool {
            id: id.to_string(),
            display_name: id.to_string(),
            owner: "tom".to_string(),
            project: "ops".to_string(),
            lifecycle_state: "active".to_string(),
            status,
            review_after: date(2026, 9, 1),
            review_due_flag: false,
            health_passed: 2,
            health_total: 2,
            drifted: false,
            manifest_path: format!("{id}.yaml"),
        }
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("date should be valid")
    }

    fn generated_at() -> DateTime<Utc> {
        "2026-06-02T00:00:00Z"
            .parse()
            .expect("timestamp should parse")
    }

    #[test]
    fn sorts_tools_by_id_regardless_of_input_order() {
        // Deliberately out of order on input; output must be lexically sorted.
        let feed = WorkstateFeed::new(
            generated_at(),
            date(2026, 6, 2),
            vec![
                tool("zsh-helper", ToolStatus::Ok),
                tool("apt-wrapper", ToolStatus::Ok),
                tool("backup-home", ToolStatus::Ok),
            ],
        );

        let ids: Vec<&str> = feed.tools.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["apt-wrapper", "backup-home", "zsh-helper"]);
    }

    #[test]
    fn derives_attention_count_and_tool_count() {
        let feed = WorkstateFeed::new(
            generated_at(),
            date(2026, 6, 2),
            vec![
                tool("a", ToolStatus::Attention),
                tool("b", ToolStatus::Ok),
                tool("c", ToolStatus::Attention),
            ],
        );

        assert_eq!(feed.tool_count, 3);
        assert_eq!(feed.attention_count, 2);
    }

    #[test]
    fn attention_count_is_zero_when_all_ok() {
        let feed = WorkstateFeed::new(
            generated_at(),
            date(2026, 6, 2),
            vec![tool("a", ToolStatus::Ok), tool("b", ToolStatus::Ok)],
        );

        assert_eq!(feed.attention_count, 0);
    }

    #[test]
    fn empty_feed_has_zero_counts() {
        let feed = WorkstateFeed::new(generated_at(), date(2026, 6, 2), Vec::new());

        assert_eq!(feed.tool_count, 0);
        assert_eq!(feed.attention_count, 0);
    }

    #[test]
    fn stamps_schema_version_and_source_tool() {
        // The producer always stamps the current schema version and identifies
        // itself as "toolfoundry" so Workstate can route the feed.
        let feed = WorkstateFeed::new(generated_at(), date(2026, 6, 2), Vec::new());

        assert_eq!(feed.schema_version, WORKSTATE_FEED_SCHEMA_VERSION);
        assert_eq!(feed.source_tool, "toolfoundry");
    }

    #[test]
    fn serializes_expected_top_level_contract_fields() {
        // Pins the wire contract Workstate depends on: a serialized feed must
        // carry these keys with these derived values.
        let feed = WorkstateFeed::new(
            generated_at(),
            date(2026, 6, 2),
            vec![tool("backup-home", ToolStatus::Attention)],
        );

        let value = serde_json::to_value(&feed).expect("feed should serialize");

        assert_eq!(value["schema_version"], WORKSTATE_FEED_SCHEMA_VERSION);
        assert_eq!(value["source_tool"], "toolfoundry");
        assert_eq!(value["tool_count"], 1);
        assert_eq!(value["attention_count"], 1);
        assert_eq!(value["tools"][0]["status"], "attention");
    }
}

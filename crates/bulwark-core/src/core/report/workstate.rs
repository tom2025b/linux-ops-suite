//! Workstate feed rendering.
//!
//! This is a producer-owned integration contract for Workstate ingestion. It is
//! intentionally separate from `scan --json`, which remains Bulwark's general
//! inventory report contract.

use serde::Serialize;

use crate::core::engine::ClassifiedEntry;
use crate::error::BulwarkError;

/// Initial serialized contract version for Bulwark's Workstate feed.
pub const WORKSTATE_FEED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
struct WorkstateFeed<'a> {
    schema_version: u32,
    source_tool: &'static str,
    generated_at: &'a str,
    item_count: usize,
    items: Vec<WorkstateItem<'a>>,
}

impl<'a> WorkstateFeed<'a> {
    fn new(generated_at: &'a str, entries: &'a [ClassifiedEntry]) -> Self {
        let items: Vec<WorkstateItem<'_>> = entries.iter().map(WorkstateItem::from).collect();

        Self {
            schema_version: WORKSTATE_FEED_SCHEMA_VERSION,
            source_tool: "bulwark",
            generated_at,
            item_count: items.len(),
            items,
        }
    }
}

/// One item in the v1 Workstate feed.
///
/// A few fields are intentionally aliased for the consumer's convenience and are
/// part of the frozen v1 contract (see the `workstate_feed_contract` test):
/// - `id` and `path` carry the same absolute-path string. Workstate keys items
///   by a stable `id`; for Bulwark the path *is* that identity, but `id` is kept
///   as the generic field Workstate dedupes on so the path field can stay a
///   plain path for display.
/// - `severity` and `risk` carry the same value. `severity` is Workstate's
///   generic cross-tool field; `risk` is Bulwark's native term, kept so the feed
///   is self-describing in Bulwark's own vocabulary. They are deliberately equal
///   in v1, not redundant by accident.
#[derive(Debug, Serialize)]
struct WorkstateItem<'a> {
    /// Stable identity Workstate dedupes on. For Bulwark this is the absolute
    /// path (same string as `path`).
    id: String,
    name: Option<String>,
    /// Workstate's generic cross-tool severity. Equal to `risk` in v1.
    severity: crate::core::rules::RiskLevel,
    path: &'a std::path::Path,
    language: crate::core::entry::Language,
    size: u64,
    is_executable: bool,
    description: Option<&'a str>,
    /// Bulwark's native risk term. Equal to `severity` in v1.
    risk: crate::core::rules::RiskLevel,
    category: &'a str,
    owner: &'a str,
}

impl<'a> From<&'a ClassifiedEntry> for WorkstateItem<'a> {
    fn from(entry: &'a ClassifiedEntry) -> Self {
        let path = &entry.entry.discovered.path;

        WorkstateItem {
            id: path.display().to_string(),
            name: path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string),
            severity: entry.classification.risk,
            path,
            language: entry.entry.language,
            size: entry.entry.discovered.size,
            is_executable: entry.entry.discovered.is_executable,
            description: entry.entry.description.as_deref(),
            risk: entry.classification.risk,
            category: &entry.classification.category,
            owner: &entry.classification.owner,
        }
    }
}

/// Render a versioned Workstate feed as pretty-printed JSON.
pub fn render_workstate_feed(
    entries: &[ClassifiedEntry],
    generated_at: &str,
) -> Result<String, BulwarkError> {
    let feed = WorkstateFeed::new(generated_at, entries);
    Ok(serde_json::to_string_pretty(&feed)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::entry::{Language, ScriptEntry};
    use crate::core::rules::{Classification, RiskLevel};
    use crate::core::scanner::DiscoveredFile;
    use std::path::PathBuf;

    #[test]
    fn render_workstate_feed_stamps_envelope_and_items() {
        let entries = [ClassifiedEntry {
            entry: ScriptEntry {
                discovered: DiscoveredFile {
                    path: PathBuf::from("/tmp/tool.sh"),
                    size: 42,
                    is_executable: true,
                },
                language: Language::Bash,
                description: Some("local tool".to_string()),
                sidecar: None,
                sidecar_warning: None,
            },
            classification: Classification {
                risk: RiskLevel::High,
                category: "script".to_string(),
                owner: "user".to_string(),
            },
        }];

        let json = render_workstate_feed(&entries, "2026-06-06T12:34:56Z").unwrap();
        let expected = concat!(
            "{\n",
            "  \"schema_version\": 1,\n",
            "  \"source_tool\": \"bulwark\",\n",
            "  \"generated_at\": \"2026-06-06T12:34:56Z\",\n",
            "  \"item_count\": 1,\n",
            "  \"items\": [\n",
            "    {\n",
            "      \"id\": \"/tmp/tool.sh\",\n",
            "      \"name\": \"tool.sh\",\n",
            "      \"severity\": \"high\",\n",
            "      \"path\": \"/tmp/tool.sh\",\n",
            "      \"language\": \"Bash\",\n",
            "      \"size\": 42,\n",
            "      \"is_executable\": true,\n",
            "      \"description\": \"local tool\",\n",
            "      \"risk\": \"high\",\n",
            "      \"category\": \"script\",\n",
            "      \"owner\": \"user\"\n",
            "    }\n",
            "  ]\n",
            "}"
        );

        assert_eq!(json, expected);
    }
}

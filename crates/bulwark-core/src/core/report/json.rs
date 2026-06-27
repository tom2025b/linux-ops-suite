//! JSON report rendering.
//!
//! JSON is the machine-oriented report adapter. Rendering is fallible because
//! it goes through `serde_json`, so the pure renderer returns a typed
//! [`BulwarkError`](crate::error::BulwarkError) instead of printing directly.

use serde::Serialize;

use crate::core::engine::ClassifiedEntry;
use crate::error::BulwarkError;

/// Flat, serializable view of one classified entry for JSON output.
///
/// This private adapter controls the exact emitted JSON shape without forcing
/// display concerns into the public [`ClassifiedEntry`] domain type.
#[derive(Debug, Serialize)]
struct EntryView<'a> {
    path: &'a std::path::Path,
    language: crate::core::entry::Language,
    size: u64,
    is_executable: bool,
    description: Option<&'a str>,
    risk: crate::core::rules::RiskLevel,
    category: &'a str,
    owner: &'a str,
}

impl<'a> From<&'a ClassifiedEntry> for EntryView<'a> {
    fn from(entry: &'a ClassifiedEntry) -> Self {
        EntryView {
            path: &entry.entry.discovered.path,
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

/// Render the classified inventory as pretty-printed JSON.
pub fn render_json_classified(entries: &[ClassifiedEntry]) -> Result<String, BulwarkError> {
    let views: Vec<EntryView<'_>> = entries.iter().map(EntryView::from).collect();
    Ok(serde_json::to_string_pretty(&views)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::entry::{Language, ScriptEntry};
    use crate::core::rules::{Classification, RiskLevel};
    use crate::core::scanner::DiscoveredFile;
    use std::path::PathBuf;

    #[test]
    fn render_json_uses_machine_friendly_shape() {
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

        let json = render_json_classified(&entries).unwrap();
        let expected = concat!(
            "[\n",
            "  {\n",
            "    \"path\": \"/tmp/tool.sh\",\n",
            "    \"language\": \"Bash\",\n",
            "    \"size\": 42,\n",
            "    \"is_executable\": true,\n",
            "    \"description\": \"local tool\",\n",
            "    \"risk\": \"high\",\n",
            "    \"category\": \"script\",\n",
            "    \"owner\": \"user\"\n",
            "  }\n",
            "]"
        );
        assert_eq!(json, expected);

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value[0]["path"], "/tmp/tool.sh");
        assert_eq!(value[0]["language"], "Bash");
        assert_eq!(value[0]["size"], 42);
        assert_eq!(value[0]["is_executable"], true);
        assert_eq!(value[0]["description"], "local tool");
        assert_eq!(value[0]["risk"], "high");
        assert_eq!(value[0]["category"], "script");
        assert_eq!(value[0]["owner"], "user");
    }

    #[test]
    fn render_json_emits_unknown_language_as_the_documented_token() {
        // A file with no recognized shebang/extension has language `Unknown`.
        // The serialized token is part of the public JSON contract, so pin it
        // here: it must be exactly "Unknown" and routed through `Language::as_str`
        // (the manual Serialize impl), never the derived Debug reflection.
        let entries = [ClassifiedEntry {
            entry: ScriptEntry {
                discovered: DiscoveredFile {
                    path: PathBuf::from("/tmp/mystery"),
                    size: 3,
                    is_executable: false,
                },
                language: Language::Unknown,
                description: None,
                sidecar: None,
                sidecar_warning: None,
            },
            classification: Classification {
                risk: RiskLevel::Low,
                category: "unknown".to_string(),
                owner: "user".to_string(),
            },
        }];

        let json = render_json_classified(&entries).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["language"], "Unknown");
    }
}

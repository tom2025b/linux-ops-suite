//! Markdown report rendering.
//!
//! Markdown is a documentation-oriented adapter: it escapes table-breaking
//! characters, keeps output plain text, and returns a pure [`String`] so callers
//! can print, save, test, or embed it without rerunning the scan.

use crate::core::engine::ClassifiedEntry;

use super::format::{human_size, md_escape};

/// Render the classified inventory as a Markdown table string.
///
/// The shape matches the historical CLI output exactly: header, separator, one
/// row per entry, and a trailing newline after every emitted line.
pub fn render_markdown_table_classified(entries: &[ClassifiedEntry]) -> String {
    let mut output = String::new();

    output.push_str("| Path | Language | Risk | Category | Owner | Size | Description |\n");
    output.push_str("|------|----------|------|----------|-------|------|-------------|\n");

    for entry in entries {
        let path = md_escape(&entry.entry.discovered.path.display().to_string());
        let description = entry
            .entry
            .description
            .as_deref()
            .map(md_escape)
            .unwrap_or_default();

        output.push_str(&format!(
            "| {} | {} | {:?} | {} | {} | {} | {} |\n",
            path,
            entry.entry.language.as_str(),
            entry.classification.risk,
            md_escape(&entry.classification.category),
            md_escape(&entry.classification.owner),
            human_size(entry.entry.discovered.size),
            description,
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::entry::{Language, ScriptEntry};
    use crate::core::rules::{Classification, RiskLevel};
    use crate::core::scanner::DiscoveredFile;
    use std::path::PathBuf;

    #[test]
    fn render_markdown_table_escapes_cells_and_keeps_trailing_newlines() {
        let entries = [ClassifiedEntry {
            entry: ScriptEntry {
                discovered: DiscoveredFile {
                    path: PathBuf::from("/tmp/a|b.sh"),
                    size: 1536,
                    is_executable: true,
                },
                language: Language::Bash,
                description: Some("first\nsecond | pipe".to_string()),
                sidecar: None,
                sidecar_warning: None,
            },
            classification: Classification {
                risk: RiskLevel::High,
                category: "ops|local".to_string(),
                owner: "me".to_string(),
            },
        }];

        let markdown = render_markdown_table_classified(&entries);

        assert_eq!(
            markdown,
            concat!(
                "| Path | Language | Risk | Category | Owner | Size | Description |\n",
                "|------|----------|------|----------|-------|------|-------------|\n",
                "| /tmp/a\\|b.sh | Bash | High | ops\\|local | me | 1.5K | first second \\| pipe |\n",
            )
        );
    }
}

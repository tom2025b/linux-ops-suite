//! Script entry enrichment from raw discovery data.
//!
//! Enrichment is deliberately read-only and best-effort. Bulwark reads a small,
//! bounded prefix of each file for language/header hints, optionally reads a
//! sidecar YAML file, and still returns a usable entry if any of that fails.

use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Serialize;

use crate::core::scanner::DiscoveredFile;

use super::language::Language;
use super::metadata::{SidecarMetadata, SidecarOutcome, load_sidecar};

/// How many leading lines Bulwark inspects for shebang and header extraction.
const HEADER_SCAN_LINES: usize = 50;

/// The rich, analyzed representation of a discovered tool or script.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptEntry {
    /// The raw discovery information: path, size, executable bit, etc.
    pub discovered: DiscoveredFile,
    /// Inferred language, using shebang before extension.
    pub language: Language,
    /// Cleaned description extracted from the leading comment block.
    pub description: Option<String>,
    /// Parsed sidecar metadata, if a companion `*.bulwark.yaml` was found.
    pub sidecar: Option<SidecarMetadata>,
    /// A human-readable warning when a sidecar file exists next to this script
    /// but could not be read or parsed. `None` when there is no sidecar or it
    /// loaded cleanly. Not part of the serialized inventory shape — it is an
    /// internal signal the engine turns into a [`ScanWarning`] so a malformed
    /// annotation is surfaced instead of silently ignored.
    ///
    /// [`ScanWarning`]: crate::core::scanner::ScanWarning
    #[serde(skip)]
    pub sidecar_warning: Option<String>,
}

impl ScriptEntry {
    /// Build a rich entry from a discovered file.
    ///
    /// This function never fails: scan output should degrade gracefully when an
    /// individual file is unreadable or has malformed optional metadata. A
    /// present-but-malformed sidecar is recorded in `sidecar_warning` rather
    /// than aborting or vanishing.
    pub fn from_discovered(discovered: DiscoveredFile) -> Self {
        let head = read_head_lines(&discovered.path, HEADER_SCAN_LINES);

        let language = infer_language(&discovered.path, &head);
        let description = extract_header(&head, &language);

        let (sidecar, sidecar_warning) = match load_sidecar(&discovered.path) {
            SidecarOutcome::None => (None, None),
            SidecarOutcome::Loaded(metadata) => (Some(metadata), None),
            SidecarOutcome::Malformed { path, reason } => {
                (None, Some(format!("{}: {reason}", path.display())))
            }
        };

        ScriptEntry {
            discovered,
            language,
            description,
            sidecar,
            sidecar_warning,
        }
    }
}

/// Infer language from an already-read file prefix.
fn infer_language(path: &Path, head: &[String]) -> Language {
    if let Some(first_line) = head.first()
        && let Some(language) = Language::from_shebang(first_line)
    {
        return language;
    }

    Language::from_extension(path)
}

/// Extract a leading comment header from an already-read file prefix.
fn extract_header(head: &[String], language: &Language) -> Option<String> {
    let leader = language.comment_leader();
    let mut header_lines = Vec::new();
    let mut first_line = true;

    for line in head {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            break;
        }

        if !trimmed.starts_with(leader) {
            break;
        }

        if first_line && trimmed.starts_with("#!") {
            first_line = false;
            continue;
        }
        first_line = false;

        let cleaned = trimmed
            .strip_prefix(leader)
            .unwrap_or(trimmed)
            .trim_start()
            .to_string();

        if !cleaned.is_empty() {
            header_lines.push(cleaned);
        }
    }

    if header_lines.is_empty() {
        None
    } else {
        Some(header_lines.join(" "))
    }
}

/// Read up to `max` lines from the start of a file.
fn read_head_lines(path: &Path, max: usize) -> Vec<String> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };

    BufReader::new(file)
        .lines()
        .take(max)
        .map_while(Result::ok)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn header_extraction_stops_at_blank_line() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("test.sh");
        let content = "#!/bin/bash\n# This is the description\n\n# Should not appear\n";
        fs::write(&script, content).unwrap();

        let discovered = DiscoveredFile {
            path: script,
            size: 0,
            is_executable: true,
        };
        let entry = ScriptEntry::from_discovered(discovered);

        assert_eq!(
            entry.description.as_deref(),
            Some("This is the description")
        );
    }

    #[test]
    fn shebang_takes_precedence_over_extension() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("test.py");
        fs::write(&script, "#!/bin/bash\n# Bash despite extension\n").unwrap();

        let discovered = DiscoveredFile {
            path: script,
            size: 0,
            is_executable: true,
        };
        let entry = ScriptEntry::from_discovered(discovered);

        assert_eq!(entry.language, Language::Bash);
    }
}

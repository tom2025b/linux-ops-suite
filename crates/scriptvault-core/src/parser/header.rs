// ============================================================================
// crates/scriptvault-core/src/parser/header.rs
// ============================================================================
// Inline header parsing. Scans the top-of-file comment block for lines of the
// form `<comment-leader> scriptvault.<key>: <value>` and collects them into a
// ScriptMetadata.
//
// The stop condition is the load-bearing decision here:
//   • Skip an optional leading shebang line.
//   • Read subsequent lines; BLANK lines are tolerated (the common
//     `shebang / blank / # scriptvault...` shape must work).
//   • STOP at the first real CODE line — a non-blank line that is NOT a comment
//     in this language. That is the PRIMARY limit: annotations only count when
//     they sit in the leading comment region, so a `# scriptvault.name: X` printed
//     inside a heredoc later in the file is never mis-parsed.
//   • A numeric cap (MAX_SCAN_LINES) exists ONLY as a pathological safety net
//     for files with no code line at all; it is set high (300) so a long
//     license/copyright header never silently truncates real annotations.
// ============================================================================

use crate::model::{Language, ScriptMetadata};

/// Pathological safety net only — NOT the primary stop. Real scanning stops at
/// the first code line. This just bounds work on, e.g., a giant all-comment
/// file. Set high so long license headers don't hide annotations beneath them.
const MAX_SCAN_LINES: usize = 300;

/// Parse the inline header of `contents`, using `leader_lang` to know which
/// comment marker introduces a comment line.
///
/// Returns a `ScriptMetadata` containing whatever `scriptvault.*` keys were found
/// (possibly all-empty if there were none — that is not an error here).
pub fn parse_header(contents: &str, leader_lang: Language) -> ScriptMetadata {
    let leader = leader_lang.comment_leader();
    let mut meta = ScriptMetadata::default();

    for (i, raw) in contents.lines().enumerate() {
        // Safety net only; the real exit is the code-line check below.
        if i >= MAX_SCAN_LINES {
            break;
        }

        let line = raw.trim();

        // Skip the shebang (only meaningful on the very first line).
        if i == 0 && line.starts_with("#!") {
            continue;
        }

        // Blank lines inside the header are allowed and don't stop the scan.
        if line.is_empty() {
            continue;
        }

        // A comment line? Try to strip the leader and read a `scriptvault.k: v`.
        if let Some(after) = line.strip_prefix(leader) {
            // `after` is the comment body (without the leader), e.g.
            // " scriptvault.name: Backup". Non-scriptvault comments are simply ignored
            // (they're comments, so they don't stop the scan).
            apply_kv(&mut meta, after.trim());
            continue;
        }

        // First non-blank, non-comment line = start of code. Stop scanning.
        break;
    }

    meta
}

/// If `body` is a `scriptvault.<key>: <value>` entry, record it on `meta`.
/// Unrecognized keys and non-scriptvault text are ignored.
fn apply_kv(meta: &mut ScriptMetadata, body: &str) {
    // Must be in the scriptvault namespace.
    let Some(kv) = body.strip_prefix("scriptvault.") else {
        return;
    };

    // Split once on ':' into key and value.
    let Some((key, value)) = kv.split_once(':') else {
        return;
    };
    let key = key.trim();
    let value = value.trim();

    // Empty values are treated as "not provided" (skip), so a stray
    // `# scriptvault.desc:` with nothing after it doesn't set an empty string.
    if value.is_empty() {
        return;
    }

    match key {
        "name" => meta.name = Some(value.to_string()),
        "desc" | "description" => meta.desc = Some(value.to_string()),
        "usage" => meta.usage = Some(value.to_string()),
        "category" => meta.category = Some(value.to_string()),
        "lang" | "language" => meta.lang = Some(value.to_string()),
        "risk" => meta.risk = Some(value.to_string()),
        "owner" => meta.owner = Some(value.to_string()),
        "tags" | "tag" => meta.tags = parse_tags(value),
        // Unknown key under scriptvault.* — ignore rather than error.
        _ => {}
    }
}

/// Split a comma-separated tag string into normalized tags.
/// "DB, Backup ,, postgres, db " -> ["db", "backup", "postgres"]
/// (lowercased, trimmed, empties dropped, de-duplicated order-preserving).
///
/// Normalization happens here so a header's own tags are already canonical, and
/// is applied AGAIN on the final merged metadata in `build_entry` so sidecar tags
/// (which never pass through here) get the same treatment — one consistent rule.
pub fn parse_tags(value: &str) -> Vec<String> {
    normalize_tags(value.split(',').map(str::to_string).collect())
}

/// Canonicalize a list of tags: trim, lowercase, drop empties, and de-duplicate
/// while preserving first-seen order. The single source of truth for "what a tag
/// looks like" — used by both `parse_tags` (header) and `build_entry` (final
/// merged metadata, which includes serde-loaded sidecar tags).
///
/// Lowercasing makes search/filter case-insensitive and consistent ("Risk:High"
/// and "risk:high" are the same tag); de-duplication keeps the badge list clean
/// when a header and sidecar both supply an overlapping tag.
pub fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    tags.into_iter()
        // Trim surrounding whitespace and lowercase for a canonical form.
        .map(|t| t.trim().to_lowercase())
        // Drop anything that was blank (or became blank after trimming).
        .filter(|t| !t.is_empty())
        // `HashSet::insert` returns false if already present → keep first only.
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_fields_after_shebang_and_blank() {
        let src = "\
#!/usr/bin/env bash

# scriptvault.name: Backup Postgres
# scriptvault.desc: Dumps the prod DB
# scriptvault.tags: db, backup, postgres
# scriptvault.usage: backup-db.sh [--full]
# scriptvault.category: database
# scriptvault.lang: bash

echo hi
# scriptvault.name: SHOULD-NOT-OVERRIDE (after code)
";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta.name.as_deref(), Some("Backup Postgres"));
        assert_eq!(meta.desc.as_deref(), Some("Dumps the prod DB"));
        assert_eq!(meta.tags, vec!["db", "backup", "postgres"]);
        assert_eq!(meta.usage.as_deref(), Some("backup-db.sh [--full]"));
        assert_eq!(meta.category.as_deref(), Some("database"));
        assert_eq!(meta.lang.as_deref(), Some("bash"));
    }

    #[test]
    fn stops_at_first_code_line() {
        // The annotation AFTER the code line must be ignored.
        let src = "\
#!/bin/bash
# scriptvault.name: Real
echo running
# scriptvault.name: Ignored
";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta.name.as_deref(), Some("Real"));
    }

    #[test]
    fn long_comment_header_does_not_truncate_annotations() {
        // 45 lines of license comments, THEN a real annotation. With a naive
        // ~40-line cap this would vanish; stop-at-code + high cap keeps it.
        let mut src = String::from("#!/bin/bash\n");
        for i in 0..45 {
            src.push_str(&format!("# license line {i}\n"));
        }
        src.push_str("# scriptvault.name: DeepName\n");
        src.push_str("echo hi\n");

        let meta = parse_header(&src, Language::Bash);
        assert_eq!(meta.name.as_deref(), Some("DeepName"));
    }

    #[test]
    fn no_annotations_yields_empty_meta() {
        let src = "#!/bin/bash\necho hello\n";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta, ScriptMetadata::default());
    }

    #[test]
    fn respects_language_comment_leader() {
        // Rust uses `//`; a `#`-style line must NOT be read as a comment here,
        // so it counts as a code line and stops the scan immediately.
        let src = "\
// scriptvault.name: RustTool
fn main() {}
";
        let meta = parse_header(src, Language::Rust);
        assert_eq!(meta.name.as_deref(), Some("RustTool"));

        // SQL uses `--`.
        let sql = "-- scriptvault.name: Report\nSELECT 1;\n";
        let meta = parse_header(sql, Language::Sql);
        assert_eq!(meta.name.as_deref(), Some("Report"));
    }

    #[test]
    fn empty_value_is_skipped() {
        let src = "#!/bin/bash\n# scriptvault.desc:\n# scriptvault.name: X\n";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta.desc, None);
        assert_eq!(meta.name.as_deref(), Some("X"));
    }

    #[test]
    fn tag_splitting_trims_and_drops_empties() {
        assert_eq!(
            parse_tags("db, backup ,, postgres "),
            vec!["db", "backup", "postgres"]
        );
        assert!(parse_tags("   ").is_empty());
    }

    #[test]
    fn tags_are_lowercased_and_deduplicated() {
        // Mixed case + a duplicate that differs only by case + an exact dup:
        // all collapse to one canonical, lowercase, first-seen-ordered list.
        assert_eq!(
            parse_tags("DB, Backup, db, BACKUP, Postgres"),
            vec!["db", "backup", "postgres"]
        );
    }

    #[test]
    fn normalize_tags_is_the_single_rule() {
        // Direct test of the shared helper (also fed serde-loaded sidecar tags).
        assert_eq!(
            normalize_tags(vec![
                "  Risk:High ".into(),
                "risk:high".into(),
                "".into(),
                "Owner:User".into(),
            ]),
            vec!["risk:high", "owner:user"]
        );
    }

    #[test]
    fn header_without_shebang_still_parses() {
        // No shebang line at all: the first line is already a comment annotation.
        // It must be read (not treated as a code line that stops the scan).
        let src = "# scriptvault.name: NoShebang\n# scriptvault.desc: works\necho hi\n";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta.name.as_deref(), Some("NoShebang"));
        assert_eq!(meta.desc.as_deref(), Some("works"));
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        // Windows CRLF: `str::lines()` strips the trailing '\r', and we `trim()`
        // each line, so `# scriptvault.name: X\r\n` parses exactly like LF.
        let src = "#!/bin/bash\r\n# scriptvault.name: CRLF Tool\r\necho hi\r\n";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta.name.as_deref(), Some("CRLF Tool"));
    }

    #[test]
    fn value_may_contain_colons() {
        // `split_once(':')` splits on the FIRST colon only, so a value with more
        // colons (a URL, a time) survives intact.
        let src = "#!/bin/bash\n# scriptvault.usage: run.sh --url http://x:8080/y\necho hi\n";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta.usage.as_deref(), Some("run.sh --url http://x:8080/y"));
    }

    #[test]
    fn unknown_scriptvault_key_is_ignored_not_an_error() {
        // A `scriptvault.*` key we don't recognize is silently ignored; known keys
        // around it still parse.
        let src = "#!/bin/bash\n# scriptvault.bogus: whatever\n# scriptvault.name: Real\necho hi\n";
        let meta = parse_header(src, Language::Bash);
        assert_eq!(meta.name.as_deref(), Some("Real"));
    }

    #[test]
    fn only_a_shebang_yields_empty_meta() {
        // A file that is JUST a shebang (no body, no annotations) is valid and
        // simply carries no metadata.
        let meta = parse_header("#!/usr/bin/env python3\n", Language::Python);
        assert_eq!(meta, ScriptMetadata::default());
    }
}

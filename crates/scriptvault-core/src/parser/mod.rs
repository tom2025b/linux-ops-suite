// parser — turn a candidate path into a fully-resolved ScriptEntry.
//
// Pipeline for one file:
//   1. read as text                    <-- the ONLY hard failure point
//   2. leader-language  = shebang/extension (picks the comment leader)
//   3. header metadata  = scan the leading comment block
//   4. sidecar metadata = load <file>.scriptvault.yaml (non-fatal if broken)
//   5. MetaSource       = from pre-merge presence (who contributed)
//   6. merged metadata  = sidecar-wins merge
//   7. resolved-language = explicit meta.lang else leader-language
//   8. build ScriptEntry
//
// The scanner pre-filters to script-looking files, so the parser never sees a
// binary. Error policy splits on "readable as text", not "has metadata": an
// unannotated file is still Ok (indexed by filename); only an unreadable/non-UTF8
// file is Err (dropped by `parse_all`); a broken sidecar warns and degrades.

mod header;
/// `pub(crate)` so the scanner shares `lang::is_known_script_ext` (one list).
pub(crate) mod lang;
mod sidecar;

use std::path::{Path, PathBuf};

use crate::error::{Result, ScriptVaultError};
use crate::model::{MetaSource, ScriptEntry, ScriptMetadata};

/// Parse one candidate path into a `ScriptEntry`. `Err` ONLY when the file can't
/// be read as UTF-8 text; a readable file always yields `Ok`, even unannotated.
pub fn parse(path: &Path) -> Result<ScriptEntry> {
    // The sole hard-failure point: a binary/unreadable file is dropped here.
    let contents =
        std::fs::read_to_string(path).map_err(|source| ScriptVaultError::NotReadable {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(build_entry(path, &contents))
}

/// Parse many paths, keeping only readable ones. A path that looked like a
/// script but can't be read as text is dropped (with a DEBUG-level diagnostic,
/// surfaced under `--verbose`/`RUST_LOG`); everything readable becomes an entry.
pub fn parse_all(paths: &[PathBuf]) -> Vec<ScriptEntry> {
    paths
        .iter()
        .filter_map(|p| match parse(p) {
            Ok(entry) => Some(entry),
            Err(err) => {
                // Non-text/unreadable file: skip. Not fatal. The scanner already
                // filtered to script-looking files, so reaching here means a file
                // that LOOKED like a script (known extension or `#!`) but couldn't
                // be read as text. DEBUG level keeps it quiet by default but
                // visible under `--verbose`/`RUST_LOG` — same gate as before, now
                // via the log level instead of a hand-rolled `is_verbose()` check.
                tracing::debug!(%err, "skipping unreadable candidate");
                None
            }
        })
        .collect()
}

/// Build an entry from already-read contents. Split out from `parse` so it can
/// be unit-tested without touching the filesystem.
fn build_entry(path: &Path, contents: &str) -> ScriptEntry {
    // Step 2 — leader language from the first line (shebang) + extension.
    let first_line = contents.lines().next().unwrap_or("");
    let leader_lang = lang::leader_language(path, first_line);

    // Step 3 — inline header metadata.
    let header_meta = header::parse_header(contents, leader_lang);

    // Step 4 — sidecar metadata (None if absent or malformed).
    let sidecar_meta = sidecar::load_sidecar(path);

    // Step 5 — provenance from PRE-merge presence (after merging we can no
    // longer tell who supplied a field).
    let source = resolve_source(&header_meta, sidecar_meta.as_ref());

    // Step 6 — merge (sidecar wins). If there's no sidecar, header stands alone.
    let mut meta = match sidecar_meta {
        Some(side) => sidecar::merge(header_meta, side),
        None => header_meta,
    };

    // Step 6b — bridge/import fields become normal tags before normalization.
    // This keeps the existing query engine convention (`risk:high`,
    // `owner:tom`) while allowing sidecars to use friendlier YAML keys.
    append_structured_metadata_tags(&mut meta);

    // Step 6c — normalize tags ONCE on the final merged metadata: lowercase +
    // trim + de-duplicate (order-preserving). This is the single choke point all
    // tags flow through — header tags, sidecar tags (which arrive straight from
    // serde and bypass the header's `parse_tags`), and the merged result — so
    // "DB", "db", " db " never split a search/filter into three. Done here rather
    // than in `parse_tags`/`merge` so BOTH provenance paths are covered uniformly.
    meta.tags = header::normalize_tags(std::mem::take(&mut meta.tags));

    // Step 7 — resolved language: explicit meta.lang wins, else leader language.
    let resolved_lang = lang::resolve_language(leader_lang, meta.lang.as_deref());

    // Step 8 — assemble. Filename is always present (the universal fallback).
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    ScriptEntry {
        path: path.to_path_buf(),
        filename,
        lang: resolved_lang,
        meta,
        source,
    }
}

/// Decide where metadata came from, based on whether each source contributed
/// any field at all.
fn resolve_source(header: &ScriptMetadata, sidecar: Option<&ScriptMetadata>) -> MetaSource {
    let has_header = has_any(header);
    let has_sidecar = sidecar.is_some_and(has_any);
    match (has_header, has_sidecar) {
        (true, true) => MetaSource::Both,
        (true, false) => MetaSource::Header,
        (false, true) => MetaSource::Sidecar,
        (false, false) => MetaSource::None,
    }
}

/// True if this metadata carries any field at all (any Option `Some` or any
/// tags). Used for `MetaSource` and to mean "this source contributed".
fn has_any(meta: &ScriptMetadata) -> bool {
    meta.name.is_some()
        || meta.desc.is_some()
        || meta.usage.is_some()
        || meta.category.is_some()
        || meta.lang.is_some()
        || meta.risk.is_some()
        || meta.owner.is_some()
        || !meta.tags.is_empty()
}

/// Convert first-class sidecar/header metadata into canonical tags consumed by
/// the existing search/filter layer.
fn append_structured_metadata_tags(meta: &mut ScriptMetadata) {
    if let Some(risk) = normalized_field_tag("risk", meta.risk.as_deref()) {
        meta.tags.push(risk);
    }
    if let Some(owner) = normalized_field_tag("owner", meta.owner.as_deref()) {
        meta.tags.push(owner);
    }
}

/// Build `prefix:value` while trimming and dropping empty values.
fn normalized_field_tag(prefix: &str, value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        None
    } else {
        Some(format!("{prefix}:{value}"))
    }
}

// ============================================================================
// Tests — filesystem-backed integration plus pure build_entry unit tests.
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Language;
    use std::fs;

    fn tmp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-parse-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unannotated_readable_file_is_indexed_not_dropped() {
        // "Never invisible": a plain script with no annotations -> Ok entry,
        // findable by filename, source None.
        let entry = build_entry(Path::new("/x/foo.sh"), "#!/bin/bash\necho hi\n");
        assert_eq!(entry.filename, "foo.sh");
        assert_eq!(entry.display_name(), "foo.sh"); // falls back to filename
        assert_eq!(entry.source, MetaSource::None);
        assert_eq!(entry.lang, Language::Bash);
        assert!(entry.meta.name.is_none());
    }

    #[test]
    fn header_only_entry() {
        let src =
            "#!/bin/bash\n# scriptvault.name: Greeter\n# scriptvault.tags: hi, demo\necho hi\n";
        let entry = build_entry(Path::new("/x/greet.sh"), src);
        assert_eq!(entry.display_name(), "Greeter");
        assert_eq!(entry.meta.tags, vec!["hi", "demo"]);
        assert_eq!(entry.source, MetaSource::Header);
    }

    #[test]
    fn explicit_lang_in_header_overrides_leader() {
        // A .py file (leader Python) whose header declares bash -> resolved Bash,
        // proving the header was still PARSED with the Python leader.
        let src =
            "#!/usr/bin/env python3\n# scriptvault.lang: bash\n# scriptvault.name: X\nprint(1)\n";
        let entry = build_entry(Path::new("/x/weird.py"), src);
        assert_eq!(entry.lang, Language::Bash);
        assert_eq!(entry.display_name(), "X");
    }

    #[test]
    fn sidecar_overrides_and_sets_source_both() {
        let dir = tmp_dir("sidecar-both");
        let script = dir.join("deploy.sh");
        fs::write(
            &script,
            "#!/bin/bash\n# scriptvault.name: HeaderName\n# scriptvault.desc: HD\necho go\n",
        )
        .unwrap();
        // Sidecar overrides the name, leaves desc to fall back to the header.
        fs::write(
            sidecar::sidecar_path(&script),
            "name: SidecarName\ncategory: ops\n",
        )
        .unwrap();

        let entry = parse(&script).unwrap();
        assert_eq!(entry.display_name(), "SidecarName"); // sidecar wins
        assert_eq!(entry.meta.desc.as_deref(), Some("HD")); // header survives
        assert_eq!(entry.meta.category.as_deref(), Some("ops"));
        assert_eq!(entry.source, MetaSource::Both);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sidecar_tags_are_normalized_like_header_tags() {
        // The important case: sidecar tags arrive straight from serde_yaml and
        // NEVER pass through `header::parse_tags`. They must STILL be lowercased
        // and de-duplicated, because normalization runs once on the final merged
        // metadata in `build_entry`. Mixed case + a cross-source duplicate here.
        let dir = tmp_dir("sidecar-tags-norm");
        let script = dir.join("svc.sh");
        fs::write(
            &script,
            "#!/bin/bash\n# scriptvault.tags: Backup, DB\necho x\n",
        )
        .unwrap();
        // Sidecar (bare serde keys) repeats "db" in a different case + adds "Prod".
        fs::write(
            sidecar::sidecar_path(&script),
            "tags:\n  - DB\n  - prod\n  - Backup\n",
        )
        .unwrap();

        let entry = parse(&script).unwrap();
        // Sidecar wins on tags (non-empty), then normalization canonicalizes:
        // lowercased + de-duplicated, first-seen order from the sidecar list.
        assert_eq!(entry.meta.tags, vec!["db", "prod", "backup"]);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sidecar_risk_owner_become_searchable_tags() {
        let dir = tmp_dir("sidecar-risk-owner");
        let script = dir.join("danger.sh");
        fs::write(&script, "#!/bin/bash\necho x\n").unwrap();
        fs::write(
            sidecar::sidecar_path(&script),
            "risk: High\nowner: Tom\ncategory: ops\n",
        )
        .unwrap();

        let entry = parse(&script).unwrap();
        assert_eq!(entry.meta.risk.as_deref(), Some("High"));
        assert_eq!(entry.meta.owner.as_deref(), Some("Tom"));
        assert!(entry.meta.tags.contains(&"risk:high".to_string()));
        assert!(entry.meta.tags.contains(&"owner:tom".to_string()));
        assert_eq!(entry.source, MetaSource::Sidecar);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_sidecar_degrades_to_header_only() {
        let dir = tmp_dir("sidecar-broken");
        let script = dir.join("task.sh");
        fs::write(
            &script,
            "#!/bin/bash\n# scriptvault.name: GoodHeader\necho x\n",
        )
        .unwrap();
        // Invalid YAML (a bare unmatched bracket).
        fs::write(sidecar::sidecar_path(&script), "name: [unclosed\n").unwrap();

        // Must NOT fail, and must keep the header annotations.
        let entry = parse(&script).unwrap();
        assert_eq!(entry.display_name(), "GoodHeader");
        assert_eq!(entry.source, MetaSource::Header);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_utf8_file_is_dropped_by_parse_all() {
        let dir = tmp_dir("binary");
        let bin = dir.join("blob.bin");
        // Bytes that are not valid UTF-8 -> read_to_string fails -> dropped.
        fs::write(&bin, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let good = dir.join("ok.sh");
        fs::write(&good, "#!/bin/bash\necho ok\n").unwrap();

        let entries = parse_all(&[bin, good]);
        // Only the readable script survives.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "ok.sh");

        fs::remove_dir_all(&dir).ok();
    }
}

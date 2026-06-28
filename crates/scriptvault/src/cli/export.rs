// cli/export.rs — structured (json/csv) serializers for `search --format`.
// -----------------------------------------------------------------------------
// Turns the engine's `SearchResult`s into a SMALL, stable export shape — just the
// fields a human wants in a saved scan (name, lang, path, tags, desc). We do NOT
// serialize `SearchResult` directly: it carries search-internals (score,
// matched_field, matched_indices) that are noise in an export and would couple
// the file format to ranking internals. A dedicated DTO keeps the output clean
// and lets the format evolve independently of the search engine.

use scriptvault_core::SearchResult;
use serde::Serialize;

/// One exported script: the user-facing fields, flattened (no nesting, no search
/// internals). `Serialize` drives the JSON output; CSV is written by hand so we
/// control quoting without pulling in a csv crate.
#[derive(Debug, Serialize)]
struct ExportRow {
    name: String,
    lang: String,
    path: String,
    tags: Vec<String>,
    desc: String,
}

impl ExportRow {
    fn from_result(r: &SearchResult) -> Self {
        ExportRow {
            // display_name() falls back to the filename when there's no explicit
            // name, so this column is never empty — matches the table view.
            name: r.entry.display_name().to_string(),
            lang: r.entry.lang.label().to_string(),
            path: r.entry.path.display().to_string(),
            tags: r.entry.meta.tags.clone(),
            desc: r.entry.meta.desc.clone().unwrap_or_default(),
        }
    }
}

/// Serialize results as a pretty JSON array. Always valid JSON — an empty slice
/// yields `[]` — so `… --format json > file.json` is always a parseable file.
pub(crate) fn to_json(results: &[SearchResult]) -> String {
    let rows: Vec<ExportRow> = results.iter().map(ExportRow::from_result).collect();
    // serde_json only errors on non-serializable types or map-key issues; our DTO
    // is plain strings/vecs, so this cannot fail in practice. Degrade to `[]`
    // rather than unwrap/panic, honouring the no-panic rule in non-test code.
    serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string())
}

/// Serialize results as CSV with a header row. Tags are joined by `;` (so the
/// cell itself contains no commas); every field is quoted via `csv_field` so
/// embedded commas, quotes, or newlines can't break the columns.
pub(crate) fn to_csv(results: &[SearchResult]) -> String {
    let mut out = String::from("name,lang,path,tags,desc\n");
    for r in results {
        let row = ExportRow::from_result(r);
        let tags = row.tags.join(";");
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            csv_field(&row.name),
            csv_field(&row.lang),
            csv_field(&row.path),
            csv_field(&tags),
            csv_field(&row.desc),
        ));
    }
    out
}

/// Quote a CSV field per RFC 4180: wrap in double quotes and double any interior
/// quote. We quote unconditionally — simpler than detecting "needs quoting", and
/// every RFC-4180 reader (Excel, pandas, the `csv` crate) accepts it.
fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scriptvault_core::{Language, MatchField, MetaSource, ScriptEntry, ScriptMetadata};
    use std::path::PathBuf;

    fn result(name: &str, desc: &str, tags: &[&str], path: &str) -> SearchResult {
        SearchResult {
            entry: ScriptEntry {
                path: PathBuf::from(path),
                filename: "x.sh".to_string(),
                lang: Language::Bash,
                meta: ScriptMetadata {
                    name: Some(name.to_string()),
                    desc: (!desc.is_empty()).then(|| desc.to_string()),
                    tags: tags.iter().map(|t| t.to_string()).collect(),
                    ..Default::default()
                },
                source: MetaSource::Header,
            },
            score: 100,
            matched_field: MatchField::Name,
            matched_indices: Vec::new(),
        }
    }

    #[test]
    fn json_is_an_array_of_the_exported_fields() {
        let results = [result("deploy", "ship it", &["ci", "prod"], "/s/deploy.sh")];
        let json = to_json(&results);
        // Round-trips to the documented shape, and carries NO search internals.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.is_array());
        let row = &v[0];
        assert_eq!(row["name"], "deploy");
        assert_eq!(row["lang"], "bash");
        assert_eq!(row["path"], "/s/deploy.sh");
        assert_eq!(row["tags"][0], "ci");
        assert_eq!(row["desc"], "ship it");
        assert!(row.get("score").is_none(), "no search internals leak");
        assert!(row.get("matched_field").is_none());
    }

    #[test]
    fn json_of_empty_results_is_valid_empty_array() {
        assert_eq!(to_json(&[]), "[]");
    }

    #[test]
    fn csv_has_header_and_quotes_fields_safely() {
        // A desc containing a comma and a quote must not break the columns.
        let results = [result(
            "deploy",
            r#"ship it, "now""#,
            &["ci", "prod"],
            "/s/deploy.sh",
        )];
        let csv = to_csv(&results);
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), "name,lang,path,tags,desc");
        let row = lines.next().unwrap();
        // tags joined by ; inside one quoted cell; embedded quote doubled.
        assert!(row.contains("\"ci;prod\""));
        assert!(row.contains(r#""ship it, ""now""""#));
    }
}

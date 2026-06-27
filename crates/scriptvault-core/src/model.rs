// model.rs — the shared data types flowing through the pipeline:
//   scan -> parse -> ScriptEntry -> index -> SearchResult -> (CLI / TUI / GUI)
// Every public type derives Serialize/Deserialize so a future Tauri GUI can ship
// them across a JSON IPC bridge without a change to core.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What we know about a script beyond where it lives. Every field is optional: an
/// unannotated file still produces a valid (all-`None`/empty) `ScriptMetadata`,
/// searchable by filename + inferred language. `#[serde(default)]` lets a sidecar
/// override only the fields it sets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptMetadata {
    /// Human-friendly display name. Falls back to the filename when absent.
    pub name: Option<String>,
    /// One-line description shown in the results list and the preview pane.
    pub desc: Option<String>,
    /// Free-form keywords. Parsed from a comma list in headers; fuzzy-searchable.
    pub tags: Vec<String>,
    /// How to invoke the script, e.g. "backup-db.sh [--full]".
    pub usage: Option<String>,
    /// A grouping label, e.g. "database" or "git". Useful for filtering later.
    pub category: Option<String>,
    /// Explicit language label. If `None`, we infer it (see `Language`).
    pub lang: Option<String>,
    /// Optional bridge/import risk value. Normalized into a `risk:<level>` tag
    /// during parsing so existing query/filter code keeps one convention.
    pub risk: Option<String>,
    /// Optional ownership value. Normalized into an `owner:<name>` tag during
    /// parsing for search/filter compatibility with sidecar metadata.
    pub owner: Option<String>,
}

/// Where an entry's metadata came from (drives a UI "has a sidecar" hint).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetaSource {
    /// Only an inline `# scriptvault.*` header was found.
    Header,
    /// Only a `<file>.scriptvault.yaml` sidecar was found.
    Sidecar,
    /// Both were found and merged (sidecar values won on conflict).
    Both,
    /// No annotations at all — indexed by filename + inferred language only.
    None,
}

/// The languages we recognize, plus a fallback. Each knows its line-comment
/// leader (`comment_leader`) so the header parser has one source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Bash,
    Python,
    Rust,
    Node,
    Ruby,
    Lua,
    Sql,
    /// Anything we could not identify. Still fully indexable by filename.
    Unknown,
}

impl Language {
    /// The line-comment marker the header parser scans for `scriptvault.*` keys.
    /// Unknown files try `#`, the most common scripting case.
    pub fn comment_leader(&self) -> &'static str {
        match self {
            Language::Bash | Language::Python | Language::Ruby => "#",
            Language::Rust | Language::Node => "//",
            Language::Sql | Language::Lua => "--",
            Language::Unknown => "#",
        }
    }

    /// A short, stable label for display and for the `lang` search field.
    pub fn label(&self) -> &'static str {
        match self {
            Language::Bash => "bash",
            Language::Python => "python",
            Language::Rust => "rust",
            Language::Node => "node",
            Language::Ruby => "ruby",
            Language::Lua => "lua",
            Language::Sql => "sql",
            Language::Unknown => "unknown",
        }
    }

    /// Parse a language from a free-text label (case-insensitive, trimmed),
    /// accepting common aliases. `None` for anything unrecognized — the query
    /// parser treats an unknown `lang:` as "no filter", not an error. The single
    /// source of truth for label→Language (the header parser delegates here).
    pub fn from_label(label: &str) -> Option<Language> {
        match label.trim().to_ascii_lowercase().as_str() {
            "bash" | "sh" | "shell" | "zsh" => Some(Language::Bash),
            "python" | "py" | "python3" => Some(Language::Python),
            "rust" | "rs" => Some(Language::Rust),
            "node" | "js" | "javascript" | "ts" | "typescript" => Some(Language::Node),
            "ruby" | "rb" => Some(Language::Ruby),
            "lua" => Some(Language::Lua),
            "sql" => Some(Language::Sql),
            _ => None,
        }
    }
}

/// One fully-resolved, indexable script — the unit the index stores and a
/// frontend renders. By the time it exists, path/language/metadata are final.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptEntry {
    /// Absolute path to the script on disk.
    pub path: PathBuf,
    /// The basename (e.g. "backup-db.sh"). Always present — the universal
    /// search fallback, so even a bare, unannotated file is findable.
    pub filename: String,
    /// Resolved language: explicit metadata > shebang > extension > Unknown.
    pub lang: Language,
    /// The merged metadata (sidecar overrides header).
    pub meta: ScriptMetadata,
    /// Where the metadata came from — see `MetaSource`.
    pub source: MetaSource,
}

impl ScriptEntry {
    /// The best display name: the explicit `name` if set, else the filename.
    /// Centralized so every frontend shows the same thing.
    pub fn display_name(&self) -> &str {
        self.meta.name.as_deref().unwrap_or(&self.filename)
    }
}

/// Which field caused a search hit (drives UI highlighting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchField {
    Name,
    Desc,
    Tags,
    Filename,
}

/// One hit from a query. OWNS a cloned `ScriptEntry` (rather than borrowing the
/// index) for simple lifetimes; trivial cost at our scale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matched script (an owned clone).
    pub entry: ScriptEntry,
    /// Fuzzy-match score; results are sorted by this descending.
    pub score: i64,
    /// Which field produced the (best) match — for UI highlighting.
    pub matched_field: MatchField,
    /// CHARACTER positions (not byte offsets) in the matched field a frontend
    /// bolds to show why the result matched. Empty on the "show all" view.
    pub matched_indices: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_label_parses_canonical_names() {
        assert_eq!(Language::from_label("bash"), Some(Language::Bash));
        assert_eq!(Language::from_label("python"), Some(Language::Python));
        assert_eq!(Language::from_label("rust"), Some(Language::Rust));
        assert_eq!(Language::from_label("node"), Some(Language::Node));
        assert_eq!(Language::from_label("ruby"), Some(Language::Ruby));
        assert_eq!(Language::from_label("lua"), Some(Language::Lua));
        assert_eq!(Language::from_label("sql"), Some(Language::Sql));
    }

    #[test]
    fn from_label_is_case_insensitive_and_trims() {
        assert_eq!(Language::from_label("  BASH "), Some(Language::Bash));
        assert_eq!(Language::from_label("Python"), Some(Language::Python));
    }

    #[test]
    fn from_label_accepts_common_aliases() {
        assert_eq!(Language::from_label("sh"), Some(Language::Bash));
        assert_eq!(Language::from_label("py"), Some(Language::Python));
        assert_eq!(Language::from_label("rs"), Some(Language::Rust));
        assert_eq!(Language::from_label("js"), Some(Language::Node));
    }

    #[test]
    fn from_label_unknown_is_none() {
        assert_eq!(Language::from_label("cobol"), None);
        assert_eq!(Language::from_label(""), None);
    }

    #[test]
    fn from_label_roundtrips_with_label() {
        // Every concrete language's canonical label must parse back to itself.
        for lang in [
            Language::Bash,
            Language::Python,
            Language::Rust,
            Language::Node,
            Language::Ruby,
            Language::Lua,
            Language::Sql,
        ] {
            assert_eq!(Language::from_label(lang.label()), Some(lang));
        }
    }
}

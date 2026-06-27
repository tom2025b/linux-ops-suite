// index — hold all entries and fuzzy-search them.
//
// For each entry, score the query against every field and keep the BEST score +
// which field won (-> `MatchField` for highlighting); a hit if ANY field matches.
// Ranking is TIERED, not arithmetic:
//   1. field rank  (Name > Tags > Desc > Filename)   <- primary
//   2. raw skim score (higher first)                 <- within a tier
//   3. display name (A→Z), then path (A→Z)           <- total, stable order
// Tiered because skim scores the best matching subsequence, so equal-quality
// matches across fields tie on raw score — a magic weight would be arbitrary.
// An empty query returns ALL entries by display name. `Index` is a concrete
// struct (no `trait Index`): a future SQLite swap is contained behind these
// signatures (YAGNI on the abstraction).

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::model::{MatchField, ScriptEntry, SearchResult};

/// The in-memory index of all discovered scripts.
pub struct Index {
    entries: Vec<ScriptEntry>,
    /// Built once, reused across queries (not reconstructed per search).
    matcher: SkimMatcherV2,
}

impl Index {
    /// Build an index from parsed entries.
    pub fn build(entries: Vec<ScriptEntry>) -> Self {
        Self {
            entries,
            // `default()` enables smart-case (uppercase makes a query sensitive).
            matcher: SkimMatcherV2::default(),
        }
    }

    /// All entries, borrowed — for "browse everything" views.
    pub fn entries(&self) -> &[ScriptEntry] {
        &self.entries
    }

    /// Consume the index, handing back its owned entries.
    pub fn into_entries(self) -> Vec<ScriptEntry> {
        self.entries
    }

    /// Fuzzy-search the entries. An empty/whitespace query returns everything
    /// (sorted by display name); otherwise returns the matching entries ordered
    /// by the tiered ranking described above.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let query = query.trim();

        if query.is_empty() {
            return self.all_results();
        }

        // Score every entry; keep only the hits (those with at least one
        // matching field), cloning the entry into an owned SearchResult.
        let mut results: Vec<SearchResult> = self
            .entries
            .iter()
            .filter_map(|entry| self.score_entry(entry, query))
            .collect();

        sort_results(&mut results);
        results
    }

    /// The "show everything" result set, ordered by display name then path so
    /// the initial TUI view is deterministic.
    fn all_results(&self) -> Vec<SearchResult> {
        let mut results: Vec<SearchResult> = self
            .entries
            .iter()
            .map(|entry| SearchResult {
                entry: entry.clone(),
                score: 0,
                // Arbitrary but harmless: nothing is highlighted on an empty query.
                matched_field: MatchField::Name,
                matched_indices: Vec::new(),
            })
            .collect();

        results.sort_by(|a, b| {
            display_name(&a.entry)
                .cmp(display_name(&b.entry))
                .then_with(|| a.entry.path.cmp(&b.entry.path))
        });
        results
    }

    /// Score one entry against a SINGLE query term across all fields, returning
    /// the best `(score, winning field, matched char positions)` or `None` if no
    /// field matched. This is the shared scoring primitive: `score_entry` wraps
    /// it into a `SearchResult` for the legacy `search`, and the query engine
    /// calls it per-term to implement multi-term AND-across-fields matching.
    ///
    /// Ties prefer the higher-priority (lower-rank) field, matching the global
    /// ranking. `fuzzy_indices` returns CHARACTER positions (verified against
    /// fuzzy-matcher 0.3.7: "déploy"/"dpl" → [0,2,3], char not byte offsets), so
    /// the indices are stored directly — no byte→char conversion (which would
    /// corrupt multi-byte text).
    pub(crate) fn score_term(
        &self,
        entry: &ScriptEntry,
        term: &str,
    ) -> Option<(i64, MatchField, Vec<usize>)> {
        let mut best: Option<(i64, MatchField, Vec<usize>)> = None;

        let mut consider = |text: &str, field: MatchField| {
            if let Some((score, indices)) = self.matcher.fuzzy_indices(text, term) {
                let take = match &best {
                    Some((bs, bf, _)) => {
                        score > *bs || (score == *bs && field_rank(field) < field_rank(*bf))
                    }
                    None => true,
                };
                if take {
                    best = Some((score, field, indices));
                }
            }
        };

        // Name — only when there is an EXPLICIT name. If absent, the basename is
        // covered by the Filename field below (otherwise we'd score the filename
        // twice and mislabel a filename hit as a Name hit).
        if let Some(name) = entry.meta.name.as_deref() {
            consider(name, MatchField::Name);
        }
        // Tags: consider each tag; `consider` keeps the best automatically.
        for tag in &entry.meta.tags {
            consider(tag, MatchField::Tags);
        }
        // Description.
        if let Some(desc) = entry.meta.desc.as_deref() {
            consider(desc, MatchField::Desc);
        }
        // Filename (always present).
        consider(&entry.filename, MatchField::Filename);

        best
    }

    /// Score one entry against the query. Returns `Some(SearchResult)` if any
    /// field matched, recording the best score, the winning field, and the
    /// CHARACTER positions that matched (for UI highlighting).
    fn score_entry(&self, entry: &ScriptEntry, query: &str) -> Option<SearchResult> {
        self.score_term(entry, query)
            .map(|(score, matched_field, indices)| SearchResult {
                entry: entry.clone(),
                score,
                matched_field,
                matched_indices: indices,
            })
    }
}

/// The display name used for searching/sorting: explicit name or filename.
fn display_name(entry: &ScriptEntry) -> &str {
    entry.display_name()
}

/// Lower rank = higher priority. Drives the primary sort key.
fn field_rank(field: MatchField) -> u8 {
    match field {
        MatchField::Name => 0,
        MatchField::Tags => 1,
        MatchField::Desc => 2,
        MatchField::Filename => 3,
    }
}

/// Apply the tiered ordering to a result set:
///   field rank asc -> raw score desc -> display name asc -> path asc.
fn sort_results(results: &mut [SearchResult]) {
    results.sort_by(|a, b| {
        // 1. Field rank (Name first).
        field_rank(a.matched_field)
            .cmp(&field_rank(b.matched_field))
            // 2. Higher raw score first (note: b vs a).
            .then_with(|| b.score.cmp(&a.score))
            // 3. Display name A→Z.
            .then_with(|| display_name(&a.entry).cmp(display_name(&b.entry)))
            // 4. Path A→Z — guarantees a total, platform-independent order.
            .then_with(|| a.entry.path.cmp(&b.entry.path))
    });
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Language, MetaSource, ScriptMetadata};
    use std::path::PathBuf;

    /// Build a minimal entry with the given name/desc/tags/filename.
    fn entry(name: Option<&str>, desc: Option<&str>, tags: &[&str], filename: &str) -> ScriptEntry {
        ScriptEntry {
            path: PathBuf::from(format!("/scripts/{filename}")),
            filename: filename.to_string(),
            lang: Language::Bash,
            meta: ScriptMetadata {
                name: name.map(str::to_string),
                desc: desc.map(str::to_string),
                tags: tags.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            source: MetaSource::None,
        }
    }

    #[test]
    fn matched_indices_are_char_positions_for_winning_field() {
        // Name "déploy" — the accented 'é' is 2 bytes but 1 char. A query "dpl"
        // matches chars at positions 0,2,3 (d, p, l) — these must be CHAR indices,
        // so a UTF-8 name highlights correctly rather than off-by-bytes.
        let idx = Index::build(vec![entry(Some("déploy"), None, &[], "x.sh")]);
        let results = idx.search("dpl");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched_field, MatchField::Name);
        // 'd'=0, 'é'=1, 'p'=2, 'l'=3, 'o'=4, 'y'=5  → "dpl" hits 0,2,3
        assert_eq!(results[0].matched_indices, vec![0, 2, 3]);
    }

    #[test]
    fn empty_query_has_no_matched_indices() {
        let idx = Index::build(vec![entry(Some("alpha"), None, &[], "a.sh")]);
        let results = idx.search("   ");
        assert!(results[0].matched_indices.is_empty());
    }

    #[test]
    fn empty_query_returns_all_sorted_by_name() {
        let idx = Index::build(vec![
            entry(Some("zebra"), None, &[], "z.sh"),
            entry(Some("apple"), None, &[], "a.sh"),
        ]);
        let results = idx.search("   ");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].entry.display_name(), "apple");
        assert_eq!(results[1].entry.display_name(), "zebra");
    }

    #[test]
    fn name_match_outranks_desc_match_tiered() {
        // THE discriminating test from the design review. Probe showed both
        // score the same raw value (71); the tiered rule must still rank the
        // NAME match first.
        let idx = Index::build(vec![
            // entry A: matches via name only.
            entry(Some("dep"), None, &[], "a.sh"),
            // entry B: matches via a long description only.
            entry(
                Some("unrelated"),
                Some("deploy deploy deployment deploy"),
                &[],
                "b.sh",
            ),
        ]);
        let results = idx.search("dep");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].matched_field, MatchField::Name);
        assert_eq!(results[0].entry.display_name(), "dep");
        assert_eq!(results[1].matched_field, MatchField::Desc);
    }

    #[test]
    fn tag_match_outranks_desc_and_filename() {
        let idx = Index::build(vec![
            entry(Some("x"), Some("nothing here"), &["backup"], "x.sh"),
            entry(Some("y"), Some("backup related text"), &[], "y.sh"),
        ]);
        let results = idx.search("backup");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].matched_field, MatchField::Tags);
        assert_eq!(results[0].entry.display_name(), "x");
    }

    #[test]
    fn unannotated_entry_matches_by_filename() {
        // No name/desc/tags — only the filename. Must still be findable.
        let idx = Index::build(vec![entry(None, None, &[], "backup-db.sh")]);
        let results = idx.search("bkdb"); // fuzzy subsequence of "backup-db"
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched_field, MatchField::Filename);
    }

    #[test]
    fn non_matching_query_returns_nothing() {
        let idx = Index::build(vec![entry(Some("deploy"), None, &[], "d.sh")]);
        assert!(idx.search("zzzzzz").is_empty());
    }

    #[test]
    fn within_tier_higher_score_first() {
        // Two NAME matches: the closer match ("deploy" vs "dep") should score
        // higher than the looser one and come first within the Name tier.
        let idx = Index::build(vec![
            entry(Some("deeployments"), None, &[], "a.sh"),
            entry(Some("dep"), None, &[], "b.sh"),
        ]);
        let results = idx.search("dep");
        // Both match by name; both in the same tier, so raw score orders them.
        assert!(results.iter().all(|r| r.matched_field == MatchField::Name));
        assert!(results[0].score >= results[1].score);
    }

    #[test]
    fn smart_case_is_inherited() {
        let idx = Index::build(vec![entry(Some("Deploy"), None, &[], "d.sh")]);
        // Lowercase query: case-insensitive -> matches.
        assert_eq!(idx.search("deploy").len(), 1);
        // Uppercase in query: case-sensitive -> "Deploy" still matches "Dep".
        assert_eq!(idx.search("Dep").len(), 1);
    }
}

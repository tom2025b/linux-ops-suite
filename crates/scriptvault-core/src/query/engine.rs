// ============================================================================
// crates/scriptvault-core/src/query/engine.rs
// ============================================================================
// engine::run — the single pipeline that turns a structured `Query` into a
// ranked, filtered `Vec<SearchResult>`. This is what every frontend calls; none
// of them re-implement view composition, filtering, or ranking.
//
// Pipeline:
//   1. CANDIDATES  every entry in the index
//   2. VIEW        narrow to All / Favorites / Recents / Playlist
//   3. FILTER      apply each structured Filter (tag/cat/lang/fav/risk/-tag), AND
//   4. MATCH+SCORE fuzzy-score survivors against the query text. Empty text =>
//                  all survivors match (score 0). Multi-term text => every term
//                  must hit at least one field (AND across fields); the score is
//                  the sum of per-term scores; the badge is the best term's
//                  winning field; highlights union across terms on that field.
//   5. RANK        the hybrid score (fuzzy + capped frecency + fav) for
//                  Sort::Auto, or a plain order for Name/RecentlyRun/Modified.
//   6. LIMIT       truncate to query.limit if set.
// ============================================================================

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::index::Index;
use crate::model::{MatchField, ScriptEntry, SearchResult};
use crate::state::State;

use super::ranking::{self, Frecency};
use super::{Filter, Query, RiskLevel, Sort, View};

/// Execute a structured query against the index + persisted state, returning
/// ranked, filtered results.
pub fn run(index: &Index, state: &State, query: &Query) -> Vec<SearchResult> {
    let now = now_secs();

    // 1-3: candidate entries surviving the view + structured filters.
    let candidates = index
        .entries()
        .iter()
        .filter(|e| in_view(e, &query.view, state))
        .filter(|e| query.filters.iter().all(|f| passes_filter(e, f, state)));

    // 4: match + score against the fuzzy text (multi-term AND across fields).
    let terms: Vec<&str> = query.text.split_whitespace().collect();
    let mut results: Vec<SearchResult> = candidates
        .filter_map(|entry| match_entry(index, entry, &terms))
        .collect();

    // 5: rank.
    rank(&mut results, query.sort, state, now);

    // 6: limit.
    if let Some(limit) = query.limit {
        results.truncate(limit);
    }
    results
}

// ----------------------------------------------------------------------------
// View
// ----------------------------------------------------------------------------
fn in_view(entry: &ScriptEntry, view: &View, state: &State) -> bool {
    match view {
        View::All => true,
        View::Favorites => state.is_favorite(&entry.path),
        View::Recents => state.recents.iter().any(|r| r.path == entry.path),
        View::Playlist(name) => state
            .playlist_named(name)
            .is_some_and(|pl| pl.paths.iter().any(|p| p == &entry.path)),
    }
}

// ----------------------------------------------------------------------------
// Structured filters
// ----------------------------------------------------------------------------
fn passes_filter(entry: &ScriptEntry, filter: &Filter, state: &State) -> bool {
    match filter {
        Filter::Tag(t) => has_tag_containing(entry, t),
        Filter::NotTag(t) => !has_tag_containing(entry, t),
        Filter::Category(c) => entry
            .meta
            .category
            .as_deref()
            .is_some_and(|cat| cat.to_lowercase().contains(&c.to_lowercase())),
        Filter::Lang(l) => entry.lang == *l,
        Filter::Favorite => state.is_favorite(&entry.path),
        Filter::Risk(level) => has_risk(entry, *level),
        Filter::Playlist(name) => state
            .playlist_named(name)
            .is_some_and(|pl| pl.paths.iter().any(|p| p == &entry.path)),
    }
}

/// Tags are normalized (lowercased) at parse time, but match case-insensitively
/// and by `contains` to mirror today's TUI behavior (so the migration preserves
/// results). The query value is already lowercased by the parser.
fn has_tag_containing(entry: &ScriptEntry, needle: &str) -> bool {
    entry
        .meta
        .tags
        .iter()
        .any(|tag| tag.to_lowercase().contains(needle))
}

/// Risk is encoded as a `risk:<level>` tag (the Bulwark bridge convention).
fn has_risk(entry: &ScriptEntry, level: RiskLevel) -> bool {
    let wanted = format!("risk:{}", level.label());
    entry.meta.tags.iter().any(|t| t.to_lowercase() == wanted)
        || entry
            .meta
            .risk
            .as_deref()
            .is_some_and(|risk| risk.trim().eq_ignore_ascii_case(level.label()))
}

// ----------------------------------------------------------------------------
// Match + score (multi-term AND across fields)
// ----------------------------------------------------------------------------
fn match_entry(index: &Index, entry: &ScriptEntry, terms: &[&str]) -> Option<SearchResult> {
    // Empty query: everything matches with score 0, no highlights.
    if terms.is_empty() {
        return Some(SearchResult {
            entry: entry.clone(),
            score: 0,
            matched_field: MatchField::Name,
            matched_indices: Vec::new(),
        });
    }

    let mut total_score: i64 = 0;
    // The badge shows the field of the BEST-scoring term; track it.
    let mut best_field: Option<(i64, MatchField)> = None;
    // Highlights union across terms, but only those that landed on the field we
    // ultimately display. We collect per-(field, indices) and resolve at the end.
    let mut per_term: Vec<(i64, MatchField, Vec<usize>)> = Vec::with_capacity(terms.len());

    for term in terms {
        // Every term must hit at least one field, else this entry is not a match.
        let (score, field, indices) = index.score_term(entry, term)?;
        total_score += score;
        per_term.push((score, field, indices.clone()));
        if best_field.is_none_or(|(bs, _)| score > bs) {
            best_field = Some((score, field));
        }
    }

    let (_, display_field) = best_field?; // terms non-empty => Some
    // Union the matched indices from every term that scored on the display field.
    let mut indices: Vec<usize> = per_term
        .iter()
        .filter(|(_, f, _)| *f == display_field)
        .flat_map(|(_, _, idx)| idx.iter().copied())
        .collect();
    indices.sort_unstable();
    indices.dedup();

    Some(SearchResult {
        entry: entry.clone(),
        score: total_score,
        matched_field: display_field,
        matched_indices: indices,
    })
}

// ----------------------------------------------------------------------------
// Ranking
// ----------------------------------------------------------------------------
fn rank(results: &mut [SearchResult], sort: Sort, state: &State, now: u64) {
    let recents: HashMap<&Path, &crate::state::RecentEntry> = state
        .recents
        .iter()
        .map(|r| (r.path.as_path(), r))
        .collect();

    match sort {
        Sort::Auto => {
            // Precompute the hybrid (final score, raw frecency) per result so the
            // comparator doesn't recompute logs/powf on every comparison. Keyed on
            // owned paths so the map doesn't borrow `results` (which sort_by needs
            // mutably).
            let scored: HashMap<PathBuf, (f64, f64)> = results
                .iter()
                .map(|r| {
                    let f = frecency_for(&r.entry.path, &recents, now);
                    let is_fav = state.is_favorite(&r.entry.path);
                    let key = (
                        ranking::score(r.score as f64, &f, is_fav),
                        ranking::frecency_raw(&f),
                    );
                    (r.entry.path.clone(), key)
                })
                .collect();
            results.sort_by(|a, b| {
                let (fa, ra) = scored[&a.entry.path];
                let (fb, rb) = scored[&b.entry.path];
                // final DESC, then raw frecency DESC ("more used" wins ties), then
                // name A→Z, mtime DESC, path A→Z for a fully stable order.
                fb.total_cmp(&fa)
                    .then_with(|| rb.total_cmp(&ra))
                    .then_with(|| a.entry.display_name().cmp(b.entry.display_name()))
                    .then_with(|| mtime_of(&b.entry.path).cmp(&mtime_of(&a.entry.path)))
                    .then_with(|| a.entry.path.cmp(&b.entry.path))
            });
        }
        Sort::Name => results.sort_by(|a, b| {
            a.entry
                .display_name()
                .cmp(b.entry.display_name())
                .then_with(|| a.entry.path.cmp(&b.entry.path))
        }),
        Sort::RecentlyRun => results.sort_by(|a, b| {
            let ra = run_age(&a.entry.path, &recents, now);
            let rb = run_age(&b.entry.path, &recents, now);
            // Smaller age = more recent = first; never-run (MAX) sinks to the end.
            ra.cmp(&rb)
                .then_with(|| a.entry.display_name().cmp(b.entry.display_name()))
        }),
        Sort::Modified => results.sort_by(|a, b| {
            mtime_of(&b.entry.path)
                .cmp(&mtime_of(&a.entry.path))
                .then_with(|| a.entry.display_name().cmp(b.entry.display_name()))
        }),
    }
}

/// Build the ranking `Frecency` input for a path from the recents map.
fn frecency_for(
    path: &Path,
    recents: &HashMap<&Path, &crate::state::RecentEntry>,
    now: u64,
) -> Frecency {
    match recents.get(path) {
        Some(r) => Frecency {
            count: r.count,
            age_secs: Some(now.saturating_sub(r.last_run)),
            last_failed: matches!(r.last_exit, Some(code) if code != 0),
        },
        None => Frecency::NONE,
    }
}

/// Age in seconds since last run, or `u64::MAX` if never run (for sorting).
fn run_age(path: &Path, recents: &HashMap<&Path, &crate::state::RecentEntry>, now: u64) -> u64 {
    recents
        .get(path)
        .map(|r| now.saturating_sub(r.last_run))
        .unwrap_or(u64::MAX)
}

/// File mtime as Unix seconds, or 0 if unavailable (sorts oldest).
fn mtime_of(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Language, MetaSource, ScriptEntry, ScriptMetadata};
    use crate::state::{RecentEntry, State};
    use std::path::PathBuf;

    fn entry(name: &str, tags: &[&str], lang: Language) -> ScriptEntry {
        ScriptEntry {
            path: PathBuf::from(format!("/s/{name}.sh")),
            filename: format!("{name}.sh"),
            lang,
            meta: ScriptMetadata {
                name: Some(name.to_string()),
                tags: tags.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            source: MetaSource::Header,
        }
    }

    fn idx(entries: Vec<ScriptEntry>) -> Index {
        Index::build(entries)
    }

    #[test]
    fn empty_query_returns_all() {
        let index = idx(vec![
            entry("alpha", &[], Language::Bash),
            entry("beta", &[], Language::Bash),
        ]);
        let out = run(&index, &State::default(), &Query::default());
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn text_filters_to_matches() {
        let index = idx(vec![
            entry("deploy", &[], Language::Bash),
            entry("backup", &[], Language::Bash),
        ]);
        let out = run(&index, &State::default(), &Query::text("deploy"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "deploy");
    }

    #[test]
    fn tag_filter_narrows() {
        let index = idx(vec![
            entry("a", &["ci"], Language::Bash),
            entry("b", &["db"], Language::Bash),
        ]);
        let q = Query {
            filters: vec![Filter::Tag("ci".into())],
            ..Default::default()
        };
        let out = run(&index, &State::default(), &q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "a");
    }

    #[test]
    fn playlist_filter_composes_with_view() {
        // Filter::Playlist keeps only members of the named playlist, and composes
        // ON TOP of a view (here Favorites): a script must be BOTH favorited AND
        // in the playlist. This preserves the TUI's "view + active playlist".
        let index = idx(vec![
            entry("a", &[], Language::Bash),
            entry("b", &[], Language::Bash),
            entry("c", &[], Language::Bash),
        ]);
        let mut state = State::default();
        state.create_playlist("ops");
        state.add_to_playlist("ops", Path::new("/s/a.sh"));
        state.add_to_playlist("ops", Path::new("/s/b.sh"));
        state.toggle_favorite(Path::new("/s/a.sh")); // only a is BOTH fav + in ops
        state.toggle_favorite(Path::new("/s/c.sh"));

        let q = Query {
            view: View::Favorites,
            filters: vec![Filter::Playlist("ops".into())],
            ..Default::default()
        };
        let out = run(&index, &state, &q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "a");
    }

    #[test]
    fn playlist_filter_unknown_name_matches_nothing() {
        let index = idx(vec![entry("a", &[], Language::Bash)]);
        let q = Query {
            filters: vec![Filter::Playlist("missing".into())],
            ..Default::default()
        };
        assert!(run(&index, &State::default(), &q).is_empty());
    }

    #[test]
    fn not_tag_excludes() {
        let index = idx(vec![
            entry("a", &["ci", "wip"], Language::Bash),
            entry("b", &["ci"], Language::Bash),
        ]);
        let q = Query {
            filters: vec![Filter::NotTag("wip".into())],
            ..Default::default()
        };
        let out = run(&index, &State::default(), &q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "b");
    }

    #[test]
    fn lang_filter_keeps_only_that_language() {
        let index = idx(vec![
            entry("a", &[], Language::Bash),
            entry("b", &[], Language::Python),
        ]);
        let q = Query {
            filters: vec![Filter::Lang(Language::Python)],
            ..Default::default()
        };
        let out = run(&index, &State::default(), &q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.lang, Language::Python);
    }

    #[test]
    fn risk_filter_matches_risk_tag() {
        let index = idx(vec![
            entry("danger", &["risk:high"], Language::Bash),
            entry("safe", &["ci"], Language::Bash),
        ]);
        let q = Query {
            filters: vec![Filter::Risk(RiskLevel::High)],
            ..Default::default()
        };
        let out = run(&index, &State::default(), &q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "danger");
    }

    #[test]
    fn favorite_view_and_filter() {
        let index = idx(vec![
            entry("a", &[], Language::Bash),
            entry("b", &[], Language::Bash),
        ]);
        let mut state = State::default();
        state.toggle_favorite(Path::new("/s/b.sh"));
        let q = Query {
            view: View::Favorites,
            ..Default::default()
        };
        let out = run(&index, &state, &q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "b");
    }

    #[test]
    fn multi_term_and_across_fields() {
        // "deploy prod": one entry NAMED deploy + TAGGED prod must match even
        // though no single field contains both terms (the bug in today's code).
        let index = idx(vec![
            entry("deploy", &["prod"], Language::Bash),
            entry("deploy", &["staging"], Language::Bash), // only first term
            entry("backup", &["prod"], Language::Bash),    // only second term
        ]);
        // Disambiguate the two "deploy" by path.
        let mut entries = index.into_entries();
        entries[1].path = PathBuf::from("/s/deploy2.sh");
        entries[1].filename = "deploy2.sh".into();
        let index = idx(entries);

        let out = run(&index, &State::default(), &Query::text("deploy prod"));
        assert_eq!(out.len(), 1, "only the deploy+prod entry should match");
        assert_eq!(out[0].entry.meta.tags, vec!["prod"]);
    }

    #[test]
    fn limit_truncates() {
        let index = idx(vec![
            entry("a", &[], Language::Bash),
            entry("b", &[], Language::Bash),
            entry("c", &[], Language::Bash),
        ]);
        let q = Query {
            limit: Some(2),
            ..Default::default()
        };
        let out = run(&index, &State::default(), &q);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn auto_sort_floats_frequently_used_among_matches() {
        // Two equally-named-ish matches; the heavily+recently used one ranks
        // first under Auto, even though raw fuzzy is identical.
        let index = idx(vec![
            entry("deploy-a", &[], Language::Bash),
            entry("deploy-b", &[], Language::Bash),
        ]);
        let mut state = State::default();
        // deploy-b run a lot, recently.
        state.recents.push(RecentEntry {
            path: PathBuf::from("/s/deploy-b.sh"),
            count: 40,
            last_run: now_secs(),
            last_exit: Some(0),
            last_output: None,
        });
        let out = run(&index, &state, &Query::text("deploy"));
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].entry.display_name(),
            "deploy-b",
            "the frequently+recently used match should rank first"
        );
    }

    #[test]
    fn frecency_never_resurrects_a_non_match() {
        // The hardest end-to-end guarantee: an entry that does NOT match the
        // query text is excluded entirely, no matter how heavily it is used.
        // Frecency only reorders MATCHES; it can never pull in a non-match. This
        // is the engine-level half of the keystone invariant ("if it doesn't
        // match what you typed, it shouldn't show up" — here, at all). The
        // *ordering* half (a strong match outranks a weak one) is proven with
        // exact scores in ranking.rs::strong_fuzzy_match_beats_weak_match.
        let index = idx(vec![
            entry("deploy", &[], Language::Bash),
            entry("backup", &[], Language::Bash), // no "deploy" subsequence
        ]);
        let mut state = State::default();
        state.recents.push(RecentEntry {
            path: PathBuf::from("/s/backup.sh"),
            count: 100_000,
            last_run: now_secs(),
            last_exit: Some(0),
            last_output: None,
        });
        let out = run(&index, &state, &Query::text("deploy"));
        assert_eq!(out.len(), 1, "the heavily-used non-match must be excluded");
        assert_eq!(out[0].entry.display_name(), "deploy");
    }

    #[test]
    fn empty_query_with_frecency_orders_most_used_first() {
        // Browse (empty query) under Auto: most-used floats up automatically.
        let index = idx(vec![
            entry("zzz", &[], Language::Bash),
            entry("aaa", &[], Language::Bash),
        ]);
        let mut state = State::default();
        state.recents.push(RecentEntry {
            path: PathBuf::from("/s/zzz.sh"),
            count: 20,
            last_run: now_secs(),
            last_exit: Some(0),
            last_output: None,
        });
        let out = run(&index, &state, &Query::default());
        // zzz is used; without frecency "aaa" would sort first alphabetically.
        assert_eq!(out[0].entry.display_name(), "zzz");
    }

    #[test]
    fn name_sort_is_alphabetical() {
        let index = idx(vec![
            entry("zebra", &[], Language::Bash),
            entry("apple", &[], Language::Bash),
        ]);
        let q = Query {
            sort: Sort::Name,
            ..Default::default()
        };
        let out = run(&index, &State::default(), &q);
        assert_eq!(out[0].entry.display_name(), "apple");
        assert_eq!(out[1].entry.display_name(), "zebra");
    }

    #[test]
    fn combined_filter_and_text() {
        let index = idx(vec![
            entry("deploy", &["ci"], Language::Bash),
            entry("deploy", &["db"], Language::Bash),
        ]);
        let mut entries = index.into_entries();
        entries[1].path = PathBuf::from("/s/deploy2.sh");
        entries[1].filename = "deploy2.sh".into();
        let index = idx(entries);

        let q = Query {
            text: "deploy".into(),
            filters: vec![Filter::Tag("ci".into())],
            ..Default::default()
        };
        let out = run(&index, &State::default(), &q);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.meta.tags, vec!["ci"]);
    }
}

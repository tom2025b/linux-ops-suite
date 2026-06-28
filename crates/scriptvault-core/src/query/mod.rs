// query — the structured request a frontend builds (from a search box, clap
// flags, or form controls) and hands to `engine::run`, which turns it into
// ranked, filtered `Vec<SearchResult>`. The single place filtering, view
// composition, and ranking live, so every frontend is a thin renderer.
//   parse.rs   — free-text -> Query (the fzf-style mini-grammar)
//   ranking.rs — hybrid fuzzy + frecency + favorite scoring & comparator
//   engine.rs  — run(): candidates -> view -> filter -> match -> rank -> limit

use serde::{Deserialize, Serialize};

use crate::model::Language;

pub mod engine;
pub mod parse;
pub mod ranking;

pub use parse::{parse_query, pop_last_filter_token};

/// A complete, structured search request. Free-text becomes a `Query` via
/// [`parse_query`]; the engine executes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// The fuzzy text left after structured operators (`t:`, `lang:`, …) are
    /// stripped. Multiple whitespace-separated terms are ANDed across fields.
    pub text: String,
    /// Structured constraints, ANDed. Empty = no constraints.
    pub filters: Vec<Filter>,
    /// Which slice of the index to consider (browse views).
    pub view: View,
    /// How to order the results.
    pub sort: Sort,
    /// Cap on the number of results. `None` = no limit.
    pub limit: Option<usize>,
}

impl Query {
    /// "Just fuzzy text, defaults otherwise" — the old `search(text)` entry point.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }
}

/// A single structured constraint; multiple are ANDed. New variants can be added
/// without touching frontends that don't use them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Filter {
    /// `t:`/`tag:` — keep entries carrying this tag (case-insensitive contains).
    Tag(String),
    /// `c:`/`category:` — keep entries whose category contains this.
    Category(String),
    /// `lang:` — keep only entries of this resolved language.
    Lang(Language),
    /// `fav:` — keep only favorited entries (favorite-ness comes from state).
    Favorite,
    /// `risk:` — keep entries at this risk level (a `risk:high` tag; see `RiskLevel`).
    Risk(RiskLevel),
    /// `-t:`/`-tag:` — EXCLUDE entries carrying this tag.
    NotTag(String),
    /// Members of the named playlist. Unlike `View`, this is a composable FILTER
    /// (e.g. Favorites AND in-playlist); a frontend adds it, the parser doesn't.
    Playlist(String),
}

impl Filter {
    /// Short operator label as a user would type it (`t:ci`, `lang:bash`, …), for
    /// an active-filter "chip". `Playlist` returns `None` (shown separately).
    pub fn chip_label(&self) -> Option<String> {
        Some(match self {
            Filter::Tag(t) => format!("t:{t}"),
            Filter::Category(c) => format!("c:{c}"),
            Filter::Lang(l) => format!("lang:{}", l.label()),
            Filter::Favorite => "fav:".to_string(),
            Filter::Risk(r) => format!("risk:{}", r.label()),
            Filter::NotTag(t) => format!("-t:{t}"),
            Filter::Playlist(_) => return None,
        })
    }
}

/// The browse "view" — a coarse pre-filter before structured filters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    #[default]
    All,
    Favorites,
    Recents,
    Playlist(String),
}

/// Result ordering. `Auto` (the default) is the hybrid that makes the tool feel
/// smart; the rest are plain orders the user can ask for explicitly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    /// Fuzzy match quality, then frecency, then name/mtime.
    #[default]
    Auto,
    /// Alphabetical by display name.
    Name,
    /// Most recently run first.
    RecentlyRun,
    /// Most recently modified on disk first.
    Modified,
}

/// A script's risk classification, encoded as a `risk:<level>` tag (written by
/// the Bulwark bridge). An enum, not a string, so the `risk:high` run-confirm
/// and frontends can reason about it precisely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    /// The canonical lowercase label used in the `risk:<level>` tag.
    pub fn label(&self) -> &'static str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
        }
    }

    /// Parse a risk level from a free-text label (case-insensitive). Returns
    /// `None` for anything unrecognized.
    pub fn from_label(label: &str) -> Option<RiskLevel> {
        match label.trim().to_ascii_lowercase().as_str() {
            "low" => Some(RiskLevel::Low),
            "medium" | "med" => Some(RiskLevel::Medium),
            "high" => Some(RiskLevel::High),
            _ => None,
        }
    }
}

#[cfg(test)]
mod chip_tests {
    use super::*;

    #[test]
    fn chip_label_renders_each_filter() {
        assert_eq!(
            Filter::Tag("ci".into()).chip_label().as_deref(),
            Some("t:ci")
        );
        assert_eq!(
            Filter::Category("ops".into()).chip_label().as_deref(),
            Some("c:ops")
        );
        assert_eq!(
            Filter::Lang(Language::Bash).chip_label().as_deref(),
            Some("lang:bash")
        );
        assert_eq!(Filter::Favorite.chip_label().as_deref(), Some("fav:"));
        assert_eq!(
            Filter::Risk(RiskLevel::High).chip_label().as_deref(),
            Some("risk:high")
        );
        assert_eq!(
            Filter::NotTag("wip".into()).chip_label().as_deref(),
            Some("-t:wip")
        );
    }

    #[test]
    fn playlist_filter_has_no_chip() {
        assert_eq!(Filter::Playlist("nightly".into()).chip_label(), None);
    }

    #[test]
    fn chip_labels_round_trip_through_the_parser() {
        // A chip label, fed back to the parser, must reproduce the same filter —
        // so the label a user sees is exactly what they could type.
        for f in [
            Filter::Tag("ci".into()),
            Filter::Category("ops".into()),
            Filter::Lang(Language::Bash),
            Filter::Favorite,
            Filter::Risk(RiskLevel::High),
            Filter::NotTag("wip".into()),
        ] {
            let label = f.chip_label().unwrap();
            let parsed = parse_query(&label);
            assert_eq!(
                parsed.filters,
                vec![f.clone()],
                "round-trip failed for {label}"
            );
            assert!(
                parsed.text.is_empty(),
                "label {label} leaked into fuzzy text"
            );
        }
    }
}

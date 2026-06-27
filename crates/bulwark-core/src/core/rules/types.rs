//! Rule data types: the YAML-facing "vocabulary" for classification rules.
//!
//! This module defines the data shapes that users write in their `rules.yaml`
//! files and that Bulwark uses internally to decide risk, category, and owner.
//!
//! The matching *behavior* lives in `matching.rs` and `engine.rs`. These types
//! are deliberately "dumb" so they can be reused everywhere (CLI, future TUI,
//! tests, etc.).

use serde::{Deserialize, Serialize};

/// Risk level assigned to a file by the rule engine.
///
/// This is one of the most important pieces of metadata Bulwark produces.
/// It is deliberately ordered (Low < Medium < High < Critical) so that:
/// - Reports can be sorted consistently.
/// - The summary line always shows risks in the same severity order.
/// - Future UIs (TUI, web, etc.) can color-code or prioritize reliably.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Lowest concern. Normal user scripts and tools.
    Low,
    /// Medium concern. Usually compiled binaries or less common tools.
    Medium,
    /// High concern. Commands that can cause significant damage (rm, dd, etc.).
    High,
    /// Highest concern. Reserved for extremely dangerous items.
    Critical,
}

impl RiskLevel {
    /// Parse a free-form risk token (e.g. from a sidecar's `risk:` field).
    ///
    /// Accepts the canonical lowercase names case-insensitively (`low`, `medium`,
    /// `high`, `critical`). Returns `None` for anything else so callers can
    /// surface a clear warning instead of silently picking a wrong level.
    pub fn from_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "low" => Some(RiskLevel::Low),
            "medium" => Some(RiskLevel::Medium),
            "high" => Some(RiskLevel::High),
            "critical" => Some(RiskLevel::Critical),
            _ => None,
        }
    }
}

/// The final classification result for a single discovered file.
///
/// This is the "output" of the entire rule engine for one item. Every
/// `ClassifiedEntry` contains exactly one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    /// How dangerous this item is considered.
    pub risk: RiskLevel,

    /// Human-readable category (e.g. "script", "binary", "config", "tool").
    ///
    /// This is free-form text chosen by the rules. It is intended for humans,
    /// not for machine parsing in most cases.
    pub category: String,

    /// Who "owns" this in the user's mental model (e.g. "user", "system", "vendor").
    ///
    /// This helps people quickly understand provenance when looking at a long
    /// inventory.
    pub owner: String,
}

/// Declarative match conditions for a rule.
///
/// This is the "if" part of a rule. All fields are optional and combine with
/// **logical AND**. A rule only matches a file if *every* condition that is
/// present evaluates to true.
///
/// Empty `MatchSpec` (all fields absent/default) matches everything — use with
/// care!
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchSpec {
    /// Exact filename matches (no path, just the final component).
    ///
    /// Example: `names: ["rm", "deploy.sh"]`
    #[serde(default)]
    pub names: Vec<String>,

    /// If present, the file's `is_executable` flag must equal this value.
    ///
    /// This is one of the most useful conditions for personal tool inventories.
    #[serde(default)]
    pub executable: Option<bool>,

    /// Filename extensions that match (include the dot, e.g. ".sh", ".py").
    ///
    /// Bulwark normalizes both with and without the leading dot for convenience.
    #[serde(default)]
    pub extensions: Vec<String>,

    /// Language names that match (e.g. "Bash", "Python", "Rust").
    /// These are matched (case-insensitively) against the inferred `Language`.
    #[serde(default)]
    pub languages: Vec<String>,

    /// Path prefixes that match (checked against the full absolute path string).
    /// Useful for distinguishing user tools (`~/bin`) vs system tools (`/usr/bin`).
    #[serde(default)]
    pub path_prefixes: Vec<String>,
}

/// A single classification rule.
///
/// This is the complete unit that users write in `rules.yaml`.
///
/// A `Rule` has two halves:
/// - `r#match`: the conditions that must be true (see [`MatchSpec`]).
/// - `classify`: what to assign if the match succeeds (risk + category + owner).
///
/// Rules are evaluated in order; the *last* matching rule wins. This is
/// intentional and powerful — it lets later (usually user) rules override
/// earlier (usually built-in) ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Unique, stable name for the rule (used in reports and debugging).
    ///
    /// Good names make it obvious *why* a file received a particular
    /// classification when you look at the output.
    pub name: String,

    /// Optional human explanation of what this rule is trying to achieve.
    #[serde(default)]
    pub description: Option<String>,

    /// The conditions under which this rule applies.
    pub r#match: MatchSpec,

    /// What classification to apply when the rule matches.
    pub classify: Classification,
}

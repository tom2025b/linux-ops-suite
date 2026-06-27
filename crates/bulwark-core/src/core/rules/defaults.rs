//! Built-in default classification rules + central source of truth for safe defaults.
//!
//! This module now serves two important purposes:
//! 1. The "batteries included" default rules (constructed as Rust values for
//!    compile-time safety).
//! 2. The single point of truth for what a safe fallback classification looks
//!    like (`safe_default_classification()`, `DEFAULT_OWNER`, `DEFAULT_CATEGORY`).
//!
//! These are constructed directly as Rust values rather than parsed from an
//! embedded YAML string. That removes a fallible round-trip (and the `expect()`
//! it used to need) — the defaults are now correct by construction and checked
//! by the compiler, not at runtime.

use super::types::{Classification, MatchSpec, RiskLevel, Rule};

/// A small, conservative, opinionated rule set for personal tool inventories.
/// This is the "batteries included" starting point; user rules append on top.
pub(super) fn default_rules() -> Vec<Rule> {
    vec![
        // Any executable is at least a "binary" worth noting.
        Rule {
            name: "user-binaries".into(),
            description: Some("Compiled binaries the user has placed in their PATH".into()),
            r#match: MatchSpec {
                executable: Some(true),
                ..MatchSpec::default()
            },
            classify: Classification {
                risk: RiskLevel::Medium,
                category: "binary".into(),
                owner: "user".into(),
            },
        },
        // Executable scripts in common scripting languages are low risk.
        Rule {
            name: "user-executable-scripts".into(),
            description: Some("Common executable scripts in user bin directories".into()),
            r#match: MatchSpec {
                executable: Some(true),
                // Dotless form is the documented convention; the engine also
                // accepts a leading dot, so either style works in user rules.
                extensions: vec![
                    "sh".into(),
                    "bash".into(),
                    "zsh".into(),
                    "py".into(),
                    "pl".into(),
                    "rb".into(),
                ],
                ..MatchSpec::default()
            },
            classify: Classification {
                risk: RiskLevel::Low,
                category: "script".into(),
                owner: "user".into(),
            },
        },
        // Well-known dotfiles / config files.
        Rule {
            name: "dotfiles-and-configs".into(),
            description: None,
            r#match: MatchSpec {
                names: vec![
                    ".bashrc".into(),
                    ".zshrc".into(),
                    ".vimrc".into(),
                    "config.yaml".into(),
                    "config.yml".into(),
                ],
                ..MatchSpec::default()
            },
            classify: Classification {
                risk: RiskLevel::Low,
                category: "config".into(),
                owner: "user".into(),
            },
        },
        // Commands that can cause significant data loss.
        Rule {
            name: "high-risk-destructive".into(),
            description: Some("Commands that can cause significant data loss".into()),
            r#match: MatchSpec {
                names: vec![
                    "rm".into(),
                    "dd".into(),
                    "mkfs".into(),
                    "shred".into(),
                    "fdisk".into(),
                ],
                ..MatchSpec::default()
            },
            classify: Classification {
                risk: RiskLevel::High,
                category: "destructive".into(),
                owner: "user".into(),
            },
        },
    ]
}

// -----------------------------------------------------------------------------
// Central Source of Truth for Classification Defaults
// -----------------------------------------------------------------------------
//
// This section is the single, obvious place that defines:
// - What strings we use for common owners and categories
// - What a "safe default" classification looks like when no rule matches
//
// Why centralize this?
// - Eliminates magic strings duplicated across the crate (currently ~27 occurrences).
// - Makes it trivial to change the "unknown" policy in one place.
// - Gives future contributors one clear point of truth.
// - Supports the project's long-term goal of being easy to understand and maintain.

/// The default owner assigned when no rule provides a more specific one.
///
/// This constant exists so we have exactly one place that defines the string
/// "user". Changing the policy for unknown items only requires editing this
/// file.
pub const DEFAULT_OWNER: &str = "user";

/// The default category assigned when no rule provides a more specific one.
pub const DEFAULT_CATEGORY: &str = "unknown";

/// Returns the safe fallback classification used when no rule matches a file.
///
/// This is the "everything else" classification. It is deliberately conservative
/// (Low risk) so that unknown tools do not accidentally get flagged as dangerous.
/// All call sites should prefer `safe_default_classification()` over
/// constructing the struct manually.
pub(crate) fn safe_default_classification() -> Classification {
    Classification {
        risk: RiskLevel::Low,
        category: DEFAULT_CATEGORY.to_string(),
        owner: DEFAULT_OWNER.to_string(),
    }
}

//! Rule engine and classification for Bulwark.
//!
//! YAML-driven rules decide how discovered files are classified (risk / category
//! / owner). This module is now split into focused files for maintainability:
//!
//! - `types`     — the YAML-facing data shapes (`RiskLevel`, `Classification`, `MatchSpec`, `Rule`).
//! - `defaults`  — the built-in default rule set, built as Rust values (single source of truth for canned rules).
//! - `matching`  — pure, testable predicates that decide whether a file satisfies a `MatchSpec` (central point for matching logic).
//! - `engine`    — the `RuleEngine` that orchestrates loading + classification (last match wins).

mod defaults;
mod engine;
mod matching;
mod types;

pub use engine::RuleEngine;
pub use types::{Classification, MatchSpec, RiskLevel, Rule};

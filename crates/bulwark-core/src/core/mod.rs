//! Core domain modules for Bulwark.
//!
//! This module tree contains all the "real work":
//! - `model`: **strong central layer** — canonical re-exports of every important domain
//!   type (ScriptEntry, Language, Classification, RiskLevel, Config, ...). See model.rs.
//! - `config`: loading and validating user + default configuration
//! - `scanner`: read-only directory walking and file discovery
//! - `entry`: enrichment (language, header comments, sidecar .bulwark.yaml)
//! - `rules`: YAML rule parsing + matching engine (last-match-wins)
//! - `engine`: orchestration (scan → enrich → classify) producing ClassifiedEntry
//! - `report`: pure renderers (String) + thin print wrappers for table/JSON/Markdown
//!
//! Architecture note (core / app / tui separation):
//! - `core::*` is pure, dependency-free (except what config/engine need), UI-agnostic.
//! - `app` (sibling at crate root) re-exports the high-level services for consumers
//!   that want the "application layer" story.
//! - `tui` (feature-gated) consumes the model via `app` or `core` and owns only
//!   presentation and input.
//!
//! Each submodule is kept small and focused. When any file approaches the ~400 line
//! limit, it will be split before that limit is reached.

pub mod config;
pub mod engine;
pub mod entry;
/// Canonical domain model (re-exports of the primary types used everywhere).
///
/// This is the "strong core::model layer" requested for clarity and as a
/// single obvious home when someone asks "what are Bulwark's core data shapes?"
pub mod model;
pub mod report;
pub mod rules;
pub mod scanner;

use std::path::PathBuf;

use directories::ProjectDirs;

/// Return Bulwark's user config directory (e.g. `~/.config/bulwark` on Linux).
///
/// Single source of truth for the `ProjectDirs` qualifier triple so config and
/// rules loading can't drift apart. Returns `None` if no home directory can be
/// determined (rare; e.g. `$HOME` unset and no platform fallback).
pub(crate) fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "bulwark", "bulwark").map(|d| d.config_dir().to_path_buf())
}

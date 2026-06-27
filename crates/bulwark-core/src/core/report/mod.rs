//! Report rendering for Bulwark.
//!
//! This module (and its submodules) turns a fully-classified inventory (from
//! `core::model::ClassifiedEntry` / `core::engine`) into one of three output
//! formats:
//! - a colorized human-readable terminal table (dynamic widths, always aligned),
//! - pretty-printed JSON,
//! - a Markdown table.
//!
//! # Design Philosophy
//!
//! The same `&[ClassifiedEntry]` data is rendered three different ways from a
//! single source of truth. This is a deliberate and powerful pattern:
//!
//! - **One data model, many presentations.** The core types (`ClassifiedEntry`,
//!   `Classification`, etc.) do not know or care how they will be displayed.
//! - **Audience-specific formatting.** JSON is machine-friendly (lowercase
//!   tokens, no color). The terminal table is human-optimized (color, alignment,
//!   truncation that keeps filenames visible). Markdown is documentation-
//!   friendly (pipes escaped, simple table syntax).
//! - **Pure rendering where possible.** The heavy table logic was extracted
//!   into `render_human_table(...) -> String` precisely so a future TUI can
//!   reuse the exact same layout decisions without duplication.
//!
//! # TUI / GUI Readiness Note
//! The pure `render_*` functions mean the CLI, TUI, or another UI can reuse
//! current formatting without duplicating scan or classification logic.
//! stdout/stderr printing belongs to presentation crates, not `bulwark-core`.
//!
//! The public API surface is deliberately tiny and stable. All heavy lifting for
//! the human table lives in the focused `human` submodule so this file stays small.
//!
//! Design goals (Bulwark Architect):
//! - Deterministic output (callers pass already-sorted data).
//! - Zero new runtime dependencies for formatting (pure std + existing crates).
//! - UTF-8 safe truncation (char counts, never byte slicing).
//! - NO_COLOR + --color respected exactly as before.

mod format;
mod human;
mod json;
mod markdown;
mod workstate;

// Re-export the pure renderers and ColorChoice (used by CLI and library).
pub use human::{ColorChoice, render_human_table};
pub use json::render_json_classified;
pub use markdown::render_markdown_table_classified;
pub use workstate::{WORKSTATE_FEED_SCHEMA_VERSION, render_workstate_feed};

// Re-export pure formatting helpers (central point of truth for truncation & sizing).
pub use format::{human_size, md_escape, truncate_desc, truncate_path};

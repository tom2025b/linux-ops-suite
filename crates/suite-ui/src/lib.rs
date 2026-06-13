//! # suite-ui
//!
//! Shared terminal-UI **chrome** for the Linux Ops Suite — the look and feel
//! that RexOps and ScriptVault have in common, in one place:
//!
//! - a [`Theme`] with cyan/amber accents and a single `NO_COLOR` gate
//!   ([`ThemeChoice`], [`ColorChoice`]), plus [`Health`] and [`Severity`]
//!   status/risk styling;
//! - the consistent rounded [`pane`] (titled), [`pane_titled`] (styled title)
//!   and [`pane_blank`] (untitled) frames, plus centering helpers
//!   ([`centered_rect`], [`centered_fixed`]);
//! - common overlays — [`HelpSheet`], [`ConfirmModal`], [`Toast`], and the
//!   command-palette chrome [`PaletteFrame`];
//! - a persistent [`StatusBar`] job-status segment ([`JobState`]), with the
//!   shared [`Outcome`] glyph/style mapping every job-event widget renders through;
//! - a [`SeverityBadge`] risk tag (`[CRIT]`/`[HIGH]`) for a [`Severity`] level;
//! - an [`AttentionFlag`] "needs attention" marker (`⚠ 3 high` vs a clear `✓`);
//! - a [`SearchBar`] live-filter input affordance;
//! - a [`KeyHints`] footer strip of `key → label` shortcut hints;
//! - a [`FilterChips`] row of active-filter chips (`[t:ci ✕]`);
//! - a [`StatusStrip`] of `·`-joined state segments (`All · Auto · 312`);
//! - a [`HealthStrip`] of `glyph + label` health segments (`● bulwark  ◐ vault`);
//! - a [`Counted`] "N of M" count span (accented when the list is narrowed);
//! - an [`EmptyState`] centered placeholder for an empty region;
//! - a [`Freshness`] provenance stamp (`just now`, `2h ago`, stale-aware);
//! - Unicode-aware [`truncate_path`]/[`truncate_desc`] helpers (one `…`);
//! - shared keymap conventions ([`keys`]);
//! - a RAII [`Tui`] terminal scope guard (setup + panic-safe teardown +
//!   ordered post-exit stdout) every tool adopts;
//!
//! ## Scope: chrome, not logic
//!
//! Every visual component takes a [`Theme`], a borrowed data slice, and a
//! `Rect`, and draws into a `Frame`. None of them owns application state or
//! domain types. Command dispatch, filtering, and effects stay in the consuming
//! application — `suite-ui` draws the box; the app owns the behaviour. This is
//! what lets two otherwise-decoupled tools share presentation without coupling
//! their internals.
//!
//! ## The terminal guard
//!
//! [`Tui`] owns the terminal *lifecycle* (mechanism every tool repeats), not
//! application logic. Each tool drives its own event loop via [`Tui::terminal`].
//!
//! ## The `clap` feature
//!
//! Off by default. Enabling it derives `clap::ValueEnum` on [`ThemeChoice`] and
//! [`ColorChoice`] so a consumer can parse `--theme`/`--color` straight into
//! them. Consumers that don't use clap stay lean.

mod app;
mod attention_flag;
mod badge;
mod health_strip;
mod overlays;
mod status_bar;

/// The theme now lives in [`thomas_tui`]; re-exported here under the same path
/// so suite code keeps using `crate::theme::*` (and `suite_ui::Theme`)
/// unchanged. `Theme`, `Severity`, `Health`, and the choice enums are all the
/// general toolkit's — suite-ui just re-exposes them.
mod theme {
    pub use thomas_tui::{ColorChoice, Health, Severity, Theme, ThemeChoice};
}

pub use app::{Tui, TuiError, TuiOptions};
pub use attention_flag::AttentionFlag;
pub use badge::SeverityBadge;
pub use health_strip::{HealthStrip, HEALTH_SEP};
pub use overlays::{ConfirmModal, HelpSheet, PaletteFrame, PaletteItem, Toast, ToastKind};
pub use status_bar::{JobState, Outcome, StatusBar};
pub use theme::{ColorChoice, Health, Severity, Theme, ThemeChoice};
pub use thomas_tui::keys;
pub use thomas_tui::{
    centered_fixed, centered_rect, pane, pane_blank, pane_titled, truncate_desc, truncate_path,
    Counted, EmptyState, FilterChips, Freshness, KeyHints, SearchBar, StatusStrip, STATUS_SEP,
};

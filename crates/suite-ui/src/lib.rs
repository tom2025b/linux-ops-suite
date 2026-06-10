//! # suite-ui
//!
//! Shared terminal-UI **chrome** for the Linux Ops Suite — the look and feel
//! that RexOps and ScriptVault have in common, in one place:
//!
//! - a [`Theme`] with cyan/amber accents and a single `NO_COLOR` gate
//!   ([`ThemeChoice`], [`ColorChoice`]), plus [`Health`] status styling;
//! - the consistent rounded [`pane`] and centering helpers
//!   ([`centered_rect`], [`centered_fixed`]);
//! - common overlays — [`HelpSheet`], [`ConfirmModal`], [`Toast`], and the
//!   command-palette chrome [`PaletteFrame`];
//! - a persistent [`StatusBar`] job-status segment ([`JobState`]), with the
//!   shared [`Outcome`] glyph/style mapping every job-event widget renders through;
//! - a [`SearchBar`] live-filter input affordance;
//! - a [`KeyHints`] footer strip of `key → label` shortcut hints;
//! - a [`FilterChips`] row of active-filter chips (`[t:ci ✕]`);
//! - a [`StatusStrip`] of `·`-joined state segments (`All · Auto · 312`);
//! - a [`Counted`] "N of M" count span (accented when the list is narrowed);
//! - an [`EmptyState`] centered placeholder for an empty region;
//! - Unicode-aware [`truncate_path`]/[`truncate_desc`] helpers (one `…`);
//! - shared keymap conventions ([`keys`]).
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
//! ## The `clap` feature
//!
//! Off by default. Enabling it derives `clap::ValueEnum` on [`ThemeChoice`] and
//! [`ColorChoice`] so a consumer can parse `--theme`/`--color` straight into
//! them. Consumers that don't use clap stay lean.

mod counted;
mod empty_state;
mod filter_chips;
mod key_hints;
pub mod keys;
mod overlays;
mod search_bar;
mod status_bar;
mod status_strip;
mod text;
mod theme;
mod widgets;

pub use counted::Counted;
pub use empty_state::EmptyState;
pub use filter_chips::FilterChips;
pub use key_hints::KeyHints;
pub use overlays::{ConfirmModal, HelpSheet, PaletteFrame, PaletteItem, Toast, ToastKind};
pub use search_bar::SearchBar;
pub use status_bar::{JobState, Outcome, StatusBar};
pub use status_strip::{StatusStrip, STATUS_SEP};
pub use text::{truncate_desc, truncate_path};
pub use theme::{ColorChoice, Health, Theme, ThemeChoice};
pub use widgets::{centered_fixed, centered_rect, pane, pane_titled};

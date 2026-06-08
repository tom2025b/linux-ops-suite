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
//! - a persistent [`StatusBar`] job-status segment ([`JobState`]);
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

pub mod keys;
mod overlays;
mod status_bar;
mod theme;
mod widgets;

pub use overlays::{ConfirmModal, HelpSheet, PaletteFrame, PaletteItem, Toast, ToastKind};
pub use status_bar::{JobState, StatusBar};
pub use theme::{ColorChoice, Health, Theme, ThemeChoice};
pub use widgets::{centered_fixed, centered_rect, pane};

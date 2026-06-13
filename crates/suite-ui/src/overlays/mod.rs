//! Modal overlays, drawn over the current frame.
//!
//! Every overlay is a plain data struct that borrows what it needs and draws
//! into a `Rect` given a [`Theme`](crate::theme::Theme). None owns application
//! state — the app keeps its state and passes in only the values to display.
//! Each `render` clears its area first and frames its content in the accent
//! border.
//!
//! The generic ones ([`ConfirmModal`], [`HelpSheet`], the command-palette
//! [`PaletteFrame`]/[`PaletteItem`]) live in [`thomas_tui`] and are re-exported
//! here. [`Toast`], which speaks the suite's [`Outcome`](crate::Outcome)
//! vocabulary, stays in suite-ui.

mod toast;

pub use thomas_tui::{ConfirmModal, HelpSheet, PaletteFrame, PaletteItem};
pub use toast::{Toast, ToastKind};

//! Common modal overlays, drawn over the current frame.
//!
//! Every overlay here is a plain data struct that borrows what it needs and
//! draws into a `Rect` given a [`Theme`](crate::theme::Theme). None of them owns
//! or reaches into application state — the app keeps its state and passes in
//! only the values to display. That is what makes them reusable across tools.
//!
//! Each `render` clears its area first (so it sits cleanly over whatever is
//! beneath) and frames its content in the shared accent border.

mod confirm;
mod help;
mod palette;
mod toast;

pub use confirm::ConfirmModal;
pub use help::HelpSheet;
pub use palette::{PaletteFrame, PaletteItem};
pub use toast::{Toast, ToastKind};

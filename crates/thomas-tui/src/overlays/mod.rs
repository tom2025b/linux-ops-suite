//! Generic modal overlays, drawn over the current frame.
//!
//! Each overlay is a plain data struct that borrows what it needs and draws into
//! a `Rect` given a [`Theme`](crate::Theme). None owns application state — the
//! app keeps its state and passes in only the values to display. Each `render`
//! clears its area first (so it sits cleanly over whatever is beneath) and
//! frames its content in the shared accent border.

mod confirm;
mod help;
mod palette;

pub use confirm::ConfirmModal;
pub use help::HelpSheet;
pub use palette::{PaletteFrame, PaletteItem};

//! Centering geometry: place a `Rect` in the middle of another, either as a
//! percentage of the parent or at a fixed size. The basis for any centered
//! overlay or modal. Pure `ratatui` layout math — no `Theme`, no rendering.

use ratatui::layout::{Constraint, Layout, Rect};

/// A `Rect` centered as a percentage of `area` (e.g. 60% wide, 40% tall).
/// The basis for percentage-sized overlays.
pub fn centered_rect(pct_w: u16, pct_h: u16, area: Rect) -> Rect {
    let [_, mid_v, _] = Layout::vertical([
        Constraint::Percentage((100 - pct_h) / 2),
        Constraint::Percentage(pct_h),
        Constraint::Percentage((100 - pct_h) / 2),
    ])
    .areas(area);
    let [_, mid, _] = Layout::horizontal([
        Constraint::Percentage((100 - pct_w) / 2),
        Constraint::Percentage(pct_w),
        Constraint::Percentage((100 - pct_w) / 2),
    ])
    .areas(mid_v);
    mid
}

/// A fixed-size `Rect` centered in `area`, clamped so it always fits inside the
/// parent (leaving room for a one-cell border on each side). For overlays that
/// want an exact size rather than a percentage.
pub fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_fixed_clamps_to_parent() {
        let parent = Rect::new(0, 0, 20, 10);
        // Asking for more than fits is clamped to parent minus a 1-cell border.
        let r = centered_fixed(100, 100, parent);
        assert_eq!(r.width, 18);
        assert_eq!(r.height, 8);
        // And it stays inside the parent.
        assert!(r.x >= parent.x && r.right() <= parent.right());
        assert!(r.y >= parent.y && r.bottom() <= parent.bottom());
    }

    #[test]
    fn centered_fixed_centers_a_small_rect() {
        let parent = Rect::new(0, 0, 40, 20);
        let r = centered_fixed(10, 4, parent);
        assert_eq!((r.width, r.height), (10, 4));
        assert_eq!(r.x, 15); // (40 - 10) / 2
        assert_eq!(r.y, 8); //  (20 - 4) / 2
    }

    #[test]
    fn centered_rect_is_a_centered_fraction() {
        let parent = Rect::new(0, 0, 100, 100);
        let r = centered_rect(50, 50, parent);
        assert_eq!((r.width, r.height), (50, 50));
        assert_eq!((r.x, r.y), (25, 25));
    }
}

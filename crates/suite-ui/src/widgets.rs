//! Shared widget chrome: the consistent rounded pane and centering helpers.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding};

use crate::theme::Theme;

/// The consistent rounded, padded pane every screen frames content with: a
/// rounded border in the dim neutral, with the title painted in the accent.
///
/// For a title that needs embedded styling — a coloured count, say — use
/// [`pane_titled`] and supply the whole title [`Line`] (this is just that with
/// the title wrapped in [`Theme::title`](crate::Theme::title)).
pub fn pane(title: &str, theme: Theme) -> Block<'static> {
    pane_titled(
        Line::from(Span::styled(format!(" {title} "), theme.title())),
        theme,
    )
}

/// The same chrome as [`pane`] — rounded border, dim border style, one-column
/// horizontal padding — but the caller supplies the whole title [`Line`], so it
/// can embed styled spans the plain-string [`pane`] can't.
///
/// This is what a "results (N of M)" title wants: build the line with a
/// [`Counted`](crate::Counted) span for the count and the rest in
/// [`Theme::title`](crate::Theme::title), and the pane no longer has to be
/// reproduced by hand to carry it.
///
/// ```no_run
/// # use suite_ui::{pane_titled, Counted, Theme};
/// # use ratatui::text::{Line, Span};
/// # let theme = Theme::with_color(true);
/// let title = Line::from(vec![
///     Span::styled(" results (", theme.title()),
///     Counted { shown: 48, total: 312 }.span(theme),
///     Span::styled(") ", theme.title()),
/// ]);
/// let block = pane_titled(title, theme);
/// ```
pub fn pane_titled(title: Line<'static>, theme: Theme) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.dim())
        .padding(Padding::horizontal(1))
        .title(title)
}

/// The same rounded, dim, one-column-padded frame as [`pane`] but **without a
/// title** — for a region that carries its own heading in the body (a header
/// strip that prints its name and a status badge inline, say), so the border
/// shouldn't repeat it.
///
/// This exists so those untitled frames stop reaching for a bare
/// `Block::bordered()`/`Block::default().borders(..)` by hand, which is how
/// square corners and off-palette borders creep in: route them through here and
/// an untitled frame matches every titled [`pane`] exactly.
pub fn pane_blank(theme: Theme) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.dim())
        .padding(Padding::horizontal(1))
}

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

    #[test]
    fn pane_renders_into_an_area_with_a_titled_border() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // Render `pane` into a buffer and assert the title text actually shows in
        // the top border — proving `pane` (built on `pane_titled`) frames + titles.
        let mut term = Terminal::new(TestBackend::new(20, 4)).unwrap();
        term.draw(|f| f.render_widget(pane("adapters", Theme::with_color(true)), f.area()))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let top: String = (0..buf.area.width)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert!(
            top.contains("adapters"),
            "pane draws its title in the border"
        );
        assert!(top.contains('╮'), "pane uses a rounded border");
    }

    #[test]
    fn pane_blank_is_a_rounded_border_with_no_title() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // pane_blank frames a region the same way as `pane` (rounded corner in
        // the top row) but writes no title text into the border.
        let mut term = Terminal::new(TestBackend::new(20, 4)).unwrap();
        term.draw(|f| f.render_widget(pane_blank(Theme::with_color(true)), f.area()))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let top: String = (0..buf.area.width)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert!(top.contains('╮'), "pane_blank uses a rounded border");
        // The top row is border only — the corners/edges, no letters.
        assert!(
            !top.chars().any(|c| c.is_alphabetic()),
            "pane_blank draws no title text: {top:?}"
        );
    }
}

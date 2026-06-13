//! Widget chrome: the consistent rounded pane.
//!
//! One rounded, dim-bordered, one-column-padded frame so every pane matches and
//! square corners / off-palette borders don't creep in by hand. For centering a
//! `Rect` inside one, see [`centered_rect`](crate::centered_rect) /
//! [`centered_fixed`](crate::centered_fixed).

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
/// # use thomas_tui::{pane_titled, Counted, Theme};
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

#[cfg(test)]
mod tests {
    use super::*;

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

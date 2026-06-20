//! `Themed<W>`: bind a stateless chrome widget to a `Theme` so it implements
//! `ratatui::Widget` (render-by-reference). This is the *opt-in* ecosystem surface:
//! the existing inherent `.line(theme)` / `.render(frame, area, theme)` methods stay.
//!
//! The fixed `Widget::render(self, Rect, &mut Buffer)` signature has no `theme`
//! parameter, so the theme rides inside `Themed`. The wrapper borrows and owns no
//! application state — the contract is unchanged.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

use crate::theme::Theme;
use crate::{FilterChips, Freshness, KeyHints, SearchBar, StatusStrip};

/// A chrome widget bound to a theme, so it can render as a `ratatui::Widget`.
/// Construct via [`Themable::themed`].
pub struct Themed<W> {
    /// The wrapped chrome widget.
    pub widget: W,
    /// The theme its `line()` is rendered through.
    pub theme: Theme,
}

/// Bind any line-producing chrome widget to a theme with `.themed(theme)`, so
/// `&Themed<Self>` can be handed to `frame.render_widget` and nested inside
/// ecosystem `Widget` containers.
pub trait Themable: Sized {
    /// Wrap `self` with `theme` so `&Themed<Self>` is a `ratatui::Widget`.
    fn themed(self, theme: Theme) -> Themed<Self> {
        Themed {
            widget: self,
            theme,
        }
    }
}

impl<W> Themable for W {}

/// Render a theme-bound `Line` into `area` as a left-origin paragraph — the shared
/// body of every one-line `Widget for &Themed<W>` impl below.
fn render_line(line: Line<'static>, area: Rect, buf: &mut Buffer) {
    Paragraph::new(line).render(area, buf);
}

impl Widget for &Themed<SearchBar<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<StatusStrip<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<FilterChips<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<KeyHints<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<Freshness> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Flatten row 0 of a buffer to a string.
    fn row0(buf: &Buffer) -> String {
        (0..buf.area.width)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn themed_searchbar_renders_via_the_widget_trait() {
        let theme = Theme::with_color(true);
        let bar = SearchBar {
            query: "bul",
            placeholder: "ph",
            match_count: Some(2),
        };
        let mut term = Terminal::new(TestBackend::new(30, 1)).unwrap();
        term.draw(|f| f.render_widget(&bar.themed(theme), f.area()))
            .unwrap();
        let got = row0(term.backend().buffer());
        assert!(got.starts_with("/ "), "prompt glyph rendered: {got:?}");
        assert!(got.contains("bul"), "query rendered: {got:?}");
    }

    #[test]
    fn themed_matches_inherent_render() {
        // The Widget impl must produce the same buffer as calling .render() directly.
        let theme = Theme::with_color(false);
        let bar = SearchBar {
            query: "q",
            placeholder: "ph",
            match_count: None,
        };

        let mut a = Terminal::new(TestBackend::new(20, 1)).unwrap();
        a.draw(|f| f.render_widget(&bar.themed(theme), f.area()))
            .unwrap();

        let bar2 = SearchBar {
            query: "q",
            placeholder: "ph",
            match_count: None,
        };
        let mut b = Terminal::new(TestBackend::new(20, 1)).unwrap();
        b.draw(|f| bar2.render(f, f.area(), theme)).unwrap();

        assert_eq!(a.backend().buffer(), b.backend().buffer());
    }
}

//! SearchBar: a one-line live-filter input affordance shared across the suite.
//!
//! The visible counterpart to a screen's filter string: a prompt glyph, the
//! current query (or a dim placeholder when empty), and an optional match count.
//! Like [`StatusBar`](crate::StatusBar) and [`Toast`](crate::Toast) it draws a
//! single line and owns no state — the consumer keeps the query string and the
//! filtered results, and hands this widget the values to show.
//!
//! The widget never captures input. Key handling (appending characters,
//! backspace, clear-on-esc) stays in the application; this only renders what the
//! current filter looks like.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// A single-line search/filter input. Holds only borrowed display values; it
/// owns no application state and reads nothing from the environment.
///
/// ```no_run
/// # use suite_ui::{SearchBar, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, row: Rect, theme: Theme) {
/// SearchBar {
///     query: "bul",
///     placeholder: "type to filter adapters",
///     match_count: Some(1),
/// }
/// .render(frame, row, theme);
/// # }
/// ```
pub struct SearchBar<'a> {
    /// The current filter text (what the user has typed so far).
    pub query: &'a str,
    /// Hint shown dim when `query` is empty, so an empty bar still explains
    /// itself (e.g. "type to filter adapters").
    pub placeholder: &'a str,
    /// How many items the current query matches. Rendered as a dim
    /// "(N matches)" suffix when `Some` and the query is non-empty; pass `None`
    /// to omit the count.
    pub match_count: Option<usize>,
}

impl SearchBar<'_> {
    /// The composed [`Line`], for a caller that wants to fold the bar into a row
    /// it lays out itself. The prompt glyph is always shown in the accent; the
    /// body is either the dim placeholder (empty query) or the query text
    /// followed by an optional dim match count.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        let mut spans = vec![Span::styled("/ ", theme.prompt())];

        if self.query.is_empty() {
            spans.push(Span::styled(self.placeholder.to_string(), theme.dim()));
        } else {
            spans.push(Span::styled(self.query.to_string(), theme.match_text()));
            if let Some(n) = self.match_count {
                // Pluralise so a single match doesn't read "(1 matches)".
                let suffix = if n == 1 {
                    "   (1 match)".to_string()
                } else {
                    format!("   ({n} matches)")
                };
                spans.push(Span::styled(suffix, theme.dim()));
            }
        }

        Line::from(spans)
    }

    /// Draw the search bar into `area` (typically a single-row strip above a list).
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme) {
        frame.render_widget(Paragraph::new(self.line(theme)), area);
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn spans(
        query: &str,
        placeholder: &str,
        count: Option<usize>,
        theme: Theme,
    ) -> Vec<Span<'static>> {
        SearchBar {
            query,
            placeholder,
            match_count: count,
        }
        .line(theme)
        .spans
    }

    fn text(query: &str, placeholder: &str, count: Option<usize>, theme: Theme) -> String {
        spans(query, placeholder, count, theme)
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn empty_query_shows_the_placeholder_not_a_count() {
        let lit = Theme::with_color(true);
        let t = text("", "type to filter", Some(5), lit);
        assert!(t.contains("type to filter"));
        assert!(!t.contains("match"), "no count when the query is empty");
    }

    #[test]
    fn non_empty_query_shows_the_query_and_pluralised_count() {
        let lit = Theme::with_color(true);
        assert!(text("bul", "ph", Some(3), lit).contains("bul"));
        assert!(text("bul", "ph", Some(3), lit).contains("(3 matches)"));
        // Singular is special-cased so it doesn't read "(1 matches)".
        assert!(text("bul", "ph", Some(1), lit).contains("(1 match)"));
        assert!(!text("bul", "ph", Some(1), lit).contains("matches"));
        // No count given → no suffix at all.
        assert!(!text("bul", "ph", None, lit).contains('('));
    }

    #[test]
    fn the_prompt_glyph_is_always_present() {
        let lit = Theme::with_color(true);
        assert!(text("", "ph", None, lit).starts_with("/ "));
        assert!(text("q", "ph", Some(2), lit).starts_with("/ "));
    }

    #[test]
    fn colour_on_accents_the_prompt_and_query_off_drops_all_hue() {
        let lit = Theme::with_color(true);
        // Prompt + query both carry the accent when colour is on.
        let lit_spans = spans("q", "ph", Some(2), lit);
        assert!(lit_spans[0].style.fg.is_some(), "prompt accented");
        assert!(lit_spans[1].style.fg.is_some(), "query accented");

        let dark = Theme::with_color(false);
        for span in spans("q", "ph", Some(2), dark) {
            assert_eq!(span.style.fg, None, "no fg under NO_COLOR");
        }
    }
}

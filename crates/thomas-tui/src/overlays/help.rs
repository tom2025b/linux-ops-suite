//! HelpSheet: a centered modal listing key → description rows.

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::centered_rect;
use crate::theme::Theme;

/// A keybinding help overlay. The caller owns the rows — keeping them next to
/// the app's real key handling, so the help can't drift from the bindings.
///
/// ```no_run
/// # use thomas_tui::{HelpSheet, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, theme: Theme) {
/// let rows = [
///     ("↑ / ↓ · j / k", "move the selection"),
///     ("Enter", "activate"),
///     ("?", "toggle this help"),
/// ];
/// HelpSheet { title: "Keybindings", rows: &rows }.render(frame, frame.area(), theme);
/// # }
/// ```
pub struct HelpSheet<'a> {
    /// The modal title (rendered centered in the border).
    pub title: &'a str,
    /// `(key, description)` pairs, one per row, in display order.
    pub rows: &'a [(&'a str, &'a str)],
}

impl HelpSheet<'_> {
    /// Draw the help sheet over `area`. Sized as a comfortable share of the
    /// area; for the table to fit without clipping, give it most of the screen.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme) {
        let area = centered_rect(64, 80, area);

        // Right-align the key column to the widest key so descriptions line up.
        let key_w = self
            .rows
            .iter()
            .map(|(k, _)| k.chars().count())
            .max()
            .unwrap_or(0);

        let lines: Vec<Line> = self
            .rows
            .iter()
            .map(|(key, description)| {
                Line::from(vec![
                    Span::styled(format!("{key:>key_w$}  "), theme.prompt()),
                    Span::raw((*description).to_string()),
                ])
            })
            .collect();

        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.accent_bar())
            .padding(Padding::uniform(1))
            .title(format!(" {} ", self.title))
            .title_alignment(Alignment::Center);

        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Render the help sheet into a generous test backend (it's percentage-sized,
    /// so give it room) and flatten the buffer to one string.
    fn render(title: &str, rows: &[(&str, &str)]) -> String {
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let theme = Theme::with_color(false);
        term.draw(|f| {
            HelpSheet { title, rows }.render(f, f.area(), theme);
        })
        .unwrap();
        term.backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn shows_the_title_and_every_key_and_description() {
        let rows = [
            ("Enter", "activate the selection"),
            ("?", "toggle this help"),
            ("q", "quit"),
        ];
        let out = render("Keybindings", &rows);
        assert!(out.contains("Keybindings"), "the title is in the border");
        for (key, desc) in rows {
            assert!(out.contains(key), "key {key:?} is shown");
            assert!(out.contains(desc), "description {desc:?} is shown");
        }
    }

    #[test]
    fn the_key_column_is_right_aligned_to_the_widest_key() {
        // A short key alongside a long one must be right-padded so the descriptions
        // line up. We assert the narrow key gets leading spaces before it (i.e. the
        // pattern "  q" appears, the short key pushed right under the wide column).
        let rows = [("a-very-wide-key", "wide"), ("q", "narrow")];
        let out = render("T", &rows);
        assert!(
            out.contains("a-very-wide-key"),
            "the wide key sets the column"
        );
        assert!(
            out.contains("  q"),
            "the narrow key is right-aligned under the wide column:\n{out}"
        );
    }

    #[test]
    fn handles_an_empty_row_set_without_panicking() {
        // The `key_w` max() defaults to 0 on no rows — must not panic on the
        // `{key:>0}` format or the empty table.
        let out = render("Empty", &[]);
        assert!(out.contains("Empty"), "still frames with the title");
    }
}

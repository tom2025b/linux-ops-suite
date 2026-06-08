//! HelpSheet: a centered modal listing key → description rows.

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;
use crate::widgets::centered_rect;

/// A keybinding help overlay. The caller owns the rows — keeping them next to
/// the app's real key handling, so the help can't drift from the bindings.
///
/// ```no_run
/// # use suite_ui::{HelpSheet, Theme};
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

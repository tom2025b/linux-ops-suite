//! ConfirmModal: a small yes/no prompt for a pending destructive action.

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;
use crate::widgets::centered_fixed;

/// A confirmation modal for an action the user must approve before it runs
/// (delete a file, mutate state, …). The message is drawn in the attention
/// style so a pending destructive action is impossible to miss.
///
/// This draws the prompt only — the app owns the y/n key handling and decides
/// what to do on each answer.
///
/// ```no_run
/// # use suite_ui::{ConfirmModal, Theme};
/// # use ratatui::Frame;
/// # fn draw(frame: &mut Frame, theme: Theme) {
/// ConfirmModal { title: "Delete file", message: "Remove backup.sh?" }
///     .render(frame, frame.area(), theme);
/// # }
/// ```
pub struct ConfirmModal<'a> {
    /// The modal title (e.g. the action being confirmed).
    pub title: &'a str,
    /// The question shown to the user.
    pub message: &'a str,
}

impl ConfirmModal<'_> {
    /// Draw the confirm modal centered over `area`. Sized to the message width
    /// (clamped to the area), tall enough for the message and the y/n footer.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme) {
        // Width: fit the longer of the message and the footer, plus border and
        // padding; height is fixed at message + blank + footer inside the frame.
        let footer = "y: yes  ·  n / Esc: no";
        let inner = self.message.chars().count().max(footer.chars().count());
        let width = (inner as u16).saturating_add(4); // 2 border + 2 h-padding
        let modal = centered_fixed(width, 5, area);

        let lines: Vec<Line> = vec![
            Line::from(Span::styled(self.message.to_string(), theme.confirm())),
            Line::from(Span::raw("")),
            Line::from(Span::styled(footer, theme.dim())),
        ];

        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.accent_bar())
            .padding(Padding::horizontal(1))
            .title(format!(" {} ", self.title))
            .title_alignment(Alignment::Center);

        frame.render_widget(Clear, modal);
        frame.render_widget(Paragraph::new(Text::from(lines)).block(block), modal);
    }
}

//! ConfirmModal: a small yes/no prompt for a pending destructive action.

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::centered_fixed;
use crate::theme::Theme;

/// A confirmation modal for an action the user must approve before it runs
/// (delete a file, mutate state, …). The message is drawn in the attention
/// style so a pending destructive action is impossible to miss.
///
/// This draws the prompt only — the app owns the y/n key handling and decides
/// what to do on each answer.
///
/// ```no_run
/// # use thomas_tui::{ConfirmModal, Theme};
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
        // Width: fit the widest of the message, the footer, and the title, plus
        // border and padding; height is fixed at message + blank + footer inside
        // the frame. The title is measured with its two framing spaces (` title `)
        // so a title longer than the body isn't clipped by the centered border.
        let footer = "y: yes  ·  n / Esc: no";
        let inner = self
            .message
            .chars()
            .count()
            .max(footer.chars().count())
            .max(self.title.chars().count() + 2);
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

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Render the modal into a `width`×`height` test backend and return the whole
    /// buffer flattened to one string (newline-free), for substring assertions on
    /// what actually reached the cells.
    fn render(width: u16, height: u16, title: &str, message: &str) -> String {
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        let theme = Theme::with_color(false);
        term.draw(|f| {
            ConfirmModal { title, message }.render(f, f.area(), theme);
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
    fn renders_the_message_and_the_yn_footer() {
        let out = render(60, 12, "Delete file", "Remove backup.sh?");
        assert!(out.contains("Remove backup.sh?"), "the message is shown");
        assert!(out.contains("y: yes"), "the yes/no footer is shown");
        assert!(out.contains("Esc: no"));
    }

    #[test]
    fn a_long_title_is_not_clipped_by_the_border() {
        // The bug this guards: width was sized to message.max(footer) and ignored
        // the title, so a title longer than the body was chopped mid-word by the
        // centered border. The width must now grow to fit the title.
        let title = "A Very Long Confirmation Title That Far Exceeds The Message";
        let out = render(120, 12, title, "ok?");
        assert!(
            out.contains(title),
            "the full title must fit; got clipped output:\n{out}"
        );
    }

    #[test]
    fn the_message_drives_the_width_when_it_is_the_widest() {
        // Symmetric guard: a message wider than title+footer still fits in full.
        let message = "This particular confirmation message is the widest element here";
        let out = render(120, 12, "Del", message);
        assert!(out.contains(message), "the full message must fit:\n{out}");
    }
}

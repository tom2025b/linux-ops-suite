//! EmptyState: a centered "nothing to show here" placeholder for an empty region.
//!
//! One shared, consistently-styled way to say a pane has no content — replacing
//! the ad-hoc "No items found" / "(no match)" / "no adapters probed" strings each
//! screen invents. The message reads as a calm, dim centered note (not an error),
//! with an optional dimmer hint below it telling the user what to do about it
//! ("Press Esc to clear the filter.").
//!
//! Draws **text only** — no border, no `Clear` — so it sits inside a pane the
//! caller has already framed, the same way the body content would. Like the rest
//! of the crate it owns no state and routes its styling through the [`Theme`].

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// A centered empty-region placeholder. Holds only borrowed display strings; it
/// owns no application state and reads nothing from the environment.
///
/// ```no_run
/// # use suite_ui::{EmptyState, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, body: Rect, theme: Theme) {
/// EmptyState {
///     message: "No items match the current filter.",
///     hint: Some("Press Esc to clear the filter."),
/// }
/// .render(frame, body, theme);
/// # }
/// ```
pub struct EmptyState<'a> {
    /// The main line, e.g. "No items found." Shown dim + bold so it reads as a
    /// deliberate placeholder rather than missing content.
    pub message: &'a str,
    /// An optional second line under the message, dimmer, telling the user what
    /// to do (clear a filter, run a scan). `None` shows the message alone.
    pub hint: Option<&'a str>,
}

impl EmptyState<'_> {
    /// The composed lines (message, then the optional hint) — pure, so the
    /// content is unit-testable without a terminal. The [`render`](Self::render)
    /// method centers these in the target area.
    fn lines(&self, theme: Theme) -> Vec<Line<'static>> {
        // Message: dim + bold. Bold lifts it just clear of ordinary dim chrome
        // (and survives NO_COLOR, where it stays the only emphasis).
        let mut lines = vec![Line::from(Span::styled(
            self.message.to_string(),
            theme.dim().add_modifier(ratatui::style::Modifier::BOLD),
        ))];
        if let Some(hint) = self.hint {
            // A blank spacer line, then the hint plain-dim (a notch quieter than
            // the bold message).
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(hint.to_string(), theme.dim())));
        }
        lines
    }

    /// Draw the placeholder centered (horizontally and vertically) in `area`.
    /// Text only — it assumes the caller already framed the region.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme) {
        let lines = self.lines(theme);
        // Vertically center: a centered band exactly as tall as the content,
        // clamped to the area, with the rest as breathing room above and below.
        let height = (lines.len() as u16).min(area.height);
        let [_, band, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), band);
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    fn lines_of(message: &str, hint: Option<&str>, theme: Theme) -> Vec<Line<'static>> {
        EmptyState { message, hint }.lines(theme)
    }

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn message_is_always_present() {
        let lit = Theme::with_color(true);
        let lines = lines_of("No items found.", None, lit);
        assert_eq!(lines.len(), 1, "message-only is a single line");
        assert_eq!(text(&lines[0]), "No items found.");
    }

    #[test]
    fn hint_is_shown_only_when_some() {
        let lit = Theme::with_color(true);
        // With a hint: message, a blank spacer, then the hint.
        let lines = lines_of("Nothing here.", Some("Press r to rescan."), lit);
        assert_eq!(lines.len(), 3);
        assert_eq!(text(&lines[0]), "Nothing here.");
        assert_eq!(text(&lines[1]), "");
        assert_eq!(text(&lines[2]), "Press r to rescan.");
        // Without one: just the message.
        assert_eq!(lines_of("Nothing here.", None, lit).len(), 1);
    }

    #[test]
    fn message_is_bold_and_the_hint_is_plain_dim() {
        let lit = Theme::with_color(true);
        let lines = lines_of("msg", Some("hint"), lit);
        // The message carries bold on top of dim.
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        // The hint is dim but not bold (a notch quieter).
        assert!(lines[2].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(!lines[2].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn no_color_keeps_it_colourless_but_still_emphasised() {
        let dark = Theme::with_color(false);
        let lines = lines_of("msg", Some("hint"), dark);
        for line in &lines {
            for span in &line.spans {
                assert_eq!(span.style.fg, None, "empty state never carries a hue");
            }
        }
        // The message stays bold so it's still the emphasised line without colour.
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }
}

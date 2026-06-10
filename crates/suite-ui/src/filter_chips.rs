//! FilterChips: a one-line row of active-filter chips (`[t:ci ✕] [lang:bash ✕]`).
//!
//! The visible counterpart to a screen's structured filters: each active filter
//! shows as a bracketed chip with a `✕` remove marker, so the user can see at a
//! glance what is narrowing the list and that each one is dismissable. Like the
//! other status-line widgets ([`SearchBar`](crate::SearchBar),
//! [`StatusBar`](crate::StatusBar)) it draws a single line, owns no state, and
//! borrows the labels the consumer passes in.
//!
//! The widget renders the chips' *look* — the `[label ✕]` framing and the chip
//! accent — but knows nothing about WHERE the row sits or HOW a chip is removed:
//! the consuming app lays out the row (e.g. on a pane's bottom border) and owns
//! the key handling that pops a filter. That is what lets two tools show the same
//! chip row from one source without sharing their filter models.

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// A single-line active-filter chip row. Holds only the borrowed labels; it owns
/// no application state and reads nothing from the environment.
///
/// ```no_run
/// # use suite_ui::{FilterChips, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, row: Rect, theme: Theme) {
/// let labels = ["t:ci", "lang:bash"];
/// FilterChips { labels: &labels }.render(frame, row, theme);
/// # }
/// ```
pub struct FilterChips<'a> {
    /// The active-filter labels in display order, e.g. `["t:ci", "lang:bash"]`.
    /// Each is framed as `[label ✕]`. An empty slice yields an empty line.
    pub labels: &'a [&'a str],
}

impl FilterChips<'_> {
    /// Frame one label as it is displayed: `[t:ci ✕]`. Pure, for tests and reuse.
    pub fn chip_text(label: &str) -> String {
        format!("[{label} ✕]")
    }

    /// The composed [`Line`], for a caller that folds the chip row into a region
    /// it lays out itself. Leads with a space so the chips don't butt against a
    /// pane border, then each chip in the cyan filter accent, space-separated. An
    /// empty `labels` slice yields a line with no chips.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        if self.labels.is_empty() {
            return Line::from(spans);
        }
        spans.push(Span::raw(" "));
        for label in self.labels {
            // The cyan filter accent — `match_label` keeps the NO_COLOR gate (it
            // drops to dim when colour is off), so the chips stay legible without
            // hue and never leak a foreground colour under NO_COLOR.
            spans.push(Span::styled(Self::chip_text(label), theme.match_label(Color::Cyan)));
            spans.push(Span::raw(" "));
        }
        Line::from(spans)
    }

    /// Draw the chip row into `area` (typically a single-row strip, e.g. a pane's
    /// bottom border). Drawing nothing when there are no labels is the consumer's
    /// call — this renders whatever `line` produces.
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

    fn spans(labels: &[&str], theme: Theme) -> Vec<Span<'static>> {
        FilterChips { labels }.line(theme).spans
    }

    fn text(labels: &[&str], theme: Theme) -> String {
        spans(labels, theme).iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn chip_text_frames_label_with_remove_marker() {
        assert_eq!(FilterChips::chip_text("t:ci"), "[t:ci ✕]");
        assert_eq!(FilterChips::chip_text("lang:bash"), "[lang:bash ✕]");
    }

    #[test]
    fn empty_labels_yield_an_empty_line() {
        assert!(spans(&[], Theme::with_color(true)).is_empty());
    }

    #[test]
    fn every_label_is_framed_and_present() {
        let lit = Theme::with_color(true);
        let t = text(&["t:ci", "lang:bash"], lit);
        assert!(t.contains("[t:ci ✕]"));
        assert!(t.contains("[lang:bash ✕]"));
    }

    #[test]
    fn chips_are_accented_when_colour_is_on() {
        let lit = Theme::with_color(true);
        // The chip span (index 1, after the leading space) carries the cyan accent.
        let s = spans(&["t:ci"], lit);
        assert_eq!(s[1].style.fg, Some(Color::Cyan));
    }

    #[test]
    fn no_color_drops_hue_but_keeps_the_chip_legible() {
        let dark = Theme::with_color(false);
        for span in spans(&["t:ci", "lang:bash"], dark) {
            assert_eq!(span.style.fg, None, "no fg under NO_COLOR");
        }
        // The chip text (brackets + ✕ marker) still carries the meaning without hue.
        assert!(text(&["t:ci"], dark).contains("[t:ci ✕]"));
    }
}

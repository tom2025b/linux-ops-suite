//! KeyHints: a one-line strip of `key → label` shortcut hints for a footer.
//!
//! The inline counterpart to the [`HelpSheet`](crate::HelpSheet) popup: where the
//! help sheet lists every binding in a modal, this renders the handful most
//! relevant to the current screen on a single line, always visible at the bottom.
//! Both take the same `(key, label)` pairs and paint the key with the same accent
//! ([`Theme::prompt`](crate::Theme)), so the inline hints and the popup can't
//! drift apart in look or wording.
//!
//! Like the other status-line widgets it draws one line, owns no state, and
//! borrows the pairs the consumer passes in. The key glyph is accented/bold so it
//! reads as a key; the label is dim, and `•` separators sit between pairs — the
//! visible key/label distinction a flat hint string lacks.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// A single-line keyboard-shortcut hint strip. Holds only the borrowed pairs; it
/// owns no application state and reads nothing from the environment.
///
/// ```no_run
/// # use suite_ui::{KeyHints, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, footer: Rect, theme: Theme) {
/// let hints = [("q", "quit"), ("^P", "palette"), ("?", "help")];
/// KeyHints { hints: &hints }.render(frame, footer, theme);
/// # }
/// ```
pub struct KeyHints<'a> {
    /// `(key, label)` pairs in display order, e.g. `("^P", "palette")`. The key is
    /// painted in the accent; the label dim.
    pub hints: &'a [(&'a str, &'a str)],
}

impl KeyHints<'_> {
    /// The composed [`Line`], for a caller that folds the hints into a footer row
    /// it lays out itself (e.g. hints on the left, a status badge on the right).
    /// An empty slice yields an empty line.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        let mut spans = Vec::new();
        for (i, (key, label)) in self.hints.iter().enumerate() {
            if i > 0 {
                // Dim separator between pairs (leading/trailing spaces included so
                // the pairs breathe regardless of key/label widths).
                spans.push(Span::styled("  •  ", theme.dim()));
            }
            // Key in the accent (bold survives NO_COLOR so it still reads as a
            // key), then the label dim.
            spans.push(Span::styled((*key).to_string(), theme.prompt()));
            spans.push(Span::styled(format!(" {label}"), theme.dim()));
        }
        Line::from(spans)
    }

    /// Draw the hint strip into `area` (typically the footer row, or its left part).
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

    fn spans(hints: &[(&str, &str)], theme: Theme) -> Vec<Span<'static>> {
        KeyHints { hints }.line(theme).spans
    }

    fn text(hints: &[(&str, &str)], theme: Theme) -> String {
        spans(hints, theme).iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn empty_hints_yield_an_empty_line() {
        assert!(spans(&[], Theme::with_color(true)).is_empty());
    }

    #[test]
    fn every_key_and_label_is_rendered_with_separators_between() {
        let lit = Theme::with_color(true);
        let t = text(&[("q", "quit"), ("^P", "palette"), ("?", "help")], lit);
        assert!(t.contains("q quit"));
        assert!(t.contains("^P palette"));
        assert!(t.contains("? help"));
        // Two separators for three pairs (none leading, none trailing).
        assert_eq!(t.matches('•').count(), 2);
    }

    #[test]
    fn a_single_hint_has_no_separator() {
        let lit = Theme::with_color(true);
        assert!(!text(&[("q", "quit")], lit).contains('•'));
    }

    #[test]
    fn keys_are_accented_and_labels_dim_when_colour_is_on() {
        let lit = Theme::with_color(true);
        let s = spans(&[("q", "quit")], lit);
        // Layout per pair: [key, label]. Key carries the accent (a fg); the label
        // is dim (no fg, just the DIM attribute).
        assert!(s[0].style.fg.is_some(), "key is accented");
        assert_eq!(s[1].style.fg, None, "label has no hue");
        assert!(s[1]
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::DIM));
    }

    #[test]
    fn no_color_drops_hue_but_the_key_stays_bold() {
        let dark = Theme::with_color(false);
        let s = spans(&[("q", "quit")], dark);
        for span in &s {
            assert_eq!(span.style.fg, None, "no fg under NO_COLOR");
        }
        // The key must still be distinguishable from the label without colour:
        // prompt() is bold, the label is dim.
        assert!(s[0]
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD));
        assert!(s[1]
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::DIM));
    }
}

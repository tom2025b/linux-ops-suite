//! StatusStrip: a one-line run of short status segments, `·`-joined.
//!
//! The general form of the `All · Auto · 312` strip a tool shows on its search
//! line: a handful of small state labels (the active view, the sort, a count),
//! dim, separated by a spaced middot, on one line. A consumer commonly draws it
//! right-aligned over a pane's inner area, but alignment is the caller's
//! `Paragraph` choice — like every one-line widget here, this draws a
//! left-origin [`Line`].
//!
//! Like [`KeyHints`](crate::KeyHints) and [`FilterChips`](crate::FilterChips) it
//! owns no state, borrows the segments the consumer passes, and is dim by design.
//! A caller that wants one segment emphasised (a narrowed count, say) composes
//! the [`line`](StatusStrip::line) spans itself or drops a
//! [`Counted`](crate::Counted) span in — the strip itself stays uniformly dim so
//! it reads as ambient state, not a call to action.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// The spaced middot that joins segments. Shared as a `const` so a caller that
/// composes its own strip by hand can match the separator exactly.
pub const STATUS_SEP: &str = " · ";

/// A single-line `·`-joined status strip. Holds only the borrowed segments; it
/// owns no application state and reads nothing from the environment.
///
/// ```no_run
/// # use suite_ui::{StatusStrip, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, row: Rect, theme: Theme) {
/// let segments = ["All", "Auto", "312"];
/// StatusStrip { segments: &segments }.render(frame, row, theme);
/// # }
/// ```
pub struct StatusStrip<'a> {
    /// The status segments in display order, e.g. `["All", "Auto", "312"]`.
    /// Joined by [`STATUS_SEP`]. An empty slice yields an empty line.
    pub segments: &'a [&'a str],
}

impl StatusStrip<'_> {
    /// The composed [`Line`] — every segment dim, a dim [`STATUS_SEP`] between
    /// each pair (none leading or trailing). For a caller that folds the strip
    /// into a row it lays out itself, or aligns it with its own `Paragraph`. An
    /// empty `segments` slice yields an empty line.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(STATUS_SEP, theme.dim()));
            }
            spans.push(Span::styled((*segment).to_string(), theme.dim()));
        }
        Line::from(spans)
    }

    /// Draw the strip into `area` (typically a one-row region, often right-aligned
    /// by the caller over a pane's inner area).
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

    fn text(segments: &[&str], theme: Theme) -> String {
        StatusStrip { segments }
            .line(theme)
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn segments_are_joined_by_the_middot() {
        let lit = Theme::with_color(true);
        assert_eq!(text(&["All", "Auto", "312"], lit), "All · Auto · 312");
    }

    #[test]
    fn empty_segments_yield_an_empty_line() {
        assert!(StatusStrip { segments: &[] }
            .line(Theme::with_color(true))
            .spans
            .is_empty());
    }

    #[test]
    fn a_single_segment_has_no_separator() {
        let lit = Theme::with_color(true);
        assert_eq!(text(&["All"], lit), "All");
        assert!(!text(&["All"], lit).contains('·'));
    }

    #[test]
    fn separator_count_is_one_fewer_than_segments() {
        let lit = Theme::with_color(true);
        assert_eq!(text(&["a", "b", "c", "d"], lit).matches('·').count(), 3);
    }

    #[test]
    fn the_strip_is_dim_in_both_colour_modes() {
        use ratatui::style::Modifier;
        for theme in [Theme::with_color(true), Theme::with_color(false)] {
            let spans = StatusStrip {
                segments: &["All", "Auto"],
            }
            .line(theme)
            .spans;
            for span in &spans {
                // Dim is an attribute, not a colour: no foreground in either mode,
                // and the DIM modifier is always present.
                assert_eq!(span.style.fg, None, "the strip never carries a hue");
                assert!(span.style.add_modifier.contains(Modifier::DIM));
            }
        }
    }
}

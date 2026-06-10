//! AttentionFlag: a small "needs attention" flag — `⚠ 3 high` vs `✓ high`.
//!
//! The one-glance marker for "is there anything here I should look at?" — the
//! count of high/critical findings a screen surfaces, or a "review due" flag on a
//! script. It has two states and the whole point is the contrast between them:
//! **raised** (a non-zero count) draws the eye with a `⚠` and the
//! [`Severity`](crate::Severity) hue; **clear** (zero) recedes to a calm dim `✓`.
//!
//! Domain-free like the rest of the crate: the consumer counts its own findings
//! (or decides a flag is due) and passes the number and a label; the widget owns
//! the look and routes its colour through the gated
//! [`Theme::severity`](crate::Theme). The `⚠` / `✓` glyphs carry the raised/clear
//! distinction under `NO_COLOR`, where the severity hue drops away.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::{Severity, Theme};

/// A raised-or-clear attention flag for a labelled count. Cheap to copy; pass by
/// value.
///
/// ```
/// use suite_ui::{AttentionFlag, Severity, Theme};
/// # let theme = Theme::with_color(true);
/// // Three high-severity findings → the flag is raised.
/// let flag = AttentionFlag { count: 3, label: "high", severity: Severity::High };
/// assert!(flag.is_raised());
/// // Nothing to flag → clear.
/// assert!(!AttentionFlag { count: 0, label: "high", severity: Severity::High }.is_raised());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionFlag<'a> {
    /// How many things need attention. `0` is the clear state; any other value
    /// raises the flag.
    pub count: usize,
    /// What the count is about, e.g. `"high"`, `"critical"`, `"review due"`. Shown
    /// after the count when raised (`⚠ 3 high`) and alone when clear (`✓ high`).
    pub label: &'a str,
    /// How loud the raised flag is — the [`Severity`] hue it escalates to. Higher
    /// severities paint a louder colour (critical red, high yellow); ignored in
    /// the clear state, which is always the calm dim `✓`.
    pub severity: Severity,
}

impl AttentionFlag<'_> {
    /// True when the flag is raised — a non-zero [`count`](Self::count). Exposed
    /// so a caller can branch on it (e.g. to decide whether to render the flag at
    /// all, or to draw a surrounding accent) without re-deriving the rule.
    pub fn is_raised(self) -> bool {
        self.count > 0
    }

    /// The composed [`Line`]. Raised: `⚠ {count} {label}` in the severity style
    /// (so the count and its glyph share the level's hue). Clear: a calm dim
    /// `✓ {label}`. The leading glyph carries the state under `NO_COLOR`, where
    /// the severity hue drops away (`⚠` = look here, `✓` = all clear).
    pub fn line(&self, theme: Theme) -> Line<'static> {
        if self.is_raised() {
            let style = theme.severity(self.severity);
            Line::from(vec![
                Span::styled("⚠ ", style),
                Span::styled(format!("{} {}", self.count, self.label), style),
            ])
        } else {
            Line::from(vec![
                Span::styled("✓ ", theme.dim()),
                Span::styled(self.label.to_string(), theme.dim()),
            ])
        }
    }

    /// Draw the flag into `area` (typically a one-row region, e.g. a header
    /// corner or a footer segment).
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
    use ratatui::style::{Color, Modifier};

    fn spans(flag: AttentionFlag, theme: Theme) -> Vec<Span<'static>> {
        flag.line(theme).spans
    }

    fn text(flag: AttentionFlag, theme: Theme) -> String {
        spans(flag, theme)
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn is_raised_only_when_count_is_nonzero() {
        let f = |count| AttentionFlag {
            count,
            label: "high",
            severity: Severity::High,
        };
        assert!(f(1).is_raised());
        assert!(f(99).is_raised());
        assert!(!f(0).is_raised());
    }

    #[test]
    fn raised_shows_warning_glyph_count_and_label() {
        let lit = Theme::with_color(true);
        let t = text(
            AttentionFlag {
                count: 3,
                label: "high",
                severity: Severity::High,
            },
            lit,
        );
        assert!(t.starts_with('⚠'), "raised leads with the warning glyph");
        assert!(t.contains("3 high"), "raised shows the count and label");
    }

    #[test]
    fn clear_shows_check_glyph_and_label_without_a_count() {
        let lit = Theme::with_color(true);
        let t = text(
            AttentionFlag {
                count: 0,
                label: "high",
                severity: Severity::High,
            },
            lit,
        );
        assert!(t.starts_with('✓'), "clear leads with the check glyph");
        assert!(t.contains("high"), "clear still names the label");
        assert!(!t.contains('0'), "clear shows no count number");
    }

    #[test]
    fn raised_takes_the_severity_hue_when_colour_on() {
        let lit = Theme::with_color(true);
        // Critical raises red; high raises yellow — the flag defers to
        // Theme::severity, so the glyph's fg matches the level.
        assert_eq!(
            spans(
                AttentionFlag {
                    count: 2,
                    label: "critical",
                    severity: Severity::Critical,
                },
                lit,
            )[0]
            .style
            .fg,
            Some(Color::Red)
        );
        assert_eq!(
            spans(
                AttentionFlag {
                    count: 2,
                    label: "high",
                    severity: Severity::High,
                },
                lit,
            )[0]
            .style
            .fg,
            Some(Color::Yellow)
        );
    }

    #[test]
    fn clear_is_dim_and_never_takes_a_hue() {
        let lit = Theme::with_color(true);
        for span in spans(
            AttentionFlag {
                count: 0,
                label: "high",
                // Even a critical-severity flag is calm when clear.
                severity: Severity::Critical,
            },
            lit,
        ) {
            assert_eq!(span.style.fg, None, "a clear flag is never coloured");
            assert!(span.style.add_modifier.contains(Modifier::DIM));
        }
    }

    #[test]
    fn no_color_drops_hue_but_the_glyph_carries_the_state() {
        let dark = Theme::with_color(false);
        let raised = AttentionFlag {
            count: 3,
            label: "high",
            severity: Severity::High,
        };
        let clear = AttentionFlag {
            count: 0,
            label: "high",
            severity: Severity::High,
        };
        for span in spans(raised, dark).iter().chain(spans(clear, dark).iter()) {
            assert_eq!(span.style.fg, None, "no fg under NO_COLOR");
        }
        // The glyph is what distinguishes raised from clear without hue.
        assert!(text(raised, dark).starts_with('⚠'));
        assert!(text(clear, dark).starts_with('✓'));
        // A raised flag stays bold (high is bold) so it still reads as louder than
        // the dim clear state.
        assert!(spans(raised, dark)[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }
}

//! SeverityBadge: a short bracketed risk badge (`[CRIT]`, `[HIGH]`, `[MED]`).
//!
//! The visible marker for a finding's risk level — the `[CRIT]` tag a flagged
//! script or a risky operation carries in a list, so the eye lands on the worst
//! items first. It pairs with the [`Severity`] axis the same way
//! [`StatusBar`](crate::StatusBar) pairs with [`Outcome`](crate::Outcome): the
//! consumer grades its own items down to a [`Severity`] and the badge paints the
//! one shared look, routed through the gated [`Theme::severity`](crate::Theme).
//!
//! Like [`Counted`](crate::Counted) it produces a styled [`Span`]/[`Line`], not a
//! widget — a badge is something you fold into a row you're already composing (a
//! table cell, a header), never a region you draw on its own. The bracketed,
//! upper-case label carries the level textually, so under `NO_COLOR` — where
//! `Critical` and `High` both drop to bold — the words `CRIT` and `HIGH` still
//! tell them apart.

use ratatui::text::{Line, Span};

use crate::theme::{Severity, Theme};

/// A severity / risk badge for one [`Severity`] level. Cheap to copy; pass by
/// value.
///
/// ```
/// use suite_ui::{SeverityBadge, Severity, Theme};
/// # let theme = Theme::with_color(true);
/// let badge = SeverityBadge { severity: Severity::Critical };
/// assert_eq!(badge.span(theme).content, "[CRIT]");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeverityBadge {
    /// The level this badge marks.
    pub severity: Severity,
}

impl SeverityBadge {
    /// The short upper-case abbreviation for a level: `CRIT`, `HIGH`, `MED`,
    /// `LOW`. Pure (no brackets, no style), shared so a caller composing its own
    /// label matches the wording exactly.
    pub fn abbr(severity: Severity) -> &'static str {
        match severity {
            Severity::Critical => "CRIT",
            Severity::High => "HIGH",
            Severity::Medium => "MED",
            Severity::Low => "LOW",
            // `Severity` is #[non_exhaustive]; a future level shows `?` rather
            // than failing to compile or masquerading as an existing level.
            _ => "?",
        }
    }

    /// The badge text as it is displayed: the abbreviation in brackets, e.g.
    /// `[CRIT]`. Pure, for tests and reuse.
    pub fn text(severity: Severity) -> String {
        format!("[{}]", Self::abbr(severity))
    }

    /// The `[LEVEL]` badge as a styled [`Span`], painted in the level's
    /// [`Theme::severity`](crate::Theme::severity) style: red+bold critical,
    /// yellow+bold high, plain medium, dim low. Under `NO_COLOR` the hue drops
    /// but the bracketed upper-case word still carries the level.
    pub fn span(self, theme: Theme) -> Span<'static> {
        Span::styled(Self::text(self.severity), theme.severity(self.severity))
    }

    /// The badge as a one-span [`Line`], for a caller that wants a `Line`
    /// directly rather than folding the [`span`](Self::span) into one.
    pub fn line(self, theme: Theme) -> Line<'static> {
        Line::from(self.span(theme))
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn text_is_the_bracketed_abbreviation() {
        assert_eq!(SeverityBadge::text(Severity::Critical), "[CRIT]");
        assert_eq!(SeverityBadge::text(Severity::High), "[HIGH]");
        assert_eq!(SeverityBadge::text(Severity::Medium), "[MED]");
        assert_eq!(SeverityBadge::text(Severity::Low), "[LOW]");
    }

    #[test]
    fn span_carries_the_badge_text() {
        let lit = Theme::with_color(true);
        assert_eq!(
            SeverityBadge {
                severity: Severity::Critical
            }
            .span(lit)
            .content,
            "[CRIT]"
        );
    }

    #[test]
    fn span_is_styled_by_severity_when_colour_on() {
        let lit = Theme::with_color(true);
        // The badge defers entirely to Theme::severity, so the hues match it:
        // critical red, high yellow, medium/low no hue.
        assert_eq!(
            SeverityBadge {
                severity: Severity::Critical
            }
            .span(lit)
            .style
            .fg,
            Some(Color::Red)
        );
        assert_eq!(
            SeverityBadge {
                severity: Severity::High
            }
            .span(lit)
            .style
            .fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            SeverityBadge {
                severity: Severity::Medium
            }
            .span(lit)
            .style
            .fg,
            None
        );
        assert_eq!(
            SeverityBadge {
                severity: Severity::Low
            }
            .span(lit)
            .style
            .fg,
            None
        );
    }

    #[test]
    fn no_color_drops_hue_but_the_word_carries_the_level() {
        let dark = Theme::with_color(false);
        for s in [
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
        ] {
            let span = SeverityBadge { severity: s }.span(dark);
            assert_eq!(span.style.fg, None, "{s:?} must have no fg under NO_COLOR");
            // The bracketed word is what distinguishes the levels without hue.
            assert_eq!(span.content, SeverityBadge::text(s));
        }
        // Critical and High both go bold under NO_COLOR — so the WORD has to be
        // what tells them apart, which it is (CRIT vs HIGH).
        let crit = SeverityBadge {
            severity: Severity::Critical,
        }
        .span(dark);
        let high = SeverityBadge {
            severity: Severity::High,
        }
        .span(dark);
        assert!(crit.style.add_modifier.contains(Modifier::BOLD));
        assert!(high.style.add_modifier.contains(Modifier::BOLD));
        assert_ne!(crit.content, high.content, "the word disambiguates them");
        // Low recedes to dim.
        assert!(SeverityBadge {
            severity: Severity::Low
        }
        .span(dark)
        .style
        .add_modifier
        .contains(Modifier::DIM));
    }

    #[test]
    fn line_carries_the_same_single_span() {
        let lit = Theme::with_color(true);
        let badge = SeverityBadge {
            severity: Severity::High,
        };
        let line = badge.line(lit);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, badge.span(lit).content);
    }
}

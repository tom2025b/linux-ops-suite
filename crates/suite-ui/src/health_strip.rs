//! HealthStrip: a compact one-line health summary — `● bulwark  ◐ vault  ○ proto`.
//!
//! The condensed counterpart to a full health table: a row of `(glyph + label)`
//! segments, each painted in its [`Health`](crate::Health) style, so a header or
//! footer can report the state of several monitored things on a single line. It
//! reuses the suite's [`Health`] axis and its [`Theme::health`](crate::Theme)
//! styling — the same colours a tool's detailed health view uses, condensed.
//!
//! Like [`StatusStrip`](crate::StatusStrip) it draws one line, owns no state, and
//! borrows the segments the consumer passes. Each segment leads with a status
//! glyph (`●` healthy, `◐` degraded, `○` unavailable, `?` unknown) so the states
//! stay distinguishable under `NO_COLOR`, where the health hues drop away (and
//! healthy/unavailable, both bold, are told apart by the filled vs empty circle).

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::{Health, Theme};

/// The gap between segments. Two spaces — each segment already leads with its own
/// glyph, so a heavier separator (the [`STATUS_SEP`](crate::STATUS_SEP) middot)
/// would fight the glyphs. Shared as a `const` so a caller composing its own row
/// can match it.
pub const HEALTH_SEP: &str = "  ";

/// A single-line health summary strip. Holds only the borrowed `(Health, label)`
/// segments; it owns no application state and reads nothing from the environment.
///
/// ```no_run
/// # use suite_ui::{HealthStrip, Health, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, row: Rect, theme: Theme) {
/// let segments = [
///     (Health::Healthy, "bulwark"),
///     (Health::Degraded, "vault"),
///     (Health::Unavailable, "proto"),
/// ];
/// HealthStrip { segments: &segments }.render(frame, row, theme);
/// # }
/// ```
pub struct HealthStrip<'a> {
    /// The health segments in display order, each a `(Health, label)` pair. Joined
    /// by [`HEALTH_SEP`]. An empty slice yields an empty line.
    pub segments: &'a [(Health, &'a str)],
}

impl HealthStrip<'_> {
    /// The status glyph for a [`Health`] level: `●` healthy, `◐` degraded, `○`
    /// unavailable, `?` unknown. Pure and shared so a caller composing its own
    /// segment matches the vocabulary, and so the glyph (not the dropped hue)
    /// carries the level under `NO_COLOR`.
    pub fn glyph(health: Health) -> &'static str {
        match health {
            Health::Healthy => "●",
            Health::Degraded => "◐",
            Health::Unavailable => "○",
            Health::Unknown => "?",
        }
    }

    /// The composed [`Line`] — each segment a `glyph label` pair in that level's
    /// [`Theme::health`](crate::Theme::health) style, [`HEALTH_SEP`] between pairs
    /// (none leading or trailing). For a caller that folds the strip into a row it
    /// lays out itself. An empty `segments` slice yields an empty line.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, (health, label)) in self.segments.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(HEALTH_SEP, theme.dim()));
            }
            // Glyph and label share the level's health style so the whole segment
            // reads as one coloured unit.
            let style = theme.health(*health);
            spans.push(Span::styled(
                format!("{} {}", Self::glyph(*health), label),
                style,
            ));
        }
        Line::from(spans)
    }

    /// Draw the strip into `area` (typically a one-row region, e.g. a header line
    /// summarising adapter health).
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

    fn spans(segments: &[(Health, &str)], theme: Theme) -> Vec<Span<'static>> {
        HealthStrip { segments }.line(theme).spans
    }

    fn text(segments: &[(Health, &str)], theme: Theme) -> String {
        spans(segments, theme)
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn glyph_is_distinct_per_level() {
        // All four must differ — that's what carries the level under NO_COLOR.
        let glyphs = [
            HealthStrip::glyph(Health::Healthy),
            HealthStrip::glyph(Health::Degraded),
            HealthStrip::glyph(Health::Unavailable),
            HealthStrip::glyph(Health::Unknown),
        ];
        for i in 0..glyphs.len() {
            for j in (i + 1)..glyphs.len() {
                assert_ne!(glyphs[i], glyphs[j], "glyphs {i} and {j} must differ");
            }
        }
    }

    #[test]
    fn each_segment_shows_its_glyph_and_label() {
        let lit = Theme::with_color(true);
        let t = text(
            &[
                (Health::Healthy, "bulwark"),
                (Health::Degraded, "vault"),
                (Health::Unavailable, "proto"),
            ],
            lit,
        );
        assert!(t.contains("● bulwark"));
        assert!(t.contains("◐ vault"));
        assert!(t.contains("○ proto"));
    }

    #[test]
    fn empty_segments_yield_an_empty_line() {
        assert!(spans(&[], Theme::with_color(true)).is_empty());
    }

    #[test]
    fn a_single_segment_has_no_separator() {
        let lit = Theme::with_color(true);
        let t = text(&[(Health::Healthy, "solo")], lit);
        assert_eq!(t, "● solo");
    }

    #[test]
    fn segments_are_separated_but_not_bracketed_by_the_gap() {
        let lit = Theme::with_color(true);
        // Two segments → exactly one separator, and the line neither starts nor
        // ends with whitespace.
        let t = text(&[(Health::Healthy, "a"), (Health::Degraded, "b")], lit);
        assert_eq!(t, format!("● a{HEALTH_SEP}◐ b"));
        assert!(!t.starts_with(' ') && !t.ends_with(' '));
    }

    #[test]
    fn each_segment_takes_its_health_hue_when_colour_on() {
        let lit = Theme::with_color(true);
        let s = spans(
            &[
                (Health::Healthy, "a"),
                (Health::Degraded, "b"),
                (Health::Unavailable, "c"),
            ],
            lit,
        );
        // Segments are at even indices (odd indices are the separators).
        assert_eq!(s[0].style.fg, Some(Color::Green), "healthy is green");
        assert_eq!(s[2].style.fg, Some(Color::Yellow), "degraded is yellow");
        assert_eq!(s[4].style.fg, Some(Color::Red), "unavailable is red");
    }

    #[test]
    fn no_color_drops_hue_but_glyphs_and_attributes_keep_levels_apart() {
        let dark = Theme::with_color(false);
        let segs = [
            (Health::Healthy, "a"),
            (Health::Degraded, "b"),
            (Health::Unavailable, "c"),
            (Health::Unknown, "d"),
        ];
        for span in spans(&segs, dark) {
            assert_eq!(span.style.fg, None, "no fg under NO_COLOR");
        }
        // Healthy and Unavailable are both bold under NO_COLOR, so the glyph is
        // what tells them apart — assert both the attribute and the glyph.
        let s = spans(&segs, dark);
        assert!(s[0].style.add_modifier.contains(Modifier::BOLD)); // healthy
        assert!(s[4].style.add_modifier.contains(Modifier::BOLD)); // unavailable
        assert!(s[0].content.starts_with('●'));
        assert!(s[4].content.starts_with('○'));
        // Unknown is dim, the quietest.
        assert!(s[6].style.add_modifier.contains(Modifier::DIM));
    }
}

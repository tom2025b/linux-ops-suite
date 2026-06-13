//! Freshness: a compact provenance stamp — `just now`, `2h ago`, `9d ago`.
//!
//! The "how recent is this?" stamp a screen puts next to a scanned inventory or a
//! last-probed status — a short, human age so the user can tell stale data from
//! fresh at a glance. It pairs with an optional staleness threshold: a stamp past
//! it is painted as a quiet warning (the data is getting old) instead of ambient
//! dim.
//!
//! Like the [`truncate`](crate::truncate_path) helpers this is a **pure**
//! formatter: it takes an already-elapsed [`Duration`], never a clock — the
//! consumer owns "now" and passes `now - timestamp`. That keeps it testable
//! without a real clock and keeps the crate's "reads nothing from the
//! environment" rule intact. The compact age text carries the recency under
//! `NO_COLOR`, where the stale hue drops away (a `9d ago` reads as older than a
//! `2h ago` without any colour).

use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// Ages at or below this read as "just now" rather than "0s ago".
const JUST_NOW_SECS: u64 = 5;

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;

/// A compact provenance / freshness stamp for an elapsed age. Cheap to copy; pass
/// by value.
///
/// ```
/// use std::time::Duration;
/// use thomas_tui::{Freshness, Theme};
/// # let theme = Theme::with_color(true);
/// assert_eq!(Freshness::from(Duration::from_secs(2)).label(), "just now");
/// assert_eq!(Freshness::from(Duration::from_secs(7200)).label(), "2h ago");
/// // A stamp past its staleness threshold is flagged.
/// let f = Freshness { age: Duration::from_secs(3 * 86_400), stale_after: Some(Duration::from_secs(86_400)) };
/// assert!(f.is_stale());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freshness {
    /// How long ago the thing happened — already elapsed (`now - timestamp`), so
    /// the stamp stays a pure function of its input.
    pub age: Duration,
    /// Optional staleness threshold. When set and [`age`](Self::age) is at or past
    /// it, the stamp is [`stale`](Self::is_stale) and painted as a quiet warning.
    /// `None` means the stamp is always ambient dim, however old.
    pub stale_after: Option<Duration>,
}

impl From<Duration> for Freshness {
    /// A stamp with no staleness threshold — always ambient dim. The common case
    /// (`Freshness::from(elapsed)`) for a stamp that only shows the age.
    fn from(age: Duration) -> Self {
        Self {
            age,
            stale_after: None,
        }
    }
}

impl Freshness {
    /// The compact age text, in the single largest unit that fits: `just now`
    /// (≤ 5s), then `Ns ago`, `Nm ago`, `Nh ago`, `Nd ago`, `Nw ago`. Pure, so
    /// it is unit-testable without a terminal or a clock, and shared so every
    /// tool stamps recency with the same wording.
    pub fn label(self) -> String {
        let secs = self.age.as_secs();
        if secs <= JUST_NOW_SECS {
            return "just now".to_string();
        }
        let (n, unit) = if secs < MINUTE {
            (secs, "s")
        } else if secs < HOUR {
            (secs / MINUTE, "m")
        } else if secs < DAY {
            (secs / HOUR, "h")
        } else if secs < WEEK {
            (secs / DAY, "d")
        } else {
            (secs / WEEK, "w")
        };
        format!("{n}{unit} ago")
    }

    /// True when a staleness threshold is set and the [`age`](Self::age) is at or
    /// past it. Exposed so a caller can branch (e.g. to add a "refresh" hint)
    /// without re-deriving the rule. Always false when no threshold is set.
    pub fn is_stale(self) -> bool {
        self.stale_after.is_some_and(|t| self.age >= t)
    }

    /// The stamp as a styled [`Span`]: ambient [`dim`](crate::Theme::dim) when
    /// fresh, the [`working`](crate::Theme::working) warning style when
    /// [`stale`](Self::is_stale). Under `NO_COLOR` both drop to dim — the age text
    /// itself carries the recency, so the hue is a bonus, never the sole signal.
    pub fn span(self, theme: Theme) -> Span<'static> {
        let style = if self.is_stale() {
            theme.working()
        } else {
            theme.dim()
        };
        Span::styled(self.label(), style)
    }

    /// The stamp as a one-span [`Line`], for a caller that wants a `Line` directly
    /// rather than folding the [`span`](Self::span) into one.
    pub fn line(self, theme: Theme) -> Line<'static> {
        Line::from(self.span(theme))
    }

    /// Draw the stamp into `area` (typically a one-row region, e.g. a header
    /// corner reporting when an inventory was last scanned).
    pub fn render(self, frame: &mut Frame, area: Rect, theme: Theme) {
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

    fn label(secs: u64) -> String {
        Freshness::from(Duration::from_secs(secs)).label()
    }

    #[test]
    fn just_now_covers_the_first_few_seconds() {
        assert_eq!(label(0), "just now");
        assert_eq!(label(5), "just now");
        // Just past the threshold it switches to a seconds count.
        assert_eq!(label(6), "6s ago");
    }

    #[test]
    fn each_unit_picks_the_largest_that_fits() {
        assert_eq!(label(45), "45s ago");
        assert_eq!(label(60), "1m ago");
        assert_eq!(label(90), "1m ago", "floors to the whole unit");
        assert_eq!(label(59 * 60), "59m ago");
        assert_eq!(label(60 * 60), "1h ago");
        assert_eq!(label(2 * 60 * 60), "2h ago");
        assert_eq!(label(23 * 60 * 60), "23h ago");
        assert_eq!(label(24 * 60 * 60), "1d ago");
        assert_eq!(label(6 * 86_400), "6d ago");
        assert_eq!(label(7 * 86_400), "1w ago");
        assert_eq!(label(30 * 86_400), "4w ago");
    }

    #[test]
    fn unit_boundaries_round_down_not_up() {
        // 119s is 1m (not 2m); 1d minus a second is still 23h.
        assert_eq!(label(119), "1m ago");
        assert_eq!(label(DAY - 1), "23h ago");
        assert_eq!(label(WEEK - 1), "6d ago");
    }

    #[test]
    fn from_duration_has_no_staleness_threshold() {
        let f = Freshness::from(Duration::from_secs(999_999));
        assert_eq!(f.stale_after, None);
        assert!(!f.is_stale(), "no threshold is never stale");
    }

    #[test]
    fn is_stale_only_at_or_past_the_threshold() {
        let day = Duration::from_secs(DAY);
        let mk = |age_secs| Freshness {
            age: Duration::from_secs(age_secs),
            stale_after: Some(day),
        };
        assert!(!mk(DAY - 1).is_stale(), "just under is fresh");
        assert!(mk(DAY).is_stale(), "exactly at the threshold is stale");
        assert!(mk(DAY + 1).is_stale(), "past it is stale");
    }

    #[test]
    fn fresh_is_dim_stale_is_the_working_warning_when_colour_on() {
        let lit = Theme::with_color(true);
        // Fresh (no threshold): dim, no hue.
        let fresh = Freshness::from(Duration::from_secs(3600)).span(lit);
        assert_eq!(fresh.style.fg, None);
        assert!(fresh.style.add_modifier.contains(Modifier::DIM));
        // Stale: the working warning hue (yellow) when colour is on.
        let stale = Freshness {
            age: Duration::from_secs(10 * DAY),
            stale_after: Some(Duration::from_secs(DAY)),
        }
        .span(lit);
        assert_eq!(stale.style.fg, Some(Color::Yellow));
    }

    #[test]
    fn no_color_drops_the_stale_hue_but_the_age_text_carries_recency() {
        let dark = Theme::with_color(false);
        let stale = Freshness {
            age: Duration::from_secs(10 * DAY),
            stale_after: Some(Duration::from_secs(DAY)),
        };
        let fresh = Freshness::from(Duration::from_secs(120));
        // Neither carries a hue without colour.
        assert_eq!(stale.span(dark).style.fg, None);
        assert_eq!(fresh.span(dark).style.fg, None);
        // The wording is what conveys recency without colour: "1w ago" vs "2m ago".
        assert_eq!(stale.span(dark).content, "1w ago");
        assert_eq!(fresh.span(dark).content, "2m ago");
    }

    #[test]
    fn line_carries_the_same_single_span() {
        let lit = Theme::with_color(true);
        let f = Freshness::from(Duration::from_secs(7200));
        let line = f.line(lit);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "2h ago");
    }
}

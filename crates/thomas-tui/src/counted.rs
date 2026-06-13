//! Counted: the "N of M" shown-of-total count, styled by one shared rule.
//!
//! Anywhere a "showing 48 of 312" count appears — a pane title, a header line, a
//! status strip — the same rule applies: **emphasise the count when the list is
//! narrowed, let it recede when it shows everything**. This is that rule in one
//! place, so every count reads alike instead of being encoded by hand each time.
//!
//! It produces a styled [`Span`]/[`Line`], not a widget — a count is something
//! you fold into a title or a row you're already composing, never a region you
//! draw on its own. It owns no state and routes its colour through the gated
//! [`Theme`], so it drops to a bold-only emphasis under `NO_COLOR`.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// A `shown`-of-`total` count. Cheap to copy; pass by value.
///
/// ```
/// use thomas_tui::{Counted, Theme};
/// # let theme = Theme::with_color(true);
/// // A narrowed view (a filter is active) → the count is emphasised.
/// let c = Counted { shown: 48, total: 312 };
/// assert!(c.is_narrowed());
/// assert_eq!(c.span(theme).content, "48 of 312");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counted {
    /// How many items are currently shown (after filtering).
    pub shown: usize,
    /// How many there are in total (before filtering).
    pub total: usize,
}

impl Counted {
    /// True when the list is narrowed — fewer shown than the total. The
    /// predicate the styling is built on, exposed so a caller can branch on it
    /// (e.g. to decide whether to draw a "clear filter" hint) without
    /// recomputing it.
    pub fn is_narrowed(self) -> bool {
        self.shown < self.total
    }

    /// The `"{shown} of {total}"` text, styled by the shared rule: the suite
    /// accent in italic when narrowed (so a filtered count draws the eye), `dim`
    /// when the view is full. Under `NO_COLOR` the accent drops to bold-only and
    /// the italic still distinguishes a narrowed count from a full one.
    pub fn span(self, theme: Theme) -> Span<'static> {
        let style = if self.is_narrowed() {
            theme.accent_bar().add_modifier(Modifier::ITALIC)
        } else {
            theme.dim()
        };
        Span::styled(format!("{} of {}", self.shown, self.total), style)
    }

    /// The count as a one-span [`Line`], for a caller that wants a `Line`
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

    #[test]
    fn text_is_shown_of_total() {
        let theme = Theme::with_color(true);
        assert_eq!(
            Counted {
                shown: 48,
                total: 312
            }
            .span(theme)
            .content,
            "48 of 312"
        );
        assert_eq!(Counted { shown: 0, total: 0 }.span(theme).content, "0 of 0");
    }

    #[test]
    fn is_narrowed_only_when_shown_is_less_than_total() {
        assert!(Counted {
            shown: 48,
            total: 312
        }
        .is_narrowed());
        assert!(!Counted {
            shown: 312,
            total: 312
        }
        .is_narrowed());
        // Equal (or the empty 0-of-0 view) is "full", not narrowed.
        assert!(!Counted { shown: 0, total: 0 }.is_narrowed());
    }

    #[test]
    fn narrowed_count_is_accented_full_count_is_dim_when_colour_on() {
        let lit = Theme::with_color(true);
        // Narrowed: an accent foreground, plus italic.
        let narrowed = Counted {
            shown: 48,
            total: 312,
        }
        .span(lit);
        assert!(narrowed.style.fg.is_some(), "a narrowed count is accented");
        assert!(narrowed.style.add_modifier.contains(Modifier::ITALIC));
        // Full: dim, no foreground hue.
        let full = Counted {
            shown: 312,
            total: 312,
        }
        .span(lit);
        assert_eq!(full.style.fg, None, "a full count is dim, not accented");
        assert!(full.style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn no_color_drops_hue_but_keeps_the_narrowed_emphasis() {
        let dark = Theme::with_color(false);
        let narrowed = Counted {
            shown: 48,
            total: 312,
        }
        .span(dark);
        let full = Counted {
            shown: 312,
            total: 312,
        }
        .span(dark);
        // No foreground in either case under NO_COLOR.
        assert_eq!(narrowed.style.fg, None);
        assert_eq!(full.style.fg, None);
        // But the narrowed count still stands out: accent_bar is bold off-colour,
        // and the italic survives. The full count is merely dim.
        assert!(narrowed.style.add_modifier.contains(Modifier::BOLD));
        assert!(narrowed.style.add_modifier.contains(Modifier::ITALIC));
        assert!(full.style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn line_carries_the_same_single_span() {
        let lit = Theme::with_color(true);
        let c = Counted { shown: 1, total: 9 };
        let line = c.line(lit);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, c.span(lit).content);
    }
}

//! Heartbeat: a liveness sparkline (`♥ ▁▂▅▇▅▂ 12ms`) for monitor-style cards.
//!
//! The one genuinely novel cockpit vital: a heart glyph, a Unicode block
//! sparkline of recent samples, and the latest value. Pure like
//! [`SeverityBadge`](crate::SeverityBadge) — it yields a styled [`Line`], not a
//! region you draw on its own; a card folds it into the cell where its vital
//! goes. Empty input degrades gracefully so a card with no samples yet still
//! reads cleanly.

use ratatui::text::{Line, Span};

use crate::theme::{Health, Theme};

/// The eight block glyphs, low→high.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A heartbeat vital over recent latency samples (oldest→newest) plus the
/// latest reading. Cheap to build; borrow the samples.
#[derive(Debug, Clone, Copy)]
pub struct Heartbeat<'a> {
    /// Recent latency samples, oldest first.
    pub samples: &'a [u64],
    /// The most recent latency, shown as `Nms`. `None` hides the number.
    pub latest_ms: Option<u64>,
}

impl<'a> Heartbeat<'a> {
    /// Map samples onto the eight block glyphs by min→max range. Empty → "".
    /// A flat series maps to the lowest block (no variation to show).
    pub fn sparkline(samples: &[u64]) -> String {
        if samples.is_empty() {
            return String::new();
        }
        let min = *samples.iter().min().unwrap();
        let max = *samples.iter().max().unwrap();
        let span = max.saturating_sub(min);
        samples
            .iter()
            .map(|&s| {
                let idx = (((s - min) * 7 + span / 2).checked_div(span).unwrap_or(0)) as usize;
                BLOCKS[idx]
            })
            .collect()
    }

    /// The textual vital, pure for tests/reuse: `♥`, then the sparkline (if any),
    /// then the latest latency (if any).
    pub fn text(&self) -> String {
        let mut out = String::from("♥");
        let spark = Self::sparkline(self.samples);
        if !spark.is_empty() {
            out.push(' ');
            out.push_str(&spark);
        }
        if let Some(ms) = self.latest_ms {
            out.push_str(&format!(" {ms}ms"));
        }
        out
    }

    /// The vital as a styled one-span [`Line`], painted in the healthy accent
    /// (gated by `NO_COLOR` via `Theme`). The glyphs carry the meaning textually
    /// when colour is off.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        Line::from(Span::styled(self.text(), theme.health(Health::Healthy)))
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_maps_samples_across_eight_levels() {
        // min→lowest block, max→highest; flat input → all the same mid-ish block.
        assert_eq!(Heartbeat::sparkline(&[0, 7, 15]), "▁▄█");
        assert_eq!(Heartbeat::sparkline(&[]), "");
        assert_eq!(Heartbeat::sparkline(&[5, 5, 5]), "▁▁▁");
    }

    #[test]
    fn text_pairs_heart_sparkline_and_latest_latency() {
        let hb = Heartbeat {
            samples: &[1, 4, 9],
            latest_ms: Some(9),
        };
        assert_eq!(hb.text(), "♥ ▁▄█ 9ms");
    }

    #[test]
    fn text_with_no_samples_shows_only_heart_and_latency() {
        let hb = Heartbeat {
            samples: &[],
            latest_ms: Some(7),
        };
        assert_eq!(hb.text(), "♥ 7ms");
    }

    #[test]
    fn text_with_nothing_is_just_the_heart() {
        let hb = Heartbeat {
            samples: &[],
            latest_ms: None,
        };
        assert_eq!(hb.text(), "♥");
    }
}

// Learning Notes
// - Pure render: `text`/`sparkline` are testable without a backend; `line` is the
//   only styled surface, matching the badge/strip widgets.
// - Range-normalised sparkline (min→max), so a steady ~7ms heartbeat still shows
//   a visible trace rather than a flat line — except a truly constant series,
//   which honestly reads flat.

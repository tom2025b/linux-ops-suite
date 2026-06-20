//! StatusBar: a one-line job-status segment shared across the suite.
//!
//! A persistent status indicator for "is a job running, and how did the last one
//! end?" — the question every screen in a tool that runs background work wants
//! answered at a glance. Like [`Toast`](crate::Toast) it draws a single line and
//! does NOT frame or clear a region: the caller lays out a status row (typically
//! the footer) and either hands the whole row to [`render`](StatusBar::render) or
//! folds [`line`](StatusBar::line) into a row it composes itself.
//!
//! Domain-free by design. The widget knows nothing about job handles, exit
//! codes, or how a tool spawns work — the consumer maps its own state onto the
//! small [`JobState`] enum and the widget paints it. That is what lets RexOps and
//! ScriptVault show the same status segment from one source.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// How a finished job ended, reduced to the three outcomes the suite paints the
/// same way everywhere: a clean exit, a failure, or a cancel/signal. This is the
/// single source of the outcome → (glyph, style) mapping — [`StatusBar`] (its
/// [`Done`](JobState::Done)/[`Cancelled`](JobState::Cancelled) states) and
/// [`Toast`](crate::Toast) (its job-lifecycle kinds) both render through it, so a
/// transient flash and the persistent status segment can never drift apart, and a
/// consumer's own history/footer rows can reuse the identical styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The job finished cleanly (exit 0): `✓` + the green health style.
    Success,
    /// The job failed (non-zero exit): `✗` + the red failure style.
    Failure,
    /// The job was cancelled or killed by a signal: `■` + the yellow working style.
    Cancelled,
}

impl Outcome {
    /// The leading glyph (with a trailing space) and the style this outcome is
    /// painted in. `✓` green, `✗` red, `■` yellow — the glyph carries the outcome
    /// under `NO_COLOR`, where the hues drop away.
    pub fn glyph_style(self, theme: Theme) -> (&'static str, Style) {
        match self {
            Outcome::Success => ("✓ ", theme.health(crate::Health::Healthy)),
            Outcome::Failure => ("✗ ", theme.status_error()),
            Outcome::Cancelled => ("■ ", theme.working()),
            // `Outcome` is #[non_exhaustive]: a future variant gets a neutral
            // marker rather than failing to compile. This is the single mapping
            // both StatusBar and Toast route through, so the fallback keeps them
            // in agreement. Unreachable today (own-crate match) but required once
            // a variant is added; the allow keeps -D warnings happy until then.
            #[allow(unreachable_patterns)]
            _ => ("? ", theme.dim()),
        }
    }
}

/// The state of the one tracked background job, as far as the status bar shows
/// it. A consumer maps its own job model onto this: a live handle → [`Running`],
/// a finished run → [`Done`] (with `ok` from the exit code), a user/​signal
/// cancel → [`Cancelled`], and nothing notable → [`Idle`].
///
/// [`Running`]: JobState::Running
/// [`Done`]: JobState::Done
/// [`Cancelled`]: JobState::Cancelled
/// [`Idle`]: JobState::Idle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JobState<'a> {
    /// No job has run, or nothing worth surfacing.
    Idle,
    /// A job is currently running (streaming output).
    Running { name: &'a str },
    /// The last job was cancelled or killed by a signal.
    Cancelled { name: &'a str },
    /// The last job finished. `ok` is true for a clean (exit-0) finish.
    Done { name: &'a str, ok: bool },
}

/// A single-line job-status segment. Holds only a borrowed [`JobState`]; it owns
/// no application state and reads nothing from the environment.
///
/// ```no_run
/// # use suite_ui::{StatusBar, JobState, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, status_row: Rect, theme: Theme) {
/// StatusBar { job: JobState::Running { name: "backup" } }
///     .render(frame, status_row, theme);
/// # }
/// ```
pub struct StatusBar<'a> {
    /// The job state to display.
    pub job: JobState<'a>,
}

impl JobState<'_> {
    /// The terminal [`Outcome`] for a finished state (the single "how it ended"
    /// vocabulary shared with [`Toast`](crate::Toast)), or `None` while the job is
    /// [`Idle`](JobState::Idle) or [`Running`](JobState::Running). Names the mapping
    /// [`StatusBar::line`] paints through, so callers (history rows, footers) can
    /// reuse the identical outcome without re-deriving it from `ok`.
    pub fn outcome(&self) -> Option<Outcome> {
        match self {
            JobState::Idle | JobState::Running { .. } => None,
            JobState::Done { ok: true, .. } => Some(Outcome::Success),
            JobState::Done { ok: false, .. } => Some(Outcome::Failure),
            JobState::Cancelled { .. } => Some(Outcome::Cancelled),
            // `JobState` is #[non_exhaustive]: a future finished state has no known
            // outcome here until it is mapped, so report None rather than guess.
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

/// The trailing word a finished [`Outcome`] reads as in the status line.
fn outcome_verb(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Success => "done",
        Outcome::Failure => "failed",
        Outcome::Cancelled => "cancelled",
        // `Outcome` is #[non_exhaustive]: a future variant gets a neutral verb.
        #[allow(unreachable_patterns)]
        _ => "finished",
    }
}

impl StatusBar<'_> {
    /// The composed status [`Line`], for a caller that wants to fold the segment
    /// into a footer row it lays out itself (e.g. status + " | " + keybind hints).
    ///
    /// Each state leads with a distinguishing glyph so the states stay readable
    /// under `NO_COLOR`, where the hues drop away:
    /// `●` running, `✓` done-ok, `✗` done-failed, `■` cancelled.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        // Finished states share one path: the outcome (via `JobState::outcome`) picks
        // the glyph/style/verb, so the mapping lives in exactly one place.
        let name = match self.job {
            JobState::Done { name, .. } | JobState::Cancelled { name } => Some(name),
            _ => None,
        };
        if let (Some(name), Some(outcome)) = (name, self.job.outcome()) {
            let (glyph, style) = outcome.glyph_style(theme);
            return Line::from(vec![
                Span::styled(glyph, style),
                Span::styled(format!("{name} — {}", outcome_verb(outcome)), style),
            ]);
        }
        match self.job {
            JobState::Idle => Line::from(Span::styled("idle", theme.dim())),
            JobState::Running { name } => Line::from(vec![
                Span::styled("● ", theme.live_marker()),
                Span::styled(format!("running {name}"), theme.title()),
            ]),
            // Finished states are handled above; any other (future) state renders as
            // a neutral dim marker rather than failing to compile.
            #[allow(unreachable_patterns)]
            _ => Line::from(Span::styled("…", theme.dim())),
        }
    }

    /// Draw the status segment into `area` (typically a single-row status line).
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

    /// The styled spans of the segment's line, for asserting on glyphs + styles.
    fn spans(job: JobState, theme: Theme) -> Vec<Span<'static>> {
        StatusBar { job }.line(theme).spans
    }

    /// Concatenated glyph text of the segment — for asserting the leading marker
    /// survives regardless of colour.
    fn text(job: JobState, theme: Theme) -> String {
        spans(job, theme)
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn outcome_maps_finished_states_and_is_none_otherwise() {
        assert_eq!(JobState::Idle.outcome(), None);
        assert_eq!(JobState::Running { name: "j" }.outcome(), None);
        assert_eq!(
            JobState::Done {
                name: "j",
                ok: true
            }
            .outcome(),
            Some(Outcome::Success)
        );
        assert_eq!(
            JobState::Done {
                name: "j",
                ok: false
            }
            .outcome(),
            Some(Outcome::Failure)
        );
        assert_eq!(
            JobState::Cancelled { name: "j" }.outcome(),
            Some(Outcome::Cancelled)
        );
    }

    #[test]
    fn outcome_glyph_style_is_the_single_mapping() {
        // The glyphs are colour-independent; the hues are the suite's standard
        // health/error/working styles. Asserting both here pins the one place the
        // mapping lives, so Toast and StatusBar can't drift from it.
        let lit = Theme::with_color(true);
        assert_eq!(Outcome::Success.glyph_style(lit).0, "✓ ");
        assert_eq!(Outcome::Failure.glyph_style(lit).0, "✗ ");
        assert_eq!(Outcome::Cancelled.glyph_style(lit).0, "■ ");
        assert_eq!(Outcome::Success.glyph_style(lit).1.fg, Some(Color::Green));
        assert_eq!(Outcome::Failure.glyph_style(lit).1.fg, Some(Color::Red));
        assert_eq!(
            Outcome::Cancelled.glyph_style(lit).1.fg,
            Some(Color::Yellow)
        );
    }

    #[test]
    fn each_state_leads_with_its_distinguishing_glyph() {
        // Glyphs are colour-independent, so assert them under NO_COLOR — that is
        // exactly where they have to carry the distinction alone.
        let dark = Theme::with_color(false);
        assert!(text(JobState::Running { name: "backup" }, dark).starts_with('●'));
        assert!(text(
            JobState::Done {
                name: "backup",
                ok: true
            },
            dark
        )
        .starts_with('✓'));
        assert!(text(
            JobState::Done {
                name: "backup",
                ok: false
            },
            dark
        )
        .starts_with('✗'));
        assert!(text(JobState::Cancelled { name: "backup" }, dark).starts_with('■'));
        assert_eq!(text(JobState::Idle, dark), "idle");
    }

    #[test]
    fn name_is_shown_for_every_non_idle_state() {
        let lit = Theme::with_color(true);
        for job in [
            JobState::Running { name: "backup" },
            JobState::Cancelled { name: "backup" },
            JobState::Done {
                name: "backup",
                ok: true,
            },
            JobState::Done {
                name: "backup",
                ok: false,
            },
        ] {
            assert!(
                text(job, lit).contains("backup"),
                "{job:?} must name the job"
            );
        }
    }

    #[test]
    fn colour_on_applies_per_state_hues() {
        let lit = Theme::with_color(true);
        // Running uses the green live marker; done-ok the green health style;
        // done-failed the red failure style. (We assert the leading glyph's fg.)
        assert_eq!(
            spans(JobState::Running { name: "j" }, lit)[0].style.fg,
            Some(Color::Green)
        );
        assert_eq!(
            spans(
                JobState::Done {
                    name: "j",
                    ok: true
                },
                lit
            )[0]
            .style
            .fg,
            Some(Color::Green)
        );
        assert_eq!(
            spans(
                JobState::Done {
                    name: "j",
                    ok: false
                },
                lit
            )[0]
            .style
            .fg,
            Some(Color::Red)
        );
        assert_eq!(
            spans(JobState::Cancelled { name: "j" }, lit)[0].style.fg,
            Some(Color::Yellow)
        );
    }

    #[test]
    fn no_color_drops_every_hue_but_keeps_an_attribute_or_glyph() {
        let dark = Theme::with_color(false);
        for job in [
            JobState::Idle,
            JobState::Running { name: "j" },
            JobState::Cancelled { name: "j" },
            JobState::Done {
                name: "j",
                ok: true,
            },
            JobState::Done {
                name: "j",
                ok: false,
            },
        ] {
            for span in spans(job, dark) {
                assert_eq!(
                    span.style.fg, None,
                    "{job:?} must have no fg under NO_COLOR"
                );
            }
        }
        // The states still differ without hue: running/done are bold, idle dim.
        assert!(spans(JobState::Running { name: "j" }, dark)[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(spans(JobState::Idle, dark)[0]
            .style
            .add_modifier
            .contains(Modifier::DIM));
    }
}

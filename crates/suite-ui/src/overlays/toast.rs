//! Toast: a one-line transient flash (a status / notification line).

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;
use crate::Health;

/// What a toast reports. Each kind picks a leading glyph and a style; the caller
/// always supplies the message text. The three job-lifecycle kinds
/// ([`Success`], [`Failure`], [`Cancelled`]) deliberately reuse the
/// [`StatusBar`](crate::StatusBar) glyphs and styles, so a transient toast and
/// the persistent status segment read identically. Every kind leads with a glyph
/// (or an `[err]` marker) so it stays distinguishable under `NO_COLOR`, where the
/// hues drop away.
///
/// [`Success`]: ToastKind::Success
/// [`Failure`]: ToastKind::Failure
/// [`Cancelled`]: ToastKind::Cancelled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// A neutral, informational flash (dim, no glyph).
    Info,
    /// A generic error (red / bold, `[err]` marker).
    Error,
    /// A job finished cleanly: `✓` + the green health style.
    Success,
    /// A job failed: `✗` + the red failure style.
    Failure,
    /// A job was cancelled or killed: `■` + the yellow working style.
    Cancelled,
}

/// A single-line transient message. Unlike the other overlays this does not
/// frame itself or clear a region — it is meant to be drawn into a status row
/// the caller has already laid out (e.g. the footer line), so it composes with
/// existing chrome instead of covering it.
///
/// ```no_run
/// # use suite_ui::{Toast, ToastKind, Theme};
/// # use ratatui::{Frame, layout::Rect};
/// # fn draw(frame: &mut Frame, status_row: Rect, theme: Theme) {
/// Toast { text: "saved", kind: ToastKind::Info }.render(frame, status_row, theme);
/// # }
/// ```
pub struct Toast<'a> {
    /// The message text.
    pub text: &'a str,
    /// Which kind of toast this is — picks the leading glyph and style.
    pub kind: ToastKind,
}

impl Toast<'_> {
    /// The composed [`Line`], for a caller that folds the toast into a row it
    /// lays out itself.
    ///
    /// Each kind leads with a marker so it survives `NO_COLOR`, where the hue
    /// drops away: `Info` is plain dim text, `Error` carries an `[err] ` prefix,
    /// and the job-lifecycle kinds lead with the same glyphs the
    /// [`StatusBar`](crate::StatusBar) uses — `✓` success, `✗` failure,
    /// `■` cancelled.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        let text = self.text.to_string();
        match self.kind {
            ToastKind::Info => Line::from(Span::styled(text, theme.dim())),
            // Prepend a marker so an error still reads as one under NO_COLOR,
            // where the red hue drops away.
            ToastKind::Error => Line::from(vec![
                Span::styled("[err] ", theme.status_error()),
                Span::styled(text, theme.status_error()),
            ]),
            ToastKind::Success => {
                let style = theme.health(Health::Healthy);
                Line::from(vec![Span::styled("✓ ", style), Span::styled(text, style)])
            }
            ToastKind::Failure => {
                let style = theme.status_error();
                Line::from(vec![Span::styled("✗ ", style), Span::styled(text, style)])
            }
            ToastKind::Cancelled => {
                let style = theme.working();
                Line::from(vec![Span::styled("■ ", style), Span::styled(text, style)])
            }
        }
    }

    /// Draw the toast into `area` (typically a single-row status line).
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

    /// The styled spans of a toast's line, for asserting on glyphs + styles.
    fn spans(text: &str, kind: ToastKind, theme: Theme) -> Vec<Span<'static>> {
        Toast { text, kind }.line(theme).spans
    }

    /// Concatenated text of a toast — for asserting the leading marker.
    fn rendered(text: &str, kind: ToastKind, theme: Theme) -> String {
        spans(text, kind, theme)
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn each_kind_leads_with_its_distinguishing_marker() {
        // Markers are colour-independent, so assert them under NO_COLOR — that is
        // exactly where they have to carry the distinction alone.
        let dark = Theme::with_color(false);
        assert_eq!(rendered("saved", ToastKind::Info, dark), "saved");
        assert!(rendered("denied", ToastKind::Error, dark).starts_with("[err] "));
        assert!(rendered("backup — done", ToastKind::Success, dark).starts_with('✓'));
        assert!(rendered("rescan — failed", ToastKind::Failure, dark).starts_with('✗'));
        assert!(rendered("deploy — cancelled", ToastKind::Cancelled, dark).starts_with('■'));
    }

    #[test]
    fn the_message_text_is_always_present() {
        let lit = Theme::with_color(true);
        for kind in [
            ToastKind::Info,
            ToastKind::Error,
            ToastKind::Success,
            ToastKind::Failure,
            ToastKind::Cancelled,
        ] {
            assert!(
                rendered("backup", kind, lit).contains("backup"),
                "{kind:?} must show the message text"
            );
        }
    }

    #[test]
    fn job_kinds_match_the_status_bar_hues_when_colour_is_on() {
        let lit = Theme::with_color(true);
        // Success → green (health Healthy); failure → red; cancelled → yellow.
        // Same hues the StatusBar paints, so a toast and the status segment agree.
        assert_eq!(spans("j", ToastKind::Success, lit)[0].style.fg, Some(Color::Green));
        assert_eq!(spans("j", ToastKind::Failure, lit)[0].style.fg, Some(Color::Red));
        assert_eq!(spans("j", ToastKind::Cancelled, lit)[0].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn no_color_drops_every_hue_but_keeps_a_marker() {
        let dark = Theme::with_color(false);
        for kind in [
            ToastKind::Info,
            ToastKind::Error,
            ToastKind::Success,
            ToastKind::Failure,
            ToastKind::Cancelled,
        ] {
            for span in spans("j", kind, dark) {
                assert_eq!(span.style.fg, None, "{kind:?} must have no fg under NO_COLOR");
            }
        }
        // The kinds still differ without hue: success/failure stay bold, the glyph
        // itself carries cancelled, and info is dim.
        assert!(spans("j", ToastKind::Success, dark)[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(spans("j", ToastKind::Failure, dark)[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(spans("j", ToastKind::Info, dark)[0]
            .style
            .add_modifier
            .contains(Modifier::DIM));
    }
}

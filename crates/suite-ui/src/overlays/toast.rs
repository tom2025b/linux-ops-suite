//! Toast: a one-line transient flash (a status / notification line).

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::Theme;

/// Whether a toast reports a normal message or an error. An error toast uses the
/// failure style (red / bold) and a `[err]` marker so it stays distinguishable
/// under `NO_COLOR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
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
    /// Whether this is an informational or error toast.
    pub kind: ToastKind,
}

impl Toast<'_> {
    /// Draw the toast into `area` (typically a single-row status line).
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme) {
        let line = match self.kind {
            ToastKind::Info => Line::from(Span::styled(self.text.to_string(), theme.dim())),
            // Prepend a marker so an error still reads as one under NO_COLOR,
            // where the red hue drops away.
            ToastKind::Error => Line::from(vec![
                Span::styled("[err] ", theme.status_error()),
                Span::styled(self.text.to_string(), theme.status_error()),
            ]),
        };
        frame.render_widget(Paragraph::new(line), area);
    }
}

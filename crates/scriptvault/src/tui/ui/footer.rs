use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use suite_ui::{KeyHints, StatusBar};

use crate::tui::app::{App, KEY_HINTS, KEY_HINTS_SHORT};

/// Render the two-row footer: a status row (free-text message + the shared
/// job-status segment) and a persistent key-hint row.
///
/// The free-text status line stays ScriptVault's own — it carries arbitrary
/// transient messages ("copied path", "no result selected", errors) that the
/// suite's typed `StatusBar` doesn't model. The structured live-run status
/// (running / done ✓ / failed ✗) is the suite-ui `StatusBar`, drawn right of the
/// message on the same row. The hint row is suite-ui's `KeyHints`.
pub(super) fn render(frame: &mut Frame, app: &App, area: Rect, narrow: bool) {
    let theme = app.theme();
    let [status_area, hint_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);

    render_status_row(frame, app, status_area, theme);

    // Persistent key hints via the shared widget. The key glyph is accented and
    // the label dim — the same look the help overlay uses — from one source.
    let hints = if narrow { KEY_HINTS_SHORT } else { KEY_HINTS };
    KeyHints { hints }.render(frame, hint_area, theme);
}

/// The status row: ScriptVault's free-text message on the left, the shared
/// job-status segment on the right. Splitting the row keeps the message readable
/// while the job segment (`● running …` / `✓ … — done`) sits out of its way.
fn render_status_row(frame: &mut Frame, app: &App, area: Rect, theme: suite_ui::Theme) {
    // Give the job segment a fixed slice on the right; the message takes the rest.
    // 28 cols comfortably fits "● running <name> — done" for typical names and
    // clamps on narrow terminals (the message still owns the left).
    let job_w = 28u16.min(area.width / 2);
    let [msg_area, job_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(job_w)]).areas(area);

    let status_msg = app.status();
    let status_style = if status_is_error(status_msg) {
        theme.status_error()
    } else {
        Style::new().bold()
    };
    let status = Paragraph::new(Line::from(Span::styled(
        format!("  {status_msg}"),
        status_style,
    )));
    frame.render_widget(status, msg_area);

    // The shared job-status segment. `Idle` renders a dim "idle"; the renderer
    // maps ScriptVault's run model onto JobState in `App::job_state`.
    StatusBar {
        job: app.job_state(),
    }
    .render(frame, job_area, theme);
}

/// Classify status messages that should render as failures.
pub(super) fn status_is_error(msg: &str) -> bool {
    let m = msg.to_lowercase();
    [
        "error",
        "failed",
        "cannot",
        "not found",
        "skipped",
        "no result",
    ]
    .iter()
    .any(|kw| m.contains(kw))
}

//! Bottom status bar renderer.

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
};
use suite_ui::{KeyHints, Theme, truncate_path};

use crate::tui::app::TuiApp;

/// The persistent key hints, as `(key, label)` pairs for the shared `KeyHints`
/// widget. The full set on a wide terminal; a trimmed set when narrow so the
/// strip never overflows.
const HINTS_WIDE: &[(&str, &str)] = &[
    ("q", "quit"),
    ("↑↓/jk", "nav"),
    ("/", "filter"),
    ("r", "rescan"),
    ("d", "details"),
    ("l/m/h/c", "risk"),
    ("a", "all"),
    ("enter", "pick"),
    ("e", "export"),
    ("s", "sort"),
    ("?", "help"),
];
const HINTS_NARROW: &[(&str, &str)] = &[
    ("q", "quit"),
    ("jk", "nav"),
    ("/", "filt"),
    ("r", "res"),
    ("d", "det"),
    ("lmc", "risk"),
    ("enter", "pick"),
    ("?", "help"),
];

/// Bottom status bar: the shared key-hint strip, then a transient message or the
/// current view-state + selected path folded onto the same row.
pub(super) fn render_status(f: &mut Frame, app: &TuiApp, area: Rect, theme: Theme) {
    let hints = if area.width >= 90 {
        HINTS_WIDE
    } else {
        HINTS_NARROW
    };
    let mut parts = KeyHints { hints }.line(theme).spans;

    if let Some(msg) = &app.status_message {
        parts.push(Span::raw("  "));
        // Transient action result: the suite accent (italic) so it stands apart
        // from the dim hints without a bespoke hue.
        parts.push(Span::styled(
            msg.clone(),
            theme.accent_bar().add_modifier(Modifier::ITALIC),
        ));
    } else if app.filter_mode {
        parts.push(Span::styled(
            "  [typing filter — Esc cancel, Enter accept]",
            theme.accent_bar(),
        ));
    } else {
        parts.extend(view_state_spans(app, theme));
        parts.extend(selected_path_spans(app, theme));
    }

    let p = Paragraph::new(Line::from(parts));
    f.render_widget(p, area);
}

fn view_state_spans(app: &TuiApp, theme: Theme) -> Vec<Span<'static>> {
    let mut parts = Vec::new();
    let state = app.view_state_tokens();
    if !state.is_empty() {
        parts.push(Span::raw("  "));
        parts.push(Span::styled(
            format!("[{}]", state.join(" ")),
            theme.accent_bar().add_modifier(Modifier::ITALIC),
        ));
    }
    parts
}

fn selected_path_spans(app: &TuiApp, theme: Theme) -> Vec<Span<'static>> {
    let mut parts = Vec::new();
    if !app.filtered.is_empty()
        && let Some(&idx) = app.filtered.get(app.selected)
    {
        let full = app.entries[idx].entry.discovered.path.display().to_string();
        let short = truncate_path(&full, 50);
        parts.push(Span::raw("  "));
        parts.push(Span::styled(format!("sel:{}", short), theme.dim()));
    }
    parts
}

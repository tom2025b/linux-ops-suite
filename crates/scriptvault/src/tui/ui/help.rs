use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::tui::app::App;

/// Draw the keybinding modal over the current frame.
pub(super) fn render(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    // Tall enough for the full binding list (16 rows + frame) on a standard
    // 24-row terminal without clipping; width comfortable for the descriptions.
    let area = centered_rect(64, 80, frame.area());

    // Keep this in lockstep with the real key handling (app/mod.rs `handle_key`)
    // and the footer's `KEY_HINTS`. Every binding a user can press should appear
    // here — a stale help screen is worse than none.
    let rows = [
        ("type", "filter the list as you type"),
        ("↑ / ↓  ·  j / k", "move the selection"),
        ("PageUp / PageDown", "move by a page"),
        (
            "Home / End  ·  g / G",
            "jump to first / last (g/G when query empty)",
        ),
        (
            "A / F / R",
            "view: all / ★ favorites / recents (when query empty)",
        ),
        (
            "Enter",
            "action menu: ↑/↓ move, Enter or 1–4 pick (open/edit · run · delete · cancel)",
        ),
        ("Ctrl-R", "run the selected script (direct shortcut)"),
        ("Ctrl-Y", "copy the script's path (yank)"),
        ("Ctrl-O", "print the path on exit (pipeable)"),
        ("Ctrl-F", "toggle ★ favorite on the selected script"),
        ("Ctrl-L", "toggle the live/last output pane"),
        (
            "Shift-PageUp / PageDown",
            "scroll the output pane (when shown)",
        ),
        ("Ctrl-U", "clear the query"),
        (
            "Ctrl-P  ·  :",
            "open the command palette (playlists, reload, …)",
        ),
        ("t:foo / c:bar", "filter by tag or category (in query)"),
        ("?", "toggle this help"),
        (
            "c / q",
            "close menus & pickers (help, actions, confirm, playlist/saved-search)",
        ),
        (
            "Esc",
            "close any overlay — including text fields (palette, save-name, editor)",
        ),
        ("Ctrl-C", "quit the whole app (works from anywhere)"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(key, description)| {
            Line::from(vec![
                Span::styled(format!("{key:>18}  "), theme.prompt()),
                Span::raw((*description).to_string()),
            ])
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.accent_bar())
        .padding(Padding::uniform(1))
        .title(" Keybindings ")
        .title_alignment(Alignment::Center);

    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn centered_rect(pct_w: u16, pct_h: u16, area: Rect) -> Rect {
    let [_, mid_v, _] = Layout::vertical([
        Constraint::Percentage((100 - pct_h) / 2),
        Constraint::Percentage(pct_h),
        Constraint::Percentage((100 - pct_h) / 2),
    ])
    .areas(area);
    let [_, mid, _] = Layout::horizontal([
        Constraint::Percentage((100 - pct_w) / 2),
        Constraint::Percentage(pct_w),
        Constraint::Percentage((100 - pct_w) / 2),
    ])
    .areas(mid_v);
    mid
}

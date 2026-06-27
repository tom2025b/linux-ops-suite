//! Help popup renderer — the suite's shared `HelpSheet`.

use ratatui::Frame;
use ratatui::layout::Rect;
use suite_ui::{HelpSheet, Theme};

use crate::tui::app::TuiApp;

/// The keybinding rows shown in the help overlay. Kept in lockstep with the real
/// key handling in `event_loop.rs`. The two trailing rows (empty key column) are
/// explanatory notes rather than bindings. The active sort/filter/risk state is
/// intentionally NOT repeated here — it already lives in the footer status bar.
const HELP_ROWS: &[(&str, &str)] = &[
    ("↑/k · ↓/j", "Move selection"),
    ("g/Home · G/End", "Jump to top / bottom"),
    ("/", "Enter live filter mode"),
    ("Esc", "Clear text filter, then risk filter (progressive)"),
    ("Enter", "Commit filter / pick item (prints path, quits)"),
    ("r", "Rescan (pick up new files)"),
    ("d", "Toggle details pane (full-width table)"),
    ("l/m/h/c", "Quick filter by risk (Low/Med/High/Crit)"),
    ("a · 0", "Clear risk filter (show all)"),
    ("PgUp/PgDn · space", "Page up / down through long lists"),
    ("e", "Export filtered view to bulwark-tui-export.json"),
    ("s", "Cycle sort (path / risk / size)"),
    ("? · F1", "Toggle this help"),
    ("q · Ctrl-C", "Quit (restores terminal)"),
    ("", "Filter matches path or description (case-insensitive)."),
    (
        "",
        "Sidecar data (.bulwark.yaml) shows in the details pane.",
    ),
];

/// Centered help popup, drawn with the suite's shared `HelpSheet` (which owns the
/// centering, `Clear`, rounded accent border, and key-column alignment).
pub(super) fn render_help_popup(f: &mut Frame, _app: &TuiApp, area: Rect, theme: Theme) {
    HelpSheet {
        title: "Bulwark TUI — Key Bindings",
        rows: HELP_ROWS,
    }
    .render(f, area, theme);
}

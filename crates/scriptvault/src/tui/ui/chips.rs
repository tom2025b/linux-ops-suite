use ratatui::Frame;
use ratatui::layout::Rect;
use suite_ui::FilterChips;

use crate::tui::app::App;

/// Render the active-filter chip row (`[t:ci ✕] [lang:bash ✕]`) just under the
/// search box. Draws nothing when there are no active filters, so the default
/// (no-filter) layout is visually unchanged. Each chip shows the operator the
/// user typed; Backspace on empty text pops the last one (handled in input).
///
/// The chip *look* comes from the shared `suite_ui::FilterChips` widget; the
/// PLACEMENT stays here — the chips overlay the search pane's bottom border row
/// (`search_area`'s last line) so no extra vertical layout slot is needed, which
/// keeps the main layout and its tests stable.
pub(super) fn render(frame: &mut Frame, app: &App, search_area: Rect) {
    let chips = app.active_chips();
    if chips.is_empty() || search_area.height < 2 {
        return;
    }
    // `active_chips` owns its Strings; the widget borrows &str, so take a view.
    let labels: Vec<&str> = chips.iter().map(String::as_str).collect();

    // Bottom border row of the search pane.
    let row = Rect {
        x: search_area.x + 1,
        y: search_area.y + search_area.height.saturating_sub(1),
        width: search_area.width.saturating_sub(2),
        height: 1,
    };
    FilterChips { labels: &labels }.render(frame, row, app.theme());
}

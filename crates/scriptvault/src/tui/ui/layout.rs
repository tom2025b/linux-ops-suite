use ratatui::layout::{Constraint, Layout, Rect};

// The rounded pane and the centering helpers are shared suite chrome now — they
// come straight from `suite-ui`, re-exported here so callers keep writing
// `super::layout::{pane, centered_rect, centered_fixed}` unchanged. Only the
// ScriptVault-specific layout MATH (the three-pane responsive split) stays local.
pub(crate) use suite_ui::{centered_fixed, centered_rect, pane};

/// Compute main layout areas responsively.
pub(crate) fn layout_areas(area: Rect) -> (Rect, Rect, Rect, Rect, bool) {
    let narrow = area.width < 72;
    let search_h = if area.height < 12 { 1 } else { 3 };
    let footer_h = 2u16;

    let [search, body, footer] = Layout::vertical([
        Constraint::Length(search_h),
        Constraint::Fill(1),
        Constraint::Length(footer_h),
    ])
    .areas(area);

    let list_c = if narrow {
        Constraint::Min(30)
    } else {
        Constraint::Percentage(42)
    };
    let [list_area, preview_area] = Layout::horizontal([list_c, Constraint::Fill(1)]).areas(body);

    (search, list_area, preview_area, footer, narrow)
}

/// Returns the outer results-list Rect used by render_list.
pub(crate) fn list_rect(area: Rect) -> Rect {
    let (_, list, _, _, _) = layout_areas(area);
    list
}

// tui/ui.rs — pure rendering dispatcher for &App.
// -----------------------------------------------------------------------------
// The actual panes live in focused ui/* modules. This file owns only the frame
// layout sequence and overlay dispatch so it stays small enough to audit.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};

use super::app::{App, Mode};

mod chips;
mod footer;
mod help;
mod layout;
mod list;
mod output;
mod overlays;
mod preview;
mod search;

use layout::layout_areas;
pub(crate) use layout::list_rect;

#[cfg(test)]
use super::theme::Theme;
#[cfg(test)]
use footer::status_is_error;
#[cfg(test)]
use list::{highlight_spans, name_indices};
#[cfg(test)]
use overlays::menu_title;
#[cfg(test)]
use scriptvault_core::{MatchField, SearchResult};

/// Height (in rows) of the output pane when shown — it splits the bottom off the
/// preview column. 9 rows fits a useful tail without crowding the preview.
const OUTPUT_PANE_HEIGHT: u16 = 9;

/// Render the whole UI for the current `App` state.
pub fn render(frame: &mut Frame, app: &App) {
    let (search_area, list_area, preview_area, footer_area, narrow) = layout_areas(frame.area());

    search::render(frame, app, search_area);
    chips::render(frame, app, search_area);
    list::render(frame, app, list_area);

    let (preview_area, output_area) = if app.is_showing_output() {
        let chunks: [Rect; 2] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(OUTPUT_PANE_HEIGHT)])
                .areas(preview_area);
        (chunks[0], Some(chunks[1]))
    } else {
        (preview_area, None)
    };

    preview::render(frame, app, preview_area, narrow);
    if let Some(area) = output_area {
        output::render(frame, app, area);
    }

    footer::render(frame, app, footer_area, narrow);
    render_overlay(frame, app);
}

fn render_overlay(frame: &mut Frame, app: &App) {
    match app.mode() {
        Mode::Help => help::render(frame, app),
        Mode::CommandPalette => overlays::render_command_palette(frame, app),
        Mode::EditMetadata => overlays::render_edit_metadata(frame, app),
        Mode::PlaylistPicker => overlays::render_playlist_picker(frame, app),
        Mode::ActionMenu => overlays::render_action_menu(frame, app),
        Mode::ConfirmDelete => overlays::render_confirm_delete(frame, app),
        Mode::SaveSearchName => overlays::render_save_search_name(frame, app),
        Mode::SavedSearchPicker => overlays::render_saved_search_picker(frame, app),
        Mode::Search => {}
    }
}

// ============================================================================
#[cfg(test)]
mod tests;

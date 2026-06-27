//! Rendering for the Bulwark TUI cockpit.
//!
//! This module owns frame layout and delegates each visible region to a focused
//! renderer file. It performs no I/O, event handling, or application mutation.

mod details;
mod header;
mod help;
mod risk;
mod status;
mod table;
mod text;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use super::app::TuiApp;
use details::render_details;
use header::render_header;
use help::render_help_popup;
use status::render_status;
use table::render_table;

/// Main render entry. Called once per frame from the event loop.
pub fn render_ui(f: &mut Frame, app: &TuiApp) {
    // The resolved palette (honours NO_COLOR), read once and handed to each
    // region renderer so every style routes through the shared suite-ui gate.
    let theme = app.theme();
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(size);

    render_header(f, app, chunks[0], theme);

    let main_area = chunks[1];
    if app.show_details {
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Min(36)])
            .split(main_area);

        render_table(f, app, main_chunks[0], theme);
        render_details(f, app, main_chunks[1], theme);
    } else {
        render_table(f, app, main_area, theme);
    }

    render_status(f, app, chunks[2], theme);

    if app.show_help {
        render_help_popup(f, app, size, theme);
    }
}

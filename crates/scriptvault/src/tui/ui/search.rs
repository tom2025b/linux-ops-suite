use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph};

use crate::tui::app::{App, ViewMode};

use super::layout::pane;

/// Render the search bar with a right-aligned status strip (View · Sort · count).
pub(super) fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let line = Line::from(vec![
        Span::styled(" > ", theme.prompt()),
        Span::raw(app.query().to_string()),
        Span::styled("█", theme.dim()),
    ]);
    let block = if area.height <= 1 {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(theme.dim())
            .padding(Padding::horizontal(1))
    } else {
        pane("ScriptVault", theme)
    };

    // The status strip (`All · Auto · 312`) sits on the same line, right-aligned,
    // rendered over the block's inner area so it lines up with the prompt. It is
    // always visible so the active view/sort and result count are glanceable.
    let inner = block.inner(area);
    frame.render_widget(Paragraph::new(line).block(block), area);

    let strip = status_strip_text(app.view_mode(), app.results().len());
    let strip_line = Line::from(Span::styled(strip, theme.dim()));
    frame.render_widget(
        Paragraph::new(strip_line).alignment(Alignment::Right),
        inner,
    );
}

/// Format the status strip shown on the search line: `View · Sort · count`.
/// Sort is `Auto` until a sort toggle exists (P3); rendering it now keeps the
/// strip stable. Pure so it is unit-testable without a terminal.
pub(super) fn status_strip_text(view: ViewMode, count: usize) -> String {
    format!("{} · Auto · {}", view_label(view), count)
}

fn view_label(view: ViewMode) -> &'static str {
    match view {
        ViewMode::All => "All",
        ViewMode::Favorites => "Favorites",
        ViewMode::Recents => "Recents",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strip_shows_view_sort_count() {
        assert_eq!(status_strip_text(ViewMode::All, 312), "All · Auto · 312");
        assert_eq!(
            status_strip_text(ViewMode::Favorites, 0),
            "Favorites · Auto · 0"
        );
        assert_eq!(
            status_strip_text(ViewMode::Recents, 7),
            "Recents · Auto · 7"
        );
    }
}

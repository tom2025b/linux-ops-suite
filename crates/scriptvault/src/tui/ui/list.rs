use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use scriptvault_core::{MatchField, SearchResult};

use crate::tui::app::{App, ViewMode};
use crate::tui::theme::Theme;

use super::layout::pane;

/// Render the results list with selected-row highlighting.
pub(super) fn render(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let show_badge = !app.query().trim().is_empty();
    let selected = app.selected();
    let items: Vec<ListItem> = app
        .results()
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let fav = app.is_favorite(&r.entry.path);
            let is_selected = selected == Some(i);
            let hint = app.run_hint_for(&r.entry.path);
            ListItem::new(list_line(r, show_badge, fav, is_selected, hint, theme))
        })
        .collect();

    let mut title = match app.view_mode() {
        ViewMode::All => format!("results ({})", app.results().len()),
        ViewMode::Favorites => format!("★ favorites ({})", app.results().len()),
        ViewMode::Recents => format!("recents ({})", app.results().len()),
    };
    if let Some(pl) = app.active_playlist() {
        title = format!("{} [{}]", title, pl);
    }

    let block = pane(&title, theme);

    // Empty state: an empty bordered box with just "(0)" leaves the user guessing
    // why nothing is there. Render a centered, context-aware hint inside the same
    // pane instead — distinguishing "no match for your query" from "this view is
    // empty" from "nothing indexed at all". Layout and the title count are
    // preserved; only the body changes.
    if app.results().is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines = empty_message(app, theme);
        let para = Paragraph::new(Text::from(lines))
            .alignment(Alignment::Center)
            .style(theme.dim());
        // Nudge the message toward the vertical middle of the pane.
        let y = inner.y + inner.height.saturating_sub(lines_height()) / 2;
        let centered = Rect {
            y,
            height: inner.height.saturating_sub(y - inner.y),
            ..inner
        };
        frame.render_widget(para, centered);
        return;
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(theme.selection());

    let mut state = ListState::default();
    state.select(app.selected());
    frame.render_stateful_widget(list, area, &mut state);
}

/// Number of text rows `empty_message` produces — used to vertically center it.
fn lines_height() -> u16 {
    2
}

/// A two-line, context-aware empty-state message. The first line names the
/// situation; the second offers the most useful next action for that situation.
fn empty_message(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let querying = !app.query().trim().is_empty();
    let (headline, hint): (String, &str) = if querying {
        (
            format!("no matches for “{}”", app.query().trim()),
            "press Ctrl-U to clear the query, or refine it",
        )
    } else if let Some(pl) = app.active_playlist() {
        (
            format!("playlist “{pl}” is empty"),
            "add scripts to it from the action menu (Enter)",
        )
    } else {
        match app.view_mode() {
            ViewMode::Favorites => (
                "no favorites yet".to_string(),
                "press Ctrl-F on a script to star it",
            ),
            ViewMode::Recents => (
                "nothing run yet".to_string(),
                "run a script (Ctrl-R) to build your history",
            ),
            ViewMode::All => (
                "no scripts found".to_string(),
                "check your roots in ~/.config/scriptvault/config.yaml",
            ),
        }
    };
    vec![
        Line::from(Span::styled(headline, theme.title())),
        Line::from(Span::styled(hint.to_string(), theme.dim())),
    ]
}

fn list_line(
    r: &SearchResult,
    show_badge: bool,
    favorite: bool,
    is_selected: bool,
    run_hint: Option<String>,
    theme: Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    if is_selected {
        spans.push(Span::styled("▌ ", theme.selected_rail()));
    } else {
        spans.push(Span::raw("  "));
    }

    if favorite {
        spans.push(Span::styled("★ ", theme.match_label(Color::Yellow)));
    } else {
        spans.push(Span::raw("  "));
    }

    spans.extend(highlight_spans(
        r.entry.display_name(),
        name_indices(r),
        theme,
    ));

    if show_badge {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("·{}", match_field_label(r.matched_field)),
            theme.match_label(match_field_color(r.matched_field)),
        ));
    }

    if !r.entry.meta.tags.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("[{}]", r.entry.meta.tags.join(", ")),
            theme.dim(),
        ));
    }

    if let Some(risk) = r
        .entry
        .meta
        .tags
        .iter()
        .find(|t| t.to_lowercase().starts_with("risk:"))
    {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("[{}]", risk),
            theme.match_label(Color::Red),
        ));
    }

    // Compact run-history hint (`▲12× 2h ✓`), dimmed and only when the script has
    // been run before. Formatted in core so every frontend shows the same badge.
    if let Some(hint) = run_hint {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(hint, theme.dim()));
    }
    Line::from(spans)
}

/// Split text into styled spans, bolding matched character positions.
pub(super) fn highlight_spans(text: &str, matched: &[usize], theme: Theme) -> Vec<Span<'static>> {
    let matched_set: std::collections::HashSet<usize> = matched.iter().copied().collect();
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let hit = matched_set.contains(&i);
        let start = i;
        while i < chars.len() && matched_set.contains(&i) == hit {
            i += 1;
        }
        let run: String = chars[start..i].iter().collect();
        let style = if hit {
            theme.match_text()
        } else {
            Style::new()
        };
        spans.push(Span::styled(run, style));
    }
    if spans.is_empty() {
        spans.push(Span::raw(text.to_string()));
    }
    spans
}

/// Match indices that apply to the displayed name.
pub(super) fn name_indices(r: &SearchResult) -> &[usize] {
    let shows_filename_as_name = r.entry.meta.name.is_none();
    let applies = match r.matched_field {
        MatchField::Name => true,
        MatchField::Filename => shows_filename_as_name,
        MatchField::Tags | MatchField::Desc => false,
    };
    if applies { &r.matched_indices } else { &[] }
}

fn match_field_label(field: MatchField) -> &'static str {
    match field {
        MatchField::Name => "name",
        MatchField::Tags => "tags",
        MatchField::Desc => "desc",
        MatchField::Filename => "file",
    }
}

fn match_field_color(field: MatchField) -> Color {
    match field {
        MatchField::Name => Color::Green,
        MatchField::Tags => Color::Cyan,
        MatchField::Desc => Color::Yellow,
        MatchField::Filename => Color::Magenta,
    }
}

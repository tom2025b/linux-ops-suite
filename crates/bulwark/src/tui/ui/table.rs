//! Results table renderer.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Cell, Row, Table, TableState},
};
use suite_ui::{Counted, Theme, pane_titled, truncate_path};

use crate::app::ClassifiedEntry;
use crate::tui::app::{SortMode, TuiApp};

use super::risk::colored_risk_cell;

/// The main results table.
pub(super) fn render_table(f: &mut Frame, app: &TuiApp, area: Rect, theme: Theme) {
    let widths = [
        Constraint::Min(38),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(7),
    ];

    let (path_h, lang_h, risk_h, cat_h, own_h, size_h) = match app.sort {
        SortMode::Path => ("PATH ▲", "LANG", "RISK", "CATEGORY", "OWNER", "SIZE"),
        SortMode::Risk => ("PATH", "LANG", "RISK ▼", "CATEGORY", "OWNER", "SIZE"),
        SortMode::Size => ("PATH", "LANG", "RISK", "CATEGORY", "OWNER", "SIZE ▼"),
    };

    // Column headers: the suite's dim neutral, kept bold so the header row still
    // reads as a header. Dim is an attribute, so it survives NO_COLOR.
    let header_style = theme.dim().add_modifier(Modifier::BOLD);
    let header_cells = [path_h, lang_h, risk_h, cat_h, own_h, size_h]
        .into_iter()
        .map(|h| Cell::from(h).style(header_style));

    let header = Row::new(header_cells).bottom_margin(0);

    let rows: Vec<Row> = app
        .filtered
        .iter()
        .map(|&idx| row_for_entry(&app.entries[idx], theme))
        .collect();

    let mut table_state = TableState::default();
    table_state.select(Some(app.selected));

    let count = Counted {
        shown: app.filtered.len(),
        total: app.entries.len(),
    };

    let sort_label = match app.sort {
        SortMode::Path => "path",
        SortMode::Risk => "risk (high first)",
        SortMode::Size => "size (largest first)",
    };

    // A composite title — a `Counted` span for the narrow-aware "N of M" count,
    // the rest in the shared title style — handed to `pane_titled` so the pane
    // chrome (rounded border, dim neutral, 1-col padding) comes from one place.
    let title_line = Line::from(vec![
        Span::styled(" results (", theme.title()),
        count.span(theme),
        Span::styled(format!(", sorted by {}) ", sort_label), theme.title()),
    ]);
    let block = pane_titled(title_line, theme);

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(theme.selection())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, area, &mut table_state);
}

fn row_for_entry(entry: &ClassifiedEntry, theme: Theme) -> Row<'static> {
    let path = entry.entry.discovered.path.display().to_string();
    // The PATH column is 38 wide at minimum but can stretch; cap the text at 42
    // and keep the tail (the filename is what matters when scanning), via the
    // shared single-`…` truncation so the cell lines up with every other tool.
    let path_short = truncate_path(&path, 42);

    let lang = format!("{:?}", entry.entry.language);
    let risk = format!("{:?}", entry.classification.risk);
    let risk_cell = colored_risk_cell(&risk, entry.classification.risk, theme);

    let cat = entry.classification.category.clone();
    let owner = entry.classification.owner.clone();
    let size = crate::core::report::human_size(entry.entry.discovered.size);

    Row::new(vec![
        Cell::from(path_short),
        Cell::from(lang),
        risk_cell,
        Cell::from(cat),
        Cell::from(owner),
        Cell::from(size),
    ])
}

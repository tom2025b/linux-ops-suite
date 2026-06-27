//! Header/dashboard renderer.

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
};
use suite_ui::{Counted, Theme, pane};

use crate::app::RiskLevel;
use crate::tui::app::{SortMode, TuiApp};
use crate::tui::ui::risk::RiskStyle;

/// Top dashboard: title, quick stats, filter/risk/sort indicators.
pub(super) fn render_header(f: &mut Frame, app: &TuiApp, area: Rect, theme: Theme) {
    let total = app.entries.len();
    let shown = app.filtered.len();
    let (low, med, high, crit) = count_risks(app);

    let title = Span::styled(
        format!(" Bulwark {} — Interactive Inventory ", crate::VERSION),
        theme.title(),
    );

    // Per-risk counts share the single RiskStyle gate with the table, so the
    // dashboard's "low/med/high/crit" colours match the cells exactly (and drop
    // to colourless under NO_COLOR together).
    let mut stats = vec![
        Span::raw("  "),
        Span::styled(format!("{low} low"), theme.risk(RiskLevel::Low)),
        Span::raw("  "),
        Span::styled(format!("{med} med"), theme.risk(RiskLevel::Medium)),
        Span::raw("  "),
        Span::styled(format!("{high} high"), theme.risk(RiskLevel::High)),
        Span::raw("  "),
        Span::styled(format!("{crit} crit"), theme.risk(RiskLevel::Critical)),
        Span::styled("   |   showing ", theme.dim()),
        // The shared narrow-aware count: accented+italic when filtered, dim when
        // the view is full — the same span the table title carries.
        Counted { shown, total }.span(theme),
    ];

    // Active-view indicators use the suite accent (italic) instead of the old
    // bespoke magenta, so the emphasis matches the rest of the suite chrome and
    // honours NO_COLOR via the accent's gate.
    let indicator = theme.accent_bar().add_modifier(Modifier::ITALIC);

    if !app.filter.is_empty() {
        stats.push(Span::styled(
            format!("   [filter: {}]", app.filter),
            indicator,
        ));
    }

    if let Some(r) = app.risk_filter {
        stats.push(Span::styled(format!("   [risk: {:?}]", r), indicator));
    }

    if app.sort != SortMode::Path {
        let label = match app.sort {
            SortMode::Risk => "risk",
            SortMode::Size => "size",
            SortMode::Path => "path",
        };
        stats.push(Span::styled(format!("   [sort: {}]", label), indicator));
    }

    let header_line = Line::from(vec![title].into_iter().chain(stats).collect::<Vec<_>>());

    let para = Paragraph::new(header_line).block(pane("cockpit", theme));
    f.render_widget(para, area);
}

fn count_risks(app: &TuiApp) -> (usize, usize, usize, usize) {
    let mut low = 0;
    let mut med = 0;
    let mut high = 0;
    let mut crit = 0;

    for &idx in &app.filtered {
        match app.entries[idx].classification.risk {
            RiskLevel::Low => low += 1,
            RiskLevel::Medium => med += 1,
            RiskLevel::High => high += 1,
            RiskLevel::Critical => crit += 1,
        }
    }
    (low, med, high, crit)
}

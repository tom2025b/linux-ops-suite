//! Details pane renderer.

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use suite_ui::{EmptyState, Theme, pane};

use crate::tui::app::TuiApp;

use super::risk::colored_risk_span;
use super::text::textwrap;

/// Right-hand details pane for the currently selected item.
pub(super) fn render_details(f: &mut Frame, app: &TuiApp, area: Rect, theme: Theme) {
    let block = pane("details", theme);

    if app.filtered.is_empty() {
        if app.entries.is_empty() {
            // Nothing was found at all: keep the richer, actionable guidance as a
            // hand-written block — EmptyState's single-hint shape can't carry the
            // three bulleted next-steps, and dropping them would hurt a first run.
            let msg = "No items found.\n\n\
                 • Check your scan paths with `bulwark config-check`\n\
                 • Press 'r' to rescan after adding files\n\
                 • Use `bulwark tui ~/your/bin` to target a specific directory";
            let p = Paragraph::new(msg).block(block).wrap(Wrap { trim: true });
            f.render_widget(p, area);
        } else {
            // Items exist but the filter hid them all: the shared, centered
            // placeholder. Frame the pane first, then draw the text into its
            // interior (EmptyState draws text only, assuming a framed region).
            let inner = block.inner(area);
            f.render_widget(block, area);
            EmptyState {
                message: "No items match the current filter.",
                hint: Some("Press Esc to clear (progressive: text then risk)."),
            }
            .render(f, inner, theme);
        }
        return;
    }

    let idx = app.filtered[app.selected];
    let e = &app.entries[idx];
    let d = &e.entry.discovered;
    let c = &e.classification;

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::raw("path: "),
            Span::styled(d.path.display().to_string(), theme.accent_bar()),
        ]),
        Line::from(format!("language: {:?}", e.entry.language)),
        Line::from(vec![
            Span::raw("risk: "),
            colored_risk_span(&format!("{:?}", c.risk), c.risk, theme),
        ]),
        Line::from(format!("category: {}", c.category)),
        Line::from(format!("owner: {}", c.owner)),
        Line::from(format!(
            "size: {} ({})",
            crate::core::report::human_size(d.size),
            d.size
        )),
        Line::from(format!("executable: {}", d.is_executable)),
    ];

    if let Some(desc) = &e.entry.description {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "description:",
            theme.dim().add_modifier(Modifier::UNDERLINED),
        )));
        for chunk in textwrap(desc, 34) {
            lines.push(Line::from(chunk));
        }
    }

    if let Some(sc) = &e.entry.sidecar {
        lines.push(Line::raw(""));
        // The sidecar header uses the suite accent (bold) in place of the old
        // bespoke magenta, so emphasis matches the rest of the chrome.
        lines.push(Line::from(Span::styled(
            "sidecar (.bulwark.yaml)",
            theme.title(),
        )));
        if let Some(sd) = &sc.description {
            lines.push(Line::from(format!("  desc: {}", sd)));
        }
        if !sc.tags.is_empty() {
            lines.push(Line::from(format!("  tags: {}", sc.tags.join(", "))));
        }
        if let Some(r) = &sc.risk {
            lines.push(Line::from(format!("  risk (suggested): {}", r)));
        }
        if let Some(cat) = &sc.category {
            lines.push(Line::from(format!("  category (suggested): {}", cat)));
        }
        if let Some(o) = &sc.owner {
            lines.push(Line::from(format!("  owner (suggested): {}", o)));
        }
    }

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    f.render_widget(para, area);
}

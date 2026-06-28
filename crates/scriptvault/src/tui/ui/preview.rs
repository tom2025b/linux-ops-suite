use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use scriptvault_core::{MatchField, ScriptEntry};

use crate::tui::app::App;
use crate::tui::theme::Theme;

use super::layout::pane;

const PREVIEW_LINES: usize = 40;

/// Render the parsed metadata and bounded file preview.
pub(super) fn render(frame: &mut Frame, app: &App, area: Rect, narrow: bool) {
    let theme = app.theme();
    let block = pane("preview", theme);

    let text = if narrow && area.width < 35 {
        Text::from(Line::from(Span::styled(
            "preview needs more width",
            theme.dim(),
        )))
    } else {
        let matched = if app.query().trim().is_empty() {
            None
        } else {
            app.selected_result().map(|r| r.matched_field)
        };

        match app.selected_result() {
            Some(r) => {
                let rec = app.recency_summary(&r.entry.path);
                let note = app.note_for(&r.entry.path);
                let last_out = app.last_output_for(&r.entry.path);
                let inner_w = area.width.saturating_sub(4) as usize;
                preview_text(
                    &r.entry,
                    matched,
                    PreviewMeta {
                        recency: rec.as_deref(),
                        note,
                        last_out,
                    },
                    inner_w,
                    theme,
                )
            }
            None => Text::from(Line::from(Span::styled("no result selected", theme.dim()))),
        }
    };

    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

/// The optional, per-entry annotations the preview pane renders alongside the
/// script's own metadata. Bundled into one struct so `preview_text` stays under
/// clippy's argument-count limit and the related fields travel together.
struct PreviewMeta<'a> {
    recency: Option<&'a str>,
    note: Option<&'a str>,
    last_out: Option<&'a str>,
}

fn preview_text(
    entry: &ScriptEntry,
    matched: Option<MatchField>,
    meta: PreviewMeta,
    inner_w: usize,
    theme: Theme,
) -> Text<'static> {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(meta_line(
        "name",
        entry.display_name(),
        Some(MatchField::Name),
        matched,
        theme,
    ));
    if let Some(desc) = entry.meta.desc.as_deref() {
        lines.push(meta_line(
            "desc",
            desc,
            Some(MatchField::Desc),
            matched,
            theme,
        ));
    }
    if !entry.meta.tags.is_empty() {
        lines.push(meta_line(
            "tags",
            &entry.meta.tags.join(", "),
            Some(MatchField::Tags),
            matched,
            theme,
        ));
    }
    if let Some(risk) = entry
        .meta
        .tags
        .iter()
        .find(|t| t.to_lowercase().starts_with("risk:"))
    {
        lines.push(Line::from(Span::styled(
            format!("  risk: {}", risk),
            theme.match_label(Color::Red),
        )));
    }
    if let Some(usage) = entry.meta.usage.as_deref() {
        lines.push(meta_line("usage", usage, None, matched, theme));
    }
    if let Some(cat) = entry.meta.category.as_deref() {
        lines.push(meta_line("category", cat, None, matched, theme));
    }
    lines.push(meta_line("lang", entry.lang.label(), None, matched, theme));
    lines.push(meta_line(
        "path",
        &entry.path.display().to_string(),
        Some(MatchField::Filename),
        matched,
        theme,
    ));

    if let Some(r) = meta.recency {
        lines.push(Line::from(Span::styled(
            format!("  last: {}", r),
            theme.dim(),
        )));
    }
    if let Some(n) = meta.note
        && !n.trim().is_empty()
    {
        lines.push(Line::from(Span::styled(
            format!("  note: {}", n),
            theme.dim(),
        )));
    }
    if let Some(o) = meta.last_out
        && !o.trim().is_empty()
    {
        lines.push(Line::from(Span::styled(
            format!("  out: {}", o),
            theme.dim(),
        )));
    }

    let rule: String = "─".repeat(inner_w.clamp(6, 200));
    lines.push(Line::from(Span::styled(rule, theme.dim())));

    match read_head(&entry.path, PREVIEW_LINES) {
        Ok(file_lines) => {
            for line in crate::tui::highlight::highlight_lines(&file_lines, entry.lang, theme) {
                lines.push(line);
            }
        }
        Err(_) => {
            lines.push(Line::from(Span::styled("⟨unreadable⟩", theme.dim())));
        }
    }

    Text::from(lines)
}

fn meta_line(
    key: &str,
    value: &str,
    field: Option<MatchField>,
    matched: Option<MatchField>,
    theme: Theme,
) -> Line<'static> {
    let is_match = field.is_some() && field == matched;
    let value_style = if is_match {
        Style::new().bold()
    } else {
        Style::new()
    };
    Line::from(vec![
        Span::styled(format!("{key:>8}: "), theme.meta_key()),
        Span::styled(value.to_string(), value_style),
    ])
}

fn read_head(path: &Path, max: usize) -> std::io::Result<Vec<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::with_capacity(max);
    for line in reader.lines().take(max) {
        match line {
            Ok(l) => out.push(l),
            Err(_) => break,
        }
    }
    Ok(out)
}

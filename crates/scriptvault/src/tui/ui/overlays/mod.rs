use ratatui::Frame;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};

use crate::tui::app::App;

use super::layout::{centered_fixed, centered_rect};

mod widgets;
use widgets::{
    PickerRow, PickerSpec, compact_modal_frame, modal_frame, render_picker, selection_prefix,
};

pub(super) fn render_action_menu(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    // Compact: inner height = 6 rows (8 − 2 border) for the title, four options,
    // and the dim hint line — no blank spacers. The digit column gives the rows
    // enough structure that they read cleanly without padding. Width is sized so
    // the full hint line ("↑/↓: move · Enter / 1–4: pick · Esc / c / q: cancel")
    // fits on one row without clipping.
    let area = centered_fixed(60, 8, frame.area());
    let name = app
        .selected_result()
        .map(|r| r.entry.display_name().to_string())
        .unwrap_or_default();

    // The action menu is the one overlay that ISN'T a plain list picker: it has a
    // bold per-script subtitle above the rows, an accent digit + (red on delete)
    // label per row, and a compact `horizontal(1)` padding so the fixed 8-row box
    // doesn't clip its hint line. So it builds its own lines here. It still shares
    // the `› `/selection-prefix convention (via `selection_prefix`) so arrow
    // navigation reads identically to the list pickers.
    let sel = app.action_menu_selected();
    let labels = ["Open / edit", "Run", "Delete file", "Cancel"];

    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        menu_title(&name),
        Style::new().bold(),
    ))];
    for (i, label) in labels.iter().enumerate() {
        let selected = i == sel;
        let (prefix, prefix_style) = selection_prefix(selected, &theme);
        // The delete row (index 2) flags only its LABEL red as the destructive
        // choice; the highlighted row otherwise uses the shared selection style.
        let label_style = if i == 2 {
            theme.status_error()
        } else if selected {
            theme.selection()
        } else {
            Style::new()
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, prefix_style),
            Span::styled(format!("{}", i + 1), theme.prompt()),
            Span::styled(format!("  {label}"), label_style),
        ]));
    }
    // A dim hint line in the shared `key: action  ·  key: action` form. Arrow nav
    // + Enter is the primary path; digits 1–4 remain direct picks. `c`/`q` are
    // advertised alongside Esc because a lone Esc can stall in the terminal's
    // escape-sequence parser; the single-byte keys never do.
    lines.push(Line::from(Span::styled(
        "↑/↓: move  ·  Enter / 1–4: pick  ·  Esc / c / q: cancel",
        theme.dim(),
    )));

    // Its own compact frame: `horizontal(1)` padding (NOT the shared uniform
    // padding) keeps the fixed 8-row box from clipping the hint line.
    let block = compact_modal_frame("Actions", &theme);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

/// The y/n gate in front of a staged delete — the suite's shared `ConfirmModal`
/// (same widget RexOps uses), so a pending destructive action looks the same
/// everywhere in the suite. App owns the y/n keys; this only draws the prompt.
pub(super) fn render_confirm_delete(frame: &mut Frame, app: &App) {
    let name = app
        .selected_result()
        .map(|r| r.entry.display_name().to_string())
        .unwrap_or_default();
    suite_ui::ConfirmModal {
        title: "Delete file",
        message: &format!("Delete {name} from disk?"),
    }
    .render(frame, frame.area(), app.theme());
}

pub(super) fn menu_title(name: &str) -> String {
    const MAX_NAME: usize = 30;
    if name.chars().count() <= MAX_NAME {
        return format!("For {name}");
    }

    let mut short: String = name.chars().take(MAX_NAME.saturating_sub(1)).collect();
    short.push('…');
    format!("For {short}")
}

pub(super) fn render_command_palette(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    let area = centered_rect(55, 55, frame.area());

    let cmds = crate::tui::app::palette_commands();
    let filtered: Vec<_> = if app.palette_query().is_empty() {
        cmds
    } else {
        let q = app.palette_query().to_lowercase();
        cmds.into_iter()
            .filter(|c| c.label.to_lowercase().contains(&q) || c.desc.to_lowercase().contains(&q))
            .collect()
    };

    // The palette is NOT a plain title+hint picker — it leads with a live search
    // input line and a "— commands —" header — so it builds its own lines here and
    // reuses only the shared `modal_frame` (the frame was the duplicated part).
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(" > ", theme.prompt()),
        Span::raw(app.palette_query().to_string()),
        Span::styled("█", theme.dim()),
    ]));
    lines.push(Line::from(Span::styled("— commands —", theme.dim())));

    let sel = app.palette_selected().unwrap_or(0);
    // Cap visible rows so the palette box stays a fixed, readable height; the
    // selection still moves through the full filtered list, this only bounds what
    // is drawn at once.
    const VISIBLE_ROWS: usize = 12;
    for (i, c) in filtered.iter().enumerate().take(VISIBLE_ROWS) {
        let prefix = if i == sel { "› " } else { "  " };
        let style = if i == sel {
            theme.selection()
        } else {
            Style::new()
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(format!("{:<14}", c.label), style),
            Span::raw(format!("  {}", c.desc)),
        ]));
    }
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled("  (no match)", theme.dim())));
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(modal_frame("Command Palette (^P / :)", &theme)),
        area,
    );
}

pub(super) fn render_playlist_picker(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    let area = centered_rect(50, 50, frame.area());
    let sel = app.playlist_picker_selected().unwrap_or(0);

    let rows = app
        .playlists()
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            PickerRow::labeled(
                pl.name.clone(),
                Some(format!(" ({} scripts)", pl.paths.len())),
                i == sel,
                &theme,
            )
        })
        .collect();

    render_picker(
        frame,
        area,
        &theme,
        PickerSpec {
            title: "Choose Playlist",
            hint: "↑/↓ or j/k: move  ·  Enter: add  ·  Esc / c / q: cancel",
            rows,
            empty_msg: "(no playlists)",
        },
    );
}

pub(super) fn render_save_search_name(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    let area = centered_rect(60, 30, frame.area());
    let lines: Vec<Line> = vec![
        Line::from(Span::styled("Save current search as:", theme.prompt())),
        Line::from(Span::styled(
            "type a name  ·  Enter: save  ·  Esc: cancel",
            theme.dim(),
        )),
        Line::from(Span::raw("")),
        Line::from(vec![
            Span::raw("name: "),
            Span::styled(format!("{}█", app.save_search_name()), theme.match_text()),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            format!("query: {}", app.save_search_query()),
            theme.dim(),
        )),
    ];

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(modal_frame("Save Search", &theme)),
        area,
    );
}

pub(super) fn render_saved_search_picker(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    let area = centered_rect(60, 60, frame.area());
    let sel = app.saved_search_selected().unwrap_or(0);

    let rows = app
        .saved_searches()
        .iter()
        .enumerate()
        .map(|(i, (name, query))| {
            PickerRow::labeled(name.clone(), Some(format!("  ({query})")), i == sel, &theme)
        })
        .collect();

    render_picker(
        frame,
        area,
        &theme,
        PickerSpec {
            title: "Saved Searches",
            hint: "↑/↓ or j/k: move  ·  Enter: load  ·  d: delete  ·  Esc / c / q: cancel",
            rows,
            empty_msg: "(no saved searches)",
        },
    );
}

pub(super) fn render_edit_metadata(frame: &mut Frame, app: &App) {
    let theme = app.theme();
    let area = centered_rect(70, 70, frame.area());
    let labels = [
        "name",
        "desc",
        "tags",
        "usage",
        "category",
        "note (personal)",
    ];
    let values = [
        app.edit_name(),
        app.edit_desc(),
        app.edit_tags(),
        app.edit_usage(),
        app.edit_category(),
        app.edit_note(),
    ];
    let focus = app.edit_focus();

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Edit metadata (saved to sidecar .yaml + personal note)",
            theme.prompt(),
        )),
        Line::from(Span::styled(
            "Tab/Shift-Tab: next/prev field  ·  Enter: save  ·  Esc: cancel",
            theme.dim(),
        )),
        Line::from(Span::raw("")),
    ];

    for (i, (lab, val)) in labels.iter().zip(values.iter()).enumerate() {
        let prefix = if i == focus { "▶ " } else { "  " };
        let val_str = if val.is_empty() { "<empty>" } else { val };
        let line = if i == focus {
            Line::from(vec![
                Span::styled(format!("{prefix}{lab:>10}: "), theme.prompt()),
                Span::styled(val_str.to_string(), Style::new().bold()),
            ])
        } else {
            Line::from(vec![
                Span::raw(format!("{prefix}{lab:>10}: ")),
                Span::raw(val_str.to_string()),
            ])
        };
        lines.push(line);
    }

    lines.push(Line::from(Span::raw("")));
    lines.push(Line::from(Span::styled(
        "Note: edits always write a sidecar (safe, overrides header).",
        theme.dim(),
    )));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(modal_frame("Metadata Editor", &theme)),
        area,
    );
}

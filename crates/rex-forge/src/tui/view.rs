//! suite-ui rendering of `AppState`. One `draw()` per frame; no state mutation.
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{pane, KeyHints, Theme};

use crate::registry::Registry;
use crate::tui::state::{AppState, Step};

const FOOTER: &[(&str, &str)] = &[
    ("up/dn", "move"),
    ("Space", "toggle"),
    ("Tab", "next"),
    ("S-Tab", "back"),
    ("/", "filter"),
    ("q", "quit"),
];

pub fn draw(frame: &mut Frame, state: &AppState, reg: &Registry) {
    let theme = Theme::with_color(true);
    let area = frame.area();
    let [body, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    match state.step {
        Step::Base => draw_base(frame, body, state, reg, theme),
        Step::Details => draw_details(frame, body, state, theme),
        Step::Components => draw_components(frame, body, state, reg, theme),
        Step::Confirm => draw_confirm(frame, body, state, reg, theme),
        Step::Done => {}
    }
    KeyHints { hints: FOOTER }.render(frame, footer, theme);
}

fn draw_base(frame: &mut Frame, area: Rect, state: &AppState, reg: &Registry, theme: Theme) {
    let block = pane(" rex-forge — choose a base ", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let bases = state.visible_bases(reg);
    let lines: Vec<Line> = bases
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let marker = if i == state.cursor { "> " } else { "  " };
            let text = format!("{marker}{:10} {}", b.name, b.summary);
            if i == state.cursor {
                Line::styled(text, theme.title())
            } else {
                Line::from(text)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_details(frame: &mut Frame, area: Rect, state: &AppState, theme: Theme) {
    let block = pane(" rex-forge — details ", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(format!("name:    {}", state.project_name)),
        Line::from(format!("license: {}", state.license)),
        Line::from(format!("author:  {}", state.author)),
        Line::from(""),
        Line::styled("Tab to continue", theme.dim()),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_components(frame: &mut Frame, area: Rect, state: &AppState, reg: &Registry, theme: Theme) {
    let title = format!(
        " rex-forge — components ({}) ",
        state.base.as_deref().unwrap_or("")
    );
    let block = pane(&title, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let comps = state.visible_components(reg);
    let mut lines: Vec<Line> = Vec::new();
    let mut last_category: Option<String> = None;
    for (i, c) in comps.iter().enumerate() {
        if last_category.as_deref() != Some(c.category.as_str()) {
            lines.push(Line::styled(
                c.category.to_uppercase(),
                theme.dim().add_modifier(Modifier::BOLD),
            ));
            last_category = Some(c.category.clone());
        }
        let check = if state.is_selected(&c.name) { "[x]" } else { "[ ]" };
        let marker = if i == state.cursor { "> " } else { "  " };
        let text = format!("{marker}{check} {:12} {}", c.name, c.summary);
        if i == state.cursor {
            lines.push(Line::styled(text, theme.title()));
        } else {
            lines.push(Line::from(text));
        }
    }
    if comps.is_empty() {
        lines.push(Line::styled("(no components for this base)", theme.dim()));
    }
    lines.push(Line::from(""));
    let mut sel = state.selected.clone();
    sel.sort();
    let sel_text = if sel.is_empty() { "-".to_string() } else { sel.join(", ") };
    lines.push(Line::styled(format!("selected: {sel_text}"), theme.title()));
    if !state.status.is_empty() {
        lines.push(Line::styled(format!("! {}", state.status), theme.prompt()));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_confirm(frame: &mut Frame, area: Rect, state: &AppState, reg: &Registry, theme: Theme) {
    let block = pane(" rex-forge — confirm ", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sel = state.selection();
    let comp_text = if sel.components.is_empty() {
        "-".to_string()
    } else {
        sel.components.join(", ")
    };
    let mut lines = vec![
        Line::from(format!("Create ./{}", sel.project_name)),
        Line::from(format!("base: {}   components: {}", sel.base, comp_text)),
        Line::from(format!("git init: {}", if state.git { "yes" } else { "no" })),
        Line::from(""),
    ];
    if let Ok(plan) = crate::resolve::resolve(reg, &sel.base, &sel.components) {
        if let Ok(gen) = crate::merge::generate(reg, &plan, &sel) {
            for p in gen.tree.paths() {
                lines.push(Line::from(format!("  {p}")));
            }
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::styled("Enter to generate", theme.title()));
    frame.render_widget(Paragraph::new(lines), inner);
}

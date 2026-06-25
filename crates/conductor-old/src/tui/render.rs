//! Ratatui renderers for Conductor's interactive view, in the shared suite look.
//!
//! `render` is the single entry the runtime calls each draw: it paints the plan
//! screen (header / situation / plan / hints panes) and, when an overlay is up,
//! draws the confirm or help modal over it. Every distinction is carried by a
//! word + glyph as well as colour, so the view reads correctly with `NO_COLOR`.
//! Drawn with `suite_ui::{pane, ConfirmModal, HelpSheet, EmptyState}` + `Theme`
//! so the chrome matches RexOps from one source. No I/O, no app state owned here.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use suite_ui::{pane, ConfirmModal, EmptyState, HelpSheet, Theme};

use super::app::{App, Screen};
use super::style::Palette;
use crate::plan::{Ring, Step, StepStatus};
use crate::run::confirm_command;

/// A titled box for the high-contrast view: a thick double border in the accent
/// colour with a bold title, replacing the suite's thin rounded `pane` so the
/// frames read cleanly for weak vision. Falls back to suite `pane` when high
/// contrast is off (see `titled_block`).
fn hc_block(title: &str, pal: Palette) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(pal.accent())
        .title(Span::styled(format!(" {title} "), pal.title()))
}

/// The titled box for a pane, honoring the contrast toggle: thick double border
/// (high contrast) or the shared suite rounded `pane` (off).
fn titled_block(title: &str, pal: Palette, theme: Theme) -> Block<'static> {
    if pal.active() {
        hc_block(title, pal)
    } else {
        pane(title, theme)
    }
}

/// Body text style: bright bold (high contrast) or the shared dim (off).
fn body(pal: Palette, theme: Theme) -> Style {
    if pal.active() {
        pal.text()
    } else {
        theme.dim()
    }
}

/// The keybinding rows for the help overlay. Kept next to the real key handling
/// (in `app::step`) so help can't drift from the bindings.
pub const HELP_ROWS: &[(&str, &str)] = &[
    ("↑ / ↓ · j / k", "move between steps"),
    ("1-9", "jump to that step"),
    ("enter", "run the step (changes-state confirm first)"),
    ("s", "skip the current step"),
    ("r", "hand off to the rexops cockpit"),
    ("+ / -", "bigger / smaller text"),
    ("c", "toggle high-contrast theme"),
    ("?", "toggle this help"),
    ("q / Esc", "quit"),
];

/// The one-line key-hint strip shown at the foot of the plan screen.
const HINT: &str = "↑/↓ move · enter run · s skip · +/- size · c contrast · ? help · q quit";

/// The glyph for a step. The focused step overrides this with `▸`.
fn glyph(status: StepStatus, focused: bool) -> char {
    if focused {
        return '▸';
    }
    match status {
        StepStatus::Pending => '○',
        StepStatus::Done => '✓',
        StepStatus::Skipped => '·',
        StepStatus::Failed => '✗',
    }
}

/// Ring tag style: changes-state reads as caution, read-only/info as body.
fn ring_style(ring: Ring, pal: Palette, theme: Theme) -> Style {
    match ring {
        Ring::ChangesState => {
            if pal.active() {
                pal.caution()
            } else {
                theme.confirm()
            }
        }
        Ring::ReadOnly | Ring::Info => body(pal, theme),
    }
}

/// Build one plan row: selection rail (accent on the focused row), status glyph,
/// number, title, the inline annotation, and the right-edge ring tag — composed
/// the way RexOps's launcher rows are. In high contrast the title is bright
/// bold and the focused row gets a strong reverse bar.
fn step_line(n: usize, step: &Step, focused: bool, pal: Palette, theme: Theme) -> Line<'static> {
    let rail = if focused {
        Span::styled("▌ ", if pal.active() { pal.accent() } else { theme.selected_rail() })
    } else {
        Span::raw("  ")
    };
    let g = glyph(step.status, focused);
    let title_style = match (focused, pal.active()) {
        (true, true) => pal.selection(),
        (true, false) => theme.selection(),
        (false, true) => pal.text(),
        (false, false) => theme.title(),
    };

    let mut spans = vec![
        rail,
        Span::styled(format!("{g} {n}  "), title_style),
        Span::styled(step.title.clone(), title_style),
    ];
    if let Some(note) = &step.annotation {
        let accent = if pal.active() { pal.accent() } else { theme.accent_bar() };
        spans.push(Span::styled(format!("  ← {note}"), accent));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        step.ring.tag().to_string(),
        ring_style(step.ring, pal, theme),
    ));
    Line::from(spans)
}

/// The plan screen: header / optional situation / plan / hints, in panes.
fn render_plan(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let pal = Palette::new(app.high_contrast);
    let has_situation = !app.plan.situation.is_empty();
    let constraints = if has_situation {
        vec![
            Constraint::Length(3), // header
            Constraint::Length(app.plan.situation.len() as u16 + 2), // situation
            Constraint::Min(3),    // plan (fills the rest)
            Constraint::Length(2), // hints + notice
        ]
    } else {
        vec![
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // Header.
    let header = Paragraph::new(Line::from(Span::styled(
        "Given the suite's state — do these, in this order.",
        body(pal, theme),
    )))
    .block(titled_block("Conductor", pal, theme));
    f.render_widget(header, chunks[0]);

    // Situation (optional) + plan + hints land in shifting indices.
    let (plan_idx, hints_idx) = if has_situation {
        let lines: Vec<Line> = app
            .plan
            .situation
            .iter()
            .map(|s| Line::from(Span::styled(s.clone(), body(pal, theme))))
            .collect();
        let sit = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(titled_block("The situation", pal, theme));
        f.render_widget(sit, chunks[1]);
        (2, 3)
    } else {
        (1, 2)
    };

    // Plan. Each step is its title row + its command row, then `density.gap()`
    // blank spacer rows so bigger settings physically space the steps out.
    let gap = app.density.gap();
    let rows: Vec<Line> = app
        .plan
        .steps
        .iter()
        .enumerate()
        .flat_map(|(i, step)| {
            let mut v = vec![step_line(i + 1, step, i == app.cursor, pal, theme)];
            if let Some(cmd) = &step.command {
                v.push(Line::from(Span::styled(
                    format!("       {cmd}"),
                    body(pal, theme),
                )));
            }
            for _ in 0..gap {
                v.push(Line::from(""));
            }
            v
        })
        .collect();
    let title = format!("The plan — {} steps", app.plan.steps.len());
    let plan = Paragraph::new(rows).block(titled_block(&title, pal, theme));
    f.render_widget(plan, chunks[plan_idx]);

    // Hints + transient notice (notice takes the line when set). Bright in both
    // modes so the foot strip never reads as faint grey.
    let hint_line = match &app.notice {
        Some(msg) => Line::from(Span::styled(
            msg.clone(),
            if pal.active() { pal.ok() } else { theme.status_error() },
        )),
        None => Line::from(Span::styled(HINT, body(pal, theme))),
    };
    f.render_widget(Paragraph::new(hint_line), chunks[hints_idx]);
}

/// The empty "nothing to conduct" screen, framed in a pane (EmptyState draws
/// text only and expects the caller to frame the region).
fn render_empty(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let pal = Palette::new(app.high_contrast);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3)])
        .split(area);
    let block = titled_block("Conductor", pal, theme);
    let inner = block.inner(chunks[0]);
    f.render_widget(block, chunks[0]);

    if pal.active() {
        // EmptyState styles via the shared theme (dim); in high contrast draw our
        // own bright centred lines instead so the "all clear" message is bold.
        let lines = vec![
            Line::from(Span::styled("nothing to conduct", pal.title())),
            Line::from(""),
            Line::from(Span::styled(
                "the suite is healthy and every feed is current",
                pal.text(),
            )),
        ];
        let para = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
        // Vertically centre by padding from the top.
        let pad = inner.height.saturating_sub(3) / 2;
        let centred = Rect {
            y: inner.y + pad,
            height: inner.height.saturating_sub(pad),
            ..inner
        };
        f.render_widget(para, centred);
    } else {
        EmptyState {
            message: "nothing to conduct",
            hint: Some("the suite is healthy and every feed is current"),
        }
        .render(f, inner, theme);
    }
}

/// The single draw entry the runtime calls each frame.
pub fn render(f: &mut Frame, app: &App, theme: Theme) {
    let area = f.area();

    if app.plan.steps.is_empty() {
        render_empty(f, app, area, theme);
        return;
    }

    render_plan(f, app, area, theme);

    match app.screen {
        Screen::Confirm => {
            if let Some(step) = app.plan.steps.get(app.cursor) {
                let cmd = confirm_command(step).unwrap_or("(no command)");
                let message = format!("{cmd}   — this changes suite state");
                ConfirmModal {
                    title: &step.title,
                    message: &message,
                }
                .render(f, area, theme);
            }
        }
        Screen::Help => {
            HelpSheet {
                title: "Keys",
                rows: HELP_ROWS,
            }
            .render(f, area, theme);
        }
        Screen::Plan => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};
    use ratatui::{backend::TestBackend, Terminal};
    use suite_ui::Theme;

    fn sample() -> plan::Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(),
            why: "AWS key".into(),
            source: "bulwark".into(),
            severity: Severity::Critical,
        });
        plan::build(&s)
    }

    /// Render `app` into an off-screen buffer and flatten it to text so a test can
    /// assert on what actually appears. Mirrors rexops-tui's screen-test helper.
    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal.draw(|f| render(f, app, theme)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let width = buffer.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buffer.content.iter().enumerate() {
            if i % width == 0 && i != 0 {
                out.push('\n');
            }
            out.push_str(cell.symbol());
        }
        out
    }

    /// Count cells in the rendered buffer that carry the DIM modifier — the
    /// thing that hurts weak vision. High contrast must produce ZERO.
    fn dim_cell_count(app: &App) -> usize {
        use ratatui::style::Modifier;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal.draw(|f| render(f, app, theme)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content
            .iter()
            .filter(|c| c.modifier.contains(Modifier::DIM))
            .count()
    }

    #[test]
    fn high_contrast_renders_no_dim_cells() {
        let app = App::new(sample()); // high_contrast on by default
        assert!(app.high_contrast);
        assert_eq!(
            dim_cell_count(&app),
            0,
            "high-contrast mode must not paint any dim/grey cells"
        );
    }

    #[test]
    fn turning_high_contrast_off_uses_the_shared_dim_theme() {
        let mut app = App::new(sample());
        app.high_contrast = false;
        // The plain suite theme uses dim for commands/hints, so some dim cells
        // should now appear — proving the toggle actually switches palettes.
        assert!(
            dim_cell_count(&app) > 0,
            "with high contrast off, the shared theme's dim text should render"
        );
    }

    /// Render into a TALL buffer (so density spacers aren't clipped) and flatten.
    fn render_tall(app: &App) -> String {
        let backend = TestBackend::new(80, 60);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal.draw(|f| render(f, app, theme)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let width = buffer.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buffer.content.iter().enumerate() {
            if i % width == 0 && i != 0 {
                out.push('\n');
            }
            out.push_str(cell.symbol());
        }
        out
    }

    /// The 0-based row index of the first line containing `needle` in the
    /// rendered (tall) frame, or None.
    fn row_of(app: &App, needle: &str) -> Option<usize> {
        render_tall(app)
            .lines()
            .position(|l| l.contains(needle))
    }

    #[test]
    fn huge_density_pushes_later_steps_further_down_than_compact() {
        use super::super::app::Density;
        // Density inserts blank spacer rows between steps. Borders make a literal
        // "blank line" count unreliable, so instead assert that a LATER step is
        // drawn on a lower row under Huge than under Compact — the visible effect
        // of the extra spacing. "bulwark show" is the last (Ring-1) step's command.
        let mut compact = App::new(sample());
        compact.density = Density::Compact;
        let mut huge = App::new(sample());
        huge.density = Density::Huge;

        let last_needle = "bulwark show deploy-prod.sh";
        let compact_row = row_of(&compact, last_needle).expect("compact shows the last step");
        let huge_row = row_of(&huge, last_needle).expect("huge shows the last step");
        assert!(
            huge_row > compact_row,
            "huge density must push later steps down (compact_row={compact_row}, huge_row={huge_row})"
        );
    }

    #[test]
    fn plan_screen_shows_title_steps_commands_and_ring_tags() {
        let app = App::new(sample());
        let text = render_to_text(&app);
        assert!(text.contains("Conductor"), "header pane title:\n{text}");
        assert!(text.contains("The plan"), "plan pane title:\n{text}");
        assert!(
            text.contains("workstate snapshot"),
            "step command shown:\n{text}"
        );
        assert!(text.contains("changes state"), "ring tag shown:\n{text}");
    }

    #[test]
    fn focused_step_shows_the_selection_rail() {
        let app = App::new(sample()); // cursor 0
        let text = render_to_text(&app);
        // The suite selection rail glyph precedes the focused row's number/title.
        assert!(
            text.contains('▌'),
            "focused row must show the accent rail:\n{text}"
        );
    }

    #[test]
    fn situation_block_renders_when_present() {
        let app = App::new(sample());
        let text = render_to_text(&app);
        assert!(
            text.contains("The situation"),
            "situation pane shown:\n{text}"
        );
    }

    #[test]
    fn empty_plan_shows_nothing_to_conduct() {
        let app = App::new(plan::build(&SuiteState::empty()));
        let text = render_to_text(&app);
        assert!(
            text.contains("nothing to conduct"),
            "empty state copy:\n{text}"
        );
        assert!(!text.contains("The plan"), "no plan pane when empty:\n{text}");
    }

    #[test]
    fn confirm_overlay_shows_command_and_caution() {
        let mut app = App::new(sample());
        app.screen = Screen::Confirm; // cursor 0 is the Ring-2 refresh
        let text = render_to_text(&app);
        assert!(
            text.contains("workstate snapshot"),
            "confirm shows the literal command:\n{text}"
        );
        assert!(
            text.to_lowercase().contains("changes suite state")
                || text.to_lowercase().contains("changes state"),
            "confirm shows the caution:\n{text}"
        );
    }

    #[test]
    fn help_overlay_lists_the_keys() {
        let mut app = App::new(sample());
        app.screen = Screen::Help;
        let text = render_to_text(&app);
        assert!(text.contains("Keys"), "help title:\n{text}");
        assert!(
            text.contains("rexops"),
            "help mentions the rexops handoff:\n{text}"
        );
        assert!(text.contains("skip"), "help mentions skip:\n{text}");
    }

    #[test]
    fn help_rows_describe_changes_state_gate() {
        // The help content must not drift from the gate behaviour.
        let joined: String = HELP_ROWS
            .iter()
            .map(|(k, d)| format!("{k} {d} "))
            .collect();
        assert!(joined.contains("run"));
        assert!(
            joined.to_lowercase().contains("confirm")
                || joined.to_lowercase().contains("changes-state")
        );
    }
}

//! Ratatui renderers for Conductor's interactive view, in the shared suite look.
//!
//! `render` is the single entry the runtime calls each draw: it paints the plan
//! screen (header / situation / plan / hints panes) and, when an overlay is up,
//! draws the confirm or help modal over it. Every distinction is carried by a
//! word + glyph as well as colour, so the view reads correctly with `NO_COLOR`.
//! Drawn with `suite_ui::{pane, ConfirmModal, HelpSheet, EmptyState}` + `Theme`
//! so the chrome matches RexOps from one source. No I/O, no app state owned here.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use suite_ui::{pane, ConfirmModal, EmptyState, HelpSheet, Theme};

use super::app::{App, Screen};
use crate::plan::{Ring, Step, StepStatus};
use crate::run::confirm_command;

/// The keybinding rows for the help overlay. Kept next to the real key handling
/// (in `app::step`) so help can't drift from the bindings.
pub const HELP_ROWS: &[(&str, &str)] = &[
    ("enter", "run the current step (changes-state steps confirm first)"),
    ("s", "skip the current step"),
    ("a", "advance focus without running"),
    ("r", "hand off to the rexops cockpit"),
    ("?", "toggle this help"),
    ("q / Esc", "quit"),
];

/// The one-line key-hint strip shown at the foot of the plan screen.
const HINT: &str = "enter run · s skip · a advance · r rexops · ? help · q quit";

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

/// Ring tag style: changes-state reads as attention, read-only/info as dim.
fn ring_style(ring: Ring, theme: Theme) -> ratatui::style::Style {
    match ring {
        Ring::ChangesState => theme.confirm(),
        Ring::ReadOnly | Ring::Info => theme.dim(),
    }
}

/// Build one plan row: selection rail (accent on the focused row), status glyph,
/// number, title, the inline annotation, and the right-edge ring tag — composed
/// the way RexOps's launcher rows are.
fn step_line(n: usize, step: &Step, focused: bool, theme: Theme) -> Line<'static> {
    let rail = if focused {
        Span::styled("▌ ", theme.selected_rail())
    } else {
        Span::raw("  ")
    };
    let g = glyph(step.status, focused);
    let title_style = if focused {
        theme.selection()
    } else {
        theme.title()
    };

    let mut spans = vec![
        rail,
        Span::styled(format!("{g} {n}  "), title_style),
        Span::styled(step.title.clone(), title_style),
    ];
    if let Some(note) = &step.annotation {
        spans.push(Span::styled(format!("  ← {note}"), theme.accent_bar()));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        step.ring.tag().to_string(),
        ring_style(step.ring, theme),
    ));
    Line::from(spans)
}

/// The plan screen: header / optional situation / plan / hints, in panes.
fn render_plan(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let has_situation = !app.plan.situation.is_empty();
    let constraints = if has_situation {
        vec![
            Constraint::Length(3), // header
            Constraint::Length(app.plan.situation.len() as u16 + 2), // situation
            Constraint::Min(3),    // plan
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
        theme.dim(),
    )))
    .block(pane("Conductor", theme));
    f.render_widget(header, chunks[0]);

    // Situation (optional) + plan + hints land in shifting indices.
    let (plan_idx, hints_idx) = if has_situation {
        let lines: Vec<Line> = app
            .plan
            .situation
            .iter()
            .map(|s| Line::from(Span::styled(s.clone(), theme.dim())))
            .collect();
        let sit = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(pane("The situation", theme));
        f.render_widget(sit, chunks[1]);
        (2, 3)
    } else {
        (1, 2)
    };

    // Plan.
    let rows: Vec<Line> = app
        .plan
        .steps
        .iter()
        .enumerate()
        .flat_map(|(i, step)| {
            let mut v = vec![step_line(i + 1, step, i == app.cursor, theme)];
            if let Some(cmd) = &step.command {
                v.push(Line::from(Span::styled(
                    format!("       {cmd}"),
                    theme.dim(),
                )));
            }
            v
        })
        .collect();
    let title = format!("The plan — {} steps", app.plan.steps.len());
    let plan = Paragraph::new(rows).block(pane(&title, theme));
    f.render_widget(plan, chunks[plan_idx]);

    // Hints + transient notice (notice takes the line when set).
    let hint_line = match &app.notice {
        Some(msg) => Line::from(Span::styled(msg.clone(), theme.status_error())),
        None => Line::from(Span::styled(HINT, theme.dim())),
    };
    f.render_widget(Paragraph::new(hint_line), chunks[hints_idx]);
}

/// The empty "nothing to conduct" screen, framed in a pane (EmptyState draws
/// text only and expects the caller to frame the region).
fn render_empty(f: &mut Frame, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3)])
        .split(area);
    let block = pane("Conductor", theme);
    let inner = block.inner(chunks[0]);
    f.render_widget(block, chunks[0]);
    EmptyState {
        message: "nothing to conduct",
        hint: Some("the suite is healthy and every feed is current"),
    }
    .render(f, inner, theme);
}

/// The single draw entry the runtime calls each frame.
pub fn render(f: &mut Frame, app: &App, theme: Theme) {
    let area = f.area();

    if app.plan.steps.is_empty() {
        render_empty(f, area, theme);
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

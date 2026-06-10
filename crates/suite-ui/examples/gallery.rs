//! Renders every suite-ui component once in each theme, into an in-memory
//! buffer that is printed to stdout. No real terminal required — it's a visual
//! smoke test and a check that the public API is usable from outside the crate.
//!
//! Run with:  cargo run -p suite-ui --example gallery

use std::time::Duration;

use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Terminal;

use suite_ui::{
    centered_rect, pane, pane_titled, truncate_desc, truncate_path, App, AttentionFlag,
    ConfirmModal, Counted, EmptyState, FilterChips, Flow, Freshness, Health, HealthStrip,
    HelpSheet, JobState, KeyHints, PaletteFrame, PaletteItem, Screen, SearchBar, Severity,
    SeverityBadge, StatusBar, StatusStrip, Theme, ThemeChoice, Toast, ToastKind,
};

fn main() {
    for (name, theme) in [
        ("cyan (colour)", Theme::with(true, ThemeChoice::Cyan)),
        ("amber (colour)", Theme::with(true, ThemeChoice::Amber)),
        ("NO_COLOR", Theme::with_color(false)),
    ] {
        println!("\n================ theme: {name} ================");
        print_frame(theme);
        demo_app_runtime(theme);
    }
}

/// Draw a small composite frame exercising panes, health styles, and each
/// overlay, then print the resulting buffer.
fn print_frame(theme: Theme) {
    let backend = TestBackend::new(80, 28);
    let mut terminal = Terminal::new(backend).expect("test backend");

    terminal
        .draw(|frame| {
            let area = frame.area();
            let [top, bottom] =
                Layout::vertical([Constraint::Length(7), Constraint::Fill(1)]).areas(area);

            // A pane with health badges inside.
            let block = pane("adapters", theme);
            let inner = block.inner(top);
            frame.render_widget(block, top);
            render_health_rows(frame, inner, theme);

            // The help sheet over the bottom region.
            let rows = [
                ("↑ / ↓ · j / k", "move the selection"),
                ("Enter", "activate the selection"),
                ("^P · :", "open the command palette"),
                ("?", "toggle this help"),
                ("q", "quit"),
            ];
            HelpSheet {
                title: "Keybindings",
                rows: &rows,
            }
            .render(frame, bottom, theme);
        })
        .unwrap();
    print!("{}", buffer_to_string(terminal));

    // Each remaining overlay on its own clean frame, so they don't overlap.
    print_overlay("confirm modal", theme, |frame, area, theme| {
        ConfirmModal {
            title: "Delete file",
            message: "Remove old-backup.sh?",
        }
        .render(frame, area, theme);
    });
    print_overlay("command palette", theme, |frame, area, theme| {
        let items = [
            PaletteItem {
                label: "reload",
                desc: "rescan scripts incrementally",
            },
            PaletteItem {
                label: "new-playlist",
                desc: "create an empty playlist",
            },
            PaletteItem {
                label: "toggle-output",
                desc: "show/hide the output pane",
            },
            PaletteItem {
                label: "help",
                desc: "show keybindings",
            },
        ];
        PaletteFrame {
            query: "re",
            items: &items,
            selected: Some(0),
        }
        .render(frame, area, theme);
    });
    print_overlay("key hints (footer strip)", theme, |frame, area, theme| {
        let [row, _] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
            .areas(centered_rect(80, 40, area));
        let hints = [
            ("q", "quit"),
            ("^P", "palette"),
            ("r", "refresh"),
            ("?", "help"),
            ("1-7", "screens"),
        ];
        KeyHints { hints: &hints }.render(frame, row, theme);
    });
    print_overlay(
        "search bar (empty + active)",
        theme,
        |frame, area, theme| {
            let [empty_row, active_row] =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
                    .areas(centered_rect(60, 10, area));
            SearchBar {
                query: "",
                placeholder: "type to filter adapters",
                match_count: None,
            }
            .render(frame, empty_row, theme);
            SearchBar {
                query: "bul",
                placeholder: "type to filter adapters",
                match_count: Some(1),
            }
            .render(frame, active_row, theme);
        },
    );
    print_overlay("status bar (job states)", theme, |frame, area, theme| {
        let rows: [Rect; 5] =
            Layout::vertical([Constraint::Length(1); 5]).areas(centered_rect(60, 40, area));
        for (row, job) in rows.into_iter().zip([
            JobState::Running { name: "backup" },
            JobState::Done {
                name: "backup",
                ok: true,
            },
            JobState::Done {
                name: "rescan",
                ok: false,
            },
            JobState::Cancelled { name: "deploy" },
            JobState::Idle,
        ]) {
            StatusBar { job }.render(frame, row, theme);
        }
    });
    print_overlay(
        "toast (info + error + job events)",
        theme,
        |frame, area, theme| {
            let rows: [Rect; 5] =
                Layout::vertical([Constraint::Length(1); 5]).areas(centered_rect(60, 40, area));
            let toasts = [
                ("saved search 'deploys'", ToastKind::Info),
                ("reload failed: permission denied", ToastKind::Error),
                ("backup — done", ToastKind::Success),
                ("rescan — failed", ToastKind::Failure),
                ("deploy — cancelled", ToastKind::Cancelled),
            ];
            for (row, (text, kind)) in rows.into_iter().zip(toasts) {
                Toast { text, kind }.render(frame, row, theme);
            }
        },
    );
    print_overlay(
        "filter chips (active filters)",
        theme,
        |frame, area, theme| {
            let [row, _] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
                .areas(centered_rect(70, 20, area));
            let labels = ["t:ci", "lang:bash", "risk:high"];
            FilterChips { labels: &labels }.render(frame, row, theme);
        },
    );
    print_overlay("status strip (· segments)", theme, |frame, area, theme| {
        let [row, _] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
            .areas(centered_rect(70, 20, area));
        let segments = ["All", "Auto", "312"];
        StatusStrip {
            segments: &segments,
        }
        .render(frame, row, theme);
    });
    print_overlay(
        "severity badges (CRIT / HIGH / MED / LOW)",
        theme,
        |frame, area, theme| {
            use ratatui::text::{Line, Span};
            use ratatui::widgets::Paragraph;
            // All four badges on one line, each followed by a sample finding, so
            // the relative loudness of the levels is visible side by side.
            let [row, _] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
                .areas(centered_rect(80, 20, area));
            let mut spans: Vec<Span> = Vec::new();
            for sev in [
                Severity::Critical,
                Severity::High,
                Severity::Medium,
                Severity::Low,
            ] {
                spans.push(SeverityBadge { severity: sev }.span(theme));
                spans.push(Span::raw("  "));
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), row);
        },
    );
    print_overlay(
        "attention flags (raised vs clear)",
        theme,
        |frame, area, theme| {
            let rows: [Rect; 4] =
                Layout::vertical([Constraint::Length(1); 4]).areas(centered_rect(60, 30, area));
            let flags = [
                AttentionFlag {
                    count: 2,
                    label: "critical",
                    severity: Severity::Critical,
                },
                AttentionFlag {
                    count: 5,
                    label: "high",
                    severity: Severity::High,
                },
                AttentionFlag {
                    count: 3,
                    label: "review due",
                    severity: Severity::High,
                },
                AttentionFlag {
                    count: 0,
                    label: "high",
                    severity: Severity::High,
                },
            ];
            for (row, flag) in rows.into_iter().zip(flags) {
                flag.render(frame, row, theme);
            }
        },
    );
    print_overlay(
        "health strip (compact health summary)",
        theme,
        |frame, area, theme| {
            let [row, _] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)])
                .areas(centered_rect(80, 20, area));
            let segments = [
                (Health::Healthy, "bulwark"),
                (Health::Degraded, "vault"),
                (Health::Unavailable, "proto"),
                (Health::Unknown, "scratch"),
            ];
            HealthStrip {
                segments: &segments,
            }
            .render(frame, row, theme);
        },
    );
    print_overlay(
        "freshness stamps (just now → stale)",
        theme,
        |frame, area, theme| {
            let rows: [Rect; 5] =
                Layout::vertical([Constraint::Length(1); 5]).areas(centered_rect(60, 40, area));
            // A spread of ages, ending with one past a 1-day staleness threshold so
            // the stale warning style shows.
            let day = Duration::from_secs(24 * 60 * 60);
            let stamps = [
                Freshness::from(Duration::from_secs(2)),
                Freshness::from(Duration::from_secs(5 * 60)),
                Freshness::from(Duration::from_secs(2 * 60 * 60)),
                Freshness::from(Duration::from_secs(3 * 24 * 60 * 60)),
                Freshness {
                    age: Duration::from_secs(9 * 24 * 60 * 60),
                    stale_after: Some(day),
                },
            ];
            for (row, stamp) in rows.into_iter().zip(stamps) {
                stamp.render(frame, row, theme);
            }
        },
    );
    print_overlay(
        "pane_titled + Counted (narrowed vs full count)",
        theme,
        |frame, area, theme| {
            let [narrowed, full] = Layout::vertical([Constraint::Length(3), Constraint::Length(3)])
                .areas(centered_rect(70, 60, area));
            // A narrowed view: the count is emphasised inside the pane title.
            let title = title_with_count(
                "results",
                Counted {
                    shown: 48,
                    total: 312,
                },
                theme,
            );
            frame.render_widget(pane_titled(title, theme), narrowed);
            // A full view: the same title, count now dim.
            let title = title_with_count(
                "results",
                Counted {
                    shown: 312,
                    total: 312,
                },
                theme,
            );
            frame.render_widget(pane_titled(title, theme), full);
        },
    );
    print_overlay(
        "empty state (message + hint)",
        theme,
        |frame, area, theme| {
            let block = pane("results", theme);
            let inner = block.inner(centered_rect(70, 70, area));
            frame.render_widget(block, centered_rect(70, 70, area));
            EmptyState {
                message: "No items match the current filter.",
                hint: Some("Press Esc to clear the filter."),
            }
            .render(frame, inner, theme);
        },
    );
    print_truncation(theme);
}

/// Demonstrates the App runtime's public surface: a real `Screen` implementation
/// rendered through the in-memory backend (the gallery never opens a real
/// terminal, and `App::run` would block, so we show the wiring, not a live loop).
fn demo_app_runtime(theme: Theme) {
    use crossterm::event::{KeyCode, KeyEvent};
    use ratatui::widgets::Paragraph;

    struct Demo {
        message: String,
    }
    impl Screen for Demo {
        fn render(&mut self, frame: &mut ratatui::Frame, theme: Theme) {
            let block = pane("app runtime", theme);
            let inner = block.inner(frame.area());
            frame.render_widget(block, frame.area());
            frame.render_widget(Paragraph::new(self.message.as_str()), inner);
        }
        fn on_key(&mut self, key: KeyEvent) -> Flow {
            if key.code == KeyCode::Char('q') {
                Flow::Exit
            } else {
                Flow::Continue
            }
        }
    }

    // Construct an App to prove the builder is reachable from outside the crate.
    // We do NOT call `run()` (it would take over the real terminal and block).
    let _app = App::new(theme).tick_rate(Duration::from_millis(100));

    // Render the Screen once via the in-memory backend, like every other widget
    // in this gallery, so the demo participates in the visual smoke test.
    let mut screen = Demo {
        message: "Screen::render drew this through App's theme.".to_string(),
    };
    let mut terminal = Terminal::new(TestBackend::new(80, 6)).expect("test backend");
    terminal.draw(|frame| screen.render(frame, theme)).unwrap();
    print!("{}", buffer_to_string(terminal));
}

/// Build a `results (N of M)` pane title with the count in [`Counted`]'s style.
fn title_with_count(label: &str, count: Counted, theme: Theme) -> ratatui::text::Line<'static> {
    use ratatui::text::{Line, Span};
    Line::from(vec![
        Span::styled(format!(" {label} ("), theme.title()),
        count.span(theme),
        Span::styled(") ", theme.title()),
    ])
}

/// The truncation helpers aren't drawn into a frame — show them as plain
/// before → after text so the gallery documents the one shared ellipsis (`…`).
fn print_truncation(_theme: Theme) {
    println!("--- truncate_path / truncate_desc (one shared `…`) ---");
    let path = "/very/deeply/nested/directory/structure/backup-tool.sh";
    let desc = "  backs up the NAS every night to the offsite mirror  ";
    println!("  path[20]: {}", truncate_path(path, 20));
    println!("  desc[28]: {}", truncate_desc(desc, 28));
    println!();
}

fn render_health_rows(frame: &mut ratatui::Frame, area: Rect, theme: Theme) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;
    let rows = [
        (Health::Healthy, "bulwark      healthy"),
        (Health::Degraded, "scriptvault  degraded"),
        (Health::Unavailable, "workstate    unavailable"),
        (Health::Unknown, "proto        unknown"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(h, label)| Line::from(Span::styled((*label).to_string(), theme.health(*h))))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn print_overlay(label: &str, theme: Theme, draw: impl FnOnce(&mut ratatui::Frame, Rect, Theme)) {
    println!("--- {label} ---");
    let backend = TestBackend::new(80, 16);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal
        .draw(|frame| {
            let area = frame.area();
            draw(frame, area, theme);
        })
        .unwrap();
    print!("{}", buffer_to_string(terminal));
}

/// Flatten the test backend's cell buffer into printable text (glyphs only —
/// this is a layout smoke check, not a colour check; colour is asserted in the
/// unit tests).
fn buffer_to_string(terminal: Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer().clone();
    let width = buffer.area.width as usize;
    let mut out = String::new();
    for (i, cell) in buffer.content.iter().enumerate() {
        if i % width == 0 && i != 0 {
            out.push('\n');
        }
        out.push_str(cell.symbol());
    }
    out.push('\n');
    out
}

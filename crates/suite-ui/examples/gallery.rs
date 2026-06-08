//! Renders every suite-ui component once in each theme, into an in-memory
//! buffer that is printed to stdout. No real terminal required — it's a visual
//! smoke test and a check that the public API is usable from outside the crate.
//!
//! Run with:  cargo run -p suite-ui --example gallery

use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Terminal;

use suite_ui::{
    centered_rect, pane, ConfirmModal, Health, HelpSheet, PaletteFrame, PaletteItem, Theme,
    ThemeChoice, Toast, ToastKind,
};

fn main() {
    for (name, theme) in [
        ("cyan (colour)", Theme::with(true, ThemeChoice::Cyan)),
        ("amber (colour)", Theme::with(true, ThemeChoice::Amber)),
        ("NO_COLOR", Theme::with_color(false)),
    ] {
        println!("\n================ theme: {name} ================");
        print_frame(theme);
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
    print_overlay("toast (info + error)", theme, |frame, area, theme| {
        let [info_row, err_row] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)])
            .areas(centered_rect(60, 10, area));
        Toast {
            text: "saved search 'deploys'",
            kind: ToastKind::Info,
        }
        .render(frame, info_row, theme);
        Toast {
            text: "reload failed: permission denied",
            kind: ToastKind::Error,
        }
        .render(frame, err_row, theme);
    });
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

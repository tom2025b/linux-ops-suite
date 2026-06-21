//! insta geometry snapshot for a suite-ui status footer. Layout/glyphs only (colour
//! is not captured); the per-state style/glyph guarantees live in status_bar.rs unit
//! tests.

use ratatui::backend::TestBackend;
use ratatui::Terminal;
use suite_ui::{Heartbeat, JobState, StatusBar, Theme};

/// Flatten a one-row render into a string (ratatui 0.29 `Buffer` has no `Display`).
fn render_row<F: FnOnce(&mut ratatui::Frame)>(w: u16, f: F) -> String {
    let mut term = Terminal::new(TestBackend::new(w, 1)).unwrap();
    term.draw(f).unwrap();
    let buf = term.backend().buffer().clone();
    (0..w)
        .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
        .collect()
}

#[test]
fn snapshot_status_bar_done_ok() {
    let theme = Theme::with_color(false);
    let bar = StatusBar {
        job: JobState::Done {
            name: "backup",
            ok: true,
        },
    };
    let row = render_row(30, |f| bar.render(f, f.area(), theme));
    insta::assert_snapshot!(row);
}

#[test]
fn heartbeat_vital_renders_heart_sparkline_and_latency() {
    let hb = Heartbeat {
        samples: &[2, 5, 9, 4],
        latest_ms: Some(4),
    };
    assert_eq!(hb.text(), "♥ ▁▄█▃ 4ms");
}

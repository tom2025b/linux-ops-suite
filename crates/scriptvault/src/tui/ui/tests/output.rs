// Output-pane rendering: the idle "last run" vs live "● live" title, the [err]
// stderr marker (NO_COLOR-safe), scroll revealing older lines, and the stderr
// red hue asserted from the buffer's cell styles.

use super::super::*;
use super::{app_with_theme, fixture_app, render_to_rows};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::fs;

#[test]
fn output_pane_renders_title_and_lines_when_toggled() {
    let (mut app, dir) = fixture_app();
    app.toggle_output_pane();
    app.push_output_line("captured stdout line one".into());
    // Raw stderr text (no marker) — the renderer adds the `[err] ` marker itself.
    app.push_stderr_line("something on stderr".into());

    let screen = render_to_rows(&app, 80, 24);
    // Idle (no live run) with buffered lines -> the "last run" title, no ● marker.
    assert!(
        screen.contains("output (last run)"),
        "idle output pane title missing\n{screen}"
    );
    assert!(
        !screen.contains('●'),
        "live marker must NOT show when no run is active\n{screen}"
    );
    assert!(
        screen.contains("captured stdout line one"),
        "stdout line not visible\n{screen}"
    );
    // The stderr line shows its raw text PLUS the draw-time `[err] ` marker — this
    // is the NO_COLOR-safe distinction (the red hue can't survive a text snapshot,
    // but the marker always does).
    assert!(
        screen.contains("[err] something on stderr"),
        "stderr line should render with the [err] marker\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_pane_shows_live_marker_only_while_active() {
    let (mut app, dir) = fixture_app();
    app.toggle_output_pane();

    // A live run is in flight (shell sets the path on spawn) -> ● live marker.
    app.set_live_run_path(Some(dir.join("deploy.sh")));
    let active = render_to_rows(&app, 80, 24);
    assert!(
        active.contains("output ● live"),
        "live marker should show while a run is active\n{active}"
    );

    // Finish bookkeeping clears the path AND records the outcome (the exact
    // sequence the event loop runs) -> the output pane's live marker is gone and
    // the title falls back to its calm form.
    let path = dir.join("deploy.sh");
    let _ = app.take_live_run_path();
    app.finish_job(&path, Some(0));
    let idle = render_to_rows(&app, 80, 24);
    // Scope to the output-pane title row: the footer's job segment now shows the
    // finished run with a ✓ (no ●), so the ● we care about is the pane's alone.
    let pane_title_has_marker = idle
        .lines()
        .find(|l| l.contains("output"))
        .is_some_and(|l| l.contains('●'));
    assert!(
        !pane_title_has_marker,
        "output-pane live marker must disappear once the run finishes\n{idle}"
    );

    fs::remove_dir_all(&dir).ok();
}

// (E) Output pane scroll: scrolling up reveals OLDER lines that the tail hid.
#[test]
fn output_pane_scroll_reveals_older_lines() {
    let (mut app, dir) = fixture_app();
    app.toggle_output_pane();
    for i in 0..60 {
        app.push_output_line(format!("row{i:03}"));
    }

    // At the tail, the newest line is visible and an early line is not.
    let tail = render_to_rows(&app, 80, 24);
    assert!(
        tail.contains("row059"),
        "tail shows the newest line\n{tail}"
    );
    assert!(
        !tail.contains("row000"),
        "tail must not show the oldest line\n{tail}"
    );

    // Scroll all the way up (clamps to the top) -> the oldest line is now in view
    // and the newest has scrolled off.
    app.scroll_output(1000);
    let scrolled = render_to_rows(&app, 80, 24);
    assert!(
        scrolled.contains("row000"),
        "scrolling to the top should reveal the oldest line\n{scrolled}"
    );
    assert!(
        !scrolled.contains("row059"),
        "the newest line should scroll out of view\n{scrolled}"
    );

    fs::remove_dir_all(&dir).ok();
}

// (D) Lock the stderr-RED hue. The tagging increment's text-snapshot test could
// only prove the [err] marker (NO_COLOR-safe path); a text snapshot can't carry
// fg colour. This inspects the rendered Buffer's cell styles directly so the red
// is actually asserted, not just applied by construction.
#[test]
fn stderr_line_renders_red_under_colour() {
    use ratatui::style::Color;
    let dir = std::env::temp_dir().join(format!(
        "scriptvault-ui-stderr-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("deploy.sh"), "#!/bin/bash\necho go\n").unwrap();

    // Colour ON so the stderr hue is in play.
    let mut app = app_with_theme(&dir, Theme::with_color(true));
    app.toggle_output_pane();
    app.push_output_line("plain stdout".into());
    app.push_stderr_line("boom".into());

    let w = 80u16;
    let h = 24u16;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();

    // Find the row holding the stderr text and confirm at least one of its cells
    // is red — the marker proves the row, the fg proves the hue.
    let mut found_red_on_stderr_row = false;
    for y in 0..h {
        let row: String = (0..w)
            .map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "))
            .collect();
        if row.contains("[err] boom") {
            found_red_on_stderr_row = (0..w).any(|x| {
                buf.cell((x, y))
                    .is_some_and(|c| c.style().fg == Some(Color::Red))
            });
            break;
        }
    }
    assert!(
        found_red_on_stderr_row,
        "the stderr line should render with a red foreground when colour is on"
    );

    fs::remove_dir_all(&dir).ok();
}

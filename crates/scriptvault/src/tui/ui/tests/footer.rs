// Footer rendering: the status severity classifier, an error status rendered
// red (success not), the persistent key hints surviving a transient status, the
// narrow-width swap to the compact hints, and the shared job-status segment.

use super::super::*;
use super::{fixture_app, render_to_rows};
use crate::tui::app::App;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::fs;

#[test]
fn status_error_classifier_flags_failures_not_successes() {
    // The renderer-side severity heuristic: failure-wording is red, plain
    // results are not. Locks the keyword set so a reworded message can't
    // silently lose (or gain) the error style.
    assert!(status_is_error("clipboard error: denied"));
    assert!(status_is_error("editor not found"));
    assert!(status_is_error("reload skipped (too soon)"));
    assert!(status_is_error("no result selected"));
    assert!(!status_is_error("copied: /tmp/x.sh"));
    assert!(!status_is_error("★ favorited: /tmp/x.sh"));
    assert!(!status_is_error(""));
}

// A failure status renders with a RED foreground (colour on); a success does
// not. Inspects cell styles directly — a text snapshot can't carry the hue.
#[test]
fn error_status_renders_red_success_does_not() {
    use ratatui::style::Color;
    let (mut app, dir) = fixture_app();

    // Scope the red check to the STATUS ROW's MESSAGE half only — the status row
    // is the second row from the bottom (footer is 2 rows: status, then hints),
    // and the message owns the left part (the job segment sits on the right). A
    // wider check could catch an unrelated red (e.g. a failed-run ✗ segment).
    let status_row_has_red = |app: &App| -> bool {
        let (w, h) = (160u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let y = h - 2; // status row (key hints are the last row)
        let msg_w = w - 28; // message half (the job segment is the right 28 cols)
        (0..msg_w).any(|x| {
            buf.cell((x, y))
                .is_some_and(|c| c.style().fg == Some(Color::Red))
        })
    };

    app.set_status("clipboard error: denied");
    assert!(
        status_row_has_red(&app),
        "an error status should render red"
    );

    app.set_status("copied: /tmp/x.sh");
    assert!(
        !status_row_has_red(&app),
        "a success status must not render red"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn persistent_key_hints_survive_an_action_status() {
    let (mut app, dir) = fixture_app();
    app.set_status("copied: /tmp/x.sh");
    let screen = render_to_rows(&app, 160, 24);
    assert!(
        screen.contains("copied: /tmp/x.sh"),
        "transient status missing\n{screen}"
    );
    // The shared KeyHints render "<key> <label>" pairs; "search" is the label of
    // the first pair (key "type"), so its presence proves the hint strip survives.
    assert!(
        screen.contains("search"),
        "persistent key hints must remain visible after a status\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

// The hint line is two-tone: keys are accented (cyan) and their labels are dim,
// so the eye parses `key → meaning` pairs. Inspect the hint row's cells — the
// accent can't survive a text snapshot. A future flattening back to one dim span
// would drop the cyan and trip this. (suite-ui's KeyHints paints keys with the
// accent and labels dim; this guards that ScriptVault renders through it.)
#[test]
fn hint_keys_are_accented_labels_are_not() {
    use ratatui::style::Color;
    let (app, dir) = fixture_app();
    let (w, h) = (120u16, 24u16);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let y = h - 1; // hint row is the last footer row

    let cyan_cells = (0..w)
        .filter(|&x| {
            buf.cell((x, y))
                .is_some_and(|c| c.style().fg == Some(Color::Cyan))
        })
        .count();
    // A non-space cell that is NOT cyan — i.e. a dim label glyph.
    let dim_label_cell = (0..w).any(|x| {
        buf.cell((x, y))
            .is_some_and(|c| c.symbol().trim() != "" && c.style().fg != Some(Color::Cyan))
    });

    assert!(cyan_cells > 0, "hint keys should be accented (cyan)");
    assert!(dim_label_cell, "hint labels should stay dim (not all cyan)");
    fs::remove_dir_all(&dir).ok();
}

// The narrow footer swaps in the compact hint set so it never truncates. The
// short set keeps "move"/"actions"/"help"/"quit" but drops the full set's
// "commands" pair — so its absence is what distinguishes the two.
#[test]
fn narrow_footer_uses_the_short_key_hints() {
    let (app, dir) = fixture_app();
    let screen = render_to_rows(&app, 60, 20); // narrow per layout_areas (<72)
    assert!(
        screen.contains("move"),
        "narrow footer should show the short hints\n{screen}"
    );
    assert!(
        !screen.contains("commands"),
        "narrow footer must drop the full-only 'commands' hint\n{screen}"
    );
    fs::remove_dir_all(&dir).ok();
}

// The shared job-status segment: idle by default, "running <name>" while a live
// run streams, and "<name> — done" once it finishes cleanly. This is the suite-ui
// StatusBar driven by App::job_state — the new shared usage this footer adds.
#[test]
fn job_status_segment_tracks_the_live_run_lifecycle() {
    let (mut app, dir) = fixture_app();

    // Idle at startup (nothing has run yet).
    let idle = render_to_rows(&app, 160, 24);
    assert!(
        idle.contains("idle"),
        "footer should show idle status\n{idle}"
    );

    // A live run streaming → "running deploy.sh".
    let path = dir.join("deploy.sh");
    app.set_live_run_path(Some(path.clone()));
    let running = render_to_rows(&app, 160, 24);
    assert!(
        running.contains("running") && running.contains("deploy.sh"),
        "footer should show the running job\n{running}"
    );

    // Finishing cleanly (exit 0) → "deploy.sh — done".
    let _ = app.take_live_run_path();
    app.finish_job(&path, Some(0));
    let done = render_to_rows(&app, 160, 24);
    assert!(
        done.contains("deploy.sh") && done.contains("done"),
        "footer should show the finished job\n{done}"
    );

    fs::remove_dir_all(&dir).ok();
}

// The output pane (toggle, bounded buffer, scroll), the typed live-run event
// stream (`apply_run_event`), the one-live-run-at-a-time guard, and printed-path
// recording. These drive the exact helpers the event loop uses, so they exercise
// real logic without spawning a process or owning a TTY.

use super::fixture_app;
use crate::tui::app::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;

#[test]
fn print_path_records_and_flushes() {
    let (mut app, dir) = fixture_app();
    let path = app
        .selected_result()
        .unwrap()
        .entry
        .path
        .display()
        .to_string();
    app.record_printed_path(path.clone());
    assert_eq!(app.printed_paths, vec![path]);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_pane_toggle_seeds_from_last_and_push_bounds() {
    let (mut app, dir) = fixture_app();
    assert!(!app.is_showing_output());
    assert!(app.output_lines().is_empty());

    // Toggle on with no prior output -> stays empty (no selected last_out in fresh fixture)
    app.toggle_output_pane();
    assert!(app.is_showing_output());

    // Simulate live/capture feeding lines
    for i in 0..300 {
        app.push_output_line(format!("log line {}", i));
    }
    // Bounded
    assert!(app.output_lines().len() <= 256);
    // Latest preserved (tail)
    assert!(app.output_lines().last().unwrap().text.contains("299"));

    // Toggle off
    app.toggle_output_pane();
    assert!(!app.is_showing_output());

    fs::remove_dir_all(&dir).ok();
}

// --- Phase 3 (E): output pane scroll -----------------------------------------
#[test]
fn output_scroll_clamps_and_resets() {
    let (mut app, dir) = fixture_app();
    for i in 0..10 {
        app.push_output_line(format!("line {i}"));
    }
    assert_eq!(app.output_scroll(), 0, "starts pinned to the tail");

    // Scroll up (older) by a positive delta; clamped to len-1 = 9.
    app.scroll_output(4);
    assert_eq!(app.output_scroll(), 4);
    app.scroll_output(100);
    assert_eq!(app.output_scroll(), 9, "cannot scroll above the buffer");

    // Scroll back down toward the tail; clamped at 0.
    app.scroll_output(-3);
    assert_eq!(app.output_scroll(), 6);
    app.scroll_output(-100);
    assert_eq!(app.output_scroll(), 0, "cannot scroll below the tail");

    // Re-scroll, then a fresh run / clear resets to the tail.
    app.scroll_output(5);
    app.clear_live_output();
    assert_eq!(app.output_scroll(), 0, "clear resets scroll to tail");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn shift_pageup_scrolls_output_only_when_pane_shown() {
    let (mut app, dir) = fixture_app();
    for i in 0..50 {
        app.push_output_line(format!("line {i}"));
    }

    // Pane hidden: Shift+PageUp is inert (doesn't scroll, doesn't page the list).
    let shift_pgup = KeyEvent::new(KeyCode::PageUp, KeyModifiers::SHIFT);
    app.handle_key(shift_pgup);
    assert_eq!(app.output_scroll(), 0, "inert while pane hidden");

    // Pane shown: Shift+PageUp scrolls the output up (older).
    app.toggle_output_pane();
    app.handle_key(shift_pgup);
    assert!(app.output_scroll() > 0, "scrolls up when pane is shown");

    // Shift+PageDown brings it back toward the tail.
    let shift_pgdn = KeyEvent::new(KeyCode::PageDown, KeyModifiers::SHIFT);
    let up = app.output_scroll();
    app.handle_key(shift_pgdn);
    assert!(app.output_scroll() < up, "scrolls back toward the tail");

    fs::remove_dir_all(&dir).ok();
}

// --- Phase 3: typed live-run events (the RunEvent refactor) ------------------
// These two tests are the discriminators the typed-enum change exists to satisfy.
// They drive `apply_run_event` (the exact helper the event loop's drain uses) so
// they exercise the real logic without spawning a process or owning a TTY.

#[test]
fn live_run_records_real_nonzero_exit_and_no_marker_leak() {
    use crate::tui::actions::{RunCompletion, RunEvent};
    let (mut app, dir) = fixture_app();
    let path = dir.join("deploy.sh");

    // Feed a realistic event stream: some stdout, some stderr, then a NONZERO exit.
    // None of these should report "finished" except the Done event.
    assert_eq!(
        app.apply_run_event(RunEvent::Stdout("building".into())),
        None
    );
    assert_eq!(
        app.apply_run_event(RunEvent::Stderr("warn: slow".into())),
        None
    );
    let finish = app.apply_run_event(RunEvent::Done(RunCompletion {
        code: Some(42),
        timed_out: false,
    }));
    assert_eq!(
        finish,
        Some(RunCompletion {
            code: Some(42),
            timed_out: false,
        }),
        "Done must surface the real exit code"
    );

    // Stream tagging (increment A): stderr is stored as a STDERR-tagged line with
    // RAW text — the `[err] ` marker is added at render time, never in the buffer.
    let buf = app.output_lines();
    assert_eq!(buf[0].stream, OutputStream::Stdout);
    assert_eq!(buf[0].text, "building");
    assert_eq!(buf[1].stream, OutputStream::Stderr);
    assert_eq!(buf[1].text, "warn: slow", "no [err] prefix in the buffer");

    // Replicate the loop's finish bookkeeping with the surfaced code.
    let exit = finish.and_then(|completion| completion.code);
    let joined = app.output_text();
    app.record_run_with_status(&path, exit, Some(joined.clone()))
        .unwrap();

    // BUG 1 fixed: the real nonzero code is recorded, not the old always-0.
    let recents = app.scriptvault.recents();
    let entry = recents
        .iter()
        .find(|r| r.path == path)
        .expect("run should be recorded");
    assert_eq!(entry.last_exit, Some(42));

    // BUG 2 fixed: the `[done exit=...]` control marker never entered the buffer
    // or the persisted output — only real script lines are stored.
    assert!(
        !joined.contains("[done exit"),
        "control marker leaked into output: {joined:?}"
    );
    let stored = entry.last_output.as_deref().unwrap_or("");
    assert!(!stored.contains("[done exit"));
    assert!(stored.contains("building"));
    // Persisted history uses raw text (tags dropped) — no `[err] ` decoration.
    assert!(stored.contains("warn: slow"));
    assert!(
        !stored.contains("[err]"),
        "stored history must not carry the marker"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn live_run_disconnect_without_done_finishes_with_unknown_exit() {
    use crate::tui::actions::RunEvent;
    let (mut app, dir) = fixture_app();
    let path = dir.join("deploy.sh");

    // Only stdout arrives, then the channel disconnects (waiter never sent Done).
    assert_eq!(
        app.apply_run_event(RunEvent::Stdout("partial".into())),
        None
    );
    // The loop treats a bare disconnect as finish-with-unknown-code.
    let exit: Option<i32> = None;
    app.record_run_with_status(&path, exit, Some(app.output_text()))
        .unwrap();

    let recents = app.scriptvault.recents();
    let entry = recents.iter().find(|r| r.path == path).unwrap();
    assert_eq!(entry.last_exit, None); // unknown, not a fabricated 0
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn live_run_keeps_trailing_output_arriving_after_done() {
    // Regression for the trailing-output race: the waiter thread can send
    // `Done` while the reader threads are still flushing buffered lines. The
    // old loop finished the run on Done+Empty and dropped those lines.
    // `drain_live` must keep the run alive until the channel DISCONNECTS,
    // appending everything that arrives in between — Done is data (the exit
    // code), not the finish signal.
    use crate::tui::actions::{RunCompletion, RunEvent};
    let (mut app, dir) = fixture_app();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut live_exit = None;

    // Tick 1: one line, then Done — senders still alive (readers mid-flush).
    tx.send(RunEvent::Stdout("early".into())).unwrap();
    tx.send(RunEvent::Done(RunCompletion {
        code: Some(7),
        timed_out: false,
    }))
    .unwrap();
    assert!(
        !crate::tui::actions::drain_live(&rx, &mut app, &mut live_exit),
        "Done must not finish the run while senders are alive"
    );
    assert_eq!(
        live_exit,
        Some(RunCompletion {
            code: Some(7),
            timed_out: false,
        }),
        "Done's exit code is stashed"
    );

    // Tick 2: trailing lines flushed AFTER Done, then EOF (all senders drop).
    tx.send(RunEvent::Stdout("trailing-out".into())).unwrap();
    tx.send(RunEvent::Stderr("trailing-err".into())).unwrap();
    drop(tx);
    assert!(
        crate::tui::actions::drain_live(&rx, &mut app, &mut live_exit),
        "disconnect is the completion signal"
    );

    let text = app.output_text();
    assert!(text.contains("early"));
    assert!(
        text.contains("trailing-out"),
        "stdout after Done must be kept"
    );
    assert!(
        text.contains("trailing-err"),
        "stderr after Done must be kept"
    );
    assert_eq!(
        live_exit,
        Some(RunCompletion {
            code: Some(7),
            timed_out: false,
        }),
        "exit code must survive to the disconnect tick"
    );

    fs::remove_dir_all(&dir).ok();
}

// --- Phase 3: one-live-run-at-a-time guard -----------------------------------
// Exercises the guard's three real branches via `take_live_run_request` (the
// App method the event loop delegates to), with no TTY and no spawned process.
#[test]
fn live_run_guard_rejects_second_request_while_active() {
    let (mut app, dir) = fixture_app();
    let req = dir.join("deploy.sh");

    // Branch 1: no pending request -> None regardless of active flag, no status churn.
    assert_eq!(app.take_live_run_request(false), None);

    // Branch 2: a request while IDLE -> hand the path back for the shell to spawn.
    app.output.pending_live_run = Some(req.clone());
    assert_eq!(app.take_live_run_request(false), Some(req.clone()));
    // The request was consumed (taken), so it doesn't linger for the next tick.
    assert_eq!(app.take_live_run_request(false), None);

    // Branch 3: a request while a run is ACTIVE -> reject, consume, set status.
    app.output.pending_live_run = Some(req.clone());
    assert_eq!(
        app.take_live_run_request(true),
        None,
        "second run must be rejected while one is active"
    );
    assert!(
        app.status().contains("already active"),
        "rejection must be user-visible, got: {:?}",
        app.status()
    );
    // And the rejected request is gone, not silently re-spawned next tick.
    assert_eq!(app.take_live_run_request(false), None);

    fs::remove_dir_all(&dir).ok();
}

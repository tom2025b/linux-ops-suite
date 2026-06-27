// tui/actions.rs — effects layer: clipboard, suspend+run editor/script, status updates.
// -----------------------------------------------------------------------------
// Seam between pure App and outside (clipboard is UI-only, not core; core
// actions are called after suspend). Failures always become status text.

use anyhow::Result;
use suite_ui::Tui;

use scriptvault_core::ScriptEntry;
use scriptvault_core::actions as core_actions;

use super::app::{ActionKind, App};

// Phase 3 live output: std only (no tokio/async in TUI for simplicity + low dep).
use std::io::{BufRead, BufReader};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Perform the requested action against the app's current selection.
/// Any failure is reported to the status line — the TUI never crashes on a
/// failed action.
pub fn perform(tui: &mut Tui, app: &mut App, kind: ActionKind) {
    // Clone the selected entry so we don't hold an immutable borrow of `app`
    // while we also need to mutate its status. (Empty selection already no-ops
    // in App, but we re-check here for safety.)
    let Some(entry) = app.selected_result().map(|r| r.entry.clone()) else {
        app.set_status("no result selected");
        return;
    };

    match kind {
        ActionKind::CopyPath => match copy_to_clipboard(&entry) {
            Ok(()) => app.set_status(format!("copied: {}", entry.path.display())),
            Err(e) => app.set_status(format!("clipboard error: {e}")),
        },
        ActionKind::PrintPath => {
            app.record_printed_path(entry.path.display().to_string());
            app.set_status(format!("will print on exit: {}", entry.path.display()));
        }
        ActionKind::OpenEditor => {
            // Editor needs the real screen. Suspend, run, resume. The core action
            // returns a typed `ScriptVaultError`; `?` maps it into this layer's
            // `anyhow::Error` at the boundary (core stays anyhow-free).
            let editor = app.editor_command();
            let res = with_suspended_terminal(tui, || {
                core_actions::open_in_editor(&entry, &editor)?;
                Ok(())
            });
            report(app, res, "opened in editor");
        }
        ActionKind::Run => {
            // Run the script in the FOREGROUND on the real terminal (terminal is
            // suspended for the whole closure). After it finishes we pause for a
            // keypress *before* re-entering the alt screen — otherwise ScriptVault
            // clears the screen the instant the child exits and the user only sees
            // a flash. We pause on BOTH success and failure so error output stays
            // readable, then propagate the run's own result for status reporting.
            let res = with_suspended_terminal(tui, || {
                let run_result = core_actions::run(&entry);
                pause_for_keypress(&run_result);
                run_result?;
                Ok(())
            });
            match &res {
                Ok(()) => {
                    // Record the run for "recents" — best-effort. A persistence
                    // failure must NOT turn a successful run into an error; we
                    // just note it on the status line.
                    if let Err(e) = app.record_run_with_status(&entry.path, Some(0), None) {
                        app.set_status(format!("ran script (state not saved: {e})"));
                    } else {
                        // Richer status + refresh cached list so recents view / ordering
                        // updates immediately for daily use.
                        app.refresh_results();
                        let name = entry
                            .path
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "script".into());
                        let status = if let Some(r) = app.recency_summary(&entry.path) {
                            format!("ran {} — {}", name, r)
                        } else {
                            format!("ran {}", name)
                        };
                        app.set_status(status);
                    }
                }
                Err(e) => app.set_status(format!("{e}")),
            }
        }
        ActionKind::ToggleFavorite => {
            let path = entry.path.clone();
            match app.toggle_favorite(&path) {
                Ok(true) => app.set_status(format!("★ favorited: {}", path.display())),
                Ok(false) => app.set_status(format!("unfavorited: {}", path.display())),
                Err(e) => app.set_status(format!("favorite error: {e}")),
            }
        }
        ActionKind::Delete => {
            // Remove the file via core, then rebuild the index so the deleted
            // script drops out of the list. No terminal suspend needed — there's
            // no child process, just a filesystem op + reload.
            match core_actions::delete(&entry) {
                Ok(()) => {
                    app.reload_after_delete();
                    app.set_status(format!("deleted: {}", entry.path.display()));
                }
                Err(e) => app.set_status(format!("delete failed: {e}")),
            }
        }
    }
}

/// Copy a script's absolute path to the system clipboard.
fn copy_to_clipboard(entry: &ScriptEntry) -> Result<()> {
    // `arboard::Clipboard` grabs the OS clipboard; held only briefly.
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(entry.path.display().to_string())?;
    Ok(())
}

/// Run a closure with the terminal SUSPENDED (alternate screen left, raw mode
/// off), then restore it. This is what an editor or a child process needs — it
/// wants the real terminal, not ratatui's alternate screen.
///
/// The leave→run→re-enter (and the post-re-enter `clear`, so ratatui doesn't
/// diff-render against a buffer the child scribbled over) is owned by the shared
/// [`Tui::suspended`] guard, which guarantees re-entry even if a step fails.
/// `suspended` returns `io::Result<T>` where `T` is the closure's own result, so
/// we flatten the two layers: an outer suspend/IO failure or the inner action
/// failure both surface as this layer's `anyhow::Error`.
fn with_suspended_terminal<F>(tui: &mut Tui, f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    tui.suspended(f)? // outer: suspend/re-enter IO error
}

/// After a foreground script finishes, hold the real terminal so its output
/// stays on screen until the user acknowledges — otherwise the TUI re-enters the
/// alt screen immediately and the output is wiped in a flash.
///
/// Called INSIDE the suspended scope (raw mode is off, so stdin is cooked and a
/// single `read_line` blocks until the user presses Enter). The prompt reflects
/// whether the script succeeded so a failure is obvious before returning. All IO
/// here is best-effort: a failed write or read must never turn a successful run
/// into an error, so we ignore the results and always fall through to return.
fn pause_for_keypress(run_result: &Result<(), scriptvault_core::ScriptVaultError>) {
    use std::io::Write;

    let prompt = match run_result {
        Ok(()) => "\n\x1b[1;32m[script finished]\x1b[0m  Press Enter to return to ScriptVault…",
        Err(_) => {
            "\n\x1b[1;31m[script exited with an error — see output above]\x1b[0m  Press Enter to return to ScriptVault…"
        }
    };

    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "{prompt}");
    let _ = stdout.flush();

    // Block until the user presses Enter (or stdin closes / EOF).
    let mut discard = String::new();
    let _ = std::io::stdin().read_line(&mut discard);
}

/// Report an action result to the status line.
fn report(app: &mut App, result: Result<()>, ok_msg: &str) {
    match result {
        Ok(()) => app.set_status(ok_msg.to_string()),
        // Surface the failure calmly in the status line rather than crashing.
        Err(e) => app.set_status(format!("{e}")),
    }
}

// Clipboard + suspend/resume live here (binary-only). Core actions stay pure.
// See ARCHITECTURE.md.

/// A single message from a live-capture run, sent over the channel to the event
/// loop. TYPED on purpose: the channel used to be `String` with magic prefixes
/// (`[err] `, `[done exit=...]`), which conflated *data* (script output) with
/// *control* (which stream, the exit code). That conflation caused two bugs —
/// the exit code was never parsed back out (always recorded as 0) and the
/// `[done]` marker leaked into the persisted output. With a typed enum the
/// payload speaks for itself and the drain loop matches instead of sniffing.
pub enum RunEvent {
    /// One line from the child's stdout (raw — no display decoration here).
    Stdout(String),
    /// One line from the child's stderr (the view layer adds any `[err]` prefix).
    Stderr(String),
    /// The child has exited; carries its real exit code (`None` if killed by a
    /// signal or otherwise unreported) plus whether ScriptVault timed it out.
    /// This is *control*, never shown as output.
    Done(RunCompletion),
}

/// Completion metadata for a live run. Keeping timeout separate from the exit
/// code prevents "killed by timeout" from collapsing into a generic unknown exit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunCompletion {
    pub code: Option<i32>,
    pub timed_out: bool,
}

/// A live-capture run in flight: the receiving end of its event stream plus a
/// shared handle on the child so dropping the run KILLS it. Without the kill,
/// quitting the TUI mid-run orphaned the script to init with no record of it
/// (the same bug RexOps' JobHandle fixed) — and there is no user-cancel of a
/// live run, so drop-on-quit is the only way out of a long-running script.
pub struct LiveRun {
    /// Typed events from the three capture threads; the event loop drains this.
    pub rx: mpsc::Receiver<RunEvent>,
    /// The child, shared with the waiter thread. The waiter only ever polls
    /// `try_wait` under a brief lock — never a blocking `wait` — so `Drop` can
    /// always take the lock to kill.
    child: Arc<Mutex<core_actions::ManagedChild>>,
}

impl Drop for LiveRun {
    fn drop(&mut self) {
        // Kill + reap, best-effort. On the normal finish path the waiter has
        // already reaped via try_wait, so kill errors (ignored — std refuses to
        // kill an already-waited child) and wait returns the cached status.
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill_and_reap();
        }
    }
}

/// Owns the live-run lifecycle on the event-loop side, so the loop itself stays a
/// clean draw/tick/input cycle instead of interleaving run plumbing inline.
///
/// One `LiveRunner` lives for the whole loop. Each iteration the loop calls
/// [`pump`](Self::pump) (drain output into the pane; on disconnect, do finish
/// bookkeeping) and [`start_pending`](Self::start_pending) (honor a queued "run
/// live"), and uses [`poll_timeout`](Self::poll_timeout) for its input poll. The
/// in-flight [`LiveRun`] is held in `run`; dropping the runner (loop exit / quit)
/// drops it, which kills+reaps the child — so quitting mid-run can't orphan a
/// script. The decision logic still lives on `App` (`take_live_run_request`,
/// `apply_run_event`, `finish_job`); this only owns the loop-side plumbing that
/// used to be smeared across `event_loop`.
pub struct LiveRunner {
    /// The in-flight run, or `None` when idle. `Some` is this side's truth for
    /// "a run is active" (mirrored to `App` via `set_live_run_path`).
    run: Option<LiveRun>,
    /// Exit metadata from the run's `Done` event, held until the channel
    /// disconnects — `Done` and the readers' final lines can land on different
    /// ticks, so it must survive across iterations.
    completion: Option<RunCompletion>,
}

impl LiveRunner {
    pub fn new() -> Self {
        Self {
            run: None,
            completion: None,
        }
    }

    /// Is a live run currently in flight? Drives the poll timeout and the
    /// one-run-at-a-time guard.
    pub fn is_active(&self) -> bool {
        self.run.is_some()
    }

    /// How long the event loop should block on input. Short while a run is live
    /// so we keep draining + redrawing the pane; effectively "forever" when idle
    /// so keys feel instant and CPU stays at zero (the poll still returns on a
    /// real event).
    pub fn poll_timeout(&self) -> Duration {
        if self.is_active() {
            Duration::from_millis(40)
        } else {
            Duration::from_secs(3600)
        }
    }

    /// Drain buffered live events into the app, then — if the channel has
    /// disconnected (the race-free "no more output" signal) — run finish
    /// bookkeeping and drop the run.
    ///
    /// The finish path records the run with its REAL exit code (from the typed
    /// `Done`, not a disconnect guess), updates the footer job status, persists
    /// the joined output snippet, refreshes the results, and sets a status line.
    /// A no-op when idle.
    pub fn pump(&mut self, app: &mut App) {
        let Some(run) = &self.run else { return };

        // Disconnect — not `Done` — is the finish signal: `Done` can arrive while
        // the reader threads are still flushing buffered lines, so we wait for
        // every sender to drop (see `drain_live`).
        if !drain_live(&run.rx, app, &mut self.completion) {
            return;
        }

        if let Some(p) = app.take_live_run_path() {
            // Exit code comes straight from the typed `Done`; a bare disconnect
            // (waiter died) leaves it genuinely unknown (`None`).
            let completion = self.completion.take().unwrap_or_default();
            let exit = completion.code;
            // Record the footer outcome BEFORE history: it survives `live_run_path`
            // being taken, so the footer keeps the last run's ✓/✗ once it's over.
            app.finish_job(&p, exit);
            // `output_text()` joins the buffer's raw text (no stream tags / control
            // marker), so the stored snippet is pure script output.
            let joined = app.output_text();
            let _ = app.record_run_with_status(&p, exit, Some(joined)); // state layer bounds it
            app.refresh_results();
            if completion.timed_out {
                app.set_status("live run timed out (recorded to history)");
            } else {
                app.set_status("live run finished (recorded to history)");
            }
        }

        // Dropping the run here is a no-op kill: the child already exited and was
        // reaped by the waiter (the disconnect proves it).
        self.run = None;
        self.completion = None; // never leak a stale code into the next run
    }

    /// Honor a pending "run live" request from the palette / key handling, if any.
    ///
    /// `App::take_live_run_request` folds in the one-run-at-a-time guard: it
    /// returns a path to spawn ONLY when idle, otherwise sets an "already active"
    /// status itself. On spawn we set the path on `App` (which flips the footer to
    /// `Running`) and remember the run; a spawn failure surfaces on the status
    /// line. A no-op when there's no pending request.
    pub fn start_pending(&mut self, app: &mut App) {
        let Some(path) = app.take_live_run_request(self.is_active()) else {
            return;
        };
        match start_live_capture(&path) {
            Ok(run) => {
                app.set_live_run_path(Some(path));
                self.run = Some(run);
                app.set_status("live run started — output streaming to pane (^L to toggle)");
            }
            Err(err) => {
                app.set_status(format!("failed to spawn live capture: {err}"));
            }
        }
    }
}

impl Default for LiveRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Drain buffered live-channel events into the app, stashing the `Done` metadata
/// into `completion`. Returns `true` only once the channel DISCONNECTS — the
/// race-free "no more output" signal, since `Done` can arrive while the readers
/// are still flushing buffered lines.
///
/// `pub(super)` so the trailing-output-race regression test can drive it directly
/// (it asserts the exact "disconnect, not Done, is the finish signal" contract).
pub(super) fn drain_live(
    rx: &mpsc::Receiver<RunEvent>,
    app: &mut App,
    completion: &mut Option<RunCompletion>,
) -> bool {
    loop {
        match rx.try_recv() {
            Ok(event) => {
                if let Some(c) = app.apply_run_event(event) {
                    *completion = Some(c);
                }
            }
            // Nothing buffered but senders alive: more may come, keep running.
            Err(mpsc::TryRecvError::Empty) => return false,
            // Every sender dropped and the buffer is empty: output is final.
            // (A disconnect with no Done — waiter died — still finishes; the
            // caller records an unknown exit code.)
            Err(mpsc::TryRecvError::Disconnected) => return true,
        }
    }
}

/// Phase 3: start a non-interactive live-capture run that streams stdout/stderr
/// over an mpsc channel of typed [`RunEvent`]s so the TUI event loop can drain
/// them into App's output buffer while staying responsive (no terminal suspend).
///
/// Uses 3 threads (stdout reader, stderr reader, exit waiter). The waiter polls
/// `try_wait`, sends `RunEvent::Done(completion)` once the child exits, then
/// drops its sender — so the receiver sees a clean disconnect after the final event. The
/// child itself lives in the returned [`LiveRun`], whose `Drop` kills and reaps
/// it (quitting mid-run cancels the script instead of orphaning it).
/// Returns an error on immediate spawn failure (caller reports via status).
///
/// Stdin is nulled: live runs are intentionally non-interactive (scripts that
/// need TTY/ input should use normal ^R which suspends and gives full TTY).
pub fn start_live_capture(path: &std::path::Path) -> Result<LiveRun> {
    let mut child = core_actions::spawn_live_capture(path)?;

    let (tx, rx) = mpsc::channel::<RunEvent>();
    let tx_err = tx.clone();

    // stdout reader thread: each line becomes a Stdout event (no decoration).
    if let Some(out) = child.take_stdout() {
        let tx_out = tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(std::result::Result::ok) {
                if tx_out.send(RunEvent::Stdout(line)).is_err() {
                    break; // receiver gone (app quitting) — stop reading
                }
            }
        });
    }

    // stderr reader thread: each line becomes a Stderr event (the view layer
    // decides how to mark it — the control/data split lives in the type now).
    if let Some(err) = child.take_stderr() {
        thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(std::result::Result::ok) {
                if tx_err.send(RunEvent::Stderr(line)).is_err() {
                    break; // receiver gone (app quitting) — stop reading
                }
            }
        });
    }

    // Waiter thread: polls try_wait under a brief lock (a blocking `wait` would
    // hold the lock until exit and deadlock LiveRun's kill), enforces the shared
    // core timeout, sends typed completion, then drops tx on scope exit. If
    // LiveRun::drop kills+reaps first (quit path), try_wait reports the cached
    // status and the failed send is ignored.
    let child = Arc::new(Mutex::new(child));
    let waiter_child = Arc::clone(&child);
    thread::spawn(move || {
        let started = Instant::now();
        let completion = loop {
            {
                let Ok(mut guard) = waiter_child.lock() else {
                    return; // poisoned: a holder panicked; nothing sane to report
                };
                match guard.try_wait() {
                    Ok(Some(status)) => {
                        break RunCompletion {
                            code: status.code(),
                            timed_out: false,
                        };
                    }
                    Ok(None) => {}
                    Err(_) => break RunCompletion::default(),
                }
            }

            if started.elapsed() >= core_actions::DEFAULT_RUN_TIMEOUT {
                let code = match waiter_child.lock() {
                    Ok(mut guard) => guard.kill_and_reap(),
                    Err(_) => None,
                };
                let _ = tx.send(RunEvent::Stderr(format!(
                    "script timed out after {}",
                    core_actions::format_duration(core_actions::DEFAULT_RUN_TIMEOUT)
                )));
                break RunCompletion {
                    code,
                    timed_out: true,
                };
            }

            thread::sleep(std::time::Duration::from_millis(25));
        };
        let _ = tx.send(RunEvent::Done(completion));
    });

    Ok(LiveRun { rx, child })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn dropping_a_live_run_kills_and_reaps_the_child() {
        // Regression for "quit during a live run orphans the script": dropping
        // the LiveRun (which is what quitting does — the event loop's binding
        // goes out of scope) must kill the child AND reap it. `yes` runs
        // forever, so the child is provably still alive at drop. A reaped pid
        // has no /proc entry (a zombie still would), so the assert proves both
        // the kill and the reap.
        let run = start_live_capture(Path::new("yes")).expect("spawn yes");
        let pid = run.child.lock().expect("lock child").id();
        let proc_path = format!("/proc/{pid}");
        assert!(
            Path::new(&proc_path).exists(),
            "child must be alive before drop"
        );

        drop(run);

        assert!(
            !Path::new(&proc_path).exists(),
            "child must be killed and reaped by Drop, not orphaned"
        );
    }
}

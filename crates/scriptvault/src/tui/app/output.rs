// tui/app/output.rs — live/captured output pane + mouse-to-select.
// -----------------------------------------------------------------------------
// These `impl App` methods own the Phase 3 output pane (a toggleable, tailing
// log fed by run-capture or a live streaming run) and the mouse click-to-select
// mapping. They live in their own file purely for readability; they share the
// same `App` (defined in mod.rs) and reach its private fields directly, because
// a child module can always see its parent's private items.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::App;

// The typed live-capture event. Lives in the sibling `actions` module (the
// effects layer that spawns the run); we match on it here in the view layer.
use crate::tui::actions::{RunCompletion, RunEvent};

/// The display name for the footer job status: the script's file name, or the
/// full path string as a fallback (a path with no file name is degenerate but
/// shouldn't render empty).
fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Upper bound on the rolling output buffer (a poor-man's ring buffer): once the
/// pane holds more than this many lines, the oldest are dropped. Keeps memory
/// bounded on a chatty live run while still showing a generous tail.
const OUTPUT_BUFFER_LINES: usize = 256;

/// Which stream a captured output line came from. Stored alongside the text so
/// the renderer can colour stderr (and add a marker under NO_COLOR) WITHOUT the
/// old `[err] ` string prefix bleeding into the buffer or persisted history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// One line in the output pane: its source stream plus the raw text (no display
/// decoration — prefixing/colour is applied at draw time by the renderer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub stream: OutputStream,
    pub text: String,
}

/// All state for the Phase 3 live/captured output pane, grouped out of `App`'s
/// top level. These five fields move and change together (toggle the pane, push
/// lines, scroll, track a live run), so bundling them shrinks `App`'s field list
/// and names the cluster. Lives here next to `OutputLine`/`OutputStream` — the
/// types it is built from — keeping the whole output-pane concern in one file.
///
/// `Default` gives the same initial values the old inline constructor set
/// (hidden pane, empty buffer, pinned to tail, no live run), so the `App`
/// constructor just writes `output: OutputState::default()`.
#[derive(Debug, Default)]
pub struct OutputState {
    /// Whether the output pane is currently visible (splits the preview area).
    pub show: bool,
    /// Bounded rolling buffer of output lines (populated live or from capture).
    /// Each line is tagged with its source stream so the renderer can colour
    /// stderr. Always tailed in the renderer; the bound keeps memory low.
    pub lines: Vec<OutputLine>,
    /// How many lines the pane is scrolled UP from the tail (0 = pinned to the
    /// bottom, following new output). Clamped to the buffer length at render time.
    pub scroll: usize,
    /// The script path for a live run in progress (set on spawn, taken at finish
    /// to record_run_with_status). `Some` is the App-side truth for "run active".
    pub live_run_path: Option<std::path::PathBuf>,
    /// Set by palette "run live" so the event loop can start the thread job
    /// without growing the `Outcome` enum. Consumed via `take_live_run_request`
    /// (which also applies the one-run-at-a-time guard).
    pub pending_live_run: Option<std::path::PathBuf>,
    /// The job-status segment the footer shows (suite-ui `StatusBar`). It outlives
    /// `live_run_path` — which is taken at finish — so the footer can still report
    /// the LAST run's outcome (`✓`/`✗`) once the run is over. Three states occur in
    /// ScriptVault: nothing run yet (`Idle`), a run streaming (`Running`), and a
    /// finished run (`Finished` ok/failed). There is no user-cancel of a live run,
    /// so `Cancelled` is never produced.
    pub job: JobStatus,
}

/// The footer job status, owning its display name so the renderer can borrow it
/// into a `suite_ui::JobState` each frame. Mirrors the suite states ScriptVault
/// actually reaches; the renderer maps this onto `JobState` in one place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum JobStatus {
    /// No live run has happened yet.
    #[default]
    Idle,
    /// A live run is streaming. `name` is the script's display file name.
    Running { name: String },
    /// The last live run finished. `ok` is true for a clean (exit-0) finish; an
    /// unknown exit (waiter thread died) is treated as a failure, the safe read.
    Finished { name: String, ok: bool },
}

impl App {
    pub fn last_output_for(&self, path: &std::path::Path) -> Option<&str> {
        self.scriptvault
            .recents()
            .iter()
            .find(|r| r.path == path)
            .and_then(|r| r.last_output.as_deref())
    }

    // --- Phase 3: live / captured output pane support (simple, no new deps) ---

    /// Is the dedicated output pane currently visible (causes a vertical split in the
    /// preview column so last/live run output is always in view when toggled).
    pub fn is_showing_output(&self) -> bool {
        self.output.show
    }

    /// The current output buffer (for renderer to tail-display). Each line carries
    /// its source stream so the renderer can style stdout vs stderr.
    pub fn output_lines(&self) -> &[OutputLine] {
        &self.output.lines
    }

    /// The buffer joined into a plain text snippet (for persisting to history /
    /// recents). Stream tags are dropped — stored history is a flat snippet, and
    /// no `[err]`/`[done]` decoration leaks in.
    pub fn output_text(&self) -> String {
        self.output
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Toggle visibility. When turning on and buffer empty, opportunistically seed from
    /// the selected script's last captured output (from recents) if present. Purely UI.
    pub fn toggle_output_pane(&mut self) {
        self.output.show = !self.output.show;
        // Opening the pane always starts at the tail (newest output in view).
        self.output.scroll = 0;
        if self.output.show
            && self.output.lines.is_empty()
            && let Some(sel) = self.selected_result()
            && let Some(prev) = self.last_output_for(&sel.entry.path)
        {
            // Seeded history has no stream tags (it's a flat snippet), so treat
            // every seeded line as stdout for display purposes.
            self.output.lines = prev
                .lines()
                .map(|s| OutputLine {
                    stream: OutputStream::Stdout,
                    text: s.to_string(),
                })
                .collect();
        }
    }

    /// Apply one [`RunEvent`] from a live capture to the output buffer.
    ///
    /// This is the single place that decides how each event affects view state,
    /// kept as a small pure-ish method so the drain loop stays thin AND so it can
    /// be unit-tested without spawning a process or owning a TTY:
    /// - `Stdout` lines are appended as a stdout-tagged line.
    /// - `Stderr` lines are appended as a stderr-tagged line — RAW text, no prefix.
    ///   The `[err] ` marker and colour are now applied at draw time by the
    ///   renderer (so it never enters the buffer or persisted history, and the
    ///   marker still appears under NO_COLOR where colour can't carry it).
    /// - `Done(completion)` is CONTROL, never output: it is not pushed to the buffer.
    ///   Instead we return `Some(completion)` so the caller runs finish-bookkeeping
    ///   (record the run with the *real* exit code). All other events return
    ///   `None`. This is why the `[done exit=...]` marker can no longer leak into
    ///   `output_lines` or the persisted `last_output`.
    pub fn apply_run_event(&mut self, event: RunEvent) -> Option<RunCompletion> {
        match event {
            RunEvent::Stdout(line) => {
                self.push_output_line(line);
                None
            }
            RunEvent::Stderr(line) => {
                self.push_stderr_line(line);
                None
            }
            RunEvent::Done(completion) => Some(completion),
        }
    }

    /// Append a stdout line to the rolling buffer. Used by the live drain and by
    /// the (stdout-only) capture path.
    pub fn push_output_line(&mut self, line: String) {
        self.push_tagged(OutputStream::Stdout, line);
    }

    /// Append a stderr line to the rolling buffer (tagged so the renderer styles it).
    pub fn push_stderr_line(&mut self, line: String) {
        self.push_tagged(OutputStream::Stderr, line);
    }

    /// Append one tagged line and enforce the [`OUTPUT_BUFFER_LINES`] bound.
    fn push_tagged(&mut self, stream: OutputStream, text: String) {
        self.output.lines.push(OutputLine { stream, text });
        if self.output.lines.len() > OUTPUT_BUFFER_LINES {
            let excess = self.output.lines.len() - OUTPUT_BUFFER_LINES;
            self.output.lines.drain(0..excess);
        }
    }

    /// Clear the live output buffer (used before starting a fresh run-live or manual clear).
    /// Also resets the scroll so the fresh run starts pinned to the tail.
    pub fn clear_live_output(&mut self) {
        self.output.lines.clear();
        self.output.scroll = 0;
    }

    /// How many lines the pane is scrolled up from the tail (0 = following output).
    /// The renderer reads this to compute its window.
    pub fn output_scroll(&self) -> usize {
        self.output.scroll
    }

    /// Scroll the output pane by `delta` lines, where a POSITIVE delta moves
    /// toward OLDER output (up, away from the tail) and negative moves back toward
    /// the tail. Clamped to `[0, len-1]`: you cannot scroll above the buffer or
    /// below the tail. Decision logic, so it lives on `App` (TTY-free testable);
    /// the renderer just consumes `output_scroll()`.
    pub fn scroll_output(&mut self, delta: isize) {
        // The largest meaningful offset is "all but one line above the tail"; an
        // empty buffer pins at 0. We clamp here, and the renderer re-clamps to the
        // visible height so a short pane can't show past the top either.
        let max = self.output.lines.len().saturating_sub(1) as isize;
        let next = (self.output.scroll as isize + delta).clamp(0, max);
        self.output.scroll = next as usize;
    }

    /// Take a pending "run live" request, applying the one-run-at-a-time guard.
    ///
    /// The shell passes whether a live run is already in flight (`live_active`,
    /// which mirrors its loop-local `live_rx`). This is decision logic, so it lives
    /// on `App` (per the architecture doctrine: decisions are here, pure and
    /// TTY-free testable) rather than inline in the event loop:
    /// - no pending request           -> `None` (nothing to do)
    /// - request, but a run is active  -> reject with a visible status, return
    ///   `None` (the request is dropped; starting a second run would orphan the
    ///   first child's threads by overwriting `live_rx`)
    /// - request, and idle             -> `Some(path)`, the caller spawns it
    pub fn take_live_run_request(&mut self, live_active: bool) -> Option<std::path::PathBuf> {
        let path = self.output.pending_live_run.take()?;
        if live_active {
            self.set_status("a live run is already active — wait for it to finish (^L to watch)");
            None
        } else {
            Some(path)
        }
    }

    /// Remember which path is feeding the current live output (for finish-time
    /// record). Setting a path also flips the footer job status to `Running` with
    /// the script's display name; clearing it leaves the status untouched (the
    /// finish path sets `Finished` explicitly so the outcome survives).
    pub fn set_live_run_path(&mut self, path: Option<std::path::PathBuf>) {
        if let Some(p) = &path {
            self.output.job = JobStatus::Running {
                name: display_name(p),
            };
        }
        self.output.live_run_path = path;
    }

    /// Record the outcome of the just-finished live run so the footer can show
    /// `✓`/`✗` after it ends. Called from the event loop's finish bookkeeping with
    /// the run's path and exit code (`None` = unknown, treated as a failure). Keeps
    /// the name from the path so the segment reads "<name> — done/failed".
    pub fn finish_job(&mut self, path: &std::path::Path, exit: Option<i32>) {
        self.output.job = JobStatus::Finished {
            name: display_name(path),
            ok: exit == Some(0),
        };
    }

    /// The footer job status as a `suite_ui::JobState`, borrowing the owned name.
    /// The single place ScriptVault's job model is mapped onto the shared widget's
    /// states — `Idle`/`Running`/`Done`. (`Cancelled` is never produced: there is
    /// no user-cancel of a live run.)
    pub fn job_state(&self) -> suite_ui::JobState<'_> {
        match &self.output.job {
            JobStatus::Idle => suite_ui::JobState::Idle,
            JobStatus::Running { name } => suite_ui::JobState::Running { name },
            JobStatus::Finished { name, ok } => suite_ui::JobState::Done { name, ok: *ok },
        }
    }

    /// Take (and clear) the live path so we only record once on done.
    pub fn take_live_run_path(&mut self) -> Option<std::path::PathBuf> {
        self.output.live_run_path.take()
    }

    /// Is a live run streaming right now? `live_run_path` is set on spawn and
    /// taken at finish, so it is the App-side truth for "a run is active" — the
    /// renderer reads this to show the live activity marker in the output pane.
    pub fn live_active(&self) -> bool {
        self.output.live_run_path.is_some()
    }

    /// Store the list pane's outer Rect (computed from the exact same layout_areas used in
    /// render). Called every draw so subsequent mouse events have up-to-date coords even
    /// after resizes, narrow terminals, or collapsed search bar.
    pub fn set_list_rect(&mut self, rect: Rect) {
        self.list_rect = Some(rect);
    }

    /// Handle mouse event for Phase 3 polish (click to select list item).
    /// Now uses the live list_rect from render for *exact* mapping instead of magic numbers.
    /// Content rows start at rect.y + 1 (after the titled block's top border line).
    /// We only care about Y for index; X is used as a loose "inside the list column" guard
    /// so clicks in the preview half don't select list rows.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(rect) = self.list_rect
        {
            let content_top = rect.y + 1; // first data row after top border + title
            let content_bottom = rect.y + rect.height.saturating_sub(1);
            let in_list_column =
                mouse.column >= rect.x && mouse.column < rect.x + rect.width.saturating_sub(1);
            if in_list_column && mouse.row >= content_top && mouse.row < content_bottom {
                let idx = (mouse.row - content_top) as usize;
                if idx < self.results.len() {
                    self.selected = Some(idx);
                }
            }
        }
    }
}

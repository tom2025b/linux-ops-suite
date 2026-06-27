use std::io::{self, Write}; // stdin reads + flushing the prompt before input
use std::path::Path;

use crate::core::executor::{self, CheckStatus, ExecutionOptions, INTERACTIVE_TIMEOUT};
use crate::core::loader;
use crate::core::protocol::{Protocol, Step, StepKind};
use crate::core::session::{Session, StepStatus, now_secs};
use crate::core::store;

// `dir` is the protocols directory; `sessions_dir` is where the finished record
// is saved (resolved by the CLI layer, default ~/.proto/sessions). `feed_dir` is
// where the Workstate feed is (re)written; `write_feed` is false under --no-feed.
// `id` is the protocol to run.
pub fn handle(
    dir: &Path,
    sessions_dir: &Path,
    feed_dir: &Path,
    write_feed: bool,
    id: &str,
) -> anyhow::Result<()> {
    // Locate + validate the requested protocol (find does both, or NotFound).
    let protocol = loader::find(dir, id)?;

    // Print the protocol header so the operator knows what they're starting.
    print_header(&protocol);

    // Build a fresh, all-Pending session from the protocol (the recipe->run
    // bridge). We'll fill in each step's outcome as we go.
    let mut session = Session::new(&protocol);

    // Walk steps in order. We zip the protocol steps with the session's result
    // slots (same order, same length) so each prompt updates the matching result.
    let total = protocol.step_count();
    for (index, step) in protocol.steps.iter().enumerate() {
        let position = index + 1; // 1-based for display

        // Render the step block, then ask the right question for its kind.
        print_step(position, total, step);
        let status = prompt_for_step(step)?;

        // Echo what we recorded so the operator sees the outcome land, then offer
        // an OPTIONAL note. Both go AFTER the answer, each on its own line — no
        // more gluing the note prompt onto the answer prompt.
        println!("    → {}", status_word(status));
        let note = prompt_for_note()?;

        // Record the outcome into the matching session slot.
        let result = &mut session.steps[index];
        result.status = status;
        result.answered_at = Some(now_secs());
        result.note = note;
    }

    // Every step now has an outcome; stamp the finish time.
    if session.is_complete() {
        session.finished_at = Some(now_secs());
    }
    // Refresh generated_at to "now" — this is when the file was actually written,
    // and it's also what the store derives the session id/filename from.
    session.generated_at = now_secs();

    // Print the summary, then persist via the shared store.
    print_summary(&protocol, &session);
    let path = store::save(sessions_dir, &session)?;
    let id = store::session_id(&session);
    println!("\nSaved session '{}'", id);
    println!("  file: {}", path.display());
    println!("  open: proto show {}", id);

    // Regenerate the Workstate feed so this run shows up in RexOps with no extra
    // step (unless --no-feed). This is BEST-EFFORT: the session is already safely
    // saved above, so a feed failure must NOT fail the run — we warn on stderr and
    // exit 0. Losing a feed refresh is recoverable (`proto feed` rebuilds it);
    // losing the run record would not be. Hence the deliberate "warn, don't fail".
    if write_feed {
        update_feed(sessions_dir, feed_dir);
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// update_feed — rebuild + write the Workstate feed, warning (not failing) on err.
// -----------------------------------------------------------------------------
// Called after a run's session is persisted. Compiles the feed from all sessions
// and writes it. Any error is reported to stderr and SWALLOWED: the run already
// succeeded, and the feed is a derived, regenerable artifact, so a write hiccup
// here is a warning, not a failure of the command.
fn update_feed(sessions_dir: &Path, feed_dir: &Path) {
    let result = store::build_feed(sessions_dir).and_then(|feed| store::save_feed(feed_dir, &feed));
    match result {
        Ok(feed_path) => println!("  feed: {}", feed_path.display()),
        // eprintln so the warning goes to stderr, separate from the run's stdout.
        Err(e) => eprintln!("  warning: could not update Workstate feed: {e}"),
    }
}

// -----------------------------------------------------------------------------
// print_header — the protocol's identity at the top of a run.
// -----------------------------------------------------------------------------
fn print_header(protocol: &Protocol) {
    println!("=== {} ===", protocol.title);
    if !protocol.description.trim().is_empty() {
        println!("{}", protocol.description.trim());
    }
    let mut meta = format!("{} steps", protocol.step_count());
    if !protocol.version.trim().is_empty() {
        meta.push_str(&format!("  •  protocol v{}", protocol.version.trim()));
    }
    println!("({meta})");
}

// -----------------------------------------------------------------------------
// print_step — the per-step block: progress, title, detail, and any command.
// -----------------------------------------------------------------------------
// A blank line separates steps so the walkthrough doesn't run together. The
// "[3/11]" progress marker tells the operator how far along they are at a glance.
// For a Command step the prompt that follows handles the command (offering to run
// a `command:` one, or reminding the operator to run a bare one themselves), so
// print_step only needs the title + detail here.
fn print_step(position: usize, total: usize, step: &Step) {
    println!("\n[{position}/{total}] {}", step.title);
    if !step.detail.trim().is_empty() {
        println!("    {}", step.detail.trim());
    }
}

// -----------------------------------------------------------------------------
// prompt_for_step — ask the right question for a step, return the status.
// -----------------------------------------------------------------------------
// The accepted answers depend on the kind:
//   * Info        — just press Enter to acknowledge (no pass/fail concept).
//   * ManualCheck — [y]es / [n]o / [s]kip.
//   * Command WITH a `command:` field — Proto OFFERS to run it for you: show the
//                   exact command, ask "Run this command? (y/n)", and on yes
//                   execute it and record pass/fail from the EXIT CODE (no
//                   self-reporting needed). On no, fall back to the manual
//                   y/n/s answer below — auto-run is an offer, not a mandate.
//   * Command WITHOUT a `command:` field — display-only as before: the operator
//                   runs it themselves and answers y/n/s.
fn prompt_for_step(step: &Step) -> anyhow::Result<StepStatus> {
    match step.kind {
        StepKind::Info => {
            // Informational: acknowledgement only. Any input (incl. empty) = read.
            read_line("    [Enter to acknowledge] ")?;
            Ok(StepStatus::Acknowledged)
        }
        StepKind::Command => {
            // If the step carries an exact, runnable command, offer to run it.
            // Some(status) means we ran it (or chose to, and got an outcome);
            // None means the operator declined — fall through to manual y/n/s.
            if let Some(command) = step.command.as_deref() {
                if let Some(status) = offer_to_run(command)? {
                    return Ok(status);
                }
            } else {
                // No runnable command: remind the operator to run it themselves.
                println!("    (you run the command above; Proto only records the result)");
            }
            prompt_yes_no_skip()
        }
        StepKind::ManualCheck => prompt_yes_no_skip(),
    }
}

// -----------------------------------------------------------------------------
// prompt_yes_no_skip — the shared [y]es / [n]o / [s]kip question.
// -----------------------------------------------------------------------------
// Loops until a valid answer so a typo never discards a half-finished run. Used
// by ManualCheck and by Command steps that are display-only or declined.
fn prompt_yes_no_skip() -> anyhow::Result<StepStatus> {
    loop {
        let answer = read_line("    [y]es  [n]o  [s]kip: ")?;
        match answer.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(StepStatus::Passed),
            "n" | "no" => return Ok(StepStatus::Failed),
            "s" | "skip" => return Ok(StepStatus::Skipped),
            // Anything else: explain and re-prompt.
            _ => println!("    Please answer y, n, or s."),
        }
    }
}

// -----------------------------------------------------------------------------
// offer_to_run — show a command, ask to run it, and (on yes) execute it.
// -----------------------------------------------------------------------------
// Returns:
//   * Ok(Some(Passed))  — ran it, exit code 0.
//   * Ok(Some(Failed))  — ran it, non-zero exit OR the process failed to start.
//   * Ok(Some(Skipped)) — operator chose to skip the command outright.
//   * Ok(None)          — operator declined to auto-run; caller falls back to
//                         the manual y/n/s answer (they'll run it themselves).
// We print the EXACT command first so the operator sees precisely what will run
// before consenting — no hidden execution, ever.
fn offer_to_run(command: &str) -> anyhow::Result<Option<StepStatus>> {
    println!("    $ {command}");
    loop {
        let answer = read_line("    Run this command? [y]es  [n]o  [s]kip: ")?;
        match answer.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(Some(run_command(command))),
            // Decline auto-run: let the caller drop to manual y/n/s so the
            // operator can still run it by hand and report the result.
            "n" | "no" => return Ok(None),
            "s" | "skip" => return Ok(Some(StepStatus::Skipped)),
            _ => println!("    Please answer y, n, or s."),
        }
    }
}

// -----------------------------------------------------------------------------
// run_command — execute one command via the shared executor, map result->status.
// -----------------------------------------------------------------------------
// Routes through `executor::run_streaming`, the SAME engine the auto-check flow
// uses, so an interactive `command:` step inherits its timeout and process-group
// kill (a hang is bounded; sh -c grandchildren don't survive a timeout). Output
// is streamed to the terminal — the operator watches it live, as if they'd typed
// it — so the executor runs it through `sh -c` for full shell semantics. The
// outcome is derived from the exit code (0 = passed, else failed); a timeout or a
// spawn failure is reported and recorded as Failed so one bad step never aborts
// the whole run.
fn run_command(command: &str) -> StepStatus {
    // A blank line so the command's own output starts cleanly below the prompt,
    // never glued to the "Run this command?" line.
    println!();

    let options = ExecutionOptions::default().with_timeout(INTERACTIVE_TIMEOUT);
    let outcome = executor::run_streaming(command, &options);

    match outcome.status {
        CheckStatus::Pass => StepStatus::Passed,
        CheckStatus::Fail => {
            // Ran to completion, non-zero exit: report the code (or signal).
            match outcome.exit_code {
                Some(code) => println!("    (exited with code {code})"),
                None => println!("    (terminated by signal)"),
            }
            StepStatus::Failed
        }
        // Timed out, or couldn't even start: report why, record as Failed.
        CheckStatus::Error => {
            if outcome.timed_out {
                let limit = outcome
                    .timeout
                    .map(format_secs)
                    .unwrap_or_else(|| "the time limit".to_string());
                println!("    (timed out after {limit}; process killed)");
            } else if let Some(message) = &outcome.error_message {
                println!("    ({message})");
            }
            StepStatus::Failed
        }
    }
}

// A compact "Nm"/"Ns" for the timeout message (the interactive limit is whole
// minutes, but format any duration sensibly).
fn format_secs(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    // `secs % 60 == 0` rather than `u64::is_multiple_of`, which is only stable
    // since Rust 1.87 — the umbrella workspace's MSRV is 1.85 (clippy's
    // incompatible_msrv lint, denied in CI, flags the newer API).
    if secs >= 60 && secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

// -----------------------------------------------------------------------------
// prompt_for_note — ask for an OPTIONAL free-text note; blank means none.
// -----------------------------------------------------------------------------
// Every step gets the chance to attach context (why it failed, a value observed,
// a follow-up). We trim and return "" for an empty answer, which the session
// model then omits from the JSON entirely — no note, no key.
fn prompt_for_note() -> anyhow::Result<String> {
    let raw = read_line("    note (optional, Enter to skip): ")?;
    Ok(raw.trim().to_string())
}

// -----------------------------------------------------------------------------
// read_line — print a prompt (no newline), flush, and read one line of stdin.
// -----------------------------------------------------------------------------
// Flushing is REQUIRED: stdout is line-buffered, so without an explicit flush
// the prompt text may not appear before we block on input, leaving the user
// staring at a blank line.
fn read_line(prompt: &str) -> anyhow::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?; // force the prompt out before we block on read
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?; // includes the trailing newline
    Ok(buf)
}

// Look up a step's human title by its id, falling back to the id itself if no
// match (shouldn't happen — the session was built from this protocol). A named
// function so the borrow is unambiguously tied to `protocol`, not the &str arg.
fn title_for<'p>(protocol: &'p Protocol, step_id: &str) -> &'p str {
    protocol
        .steps
        .iter()
        .find(|s| s.id == step_id)
        .map(|s| s.title.as_str())
        .unwrap_or("(unknown step)")
}

// -----------------------------------------------------------------------------
// status_word — the lowercase outcome word echoed after an answer.
// -----------------------------------------------------------------------------
fn status_word(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
        StepStatus::Acknowledged => "acknowledged",
        StepStatus::Pending => "pending", // shouldn't happen post-answer
    }
}

// -----------------------------------------------------------------------------
// print_summary — outcome tally plus the follow-up list (failed/skipped steps).
// -----------------------------------------------------------------------------
// The tally comes from the shared `Session::tally()` so the wording matches
// `proto sessions`. We then list any failed or skipped steps by their title —
// the "what still needs attention" view, which is the whole point of a summary.
fn print_summary(protocol: &Protocol, session: &Session) {
    println!("\n--- Summary ---");
    println!("{}", session.tally().summary_line());

    // Map step_id -> title so we can show human titles, not bare ids. The session
    // only stores ids; the protocol we just ran has the matching titles.
    let mut follow_up = Vec::new();
    for r in &session.steps {
        let title = title_for(protocol, &r.step_id);
        match r.status {
            StepStatus::Failed => follow_up.push(("FAIL", title, &r.note)),
            StepStatus::Skipped => follow_up.push(("skip", title, &r.note)),
            _ => {}
        }
    }

    if !follow_up.is_empty() {
        println!("\nNeeds attention:");
        for (mark, title, note) in follow_up {
            println!("  [{mark}] {title}");
            if !note.is_empty() {
                println!("         note: {note}");
            }
        }
    }
}

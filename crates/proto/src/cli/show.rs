use std::path::Path;

use crate::core::session::StepStatus;
use crate::core::store;

pub fn handle(sessions_dir: &Path, id: &str) -> anyhow::Result<()> {
    // Load the one session, or get NotFound (which main.rs prints + exits non-zero).
    let session = store::load(sessions_dir, id)?;

    // --- Header -------------------------------------------------------------
    println!("=== {} ===", session.protocol_title);
    println!("session:   {id}");
    println!("protocol:  {}", session.protocol_id);
    println!(
        "started:   {}",
        session.started_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    match session.finished_at {
        Some(t) => {
            println!("finished:  {}", t.format("%Y-%m-%d %H:%M:%S UTC"));
            // Show how long the walkthrough took when we have both ends. A run is
            // human-paced, so seconds/minutes is the useful resolution.
            println!("duration:  {}", human_duration(t - session.started_at));
        }
        // An incomplete run (quit partway) is a valid, readable state.
        None => println!("finished:  (incomplete)"),
    }
    // Outcome tally — the same one-line summary `sessions` and `run` print.
    println!("outcome:   {}", session.tally().summary_line());

    // --- Steps --------------------------------------------------------------
    println!("\nsteps:");
    for (index, r) in session.steps.iter().enumerate() {
        let position = index + 1;
        // A glyph per status gives a fast visual scan down the list.
        let mark = status_mark(r.status);
        println!("  {position:>2}. {mark}  {}", r.step_id);
        // Show the note indented under its step when present.
        if !r.note.is_empty() {
            println!("          note: {}", r.note);
        }
    }

    Ok(())
}

// Map a status to a short, aligned label + glyph. Kept text-only (no color) so
// the output is readable when piped or in a plain terminal.
fn status_mark(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Passed => "[pass]",
        StepStatus::Failed => "[FAIL]",
        StepStatus::Skipped => "[skip]",
        StepStatus::Acknowledged => "[info]",
        StepStatus::Pending => "[ -- ]",
    }
}

// Format a chrono Duration as a compact human string: "12s", "3m 04s", or
// "1h 02m". A guided run is short, so we don't bother with days.
fn human_duration(d: chrono::Duration) -> String {
    let secs = d.num_seconds().max(0); // clamp any clock skew to a sane floor
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

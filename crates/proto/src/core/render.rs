use crate::core::session::{Session, StepStatus};

// The session id (filename stem) isn't stored inside the Session, so the caller
// passes it in — it's the natural title for the document. Returns a complete
// Markdown string with a trailing newline.
pub fn session_markdown(id: &str, session: &Session) -> String {
    // Build into one String. We size nothing up front — sessions are tiny — and
    // just push sections in reading order: heading, metadata, outcome, steps.
    let mut md = String::new();

    // --- Heading ------------------------------------------------------------
    // The protocol title is the human name; the session id is the unique handle.
    md.push_str(&format!("# {}\n\n", session.protocol_title));

    // --- Metadata list ------------------------------------------------------
    // A compact bullet list of the provenance/timing a reader wants up top.
    md.push_str(&format!("- **Session:** `{id}`\n"));
    md.push_str(&format!("- **Protocol:** `{}`\n", session.protocol_id));
    md.push_str(&format!(
        "- **Started:** {}\n",
        session.started_at.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    match session.finished_at {
        Some(t) => md.push_str(&format!(
            "- **Finished:** {}\n",
            t.format("%Y-%m-%d %H:%M:%S UTC")
        )),
        // Mirror `show`: an incomplete run is a valid, labelled state.
        None => md.push_str("- **Finished:** _(incomplete)_\n"),
    }
    // The shared tally line — same wording the CLI prints everywhere.
    md.push_str(&format!(
        "- **Outcome:** {}\n",
        session.tally().summary_line()
    ));

    // --- Steps table --------------------------------------------------------
    // A Markdown table renders nicely in PRs/wikis and scans top-to-bottom. We
    // include the note column only conceptually — empty notes just render blank.
    md.push_str("\n## Steps\n\n");
    md.push_str("| # | Step | Status | Note |\n");
    md.push_str("|---|------|--------|------|\n");
    for (index, r) in session.steps.iter().enumerate() {
        let pos = index + 1;
        // Escape pipes in free-text so a note containing `|` can't break the
        // table layout — the one Markdown-injection hazard in user-entered text.
        let note = r.note.replace('|', "\\|");
        md.push_str(&format!(
            "| {pos} | `{}` | {} | {} |\n",
            r.step_id,
            status_label(r.status),
            note
        ));
    }

    md
}

// A human word per status for the Markdown table. Lowercase, plain — no glyphs,
// since Markdown is read across many renderers and plain words are safest.
fn status_label(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
        StepStatus::Acknowledged => "acknowledged",
        StepStatus::Pending => "pending",
    }
}

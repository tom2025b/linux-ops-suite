use std::path::Path;

use crate::core::store;

pub fn handle(sessions_dir: &Path) -> anyhow::Result<()> {
    // store::list returns entries newest-first, or an empty Vec if nothing's been
    // run yet (a missing store dir is "no sessions", not a failure).
    let entries = store::list(sessions_dir)?;

    if entries.is_empty() {
        println!("No sessions yet in {}", sessions_dir.display());
        println!("Run one with: proto run <protocol-id>");
        return Ok(());
    }

    println!(
        "{} session(s) in {}:\n",
        entries.len(),
        sessions_dir.display()
    );

    // One block per session. The id leads each block (it's the handle for `show`),
    // then an indented "title • when • outcome" line so the listing scans top to
    // bottom without needing column math across variable-length ids.
    for entry in &entries {
        let s = &entry.session;
        // Human-readable start time (to the minute is enough for a listing).
        let when = s.started_at.format("%Y-%m-%d %H:%M");
        // The shared tally: same renderer `run`'s summary uses, so the wording of
        // "3 passed, 1 failed" is identical wherever outcomes appear.
        let outcome = s.tally().summary_line();
        println!("  {}", entry.id);
        println!("      {}  •  {}  •  {}", s.protocol_title, when, outcome);
    }

    println!("\nSee one in full with: proto show <session-id>");
    Ok(())
}

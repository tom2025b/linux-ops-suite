use std::path::Path;

use crate::core::store;

pub fn handle(sessions_dir: &Path, query: &str) -> anyhow::Result<()> {
    // An empty query would match everything, which is just `proto sessions` — tell
    // the user rather than dumping the whole store under a misleading "search".
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        println!("Empty search — try `proto search <text>` (or `proto sessions` to list all).");
        return Ok(());
    }

    // Newest-first, empty if nothing's been run yet (a missing store is normal).
    let entries = store::list(sessions_dir)?;

    // Collect matches with a short reason for each, so the output explains the hit.
    let mut matches = Vec::new();
    for entry in &entries {
        let s = &entry.session;

        // Check the searchable fields in priority order, recording the FIRST place
        // the needle appears. protocol id/title identify the run; notes are the
        // free-text the operator added — between them they cover "what was this run".
        let reason = if s.protocol_id.to_lowercase().contains(&needle) {
            Some("protocol id".to_string())
        } else if s.protocol_title.to_lowercase().contains(&needle) {
            Some("protocol title".to_string())
        } else {
            // Scan step notes; report the step id whose note matched so the user
            // can jump straight to it with `proto show`.
            s.steps
                .iter()
                .find(|r| r.note.to_lowercase().contains(&needle))
                .map(|r| format!("note on step '{}'", r.step_id))
        };

        if let Some(reason) = reason {
            matches.push((entry, reason));
        }
    }

    if matches.is_empty() {
        println!("No sessions match '{query}'.");
        return Ok(());
    }

    println!("{} match(es) for '{}':\n", matches.len(), query);
    // One block per match: the id (the handle for `show`), then title • when • why.
    for (entry, reason) in &matches {
        let s = &entry.session;
        let when = s.started_at.format("%Y-%m-%d %H:%M");
        println!("  {}", entry.id);
        println!(
            "      {}  •  {}  •  matched {}",
            s.protocol_title, when, reason
        );
    }

    println!("\nOpen one with: proto show <session-id>");
    Ok(())
}

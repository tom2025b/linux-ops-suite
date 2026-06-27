use std::io::{self, Write};
use std::path::Path;

use crate::core::loader;
use crate::core::protocol::Protocol;
use crate::core::store; // for last-run recency annotations

use super::run; // reuse the existing run handler to execute the chosen protocol

// Show the picker, then run the chosen protocol. `dir` is the protocols dir,
// `sessions_dir` is where run will persist the session, and `feed_dir`/`write_feed`
// control the post-run Workstate feed — all forwarded straight to the run handler
// so a picked run behaves identically to `proto run <id>`.
pub fn handle(
    dir: &Path,
    sessions_dir: &Path,
    feed_dir: &Path,
    write_feed: bool,
) -> anyhow::Result<()> {
    // Load + validate everything, same as `proto list`. A broken folder surfaces
    // its precise error here rather than at run time.
    let protocols = loader::load_all(dir)?;

    if protocols.is_empty() {
        println!("No protocols found in {}", dir.display());
        println!("Add a protocol *.yaml there, then run `proto` again.");
        return Ok(());
    }

    // A one-line launch banner so a RexOps-launched Proto announces itself rather
    // than dropping straight into a bare list — it reads as an intentional entry
    // point, not a help dump.
    println!("Proto — guided checklists for the Linux Ops Suite\n");

    // When was each protocol last run? Used to annotate stale checklists. This is
    // best-effort: if the session store can't be read for any reason, we just show
    // no recency rather than failing the picker (the point is to RUN something).
    let last_run = store::latest_run_by_protocol(sessions_dir).unwrap_or_default();

    // Present the numbered menu with the id column aligned to the widest id, so
    // titles line up regardless of id length — the same scannable shape as `list`.
    let id_width = protocols.iter().map(|p| p.id.len()).max().unwrap_or(0);
    println!("Protocols ({}):\n", protocols.len());
    let now = chrono::Utc::now();
    for (index, p) in protocols.iter().enumerate() {
        // Recency suffix: "last run 2d ago" or "never run", so the operator sees
        // at a glance which checklists are overdue.
        let recency = match last_run.get(&p.id) {
            Some(when) => format!("last run {}", humanize_ago(now - *when)),
            None => "never run".to_string(),
        };
        // 1-based numbering is what the operator types; the id doubles as a name
        // they can type instead. Pad the id so the "—" separators line up.
        println!(
            "  {:>2}.  {:<width$}  —  {}  ({})",
            index + 1,
            p.id,
            p.title,
            recency,
            width = id_width
        );
    }

    // Ask for a choice, re-prompting on bad input so a typo doesn't bail out.
    let chosen = match prompt_choice(&protocols)? {
        Some(p) => p,
        // Empty input / Ctrl-D / q: a graceful "never mind", exit 0 without running.
        None => {
            println!("No protocol selected.");
            return Ok(());
        }
    };

    // Delegate to the real run handler — one execution path, no duplication.
    let chosen_id = chosen.id.clone();
    run::handle(dir, sessions_dir, feed_dir, write_feed, &chosen_id)?;

    // After the run, offer a "what next" prompt so a launched session flows
    // instead of dumping the operator back to a shell. This only makes sense
    // interactively; the dispatcher already guarantees we're on a TTY here.
    post_run_prompt(dir, sessions_dir, feed_dir, write_feed)
}

// -----------------------------------------------------------------------------
// post_run_prompt — the "what next" menu shown after a picked run completes.
// -----------------------------------------------------------------------------
// Offers: [r] run another (back to the picker), [s] show the run just saved,
// [Enter] done. "Run another" recurses into `handle` (one more menu); the
// recursion depth equals how many runs the operator chains in one sitting — tiny,
// and each frame is independent. Any non-recognized input (or Enter) ends cleanly.
fn post_run_prompt(
    dir: &Path,
    sessions_dir: &Path,
    feed_dir: &Path,
    write_feed: bool,
) -> anyhow::Result<()> {
    loop {
        print!("\nNext: [r]un another  [s]how this run  [Enter] done: ");
        io::stdout().flush()?;
        let mut buf = String::new();
        // EOF (Ctrl-D) => done, same as Enter.
        if io::stdin().read_line(&mut buf)? == 0 {
            return Ok(());
        }
        match buf.trim().to_lowercase().as_str() {
            // Done — the common case, so Enter (empty) lands here.
            "" | "q" | "quit" | "done" => return Ok(()),
            // Run another: back to the top of the picker for a fresh choice.
            "r" | "run" => return handle(dir, sessions_dir, feed_dir, write_feed),
            // Show the run we just saved: find the newest session and print it.
            "s" | "show" => {
                show_latest(sessions_dir)?;
                // Stay in the prompt afterwards so they can then run another or quit.
            }
            other => println!("  Unrecognized '{other}'. Use r, s, or Enter."),
        }
    }
}

// Show the most-recently-saved session (the one the picker just produced). We ask
// the store for its newest entry rather than threading the id back out of `run`,
// keeping `run`'s signature unchanged. An empty store (shouldn't happen right
// after a run) is handled gracefully.
fn show_latest(sessions_dir: &Path) -> anyhow::Result<()> {
    match store::list(sessions_dir)?.first() {
        Some(entry) => super::show::handle(sessions_dir, &entry.id),
        None => {
            println!("  (no saved session to show)");
            Ok(())
        }
    }
}

// Prompt for a selection by 1-based NUMBER or by protocol ID. Returns:
//   * Ok(Some(protocol)) for a valid choice,
//   * Ok(None) for empty input / EOF / "q" (the operator backed out),
//   * loops on an out-of-range number or an unknown id.
fn prompt_choice(protocols: &[Protocol]) -> anyhow::Result<Option<&Protocol>> {
    loop {
        print!(
            "\nPick a protocol [1-{}] or id (Enter/q to cancel): ",
            protocols.len()
        );
        io::stdout().flush()?; // flush so the prompt shows before we block

        let mut buf = String::new();
        let read = io::stdin().read_line(&mut buf)?;
        // read == 0 means EOF (Ctrl-D / closed stdin): treat as cancel.
        if read == 0 {
            return Ok(None);
        }

        let trimmed = buf.trim();
        // Bare Enter or an explicit quit word = cancel.
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("q")
            || trimmed.eq_ignore_ascii_case("quit")
        {
            return Ok(None);
        }

        // A pure number is a 1-based menu index; bounds-check it.
        if let Ok(n) = trimmed.parse::<usize>() {
            if (1..=protocols.len()).contains(&n) {
                return Ok(Some(&protocols[n - 1]));
            }
            println!("  No protocol number {n}. Enter 1-{}.", protocols.len());
            continue;
        }

        // Otherwise treat the input as a protocol id (exact match).
        if let Some(p) = protocols.iter().find(|p| p.id == trimmed) {
            return Ok(Some(p));
        }
        println!(
            "  No protocol numbered or named '{trimmed}'. Try a number or an id from the list."
        );
    }
}

// -----------------------------------------------------------------------------
// humanize_ago — a compact "how long ago" string for a positive duration.
// -----------------------------------------------------------------------------
// Turns an elapsed Duration into "just now" / "5m ago" / "3h ago" / "2d ago" /
// "4w ago". Coarse by design: for a "last run" hint you want the magnitude, not
// the exact seconds. A negative duration (clock skew / a future timestamp) clamps
// to "just now" so we never print a nonsense "-3h ago".
fn humanize_ago(d: chrono::Duration) -> String {
    let secs = d.num_seconds();
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d ago");
    }
    let weeks = days / 7;
    format!("{weeks}w ago")
}

use std::io::{self, Write};
use std::path::Path;

use crate::core::store;

// `sessions_dir` is where the record lives; `feed_dir`/`write_feed` control the
// post-delete feed refresh; `id` is the session to remove; `assume_yes` skips the
// confirmation prompt (for scripts / when the caller is sure).
pub fn handle(
    sessions_dir: &Path,
    feed_dir: &Path,
    write_feed: bool,
    id: &str,
    assume_yes: bool,
) -> anyhow::Result<()> {
    // Confirm the session exists BEFORE prompting, so a typo'd id fails fast with
    // the standard NotFound message rather than asking "delete <nonexistent>?".
    // load returns NotFound for a missing id — we discard the value, we only want
    // the existence check and a friendly title to show in the prompt.
    let session = store::load(sessions_dir, id)?;

    // Unless --yes, ask for explicit confirmation. A delete is irreversible, so we
    // default to caution and require a clear "y" (anything else cancels).
    if !assume_yes {
        print!(
            "Delete session '{}' ({})? [y/N]: ",
            id, session.protocol_title
        );
        io::stdout().flush()?; // show the prompt before blocking on input
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        // Only an explicit yes proceeds; empty/anything-else is a safe cancel.
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("Cancelled — nothing deleted.");
            return Ok(());
        }
    }

    // Remove the file. NotFound can't happen here (we just loaded it), but a write
    // error (permissions) surfaces with the WriteFile message.
    store::delete(sessions_dir, id)?;
    println!("Deleted session '{id}'.");

    // The feed summarized recent sessions including this one, so refresh it. Like
    // `run`, this is BEST-EFFORT: the delete already succeeded, so a feed write
    // failure is a warning, not a command failure (`proto feed` can rebuild it).
    if write_feed {
        match store::build_feed(sessions_dir).and_then(|f| store::save_feed(feed_dir, &f)) {
            Ok(path) => println!("Refreshed Workstate feed: {}", path.display()),
            Err(e) => eprintln!("warning: could not refresh Workstate feed: {e}"),
        }
    }

    Ok(())
}

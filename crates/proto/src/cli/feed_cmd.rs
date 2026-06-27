use std::path::Path;

use crate::core::store;

// `sessions_dir` is where runs are read FROM; `feed_dir` is where the feed is
// written TO. Both resolved by the CLI layer (defaults / overrides) before we get
// here, so this handler is pure orchestration.
pub fn handle(sessions_dir: &Path, feed_dir: &Path) -> anyhow::Result<()> {
    // Compile the feed from every saved session (newest first, capped). A missing
    // session store is a normal empty feed, not an error — you may legitimately
    // refresh before you've run anything.
    let feed = store::build_feed(sessions_dir)?;

    // Write the well-known proto.json into the feed dir.
    let path = store::save_feed(feed_dir, &feed)?;

    // Report what we wrote so the operator (or a cron log) can see it landed.
    println!(
        "Wrote Workstate feed: {} item(s) -> {}",
        feed.item_count,
        path.display()
    );
    Ok(())
}

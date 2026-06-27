use std::path::Path;

use crate::core::loader; // discover/load/validate live here

// `anyhow::Result` at the CLI layer: we only report + exit, no variant matching.
pub fn handle(dir: &Path) -> anyhow::Result<()> {
    // Load + validate everything. A broken folder/file short-circuits with a
    // precise ProtoError, which `?` converts into an anyhow error for main.rs.
    let protocols = loader::load_all(dir)?;

    // Empty but valid directory: say so plainly instead of printing nothing,
    // which would look like a hang or a bug.
    if protocols.is_empty() {
        println!("No protocols found in {}", dir.display());
        return Ok(());
    }

    // Header line giving the count and where we looked — orients the reader.
    println!("{} protocol(s) in {}:\n", protocols.len(), dir.display());

    // One line per protocol: id, title, and how many steps it has. Stable order
    // is guaranteed by load_all (discover sorts the files).
    for p in &protocols {
        println!("  {}  —  {}  ({} steps)", p.id, p.title, p.step_count());
    }

    Ok(())
}

use std::path::Path;

use anyhow::Context;

use crate::core::render;
use crate::core::store;

// Which serialization the user asked for. Defaulting to Markdown is set by the
// CLI layer (mod.rs) before we get here, so this handler always has a concrete one.
#[derive(Debug, Clone, Copy)]
pub enum Format {
    Markdown,
    Json,
}

// `id` is the session to export; `format` picks the rendering; `out` is an
// optional file path (None => stdout).
pub fn handle(
    sessions_dir: &Path,
    id: &str,
    format: Format,
    out: Option<&Path>,
) -> anyhow::Result<()> {
    // Load the session (NotFound for a bad id — same vocabulary as `show`).
    let session = store::load(sessions_dir, id)?;

    // Produce the text in the requested format. Markdown comes from the pure
    // renderer in `core`; JSON is the pretty contract form (what's on disk, but we
    // re-serialize from the parsed value so the output is canonical/pretty).
    let body = match format {
        Format::Markdown => render::session_markdown(id, &session),
        Format::Json => serde_json::to_string_pretty(&session)
            .context("serializing session to JSON for export")?,
    };

    // Write to the file if --out was given, else print to stdout.
    match out {
        Some(path) => {
            // A trailing newline keeps the file POSIX-tidy. write() truncates/creates.
            std::fs::write(path, format!("{body}\n"))
                .with_context(|| format!("writing export to {}", path.display()))?;
            println!("Wrote {} export to {}", format_word(format), path.display());
        }
        // stdout: print exactly the body (no extra "Wrote ..." noise) so it pipes
        // cleanly into pbcopy / a file redirect / another tool.
        None => println!("{body}"),
    }

    Ok(())
}

// A short word for the format, for the "Wrote <md> export" confirmation line.
fn format_word(format: Format) -> &'static str {
    match format {
        Format::Markdown => "markdown",
        Format::Json => "json",
    }
}

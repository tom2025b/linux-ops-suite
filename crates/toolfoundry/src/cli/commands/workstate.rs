use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::{DateTime, Local, NaiveDate, Utc};
use toolfoundry_core::workstate::load_workstate_feed;

use crate::cli::commands::resolve_manifest_directory;

pub(super) fn export_workstate_feed(
    directory: Option<PathBuf>,
    config: Option<PathBuf>,
    as_of: Option<NaiveDate>,
    generated_at: Option<DateTime<Utc>>,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let directory = resolve_manifest_directory(directory, config)?;
    let as_of = as_of.unwrap_or_else(|| Local::now().date_naive());
    let generated_at = generated_at.unwrap_or_else(Utc::now);
    let feed = load_workstate_feed(&directory, as_of, generated_at)?;
    let json = serde_json::to_string_pretty(&feed)?;

    if let Some(output) = output {
        write_atomic(&output, &json)?;
    } else {
        println!("{json}");
    }

    Ok(())
}

/// Publish the feed to `path` so readers never observe a partially written file.
///
/// We write to a sibling temp file in the *same* directory and then `rename` it
/// over the destination. On a single filesystem `rename` is atomic, so any
/// concurrent reader sees either the previous file or the complete new one —
/// never a half-written feed (this prevents *torn reads*).
///
/// Note on durability: this does **not** `fsync` the temp file or its directory,
/// so it does not guarantee the new contents survive a power loss / kernel crash
/// that happens right after the rename. That is an intentional trade-off: the
/// Workstate feed is cheaply regenerated from manifests on the next run, so
/// crash durability buys little here. If this feed ever becomes the source of
/// truth, add an `fsync` on the temp file before the rename and on the parent
/// directory after it.
fn write_atomic(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }

    let temp_path = temp_output_path(path);
    fs::write(&temp_path, contents)
        .with_context(|| format!("writing temporary feed {}", temp_path.display()))?;

    // Atomic swap into place; on failure, don't leave the temp sidecar behind.
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("publishing feed {}", path.display()));
    }

    Ok(())
}

fn temp_output_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("toolfoundry-workstate-feed");

    parent.join(format!(".{file_name}.{}.tmp", std::process::id()))
}

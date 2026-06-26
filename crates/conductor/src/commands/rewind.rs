//! `rewind` — capture, list, and restore snapshot copies.

use crate::cli::RewindCmd;
use crate::commands::snapshot;
use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::ui::display;

pub fn run(cmd: RewindCmd, config: &Config) -> Result<()> {
    match cmd {
        RewindCmd::Capture => capture(config),
        RewindCmd::Restore { id } => restore(config, &id),
        RewindCmd::List => list(config),
    }
}

/// Copy the current snapshot into a restore point named by its timestamp.
fn capture(config: &Config) -> Result<()> {
    let path = config.workstate_path();
    if !path.exists() {
        return Err(Error::NotFound("workstate snapshot".into()));
    }
    let ws = snapshot::load(&path)?;
    let id = ws.built_at.to_string();
    snapshot::save(&config.rewind_dir().join(format!("{id}.json")), &ws)?;
    display::message(config, &format!("captured restore point {id}"));
    Ok(())
}

/// Replace the current snapshot with a saved restore point.
fn restore(config: &Config, id: &str) -> Result<()> {
    let path = config.rewind_dir().join(format!("{id}.json"));
    if !path.exists() {
        return Err(Error::NotFound(format!("restore point {id}")));
    }
    let ws = snapshot::load(&path)?;
    snapshot::save(&config.workstate_path(), &ws)?;
    display::message(config, &format!("restored {id}"));
    Ok(())
}

/// List saved restore-point ids, oldest first.
fn list(config: &Config) -> Result<()> {
    let mut ids = Vec::new();
    if let Ok(entries) = std::fs::read_dir(config.rewind_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(stem.to_string());
                }
            }
        }
    }
    ids.sort();
    display::rewind_list(config, &ids);
    Ok(())
}

//! `workstate` — write, re-stamp, and report the state snapshot.

use crate::cli::WorkstateCmd;
use crate::commands::snapshot;
use crate::core::config::Config;
use crate::core::error::Result;
use crate::state::workstate::Workstate;
use crate::ui::display;

pub fn run(cmd: WorkstateCmd, config: &Config) -> Result<()> {
    match cmd {
        WorkstateCmd::Snapshot => snapshot_cmd(config),
        WorkstateCmd::Refresh => refresh(config),
        WorkstateCmd::Status => status(config),
    }
}

/// Write a fresh snapshot, preserving any findings already on disk.
fn snapshot_cmd(config: &Config) -> Result<()> {
    let mut ws = load_or_empty(config);
    ws.restamp();
    snapshot::save(&config.workstate_path(), &ws)?;
    display::workstate_saved(config, &ws);
    Ok(())
}

/// Re-stamp the snapshot as current.
fn refresh(config: &Config) -> Result<()> {
    let mut ws = load_or_empty(config);
    ws.restamp();
    snapshot::save(&config.workstate_path(), &ws)?;
    display::message(config, "workstate refreshed");
    Ok(())
}

/// Report freshness and counts.
fn status(config: &Config) -> Result<()> {
    let path = config.workstate_path();
    if !path.exists() {
        display::message(
            config,
            "no workstate snapshot yet — run: conductor workstate snapshot",
        );
        return Ok(());
    }
    let ws = snapshot::load(&path)?;
    if config.json {
        display::print_json(&ws)?;
    } else {
        display::workstate_status(config, &ws);
    }
    Ok(())
}

/// The current snapshot, or a fresh empty one if none exists yet.
fn load_or_empty(config: &Config) -> Workstate {
    snapshot::load(&config.workstate_path()).unwrap_or_else(|_| Workstate::empty())
}

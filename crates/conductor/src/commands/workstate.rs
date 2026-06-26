//! `workstate` — refresh the canonical snapshot (via the producer) and report it.
//!
//! Conductor does NOT compile or write the snapshot itself: Workstate is the single
//! producer. `refresh` invokes the `workstate` binary (which writes the canonical
//! snapshot), and `status` reads that snapshot back through the shared loader.

use crate::cli::WorkstateCmd;
use crate::commands::snapshot;
use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::ui::display;

pub fn run(cmd: WorkstateCmd, config: &Config) -> Result<()> {
    match cmd {
        // Both "snapshot" and "refresh" mean the same thing now: ask the producer
        // to recompile the canonical snapshot. Two names kept for muscle memory.
        WorkstateCmd::Snapshot | WorkstateCmd::Refresh => refresh(config),
        WorkstateCmd::Status => status(config),
    }
}

/// Regenerate the canonical snapshot by invoking the Workstate producer, then show
/// the result.
fn refresh(config: &Config) -> Result<()> {
    run_producer()?;
    let path = config.workstate_path();
    if config.json {
        display::print_json(&snapshot::load(&path)?)?;
    } else {
        display::message(
            config,
            &format!("workstate refreshed -> {}", path.display()),
        );
    }
    Ok(())
}

/// Report freshness and counts for the canonical snapshot.
fn status(config: &Config) -> Result<()> {
    let path = config.workstate_path();
    if !path.exists() {
        display::message(
            config,
            "no workstate snapshot yet — run: conductor workstate refresh",
        );
        return Ok(());
    }
    let snap = snapshot::load(&path)?;
    if config.json {
        display::print_json(&snap)?;
    } else {
        display::workstate_status(config, &snap);
    }
    Ok(())
}

/// Resolve and spawn the Workstate producer (it writes the canonical snapshot to
/// its own default path). The binary is `workstate` on `$PATH`, overridable via
/// `CONDUCTOR_WORKSTATE_BIN` for a build that isn't installed.
fn run_producer() -> Result<()> {
    let bin = std::env::var("CONDUCTOR_WORKSTATE_BIN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "workstate".to_string());
    let status = std::process::Command::new(&bin).status().map_err(|e| {
        Error::Snapshot(format!(
            "could not run `{bin}`: {e}\ninstall workstate or set CONDUCTOR_WORKSTATE_BIN"
        ))
    })?;
    if !status.success() {
        return Err(Error::Snapshot(format!("`{bin}` exited with {status}")));
    }
    Ok(())
}

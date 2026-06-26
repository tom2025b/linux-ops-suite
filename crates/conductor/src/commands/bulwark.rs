//! `bulwark` — inspect findings and high-severity drift.

use crate::cli::BulwarkCmd;
use crate::commands::snapshot;
use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::state::workstate::Workstate;
use crate::ui::display;

pub fn run(cmd: BulwarkCmd, config: &Config) -> Result<()> {
    match cmd {
        BulwarkCmd::Show { id } => show(config, &id),
        BulwarkCmd::Check => check(config),
        BulwarkCmd::Tripwire => tripwire(config),
    }
}

/// Show one finding by id.
fn show(config: &Config, id: &str) -> Result<()> {
    let ws = load(config)?;
    let finding = ws
        .finding(id)
        .ok_or_else(|| Error::NotFound(format!("finding {id}")))?;
    if config.json {
        display::print_json(finding)?;
    } else {
        display::finding(config, finding);
    }
    Ok(())
}

/// Summarise all findings.
fn check(config: &Config) -> Result<()> {
    let ws = load(config)?;
    if config.json {
        display::print_json(&ws)?;
    } else {
        display::bulwark_check(config, &ws);
    }
    Ok(())
}

/// Report the high-severity findings worth investigating first.
fn tripwire(config: &Config) -> Result<()> {
    let ws = load(config)?;
    if config.json {
        display::print_json(&ws)?;
    } else {
        display::bulwark_tripwire(config, &ws);
    }
    Ok(())
}

/// Load the current snapshot, or a friendly error if none exists.
fn load(config: &Config) -> Result<Workstate> {
    let path = config.workstate_path();
    if !path.exists() {
        return Err(Error::NotFound("workstate snapshot".into()));
    }
    snapshot::load(&path)
}

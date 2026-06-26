//! `bulwark` — inspect the security findings carried in the canonical snapshot.
//!
//! These read the Workstate snapshot's `findings` section (Bulwark's normalized
//! findings); conductor never talks to Bulwark directly.

use workstate_schema::model::normalized::Finding;
use workstate_schema::Snapshot;

use crate::cli::BulwarkCmd;
use crate::commands::snapshot;
use crate::core::config::Config;
use crate::core::error::{Error, Result};
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
    let snap = load(config)?;
    let finding = snap
        .findings
        .data
        .as_ref()
        .and_then(|inv| inv.findings.iter().find(|f| f.id.0 == id))
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
    let snap = load(config)?;
    if config.json {
        display::print_json(&findings(&snap))?;
    } else {
        display::bulwark_check(config, &snap);
    }
    Ok(())
}

/// Report the high-severity findings worth investigating first.
fn tripwire(config: &Config) -> Result<()> {
    let snap = load(config)?;
    if config.json {
        display::print_json(&findings(&snap))?;
    } else {
        display::bulwark_tripwire(config, &snap);
    }
    Ok(())
}

/// Load the canonical snapshot, or a friendly error if it hasn't been compiled.
fn load(config: &Config) -> Result<Snapshot> {
    let path = config.workstate_path();
    if !path.exists() {
        return Err(Error::NotFound("workstate snapshot".into()));
    }
    snapshot::load(&path)
}

/// The findings the snapshot carries (empty when the section is Missing/Failed).
fn findings(snap: &Snapshot) -> Vec<&Finding> {
    snap.findings
        .data
        .as_ref()
        .map(|inv| inv.findings.iter().collect())
        .unwrap_or_default()
}

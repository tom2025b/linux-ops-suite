//! JSON handling for state: read and write `Workstate` snapshots on disk.

use std::path::Path;

use crate::core::error::Result;
use crate::state::workstate::Workstate;

/// Load a snapshot from `path`.
pub fn load(path: &Path) -> Result<Workstate> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Write `workstate` to `path`, creating parent directories as needed.
pub fn save(path: &Path, workstate: &Workstate) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(workstate)?;
    std::fs::write(path, json)?;
    Ok(())
}

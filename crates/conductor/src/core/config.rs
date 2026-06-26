//! Where state lives and how output is rendered.

use std::path::PathBuf;

/// Resolved runtime settings, built once from the CLI flags.
pub struct Config {
    pub data_dir: PathBuf,
    pub json: bool,
    pub color: bool,
}

impl Config {
    /// Build the config, falling back to the XDG data directory.
    pub fn new(data_dir: Option<PathBuf>, json: bool, no_color: bool) -> Self {
        Config {
            data_dir: data_dir.unwrap_or_else(default_data_dir),
            json,
            color: !no_color,
        }
    }

    /// The canonical Workstate snapshot — the single source of truth conductor
    /// reads. It lives at Workstate's own published location (via workstate-schema),
    /// NOT under conductor's data dir: conductor consumes what Workstate writes. The
    /// `--data-dir` override governs only conductor's own rewind store below.
    pub fn workstate_path(&self) -> PathBuf {
        workstate_schema::default_output_path()
            .unwrap_or_else(|| self.data_dir.join("rexops/feeds/workstate.snapshot.json"))
    }

    /// The directory holding rewind restore points.
    pub fn rewind_dir(&self) -> PathBuf {
        self.data_dir.join("rewind")
    }
}

/// `$XDG_DATA_HOME/linux-ops-suite`, else `~/.local/share/linux-ops-suite`,
/// else the current directory.
fn default_data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("linux-ops-suite")
}

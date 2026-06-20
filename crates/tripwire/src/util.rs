//! Tripwire's path/env helpers — thin wrappers over [`suite_core`].
//!
//! The generic logic (TTY rule, `$HOME`, the `linux-ops-suite/<tool>` XDG
//! layout, tilde expansion) lives in `suite-core` so the whole suite agrees.
//! What stays here is only tripwire's *naming*: the `"tripwire"` leaf and the
//! `baseline_path` / `config_path` compositions its modules call.

use std::path::PathBuf;

pub use suite_core::env::{home_dir, is_root, stdout_is_tty};
pub use suite_core::xdg::expand_tilde;

/// The suite's per-tool *data* directory for tripwire
/// (`…/linux-ops-suite/tripwire`).
pub fn data_dir() -> Option<PathBuf> {
    suite_core::xdg::data_dir("tripwire")
}

/// The suite's per-tool *config* directory for tripwire
/// (`…/linux-ops-suite/tripwire`).
pub fn config_dir() -> Option<PathBuf> {
    suite_core::xdg::config_dir("tripwire")
}

/// The default baseline path: `<data_dir>/baseline.json`.
pub fn baseline_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("baseline.json"))
}

/// The default watch-config path: `<config_dir>/watch.conf`.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("watch.conf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_ends_with_suite_tool_suffix() {
        if let Some(dir) = data_dir() {
            assert!(dir.ends_with("linux-ops-suite/tripwire"));
        }
    }

    #[test]
    fn baseline_path_lives_under_data_dir() {
        if let (Some(d), Some(b)) = (data_dir(), baseline_path()) {
            assert!(b.starts_with(&d));
            assert!(b.ends_with("baseline.json"));
        }
    }

    #[test]
    fn config_path_lives_under_config_dir() {
        if let (Some(d), Some(p)) = (config_dir(), config_path()) {
            assert!(p.starts_with(&d));
            assert!(p.ends_with("watch.conf"));
        }
    }
}

//! Rewind's path/env helpers — thin wrappers over [`suite_core`].
//!
//! The generic logic (TTY rule, `$HOME`, the `linux-ops-suite/<tool>` XDG
//! layout, tilde expansion) lives in `suite-core` so the whole suite agrees.
//! What stays here is only rewind's *naming*: the `"rewind"` leaf and the
//! `store_dir` / `config_path` compositions its modules call.

use std::path::PathBuf;

pub use suite_core::env::{home_dir, is_root, stdout_is_tty};
pub use suite_core::xdg::expand_tilde;

/// The suite's per-tool *data* directory for rewind
/// (`…/linux-ops-suite/rewind`).
pub fn data_dir() -> Option<PathBuf> {
    suite_core::xdg::data_dir("rewind")
}

/// The suite's per-tool *config* directory for rewind
/// (`…/linux-ops-suite/rewind`).
pub fn config_dir() -> Option<PathBuf> {
    suite_core::xdg::config_dir("rewind")
}

/// The default store directory: the per-tool data dir itself.
pub fn store_dir() -> Option<PathBuf> {
    data_dir()
}

/// The default capture-config path: `<config_dir>/capture.conf`.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("capture.conf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_ends_with_suite_tool_suffix() {
        if let Some(dir) = data_dir() {
            assert!(dir.ends_with("linux-ops-suite/rewind"));
        }
    }

    #[test]
    fn store_dir_matches_data_dir() {
        assert_eq!(store_dir(), data_dir());
    }

    #[test]
    fn config_path_lives_under_config_dir() {
        if let (Some(d), Some(p)) = (config_dir(), config_path()) {
            assert!(p.starts_with(&d));
            assert!(p.ends_with("capture.conf"));
        }
    }
}

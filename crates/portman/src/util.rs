//! Portman's path/env helpers — thin wrappers over [`suite_core`].
//!
//! The generic logic (TTY rule, the `linux-ops-suite/<tool>` XDG layout) lives
//! in `suite-core` so the whole suite agrees. What stays here is only portman's
//! *naming*: the `"portman"` leaf and the `baseline_path` composition.

use std::path::PathBuf;

pub use suite_core::env::{is_root, stdout_is_tty};

/// The suite's per-tool data directory for portman
/// (`…/linux-ops-suite/portman`).
pub fn data_dir() -> Option<PathBuf> {
    suite_core::xdg::data_dir("portman")
}

/// The default baseline path: `<data_dir>/baseline.json`.
pub fn baseline_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("baseline.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_ends_with_suite_tool_suffix() {
        if let Some(dir) = data_dir() {
            assert!(dir.ends_with("linux-ops-suite/portman"));
        }
    }

    #[test]
    fn baseline_path_lives_under_data_dir() {
        if let (Some(d), Some(b)) = (data_dir(), baseline_path()) {
            assert!(b.starts_with(&d));
            assert!(b.ends_with("baseline.json"));
        }
    }
}

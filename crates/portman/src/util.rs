//! Small, dependency-free helpers shared across portman's modules.
//!
//! The TTY rule mirrors rex-doctor/rex-check (same `isatty(3)` call) so the
//! suite agrees on what "a terminal" means. The data-dir resolution follows the
//! same XDG path rex-doctor's `env.*` checks expect the suite to use.

use std::env;
use std::path::PathBuf;

/// Whether stdout is a TTY — gates color.
pub fn stdout_is_tty() -> bool {
    is_tty(1)
}

/// Whether the given fd is a TTY, via `isatty(3)`.
fn is_tty(fd: i32) -> bool {
    // SAFETY: isatty merely queries a file descriptor and has no preconditions.
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) == 1 }
}

/// Whether the current process is root. Used only to phrase the "owners hidden"
/// hint correctly — portman never *requires* root, it just resolves more with it.
pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() == 0 }
}

/// The suite's per-tool data directory for portman, honoring `$XDG_DATA_HOME`
/// then falling back to `~/.local/share`. Same convention rex-doctor checks for.
/// Returns `None` only when neither `$XDG_DATA_HOME` nor `$HOME` is usable.
pub fn data_dir() -> Option<PathBuf> {
    let base = env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".local/share"))
        })?;
    Some(base.join("linux-ops-suite").join("portman"))
}

/// The default baseline path: `<data_dir>/baseline.json`.
pub fn baseline_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("baseline.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_prefers_xdg_then_home() {
        // We can't safely mutate process env in parallel tests, so just assert
        // the shape: whichever anchor exists, the suite/tool suffix is appended.
        if let Some(dir) = data_dir() {
            let s = dir.to_string_lossy();
            assert!(s.ends_with("linux-ops-suite/portman"));
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

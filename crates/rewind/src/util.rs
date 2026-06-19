//! Small, dependency-free helpers shared across rewind's modules.
//!
//! The TTY rule mirrors tripwire/portman (same `isatty(3)` call) so the suite
//! agrees on what "a terminal" means. The data-dir resolution follows the same
//! XDG path the rest of the suite uses (`linux-ops-suite/<tool>`); the
//! config-dir follows the parallel `$XDG_CONFIG_HOME` convention.

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

/// Whether the current process is root. Used only to phrase the "some files are
/// unreadable" hint correctly — rewind never *requires* root, it just reads (and
/// restores) more with it.
pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() == 0 }
}

/// The user's home directory, used to expand a leading `~/` in capture paths and
/// to anchor the XDG fallbacks. `None` when `$HOME` is unset/empty.
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The suite's per-tool *data* directory for rewind, honoring `$XDG_DATA_HOME`
/// then falling back to `~/.local/share`. Same convention tripwire/portman use.
/// Returns `None` only when neither `$XDG_DATA_HOME` nor `$HOME` is usable.
pub fn data_dir() -> Option<PathBuf> {
    let base = env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".local/share")))?;
    Some(base.join("linux-ops-suite").join("rewind"))
}

/// The suite's per-tool *config* directory for rewind, honoring
/// `$XDG_CONFIG_HOME` then falling back to `~/.config`. Returns `None` only when
/// neither `$XDG_CONFIG_HOME` nor `$HOME` is usable.
pub fn config_dir() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".config")))?;
    Some(base.join("linux-ops-suite").join("rewind"))
}

/// The default store directory: the per-tool data dir itself.
pub fn store_dir() -> Option<PathBuf> {
    data_dir()
}

/// The default capture-config path: `<config_dir>/capture.conf`.
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("capture.conf"))
}

/// Expand a leading `~` (or `~/…`) against `$HOME`. Anything else is returned
/// unchanged. Keeps the config format friendly without a shell.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_ends_with_suite_tool_suffix() {
        if let Some(dir) = data_dir() {
            assert!(dir.to_string_lossy().ends_with("linux-ops-suite/rewind"));
        }
    }

    #[test]
    fn config_dir_ends_with_suite_tool_suffix() {
        if let Some(dir) = config_dir() {
            assert!(dir.to_string_lossy().ends_with("linux-ops-suite/rewind"));
        }
    }

    #[test]
    fn store_dir_matches_data_dir() {
        assert_eq!(store_dir(), data_dir());
    }

    #[test]
    fn expand_tilde_expands_only_a_leading_tilde() {
        if let Some(home) = home_dir() {
            assert_eq!(expand_tilde("~/.bashrc"), home.join(".bashrc"));
            assert_eq!(expand_tilde("~"), home);
        }
        assert_eq!(expand_tilde("/etc/passwd"), PathBuf::from("/etc/passwd"));
        assert_eq!(expand_tilde("/a/~b"), PathBuf::from("/a/~b"));
    }
}

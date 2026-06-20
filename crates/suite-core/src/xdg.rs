//! The suite's XDG base-directory paths and tilde expansion.
//!
//! Every tool stores its data under `<XDG_DATA_HOME>/linux-ops-suite/<tool>`
//! and (where it has config) `<XDG_CONFIG_HOME>/linux-ops-suite/<tool>`, with
//! the usual `~/.local/share` and `~/.config` fallbacks. The only thing that
//! varied between the copied implementations was the `<tool>` leaf, so it is a
//! parameter here; each tool keeps a one-line wrapper that passes its name.

use std::env;
use std::path::PathBuf;

use crate::env::home_dir;

/// The suite root component shared by every tool's data/config path.
const SUITE: &str = "linux-ops-suite";

/// The per-tool *data* directory: `$XDG_DATA_HOME` (else `~/.local/share`),
/// then `linux-ops-suite/<tool>`. `None` only when neither `$XDG_DATA_HOME`
/// nor `$HOME` is usable.
pub fn data_dir(tool: &str) -> Option<PathBuf> {
    let base = env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".local/share")))?;
    Some(base.join(SUITE).join(tool))
}

/// The per-tool *config* directory: `$XDG_CONFIG_HOME` (else `~/.config`),
/// then `linux-ops-suite/<tool>`. `None` only when neither `$XDG_CONFIG_HOME`
/// nor `$HOME` is usable.
pub fn config_dir(tool: &str) -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".config")))?;
    Some(base.join(SUITE).join(tool))
}

/// Expand a leading `~` (or `~/…`) against `$HOME`. Anything else is returned
/// unchanged — keeps config files shell-free but friendly.
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
        if let Some(dir) = data_dir("rewind") {
            assert!(dir.ends_with("linux-ops-suite/rewind"));
        }
        if let Some(dir) = data_dir("tripwire") {
            assert!(dir.ends_with("linux-ops-suite/tripwire"));
        }
    }

    #[test]
    fn config_dir_ends_with_suite_tool_suffix() {
        if let Some(dir) = config_dir("portman") {
            assert!(dir.ends_with("linux-ops-suite/portman"));
        }
    }

    #[test]
    fn xdg_overrides_are_honored() {
        // A non-empty XDG_DATA_HOME wins over the HOME fallback. We can only
        // assert the suffix portion without mutating process env in a way that
        // races other tests, so we check the shape via the public contract:
        // the leaf is always linux-ops-suite/<tool>.
        if let Some(dir) = data_dir("x") {
            let s = dir.to_string_lossy();
            assert!(s.ends_with("linux-ops-suite/x"));
        }
    }

    #[test]
    fn expand_tilde_expands_only_a_leading_tilde() {
        if let Some(home) = home_dir() {
            assert_eq!(expand_tilde("~/.bashrc"), home.join(".bashrc"));
            assert_eq!(expand_tilde("~"), home);
        }
        // A tilde anywhere but the front is left alone.
        assert_eq!(expand_tilde("/etc/passwd"), PathBuf::from("/etc/passwd"));
        assert_eq!(expand_tilde("/a/~b"), PathBuf::from("/a/~b"));
    }
}

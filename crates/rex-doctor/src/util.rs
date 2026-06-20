//! Rex-doctor's environment helpers.
//!
//! The generic primitives (TTY rule, `$HOME`, `$PATH` resolution, the exec-bit
//! predicate) now come from [`suite_core`] so the whole suite agrees on what
//! "on PATH" and "a terminal" mean. What stays here is rex-doctor's own
//! diagnostics-specific logic: the suite-binary roster, *all* PATH matches for
//! a name (shadow detection), and the "is this dir on PATH" check.

use std::env;
use std::path::{Path, PathBuf};

pub use suite_core::env::stdout_is_tty;
pub use suite_core::path::is_executable_file;

/// The suite binaries rex-doctor expects on `PATH`, in display order. Mirrors
/// the roster the installers and rex-check know (the seven shipped tools plus
/// rex-check itself); rex-doctor checks for its own siblings, not for itself.
pub const SUITE_BINS: &[&str] = &[
    "bulwark",
    "scriptvault",
    "toolfoundry",
    "workstate",
    "proto",
    "rexops",
    "toolbox-bridge",
    "rex-check",
];

/// Resolve `$HOME` as a path, if set and non-empty.
pub fn home() -> Option<PathBuf> {
    suite_core::env::home_dir()
}

/// Locate a bare command name on `PATH`, returning the first match. A name
/// containing `/` is treated as a literal path. Delegates to suite-core's
/// `resolve_on_path` (same resolution rex-check uses).
pub fn which(name: &str) -> Option<PathBuf> {
    suite_core::path::resolve_on_path(name)
}

/// Every PATH match for a bare name, in PATH order. Used to detect shadowing
/// (two copies of the same suite binary winning/losing on PATH). Built on
/// suite-core's exec-bit predicate.
pub fn which_all(name: &str) -> Vec<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return if is_executable_file(&p) {
            vec![p]
        } else {
            vec![]
        };
    }
    let Some(path_var) = env::var_os("PATH") else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) && !hits.contains(&candidate) {
            hits.push(candidate);
        }
    }
    hits
}

/// Whether `dir` appears as an entry in `$PATH`. Compared on normalized
/// (trailing-slash-stripped) string form so `~/bin` and `~/bin/` match.
pub fn dir_on_path(dir: &Path) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    let want = normalize(dir);
    env::split_paths(&path_var).any(|p| normalize(&p) == want)
}

/// Strip a single trailing separator so path equality is slash-insensitive.
fn normalize(p: &Path) -> String {
    p.to_string_lossy().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_sh_and_rejects_bogus() {
        // `sh` is essentially always present; the bogus name never is.
        assert!(which("sh").is_some());
        assert!(which("definitely-not-a-real-command-xyzzy").is_none());
        // A name with a separator is treated as a literal path.
        assert!(which("/no/such/bin/nope").is_none());
    }

    #[test]
    fn which_all_returns_at_least_one_for_present_binary() {
        let hits = which_all("sh");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|p| is_executable_file(p)));
    }

    #[test]
    fn dir_on_path_detects_membership() {
        // Whatever the first PATH entry is, it must be reported as on PATH.
        if let Some(path_var) = env::var_os("PATH") {
            if let Some(first) = env::split_paths(&path_var).next() {
                assert!(dir_on_path(&first));
            }
        }
        assert!(!dir_on_path(Path::new("/nonexistent/path/xyzzy")));
    }

    #[test]
    fn normalize_is_trailing_slash_insensitive() {
        assert_eq!(normalize(Path::new("/a/b/")), normalize(Path::new("/a/b")));
    }
}

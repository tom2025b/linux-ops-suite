//! Small, dependency-free environment helpers shared by the check modules.
//!
//! These mirror rex-check's helpers deliberately (same PATH-walk, same TTY rule
//! via `isatty(3)`) so the two tools agree on what "on PATH" and "a terminal"
//! mean. Nothing here does I/O beyond `stat`/`isatty` and reading `$PATH`.

use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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

/// Whether stdout is a TTY — gates color and (later) any interactive repair.
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

/// Resolve `$HOME` as a path, if set and non-empty.
pub fn home() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Locate a bare command name on `PATH`, returning the first match. A name
/// containing `/` is treated as a literal path. Same resolution rex-check uses.
pub fn which(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable_file(&p).then_some(p);
    }
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|p| is_executable_file(p))
}

/// Every PATH match for a bare name, in PATH order. Used to detect shadowing
/// (two copies of the same suite binary winning/losing on PATH).
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

/// Whether `path` is a regular file with any execute bit set. Same predicate
/// rex-check uses to decide a binary is runnable.
pub fn is_executable_file(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(md) => md.is_file() && (md.permissions().mode() & 0o111) != 0,
        Err(_) => false,
    }
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

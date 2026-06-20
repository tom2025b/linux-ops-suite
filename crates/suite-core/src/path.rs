//! `$PATH` resolution — an in-process `which(1)` with no fork and no `which`
//! crate.
//!
//! This consolidates five near-identical copies from across the suite. One of
//! them (pulse's cockpit launcher) checked only `is_file()` and skipped the
//! execute bit; the canonical implementation here **always** checks the exec
//! bit, which is what the other four already did.

use std::env;
use std::path::{Path, PathBuf};

/// Whether `path` is a regular file with any execute bit set (`0o111`). The
/// single predicate the suite uses to decide a binary is runnable.
pub fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(md) => md.is_file() && (md.permissions().mode() & 0o111) != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Locate a command on `$PATH`, returning the first executable match.
///
/// A `name` containing `/` is treated as a literal path (and still must be an
/// executable file). A bare name is searched across `$PATH` entries in order;
/// the first entry whose `dir/name` is an executable file wins.
pub fn resolve_on_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable_file(&p).then_some(p);
    }
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|p| is_executable_file(p))
}

/// Whether `name` resolves to an executable on `$PATH`. The boolean form of
/// [`resolve_on_path`].
pub fn which(name: &str) -> bool {
    resolve_on_path(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_sh_and_rejects_bogus() {
        // `sh` is essentially always present and executable; the bogus name
        // never is.
        assert!(resolve_on_path("sh").is_some());
        assert!(resolve_on_path("definitely-not-a-real-command-xyzzy").is_none());
    }

    #[test]
    fn which_agrees_with_resolve() {
        assert!(which("sh"));
        assert!(!which("definitely-not-a-real-command-xyzzy"));
    }

    #[test]
    fn name_with_slash_is_a_literal_path() {
        assert!(resolve_on_path("/no/such/bin/nope").is_none());
        // The first PATH hit for `sh`, addressed as a literal path, still
        // resolves to itself.
        if let Some(p) = resolve_on_path("sh") {
            let as_literal = p.to_string_lossy().into_owned();
            assert_eq!(resolve_on_path(&as_literal), Some(p));
        }
    }

    #[test]
    fn is_executable_file_rejects_dirs_and_missing() {
        assert!(!is_executable_file(Path::new("/")));
        assert!(!is_executable_file(Path::new("/nonexistent/xyzzy")));
        if let Some(p) = resolve_on_path("sh") {
            assert!(is_executable_file(&p));
        }
    }
}

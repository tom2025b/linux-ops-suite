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

/// Whether the **current** user can write inside `dir`, asked of the kernel via
/// `access(2)` with `W_OK`. This is the honest answer to "can I create files
/// here": it accounts for ownership, group, and the sudo case where a root-owned
/// `0755` directory has the owner-write bit set but is unwritable to everyone
/// else. A plain `mode & 0o200` check cannot see that — it only inspects the
/// owner bit, not who the owner is.
pub fn is_writable_dir(dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        // W_OK == 2 on every Linux/Unix; access() resolves it against the real
        // uid/gid, which is exactly what a non-suid CLI wants to report.
        const W_OK: i32 = 2;
        // SAFETY: access merely tests permissions on a NUL-terminated path and
        // has no preconditions; a NUL in the path makes CString::new fail first.
        extern "C" {
            fn access(path: *const std::os::raw::c_char, mode: i32) -> i32;
        }
        match CString::new(dir.as_os_str().as_bytes()) {
            Ok(c) => unsafe { access(c.as_ptr(), W_OK) == 0 },
            Err(_) => false, // path contains an interior NUL — cannot be a real dir
        }
    }
    #[cfg(not(unix))]
    {
        // No portable access() equivalent; fall back to "exists and is a dir".
        dir.is_dir()
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
    fn is_writable_dir_reflects_real_access() {
        // A freshly-created temp dir is writable by its creator…
        let tmp = std::env::temp_dir();
        assert!(is_writable_dir(&tmp), "temp dir should be writable: {tmp:?}");
        // …a nonexistent path is not…
        assert!(!is_writable_dir(Path::new("/nonexistent/xyzzy/dir")));
        // …and (when not running as root) a root-owned system dir is not
        // writable even though its mode has the owner-write bit set — the exact
        // case a bare `mode & 0o200` check gets wrong. Skip under root, where it
        // legitimately *is* writable.
        if !crate::env::is_root() {
            assert!(
                !is_writable_dir(Path::new("/usr/bin")),
                "/usr/bin must not be writable to a non-root user"
            );
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

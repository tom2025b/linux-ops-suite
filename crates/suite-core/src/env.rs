//! Environment probes: TTY, root, and `$HOME`.
//!
//! These were previously copied byte-for-byte into nearly every tool's
//! `util.rs`. The TTY rule (`isatty(3)` on fd 1) is what the whole suite uses
//! to agree on "is this a terminal" — and therefore on whether to emit color.

use std::env;
use std::path::PathBuf;

/// Whether stdout is a TTY — the suite-wide gate for color and interactivity.
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

/// Whether the current process is root (euid 0). Tools use this only to phrase
/// "some files were unreadable" hints — none of them *require* root.
pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() == 0 }
}

/// The user's home directory from `$HOME`. `None` when unset or empty. Anchors
/// the XDG fallbacks and tilde expansion in [`crate::xdg`].
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_reads_env() {
        // We don't assume HOME is set in CI, but if it is, it must come back
        // non-empty and as a PathBuf of that value.
        match env::var_os("HOME") {
            Some(h) if !h.is_empty() => {
                assert_eq!(home_dir(), Some(PathBuf::from(h)));
            }
            _ => assert_eq!(home_dir(), None),
        }
    }

    #[test]
    fn stdout_is_tty_does_not_panic() {
        // Under `cargo test` stdout is usually a pipe, so this is false; the
        // point is the call is sound and returns a bool either way.
        let _ = stdout_is_tty();
    }

    #[test]
    fn is_root_does_not_panic() {
        let _ = is_root();
    }
}

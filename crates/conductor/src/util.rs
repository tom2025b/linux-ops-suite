//! Small, dependency-free helpers. The TTY rule mirrors rewind/tripwire/pulse
//! (same `isatty(3)` call) so the suite agrees on what "a terminal" means. The
//! data root follows the same XDG path the rest of the suite uses, but with NO
//! per-tool suffix: conductor reads *other* tools' subtrees (rexops/…,
//! workstate/…, proto/…) under this one root.

use std::env;
use std::path::PathBuf;

/// Whether stdout is a TTY — gates color.
pub fn stdout_is_tty() -> bool {
    // SAFETY: isatty merely queries a file descriptor and has no preconditions.
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(1) == 1 }
}

/// The user's home directory; `None` when `$HOME` is unset/empty.
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The suite *data root*: `$XDG_DATA_HOME`, else `~/.local/share`. Unlike
/// rewind's per-tool `data_dir`, this is the shared root the other tools write
/// their subtrees under, so conductor can read them. `None` only when neither
/// `$XDG_DATA_HOME` nor `$HOME` is usable.
pub fn data_root() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".local/share")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_root_prefers_xdg_data_home() {
        // Save + restore env to avoid cross-test leakage.
        let prev_xdg = env::var_os("XDG_DATA_HOME");
        env::set_var("XDG_DATA_HOME", "/tmp/conductor-xdg-test");
        assert_eq!(data_root(), Some(PathBuf::from("/tmp/conductor-xdg-test")));
        match prev_xdg {
            Some(v) => env::set_var("XDG_DATA_HOME", v),
            None => env::remove_var("XDG_DATA_HOME"),
        }
    }

    #[test]
    fn data_root_falls_back_to_home_local_share() {
        let prev_xdg = env::var_os("XDG_DATA_HOME");
        let prev_home = env::var_os("HOME");
        env::remove_var("XDG_DATA_HOME");
        env::set_var("HOME", "/home/example");
        assert_eq!(
            data_root(),
            Some(PathBuf::from("/home/example/.local/share"))
        );
        match prev_xdg {
            Some(v) => env::set_var("XDG_DATA_HOME", v),
            None => env::remove_var("XDG_DATA_HOME"),
        }
        match prev_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }
}

//! Conductor's path/env helpers — thin wrappers over [`suite_core`].
//!
//! The TTY rule and `$HOME` resolution come from `suite-core` so the suite
//! agrees on what "a terminal" means. What stays here is conductor's *data
//! root*: the same XDG path the rest of the suite uses, but with NO per-tool
//! suffix — conductor reads *other* tools' subtrees (rexops/…, workstate/…,
//! proto/…) under this one root, so it can't use the per-tool `xdg::data_dir`.

use std::path::PathBuf;

pub use suite_core::env::stdout_is_tty;

/// The suite *data root*: `$XDG_DATA_HOME`, else `~/.local/share`. Unlike the
/// per-tool `suite_core::xdg::data_dir`, this is the shared root the other tools
/// write their subtrees under, so conductor can read them. `None` only when
/// neither `$XDG_DATA_HOME` nor `$HOME` is usable.
pub fn data_root() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| suite_core::env::home_dir().map(|h| h.join(".local/share")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn data_root_prefers_xdg_data_home() {
        // Mutates process env; share the crate-wide lock so it can't race the
        // sibling test below (or PATH tests elsewhere) under parallel runs.
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
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
        // Mutates process env; share the crate-wide lock (see sibling test).
        let _guard = crate::ENV_TEST_LOCK.lock().unwrap();
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

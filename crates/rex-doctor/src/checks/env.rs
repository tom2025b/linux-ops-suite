//! `env.*` — environment & PATH checks. Answers "is this machine set up so the
//! suite's binaries and data are even findable?" before we bother probing them.
//!
//! All read-only: these `stat` directories and read `$PATH`/`$HOME`/`$XDG_*`.
//! The messages carry the exact line to add to PATH — the same guidance
//! `linux-ops-install` prints — so a FAIL is self-resolving.

use std::path::{Path, PathBuf};

use crate::model::{Category, Check};
use crate::util;

const CAT: Category = Category::Env;

/// Run every `env.*` check, in id order.
pub fn run() -> Vec<Check> {
    let Some(home) = util::home() else {
        // Without $HOME we can't anchor any path. One FAIL stands in for the
        // group rather than emitting five identical "no $HOME" lines.
        return vec![Check::fail(
            "env.install-dirs",
            CAT,
            "cannot resolve $HOME, so suite paths can't be located",
            "set $HOME and re-run",
        )];
    };

    vec![
        install_dirs(&home),
        xdg_data(&home),
        writable(&home),
        no_color(),
        shell_rc(&home),
    ]
}

/// `env.install-dirs` — `~/.local/bin` and `~/bin` (the dirs `linux-ops-install`
/// targets) exist and are on `PATH`. A binary installed into a dir that isn't
/// on PATH is invisible, which looks exactly like "not installed".
fn install_dirs(home: &Path) -> Check {
    let id = "env.install-dirs";
    let local_bin = home.join(".local/bin");
    let bin = home.join("bin");

    let mut missing_dir = Vec::new();
    let mut off_path = Vec::new();
    for dir in [&local_bin, &bin] {
        if !dir.is_dir() {
            missing_dir.push(dir.display().to_string());
        } else if !util::dir_on_path(dir) {
            off_path.push(dir.display().to_string());
        }
    }

    if missing_dir.is_empty() && off_path.is_empty() {
        return Check::pass(id, CAT, "~/.local/bin and ~/bin exist and are on PATH");
    }
    // The fix is the same export line the installer prints.
    let fix = "add to your shell rc: export PATH=\"$HOME/.local/bin:$HOME/bin:$PATH\"";
    if !off_path.is_empty() {
        Check::fail(
            id,
            CAT,
            format!("install dir(s) not on PATH: {}", off_path.join(", ")),
            fix,
        )
    } else {
        // Dirs missing entirely: a warning — they're created on install, and
        // their absence just means nothing's installed there yet.
        Check::warn(
            id,
            CAT,
            format!("install dir(s) missing: {}", missing_dir.join(", ")),
            fix,
        )
    }
}

/// `env.xdg-data` — the XDG data dir the suite writes feeds under resolves.
/// Reports the concrete `rexops/feeds` path the flow checks will read, so the
/// operator sees exactly where state is expected to live.
fn xdg_data(home: &Path) -> Check {
    let id = "env.xdg-data";
    let (base, from) = match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(x) => (PathBuf::from(x), "$XDG_DATA_HOME"),
        None => (home.join(".local/share"), "~/.local/share (fallback)"),
    };
    let feeds = base.join("rexops/feeds");
    Check::pass(
        id,
        CAT,
        format!("feeds dir resolves via {from}: {}", feeds.display()),
    )
}

/// `env.writable` — `~/.local/bin` and the feeds dir are writable by this user.
/// Catches the classic "installed with sudo, now root owns it" failure where a
/// later non-root install or feed write silently can't proceed.
fn writable(home: &Path) -> Check {
    let id = "env.writable";
    let local_bin = home.join(".local/bin");
    let feeds = xdg_feeds_dir(home);

    let mut not_writable = Vec::new();
    for dir in [&local_bin, &feeds] {
        // Only judge dirs that exist; a missing dir is env.install-dirs' job.
        if dir.is_dir() && is_readonly(dir) {
            not_writable.push(dir.display().to_string());
        }
    }
    if not_writable.is_empty() {
        Check::pass(id, CAT, "install and feeds dirs are writable")
    } else {
        Check::fail(
            id,
            CAT,
            format!("not writable by you: {}", not_writable.join(", ")),
            format!(
                "fix ownership: sudo chown -R \"$USER\" {}",
                not_writable.join(" ")
            ),
        )
    }
}

/// `env.no-color` — report whether color output is on, and why. Purely
/// informational (always PASS): a run that looks oddly plain or oddly colored
/// is then self-explaining rather than a mystery.
fn no_color() -> Check {
    let id = "env.no-color";
    let tty = util::stdout_is_tty();
    let no_color_set = std::env::var_os("NO_COLOR").is_some();
    let on = tty && !no_color_set;
    let why = if !tty {
        "stdout is not a TTY"
    } else if no_color_set {
        "NO_COLOR is set"
    } else {
        "TTY and NO_COLOR unset"
    };
    Check::pass(
        id,
        CAT,
        format!("color {} ({why})", if on { "on" } else { "off" }),
    )
}

/// `env.shell-rc` — `~/.rust_aliases.sh` (the suite's shell aliases) exists and
/// looks sourced from a login/interactive rc. WARN, never edits: missing
/// sourcing only means the convenience aliases won't be present in new shells;
/// the bare binaries still work off PATH.
fn shell_rc(home: &Path) -> Check {
    let id = "env.shell-rc";
    let aliases = home.join(".rust_aliases.sh");
    if !aliases.is_file() {
        return Check::warn(
            id,
            CAT,
            "~/.rust_aliases.sh not found (suite shell aliases absent)",
            "create it and source it from your shell rc",
        );
    }
    if rc_sources_aliases(home) {
        Check::pass(id, CAT, "~/.rust_aliases.sh exists and is sourced from rc")
    } else {
        Check::warn(
            id,
            CAT,
            "~/.rust_aliases.sh exists but no rc file appears to source it",
            "add to ~/.bashrc or ~/.zshrc: source ~/.rust_aliases.sh",
        )
    }
}

/// Whether any common rc file contains a line sourcing `~/.rust_aliases.sh`.
/// Best-effort substring match — we only ever WARN on the result, never act.
fn rc_sources_aliases(home: &Path) -> bool {
    for rc in [".bashrc", ".zshrc", ".profile", ".bash_profile"] {
        let path = home.join(rc);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if text.contains("rust_aliases.sh") {
                return true;
            }
        }
    }
    false
}

/// The concrete `…/rexops/feeds` directory, mirroring `env.xdg-data`'s logic.
fn xdg_feeds_dir(home: &Path) -> PathBuf {
    let base = match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x),
        None => home.join(".local/share"),
    };
    base.join("rexops/feeds")
}

/// Whether a directory's permissions deny write to its owner. A coarse,
/// no-extra-deps proxy for "I can't write here"; the common sudo-ownership case
/// shows up as a non-owner-writable dir.
fn is_readonly(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(dir) {
        Ok(md) => (md.permissions().mode() & 0o200) == 0,
        // If we can't stat it, treat as not-readonly here; env.install-dirs and
        // the actual write site will surface a real problem.
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_emits_one_check_per_env_id() {
        // With a real $HOME present in the test env, all five ids appear once.
        if util::home().is_some() {
            let checks = run();
            let ids: Vec<_> = checks.iter().map(|c| c.id).collect();
            assert!(ids.contains(&"env.install-dirs"));
            assert!(ids.contains(&"env.xdg-data"));
            assert!(ids.contains(&"env.writable"));
            assert!(ids.contains(&"env.no-color"));
            assert!(ids.contains(&"env.shell-rc"));
            // Every env check is tagged with the Env category.
            assert!(checks.iter().all(|c| c.category == Category::Env));
        }
    }

    #[test]
    fn xdg_data_is_informational_pass() {
        let home = PathBuf::from("/home/example");
        let c = xdg_data(&home);
        assert_eq!(c.status, crate::model::Status::Pass);
        assert!(c.detail.contains("rexops/feeds"));
    }

    #[test]
    fn no_color_reports_a_reason() {
        let c = no_color();
        assert_eq!(c.status, crate::model::Status::Pass);
        assert!(c.detail.starts_with("color "));
    }
}

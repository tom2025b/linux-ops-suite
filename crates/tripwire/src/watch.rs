//! The watch set: which paths tripwire looks at, with what options, and where
//! that list came from. Resolution precedence is `--path` flags, then a config
//! file (`--config` or the default `watch.conf`), then a built-in default set.
//! The source is always recorded so `tripwire watch` and the JSON envelope can
//! say exactly what is covered and why — nothing is watched silently.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::TripwireError;
use crate::util;

/// Where the active watch set was resolved from. Recorded in the JSON envelope
/// and shown by `tripwire watch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchSource {
    /// `--path` flags on the command line.
    Cli,
    /// A `watch.conf` (explicit `--config` or the default XDG path).
    Config,
    /// The compiled-in default set.
    Builtin,
}

impl WatchSource {
    pub fn tag(self) -> &'static str {
        match self {
            WatchSource::Cli => "cli",
            WatchSource::Config => "config",
            WatchSource::Builtin => "builtin",
        }
    }
}

/// One path to watch, plus its per-path options. Options have file-vs-dir
/// defaults that only take effect once the kind is known at scan time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEntry {
    /// The (tilde-expanded) path to watch.
    pub path: PathBuf,
    /// Descend into subdirectories (directories only). Default true.
    pub recursive: bool,
    /// Follow symlinks instead of recording them as symlinks. Default false —
    /// following can escape the watch set, a footgun for an integrity tool.
    pub follow_symlinks: bool,
    /// Hash file contents (files only). Default true; set false for large or
    /// append-only files where only metadata drift matters.
    pub content: bool,
    /// Glob patterns pruned during a recursive walk (matched against each
    /// entry's file name and its full path).
    pub exclude: Vec<String>,
}

impl WatchEntry {
    /// A watch entry for `path` with all defaults.
    pub fn new(path: PathBuf) -> Self {
        WatchEntry {
            path,
            recursive: true,
            follow_symlinks: false,
            content: true,
            exclude: Vec::new(),
        }
    }
}

/// The resolved watch set and the source it came from.
#[derive(Debug, Clone)]
pub struct WatchSet {
    pub entries: Vec<WatchEntry>,
    pub source: WatchSource,
}

/// Resolve the watch set following the precedence: CLI `--path` flags win;
/// otherwise a config file (explicit `--config`, else the default `watch.conf`
/// if it exists); otherwise the built-in default set.
///
/// Errors only when an *explicit* `--config` can't be read or parses to nothing
/// (the operator asked for that file by name, so silence would be wrong). A
/// missing *default* config simply falls through to the built-in set.
pub fn resolve(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
) -> Result<WatchSet, TripwireError> {
    if !cli_paths.is_empty() {
        let entries = cli_paths
            .iter()
            .map(|p| WatchEntry::new(p.clone()))
            .collect();
        return Ok(WatchSet {
            entries,
            source: WatchSource::Cli,
        });
    }

    // Explicit --config: must exist and yield entries, or it's an error.
    if let Some(cfg) = config_override {
        let entries = parse_config_file(cfg)?;
        if entries.is_empty() {
            return Err(TripwireError::EmptyWatchSet);
        }
        return Ok(WatchSet {
            entries,
            source: WatchSource::Config,
        });
    }

    // Default config path: use it only if it exists; else fall through.
    if let Some(default_cfg) = util::config_path() {
        if default_cfg.exists() {
            let entries = parse_config_file(&default_cfg)?;
            if !entries.is_empty() {
                return Ok(WatchSet {
                    entries,
                    source: WatchSource::Config,
                });
            }
        }
    }

    Ok(WatchSet {
        entries: builtin_entries(),
        source: WatchSource::Builtin,
    })
}

/// The compiled-in default watch set: the system files and dotfiles an operator
/// most often wants to know changed. Missing paths are skipped at scan time, so
/// this set is safe to ship as-is on any host.
pub fn builtin_entries() -> Vec<WatchEntry> {
    const SYSTEM: &[&str] = &[
        "/etc/passwd",
        "/etc/group",
        "/etc/shadow",
        "/etc/sudoers",
        "/etc/hosts",
        "/etc/hostname",
        "/etc/fstab",
        "/etc/crontab",
        "/etc/ssh/sshd_config",
        "/etc/ssh/ssh_config",
    ];
    const DIRS: &[&str] = &["/etc/cron.d"];
    const DOTFILES: &[&str] = &[
        "~/.ssh/authorized_keys",
        "~/.bashrc",
        "~/.zshrc",
        "~/.profile",
    ];

    let mut entries = Vec::new();
    for p in SYSTEM.iter().chain(DOTFILES.iter()) {
        entries.push(WatchEntry::new(util::expand_tilde(p)));
    }
    for d in DIRS {
        // Recursive by default; exclude is empty.
        entries.push(WatchEntry::new(util::expand_tilde(d)));
    }
    entries
}

/// Read and parse a line-based `watch.conf`. Distinguishes "couldn't read the
/// file" (an error) from "read it but it has no entries" (the caller decides).
fn parse_config_file(path: &Path) -> Result<Vec<WatchEntry>, TripwireError> {
    let text = fs::read_to_string(path).map_err(|e| TripwireError::BadConfig {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })?;
    Ok(parse_config(&text))
}

/// Parse the line-based config format. One watch entry per non-blank,
/// non-comment line:
///
/// ```text
/// # comment
/// /etc/ssh/sshd_config
/// /etc/cron.d            recursive=false
/// /var/log/app.log       content=false
/// /srv/www               exclude=*.log exclude=.git recursive=true
/// ```
///
/// The path is the first whitespace-delimited token (tilde-expanded); any
/// remaining `key=value` tokens set options. Unknown keys and malformed
/// booleans are ignored rather than failing the whole run — a forgiving parser
/// keeps a hand-rolled format from becoming brittle.
pub fn parse_config(text: &str) -> Vec<WatchEntry> {
    let mut entries = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        // A `#` is only a comment when it starts the line — `#` is a legal
        // character in a filesystem path (e.g. `/var/data#v2.log`), so stripping
        // from the first `#` anywhere on the line would silently truncate such a
        // path and watch the wrong file. Full-line comments and blanks are
        // skipped; everything else is taken verbatim.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut tokens = line.split_whitespace();
        let path_tok = match tokens.next() {
            Some(p) => p,
            None => continue,
        };
        let mut entry = WatchEntry::new(util::expand_tilde(path_tok));

        for opt in tokens {
            let (key, value) = match opt.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            match key {
                "recursive" => {
                    if let Some(b) = parse_bool(value) {
                        entry.recursive = b;
                    }
                }
                "follow_symlinks" => {
                    if let Some(b) = parse_bool(value) {
                        entry.follow_symlinks = b;
                    }
                }
                "content" => {
                    if let Some(b) = parse_bool(value) {
                        entry.content = b;
                    }
                }
                "exclude" if !value.is_empty() => {
                    entry.exclude.push(value.to_string());
                }
                _ => {} // unknown key: ignore
            }
        }
        entries.push(entry);
    }
    entries
}

/// Parse a permissive boolean for config options.
fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// Match a path component against a tiny glob supporting `*` (any run, including
/// empty) and `?` (one char). Used for `exclude` patterns. Anchored: the whole
/// candidate must match the whole pattern. Kept deliberately small — full
/// regex/globbing would mean a dependency.
pub fn glob_match(pattern: &str, candidate: &str) -> bool {
    glob_inner(pattern.as_bytes(), candidate.as_bytes())
}

fn glob_inner(pat: &[u8], txt: &[u8]) -> bool {
    // Iterative backtracking matcher (no recursion blow-up on `***`).
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star_p, mut star_t): (Option<usize>, usize) = (None, 0);

    while t < txt.len() {
        if p < pat.len() && (pat[p] == b'?' || pat[p] == txt[t]) {
            p += 1;
            t += 1;
        } else if p < pat.len() && pat[p] == b'*' {
            star_p = Some(p);
            star_t = t;
            p += 1;
        } else if let Some(sp) = star_p {
            // Backtrack: let the last `*` swallow one more char.
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == b'*' {
        p += 1;
    }
    p == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_paths_take_precedence_and_record_source() {
        let cli = vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")];
        let set = resolve(&cli, None).unwrap();
        assert_eq!(set.source, WatchSource::Cli);
        assert_eq!(set.entries.len(), 2);
        assert_eq!(set.entries[0].path, PathBuf::from("/tmp/a"));
    }

    #[test]
    fn builtin_used_when_no_cli_and_no_config() {
        // No CLI paths, no explicit config. If the user happens to have a real
        // default watch.conf this could be Config; accept either non-CLI source
        // but require a non-empty set.
        let set = resolve(&[], None).unwrap();
        assert_ne!(set.source, WatchSource::Cli);
        assert!(!set.entries.is_empty());
    }

    #[test]
    fn builtin_set_covers_the_key_system_files() {
        let entries = builtin_entries();
        let has = |p: &str| entries.iter().any(|e| e.path == Path::new(p));
        assert!(has("/etc/passwd"));
        assert!(has("/etc/shadow"));
        assert!(has("/etc/ssh/sshd_config"));
        assert!(has("/etc/cron.d"));
    }

    #[test]
    fn hash_in_path_is_kept_but_leading_hash_is_a_comment() {
        // L3 regression: `#` is legal in a filesystem path. Only a line whose
        // first non-space char is `#` is a comment; an inline `#` must not
        // truncate the path (which would silently watch the wrong file).
        let text = "\
# a real comment
/var/data#v2.log       content=false
    # indented comment
/srv/cache#tmp
";
        let entries = parse_config(text);
        assert_eq!(entries.len(), 2, "two paths, two comments");
        assert_eq!(entries[0].path, PathBuf::from("/var/data#v2.log"));
        assert!(!entries[0].content);
        assert_eq!(entries[1].path, PathBuf::from("/srv/cache#tmp"));
    }

    #[test]
    fn parse_config_reads_paths_and_options() {
        let text = "\
# system
/etc/ssh/sshd_config
/etc/cron.d            recursive=false
/var/log/app.log       content=false
/srv/www               exclude=*.log exclude=.git recursive=true follow_symlinks=yes

   # blank line above, indented comment here
";
        let entries = parse_config(text);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].path, PathBuf::from("/etc/ssh/sshd_config"));
        assert!(entries[0].recursive); // default

        assert_eq!(entries[1].path, PathBuf::from("/etc/cron.d"));
        assert!(!entries[1].recursive);

        assert_eq!(entries[2].path, PathBuf::from("/var/log/app.log"));
        assert!(!entries[2].content);

        let www = &entries[3];
        assert_eq!(www.exclude, vec!["*.log".to_string(), ".git".to_string()]);
        assert!(www.recursive);
        assert!(www.follow_symlinks);
    }

    #[test]
    fn parse_config_ignores_unknown_keys_and_bad_bools() {
        let entries = parse_config("/x  bogus=1 recursive=maybe content=off");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].recursive); // "maybe" rejected -> default true
        assert!(!entries[0].content); // "off" accepted
    }

    #[test]
    fn parse_config_strips_inline_comments() {
        let entries = parse_config("/etc/hosts   # the hosts file");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn glob_matches_star_and_question() {
        assert!(glob_match("*.log", "app.log"));
        assert!(glob_match("*.log", ".log"));
        assert!(!glob_match("*.log", "app.txt"));
        assert!(glob_match("__pycache__", "__pycache__"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
    }
}

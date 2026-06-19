//! The capture set: which paths rewind records, with what options, and where
//! that list came from. Resolution precedence is `--path` flags, then a config
//! file (`--config` or the default `capture.conf`), then a built-in default set.
//! The source is always recorded so `rewind sources` and the JSON envelope can
//! say exactly what is covered and why — nothing is captured silently. This is
//! tripwire's watch-set resolver, narrowed to rewind's needs (no `content`
//! toggle — rewind always stores content) with rewind's three-item default set.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::RewindError;
use crate::util;

/// Where the active capture set was resolved from. Recorded in the JSON envelope
/// and shown by `rewind sources`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetSource {
    /// `--path` flags on the command line.
    Cli,
    /// A `capture.conf` (explicit `--config` or the default XDG path).
    Config,
    /// The compiled-in default set.
    Builtin,
}

impl SetSource {
    pub fn tag(self) -> &'static str {
        match self {
            SetSource::Cli => "cli",
            SetSource::Config => "config",
            SetSource::Builtin => "builtin",
        }
    }
}

/// One path to capture, plus its per-path options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSpec {
    /// The (tilde-expanded) path to capture.
    pub path: PathBuf,
    /// Descend into subdirectories (directories only). Default true.
    pub recursive: bool,
    /// Follow symlinks instead of recording them as symlinks. Default false —
    /// following can escape the capture set, a footgun for a rollback tool.
    pub follow_symlinks: bool,
    /// Glob patterns pruned during a recursive walk (matched against each
    /// entry's file name and its full path).
    pub exclude: Vec<String>,
}

impl CaptureSpec {
    /// A capture spec for `path` with all defaults.
    pub fn new(path: PathBuf) -> Self {
        CaptureSpec {
            path,
            recursive: true,
            follow_symlinks: false,
            exclude: Vec::new(),
        }
    }
}

/// The resolved capture set and the source it came from.
#[derive(Debug, Clone)]
pub struct CaptureSet {
    pub specs: Vec<CaptureSpec>,
    pub source: SetSource,
}

/// Resolve the capture set following the precedence: CLI `--path` flags win;
/// otherwise a config file (explicit `--config`, else the default
/// `capture.conf` if it exists); otherwise the built-in default set.
///
/// Errors only when an *explicit* `--config` can't be read or parses to nothing
/// (the operator asked for that file by name, so silence would be wrong). A
/// missing *default* config simply falls through to the built-in set.
pub fn resolve(
    cli_paths: &[PathBuf],
    config_override: Option<&Path>,
) -> Result<CaptureSet, RewindError> {
    if !cli_paths.is_empty() {
        let specs = cli_paths
            .iter()
            .map(|p| CaptureSpec::new(p.clone()))
            .collect();
        return Ok(CaptureSet {
            specs,
            source: SetSource::Cli,
        });
    }

    // Explicit --config: must exist and yield specs, or it's an error.
    if let Some(cfg) = config_override {
        let specs = parse_config_file(cfg)?;
        if specs.is_empty() {
            return Err(RewindError::EmptySet);
        }
        return Ok(CaptureSet {
            specs,
            source: SetSource::Config,
        });
    }

    // Default config path: use it only if it exists; else fall through.
    if let Some(default_cfg) = util::config_path() {
        if default_cfg.exists() {
            let specs = parse_config_file(&default_cfg)?;
            if !specs.is_empty() {
                return Ok(CaptureSet {
                    specs,
                    source: SetSource::Config,
                });
            }
        }
    }

    Ok(CaptureSet {
        specs: builtin_specs(),
        source: SetSource::Builtin,
    })
}

/// The compiled-in default capture set: the suite's own state files. Each is
/// existing-only — a missing path is skipped at scan time, so the set is safe to
/// ship as-is on any host. The three items make rewind a forensic black box:
/// the compiled snapshot, the producer feeds that fed it, and tripwire's
/// integrity baseline (so even that is recoverable).
pub fn builtin_specs() -> Vec<CaptureSpec> {
    let data_base = data_home();
    vec![
        // The flagship: the compiled Workstate snapshot RexOps consumes.
        CaptureSpec::new(
            data_base
                .join("rexops")
                .join("feeds")
                .join("workstate.snapshot.json"),
        ),
        // The producer feeds directory (the inputs to the compile), recursive.
        CaptureSpec::new(data_base.join("workstate").join("feeds")),
        // Tripwire's integrity baseline, so the baseline itself is recoverable.
        CaptureSpec::new(
            data_base
                .join("linux-ops-suite")
                .join("tripwire")
                .join("baseline.json"),
        ),
    ]
}

/// The base of the suite's data area (`$XDG_DATA_HOME` or `~/.local/share`),
/// under which the per-tool dirs live. Falls back to `~/.local/share` when
/// `$XDG_DATA_HOME` is unset, and to `.` only if even `$HOME` is missing (in
/// which case the builtin paths simply won't exist and are skipped).
fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| util::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Read and parse a line-based `capture.conf`. Distinguishes "couldn't read the
/// file" (an error) from "read it but it has no entries" (the caller decides).
fn parse_config_file(path: &Path) -> Result<Vec<CaptureSpec>, RewindError> {
    let text = fs::read_to_string(path).map_err(|e| RewindError::BadConfig {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })?;
    Ok(parse_config(&text))
}

/// Parse the line-based config format. One capture spec per non-blank,
/// non-comment line:
///
/// ```text
/// # comment
/// ~/.local/share/rexops/feeds/workstate.snapshot.json
/// ~/.local/share/workstate/feeds   recursive=true exclude=*.tmp
/// ```
///
/// The path is the first whitespace-delimited token (tilde-expanded); any
/// remaining `key=value` tokens set options. Unknown keys and malformed
/// booleans are ignored rather than failing the whole run — a forgiving parser
/// keeps a hand-rolled format from becoming brittle. (`content` is accepted and
/// ignored for tripwire-config compatibility: rewind always stores content.)
pub fn parse_config(text: &str) -> Vec<CaptureSpec> {
    let mut specs = Vec::new();
    for raw in text.lines() {
        let line = match raw.split('#').next() {
            Some(l) => l.trim(),
            None => continue,
        };
        if line.is_empty() {
            continue;
        }

        let mut tokens = line.split_whitespace();
        let path_tok = match tokens.next() {
            Some(p) => p,
            None => continue,
        };
        let mut spec = CaptureSpec::new(util::expand_tilde(path_tok));

        for opt in tokens {
            let (key, value) = match opt.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            match key {
                "recursive" => {
                    if let Some(b) = parse_bool(value) {
                        spec.recursive = b;
                    }
                }
                "follow_symlinks" => {
                    if let Some(b) = parse_bool(value) {
                        spec.follow_symlinks = b;
                    }
                }
                "exclude" if !value.is_empty() => {
                    spec.exclude.push(value.to_string());
                }
                _ => {} // unknown key (incl. `content`): ignore
            }
        }
        specs.push(spec);
    }
    specs
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
/// candidate must match the whole pattern. Kept small to avoid a dependency.
pub fn glob_match(pattern: &str, candidate: &str) -> bool {
    glob_inner(pattern.as_bytes(), candidate.as_bytes())
}

fn glob_inner(pat: &[u8], txt: &[u8]) -> bool {
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
        assert_eq!(set.source, SetSource::Cli);
        assert_eq!(set.specs.len(), 2);
        assert_eq!(set.specs[0].path, PathBuf::from("/tmp/a"));
    }

    #[test]
    fn builtin_used_when_no_cli_and_no_config() {
        let set = resolve(&[], None).unwrap();
        assert_ne!(set.source, SetSource::Cli);
        assert!(!set.specs.is_empty());
    }

    #[test]
    fn builtin_set_is_the_three_suite_state_targets() {
        let specs = builtin_specs();
        assert_eq!(specs.len(), 3);
        let joined: Vec<String> = specs
            .iter()
            .map(|s| s.path.to_string_lossy().into_owned())
            .collect();
        assert!(joined
            .iter()
            .any(|p| p.ends_with("rexops/feeds/workstate.snapshot.json")));
        assert!(joined.iter().any(|p| p.ends_with("workstate/feeds")));
        assert!(joined
            .iter()
            .any(|p| p.ends_with("linux-ops-suite/tripwire/baseline.json")));
    }

    #[test]
    fn parse_config_reads_paths_and_options() {
        let text = "\
# suite state
/d/workstate.snapshot.json
/d/feeds                recursive=false
/srv/www                exclude=*.tmp exclude=.git follow_symlinks=yes content=false

   # an indented comment, and a blank line above
";
        let specs = parse_config(text);
        assert_eq!(specs.len(), 3);

        assert_eq!(specs[0].path, PathBuf::from("/d/workstate.snapshot.json"));
        assert!(specs[0].recursive); // default

        assert_eq!(specs[1].path, PathBuf::from("/d/feeds"));
        assert!(!specs[1].recursive);

        let www = &specs[2];
        assert_eq!(www.exclude, vec!["*.tmp".to_string(), ".git".to_string()]);
        assert!(www.follow_symlinks);
        // `content=false` is accepted-and-ignored (rewind always stores content).
    }

    #[test]
    fn parse_config_ignores_unknown_keys_and_bad_bools() {
        let specs = parse_config("/x  bogus=1 recursive=maybe follow_symlinks=on");
        assert_eq!(specs.len(), 1);
        assert!(specs[0].recursive); // "maybe" rejected -> default true
        assert!(specs[0].follow_symlinks); // "on" accepted
    }

    #[test]
    fn parse_config_strips_inline_comments() {
        let specs = parse_config("/etc/hosts   # the hosts file");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn glob_matches_star_and_question() {
        assert!(glob_match("*.tmp", "scratch.tmp"));
        assert!(!glob_match("*.tmp", "keep.json"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "axxbyyc"));
    }
}

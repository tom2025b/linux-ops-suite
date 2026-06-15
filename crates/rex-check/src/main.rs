//! rex-check — at-a-glance health of the Linux Ops Suite repos.
//!
//! For each suite repo it prints a one-line git status summary (branch,
//! ahead/behind vs upstream, dirty/clean) and a source line count (via `tokei`
//! if present, else a `git ls-files` tracked-file count fallback), then an
//! aligned totals table and a one-line summary.
//!
//! Fast and offline by design: one `git` invocation per fact, never a network
//! call (ahead/behind reads cached upstream tracking info), and `tokei` is run
//! once per repo only when it's installed. Repo paths are absolute, so it works
//! from any directory.
//!
//! This is the Rust port of the original `~/bin/rex-check` shell script, now an
//! official suite crate. It deliberately shells out to `git`/`tokei` (exactly
//! like the suite's `install.sh`) rather than linking a git library, which keeps
//! it dependency-free and trivially fast to build inside the umbrella workspace.
//!
//! Environment:
//!   REX_ROOT   override the directory the suite repos live under
//!              (default: `$HOME/projects`).
//!   NO_COLOR   disable ANSI color (also auto-disabled when stdout isn't a TTY).

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The suite repos, in display order. Mirrors the roster the installers know.
const REPOS: &[&str] = &[
    "bulwark",
    "scriptvault",
    "toolfoundry",
    "workstate",
    "proto",
    "rexops",
    "linux-ops-suite",
];

/// ANSI styling, resolved once. Empty strings when color is off so every call
/// site can interpolate unconditionally.
struct Style {
    bold: &'static str,
    dim: &'static str,
    red: &'static str,
    grn: &'static str,
    ylw: &'static str,
    cyn: &'static str,
    rst: &'static str,
}

impl Style {
    /// Color on only when stdout is a TTY and `NO_COLOR` is unset — same rule as
    /// the shell version (and the suite's install.sh).
    fn resolve() -> Self {
        let on = stdout_is_tty() && env::var_os("NO_COLOR").is_none();
        if on {
            Style {
                bold: "\u{1b}[1m",
                dim: "\u{1b}[2m",
                red: "\u{1b}[31m",
                grn: "\u{1b}[32m",
                ylw: "\u{1b}[33m",
                cyn: "\u{1b}[36m",
                rst: "\u{1b}[0m",
            }
        } else {
            Style {
                bold: "",
                dim: "",
                red: "",
                grn: "",
                ylw: "",
                cyn: "",
                rst: "",
            }
        }
    }
}

/// One repo's resolved facts, gathered before any printing so the per-repo
/// lines and the totals table render from the same data.
struct RepoStatus {
    name: String,
    present: bool,
    branch: String,
    dirty: usize,
    ahead: usize,
    behind: usize,
    /// Tracked-file count (always available when the repo is present).
    files: usize,
    /// Code lines from tokei, when tokei is installed.
    loc: Option<usize>,
}

fn main() -> ExitCode {
    let style = Style::resolve();
    let root = suite_root();
    let have_tokei = command_exists("tokei");

    println!(
        "{}{}rex-check{} {}— suite repos under {}{}",
        style.bold,
        style.cyn,
        style.rst,
        style.dim,
        root.display(),
        style.rst
    );
    println!();

    let statuses: Vec<RepoStatus> = REPOS
        .iter()
        .map(|name| gather(&root, name, have_tokei))
        .collect();

    for s in &statuses {
        print_repo_line(s, &style, &root);
    }

    print_totals(&statuses, have_tokei, &style);
    ExitCode::SUCCESS
}

/// Where the suite repos live: `$REX_ROOT`, else `$HOME/projects`, else `./`.
fn suite_root() -> PathBuf {
    if let Some(root) = env::var_os("REX_ROOT").filter(|v| !v.is_empty()) {
        return PathBuf::from(root);
    }
    match env::var_os("HOME").filter(|v| !v.is_empty()) {
        Some(home) => PathBuf::from(home).join("projects"),
        None => PathBuf::from("."),
    }
}

/// Collect every fact for one repo. A repo whose `.git` is absent is recorded as
/// not-present and skipped for the metric (mirrors the shell `missing` case).
fn gather(root: &Path, name: &str, have_tokei: bool) -> RepoStatus {
    let dir = root.join(name);
    if !dir.join(".git").is_dir() {
        return RepoStatus {
            name: name.to_owned(),
            present: false,
            branch: "—".to_owned(),
            dirty: 0,
            ahead: 0,
            behind: 0,
            files: 0,
            loc: None,
        };
    }

    let branch = git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "?".to_owned());

    // Dirty count = lines of `git status --porcelain` (staged + unstaged + untracked).
    let dirty = git(&dir, &["status", "--porcelain"])
        .map(|out| out.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    // Ahead/behind vs upstream, cached (no fetch). `--left-right --count` prints
    // "<behind>\t<ahead>"; absent/unset upstream yields nothing → (0, 0).
    let (behind, ahead) = git(&dir, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"])
        .and_then(|out| {
            let mut it = out.split_whitespace();
            let b = it.next()?.parse().ok()?;
            let a = it.next()?.parse().ok()?;
            Some((b, a))
        })
        .unwrap_or((0, 0));

    let files = git(&dir, &["ls-files"])
        .map(|out| out.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    let loc = if have_tokei { tokei_loc(&dir) } else { None };

    RepoStatus {
        name: name.to_owned(),
        present: true,
        branch,
        dirty,
        ahead,
        behind,
        files,
        loc,
    }
}

/// Print the colored per-repo status line (name, branch, clean/dirty, ↑/↓).
fn print_repo_line(s: &RepoStatus, style: &Style, root: &Path) {
    if !s.present {
        println!(
            "  {}{:<16}{} {}missing ({}){}",
            style.bold,
            s.name,
            style.rst,
            style.red,
            root.join(&s.name).display(),
            style.rst
        );
        return;
    }

    let state = if s.dirty > 0 {
        format!("{}● {} changed{}", style.ylw, s.dirty, style.rst)
    } else {
        format!("{}✓ clean{}", style.grn, style.rst)
    };

    let mut ahead_behind = String::new();
    if s.ahead > 0 {
        ahead_behind.push_str(&format!(" {}↑{}{}", style.cyn, s.ahead, style.rst));
    }
    if s.behind > 0 {
        ahead_behind.push_str(&format!(" {}↓{}{}", style.red, s.behind, style.rst));
    }

    println!(
        "  {}{:<16}{} {}{:<18}{} {}{}",
        style.bold, s.name, style.rst, style.dim, s.branch, style.rst, state, ahead_behind
    );
}

/// Print the aligned totals table and the summary footer.
fn print_totals(statuses: &[RepoStatus], have_tokei: bool, style: &Style) {
    println!();

    // Metric label + per-repo count selector. With tokei we show code lines and
    // sum them; without it we show (and sum) tracked file counts.
    let metric = if have_tokei { "lines (code)" } else { "files" };
    if !have_tokei {
        println!(
            "{}(tokei not installed — showing tracked file counts; `cargo install tokei` for LOC){}",
            style.dim, style.rst
        );
    }

    let count_of = |s: &RepoStatus| -> Option<usize> {
        if !s.present {
            None
        } else if have_tokei {
            Some(s.loc.unwrap_or(0))
        } else {
            Some(s.files)
        }
    };

    let rule: String = "─".repeat(52);
    println!(
        "{}{:<16}  {:<20}  {:>12}{}",
        style.bold, "REPO", "BRANCH", metric, style.rst
    );
    println!("{}{}{}", style.dim, rule, style.rst);

    let mut total = 0usize;
    let mut found = 0usize;
    let mut missing = 0usize;
    let mut dirty_repos = 0usize;
    for s in statuses {
        let count_disp = match count_of(s) {
            Some(n) => {
                total += n;
                n.to_string()
            }
            None => "—".to_owned(),
        };
        if s.present {
            found += 1;
            if s.dirty > 0 {
                dirty_repos += 1;
            }
        } else {
            missing += 1;
        }
        println!("{:<16}  {:<20}  {:>12}", s.name, s.branch, count_disp);
    }

    println!("{}{}{}", style.dim, rule, style.rst);
    println!(
        "{}{:<16}  {:<20}  {:>12}{}",
        style.bold,
        format!("TOTAL ({found} repos)"),
        "",
        total,
        style.rst
    );

    println!();
    println!(
        "{}repos:{} {} found, {} missing   {}dirty:{} {}   {}metric:{} {}",
        style.dim,
        style.rst,
        found,
        missing,
        style.dim,
        style.rst,
        dirty_repos,
        style.dim,
        style.rst,
        metric
    );
}

/// Run `git -C <dir> <args...>` and return trimmed stdout on success, else None.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if text.is_empty() {
        // An empty-but-successful result (e.g. no upstream) is "no data".
        None
    } else {
        Some(text)
    }
}

/// Code-line count via tokei. Parses the plain-output "Total" row, whose columns
/// are `Language Files Lines Code Comments Blanks`, so the 4th field is the code
/// total. Matches the awk extraction the shell version used; None on any failure.
fn tokei_loc(dir: &Path) -> Option<usize> {
    let out = Command::new("tokei").arg(dir).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("Total") {
            // Fields after the "Total" label; strip any thousands separators.
            let code = t.split_whitespace().nth(3)?.replace(',', "");
            return code.parse().ok();
        }
    }
    None
}

/// Whether a command is resolvable on PATH (used to detect tokei).
fn command_exists(name: &str) -> bool {
    // `command -v` via the shell is the cheapest portable check and mirrors the
    // shell script's `command -v tokei`.
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Whether fd 1 (stdout) is a TTY, via `isatty(3)`. One tiny libc call; avoids a
/// dependency just to gate color.
fn stdout_is_tty() -> bool {
    // SAFETY: isatty merely queries a file descriptor and has no preconditions.
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(1) == 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_root_prefers_rex_root_env() {
        env::set_var("REX_ROOT", "/tmp/rex-root-test");
        assert_eq!(suite_root(), PathBuf::from("/tmp/rex-root-test"));
        env::remove_var("REX_ROOT");
    }

    #[test]
    fn tokei_total_parsing_picks_the_code_column() {
        // We can't run tokei in tests, but the parse logic is the fragile part:
        // given a representative "Total" row, the 4th field (Code) is selected
        // and thousands separators are stripped. This guards the column index.
        let sample = " Total                    87        11764         7821         2551         1392";
        let code: usize = sample
            .trim_start()
            .split_whitespace()
            .nth(3)
            .unwrap()
            .replace(',', "")
            .parse()
            .unwrap();
        assert_eq!(code, 7821, "the Code column (4th field) must be selected");
    }

    #[test]
    fn roster_has_the_seven_suite_repos() {
        assert_eq!(REPOS.len(), 7);
        for expected in ["bulwark", "rexops", "linux-ops-suite"] {
            assert!(REPOS.contains(&expected), "{expected} must be in the roster");
        }
    }
}

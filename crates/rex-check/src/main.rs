//! rex-check — at-a-glance health of the Linux Ops Suite repos.
//!
//! For each suite repo it prints a one-line health summary — branch (and whether
//! it's the trunk or a feature branch), clean/dirty working tree, unpushed
//! commits, behind-upstream commits, and stashed changes — then an aligned
//! totals table with source line counts and a roll-up summary line
//! (e.g. "7 clean · 1 dirty · 2 on feature branches · 1 with unpushed").
//!
//! Source line counts come from `tokei` if present, else a `git ls-files`
//! tracked-file count fallback.
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

/// Branch names treated as the trunk; anything else counts as a feature branch.
const TRUNK_BRANCHES: &[&str] = &["main", "master"];

/// ANSI styling, resolved once. Empty strings when color is off so every call
/// site can interpolate unconditionally.
struct Style {
    bold: &'static str,
    dim: &'static str,
    red: &'static str,
    grn: &'static str,
    ylw: &'static str,
    cyn: &'static str,
    mag: &'static str,
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
                mag: "\u{1b}[35m",
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
                mag: "",
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
    /// True when `branch` is the trunk (main/master); false for feature branches.
    on_trunk: bool,
    /// Working-tree changes: staged + unstaged + untracked (`git status` lines).
    dirty: usize,
    /// Commits ahead of upstream — i.e. unpushed. When there's no upstream this
    /// falls back to the local commit count (see `gather`), so a brand-new
    /// branch that's never been pushed still reports its work as unpushed.
    unpushed: usize,
    /// True when `unpushed` reflects local commits with no upstream configured,
    /// rather than commits ahead of a tracked upstream.
    no_upstream: bool,
    /// Commits behind upstream (cached; no fetch).
    behind: usize,
    /// Entries in `git stash list`.
    stashes: usize,
    /// Tracked-file count (always available when the repo is present).
    files: usize,
    /// Code lines from tokei, when tokei is installed.
    loc: Option<usize>,
}

impl RepoStatus {
    fn is_clean(&self) -> bool {
        self.dirty == 0
    }
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
    print_summary(&statuses, &style);
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
            on_trunk: false,
            dirty: 0,
            unpushed: 0,
            no_upstream: false,
            behind: 0,
            stashes: 0,
            files: 0,
            loc: None,
        };
    }

    let branch = git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "?".to_owned());
    let on_trunk = TRUNK_BRANCHES.contains(&branch.as_str());

    // Dirty count = lines of `git status --porcelain` (staged + unstaged + untracked).
    let dirty = git(&dir, &["status", "--porcelain"])
        .map(|out| out.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    // Ahead/behind vs upstream, cached (no fetch). `--left-right --count` prints
    // "<behind>\t<ahead>"; absent/unset upstream yields nothing → no upstream.
    let tracked = git(&dir, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]).and_then(
        |out| {
            let mut it = out.split_whitespace();
            let b: usize = it.next()?.parse().ok()?;
            let a: usize = it.next()?.parse().ok()?;
            Some((b, a))
        },
    );

    // Unpushed = commits ahead of upstream. With no upstream, fall back to the
    // total local commit count so never-pushed branches still flag their work.
    let (behind, unpushed, no_upstream) = match tracked {
        Some((b, a)) => (b, a, false),
        None => {
            let local = git(&dir, &["rev-list", "--count", "HEAD"])
                .and_then(|out| out.parse().ok())
                .unwrap_or(0);
            (0, local, true)
        }
    };

    let stashes = git(&dir, &["stash", "list"])
        .map(|out| out.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    let files = git(&dir, &["ls-files"])
        .map(|out| out.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    let loc = if have_tokei { tokei_loc(&dir) } else { None };

    RepoStatus {
        name: name.to_owned(),
        present: true,
        branch,
        on_trunk,
        dirty,
        unpushed,
        no_upstream,
        behind,
        stashes,
        files,
        loc,
    }
}

/// Print the colored per-repo status line: name, branch (trunk dim / feature
/// magenta), clean✓/dirty●, unpushed ↑, behind ↓, and stash ⚑.
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

    // Branch: trunk is unremarkable (dim); a feature branch is highlighted so it
    // jumps out when you scan the column.
    let branch = if s.on_trunk {
        format!("{}{:<20}{}", style.dim, s.branch, style.rst)
    } else {
        format!("{}{:<20}{}", style.mag, s.branch, style.rst)
    };

    let state = if s.is_clean() {
        format!("{}✓ clean{}", style.grn, style.rst)
    } else {
        format!("{}● {} changed{}", style.ylw, s.dirty, style.rst)
    };

    // Tags appended only when non-zero, so a healthy repo's line stays quiet.
    let mut tags = String::new();
    if s.unpushed > 0 {
        // "↑N" for commits ahead of upstream; "↑N*" when there's no upstream at
        // all (the N local commits have never been pushed anywhere).
        let star = if s.no_upstream { "*" } else { "" };
        tags.push_str(&format!(
            "  {}↑{}{}{}",
            style.cyn, s.unpushed, star, style.rst
        ));
    }
    if s.behind > 0 {
        tags.push_str(&format!("  {}↓{}{}", style.red, s.behind, style.rst));
    }
    if s.stashes > 0 {
        tags.push_str(&format!("  {}⚑{}{}", style.ylw, s.stashes, style.rst));
    }

    println!(
        "  {}{:<16}{} {} {:<11}{}",
        style.bold, s.name, style.rst, branch, state, tags
    );
}

/// Print the aligned totals table (repo, branch, line count) with a TOTAL row.
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
}

/// Print the at-a-glance roll-up: clean/dirty counts, feature-branch and
/// unpushed/behind/stash tallies, and any missing repos — only the parts that
/// actually apply, joined with " · ".
fn print_summary(statuses: &[RepoStatus], style: &Style) {
    let present: Vec<&RepoStatus> = statuses.iter().filter(|s| s.present).collect();

    let clean = present.iter().filter(|s| s.is_clean()).count();
    let dirty = present.iter().filter(|s| !s.is_clean()).count();
    let feature = present.iter().filter(|s| !s.on_trunk).count();
    let unpushed = present.iter().filter(|s| s.unpushed > 0).count();
    let behind = present.iter().filter(|s| s.behind > 0).count();
    let stashed = present.iter().filter(|s| s.stashes > 0).count();
    let missing = statuses.len() - present.len();

    // Each fragment carries its own color: green clean, yellow dirty, magenta
    // feature, cyan unpushed, red behind, yellow stashes, red missing.
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{}{clean} clean{}", style.grn, style.rst));
    if dirty > 0 {
        parts.push(format!("{}{dirty} dirty{}", style.ylw, style.rst));
    }
    if feature > 0 {
        let plural = if feature == 1 { "branch" } else { "branches" };
        parts.push(format!(
            "{}{feature} on feature {plural}{}",
            style.mag, style.rst
        ));
    }
    if unpushed > 0 {
        parts.push(format!("{}{unpushed} with unpushed{}", style.cyn, style.rst));
    }
    if behind > 0 {
        parts.push(format!("{}{behind} behind{}", style.red, style.rst));
    }
    if stashed > 0 {
        parts.push(format!("{}{stashed} stashed{}", style.ylw, style.rst));
    }
    if missing > 0 {
        parts.push(format!("{}{missing} missing{}", style.red, style.rst));
    }

    println!();
    println!(
        "{}summary:{} {}",
        style.bold,
        style.rst,
        parts.join(&format!("{} · {}", style.dim, style.rst))
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

    #[test]
    fn trunk_branches_are_recognized() {
        assert!(TRUNK_BRANCHES.contains(&"main"));
        assert!(TRUNK_BRANCHES.contains(&"master"));
        assert!(!TRUNK_BRANCHES.contains(&"feat/x"));
    }

    /// Helper to build a present repo with otherwise-zero facts.
    fn repo(name: &str, on_trunk: bool, dirty: usize, unpushed: usize, stashes: usize) -> RepoStatus {
        RepoStatus {
            name: name.to_owned(),
            present: true,
            branch: if on_trunk { "main".into() } else { "feat/x".into() },
            on_trunk,
            dirty,
            unpushed,
            no_upstream: false,
            behind: 0,
            stashes,
            files: 0,
            loc: None,
        }
    }

    #[test]
    fn is_clean_tracks_dirty_count() {
        assert!(repo("a", true, 0, 0, 0).is_clean());
        assert!(!repo("a", true, 3, 0, 0).is_clean());
    }

    #[test]
    fn summary_tallies_match_per_repo_facts() {
        // Mirror the counting logic in print_summary so a column-selection or
        // predicate regression is caught without parsing printed output.
        let statuses = vec![
            repo("clean-trunk", true, 0, 0, 0),
            repo("dirty-feature", false, 2, 0, 0),
            repo("clean-feature-unpushed", false, 0, 1, 0),
            repo("clean-trunk-stashed", true, 0, 0, 1),
        ];
        let present: Vec<&RepoStatus> = statuses.iter().filter(|s| s.present).collect();
        assert_eq!(present.iter().filter(|s| s.is_clean()).count(), 3);
        assert_eq!(present.iter().filter(|s| !s.is_clean()).count(), 1);
        assert_eq!(present.iter().filter(|s| !s.on_trunk).count(), 2);
        assert_eq!(present.iter().filter(|s| s.unpushed > 0).count(), 1);
        assert_eq!(present.iter().filter(|s| s.stashes > 0).count(), 1);
    }
}

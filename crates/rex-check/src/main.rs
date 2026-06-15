//! rex-check — at-a-glance health of the Linux Ops Suite repos.
//!
//! For each suite repo it prints a one-line git status summary (branch,
//! ahead/behind, dirty/clean) and a source-size metric, then a totals table.
//!
//! Metric is lines-of-code via `tokei` when installed, otherwise a tracked
//! file count from `git ls-files`. Fast by design: one `git` invocation per
//! fact, no network (cached upstream tracking info), tokei run once per repo.
//! Works from any directory — repo paths are absolute.
//!
//! Faithful Rust port of the original ~/bin/rex-check bash script. Honors the
//! same `REX_ROOT` and `NO_COLOR` environment variables; adds a `--no-color`
//! flag for explicit control.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;

/// The suite repos, in display order.
const REPOS: &[&str] = &[
    "bulwark",
    "scriptvault",
    "toolfoundry",
    "workstate",
    "proto",
    "rexops",
    "linux-ops-suite",
];

#[derive(Parser)]
#[command(
    name = "rex-check",
    about = "At-a-glance git health and source-size dashboard for the Linux Ops Suite repos."
)]
struct Cli {
    /// Disable colored output (also honored via the NO_COLOR env var).
    #[arg(long)]
    no_color: bool,
}

/// ANSI escape codes, blanked out when color is disabled.
struct Palette {
    bold: &'static str,
    dim: &'static str,
    red: &'static str,
    grn: &'static str,
    ylw: &'static str,
    cyn: &'static str,
    rst: &'static str,
}

impl Palette {
    fn colored() -> Self {
        Palette {
            bold: "\x1b[1m",
            dim: "\x1b[2m",
            red: "\x1b[31m",
            grn: "\x1b[32m",
            ylw: "\x1b[33m",
            cyn: "\x1b[36m",
            rst: "\x1b[0m",
        }
    }

    fn plain() -> Self {
        Palette {
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

/// One repo's computed row, kept so we can print the aligned table at the end.
struct Row {
    name: String,
    branch: String,
    count_disp: String,
}

fn main() {
    let cli = Cli::parse();

    // Color is on only when stdout is a TTY, NO_COLOR is unset, and --no-color
    // was not passed. Mirrors the bash `[[ -t 1 && -z "$NO_COLOR" ]]` test.
    let use_color = is_tty_stdout() && env::var_os("NO_COLOR").is_none() && !cli.no_color;
    let c = if use_color {
        Palette::colored()
    } else {
        Palette::plain()
    };

    // Override the search root with REX_ROOT=/path rex-check.
    let root: PathBuf = match env::var_os("REX_ROOT") {
        Some(r) => PathBuf::from(r),
        None => home_dir().join("projects"),
    };

    let have_tokei = command_exists("tokei");

    let mut total_loc: u64 = 0; // sum of code lines (tokei) — only when present
    let mut total_files: u64 = 0; // sum of tracked files (always available)
    let mut total_dirty: u64 = 0; // repos with uncommitted changes
    let mut found: u64 = 0;
    let mut missing: u64 = 0;
    let mut rows: Vec<Row> = Vec::new();

    println!(
        "{}{}rex-check{} {}— suite repos under {}{}",
        c.bold,
        c.cyn,
        c.rst,
        c.dim,
        root.display(),
        c.rst
    );
    println!();

    for name in REPOS {
        let dir = root.join(name);

        if !dir.join(".git").is_dir() {
            missing += 1;
            rows.push(Row {
                name: name.to_string(),
                branch: "—".to_string(),
                count_disp: "—".to_string(),
            });
            println!(
                "  {}{:<16}{} {}missing ({}){}",
                c.bold,
                name,
                c.rst,
                c.red,
                dir.display(),
                c.rst
            );
            continue;
        }
        found += 1;

        let branch = git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "?".into());

        // Dirty/clean from porcelain (staged + unstaged + untracked).
        let dirty_n = git(&dir, &["status", "--porcelain"])
            .map(|out| out.lines().filter(|l| !l.is_empty()).count())
            .unwrap_or(0);
        let state = if dirty_n > 0 {
            total_dirty += 1;
            format!("{}● {} changed{}", c.ylw, dirty_n, c.rst)
        } else {
            format!("{}✓ clean{}", c.grn, c.rst)
        };

        // Ahead/behind upstream (cached; no fetch). Empty when no upstream set.
        let mut ahead_behind = String::new();
        if let Some(ab) = git(&dir, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]) {
            let mut parts = ab.split_whitespace();
            let behind: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let ahead: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            if ahead > 0 {
                ahead_behind.push_str(&format!(" {}↑{}{}", c.cyn, ahead, c.rst));
            }
            if behind > 0 {
                ahead_behind.push_str(&format!(" {}↓{}{}", c.red, behind, c.rst));
            }
        }

        let files = git_files(&dir);
        total_files += files;

        let count_disp = if have_tokei {
            let loc = tokei_loc(&dir);
            total_loc += loc;
            loc.to_string()
        } else {
            files.to_string()
        };

        rows.push(Row {
            name: name.to_string(),
            branch: branch.clone(),
            count_disp,
        });

        println!(
            "  {}{:<16}{} {}{:<18}{} {}{}",
            c.bold, name, c.rst, c.dim, branch, c.rst, state, ahead_behind
        );
    }

    // --- totals table -----------------------------------------------------
    println!();
    let (metric, total_metric) = if have_tokei {
        ("lines (code)", total_loc)
    } else {
        println!(
            "{}(tokei not installed — showing tracked file counts; `cargo install tokei` for LOC){}",
            c.dim, c.rst
        );
        ("files", total_files)
    };

    let rule: String = "─".repeat(52);

    println!(
        "{}{:<16}  {:<20}  {:>12}{}",
        c.bold, "REPO", "BRANCH", metric, c.rst
    );
    println!("{}{}{}", c.dim, rule, c.rst);
    for r in &rows {
        println!("{:<16}  {:<20}  {:>12}", r.name, r.branch, r.count_disp);
    }
    println!("{}{}{}", c.dim, rule, c.rst);
    println!(
        "{}{:<16}  {:<20}  {:>12}{}",
        c.bold,
        format!("TOTAL ({} repos)", found),
        "",
        total_metric,
        c.rst
    );

    println!();
    println!(
        "{}repos:{} {} found, {} missing   {}dirty:{} {}   {}metric:{} {}",
        c.dim, c.rst, found, missing, c.dim, c.rst, total_dirty, c.dim, c.rst, metric
    );
}

/// Run `git -C <dir> <args>`, returning trimmed stdout on success.
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
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Count tracked source files via git (fallback metric, always works).
fn git_files(dir: &Path) -> u64 {
    git(dir, &["ls-files"])
        .map(|out| out.lines().filter(|l| !l.is_empty()).count() as u64)
        .unwrap_or(0)
}

/// Code-line count via tokei. Parses the plain-output "Total" row, whose
/// columns are: Language Files Lines Code Comments Blanks — so the 4th numeric
/// field (Code) is the total. Thousands separators are stripped.
fn tokei_loc(dir: &Path) -> u64 {
    let out = match Command::new("tokei").arg(dir).output() {
        Ok(o) if o.status.success() => o,
        _ => return 0,
    };
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.trim_start().starts_with("Total") {
            // Fields after the "Total" label: Files Lines Code Comments Blanks.
            let nums: Vec<&str> = line.split_whitespace().skip(1).collect();
            if let Some(code) = nums.get(2) {
                let cleaned: String = code.chars().filter(|ch| *ch != ',').collect();
                return cleaned.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// True if `name` is found on PATH.
fn command_exists(name: &str) -> bool {
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            if dir.join(name).is_file() {
                return true;
            }
        }
    }
    false
}

/// Best-effort home directory without pulling in an external crate.
fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// True when stdout is a terminal. Uses the libc isatty via a tiny unsafe call
/// to avoid an extra dependency.
fn is_tty_stdout() -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    // SAFETY: isatty just inspects fd 1 (stdout) and has no memory effects.
    unsafe { isatty(1) == 1 }
}

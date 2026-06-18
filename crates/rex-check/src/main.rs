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
//! Beyond the read-only dashboard it also performs two housekeeping passes:
//!   1. For any repo with uncommitted changes it prints that repo's full
//!      `git status` so the dirty count is never a mystery.
//!   2. It audits every repo for a `.claude/` folder (Claude Code's local
//!      worktree/agent state) and, after a clear warning + an explicit y/N
//!      prompt, ignores + untracks + deletes them. Deletion NEVER happens
//!      without a typed "yes" at an interactive terminal — a piped/non-TTY run
//!      only reports, it never removes anything.
//!
//! Environment:
//!   REX_ROOT   override the directory the suite repos live under
//!              (default: `$HOME/projects`).
//!   NO_COLOR   disable ANSI color (also auto-disabled when stdout isn't a TTY).

use std::env;
use std::io::Write;
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
    /// Absolute path to the repo, kept so the post-passes (git status, .claude
    /// cleanup) act on the same directory the facts were gathered from.
    dir: PathBuf,
    present: bool,
    branch: String,
    dirty: usize,
    ahead: usize,
    behind: usize,
    /// Tracked-file count (always available when the repo is present).
    files: usize,
    /// Code lines from tokei, when tokei is installed.
    loc: Option<usize>,
    /// Whether a `.claude/` directory exists at the repo root. Audited every run.
    has_claude: bool,
    /// Number of `.claude/` paths git is tracking (0 = ignored/untracked). Drives
    /// whether the `git rm --cached` step has anything to do.
    claude_tracked: usize,
    /// Whether `.claude/` is already covered by the repo's `.gitignore`.
    claude_ignored: bool,
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

    // Pass 1: audit + (after an explicit y/N) clean any `.claude/` folders.
    audit_and_clean_claude(&statuses, &style);

    // Pass 2 (final): recap the repos that are still dirty AFTER cleanup — so
    // status is the last thing on screen regardless of the .claude answer —
    // then, at a terminal, walk them one at a time to commit each with its own
    // message. The concise per-repo `git status` lives inside this pass, shown
    // once right where you act on it (no separate verbose status block above).
    offer_commit_dirty(&statuses, &style);

    ExitCode::SUCCESS
}

/// Print a clean, full-width section banner: a heavy rule, the title, a rule.
/// Used to separate the major phases (cleanup, commit) so the output reads as
/// distinct blocks instead of one undifferentiated wall.
fn section(title: &str, style: &Style) {
    let rule: String = "━".repeat(60);
    println!();
    println!("{}{}{}{}", style.bold, style.cyn, rule, style.rst);
    println!("{}{}{}{}", style.bold, style.cyn, title, style.rst);
    println!("{}{}{}{}", style.dim, style.cyn, rule, style.rst);
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
            dir,
            present: false,
            branch: "—".to_owned(),
            dirty: 0,
            ahead: 0,
            behind: 0,
            files: 0,
            loc: None,
            has_claude: false,
            claude_tracked: 0,
            claude_ignored: false,
        };
    }

    let branch =
        git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "?".to_owned());

    // Dirty count = lines of `git status --porcelain` (staged + unstaged + untracked).
    let dirty = git(&dir, &["status", "--porcelain"])
        .map(|out| out.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    // Ahead/behind vs upstream, cached (no fetch). `--left-right --count` prints
    // "<behind>\t<ahead>"; absent/unset upstream yields nothing → (0, 0).
    let (behind, ahead) = git(
        &dir,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    )
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

    // .claude/ audit facts, gathered up front so the cleanup pass and the
    // warning render from the same snapshot.
    let has_claude = dir.join(".claude").is_dir();
    let claude_tracked = if has_claude {
        git(&dir, &["ls-files", "--", ".claude"])
            .map(|out| out.lines().filter(|l| !l.is_empty()).count())
            .unwrap_or(0)
    } else {
        0
    };
    let claude_ignored = has_claude && gitignore_has_claude(&dir);

    RepoStatus {
        name: name.to_owned(),
        dir,
        present: true,
        branch,
        dirty,
        ahead,
        behind,
        files,
        loc,
        has_claude,
        claude_tracked,
        claude_ignored,
    }
}

/// The exact line rex-check manages in a `.gitignore` to ignore Claude's local
/// state. A trailing slash makes it directory-only, matching the folder we clean.
const CLAUDE_IGNORE_LINE: &str = ".claude/";

/// Whether `<dir>/.gitignore` already ignores `.claude/`. Accepts the common
/// equivalent spellings so we never append a duplicate that only differs by a
/// slash or a leading `/`: `.claude/`, `.claude`, `/.claude/`, `/.claude`.
fn gitignore_has_claude(dir: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(dir.join(".gitignore")) else {
        return false;
    };
    contents.lines().any(|line| {
        let t = line.trim();
        matches!(t, ".claude/" | ".claude" | "/.claude/" | "/.claude")
    })
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
    let mut dirty_names: Vec<&str> = Vec::new();
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
                dirty_names.push(&s.name);
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

    // Name the dirty repos inline so the summary is actionable at a glance;
    // bare "0" when the whole suite is clean.
    let dirty_disp = if dirty_names.is_empty() {
        "0".to_owned()
    } else {
        format!("{} ({})", dirty_names.len(), dirty_names.join(", "))
    };

    println!();
    println!(
        "{}repos:{} {} found, {} missing   {}dirty:{} {}   {}metric:{} {}",
        style.dim,
        style.rst,
        found,
        missing,
        style.dim,
        style.rst,
        dirty_disp,
        style.dim,
        style.rst,
        metric
    );
}

/// Pass 2 — audit every repo for a `.claude/` folder. If any exist, print a
/// warning that lists each with its size, then prompt ONCE for a typed yes
/// before ignoring + untracking + deleting them. Deletion never happens without
/// an interactive "yes": a non-TTY (piped) run reports and skips.
fn audit_and_clean_claude(statuses: &[RepoStatus], style: &Style) {
    let found: Vec<&RepoStatus> = statuses.iter().filter(|s| s.has_claude).collect();
    if found.is_empty() {
        return;
    }

    section(
        &format!("⚠  .claude/ folders — {} repo(s)", found.len()),
        style,
    );
    println!(
        "{}  Claude Code's local worktree/agent state; not part of the suite.{}",
        style.dim, style.rst
    );
    println!();
    for s in &found {
        let size = dir_size_human(&s.dir.join(".claude"));
        let tracked = if s.claude_tracked > 0 {
            format!(
                "{}tracked: {} path(s){}",
                style.red, s.claude_tracked, style.rst
            )
        } else {
            format!("{}untracked{}", style.dim, style.rst)
        };
        let ignored = if s.claude_ignored {
            format!("{}gitignored{}", style.dim, style.rst)
        } else {
            format!("{}not in .gitignore{}", style.ylw, style.rst)
        };
        println!(
            "  {}{:<16}{} {:>8}   {}   {}",
            style.bold, s.name, style.rst, size, tracked, ignored
        );
    }

    // Confirmation gate. A piped / non-interactive run must never delete.
    if !stdin_is_tty() {
        println!();
        println!(
            "{}  non-interactive (stdin is not a terminal) — left untouched.{}",
            style.dim, style.rst
        );
        println!(
            "{}  run rex-check in a terminal to clean these up.{}",
            style.dim, style.rst
        );
        return;
    }

    println!();
    let prompt = format!(
        "{}Delete these {} .claude/ folder(s)? This cannot be undone. [y/N] {}",
        style.bold,
        found.len(),
        style.rst
    );
    match prompt_yes_no(&prompt) {
        Some(true) => {}
        Some(false) => {
            println!("{}  left untouched.{}", style.dim, style.rst);
            return;
        }
        None => {
            println!(
                "{}  could not read a response — left untouched.{}",
                style.dim, style.rst
            );
            return;
        }
    }

    // Confirmed: clean each repo. Best-effort and independent — one repo's
    // failure is reported and never aborts the rest.
    println!();
    println!("{}{}Cleaned up:{}", style.bold, style.grn, style.rst);
    let mut freed = 0u64;
    for s in &found {
        clean_one_claude(s, style, &mut freed);
    }
    println!();
    println!(
        "{}total freed:{} {}",
        style.dim,
        style.rst,
        human_size(freed)
    );
}

/// Ignore + untrack + delete one repo's `.claude/`, reporting each sub-step.
/// Adds to `freed` the size reclaimed by the delete. Every step is independent
/// and best-effort so a single failure never aborts the sweep.
fn clean_one_claude(s: &RepoStatus, style: &Style, freed: &mut u64) {
    println!("  {}{}{}", style.bold, s.name, style.rst);

    // 1. Ensure `.claude/` is in .gitignore (append if missing; create if none).
    if s.claude_ignored {
        println!("    .gitignore   already ignores .claude/");
    } else {
        match append_gitignore_claude(&s.dir) {
            Ok(()) => println!("    .gitignore   {}added .claude/{}", style.grn, style.rst),
            Err(e) => println!("    .gitignore   {}failed: {e}{}", style.red, style.rst),
        }
    }

    // 2. Untrack from git's index if anything was tracked (else a clean no-op).
    if s.claude_tracked > 0 {
        match git_output(&s.dir, &["rm", "-r", "--cached", "--quiet", ".claude"]) {
            Some(_) => println!(
                "    git index    {}removed {} tracked path(s) from index{}",
                style.grn, s.claude_tracked, style.rst
            ),
            None => println!(
                "    git index    {}git rm --cached failed{}",
                style.red, style.rst
            ),
        }
    } else {
        println!("    git index    not tracked — nothing to untrack");
    }

    // 3. Delete the folder. Measure size first so we can report what was freed.
    let path = s.dir.join(".claude");
    let size = dir_size_bytes(&path);
    match std::fs::remove_dir_all(&path) {
        Ok(()) => {
            *freed += size;
            println!(
                "    folder       {}deleted ({} freed){}",
                style.grn,
                human_size(size),
                style.rst
            );
        }
        Err(e) => println!(
            "    folder       {}delete failed: {e}{}",
            style.red, style.rst
        ),
    }
}

/// Pass 3 — the final recap. ALWAYS re-lists the repos that are dirty *now*
/// (re-queried live, because the .claude cleanup may have changed each repo's
/// working tree — e.g. untracking a file or adding `.gitignore`), so the status
/// is the last thing on screen no matter how the .claude prompt was answered.
/// Then, at an interactive terminal, walks the dirty repos ONE AT A TIME: each
/// gets its own status shown and its own commit message, so a single suite-wide
/// message is never forced across unrelated repos.
fn offer_commit_dirty(statuses: &[RepoStatus], style: &Style) {
    // Re-query dirtiness live rather than trust the pre-cleanup snapshot.
    let dirty: Vec<&RepoStatus> = statuses
        .iter()
        .filter(|s| s.present && repo_is_dirty(&s.dir))
        .collect();

    if dirty.is_empty() {
        section("✓  All repos clean", style);
        println!("{}  Nothing to commit.{}", style.dim, style.rst);
        return;
    }

    section(
        &format!("✎  Uncommitted changes — {} repo(s)", dirty.len()),
        style,
    );

    // No human to answer when piped: name the dirty repos + a hint, then stop.
    // Never commit without an interactive yes.
    if !stdin_is_tty() {
        for s in &dirty {
            println!("  {}{}{}", style.bold, s.name, style.rst);
        }
        println!();
        println!(
            "{}  Run rex-check in a terminal to commit these.{}",
            style.dim, style.rst
        );
        return;
    }

    println!(
        "{}  Going through each one at a time — blank message skips a repo.{}",
        style.dim, style.rst
    );

    // Walk each dirty repo independently: show it, show its status, ask for a
    // message JUST for it, commit only it, then move on. A blank message skips
    // that one repo (leaving it dirty) rather than aborting the whole loop —
    // best-effort, so one repo never blocks the rest.
    let mut committed = 0usize;
    let mut skipped = 0usize;
    for (i, s) in dirty.iter().enumerate() {
        // Per-repo header: counter + name on one line, path dimmed beneath, then
        // the concise status. This is the ONLY place status is shown — there is
        // no separate verbose status block, so nothing is repeated.
        println!();
        println!(
            "{}{}[{}/{}] {}{}",
            style.bold,
            style.cyn,
            i + 1,
            dirty.len(),
            s.name,
            style.rst
        );
        println!("{}      {}{}", style.dim, s.dir.display(), style.rst);
        match git_output(&s.dir, &["status", "--short"]) {
            Some(text) => {
                for line in text.lines() {
                    println!("    {}{}{}", style.dim, line, style.rst);
                }
            }
            None => println!("    {}(could not read git status){}", style.red, style.rst),
        }

        // The prompt stands alone on its own line, set off by a marker, so it
        // never blends into the status lines above it.
        let message = match prompt_line(&format!(
            "  {}→ commit message{} {}(blank skips):{} ",
            style.bold, style.rst, style.dim, style.rst
        )) {
            Some(m) if !m.trim().is_empty() => m.trim().to_owned(),
            _ => {
                println!("    {}↳ skipped{}", style.dim, style.rst);
                skipped += 1;
                continue;
            }
        };

        if commit_one(s, &message, style) {
            committed += 1;
        } else {
            skipped += 1;
        }
    }

    println!();
    println!(
        "{}{}Summary:{} {}{} committed{}, {}{} skipped{} (of {} dirty)",
        style.bold,
        style.grn,
        style.rst,
        style.grn,
        committed,
        style.rst,
        style.dim,
        skipped,
        style.rst,
        dirty.len()
    );
}

/// `git add -A && git commit -m <message>` for one repo, reporting the result.
/// Returns true on a successful commit. Best-effort: a failed `add` or `commit`
/// is reported and returns false without aborting the caller's sweep.
fn commit_one(s: &RepoStatus, message: &str, style: &Style) -> bool {
    if git_output(&s.dir, &["add", "-A"]).is_none() {
        println!("    {}↳ git add failed{}", style.red, style.rst);
        return false;
    }
    match git_output(&s.dir, &["commit", "-m", message]) {
        Some(_) => {
            // Short hash for a tidy confirmation; absence is non-fatal.
            let hash = git(&s.dir, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
            println!(
                "    {}↳ committed{} {}{}{}",
                style.grn, style.rst, style.dim, hash, style.rst
            );
            true
        }
        None => {
            println!(
                "    {}↳ commit failed (nothing to commit?){}",
                style.red, style.rst
            );
            false
        }
    }
}

/// Whether a repo currently has any staged, unstaged, or untracked change —
/// queried live via `git status --porcelain` (a non-empty body = dirty).
fn repo_is_dirty(dir: &Path) -> bool {
    git_output(dir, &["status", "--porcelain"])
        .map(|out| out.lines().any(|l| !l.is_empty()))
        .unwrap_or(false)
}

/// Print `prompt`, read one line from stdin, and interpret a yes/no answer:
/// `Some(true)` for y/yes, `Some(false)` for anything else, `None` if the read
/// itself failed. The default (bare Enter) is No. Shared by every confirm gate.
fn prompt_yes_no(prompt: &str) -> Option<bool> {
    let line = prompt_line(prompt)?;
    Some(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Print `prompt` (flushing so it shows before blocking) and return one line of
/// stdin with the trailing newline stripped, or `None` on read failure.
fn prompt_line(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return None;
    }
    // Strip the trailing newline(s) the user's Enter left.
    Some(line.trim_end_matches(['\n', '\r']).to_owned())
}

/// Append the managed `.claude/` line to `<dir>/.gitignore`, creating the file
/// if absent and adding a leading newline only when the existing file doesn't
/// already end with one (so we never glue it onto a previous entry).
fn append_gitignore_claude(dir: &Path) -> std::io::Result<()> {
    let path = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut out = existing.clone();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(CLAUDE_IGNORE_LINE);
    out.push('\n');
    std::fs::write(&path, out)
}

/// Total size of a directory tree in bytes, following no symlinks (uses
/// `symlink_metadata` so a symlink counts as its own tiny entry, never the
/// target). Best-effort: unreadable entries are skipped, returning what we could
/// sum. Used both to report freed space and to warn the user of the magnitude.
fn dir_size_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.file_type().is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
        } else {
            total += meta.len();
        }
    }
    total
}

/// `dir_size_bytes` rendered for humans (e.g. "4.0G").
fn dir_size_human(path: &Path) -> String {
    human_size(dir_size_bytes(path))
}

/// Format a byte count as a short human string (B/K/M/G/T), one decimal for
/// non-byte units. Powers of 1024, matching `du -h`.
fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}{}", UNITS[0])
    } else {
        format!("{size:.1}{}", UNITS[unit])
    }
}

/// Run `git -C <dir> <args...>` and return its stdout on success regardless of
/// whether it is empty (unlike [`git`], which treats an empty body as "no
/// data"). Used by the action passes where a successful empty result (e.g.
/// `git rm --cached`) is itself the signal of success.
fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
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
    is_tty(1)
}

/// Whether fd 0 (stdin) is a TTY. The destructive `.claude/` cleanup is gated on
/// this: a piped / redirected run has no human to confirm, so it must never
/// delete — it only reports.
fn stdin_is_tty() -> bool {
    is_tty(0)
}

/// Whether the given file descriptor is a TTY, via `isatty(3)`.
fn is_tty(fd: i32) -> bool {
    // SAFETY: isatty merely queries a file descriptor and has no preconditions.
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) == 1 }
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
        let sample =
            " Total                    87        11764         7821         2551         1392";
        let code: usize = sample
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
            assert!(
                REPOS.contains(&expected),
                "{expected} must be in the roster"
            );
        }
    }

    #[test]
    fn human_size_scales_units_and_keeps_bytes_whole() {
        // Bytes render without a decimal; larger units get one decimal place,
        // using powers of 1024 (du -h style).
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(1024 * 1024), "1.0M");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0G");
    }

    #[test]
    fn gitignore_has_claude_accepts_equivalent_spellings() {
        let dir = unique_tmp_dir("gi-has");
        std::fs::create_dir_all(&dir).unwrap();

        // No .gitignore at all → false.
        assert!(!gitignore_has_claude(&dir));

        // Each accepted spelling matches; an unrelated entry does not.
        for (body, expected) in [
            (".claude/\n", true),
            (".claude\n", true),
            ("/.claude/\n", true),
            ("/.claude\n", true),
            ("target/\nnode_modules/\n", false),
            ("# .claude/ in a comment\n", false),
        ] {
            std::fs::write(dir.join(".gitignore"), body).unwrap();
            assert_eq!(
                gitignore_has_claude(&dir),
                expected,
                "spelling {body:?} should match={expected}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_gitignore_claude_creates_and_appends_without_duplicating() {
        let dir = unique_tmp_dir("gi-append");
        std::fs::create_dir_all(&dir).unwrap();
        let gi = dir.join(".gitignore");

        // No file yet → it is created with exactly the managed line.
        append_gitignore_claude(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(&gi).unwrap(), ".claude/\n");
        assert!(gitignore_has_claude(&dir));

        // An existing file WITHOUT a trailing newline gets one inserted, so the
        // new entry never glues onto the previous line.
        std::fs::write(&gi, "target/").unwrap();
        append_gitignore_claude(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(&gi).unwrap(), "target/\n.claude/\n");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dir_size_bytes_sums_a_tree() {
        let dir = unique_tmp_dir("size");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a"), [0u8; 100]).unwrap();
        std::fs::write(dir.join("sub").join("b"), [0u8; 23]).unwrap();
        assert_eq!(dir_size_bytes(&dir), 123, "100 + 23 across the tree");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A unique temp dir under the OS temp root, namespaced by pid + a label so
    /// parallel tests never collide.
    fn unique_tmp_dir(label: &str) -> PathBuf {
        env::temp_dir().join(format!("rex-check-test-{label}-{}", std::process::id()))
    }
}

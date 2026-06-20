//! Rendering. Turns a scan or a diff into either a human table or the suite's
//! JSON envelope. Color follows the suite rule (TTY + `NO_COLOR`, force-off via
//! `--no-color`); the table is aligned by hand so it has no extra dependency and
//! reads the same with color stripped. The structure mirrors portman's report.rs.

use serde::Serialize;
use suite_core::fmt::human_size;

use crate::baseline::{Change, Diff, Field};
use crate::model::{Entry, EntryKind};
use crate::scan::Scan;
use crate::util;
use crate::watch::{WatchSet, WatchSource};

/// Resolved styling. Empty strings when color is off so call sites interpolate
/// unconditionally — same approach as portman/rex-doctor.
pub struct Style {
    pub bold: &'static str,
    pub dim: &'static str,
    pub red: &'static str,
    pub grn: &'static str,
    pub ylw: &'static str,
    pub cyn: &'static str,
    pub rst: &'static str,
}

impl Style {
    /// Color on only when stdout is a TTY and `NO_COLOR` is unset, unless the
    /// caller forced it off (`--no-color`).
    pub fn resolve(force_off: bool) -> Self {
        let on = !force_off && util::stdout_is_tty() && std::env::var_os("NO_COLOR").is_none();
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

// ---- Current view ---------------------------------------------------------

/// Print the current-view table for a scan.
pub fn print_scan(scan: &Scan, style: &Style, verbose: bool) {
    println!(
        "{}{}tripwire{} {}— what is being watched, and its state now{}",
        style.bold, style.cyn, style.rst, style.dim, style.rst
    );
    println!();

    if scan.entries.is_empty() {
        println!("{}No watched paths are present.{}", style.dim, style.rst);
        print_footer(scan, style);
        return;
    }

    print_table(&scan.entries, style, verbose);
    print_footer(scan, style);
}

/// The aligned entry table. `verbose` adds the hash-prefix, uid/gid and mtime
/// columns; the default view stays to the high-signal columns.
fn print_table(entries: &[Entry], style: &Style, verbose: bool) {
    let rows: Vec<Row> = entries.iter().map(Row::of).collect();

    let w_path = col_width(&rows, |r| &r.path, 4);
    let w_kind = col_width(&rows, |r| &r.kind, 4);
    let w_mode = 5; // "0644" + a little
    let w_size = col_width(&rows, |r| &r.size, 4);

    print!(
        "{}{:<wp$}  {:<wk$}  {:<wm$}  {:>ws$}  STATE",
        style.bold,
        "PATH",
        "KIND",
        "MODE",
        "SIZE",
        wp = w_path,
        wk = w_kind,
        wm = w_mode,
        ws = w_size,
    );
    if verbose {
        print!("  {:<12}  {:<8}  MTIME", "HASH", "OWNER");
    }
    println!("{}", style.rst);

    for (e, r) in entries.iter().zip(&rows) {
        let (state_txt, state_col) = state_cell(e, style);
        print!(
            "{:<wp$}  {}{:<wk$}{}  {:<wm$}  {:>ws$}  {}{}{}",
            r.path,
            style.dim,
            r.kind,
            style.rst,
            r.mode,
            r.size,
            state_col,
            state_txt,
            style.rst,
            wp = w_path,
            wk = w_kind,
            wm = w_mode,
            ws = w_size,
        );
        if verbose {
            print!(
                "  {}{:<12}  {:<8}  {}{}",
                style.dim, r.hash, r.owner, r.mtime, style.rst
            );
        }
        println!();
    }
}

/// A pre-rendered table row.
struct Row {
    path: String,
    kind: String,
    mode: String,
    size: String,
    hash: String,
    owner: String,
    mtime: String,
}

impl Row {
    fn of(e: &Entry) -> Self {
        Row {
            path: e.path.clone(),
            kind: e.kind.tag().to_string(),
            mode: e.mode.clone().unwrap_or_else(|| "—".into()),
            size: e.size.map(human_size).unwrap_or_else(|| "—".into()),
            hash: e
                .hash
                .as_deref()
                .map(|h| h.chars().take(10).collect::<String>() + "…")
                .unwrap_or_else(|| "—".into()),
            owner: e.owner_label(),
            mtime: e.mtime.clone().unwrap_or_else(|| "—".into()),
        }
    }
}

/// The STATE cell: `ok` for a hashed file, `unreadable`, the symlink target for
/// a symlink, `N entries`-style note left blank for dirs (they have no single
/// state), or a dash. Colored only when it carries a warning.
fn state_cell<'a>(e: &Entry, style: &'a Style) -> (String, &'a str) {
    if e.unreadable {
        return ("unreadable".to_string(), style.ylw);
    }
    match e.kind {
        EntryKind::File => ("ok".to_string(), style.grn),
        EntryKind::Symlink => (
            format!("→ {}", e.target.as_deref().unwrap_or("?")),
            style.dim,
        ),
        EntryKind::Dir => ("dir".to_string(), style.dim),
        EntryKind::Other => ("special".to_string(), style.dim),
    }
}

/// Width of a column = widest cell, floored at a minimum.
fn col_width(rows: &[Row], field: impl Fn(&Row) -> &String, min: usize) -> usize {
    rows.iter()
        .map(|r| field(r).len())
        .max()
        .unwrap_or(0)
        .max(min)
}

/// The footer line: counts + watch-set source, then the not-root hint.
fn print_footer(scan: &Scan, style: &Style) {
    let unreadable = scan.entries.iter().filter(|e| e.unreadable).count();
    println!();
    if unreadable > 0 {
        println!(
            "{}{} watched · {} unreadable · source: {}{}",
            style.dim,
            scan.entries.len(),
            unreadable,
            scan.source.tag(),
            style.rst
        );
    } else {
        println!(
            "{}{} watched · source: {}{}",
            style.dim,
            scan.entries.len(),
            scan.source.tag(),
            style.rst
        );
    }
    print_root_hint(scan, style);
}

/// One-line hint, shown only to non-root callers when something was unreadable,
/// that some files may be hidden. Root sees the full picture, so it gets no nag.
fn print_root_hint(scan: &Scan, style: &Style) {
    let any_unreadable = scan.entries.iter().any(|e| e.unreadable);
    if !util::is_root() && any_unreadable {
        println!(
            "{}(not root: some system files show as ‘unreadable’; re-run with sudo for full coverage){}",
            style.dim, style.rst
        );
    }
}

// ---- Watch set view (`tripwire watch`) ------------------------------------

/// Print the resolved watch set and where it came from. Lets the operator see
/// exactly what is and isn't covered before recording a baseline.
pub fn print_watch_set(set: &WatchSet, style: &Style) {
    println!(
        "{}{}tripwire watch{} {}— the watch set ({} source){}",
        style.bold,
        style.cyn,
        style.rst,
        style.dim,
        set.source.tag(),
        style.rst
    );
    println!();
    for w in &set.entries {
        let mut notes = Vec::new();
        if !w.recursive {
            notes.push("non-recursive".to_string());
        }
        if w.follow_symlinks {
            notes.push("follow-symlinks".to_string());
        }
        if !w.content {
            notes.push("metadata-only".to_string());
        }
        if !w.exclude.is_empty() {
            notes.push(format!("exclude {}", w.exclude.join(",")));
        }
        let suffix = if notes.is_empty() {
            String::new()
        } else {
            format!("  {}({}){}", style.dim, notes.join(", "), style.rst)
        };
        println!("  {}{}", w.path.display(), suffix);
    }
    println!();
    println!(
        "{}{} paths · source: {}{}",
        style.dim,
        set.entries.len(),
        set.source.tag(),
        style.rst
    );
    if set.source == WatchSource::Builtin {
        println!(
            "{}(built-in default set — create a watch.conf to customize){}",
            style.dim, style.rst
        );
    }
}

// ---- Diff view ------------------------------------------------------------

/// Print a diff against the baseline.
pub fn print_diff(diff: &Diff, style: &Style) {
    println!(
        "{}{}tripwire diff{} {}— changes since baseline{}",
        style.bold, style.cyn, style.rst, style.dim, style.rst
    );
    println!();

    if diff.is_clean() {
        println!(
            "{}No changes — the watch set matches the baseline.{}",
            style.grn, style.rst
        );
        return;
    }

    for change in &diff.changes {
        match change {
            Change::Added(e) => println!(
                "  {}+ {}{}  {}new {}{}  {}",
                style.grn,
                e.path,
                style.rst,
                style.dim,
                e.kind,
                style.rst,
                e.mode.as_deref().unwrap_or("")
            ),
            Change::Removed(e) => println!(
                "  {}- {}{}  {}removed{}",
                style.red, e.path, style.rst, style.dim, style.rst
            ),
            Change::Modified { was, now, fields } => {
                println!(
                    "  {}~ {}{}  {}",
                    style.ylw,
                    now.path,
                    style.rst,
                    fields_detail(was, now, fields, style)
                );
            }
        }
    }

    let (added, removed, modified) = diff.tally();
    println!();
    println!(
        "{}{} added · {} removed · {} modified{}",
        style.dim, added, removed, modified, style.rst
    );
}

/// Render the per-field detail for a modified entry, spelling out exactly what
/// drifted and tagging the security-relevant ones.
fn fields_detail(was: &Entry, now: &Entry, fields: &[Field], style: &Style) -> String {
    let mut parts = Vec::new();
    let mut security = false;
    for f in fields {
        if f.is_security() {
            security = true;
        }
        match f {
            Field::Content => {
                let a = hash_short(was.hash.as_deref());
                let b = hash_short(now.hash.as_deref());
                if was.kind == EntryKind::Symlink {
                    parts.push(format!(
                        "target {} → {}",
                        was.target.as_deref().unwrap_or("?"),
                        now.target.as_deref().unwrap_or("?")
                    ));
                } else {
                    parts.push(format!("content changed   hash {a} → {b}"));
                }
            }
            Field::Mode => parts.push(format!(
                "mode {} → {}",
                was.mode.as_deref().unwrap_or("?"),
                now.mode.as_deref().unwrap_or("?")
            )),
            Field::Owner => parts.push(format!(
                "owner {} → {}",
                was.owner_label(),
                now.owner_label()
            )),
            Field::Size => parts.push(format!(
                "size {} → {}",
                was.size.map(human_size).unwrap_or_else(|| "?".into()),
                now.size.map(human_size).unwrap_or_else(|| "?".into())
            )),
            Field::Type => parts.push(format!("type {} → {}", was.kind, now.kind)),
            Field::Readability => parts.push(if now.unreadable {
                "became unreadable".to_string()
            } else {
                "became readable".to_string()
            }),
        }
    }
    let mut line = parts.join("   ");
    if security {
        let tag = if fields.contains(&Field::Mode) && fields.contains(&Field::Owner) {
            "[PERM/OWNER]"
        } else if fields.contains(&Field::Mode) {
            "[PERM]"
        } else {
            "[OWNER]"
        };
        line.push_str(&format!("   {}{}{}", style.ylw, tag, style.rst));
    }
    line
}

/// A short hash prefix for diff lines.
fn hash_short(h: Option<&str>) -> String {
    match h {
        Some(s) => s.chars().take(4).collect::<String>() + "…",
        None => "—".to_string(),
    }
}

// ---- JSON envelopes -------------------------------------------------------

/// The current-view JSON envelope, same shape family as the rest of the suite.
#[derive(Serialize)]
struct ScanReport<'a> {
    schema_version: u32,
    source_tool: &'a str,
    watch_source: &'a str,
    count: usize,
    unreadable: usize,
    entries: &'a [Entry],
}

/// Serialize the current view as pretty JSON.
pub fn scan_json(scan: &Scan) -> String {
    let unreadable = scan.entries.iter().filter(|e| e.unreadable).count();
    let report = ScanReport {
        schema_version: 1,
        source_tool: "tripwire",
        watch_source: scan.source.tag(),
        count: scan.entries.len(),
        unreadable,
        entries: &scan.entries,
    };
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| {
        String::from("{\"schema_version\":1,\"source_tool\":\"tripwire\",\"entries\":[]}")
    })
}

/// The watch-set JSON envelope for `tripwire watch --json`.
#[derive(Serialize)]
struct WatchEntryOut<'a> {
    path: String,
    recursive: bool,
    follow_symlinks: bool,
    content: bool,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    exclude: &'a [String],
}

#[derive(Serialize)]
struct WatchReport<'a> {
    schema_version: u32,
    source_tool: &'a str,
    watch_source: &'a str,
    count: usize,
    watches: Vec<WatchEntryOut<'a>>,
}

/// Serialize the resolved watch set as pretty JSON.
pub fn watch_json(set: &WatchSet) -> String {
    let watches = set
        .entries
        .iter()
        .map(|w| WatchEntryOut {
            path: w.path.to_string_lossy().into_owned(),
            recursive: w.recursive,
            follow_symlinks: w.follow_symlinks,
            content: w.content,
            exclude: &w.exclude,
        })
        .collect();
    let report = WatchReport {
        schema_version: 1,
        source_tool: "tripwire",
        watch_source: set.source.tag(),
        count: set.entries.len(),
        watches,
    };
    serde_json::to_string_pretty(&report)
        .unwrap_or_else(|_| String::from("{\"schema_version\":1,\"source_tool\":\"tripwire\"}"))
}

/// JSON-friendly view of one change.
#[derive(Serialize)]
struct ChangeOut {
    kind: &'static str,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now_mode: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    security: bool,
}

#[derive(Serialize)]
struct DiffReport<'a> {
    schema_version: u32,
    source_tool: &'a str,
    clean: bool,
    added: usize,
    removed: usize,
    modified: usize,
    changes: Vec<ChangeOut>,
}

/// Serialize a diff as pretty JSON.
pub fn diff_json(diff: &Diff) -> String {
    let changes = diff
        .changes
        .iter()
        .map(|c| match c {
            Change::Added(e) => ChangeOut {
                kind: "added",
                path: e.path.clone(),
                entry_kind: Some(e.kind.tag()),
                fields: Vec::new(),
                was_hash: None,
                now_hash: None,
                was_mode: None,
                now_mode: None,
                security: false,
            },
            Change::Removed(e) => ChangeOut {
                kind: "removed",
                path: e.path.clone(),
                entry_kind: Some(e.kind.tag()),
                fields: Vec::new(),
                was_hash: None,
                now_hash: None,
                was_mode: None,
                now_mode: None,
                security: false,
            },
            Change::Modified { was, now, fields } => {
                let security = fields.iter().any(|f| f.is_security());
                let content = fields.contains(&Field::Content);
                let mode = fields.contains(&Field::Mode);
                ChangeOut {
                    kind: "modified",
                    path: now.path.clone(),
                    entry_kind: None,
                    fields: fields.iter().map(field_tag).collect(),
                    was_hash: if content { was.hash.clone() } else { None },
                    now_hash: if content { now.hash.clone() } else { None },
                    was_mode: if mode { was.mode.clone() } else { None },
                    now_mode: if mode { now.mode.clone() } else { None },
                    security,
                }
            }
        })
        .collect();

    let (added, removed, modified) = diff.tally();
    let report = DiffReport {
        schema_version: 1,
        source_tool: "tripwire",
        clean: diff.is_clean(),
        added,
        removed,
        modified,
        changes,
    };
    serde_json::to_string_pretty(&report)
        .unwrap_or_else(|_| String::from("{\"schema_version\":1,\"source_tool\":\"tripwire\"}"))
}

/// Stable snake_case tag for a changed field, for the JSON `fields` array.
fn field_tag(f: &Field) -> String {
    match f {
        Field::Content => "content",
        Field::Mode => "mode",
        Field::Owner => "owner",
        Field::Size => "size",
        Field::Type => "type",
        Field::Readability => "readability",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::Diff;
    use crate::model::EntryKind;
    use crate::scan::Scan;

    fn entry(path: &str, hash: Option<&str>) -> Entry {
        Entry {
            path: path.into(),
            kind: EntryKind::File,
            size: Some(10),
            mode: Some("0644".into()),
            uid: Some(0),
            gid: Some(0),
            mtime: Some("2026-01-01T00:00:00Z".into()),
            hash: hash.map(|h| h.into()),
            target: None,
            unreadable: hash.is_none(),
        }
    }

    #[test]
    fn scan_json_has_stable_envelope_and_counts() {
        let scan = Scan {
            entries: vec![entry("/a", Some("aa")), entry("/b", None)],
            source: WatchSource::Builtin,
        };
        let v: serde_json::Value = serde_json::from_str(&scan_json(&scan)).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "tripwire");
        assert_eq!(v["watch_source"], "builtin");
        assert_eq!(v["count"], 2);
        assert_eq!(v["unreadable"], 1);
        assert_eq!(v["entries"].as_array().map(|a| a.len()), Some(2));
    }

    #[test]
    fn diff_json_encodes_each_change_kind_and_security() {
        let mut now_mode = entry("/etc/passwd", Some("aa"));
        now_mode.mode = Some("0666".into());
        let diff = Diff {
            changes: vec![
                Change::Added(entry("/new", Some("nn"))),
                Change::Removed(entry("/gone", Some("gg"))),
                Change::Modified {
                    was: Box::new(entry("/etc/passwd", Some("aa"))),
                    now: Box::new(now_mode),
                    fields: vec![Field::Mode],
                },
            ],
        };
        let v: serde_json::Value = serde_json::from_str(&diff_json(&diff)).unwrap();
        assert_eq!(v["clean"], false);
        assert_eq!(v["added"], 1);
        assert_eq!(v["removed"], 1);
        assert_eq!(v["modified"], 1);

        let changes = v["changes"].as_array().unwrap();
        let kinds: Vec<&str> = changes
            .iter()
            .map(|c| c["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"added"));
        assert!(kinds.contains(&"removed"));
        assert!(kinds.contains(&"modified"));

        let modified = changes.iter().find(|c| c["kind"] == "modified").unwrap();
        assert_eq!(modified["security"], true);
        assert_eq!(modified["was_mode"], "0644");
        assert_eq!(modified["now_mode"], "0666");
    }

    #[test]
    fn clean_diff_json_is_marked_clean() {
        let v: serde_json::Value = serde_json::from_str(&diff_json(&Diff::default())).unwrap();
        assert_eq!(v["clean"], true);
        assert_eq!(v["changes"].as_array().map(|a| a.len()), Some(0));
    }
}

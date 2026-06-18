//! Rendering. Turns a scan or a diff into either a human table or the suite's
//! JSON envelope. Color follows the suite rule (TTY + `NO_COLOR`, force-off via
//! `--no-color`); the table is aligned by hand so it has no extra dependency and
//! reads the same with color stripped.

use serde::Serialize;

use crate::baseline::{Change, Diff};
use crate::model::{Exposure, Listener};
use crate::util;

/// Resolved styling. Empty strings when color is off so call sites interpolate
/// unconditionally — same approach as rex-doctor.
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

    /// The color for an exposure level — public is the one to notice.
    fn exposure_color(&self, e: Exposure) -> &'static str {
        match e {
            Exposure::AllInterfaces => self.ylw,
            Exposure::Interface => self.cyn,
            Exposure::Loopback => self.dim,
        }
    }
}

/// Print the current-view table. `verbose` widens the chain to show exe +
/// package columns; the default view stays to the high-signal columns.
pub fn print_listeners(listeners: &[Listener], style: &Style, verbose: bool) {
    println!(
        "{}{}portman{} {}— what is listening, and why{}",
        style.bold, style.cyn, style.rst, style.dim, style.rst
    );
    println!();

    if listeners.is_empty() {
        println!("{}No listening sockets found.{}", style.dim, style.rst);
        print_root_hint(style);
        return;
    }

    print_table(listeners, style, verbose);

    let public = listeners
        .iter()
        .filter(|l| l.exposure == Exposure::AllInterfaces)
        .count();
    println!();
    println!(
        "{}{} listeners · {} public-facing{}",
        style.dim,
        listeners.len(),
        public,
        style.rst
    );
    print_root_hint(style);
}

/// The aligned listener table. Columns sized to the widest cell so it stays
/// readable whether or not color is on.
fn print_table(listeners: &[Listener], style: &Style, verbose: bool) {
    // Build the rows first so we can size columns.
    let rows: Vec<Row> = listeners.iter().map(|l| Row::of(l, verbose)).collect();

    let w_proto = col_width(&rows, |r| &r.proto, 5);
    let w_addr = col_width(&rows, |r| &r.addr, 4);
    let w_expo = 6; // "PUBLIC"
    let w_owner = col_width(&rows, |r| &r.owner, 7);
    let w_unit = col_width(&rows, |r| &r.unit, 4);

    // Header.
    print!(
        "{}{:<wp$}  {:<wa$}  {:<we$}  {:<wo$}  {:<wu$}",
        style.bold,
        "PROTO",
        "ADDRESS",
        "SCOPE",
        "OWNER",
        "UNIT",
        wp = w_proto,
        wa = w_addr,
        we = w_expo,
        wo = w_owner,
        wu = w_unit,
    );
    if verbose {
        print!("  {:<8}  EXE", "PKG");
    }
    println!("{}", style.rst);

    for (l, r) in listeners.iter().zip(&rows) {
        let ec = style.exposure_color(l.exposure);
        print!(
            "{:<wp$}  {:<wa$}  {}{:<we$}{}  {:<wo$}  {}{:<wu$}{}",
            r.proto,
            r.addr,
            ec,
            r.scope,
            style.rst,
            r.owner,
            style.dim,
            r.unit,
            style.rst,
            wp = w_proto,
            wa = w_addr,
            we = w_expo,
            wo = w_owner,
            wu = w_unit,
        );
        if verbose {
            print!("  {:<8}  {}{}{}", r.pkg, style.dim, r.exe, style.rst);
        }
        println!();
    }
}

/// A pre-rendered table row, all cells as owned strings.
struct Row {
    proto: String,
    addr: String,
    scope: String,
    owner: String,
    unit: String,
    pkg: String,
    exe: String,
}

impl Row {
    fn of(l: &Listener, _verbose: bool) -> Self {
        Row {
            proto: l.proto.tag().to_string(),
            addr: format!("{}:{}", l.addr, l.port),
            scope: l.exposure.tag().to_string(),
            owner: owner_cell(l),
            unit: l.owner.unit.clone().unwrap_or_else(|| "—".into()),
            pkg: l.owner.package.clone().unwrap_or_else(|| "—".into()),
            exe: l.owner.exe.clone().unwrap_or_else(|| "—".into()),
        }
    }
}

/// The owner cell: `process(pid)` when both are known, else the best label.
fn owner_cell(l: &Listener) -> String {
    match (&l.owner.process, l.owner.pid) {
        (Some(p), Some(pid)) => format!("{p}({pid})"),
        _ => l.owner_label(),
    }
}

/// Width of a column = widest cell, floored at the header's own width.
fn col_width(rows: &[Row], field: impl Fn(&Row) -> &String, min: usize) -> usize {
    rows.iter()
        .map(|r| field(r).len())
        .max()
        .unwrap_or(0)
        .max(min)
}

/// One-line hint, shown only to non-root callers, that some owners may be
/// hidden. Root sees the full picture, so it gets no nag.
fn print_root_hint(style: &Style) {
    if !util::is_root() {
        println!(
            "{}(not root: owners of other users' sockets may show as ‘?’; re-run with sudo for the full chain){}",
            style.dim, style.rst
        );
    }
}

/// Print a diff against the baseline.
pub fn print_diff(diff: &Diff, style: &Style) {
    println!(
        "{}{}portman diff{} {}— changes since baseline{}",
        style.bold, style.cyn, style.rst, style.dim, style.rst
    );
    println!();

    if diff.is_clean() {
        println!(
            "{}No changes — live listeners match the baseline.{}",
            style.grn, style.rst
        );
        return;
    }

    for change in &diff.changes {
        match change {
            Change::Added(l) => println!(
                "  {}+ {}{}  {}  {}",
                style.grn,
                l.key(),
                style.rst,
                owner_cell(l),
                scope_note(l, style),
            ),
            Change::Removed(l) => println!(
                "  {}- {}{}  {}",
                style.red,
                l.key(),
                style.rst,
                style.dim.to_string() + "(was " + &l.owner_label() + ")" + style.rst,
            ),
            Change::OwnerChanged { key, was, now } => println!(
                "  {}~ {}{}  owner {} → {}{}{}",
                style.ylw, key, style.rst, was, style.bold, now, style.rst
            ),
        }
    }

    let (added, removed, changed) = tally(diff);
    println!();
    println!(
        "{}{} added · {} removed · {} owner-changed{}",
        style.dim, added, removed, changed, style.rst
    );
}

/// A short note flagging a newly-public listener in the diff.
fn scope_note(l: &Listener, style: &Style) -> String {
    if l.exposure == Exposure::AllInterfaces {
        format!("{}[{}]{}", style.ylw, l.exposure.tag(), style.rst)
    } else {
        String::new()
    }
}

/// Count changes by kind for the diff footer.
fn tally(diff: &Diff) -> (usize, usize, usize) {
    let mut a = 0;
    let mut r = 0;
    let mut c = 0;
    for ch in &diff.changes {
        match ch {
            Change::Added(_) => a += 1,
            Change::Removed(_) => r += 1,
            Change::OwnerChanged { .. } => c += 1,
        }
    }
    (a, r, c)
}

// ---- JSON envelopes -------------------------------------------------------

/// The current-view JSON envelope, same shape family as the rest of the suite.
#[derive(Serialize)]
struct ListenersReport<'a> {
    schema_version: u32,
    source_tool: &'a str,
    count: usize,
    public_facing: usize,
    listeners: &'a [Listener],
}

/// Serialize the current view as pretty JSON.
pub fn listeners_json(listeners: &[Listener]) -> String {
    let public = listeners
        .iter()
        .filter(|l| l.exposure == Exposure::AllInterfaces)
        .count();
    let report = ListenersReport {
        schema_version: 1,
        source_tool: "portman",
        count: listeners.len(),
        public_facing: public,
        listeners,
    };
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| {
        String::from("{\"schema_version\":1,\"source_tool\":\"portman\",\"listeners\":[]}")
    })
}

/// JSON-friendly view of one change.
#[derive(Serialize)]
struct ChangeOut {
    kind: &'static str,
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now: Option<String>,
}

#[derive(Serialize)]
struct DiffReport<'a> {
    schema_version: u32,
    source_tool: &'a str,
    clean: bool,
    changes: Vec<ChangeOut>,
}

/// Serialize a diff as pretty JSON.
pub fn diff_json(diff: &Diff) -> String {
    let changes = diff
        .changes
        .iter()
        .map(|c| match c {
            Change::Added(l) => ChangeOut {
                kind: "added",
                key: l.key(),
                owner: Some(l.owner_label()),
                was: None,
                now: None,
            },
            Change::Removed(l) => ChangeOut {
                kind: "removed",
                key: l.key(),
                owner: Some(l.owner_label()),
                was: None,
                now: None,
            },
            Change::OwnerChanged { key, was, now } => ChangeOut {
                kind: "owner_changed",
                key: key.clone(),
                owner: None,
                was: Some(was.clone()),
                now: Some(now.clone()),
            },
        })
        .collect();
    let report = DiffReport {
        schema_version: 1,
        source_tool: "portman",
        clean: diff.is_clean(),
        changes,
    };
    serde_json::to_string_pretty(&report)
        .unwrap_or_else(|_| String::from("{\"schema_version\":1,\"source_tool\":\"portman\"}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Owner, Proto};

    fn l(port: u16, exposure: Exposure, process: &str) -> Listener {
        Listener {
            proto: Proto::Tcp,
            addr: if exposure == Exposure::AllInterfaces {
                "0.0.0.0".into()
            } else {
                "127.0.0.1".into()
            },
            port,
            exposure,
            owner: Owner {
                pid: Some(42),
                process: Some(process.into()),
                ..Owner::unknown()
            },
        }
    }

    #[test]
    fn listeners_json_has_stable_envelope_and_counts() {
        let listeners = vec![
            l(22, Exposure::AllInterfaces, "sshd"),
            l(631, Exposure::Loopback, "cupsd"),
        ];
        let json = listeners_json(&listeners);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "portman");
        assert_eq!(v["count"], 2);
        assert_eq!(v["public_facing"], 1);
        assert_eq!(v["listeners"].as_array().map(|a| a.len()), Some(2));
    }

    #[test]
    fn owner_cell_combines_process_and_pid() {
        let listener = l(22, Exposure::AllInterfaces, "sshd");
        assert_eq!(owner_cell(&listener), "sshd(42)");
    }

    #[test]
    fn diff_json_encodes_each_change_kind() {
        let diff = Diff {
            changes: vec![
                Change::Added(l(443, Exposure::AllInterfaces, "nginx")),
                Change::Removed(l(22, Exposure::Loopback, "sshd")),
                Change::OwnerChanged {
                    key: "tcp/0.0.0.0:80".into(),
                    was: "nginx".into(),
                    now: "apache2".into(),
                },
            ],
        };
        let v: serde_json::Value = serde_json::from_str(&diff_json(&diff)).unwrap();
        assert_eq!(v["clean"], false);
        let kinds: Vec<&str> = v["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, vec!["added", "removed", "owner_changed"]);
    }

    #[test]
    fn clean_diff_json_is_marked_clean() {
        let v: serde_json::Value = serde_json::from_str(&diff_json(&Diff::default())).unwrap();
        assert_eq!(v["clean"], true);
        assert_eq!(v["changes"].as_array().map(|a| a.len()), Some(0));
    }
}

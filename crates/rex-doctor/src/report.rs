//! Rendering. Turns a slice of [`Check`]s into either the grouped human report
//! or the JSON envelope. Color follows the suite rule (TTY + `NO_COLOR`), and a
//! clean run collapses to a one-line-per-group rollup so the signal — the
//! WARN/FAIL lines and the verdict — is never buried.

use serde::Serialize;

use crate::model::{Category, Check, Status, Summary};
use crate::util;

/// Resolved styling. Empty strings when color is off so call sites interpolate
/// unconditionally — same approach as rex-check.
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

    /// The color for a status tag.
    fn color(&self, status: Status) -> &'static str {
        match status {
            Status::Pass => self.grn,
            Status::Warn => self.ylw,
            Status::Fail => self.red,
            Status::Skip => self.dim,
        }
    }
}

/// Print the grouped human report. `verbose` also shows PASS lines.
pub fn print_human(checks: &[Check], style: &Style, verbose: bool) {
    println!(
        "{}{}rex-doctor{} {}— Linux Ops Suite health{}",
        style.bold, style.cyn, style.rst, style.dim, style.rst
    );
    println!();

    for cat in Category::all() {
        let group: Vec<&Check> = checks.iter().filter(|c| c.category == *cat).collect();
        if group.is_empty() {
            continue;
        }
        print_group(*cat, &group, style, verbose);
    }

    print_verdict(checks, style);
}

/// One category block. If every check in it passed (or skipped) and we're not
/// verbose, collapse to a single rollup line; otherwise list the notable ones.
fn print_group(cat: Category, group: &[&Check], style: &Style, verbose: bool) {
    let summary = Summary::of_refs(group);
    let notable: Vec<&&Check> = group
        .iter()
        .filter(|c| c.status == Status::Warn || c.status == Status::Fail)
        .collect();

    if notable.is_empty() && !verbose {
        // All-clear group: one dotted-leader rollup line.
        let mut tail = format!("{} ok", summary.pass);
        if summary.skip > 0 {
            tail.push_str(&format!("  {} skipped", summary.skip));
        }
        let leader = leader(cat.title(), &tail);
        println!(
            "{}{}{} {}{}{}",
            style.dim,
            cat.title(),
            style.rst,
            style.dim,
            leader,
            style.rst
        );
        return;
    }

    // Expanded group: heading then the notable (and, if verbose, all) lines.
    println!("{}{}{}", style.bold, cat.title(), style.rst);
    for c in group {
        let show = verbose || c.status == Status::Warn || c.status == Status::Fail;
        if !show {
            continue;
        }
        print_check_line(c, style);
    }
}

/// One check line: `  TAG  id   detail` plus an indented `→ fix` when present.
fn print_check_line(c: &Check, style: &Style) {
    println!(
        "  {}{:<4}{}  {}  {}",
        style.color(c.status),
        c.status.tag(),
        style.rst,
        c.id,
        c.detail
    );
    if let Some(fix) = &c.fix {
        println!("        {}→ {}{}", style.dim, fix, style.rst);
    }
}

/// The verdict banner: a rule, the worst status + counts, and the single
/// highest-leverage fix (first FAIL, else first WARN).
fn print_verdict(checks: &[Check], style: &Style) {
    let summary = Summary::of(checks);
    let verdict = summary.verdict();
    let rule: String = "─".repeat(61);
    println!();
    println!("{}{}{}", style.dim, rule, style.rst);
    println!(
        "{}Verdict: {}{}{}   {} fail · {} warn · {} pass · {} skip",
        style.bold,
        style.color(verdict),
        verdict.tag(),
        style.rst,
        summary.fail,
        summary.warn,
        summary.pass,
        summary.skip
    );
    if let Some(top) = most_important(checks) {
        if let Some(fix) = &top.fix {
            println!(
                "{}Most important:{} {} {}({}){}",
                style.bold, style.rst, fix, style.dim, top.id, style.rst
            );
        }
    }
}

/// The check whose fix the operator should run first: the first FAIL in display
/// order, falling back to the first WARN. `None` when the run is all-clear.
pub fn most_important(checks: &[Check]) -> Option<&Check> {
    checks
        .iter()
        .find(|c| c.status == Status::Fail)
        .or_else(|| checks.iter().find(|c| c.status == Status::Warn))
}

/// A dotted leader filling `title` out to a fixed column before `tail`.
fn leader(title: &str, tail: &str) -> String {
    const WIDTH: usize = 52;
    let used = title.len();
    let dots = WIDTH.saturating_sub(used).max(1);
    format!(" {} {}", ".".repeat(dots), tail)
}

/// The JSON envelope, same shape as the rest of the suite's feeds.
#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    source_tool: &'a str,
    verdict: Status,
    summary: SummaryOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    most_important: Option<&'a str>,
    checks: &'a [Check],
}

#[derive(Serialize)]
struct SummaryOut {
    pass: usize,
    warn: usize,
    fail: usize,
    skip: usize,
}

/// Serialize the report as pretty JSON. Stable envelope: `schema_version` and
/// `source_tool` first, matching every other suite tool.
pub fn to_json(checks: &[Check]) -> String {
    let summary = Summary::of(checks);
    let report = Report {
        schema_version: 1,
        source_tool: "rex-doctor",
        verdict: summary.verdict(),
        summary: SummaryOut {
            pass: summary.pass,
            warn: summary.warn,
            fail: summary.fail,
            skip: summary.skip,
        },
        most_important: most_important(checks).map(|c| c.id),
        checks,
    };
    // The data is plain owned strings/enums; serialization cannot fail in
    // practice, but we never unwrap — fall back to a minimal valid envelope.
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| {
        String::from("{\"schema_version\":1,\"source_tool\":\"rex-doctor\",\"checks\":[]}")
    })
}

impl Summary {
    /// Tally over a slice of references (the human renderer holds `&&Check`).
    fn of_refs(checks: &[&Check]) -> Self {
        let mut s = Summary {
            pass: 0,
            warn: 0,
            fail: 0,
            skip: 0,
        };
        for c in checks {
            match c.status {
                Status::Pass => s.pass += 1,
                Status::Warn => s.warn += 1,
                Status::Fail => s.fail += 1,
                Status::Skip => s.skip += 1,
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Check> {
        vec![
            Check::pass("env.install-dirs", Category::Env, "ok"),
            Check::fail("bin.present", Category::Bin, "missing x", "install"),
            Check::warn("bin.version-skew", Category::Bin, "skew", "reinstall"),
        ]
    }

    #[test]
    fn most_important_prefers_fail_over_warn() {
        let checks = sample();
        let top = most_important(&checks).expect("a notable check");
        assert_eq!(top.id, "bin.present");
    }

    #[test]
    fn most_important_is_none_when_all_clear() {
        let checks = vec![Check::pass("env.no-color", Category::Env, "on")];
        assert!(most_important(&checks).is_none());
    }

    #[test]
    fn json_has_stable_envelope() {
        let json = to_json(&sample());
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "rex-doctor");
        assert_eq!(v["verdict"], "fail");
        assert_eq!(v["most_important"], "bin.present");
        assert_eq!(v["summary"]["fail"], 1);
        assert_eq!(v["checks"].as_array().map(|a| a.len()), Some(3));
        // A passing check carries no `fix` key (skip_serializing_if).
        let env_check = v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "env.install-dirs")
            .unwrap();
        assert!(env_check.get("fix").is_none());
    }

    #[test]
    fn leader_pads_to_a_column() {
        let l = leader("Short", "3 ok");
        assert!(l.contains("3 ok"));
        assert!(l.contains('.'));
    }
}

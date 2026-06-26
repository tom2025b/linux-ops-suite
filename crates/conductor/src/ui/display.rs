//! Human output and colours — the only module that prints to stdout.

use crate::core::config::Config;
use crate::core::error::Result;
use crate::state::workstate::{Finding, Workstate};

/// Wrap `text` in an ANSI colour code when colour is enabled.
fn paint(config: &Config, code: &str, text: &str) -> String {
    if config.color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn bold(config: &Config, text: &str) -> String {
    paint(config, "1", text)
}
fn dim(config: &Config, text: &str) -> String {
    paint(config, "2", text)
}
fn red(config: &Config, text: &str) -> String {
    paint(config, "31", text)
}
fn green(config: &Config, text: &str) -> String {
    paint(config, "32", text)
}
fn yellow(config: &Config, text: &str) -> String {
    paint(config, "33", text)
}

/// A plain line of output.
pub fn message(_config: &Config, text: &str) {
    println!("{text}");
}

/// Confirmation that a snapshot was written.
pub fn workstate_saved(config: &Config, ws: &Workstate) {
    println!(
        "{} snapshot written ({} tools, {} findings)",
        green(config, "✓"),
        ws.tools.len(),
        ws.findings.len()
    );
}

/// Freshness and counts for the current snapshot.
pub fn workstate_status(config: &Config, ws: &Workstate) {
    let state = if ws.is_stale() {
        yellow(config, "stale")
    } else {
        green(config, "current")
    };
    println!("{} {}", bold(config, "workstate"), state);
    println!("  built {}s ago", ws.age_secs());
    println!("  {} tools · {} findings", ws.tools.len(), ws.findings.len());
}

/// One finding in detail.
pub fn finding(config: &Config, finding: &Finding) {
    println!(
        "{} {}",
        bold(config, &finding.id),
        severity(config, &finding.severity)
    );
    println!("  {}", finding.reason);
}

/// A summary of every finding.
pub fn bulwark_check(config: &Config, ws: &Workstate) {
    if ws.findings.is_empty() {
        println!("{} no findings", green(config, "✓"));
        return;
    }
    println!("{} {} findings", yellow(config, "⚠"), ws.findings.len());
    for f in &ws.findings {
        println!("  {} {}", severity(config, &f.severity), f.id);
    }
}

/// The high-severity findings worth investigating first.
pub fn bulwark_tripwire(config: &Config, ws: &Workstate) {
    let severe: Vec<&Finding> = ws
        .findings
        .iter()
        .filter(|f| is_severe(&f.severity))
        .collect();
    if severe.is_empty() {
        println!("{} no high-severity drift", green(config, "✓"));
        return;
    }
    println!("{} {} to review first", yellow(config, "⚠"), severe.len());
    for f in severe {
        println!("  {} {}", severity(config, &f.severity), f.id);
    }
}

/// The saved restore points.
pub fn rewind_list(config: &Config, ids: &[String]) {
    if ids.is_empty() {
        println!("no restore points");
        return;
    }
    println!("{}", bold(config, "restore points"));
    for id in ids {
        println!("  {id}");
    }
}

/// Pretty-print any serialisable value as JSON.
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// A coloured severity tag like `[CRIT]` / `[HIGH]`.
fn severity(config: &Config, level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "critical" => red(config, "[CRIT]"),
        "high" => yellow(config, "[HIGH]"),
        other => dim(config, &format!("[{other}]")),
    }
}

/// Whether a severity level counts as high-severity drift.
fn is_severe(level: &str) -> bool {
    matches!(level.to_ascii_lowercase().as_str(), "critical" | "high")
}

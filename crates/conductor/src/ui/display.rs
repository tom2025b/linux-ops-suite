//! Human output and colours — the only module that prints to stdout.

use chrono::Utc;

use workstate_schema::model::normalized::{Finding, Severity};
use workstate_schema::Snapshot;

use crate::core::config::Config;
use crate::core::error::Result;

/// A snapshot older than this (in seconds) reads as stale.
const STALE_AFTER_SECS: i64 = 24 * 60 * 60;

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

/// Freshness and counts for the canonical snapshot.
pub fn workstate_status(config: &Config, snap: &Snapshot) {
    // Age from the snapshot's own build time; clamp negatives (clock skew) to 0.
    let age = (Utc::now() - snap.built_at).num_seconds().max(0);
    let state = if age > STALE_AFTER_SECS {
        yellow(config, "stale")
    } else {
        green(config, "current")
    };
    let tools = snap.tools.data.as_ref().map_or(0, |i| i.tools.len());
    let findings = snap.findings.data.as_ref().map_or(0, |i| i.findings.len());
    let jobs = snap.jobs.data.as_ref().map_or(0, |i| i.jobs.len());
    println!("{} {}", bold(config, "workstate"), state);
    println!("  built {age}s ago");
    println!("  {tools} tools · {findings} findings · {jobs} jobs");
}

/// One finding in detail.
pub fn finding(config: &Config, finding: &Finding) {
    println!(
        "{} {}",
        bold(config, &finding.id.0),
        severity_tag(config, finding.severity)
    );
    if let Some(desc) = finding.description.as_deref().filter(|d| !d.is_empty()) {
        println!("  {desc}");
    }
}

/// A summary of every finding.
pub fn bulwark_check(config: &Config, snap: &Snapshot) {
    let findings = snap
        .findings
        .data
        .as_ref()
        .map(|i| i.findings.as_slice())
        .unwrap_or(&[]);
    if findings.is_empty() {
        println!("{} no findings", green(config, "✓"));
        return;
    }
    println!("{} {} findings", yellow(config, "⚠"), findings.len());
    for f in findings {
        println!("  {} {}", severity_tag(config, f.severity), f.id.0);
    }
}

/// The high-severity findings worth investigating first.
pub fn bulwark_tripwire(config: &Config, snap: &Snapshot) {
    let severe: Vec<&Finding> = snap
        .findings
        .data
        .as_ref()
        .map(|i| {
            i.findings
                .iter()
                .filter(|f| is_severe(f.severity))
                .collect()
        })
        .unwrap_or_default();
    if severe.is_empty() {
        println!("{} no high-severity drift", green(config, "✓"));
        return;
    }
    println!("{} {} to review first", yellow(config, "⚠"), severe.len());
    for f in severe {
        println!("  {} {}", severity_tag(config, f.severity), f.id.0);
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

/// A coloured severity tag from the canonical `Severity`.
fn severity_tag(config: &Config, sev: Severity) -> String {
    match sev {
        Severity::Critical => red(config, "[CRIT]"),
        Severity::High => yellow(config, "[HIGH]"),
        Severity::Medium => dim(config, "[MED]"),
        Severity::Low => dim(config, "[LOW]"),
        Severity::Info => dim(config, "[INFO]"),
        Severity::Unrated => dim(config, "[UNRATED]"),
        // `Severity` is #[non_exhaustive]; `Unknown` and any future bucket land here.
        _ => dim(config, "[UNKNOWN]"),
    }
}

/// Whether a severity counts as high-severity drift.
fn is_severe(sev: Severity) -> bool {
    matches!(sev, Severity::High | Severity::Critical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severe_is_high_and_critical_only() {
        assert!(is_severe(Severity::Critical));
        assert!(is_severe(Severity::High));
        assert!(!is_severe(Severity::Medium));
        assert!(!is_severe(Severity::Low));
        assert!(!is_severe(Severity::Unrated));
    }
}

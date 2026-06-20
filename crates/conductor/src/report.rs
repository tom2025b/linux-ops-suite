//! Rendering. Turns a `Plan` (and the raw `SuiteState`, for `health`) into human
//! text or the suite's JSON envelope. Color follows the suite rule (TTY +
//! `NO_COLOR`, force-off via `--no-color`); structure is plain and reads the same
//! with color stripped — state is carried by word and glyph, never color alone.
//! The library does the work; these functions only present it. Mirrors rewind's
//! `report.rs`.
//!
//! The JSON envelope carries a stable `plan_id` and per-step `id` so the Phase 3
//! driver and external consumers can address a plan/step across runs.

use serde::Serialize;

use crate::plan::{Plan, Ring, Step, StepStatus};
use crate::state::{Freshness, SuiteState};
use crate::util;

/// Resolved styling. Empty strings when color is off so call sites interpolate
/// unconditionally — same approach as rewind/pulse.
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

    #[cfg(test)]
    fn plain() -> Self {
        Self::resolve(true)
    }
}

/// Color for a ring tag: amber for state changes, dim for read-only/info.
fn ring_color(ring: Ring, style: &Style) -> &'static str {
    match ring {
        Ring::ChangesState => style.ylw,
        Ring::ReadOnly | Ring::Info => style.dim,
    }
}

/// The glyph for a step's status: the one-shot renderer marks every pending step
/// with `○` (the TUI decides the `▸` current marker); `✓` done, `·` skipped.
fn status_glyph(status: StepStatus) -> char {
    match status {
        StepStatus::Pending => '○',
        StepStatus::Done => '✓',
        StepStatus::Skipped => '·',
    }
}

/// One step block: "  ○ N  <title>  [← annotation]" then the dim command line
/// with its ring tag.
fn render_step(out: &mut String, n: usize, step: &Step, style: &Style) {
    let glyph = status_glyph(step.status);
    out.push_str(&format!("  {glyph} {n}  {}", step.title));
    if let Some(note) = &step.annotation {
        out.push_str(&format!("  {}← {}{}", style.cyn, note, style.rst));
    }
    out.push('\n');
    if let Some(cmd) = &step.command {
        out.push_str(&format!(
            "       {dim}{cmd}{rst}   {rc}{tag}{rst}\n",
            dim = style.dim,
            cmd = cmd,
            rc = ring_color(step.ring, style),
            tag = step.ring.tag(),
            rst = style.rst,
        ));
    }
}

/// The `status` verb: situation + ordered plan, or the healthy message.
pub fn print_status(plan: &Plan, _built_at: Option<&str>, style: &Style) -> String {
    if plan.is_empty() {
        return format!(
            "{grn}nothing to conduct{rst}\nthe suite is healthy and every feed is current\n",
            grn = style.grn,
            rst = style.rst,
        );
    }
    let mut out = String::new();
    if !plan.situation.is_empty() {
        out.push_str(&format!("{}the situation{}\n", style.bold, style.rst));
        for line in &plan.situation {
            out.push_str(&format!("  {line}\n"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{}the plan{}   {} steps\n",
        style.bold,
        style.rst,
        plan.steps.len()
    ));
    for (i, step) in plan.steps.iter().enumerate() {
        render_step(&mut out, i + 1, step, style);
    }
    out
}

/// The `plan` verb: just the ordered steps, no situation prose.
pub fn print_plan(plan: &Plan, style: &Style) -> String {
    if plan.is_empty() {
        return format!("{}nothing to conduct{}\n", style.grn, style.rst);
    }
    let mut out = String::new();
    for (i, step) in plan.steps.iter().enumerate() {
        render_step(&mut out, i + 1, step, style);
    }
    out
}

/// A freshness word for the health view.
fn freshness_word(f: Freshness) -> &'static str {
    match f {
        Freshness::Current => "current",
        Freshness::Stale => "stale",
        Freshness::Unavailable => "unavailable",
    }
}

/// The `health` verb: per-feed and per-binary readiness as conductor sees it.
pub fn print_health(state: &SuiteState, style: &Style) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}feeds{}\n", style.bold, style.rst));
    if state.feeds.is_empty() {
        out.push_str(&format!("  {}none readable{}\n", style.dim, style.rst));
    }
    for f in &state.feeds {
        let color = match f.freshness {
            Freshness::Current => style.grn,
            Freshness::Stale => style.ylw,
            Freshness::Unavailable => style.red,
        };
        out.push_str(&format!(
            "  {:<10} {}{}{}\n",
            f.name,
            color,
            freshness_word(f.freshness),
            style.rst
        ));
    }
    out.push_str(&format!("\n{}tools on PATH{}\n", style.bold, style.rst));
    for b in &state.binaries {
        let (mark, color) = if b.present {
            ("present", style.grn)
        } else {
            ("missing", style.dim)
        };
        out.push_str(&format!(
            "  {:<12} {}{}{}\n",
            b.name, color, mark, style.rst
        ));
    }
    out
}

/// Phase 1 keeps `health` informational: it always exits 0. A non-zero policy
/// (e.g. exit 1 on any unavailable feed) is deferred so cron users don't get
/// surprised before the policy is designed. Documented here so the reserved
/// behaviour is explicit.
pub fn health_exit_code(_state: &SuiteState) -> u8 {
    0
}

// ── JSON envelopes ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StepOut {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    ring: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation: Option<String>,
}

impl StepOut {
    fn of(step: &Step) -> Self {
        StepOut {
            id: step.id.clone(),
            title: step.title.clone(),
            command: step.command.clone(),
            ring: step.ring.tag(),
            annotation: step.annotation.clone(),
        }
    }
}

#[derive(Serialize)]
struct StatusEnvelope {
    schema_version: u32,
    source_tool: &'static str,
    plan_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    built_at: Option<String>,
    situation: Vec<String>,
    step_count: usize,
    steps: Vec<StepOut>,
}

/// The `status`/`plan` JSON envelope.
pub fn status_json(plan: &Plan, built_at: Option<&str>) -> String {
    let env = StatusEnvelope {
        schema_version: 1,
        source_tool: "conductor",
        plan_id: plan.plan_id(),
        built_at: built_at.map(|s| s.to_string()),
        situation: plan.situation.clone(),
        step_count: plan.steps.len(),
        steps: plan.steps.iter().map(StepOut::of).collect(),
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Serialize)]
struct FeedOut {
    name: &'static str,
    freshness: &'static str,
}

#[derive(Serialize)]
struct BinaryOut {
    name: &'static str,
    present: bool,
}

#[derive(Serialize)]
struct HealthEnvelope {
    schema_version: u32,
    source_tool: &'static str,
    feeds: Vec<FeedOut>,
    tools: Vec<BinaryOut>,
}

/// The `health` JSON envelope.
pub fn health_json(state: &SuiteState) -> String {
    let env = HealthEnvelope {
        schema_version: 1,
        source_tool: "conductor",
        feeds: state
            .feeds
            .iter()
            .map(|f| FeedOut {
                name: f.name,
                freshness: freshness_word(f.freshness),
            })
            .collect(),
        tools: state
            .binaries
            .iter()
            .map(|b| BinaryOut {
                name: b.name,
                present: b.present,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&env).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::state::{BinaryStatus, FeedStatus, Finding, Freshness, Severity, SuiteState};

    fn plan_with_findings() -> Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(),
            why: "AWS key".into(),
            source: "bulwark".into(),
            severity: Severity::Critical,
        });
        plan::build(&s)
    }

    #[test]
    fn empty_plan_status_says_nothing_to_conduct() {
        let plan = Plan::default();
        let out = print_status(&plan, None, &Style::plain());
        assert!(out.contains("nothing to conduct"));
        assert!(!out.contains("the plan"));
    }

    #[test]
    fn status_shows_situation_then_plan_with_commands_and_tags() {
        let out = print_status(
            &plan_with_findings(),
            Some("2026-06-14T12:00:00Z"),
            &Style::plain(),
        );
        assert!(out.contains("the situation"));
        assert!(out.contains("the plan"));
        assert!(out.contains("workstate snapshot")); // refresh command shown
        assert!(out.contains("changes state")); // ring tag shown
        assert!(out.contains("bulwark show deploy-prod.sh"));
        assert!(out.contains("read-only"));
    }

    #[test]
    fn plan_verb_omits_situation_prose() {
        let out = print_plan(&plan_with_findings(), &Style::plain());
        assert!(!out.contains("the situation"));
        assert!(out.contains("workstate snapshot"));
    }

    #[test]
    fn health_lists_feeds_and_tools() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.binaries.push(BinaryStatus {
            name: "pulse",
            present: true,
        });
        s.binaries.push(BinaryStatus {
            name: "rewind",
            present: false,
        });
        let out = print_health(&s, &Style::plain());
        assert!(out.contains("tools"));
        assert!(out.contains("stale"));
        assert!(out.contains("pulse"));
        assert!(out.contains("present"));
        assert!(out.contains("rewind"));
        assert!(out.contains("missing"));
    }

    #[test]
    fn status_json_is_the_suite_envelope_with_plan_and_step_ids() {
        let json = status_json(&plan_with_findings(), Some("2026-06-14T12:00:00Z"));
        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"source_tool\": \"conductor\""));
        assert!(json.contains("\"ring\": \"changes state\""));
        assert!(json.contains("deploy-prod.sh"));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["step_count"], 3);
        // plan_id present + stable shape; step.id present for Phase 3
        assert!(v["plan_id"].as_str().unwrap().starts_with("plan-"));
        assert_eq!(v["steps"][0]["id"], "refresh-stale-data");
        let inv = v["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "investigate-deploy-prod-sh")
            .unwrap();
        assert_eq!(inv["ring"], "read-only");
    }

    #[test]
    fn empty_plan_json_has_nothing_to_conduct_plan_id() {
        let json = status_json(&Plan::default(), None);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["plan_id"], "nothing-to-conduct");
        assert_eq!(v["step_count"], 0);
    }

    #[test]
    fn health_json_is_the_suite_envelope() {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Current,
        });
        s.binaries.push(BinaryStatus {
            name: "pulse",
            present: true,
        });
        let json = health_json(&s);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "conductor");
        assert_eq!(v["feeds"][0]["freshness"], "current");
        assert_eq!(v["tools"][0]["present"], true);
    }

    #[test]
    fn no_color_output_has_no_escape_codes() {
        let out = print_status(&plan_with_findings(), None, &Style::plain());
        assert!(
            !out.contains('\u{1b}'),
            "plain style must emit no ANSI escapes"
        );
    }
}

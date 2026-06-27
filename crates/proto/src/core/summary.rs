//! Human and structured summaries for automatic check runs.

use std::time::Duration;

use serde::Serialize;

use crate::core::executor::{CheckOutcome, CheckStatus};

const OUTPUT_EXCERPT_LINES: usize = 12;
const OUTPUT_EXCERPT_CHARS: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckSummaryStatus {
    Pass,
    Fail,
    Error,
}

impl CheckSummaryStatus {
    fn marker(self) -> &'static str {
        match self {
            Self::Pass => "[pass]",
            Self::Fail => "[FAIL]",
            Self::Error => "[ERR ]",
        }
    }
}

impl From<CheckStatus> for CheckSummaryStatus {
    fn from(status: CheckStatus) -> Self {
        match status {
            CheckStatus::Pass => Self::Pass,
            CheckStatus::Fail => Self::Fail,
            CheckStatus::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverallCheckStatus {
    Pass,
    Fail,
    Error,
    Empty,
}

impl OverallCheckStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Error => "ERROR",
            Self::Empty => "NO CHECKS",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckRunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub duration_ms: u64,
    pub overall_status: OverallCheckStatus,
    pub checks: Vec<CheckSummaryItem>,
}

impl CheckRunSummary {
    pub fn from_outcomes(outcomes: &[CheckOutcome]) -> Self {
        let checks: Vec<CheckSummaryItem> = outcomes
            .iter()
            .map(CheckSummaryItem::from_outcome)
            .collect();

        let passed = checks
            .iter()
            .filter(|item| item.status == CheckSummaryStatus::Pass)
            .count();
        let failed = checks
            .iter()
            .filter(|item| item.status == CheckSummaryStatus::Fail)
            .count();
        let errors = checks
            .iter()
            .filter(|item| item.status == CheckSummaryStatus::Error)
            .count();
        let duration = outcomes
            .iter()
            .fold(Duration::ZERO, |total, outcome| total + outcome.duration);

        let overall_status = if checks.is_empty() {
            OverallCheckStatus::Empty
        } else if errors > 0 {
            OverallCheckStatus::Error
        } else if failed > 0 {
            OverallCheckStatus::Fail
        } else {
            OverallCheckStatus::Pass
        };

        Self {
            total: checks.len(),
            passed,
            failed,
            errors,
            duration_ms: duration_millis(duration),
            overall_status,
            checks,
        }
    }

    pub fn passed(&self) -> bool {
        self.overall_status == OverallCheckStatus::Pass
    }

    pub fn count_line(&self) -> String {
        format!(
            "{} total, {} passed, {} failed, {} errors",
            self.total, self.passed, self.failed, self.errors
        )
    }

    pub fn render_human(&self) -> String {
        let mut rendered = String::new();
        rendered.push_str("--- Check Summary ---\n");
        rendered.push_str(&format!("Result: {}\n", self.overall_status.label()));
        rendered.push_str(&format!("Checks: {}\n", self.count_line()));
        rendered.push_str(&format!("Duration: {}\n", format_millis(self.duration_ms)));

        if self.checks.is_empty() {
            rendered.push_str("\nNo checks were run.\n");
            return rendered;
        }

        rendered.push_str("\nResults:\n");
        for item in &self.checks {
            rendered.push_str(&format!(
                "  {} {} ({})\n",
                item.status.marker(),
                item.name,
                format_millis(item.duration_ms)
            ));
            rendered.push_str(&format!("      command: {}\n", item.full_command));

            match item.status {
                CheckSummaryStatus::Pass => {}
                CheckSummaryStatus::Fail => {
                    if let Some(code) = item.exit_code {
                        rendered.push_str(&format!("      exit code: {code}\n"));
                    } else {
                        rendered.push_str("      exit code: unavailable\n");
                    }
                }
                CheckSummaryStatus::Error => {
                    if item.timed_out {
                        rendered.push_str("      timed out: yes\n");
                    }
                    if let Some(message) = &item.error_message {
                        rendered.push_str(&format!("      error: {message}\n"));
                    }
                }
            }

            append_output_block(&mut rendered, "stdout", item.stdout_excerpt.as_deref());
            append_output_block(&mut rendered, "stderr", item.stderr_excerpt.as_deref());
        }

        rendered
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckSummaryItem {
    pub check_id: String,
    pub name: String,
    pub status: CheckSummaryStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub full_command: String,
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub timed_out: bool,
    pub error_message: Option<String>,
    pub stdout_excerpt: Option<String>,
    pub stderr_excerpt: Option<String>,
}

impl CheckSummaryItem {
    fn from_outcome(outcome: &CheckOutcome) -> Self {
        let status = CheckSummaryStatus::from(outcome.status);
        let include_output = status != CheckSummaryStatus::Pass;

        Self {
            check_id: outcome.check_id.clone(),
            name: outcome.name.clone(),
            status,
            exit_code: outcome.exit_code,
            duration_ms: duration_millis(outcome.duration),
            full_command: outcome.full_command.clone(),
            program: outcome.program.clone(),
            args: outcome.args.clone(),
            working_dir: outcome
                .working_dir
                .as_ref()
                .map(|path| path.display().to_string()),
            timed_out: outcome.timed_out,
            error_message: outcome.error_message.clone(),
            stdout_excerpt: include_output
                .then(|| output_excerpt(&outcome.stdout))
                .flatten(),
            stderr_excerpt: include_output
                .then(|| output_excerpt(&outcome.stderr))
                .flatten(),
        }
    }
}

pub fn summarize(outcomes: &[CheckOutcome]) -> CheckRunSummary {
    CheckRunSummary::from_outcomes(outcomes)
}

pub fn render_human(outcomes: &[CheckOutcome]) -> String {
    summarize(outcomes).render_human()
}

pub fn format_duration(duration: Duration) -> String {
    format_millis(duration_millis(duration))
}

fn append_output_block(rendered: &mut String, label: &str, excerpt: Option<&str>) {
    let Some(excerpt) = excerpt else {
        return;
    };

    rendered.push_str(&format!("      {label}:\n"));
    for line in excerpt.lines() {
        rendered.push_str(&format!("        {line}\n"));
    }
}

fn output_excerpt(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let omitted = lines.len().saturating_sub(OUTPUT_EXCERPT_LINES);
    let mut excerpt = String::new();

    if omitted > 0 {
        excerpt.push_str(&format!("... {omitted} earlier lines omitted\n"));
    }
    excerpt.push_str(&lines[omitted..].join("\n"));

    Some(truncate_chars(&excerpt, OUTPUT_EXCERPT_CHARS))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((index, _)) => format!("{}...", &text[..index]),
        None => text.to_string(),
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn format_millis(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }

    let seconds = ms / 1_000;
    if seconds < 60 {
        return format!("{:.2}s", ms as f64 / 1_000.0);
    }

    let minutes = seconds / 60;
    let remaining_seconds = seconds % 60;
    format!("{minutes}m {remaining_seconds:02}s")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn outcome(id: &str, status: CheckStatus) -> CheckOutcome {
        CheckOutcome {
            check_id: id.to_string(),
            name: id.to_string(),
            status,
            exit_code: match status {
                CheckStatus::Pass => Some(0),
                CheckStatus::Fail => Some(1),
                CheckStatus::Error => None,
            },
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_millis(25),
            full_command: format!("run {id}"),
            program: "run".to_string(),
            args: vec![id.to_string()],
            working_dir: Some(PathBuf::from("/tmp/project")),
            timed_out: false,
            error_message: None,
        }
    }

    #[test]
    fn counts_pass_fail_and_error_outcomes() {
        let summary = summarize(&[
            outcome("build", CheckStatus::Pass),
            outcome("test", CheckStatus::Fail),
            outcome("audit", CheckStatus::Error),
        ]);

        assert_eq!(summary.total, 3);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.overall_status, OverallCheckStatus::Error);
        assert_eq!(summary.duration_ms, 75);
    }

    #[test]
    fn renders_failure_details_without_pass_output_noise() {
        let mut pass = outcome("build", CheckStatus::Pass);
        pass.stdout = "compiled\n".to_string();
        let mut fail = outcome("test", CheckStatus::Fail);
        fail.stderr = "assertion failed\n".to_string();

        let rendered = render_human(&[pass, fail]);

        assert!(rendered.contains("Result: FAIL"));
        assert!(rendered.contains("[pass] build"));
        assert!(rendered.contains("[FAIL] test"));
        assert!(rendered.contains("assertion failed"));
        assert!(!rendered.contains("compiled"));
    }
}
